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
//! * **Reads take turns with verbs** ([`RepoTurns`]). A question on the read worker about a
//!   repository waits for the verbs its share had minted when the question was asked — a request
//!   is minted at the press, so a read asked after a press never runs in the middle of that verb —
//!   and for a verb running there; a verb pressed after the read was asked waits for the read
//!   instead, so later presses never lengthen a read's wait. A verb starts only while no read of
//!   its repository runs. A read waits **in the read worker's line, never on its thread**: one
//!   whose turn has not come is parked ([`ReadTicket::begin`] hands its ticket back) and the
//!   worker answers the next question — of any other repository — meanwhile; when a verb's end
//!   may have brought a parked read's turn, the worker is nudged and takes the parked reads whose
//!   turn has come first, in the order they were parked. A read waits at most the verbs owed when
//!   it was asked, each one git child bounded by [`crate::git::GIT_COMMAND_TIMEOUT`] (up to
//!   [`MUTATION_BOUND`] of them); a read of another repository is not held by it. A verb waits, on
//!   its share's worker, at most the one read running, or the reads asked before it was pressed.
//! * **Capacity.** A share owes at most [`MUTATION_BOUND`] requests, a terminal not yet taken
//!   holding its place: what one Git page can hand it at once. A request past it — or asked of a
//!   share whose worker has ended — is refused at the door, synchronously, with no identity minted
//!   (revision (b)3), and handed back; the page files it at once as a receipt that says so
//!   ([`refusal`]), so its rows undim. Nothing is retried on a clock (revision (b)5).
//! * **Completion.** `Done` carries git's receipt (`GitAnswer::Write` / `::Checkout`): a verb git
//!   refused is that receipt's own error, in git's words. A panic in one request's work is that
//!   request's `Failed`, caught by its [`crate::lane::task`] and said once by the share's ledger;
//!   the worker goes on, and the page files it as [`GitFault::Unfinished`] — git may have run the
//!   verb before the panic, so the rows undim and the page reads the repository again. A worker's
//!   end ends every request of that share still in line `WorkerGone`, faults the share and wakes
//!   the loop; those, and a refusal at the door, never ran, and reach the page as
//!   [`GitFault::NotRun`] ([`receipt`]): the rows undim and the notice says Folio did not run it.
//! * **Abandonment.** A request is owed to the repository cache that asked
//!   ([`git::GitCacheId`]), not to the place it asked from. One whose cache no open surface holds —
//!   its column or its tab closed, its floating window dismissed, its window closed, its column
//!   pointed at another folder — is let go by the drain before it takes anything ([`OpenPages`]);
//!   its terminal, now or when it comes, goes to the lane's sink, which ignores it (C6): nobody is
//!   left to undim, and a failure in it was said by the ledger. A view that **moved** while git ran
//!   — a float docked into a column, a column popped out into a float, a pane carried into another
//!   tab — still holds its cache, and its receipt reaches it where it stands.
//! * **Lifetime.** A share is made by the first request for its repository and let go by the drain
//!   once the window thread holds none of its requests **and its ledger owes none** — a let-go
//!   request still running or in line keeps its share, its one worker and its place in the
//!   turnstile until its terminal has reached the sink, and that terminal's wake brings the drain
//!   that lets the share go. Its worker then runs what is in line — nothing — and returns, and the
//!   next press on that repository makes a new share, of a new generation; one repository never has
//!   two workers with verbs.
//! * **Wake.** Each worker publishes the terminal first and then wakes the loop
//!   (`AppEvent::GitReady`, the read worker's own wake); each ledger wakes it the same way when its
//!   worker ends with requests owed. Every wake also tells the turnstile ([`RepoTurns`]) that what
//!   a repository owes may have changed, which nudges the read worker when a read is parked.

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use bt_platform::admission::WorkerCtx;

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
/// by the read worker (`git::GitWorker`) and every share of the lane. Only a share's worker ever
/// waits in it ([`RepoTurns::verb_turn`]); the read worker never does — a read whose turn has not
/// come is parked in the worker's own line ([`ReadTicket::begin`]), and the worker is nudged when
/// it may have come.
///
/// What a share owes is its ledger's to say ([`Ledger::earliest_owed`]): the turnstile holds each
/// repository's ledger, never a count of its own. A request is owed from its admission until its
/// terminal is published, and its worker's end ends every one it still owes, so a read never waits
/// on a share that cannot answer.
pub(crate) struct RepoTurns {
    state: Mutex<TurnState>,
    changed: Condvar,
}

/// **The read worker's nudge**: what tells it that a parked read's turn may have come.
type Nudge = Box<dyn Fn() + Send>;

#[derive(Default)]
struct TurnState {
    /// Each repository's share's ledger, while it has a share.
    shares: Vec<(PathBuf, Arc<MutationLedger>)>,
    /// The reads asked and not yet run, each with the verbs it waits for.
    asked: Vec<Asked>,
    /// How many reads have been asked: the last read's number.
    reads: u64,
    /// The folders a read is running in.
    reading: Vec<PathBuf>,
    /// The folders a verb is running in.
    running: Vec<PathBuf>,
    /// The read worker's nudge, once it has one ([`RepoTurns::nudge_with`]).
    nudge: Option<Nudge>,
    /// How many verbs wait their turn now — where a test sees a verb wait.
    #[cfg(test)]
    verbs_waiting: usize,
}

/// **A read asked and not yet run**: its folder, the verbs it waits for — every one its folder's
/// share had minted when the read was asked, named by that share's ledger and the last number it
/// had minted; none when the folder had no share or the share had minted nothing — and whether it
/// is parked in the read worker's line, its turn not come when it was last looked at.
struct Asked {
    read: u64,
    folder: PathBuf,
    behind: Option<(Arc<MutationLedger>, Minted)>,
    parked: bool,
}

impl Asked {
    /// Whether a verb this read waits for is still owed.
    fn behind_a_verb(&self) -> bool {
        self.behind
            .as_ref()
            .is_some_and(|(ledger, last)| ledger.earliest_owed().is_some_and(|owed| owed <= *last))
    }

    /// Whether the verb `running` — the earliest `ledger` owes — is one this read waits for.
    fn waits_for(&self, ledger: &Arc<MutationLedger>, running: Option<Minted>) -> bool {
        self.behind.as_ref().is_some_and(|(behind, last)| {
            Arc::ptr_eq(behind, ledger) && running.is_some_and(|running| running <= *last)
        })
    }
}

impl TurnState {
    fn share(&self, folder: &Path) -> Option<&Arc<MutationLedger>> {
        self.shares
            .iter()
            .find(|(root, _)| same_folder(root, folder))
            .map(|(_, ledger)| ledger)
    }

    fn reading(&self, folder: &Path) -> bool {
        self.reading.iter().any(|at| same_folder(at, folder))
    }

    fn running(&self, folder: &Path) -> bool {
        self.running.iter().any(|at| same_folder(at, folder))
    }

    /// Whether the read numbered `read` must still wait: a verb it waits for is owed, a verb runs
    /// in its folder, or a read of its folder asked before it still waits — so a folder's reads
    /// begin in the order they were asked. That last clause is what keeps the order: what a share
    /// owes is read from its ledger, outside this lock, so two reads looked at one after the other
    /// can see two different answers.
    fn read_waits(&self, read: u64) -> bool {
        self.asked
            .iter()
            .find(|asked| asked.read == read)
            .is_some_and(|asked| {
                let asked_before = self.asked.iter().any(|earlier| {
                    earlier.read < read && same_folder(&earlier.folder, &asked.folder)
                });
                asked_before || asked.behind_a_verb() || self.running(&asked.folder)
            })
    }

    /// Whether the verb `ledger` runs now in `folder` must still wait: a read of the folder runs, or
    /// a read of it was asked before the verb was pressed (one that does not wait for it) — parked
    /// in the read worker's line or not.
    fn verb_waits(&self, folder: &Path, ledger: &Arc<MutationLedger>) -> bool {
        let running = ledger.earliest_owed();
        self.reading(folder)
            || self.asked.iter().any(|asked| {
                same_folder(&asked.folder, folder) && !asked.waits_for(ledger, running)
            })
    }

    /// **Tell the read worker to look at its line again** when a read is parked there. Called,
    /// under the lock, after every change that can bring a parked read's turn: a verb's terminal
    /// published or its share's worker ended ([`RepoTurns::changed`]) and a verb's mark coming off
    /// ([`Running`]). A read is parked under the same lock in which its turn was found not come,
    /// so a change either came before that look, which saw it, or finds the read parked here.
    fn nudge_if_parked(&self) {
        if self.asked.iter().any(|asked| asked.parked)
            && let Some(nudge) = &self.nudge
        {
            nudge();
        }
    }
}

/// **A verb's mark in the turnstile** — a verb running in a folder — taken off when it is over,
/// or its work unwound; the read worker is nudged, since a read parked behind it may run now.
struct Running<'a> {
    turns: &'a RepoTurns,
    folder: &'a Path,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        {
            let mut state = locked(&self.turns.state);
            if let Some(at) = state
                .running
                .iter()
                .position(|at| same_folder(at, self.folder))
            {
                state.running.remove(at);
            }
            state.nudge_if_parked();
        }
        self.turns.changed.notify_all();
    }
}

/// **One read asked of the turnstile and not yet run** ([`RepoTurns::ask`]): its number, and the
/// folder it reads. It keeps the read's place — the verbs it waits for, fixed when it was asked —
/// however many times it is parked and looked at again ([`Self::begin`]); a verb pressed after it
/// waits for it while it stands. Dropped unrun — a newer ask of the same question replaced it, or
/// the read worker ended — it takes its place out, so no verb waits on a read that will not run.
pub(crate) struct ReadTicket {
    turns: Arc<RepoTurns>,
    read: u64,
    folder: PathBuf,
}

impl ReadTicket {
    /// **The read's turn, if it has come** — every verb it waits for has ended, no verb runs in
    /// its folder, and no read of its folder asked before it still waits: the folder is marked as being read until the [`Reading`] answered is dropped,
    /// so no verb of it starts before then. Otherwise the read is parked and its ticket handed
    /// back. Never waits.
    pub(crate) fn begin(self) -> Result<Reading, Self> {
        let parked = {
            let mut state = locked(&self.turns.state);
            let waits = state.read_waits(self.read);
            if waits {
                if let Some(asked) = state.asked.iter_mut().find(|asked| asked.read == self.read) {
                    asked.parked = true;
                }
            } else {
                state.asked.retain(|asked| asked.read != self.read);
                state.reading.push(self.folder.clone());
            }
            waits
        };
        if parked {
            return Err(self);
        }
        // Dropping the ticket wakes the verbs that waited on this read as asked: it runs now.
        Ok(Reading {
            turns: Arc::clone(&self.turns),
            folder: self.folder.clone(),
        })
    }
}

impl Drop for ReadTicket {
    fn drop(&mut self) {
        locked(&self.turns.state)
            .asked
            .retain(|asked| asked.read != self.read);
        self.turns.changed.notify_all();
    }
}

/// **A read running in a folder** ([`ReadTicket::begin`]): while it stands, no verb of the
/// folder starts. The read worker runs the question and sends its answer before dropping it.
pub(crate) struct Reading {
    turns: Arc<RepoTurns>,
    folder: PathBuf,
}

impl Drop for Reading {
    fn drop(&mut self) {
        {
            let mut state = locked(&self.turns.state);
            if let Some(at) = state
                .reading
                .iter()
                .position(|at| same_folder(at, &self.folder))
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

    /// **Ask for a read's turn on `folder`**, on the read worker as it takes the question from its
    /// line: the read waits for every verb the folder's share has minted now, and a verb pressed
    /// from now on waits for the read. Never waits; the turn is taken by [`ReadTicket::begin`].
    pub(crate) fn ask(self: &Arc<Self>, folder: &Path) -> ReadTicket {
        let mut state = locked(&self.state);
        state.reads += 1;
        let read = state.reads;
        let behind = state
            .share(folder)
            .and_then(|ledger| ledger.last_minted().map(|last| (Arc::clone(ledger), last)));
        state.asked.push(Asked {
            read,
            folder: folder.to_path_buf(),
            behind,
            parked: false,
        });
        ReadTicket {
            turns: Arc::clone(self),
            read,
            folder: folder.to_path_buf(),
        }
    }

    /// **How the read worker is told to look at its parked reads again** (`git::read_inbox`).
    /// Called under the turnstile's lock: it must not wait, nor call back into the turnstile.
    pub(crate) fn nudge_with(&self, nudge: impl Fn() + Send + 'static) {
        locked(&self.state).nudge = Some(Box::new(nudge));
    }

    /// **A share worker's turn on `folder`** for the verb its ledger owes first (`ledger`): waits
    /// while a read of the folder runs, or while a read of it asked before the verb was pressed
    /// waits its own turn; then runs `verb` with the folder marked as running a verb, so no read of
    /// it starts until `verb` is over.
    fn verb_turn<T>(
        &self,
        worker: &WorkerCtx,
        folder: &Path,
        ledger: &Arc<MutationLedger>,
        verb: impl FnOnce() -> T,
    ) -> T {
        let _ = worker;
        {
            let mut state = locked(&self.state);
            #[cfg(test)]
            {
                state.verbs_waiting += 1;
            }
            while state.verb_waits(folder, ledger) {
                state = self
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            #[cfg(test)]
            {
                state.verbs_waiting -= 1;
            }
            state.running.push(folder.to_path_buf());
        }
        let _running = Running {
            turns: self,
            folder,
        };
        verb()
    }

    /// `root`'s share is `ledger` from now on.
    fn hold(&self, root: &Path, ledger: Arc<MutationLedger>) {
        locked(&self.state)
            .shares
            .push((root.to_path_buf(), ledger));
        self.changed.notify_all();
    }

    /// The share whose ledger is `ledger` is gone — named by its ledger, not its root, so a
    /// newer share of the same repository is never forgotten in its place.
    fn forget(&self, ledger: &Arc<MutationLedger>) {
        locked(&self.state)
            .shares
            .retain(|(_, held)| !Arc::ptr_eq(held, ledger));
        self.changed.notify_all();
    }

    /// What a repository owes may have changed: every verb waiting looks again, and the read
    /// worker is nudged when a read is parked. The lock is taken after the change, so a turn
    /// between its look and its wait — or a read being parked — cannot miss this.
    fn changed(&self) {
        locked(&self.state).nudge_if_parked();
        self.changed.notify_all();
    }

    /// How many reads are parked in the read worker's line now: asked, and their turn found not
    /// come — where a test sees a read wait.
    #[cfg(test)]
    fn reads_parked(&self) -> usize {
        locked(&self.state)
            .asked
            .iter()
            .filter(|asked| asked.parked)
            .count()
    }

    /// How many verbs wait their turn now.
    #[cfg(test)]
    fn verbs_waiting(&self) -> usize {
        locked(&self.state).verbs_waiting
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
    fn let_go(&mut self, gone: &impl Fn(&GitRequest) -> bool) {
        let numbers: Vec<Minted> = self
            .held
            .iter()
            .filter(|(_, (_, request))| gone(request))
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
        let ledger = Arc::clone(share.queue.ledger());
        let turns = Arc::clone(&self.turns);
        let wake = Arc::clone(&self.wake);
        let held = match share.queue.ask(
            request.question.clone(),
            asker,
            |_, _| Relation::Other,
            |ran| {
                self.executor
                    .start_worker("bt-git-mutation", ran, move |ctx| {
                        let mut door = make(ctx);
                        queue.serve(
                            ctx,
                            |ctx, question| {
                                Ok(turns.verb_turn(ctx, question.folder(), &ledger, || {
                                    door(ctx, &question)
                                }))
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
    /// below its drain mark is taken, share by share, each in mint order. A share that holds
    /// nothing **and owes nothing** is let go (its worker returns, owing nothing): a let-go request
    /// still running or in line is still owed, and its share — its one worker, and its place in the
    /// turnstile — stays until the request's terminal has reached the sink, whose wake brings the
    /// drain that lets it go. One lock; never waits for a worker.
    pub(crate) fn ended(&self, gone: impl Fn(&GitRequest) -> bool) -> Vec<Ended> {
        let mut shares = locked(&self.shares);
        let mut ended = Vec::new();
        for share in &mut shares.live {
            share.let_go(&gone);
            share.ended(&mut ended);
        }
        let (idle, live): (Vec<Share>, Vec<Share>) = std::mem::take(&mut shares.live)
            .into_iter()
            .partition(|share| {
                share.held.is_empty() && share.queue.ledger().earliest_owed().is_none()
            });
        shares.live = live;
        drop(shares);
        for share in idle {
            self.turns.forget(share.queue.ledger());
            share.queue.close();
        }
        ended
    }

    /// How many shares the lane holds now.
    #[cfg(test)]
    fn live_shares(&self) -> usize {
        locked(&self.shares).live.len()
    }

    /// How many requests wait in line in the share request `id` was minted in, behind the one its
    /// worker runs.
    #[cfg(test)]
    fn in_line_with(&self, id: MutationId) -> usize {
        locked(&self.shares)
            .live
            .iter()
            .filter(|share| share.generation == id.share)
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

/// **Where one repository cache stands this turn**: a docked Files column (its tab and seat), a
/// tab's commit graph (its tab and repository), or a floating Files view (the epoch that minted
/// its window).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Place {
    Column(crate::LeafId),
    Graph { tab: crate::TabId, root: PathBuf },
    Float(crate::float::FloatId),
}

/// **Every repository cache the open windows hold, by its identity**, gathered by the window
/// thread before its drain: each column's, each graph's and each floating Files view's, with the
/// window it stands in, for every window that is not leaving.
///
/// An answer is owed to the cache that asked ([`git::GitCacheId`]), never to the place it asked
/// from, so a Files view docked into a column or popped out into a float while git ran is found
/// where it stands now ([`Self::resolve`]) — the one resolution both the drain's test for a gone
/// page ([`Self::gone`]) and the filing ([`Self::address`], whose host `apply_git_results` files
/// by) read. Only a cache no open surface holds is gone: its view was closed, its window
/// dismissed or its column pointed at another folder.
#[derive(Default)]
pub(crate) struct OpenPages {
    places: std::collections::BTreeMap<git::GitCacheId, (winit::window::WindowId, Place)>,
}

impl OpenPages {
    /// The cache `asker` stands at `place`, in `window`, this turn.
    pub(crate) fn stand(
        &mut self,
        asker: git::GitCacheId,
        window: winit::window::WindowId,
        place: Place,
    ) {
        let earlier = self.places.insert(asker, (window, place));
        debug_assert!(
            earlier.is_none(),
            "one cache stands in one place: {asker:?} was already standing at {earlier:?}"
        );
    }

    /// **The window and the host the cache `asker` is reached under now**, or `None` when no open
    /// surface holds it. `asked` is where the question was asked from: a floating view keeps the
    /// tab a document it asked for goes to (`GitHost::Float`'s `tab`), which is the tab the
    /// question was asked in, wherever the view stood then.
    pub(crate) fn resolve(
        &self,
        asker: git::GitCacheId,
        asked: &git::GitHost,
    ) -> Option<(winit::window::WindowId, git::GitHost)> {
        let (window, place) = self.places.get(&asker)?;
        let host = match place {
            Place::Column(leaf) => git::GitHost::Column(*leaf),
            Place::Graph { tab, root } => git::GitHost::Graph {
                tab: *tab,
                root: root.clone(),
            },
            Place::Float(id) => git::GitHost::Float {
                id: *id,
                tab: asked.tab(),
            },
        };
        Some((*window, host))
    }

    /// Whether the page that asked `request` has gone — no open surface holds its cache; its
    /// receipt is then the lane's sink's.
    pub(crate) fn gone(&self, request: &GitRequest) -> bool {
        self.resolve(request.asker, &request.host).is_none()
    }

    /// **An answer addressed for the window that files it**, or `None` when the cache that asked
    /// is gone (the cancellation, arriving as a dropped answer). A page's answer is addressed to
    /// where its cache stands now. A document ([`GitAnswer::is_document`]) keeps the address it
    /// was asked under: it is filed into the preview pool of the tab it was asked in, which no
    /// move of the view changes.
    pub(crate) fn address(&self, response: GitResponse) -> Option<GitResponse> {
        let (window, host) = self.resolve(response.asker, &response.host)?;
        if response.answer.is_document() {
            return Some(response);
        }
        Some(GitResponse {
            window,
            host,
            ..response
        })
    }
}

/// **The receipt a terminal is, for the page that asked**: git's own receipt when the verb ran
/// (`Done`, or the receipt a request joined to it carries); `Failed` — a panic in its task, which
/// may come after git ran it — is the receipt with [`GitFault::Unfinished`], so the rows undim and
/// the page reads the repository again; `WorkerGone` — a verb still in line when its worker ended,
/// never started — is the receipt with [`GitFault::NotRun`], so the rows undim and the notice says
/// Folio did not run it.
pub(crate) fn receipt(terminal: MutationTerminal, request: GitRequest) -> GitResponse {
    let answer = match terminal {
        Terminal::Done(answer) => answer,
        Terminal::Joined { outcome, .. } => return receipt(*outcome, request),
        Terminal::Failed(_) => git::faulted(&request.question, GitFault::Unfinished),
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
        asker: request.asker,
        answer,
    }
}

/// **The receipt a refusal at the door is**: the request's question with [`GitFault::NotRun`] —
/// `Full` past the bound, `WorkerGone` for a share whose worker has ended, `NotStarted` for a worker
/// the thread door would not start — filed by the page at once, so its rows undim.
pub(crate) fn refusal(refused: &Refused, request: GitRequest) -> GitResponse {
    let why = match refused {
        Refused::Full { .. } => NotRun::Full,
        Refused::Faulted(_) => NotRun::WorkerGone,
        Refused::NotStarted(_) => NotRun::NotStarted,
        Refused::TwinEnded => unreachable!("the git mutation lane joins nothing"),
    };
    GitResponse {
        answer: git::faulted(&request.question, GitFault::NotRun(why)),
        window: request.window,
        host: request.host,
        asker: request.asker,
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
        other => {
            unreachable!("the git mutation lane is asked only writes and checkouts: {other:?}")
        }
    }
}

/// **The lane contract suite's adapter for the git mutation lane** (`crate::lane`): the real
/// [`GitMutationLane`], driven with the suite's scripted executor through its own admission
/// ([`GitMutationLane::ask_through`]) and the product's acceptance — each target is a column
/// whose repository cache stands in its tab while it is open ([`OpenPages`]), the drain lets go
/// of a closed target's requests ([`GitMutationLane::ended`]) and each receipt is taken by the
/// column its cache stands in ([`OpenPages::resolve`]), as the application's drain does.
///
/// Every question is a `Stage` of one file in one repository — the file named by the question,
/// in Chinese and Latin — and the door is a fake that tells the suite's gate which question it
/// executes (`Gate::execute`), inside the request's task, then answers that the write went
/// through: no `git` is started.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::git::{GitCacheId, GitHost, GitWriteVerb};
    use crate::lane::{
        Admission, Contract, Delivered, GIT_MUTATION, Gate, LaneUnderTest, Outcome, WakeProbe,
    };
    use crate::{LeafId, SeatId, TabId, WindowId};

    struct Adapter {
        lane: GitMutationLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        /// Each target's page, by the identity of its repository cache.
        opened: BTreeMap<u32, GitCacheId>,
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

    /// The column a target's page stands in: the first seat of its tab.
    fn column_of(target: u32) -> GitHost {
        GitHost::Column(LeafId {
            tab: tab_of(target),
            seat: SeatId(1),
        })
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
            opened: BTreeMap::new(),
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
            let asker = *self.opened.entry(target).or_insert_with(GitCacheId::mint);
            let request = GitRequest {
                window: adapter_window(),
                host: column_of(target),
                asker,
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
            let mut open = OpenPages::default();
            for (target, asker) in &self.opened {
                if !self.closed.contains(target) {
                    open.stand(
                        *asker,
                        adapter_window(),
                        Place::Column(LeafId {
                            tab: tab_of(*target),
                            seat: SeatId(1),
                        }),
                    );
                }
            }
            self.lane
                .ended(|request| open.gone(request))
                .into_iter()
                .map(|(id, terminal, request)| match &terminal {
                    Terminal::Done(GitAnswer::Write { paths, .. }) => {
                        let target = open
                            .resolve(request.asker, &request.host)
                            .and_then(|(_, host)| u32::try_from(host.tab().0 - 1).ok());
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
    use crate::git::{GitCacheId, GitHost, GitWriteVerb};
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
            gone: impl Fn(&GitRequest) -> bool,
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

    fn nobody(_: &GitRequest) -> bool {
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
            asker: GitCacheId::mint(),
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
            asker: GitCacheId::mint(),
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

    /// **The read worker under a test's eye**: the product's loop ([`git::run_git_worker`]) on a
    /// thread of its own, its channel and its nudge made as the product's are
    /// ([`git::read_inbox`]), each question answered by the test's `answer` while its folder is
    /// marked as being read. Dropped, its channel closes and the worker ends.
    struct Reader {
        asks: Arc<mpsc::Sender<git::GitMail>>,
    }

    impl Reader {
        fn ask(&self, request: GitRequest) {
            self.asks
                .send(git::GitMail::Ask(request))
                .expect("the test's reader is there");
        }
    }

    fn reader(turns: Arc<RepoTurns>, answer: impl FnMut(GitRequest) + Send + 'static) -> Reader {
        let (asks, inbox) = git::read_inbox(&turns);
        bt_platform::spawn_at_priority(
            "git-mutation-lane-test-reader",
            bt_platform::ThreadPriority::Normal,
            move |_| git::run_git_worker(inbox, &turns, answer),
        )
        .expect("the test's reader starts");
        Reader { asks }
    }

    /// The status of `root`, asked by a cache of its own.
    fn status(root: &str) -> GitRequest {
        GitRequest {
            window: WindowId::from(1_u64),
            host: column(1),
            asker: GitCacheId::mint(),
            question: GitQuestion::Status {
                root: PathBuf::from(root),
            },
        }
    }

    /// The refs of `root`, asked by a cache of its own.
    fn refs(root: &str) -> GitRequest {
        GitRequest {
            window: WindowId::from(1_u64),
            host: column(1),
            asker: GitCacheId::mint(),
            question: GitQuestion::Refs {
                root: PathBuf::from(root),
            },
        }
    }

    /// What a read asks, for a log: `status <root>` or `refs <root>`.
    fn read_subject(question: &GitQuestion) -> String {
        match question {
            GitQuestion::Status { root } => format!("status {}", root.display()),
            GitQuestion::Refs { root } => format!("refs {}", root.display()),
            other => format!("{other:?}"),
        }
    }

    /// **A read worker whose answers log** `read <subject>` as each read begins, stand in `hold`
    /// when the read's folder is `held_root`, and are announced, by subject, on the receiver.
    fn logging_reader(
        turns: Arc<RepoTurns>,
        log: Arc<Mutex<Vec<String>>>,
        hold: Arc<Hold>,
        held_root: &str,
    ) -> (Reader, mpsc::Receiver<String>) {
        let held_root = PathBuf::from(held_root);
        let (told, answered) = mpsc::channel();
        let reader = reader(turns, move |request| {
            let subject = read_subject(&request.question);
            locked(&log).push(format!("read {subject}"));
            if same_folder(request.question.folder(), &held_root) {
                hold.pass();
            }
            let _ = told.send(subject);
        });
        (reader, answered)
    }

    /// One status read of `folder` on a read worker of its own: it logs `read` when its turn comes,
    /// and says so on `done`. The reader answered keeps its worker; dropping it ends the worker.
    fn read_on_a_worker(
        turns: Arc<RepoTurns>,
        folder: &str,
        log: Arc<Mutex<Vec<String>>>,
        done: mpsc::Sender<()>,
    ) -> Reader {
        let reader = reader(turns, move |_| {
            locked(&log).push("read".to_owned());
            let _ = done.send(());
        });
        reader.ask(status(folder));
        reader
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
            lane.lane.in_line_with(write) == 1
                || locked(&log)
                    .iter()
                    .any(|line| line.contains("功能/feature"))
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
            Terminal::Done(GitAnswer::Write {
                outcome: Ok(()),
                ..
            })
        ));
        assert!(matches!(
            second,
            Terminal::Done(GitAnswer::Checkout {
                outcome: Ok(()),
                ..
            })
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
        let door = || {
            logging(
                Arc::clone(&log),
                Arc::clone(&hold),
                PathBuf::from("仓库 alpha"),
            )
        };
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
    /// MUTATION: have `ReadTicket::begin` begin every read (ignore `TurnState::read_waits`) — the
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
        let _reader = read_on_a_worker(Arc::clone(lane.lane.turns()), root, Arc::clone(&log), done);
        lane::wait_until("the read waiting its turn, or run", || {
            lane.lane.turns().reads_parked() == 1 || locked(&log).iter().any(|line| line == "read")
        });
        hold.release();
        lane::receive(&read, "the read's turn");
        assert!(matches!(
            lane.terminal(checkout, nobody).0,
            Terminal::Done(_)
        ));
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
        let gone = |request: &GitRequest| request.host == column(2);
        for id in [running, open] {
            let (_, asked) = lane.terminal(id, gone);
            assert_ne!(
                asked.host,
                column(2),
                "a page took the closed tab's receipt"
            );
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
                assert_eq!(
                    paths,
                    &["多余 extra.txt"],
                    "the refused write's own rows undim"
                );
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
    /// MUTATION: in `GitMutationLane::ask_through`, hand `start_worker` an `Ends` that ignores every
    /// end in place of `ran` — no terminal comes within the lane suite's patience.
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
        assert_eq!(
            lane.said()
                .iter()
                .filter(|line| line.contains("WorkerGone"))
                .count(),
            1,
            "the death is said exactly once"
        );
        let log = Arc::new(Mutex::new(Vec::new()));
        let (done, read) = mpsc::channel();
        let _reader = read_on_a_worker(Arc::clone(lane.lane.turns()), root, log, done);
        lane::receive(&read, "a read after the worker's death");
        let next = lane
            .lane
            .ask_through(stage(1, root, "丁 d.md"), through)
            .expect("the next press is a new request");
        assert!(matches!(lane.terminal(next, nobody).0, Terminal::Done(_)));
    }

    /// One status read of `folder` on a read worker of its own, that logs `read start`, stands in
    /// `hold`, then logs `read end` — a read whose git child is still running — and says so on
    /// `done`. The reader answered keeps its worker; dropping it ends the worker.
    fn standing_read(
        turns: Arc<RepoTurns>,
        folder: &str,
        log: Arc<Mutex<Vec<String>>>,
        hold: Arc<Hold>,
        done: mpsc::Sender<()>,
    ) -> Reader {
        let reader = reader(turns, move |_| {
            locked(&log).push("read start".to_owned());
            hold.pass();
            locked(&log).push("read end".to_owned());
            let _ = done.send(());
        });
        reader.ask(status(folder));
        reader
    }

    /// RED (T-GIT-MUTATIONS round 2, B1) — **A closed page's verb still running keeps its
    /// repository's one share: a read of the repository waits for it, and the next press runs
    /// after it, on the same worker.**
    ///
    /// The page in tab 2 stages `草稿 draft.md` and closes while git is at it; the drain lets the
    /// request go to the sink. The share still owes the verb, so it is not let go: a read of the
    /// repository asked now waits, and a press from tab 3 — `终稿 final.md`, after the read — is
    /// minted in the same share (one worker) and runs after the read, which runs after the
    /// closed page's verb. The closed page's receipt reaches the sink once.
    ///
    /// MUTATION: let a share go when it holds nothing, whatever its ledger owes (drop
    /// `&& share.queue.ledger().earliest_owed().is_none()` in `GitMutationLane::ended`) — the next
    /// press is minted in a new share, with a second worker.
    #[test]
    fn a_closed_pages_running_verb_keeps_its_repositorys_one_share_and_turn() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let root = "仓库 repo";
        let door = || logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from(root));
        let closed = lane
            .lane
            .ask_through(stage(2, root, "草稿 draft.md"), door())
            .expect("admitted");
        hold.wait_standing("the closed page's verb running");
        assert!(
            lane.lane
                .ended(|request| request.host == column(2))
                .is_empty(),
            "nothing has ended yet"
        );
        let (done, read) = mpsc::channel();
        let _reader = read_on_a_worker(Arc::clone(lane.lane.turns()), root, Arc::clone(&log), done);
        lane::wait_until("the read waiting its turn, or run", || {
            lane.lane.turns().reads_parked() == 1 || locked(&log).iter().any(|line| line == "read")
        });
        let next = lane
            .lane
            .ask_through(stage(3, root, "终稿 final.md"), door())
            .expect("admitted");
        assert_eq!(
            next.share, closed.share,
            "the next press is minted in the repository's one share, run by its one worker"
        );
        hold.release();
        lane::receive(&read, "the read's turn");
        let gone = |request: &GitRequest| request.host == column(2);
        assert!(matches!(lane.terminal(next, gone).0, Terminal::Done(_)));
        assert_eq!(
            *locked(&log),
            [
                "start 草稿 draft.md",
                "end 草稿 draft.md",
                "read",
                "start 终稿 final.md",
                "end 终稿 final.md",
            ],
            "the read after the closed page's verb, the next press after the read"
        );
        assert_eq!(
            lane.sunk().iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [closed],
            "the closed page's receipt reaches the sink, once"
        );
        assert_eq!(
            lane.lane.live_shares(),
            0,
            "once nothing of it is held or owed, the drain lets the repository's share go"
        );
    }

    /// RED (T-GIT-MUTATIONS round 3, N2) — **A verb whose work panicked is filed as unfinished,
    /// not as never run.**
    ///
    /// A stage of `半途 halfway.md` whose door panics — after git's child may have exited — ends
    /// `Failed`; its receipt is the stage's own question with `GitFault::Unfinished`, so the page
    /// undims its row and reads the repository again.
    ///
    /// MUTATION: map `Terminal::Failed` to `GitFault::NotRun(NotRun::WorkerGone)` in `receipt` —
    /// the panicked stage reads as never run.
    #[test]
    fn a_panicked_verb_is_filed_as_unfinished() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let id = lane
            .lane
            .ask_through(stage(1, "仓库 repo", "半途 halfway.md"), |_| {
                Box::new(|_: &WorkerCtx, _: &GitQuestion| -> GitAnswer {
                    panic!("the door fails after git's child exited, on purpose")
                })
            })
            .expect("admitted");
        let (terminal, asked) = lane.terminal(id, nobody);
        assert!(matches!(terminal, Terminal::Failed(_)), "{terminal:?}");
        let receipt = receipt(terminal, asked);
        assert!(
            matches!(
                &receipt.answer,
                GitAnswer::Write {
                    paths,
                    outcome: Err(GitFault::Unfinished),
                    ..
                } if paths == &["半途 halfway.md"]
            ),
            "the page files it as unfinished: {receipt:?}"
        );
    }

    /// The one window the moving views stand in.
    fn moving_window() -> WindowId {
        WindowId::from(1_u64)
    }

    /// A floating Files view, by its epoch, in tab 1.
    fn float_host(epoch: u64) -> GitHost {
        GitHost::Float {
            id: epoch,
            tab: TabId(1),
        }
    }

    /// A Files view's repository cache, on `仓库 repo`, whose root is known.
    fn files_view_cache() -> git::GitCache {
        git::GitCache::at_root(PathBuf::from("仓库 repo"), git::GitRole::Page)
    }

    /// `question`, asked by `cache` from `host`.
    fn asked_by(cache: &git::GitCache, host: GitHost, question: GitQuestion) -> GitRequest {
        GitRequest {
            window: moving_window(),
            host,
            asker: cache.id(),
            question,
        }
    }

    /// The pages open after a move: only `cache`, standing at `place`.
    fn standing(cache: &git::GitCache, place: Place) -> OpenPages {
        let mut open = OpenPages::default();
        open.stand(cache.id(), moving_window(), place);
        open
    }

    /// **The receipt for `id`, taken and addressed as the application's drain does** — let go
    /// against `open`, made the page's receipt, and addressed to where its cache stands now.
    fn delivered(lane: &mut Watched, id: MutationId, open: &OpenPages) -> GitResponse {
        let (terminal, request) = lane.terminal(id, |request| open.gone(request));
        open.address(receipt(terminal, request))
            .expect("a receipt the drain took is addressed to the cache that asked")
    }

    /// RED (T-GIT-RECEIPTS-FOLLOW-A-MOVED-HOST) — **A Files view docked while its verbs ran
    /// receives their receipts in the column it became: its row undims and its checkout lets go.**
    ///
    /// A floating Files view (epoch 7) stages `说明 notes.md` and presses a checkout of
    /// `功能/feature`; while git is at the first, the float is docked — its cache, with the dimmed
    /// row and the held checkout, now stands in column seat 3 of tab 1, and no float of epoch 7 is
    /// open. Both receipts reach the column: addressed to it, filed into its cache, the row no
    /// longer pending, the checkout no longer held, and nothing in the sink.
    ///
    /// MUTATION: in `OpenPages::resolve`, find the place by the asked host instead of the cache's
    /// identity — the float is gone, so both receipts are sunk and none comes within the lane
    /// suite's patience.
    #[test]
    fn a_receipt_reaches_a_files_view_docked_while_its_verb_ran() {
        let gate = Arc::new(Gate::default());
        let mut lane = watched(Executor::scripted(Arc::clone(&gate)));
        gate.hold();
        let mut cache = files_view_cache();
        let write = cache
            .begin_write(GitWriteVerb::Stage, vec!["说明 notes.md".to_owned()])
            .expect("the row's write is built");
        let checkout = cache
            .begin_checkout("功能/feature".to_owned(), false)
            .expect("the checkout is built");
        let write = lane
            .lane
            .ask_through(asked_by(&cache, float_host(7), write), through)
            .expect("admitted");
        assert!(gate.wait_entered(1), "the write reaches the worker");
        let checkout = lane
            .lane
            .ask_through(asked_by(&cache, float_host(7), checkout), through)
            .expect("admitted");
        // DOCK: the float is wiped and its cache moves into a column.
        let column = LeafId {
            tab: TabId(1),
            seat: SeatId(3),
        };
        let mut docked = cache;
        let open = standing(&docked, Place::Column(column));
        gate.release();
        for id in [write, checkout] {
            let response = delivered(&mut lane, id, &open);
            assert_eq!(
                response.host,
                GitHost::Column(column),
                "addressed to the column the float became"
            );
            assert!(docked.accept(response.answer), "filed into its cache");
        }
        assert!(!docked.write_pending("说明 notes.md"), "the row undims");
        assert_eq!(docked.checkout_pending(), None, "the checkout lets go");
        assert!(lane.sunk().is_empty(), "nothing is sunk: the view is open");
    }

    /// RED (T-GIT-RECEIPTS-FOLLOW-A-MOVED-HOST) — **A Files column popped out into a float while
    /// its verb ran receives the receipt in the float.**
    ///
    /// A column (seat 1 of tab 1) stages `草稿 draft.md`; while git is at it, the column pops out
    /// into a floating window of epoch 9 carrying its cache, and the column is closed. The receipt
    /// is addressed to the float — by its epoch, keeping the tab the question was asked in — and
    /// undims the row.
    ///
    /// MUTATION: in `OpenPages::resolve`, find the place by the asked host instead of the cache's
    /// identity — the column is gone, so the receipt is sunk and none comes within the lane
    /// suite's patience.
    #[test]
    fn a_receipt_reaches_a_files_view_popped_out_while_its_verb_ran() {
        let gate = Arc::new(Gate::default());
        let mut lane = watched(Executor::scripted(Arc::clone(&gate)));
        gate.hold();
        let mut cache = files_view_cache();
        let write = cache
            .begin_write(GitWriteVerb::Stage, vec!["草稿 draft.md".to_owned()])
            .expect("the row's write is built");
        let id = lane
            .lane
            .ask_through(asked_by(&cache, column(1), write), through)
            .expect("admitted");
        assert!(gate.wait_entered(1), "the write reaches the worker");
        // The pop-out: the column's cache moves into a floating window of epoch 9.
        let mut floating = cache;
        let open = standing(&floating, Place::Float(9));
        gate.release();
        let response = delivered(&mut lane, id, &open);
        assert_eq!(
            response.host,
            float_host(9),
            "addressed to the float the column became"
        );
        assert!(floating.accept(response.answer), "filed into its cache");
        assert!(!floating.write_pending("草稿 draft.md"), "the row undims");
        assert!(lane.sunk().is_empty(), "nothing is sunk: the view is open");
    }

    /// RED (T-GIT-MUTATIONS round 3, N3; T-GIT-RECEIPTS-FOLLOW-A-MOVED-HOST) — **A closed Files
    /// view's receipt still reaches the sink, recorded once; an open one's reaches its page.**
    ///
    /// Two floating Files views in one window stage a file each — `浮窗 float-7.md` from epoch 7,
    /// `浮窗 float-8.md` from epoch 8. Epoch 7 is dismissed before the drain, and no surface holds
    /// its cache: its receipt is let go to the sink, once, with git's own receipt. Epoch 8's view
    /// still holds its cache, and its receipt is taken for it.
    ///
    /// MUTATION: have `OpenPages::gone` answer `false` for every request — the closed view's
    /// receipt is taken for a page that is not there, and the sink records nothing.
    #[test]
    fn a_closed_files_views_receipt_is_still_sunk_and_recorded() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let mut closed_cache = files_view_cache();
        let mut open_cache = files_view_cache();
        let stage = |cache: &mut git::GitCache, epoch: u64| {
            let question = cache
                .begin_write(GitWriteVerb::Stage, vec![format!("浮窗 float-{epoch}.md")])
                .expect("the row's write is built");
            asked_by(cache, float_host(epoch), question)
        };
        let closed = lane
            .lane
            .ask_through(stage(&mut closed_cache, 7), through)
            .expect("admitted");
        let live = lane
            .lane
            .ask_through(stage(&mut open_cache, 8), through)
            .expect("admitted");
        // The dismissal: epoch 7's window, and its cache with it, are gone.
        drop(closed_cache);
        let open = standing(&open_cache, Place::Float(8));
        let response = delivered(&mut lane, live, &open);
        assert_eq!(response.host, float_host(8), "the open view takes its own");
        assert!(open_cache.accept(response.answer));
        lane::wait_until("the closed view's receipt in the sink", || {
            lane.lane.ended(|request| open.gone(request));
            !lane.sunk().is_empty()
        });
        let sunk = lane.sunk();
        assert_eq!(
            sunk.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [closed],
            "the closed view's receipt reaches the sink, once"
        );
        assert!(
            matches!(&sunk[0].1, Terminal::Done(GitAnswer::Write { paths, .. }) if paths == &["浮窗 float-7.md"]),
            "with git's own receipt"
        );
    }

    /// RED (T-GIT-RECEIPTS-FOLLOW-A-MOVED-HOST round 2) — **A column closed inside a tab that
    /// stays open has its receipt sunk; the column beside it in the same tab takes its own.**
    ///
    /// Two Files columns stand in tab 1 — seat 1 and seat 2, each with its own cache — and each
    /// stages a file (`左栏 left.md`, `右栏 right.md`). Seat 1 is closed before the drain while the
    /// tab stays open: no surface holds its cache, so its receipt reaches the sink, once, with
    /// git's own receipt, and seat 2's receipt is addressed to seat 2.
    ///
    /// MUTATION: in `OpenPages::resolve`, find the place by the asked host's tab instead of the
    /// cache's identity — seat 1's receipt is taken for the column still standing in its tab, and
    /// the sink records nothing.
    #[test]
    fn a_column_closed_in_a_tab_that_stays_open_has_its_receipt_sunk() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let mut closed_cache = files_view_cache();
        let mut open_cache = files_view_cache();
        let seat = |n: u64| LeafId {
            tab: TabId(1),
            seat: SeatId(n),
        };
        let stage = |cache: &mut git::GitCache, n: u64, path: &str| {
            let question = cache
                .begin_write(GitWriteVerb::Stage, vec![path.to_owned()])
                .expect("the row's write is built");
            asked_by(cache, GitHost::Column(seat(n)), question)
        };
        let closed = lane
            .lane
            .ask_through(stage(&mut closed_cache, 1, "左栏 left.md"), through)
            .expect("admitted");
        let live = lane
            .lane
            .ask_through(stage(&mut open_cache, 2, "右栏 right.md"), through)
            .expect("admitted");
        // Seat 1 closes; its cache goes with it, and tab 1 stays open.
        drop(closed_cache);
        let open = standing(&open_cache, Place::Column(seat(2)));
        let response = delivered(&mut lane, live, &open);
        assert_eq!(
            response.host,
            GitHost::Column(seat(2)),
            "the column still standing takes its own"
        );
        assert!(open_cache.accept(response.answer));
        lane::wait_until("the closed column's receipt in the sink", || {
            lane.lane.ended(|request| open.gone(request));
            !lane.sunk().is_empty()
        });
        let sunk = lane.sunk();
        assert_eq!(
            sunk.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [closed],
            "the closed column's receipt reaches the sink, once"
        );
        assert!(
            matches!(&sunk[0].1, Terminal::Done(GitAnswer::Write { paths, .. }) if paths == &["左栏 left.md"]),
            "with git's own receipt"
        );
    }

    /// RED (T-GIT-RECEIPTS-FOLLOW-A-MOVED-HOST, the read side) — **A read answered after its Files
    /// view docked fills the column the view became; one whose view closed is dropped.**
    ///
    /// A floating Files view (epoch 7) asks its status, and is docked into column seat 3 of tab 1
    /// before the answer comes: the answer is addressed to the column and filed into the cache
    /// there, so the page stops reading instead of waiting for an answer that was dropped. The same
    /// answer, once no surface holds the cache, is dropped.
    ///
    /// MUTATION: in `OpenPages::resolve`, find the place by the asked host instead of the cache's
    /// identity — the docked view's answer is dropped and its status stays pending.
    #[test]
    fn a_read_answer_reaches_a_files_view_docked_while_it_was_read() {
        let mut cache = files_view_cache();
        let status = GitQuestion::Status {
            root: PathBuf::from("仓库 repo"),
        };
        cache.mark_pending(&status);
        let asked = asked_by(&cache, float_host(7), status);
        let answer = GitResponse {
            window: asked.window,
            host: asked.host,
            asker: asked.asker,
            answer: GitAnswer::Status {
                root: PathBuf::from("仓库 repo"),
                outcome: Ok(git::GitStatus::default()),
            },
        };
        let column = LeafId {
            tab: TabId(1),
            seat: SeatId(3),
        };
        let mut docked = cache;
        let open = standing(&docked, Place::Column(column));
        let addressed = open
            .address(answer.clone())
            .expect("the docked view's answer is not dropped");
        assert_eq!(addressed.host, GitHost::Column(column));
        assert!(docked.accept(addressed.answer), "filed into its cache");
        assert!(docked.status().ready().is_some(), "the page stops reading");
        assert_eq!(
            OpenPages::default().address(answer),
            None,
            "an answer whose view closed is dropped"
        );
    }

    /// RED (T-GIT-RECEIPTS-FOLLOW-A-MOVED-HOST, the read side) — **A document asked from a Files
    /// view keeps the pool it was asked for when the view moves.**
    ///
    /// A floating Files view (epoch 7, asked while tab 4 was in front) opens a diff of
    /// `说明 notes.md` and is docked into column seat 3 of tab 1 before the diff comes. The diff
    /// is not the cache's: it keeps the address it was asked under, so it is filed into tab 4's
    /// pool, where its buffer waits — and it is not dropped, because the view that asked is still
    /// open.
    ///
    /// MUTATION: drop the document's early return in `OpenPages::address` — the diff is addressed
    /// to the column in tab 1, a pool its buffer is not in.
    #[test]
    fn a_document_keeps_the_pool_it_was_asked_for_when_its_view_moves() {
        let cache = files_view_cache();
        let asked_from = GitHost::Float {
            id: 7,
            tab: TabId(4),
        };
        let answer = GitResponse {
            window: moving_window(),
            host: asked_from.clone(),
            asker: cache.id(),
            answer: GitAnswer::Diff {
                root: PathBuf::from("仓库 repo"),
                path: "说明 notes.md".to_owned(),
                against: crate::preview::GitDiffAgainst::WorkingTree,
                outcome: Ok(b"diff".to_vec()),
            },
        };
        let docked = cache;
        let open = standing(
            &docked,
            Place::Column(LeafId {
                tab: TabId(1),
                seat: SeatId(3),
            }),
        );
        let addressed = open
            .address(answer)
            .expect("the view that asked is still open");
        assert_eq!(
            addressed.host, asked_from,
            "the document keeps the address it was asked under"
        );
        assert_eq!(addressed.host.tab(), TabId(4), "and with it its pool");
    }

    /// RED (T-GIT-MUTATIONS round 2, B2) — **A read waits for the verbs owed when it was asked,
    /// and not for presses made after it.**
    ///
    /// A stage of `一 one.md` runs; a read of the repository is asked; then `二 two.md` and
    /// `三 three.md` are pressed. The read runs as soon as the first verb has ended, before the two
    /// pressed after it: later presses never lengthen a read's wait.
    ///
    /// MUTATION: have `Asked::behind_a_verb` answer while the share owes anything
    /// (`ledger.earliest_owed().is_some()`, round 1's rule) — the read waits for the later presses,
    /// which wait for the read, and nothing comes within the lane suite's patience.
    #[test]
    fn a_read_waits_for_the_verbs_owed_when_it_was_asked_not_for_later_presses() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let root = "仓库 repo";
        let door = || logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from(root));
        let first = lane
            .lane
            .ask_through(stage(1, root, "一 one.md"), door())
            .expect("admitted");
        hold.wait_standing("the first verb running");
        let (done, read) = mpsc::channel();
        let _reader = read_on_a_worker(Arc::clone(lane.lane.turns()), root, Arc::clone(&log), done);
        lane::wait_until("the read waiting its turn, or run", || {
            lane.lane.turns().reads_parked() == 1 || locked(&log).iter().any(|line| line == "read")
        });
        let later: Vec<MutationId> = ["二 two.md", "三 three.md"]
            .into_iter()
            .map(|path| {
                lane.lane
                    .ask_through(stage(1, root, path), door())
                    .expect("admitted")
            })
            .collect();
        hold.release();
        lane::receive(&read, "the read's turn");
        for id in std::iter::once(first).chain(later) {
            assert!(matches!(lane.terminal(id, nobody).0, Terminal::Done(_)));
        }
        assert_eq!(
            *locked(&log),
            [
                "start 一 one.md",
                "end 一 one.md",
                "read",
                "start 二 two.md",
                "end 二 two.md",
                "start 三 three.md",
                "end 三 three.md",
            ],
            "the read after the verb owed when it was asked, before the presses after it"
        );
    }

    /// RED (T-GIT-MUTATIONS round 2, B3) — **A verb pressed while a read of its repository runs
    /// starts only after the read's answer is sent.**
    ///
    /// A read of the repository is running — its git child not yet done — when a checkout of
    /// `修复/fix` is pressed. The checkout is seen waiting its turn, and its child starts only
    /// after the read has ended: a checkout under a running `status` would tear the reading.
    ///
    /// MUTATION: have `RepoTurns::verb_turn` not wait (drop its `while state.verb_waits(…)` loop)
    /// — the checkout starts while the read stands.
    #[test]
    fn a_verb_waits_for_the_read_running_in_its_repository() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let root = "仓库 repo";
        let (done, read) = mpsc::channel();
        let _reader = standing_read(
            Arc::clone(lane.lane.turns()),
            root,
            Arc::clone(&log),
            Arc::clone(&hold),
            done,
        );
        hold.wait_standing("the read running");
        let checkout = lane
            .lane
            .ask_through(
                checkout(1, root, "修复/fix"),
                logging(
                    Arc::clone(&log),
                    Arc::clone(&hold),
                    PathBuf::from("别处 elsewhere"),
                ),
            )
            .expect("admitted");
        lane::wait_until("the checkout waiting its turn, or started", || {
            lane.lane.turns().verbs_waiting() == 1
                || locked(&log).iter().any(|line| line == "start 修复/fix")
        });
        hold.release();
        lane::receive(&read, "the read's end");
        assert!(matches!(
            lane.terminal(checkout, nobody).0,
            Terminal::Done(_)
        ));
        assert_eq!(
            *locked(&log),
            ["read start", "read end", "start 修复/fix", "end 修复/fix"],
            "the checkout's child started after the read had ended"
        );
    }

    /// RED (T-GIT-MUTATIONS round 2) — **a verb's panic is contained under the resident panic
    /// hook.** The product's hook ends the process for a panic it is not told is contained, before
    /// any catch runs; the lane's door is called inside the seam's task, so the hook's decision
    /// there is yes and a panicking verb is that request's `Failed`, not the end of Folio.
    ///
    /// MUTATION: run both the seam's task and its worker body under `catch_unwind` instead of
    /// `contained_panic::contain` (`lane::task`, `Executor::start_worker`), and the decision at the
    /// door is no.
    #[test]
    fn a_verbs_panic_is_contained_under_the_resident_hook() {
        let (told, heard) = mpsc::channel();
        let told = Mutex::new(told);
        let mut lane = watched(Executor::thread_door(
            bt_platform::ThreadPriority::BelowNormal,
        ));
        let id = lane
            .lane
            .ask_through(stage(1, "仓库 repo", "钩子 hook.md"), move |_| {
                Box::new(move |_: &WorkerCtx, question: &GitQuestion| {
                    let _ = locked(&told).send(crate::the_hook_lets_this_panic_unwind());
                    went_through(question)
                })
            })
            .expect("admitted");
        assert!(
            lane::receive(&heard, "the door's decision"),
            "the door runs where the hook lets a panic unwind"
        );
        assert!(matches!(lane.terminal(id, nobody).0, Terminal::Done(_)));
    }

    /// RED (T-GIT-READS-PER-REPOSITORY) — **A read of another repository is answered while this
    /// one's verb runs.**
    ///
    /// A checkout of `发布/release` stands in git in `仓库 一号`; the status of `仓库 一号` is asked
    /// and waits its turn; the status of `仓库 二号`, asked after it, is answered while the checkout
    /// still stands — the read worker parked the first read and went on. The first is answered
    /// after the checkout.
    ///
    /// MUTATION: have `ReadTicket::begin` wait on the turnstile until the read's turn comes (the
    /// blocking wait: `while state.read_waits(self.read) { wait }`) — the read worker stands in the
    /// first read's wait, and the second is not answered within the lane suite's patience.
    #[test]
    fn a_read_of_another_repository_is_answered_while_this_ones_verbs_run() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let one = "仓库 一号 one";
        let two = "仓库 二号 two";
        let checkout = lane
            .lane
            .ask_through(
                checkout(1, one, "发布/release"),
                logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from(one)),
            )
            .expect("admitted");
        hold.wait_standing("the checkout running");
        let (reader, answered) = logging_reader(
            Arc::clone(lane.lane.turns()),
            Arc::clone(&log),
            Arc::new(Hold::default()),
            "无 none",
        );
        reader.ask(status(one));
        lane::wait_until("the status of 一号 asked and waiting its turn", || {
            lane.lane.turns().reads_parked() == 1
        });
        reader.ask(status(two));
        assert_eq!(
            lane::receive(&answered, "the status of 二号"),
            format!("status {two}")
        );
        assert!(
            !locked(&log).iter().any(|line| line == "end 发布/release"),
            "answered while the checkout in 一号 still stood"
        );
        hold.release();
        assert_eq!(
            lane::receive(&answered, "the status of 一号"),
            format!("status {one}")
        );
        assert!(matches!(
            lane.terminal(checkout, nobody).0,
            Terminal::Done(_)
        ));
        assert_eq!(
            *locked(&log),
            [
                "start 发布/release".to_owned(),
                format!("read status {two}"),
                "end 发布/release".to_owned(),
                format!("read status {one}"),
            ],
            "二号's read beside 一号's checkout; 一号's read after it"
        );
    }

    /// RED (T-GIT-READS-PER-REPOSITORY; the watermark rule kept) — **A parked read runs after the
    /// verbs owed when it was asked, and before a verb pressed after it.**
    ///
    /// `一 one.md` is being staged in `仓库 repo`; its status is asked and parked; `二 two.md` is
    /// pressed after it. The read worker is then kept busy elsewhere (the status of `仓库 三号`
    /// stands) while the first verb ends, so the parked read cannot run yet: the second verb is
    /// seen waiting for it, not starting. Once the worker is free, the parked read runs, then the
    /// second verb.
    ///
    /// MUTATION: have `TurnState::verb_waits` pass over a parked read (`&& !asked.parked`) — the
    /// second verb starts while the read is parked, before it.
    #[test]
    fn a_waiting_read_runs_after_its_watermark_verbs_and_before_later_presses() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let busy = Arc::new(Hold::default());
        busy.hold();
        let root = "仓库 repo";
        let elsewhere = "仓库 三号 three";
        let door = || logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from(root));
        let first = lane
            .lane
            .ask_through(stage(1, root, "一 one.md"), door())
            .expect("admitted");
        hold.wait_standing("the first verb running");
        let (reader, answered) = logging_reader(
            Arc::clone(lane.lane.turns()),
            Arc::clone(&log),
            Arc::clone(&busy),
            elsewhere,
        );
        reader.ask(status(root));
        lane::wait_until("the read asked and waiting its turn", || {
            lane.lane.turns().reads_parked() == 1
        });
        let second = lane
            .lane
            .ask_through(stage(1, root, "二 two.md"), door())
            .expect("admitted");
        reader.ask(status(elsewhere));
        busy.wait_standing("the read worker busy in 三号");
        hold.release();
        assert!(matches!(lane.terminal(first, nobody).0, Terminal::Done(_)));
        lane::wait_until("the second verb waiting its turn, or started", || {
            lane.lane.turns().verbs_waiting() == 1
                || locked(&log).iter().any(|line| line == "start 二 two.md")
        });
        busy.release();
        assert_eq!(
            lane::receive(&answered, "the status of 三号"),
            format!("status {elsewhere}")
        );
        assert_eq!(
            lane::receive(&answered, "the parked status"),
            format!("status {root}")
        );
        assert!(matches!(lane.terminal(second, nobody).0, Terminal::Done(_)));
        assert_eq!(
            *locked(&log),
            [
                "start 一 one.md".to_owned(),
                format!("read status {elsewhere}"),
                "end 一 one.md".to_owned(),
                format!("read status {root}"),
                "start 二 two.md".to_owned(),
                "end 二 two.md".to_owned(),
            ],
            "the parked read after the verb owed when it was asked, before the press after it"
        );
    }

    /// RED (T-GIT-READS-PER-REPOSITORY) — **Reads parked on two repositories keep their order
    /// within each.**
    ///
    /// A checkout stands in `仓库 甲` and another in `仓库 乙`; the status of 甲, the status of 乙,
    /// the refs of 甲 and the refs of 乙 are asked, in that order, and all four are parked. When
    /// the checkouts end, each repository's reads run in the order they were asked.
    ///
    /// MUTATION: have `PendingGitRequests::next_turn` look at the parked reads latest first
    /// (`(0..self.waiting.len()).rev()`) — a repository's refs run before its status.
    #[test]
    fn reads_waiting_on_two_repositories_keep_their_order_within_each() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let jia = "仓库 甲 jia";
        let yi = "仓库 乙 yi";
        let checkouts: Vec<MutationId> = [jia, yi]
            .into_iter()
            .map(|root| {
                lane.lane
                    .ask_through(
                        checkout(1, root, "主线/main"),
                        logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from(root)),
                    )
                    .expect("admitted")
            })
            .collect();
        lane::wait_until("both checkouts running", || {
            locked(&log)
                .iter()
                .filter(|line| line.starts_with("start"))
                .count()
                == 2
        });
        let (reader, answered) = logging_reader(
            Arc::clone(lane.lane.turns()),
            Arc::clone(&log),
            Arc::new(Hold::default()),
            "无 none",
        );
        for request in [status(jia), status(yi), refs(jia), refs(yi)] {
            reader.ask(request);
        }
        lane::wait_until("the four reads asked and waiting their turns", || {
            lane.lane.turns().reads_parked() == 4
        });
        hold.release();
        let mut heard: Vec<String> = (0..4)
            .map(|_| lane::receive(&answered, "a parked read's answer"))
            .collect();
        for id in checkouts {
            assert!(matches!(lane.terminal(id, nobody).0, Terminal::Done(_)));
        }
        heard.sort_by_key(|subject| !subject.ends_with(jia));
        assert_eq!(
            heard,
            [
                format!("status {jia}"),
                format!("refs {jia}"),
                format!("status {yi}"),
                format!("refs {yi}"),
            ],
            "within each repository, its reads in the order they were asked"
        );
    }

    /// RED (T-GIT-READS-PER-REPOSITORY) — **Asks keep coalescing while a read waits its turn.**
    ///
    /// A checkout stands in `仓库 一号` and its status is parked; the read worker is busy with the
    /// status of `仓库 三号` while the same cache asks the status of `仓库 二号` twice. Once the
    /// worker is free it answers 二号 once — the two asks were one — before the status of `仓库 四号`
    /// asked after them, all while the checkout still stands.
    ///
    /// MUTATIONS: (1) have `ReadTicket::begin` wait for the read's turn (the blocking wait) — the
    /// worker never reaches 三号; (2) have `PendingGitRequests::drain_channel` take nothing (the
    /// worker takes one ask at a time) — 二号 is answered twice.
    #[test]
    fn coalescing_keeps_working_while_a_read_waits() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let busy = Arc::new(Hold::default());
        busy.hold();
        let one = "仓库 一号 one";
        let two = "仓库 二号 two";
        let three = "仓库 三号 three";
        let four = "仓库 四号 four";
        let checkout = lane
            .lane
            .ask_through(
                checkout(1, one, "发布/release"),
                logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from(one)),
            )
            .expect("admitted");
        hold.wait_standing("the checkout running");
        let (reader, answered) = logging_reader(
            Arc::clone(lane.lane.turns()),
            Arc::clone(&log),
            Arc::clone(&busy),
            three,
        );
        reader.ask(status(one));
        lane::wait_until("the status of 一号 asked and waiting its turn", || {
            lane.lane.turns().reads_parked() == 1
        });
        reader.ask(status(three));
        busy.wait_standing("the read worker busy in 三号");
        let twice = status(two);
        reader.ask(twice.clone());
        reader.ask(twice);
        busy.release();
        reader.ask(status(four));
        let heard: Vec<String> = (0..3)
            .map(|_| lane::receive(&answered, "an answer"))
            .collect();
        assert_eq!(
            heard,
            [
                format!("status {three}"),
                format!("status {two}"),
                format!("status {four}"),
            ],
            "the two asks of 二号 were one question, answered once"
        );
        assert!(
            !locked(&log).iter().any(|line| line == "end 发布/release"),
            "all while the checkout in 一号 still stood"
        );
        hold.release();
        assert_eq!(
            lane::receive(&answered, "the status of 一号"),
            format!("status {one}")
        );
        assert!(matches!(
            lane.terminal(checkout, nobody).0,
            Terminal::Done(_)
        ));
    }

    /// RED (T-GIT-READS-PER-REPOSITORY) — **A newer ask of a parked read replaces it.**
    ///
    /// A stage stands in `仓库 repo`; a cache's status of the repository is parked, and the same
    /// cache asks it again. The parked read is replaced — one read waits, not two — and the
    /// repository's status is answered once when the verb ends.
    ///
    /// MUTATION: have `PendingGitRequests::push_latest` leave the parked reads alone (drop its
    /// `self.waiting.retain(…)`) — two reads wait, and the status is answered twice.
    #[test]
    fn a_newer_ask_replaces_a_parked_read_of_the_same_question() {
        let mut lane = watched(Executor::scripted(Arc::new(Gate::default())));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hold = Arc::new(Hold::default());
        hold.hold();
        let root = "仓库 repo";
        let elsewhere = "仓库 别处 elsewhere";
        let verb = lane
            .lane
            .ask_through(
                stage(1, root, "说明 notes.md"),
                logging(Arc::clone(&log), Arc::clone(&hold), PathBuf::from(root)),
            )
            .expect("admitted");
        hold.wait_standing("the stage running");
        let (reader, answered) = logging_reader(
            Arc::clone(lane.lane.turns()),
            Arc::clone(&log),
            Arc::new(Hold::default()),
            "无 none",
        );
        let read = status(root);
        reader.ask(read.clone());
        lane::wait_until("the status asked and waiting its turn", || {
            lane.lane.turns().reads_parked() == 1
        });
        reader.ask(read);
        // Asked after the repeat, answered after the worker has taken it.
        reader.ask(status(elsewhere));
        assert_eq!(
            lane::receive(&answered, "the status elsewhere"),
            format!("status {elsewhere}")
        );
        assert_eq!(
            lane.lane.turns().reads_parked(),
            1,
            "the repeat replaced the parked read"
        );
        hold.release();
        assert!(matches!(lane.terminal(verb, nobody).0, Terminal::Done(_)));
        assert_eq!(
            lane::receive(&answered, "the repository's status"),
            format!("status {root}")
        );
        reader.ask(status(elsewhere));
        assert_eq!(
            lane::receive(&answered, "the status elsewhere, again"),
            format!("status {elsewhere}"),
            "no second answer of the repository's status before it"
        );
    }
}
