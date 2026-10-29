//! **The OS hand-off lane** — every hand-off that leaves the window runs here, never on the
//! window thread (`docs/ARCHITECTURE.md` §5.1 and §5.3 row 1; `docs/DESIGN.md`, 2026-09-22 —
//! *a hand-off to the system runs on its own lane*).
//!
//! `ShellExecuteW` loads the shell extensions registered for what it is handed, in this process,
//! before it returns: a `Ctrl`+click on a printed `.md` held the window for 1447 ms, 1219 ms of it
//! the window thread's own CPU. The doors in `bt_platform::handoff` stay exactly as they are and
//! stay synchronous; this module is the one place that calls them, from one below-normal thread,
//! and the window hears the answer through its event loop.
//!
//! **Built on the completion seam** (`crate::lane`; the side-effects contract's C1–C7, revision
//! (b)7; S-5): the worker is started by a [`crate::lane::Executor`] (the thread door's in the
//! product, the suite's scripted one under test), every identity is minted by a
//! [`crate::lane::Ledger`], each hand-off runs inside [`crate::lane::task`], and every request
//! ends through the ledger's hooks.
//!
//! # The contract
//!
//! * **Identity.** Every admitted request gets a [`HandoffId`] minted by the lane's ledger, unique
//!   for the life of the lane (one per process). The window that asked holds the request, and what
//!   it owes the answer, in its own [`Pending`] under that id — which is the target incarnation: a
//!   window that closes takes its `Pending` with it, and a new window that happens to reuse a
//!   platform handle starts with an empty one.
//! * **Ordering.** One thread, one FIFO queue: hand-offs are executed in press order, across
//!   every window, and answered in that order across every window — the application reads the
//!   earliest request still owed ([`HandoffLane::earliest_owed`]) once, each window takes the
//!   answers its requests hold below it, and they are put back in the order the ledger minted
//!   them before any is answered ([`in_press_order`]). A request that ends while the windows are
//!   being read waits for the next drain, so no later answer is ever taken before it.
//! * **Capacity.** The ledger owes at most [`BOUND`] identities at once — the one running and
//!   [`CAPACITY`] waiting; an answer its window has not taken yet holds its place. Nothing is
//!   coalesced — each press is a distinct request, and two presses on one link are two hand-offs,
//!   as they always were. A press past the bound is refused at the door, synchronously, with
//!   [`LANE_FULL`] as its terminal outcome, and no identity is minted for it (revision (b)3); a
//!   press after the worker has ended is refused the same way with [`LANE_GONE`].
//! * **Completion.** Every admitted request ends exactly once: `Done` with the door's own
//!   `Result` — `Ok(())`, the operating system accepted it, or `Err(refusal)`, the door's own
//!   [`HandOffRefusal`] as the door returned it — no handler, not found, access denied, or another
//!   refusal in the door's or the system's words (never "the other program finished": the lane
//!   returns when `ShellExecuteW` or `NSWorkspace` does); `Failed` when the
//!   hand-off panicked, which its task catches — the worker goes on, and the ledger says the
//!   failure in `diagnostics.log` once; or `WorkerGone` for every request still owed when the
//!   worker's thread ends, which leaves the lane faulted.
//! * **Abandonment.** An answer is raised only in the window whose [`Pending`] holds its id. A
//!   window that closes drops its requests, and each one's terminal goes to the lane's sink for
//!   the abandoned (C6) — never to another window. The sink lets a success go and says a refusal
//!   in `diagnostics.log` ([`abandoned`]).
//! * **Wake.** The worker publishes the terminal first and then wakes the loop
//!   (`AppEvent::HandoffAnswered`); the ledger wakes it the same way when the worker ends, after
//!   every owed request has its terminal. The loop takes the answers without waiting and never
//!   blocks on the lane.
//!
//! Results come back the first of `ARCHITECTURE` §5.1's three ways — an `AppEvent` through the
//! event-loop proxy — because this lane is `bt-app`'s and may name `AppEvent`.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::{Arc, mpsc};

use anyhow::{Context, Result};
use bt_platform::admission::WorkerCtx;
use bt_platform::{HandOffRefusal, Handoff, NativeWindow};

use crate::i18n;
use crate::lane::{self, Executor, Ledger, Owed, Refused, Saying, Terminal};

/// How many hand-offs may wait behind the one running.
///
/// Far beyond a person: each is one press. The bound exists so that a lane stuck inside a shell
/// extension that never returns is an accounted-for queue and not an unbounded one.
pub(crate) const CAPACITY: usize = 32;

/// **How many identities the lane's ledger owes at once**: the one running and [`CAPACITY`]
/// waiting. An answer its window has not taken yet holds its place, so the answers held for an
/// undrained window are bounded by the same number.
pub(crate) const BOUND: usize = CAPACITY + 1;

/// The refusal a press meets when the ledger already owes [`BOUND`] hand-offs.
pub(crate) const LANE_FULL: &str = "the hand-off lane is full";

/// The refusal a press meets when the lane's thread is gone.
pub(crate) const LANE_GONE: &str = "the hand-off lane has stopped";

/// The lane's name in the lines its ledger says.
const LANE: &str = "the OS hand-off lane";

/// **One hand-off's name**, minted by the lane's ledger and unique for the life of the lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct HandoffId(u64);

impl From<u64> for HandoffId {
    fn from(minted: u64) -> Self {
        Self(minted)
    }
}

/// How one hand-off ended: the door's own `Result` when it was done; a panic in it is `Failed`.
pub(crate) type HandoffTerminal = Terminal<HandoffId, Result<(), HandOffRefusal>, Infallible>;

/// The lane's sink for the abandoned, shared by every request it admits.
type Sink = Arc<dyn Fn(HandoffId, HandoffTerminal) + Send + Sync>;

/// **What a hand-off is owed when its window has gone** (C6): the lane's sink for the abandoned,
/// which outlives every window.
pub(crate) struct Abandoned(Sink);

impl Owed<HandoffId> for Abandoned {
    type Answer = Result<(), HandOffRefusal>;
    type Error = Infallible;

    fn sink(self, id: HandoffId, terminal: HandoffTerminal) {
        (self.0)(id, terminal);
    }
}

/// **The product's sink for the abandoned**: a hand-off whose window closed before its answer
/// came. A success is let go; the door's or the system's refusal, which no window is left to say,
/// is said through `say` (`diagnostics::note`). A failure and the worker's end were said by the
/// ledger as they were delivered, so they are not said twice.
pub(crate) fn abandoned(id: HandoffId, terminal: &HandoffTerminal, say: &dyn Fn(&str)) {
    if let Terminal::Done(Err(reason)) = terminal {
        say(&format!(
            "{LANE}: hand-off {id:?}, asked by a window that has closed, was refused: {reason}"
        ));
    }
}

/// One admitted hand-off, held by the window that asked: its terminal is taken once, and a window
/// that drops it leaves the terminal to the sink.
pub(crate) type HandoffRequest = lane::Request<HandoffId, Abandoned>;

/// What the window thread puts on the queue.
struct Job {
    id: HandoffId,
    window: NativeWindow,
    handoff: Handoff,
}

/// **Why a hand-off was refused**, as a value: by whom, and in which words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Declined {
    /// The hand-off door refused it, or the system did — the door's own `Err`, as the kind of
    /// refusal it is (T-HANDOFF-DOOR-REASON): `ShellExecuteW`'s code, or LaunchServices and the
    /// disk asked before anything opened, read by the door.
    System(HandOffRefusal),
    /// The lane's ledger already owed [`BOUND`] hand-offs (said [`LANE_FULL`]).
    LaneFull,
    /// The lane's worker had ended (said [`LANE_GONE`]).
    LaneGone,
    /// The asking window could not be named to the door, in the platform's words.
    NoWindow(String),
}

impl Declined {
    /// The refusal's own words: the stderr line's reason and the diagnostics a refusal leaves.
    pub(crate) fn reason(&self) -> &str {
        match self {
            Self::System(refusal) => refusal.reason(),
            Self::NoWindow(reason) => reason,
            Self::LaneFull => LANE_FULL,
            Self::LaneGone => LANE_GONE,
        }
    }
}

/// **One hand-off, answered**, as the window that asked hears it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Answer {
    /// The operating system accepted it.
    HandedOver,
    /// It was refused — at the lane's door (full, its worker gone, a window that could not be
    /// named), by the hand-off door, or by the system.
    Refused(Declined),
    /// The hand-off panicked, in these words; the ledger has said them.
    Failed(String),
    /// The lane's worker ended with the hand-off owed; the ledger has said so.
    WorkerGone,
}

impl Answer {
    fn of(terminal: HandoffTerminal) -> Self {
        match terminal {
            Terminal::Done(Ok(())) => Self::HandedOver,
            Terminal::Done(Err(reason)) => Self::Refused(Declined::System(reason)),
            Terminal::Failed(failure) => Self::Failed(failure.to_string()),
            Terminal::WorkerGone => Self::WorkerGone,
            Terminal::Cancelled | Terminal::Superseded { .. } | Terminal::Joined { .. } => {
                unreachable!("the hand-off lane cancels, supersedes and joins nothing")
            }
        }
    }
}

/// **The lane's owner, held by the application** — the ledger that mints and ends every request,
/// the queue's sending half, and the sink a closed window's requests are owed.
pub(crate) struct HandoffLane {
    ledger: Arc<Ledger<HandoffId, Abandoned>>,
    jobs: mpsc::Sender<Job>,
    sink: Sink,
}

impl HandoffLane {
    /// **Start the lane** — one thread, `bt-os-handoff`, below normal, for the life of the
    /// process, through the thread door; its ledger's lines, and the refusals no window is left
    /// to hear, said in `diagnostics.log`.
    ///
    /// The thread enters its [`bt_platform::ShellThread`] first, which on Windows is the COM
    /// apartment `ShellExecuteW` wants, and then hands each request to the door it names. The
    /// entry takes the capability the thread door lent this thread's body — the hand-off is a
    /// worker-only door, and this lane is the one worker that holds it.
    pub(crate) fn spawn(wake: impl Fn() + Send + Sync + 'static) -> Result<Self> {
        Self::start(
            &Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            |ctx| {
                let shell = bt_platform::ShellThread::enter(ctx);
                move |window: NativeWindow, handoff: &Handoff| shell.hand_over(window, handoff)
            },
            wake,
            crate::diagnostics::note,
            |id, terminal| abandoned(id, &terminal, &crate::diagnostics::note),
        )
    }

    /// The lane with its worker started by `executor`, and the door made on that thread from the
    /// capability the executor lent it — the production one above, or a recording one in a test.
    /// `say` is where the ledger's lines go, and `sink` what a closed window's request ends in.
    fn start<M, D>(
        executor: &Executor,
        make_door: M,
        wake: impl Fn() + Send + Sync + 'static,
        say: impl Fn(&str) + Send + Sync + 'static,
        sink: impl Fn(HandoffId, HandoffTerminal) + Send + Sync + 'static,
    ) -> Result<Self>
    where
        M: FnOnce(&WorkerCtx) -> D + Send + 'static,
        D: FnMut(NativeWindow, &Handoff) -> Result<(), HandOffRefusal>,
    {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let ledger_wake = Arc::clone(&wake);
        let ledger = Ledger::new(LANE, Some(BOUND), Saying::PerRequest, say, move || {
            ledger_wake()
        });
        let (jobs, queue) = mpsc::channel::<Job>();
        let publish = Arc::clone(&ledger);
        executor
            .start_worker("bt-os-handoff", Arc::clone(&ledger), move |ctx| {
                run_handoff_lane(ctx, queue, &publish, make_door(ctx), &*wake)
            })
            .map_err(|end| anyhow::anyhow!("{end}"))
            .context("spawn the OS hand-off lane")?;
        Ok(Self {
            ledger,
            jobs,
            sink: Arc::new(sink),
        })
    }

    /// **The earliest request still owed an answer** — every request below it has ended, because
    /// the one worker ends them in the order they were minted (and its end ends the rest). The
    /// drain reads it once, before any window takes its answers ([`Pending::answered`]).
    pub(crate) fn earliest_owed(&self) -> Option<HandoffId> {
        self.ledger.earliest_owed()
    }

    /// **Put one hand-off on the lane and return at once** with its request, or refuse it at the
    /// door with its terminal outcome: [`Declined::LaneFull`] past [`BOUND`],
    /// [`Declined::LaneGone`] once the worker has ended. A refused press mints no identity.
    pub(crate) fn submit(
        &self,
        window: NativeWindow,
        handoff: Handoff,
    ) -> Result<HandoffRequest, Declined> {
        let request = self
            .ledger
            .admit(Abandoned(Arc::clone(&self.sink)))
            .map_err(|refused| match refused {
                Refused::Full { .. } => Declined::LaneFull,
                Refused::Faulted(_) => Declined::LaneGone,
                Refused::TwinEnded => unreachable!("the hand-off lane joins nothing"),
                Refused::NotStarted(_) => {
                    unreachable!(
                        "the hand-off lane's worker is started with the lane, not by a press"
                    )
                }
            })?;
        // A worker that has ended and not yet told the ledger has dropped the queue's receiving
        // end; its end is on its way to the ledger, which ends this request `WorkerGone` with the
        // rest it owes, so the job is let go here.
        let _ = self.jobs.send(Job {
            id: request.id(),
            window,
            handoff,
        });
        Ok(request)
    }
}

/// **The lane's body**: one request, one door inside its task, its terminal published, then the
/// wake — `run_path_verify_worker`'s shape, and nothing else runs here.
fn run_handoff_lane(
    worker: &WorkerCtx,
    jobs: mpsc::Receiver<Job>,
    ledger: &Ledger<HandoffId, Abandoned>,
    mut execute: impl FnMut(NativeWindow, &Handoff) -> Result<(), HandOffRefusal>,
    wake: &dyn Fn(),
) {
    while let Ok(Job {
        id,
        window,
        handoff,
    }) = jobs.recv()
    {
        let terminal = match lane::task(worker, |_ctx| {
            Ok::<_, Infallible>(execute(window, &handoff))
        }) {
            Ok(outcome) => Terminal::Done(outcome),
            Err(failure) => Terminal::Failed(failure),
        };
        ledger.publish(id, terminal);
        wake();
    }
}

/// **What one window owes the answers to its own hand-offs** — each request it holds, by id, with
/// what its answer is owed.
///
/// Held by the window, so that closing the window is what abandons them: the requests go with it,
/// and their terminals to the lane's sink.
pub(crate) struct Pending<D> {
    owed: BTreeMap<HandoffId, (HandoffRequest, D)>,
}

impl<D> Default for Pending<D> {
    fn default() -> Self {
        Self {
            owed: BTreeMap::new(),
        }
    }
}

impl<D> Pending<D> {
    /// Hold `request`, and remember what its answer is owed.
    pub(crate) fn owe(&mut self, request: HandoffRequest, duty: D) {
        self.owed.insert(request.id(), (request, duty));
    }

    /// **Take every answer below `owed`** — the lane's [`HandoffLane::earliest_owed`], read once
    /// for every window of one drain — in the order the lane minted the requests, without waiting
    /// for one; each request's terminal taken once, with its duty. A request at or past `owed` is
    /// left for the next drain even when it has ended since, so that no window takes an answer
    /// before an earlier one another window has not been asked for yet.
    pub(crate) fn answered(&mut self, owed: Option<HandoffId>) -> Vec<(HandoffId, Answer, D)> {
        let ended: Vec<(HandoffId, HandoffTerminal)> = self
            .owed
            .iter_mut()
            .take_while(|(id, _)| owed.is_none_or(|owed| **id < owed))
            .filter_map(|(id, (request, _))| Some((*id, request.take()?)))
            .collect();
        ended
            .into_iter()
            .filter_map(|(id, terminal)| {
                let (_, duty) = self.owed.remove(&id)?;
                Some((id, Answer::of(terminal), duty))
            })
            .collect()
    }
}

/// **Every window's answers, in press order across the windows** — what each window `K` took from
/// its own [`Pending`], merged by the id the ledger minted, which is the order the presses were
/// made in and the one worker ran them in (the seam's [`lane::in_admission_order`]). The
/// application answers them in this order, each in the window that asked.
pub(crate) fn in_press_order<K, D>(
    per_window: impl IntoIterator<Item = (K, Vec<(HandoffId, Answer, D)>)>,
) -> Vec<(K, HandoffId, Answer, D)>
where
    K: Copy,
{
    let mut keyed = Vec::new();
    for (window, answers) in per_window {
        let mut owed = Vec::new();
        for (id, answer, duty) in answers {
            owed.push((id, (answer, duty)));
        }
        keyed.push((window, owed));
    }
    let mut all = Vec::new();
    for (window, id, (answer, duty)) in lane::in_admission_order(keyed) {
        all.push((window, id, answer, duty));
    }
    all
}

/// **What one window owes one of its hand-offs' answers** — the refusal's words, and what a
/// surface asked to happen on either answer.
pub(crate) struct Duty {
    pub(crate) refusal: Refusal,
    pub(crate) accepted: Option<OnAccepted>,
    pub(crate) refused: Option<OnRefused>,
}

impl Duty {
    /// **What the window does for `answer`**, as values — the one function the window's answer
    /// and the tests both read.
    ///
    /// Every refusal says the surface's own words with its reason (the stderr line, the files
    /// notice), and then what the surface asked for beyond them, in the words of who refused.
    /// The door's or the system's refusal is said by its kind (T-HANDOFF-DOOR-REASON): only
    /// [`HandOffRefusal::NoHandler`] says there is no program to open it; a target that is not
    /// there says not found; one the system kept from this process says access denied; any other
    /// says it was not opened; every kind's own detail (the code, the path, the system's error) is
    /// left to the stderr line. A full lane says so; a lane
    /// whose worker has stopped — at the door, or with the hand-off owed — says hand-offs have
    /// stopped; a hand-off that failed, or whose window could not be named, says it could not
    /// run. None claims the system declined what it was never asked.
    pub(crate) fn reply(self, answer: Answer) -> Reply {
        let could_not_run = || {
            (
                crate::i18n::Text::HandoffCouldNotRun.text().to_owned(),
                crate::LinkRefusal::CouldNotRun,
            )
        };
        let stopped = || {
            (
                crate::i18n::Text::HandoffsStopped.text().to_owned(),
                crate::LinkRefusal::WorkerStopped,
            )
        };
        let (reason, (shown, link)) = match answer {
            Answer::HandedOver => return Reply::HandedOver(self.accepted),
            Answer::Refused(declined) => {
                let reason = declined.reason().to_owned();
                let said = match declined {
                    Declined::System(refusal) => {
                        let link = crate::LinkRefusal::of_door(&refusal);
                        (link.word(crate::i18n::current()).to_owned(), link)
                    }
                    Declined::LaneFull => (
                        crate::i18n::Text::HandoffsTooMany.text().to_owned(),
                        crate::LinkRefusal::TooMany,
                    ),
                    Declined::LaneGone => stopped(),
                    Declined::NoWindow(_) => could_not_run(),
                };
                (reason, said)
            }
            Answer::Failed(words) => (words, could_not_run()),
            Answer::WorkerGone => (LANE_GONE.to_owned(), stopped()),
        };
        Reply::Refused {
            words: self.refusal.words(&reason),
            then: self.refused.map(|then| match then {
                OnRefused::HyperlinkRefused(hyperlink) => FollowUp::Hover(hyperlink, link),
                OnRefused::PreviewAddressRefused(surface, address) => {
                    FollowUp::Address(surface, address, link)
                }
                OnRefused::FontsToast => FollowUp::FontsToast(shown),
            }),
        }
    }
}

/// **What a window does for one answer** ([`Duty::reply`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Reply {
    /// The system took it: the confirmation the surface named, if any.
    HandedOver(Option<OnAccepted>),
    /// It was refused, failed, or lost with the lane's worker: the surface's own words, and what
    /// the surface asked to happen beyond them.
    Refused {
        words: RefusalWords,
        then: Option<FollowUp>,
    },
}

/// **What a surface shows beyond a refusal's words**, with the refusal it says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FollowUp {
    /// The hover line under the address a `Ctrl`+click was on.
    Hover(bt_viewport::HyperlinkHit, crate::LinkRefusal),
    /// The notice a refused address raises on the preview surface a link was pressed on.
    Address(crate::PreviewSurface, String, crate::LinkRefusal),
    /// The settings dialog's card under `Install fonts…`, holding these words.
    FontsToast(String),
}

/// **One hand-off as a surface asks for it**: the request, and what its answer is owed — said
/// before it is handed off, because a press refused at the lane's door is answered at once.
pub(crate) struct Ask {
    pub(crate) handoff: Handoff,
    pub(crate) duty: Duty,
}

impl Ask {
    /// A hand-off whose refusal says `refusal`'s words, and that owes nothing else.
    pub(crate) fn new(handoff: Handoff, refusal: Refusal) -> Self {
        Self {
            handoff,
            duty: Duty {
                refusal,
                accepted: None,
                refused: None,
            },
        }
    }

    /// What to draw when the system has taken it — a confirmation that claims the hand-off
    /// happened, which is why it waits for the answer.
    pub(crate) fn when_handed_over(mut self, then: OnAccepted) -> Self {
        self.duty.accepted = Some(then);
        self
    }

    /// What to say, beyond the refusal's own words, when the system declines it.
    pub(crate) fn if_refused(mut self, then: OnRefused) -> Self {
        self.duty.refused = Some(then);
        self
    }
}

/// What a surface draws when the system has taken its hand-off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OnAccepted {
    /// A foot's `Revealed` — it claims the reveal happened, so it waits for the answer.
    Revealed(crate::RevealedFoot),
    /// The no-preview card's `Opened`.
    PreviewOpened,
}

/// What a surface says, beyond the refusal's own words, when the system declines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OnRefused {
    /// The hover line under the address a `Ctrl`+click was on.
    HyperlinkRefused(bt_viewport::HyperlinkHit),
    /// The settings dialog's card under `Install fonts…`, holding the refusal's words.
    FontsToast,
    /// The notice a refused address raises on the preview surface a link was pressed on — a
    /// document's link to a scheme the machine has no handler for (ticket 14).
    PreviewAddressRefused(crate::PreviewSurface, String),
}

/// **What a refusal says and where**, fixed per surface when the press is made.
///
/// Each field is what that surface's code said on the window thread before the lane existed, so
/// a refusal that now arrives later arrives in the same words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Refusal {
    /// The stderr line's lead and the context the door's reason was wrapped in —
    /// `eprintln!("{lead}: {context}: {reason}")`, which is what `anyhow`'s `{:#}` printed.
    /// `None` for a surface that wrote no line.
    pub(crate) stderr: Option<(&'static str, &'static str)>,
    /// Whether the door's program refusal ([`bt_platform::PROGRAM_REFUSED`]) is told to the
    /// reader in the files notice — the surfaces where the user picked the file.
    pub(crate) program_notice: bool,
}

/// What a refusal produces, as values — the stderr line and the notice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RefusalWords {
    pub(crate) line: Option<String>,
    pub(crate) notice: Option<&'static str>,
}

impl Refusal {
    /// **The words a refusal with `reason` produces** — the one function the window's answer
    /// and the test both read.
    pub(crate) fn words(self, reason: &str) -> RefusalWords {
        RefusalWords {
            line: self
                .stderr
                .map(|(lead, context)| format!("{lead}: {context}: {reason}")),
            notice: (self.program_notice && reason.contains(bt_platform::PROGRAM_REFUSED))
                .then(files_program_refused_notice),
        }
    }
}

/// What the tree says when a row it will not open is activated.
///
/// **Short on purpose.** The status overlay is right-aligned into the pane's own
/// width and keeps the *tail* of anything longer, so a sentence that does not
/// fit loses its beginning — and the beginning is the half that says what
/// happened. Measured against a files column beside one terminal in a 1030pt
/// window, which leaves the shell about 37 columns: this is 36, and what a
/// narrower pane drops (`The files tree`) still leaves a clause that reads.
///
/// The advice it might have added — that this is a terminal, and typing the
/// name is how you run one — is left out for the same reason. It doubled the
/// length to say something the prompt underneath is already saying.
pub(crate) fn files_program_refused_notice() -> &'static str {
    i18n::Text::FilesProgramRefused.text()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::lane::Gate;

    /// What a test lane's sink for the abandoned was handed, in order.
    type Sunk = Arc<Mutex<Vec<(HandoffId, HandoffTerminal)>>>;

    /// A lane on the product's executor whose door writes down what it was handed and answers
    /// `answer`, so the lane's ledger, queue, thread and wake are the real ones and only the door
    /// is a stand-in; its wakes come back on a channel, and what its sink is handed is kept.
    fn recording_lane(
        executor: &Executor,
        answer: impl Fn(&Handoff) -> Result<(), HandOffRefusal> + Send + 'static,
    ) -> (
        HandoffLane,
        Arc<Mutex<Vec<Handoff>>>,
        mpsc::Receiver<()>,
        Sunk,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let sunk: Sunk = Arc::default();
        let kept = Arc::clone(&sunk);
        let (wake, wakes) = crate::lane::wake_channel();
        let lane = HandoffLane::start(
            executor,
            move |_ctx| {
                move |_window: NativeWindow, handoff: &Handoff| {
                    log.lock().expect("the log").push(handoff.clone());
                    answer(handoff)
                }
            },
            wake,
            |_line: &str| {},
            move |id, terminal| kept.lock().expect("the sink").push((id, terminal)),
        )
        .expect("the lane starts");
        (lane, seen, wakes, sunk)
    }

    /// Every answer to `count` requests `pending` holds, each awaited on the lane's own wake —
    /// which follows its terminal's publication — within the lane suite's patience.
    fn answers<D>(
        lane: &HandoffLane,
        pending: &mut Pending<D>,
        wakes: &mpsc::Receiver<()>,
        count: usize,
    ) -> Vec<(HandoffId, Answer, D)> {
        let mut all = pending.answered(lane.earliest_owed());
        while all.len() < count {
            crate::lane::wait_for_a_wake(wakes, &format!("answer {} of {count}", all.len() + 1));
            all.extend(pending.answered(lane.earliest_owed()));
        }
        all
    }

    fn window() -> NativeWindow {
        NativeWindow::stand_in(0)
    }

    /// RED (ticket 10; S-5 round 2) — **hand-offs complete in press order, across windows.**
    ///
    /// Two presses in a row are two things the reader asked for in a sequence — a reveal and then
    /// an open, say — and a lane that ran or answered them out of order would put the second
    /// window behind the first, whichever windows they were pressed in. Five presses alternate
    /// between two windows while the suite's executor holds the worker; once all five have ended,
    /// the windows' answers are taken the way `FolioApp::answer_handoffs` takes them, and come
    /// out in press order — not one window's and then the other's.
    ///
    /// MUTATION: drop the sort in [`lane::in_admission_order`], which [`in_press_order`] merges
    /// through (each window's answers then follow the other's), and the answers come out by
    /// window.
    #[test]
    fn handoffs_complete_in_press_order() {
        let gate = Arc::new(Gate::default());
        let (lane, seen, wakes, _) =
            recording_lane(&Executor::scripted(Arc::clone(&gate)), |_| Ok(()));
        let requests: Vec<Handoff> = (0..5)
            .map(|at| Handoff::Address(format!("https://example.invalid/{at}")))
            .collect();
        let mut windows: [Pending<()>; 2] = [Pending::default(), Pending::default()];
        let mut ids = Vec::new();
        gate.hold();
        for (at, request) in requests.iter().enumerate() {
            let admitted = lane
                .submit(window(), request.clone())
                .expect("the lane admits it");
            ids.push(admitted.id());
            // The first press in the second window, so that window order is not press order.
            windows[(at + 1) % 2].owe(admitted, ());
        }
        gate.release();
        for at in 0..ids.len() {
            crate::lane::wait_for_a_wake(&wakes, &format!("answer {} of {}", at + 1, ids.len()));
        }
        let [first, second] = &mut windows;
        let owed = lane.earliest_owed();
        let answered: Vec<HandoffId> =
            in_press_order([(0, first.answered(owed)), (1, second.answered(owed))])
                .into_iter()
                .map(|(_, id, answer, ())| {
                    assert_eq!(answer, Answer::HandedOver);
                    id
                })
                .collect();
        assert_eq!(answered, ids, "answered in the order pressed");
        assert_eq!(
            *seen.lock().expect("the log"),
            requests,
            "executed in the order pressed"
        );
    }

    /// RED (S-5 round 4) — **an answer that ends while the windows are being read waits for the
    /// next drain**, so press order holds across drains, not only inside one.
    ///
    /// Window A asked 1, 3 and 5, window B 2, 4 and 6. With 1–3 answered and the worker held on
    /// 4, a drain reads the earliest request still owed (4), then A takes its answers; the worker
    /// is let go and answers 4, 5 and 6 before B is read — the moment the scheduler can put
    /// between two windows' reads. B takes only 2, below the mark, so the first drain answers 1,
    /// 2, 3 and the next 4, 5, 6. Read without the mark, B would take 2, 4 and 6 at once and A's 5
    /// would come after 6. (This race is what made `OrderIsAsDeclared` fail now and then on a
    /// machine whose worker ran between the two windows' reads.)
    ///
    /// MUTATION: take every ended answer whatever the mark (`owed` ignored in
    /// [`Pending::answered`]), and 6 is answered before 5.
    #[test]
    fn an_answer_that_ends_while_the_windows_are_read_waits_for_the_next_drain() {
        let gate = Arc::new(Gate::default());
        let (lane, _, wakes, _) =
            recording_lane(&Executor::scripted(Arc::clone(&gate)), |_| Ok(()));
        let mut a: Pending<()> = Pending::default();
        let mut b: Pending<()> = Pending::default();
        let mut press = |into_a: bool| {
            let request = lane
                .submit(window(), Handoff::FontsPage)
                .expect("the lane admits it");
            let id = request.id();
            if into_a {
                a.owe(request, ());
            } else {
                b.owe(request, ());
            }
            id
        };
        let mut ids = vec![press(true), press(false), press(true)];
        for at in 0..3 {
            crate::lane::wait_for_a_wake(&wakes, &format!("answer {} of 3", at + 1));
        }
        gate.hold();
        ids.push(press(false));
        assert!(gate.wait_entered(4), "the worker holds the fourth press");
        ids.push(press(true));
        ids.push(press(false));

        let owed = lane.earliest_owed();
        assert_eq!(owed, Some(ids[3]), "the fourth is the earliest still owed");
        let first_a = a.answered(owed);
        gate.release();
        for at in 3..6 {
            crate::lane::wait_for_a_wake(&wakes, &format!("answer {} of 6", at + 1));
        }
        let first_b = b.answered(owed);
        let first: Vec<HandoffId> = in_press_order([(0, first_a), (1, first_b)])
            .into_iter()
            .map(|(_, id, _, ())| id)
            .collect();
        let owed = lane.earliest_owed();
        let second: Vec<HandoffId> = in_press_order([(0, a.answered(owed)), (1, b.answered(owed))])
            .into_iter()
            .map(|(_, id, _, ())| id)
            .collect();
        assert_eq!(
            first,
            ids[..3],
            "the first drain answers what ended before it"
        );
        assert_eq!(
            second,
            ids[3..],
            "and the next drain the rest, in press order"
        );
    }

    /// RED (S-5 round 2) — **a hand-off that could not run is said on its surface, in words of its
    /// own.**
    ///
    /// The fonts card's press has no stderr line and no notice: its refusal is the card under
    /// `Install fonts…`. A hand-off that panicked, or that the lane's stopped worker took with it,
    /// is still answered there, with "the hand-off could not run" or "hand-offs have
    /// stopped" — never silence, and never the system's refusal. A link's hover line and an
    /// address's notice say the same two refusals, not "no program to open it".
    ///
    /// MUTATION: answer `Answer::Failed` in [`Duty::reply`] with no follow-up (`then: None`, as
    /// the window's answer did when it returned after the refusal's words), and the fonts card
    /// says nothing.
    #[test]
    fn a_hand_off_that_could_not_run_is_said_on_its_surface() {
        let fonts = || Duty {
            refusal: Refusal {
                stderr: None,
                program_notice: false,
            },
            accepted: None,
            refused: Some(OnRefused::FontsToast),
        };
        let failed = Answer::Failed("the task panicked: no shell".to_owned());
        assert_eq!(
            fonts().reply(failed.clone()),
            Reply::Refused {
                words: RefusalWords {
                    line: None,
                    notice: None,
                },
                then: Some(FollowUp::FontsToast(
                    crate::i18n::Text::HandoffCouldNotRun.text().to_owned()
                )),
            },
            "a failed hand-off from the fonts card raises the card"
        );
        assert_eq!(
            fonts().reply(Answer::WorkerGone),
            Reply::Refused {
                words: RefusalWords {
                    line: None,
                    notice: None,
                },
                then: Some(FollowUp::FontsToast(
                    crate::i18n::Text::HandoffsStopped.text().to_owned()
                )),
            },
            "so does one the stopped worker took"
        );
        assert_eq!(
            fonts().reply(Answer::Refused(Declined::System(
                HandOffRefusal::NoHandler("this machine has no Font Book".to_owned())
            ))),
            Reply::Refused {
                words: RefusalWords {
                    line: None,
                    notice: None,
                },
                then: Some(FollowUp::FontsToast(
                    crate::i18n::Text::HyperlinkNoProgram.text().to_owned()
                )),
            },
            "and a refusal still raises it, in the words of its kind"
        );

        let surface = crate::PreviewSurface::Seat(crate::LeafId {
            tab: crate::TabId(1),
            seat: bt_layout::SeatId(2),
        });
        let address = |refusal| Duty {
            refusal: Refusal {
                stderr: Some(("recoverable link hand-off failure", "hand a link over")),
                program_notice: false,
            },
            accepted: None,
            refused: Some(refusal),
        };
        let link = OnRefused::PreviewAddressRefused(surface, "mailto:a@example.invalid".into());
        assert_eq!(
            address(link.clone()).reply(failed),
            Reply::Refused {
                words: RefusalWords {
                    line: Some(
                        "recoverable link hand-off failure: hand a link over: the task \
                         panicked: no shell"
                            .to_owned()
                    ),
                    notice: None,
                },
                then: Some(FollowUp::Address(
                    surface,
                    "mailto:a@example.invalid".to_owned(),
                    crate::LinkRefusal::CouldNotRun
                )),
            },
            "an address's notice says the hand-off could not run, not that nothing opens it"
        );
        assert_eq!(
            address(link).reply(Answer::WorkerGone),
            Reply::Refused {
                words: RefusalWords {
                    line: Some(format!(
                        "recoverable link hand-off failure: hand a link over: {LANE_GONE}"
                    )),
                    notice: None,
                },
                then: Some(FollowUp::Address(
                    surface,
                    "mailto:a@example.invalid".to_owned(),
                    crate::LinkRefusal::WorkerStopped
                )),
            },
        );
    }

    /// RED (S-5; the side-effects contract's C6) — **a hand-off asked by a window that closed
    /// reaches the abandoned sink**, and no other window.
    ///
    /// The answer to a hand-off belongs to the window that asked: a confirmation or a refusal
    /// raised in a different window would be a sentence about something that reader never did. A
    /// window holds its requests in its own [`Pending`], so closing the window drops them, and
    /// each one's terminal goes to the lane's sink for the abandoned — which outlives every
    /// window — exactly once. No other window takes it, including one opened afterwards, which
    /// starts empty. The product's sink lets a success go and says a refusal no window is left to
    /// hear ([`abandoned`]).
    ///
    /// MUTATION: make `<Abandoned as Owed>::sink` discard what it is handed (an empty body), and
    /// the closed window's answer reaches nothing.
    #[test]
    fn a_hand_off_asked_by_a_window_that_closed_reaches_the_abandoned_sink() {
        let (lane, _, wakes, sunk) = recording_lane(
            &Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            |_| Ok(()),
        );
        let mut first: Pending<&str> = Pending::default();
        let mut second: Pending<&str> = Pending::default();
        let asked = lane
            .submit(window(), Handoff::FontsPage)
            .expect("the lane admits it");
        let asked_id = asked.id();
        first.owe(asked, "the first window's confirmation");
        let other = lane
            .submit(window(), Handoff::FontsPage)
            .expect("the lane admits it");
        let other_id = other.id();
        second.owe(other, "the second window's confirmation");
        // The first window closes before its answer is taken.
        drop(first);
        let mut third: Pending<&str> = Pending::default();

        // One thread, in press order: by the time the second window's answer is in, the first's
        // has ended — in the sink, wherever the window's close fell.
        assert_eq!(
            answers(&lane, &mut second, &wakes, 1),
            vec![(
                other_id,
                Answer::HandedOver,
                "the second window's confirmation"
            )],
            "the window that asked still hears its own, and only its own"
        );
        assert_eq!(
            third.answered(lane.earliest_owed()),
            Vec::new(),
            "nor one opened since"
        );
        assert_eq!(
            *sunk.lock().expect("the sink"),
            vec![(asked_id, Terminal::Done(Ok(())))],
            "the closed window's answer reached the sink, once"
        );

        // The product's sink: a refusal is said, a success let go.
        let said = Mutex::new(Vec::new());
        let say = |line: &str| said.lock().expect("the lines").push(line.to_owned());
        abandoned(asked_id, &Terminal::Done(Ok(())), &say);
        abandoned(
            asked_id,
            &Terminal::Done(Err(HandOffRefusal::Other(
                "no program is registered for it".to_owned(),
            ))),
            &say,
        );
        let said = said.into_inner().expect("the lines");
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].ends_with("was refused: no program is registered for it"),
            "{said:?}"
        );
    }

    /// RED (S-5 round 4) — **a press refused at the lane's door says who refused it**, on every
    /// surface, and never that nothing can open it.
    ///
    /// The real lane refuses a press synchronously when it is full and when its worker has ended;
    /// each refusal reaches the link's hover line, the address's notice and the fonts card in its
    /// own words — "too many hand-offs at once", "hand-offs have stopped" — and the surface's
    /// stderr line with the lane's reason. A window that could not be named says the hand-off
    /// could not run. Only the door's or the system's own refusal says "no program to open it".
    ///
    /// MUTATION: map every `Answer::Refused` to `LinkRefusal::NoProgram` with its reason as the
    /// card's words in [`Duty::reply`] (as round 3 did), and each door refusal says "no program".
    #[test]
    fn a_press_refused_at_the_lanes_door_says_who_refused_it() {
        let gate = Arc::new(Gate::default());
        let (wake, _wakes) = crate::lane::wake_channel();
        let lane = HandoffLane::start(
            &Executor::scripted(Arc::clone(&gate)),
            |_ctx| |_window: NativeWindow, _handoff: &Handoff| Ok(()),
            wake,
            |_line: &str| {},
            |_id, _terminal| {},
        )
        .expect("the lane starts");
        gate.hold();
        let mut held: Pending<()> = Pending::default();
        held.owe(
            lane.submit(window(), Handoff::FontsPage)
                .expect("the first press is admitted"),
            (),
        );
        assert!(gate.wait_entered(1), "the worker starts the first press");
        for _ in 0..CAPACITY {
            held.owe(
                lane.submit(window(), Handoff::FontsPage)
                    .expect("a press within the bound is admitted"),
                (),
            );
        }
        let full = lane
            .submit(window(), Handoff::FontsPage)
            .err()
            .expect("the press past the bound is refused at the door");
        assert_eq!(full, Declined::LaneFull);
        gate.release();
        drop(held);
        drop(lane);

        let hyperlink = crate::test_support::hyperlink_hit("https://example.invalid/");
        let surface = crate::PreviewSurface::Seat(crate::LeafId {
            tab: crate::TabId(1),
            seat: bt_layout::SeatId(2),
        });
        let duty = |refused: OnRefused| Duty {
            refusal: Refusal {
                stderr: Some(("recoverable link hand-off failure", "hand a link over")),
                program_notice: false,
            },
            accepted: None,
            refused: Some(refused),
        };
        let surfaces = |declined: &Declined| -> Vec<Reply> {
            [
                OnRefused::HyperlinkRefused(hyperlink.clone()),
                OnRefused::PreviewAddressRefused(surface, "https://example.invalid/".to_owned()),
                OnRefused::FontsToast,
            ]
            .into_iter()
            .map(|refused| duty(refused).reply(Answer::Refused(declined.clone())))
            .collect()
        };
        let expected = |reason: &str, words: crate::i18n::Text, link: crate::LinkRefusal| {
            let words_of = RefusalWords {
                line: Some(format!(
                    "recoverable link hand-off failure: hand a link over: {reason}"
                )),
                notice: None,
            };
            vec![
                Reply::Refused {
                    words: words_of.clone(),
                    then: Some(FollowUp::Hover(hyperlink.clone(), link)),
                },
                Reply::Refused {
                    words: words_of.clone(),
                    then: Some(FollowUp::Address(
                        surface,
                        "https://example.invalid/".to_owned(),
                        link,
                    )),
                },
                Reply::Refused {
                    words: words_of,
                    then: Some(FollowUp::FontsToast(words.text().to_owned())),
                },
            ]
        };
        assert_eq!(
            surfaces(&full),
            expected(
                LANE_FULL,
                crate::i18n::Text::HandoffsTooMany,
                crate::LinkRefusal::TooMany
            ),
            "a full lane says so on every surface"
        );
        assert_eq!(
            surfaces(&Declined::LaneGone),
            expected(
                LANE_GONE,
                crate::i18n::Text::HandoffsStopped,
                crate::LinkRefusal::WorkerStopped
            ),
            "a stopped lane says so on every surface"
        );
        assert_eq!(
            surfaces(&Declined::NoWindow("no window handle".to_owned())),
            expected(
                "no window handle",
                crate::i18n::Text::HandoffCouldNotRun,
                crate::LinkRefusal::CouldNotRun
            ),
            "a window that could not be named: the hand-off could not run"
        );
        let no_handler =
            HandOffRefusal::NoHandler("ShellExecuteW failed with code 31 (SE_ERR_NOASSOC)".into());
        let system = surfaces(&Declined::System(no_handler.clone()));
        assert_eq!(
            system[0],
            Reply::Refused {
                words: RefusalWords {
                    line: Some(format!(
                        "recoverable link hand-off failure: hand a link over: {}",
                        no_handler.reason()
                    )),
                    notice: None,
                },
                then: Some(FollowUp::Hover(
                    hyperlink.clone(),
                    crate::LinkRefusal::NoProgram
                )),
            },
            "and only the system's refusal says nothing can open it"
        );
    }

    /// RED (T-HANDOFF-DOOR-REASON) — **the door's refusal is said by its kind on every
    /// surface**, and "no program to open it" only when no program is registered.
    ///
    /// The door answers a [`HandOffRefusal`]; the real lane carries it, as the door returned it,
    /// to the window that asked, where [`Duty::reply`] says it on the link's hover line, the
    /// address's notice and the fonts card: no handler — "no program to open it"; not found —
    /// "not found"; access denied — "access denied"; any other — "not opened", with the door's
    /// or the system's own detail of every kind on the stderr line only.
    ///
    /// MUTATION: map every `Declined::System` to `LinkRefusal::NoProgram` in [`Duty::reply`] (as
    /// it was before the door said which), and the three other kinds say "no program".
    #[test]
    fn the_doors_refusal_is_said_by_its_kind_on_every_surface() {
        let kinds = [
            HandOffRefusal::NoHandler("ShellExecuteW failed with code 31 (SE_ERR_NOASSOC)".into()),
            HandOffRefusal::NotFound("\"/Users/a/文档.md\": not there".to_owned()),
            HandOffRefusal::AccessDenied(
                "ShellExecuteW failed with code 5 (SE_ERR_ACCESSDENIED)".to_owned(),
            ),
            HandOffRefusal::Other("ShellExecuteW failed with code 26 (SE_ERR_SHARE)".to_owned()),
        ];
        // Through the real lane: each address's door answers one kind, and the window that
        // asked hears that kind, unchanged.
        let answering = kinds.clone();
        let (lane, _, wakes, _) = recording_lane(
            &Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            move |handoff| match handoff {
                Handoff::Address(address) => Err(answering[address
                    .strip_prefix("https://例子.invalid/")
                    .and_then(|at| at.parse::<usize>().ok())
                    .expect("one of the test's addresses")]
                .clone()),
                other => panic!("only addresses are handed over here: {other:?}"),
            },
        );
        let mut pending: Pending<()> = Pending::default();
        for at in 0..kinds.len() {
            pending.owe(
                lane.submit(
                    window(),
                    Handoff::Address(format!("https://例子.invalid/{at}")),
                )
                .expect("the lane admits it"),
                (),
            );
        }
        let heard: Vec<Answer> = answers(&lane, &mut pending, &wakes, kinds.len())
            .into_iter()
            .map(|(_, answer, ())| answer)
            .collect();
        assert_eq!(
            heard,
            kinds
                .iter()
                .map(|kind| Answer::Refused(Declined::System(kind.clone())))
                .collect::<Vec<_>>(),
            "the lane carries the door's refusal as the door returned it"
        );

        let hyperlink = crate::test_support::hyperlink_hit("https://例子.invalid/文档");
        let surface = crate::PreviewSurface::Seat(crate::LeafId {
            tab: crate::TabId(1),
            seat: bt_layout::SeatId(2),
        });
        let duty = |refused: OnRefused| Duty {
            refusal: Refusal {
                stderr: Some(("recoverable link hand-off failure", "hand a link over")),
                program_notice: false,
            },
            accepted: None,
            refused: Some(refused),
        };
        let expected = [
            (crate::LinkRefusal::NoProgram, "no program to open it"),
            (crate::LinkRefusal::NotFound, "not found"),
            (crate::LinkRefusal::AccessDenied, "access denied"),
            (crate::LinkRefusal::NotOpened, "not opened"),
        ];
        let lang = crate::i18n::current();
        for ((answer, kind), (link, english)) in heard.into_iter().zip(&kinds).zip(expected) {
            let line = Some(format!(
                "recoverable link hand-off failure: hand a link over: {}",
                kind.reason()
            ));
            let words = RefusalWords { line, notice: None };
            assert_eq!(
                duty(OnRefused::HyperlinkRefused(hyperlink.clone())).reply(answer.clone()),
                Reply::Refused {
                    words: words.clone(),
                    then: Some(FollowUp::Hover(hyperlink.clone(), link)),
                },
                "the hover line, for {kind:?}"
            );
            assert_eq!(
                duty(OnRefused::PreviewAddressRefused(
                    surface,
                    "https://例子.invalid/文档".to_owned()
                ))
                .reply(answer.clone()),
                Reply::Refused {
                    words: words.clone(),
                    then: Some(FollowUp::Address(
                        surface,
                        "https://例子.invalid/文档".to_owned(),
                        link
                    )),
                },
                "the address's notice, for {kind:?}"
            );
            assert_eq!(
                duty(OnRefused::FontsToast).reply(answer),
                Reply::Refused {
                    words,
                    then: Some(FollowUp::FontsToast(link.word(lang).to_owned())),
                },
                "the fonts card, for {kind:?}"
            );
            assert_eq!(
                link.word(crate::i18n::Lang::English),
                english,
                "{kind:?} says its own words"
            );
            assert_eq!(
                link.word(crate::i18n::Lang::English) == "no program to open it",
                matches!(kind, HandOffRefusal::NoHandler(_)),
                "only no handler says there is no program: {kind:?}"
            );
        }
    }

    /// RED (S-5 round 3; D-70) — **a hand-off's panic is contained under the resident panic
    /// hook.** The product's hook ends the process for a panic it is not told is contained, before
    /// any catch runs; the door is called inside the seam's task, so the hook's decision there is
    /// yes and a panicking hand-off is that request's `Failed`, not the end of Folio.
    ///
    /// MUTATION: run both the seam's task and its worker body under `catch_unwind` instead of
    /// `contained_panic::contain` (`lane::task`, `Executor::start_worker`), and the decision at the
    /// door is no. (Either one alone still contains the door.)
    #[test]
    fn a_hand_offs_panic_is_contained_under_the_resident_hook() {
        let (told, heard) = mpsc::channel();
        let told = Mutex::new(told);
        let (lane, _, wakes, _) = recording_lane(
            &Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            move |_| {
                let _ = told
                    .lock()
                    .expect("the channel")
                    .send(crate::the_hook_lets_this_panic_unwind());
                Ok(())
            },
        );
        let mut pending: Pending<()> = Pending::default();
        pending.owe(
            lane.submit(window(), Handoff::FontsPage)
                .expect("the lane admits it"),
            (),
        );
        assert!(
            crate::lane::receive(&heard, "the door's decision"),
            "the door runs where the hook lets a panic unwind"
        );
        assert_eq!(answers(&lane, &mut pending, &wakes, 1).len(), 1);
    }

    /// PIN (ticket 10) — **a flood of presses is refused at the lane's door, never waited on.**
    ///
    /// The window thread may not block on the lane. With the door held, the ledger fills to
    /// [`BOUND`] — the one running and [`CAPACITY`] behind it — and every press past that is
    /// refused at once, with [`LANE_FULL`] as its terminal; each admitted press is its own request
    /// with its own id, so nothing is merged to make room.
    ///
    /// MUTATION: build the lane's ledger with no bound (`None` for `Some(BOUND)` in
    /// `HandoffLane::start`), and no press is refused.
    #[test]
    fn a_flood_of_presses_is_refused_rather_than_waited_on() {
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().expect("the gate");
        let inside = Arc::clone(&gate);
        let (lane, _, _, _) = recording_lane(
            &Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            move |_| {
                drop(inside.lock().expect("the gate"));
                Ok(())
            },
        );
        let started = Instant::now();
        let presses: Vec<Result<HandoffRequest, Declined>> = (0..BOUND + 3)
            .map(|at| lane.submit(window(), Handoff::Open(PathBuf::from(format!("{at}")))))
            .collect();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the press never waits"
        );
        let ids: Vec<HandoffId> = presses
            .iter()
            .filter_map(|press| press.as_ref().ok().map(lane::Request::id))
            .collect();
        assert!(
            ids.windows(2).all(|pair| pair[0] < pair[1]),
            "every press is its own request"
        );
        let refused: Vec<&Declined> = presses
            .iter()
            .filter_map(|press| press.as_ref().err())
            .collect();
        assert_eq!(
            refused,
            vec![&Declined::LaneFull; 3],
            "the presses past the bound are refused at the door"
        );
        drop(held);
    }

    /// RED (S-5; D-71) — **the hand-off lane refuses synchronously above its bound**, and its
    /// answers held are bounded by the same number.
    ///
    /// With the worker held on the suite's executor, the ledger takes [`BOUND`] presses and
    /// refuses the next at the door, minting nothing for it. An answer its window has not taken
    /// holds its place: with every request answered and none taken, a press is still refused; once
    /// the window takes its answers, the next press is admitted, under the next id.
    ///
    /// MUTATION: build the lane's ledger with no bound (`None` for `Some(BOUND)` in
    /// `HandoffLane::start`) — unbounded admission — and the press past the bound is admitted.
    #[test]
    fn the_hand_off_lane_refuses_synchronously_above_its_bound() {
        let gate = Arc::new(Gate::default());
        let (wake, wakes) = crate::lane::wake_channel();
        let lane = HandoffLane::start(
            &Executor::scripted(Arc::clone(&gate)),
            |_ctx| |_window: NativeWindow, _handoff: &Handoff| Ok(()),
            wake,
            |_line: &str| {},
            |_id, _terminal| {},
        )
        .expect("the lane starts");
        let press = || lane.submit(window(), Handoff::FontsPage);
        gate.hold();
        let mut pending: Pending<()> = Pending::default();
        let first = press().expect("the first press is admitted");
        let first_id = first.id();
        pending.owe(first, ());
        assert!(gate.wait_entered(1), "the worker starts the first press");
        for _ in 0..CAPACITY {
            pending.owe(press().expect("a press within the bound is admitted"), ());
        }
        assert_eq!(
            press().err(),
            Some(Declined::LaneFull),
            "refused at the door, at once"
        );

        gate.release();
        for at in 0..BOUND {
            crate::lane::wait_for_a_wake(&wakes, &format!("answer {} of {BOUND}", at + 1));
        }
        assert_eq!(
            press().err(),
            Some(Declined::LaneFull),
            "an answer not yet taken holds its place"
        );
        assert_eq!(
            pending.answered(lane.earliest_owed()).len(),
            BOUND,
            "every press answered"
        );
        let next = press().expect("taken answers make room");
        assert_eq!(
            next.id(),
            HandoffId(first_id.0 + BOUND as u64),
            "a refused press minted no identity"
        );
    }

    /// RED (ticket 10) — **a refused hand-off raises the same words it did before.**
    ///
    /// The refusal used to be read off the door's `Result` on the window thread, wrapped in an
    /// `anyhow` context and printed with `{:#}`; the files notice was raised when that text held
    /// the program refusal. The door now answers on the lane, so the words are rebuilt from an
    /// [`Answer`] — and a reader must not be able to tell. Run through the **real** lane with the
    /// **real** [`bt_platform::ShellThread`] over the **real** verifier's answer for a real
    /// folder, against the old code's own spelling of the same line. The door's own answer for that
    /// spelling is asked through the only road to it that is left (A1b made the verbs private to
    /// `bt_platform::handoff`): a `ShellThread` entered on a second thread the thread door started,
    /// handed the same two requests — not through the lane under test.
    ///
    /// The program is a folder so that one fixture is a program on both machines without touching
    /// a mode bit: `payload.exe` is one by name to Windows, `Payload.app` is one by bundle to
    /// macOS, and `bt_term::verify_path` answers both. A third platform's door refuses everything
    /// with its own sentence, and the line is held to that sentence the same way.
    ///
    /// MUTATION: format the line as `"{lead}: {reason}"` in [`Refusal::words`], or drop the
    /// `contains(PROGRAM_REFUSED)` test, and the words differ.
    #[test]
    fn a_refused_handoff_raises_the_same_words_it_did_before() {
        let scratch = bt_testpath::temp_path("folio-handoff-refusal");
        let program = scratch.join(match bt_platform::host_platform() {
            bt_platform::HostPlatform::MacOs => "Payload.app",
            _ => "payload.exe",
        });
        std::fs::create_dir_all(&program).expect("a scratch program");
        let missing = scratch.join("gone.md");
        let facts = crate::verified_target_of(Some(&bt_term::verify_path(
            &program,
            &bt_platform::resolved_for_a_door,
        )));
        assert!(facts.exists, "the verifier saw the fixture");

        let (wake, wakes) = crate::lane::wake_channel();
        let lane = HandoffLane::spawn(wake).expect("the lane starts");
        let requests = [
            Handoff::OpenVerified(program.clone(), facts.clone()),
            Handoff::OpenVerified(missing.clone(), bt_platform::VerifiedTarget::absent()),
        ];
        let mut pending: Pending<()> = Pending::default();
        for request in &requests {
            pending.owe(
                lane.submit(window(), request.clone())
                    .expect("the lane admits it"),
                (),
            );
        }
        let answered = answers(&lane, &mut pending, &wakes, requests.len());

        // The old surface: `Runtime::open_local_path_verified` as it stood on the window thread.
        let refusal = Refusal {
            stderr: Some((
                "recoverable reference open failure",
                "open a printed reference with its default handler",
            )),
            program_notice: true,
        };
        let before = |door: Result<(), HandOffRefusal>| {
            let error = door
                .map_err(|error| anyhow::anyhow!(error.reason().to_owned()))
                .context("open a printed reference with its default handler")
                .expect_err("the door refuses this");
            let text = format!("{error:#}");
            RefusalWords {
                line: Some(format!("recoverable reference open failure: {text}")),
                notice: text
                    .contains(bt_platform::PROGRAM_REFUSED)
                    .then(files_program_refused_notice),
            }
        };
        let doors = bt_platform::spawn_at_priority(
            "bt-test-handoff-door",
            bt_platform::ThreadPriority::BelowNormal,
            move |ctx| {
                let shell = bt_platform::ShellThread::enter(ctx);
                requests.map(|request| shell.hand_over(window(), &request))
            },
        )
        .expect("the door starts a thread")
        .join()
        .expect("the door answered");
        let old = doors.map(before);
        let reasons: Vec<String> = answered
            .iter()
            .map(|(_, answer, ())| match answer {
                Answer::Refused(Declined::System(refusal)) => refusal.reason().to_owned(),
                other => panic!("the lane carried the door's refusal, not {other:?}"),
            })
            .collect();
        for (reason, old) in reasons.iter().zip(old) {
            assert_eq!(refusal.words(reason), old, "{reason}");
        }
        if bt_platform::host_platform() != bt_platform::HostPlatform::OtherUnix {
            assert_eq!(
                refusal.words(&reasons[0]).notice,
                Some(files_program_refused_notice()),
                "a program is refused in the reader's words"
            );
        }

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// RED GATE (ticket 10) — **no hand-off runs on the window thread**: nothing in the product
    /// names a `bt_platform::handoff` door, or the thread type that calls them, except this
    /// module — so every one of them is reached through the lane and none from a `Runtime`
    /// method.
    ///
    /// Read through `bt_source` rather than as text of a file, so a door call moved into any file
    /// of this crate is still found, and its owner named. Both spellings of each door are asked:
    /// the crate-root re-export (`bt_platform::open_local_path`) and the module path
    /// (`handoff::open_local_path`), and a `use` that imports either — which would let a bare call
    /// hide from the first two — is an occurrence outside every callable and is refused as well.
    ///
    /// MUTATION: make `Runtime::open_local_path` call
    /// `bt_platform::open_local_path(native, path)` directly again, and this names it.
    #[test]
    fn no_handoff_runs_on_the_window_thread() {
        use bt_source::{Index, Pattern, Search, View, needle};

        let index = Index::of_package("bt-app");
        let mut names: Vec<String> = [
            "open_local_path",
            "open_local_path_verified",
            "reveal_in_explorer",
            "reveal_verified",
            "shell_execute",
            "open_local_file",
            "open_system_fonts_page",
        ]
        .iter()
        .flat_map(|door| [format!("bt_platform::{door}"), format!("handoff::{door}")])
        .collect();
        names.push("bt_platform::ShellThread".to_owned());
        let mut lane_callers = 0;
        for name in &names {
            let found = index
                .search(&Search::new(
                    needle!(Pattern::path(name)),
                    View::Identifiers,
                ))
                .unwrap_or_else(|failure| panic!("{failure}"))
                .in_the_product(index);
            assert_eq!(
                found.outside_items(index),
                0,
                "`{name}` is imported or named outside any function:\n{}",
                found.report(index)
            );
            for (owner, count) in found.owners(index) {
                assert!(
                    owner.module_path == "crate::handoff_lane"
                        || owner.module_path.starts_with("crate::handoff_lane::"),
                    "`{name}` is called from {owner} ({count}×), outside the OS hand-off lane — \
                     a door called there runs on the window thread:\n{}",
                    found.report(index)
                );
                lane_callers += count;
            }
        }
        assert!(
            lane_callers >= 1,
            "the lane itself no longer reaches the doors, so this gate is reading nothing"
        );
    }
}

/// **The lane contract's adapter** (`crate::lane`, `lane_contract_tests`): the real lane — its
/// ledger, its queue, its worker body, its sink and each window's [`Pending`] — with the suite's
/// scripted [`crate::lane::Executor`] starting the worker, so the suite's [`crate::lane::Gate`]
/// holds each task, ends the worker or fails a hand-off, and its [`crate::lane::WakeProbe`] stands
/// for the loop's wake. Only the door is a stand-in; nothing of the lane is copied.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};

    use bt_platform::{Handoff, NativeWindow};

    use super::{Answer, HandoffId, HandoffLane, HandoffTerminal, Pending, in_press_order};
    use crate::lane::{
        Admission, Contract, Delivered, Executor, Gate, HANDOFF, LaneUnderTest, Outcome, WakeProbe,
    };

    /// The address a question is handed over as, so the door can say which one it runs.
    const ADDRESS: &str = "https://lane-contract.invalid/";

    fn question_of(handoff: &Handoff) -> Option<u64> {
        match handoff {
            Handoff::Address(address) => address.strip_prefix(ADDRESS)?.parse().ok(),
            _ => None,
        }
    }

    fn outcome(answer: Answer) -> Outcome {
        match answer {
            Answer::HandedOver => Outcome::Answered,
            Answer::Refused(declined) => Outcome::Refused(declined.reason().to_owned()),
            Answer::Failed(words) => Outcome::Failed(words),
            Answer::WorkerGone => Outcome::WorkerGone,
        }
    }

    struct HandoffAdapter {
        lane: HandoffLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        /// One `Pending` per open window; a closed window's is dropped, as `WindowRuntime`'s is.
        windows: BTreeMap<u32, Pending<()>>,
        /// The suite's own record of which question each id carried, for its tally.
        questions: HashMap<HandoffId, u64>,
        /// What the lane's sink for the abandoned has been handed and the suite not yet read.
        sunk: Arc<Mutex<Vec<(HandoffId, HandoffTerminal)>>>,
        /// The lines the ledger said, counted through its `say`.
        said: Arc<AtomicUsize>,
    }

    /// A fresh lane, its `bt-os-handoff` worker started by the suite's executor.
    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let sunk = Arc::new(Mutex::new(Vec::new()));
        let said = Arc::new(AtomicUsize::new(0));
        let door = Arc::clone(&gate);
        let wake = Arc::clone(&probe);
        let (kept, counted) = (Arc::clone(&sunk), Arc::clone(&said));
        let lane = HandoffLane::start(
            &Executor::scripted(Arc::clone(&gate)),
            move |_ctx| {
                move |_window: NativeWindow, handoff: &Handoff| {
                    // The suite's file, held while the hand-off runs.
                    let held = question_of(handoff).map(|question| door.execute(question));
                    drop(held);
                    Ok(())
                }
            },
            move || wake.woke(),
            move |_line: &str| {
                counted.fetch_add(1, Ordering::SeqCst);
            },
            move |id, terminal| {
                kept.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((id, terminal));
            },
        )
        .expect("the hand-off lane starts");
        Box::new(HandoffAdapter {
            lane,
            gate,
            probe,
            windows: BTreeMap::new(),
            questions: HashMap::new(),
            sunk,
            said,
        })
    }

    impl LaneUnderTest for HandoffAdapter {
        fn contract(&self) -> &'static Contract {
            &HANDOFF
        }

        fn gate(&self) -> &Gate {
            &self.gate
        }

        fn probe(&self) -> &WakeProbe {
            &self.probe
        }

        /// `Runtime::hand_off`: the lane's own admission, the request held in the asking window's
        /// `Pending`, or the refusal at the door as the press's terminal.
        fn submit(&mut self, target: u32, question: u64) -> Admission {
            match self.lane.submit(
                NativeWindow::stand_in(0),
                Handoff::Address(format!("{ADDRESS}{question}")),
            ) {
                Ok(request) => {
                    let id = request.id();
                    self.questions.insert(id, question);
                    self.windows.entry(target).or_default().owe(request, ());
                    Admission {
                        ticket: Some(id.0),
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

        /// `FolioApp::answer_handoffs`: every open window takes the answers it holds.
        fn drain(&mut self) -> Vec<Delivered> {
            let owed = self.lane.earliest_owed();
            let held: Vec<(u32, _)> = self
                .windows
                .iter_mut()
                .map(|(window, pending)| (*window, pending.answered(owed)))
                .collect();
            in_press_order(held)
                .into_iter()
                .map(|(window, id, answer, ())| Delivered {
                    target: Some(window),
                    ticket: Some(id.0),
                    question: self.questions.get(&id).copied(),
                    outcome: outcome(answer),
                })
                .collect()
        }

        fn offer_stale(&mut self, _ticket: u64) {
            unreachable!("the hand-off lane's target is the asking window, not the application");
        }

        fn sunk(&mut self) -> Vec<Delivered> {
            let sunk =
                std::mem::take(&mut *self.sunk.lock().unwrap_or_else(PoisonError::into_inner));
            sunk.into_iter()
                .map(|(id, terminal)| Delivered {
                    target: None,
                    ticket: Some(id.0),
                    question: self.questions.get(&id).copied(),
                    outcome: outcome(Answer::of(terminal)),
                })
                .collect()
        }

        fn said(&self) -> Option<usize> {
            Some(self.said.load(Ordering::SeqCst))
        }
    }
}
