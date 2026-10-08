//! **What this machine can start, asked off the window thread** (T-PROGRAMS-REFRESH, 0.4.8 B2;
//! `docs/ARCHITECTURE.md` §4.4 and §5.1's observation lane).
//!
//! One question, three answers: which program each profile row starts on this machine (the
//! `PATH` walk of `profiles::ProfilePrograms::resolve_row`), what WSL's installation says
//! (`wsl::read_this_machine`, once the walk has found `wsl.exe`), and where git is
//! (`profiles::find_git`). Until this lane the first was asked on the window thread at launch and
//! at every table edit, and all three were asked once and kept for the life of the process, so a
//! program installed while Folio ran stayed missing from the new-tab menu, `Split with`, the
//! default profile, the Agents page and the Git page until a restart.
//!
//! **The walk reads the environment a pane is born with**: the current logon block
//! (`bt_platform::environment::fresh_logon_environment`, the door T-ENV-REFRESH's births use), not
//! this process's launch environment — an installer's new `PATH` entry is in the first and never
//! in the second.
//!
//! **The contract** (`crate::lane`'s shape; measured cost in DESIGN 2026-10-08): numbered
//! requests; one `program-walk` worker at below-normal priority, started by the first request and
//! waiting for the next; it serves the newest request standing, and the requests made while a
//! walk is out are answered by the next walk (said once in `diagnostics.log` when that walk ends);
//! answers are published row by row into a mailbox that keeps the newest answer per row, then the
//! machine facts, then the walk's end; the event loop is woken after the walk's end and after each
//! row the request asked for first, and never before the publication it is woken for. The
//! window thread adopts them between frames and refuses an answer older than the one it holds for
//! the same row (`ProfilePrograms::adopt`). No request waits, and nothing here has a deadline: a
//! slow `PATH` entry makes the rows behind it late, and late answers are used.
//!
//! **Rows the default's rule reads are answered first** (the request's `first`, from
//! `profiles::default_chain` and any pane waiting for its row), so a walk held up behind one slow
//! entry has already answered what a launch's panes need.
//!
//! **Who asks** (the triggers, and there is no timer): the launch; a table edit; a menu that lists
//! programs opening (the new-tab menu, a pane's menu, a terminal's menu); the Profiles, Agents
//! and Git pages opening; Windows saying a setting moved (`WM_SETTINGCHANGE`, the one listener
//! `SystemSettingsWatch` already has); and a pane's birth that finds a row it needs unknown.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use bt_platform::admission::WorkerCtx;
use bt_pty::ShellEnvironment;

use crate::profiles::{self, Profile, ProfilePrograms, RowVerdict};
use crate::wsl::WslFacts;

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
            Self::Birth => "a pane waits for its program",
        }
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
}

/// **What the window thread finds** when it drains the mailbox.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Answers {
    /// The newest answer for each row published since the last drain.
    pub verdicts: Vec<RowVerdict>,
    /// The newest machine facts published since the last drain.
    pub facts: Option<MachineFacts>,
    /// The newest walk that ran to its end since the last drain.
    pub finished: Option<u64>,
    /// The newest walk whose worker died before its end, since the last drain.
    pub died: Option<u64>,
}

impl Answers {
    /// Whether the drain found nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.verdicts.is_empty()
            && self.facts.is_none()
            && self.finished.is_none()
            && self.died.is_none()
    }
}

/// **The machine a walk asks** — the product's is [`ThisMachine`]; a test hands its own.
pub trait Machine: Send + Sync {
    /// The environment this walk reads programs out of.
    fn environment(&self, ctx: &WorkerCtx, note: &dyn Fn(&str)) -> Box<dyn ShellEnvironment>;
    /// WSL's installation, asked only once the walk has found `wsl.exe`.
    fn wsl(&self) -> WslFacts;
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
fn walk(
    machine: &dyn Machine,
    ctx: &WorkerCtx,
    generation: u64,
    request: &WalkRequest,
    publish: &mut dyn FnMut(Published),
    note: &dyn Fn(&str),
) -> (usize, u64) {
    let environment = machine.environment(ctx, note);
    let counting = Counting {
        inner: environment.as_ref(),
        file_questions: std::cell::Cell::new(0),
    };
    let mut wsl_found = false;
    for row in ordered(&request.rows, &request.first) {
        let program = ProfilePrograms::resolve_row(row, &counting);
        wsl_found |= row.id == profiles::WSL_ID && program.is_some();
        let asked_first = request.first.contains(&row.id);
        publish(Published::Row(
            RowVerdict {
                generation,
                id: row.id.clone(),
                source: row.program.clone(),
                program,
            },
            asked_first,
        ));
    }
    let wsl = if wsl_found {
        machine.wsl()
    } else {
        WslFacts::default()
    };
    let git = profiles::find_git(&counting);
    publish(Published::Facts(MachineFacts {
        generation,
        wsl,
        git,
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

/// The requests, and the worker that serves them.
#[derive(Default)]
struct Asks {
    /// The number of the newest request; counted from `1`.
    requested: u64,
    /// The newest request not yet started, with its number.
    waiting: Option<(u64, WalkRequest)>,
    /// The walk out now: its number, why, and how many requests were made while it was out.
    out: Option<(u64, Trigger, u64)>,
    /// A worker thread is running (started by the first request, waiting for the next).
    worker: bool,
}

/// What the worker has published and the window thread has not drained.
#[derive(Default)]
struct Mailbox {
    verdicts: BTreeMap<String, RowVerdict>,
    facts: Option<MachineFacts>,
    finished: Option<u64>,
    died: Option<u64>,
}

/// **The lane**: the machine it asks, the numbered requests, the mailbox, the wake.
///
/// A type rather than statics so a test runs a lane of its own, with a machine it wrote and a
/// diagnostics door it reads, beside the product's.
pub struct ProgramsLane {
    machine: Box<dyn Machine>,
    asks: Mutex<Asks>,
    asked: Condvar,
    mailbox: Mutex<Mailbox>,
    wake: OnceLock<Box<dyn Fn() + Send + Sync>>,
    note: Box<dyn Fn(&str) + Send + Sync>,
}

impl ProgramsLane {
    /// A lane asking `machine`, saying its diagnostics lines through `note`.
    pub fn new(
        machine: impl Machine + 'static,
        note: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            machine: Box::new(machine),
            asks: Mutex::new(Asks::default()),
            asked: Condvar::new(),
            mailbox: Mutex::new(Mailbox::default()),
            wake: OnceLock::new(),
            note: Box::new(note),
        }
    }

    /// Teach the worker how to bring the event loop round. Once; a second call is ignored.
    pub fn install_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.wake.set(Box::new(wake));
    }

    /// **Number a request and see that the worker serves it.** Never waits for a walk: a lock,
    /// and a condition-variable signal or — the first time, and after a worker died — a thread
    /// start. Answers the request's number.
    pub fn request(&'static self, request: WalkRequest) -> u64 {
        let mut asks = self.lock();
        asks.requested += 1;
        let generation = asks.requested;
        // A request replaced before its walk started keeps the rows it wanted first, behind the
        // newer request's own.
        let mut first = request.first;
        if let Some((_, replaced)) = asks.waiting.take() {
            for id in replaced.first {
                if !first.contains(&id) {
                    first.push(id);
                }
            }
        }
        asks.waiting = Some((generation, WalkRequest { first, ..request }));
        if let Some((_, _, joined)) = asks.out.as_mut() {
            *joined += 1;
        }
        if asks.worker {
            self.asked.notify_one();
            return generation;
        }
        asks.worker = true;
        drop(asks);
        let started = bt_platform::spawn_at_priority(
            "program-walk",
            bt_platform::ThreadPriority::BelowNormal,
            move |ctx| self.serve(ctx),
        );
        if let Err(error) = started {
            // The request stays standing and the next one tries the thread again; nothing on the
            // window thread waits for an answer meanwhile.
            self.lock().worker = false;
            (self.note)(&format!(
                "program walk: its worker would not start ({error}); request {generation} is \
                 served when the next request starts one"
            ));
        }
        generation
    }

    /// **Everything published since the last drain.** One lock; never waits for the worker.
    pub fn take(&self) -> Answers {
        let mut mailbox = self.mailbox();
        let mailbox = std::mem::take(&mut *mailbox);
        Answers {
            verdicts: mailbox.verdicts.into_values().collect(),
            facts: mailbox.facts,
            finished: mailbox.finished,
            died: mailbox.died,
        }
    }

    /// **The worker's whole body**: take the newest request, walk, publish, and wait for the next.
    fn serve(&self, ctx: &WorkerCtx) {
        loop {
            let (generation, request) = {
                let mut asks = self.lock();
                let (generation, request) = loop {
                    if let Some(waiting) = asks.waiting.take() {
                        break waiting;
                    }
                    asks = self
                        .asked
                        .wait(asks)
                        .unwrap_or_else(PoisonError::into_inner);
                };
                asks.out = Some((generation, request.trigger, 0));
                (generation, request)
            };
            // A walk that unwinds leaves the lane able to answer: the worker is marked gone, so
            // the next request starts another, and the window thread is told which walk died.
            let guard = WalkOut {
                lane: self,
                generation,
            };
            let started = Instant::now();
            let (path_entries, file_questions) = walk(
                self.machine.as_ref(),
                ctx,
                generation,
                &request,
                &mut |published| self.publish(published),
                &*self.note,
            );
            std::mem::forget(guard);
            let elapsed = started.elapsed();
            let joined = self.lock().out.take().map_or(0, |(_, _, joined)| joined);
            if std::env::var_os("BT_PERF_TRACE").is_some_and(|value| !value.is_empty()) {
                crate::trace_sink::stderr_line(format!(
                    "BT_PERF_TRACE program_walk_us={} generation={generation} trigger={:?} \
                     rows={} path_entries={path_entries} file_questions={file_questions} \
                     joined={joined}",
                    elapsed.as_micros(),
                    request.trigger.name(),
                    request.rows.len(),
                ));
            }
            if joined > 0 {
                (self.note)(&joined_line(generation, request.trigger, elapsed, joined));
            }
            self.mailbox().finished = Some(generation);
            self.wake();
        }
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

/// **The line for requests that were not walked on their own** — made while a walk was out,
/// they are answered by the next walk, which serves the newest of them.
fn joined_line(generation: u64, trigger: Trigger, elapsed: Duration, joined: u64) -> String {
    format!(
        "program walk {generation} ({}) took {} ms; {joined} request(s) made while it was out \
         were not walked on their own and are answered by the next walk",
        trigger.name(),
        elapsed.as_millis()
    )
}

/// **A walk on its way**: dropped only by an unwind (a finished walk forgets it), when it marks
/// the worker gone and tells the window thread which walk died.
struct WalkOut<'a> {
    lane: &'a ProgramsLane,
    generation: u64,
}

impl Drop for WalkOut<'_> {
    fn drop(&mut self) {
        {
            let mut asks = self.lane.lock();
            asks.worker = false;
            asks.out = None;
        }
        (self.lane.note)(&format!(
            "program walk {} ended before its last answer; rows it had not answered keep the \
             answers they had, and the next request starts the walk again",
            self.generation
        ));
        self.lane.mailbox().died = Some(self.generation);
        self.lane.wake();
    }
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
/// rule reads first (`stored_default` is `settings.json`'s choice), then `also_first`.
pub fn request(trigger: Trigger, stored_default: &str, also_first: &[String]) -> u64 {
    let mut first = profiles::default_chain(stored_default);
    for id in also_first {
        if !first.contains(id) {
            first.insert(0, id.clone());
        }
    }
    LANE.request(WalkRequest {
        rows: profiles::table().profiles().to_vec(),
        first,
        trigger,
    })
}

/// [`ProgramsLane::take`] on the product's lane.
pub fn take() -> Answers {
    LANE.take()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::profiles::{ProgramCandidate, ProgramSource};
    use std::sync::{Arc, mpsc};

    /// **A machine a test writes**: a `PATH`, the files on it, WSL's distributions — all of it
    /// changeable while the lane runs, which is the whole point (a program installed after
    /// launch). `hold` makes the walk wait inside its first file question until the test lets
    /// it go.
    #[derive(Clone, Default)]
    pub(crate) struct FakeMachine {
        state: Arc<Mutex<FakeState>>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    #[derive(Default)]
    struct FakeState {
        path: Vec<PathBuf>,
        files: Vec<PathBuf>,
        wsl: WslFacts,
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

        /// From now on every walk stops at its first file question until [`Self::release`].
        pub(crate) fn hold(&self) {
            *self.gate.0.lock().unwrap() = true;
        }

        /// Every file question the walks asked, in order.
        pub(crate) fn asked(&self) -> Vec<PathBuf> {
            self.lock().asked.lock().unwrap().clone()
        }

        pub(crate) fn release(&self) {
            *self.gate.0.lock().unwrap() = false;
            self.gate.1.notify_all();
        }

        fn lock(&self) -> MutexGuard<'_, FakeState> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    struct FakeEnvironment {
        path: OsString,
        files: Vec<PathBuf>,
        gate: Arc<(Mutex<bool>, Condvar)>,
        asked: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl ShellEnvironment for FakeEnvironment {
        fn var_os(&self, key: &str) -> Option<OsString> {
            (key == "PATH").then(|| self.path.clone())
        }

        fn is_file(&self, path: &Path) -> bool {
            let (held, released) = &*self.gate;
            let mut held = held.lock().unwrap();
            while *held {
                held = released.wait(held).unwrap();
            }
            self.asked.lock().unwrap().push(path.to_path_buf());
            self.files.iter().any(|file| file == path)
        }
    }

    impl Machine for FakeMachine {
        fn environment(&self, _ctx: &WorkerCtx, _note: &dyn Fn(&str)) -> Box<dyn ShellEnvironment> {
            let state = self.lock();
            Box::new(FakeEnvironment {
                path: std::env::join_paths(&state.path).unwrap(),
                files: state.files.clone(),
                gate: Arc::clone(&self.gate),
                asked: Arc::clone(&state.asked),
            })
        }

        fn wsl(&self) -> WslFacts {
            self.lock().wsl.clone()
        }
    }

    /// A lane of the test's own over `machine`, leaked for the `'static` its worker needs; its
    /// wakes and its diagnostics lines come back on channels.
    pub(crate) fn lane(
        machine: FakeMachine,
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
            if answers.died.is_some() {
                gathered.died = answers.died;
            }
            if answers
                .finished
                .is_some_and(|finished| finished >= generation)
            {
                gathered.finished = answers.finished;
                return gathered;
            }
            wait_for_a_wake(wakes, &format!("walk {generation}'s end"));
        }
    }

    /// **One wake, within the lane suite's patience** (`crate::lane::PATIENCE`): a lane that never
    /// wakes fails the test that awaited `what`, by name, rather than hanging it.
    pub(crate) fn wait_for_a_wake(wakes: &mpsc::Receiver<()>, what: &str) {
        if wakes.recv_timeout(crate::lane::PATIENCE).is_err() {
            panic!("no wake within the lane suite's patience while awaiting {what}");
        }
    }

    /// Drain until a walk's death is reported, and answer which walk died.
    fn death_reported(lane: &ProgramsLane, wakes: &mpsc::Receiver<()>, of: u64) -> Option<u64> {
        loop {
            wait_for_a_wake(wakes, &format!("walk {of}'s death"));
            if let Some(died) = lane.take().died {
                return Some(died);
            }
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

        let first = lane.request(request(&rows, Trigger::Launch));
        let answers = answers_through(lane, &wakes, first);
        assert_eq!(answers.verdicts.len(), 1);
        assert_eq!(answers.verdicts[0].program, None, "not installed yet");
        assert_eq!(answers.verdicts[0].generation, first);

        machine.install(&bin("rg.exe"));
        let second = lane.request(request(&rows, Trigger::Environment));
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

        let first = lane.request(request(&rows, Trigger::Launch));
        let answers = answers_through(lane, &wakes, first);
        let facts = answers.facts.expect("a walk publishes the machine facts");
        assert_eq!(facts.wsl, WslFacts::default());
        assert_eq!(facts.git, None);

        let distributions = crate::wsl::facts_of(&["Ubuntu-24.04", "Debian"], "Debian");
        machine.set_wsl(distributions.clone());
        let git = profiles::git_file_name_on(bt_platform::host_platform());
        machine.install(&bin(git));
        let second = lane.request(request(&rows, Trigger::ProfilesPage));
        let answers = answers_through(lane, &wakes, second);
        let facts = answers.facts.expect("the second walk publishes its facts");
        assert_eq!(facts.generation, second);
        assert_eq!(facts.wsl, distributions);
        assert_eq!(facts.git, Some(bin(git)));
    }

    /// RED — **requests made while a walk is out are answered by one later walk, and that is
    /// said once in diagnostics** (the skipped-probe line).
    ///
    /// MUTATION (observed red): `serve` never saying the line (`if joined > 0` made never true) —
    /// no line arrives.
    #[test]
    fn requests_made_while_a_walk_is_out_are_said_once_and_answered_by_the_next_walk() {
        let machine = FakeMachine::with_path(&[bin_dir()]);
        let (lane, wakes, notes) = lane(machine.clone());
        let rows = [row("rg", "rg.exe")];
        machine.hold();
        let held = lane.request(request(&rows, Trigger::Launch));
        // Wait until the worker has taken it, so the next three find it out.
        while lane.lock().out.is_none() {
            std::thread::yield_now();
        }
        machine.install(&bin("rg.exe"));
        lane.request(request(&rows, Trigger::ProgramMenu));
        lane.request(request(&rows, Trigger::ProgramMenu));
        let newest = lane.request(request(&rows, Trigger::Environment));
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
            line.contains("3 request(s) made while it was out were not walked on their own"),
            "{line}"
        );
        assert!(notes.try_recv().is_err(), "said once, not per request");
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
        let generation = lane.request(WalkRequest {
            rows: rows.to_vec(),
            first: vec!["默认".to_owned()],
            trigger: Trigger::Birth,
        });
        answers_through(lane, &wakes, generation);
        let asked = machine.asked();
        assert_eq!(
            asked.first().and_then(|path| path.file_name()),
            Some(OsStr::new("c.exe")),
            "{asked:?}"
        );
    }

    /// RED — **a walk that dies leaves the lane able to answer**: the window thread hears which
    /// walk died, and the next request starts a new worker that answers.
    ///
    /// MUTATION (observed red): `WalkOut::drop` returning at once — no death is reported and the
    /// next request is never served (the worker flag stays set).
    #[test]
    fn a_walk_that_dies_is_reported_and_the_next_request_is_answered() {
        struct Dies(FakeMachine, Arc<Mutex<bool>>);
        impl Machine for Dies {
            fn environment(
                &self,
                ctx: &WorkerCtx,
                note: &dyn Fn(&str),
            ) -> Box<dyn ShellEnvironment> {
                let mut first = self.1.lock().unwrap();
                if *first {
                    *first = false;
                    drop(first);
                    panic!("the walk dies on purpose");
                }
                self.0.environment(ctx, note)
            }
            fn wsl(&self) -> WslFacts {
                self.0.wsl()
            }
        }
        let machine = FakeMachine::with_path(&[bin_dir()]);
        let (noted, notes) = mpsc::channel();
        let noted = Mutex::new(noted);
        let lane: &'static ProgramsLane = Box::leak(Box::new(ProgramsLane::new(
            Dies(machine, Arc::new(Mutex::new(true))),
            move |line: &str| {
                let _ = noted.lock().unwrap().send(line.to_owned());
            },
        )));
        let (woke, wakes) = mpsc::channel();
        let woke = Mutex::new(woke);
        lane.install_wake(move || {
            let _ = woke.lock().unwrap().send(());
        });
        let rows = [row("rg", "rg.exe")];
        let dead = lane.request(request(&rows, Trigger::Launch));
        assert_eq!(death_reported(lane, &wakes, dead), Some(dead));
        // Said before the death was published, so it is there now.
        assert!(
            notes
                .try_recv()
                .expect("the death is said")
                .contains("ended before its last answer")
        );
        let next = lane.request(request(&rows, Trigger::ProgramMenu));
        let answers = answers_through(lane, &wakes, next);
        assert_eq!(answers.verdicts[0].generation, next);
    }
}

/// **The lane contract's adapter** (`crate::lane`, `lane_contract_tests`): a [`ProgramsLane`] of
/// its own — the real numbered requests, worker, mailbox and wake — over a machine whose walk the
/// suite's [`crate::lane::Gate`] can hold. Acceptance is the product's: `ProfilePrograms::adopt`
/// against the live table, which refuses an answer older than the one held.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::sync::Arc;

    use super::{Machine, ProgramsLane, Trigger, WalkRequest};
    use crate::lane::{
        Admission, Contract, Delivered, Gate, LaneUnderTest, Outcome, PROGRAMS, WakeProbe,
    };
    use crate::profiles::{ProfilePrograms, RowVerdict};
    use crate::wsl::WslFacts;
    use bt_platform::admission::WorkerCtx;
    use bt_pty::ShellEnvironment;

    struct Gated(Arc<Gate>);

    impl Machine for Gated {
        fn environment(&self, _ctx: &WorkerCtx, _note: &dyn Fn(&str)) -> Box<dyn ShellEnvironment> {
            self.0.pass(None);
            Box::new(Nothing)
        }

        fn wsl(&self) -> WslFacts {
            WslFacts::default()
        }
    }

    struct Nothing;

    impl ShellEnvironment for Nothing {
        fn var_os(&self, _key: &str) -> Option<std::ffi::OsString> {
            None
        }

        fn is_file(&self, _path: &std::path::Path) -> bool {
            false
        }
    }

    struct ProgramsAdapter {
        lane: &'static ProgramsLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        /// What the window thread holds.
        programs: ProfilePrograms,
        /// The newest walk the consumer has heard from.
        seen: u64,
        /// A stale answer the acceptance took, raised at the next drain.
        raised: Vec<Delivered>,
    }

    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let lane: &'static ProgramsLane = Box::leak(Box::new(ProgramsLane::new(
            Gated(Arc::clone(&gate)),
            |_: &str| {},
        )));
        let wake = Arc::clone(&probe);
        lane.install_wake(move || wake.woke());
        Box::new(ProgramsAdapter {
            lane,
            gate,
            probe,
            programs: ProfilePrograms::unknown(),
            seen: 0,
            raised: Vec::new(),
        })
    }

    /// The one row the suite's walks answer: the shipped fallback, which every table has.
    fn rows() -> Vec<crate::profiles::Profile> {
        vec![
            crate::profiles::row_of(crate::profiles::fallback_profile_id())
                .expect("the shipped table has its fallback"),
        ]
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

        fn submit(&mut self, _target: u32, question: u64) -> Admission {
            let ticket = self.lane.request(WalkRequest {
                rows: rows(),
                first: Vec::new(),
                trigger: Trigger::ProgramMenu,
            });
            Admission {
                ticket: Some(ticket),
                question,
                refused: None,
            }
        }

        fn close_target(&mut self, _target: u32) {
            unreachable!("the program walk's target is the application, which does not close");
        }

        /// The window thread's drain: the rows adopted, and the newest walk heard from — by a
        /// row it answered or by its end — raised once. A walk that died is a fault.
        fn drain(&mut self) -> Vec<Delivered> {
            let answers = self.lane.take();
            let newest = answers
                .verdicts
                .iter()
                .map(|verdict| verdict.generation)
                .chain(answers.finished)
                .max();
            self.programs.adopt(answers.verdicts);
            let mut delivered = std::mem::take(&mut self.raised);
            if let Some(died) = answers.died {
                delivered.push(Delivered {
                    target: Some(0),
                    ticket: Some(died),
                    question: None,
                    outcome: Outcome::Fault("the walk's worker died".to_owned()),
                });
            }
            if let Some(newest) = newest.filter(|newest| *newest > self.seen) {
                self.seen = newest;
                delivered.push(Delivered {
                    target: Some(0),
                    ticket: Some(newest),
                    question: None,
                    outcome: Outcome::Answered,
                });
            }
            delivered
        }

        /// A late answer to the older walk `ticket`, offered to the window thread's acceptance:
        /// the adoption refuses it because the row holds a newer walk's answer.
        fn offer_stale(&mut self, ticket: u64) {
            let row = &rows()[0];
            let stale = RowVerdict {
                generation: ticket,
                id: row.id.clone(),
                source: row.program.clone(),
                program: Some("stale".into()),
            };
            if self.programs.adopt([stale]) {
                self.raised.push(Delivered {
                    target: Some(0),
                    ticket: Some(ticket),
                    question: None,
                    outcome: Outcome::Answered,
                });
            }
        }
    }
}
