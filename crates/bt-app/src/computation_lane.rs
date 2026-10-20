//! **The computation lane** — a formula's typesetting, a picture's decode, a video's and a
//! page's glance, a picture's resample and a printed path's check, each on a worker and never on
//! the window thread (`docs/ARCHITECTURE.md` §5.1; `docs/DESIGN.md`, *the computation lane
//! completes every request*).
//!
//! **Built on the completion seam** (`crate::lane`; the side-effects contract's C1–C7, revision
//! (b)7; S-8). The lane is three shares, each a [`crate::lane::Queue`] with a
//! [`crate::lane::Ledger`] of its own and one worker started by a [`crate::lane::Executor`] (the
//! thread door's in the product, the suite's scripted one under test):
//!
//! | share | thread | what it runs |
//! |---|---|---|
//! | [`Share::Decoration`] | `bt-math-worker` | formulas, decodes, a video's frame, an animation's frames, a page's raster, a preview's formula |
//! | [`Share::Scale`] | `bt-image-scale-worker` | a decoded picture resampled to the size it is drawn at |
//! | [`Share::Path`] | `bt-path-verify-worker` | whether a printed path names anything |
//!
//! **Three ledgers, not one**, because the three threads are three rows of the completions
//! ledger with three bounds and three deaths: the path thread may block for the redirector's
//! twenty-one seconds on a dead share, and a bound it shared with the formulas would let one
//! hostile line of output fill the formulas' lane; and a ledger ends **every** identity it owes
//! when its worker ends, so one ledger over three threads would end the live threads' requests
//! with the dead one's. The drain is still one (`AppEvent::MathReady`): it takes each share's
//! terminals below that share's own drain mark, and the shares' answers are independent of each
//! other, so no order across them is owed.
//!
//! # The contract
//!
//! * **Identity.** Every admitted request gets a [`ComputationId`] minted by its share's ledger,
//!   and is owed to its asker ([`Asker`]): the tab whose pane asked, or the window whose surface
//!   did ([`AnswerOwner`]). The lane holds every request it admitted until the window thread takes
//!   its terminal ([`ComputationLane::ended`]).
//! * **Ordering.** One worker per share, first in first out: requests run, and end, in the order
//!   their identities were minted.
//! * **Supersession.** Nothing on the formula and path shares is coalesced: one question asked
//!   twice is two requests, two runs and two terminals — but for a window's open of an animated
//!   file and its read of a file's pixels: a newer ask of the same file for the same window
//!   **supersedes** the one waiting for it ([`crate::MathWorkerRequest::relate`]), because the
//!   window asks again only after letting the older read go (the file changed under it), and the
//!   window files an answer only under the identity its record awaits, so the answer of an older
//!   ask that was already running is never filed either (`AnimationCache::awaits`,
//!   `peek_awaits`). On the scale share a newer request for the same picture **from the same
//!   asker** ([`crate::ScaleWorkerRequest::same_target`]) that waits behind one not yet started
//!   **supersedes** it when it asks for another size (`Superseded { by }`) and **joins** it when
//!   it asks for the same size (`Joined { with }`) — revision (b)4. A superseded or joined
//!   terminal reaches its asker's bookkeeping and nothing on screen (owner ruling Q4): the newer
//!   request's answer, or the one it joined, is what lands.
//!
//!   **The address names the asker down to the slot that takes the answer**
//!   (T-SCALE-ADDRESS-PER-ASKER; CONVENTIONS「共享队列上的地址必须一路唯一到取答案的那一层」),
//!   because a joined terminal carries no answer of its own and a superseded one none at all: two
//!   askers under one address would leave one of them waiting for good. The leaf names the pane
//!   or the tab's focused shell; below it the purpose ([`crate::ScalePurpose`]) names a pane's
//!   printed picture by its occurrence, a glance by its surface ([`crate::PeekSurface`]), a
//!   preview's picture by its own number ([`crate::PictureAsker`], carried home on the answer),
//!   and a page's picture by its size, which is how the window's raster cache files it. So a
//!   newer size displaces only its own asker's older one, which that asker has already stopped
//!   awaiting, and a join answers one asker once.
//!   A supersession is announced by a wake, and the drain takes a superseded terminal at once —
//!   past the drain mark, since it carries no answer — so it leaves the bound while the run before
//!   it is still out.
//! * **Capacity.** Each share's ledger owes at most its bound ([`DECORATION_BOUND`],
//!   [`SCALE_BOUND`], [`PATH_BOUND`]), an answer not yet taken holding its place. The bounds are
//!   sizing choices, not derived worst cases: each is what one pane's session can hold queued for
//!   that share, so one hand-out pass of one pane fits; a second pane or window asking at the same
//!   time reaches it sooner. A request past it is refused at the door, synchronously, with no
//!   identity minted (revision (b)3), and handed back to its asker: a pane's task goes back to the
//!   head of its pane's queue (`DualPlaneSession::give_back_decoration_worker_task`), only that
//!   share's work stops for that hand-out, and the next hand-out (the next output, frame or
//!   answer) hands it out again; a window's own surface records nothing as asked, so its next
//!   resolve asks again. Nothing is retried on a clock (revision (b)5, rule 1).
//! * **Completion.** `Done` carries the work's answer — an error the work can report (a decode it
//!   refuses, a formula the engine refuses, a render panic that `bt_math`'s own boundary turns
//!   into `MathRenderError::Aborted`) is part of that answer. Any other panic in one request's
//!   work is that request's `Failed`, caught by its [`crate::lane::task`] and said once per streak
//!   by the share's ledger, and the worker goes on. Nothing on screen changes, and its question
//!   is asked again only by new input to the surface that asked (`crate::settle_failure`): a
//!   pane holds the task until a resize, a scroll, or output that changed the task's own content
//!   (`Runtime::settle_failed_requests`); a window surface hears its own error answer — a failed
//!   decode, a refused animation or formula — or keeps its asked mark, and asks again at its next
//!   new input: a new hover, the file opened again, a new size, new formula content.
//!   A worker's end ends every request **of that share** still owed `WorkerGone`, faults that
//!   share (every later request on it refused at the door) and wakes the loop: the death is
//!   observed per share, the other two threads still answering inside the lane; the product's
//!   answer to it is the whole feature's, as it always was — the window thread turns the lane off
//!   once, with its notice, and no surface asks any share again.
//! * **Start.** Each share's worker is started by the first request made of it; a start the
//!   thread door refuses refuses that request at the door ([`Refused::NotStarted`]), said once per
//!   streak, and leaves the share unfaulted. The pane whose task it was hands nothing more out
//!   until its next new input, which tries the start once; a window surface marks the request
//!   asked and failed, as for a failure, and asks again at its own next new input.
//! * **Abandonment.** A request whose asker has gone — a tab closed, a window closed — is let go
//!   by the drain before it takes anything ([`ComputationLane::ended`]); its terminal, now or
//!   when it comes, goes to the lane's sink, which ignores it by design (C6): what it would have
//!   decorated is gone, and a failure in it was said by the ledger. A pane closed inside a tab
//!   that stays open is the tab's: its answer reaches the tab, which finds no pane to give it to.
//! * **Wake.** Each worker publishes the terminal first and then wakes the loop
//!   (`AppEvent::MathReady`); each ledger wakes it the same way when its worker ends.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bt_platform::admission::WorkerCtx;

use crate::lane::{self, Executor, Ledger, Owed, Queue, Refused, Relation, Saying, Terminal};
use crate::{
    AnswerOwner, Asked, MathWorkerRequest, MathWorkerResult, PathWorkerRequest, ScaleWorkerRequest,
};

/// **How many requests the formula and decode share owes at once** — a sizing choice: what one
/// pane's session can hold queued for this worker (its frozen and live formula queues, its pasted
/// pictures and its printed picture paths, `bt_term::DECORATION_TASKS_HELD`), so one hand-out pass
/// of one pane fits. A window's own surfaces (the glance card, a preview's formulas, an
/// animation's frames) and a second pane asking at the same time share it and reach it sooner.
pub(crate) const DECORATION_BOUND: usize = bt_term::DECORATION_TASKS_HELD;

/// **How many requests the resample share owes at once** — a sizing choice: one resample for each
/// picture one pane's session can hold for decoding (`bt_term::DECODE_TASKS_HELD`).
pub(crate) const SCALE_BOUND: usize = bt_term::DECODE_TASKS_HELD;

/// **How many requests the path share owes at once** — a sizing choice: one pane's whole queue of
/// printed paths (`bt_term::PATH_TASKS_HELD`).
pub(crate) const PATH_BOUND: usize = bt_term::PATH_TASKS_HELD;

/// **What a surface reads off its request's admission** — whether to mark it asked: admitted, or
/// its thread not started (asked, and failed: the surface asks again only on its own new input);
/// and whether the lane was full (nothing out: the next resolve asks again).
pub(crate) trait Asking {
    fn is_ok_or_unstarted(&self) -> bool;
    fn is_full(&self) -> bool;
}

impl<Job> Asking for Result<ComputationId, (Refused, Job)> {
    fn is_ok_or_unstarted(&self) -> bool {
        matches!(self, Ok(_) | Err((Refused::NotStarted(_), _)))
    }

    fn is_full(&self) -> bool {
        matches!(self, Err((Refused::Full { .. }, _)))
    }
}

/// **One request's name**, minted by its share's ledger and unique within that share.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct ComputationId(u64);

impl From<u64> for ComputationId {
    fn from(minted: u64) -> Self {
        Self(minted)
    }
}

impl ComputationId {
    /// The number the ledger minted.
    #[cfg(test)]
    pub(crate) fn number(self) -> u64 {
        self.0
    }
}

/// **Which of the lane's three workers a request is for.**
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) enum Share {
    /// `bt-math-worker`: formulas, decodes and glances.
    Decoration,
    /// `bt-image-scale-worker`: resamples.
    Scale,
    /// `bt-path-verify-worker`: printed paths.
    Path,
}

/// **One request's answer**, held for whoever takes it: the request's asker, or — for a request
/// that joined it — its twin's. The seam carries a terminal to each identity that ends with it, so
/// the answer is shared; the answer itself (a decoded picture, an animation's decoder) is not a
/// value to copy, so it is taken out once, by the first that takes it ([`Self::take`]).
#[derive(Clone)]
pub(crate) struct Answer(Arc<Mutex<Option<MathWorkerResult>>>);

impl fmt::Debug for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Answer(..)")
    }
}

impl Answer {
    fn new(result: MathWorkerResult) -> Self {
        Self(Arc::new(Mutex::new(Some(result))))
    }

    /// The answer, once: `None` when it has been taken already.
    pub(crate) fn take(&self) -> Option<MathWorkerResult> {
        locked(&self.0).take()
    }
}

/// How one request ended.
pub(crate) type ComputationTerminal = Terminal<ComputationId, Answer, Infallible>;

/// The lane's sink for the abandoned, shared by every request it admits.
type Sink = Arc<dyn Fn(ComputationId, ComputationTerminal) + Send + Sync>;

/// **What one request is owed** (C6): the lane's sink for the abandoned, which its terminal goes
/// to when its asker — the tab or window the lane holds it for — has gone.
pub(crate) struct Asker {
    sink: Sink,
}

impl Owed<ComputationId> for Asker {
    type Answer = Answer;
    type Error = Infallible;

    fn sink(self, id: ComputationId, terminal: ComputationTerminal) {
        (self.sink)(id, terminal);
    }
}

/// One request held by the lane for its asker, until the window thread takes its terminal.
type Held = lane::Request<ComputationId, Asker>;

/// **One worker's door**: what the worker runs for one request, made on the worker's own thread
/// from the capability the thread door lent it. The product's are [`crate::run_decoration_worker`],
/// [`crate::scale_door`] and [`crate::path_door`].
pub(crate) type Door<Job> = Box<dyn FnMut(&WorkerCtx, Job) -> MathWorkerResult>;

/// **One request that has ended, taken by the window thread**: its share, its identity, its
/// terminal, and who asked what ([`crate::Asked`]) — which a `Failed` terminal settles.
pub(crate) type Ended = (
    Share,
    ComputationId,
    ComputationTerminal,
    (AnswerOwner, Asked),
);

/// **One share**: its queue (and with it its ledger), and the requests it holds for their askers,
/// by identity.
struct ShareOf<Job> {
    share: Share,
    queue: Arc<Queue<ComputationId, Asker, Job>>,
    held: Mutex<BTreeMap<ComputationId, (Held, AnswerOwner, Asked)>>,
}

impl<Job: Send + 'static> ShareOf<Job> {
    /// A share named `lane` in the lines its ledger says, owing at most `bound` requests.
    fn new(
        share: Share,
        lane: &'static str,
        bound: usize,
        say: Arc<dyn Fn(&str) + Send + Sync>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let ledger = Ledger::new(
            lane,
            Some(bound),
            Saying::PerStreak,
            move |line| say(line),
            move || wake(),
        );
        Self {
            share,
            queue: Arc::new(Queue::new(ledger)),
            held: Mutex::new(BTreeMap::new()),
        }
    }

    /// **Admit one job for `owner`** — the worker started through `start` if it is not started,
    /// the job's identity minted, its request held — or refuse it at the door and hand it back.
    fn ask(
        &self,
        job: Job,
        owner: AnswerOwner,
        asked: Asked,
        sink: &Sink,
        relate: impl Fn(&Job, &Job) -> Relation,
        start: impl FnOnce(Arc<lane::Ran<ComputationId, Asker>>) -> Result<(), lane::WorkerEnd>,
    ) -> Result<ComputationId, (Refused, Job)> {
        let asker = Asker {
            sink: Arc::clone(sink),
        };
        let request = self.queue.ask(job, asker, relate, start)?;
        let id = request.id();
        locked(&self.held).insert(id, (request, owner, asked));
        Ok(id)
    }

    /// **The worker's body**: the door made on its thread, then every job in line run through it
    /// inside its task — until the lane is let go.
    fn serve(
        queue: &Queue<ComputationId, Asker, Job>,
        ctx: &WorkerCtx,
        mut door: Door<Job>,
        wake: &dyn Fn(),
    ) {
        queue.serve(ctx, |ctx, job| Ok(Answer::new(door(ctx, job))), wake);
    }

    /// Let go of every request whose asker has `gone`: each one's terminal goes to the sink.
    fn let_go(&self, gone: &impl Fn(AnswerOwner) -> bool) {
        let abandoned: Vec<Held> = {
            let mut held = locked(&self.held);
            let ids: Vec<ComputationId> = held
                .iter()
                .filter(|(_, (_, owner, _))| gone(*owner))
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| held.remove(&id).map(|(request, _, _)| request))
                .collect()
        };
        // Dropped outside the lock: a request's drop hands its terminal to the sink.
        drop(abandoned);
    }

    /// **Every terminal below the share's drain mark** ([`Ledger::earliest_owed`], read once,
    /// before the requests are looked at), in the order the ledger minted them, each taken once —
    /// and every `Superseded` terminal past the mark too: it carries no answer, so no answer's
    /// order depends on it, and taking it at once frees its place in the share's bound while the
    /// run before it is still out (a divider drag during a long resample).
    fn ended(&self, into: &mut Vec<Ended>) {
        let owed = self.queue.ledger().earliest_owed();
        let mut held = locked(&self.held);
        let taken: Vec<(ComputationId, ComputationTerminal)> = held
            .iter_mut()
            .filter_map(|(id, (request, _, _))| {
                let below = owed.is_none_or(|owed| *id < owed);
                let terminal = request
                    .take_if(|terminal| below || matches!(terminal, Terminal::Superseded { .. }))?;
                Some((*id, terminal))
            })
            .collect();
        for (id, terminal) in taken {
            if let Some((_, owner, asked)) = held.remove(&id) {
                into.push((self.share, id, terminal, (owner, asked)));
            }
        }
    }

    /// How many requests the share holds for their askers.
    #[cfg(test)]
    fn held(&self) -> usize {
        locked(&self.held).len()
    }
}

/// **The lane, held by the application**: its three shares, the executor their workers are
/// started by, the wake, and the sink a gone asker's request is owed.
pub(crate) struct ComputationLane {
    decoration: ShareOf<MathWorkerRequest>,
    scale: ShareOf<ScaleWorkerRequest>,
    path: ShareOf<PathWorkerRequest>,
    executor: Executor,
    wake: Arc<dyn Fn() + Send + Sync>,
    sink: Sink,
}

impl ComputationLane {
    /// **The product's lane**: workers started through the thread door, below normal, by the
    /// first request each share is asked; the ledgers' lines said in `diagnostics.log`; and the
    /// sink that ignores what a gone asker is owed.
    pub(crate) fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::start_with(
            Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            wake,
            crate::diagnostics::note,
            |_, _| {},
        )
    }

    /// The lane with its workers started by `executor`, waking through `wake`, saying its ledgers'
    /// lines through `say`, and ending a gone asker's request in `sink`.
    pub(crate) fn start_with(
        executor: Executor,
        wake: impl Fn() + Send + Sync + 'static,
        say: impl Fn(&str) + Send + Sync + 'static,
        sink: impl Fn(ComputationId, ComputationTerminal) + Send + Sync + 'static,
    ) -> Self {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let say: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(say);
        Self {
            decoration: ShareOf::new(
                Share::Decoration,
                "the formula and picture lane",
                DECORATION_BOUND,
                Arc::clone(&say),
                Arc::clone(&wake),
            ),
            scale: ShareOf::new(
                Share::Scale,
                "the picture resample lane",
                SCALE_BOUND,
                Arc::clone(&say),
                Arc::clone(&wake),
            ),
            path: ShareOf::new(
                Share::Path,
                "the path verification lane",
                PATH_BOUND,
                say,
                Arc::clone(&wake),
            ),
            executor,
            wake,
            sink: Arc::new(sink),
        }
    }

    /// **Ask for one formula, decode or glance** (C1): admitted and held for its asker — its
    /// identity answered — or refused at the door and handed back. Never waits for the worker.
    /// Nothing is coalesced: one question asked twice is two requests — but a newer open of an
    /// animated file, or a newer read of a file's pixels, supersedes the same window's ask of that
    /// file still waiting ([`crate::MathWorkerRequest::relate`]).
    pub(crate) fn decorate(
        &self,
        request: MathWorkerRequest,
    ) -> Result<ComputationId, (Refused, MathWorkerRequest)> {
        self.decorate_through(request, crate::run_decoration_worker)
    }

    /// [`Self::decorate`], the formula share's worker — when this request is the one that starts
    /// it — answering through the door `make` makes on its thread.
    pub(crate) fn decorate_through(
        &self,
        request: MathWorkerRequest,
        make: impl FnOnce(&WorkerCtx) -> Door<MathWorkerRequest> + Send + 'static,
    ) -> Result<ComputationId, (Refused, MathWorkerRequest)> {
        let owner = request.owner();
        let asked = request.asked();
        let queue = Arc::clone(&self.decoration.queue);
        let wake = Arc::clone(&self.wake);
        self.decoration.ask(
            request,
            owner,
            asked,
            &self.sink,
            MathWorkerRequest::relate,
            |ran| {
                // **This thread says how much stack it wants**
                // (`bt_math::MATH_WORKER_STACK_BYTES`). One formula's render descends through a
                // LaTeX parser, then a LaTeX-to-Typst converter, then Typst's parser, math layout
                // and SVG writer, all over text a program printed into a terminal; a stack
                // overflow is not a panic and would take the window with it, so the depth that is
                // refused and the stack it is refused against are chosen together, in `bt-math`,
                // and this is where the second half of that pair is spent.
                self.executor
                    .with_stack(bt_math::MATH_WORKER_STACK_BYTES)
                    .start_worker("bt-math-worker", ran, move |ctx| {
                        ShareOf::serve(&queue, ctx, make(ctx), &*wake);
                    })
            },
        )
    }

    /// **Ask for one resample** (C1). A request for the same picture from the same asker as one
    /// waiting supersedes it, or joins it when it asks for the same size.
    #[expect(
        clippy::result_large_err,
        reason = "permanent: a refused request is handed back by value to the surface that asked, \
                  as large as the request it is"
    )]
    pub(crate) fn scale(
        &self,
        request: ScaleWorkerRequest,
    ) -> Result<ComputationId, (Refused, ScaleWorkerRequest)> {
        self.scale_through(request, crate::scale_door)
    }

    /// [`Self::scale`], the resample share's worker — when this request is the one that starts
    /// it — answering through the door `make` makes on its thread.
    #[expect(
        clippy::result_large_err,
        reason = "permanent: a refused request is handed back by value to the surface that asked, \
                  as large as the request it is"
    )]
    pub(crate) fn scale_through(
        &self,
        request: ScaleWorkerRequest,
        make: impl FnOnce(&WorkerCtx) -> Door<ScaleWorkerRequest> + Send + 'static,
    ) -> Result<ComputationId, (Refused, ScaleWorkerRequest)> {
        let owner = request.owner();
        let asked = request.asked();
        let queue = Arc::clone(&self.scale.queue);
        let wake = Arc::clone(&self.wake);
        self.scale.ask(
            request,
            owner,
            asked,
            &self.sink,
            |waiting, asked| {
                if !waiting.same_target(asked) {
                    Relation::Other
                } else if waiting.size() == asked.size() {
                    Relation::Same
                } else {
                    Relation::Displaces
                }
            },
            |ran| {
                self.executor
                    .start_worker("bt-image-scale-worker", ran, move |ctx| {
                        ShareOf::serve(&queue, ctx, make(ctx), &*wake);
                    })
            },
        )
    }

    /// **Ask about one printed path** (C1). Nothing is coalesced.
    pub(crate) fn verify(
        &self,
        request: PathWorkerRequest,
    ) -> Result<ComputationId, (Refused, PathWorkerRequest)> {
        self.verify_through(request, crate::path_door)
    }

    /// [`Self::verify`], the path share's worker — when this request is the one that starts it —
    /// answering through the door `make` makes on its thread.
    pub(crate) fn verify_through(
        &self,
        request: PathWorkerRequest,
        make: impl FnOnce(&WorkerCtx) -> Door<PathWorkerRequest> + Send + 'static,
    ) -> Result<ComputationId, (Refused, PathWorkerRequest)> {
        let owner = request.owner();
        let asked = request.asked();
        let queue = Arc::clone(&self.path.queue);
        let wake = Arc::clone(&self.wake);
        self.path.ask(
            request,
            owner,
            asked,
            &self.sink,
            |_, _| Relation::Other,
            |ran| {
                self.executor
                    .start_worker("bt-path-verify-worker", ran, move |ctx| {
                        ShareOf::serve(&queue, ctx, make(ctx), &*wake);
                    })
            },
        )
    }

    /// **The window thread's drain**: first every request whose asker has `gone` is let go — its
    /// terminal, now or when it comes, goes to the sink — then every terminal each share holds
    /// below its drain mark is taken, share by share, each in mint order, and with them every
    /// `Superseded` terminal past the mark (it carries no answer). One lock per share at a time;
    /// never waits for a worker.
    pub(crate) fn ended(&self, gone: impl Fn(AnswerOwner) -> bool) -> Vec<Ended> {
        self.decoration.let_go(&gone);
        self.scale.let_go(&gone);
        self.path.let_go(&gone);
        let mut ended = Vec::new();
        self.decoration.ended(&mut ended);
        self.scale.ended(&mut ended);
        self.path.ended(&mut ended);
        ended
    }

    /// How many requests `share` holds for their askers, ended or not.
    #[cfg(test)]
    pub(crate) fn held(&self, share: Share) -> usize {
        match share {
            Share::Decoration => self.decoration.held(),
            Share::Scale => self.scale.held(),
            Share::Path => self.path.held(),
        }
    }
}

impl Drop for ComputationLane {
    /// The workers run what is in line and return; the requests held go to the sink.
    fn drop(&mut self) {
        self.decoration.queue.close();
        self.scale.queue.close();
        self.path.queue.close();
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// **A lane under a test's eye** — the lane, its wakes, the lines its ledgers said, what its sink
/// took, and every terminal the test has drained.
#[cfg(test)]
pub(crate) mod fixtures {
    use std::sync::mpsc;

    use super::*;

    /// One terminal the sink took: the request's identity and how it ended.
    pub(crate) type Sunk = (ComputationId, ComputationTerminal);

    pub(crate) struct Watched {
        pub(crate) lane: Arc<ComputationLane>,
        pub(crate) wakes: mpsc::Receiver<()>,
        said: Arc<Mutex<Vec<String>>>,
        sunk: Arc<Mutex<Vec<Sunk>>>,
        /// Every terminal drained and not yet asked for, in the order it was taken.
        taken: Vec<Ended>,
    }

    /// A lane started by `executor`, watched.
    pub(crate) fn watched(executor: Executor) -> Watched {
        let (wake, wakes) = lane::wake_channel();
        let said = Arc::new(Mutex::new(Vec::new()));
        let sunk = Arc::new(Mutex::new(Vec::new()));
        let lines = Arc::clone(&said);
        let sink = Arc::clone(&sunk);
        let lane = Arc::new(ComputationLane::start_with(
            executor,
            wake,
            move |line| locked(&lines).push(line.to_owned()),
            move |id, terminal| locked(&sink).push((id, terminal)),
        ));
        Watched {
            lane,
            wakes,
            said,
            sunk,
            taken: Vec::new(),
        }
    }

    impl Watched {
        /// **`share`'s request `id`'s terminal**, drained as the window thread drains — once now,
        /// then once after each wake, each wake awaited within the lane suite's patience — with
        /// `gone` the askers no window holds.
        pub(crate) fn terminal(
            &mut self,
            share: Share,
            id: ComputationId,
            gone: impl Fn(AnswerOwner) -> bool,
        ) -> ComputationTerminal {
            loop {
                let ended = self.lane.ended(&gone);
                self.taken.extend(ended);
                if let Some(at) = self
                    .taken
                    .iter()
                    .position(|(of, taken, _, _)| *of == share && *taken == id)
                {
                    return self.taken.remove(at).2;
                }
                lane::wait_for_a_wake(&self.wakes, "a computation lane's terminal");
            }
        }

        /// **The next terminal `share` ends**, drained as the window thread drains, each wake
        /// awaited within the lane suite's patience.
        pub(crate) fn next_ended(
            &mut self,
            share: Share,
            gone: impl Fn(AnswerOwner) -> bool,
        ) -> Ended {
            loop {
                let ended = self.lane.ended(&gone);
                self.taken.extend(ended);
                if let Some(at) = self.taken.iter().position(|(of, ..)| *of == share) {
                    return self.taken.remove(at);
                }
                lane::wait_for_a_wake(&self.wakes, "a computation lane's terminal");
            }
        }

        /// **Every terminal `share` ends until it holds nothing**, drained as the window thread
        /// drains, each wake awaited within the lane suite's patience, in the order taken.
        pub(crate) fn every_ended(
            &mut self,
            share: Share,
            gone: impl Fn(AnswerOwner) -> bool,
        ) -> Vec<Ended> {
            loop {
                let ended = self.lane.ended(&gone);
                self.taken.extend(ended);
                if self.lane.held(share) == 0 {
                    let (of_share, rest) = std::mem::take(&mut self.taken)
                        .into_iter()
                        .partition(|(of, ..)| *of == share);
                    self.taken = rest;
                    return of_share;
                }
                lane::wait_for_a_wake(&self.wakes, "a computation lane's terminal");
            }
        }

        /// The terminals the sink has taken.
        pub(crate) fn sunk(&self) -> Vec<Sunk> {
            locked(&self.sunk).clone()
        }

        /// The lines the ledgers have said.
        pub(crate) fn said(&self) -> Vec<String> {
            locked(&self.said).clone()
        }
    }

    /// **The product's formula door, telling `heard` whom each request is for** before it runs
    /// it — for [`ComputationLane::decorate_through`].
    pub(crate) fn recording(
        heard: mpsc::Sender<crate::ShellAddress>,
    ) -> impl FnOnce(&WorkerCtx) -> Door<MathWorkerRequest> + Send + 'static {
        move |ctx| {
            let mut real = crate::run_decoration_worker(ctx);
            Box::new(move |ctx: &WorkerCtx, request: MathWorkerRequest| {
                let leaf = match &request {
                    MathWorkerRequest::Math { leaf, .. }
                    | MathWorkerRequest::InlineImage { leaf, .. }
                    | MathWorkerRequest::PeekImage { leaf, .. }
                    | MathWorkerRequest::PeekVideoFrame { leaf, .. }
                    | MathWorkerRequest::PeekAnimation { leaf, .. }
                    | MathWorkerRequest::AnimationFill { leaf, .. }
                    | MathWorkerRequest::PeekPage { leaf, .. }
                    | MathWorkerRequest::PreviewMath { leaf, .. } => *leaf,
                };
                let _ = heard.send(leaf);
                real(ctx, request)
            })
        }
    }

    /// Nobody has gone.
    pub(crate) fn nobody(_: AnswerOwner) -> bool {
        false
    }

    /// The answer a terminal carries, taken.
    pub(crate) fn answer(terminal: ComputationTerminal) -> Option<MathWorkerResult> {
        lane::answer_of(terminal)?.take()
    }
}

/// **The lane contract suite's adapters for the computation lane** (`crate::lane`): the real
/// [`ComputationLane`], one adapter for each of its three shares, each driven with the suite's
/// scripted executor through the share's own admission ([`ComputationLane::decorate_through`],
/// [`ComputationLane::scale_through`], [`ComputationLane::verify_through`]) and its real door, and
/// the product's acceptance.
///
/// A formula-share question is a pasted picture whose payload is empty, which the real decoder
/// refuses at once without touching a disk; a resample question is a one-pixel picture named by
/// the question; a path question is a file, named by the question, that is not there. Each
/// answer is still the lane's answer to that question. The door is
/// the product's, with the suite's gate told which question it executes (`Gate::execute`), inside
/// the request's task, so a panic the suite plants there is that request's failure. Acceptance is
/// the product's: the drain lets go of a closed tab's requests (`ComputationLane::ended`), and
/// each answer is claimed by its tab (`claimed_by`), as `apply_math_results` asks through
/// `Runtime::owns`; a worker's end turns the lane off once (`disable_math_worker_state`).
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::lane::{
        Admission, COMPUTATION, COMPUTATION_PATH, COMPUTATION_SCALE, Contract, Delivered, Gate,
        LaneUnderTest, Outcome, WakeProbe,
    };
    use crate::{
        DecorationWorkerCompletion, LeafId, SeatId, ShellAddress, TabId, WindowId, claimed_by,
        disable_math_worker_state, math_worker_stopped_notice,
    };

    struct Adapter {
        share: Share,
        lane: ComputationLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        opened: BTreeSet<u32>,
        closed: BTreeSet<u32>,
        sunk: Arc<Mutex<Vec<Delivered>>>,
        said: Arc<AtomicUsize>,
        running: bool,
        notice_pending: bool,
        /// The one picture every resample the adapter asks stands in for: the suite's targets are
        /// the leaves the requests are asked under, so the asker below the leaf is held fixed.
        picture: crate::PictureAsker,
    }

    /// The one window the adapter's tabs stand in.
    fn adapter_window() -> WindowId {
        WindowId::from(1_u64)
    }

    fn tab_of(target: u32) -> TabId {
        TabId(u64::from(target) + 1)
    }

    /// The picture a resample question names: its content key carries the question.
    fn content_of(question: u64) -> String {
        format!("截图 q{question}")
    }

    /// The path a path question names: its file stem carries the question.
    fn path_of(question: u64) -> PathBuf {
        PathBuf::from(format!("不在这里/{question}.md"))
    }

    fn question_of_content(content: &str) -> Option<u64> {
        content.strip_prefix("截图 q")?.parse().ok()
    }

    fn question_of_path(path: &Path) -> Option<u64> {
        path.file_stem()?.to_str()?.parse().ok()
    }

    fn outcome(terminal: &ComputationTerminal) -> Outcome {
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

    /// A fresh lane for one claim, driven through its formula share.
    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        make_for(Share::Decoration)
    }

    /// A fresh lane for one claim, driven through its resample share.
    pub(crate) fn make_scale() -> Box<dyn LaneUnderTest> {
        make_for(Share::Scale)
    }

    /// A fresh lane for one claim, driven through its path share.
    pub(crate) fn make_path() -> Box<dyn LaneUnderTest> {
        make_for(Share::Path)
    }

    fn make_for(share: Share) -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let sunk = Arc::new(Mutex::new(Vec::new()));
        let said = Arc::new(AtomicUsize::new(0));
        let heard = Arc::clone(&probe);
        let counted = Arc::clone(&said);
        let sink = Arc::clone(&sunk);
        let lane = ComputationLane::start_with(
            Executor::scripted(Arc::clone(&gate)),
            move || heard.woke(),
            move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
            },
            move |id, terminal| {
                locked(&sink).push(Delivered {
                    target: None,
                    ticket: Some(id.number()),
                    question: None,
                    outcome: outcome(&terminal),
                });
            },
        );
        Box::new(Adapter {
            share,
            lane,
            gate,
            probe,
            opened: BTreeSet::new(),
            closed: BTreeSet::new(),
            sunk,
            said,
            running: true,
            notice_pending: false,
            picture: crate::PictureAsker::mint(),
        })
    }

    impl Adapter {
        /// Put `question` on the share the adapter drives, for the tab `leaf` stands in, each
        /// share's own door wrapped so the suite's gate is told which question it executes.
        fn ask(&self, leaf: ShellAddress, question: u64) -> Result<ComputationId, Refused> {
            let gate = Arc::clone(&self.gate);
            match self.share {
                Share::Decoration => {
                    // The decode reaches the host's SVG codec for any payload no raster
                    // container claims; install it as `main` does, so the lane's answer never
                    // depends on the payload.
                    crate::test_support::install_host_answers();
                    let request = MathWorkerRequest::InlineImage {
                        leaf,
                        task: bt_term::InlineImageTask {
                            occurrence_id: question,
                            source: bt_term::InlineImageSource::Osc1337(Vec::new()),
                        },
                    };
                    let door = move |ctx: &WorkerCtx| -> Door<MathWorkerRequest> {
                        let mut real = crate::run_decoration_worker(ctx);
                        Box::new(move |ctx: &WorkerCtx, request: MathWorkerRequest| {
                            // The suite's file, held while the real work runs.
                            let held = match &request {
                                MathWorkerRequest::InlineImage { task, .. } => {
                                    Some(gate.execute(task.occurrence_id))
                                }
                                _ => None,
                            };
                            let answer = real(ctx, request);
                            drop(held);
                            answer
                        })
                    };
                    self.lane
                        .decorate_through(request, door)
                        .map_err(|(refused, _)| refused)
                }
                Share::Scale => {
                    let request = ScaleWorkerRequest::Preview {
                        leaf,
                        asker: self.picture,
                        task: crate::test_support::scale_task(&content_of(question), 8),
                    };
                    let door = move |ctx: &WorkerCtx| -> Door<ScaleWorkerRequest> {
                        let mut real = crate::scale_door(ctx);
                        Box::new(move |ctx: &WorkerCtx, request: ScaleWorkerRequest| {
                            // The suite's file, held while the real work runs.
                            let held = match &request {
                                ScaleWorkerRequest::Preview { task, .. } => {
                                    question_of_content(&task.content_key)
                                        .map(|question| gate.execute(question))
                                }
                                _ => None,
                            };
                            let answer = real(ctx, request);
                            drop(held);
                            answer
                        })
                    };
                    self.lane
                        .scale_through(request, door)
                        .map_err(|(refused, _)| refused)
                }
                Share::Path => {
                    let request = PathWorkerRequest {
                        leaf,
                        path: path_of(question),
                    };
                    let door = move |ctx: &WorkerCtx| -> Door<PathWorkerRequest> {
                        let mut real = crate::path_door(ctx);
                        Box::new(move |ctx: &WorkerCtx, request: PathWorkerRequest| {
                            // The suite's file, held while the real work runs.
                            let held = question_of_path(&request.path)
                                .map(|question| gate.execute(question));
                            let answer = real(ctx, request);
                            drop(held);
                            answer
                        })
                    };
                    self.lane
                        .verify_through(request, door)
                        .map_err(|(refused, _)| refused)
                }
            }
        }
    }

    /// The question an answer is for, read off the answer itself.
    fn question_of(result: &MathWorkerResult) -> Option<u64> {
        match &result.completion {
            DecorationWorkerCompletion::InlineImage { task, .. } => Some(task.occurrence_id),
            DecorationWorkerCompletion::PreviewScaledImage { scaled, .. } => {
                question_of_content(&scaled.content_key)
            }
            DecorationWorkerCompletion::VerifiedPath { path, .. } => question_of_path(path),
            _ => None,
        }
    }

    impl LaneUnderTest for Adapter {
        fn contract(&self) -> &'static Contract {
            match self.share {
                Share::Decoration => &COMPUTATION,
                Share::Scale => &COMPUTATION_SCALE,
                Share::Path => &COMPUTATION_PATH,
            }
        }

        fn gate(&self) -> &Gate {
            &self.gate
        }

        fn probe(&self) -> &WakeProbe {
            &self.probe
        }

        fn submit(&mut self, target: u32, question: u64) -> Admission {
            self.opened.insert(target);
            let leaf = ShellAddress {
                window: adapter_window(),
                leaf: LeafId {
                    tab: tab_of(target),
                    seat: SeatId(1),
                },
            };
            match self.ask(leaf, question) {
                Ok(id) => Admission {
                    ticket: Some(id.number()),
                    question,
                    refused: None,
                },
                Err(refused) => Admission {
                    ticket: None,
                    question,
                    refused: Some(format!("{refused:?}")),
                },
            }
        }

        fn close_target(&mut self, target: u32) {
            self.closed.insert(target);
        }

        fn drain(&mut self) -> Vec<Delivered> {
            let open: Vec<TabId> = self
                .opened
                .difference(&self.closed)
                .map(|target| tab_of(*target))
                .collect();
            let ended = self
                .lane
                .ended(|owner| !claimed_by(owner, adapter_window(), open.iter().copied()));
            let mut found = Vec::new();
            let mut worker_gone = false;
            for (_, id, terminal, _) in ended {
                let delivered = match &terminal {
                    Terminal::Done(answer) => {
                        let result = answer.take().expect("an answer is taken once");
                        let target =
                            claimed_by(result.owner(), adapter_window(), open.iter().copied())
                                .then(|| u32::try_from(result.leaf.leaf.tab.0 - 1).ok())
                                .flatten();
                        Delivered {
                            target,
                            ticket: Some(id.number()),
                            question: question_of(&result),
                            outcome: Outcome::Answered,
                        }
                    }
                    other => {
                        worker_gone |= matches!(other, Terminal::WorkerGone);
                        Delivered {
                            target: None,
                            ticket: Some(id.number()),
                            question: None,
                            outcome: outcome(other),
                        }
                    }
                };
                found.push(delivered);
            }
            if worker_gone && disable_math_worker_state(&mut self.running, &mut self.notice_pending)
            {
                found.push(Delivered {
                    target: None,
                    ticket: None,
                    question: None,
                    outcome: Outcome::Fault(math_worker_stopped_notice().to_owned()),
                });
            }
            found
        }

        fn offer_stale(&mut self, _ticket: u64) {
            unreachable!("the computation lane's target is the asking tab, not the application");
        }

        fn sunk(&mut self) -> Vec<Delivered> {
            std::mem::take(&mut *locked(&self.sunk))
        }

        fn said(&self) -> Option<usize> {
            Some(self.said.load(Ordering::SeqCst))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::fixtures::{answer, nobody, watched};
    use super::*;
    use crate::lane::Gate;
    use crate::test_support::{probe_leaf, scale_task};
    use crate::{DecorationWorkerCompletion, LeafId, SeatId, ShellAddress, TabId, WindowId};

    fn address(tab: u64) -> ShellAddress {
        ShellAddress {
            window: WindowId::from(1_u64),
            leaf: LeafId {
                tab: TabId(tab),
                seat: SeatId(1),
            },
        }
    }

    /// A pasted picture whose payload is empty: the real decoder refuses it at once, touching no
    /// disk, and that refusal is the request's answer.
    fn pasted(tab: u64, occurrence_id: u64) -> MathWorkerRequest {
        crate::test_support::install_host_answers();
        MathWorkerRequest::InlineImage {
            leaf: address(tab),
            task: bt_term::InlineImageTask {
                occurrence_id,
                source: bt_term::InlineImageSource::Osc1337(Vec::new()),
            },
        }
    }

    /// The occurrence a pasted picture's answer is for.
    fn occurrence(result: &MathWorkerResult) -> Option<u64> {
        match &result.completion {
            DecorationWorkerCompletion::InlineImage { task, .. } => Some(task.occurrence_id),
            _ => None,
        }
    }

    /// A lane whose workers wait in `gate` before each request, running the product's doors.
    fn gated() -> (super::fixtures::Watched, Arc<Gate>) {
        let gate = Arc::new(Gate::default());
        (watched(Executor::scripted(Arc::clone(&gate))), gate)
    }

    /// RED (S-8; C6) — **A closed pane's answer reaches the lane's sink, never the floor and never
    /// the window.**
    ///
    /// A tab asks for a picture while the worker is busy, and closes before the answer comes; a
    /// tab that stays open asks after it. The drain lets go of the closed tab's request by its
    /// identity, and its terminal — the decoder's own answer, written in Chinese and Latin on a
    /// pasted picture's occurrence — goes to the sink, once; the open tab takes its own answer,
    /// and nothing the closed tab asked is ever taken by a window.
    ///
    /// MUTATION: make `Asker::sink` drop its terminal (an empty body) — the sink takes nothing.
    #[test]
    fn a_closed_panes_answer_reaches_the_ignoring_sink_not_the_floor() {
        let (mut lane, gate) = gated();
        gate.hold();
        let running = lane.lane.decorate(pasted(1, 1)).ok().expect("admitted");
        assert!(gate.wait_entered(1), "the first request reaches the worker");
        // The closed tab's 截图 and the open tab's mixed-script screenshot.
        let closed = lane
            .lane
            .decorate(pasted(2, 0x622a))
            .ok()
            .expect("admitted");
        let open = lane
            .lane
            .decorate(pasted(3, 0x5c4f))
            .ok()
            .expect("admitted");
        gate.release();
        let gone = |owner| owner == AnswerOwner::Tab(TabId(2));
        for id in [running, open] {
            let result = answer(lane.terminal(Share::Decoration, id, gone)).expect("its answer");
            assert_ne!(
                occurrence(&result),
                Some(0x622a),
                "a window took the closed tab's answer"
            );
        }
        let sunk = lane.sunk();
        assert_eq!(
            sunk.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [closed],
            "the closed tab's request ends in the sink, once"
        );
        assert!(
            matches!(&sunk[0].1, Terminal::Done(_)),
            "with the work's own answer"
        );
    }

    /// RED (S-8; D-75) — **One question asked twice is two requests: two identities, two runs and
    /// two terminals, each with its own answer.**
    ///
    /// A pane asks twice for one picture it printed (a name in Chinese and Latin) while the worker
    /// is busy. Each ask is minted its own identity, and each ends `Done` with an answer of its
    /// own — neither is folded into the other by its question. (A window's read of a file's pixels
    /// and its open of an animated file are the share's two exceptions, which a newer ask by the
    /// same window displaces: `MathWorkerRequest::relate`.)
    ///
    /// MUTATION: have `ComputationLane::decorate` relate every pair `Relation::Same` (an answer
    /// found by its question) — the second ends `Joined`, with no answer of its own.
    #[test]
    fn one_question_asked_twice_has_two_identities_and_two_terminals() {
        let (mut lane, gate) = gated();
        gate.hold();
        lane.lane.decorate(pasted(1, 1)).ok().expect("admitted");
        assert!(gate.wait_entered(1), "the first request reaches the worker");
        let path = PathBuf::from("不在这里/截图 screenshot 1.png");
        let ask = || MathWorkerRequest::InlineImage {
            leaf: probe_leaf(),
            task: bt_term::InlineImageTask {
                occurrence_id: 7,
                source: bt_term::InlineImageSource::LocalPath(path.clone()),
            },
        };
        let first = lane.lane.decorate(ask()).ok().expect("admitted");
        let second = lane.lane.decorate(ask()).ok().expect("admitted");
        assert_ne!(first, second, "two identities");
        gate.release();
        for id in [first, second] {
            let terminal = lane.terminal(Share::Decoration, id, nobody);
            let Terminal::Done(answered) = terminal else {
                panic!("request {id:?} ended {terminal:?}-like, not with its own answer");
            };
            let result = answered.take().expect("its own answer, not a twin's");
            assert!(matches!(
                &result.completion,
                DecorationWorkerCompletion::InlineImage { task, .. }
                    if matches!(&task.source, bt_term::InlineImageSource::LocalPath(asked) if *asked == path)
            ));
        }
    }

    /// RED (S-8; D-76) — **The formula thread's death is seen at once, while the resample and
    /// path threads live.**
    ///
    /// Each share has its own worker and its own ledger. The formula worker ends with a request
    /// owed: that request ends `WorkerGone`, its share says so once and refuses every later
    /// request at its door — while the other two threads go on answering, before and after.
    ///
    /// MUTATION: have `lane::Ran::worker_ended` report nothing (the death heard only when every
    /// sender of one shared answer channel has gone) — the owed request is never answered.
    #[test]
    fn the_decoration_threads_death_is_seen_while_the_other_two_live() {
        let (mut lane, gate) = gated();
        let resample = || ScaleWorkerRequest::Peek {
            leaf: probe_leaf(),
            surface: crate::PeekSurface::Flyout,
            task: scale_task("截图 shot.png", 8),
        };
        let printed = || PathWorkerRequest {
            leaf: probe_leaf(),
            path: PathBuf::from("不在这里/notes 笔记.md"),
        };
        for _ in 0..2 {
            let scaled = lane.lane.scale(resample()).ok().expect("admitted");
            let verified = lane.lane.verify(printed()).ok().expect("admitted");
            assert!(answer(lane.terminal(Share::Scale, scaled, nobody)).is_some());
            assert!(answer(lane.terminal(Share::Path, verified, nobody)).is_some());
            if lane.said().is_empty() {
                gate.kill();
                let owed = lane.lane.decorate(pasted(1, 1)).ok().expect("admitted");
                assert!(matches!(
                    lane.terminal(Share::Decoration, owed, nobody),
                    Terminal::WorkerGone
                ));
                assert!(
                    matches!(
                        lane.lane.decorate(pasted(1, 2)),
                        Err((Refused::Faulted(_), _))
                    ),
                    "the formula share refuses at its door once its worker has ended"
                );
            }
        }
        let said = lane.said();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with("the formula and picture lane: the worker ended"),
            "{said:?}"
        );
    }

    /// RED (S-8; D-74) — **A full share refuses at its door, at once, and hands the request
    /// back.**
    ///
    /// The path worker is held on its first question. The share admits exactly its bound — the
    /// one held and the rest waiting — and the next printed path (a name in Chinese and Latin) is
    /// refused `Full` while the worker is still held: the door did not wait for it. The refused
    /// request comes back as it was asked, to wait in its pane; every admitted one is answered
    /// once the worker is let go.
    ///
    /// MUTATION: make `ShareOf::ask` wait while its share is full (a door that blocks on a full
    /// queue) — its answer never comes, and the wait for it ends red at the suite's patience. Or
    /// build each share's ledger with no bound (`None` for `Some(bound)` in `ShareOf::new`) — the
    /// request past the bound is admitted.
    #[test]
    fn a_full_lane_refuses_at_the_door_without_waiting() {
        let (mut lane, gate) = gated();
        gate.hold();
        let printed = |at: usize| PathWorkerRequest {
            leaf: probe_leaf(),
            path: PathBuf::from(format!("不在这里/{at} 号 notes.md")),
        };
        let mut admitted = vec![lane.lane.verify(printed(0)).ok().expect("admitted")];
        assert!(
            gate.wait_entered(1),
            "the first question reaches the worker"
        );
        for at in 1..PATH_BOUND {
            admitted.push(
                lane.lane
                    .verify(printed(at))
                    .ok()
                    .expect("within the bound"),
            );
        }
        // Asked from a thread of its own, its answer awaited within the lane suite's patience: a
        // door that waited for room would never answer, and the wait would end in red, not hang.
        let (told, heard) = std::sync::mpsc::channel();
        let asking = Arc::clone(&lane.lane);
        let past = printed(PATH_BOUND);
        std::thread::spawn(move || {
            let _ = told.send(
                asking
                    .verify(past)
                    .map_err(|(refused, back)| (refused, back.path)),
            );
        });
        let Err((refused, handed_back)) = lane::receive(&heard, "the door's answer past the bound")
        else {
            panic!("a request past the bound was admitted");
        };
        assert_eq!(refused, Refused::Full { bound: PATH_BOUND });
        assert_eq!(handed_back, printed(PATH_BOUND).path);
        assert_eq!(
            gate.entered(),
            1,
            "the worker was still held when the door answered"
        );
        assert_eq!(lane.lane.held(Share::Path), PATH_BOUND);
        gate.release();
        for id in admitted {
            assert!(answer(lane.terminal(Share::Path, id, nobody)).is_some());
        }
        assert!(
            lane.lane.verify(printed(PATH_BOUND)).is_ok(),
            "the answers taken, the share admits again"
        );
    }

    /// RED (S-8) — **A formula whose render panics is still a value, through `bt_math`'s own
    /// boundary.** The conversion of `\newcommand{\a}{#}` panics inside MiTeX; the request ends
    /// `Done`, carrying the engine's `ConversionPanic`, and the share says no failure — the
    /// task's own catch is never reached. The engine goes on: the next formula is typeset.
    ///
    /// MUTATION: take `bt_math`'s boundaries away (`contained` and `convert_math` calling their
    /// closures without `catch_unwind`) — the request ends `Failed`, the task's catch, instead
    /// of the render's value.
    #[test]
    fn a_formula_render_panic_stays_a_value_through_the_bt_math_boundary() {
        let (mut lane, _gate) = gated();
        let formula = |source: &str| MathWorkerRequest::PreviewMath {
            leaf: probe_leaf(),
            key: Box::new(crate::PreviewMathKey {
                source: source.to_owned(),
                mode: crate::MathMode::Display,
                em_milli_px: 16_000,
                foreground_rgb: [220, 220, 220],
            }),
        };
        let mut render = |source: &str| {
            let id = lane.lane.decorate(formula(source)).ok().expect("admitted");
            let terminal = lane.terminal(Share::Decoration, id, nobody);
            let Terminal::Done(answered) = terminal else {
                panic!("the formula ended {terminal:?}-like, not with the render's value");
            };
            match answered.take().expect("its answer").completion {
                DecorationWorkerCompletion::PreviewMath { result, .. } => result,
                _ => panic!("a preview formula's answer"),
            }
        };
        assert_eq!(
            render(r"\newcommand{\a}{#}").unwrap_err(),
            bt_math::MathRenderError::ConversionPanic
        );
        assert!(render("x+1").is_ok(), "the engine goes on");
        assert_eq!(lane.said(), Vec::<String>::new(), "no failure was said");
    }

    /// RED (S-8; was the scale worker's drag storm) — **A drag runs the size it ends at; every
    /// size it passed through says it was superseded, and the same size asked again joins.**
    ///
    /// A divider drag asks for a preview picture at every width; the first is already running.
    /// Of the widths asked behind it — as many as the share's bound leaves room for while nothing
    /// is drained — each is displaced by the next (`Superseded { by }` the newer) and only the
    /// last is run; the same width asked again joins it (`Joined { with }`), and a different
    /// purpose of the same picture (the glance's) is a question of its own.
    ///
    /// MUTATION: relate every resample `Relation::Other` in `ComputationLane::scale` (no
    /// coalescing) — every width is run.
    #[test]
    fn a_resize_drag_runs_the_size_it_ends_at_and_the_sizes_it_passed_say_superseded() {
        let (mut lane, gate) = gated();
        // One picture, dragged through its widths.
        let picture = crate::PictureAsker::mint();
        let preview = |width| ScaleWorkerRequest::Preview {
            leaf: probe_leaf(),
            asker: picture,
            task: scale_task("截图 same-path.png", width),
        };
        gate.hold();
        let first = lane.lane.scale(preview(1)).ok().expect("admitted");
        assert!(gate.wait_entered(1), "the first width reaches the worker");
        // The one running, the widths, the same width again and the glance: the whole bound.
        let last_width = u32::try_from(SCALE_BOUND - 2).expect("a small bound");
        let dragged: Vec<ComputationId> = (2..=last_width)
            .map(|width| lane.lane.scale(preview(width)).ok().expect("admitted"))
            .collect();
        let again = lane.lane.scale(preview(last_width)).ok().expect("admitted");
        let glance = lane
            .lane
            .scale(ScaleWorkerRequest::Peek {
                leaf: probe_leaf(),
                surface: crate::PeekSurface::Card,
                task: scale_task("截图 same-path.png", 17),
            })
            .ok()
            .expect("admitted");
        gate.release();
        let width =
            |terminal: ComputationTerminal| match answer(terminal).expect("an answer").completion {
                DecorationWorkerCompletion::PreviewScaledImage { scaled, .. }
                | DecorationWorkerCompletion::PeekScaledImage { scaled, .. } => scaled.width_px,
                _ => panic!("a resample's answer"),
            };
        assert_eq!(width(lane.terminal(Share::Scale, first, nobody)), 1);
        let last = *dragged.last().expect("the widths");
        for pair in dragged.windows(2) {
            assert!(matches!(
                lane.terminal(Share::Scale, pair[0], nobody),
                Terminal::Superseded { by } if by == pair[1]
            ));
        }
        let ended = lane.terminal(Share::Scale, last, nobody);
        let joined = lane.terminal(Share::Scale, again, nobody);
        assert!(
            matches!(&joined, Terminal::Joined { with, .. } if *with == last),
            "the same width asked again joins the one waiting"
        );
        assert_eq!(width(ended), last_width);
        assert_eq!(width(lane.terminal(Share::Scale, glance, nobody)), 17);
        assert_eq!(
            gate.entered(),
            3,
            "three resamples were run: the first width, the last and the glance's"
        );
    }

    /// A formula door — the product's — whose first terminal formula's work panics.
    fn formula_fails_once(ctx: &WorkerCtx) -> Door<MathWorkerRequest> {
        let mut real = crate::run_decoration_worker(ctx);
        let mut failed = false;
        Box::new(move |ctx: &WorkerCtx, request: MathWorkerRequest| {
            if !failed && matches!(request, MathWorkerRequest::Math { .. }) {
                failed = true;
                panic!("the suite fails this formula's work once, on purpose");
            }
            real(ctx, request)
        })
    }

    /// A path door — the product's — whose first question about a `笔记-notes.md` panics.
    fn path_fails_once(ctx: &WorkerCtx) -> Door<PathWorkerRequest> {
        let mut real = crate::path_door(ctx);
        let mut failed = false;
        Box::new(move |ctx: &WorkerCtx, request: PathWorkerRequest| {
            if !failed && request.path.ends_with("笔记-notes.md") {
                failed = true;
                panic!("the suite fails this path's work once, on purpose");
            }
            real(ctx, request)
        })
    }

    /// A pane showing one live formula, settled and ready to be handed out.
    fn a_pane_with_a_formula() -> bt_term::DualPlaneSession {
        let start = std::time::Instant::now();
        let mut session = bt_term::DualPlaneSession::new(
            std::num::NonZeroU32::new(40).unwrap(),
            std::num::NonZeroU32::new(4).unwrap(),
        );
        session
            .feed_at("$$x+\u{03b1}$$\x1b[?25l".as_bytes(), start)
            .unwrap();
        session.advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL);
        session
    }

    /// A pane that printed one path (a name in Chinese and Latin), its question queued.
    fn a_pane_with_a_printed_path(path: &std::path::Path) -> bt_term::DualPlaneSession {
        let mut session = bt_term::DualPlaneSession::new(
            std::num::NonZeroU32::new(120).unwrap(),
            std::num::NonZeroU32::new(6).unwrap(),
        );
        session.set_math_layout_options(bt_term::MathLayoutOptions {
            detect_image_paths: true,
            ..bt_term::MathLayoutOptions::default()
        });
        session
            .feed(format!("{}\r\n", path.to_string_lossy()).as_bytes())
            .unwrap();
        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        session.absorb_printed_path_probes(&mut projection);
        session
    }

    /// One hand-out of `session`'s work to `lane`, as a frame makes it.
    fn hand_out(session: &mut bt_term::DualPlaneSession, lane: &ComputationLane) -> bool {
        let (mut running, mut notice) = (true, false);
        crate::dispatch_pending_math_tasks(probe_leaf(), session, lane, &mut running, &mut notice)
    }

    /// The pane tasks a drain's failures hold, as the window would hold them in `session`.
    fn hold_in(session: &mut bt_term::DualPlaneSession, answers: crate::MathAnswers) -> usize {
        let mut held = 0;
        for (_, settled) in answers.failed {
            let crate::Settled::Hold { task, .. } = settled else {
                panic!("a pane's failed task is held");
            };
            session.hold_decoration_task_until_new_input(task);
            held += 1;
        }
        held
    }

    /// RED (S-8 rounds 2 and 3; revision (b)5 rule 1) — **A formula whose work failed is asked
    /// again by new input that could change its answer, and answered; never before.**
    ///
    /// The formula's work panics once. Its `Failed` terminal is sorted into a pane task to hold
    /// (`sort_math_terminals` → `settle_failure` → `Settled::Hold`), and the pane holds it: frames
    /// drawn again with nothing new hand nothing out, and a resize hands it out once more, to an
    /// answer.
    ///
    /// MUTATION: make `settle_failure` answer `Settled::Nothing` for a pane's task — no task is
    /// held, and the mark stands for ever.
    #[test]
    fn a_failed_formula_is_asked_again_by_new_input_and_answered() {
        let (mut lane, _gate) = gated();
        // The formula worker starts with the door that fails a formula once.
        let started = lane
            .lane
            .decorate_through(
                MathWorkerRequest::PeekImage {
                    leaf: probe_leaf(),
                    path: PathBuf::from("不在这里/启动 start.png"),
                },
                formula_fails_once,
            )
            .ok()
            .expect("admitted");
        lane.terminal(Share::Decoration, started, nobody);
        let mut session = a_pane_with_a_formula();
        assert!(!hand_out(&mut session, &lane.lane));
        let ended = lane.next_ended(Share::Decoration, nobody);
        assert!(matches!(ended.2, Terminal::Failed(_)), "{:?}", ended.2);
        assert_eq!(
            hold_in(&mut session, crate::sort_math_terminals(vec![ended])),
            1
        );
        for _ in 0..3 {
            hand_out(&mut session, &lane.lane);
            assert_eq!(
                lane.lane.held(Share::Decoration),
                0,
                "no new input, no new question"
            );
        }
        session
            .resize(
                std::num::NonZeroU32::new(44).unwrap(),
                std::num::NonZeroU32::new(4).unwrap(),
            )
            .unwrap();
        hand_out(&mut session, &lane.lane);
        let (_, _, answered, _) = lane.next_ended(Share::Decoration, nobody);
        let completion = answer(answered)
            .expect("the formula is answered")
            .completion;
        assert!(
            matches!(
                completion,
                DecorationWorkerCompletion::Math { result: Ok(_), .. }
            ),
            "the formula asked again is typeset"
        );
    }

    /// RED (S-8 rounds 2 and 3; revision (b)5 rule 1) — **A printed path whose check failed is
    /// asked again by a resize, and answered; not by more output that leaves it as it was.**
    ///
    /// MUTATION: as above — `Settled::Nothing` for a pane's task.
    #[test]
    fn a_failed_path_is_asked_again_by_new_input_and_answered() {
        let (mut lane, _gate) = gated();
        let started = lane
            .lane
            .verify_through(
                PathWorkerRequest {
                    leaf: probe_leaf(),
                    path: PathBuf::from("不在这里/启动.md"),
                },
                path_fails_once,
            )
            .ok()
            .expect("admitted");
        assert!(answer(lane.terminal(Share::Path, started, nobody)).is_some());
        let path = bt_testpath::temp_path("bt-app-path-failed").join("笔记-notes.md");
        let mut session = a_pane_with_a_printed_path(&path);
        assert!(!hand_out(&mut session, &lane.lane));
        let ended = lane.next_ended(Share::Path, nobody);
        assert!(matches!(ended.2, Terminal::Failed(_)), "{:?}", ended.2);
        assert_eq!(
            hold_in(&mut session, crate::sort_math_terminals(vec![ended])),
            1
        );
        session
            .feed("\u{65e5}\u{5fd7} more output\r\n".as_bytes())
            .unwrap();
        for _ in 0..3 {
            hand_out(&mut session, &lane.lane);
            assert_eq!(
                lane.lane.held(Share::Path),
                0,
                "output that leaves the path as it was asks nothing"
            );
        }
        session
            .resize(
                std::num::NonZeroU32::new(100).unwrap(),
                std::num::NonZeroU32::new(6).unwrap(),
            )
            .unwrap();
        hand_out(&mut session, &lane.lane);
        let (_, _, answered, _) = lane.next_ended(Share::Path, nobody);
        assert!(
            matches!(
                answer(answered).expect("answered").completion,
                DecorationWorkerCompletion::VerifiedPath { path: asked, .. } if asked == path
            ),
            "the path asked again is checked"
        );
    }

    /// RED (S-8 round 3) — **A glance whose decode failed is answered as a failed decode**, down
    /// the road a decode error already takes (`complete_peek_image` files `PeekCacheEntry::Failed`
    /// and refuses the picture to waiting preview panes), so every surface agrees and the glance
    /// asks again only at its next settled hover — never on a redraw.
    ///
    /// MUTATION: make `settle_failure` answer `Settled::Nothing` for a file's pixels — the
    /// glance's `Pending` stands for ever.
    #[test]
    fn a_failed_glance_is_answered_as_a_failed_decode() {
        let (mut lane, _gate) = gated();
        let path = PathBuf::from("不在这里/截图 glance.png");
        let id = lane
            .lane
            .decorate_through(
                MathWorkerRequest::PeekImage {
                    leaf: probe_leaf(),
                    path: path.clone(),
                },
                |ctx: &WorkerCtx| -> Door<MathWorkerRequest> {
                    let _ = ctx;
                    Box::new(|_: &WorkerCtx, _: MathWorkerRequest| -> MathWorkerResult {
                        panic!("the suite fails this glance's decode on purpose")
                    })
                },
            )
            .ok()
            .expect("admitted");
        let ended = lane.next_ended(Share::Decoration, nobody);
        assert_eq!(ended.1, id);
        let answers = crate::sort_math_terminals(vec![ended]);
        assert!(
            answers.failed.is_empty(),
            "a window surface settles by its answer"
        );
        assert!(
            matches!(
                answers.done.as_slice(),
                [(answering, MathWorkerResult {
                    completion: DecorationWorkerCompletion::PeekImage { path: answered, result: Err(_) },
                    ..
                })] if *answered == path && *answering == id
            ),
            "the glance hears a failed decode for its file"
        );
    }

    /// RED (S-8 round 3) — **A failure reaches the pane that asked**, and a request joined to a
    /// failed one is failed too: `hold_failed_in_tabs` finds the asking seat of the asking tab
    /// (another tab's pane, or a pane closed since, takes nothing), and `sort_math_terminals` sorts
    /// a `Joined` carrying a `Failed` with the failures.
    ///
    /// MUTATION: make `sort_math_terminals` treat every `Joined` as answered (`false`) — the
    /// joined failure is lost.
    #[test]
    fn a_failure_reaches_the_pane_that_asked_and_a_joined_failure_is_a_failure() {
        let tab = crate::test_support::cross_tab(1, &["一", "two"]);
        let seats = tab.seats.terminals();
        let leaf = |seat: bt_layout::SeatId, tab_id| ShellAddress {
            window: WindowId::from(1_u64),
            leaf: LeafId { tab: tab_id, seat },
        };
        let task = bt_term::SessionDecorationTask::VerifyPath(PathBuf::from("不在这里/路径.md"));
        let tab_id = tab.id;
        let mut tabs = vec![tab];
        assert!(crate::hold_failed_in_tabs(
            &mut tabs,
            leaf(seats[1], tab_id),
            task.clone()
        ));
        // A resize gives every held task back: only the seat that asked hands one out.
        let handed: Vec<bool> = tabs[0]
            .leaves_mut()
            .map(|(_, pane)| {
                let (columns, rows) = pane.session.terminal().dimensions();
                pane.session
                    .resize(columns, rows.saturating_add(1))
                    .unwrap();
                pane.session.take_decoration_worker_task().is_some()
            })
            .collect();
        assert_eq!(
            handed,
            [false, true],
            "held by the seat that asked, and no other"
        );
        assert!(
            !crate::hold_failed_in_tabs(&mut tabs, leaf(seats[0], TabId(999)), task.clone()),
            "a tab no window holds takes nothing"
        );

        let joined = Terminal::Joined {
            with: ComputationId(1),
            outcome: Box::new(Terminal::Failed(lane::TaskFailure::Panicked(
                "planted".into(),
            ))),
        };
        let answers = crate::sort_math_terminals(vec![(
            Share::Path,
            ComputationId(2),
            joined,
            (
                crate::AnswerOwner::Tab(tab_id),
                PathWorkerRequest {
                    leaf: leaf(seats[1], tab_id),
                    path: PathBuf::from("不在这里/路径.md"),
                }
                .asked(),
            ),
        )]);
        assert!(
            matches!(
                answers.failed.as_slice(),
                [(_, crate::Settled::Hold { .. })]
            ),
            "a request joined to a failure is held as a failure"
        );
    }

    /// RED (S-8 round 2; revision (b)5 rule 1) — **A worker the door could not start is tried
    /// again by the pane's next new input, once — not by every frame.**
    ///
    /// The thread door refuses the formula worker's first start. The pane's task is handed back
    /// and its hand-out paused: frames drawn again with nothing new start nothing; the next
    /// bytes from the shell start the worker, and the task is admitted.
    ///
    /// MUTATION: drop `session.pause_hand_out_until_new_input()` from the `NotStarted` arm of
    /// `dispatch_pending_math_tasks` — the next idle frame starts the worker.
    #[test]
    fn a_refused_start_is_tried_again_only_by_new_input() {
        let (lane, gate) = gated();
        gate.hold();
        gate.refuse_next_start();
        let mut session = a_pane_with_a_formula();
        assert!(
            !hand_out(&mut session, &lane.lane),
            "a refused start is no fault"
        );
        assert_eq!(lane.lane.held(Share::Decoration), 0);
        for _ in 0..3 {
            hand_out(&mut session, &lane.lane);
            assert_eq!(
                lane.lane.held(Share::Decoration),
                0,
                "no new input, no new start"
            );
        }
        session.feed(b" ").unwrap();
        hand_out(&mut session, &lane.lane);
        assert_eq!(
            lane.lane.held(Share::Decoration),
            1,
            "new input starts the worker once"
        );
        gate.release();
    }

    /// RED (S-8 round 2) — **A full share stops only its own work for a hand-out**: with the
    /// formula share full, a pane's formula goes back to the pane while its printed path is still
    /// handed to the path share — the three bounds exist so the shares do not block each other.
    ///
    /// MUTATION: make `dispatch_pending_math_tasks` stop the whole hand-out at the first `Full`
    /// (the round-1 `break`) — the path waits behind the formula.
    #[test]
    fn a_full_formula_share_does_not_hold_back_a_panes_paths() {
        let (lane, gate) = gated();
        gate.hold();
        for at in 0..DECORATION_BOUND {
            lane.lane
                .decorate(MathWorkerRequest::PeekImage {
                    leaf: probe_leaf(),
                    path: PathBuf::from(format!("不在这里/{at} 图.png")),
                })
                .ok()
                .expect("within the bound");
        }
        // A pane with a formula on screen, then the path the program printed after it.
        let mut session = a_pane_with_a_formula();
        let path = bt_testpath::temp_path("bt-app-shares-apart").join("笔记-apart.md");
        session.set_math_layout_options(bt_term::MathLayoutOptions {
            detect_image_paths: true,
            ..bt_term::MathLayoutOptions::default()
        });
        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        session
            .feed(format!("\r\n{}\r\n", path.to_string_lossy()).as_bytes())
            .unwrap();
        session.refresh_projection(&mut projection);
        session.viewport_frame(&mut projection).unwrap();
        session.absorb_printed_path_probes(&mut projection);
        let outstanding = session.outstanding_decoration_work();
        assert_eq!(outstanding, 2, "a formula and a path to hand out");
        assert!(!hand_out(&mut session, &lane.lane));
        assert_eq!(
            lane.lane.held(Share::Path),
            1,
            "the path went to its own share"
        );
        assert_eq!(
            lane.lane.held(Share::Decoration),
            DECORATION_BOUND,
            "the formula share stayed full"
        );
        assert_eq!(
            session.outstanding_decoration_work(),
            outstanding,
            "the formula waits in the pane, not lost"
        );
        gate.release();
    }

    /// RED (S-8 round 2) — **A superseded resample is announced and leaves the share's bound at
    /// once**, while the resample before it is still running: the drain takes its terminal at the
    /// wake, past the drain mark, because it carries no answer whose order could depend on it.
    ///
    /// MUTATION: take only the terminals below the drain mark in `ShareOf::ended` (drop the
    /// `Superseded` clause) — the superseded request stays held until the running one ends.
    #[test]
    fn a_superseded_resample_leaves_the_bound_while_the_run_before_it_is_out() {
        let (mut lane, gate) = gated();
        gate.hold();
        let picture = crate::PictureAsker::mint();
        let preview = |width| ScaleWorkerRequest::Preview {
            leaf: probe_leaf(),
            asker: picture,
            task: scale_task("截图 drag.png", width),
        };
        let running = lane.lane.scale(preview(1)).ok().expect("admitted");
        assert!(gate.wait_entered(1), "the first width reaches the worker");
        let older = lane.lane.scale(preview(2)).ok().expect("admitted");
        let newer = lane.lane.scale(preview(3)).ok().expect("admitted");
        lane::wait_for_a_wake(&lane.wakes, "the supersession's wake");
        let ended = lane.lane.ended(nobody);
        assert!(
            ended
                .iter()
                .any(|(share, id, terminal, _)| *share == Share::Scale
                    && *id == older
                    && matches!(terminal, Terminal::Superseded { by } if *by == newer)),
            "the superseded width is taken at once"
        );
        assert_eq!(
            lane.lane.held(Share::Scale),
            2,
            "the running one and the newest"
        );
        gate.release();
        assert!(answer(lane.terminal(Share::Scale, running, nobody)).is_some());
        assert!(answer(lane.terminal(Share::Scale, newer, nobody)).is_some());
    }

    /// RED (S-8 round 4) — **An animation open whose work failed is unanswered, not refused**: a
    /// refusal is a fact about the file and is kept, while an open that never answered is let go
    /// by the file's next open, which asks again.
    ///
    /// MUTATION: settle a failed open as the decoder's refusal (`PeekAnimation { Err(Undecodable)
    /// }`, round 3) — it is kept for ever.
    #[test]
    fn a_failed_animation_open_is_unanswered_not_refused() {
        let asked = MathWorkerRequest::PeekAnimation {
            leaf: probe_leaf(),
            path: PathBuf::from("不在这里/动图 loading.gif"),
        }
        .asked();
        assert!(
            matches!(
                crate::settle_failure(ComputationId(7), asked),
                crate::Settled::AnimationUnanswered { path, asked: ComputationId(7) }
                    if path.ends_with("动图 loading.gif")
            ),
            "the open is unanswered until the file's next open"
        );
    }

    /// A scratch copy of the shipped asset `asset` under the build directory, named after `tag` —
    /// where a peek reads unasked, as a page beside it would name it.
    fn scratch_copy(asset: &str, tag: &str) -> crate::test_support::ScratchFile {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/assets")
            .join(asset);
        let extension = source
            .extension()
            .expect("an asset has an extension")
            .to_owned();
        let copy = crate::test_support::scratch_png_named(tag).with_extension(extension);
        std::fs::copy(&source, &copy).expect("a scratch copy of a shipped asset");
        crate::test_support::ScratchFile(copy)
    }

    /// A scratch file of `bytes` under the build directory, named after `tag` with `extension`.
    fn scratch_bytes(tag: &str, extension: &str, bytes: &[u8]) -> crate::test_support::ScratchFile {
        let file = crate::test_support::scratch_png_named(tag).with_extension(extension);
        std::fs::write(&file, bytes).expect("a scratch file this test owns");
        crate::test_support::ScratchFile(file)
    }

    /// RED (T-ANSWER-AFTER-THE-FILE-IS-CLOSED) — **every read the decoration worker answers has
    /// let go of its file by the time the window can take the answer**: a picture's pixels (a PNG,
    /// and a GIF's first frame), a recording's frame and facts (a real one, and one that is no
    /// video, which Media Foundation refuses), a page's raster and a printed picture's decode.
    /// Each is asked through the product's door on the real lane; the moment its terminal is
    /// taken, nobody holds the file (`bt_testpath::nobody_holds`) and a writer saving over it
    /// finishes — what an editor does the moment the card has shown the answer
    /// (`runtime::preview::tests::a_set_aside_peek_answer_changes_nothing_on_the_window`).
    ///
    /// MUTATION: `std::mem::forget` the `File` in `bt_term`'s `read_and_decode_local_image`
    /// (the handle outlives the decode) — "a picture's pixels: the answer is taken, and nobody
    /// holds the file" fails. The recording's arm rests on
    /// `bt_platform::video::tests::a_frame_given_up_on_leaves_its_file_free`.
    #[test]
    fn an_answered_read_has_let_go_of_its_file() {
        crate::test_support::install_host_answers();
        let png = scratch_bytes(
            "截图 answered",
            "png",
            &crate::test_support::png_bytes(6, 3),
        );
        let gif = scratch_copy("folio-anim-test.gif", "动图 answered");
        let mp4 = scratch_copy("folio-video-test.mp4", "录屏 answered");
        let junk = scratch_bytes("录屏 not a video", "mp4", &[0x5a; 64]);
        let pdf = scratch_copy("folio-pdf-test.pdf", "文档 answered");
        let printed = scratch_bytes("截图 printed", "png", &crate::test_support::png_bytes(4, 4));
        let leaf = probe_leaf();
        let asks = [
            (
                "a picture's pixels",
                &png.0,
                MathWorkerRequest::PeekImage {
                    leaf,
                    path: png.0.clone(),
                },
            ),
            (
                "a GIF's first frame",
                &gif.0,
                MathWorkerRequest::PeekImage {
                    leaf,
                    path: gif.0.clone(),
                },
            ),
            (
                "a recording's frame",
                &mp4.0,
                MathWorkerRequest::PeekVideoFrame {
                    leaf,
                    path: mp4.0.clone(),
                },
            ),
            (
                "a file that is no video",
                &junk.0,
                MathWorkerRequest::PeekVideoFrame {
                    leaf,
                    path: junk.0.clone(),
                },
            ),
            (
                "a page's raster",
                &pdf.0,
                MathWorkerRequest::PeekPage {
                    leaf,
                    path: pdf.0.clone(),
                    page: 0,
                    fit: (160, 160),
                    known: None,
                },
            ),
            (
                "a printed picture",
                &printed.0,
                MathWorkerRequest::InlineImage {
                    leaf,
                    task: bt_term::InlineImageTask {
                        occurrence_id: 1,
                        source: bt_term::InlineImageSource::LocalPath(printed.0.clone()),
                    },
                },
            ),
        ];
        let (mut lane, _gate) = gated();
        for (what, file, request) in asks {
            let id = lane.lane.decorate(request).ok().expect("admitted");
            let ended = lane.next_ended(Share::Decoration, nobody);
            assert_eq!(ended.1, id, "{what}: its own answer");
            assert!(
                bt_testpath::nobody_holds(file),
                "{what}: the answer is taken, and nobody holds the file"
            );
            let written = std::fs::write(file, b"saved over");
            assert!(written.is_ok(), "{what}: the writer finishes: {written:?}");
            assert!(
                answer(ended.2).is_some(),
                "{what}: the terminal carried an answer"
            );
        }
    }

    /// PIN (T-ANSWER-AFTER-THE-FILE-IS-CLOSED) — **an animation's answer is the one that keeps
    /// its file, because the file is the answer**: the open hands the window a cursor that streams
    /// the frames still to come from the open file (`animation::FileAnimationSource`), so the
    /// handle lives exactly as long as the window keeps the animation — and it is opened as every
    /// read in this process is, sharing reading, writing and deletion, so a writer saving over the
    /// file is never refused by it. Let the answer go and nobody holds the file. (That the cursor
    /// holds the file while it is kept is not asserted: on Windows an exclusive open would show it,
    /// but a Unix descriptor takes no lock, so `bt_testpath::nobody_holds` cannot see it there.)
    ///
    /// MUTATION: open the animation's file sharing nothing (or sharing reading only) — the writer
    /// is refused while the window plays it.
    #[test]
    fn an_animations_answer_keeps_its_file_and_refuses_no_writer() {
        let gif = scratch_copy("folio-anim-test.gif", "动图 playing");
        let (mut lane, _gate) = gated();
        let id = lane
            .lane
            .decorate(MathWorkerRequest::PeekAnimation {
                leaf: probe_leaf(),
                path: gif.0.clone(),
            })
            .ok()
            .expect("admitted");
        let ended = lane.next_ended(Share::Decoration, nobody);
        assert_eq!(ended.1, id);
        let opened = answer(ended.2).expect("the open answers");
        let DecorationWorkerCompletion::PeekAnimation { frames: Ok(_), .. } = &opened.completion
        else {
            panic!("the shipped GIF opens as an animation");
        };
        let original = std::fs::read(&gif.0).expect("the file reads");
        let written = std::fs::write(&gif.0, &original);
        assert!(
            written.is_ok(),
            "and a writer saving over it is not refused: {written:?}"
        );
        drop(opened);
        assert!(
            bt_testpath::nobody_holds(&gif.0),
            "the answer let go, nobody holds the file"
        );
    }
}
