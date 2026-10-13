//! **The git mutation lane** — a Git page's writes and checkouts (`GitQuestion::Write`,
//! `GitQuestion::Checkout`), each run on a worker of its repository's own, one after another, and
//! never on the read worker or the window thread (`docs/ARCHITECTURE.md` §5.1, *storage and
//! integration transactions*: serialised by the resource they change; `docs/DESIGN.md`, *a Git
//! page's verbs run one after another, per repository*).
//!
//! **Not the storage worker.** The storage worker's one request protocol (B6; the side-effects
//! contract's revision (c)3) is a conditional exchange of bytes Folio owns, against the exact bytes
//! it last read. A git verb is a command handed to a program that owns its own files — the index,
//! the refs, the working tree — and its own locks; Folio holds no revision of them, and git's own
//! refusal is the outcome. So the verbs keep the storage rule's other half — one at a time per
//! resource, an identity per operation, its durable outcome kept — on a lane of their own.
//!
//! **Built on the completion seam** (`crate::lane`; C1–C7, revision (b)7). One share per
//! repository, each a [`crate::lane::Queue`] with its own [`crate::lane::Ledger`] and its own
//! `bt-git-mutation` worker started by a [`crate::lane::Executor`] on the share's first request —
//! so two repositories' verbs never wait for each other, and one share's death ends only its own
//! requests.
//!
//! # The contract
//!
//! * **Identity.** Every admitted request gets a [`MutationId`]: the share's generation (a serial
//!   the lane counts) and the number the share's ledger minted. The lane keeps what each one asked
//!   — its window, its page and its question, the repository, the verb and the paths — until the
//!   window thread takes its terminal ([`GitMutationLane::ended`]).
//! * **Ordering.** First in, first out per repository: a request runs, and ends, after every request
//!   of its repository minted before it. **Nothing is coalesced**: two presses are two requests,
//!   two runs and two terminals, and a checkout after a write waits for the write.
//! * **Reads wait their turn** ([`RepoTurns`]). A question on the read worker about a repository
//!   runs only while that repository's share owes nothing — a request is owed from its admission,
//!   so a read asked after a press never runs in the middle of the verb — and a verb starts only
//!   while no read of its repository is running. Both waits are on workers; each is as long as the
//!   other side's one git child, which [`crate::git::GIT_COMMAND_TIMEOUT`] bounds.
//! * **Capacity.** A share owes at most [`MUTATION_BOUND`] requests, a terminal not yet taken
//!   holding its place: what one Git page can hand it at once. A request past it — or asked of a
//!   share whose worker has ended — is refused at the door, synchronously, with no identity minted
//!   (revision (b)3), and handed back; the page files it at once as a receipt that says so
//!   ([`refusal`]), so its rows undim. Nothing is retried on a clock (revision (b)5).
//! * **Completion.** `Done` carries git's receipt (`GitAnswer::Write` / `::Checkout`): a verb git
//!   refused is that receipt's own error, in git's words. A panic in one request's work is that
//!   request's `Failed`, caught by its [`crate::lane::task`] and said once by the share's ledger;
//!   the worker goes on. A worker's end ends every request of that share still owed `WorkerGone`,
//!   faults the share and wakes the loop. Each of those, and a refusal at the door, reaches the page
//!   as the receipt of its question with [`GitFault::NotRun`] ([`receipt`]): the rows undim and
//!   the notice says Folio did not run it.
//! * **Abandonment.** A request whose page has gone — its tab closed, its floating window
//!   dismissed — is let go by the drain before it takes anything; its terminal, now or when it
//!   comes, goes to the lane's sink, which ignores it (C6): the verb has run, nobody is left to
//!   undim, and a failure in it was said by the ledger. A column closed inside a tab that stays open
//!   is the tab's: its receipt reaches the tab, which finds no column to file it in.
//! * **Lifetime.** A share is made by the first request for its repository and let go by the drain
//!   once the window thread holds none of its requests: its worker runs what is in line — nothing —
//!   and returns. The next press on that repository makes a new share, of a new generation.
//! * **Wake.** Each worker publishes the terminal first and then wakes the loop
//!   (`AppEvent::GitReady`, the read worker's own wake); each ledger wakes it the same way when its
//!   worker ends. Every wake also tells the read worker's turn ([`RepoTurns`]) that what a
//!   repository owes may have changed.

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use bt_platform::admission::WorkerCtx;

use crate::AnswerOwner;
use crate::git::{self, GitAnswer, GitFault, GitQuestion, GitRequest, GitResponse, NotRun};
use crate::lane::{self, Executor, Ledger, Owed, Queue, Refused, Relation, Saying, Terminal};

/// **How many requests one repository's share owes at once** — what one Git page can hand it: a
/// write for each status row the page can show ([`git::GIT_STATUS_CAP`]; `GitCache::pending_writes`
/// holds each row's path to one write in flight), and the page's one checkout (`GitCache::checkout`
/// holds it to one). A ref verb (each ref held to one in flight by `GitCache::pending_refs`), the
/// repository's graph or a second page on it share the bound and reach it sooner.
pub(crate) const MUTATION_BOUND: usize = git::GIT_STATUS_CAP + 1;

/// The name a share's ledger says its lines under.
const LANE: &str = "the git mutation lane";

/// **One request's number within its share**, minted by the share's ledger.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct Minted(u64);

impl From<u64> for Minted {
    fn from(minted: u64) -> Self {
        Self(minted)
    }
}

/// **One request's name in the lane**: its share's generation and the number the share minted —
/// unique for the life of the lane, across repositories and across a repository's shares.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct MutationId {
    share: u64,
    minted: Minted,
}

impl MutationId {
    /// The identity as one number, for the lane suite's tally: the generation above the minted
    /// number.
    #[cfg(test)]
    pub(crate) fn number(self) -> u64 {
        (self.share << 32) | self.minted.0
    }
}

/// How one request ended.
pub(crate) type MutationTerminal = Terminal<Minted, GitAnswer, Infallible>;

/// The lane's sink for the abandoned, shared by every request it admits.
type Sink = Arc<dyn Fn(MutationId, MutationTerminal) + Send + Sync>;

/// **What one request is owed** (C6): the lane's sink, which its terminal goes to when its page has
/// gone — named by the share it was minted in.
pub(crate) struct Asker {
    share: u64,
    sink: Sink,
}

impl Owed<Minted> for Asker {
    type Answer = GitAnswer;
    type Error = Infallible;

    fn sink(self, minted: Minted, terminal: MutationTerminal) {
        (self.sink)(
            MutationId {
                share: self.share,
                minted,
            },
            terminal,
        );
    }
}

type MutationLedger = Ledger<Minted, Asker>;

/// **One worker's door**: what the worker runs for one question, made on the worker's own thread
/// from the capability the thread door lent it. The product's is [`git::ask_git`].
pub(crate) type Door = Box<dyn FnMut(&WorkerCtx, &GitQuestion) -> GitAnswer>;

/// **One request that has ended, taken by the window thread**: its identity, its terminal, and what
/// it asked.
pub(crate) type Ended = (MutationId, MutationTerminal, GitRequest);

/// Whether two folders are one: component by component, in the platform's case rule
/// ([`git::stands_inside`] both ways).
fn same_folder(left: &Path, right: &Path) -> bool {
    git::stands_inside(left, right) && git::stands_inside(right, left)
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ------------------------------------------------------------------------------------------------
// The turn a repository's readers and writers take
// ------------------------------------------------------------------------------------------------

/// **Whose turn a repository is**: the read worker's, or its mutation share's — never both. Shared
/// by the read worker (`git::GitWorker`) and every share of the lane; waited on only on those
/// workers.
///
/// What a share owes is its ledger's to say ([`Ledger::earliest_owed`]): the turnstile holds each
/// repository's ledger, never a count of its own. A request is owed from its admission until its
/// terminal is published, and its worker's end ends every one it still owes, so a read never waits
/// on a share that cannot answer.
pub(crate) struct RepoTurns {
    state: Mutex<TurnState>,
    changed: Condvar,
}

#[derive(Default)]
struct TurnState {
    /// Each repository's share's ledger, while it has a share.
    shares: Vec<(PathBuf, Arc<MutationLedger>)>,
    /// The folders a read is running in.
    reading: Vec<PathBuf>,
    /// How many reads wait their turn now — where a test sees a read wait.
    #[cfg(test)]
    reads_waiting: usize,
}

impl TurnState {
    fn owes(&self, folder: &Path) -> bool {
        self.shares
            .iter()
            .any(|(root, ledger)| same_folder(root, folder) && ledger.earliest_owed().is_some())
    }

    fn reading(&self, folder: &Path) -> bool {
        self.reading.iter().any(|at| same_folder(at, folder))
    }
}

/// A read's mark in the turnstile, taken off when the read is over — its answer sent, or its work
/// unwound.
struct Reading<'a> {
    turns: &'a RepoTurns,
    folder: &'a Path,
}

impl Drop for Reading<'_> {
    fn drop(&mut self) {
        {
            let mut state = locked(&self.turns.state);
            if let Some(at) = state
                .reading
                .iter()
                .position(|at| same_folder(at, self.folder))
            {
                state.reading.remove(at);
            }
        }
        self.turns.changed.notify_all();
    }
}

impl RepoTurns {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(TurnState::default()),
            changed: Condvar::new(),
        })
    }

    /// **The read worker's turn on `folder`** (`worker`, the capability the thread door lent it):
    /// waits while the folder's share owes a mutation, then runs `read` — the question and the
    /// sending of its answer — with the folder marked as being read, so no mutation of it starts
    /// until `read` is over.
    pub(crate) fn read<T>(&self, worker: &WorkerCtx, folder: &Path, read: impl FnOnce() -> T) -> T {
        let _ = worker;
        {
            let mut state = locked(&self.state);
            #[cfg(test)]
            {
                state.reads_waiting += 1;
            }
            while state.owes(folder) {
                state = self
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            #[cfg(test)]
            {
                state.reads_waiting -= 1;
            }
            state.reading.push(folder.to_path_buf());
        }
        let _reading = Reading {
            turns: self,
            folder,
        };
        read()
    }

    /// **A share worker's turn on `folder`**: waits while a read of the folder is running, then
    /// runs `mutate`. No read of the folder starts meanwhile: the request running is owed until its
    /// terminal is published.
    fn mutate<T>(&self, worker: &WorkerCtx, folder: &Path, mutate: impl FnOnce() -> T) -> T {
        let _ = worker;
        {
            let mut state = locked(&self.state);
            while state.reading(folder) {
                state = self
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
        mutate()
    }

    /// `root`'s share is `ledger` from now on.
    fn hold(&self, root: &Path, ledger: Arc<MutationLedger>) {
        locked(&self.state).shares.push((root.to_path_buf(), ledger));
        self.changed.notify_all();
    }

    /// `root` has no share any more.
    fn forget(&self, root: &Path) {
        locked(&self.state)
            .shares
            .retain(|(held, _)| !same_folder(held, root));
        self.changed.notify_all();
    }

    /// What a repository owes may have changed: every turn waiting looks again. The lock is taken
    /// first, so a worker between its look and its wait cannot miss this.
    fn changed(&self) {
        drop(locked(&self.state));
        self.changed.notify_all();
    }

    /// How many reads wait their turn now.
    #[cfg(test)]
    fn reads_waiting(&self) -> usize {
        locked(&self.state).reads_waiting
    }
}

// ------------------------------------------------------------------------------------------------
// The lane
// ------------------------------------------------------------------------------------------------

/// **One repository's share**: its queue (and with it its ledger), and the requests it holds for
/// their pages, by number.
struct Share {
    root: PathBuf,
    generation: u64,
    queue: Arc<Queue<Minted, Asker, GitQuestion>>,
    held: std::collections::BTreeMap<Minted, (lane::Request<Minted, Asker>, GitRequest)>,
}

impl Share {
    /// Let go of every request whose page has `gone`: each one's terminal goes to the sink.
    fn let_go(&mut self, gone: &impl Fn(AnswerOwner) -> bool) {
        let numbers: Vec<Minted> = self
            .held
            .iter()
            .filter(|(_, (_, request))| gone(request.owner()))
            .map(|(minted, _)| *minted)
            .collect();
        // Each request dropped hands its terminal to the sink, now or when it comes.
        for minted in numbers {
            drop(self.held.remove(&minted));
        }
    }

    /// **Every terminal below the share's drain mark** ([`Ledger::earliest_owed`], read once,
    /// before the requests are looked at), in the order the ledger minted them, each taken once.
    fn ended(&mut self, into: &mut Vec<Ended>) {
        let owed = self.queue.ledger().earliest_owed();
        let taken: Vec<(Minted, MutationTerminal)> = self
            .held
            .iter_mut()
            .filter_map(|(minted, (request, _))| {
                let below = owed.is_none_or(|owed| *minted < owed);
                Some((*minted, request.take_if(|_| below)?))
            })
            .collect();
        for (minted, terminal) in taken {
            if let Some((_, request)) = self.held.remove(&minted) {
                let id = MutationId {
                    share: self.generation,
                    minted,
                };
                into.push((id, terminal, request));
            }
        }
    }
}

/// **How a share's worker reports its end** to the share's ledger ([`lane::Ran`]): a worker that
/// panicked, or was never started, is reported as the seam reports it — every request it owed
/// ends `WorkerGone` and the loop is woken. A body that returned did so because the drain let its
/// share go ([`GitMutationLane::ended`], [`Queue::close`]) with nothing held and nothing owed: no
/// request ends with it and nobody is left to tell, so it is not reported, and its share — gone
/// from the lane already — wakes nobody.
struct Ending(Arc<lane::Ran<Minted, Asker>>);

impl lane::Ends for Ending {
    fn worker_ended(&self, cause: lane::WorkerEnd) {
        if cause != lane::WorkerEnd::Returned {
            lane::Ends::worker_ended(&*self.0, cause);
        }
    }
}

/// **The lane, held by the application**: a share per repository with requests, the turnstile the
/// read worker shares, the executor the shares' workers are started by, the wake, what the ledgers'
/// lines are said through, and the sink a gone page's request is owed.
pub(crate) struct GitMutationLane {
    shares: Mutex<Shares>,
    turns: Arc<RepoTurns>,
    executor: Executor,
    /// The application's wake, and the turnstile told: what every share's worker and ledger call.
    wake: Arc<dyn Fn() + Send + Sync>,
    say: Arc<dyn Fn(&str) + Send + Sync>,
    sink: Sink,
}

#[derive(Default)]
struct Shares {
    /// How many shares the lane has made: the last generation.
    generations: u64,
    live: Vec<Share>,
}

impl GitMutationLane {
    /// **The product's lane**: workers started through the thread door, below normal, by the first
    /// request each repository's share is asked; the ledgers' lines said in `diagnostics.log`; and
    /// the sink that ignores what a gone page is owed.
    pub(crate) fn new(turns: Arc<RepoTurns>, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::start_with(
            Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            turns,
            wake,
            crate::diagnostics::note,
            |_, _| {},
        )
    }

    /// The lane with its workers started by `executor`, ordered against the read worker by
    /// `turns`, waking through `wake`, saying its ledgers' lines through `say`, and ending a gone
    /// page's request in `sink`.
    pub(crate) fn start_with(
        executor: Executor,
        turns: Arc<RepoTurns>,
        wake: impl Fn() + Send + Sync + 'static,
        say: impl Fn(&str) + Send + Sync + 'static,
        sink: impl Fn(MutationId, MutationTerminal) + Send + Sync + 'static,
    ) -> Self {
        let told = Arc::clone(&turns);
        Self {
            shares: Mutex::new(Shares::default()),
            turns,
            executor,
            wake: Arc::new(move || {
                told.changed();
                wake();
            }),
            say: Arc::new(say),
            sink: Arc::new(sink),
        }
    }

    /// The turnstile the read worker takes its turns through.
    pub(crate) fn turns(&self) -> &Arc<RepoTurns> {
        &self.turns
    }

    /// **Ask for one write or checkout** (C1): admitted on its repository's share and held for its
    /// page — its identity answered — or refused at the door and handed back. Never waits for a
    /// worker. Nothing is coalesced.
    #[expect(
        clippy::result_large_err,
        reason = "permanent: a refused request is handed back by value to the page that asked, \
                  which files it as its receipt"
    )]
    pub(crate) fn ask(&self, request: GitRequest) -> Result<MutationId, (Refused, GitRequest)> {
        self.ask_through(request, |_| Box::new(git::ask_git))
    }

    /// [`Self::ask`], the share's worker — when this request is the one that starts it — answering
    /// through the door `make` makes on its thread.
    #[expect(
        clippy::result_large_err,
        reason = "permanent: a refused request is handed back by value to the page that asked, \
                  which files it as its receipt"
    )]
    pub(crate) fn ask_through(
        &self,
        request: GitRequest,
        make: impl FnOnce(&WorkerCtx) -> Door + Send + 'static,
    ) -> Result<MutationId, (Refused, GitRequest)> {
        let mut shares = locked(&self.shares);
        let root = request.question.folder().to_path_buf();
        let at = match shares
            .live
            .iter()
            .position(|share| same_folder(&share.root, &root))
        {
            Some(at) => at,
            None => {
                shares.generations += 1;
                let generation = shares.generations;
                let share = self.share(root.clone(), generation);
                shares.live.push(share);
                shares.live.len() - 1
            }
        };
        let share = &mut shares.live[at];
        let asker = Asker {
            share: share.generation,
            sink: Arc::clone(&self.sink),
        };
        let queue = Arc::clone(&share.queue);
        let turns = Arc::clone(&self.turns);
        let wake = Arc::clone(&self.wake);
        let held = match share.queue.ask(
            request.question.clone(),
            asker,
            |_, _| Relation::Other,
            |ran| {
                self.executor
                    .start_worker("bt-git-mutation", Arc::new(Ending(ran)), move |ctx| {
                        let mut door = make(ctx);
                        queue.serve(
                            ctx,
                            |ctx, question| {
                                Ok(turns.mutate(ctx, question.folder(), || door(ctx, &question)))
                            },
                            &*wake,
                        );
                    })
            },
        ) {
            Ok(held) => held,
            Err((refused, _)) => return Err((refused, request)),
        };
        let id = MutationId {
            share: share.generation,
            minted: held.id(),
        };
        share.held.insert(held.id(), (held, request));
        Ok(id)
    }

    /// A new share for `root`, of `generation`, its ledger held by the turnstile.
    fn share(&self, root: PathBuf, generation: u64) -> Share {
        let say = Arc::clone(&self.say);
        let wake = Arc::clone(&self.wake);
        let ledger = Ledger::new(
            LANE,
            Some(MUTATION_BOUND),
            Saying::PerRequest,
            move |line| say(line),
            move || wake(),
        );
        self.turns.hold(&root, Arc::clone(&ledger));
        Share {
            root,
            generation,
            queue: Arc::new(Queue::new(ledger)),
            held: std::collections::BTreeMap::new(),
        }
    }

    /// **The window thread's drain**: first every request whose page has `gone` is let go — its
    /// terminal, now or when it comes, goes to the sink — then every terminal each share holds
    /// below its drain mark is taken, share by share, each in mint order; a share left holding
    /// nothing is let go (its worker returns, owing nothing). One lock; never waits for a worker.
    pub(crate) fn ended(&self, gone: impl Fn(AnswerOwner) -> bool) -> Vec<Ended> {
        let mut shares = locked(&self.shares);
        let mut ended = Vec::new();
        for share in &mut shares.live {
            share.let_go(&gone);
            share.ended(&mut ended);
        }
        let (idle, live): (Vec<Share>, Vec<Share>) = std::mem::take(&mut shares.live)
            .into_iter()
            .partition(|share| share.held.is_empty());
        shares.live = live;
        drop(shares);
        for share in idle {
            self.turns.forget(&share.root);
            share.queue.close();
        }
        ended
    }

    /// How many of `root`'s requests wait in its share's line behind the one its worker runs.
    #[cfg(test)]
    fn in_line(&self, root: &Path) -> usize {
        locked(&self.shares)
            .live
            .iter()
            .filter(|share| same_folder(&share.root, root))
            .map(|share| share.queue.in_line())
            .sum()
    }
}

impl Drop for GitMutationLane {
    /// The workers run what is in line and return; the requests held go to the sink.
    fn drop(&mut self) {
        for share in &locked(&self.shares).live {
            share.queue.close();
        }
    }
}

/// **The receipt a terminal is, for the page that asked**: git's own receipt when the verb ran
/// (`Done`, or the receipt a request joined to it carries); otherwise the receipt of its question
/// with [`GitFault::NotRun`] — `Failed` for a panic in its work, `WorkerGone` for a worker that
/// ended with it owed — so the rows it dimmed undim, and the notice says Folio did not run it.
pub(crate) fn receipt(terminal: MutationTerminal, request: GitRequest) -> GitResponse {
    let answer = match terminal {
        Terminal::Done(answer) => answer,
        Terminal::Joined { outcome, .. } => return receipt(*outcome, request),
        Terminal::Failed(_) => git::faulted(&request.question, GitFault::NotRun(NotRun::Failed)),
        Terminal::WorkerGone => {
            git::faulted(&request.question, GitFault::NotRun(NotRun::WorkerGone))
        }
        Terminal::Superseded { .. } | Terminal::Cancelled => {
            unreachable!("the git mutation lane supersedes and cancels nothing")
        }
    };
    GitResponse {
        window: request.window,
        host: request.host,
        answer,
    }
}

/// **The receipt a refusal at the door is**: the request's question with [`GitFault::NotRun`] —
/// `Full` past the bound, `WorkerGone` for a share whose worker has ended, `Failed` for a worker
/// the thread door would not start — filed by the page at once, so its rows undim.
pub(crate) fn refusal(refused: &Refused, request: GitRequest) -> GitResponse {
    let why = match refused {
        Refused::Full { .. } => NotRun::Full,
        Refused::Faulted(_) => NotRun::WorkerGone,
        Refused::NotStarted(_) => NotRun::Failed,
        Refused::TwinEnded => unreachable!("the git mutation lane joins nothing"),
    };
    GitResponse {
        answer: git::faulted(&request.question, GitFault::NotRun(why)),
        window: request.window,
        host: request.host,
    }
}

/// **A git answer that went through**, for a fake door: the receipt git gives a write or a
/// checkout that succeeded, with the question's own subject.
#[cfg(test)]
pub(crate) fn went_through(question: &GitQuestion) -> GitAnswer {
    match question {
        GitQuestion::Write { root, verb, paths } => GitAnswer::Write {
            root: root.clone(),
            verb: verb.clone(),
            paths: paths.clone(),
            outcome: Ok(()),
        },
        GitQuestion::Checkout { root, target, .. } => GitAnswer::Checkout {
            root: root.clone(),
            target: target.clone(),
            outcome: Ok(()),
        },
        other => unreachable!("the git mutation lane is asked only writes and checkouts: {other:?}"),
    }
}

/// **The lane contract suite's adapter for the git mutation lane** (`crate::lane`): the real
/// [`GitMutationLane`], driven with the suite's scripted executor through its own admission
/// ([`GitMutationLane::ask_through`]) and the product's acceptance — the drain lets go of a closed
/// tab's requests ([`GitMutationLane::ended`]) and each receipt is claimed by its tab
/// (`claimed_by`), as `apply_git_results` asks through `Runtime::owns`.
///
/// Every question is a `Stage` of one file in one repository — the file named by the question,
/// in Chinese and Latin — and the door is a fake that tells the suite's gate which question it
/// executes (`Gate::execute`), inside the request's task, then answers that the write went
/// through: no `git` is started.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::git::{GitHost, GitWriteVerb};
    use crate::lane::{
        Admission, Contract, Delivered, GIT_MUTATION, Gate, LaneUnderTest, Outcome, WakeProbe,
    };
    use crate::{LeafId, SeatId, TabId, WindowId, claimed_by};

    struct Adapter {
        lane: GitMutationLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        opened: BTreeSet<u32>,
        closed: BTreeSet<u32>,
        sunk: Arc<Mutex<Vec<Delivered>>>,
        said: Arc<AtomicUsize>,
    }

    /// The one window the adapter's tabs stand in.
    fn adapter_window() -> WindowId {
        WindowId::from(1_u64)
    }

    fn tab_of(target: u32) -> TabId {
        TabId(u64::from(target) + 1)
    }

    /// The file a question stages: its name carries the question.
    fn path_of(question: u64) -> String {
        format!("改动 notes-{question}.md")
    }

    fn question_of(path: &str) -> Option<u64> {
        path.strip_prefix("改动 notes-")?
            .strip_suffix(".md")?
            .parse()
            .ok()
    }

    fn ticket(generation: u64, minted: Minted) -> u64 {
        MutationId {
            share: generation,
            minted,
        }
        .number()
    }

    fn outcome(generation: u64, terminal: &MutationTerminal) -> Outcome {
        match terminal {
            Terminal::Done(_) => Outcome::Answered,
            Terminal::Failed(failure) => Outcome::Failed(failure.to_string()),
            Terminal::Cancelled => Outcome::Cancelled,
            Terminal::Superseded { by } => Outcome::Superseded {
                by: ticket(generation, *by),
            },
            Terminal::Joined {
                with,
                outcome: carried,
            } => Outcome::Joined {
                with: ticket(generation, *with),
                carried: Box::new(outcome(generation, carried)),
            },
            Terminal::WorkerGone => Outcome::WorkerGone,
        }
    }

    /// A fresh lane for one claim.
    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let sunk = Arc::new(Mutex::new(Vec::new()));
        let said = Arc::new(AtomicUsize::new(0));
        let heard = Arc::clone(&probe);
        let counted = Arc::clone(&said);
        let sink = Arc::clone(&sunk);
        let lane = GitMutationLane::start_with(
            Executor::scripted(Arc::clone(&gate)),
            RepoTurns::new(),
            move || heard.woke(),
            move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
            },
            move |id: MutationId, terminal| {
                locked(&sink).push(Delivered {
                    target: None,
                    ticket: Some(id.number()),
                    question: None,
                    outcome: outcome(id.share, &terminal),
                });
            },
        );
        Box::new(Adapter {
            lane,
            gate,
            probe,
            opened: BTreeSet::new(),
            closed: BTreeSet::new(),
            sunk,
            said,
        })
    }

    impl LaneUnderTest for Adapter {
        fn contract(&self) -> &'static Contract {
            &GIT_MUTATION
        }

        fn gate(&self) -> &Gate {
            &self.gate
        }

        fn probe(&self) -> &WakeProbe {
            &self.probe
        }

        fn submit(&mut self, target: u32, question: u64) -> Admission {
            self.opened.insert(target);
            let request = GitRequest {
                window: adapter_window(),
                host: GitHost::Column(LeafId {
                    tab: tab_of(target),
                    seat: SeatId(1),
                }),
                question: GitQuestion::Write {
                    root: PathBuf::from("仓库 repo"),
                    verb: GitWriteVerb::Stage,
                    paths: vec![path_of(question)],
                },
            };
            let gate = Arc::clone(&self.gate);
            let door = move |_: &WorkerCtx| -> Door {
                Box::new(move |_: &WorkerCtx, question: &GitQuestion| {
                    if let GitQuestion::Write { paths, .. } = question
                        && let Some(asked) = paths.first().and_then(|path| question_of(path))
                    {
                        gate.execute(asked);
                    }
                    went_through(question)
                })
            };
            match self.lane.ask_through(request, door) {
                Ok(id) => Admission {
                    ticket: Some(id.number()),
                    question,
                    refused: None,
                },
                Err((refused, _)) => Admission {
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
            self.lane
                .ended(|owner| !claimed_by(owner, adapter_window(), open.iter().copied()))
                .into_iter()
                .map(|(id, terminal, request)| match &terminal {
                    Terminal::Done(GitAnswer::Write { paths, .. }) => {
                        let target =
                            claimed_by(request.owner(), adapter_window(), open.iter().copied())
                                .then(|| u32::try_from(request.host.tab().0 - 1).ok())
                                .flatten();
                        Delivered {
                            target,
                            ticket: Some(id.number()),
                            question: paths.first().and_then(|path| question_of(path)),
                            outcome: Outcome::Answered,
                        }
                    }
                    other => Delivered {
                        target: None,
                        ticket: Some(id.number()),
                        question: None,
                        outcome: outcome(id.share, other),
                    },
                })
                .collect()
        }

        fn offer_stale(&mut self, _ticket: u64) {
            unreachable!("the git mutation lane's target is the asking page, not the application");
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
    use std::sync::mpsc;

    use super::*;
    use crate::git::{GitHost, GitWriteVerb};
    use crate::lane::{Gate, Hold};
    use crate::{LeafId, SeatId, TabId, WindowId};

    /// One terminal the sink took: the request's identity and how it ended.
    type Sunk = (MutationId, MutationTerminal);

    /// **A lane under a test's eye** — the lane, its wakes, the lines its ledgers said, what its
    /// sink took, and every terminal drained and not yet asked for.
    struct Watched {
        lane: GitMutationLane,
        wakes: mpsc::Receiver<()>,
        said: Arc<Mutex<Vec<String>>>,
        sunk: Arc<Mutex<Vec<Sunk>>>,
        taken: Vec<Ended>,
    }

    fn watched(executor: Executor) -> Watched {
        let (wake, wakes) = lane::wake_channel();
        let said = Arc::new(Mutex::new(Vec::new()));
        let sunk = Arc::new(Mutex::new(Vec::new()));
        let lines = Arc::clone(&said);
        let sink = Arc::clone(&sunk);
        Watched {
            lane: GitMutationLane::start_with(
                executor,
                RepoTurns::new(),
                wake,
                move |line| locked(&lines).push(line.to_owned()),
                move |id, terminal| locked(&sink).push((id, terminal)),
            ),
            wakes,
            said,
            sunk,
            taken: Vec::new(),
        }
    }

    impl Watched {
        /// **Request `id`'s terminal and what it asked**, drained as the window thread drains —
        /// once now, then once after each wake, each wake awaited within the lane suite's
        /// patience — with `gone` the pages no window holds.
        fn terminal(
            &mut self,
            id: MutationId,
            gone: impl Fn(AnswerOwner) -> bool,
        ) -> (MutationTerminal, GitRequest) {
            loop {
                let ended = self.lane.ended(&gone);
                self.taken.extend(ended);
                if let Some(at) = self.taken.iter().position(|(taken, ..)| *taken == id) {
                    let (_, terminal, request) = self.taken.remove(at);
                    return (terminal, request);
                }
                lane::wait_for_a_wake(&self.wakes, "a git mutation's terminal");
            }
        }

        fn sunk(&self) -> Vec<Sunk> {
            locked(&self.sunk).clone()
        }

        fn said(&self) -> Vec<String> {
            locked(&self.said).clone()
        }
    }

    fn nobody(_: AnswerOwner) -> bool {
        false
    }

    fn column(tab: u64) -> GitHost {
        GitHost::Column(LeafId {
            tab: TabId(tab),
            seat: SeatId(1),
        })
    }

    /// Stage one file in `root`, from the column in tab `tab`.
    fn stage(tab: u64, root: &str, path: &str) -> GitRequest {
        GitRequest {
            window: WindowId::from(1_u64),
            host: column(tab),
            question: GitQuestion::Write {
                root: PathBuf::from(root),
                verb: GitWriteVerb::Stage,
                paths: vec![path.to_owned()],
            },
        }
    }

    /// Stand on `target` in `root`, from the column in tab `tab`.
    fn checkout(tab: u64, root: &str, target: &str) -> GitRequest {
        GitRequest {
            window: WindowId::from(1_u64),
            host: column(tab),
            question: GitQuestion::Checkout {
                root: PathBuf::from(root),
                target: target.to_owned(),
                detach: false,
            },
        }
    }

    /// What a question is about, for a log: its file or its branch.
    fn subject(question: &GitQuestion) -> String {
        match question {
            GitQuestion::Write { paths, .. } => paths.join(","),
            GitQuestion::Checkout { target, .. } => target.clone(),
            other => format!("{other:?}"),
        }
    }

    /// **A fake door that logs** `start <subject>` as a question begins, stands in `hold` when
    /// the question's repository is `held_root`, then logs `end <subject>` and answers that the
    /// verb went through.
    fn logging(
        log: Arc<Mutex<Vec<String>>>,
        hold: Arc<Hold>,
        held_root: PathBuf,
    ) -> impl FnOnce(&WorkerCtx) -> Door + Send + 'static {
        move |_| {
            Box::new(move |_: &WorkerCtx, question: &GitQuestion| {
                locked(&log).push(format!("start {}", subject(question)));
                if same_folder(question.folder(), &held_root) {
                    hold.pass();
                }
                locked(&log).push(format!("end {}", subject(question)));
                went_through(question)
            })
        }
    }

    /// A door that answers at once that the verb went through.
    fn through(_: &WorkerCtx) -> Door {
        Box::new(|_: &WorkerCtx, question: &GitQuestion| went_through(question))
    }

    /// A read's turn on `folder`, taken on a worker of its own as the read worker takes it: it
    /// logs `read` when its turn comes, and says so on `done`.
    fn read_on_a_worker(
        turns: Arc<RepoTurns>,
        folder: &str,
        log: Arc<Mutex<Vec<String>>>,
        done: mpsc::Sender<()>,
    ) {
        let folder = PathBuf::from(folder);
        bt_platform::spawn_at_priority(
            "git-mutation-lane-test-read",
            bt_platform::ThreadPriority::Normal,
            move |ctx| {
                turns.read(ctx, &folder, || locked(&log).push("read".to_owned()));
                let _ = done.send(());
            },
        )
        .expect("the test's reader starts");
    }

    /// RED (T-GIT-MUTATIONS; A§5.1's storage rule) — **Two presses on one repository run one after
    /// the other, each with a terminal of its own.**
    ///
    /// A page stages `说明 notes.md` and, while git is still at it, a checkout of `功能/feature`
    /// is pressed. The checkout waits in the repository's line until the write has ended — it is
    /// seen waiting there, not running beside it — then runs; each ends `Done` with its own
    /// receipt, under an identity of its own.
    ///
    /// MUTATION: have `GitMutationLane::ask_through` make a share for every request (never find the
    /// root's share) — the checkout runs while the write stands, and the log interleaves.
    #[test]
    fn two_mutations_on_one_repository_run_in_order_with_two_terminals() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let root = "仓库 repo";
        let door = || logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from(root));
        let write = lane
            .lane
            .ask_through(stage(1, root, "说明 notes.md"), door())
            .expect("the write is admitted");
        hold.wait_standing("the write running");
        let checkout = lane
            .lane
            .ask_through(checkout(1, root, "功能/feature"), door())
            .expect("the checkout is admitted");
        lane::wait_until("the checkout waiting in line, or running", || {
            lane.lane.in_line(Path::new(root)) == 1
                || locked(&log).iter().any(|line| line.contains("功能/feature"))
        });
        hold.release();
        let (first, asked_first) = lane.terminal(write, nobody);
        let (second, asked_second) = lane.terminal(checkout, nobody);
        assert_ne!(write, checkout, "two presses, two identities");
        assert_eq!(
            *locked(&log),
            [
                "start 说明 notes.md",
                "end 说明 notes.md",
                "start 功能/feature",
                "end 功能/feature",
            ],
            "the checkout ran after the write, never beside it"
        );
        assert!(matches!(
            first,
            Terminal::Done(GitAnswer::Write { outcome: Ok(()), .. })
        ));
        assert!(matches!(
            second,
            Terminal::Done(GitAnswer::Checkout { outcome: Ok(()), .. })
        ));
        assert_eq!(subject(&asked_first.question), "说明 notes.md");
        assert_eq!(subject(&asked_second.question), "功能/feature");
    }

    /// RED (T-GIT-MUTATIONS) — **One repository's verb does not wait for another's.**
    ///
    /// A write in `仓库 alpha` stands in git; a write in `仓库 beta`, pressed meanwhile, runs and
    /// ends while the first still stands.
    ///
    /// MUTATION: have `GitMutationLane::ask_through` find every request's share as the first
    /// share there is (one line for every repository) — the second write waits behind the first,
    /// and no terminal comes within the lane suite's patience.
    #[test]
    fn mutations_on_two_repositories_do_not_wait_for_each_other() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let door = || logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from("仓库 alpha"));
        let alpha = lane
            .lane
            .ask_through(stage(1, "仓库 alpha", "甲 a.txt"), door())
            .expect("admitted");
        hold.wait_standing("the write in alpha running");
        let beta = lane
            .lane
            .ask_through(stage(2, "仓库 beta", "乙 b.txt"), door())
            .expect("admitted");
        let (ended, _) = lane.terminal(beta, nobody);
        assert!(matches!(ended, Terminal::Done(_)), "beta's write ran");
        assert!(
            !locked(&log).iter().any(|line| line == "end 甲 a.txt"),
            "while alpha's still stood"
        );
        hold.release();
        assert!(matches!(lane.terminal(alpha, nobody).0, Terminal::Done(_)));
    }

    /// RED (T-GIT-MUTATIONS; the torn answer) — **A read of a repository waits behind the verb
    /// running in it, and runs after it ends.**
    ///
    /// A checkout of `发布/release` stands in git; the read worker's turn for the same repository
    /// is asked meanwhile. The read is seen waiting, and runs only after the checkout has ended: a
    /// status read in the middle of a checkout would describe neither branch.
    ///
    /// MUTATION: have `RepoTurns::read` not wait (drop its `while state.owes(folder)` loop) — the
    /// read runs while the checkout stands.
    #[test]
    fn a_read_of_a_repository_waits_behind_its_in_flight_mutation() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let root = "仓库 repo";
        let checkout = lane
            .lane
            .ask_through(
                checkout(1, root, "发布/release"),
                logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from(root)),
            )
            .expect("admitted");
        hold.wait_standing("the checkout running");
        let (done, read) = mpsc::channel();
        read_on_a_worker(
            Arc::clone(lane.lane.turns()),
            root,
            Arc::clone(&log),
            done,
        );
        lane::wait_until("the read waiting its turn, or run", || {
            lane.lane.turns().reads_waiting() == 1
                || locked(&log).iter().any(|line| line == "read")
        });
        hold.release();
        lane::receive(&read, "the read's turn");
        assert!(matches!(lane.terminal(checkout, nobody).0, Terminal::Done(_)));
        assert_eq!(
            *locked(&log),
            ["start 发布/release", "end 发布/release", "read"],
            "the read ran after the checkout, never in the middle of it"
        );
    }

    /// RED (T-GIT-MUTATIONS; C6) — **A closed page's receipt reaches the lane's sink, never the
    /// floor and never another page.**
    ///
    /// The page in tab 2 stages `草稿 draft.md` while a write from tab 1 runs, and its tab closes;
    /// tab 3's page stages after it. The drain lets go of tab 2's request by its identity, and its
    /// terminal — git's receipt — goes to the sink, once; tabs 1 and 3 take their own.
    ///
    /// MUTATION: make `Asker::sink` drop its terminal (an empty body) — the sink takes nothing.
    #[test]
    fn a_closed_git_pages_answer_reaches_the_ignoring_sink() {
        let gate = Arc::new(Gate::default());
        let mut lane = watched(Executor::scripted(Arc::clone(&gate)));
        gate.hold();
        let root = "仓库 repo";
        let running = lane
            .lane
            .ask_through(stage(1, root, "一 one.md"), through)
            .expect("admitted");
        assert!(gate.wait_entered(1), "the first write reaches the worker");
        let closed = lane
            .lane
            .ask_through(stage(2, root, "草稿 draft.md"), through)
            .expect("admitted");
        let open = lane
            .lane
            .ask_through(stage(3, root, "三 three.md"), through)
            .expect("admitted");
        gate.release();
        let gone = |owner| owner == AnswerOwner::Tab(TabId(2));
        for id in [running, open] {
            let (_, asked) = lane.terminal(id, gone);
            assert_ne!(asked.host, column(2), "a page took the closed tab's receipt");
        }
        let sunk = lane.sunk();
        assert_eq!(
            sunk.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [closed],
            "the closed tab's request ends in the sink, once"
        );
        assert!(
            matches!(&sunk[0].1, Terminal::Done(GitAnswer::Write { paths, .. }) if paths == &["草稿 draft.md"]),
            "with git's own receipt"
        );
    }

    /// RED (T-GIT-MUTATIONS; revision (b)3) — **A full repository's lane refuses at the press,
    /// at once, and the page files the refusal as its receipt.**
    ///
    /// With one write running and the rest of the bound waiting, one more press is refused at the
    /// door — synchronously, with no identity — and handed back; its receipt is the write's own
    /// question with `NotRun::Full`, in the words the page's notice says. Every admitted request
    /// still ends.
    ///
    /// MUTATION: build a share's ledger with no bound (`None` for `Some(MUTATION_BOUND)`) — the
    /// press past the bound is admitted.
    #[test]
    fn a_full_git_lane_refuses_at_the_door_without_waiting() {
        let gate = Arc::new(Gate::default());
        let mut lane = watched(Executor::scripted(Arc::clone(&gate)));
        gate.hold();
        let root = "仓库 repo";
        let mut admitted = Vec::new();
        for n in 0..MUTATION_BOUND {
            let id = lane
                .lane
                .ask_through(stage(1, root, &format!("文件 {n}.txt")), through)
                .expect("within the bound");
            admitted.push(id);
            if n == 0 {
                assert!(gate.wait_entered(1), "the first write reaches the worker");
            }
        }
        let (refused, handed_back) = lane
            .lane
            .ask_through(stage(1, root, "多余 extra.txt"), through)
            .expect_err("the press past the bound is refused at the door");
        assert_eq!(
            refused,
            Refused::Full {
                bound: MUTATION_BOUND
            }
        );
        let receipt = refusal(&refused, handed_back);
        match &receipt.answer {
            GitAnswer::Write {
                paths,
                outcome: Err(fault),
                ..
            } => {
                assert_eq!(paths, &["多余 extra.txt"], "the refused write's own rows undim");
                assert_eq!(*fault, GitFault::NotRun(NotRun::Full));
                assert_eq!(
                    git::fault_sentence_in(fault, crate::i18n::Lang::English),
                    crate::i18n::Text::GitNotRunFull.in_lang(crate::i18n::Lang::English)
                );
            }
            other => panic!("the refusal's receipt is the write's: {other:?}"),
        }
        gate.release();
        for id in admitted {
            assert!(matches!(lane.terminal(id, nobody).0, Terminal::Done(_)));
        }
    }

    /// RED (T-GIT-MUTATIONS; C2) — **A repository's worker that dies ends every verb it owed as
    /// `WorkerGone`, the page files each as Folio's refusal, and the repository's reads go on.**
    ///
    /// Three writes are owed when the worker dies on the first. Each ends `WorkerGone`, and its
    /// receipt carries `NotRun::WorkerGone`, so its rows undim. A read of the repository is not
    /// left waiting on a share that can no longer answer, and the next press on the repository —
    /// a new request, not a retry — is run by a new worker.
    ///
    /// MUTATION: hand `start_worker` an `Ends` that ignores the worker's end in place of `ran`
    /// (`GitMutationLane::ask_through`) — no terminal comes, and the read waits.
    #[test]
    fn the_git_workers_death_ends_its_owed_mutations_as_worker_gone() {
        let gate = Arc::new(Gate::default());
        let mut lane = watched(Executor::scripted(Arc::clone(&gate)));
        gate.hold();
        let root = "仓库 repo";
        let owed: Vec<MutationId> = ["甲 a.md", "乙 b.md", "丙 c.md"]
            .into_iter()
            .map(|path| {
                lane.lane
                    .ask_through(stage(1, root, path), through)
                    .expect("admitted")
            })
            .collect();
        assert!(gate.wait_entered(1), "the first write reaches the worker");
        gate.kill();
        gate.release();
        for id in owed {
            let (terminal, asked) = lane.terminal(id, nobody);
            assert_eq!(terminal, Terminal::WorkerGone);
            let receipt = receipt(terminal, asked);
            assert!(
                matches!(
                    &receipt.answer,
                    GitAnswer::Write {
                        outcome: Err(GitFault::NotRun(NotRun::WorkerGone)),
                        ..
                    }
                ),
                "the page files Folio's refusal: {receipt:?}"
            );
        }
        assert!(
            lane.said().iter().any(|line| line.contains("WorkerGone")),
            "the death is said once"
        );
        let log = Arc::new(Mutex::new(Vec::new()));
        let (done, read) = mpsc::channel();
        read_on_a_worker(Arc::clone(lane.lane.turns()), root, log, done);
        lane::receive(&read, "a read after the worker's death");
        let next = lane
            .lane
            .ask_through(stage(1, root, "丁 d.md"), through)
            .expect("the next press is a new request");
        assert!(matches!(lane.terminal(next, nobody).0, Terminal::Done(_)));
    }
}
