//! **What this machine can start, asked off the window thread** (T-PROGRAMS-REFRESH, 0.4.8 B2;
//! `docs/ARCHITECTURE.md` §4.4 and §5.1's observation lane).
//!
//! One question, four answers: which program each profile row starts on this machine (the
//! `PATH` walk of `profiles::ProfilePrograms::resolve_row`, and what kind of program it found,
//! `profiles::shell_program_of`), what WSL's installation says
//! (`wsl::read_this_machine`, once the walk has found `wsl.exe` or a Store alias), where git is
//! (`profiles::find_git`), and where the agents keep their configuration
//! (`attention_hooks::AgentHomes`, T-FRESH-FACTS — the same environment, four variables). Until this lane the first was asked on the window thread at launch and
//! at every table edit, and all three were asked once and kept for the life of the process, so a
//! program installed while Folio ran stayed missing from the new-tab menu, `Split with`, the
//! default profile, the Agents page and the Git page until a restart.
//!
//! **The walk reads the environment a pane is born with**: the current logon block
//! (`bt_platform::environment::fresh_logon_environment`, the door T-ENV-REFRESH's births use), not
//! this process's launch environment — an installer's new `PATH` entry is in the first and never
//! in the second.
//!
//! **The contract** (`crate::lane`'s shape; measured cost in DESIGN 2026-10-08), **built on the
//! completion seam** (`crate::lane`; the side-effects contract's C1–C7, S-4): every request's
//! identity is minted by the lane's [`crate::lane::Ledger`], or the request is refused at the door
//! (the ledger owes [`BOUND`], or the worker has ended); one `program-walk` worker, started by the
//! first request through a [`crate::lane::Executor`] (the thread door's in the product, the
//! suite's scripted one under test) and waiting for the next; one walk out and at most one
//! waiting. A request made while one waits is **joined** to it when it asks about the same rows
//! (`Joined { with }`: one walk answers both, its rows asked first and its urgency kept), and
//! **supersedes** it when it asks about a different table (`Superseded { by }`: the newer table is
//! walked, and the older request's rows asked first and urgency kept); a request made while a walk
//! is out waits for the next walk, because the machine may have changed since that walk read it
//! (said once in `diagnostics.log` when that walk ends). Each walk runs in its task
//! ([`crate::lane::task`]): its end is the operation's `Done`, carrying the walk's number; a panic
//! in it is the request's `Failed`, said once per streak by the ledger, and the worker goes on;
//! the worker's end ends every request still owed `WorkerGone`, faults the lane and wakes the loop.
//! **A walk that failed is not walked again on its own** (revision (b)5, rule 1): the next request
//! — a new trigger below — walks again. Each walk runs in the band its request carries (below);
//! answers are published row by row into a mailbox that keeps the newest answer per row, then the
//! machine facts, then the walk's terminal; the event loop is woken after the walk's terminal and
//! after each row the request asked for first, and never before the publication it is woken for.
//! The window thread takes the terminals in the order the requests were made and adopts the rows
//! between frames, refusing an answer older than the one it holds for the same row
//! (`ProfilePrograms::adopt`). No request waits, and nothing here has a deadline: a slow `PATH`
//! entry makes the rows behind it late, and late answers are used.
//!
//! **Rows the default's rule reads are answered first** (the request's `first`, from
//! `profiles::default_chain` and any pane waiting for its row), so a walk held up behind one slow
//! entry has already answered what a launch's panes need.
//!
//! **A walk a pane waits on runs at normal priority, every other walk below normal** (RULES 53's
//! exception for this lane). A pane in birth is waiting for a walk's answer, and on a cold machine
//! that walk is not short — measured on a clean guest after boot, 1.2–2 s of `PATH` metadata while
//! the antivirus scans the freshly unpacked program at normal priority (DESIGN 2026-10-08) — so it
//! does not stand behind that work. A request is urgent when its trigger is one a pane waits on
//! (the launch, whose panes are made from its answer, and a pane's birth that found its row
//! unknown — [`Trigger::urgent`]) or when any pane is in birth as it is made ([`PaneWaits`], held
//! by every pane in birth): a table edit or a broadcast then answers a waiting pane too. The
//! opportunistic walks — a menu or page opening, the broadcast — with no pane waiting only refresh
//! what is shown and stay out of the frame's way. A request that replaces one not yet started
//! keeps the more urgent band of the two, as it keeps its rows asked first; and a walk already out
//! below normal is raised after its next row when a pane starts waiting, because that pane lands on
//! its answer. The one worker sets its own band (`bt_platform::set_current_thread_priority`); no
//! second thread and no wait.
//!
//! **Who asks** (the triggers, and there is no timer): the launch; a table edit; a menu that lists
//! programs opening (the new-tab menu, a pane's menu, a terminal's menu); the Profiles, Agents
//! and Git pages opening; Windows saying a setting moved (`WM_SETTINGCHANGE`, the one listener
//! `SystemSettingsWatch` already has); and a pane's birth that finds a row it needs unknown.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use bt_platform::admission::WorkerCtx;
use bt_pty::ShellEnvironment;

use crate::attention_hooks::AgentHomes;
use crate::lane::{self, Coalescing, Executor, Ledger, Refused, Saying, Terminal, Waiting};
use crate::profiles::{self, Profile, ProfilePrograms, RowVerdict};
use crate::wsl::WslFacts;

/// **How many requests the lane's ledger owes at once**: the walk out, the one waiting and every
/// request joined to it, and every terminal the window thread has not taken yet. Each request is
/// a launch, a table edit, a menu or page opening, a broadcast, or a pane in birth asking for its
/// row — a restored session's panes each ask once — and the window thread takes the terminals at
/// every wake. The bound makes a walk stuck inside a slow `PATH` entry an accounted-for set of
/// requests, not an unbounded one; a request past it is refused at the door, and the walk already
/// standing walks every row it would have asked about.
pub(crate) const BOUND: usize = 64;

/// The lane's name in the lines its ledger says.
const LANE_NAME: &str = "program walk";

/// **One request for a walk**, its identity minted by the lane's ledger and rising with every
/// request — the number a walk's answers carry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct ProgramsId(u64);

impl From<u64> for ProgramsId {
    fn from(minted: u64) -> Self {
        Self(minted)
    }
}

/// What a walk request is owed: the application asked it and holds it.
type ProgramsOwed = lane::ToTheApplication<u64, Infallible>;

/// How one walk request ended: `Done` carries the number of the walk that answered it.
pub(crate) type ProgramsTerminal = Terminal<ProgramsId, u64, Infallible>;

/// **Why a walk was asked for** — said in the diagnostics line and the perf trace, and nothing
/// decides on it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trigger {
    Launch,
    TableChanged,
    ProgramMenu,
    ProfilesPage,
    AgentsPage,
    GitPage,
    Environment,
    PathDirectory,
    Birth,
}

impl Trigger {
    const fn name(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::TableChanged => "profile table changed",
            Self::ProgramMenu => "program menu opened",
            Self::ProfilesPage => "Profiles page opened",
            Self::AgentsPage => "Agents page opened",
            Self::GitPage => "Git page opened",
            Self::Environment => "system setting changed",
            Self::PathDirectory => "PATH directory changed",
            Self::Birth => "a pane waits for its program",
        }
    }

    /// **Whether a pane waits on a walk for this trigger, whatever else is in birth**: the
    /// launch's (its panes are made from the answer) and a birth's (the pane asking is in birth).
    /// Every other trigger is urgent only while a pane is in birth ([`PaneWaits`]).
    #[must_use]
    pub const fn urgent(self) -> bool {
        matches!(self, Self::Launch | Self::Birth)
    }
}

/// The band a walk runs in, from its request's urgency.
const fn band(urgent: bool) -> bt_platform::ThreadPriority {
    if urgent {
        bt_platform::ThreadPriority::Normal
    } else {
        bt_platform::ThreadPriority::BelowNormal
    }
}

/// **One request**: the table as the window thread held it when it asked, the rows to answer
/// first, and why.
#[derive(Clone, Debug)]
pub struct WalkRequest {
    pub rows: Vec<Profile>,
    pub first: Vec<String>,
    pub trigger: Trigger,
}

/// **The machine facts a walk reads beside the rows**, numbered by the walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineFacts {
    pub generation: u64,
    pub wsl: WslFacts,
    pub git: Option<PathBuf>,
    /// Where the agents keep their configuration, out of the same environment
    /// (`attention_hooks::AGENT_HOMES`, T-FRESH-FACTS).
    pub agent_homes: AgentHomes,
    /// The exact program-search variables frozen for this walk, and the eligible directories
    /// derived from that same value on the worker.
    pub program_environment: ProgramEnvironmentSnapshot,
}

/// The program-search environment one completed walk actually used.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramEnvironmentSnapshot {
    pub path: Option<OsString>,
    pub pathext: Option<OsString>,
    pub directories: Vec<PathBuf>,
}

/// **What the window thread finds** when it drains the lane.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Answers {
    /// The newest answer for each row published since the last drain.
    pub verdicts: Vec<RowVerdict>,
    /// The newest machine facts published since the last drain.
    pub facts: Option<MachineFacts>,
    /// Every request the application made that has ended since the last drain, by its number, in
    /// the order the requests were made: each one's terminal, taken once.
    pub(crate) ends: Vec<(u64, ProgramsTerminal)>,
}

impl Answers {
    /// Whether the drain found nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.verdicts.is_empty() && self.facts.is_none() && self.ends.is_empty()
    }

    /// **The newest walk that ran to its end since the last drain** — the number a request's
    /// `Done` carries, or the one it joined. A walk that failed, or a worker that ended, ran to no
    /// end, and asks nothing again on its own (revision (b)5).
    #[must_use]
    pub fn finished(&self) -> Option<u64> {
        self.ends
            .iter()
            .filter_map(|(_, terminal)| lane::answer_of(terminal.clone()))
            .max()
    }
}

/// **The machine a walk asks** — the product's is [`ThisMachine`]; a test hands its own.
pub trait Machine: Send + Sync {
    /// The environment this walk reads programs out of.
    fn environment(&self, ctx: &WorkerCtx, note: &dyn Fn(&str)) -> Box<dyn ShellEnvironment>;
    /// WSL's installation, asked at most once a walk: once the walk has found `wsl.exe`, or for
    /// the first row whose program is among the Store's aliases.
    fn wsl(&self) -> WslFacts;
    /// Classify one absolute PATH directory. Test machines are local unless they say otherwise;
    /// the product delegates the worker-only filesystem question to `bt-platform`.
    fn path_directory(
        &self,
        _ctx: &WorkerCtx,
        _path: &Path,
    ) -> std::io::Result<bt_platform::ProgramPathDirectory> {
        Ok(bt_platform::ProgramPathDirectory::Local)
    }
}

/// **The product's machine**: the current logon environment, `PATH` probed through the real
/// filesystem, WSL's registry keys.
pub struct ThisMachine;

impl Machine for ThisMachine {
    fn environment(&self, ctx: &WorkerCtx, note: &dyn Fn(&str)) -> Box<dyn ShellEnvironment> {
        match bt_platform::environment::fresh_logon_environment(ctx) {
            Ok(Some(block)) => Box::new(BlockEnvironment(block)),
            // No logon block on this platform: a process here inherits its environment, and so
            // does every pane it starts.
            Ok(None) => Box::new(bt_pty::SystemShellEnvironment),
            // The pane birth's own rule (`pty_door::environment_refresh`): a block that cannot be
            // read is said, and the inherited environment is what a pane would get too.
            Err(error) => {
                note(&format!(
                    "program walk: the current environment could not be read ({error}); \
                     programs are looked for in the inherited one"
                ));
                Box::new(bt_pty::SystemShellEnvironment)
            }
        }
    }

    fn wsl(&self) -> WslFacts {
        crate::wsl::read_this_machine()
    }

    fn path_directory(
        &self,
        ctx: &WorkerCtx,
        path: &Path,
    ) -> std::io::Result<bt_platform::ProgramPathDirectory> {
        bt_platform::program_path_directory(ctx, path)
    }
}

/// **An environment block as a [`ShellEnvironment`]**: its variables, and the real filesystem.
struct BlockEnvironment(Vec<(OsString, OsString)>);

impl ShellEnvironment for BlockEnvironment {
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.0
            .iter()
            .find(|(name, _)| same_variable(name, key))
            .map(|(_, value)| value.clone())
    }

    fn is_file(&self, path: &Path) -> bool {
        bt_pty::SystemShellEnvironment.is_file(path)
    }
}

/// A variable's name, compared as Windows compares it — in any case. A logon block exists only
/// where the platform has one (`fresh_logon_environment` answers `None` elsewhere), and that is
/// Windows.
fn same_variable(name: &OsStr, key: &str) -> bool {
    name.to_string_lossy().eq_ignore_ascii_case(key)
}

/// **What one walk cost** — counted, for the perf trace and the diagnostics line.
struct Counting<'a> {
    inner: &'a dyn ShellEnvironment,
    file_questions: std::cell::Cell<u64>,
}

/// Freeze the two search variables so every row and the published snapshot see one environment,
/// even when the fallback environment is the live process environment on a non-Windows host.
struct WalkEnvironment<'a> {
    inner: &'a dyn ShellEnvironment,
    path: Option<OsString>,
    pathext: Option<OsString>,
}

impl ShellEnvironment for WalkEnvironment<'_> {
    fn var_os(&self, key: &str) -> Option<OsString> {
        match key {
            "PATH" => self.path.clone(),
            "PATHEXT" => self.pathext.clone(),
            _ => self.inner.var_os(key),
        }
    }

    fn is_file(&self, path: &Path) -> bool {
        self.inner.is_file(path)
    }
}

impl ShellEnvironment for Counting<'_> {
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.inner.var_os(key)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.file_questions.set(self.file_questions.get() + 1);
        self.inner.is_file(path)
    }
}

/// What a walk publishes, in the order it publishes it.
enum Published {
    /// One row's answer, and whether the event loop is woken for it: only a row the request
    /// asked for first is, so a pane waiting on it is born without waiting for the rest of the
    /// walk; the others are read at the walk's end, which always wakes.
    Row(RowVerdict, bool),
    Facts(MachineFacts),
}

/// **The walk itself**: every row in the request's order, then WSL and git. Answers how many
/// `PATH` entries it read and how many file questions it asked.
///
/// The WSL facts it publishes are the installation it read: after the rows once `wsl.exe` was
/// found, or the one it read earlier for a row's Store alias even when `wsl.exe` was not found
/// (a reading is a fact about this machine either way); the empty installation when it read none.
fn walk(
    machine: &dyn Machine,
    ctx: &WorkerCtx,
    generation: u64,
    request: &WalkRequest,
    publish: &mut dyn FnMut(Published),
    note: &dyn Fn(&str),
) -> (usize, u64) {
    let environment = machine.environment(ctx, note);
    let path = environment.var_os("PATH");
    let pathext = environment.var_os("PATHEXT");
    let frozen = WalkEnvironment {
        inner: environment.as_ref(),
        path: path.clone(),
        pathext: pathext.clone(),
    };
    let counting = Counting {
        inner: &frozen,
        file_questions: std::cell::Cell::new(0),
    };
    let mut wsl_found = false;
    // WSL's installation, read at most once a walk: for the first program among the Store's
    // aliases (`profiles::shell_program_of`), or after the rows once `wsl.exe` was found.
    let installation: std::cell::OnceCell<WslFacts> = std::cell::OnceCell::new();
    let read_installation = || installation.get_or_init(|| machine.wsl()).clone();
    for row in ordered(&request.rows, &request.first) {
        let program = ProfilePrograms::resolve_row(row, &counting);
        wsl_found |= row.id == profiles::WSL_ID && program.is_some();
        let shell = program
            .as_deref()
            .map_or(profiles::ShellProgram::Native, |program| {
                profiles::shell_program_of(
                    profiles::SeedPlatform::of_this_build(),
                    Path::new(program),
                    &counting,
                    &read_installation,
                )
            });
        let asked_first = request.first.contains(&row.id);
        publish(Published::Row(
            RowVerdict {
                generation,
                id: row.id.clone(),
                source: row.program.clone(),
                program,
                shell,
            },
            asked_first,
        ));
    }
    let wsl = if wsl_found {
        read_installation()
    } else {
        installation.get().cloned().unwrap_or_default()
    };
    let git = profiles::find_git(&counting);
    let agent_homes = AgentHomes::in_environment(&|name| counting.var_os(name));
    let mut seen = BTreeSet::new();
    let directories = path
        .as_ref()
        .into_iter()
        .flat_map(|path| std::env::split_paths(path))
        .filter(|directory| directory.is_absolute())
        .filter(|directory| seen.insert(directory.clone()))
        .filter(|directory| match machine.path_directory(ctx, directory) {
            Ok(bt_platform::ProgramPathDirectory::Local) => true,
            Ok(bt_platform::ProgramPathDirectory::Removable) => {
                crate::watch_steward::WATCH_REMOVABLE_VOLUMES
            }
            Ok(bt_platform::ProgramPathDirectory::Remote) | Err(_) => false,
        })
        .collect();
    publish(Published::Facts(MachineFacts {
        generation,
        wsl,
        git,
        agent_homes,
        program_environment: ProgramEnvironmentSnapshot {
            path,
            pathext,
            directories,
        },
    }));
    let path_entries = counting
        .var_os("PATH")
        .map_or(0, |path| std::env::split_paths(&path).count());
    (path_entries, counting.file_questions.get())
}

/// The rows in the order a walk answers them: the ones named in `first`, in that order, then the
/// rest in table order.
fn ordered<'a>(rows: &'a [Profile], first: &[String]) -> Vec<&'a Profile> {
    let mut ordered: Vec<&Profile> = first
        .iter()
        .filter_map(|id| rows.iter().find(|row| &row.id == id))
        .collect();
    for row in rows {
        if !ordered.iter().any(|held| held.id == row.id) {
            ordered.push(row);
        }
    }
    ordered
}

/// What the lane counts beside its requests: the panes waiting on a walk, and the walk out.
#[derive(Default)]
struct Asks {
    /// The walk out now: its number and why.
    out: Option<(u64, Trigger)>,
    /// How many panes are in birth, each waiting on a walk's answer ([`PaneWaits`]).
    panes_waiting: usize,
}

/// **A pane in birth, counted by the lane while it waits** — held by the pane
/// (`PaneBirth::_waits`, kept for its drop) from the moment it is made in birth until it is born
/// or closed. While any is held, every request is urgent and a walk out below normal is raised.
pub struct PaneWaits {
    lane: &'static ProgramsLane,
}

impl std::fmt::Debug for PaneWaits {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PaneWaits")
    }
}

impl Drop for PaneWaits {
    fn drop(&mut self) {
        let mut asks = self.lane.lock();
        debug_assert!(
            asks.panes_waiting > 0,
            "a pane stopped waiting that the lane never counted: every PaneWaits is made by \
             pane_waits, which counts it in"
        );
        asks.panes_waiting -= 1;
    }
}

/// **What a walk standing for the worker is asked to do**: the request — the newest of the ones
/// it answers, with the rows every one of them asked first — and whether it runs at normal
/// priority: its own trigger's urgency or that of a request it joined or replaced.
struct Walk {
    request: WalkRequest,
    urgent: bool,
}

impl Walk {
    /// The same operation: a walk of the same table. The rows asked first and the trigger only
    /// order and name the walk; the answers are the same.
    fn same(waiting: &Self, asked: &Self) -> bool {
        waiting.request.rows == asked.request.rows
    }

    /// The walk standing after `newer` joined or replaced `older`: the newer request's table and
    /// trigger, its rows asked first followed by the older's, and the more urgent band of the two
    /// — a launch joined or replaced before its walk started is still the walk the first panes
    /// wait on.
    fn fold(older: Self, newer: Self) -> Self {
        let mut first = newer.request.first;
        for id in older.request.first {
            if !first.contains(&id) {
                first.push(id);
            }
        }
        Self {
            request: WalkRequest {
                first,
                ..newer.request
            },
            urgent: older.urgent || newer.urgent,
        }
    }
}

/// What the worker has published and the window thread has not drained.
#[derive(Default)]
struct Mailbox {
    verdicts: BTreeMap<String, RowVerdict>,
    facts: Option<MachineFacts>,
}

/// **The lane**: the machine it asks, the requests, the mailbox, the wake.
///
/// A type rather than statics so a test runs a lane of its own, with a machine it wrote and a
/// diagnostics door it reads, beside the product's.
pub struct ProgramsLane {
    machine: Box<dyn Machine>,
    asks: Mutex<Asks>,
    mailbox: Mutex<Mailbox>,
    /// How the lane brings the event loop round once [`Self::install_wake`] has taught it; shared
    /// with the ledger, which wakes it after the worker's end.
    wake: Arc<OnceLock<Box<dyn Fn() + Send + Sync>>>,
    /// The lane's diagnostics door, shared with the ledger.
    note: Arc<dyn Fn(&str) + Send + Sync>,
    executor: Executor,
    requests: Coalescing<ProgramsId, ProgramsOwed, Walk>,
}

impl ProgramsLane {
    /// A lane asking `machine`, saying its diagnostics lines through `note`; its worker is started
    /// through the thread door by the first request.
    pub fn new(
        machine: impl Machine + 'static,
        note: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        let note: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(note);
        let said = Arc::clone(&note);
        Self::start_with(
            Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            machine,
            note,
            move |line: &str| said(line),
        )
    }

    /// A lane whose worker `executor` starts, asking `machine`, saying its own lines through `note`
    /// and its ledger's through `say` (in the product both are `diagnostics.log`). The worker sets its own band for every walk, so the band it is started in lasts
    /// only until its first walk begins.
    fn start_with(
        executor: Executor,
        machine: impl Machine + 'static,
        note: Arc<dyn Fn(&str) + Send + Sync>,
        say: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        let wake: Arc<OnceLock<Box<dyn Fn() + Send + Sync>>> = Arc::new(OnceLock::new());
        let installed = Arc::clone(&wake);
        let ledger = Ledger::new(LANE_NAME, Some(BOUND), Saying::PerStreak, say, move || {
            if let Some(wake) = installed.get() {
                wake();
            }
        });
        Self {
            machine: Box::new(machine),
            asks: Mutex::new(Asks::default()),
            mailbox: Mutex::new(Mailbox::default()),
            wake,
            note,
            executor,
            requests: Coalescing::new(ledger),
        }
    }

    /// Teach the worker how to bring the event loop round. Once; a second call is ignored.
    pub fn install_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.wake.set(Box::new(wake));
    }

    /// **Make a request and see that the worker serves it** — the lane's one admission. Never
    /// waits for a walk: a lock, a ledger entry and a condition-variable signal or — the first
    /// time — a thread start. Answers the request's number, or the ledger's refusal at the door:
    /// it already owes [`BOUND`], or the worker ran and has ended; or the thread door refused to
    /// start the worker for this request (`Refused::NotStarted`, said once per streak), which
    /// leaves the lane as it was, so the next request starts the worker again.
    pub(crate) fn request(&'static self, request: WalkRequest) -> Result<u64, Refused> {
        let asks = self.lock();
        let urgent = request.trigger.urgent() || asks.panes_waiting > 0;
        self.requests
            .ask(
                Walk { request, urgent },
                ProgramsOwed::owed(),
                Walk::same,
                Walk::fold,
                |ran| {
                    self.executor
                        .start_worker("program-walk", ran, move |ctx| run_program_walk(ctx, self))
                },
            )
            .map(|admitted| admitted.id.0)
    }

    /// **Count a pane in birth until the answer is dropped.** One lock; never waits for the worker.
    pub fn pane_waits(&'static self) -> PaneWaits {
        self.lock().panes_waiting += 1;
        PaneWaits { lane: self }
    }

    /// **Everything ended and published since the last drain** — the window thread's acceptance:
    /// first every terminal the lane has ended, in the order the requests were made, then the
    /// mailbox, so the facts a walk published before its terminal are found with it. Never waits
    /// for the worker.
    pub fn take(&self) -> Answers {
        let ends = self
            .requests
            .ended()
            .into_iter()
            .map(|(id, terminal)| (id.0, terminal))
            .collect();
        let mailbox = std::mem::take(&mut *self.mailbox());
        Answers {
            verdicts: mailbox.verdicts.into_values().collect(),
            facts: mailbox.facts,
            ends,
        }
    }

    /// Whether a walk out now has a pane waiting on it beyond what its own request said: a pane in
    /// birth, or an urgent request standing behind it.
    fn a_pane_waits(&self) -> bool {
        self.lock().panes_waiting > 0
            || self
                .requests
                .waiting(|waiting| waiting.is_some_and(|waiting| waiting.op.urgent))
    }

    fn publish(&self, published: Published) {
        let wake = {
            let mut mailbox = self.mailbox();
            match published {
                Published::Row(verdict, wake) => {
                    let newer = mailbox
                        .verdicts
                        .get(&verdict.id)
                        .is_none_or(|held| held.generation <= verdict.generation);
                    if newer {
                        mailbox.verdicts.insert(verdict.id.clone(), verdict);
                    }
                    wake
                }
                Published::Facts(facts) => {
                    if mailbox
                        .facts
                        .as_ref()
                        .is_none_or(|held| held.generation <= facts.generation)
                    {
                        mailbox.facts = Some(facts);
                    }
                    false
                }
            }
        };
        // After the answer is in the mailbox and never before: a wake that raced the publication
        // would send the loop to drain an empty box.
        if wake {
            self.wake();
        }
    }

    fn wake(&self) {
        if let Some(wake) = self.wake.get() {
            wake();
        }
    }

    fn lock(&self) -> MutexGuard<'_, Asks> {
        self.asks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn mailbox(&self) -> MutexGuard<'_, Mailbox> {
        self.mailbox.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// **The lane's worker body**: the next walk standing, walked inside its task in its request's
/// band for the newest request it answers, its rows and facts published as they come, its terminal
/// published, then the wake — and round again, for the life of the lane.
fn run_program_walk(ctx: &WorkerCtx, lane: &ProgramsLane) {
    loop {
        let Waiting {
            operation,
            newest,
            op: Walk {
                request,
                mut urgent,
            },
            ..
        } = lane.requests.next(ctx);
        let generation = newest.0;
        lane.lock().out = Some((generation, request.trigger));
        // The walk's band is its request's. Off Windows a band is requested, not enforced
        // (RULES 53), and the answer is not consulted.
        let _ = bt_platform::set_current_thread_priority(band(urgent));
        let started = Instant::now();
        let walked = lane::task(ctx, |ctx| {
            Ok::<_, Infallible>(walk(
                lane.machine.as_ref(),
                ctx,
                generation,
                &request,
                &mut |published| {
                    lane.publish(published);
                    // A pane that started waiting while this walk was out lands on its answer:
                    // the rest of the walk runs in a waiting pane's band.
                    if !urgent && lane.a_pane_waits() {
                        urgent = true;
                        let _ = bt_platform::set_current_thread_priority(band(true));
                    }
                },
                &*lane.note,
            ))
        });
        let elapsed = started.elapsed();
        // The requests made since the last walk ended and not walked by this one, counted by the
        // coalescing under the lock every admission and the worker's take hold: those the waiting
        // run stands for — the next walk answers them, its operation `Done` and the rest `Joined`
        // to it — and those a newer table displaced, whenever that happened (S-4 round 4).
        lane.lock().out = None;
        let lane::Arrivals {
            waiting,
            superseded,
        } = lane.requests.count_arrivals();
        let terminal = match walked {
            Ok((path_entries, file_questions)) => {
                if crate::entry_doors::installed().perf {
                    crate::diagnostics::stderr_line(format!(
                        "BT_PERF_TRACE program_walk_us={} generation={generation} trigger={:?} \
                         rows={} path_entries={path_entries} file_questions={file_questions} \
                         waiting={waiting} superseded={superseded} band={:?}",
                        elapsed.as_micros(),
                        request.trigger.name(),
                        request.rows.len(),
                        band(urgent),
                    ));
                }
                Terminal::Done(generation)
            }
            Err(failure) => Terminal::Failed(failure),
        };
        if waiting + superseded > 0 {
            (lane.note)(&arrivals_line(
                generation,
                request.trigger,
                elapsed,
                waiting,
                superseded,
            ));
        }
        lane.requests.ledger().publish(operation, terminal);
        // After the terminal is published and never before: a wake that raced it would send the
        // loop to find the walk not ended.
        lane.wake();
    }
}

/// **The line for the requests made since the last walk ended that this walk did not answer**
/// ([`lane::Coalescing::count_arrivals`]) — those the waiting run stands for wait for the next
/// walk, which answers them (its operation `Done`, the rest `Joined` to it); those displaced by a
/// different table ended `Superseded` and are not described as answered by it.
fn arrivals_line(
    generation: u64,
    trigger: Trigger,
    elapsed: Duration,
    waiting: usize,
    superseded: usize,
) -> String {
    format!(
        "program walk {generation} ({}) took {} ms; of the requests made since the last walk \
         ended that this walk did not answer, {waiting} wait for the next walk, \
         {superseded} superseded by a newer table",
        trigger.name(),
        elapsed.as_millis()
    )
}

/// The product's lane, asking this machine and writing its lines to `diagnostics.log`.
static LANE: LazyLock<ProgramsLane> =
    LazyLock::new(|| ProgramsLane::new(ThisMachine, crate::diagnostics::note));

/// [`ProgramsLane::install_wake`] on the product's lane. Once, at startup, before the first
/// request.
pub fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    LANE.install_wake(wake);
}

/// [`ProgramsLane::request`] on the product's lane, over the live table: the rows the default's
/// rule reads first (`stored_default` is `settings.json`'s choice), then `also_first`. A request
/// the lane refuses at its door has that refusal as its outcome and is waited on by nobody. Past
/// the ledger's bound, the walks already owed walk every row it would have asked about; after the
/// worker's end, that end was said once, by the ledger; and when the thread door refused to start
/// the worker, the ledger said so (once per streak), no walk stands, and the next new trigger
/// starts the worker again.
pub fn request(trigger: Trigger, stored_default: &str, also_first: &[String]) {
    let mut first = profiles::default_chain(stored_default);
    for id in also_first {
        if !first.contains(id) {
            first.insert(0, id.clone());
        }
    }
    let _ = LANE.request(WalkRequest {
        rows: profiles::table().profiles().to_vec(),
        first,
        trigger,
    });
}

/// [`ProgramsLane::pane_waits`] on the product's lane.
pub fn pane_waits() -> PaneWaits {
    LANE.pane_waits()
}

/// [`ProgramsLane::take`] on the product's lane.
pub fn take() -> Answers {
    LANE.take()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::lane::{Executor, Gate, Hold, wait_for_a_wake, wake_channel};
    use crate::profiles::{ProgramCandidate, ProgramSource};
    use std::sync::{Arc, mpsc};

    /// **A machine a test writes**: a `PATH`, the files on it, WSL's distributions — all of it
    /// changeable while the lane runs, which is the whole point (a program installed after
    /// launch). `hold` makes the walk wait inside its first file question until the test lets
    /// it go.
    #[derive(Clone, Default)]
    pub(crate) struct FakeMachine {
        state: Arc<Mutex<FakeState>>,
        gate: Arc<Hold>,
    }

    #[derive(Default)]
    struct FakeState {
        path: Vec<PathBuf>,
        path_kinds: BTreeMap<PathBuf, std::io::Result<bt_platform::ProgramPathDirectory>>,
        files: Vec<PathBuf>,
        wsl: WslFacts,
        /// Variables other than `PATH`, as the account has them now.
        variables: Vec<(String, OsString)>,
        /// Every file question asked, in order, across every walk.
        asked: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl FakeMachine {
        pub(crate) fn with_path(path: &[PathBuf]) -> Self {
            let machine = Self::default();
            machine.lock().path = path.to_vec();
            machine
        }

        pub(crate) fn install(&self, file: &Path) {
            self.lock().files.push(file.to_path_buf());
        }

        pub(crate) fn uninstall(&self, file: &Path) {
            self.lock().files.retain(|held| held != file);
        }

        pub(crate) fn set_wsl(&self, wsl: WslFacts) {
            self.lock().wsl = wsl;
        }

        /// The account sets `name` to `value` (`setx`, System Properties).
        pub(crate) fn set_variable(&self, name: &str, value: &str) {
            let mut state = self.lock();
            state.variables.retain(|(held, _)| held != name);
            state.variables.push((name.to_owned(), value.into()));
        }

        pub(crate) fn set_path_kind(
            &self,
            path: &Path,
            kind: std::io::Result<bt_platform::ProgramPathDirectory>,
        ) {
            self.lock().path_kinds.insert(path.to_path_buf(), kind);
        }

        /// From now on every walk stops at its first file question until [`Self::release`].
        pub(crate) fn hold(&self) {
            self.gate.hold();
        }

        /// Waits until a walk stands at its first file question, held there: the walk is out.
        pub(crate) fn wait_held(&self, what: &str) {
            self.gate.wait_standing(what);
        }

        /// Every file question the walks asked, in order.
        pub(crate) fn asked(&self) -> Vec<PathBuf> {
            self.lock().asked.lock().unwrap().clone()
        }

        pub(crate) fn release(&self) {
            self.gate.release();
        }

        fn lock(&self) -> MutexGuard<'_, FakeState> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    struct FakeEnvironment {
        path: OsString,
        variables: Vec<(String, OsString)>,
        files: Vec<PathBuf>,
        gate: Arc<Hold>,
        asked: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl ShellEnvironment for FakeEnvironment {
        fn var_os(&self, key: &str) -> Option<OsString> {
            if key == "PATH" {
                return Some(self.path.clone());
            }
            self.variables
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }

        fn is_file(&self, path: &Path) -> bool {
            self.gate.pass();
            self.asked.lock().unwrap().push(path.to_path_buf());
            self.files.iter().any(|file| file == path)
        }
    }

    impl Machine for FakeMachine {
        fn environment(&self, _ctx: &WorkerCtx, _note: &dyn Fn(&str)) -> Box<dyn ShellEnvironment> {
            let state = self.lock();
            Box::new(FakeEnvironment {
                path: std::env::join_paths(&state.path).unwrap(),
                variables: state.variables.clone(),
                files: state.files.clone(),
                gate: Arc::clone(&self.gate),
                asked: Arc::clone(&state.asked),
            })
        }

        fn wsl(&self) -> WslFacts {
            self.lock().wsl.clone()
        }

        fn path_directory(
            &self,
            _ctx: &WorkerCtx,
            path: &Path,
        ) -> std::io::Result<bt_platform::ProgramPathDirectory> {
            self.lock()
                .path_kinds
                .get(path)
                .map(|result| match result {
                    Ok(kind) => Ok(*kind),
                    Err(error) => Err(std::io::Error::new(error.kind(), error.to_string())),
                })
                .unwrap_or(Ok(bt_platform::ProgramPathDirectory::Local))
        }
    }

    /// A lane of the test's own over `machine`, leaked for the `'static` its worker needs; its
    /// wakes and its diagnostics lines come back on channels.
    pub(crate) fn lane(
        machine: impl Machine + 'static,
    ) -> (
        &'static ProgramsLane,
        mpsc::Receiver<()>,
        mpsc::Receiver<String>,
    ) {
        let (noted, notes) = mpsc::channel();
        let noted = Mutex::new(noted);
        let lane: &'static ProgramsLane =
            Box::leak(Box::new(ProgramsLane::new(machine, move |line: &str| {
                let _ = noted.lock().unwrap().send(line.to_owned());
            })));
        let (woke, wakes) = mpsc::channel();
        let woke = Mutex::new(woke);
        lane.install_wake(move || {
            let _ = woke.lock().unwrap().send(());
        });
        (lane, wakes, notes)
    }

    /// Drain until walk `generation` has finished, gathering everything published on the way.
    pub(crate) fn answers_through(
        lane: &ProgramsLane,
        wakes: &mpsc::Receiver<()>,
        generation: u64,
    ) -> Answers {
        let mut gathered = Answers::default();
        loop {
            let answers = lane.take();
            gathered.verdicts.extend(answers.verdicts);
            if answers.facts.is_some() {
                gathered.facts = answers.facts;
            }
            gathered.ends.extend(answers.ends);
            if gathered
                .finished()
                .is_some_and(|finished| finished >= generation)
            {
                return gathered;
            }
            wait_for_a_wake(wakes, &format!("walk {generation}'s end"));
        }
    }

    /// Drain until request `of` has ended, gathering every terminal taken on the way.
    fn ends_through(
        lane: &ProgramsLane,
        wakes: &mpsc::Receiver<()>,
        of: u64,
    ) -> Vec<(u64, ProgramsTerminal)> {
        let mut ended = Vec::new();
        loop {
            ended.extend(lane.take().ends);
            if ended.iter().any(|(id, _)| *id == of) {
                return ended;
            }
            wait_for_a_wake(wakes, &format!("request {of}'s terminal"));
        }
    }

    /// A program row named for this test, found on `PATH` by its file name — and, for the mixed
    /// script samples CONVENTIONS §三 asks for, titled in Chinese.
    pub(crate) fn row(id: &str, file: &str) -> Profile {
        let mut row = profiles::row_of(profiles::fallback_profile_id())
            .expect("the shipped table has its fallback");
        row.id = id.to_owned();
        row.display_title = format!("工具 {id}");
        row.program = ProgramSource::FirstOf(vec![ProgramCandidate::OnPath {
            name: file.to_owned(),
        }]);
        row
    }

    fn request(rows: &[Profile], trigger: Trigger) -> WalkRequest {
        WalkRequest {
            rows: rows.to_vec(),
            first: Vec::new(),
            trigger,
        }
    }

    /// A folder that is absolute on every platform, named in Chinese; nothing is ever written
    /// there — the fake machine answers every file question.
    pub(crate) fn bin_dir() -> PathBuf {
        std::env::temp_dir().join("工具").join("bin")
    }

    pub(crate) fn bin(file: &str) -> PathBuf {
        bin_dir().join(file)
    }

    /// RED — **a program installed after the first walk is found by the next walk, and a
    /// late request is answered row by row with its number.**
    ///
    /// MUTATION (observed red): `ProgramsLane::serve` returning after its first walk — the frozen
    /// at launch shape; the second request is never walked and the drain times out.
    #[test]
    fn a_program_installed_after_the_first_walk_is_found_by_the_next() {
        let machine = FakeMachine::with_path(&[bin_dir()]);
        let (lane, wakes, _) = lane(machine.clone());
        let rows = [row("rg", "rg.exe")];

        let first = lane
            .request(request(&rows, Trigger::Launch))
            .expect("admitted");
        let answers = answers_through(lane, &wakes, first);
        assert_eq!(answers.verdicts.len(), 1);
        assert_eq!(answers.verdicts[0].program, None, "not installed yet");
        assert_eq!(answers.verdicts[0].generation, first);

        machine.install(&bin("rg.exe"));
        let second = lane
            .request(request(&rows, Trigger::Environment))
            .expect("admitted");
        let answers = answers_through(lane, &wakes, second);
        assert_eq!(
            answers.verdicts[0].program.as_deref(),
            Some(bin("rg.exe").as_os_str()),
            "the second walk asks the machine again"
        );
        assert_eq!(answers.verdicts[0].generation, second);
    }

    /// RED — **WSL and git are read again by every walk** (the "git/WSL refresh rows").
    ///
    /// MUTATION (observed red, each alone): `walk` reading WSL only when `generation == 1` (the
    /// old `OnceLock`) — the second walk still reports no distribution; git located only by the
    /// first walk — the second walk still reports no git.
    #[test]
    fn every_walk_reads_wsl_and_git_again() {
        let machine = FakeMachine::with_path(&[bin_dir()]);
        let (lane, wakes, _) = lane(machine.clone());
        let wsl_row = {
            let mut row = row(profiles::WSL_ID, "wsl.exe");
            row.display_title = "WSL".to_owned();
            row
        };
        let rows = [wsl_row];
        machine.install(&bin("wsl.exe"));

        let first = lane
            .request(request(&rows, Trigger::Launch))
            .expect("admitted");
        let answers = answers_through(lane, &wakes, first);
        let facts = answers.facts.expect("a walk publishes the machine facts");
        assert_eq!(facts.wsl, WslFacts::default());
        assert_eq!(facts.git, None);

        let distributions = crate::wsl::facts_of(&["Ubuntu-24.04", "Debian"], "Debian");
        machine.set_wsl(distributions.clone());
        let git = profiles::git_file_name_on(bt_platform::host_platform());
        machine.install(&bin(git));
        let second = lane
            .request(request(&rows, Trigger::ProfilesPage))
            .expect("admitted");
        let answers = answers_through(lane, &wakes, second);
        let facts = answers.facts.expect("the second walk publishes its facts");
        assert_eq!(facts.generation, second);
        assert_eq!(facts.wsl, distributions);
        assert_eq!(facts.git, Some(bin(git)));
    }

    /// RED (T-FRESH-FACTS) — **an agent folder the account sets after launch is read by the next
    /// walk**, beside the rows, out of the same environment.
    ///
    /// MUTATION (observed red): `walk` publishing `AgentHomes::default()` — the second walk
    /// reports no `CODEX_HOME`.
    #[test]
    fn every_walk_reads_the_agent_folders_again() {
        let machine = FakeMachine::with_path(&[bin_dir()]);
        let (lane, wakes, _) = lane(machine.clone());
        let rows = [row("rg", "rg.exe")];
        let first = lane
            .request(request(&rows, Trigger::Launch))
            .expect("admitted");
        let facts = answers_through(lane, &wakes, first)
            .facts
            .expect("a walk publishes the machine facts");
        assert_eq!(facts.agent_homes, AgentHomes::default());

        machine.set_variable("CODEX_HOME", r"D:\代理\codex home");
        let second = lane
            .request(request(&rows, Trigger::Environment))
            .expect("admitted");
        let facts = answers_through(lane, &wakes, second)
            .facts
            .expect("the second walk publishes its facts");
        assert_eq!(
            facts.agent_homes,
            AgentHomes::in_environment(
                &|name| (name == "CODEX_HOME").then(|| OsString::from(r"D:\代理\codex home"))
            ),
            "the variable set after the first walk is in the second walk's facts"
        );
    }

    /// RED (T-OS-SIGNALS-WIN) — the walk publishes the exact frozen PATH/PATHEXT it used and
    /// admits only distinct, existing local absolute directories under the current policy.
    ///
    /// MUTATIONS: reread either variable at publication; admit remote, removable, failed or
    /// relative entries; the snapshot differs.
    #[test]
    fn a_walk_publishes_its_exact_eligible_program_environment() {
        use bt_platform::ProgramPathDirectory::{Remote, Removable};

        let local = bin_dir();
        let fixture = std::env::temp_dir().join("folio-program-environment");
        let remote = fixture.join("remote");
        let removable = fixture.join("removable");
        let missing = fixture.join("missing");
        let relative = PathBuf::from("relative-bin");
        let entries = vec![
            local.clone(),
            remote.clone(),
            removable.clone(),
            missing.clone(),
            relative,
            local.clone(),
        ];
        let exact_path = std::env::join_paths(&entries).unwrap();
        let machine = FakeMachine::with_path(&entries);
        machine.set_variable("PATHEXT", ".EXE;.Cmd");
        machine.set_path_kind(&remote, Ok(Remote));
        machine.set_path_kind(&removable, Ok(Removable));
        machine.set_path_kind(
            &missing,
            Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        );
        let (lane, wakes, _) = lane(machine);

        let generation = lane
            .request(request(&[], Trigger::Launch))
            .expect("admitted");
        let snapshot = answers_through(lane, &wakes, generation)
            .facts
            .expect("the walk publishes facts")
            .program_environment;
        assert_eq!(snapshot.path, Some(exact_path));
        assert_eq!(snapshot.pathext, Some(OsString::from(".EXE;.Cmd")));
        assert_eq!(snapshot.directories, vec![local]);
    }

    /// RED — **requests made while a walk is out are answered by one later walk, and that is
    /// said once in diagnostics** (the skipped-probe line).
    ///
    /// MUTATION (observed red): `run_program_walk` never saying the line (`if waiting + superseded
    /// > 0` made never true) — no line arrives.
    #[test]
    fn requests_made_while_a_walk_is_out_are_said_once_and_answered_by_the_next_walk() {
        let machine = FakeMachine::with_path(&[bin_dir()]);
        let (lane, wakes, notes) = lane(machine.clone());
        let rows = [row("rg", "rg.exe")];
        machine.hold();
        let held = lane
            .request(request(&rows, Trigger::Launch))
            .expect("admitted");
        // Wait until the worker has taken it, so the next three find it out.
        machine.wait_held("the held walk");
        machine.install(&bin("rg.exe"));
        lane.request(request(&rows, Trigger::ProgramMenu))
            .expect("admitted");
        lane.request(request(&rows, Trigger::ProgramMenu))
            .expect("admitted");
        let newest = lane
            .request(request(&rows, Trigger::Environment))
            .expect("admitted");
        machine.release();

        let answers = answers_through(lane, &wakes, newest);
        assert_eq!(
            answers.verdicts.last().map(|verdict| verdict.generation),
            Some(newest),
            "one walk answers the three, numbered by the newest"
        );
        // The held walk said its line before it published its end, and the newest walk's end has
        // been drained since: the line is there now, with nothing to wait for.
        let line = notes
            .try_recv()
            .expect("the held walk's end says what joined it");
        assert!(
            line.starts_with(&format!("program walk {held} (launch) took ")),
            "{line}"
        );
        assert!(
            line.contains("3 wait for the next walk, 0 superseded by a newer table"),
            "{line}"
        );
        assert!(notes.try_recv().is_err(), "said once, not per request");
    }

    /// RED — **a walk a pane waits on runs at normal priority and every other walk below
    /// normal**, the band read from inside each walk on the one worker. Waited on: the launch's
    /// walk, a birth's walk, any walk asked while a pane is in birth (a table edit), a launch
    /// replaced before its walk started (the replacing request keeps its band), and the rest of a
    /// walk already out below normal when a pane starts waiting. Not waited on: a menu's or page's
    /// walk, the broadcast's, with no pane in birth.
    ///
    /// MUTATIONS (observed red, each alone): `Trigger::urgent` answering `false` for the launch
    /// (walk 1 below normal); the worker spawned below normal and `serve` not setting the band
    /// (walk 1 below normal); `serve` not setting the band per walk (walk 2 stays at the launch's
    /// normal); `request` dropping the replaced request's urgency (walk 4 below normal);
    /// `Trigger::urgent` answering `false` for a birth (walk 6 below normal); `Trigger::urgent`
    /// answering `true` for every trigger (the menu's walk 2 normal); `request` not reading the
    /// panes in birth (the table edit's walk 7 below normal); `PaneWaits::drop` not counting the
    /// pane out (walk 8 normal); `serve` not raising a walk out when a pane starts waiting (walk
    /// 9's second row below normal).
    ///
    /// Each band is expected as the platform reads it back from a thread started in it: on
    /// Windows the band itself; where a band is requested and not enforced (RULES 53) nothing,
    /// and the sequence is then all `None`.
    #[test]
    fn a_walk_a_pane_waits_on_runs_at_normal_priority_and_every_other_walk_below_normal() {
        type Bands = Arc<Mutex<Vec<Option<bt_platform::ThreadPriority>>>>;
        /// The fake machine, the band each walk was in when it asked for its environment, and the
        /// band each file question was asked in.
        struct Banded(FakeMachine, Bands, Bands);
        struct BandedEnvironment(Box<dyn ShellEnvironment>, Bands);
        impl ShellEnvironment for BandedEnvironment {
            fn var_os(&self, key: &str) -> Option<OsString> {
                self.0.var_os(key)
            }
            fn is_file(&self, path: &Path) -> bool {
                self.1
                    .lock()
                    .unwrap()
                    .push(bt_platform::current_thread_priority());
                self.0.is_file(path)
            }
        }
        impl Machine for Banded {
            fn environment(
                &self,
                ctx: &WorkerCtx,
                note: &dyn Fn(&str),
            ) -> Box<dyn ShellEnvironment> {
                self.1
                    .lock()
                    .unwrap()
                    .push(bt_platform::current_thread_priority());
                Box::new(BandedEnvironment(
                    self.0.environment(ctx, note),
                    Arc::clone(&self.2),
                ))
            }
            fn wsl(&self) -> WslFacts {
                self.0.wsl()
            }
        }
        let read_back = |band| {
            bt_platform::spawn_at_priority("band-read-back", band, |_| {
                bt_platform::current_thread_priority()
            })
            .expect("a thread starts")
            .join()
            .expect("it answers")
        };
        let normal = read_back(bt_platform::ThreadPriority::Normal);
        let below = read_back(bt_platform::ThreadPriority::BelowNormal);
        let machine = FakeMachine::with_path(&[bin_dir()]);
        let bands: Bands = Arc::default();
        let questions: Bands = Arc::default();
        let (lane, wakes, _) = lane(Banded(
            machine.clone(),
            Arc::clone(&bands),
            Arc::clone(&questions),
        ));
        let rows = [row("rg", "rg.exe")];
        let banded = || bands.lock().unwrap().clone();
        let walk_for = |trigger| {
            let generation = lane.request(request(&rows, trigger)).expect("admitted");
            answers_through(lane, &wakes, generation);
        };

        walk_for(Trigger::Launch);
        assert_eq!(banded(), [normal], "the launch's walk");
        walk_for(Trigger::ProgramMenu);
        assert_eq!(
            banded(),
            [normal, below],
            "a menu's walk on the same worker"
        );

        // Walk 3 is held out; a launch request waits behind it and a menu's request replaces it.
        machine.hold();
        lane.request(request(&rows, Trigger::ProfilesPage))
            .expect("admitted");
        machine.wait_held("walk 3, held");
        lane.request(request(&rows, Trigger::Launch))
            .expect("admitted");
        let replacing = lane
            .request(request(&rows, Trigger::ProgramMenu))
            .expect("admitted");
        machine.release();
        answers_through(lane, &wakes, replacing);
        walk_for(Trigger::Environment);
        assert_eq!(
            banded(),
            [normal, below, below, normal, below],
            "the replaced launch's urgency is kept by the request that replaced it, and only by it"
        );

        walk_for(Trigger::Birth);
        let in_birth = lane.pane_waits();
        walk_for(Trigger::TableChanged);
        drop(in_birth);
        walk_for(Trigger::TableChanged);
        assert_eq!(
            banded()[5..],
            [normal, normal, below],
            "a birth's walk; a table edit while a pane is in birth; one after it was born"
        );

        // Walk 9, a menu's over two rows, is held in its first question; a pane starts waiting.
        let two = [row("rg", "rg.exe"), row("fd", "fd.exe")];
        questions.lock().unwrap().clear();
        machine.hold();
        let held = lane
            .request(request(&two, Trigger::ProgramMenu))
            .expect("admitted");
        machine.wait_held("walk 9, held in its first question");
        let in_birth = lane.pane_waits();
        machine.release();
        answers_through(lane, &wakes, held);
        drop(in_birth);
        assert_eq!(banded()[8..], [below], "walk 9 started below normal");
        let asked = questions.lock().unwrap().clone();
        assert_eq!(
            [asked.first().copied(), asked.last().copied()],
            [Some(below), Some(normal)],
            "the rest of the walk runs in the waiting pane's band: {asked:?}"
        );
    }

    /// RED — **the rows the default's rule reads are answered before the rest**, and a row asked
    /// for first wakes the loop as soon as it is answered.
    ///
    /// MUTATION (observed red): `ordered` returning the table order — the walk asks about `a.exe`
    /// first.
    #[test]
    fn the_rows_asked_first_are_answered_first() {
        let machine = FakeMachine::with_path(&[bin_dir()]);
        let (lane, wakes, _) = lane(machine.clone());
        let rows = [row("a", "a.exe"), row("b", "b.exe"), row("默认", "c.exe")];
        let generation = lane
            .request(WalkRequest {
                rows: rows.to_vec(),
                first: vec!["默认".to_owned()],
                trigger: Trigger::Birth,
            })
            .expect("admitted");
        answers_through(lane, &wakes, generation);
        let asked = machine.asked();
        assert_eq!(
            asked.first().and_then(|path| path.file_name()),
            Some(OsStr::new("c.exe")),
            "{asked:?}"
        );
    }

    /// **A machine whose first walk dies**: the walk panics as it asks for its environment, and
    /// every later walk asks the fake machine.
    struct DiesOnce(FakeMachine, Arc<Mutex<bool>>);

    impl Machine for DiesOnce {
        fn environment(&self, ctx: &WorkerCtx, note: &dyn Fn(&str)) -> Box<dyn ShellEnvironment> {
            let mut first = self.1.lock().unwrap();
            if *first {
                *first = false;
                drop(first);
                panic!("the walk dies on purpose — 走查中止");
            }
            self.0.environment(ctx, note)
        }

        fn wsl(&self) -> WslFacts {
            self.0.wsl()
        }
    }

    /// **A lane of the test's own on the suite's executor** over `gate`, asking `machine`, its
    /// wakes and its diagnostics lines coming back on channels. Leaked, for the `'static` its
    /// worker needs.
    fn scripted_lane(
        gate: &Arc<Gate>,
        machine: impl Machine + 'static,
    ) -> (
        &'static ProgramsLane,
        mpsc::Receiver<()>,
        mpsc::Receiver<String>,
    ) {
        let (noted, notes) = mpsc::channel();
        let noted = Mutex::new(noted);
        let note: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line: &str| {
            let _ = noted.lock().unwrap().send(line.to_owned());
        });
        let said = Arc::clone(&note);
        let lane: &'static ProgramsLane = Box::leak(Box::new(ProgramsLane::start_with(
            Executor::scripted(Arc::clone(gate)),
            machine,
            note,
            move |line: &str| said(line),
        )));
        let (wake, wakes) = wake_channel();
        lane.install_wake(wake);
        (lane, wakes, notes)
    }

    /// RED (S-4; D-85) — **The requests made during a walk end `Superseded` or `Joined`, and the
    /// one the next walk serves ends `Done`** (revision (b)4).
    ///
    /// A walk is held out; three requests are made behind it. The first waits for the next walk.
    /// The second asks about the same table — the same operation — and joins it. The third asks
    /// about a different table (a row was added): a newer, different operation, so the waiting
    /// request ends `Superseded { by: the third }`, its join carrying that, and the next walk walks
    /// the newer table for the third, which ends `Done` with that walk's number.
    ///
    /// MUTATION: the old silence — `Coalescing::ask` displacing the waiting request without
    /// publishing its `Superseded` — the first two requests behind the walk never end, and the
    /// drain finds only the walk out's terminal and the third's.
    ///
    /// **And the walk out's line distinguishes their fates** (S-4 round 2): one request remains
    /// represented by the newer table's pending walk, while the two requests for the older table
    /// were superseded — never described as answered by the next walk.
    /// MUTATION: the old undifferentiated count (`request` adding one to `superseded` for every
    /// arrival while the walk is out) — the line says 3 were superseded.
    #[test]
    fn requests_made_during_a_walk_end_superseded_or_joined_and_the_served_one_done() {
        let gate = Arc::new(Gate::default());
        let (lane, wakes, notes) = scripted_lane(&gate, FakeMachine::with_path(&[bin_dir()]));
        let one = [row("rg", "rg.exe")];
        let two = [row("rg", "rg.exe"), row("fd", "fd.exe")];
        gate.hold();
        let out = lane
            .request(request(&one, Trigger::Launch))
            .expect("admitted");
        assert!(gate.wait_entered(1), "the walk is out");
        let waiting = lane
            .request(request(&one, Trigger::ProgramMenu))
            .expect("admitted behind the walk out");
        let joined = lane
            .request(request(&one, Trigger::Environment))
            .expect("the same table, joined to it");
        let newer = lane
            .request(request(&two, Trigger::TableChanged))
            .expect("a different table, waiting in its place");
        gate.release();
        let ended = ends_through(lane, &wakes, newer);
        assert_eq!(
            ended,
            [
                (out, Terminal::Done(out)),
                (
                    waiting,
                    Terminal::Superseded {
                        by: ProgramsId(newer)
                    }
                ),
                (
                    joined,
                    Terminal::Joined {
                        with: ProgramsId(waiting),
                        outcome: Box::new(Terminal::Superseded {
                            by: ProgramsId(newer)
                        }),
                    }
                ),
                (newer, Terminal::Done(newer)),
            ]
        );
        assert_eq!(gate.entered(), 2, "two walks for four requests");
        let said: Vec<String> = notes.try_iter().collect();
        assert_eq!(
            said.len(),
            1,
            "the walk out's line, and nothing else: {said:?}"
        );
        assert!(
            said[0].starts_with(&format!("program walk {out} (launch) took ")),
            "{said:?}"
        );
        assert!(
            said[0].contains("1 wait for the next walk, 2 superseded by a newer table"),
            "{said:?}"
        );
    }

    /// RED (S-4 round 4) — **A request displaced between two walks is counted in the next walk's
    /// line.**
    ///
    /// The first walk ends with one request waiting behind it and says so. While the worker is
    /// between that walk's count and its take of the next run (stopped here inside the line's
    /// saying), a request for a different table displaces the waiting one, and a request for the
    /// first table again displaces it in turn. Round 3 counted displacements only on the walk out
    /// (`out`, set after the worker took its run and taken at the walk's end), so the middle
    /// request — displaced while no walk was out — was in no line, and the next walk said nothing.
    ///
    /// MUTATION: the old placement — `Coalescing::next` zeroing the superseded count as it takes
    /// the run, so only what is displaced after the take is counted — the next walk's line is
    /// never said.
    #[test]
    fn a_request_displaced_between_walks_is_counted_in_the_next_walks_line() {
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicBool, Ordering};
        let gate = Arc::new(Gate::default());
        let (noted, notes) = mpsc::channel();
        let (go, going) = mpsc::channel::<()>();
        let noted = Mutex::new(noted);
        let going = Mutex::new(going);
        let first = AtomicBool::new(true);
        let note: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line: &str| {
            let _ = noted.lock().unwrap().send(line.to_owned());
            if first.swap(false, Ordering::SeqCst) {
                crate::lane::receive(&going.lock().unwrap(), "the test's go-ahead");
            }
        });
        let said = Arc::clone(&note);
        let lane: &'static ProgramsLane = Box::leak(Box::new(ProgramsLane::start_with(
            Executor::scripted(Arc::clone(&gate)),
            FakeMachine::with_path(&[bin_dir()]),
            note,
            move |line: &str| said(line),
        )));
        let (wake, wakes) = wake_channel();
        lane.install_wake(wake);
        let one = [row("rg", "rg.exe")];
        let two = [row("rg", "rg.exe"), row("fd", "fd.exe")];
        gate.hold();
        let out = lane
            .request(request(&one, Trigger::Launch))
            .expect("admitted");
        assert!(gate.wait_entered(1), "the walk is out");
        lane.request(request(&one, Trigger::ProgramMenu))
            .expect("admitted behind the walk out");
        gate.release();
        let line = crate::lane::receive(&notes, "the first walk's line");
        assert!(
            line.starts_with(&format!("program walk {out} (launch) took "))
                && line.contains("1 wait for the next walk, 0 superseded by a newer table"),
            "{line}"
        );
        let displaced = lane
            .request(request(&two, Trigger::TableChanged))
            .expect("a different table, displacing the waiting request");
        let newest = lane
            .request(request(&one, Trigger::Environment))
            .expect("the first table again, displacing that one");
        go.send(()).expect("the worker waits in the line");
        let ended = ends_through(lane, &wakes, newest);
        assert!(
            ended.contains(&(
                displaced,
                Terminal::Superseded {
                    by: ProgramsId(newest)
                }
            )),
            "{ended:?}"
        );
        let line = crate::lane::receive(&notes, "the next walk's line");
        assert!(
            line.starts_with(&format!("program walk {newest} ("))
                && line.contains("0 wait for the next walk, 1 superseded by a newer table"),
            "{line}"
        );
    }

    /// RED (S-4 round 2) — **A start the thread door refuses refuses that request at the door,
    /// says so once, and the next request starts the worker** (revision (b)5, rule 1: a new
    /// request tries again; the lane is not in fault, because its worker never ran). In round 1
    /// the request was minted, ended `WorkerGone`, and faulted the lane; now it is refused without
    /// an identity, and the next request walks. A door that refuses twice in a row is one streak,
    /// said once (C3, round 3).
    ///
    /// MUTATION: fault the lane on a refused start (`lane::Ran::worker_ended` handing
    /// `NotStarted` to the ledger) — the ledger says the worker's end beside the refusal (two
    /// lines, observed red at "said once"), and the next request would be refused `Faulted`.
    /// MUTATION: say every refused start (`Ledger::say_not_started` ignoring the streak) — the two
    /// refusals are two lines.
    #[test]
    fn a_refused_spawn_refuses_this_request_and_the_next_request_starts_the_worker() {
        let gate = Arc::new(Gate::default());
        let (lane, wakes, notes) = scripted_lane(&gate, FakeMachine::with_path(&[bin_dir()]));
        let rows = [row("rg", "rg.exe")];
        gate.refuse_next_start();
        gate.refuse_next_start();
        for trigger in [Trigger::Launch, Trigger::Environment] {
            assert!(
                matches!(
                    lane.request(request(&rows, trigger)),
                    Err(crate::lane::Refused::NotStarted(_))
                ),
                "the request whose start was refused is refused at the door"
            );
        }
        assert_eq!(
            lane.requests.ledger().earliest_owed(),
            None,
            "minting nothing"
        );
        let said: Vec<String> = notes.try_iter().collect();
        assert_eq!(said.len(), 1, "said once: {said:?}");
        assert!(said[0].contains(crate::lane::REFUSED_START), "{said:?}");
        let next = lane
            .request(request(&rows, Trigger::ProgramMenu))
            .expect("the next request starts the worker");
        assert_eq!(next, 1, "the refused requests took no number");
        let answers = answers_through(lane, &wakes, next);
        assert_eq!(
            answers.verdicts[0].generation, next,
            "and the worker it started walks"
        );
    }

    /// RED (S-4; D-85) — **A worker that dies ends every request it owes `WorkerGone`, wakes the
    /// loop, says so once, and the next request is refused at the door.**
    ///
    /// Until S-4 a walk that unwound marked the worker gone (`WalkOut`) and the next request
    /// started another: the death was a lane state, not a terminal of the requests it owed. The
    /// suite's gate ends the worker while it holds one walk out and two requests stand behind it.
    ///
    /// MUTATION: leave `Asks::worker` set — `Ledger::worker_ended` returning at once (the death
    /// leaves the lane as it was) — no wake comes, and the wait for it runs out of patience.
    #[test]
    fn a_dead_walk_ends_every_owed_request_worker_gone_and_the_next_request_is_refused() {
        let gate = Arc::new(Gate::default());
        let (lane, wakes, notes) = scripted_lane(&gate, FakeMachine::with_path(&[bin_dir()]));
        let rows = [row("rg", "rg.exe")];
        gate.hold();
        let out = lane
            .request(request(&rows, Trigger::Launch))
            .expect("admitted");
        assert!(gate.wait_entered(1), "the walk is out");
        let waiting = lane
            .request(request(&rows, Trigger::ProgramMenu))
            .expect("admitted behind it");
        let joined = lane
            .request(request(&rows, Trigger::ProgramMenu))
            .expect("and joined to that");
        gate.kill();
        gate.release();
        wait_for_a_wake(&wakes, "the worker's end");
        assert_eq!(
            lane.take().ends,
            [
                (out, Terminal::WorkerGone),
                (waiting, Terminal::WorkerGone),
                (
                    joined,
                    Terminal::Joined {
                        with: ProgramsId(waiting),
                        outcome: Box::new(Terminal::WorkerGone),
                    }
                ),
            ]
        );
        assert!(
            matches!(
                lane.request(request(&rows, Trigger::Environment)),
                Err(crate::lane::Refused::Faulted(
                    crate::lane::WorkerEnd::Panicked(_)
                ))
            ),
            "a request after the worker's end is refused at the door, minting nothing"
        );
        let said: Vec<String> = notes.try_iter().collect();
        assert_eq!(said.len(), 1, "the end is said once: {said:?}");
        assert!(
            said[0].contains("3 request(s) owed end as WorkerGone"),
            "{said:?}"
        );
    }

    /// RED (S-4; revision (b)5, rule 1) — **A dead walk is not asked again on its own.**
    ///
    /// A walk that dies is its request's `Failed`, said once; the window thread takes it, and
    /// nothing is asked in its place — no request is minted, nothing is owed, the worker waits.
    /// The next new input asks again, and that walk answers. (T-FRESH-FACTS round 2 walked a
    /// dead walk again once, on the window thread's word; the contract forbids a new request
    /// scheduled because a terminal failed.)
    ///
    /// MUTATION: the old automatic re-ask — `ProgramsLane::take` asking again in the failed
    /// request's place (round 2's `request_after_death`, moved into the acceptance) — a request
    /// is owed after the take.
    #[test]
    fn a_dead_walk_is_not_asked_again_on_its_own() {
        let gate = Arc::new(Gate::default());
        let machine = FakeMachine::with_path(&[bin_dir()]);
        machine.set_variable("CODEX_HOME", r"D:\代理\codex");
        let (lane, wakes, notes) =
            scripted_lane(&gate, DiesOnce(machine, Arc::new(Mutex::new(true))));
        let rows = [row("rg", "rg.exe")];
        let dead = lane
            .request(request(&rows, Trigger::Launch))
            .expect("admitted");
        wait_for_a_wake(&wakes, "the dead walk's terminal");
        // Every walk from here on is held at the gate: one asked on its own would stay owed.
        gate.hold();
        let answers = lane.take();
        assert!(
            matches!(answers.ends[..], [(id, Terminal::Failed(_))] if id == dead),
            "{answers:?}"
        );
        assert_eq!(answers.finished(), None, "the walk ran to no end");
        assert_eq!(
            lane.requests.ledger().earliest_owed(),
            None,
            "nothing is asked in the dead walk's place"
        );
        assert_eq!(gate.entered(), 1, "and no second walk has begun");
        let said: Vec<String> = notes.try_iter().collect();
        assert_eq!(said.len(), 1, "the failure is said once: {said:?}");
        assert!(said[0].contains("走查中止"), "{said:?}");

        gate.release();
        let next = lane
            .request(request(&rows, Trigger::ProgramMenu))
            .expect("the next new input asks again");
        let facts = answers_through(lane, &wakes, next)
            .facts
            .expect("and that walk publishes the facts the dead one never did");
        assert_eq!(facts.generation, next);
    }

    /// RED — **a walk that dies leaves the lane able to answer**: its request ends `Failed`, the
    /// window thread hears it and the ledger says it once, the worker goes on, and the next
    /// request is answered by it.
    ///
    /// MUTATION: run the walk outside its task in `run_program_walk` (`walk(..)` called bare) —
    /// the panic ends the worker: the request ends `WorkerGone`, and the next one is refused.
    #[test]
    fn a_walk_that_dies_is_reported_and_the_next_request_is_answered() {
        let machine = FakeMachine::with_path(&[bin_dir()]);
        let (noted, notes) = mpsc::channel();
        let noted = Mutex::new(noted);
        let lane: &'static ProgramsLane = Box::leak(Box::new(ProgramsLane::new(
            DiesOnce(machine, Arc::new(Mutex::new(true))),
            move |line: &str| {
                let _ = noted.lock().unwrap().send(line.to_owned());
            },
        )));
        let (wake, wakes) = wake_channel();
        lane.install_wake(wake);
        let rows = [row("rg", "rg.exe")];
        let dead = lane
            .request(request(&rows, Trigger::Launch))
            .expect("admitted");
        let ended = ends_through(lane, &wakes, dead);
        assert!(
            matches!(ended[..], [(id, Terminal::Failed(_))] if id == dead),
            "{ended:?}"
        );
        // Said as the terminal was taken, so it is there now.
        assert!(
            notes
                .try_recv()
                .expect("the death is said")
                .contains("the walk dies on purpose"),
        );
        let next = lane
            .request(request(&rows, Trigger::ProgramMenu))
            .expect("admitted");
        let answers = answers_through(lane, &wakes, next);
        assert_eq!(answers.verdicts[0].generation, next);
    }

    /// RED (T-PANIC-ROWS-HONEST; S-4) — **a walk that panics under the resident panic hook is a
    /// reported death, not the process's end.** The product's hook ends the process for a panic it
    /// is not told is contained; the walk runs inside `lane::task` and the worker's body inside
    /// the executor, both within `contained_panic::contain` (S-4: the seam's containment, which
    /// replaced the program walk's own), so the hook's decision at the panic is yes and the
    /// unwind reaches the task: the request ends `Failed`, which the window thread takes.
    ///
    /// MUTATION: `lane::task` and `Executor::start_worker` without `contained_panic::contain` —
    /// the decision at the panic is no (the process would end there).
    #[test]
    fn a_walk_that_panics_under_the_resident_hook_is_a_reported_death() {
        struct Decides(FakeMachine, Mutex<mpsc::Sender<bool>>);
        impl Machine for Decides {
            fn environment(
                &self,
                _ctx: &WorkerCtx,
                _note: &dyn Fn(&str),
            ) -> Box<dyn ShellEnvironment> {
                let _ = self
                    .1
                    .lock()
                    .unwrap()
                    .send(crate::the_hook_lets_this_panic_unwind());
                panic!("scripted: 程序清点中途失败 the walk fails");
            }
            fn wsl(&self) -> WslFacts {
                self.0.wsl()
            }
        }
        let (told, decision) = mpsc::channel();
        let lane: &'static ProgramsLane = Box::leak(Box::new(ProgramsLane::new(
            Decides(FakeMachine::with_path(&[bin_dir()]), Mutex::new(told)),
            |_: &str| {},
        )));
        let (woke, wakes) = mpsc::channel();
        let woke = Mutex::new(woke);
        lane.install_wake(move || {
            let _ = woke.lock().unwrap().send(());
        });
        let rows = [row("rg", "rg.exe")];
        let dead = lane
            .request(request(&rows, Trigger::Launch))
            .expect("admitted");
        assert!(
            crate::lane::receive(&decision, "the hook's decision inside the walk"),
            "the walk runs where the resident hook lets its panic unwind"
        );
        let ended = ends_through(lane, &wakes, dead);
        assert!(
            matches!(ended[..], [(id, Terminal::Failed(_))] if id == dead),
            "{ended:?}"
        );
    }
}

/// **The lane contract's adapter** (`crate::lane`, `lane_contract_tests`): a [`ProgramsLane`] of
/// its own — the real admission, worker body, ledger, mailbox and take — its worker started by the
/// suite's executor, over a machine whose walk executes the suite's question: each question is a
/// table of one row whose program is a path naming it, so the same question is the same table
/// (joined) and a different one a different table (superseding), and the walk's one file question
/// is where the suite's [`crate::lane::Gate`] counts it and fires a panic planted in it.
/// Acceptance is the product's take: the terminals in the order they were minted.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{Machine, ProgramsId, ProgramsLane, ProgramsTerminal, Trigger, WalkRequest};
    use crate::lane::{
        Admission, Contract, Delivered, Executor, Gate, LaneUnderTest, Outcome, PROGRAMS, Terminal,
        WakeProbe,
    };
    use crate::profiles::ProgramSource;
    use crate::wsl::WslFacts;
    use bt_platform::admission::WorkerCtx;
    use bt_pty::ShellEnvironment;

    /// The prefix of the path each question's row names.
    const QUESTION: &str = "lane-contract-question-";

    struct Gated(Arc<Gate>);

    impl Machine for Gated {
        fn environment(&self, _ctx: &WorkerCtx, _note: &dyn Fn(&str)) -> Box<dyn ShellEnvironment> {
            Box::new(Questioned(Arc::clone(&self.0)))
        }

        fn wsl(&self) -> WslFacts {
            WslFacts::default()
        }
    }

    /// An environment with no variables, whose one file question per walk — the row's own path —
    /// executes the question that path names.
    struct Questioned(Arc<Gate>);

    impl ShellEnvironment for Questioned {
        fn var_os(&self, _key: &str) -> Option<std::ffi::OsString> {
            None
        }

        fn is_file(&self, path: &Path) -> bool {
            if let Some(question) = path
                .to_str()
                .and_then(|path| path.strip_prefix(QUESTION))
                .and_then(|question| question.parse().ok())
            {
                // The suite's file, held while the walk asks about this path.
                let held = self.0.execute(question);
                drop(held);
            }
            false
        }
    }

    struct ProgramsAdapter {
        lane: &'static ProgramsLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        /// The suite's own record of which question each request carried.
        questions: std::collections::HashMap<u64, u64>,
        /// The lines the ledger said, counted through its `say`.
        said: Arc<AtomicUsize>,
        /// A stale answer the acceptance took, raised at the next drain.
        raised: Vec<Delivered>,
    }

    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let said = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&said);
        let lane: &'static ProgramsLane = Box::leak(Box::new(ProgramsLane::start_with(
            Executor::scripted(Arc::clone(&gate)),
            Gated(Arc::clone(&gate)),
            Arc::new(|_: &str| {}),
            move |_: &str| {
                counted.fetch_add(1, Ordering::SeqCst);
            },
        )));
        let wake = Arc::clone(&probe);
        lane.install_wake(move || wake.woke());
        Box::new(ProgramsAdapter {
            lane,
            gate,
            probe,
            questions: std::collections::HashMap::new(),
            said,
            raised: Vec::new(),
        })
    }

    /// The table a question asks about: the shipped fallback row, its program the path that names
    /// the question.
    fn rows(question: u64) -> Vec<crate::profiles::Profile> {
        let mut row = crate::profiles::row_of(crate::profiles::fallback_profile_id())
            .expect("the shipped table has its fallback");
        row.program = ProgramSource::Path(PathBuf::from(format!("{QUESTION}{question}")));
        vec![row]
    }

    fn outcome(terminal: ProgramsTerminal) -> Outcome {
        match terminal {
            Terminal::Done(_) => Outcome::Answered,
            Terminal::Failed(failure) => Outcome::Failed(failure.to_string()),
            Terminal::Cancelled => Outcome::Cancelled,
            Terminal::Superseded { by } => Outcome::Superseded { by: by.0 },
            Terminal::Joined {
                with,
                outcome: carried,
            } => Outcome::Joined {
                with: with.0,
                carried: Box::new(self::outcome(*carried)),
            },
            Terminal::WorkerGone => Outcome::WorkerGone,
        }
    }

    impl LaneUnderTest for ProgramsAdapter {
        fn contract(&self) -> &'static Contract {
            &PROGRAMS
        }

        fn gate(&self) -> &Gate {
            &self.gate
        }

        fn probe(&self) -> &WakeProbe {
            &self.probe
        }

        /// `programs_lane::request`, on this lane: the lane's own admission, or its refusal at the
        /// door as the request's terminal.
        fn submit(&mut self, _target: u32, question: u64) -> Admission {
            match self.lane.request(WalkRequest {
                rows: rows(question),
                first: Vec::new(),
                trigger: Trigger::ProgramMenu,
            }) {
                Ok(ticket) => {
                    self.questions.insert(ticket, question);
                    Admission {
                        ticket: Some(ticket),
                        question,
                        refused: None,
                    }
                }
                Err(refused) => Admission {
                    ticket: None,
                    question,
                    refused: Some(format!("{refused:?}")),
                },
            }
        }

        fn close_target(&mut self, _target: u32) {
            unreachable!("the program walk's target is the application, which does not close");
        }

        /// The window thread's drain (`FolioApp::adopt_program_walk`): the terminals taken in the
        /// order they were minted, each a delivery.
        fn drain(&mut self) -> Vec<Delivered> {
            let answers = self.lane.take();
            let mut delivered = std::mem::take(&mut self.raised);
            delivered.extend(
                answers
                    .ends
                    .into_iter()
                    .map(|(ticket, terminal)| Delivered {
                        target: Some(0),
                        ticket: Some(ticket),
                        question: self.questions.get(&ticket).copied(),
                        outcome: outcome(terminal),
                    }),
            );
            delivered
        }

        /// A late answer to the older walk `ticket`, through the lane's own publication, which
        /// refuses a second terminal; taking it would be an answer raised. (The acceptance's own
        /// refusal of a row older than the one held is `ProfilePrograms::adopt`'s, pinned by its
        /// tests: the rows the suite walks name paths no live row has, so it takes none of them.)
        fn offer_stale(&mut self, ticket: u64) {
            if self
                .lane
                .requests
                .ledger()
                .publish(ProgramsId(ticket), Terminal::Done(ticket))
            {
                self.raised.push(Delivered {
                    target: Some(0),
                    ticket: Some(ticket),
                    question: None,
                    outcome: Outcome::Answered,
                });
            }
        }

        fn said(&self) -> Option<usize> {
            Some(self.said.load(Ordering::SeqCst))
        }
    }
}
