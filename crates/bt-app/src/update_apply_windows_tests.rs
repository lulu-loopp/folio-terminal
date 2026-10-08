//! **The Windows applier, its rollback and recovery over real install
//! folders, real signed synthetic programs, real locks, a real claim, real
//! moves and real processes** (Windows); off Windows nothing here runs but the
//! pure half of the rule for a live applier.
//!
//! Every file is made here, under the test's own temporary folder
//! (`%TEMP%\bt-u24-*`, removed when the test is over): an install folder
//! holding the running 0.4.6 build and a staged 0.4.7 set in its home's
//! `set\`, both written by U-20's release writer
//! (`update_prepare_windows::tests::build_that`) and signed by U-15's test root
//! (`bt_platform::trust_harness`), a rescue copy of the running `folio.exe`,
//! and the journal. Each `folio.exe` is a small program that imports nothing,
//! opens no window and stays up until it is ended: the trial, the old build
//! still running (E-7) and an applier still alive are real processes of it,
//! and every one this test starts is ended by the handle it recorded
//! ([`Children`]) — or, the trial a rollback stops, by the product's own
//! `install_flip::ask`, which touches only a process whose pid, creation time
//! and image are the recorded ones. No Folio is ever started: a start the
//! applier or the recovery asks for is recorded ([`Fake::opened`]), and the
//! programs open no window.
//!
//! The entrance goes through the real `bt_platform::logon_hook::arm_in` and
//! `disarm_in` over a registry held in memory ([`Memory`]), under a key of
//! the test's own: the real `Run` key is never written.

use super::*;

use std::collections::BTreeMap;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use bt_platform::logon_hook::{self, Registry};
use bt_platform::trust::FileVersion;
use bt_platform::trust_harness::{Behaviour, IDENTITY, TestCa};
use bt_winres::digest::sha256;

use crate::update_apply::{LastTrialCommit, Left, Window, commit_last_trial_as};
use crate::update_handoff::{Leaving, Spawner};
use crate::update_job::Failure;
use crate::update_prepare_windows::tests::build_that;
use crate::update_txn::{
    Adapter, Body, Class, Header, HeaderOutcome, Member, Outcome, PhaseKind, Receipt,
    STUCK_ATTEMPT_LIMIT,
};

/// The running build's `VERSIONINFO`, and the staged one's.
const RUNNING: FileVersion = FileVersion([0, 4, 6, 0]);
const OFFERED: FileVersion = FileVersion([0, 4, 7, 0]);

/// The key the in-memory registry is asked under: never `RUN_KEY`.
const KEY: &str = r"Software\Folio-Test\u23";

// ── the stand-ins ───────────────────────────────────────────────────────────

/// A registry's values by name: each value's type and bytes.
type Values = BTreeMap<String, (u32, Vec<u8>)>;

/// **A registry held in memory**, shared between the world and the test.
#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Values>>);

impl Registry for Memory {
    fn set(&mut self, key: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
        assert_eq!(key, KEY, "only the test's own key");
        self.0
            .lock()
            .unwrap()
            .insert(name.to_owned(), (kind, data.to_vec()));
        Ok(())
    }

    fn flush(&mut self, _key: &str) -> io::Result<()> {
        Ok(())
    }

    fn get(&mut self, _key: &str, name: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }

    fn delete(&mut self, _key: &str, name: &str) -> io::Result<bool> {
        Ok(self.0.lock().unwrap().remove(name).is_some())
    }

    fn names(&mut self, _key: &str) -> io::Result<Vec<String>> {
        Ok(self.0.lock().unwrap().keys().cloned().collect())
    }
}

impl Memory {
    fn holds(&self, txn: TxnId) -> bool {
        self.0
            .lock()
            .unwrap()
            .contains_key(&logon_hook::value_name(txn.bytes()))
    }
}

/// **Every process a test started**, ended by its own handle when the test
/// is over, pass or fail.
#[derive(Clone, Default)]
struct Children(Arc<Mutex<Vec<Child>>>);

impl Children {
    /// Start the synthetic program at `program`: it stays up.
    fn start(&self, program: &Path, args: &[OsString]) -> u32 {
        let child = bt_platform::quiet_command(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the synthetic program starts");
        let pid = child.id();
        self.0.lock().unwrap().push(child);
        pid
    }

    /// End the one started as `pid`, by its handle, and wait for it.
    fn end(&self, pid: u32) {
        let mut children = self.0.lock().unwrap();
        if let Some(at) = children.iter().position(|child| child.id() == pid) {
            let mut child = children.remove(at);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Children {
    fn drop(&mut self) {
        for mut child in self.0.lock().unwrap().drain(..) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// What the synthetic trial does once it is started.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trial {
    /// Writes its receipt, as `update_trial` does at first pane text.
    Answers,
    /// Writes a receipt at its own name that carries another nonce.
    AnswersWithAnotherNonce,
    /// Writes nothing.
    Silent,
    /// Ends at once, writing nothing.
    Dies,
    /// Runs, is recorded, and ends by itself within its first second,
    /// writing nothing (U-38's Windows twin).
    DiesSoon,
}

type MoveHook = Box<dyn FnMut(&Move) + Send>;
type SayHook = Box<dyn FnMut(&str) + Send>;
type DisarmHook = Box<dyn FnMut() + Send>;
type StartHook = Box<dyn FnMut(usize, &Path, &[OsString]) + Send>;

/// **The stand-in world**: its lines kept, the entrance through the real door
/// over [`Memory`], the trial a real synthetic process, every other start
/// recorded and never made.
struct Fake {
    said: Vec<String>,
    registry: Memory,
    opened: Vec<(PathBuf, Vec<OsString>)>,
    launched: Vec<Vec<OsString>>,
    children: Children,
    trial: Trial,
    home: Home,
    on_move: Option<MoveHook>,
    on_disarm: Option<DisarmHook>,
    /// **Hold `journal.json` open without delete sharing from the trial's
    /// launch** (U-34), as a scanner might: every rename over it is refused
    /// until the applier says the trial could not be recorded.
    hold_at_launch: bool,
    /// The handle [`Fake::hold_at_launch`] opened.
    held: Option<std::fs::File>,
    /// **Keep that handle past "could not be recorded"** (U-37, D-14): the
    /// rollback's own records are refused too, as on the clean VM's 120 s
    /// hold.
    keep_held: bool,
    /// **Panic in the entrance's write** (U-34): a fault inside the road.
    panic_at_arm: bool,
    /// **A program that will not start** (U-34): its start is recorded and
    /// refused.
    refuse_start_of: Option<PathBuf>,
    /// **So many starts die before they take the data directory** (U-34,
    /// round 2): created, never acknowledged.
    starts_die: usize,
    /// **Every start is refused** (U-34, round 2).
    refuse_every_start: bool,
    /// So many successive starts are refused before later ones succeed
    /// (U-35's two ordinary choices, then the last trial).
    refuse_starts: usize,
    /// The failure windows shown in this process (U-34, round 2).
    shown: Vec<String>,
    /// Looks at every line as it is said (U-37).
    on_say: Option<SayHook>,
    /// Looks immediately before each detached start, so a test can observe
    /// the journal at the process-creation boundary.
    before_start: Option<StartHook>,
    /// **Acknowledge a start only as the product does** (U-37, H.3): a Folio
    /// holding the data directory, asked through `update_apply::claimed_within`
    /// for this long — a denied claim question is no acknowledgement.
    real_ack: Option<Duration>,
    /// **A person's start of the new build just before the trial** (U-40):
    /// the installed program started with no trial words at each launch, its
    /// pid kept here.
    beside_launch: Option<Arc<Mutex<Vec<u32>>>>,
}

impl World for Fake {
    fn say(&mut self, line: &str) {
        if let Some(look) = &mut self.on_say {
            look(line);
        }
        if line.contains("could not be recorded") && !self.keep_held {
            // The scanner lets go: the rollback's own records go through.
            self.held = None;
        }
        self.said.push(line.to_owned());
    }

    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        if let Some(look) = &mut self.before_start {
            look(self.opened.len(), program, args);
        }
        self.opened.push((program.to_path_buf(), args.to_vec()));
        if self.refuse_every_start
            || self.refuse_start_of.as_deref() == Some(program)
            || self.refuse_starts > 0
        {
            self.refuse_starts = self.refuse_starts.saturating_sub(1);
            return Err(io::Error::other("the system would not start it (test)"));
        }
        Ok(())
    }

    fn arm(&mut self, txn: TxnId, rescue: &Path) -> Result<Armed, String> {
        assert!(!self.panic_at_arm, "a fault inside the road (test)");
        logon_hook::arm_in(&mut self.registry, KEY, txn.bytes(), rescue)
            .map_err(|refusal| refusal.to_string())
    }

    fn disarm(&mut self, txn: TxnId) -> Result<(), String> {
        if self.registry.holds(txn)
            && let Some(look) = &mut self.on_disarm
        {
            look();
        }
        logon_hook::disarm_in(&mut self.registry, KEY, txn.bytes())
            .map_err(|refusal| refusal.to_string())
    }

    fn is_armed(&mut self, txn: TxnId) -> Result<bool, String> {
        Ok(self.registry.holds(txn))
    }

    fn launch_trial(&mut self, program: &Path, args: &[OsString]) -> io::Result<u32> {
        self.launched.push(args.to_vec());
        if let Some(people) = &self.beside_launch {
            let person = self.children.start(program, &[OsString::from("person")]);
            people.lock().unwrap().push(person);
        }
        let pid = self.children.start(program, args);
        if self.hold_at_launch {
            self.held = Some(
                bt_platform::trust_harness::hold_without_delete_sharing(&self.home.journal())
                    .unwrap(),
            );
        }
        let txn = TxnId::parse(&args[1].to_string_lossy()).unwrap();
        let nonce = Nonce::parse(&args[2].to_string_lossy()).unwrap();
        let carried = match self.trial {
            Trial::Silent => return Ok(pid),
            Trial::Dies => {
                self.children.end(pid);
                return Ok(pid);
            }
            Trial::DiesSoon => {
                let children = self.children.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(300));
                    children.end(pid);
                });
                return Ok(pid);
            }
            Trial::Answers => nonce,
            Trial::AnswersWithAnotherNonce => Nonce::new([0x99; 32]),
        };
        let receipt = Receipt {
            txn,
            nonce: carried,
            pid,
            version: "0.4.7".to_owned(),
            // As the product's trial writes it (H.1).
            started: install_flip::started_of(pid),
        };
        install_txn::durable_create(&self.home.receipt_path(txn, &nonce), &receipt.encode())
            .unwrap();
        Ok(pid)
    }

    fn moved(&mut self, done: &Move) {
        if let Some(look) = &mut self.on_move {
            look(done);
        }
    }

    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool {
        if let Some(within) = self.real_ack {
            return crate::update_apply::claimed_within(worker, data, within);
        }
        if self.starts_die == 0 {
            return true;
        }
        self.starts_die -= 1;
        false
    }

    fn show_here(&mut self, text: &str) {
        self.shown.push(text.to_owned());
    }
}

// ── the installation ────────────────────────────────────────────────────────

/// A folder of the test's own under the temporary directory, removed when
/// dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One file's name and bytes.
type Files = Vec<(String, Vec<u8>)>;

/// **One installation**: the running 0.4.6 build in `install`, the staged
/// 0.4.7 set in `set\`, the rescue copy, the journal at `Handoff`, a data
/// directory, and the registry the entrance goes into.
struct Install {
    /// Ended before the folder is removed (fields drop in this order).
    children: Children,
    _scratch: Scratch,
    ca: TestCa,
    installed: PathBuf,
    rescue: PathBuf,
    home: Home,
    txn: TxnId,
    applier: Nonce,
    data: PathBuf,
    inventories: Inventories,
    old: Files,
    new: Files,
    registry: Memory,
}

impl Install {
    /// The installation, or `None` off Windows, where no test root signs.
    fn new(tag: &str) -> Option<Self> {
        let ca = TestCa::new().ok()?;
        let root = bt_testpath::temp_path(&format!("bt-u24-{tag}"));
        let scratch = Scratch(root.clone());
        let install = root.join("Folio");
        std::fs::create_dir_all(&install).unwrap();
        let old = build_that(
            &ca,
            &install,
            RUNNING,
            "0.4.6",
            IDENTITY,
            "the running build",
            Behaviour::StaysUp,
        );
        let installed = install.join("folio.exe");
        let home = Home::of(HostPlatform::Windows, &installed).unwrap();
        let txn = TxnId::new([0x3c; 16]);
        let set = home.members_folder(txn, Place::Set).unwrap();
        std::fs::create_dir_all(&set).unwrap();
        let new = build_that(
            &ca,
            &set,
            OFFERED,
            "0.4.7",
            IDENTITY,
            "the new build",
            Behaviour::StaysUp,
        );
        let rescue = home
            .rescue_copy(txn, std::ffi::OsStr::new("folio.exe"))
            .unwrap();
        std::fs::create_dir_all(rescue.parent().unwrap()).unwrap();
        std::fs::copy(&installed, &rescue).unwrap();
        let data = root.join("data").join("Folio");
        std::fs::create_dir_all(&data).unwrap();
        let member = |(name, bytes): &(String, Vec<u8>)| Member {
            name: name.clone(),
            digest: Digest::new(sha256(bytes)),
            size: bytes.len() as u64,
        };
        let mut old_shipped = vec!["folio.exe".to_owned(), "folio.msix".to_owned()];
        old_shipped.extend(
            old.iter()
                .map(|(name, _)| name.clone())
                .filter(|name| name != "folio.exe"),
        );
        let inventories = Inventories {
            old_shipped,
            old_present: old.iter().map(member).collect(),
            new: new.iter().map(member).collect(),
        };
        let install = Self {
            children: Children::default(),
            _scratch: scratch,
            ca,
            installed,
            rescue,
            home,
            txn,
            applier: Nonce::new([0x44; 32]),
            data,
            inventories,
            old,
            new,
            registry: Memory::default(),
        };
        install.write(Phase::Handoff {
            applier: install.applier,
        });
        Some(install)
    }

    /// The journal, durably at `phase`, with the inventories.
    fn write(&self, phase: Phase) {
        let journal = Journal {
            txn: self.txn,
            rescue: self.rescue.display().to_string(),
            body: Body {
                adapter: crate::update_txn::Adapter::Ours,
                phase,
                layout: Layout::Members(self.inventories.clone()),
            },
        };
        install_txn::durable_write(&self.home.journal(), &journal.encode()).unwrap();
    }

    fn on_disk(&self) -> Journal {
        Journal::parse(&std::fs::read(self.home.journal()).unwrap()).unwrap()
    }

    fn header(&self) -> Header {
        Header::parse(&std::fs::read(self.home.journal()).unwrap()).unwrap()
    }

    fn road(&self, limits: Limits) -> Road {
        Road {
            home: self.home.clone(),
            installed: self.installed.clone(),
            rescue: self.rescue.clone(),
            data: self.data.clone(),
            policy: self.ca.policy(),
            channel: Some(Channel::Ours),
            limits,
            me: Running {
                pid: std::process::id(),
                started: 0,
            },
            starter: None,
            handed_back: None,
            layouts: own_layouts(),
            as_046: false,
        }
    }

    fn world(&self, trial: Trial) -> Fake {
        Fake {
            said: Vec::new(),
            registry: self.registry.clone(),
            opened: Vec::new(),
            launched: Vec::new(),
            children: self.children.clone(),
            trial,
            home: self.home.clone(),
            on_move: None,
            on_disarm: None,
            hold_at_launch: false,
            held: None,
            keep_held: false,
            panic_at_arm: false,
            refuse_start_of: None,
            starts_die: 0,
            refuse_every_start: false,
            refuse_starts: 0,
            shown: Vec::new(),
            on_say: None,
            before_start: None,
            real_ack: None,
            beside_launch: None,
        }
    }

    /// The entrance, as a dead attempt left it.
    fn arm(&self) {
        let armed = logon_hook::arm_in(
            &mut self.registry.clone(),
            KEY,
            self.txn.bytes(),
            &self.rescue,
        )
        .unwrap();
        assert_eq!(armed.transaction(), self.txn.bytes());
    }

    /// Record this test process as the holder before a fixture advances past
    /// `Handoff`. In product, one `apply` call keeps this election success in
    /// its exit guard while it advances the journal; a test that enters the
    /// road directly at a later phase must represent that existing duty.
    fn claim_window(&self) {
        assert!(
            crate::update_apply::take_the_window(
                None,
                &self.home,
                self.txn,
                Running {
                    pid: std::process::id(),
                    started: 0,
                },
                Instant::now(),
            )
            .is_mine()
        );
    }

    fn install(&self) -> PathBuf {
        self.installed.parent().unwrap().to_path_buf()
    }

    fn folder(&self, place: Place) -> PathBuf {
        match place {
            Place::Install => self.install(),
            other => self.home.members_folder(self.txn, other).unwrap(),
        }
    }

    /// **The bytes at each of `files`' names in `place`**, `None` where there
    /// is nothing.
    fn in_place(&self, place: Place, files: &Files) -> Vec<Option<Vec<u8>>> {
        files
            .iter()
            .map(|(name, _)| std::fs::read(self.folder(place).join(name)).ok())
            .collect()
    }

    /// Whether `place` holds exactly `files`, byte for byte.
    fn holds(&self, place: Place, files: &Files) -> bool {
        self.in_place(place, files)
            == files
                .iter()
                .map(|(_, bytes)| Some(bytes.clone()))
                .collect::<Vec<_>>()
    }

    /// The first `count` moves of the flip, made by hand, as a dead applier
    /// left them.
    fn move_first(&self, count: usize) {
        for step in self.inventories.forward_moves().into_iter().take(count) {
            let from = self.folder(step.from).join(&step.name);
            let to = self.folder(step.to).join(&step.name);
            if step.to == Place::Backup {
                std::fs::create_dir_all(self.folder(Place::Backup)).unwrap();
            }
            install_txn::durable_move(&from, &to).unwrap();
        }
    }

    /// The receipt the trial writes, carrying `carried`, at `nonce`'s name.
    fn receipt(&self, nonce: Nonce, carried: Nonce, pid: u32) {
        let receipt = Receipt {
            txn: self.txn,
            nonce: carried,
            pid,
            version: "0.4.7".to_owned(),
            // As the product's trial writes it (H.1).
            started: install_flip::started_of(pid),
        };
        install_txn::durable_create(&self.home.receipt_path(self.txn, &nonce), &receipt.encode())
            .unwrap();
    }

    /// A trial process: the installed program started, and the journal's
    /// record of it.
    fn start_trial(&self) -> TrialProcess {
        let pid = self.children.start(&self.installed, &[]);
        TrialProcess {
            pid,
            started: install_flip::started_of(pid).expect("the trial runs"),
        }
    }
}

/// Short limits for a test: `old_within_ms` for the old build, `trial_ms`
/// for the trial, [`GRACE`] for a trial asked to quit.
fn limits(old_within_ms: u64, trial_ms: u64) -> Limits {
    Limits {
        old_within: Duration::from_millis(old_within_ms),
        trial_within_ms: trial_ms,
        poll: Duration::from_millis(40),
        quit_within: GRACE,
        end_within: Duration::from_secs(10),
    }
}

/// A test's grace for a trial asked to quit (the product's is 5 s).
const GRACE: Duration = Duration::from_millis(600);

fn start_on_a_worker<T: Send + 'static>(
    body: impl FnOnce(&WorkerCtx) -> T + Send + 'static,
) -> JoinHandle<T> {
    bt_platform::spawn_at_priority(
        "bt-u23-test",
        bt_platform::ThreadPriority::BelowNormal,
        body,
    )
    .expect("the thread door starts a thread")
}

fn on_a_worker<T: Send + 'static>(body: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
    match start_on_a_worker(body).join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// The applier over `road`, on its own worker, started now.
fn start(road: Road, txn: TxnId, nonce: Nonce, mut world: Fake) -> JoinHandle<(Ended, Fake)> {
    start_on_a_worker(move |worker| {
        let ended = apply(worker, &road, txn, nonce, &mut world);
        (ended, world)
    })
}

/// The applier, run to its end.
fn applied(install: &Install, limits: Limits, world: Fake) -> (Ended, Fake) {
    match start(install.road(limits), install.txn, install.applier, world).join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// The recovery door, as an ordinary start hands itself to it, run to its
/// end: its exit code, and the world.
fn recovered(install: &Install, limits: Limits, mut world: Fake) -> (i32, Fake) {
    let road = install.road(limits);
    on_a_worker(move |worker| {
        let code = crate::update_recover::run_windows(worker, &road, Some(&handed()), &mut world);
        (code, world)
    })
}

/// The command line an ordinary start handed over.
fn handed() -> Vec<OsString> {
    ["--cwd", r"D:\x"].into_iter().map(OsString::from).collect()
}

/// The phases the journal went through, in order, as the world's line says.
fn wrote(world: &Fake) -> &str {
    world
        .said
        .iter()
        .find(|line| line.contains(" wrote "))
        .map_or("", String::as_str)
}

/// Wait until the journal on disk is at `phase`, for as long as `applier` is still on its road.
///
/// **No total of its own** (CONVENTIONS §3, the child's silence and not the clock): a loaded
/// machine walks the same road more slowly, and the only thing that can make the phase never come
/// is the applier ending first — which is red here, at once, with the phase it left behind. The
/// wait is bounded only by the applier thread's end: an applier stuck alive hangs the test rather
/// than turning it red.
fn until_journal<T>(install: &Install, phase: PhaseKind, applier: &JoinHandle<T>) {
    while install.on_disk().body.phase.kind() != phase {
        assert!(
            !applier.is_finished(),
            "the applier ended with the journal at {:?}, never at {phase:?}",
            install.on_disk().body.phase.kind()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// **Nothing was moved**: the install holds the old build, `set\` the new
/// one, and `backup\` nothing of either.
fn nothing_moved(install: &Install) {
    assert!(
        install.holds(Place::Install, &install.old),
        "the install is the old build"
    );
    assert!(
        install.holds(Place::Set, &install.new),
        "set\\ holds the new build"
    );
    assert!(
        install
            .in_place(Place::Backup, &install.old)
            .iter()
            .all(Option::is_none),
        "nothing is in backup\\"
    );
}

/// **The old set is installed again, byte for byte, and the new one is out**
/// — in `rolledout\` or still in `set\`, each new file in exactly one.
fn rolled_back_on_disk(install: &Install) {
    assert!(
        install.holds(Place::Install, &install.old),
        "the install is the old build again"
    );
    for (name, bytes) in &install.new {
        let places = [Place::Set, Place::RolledOut]
            .iter()
            .filter(|place| {
                std::fs::read(install.folder(**place).join(name))
                    .ok()
                    .as_ref()
                    == Some(bytes)
            })
            .count();
        assert_eq!(places, 1, "the new `{name}` is out, once");
    }
}

/// The words a build started after a rollback carries, then `handed`.
fn failed_then(install: &Install, handed: &[OsString]) -> Vec<OsString> {
    let mut words = failed_words(&install.home).to_vec();
    words.extend_from_slice(handed);
    words
}

/// Whether `pid`, recorded at `started`, still runs.
fn runs(process: TrialProcess) -> bool {
    install_flip::still_running(Running {
        pid: process.pid,
        started: process.started,
    })
}

/// The first line that contains `needle`, by its index.
fn said_at(world: &Fake, needle: &str) -> Option<usize> {
    world.said.iter().position(|line| line.contains(needle))
}

/// **A foreign file at every new name in `rolledout\`**: a move out there is
/// refused (a move never replaces), so a rollback stops at its first move
/// with the new set still installed.
fn block_rolledout(install: &Install) {
    let rolledout = install.folder(Place::RolledOut);
    std::fs::create_dir_all(&rolledout).unwrap();
    for (name, _) in &install.new {
        std::fs::write(rolledout.join(name), b"somebody else's file").unwrap();
    }
}

// ── the tests ───────────────────────────────────────────────────────────────

/// RED (U-23) — **the applier takes a handed-over update to `Committed`
/// only once the old build has let go of the transaction lock and then of the
/// data directory's claim, and lets the claim go at once; a lock kept to the
/// end of the wait leaves the journal byte for byte as it was, and a claim
/// kept to the end abandons the update with nothing moved.**
///
/// §C.4: "P first waits for O to be gone: it holds the installation lock,
/// polls O's process, and — the authoritative test — polls
/// `claim_data_directory` until it succeeds, then immediately releases it …
/// If 60 s pass, P journals `Failed` … and exits without touching anything."
///
/// MUTATION: in `update_apply::wait_for_the_claim`, answer `Ok(())` for a
/// claim that is held.
#[test]
fn p_waits_for_o_and_releases_the_claim() {
    let Some(install) = Install::new("wait") else {
        return;
    };
    // O keeps the lock to the end.
    let journal = std::fs::read(install.home.journal()).unwrap();
    let held = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let (ended, _) = applied(&install, limits(600, 20_000), install.world(Trial::Answers));
    assert_eq!(ended, Ended::OldHeldTheLock);
    assert_eq!(std::fs::read(install.home.journal()).unwrap(), journal);
    drop(held);

    // O lets go of the lock, then of the claim.
    let lock = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let claim = crate::persist::try_claim(&install.data).unwrap();
    let applier = start(
        install.road(limits(20_000, 20_000)),
        install.txn,
        install.applier,
        install.world(Trial::Answers),
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(install.on_disk().body.phase.kind(), PhaseKind::Handoff);
    drop(lock);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        install.on_disk().body.phase.kind(),
        PhaseKind::Handoff,
        "nothing is written while the claim is held"
    );
    nothing_moved(&install);
    drop(claim);
    let (ended, world) = applier.join().unwrap();
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(
        crate::persist::try_claim(&install.data).is_ok(),
        "the applier let the claim go"
    );

    // O keeps the claim to the end: abandoned, nothing moved.
    let Some(install) = Install::new("stayed") else {
        return;
    };
    let claim = crate::persist::try_claim(&install.data).unwrap();
    let (ended, world) = applied(&install, limits(600, 20_000), install.world(Trial::Answers));
    drop(claim);
    assert_eq!(ended, Ended::Abandoned, "{:?}", world.said);
    assert_eq!(install.on_disk().body.phase, Phase::Abandoned);
    nothing_moved(&install);
    assert!(!install.registry.holds(install.txn), "nothing was armed");
    // U-34 supersedes U-23's "O stayed: nothing is started": the applier
    // leaves through its exit guard, and with no successor running it starts
    // the installed build — plainly, the journal being over.
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), Vec::new())],
        "{:?}",
        world.said
    );
}

/// RED (U-23) — **`Armed` is durable, with the `Run` value in place, before
/// anything moves; `Moving` is durable before the first move.**
///
/// (b).2 "Durability": "Every destructive step is preceded by a durable
/// journal state that names it". A running copy holds the admission shared:
/// the applier stops at `Armed`, the entrance written, the install untouched,
/// until the copy lets go.
///
/// MUTATION: in `at_armed`, record `Admitted` after the first move.
#[test]
fn armed_is_durable_before_moving() {
    let Some(install) = Install::new("armed") else {
        return;
    };
    std::fs::write(install.home.admission(), b"").unwrap();
    let running = install_txn::try_hold(&install.home.admission(), Hold::Shared)
        .unwrap()
        .unwrap();
    let mut world = install.world(Trial::Answers);
    let journal = install.home.journal();
    let first = Arc::new(Mutex::new(None::<PhaseKind>));
    let seen = Arc::clone(&first);
    world.on_move = Some(Box::new(move |_| {
        let mut seen = seen.lock().unwrap();
        if seen.is_none() {
            let on_disk = Journal::parse(&std::fs::read(&journal).unwrap()).unwrap();
            *seen = Some(on_disk.body.phase.kind());
        }
    }));
    let applier = start(
        install.road(limits(20_000, 20_000)),
        install.txn,
        install.applier,
        world,
    );
    until_journal(&install, PhaseKind::Armed, &applier);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(install.on_disk().body.phase, Phase::Armed);
    assert!(
        install.registry.holds(install.txn),
        "the entrance is written"
    );
    nothing_moved(&install);
    drop(running);
    let (ended, world) = applier.join().unwrap();
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert_eq!(*first.lock().unwrap(), Some(PhaseKind::Moving));
    assert!(
        wrote(&world).ends_with("[Armed, Moving, Trial, Committed, Retired]"),
        "{:?}",
        world.said
    );
}

/// RED (U-23) — **a file of the install folder held open by another process
/// refuses the flip before any move: the entrance is removed, the journal is
/// `Prepared` again, and the old build is started again with no argument.**
///
/// E-7, on a real held-open file: the old `folio.exe` itself, running.
///
/// MUTATION: in `ready_to_move`, skip the `held_open` loop.
#[test]
fn a_held_open_file_refuses_before_any_move_and_relaunches_o() {
    let Some(install) = Install::new("held") else {
        return;
    };
    let old = install.children.start(&install.installed, &[]);
    let (ended, world) = applied(
        &install,
        limits(1_500, 20_000),
        install.world(Trial::Answers),
    );
    install.children.end(old);
    assert_eq!(ended, Ended::Reverted, "{:?}", world.said);
    assert!(
        world.said.iter().any(|line| line.contains("held open")),
        "{:?}",
        world.said
    );
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Prepared {
            deferred_launches: 0
        }
    );
    nothing_moved(&install);
    assert!(
        !install.registry.holds(install.txn),
        "the entrance is removed"
    );
    assert!(world.launched.is_empty(), "no trial");
    assert_eq!(world.opened, vec![(install.installed.clone(), Vec::new())]);
}

/// RED (U-23) — **between every two moves, every old file is in exactly one
/// of the install and `backup\`, and every new file in exactly one of `set\`
/// and the install** (I1′), by their digests; and the flip ends with the new
/// set installed and the old one in `backup\` until the commit.
///
/// (b).2 W6: "every old file in exactly one of install/backup, every new file
/// in exactly one of set/install (I1′)", so recovery reads the disk.
///
/// MUTATION: in `update_txn::Inventories::forward_moves`, move the new files
/// in before the old ones out.
#[test]
fn moving_keeps_i1_prime_at_every_instant() {
    let Some(install) = Install::new("i1") else {
        return;
    };
    let folders = [
        install.folder(Place::Install),
        install.folder(Place::Backup),
        install.folder(Place::Set),
    ];
    let (old, new) = (install.old.clone(), install.new.clone());
    let instants = Arc::new(Mutex::new(Vec::<String>::new()));
    let broken = Arc::clone(&instants);
    let mut world = install.world(Trial::Answers);
    world.on_move = Some(Box::new(move |done| {
        let count = |files: &Files, places: &[&PathBuf]| -> Vec<usize> {
            files
                .iter()
                .map(|(name, bytes)| {
                    places
                        .iter()
                        .filter(|place| {
                            std::fs::read(place.join(name)).ok().as_ref() == Some(bytes)
                        })
                        .count()
                })
                .collect()
        };
        let olds = count(&old, &[&folders[0], &folders[1]]);
        let news = count(&new, &[&folders[2], &folders[0]]);
        if olds.iter().chain(&news).any(|places| *places != 1) {
            broken
                .lock()
                .unwrap()
                .push(format!("after `{}`: old {olds:?}, new {news:?}", done.name));
        }
    }));
    let (ended, world) = applied(&install, limits(20_000, 20_000), world);
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(
        instants.lock().unwrap().is_empty(),
        "{:?}",
        instants.lock().unwrap()
    );
    assert!(install.holds(Place::Install, &install.new));
}

/// RED (U-23; U-24) — **a move that fails leaves the disk in I1′ and the
/// applier journals `RollbackIntent` — then rolls back from what is on disk,
/// which never moves a file the journal does not know**: somebody else's file
/// at `folio.exe`'s name stops the rollback at `Stuck`, everything kept — the
/// old files in `backup\`, the new ones in `set\`, the entrance — and, the
/// install holding neither whole set, the rescue copy opens with
/// `--update-failed` (the fallback).
///
/// (b).2 W6: "a move that fails leaves the disk in I1′"; W9: "move install
/// files with new digests"; W10: "`Stuck` … journal, backup and Run value
/// kept".
///
/// MUTATION: in `at_armed`, go on to the next move after a failed one.
#[test]
fn a_failed_move_is_declared_and_a_foreign_file_leaves_the_rollback_stuck() {
    let Some(install) = Install::new("failed") else {
        return;
    };
    let mut world = install.world(Trial::Answers);
    let old_count = install.inventories.old_present.len();
    let obstacle = install.installed.clone();
    let mut moves = 0;
    world.on_move = Some(Box::new(move |_| {
        moves += 1;
        if moves == old_count {
            // Every old file is out; something takes `folio.exe`'s name.
            std::fs::write(&obstacle, b"somebody else's file").unwrap();
        }
    }));
    let (ended, world) = applied(&install, limits(20_000, 20_000), world);
    assert!(
        matches!(ended, Ended::Stuck(_)),
        "{ended:?} {:?}",
        world.said
    );
    assert!(
        wrote(&world).ends_with("[Armed, Moving, RollbackIntent, Stuck]"),
        "{:?}",
        world.said
    );
    assert_eq!(install.header().outcome, HeaderOutcome::RolledBack);
    assert_eq!(install.header().class, Class::Destructive);
    assert!(
        install.holds(Place::Backup, &install.old),
        "every old file is in backup\\"
    );
    assert!(
        install.holds(Place::Set, &install.new),
        "every new file is still in set\\"
    );
    assert_eq!(
        std::fs::read(&install.installed).unwrap(),
        b"somebody else's file",
        "the foreign file is left as it is"
    );
    assert!(world.launched.is_empty(), "no trial");
    assert!(
        install.registry.holds(install.txn),
        "the entrance stays for the next attempt"
    );
    assert_eq!(
        world.opened,
        vec![(install.rescue.clone(), failed_then(&install, &[]))],
        "neither whole set is installed: the rescue copy opens"
    );
}

/// RED (U-23) — **a receipt that carries another trial's nonce is refused**:
/// it is said once, the applier goes on waiting, and without a receipt of its
/// own by the deadline the trial is declared rolled back.
///
/// "Who may write what": "`Committed` … only on a receipt whose `txn` and
/// `nonce` match".
///
/// MUTATION: in `update_txn::next`, drop the `ReceiptForAnotherTrial` arm.
#[test]
fn a_receipt_for_another_nonce_is_refused() {
    let Some(install) = Install::new("nonce") else {
        return;
    };
    let (ended, world) = applied(
        &install,
        limits(20_000, 1_500),
        install.world(Trial::AnswersWithAnotherNonce),
    );
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    let said = world
        .said
        .iter()
        .filter(|line| line.contains("is refused"))
        .count();
    assert_eq!(said, 1, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Armed, Moving, Trial, RollbackIntent, RolledBack, Retired]"),
        "{:?}",
        world.said
    );
    rolled_back_on_disk(&install);
}

/// RED (U-23) — **`Committed` is written after `Trial`, on the receipt of
/// this transaction and this trial, and the header then says `committed`.**
///
/// "Who may write what": "`Committed` is written only by the lock holder,
/// only while the journal says `Trial`, and only on a receipt whose `txn` and
/// `nonce` match"; `outcome: committed` at `Committed`. The trial was launched
/// as `<install>\folio.exe --update-trial <txn> <nonce>` (ruling 2).
///
/// MUTATION: in `trial`, skip recording `TrialBegan`.
#[test]
fn committed_is_written_only_on_a_matching_receipt_while_trial() {
    let Some(install) = Install::new("commit") else {
        return;
    };
    let (ended, world) = applied(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Answers),
    );
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Armed, Moving, Trial, Committed, Retired]"),
        "{:?}",
        world.said
    );
    assert_eq!(world.launched.len(), 1);
    let args = &world.launched[0];
    assert_eq!(args[0], OsString::from(cli::UPDATE_TRIAL_FLAG));
    assert_eq!(args[1], OsString::from(install.txn.to_string()));
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::Committed,
            untried: false,
        }
    );
    let header = install.header();
    assert_eq!(header.outcome, HeaderOutcome::Committed);
    assert_eq!(header.class, Class::Terminal);
    assert!(
        install.holds(Place::Install, &install.new),
        "the new set is installed"
    );
    assert!(
        world.opened.is_empty(),
        "after a commit nothing is started: the trial runs"
    );
    assert!(
        install.rescue.is_file(),
        "the rescue copy is the next start's"
    );
}

/// RED (U-23) — **after the commit, the `Run` value goes first, then exactly
/// the recorded old files in `backup\`, and only then the class is
/// `terminal`**: at the instant the entrance is removed the journal says
/// `Committed` and `backup\` is whole.
///
/// The coordinator's ruling 6 and W8: "`Committed` durable → Run value removed
/// and flushed → `backup\` deleted → class `terminal`".
///
/// MUTATION: in `retire_committed_steps`, delete the backup files before the
/// entrance.
#[test]
fn backup_is_deleted_only_after_committed_and_the_run_value_is_gone_first() {
    let Some(install) = Install::new("order") else {
        return;
    };
    let mut world = install.world(Trial::Answers);
    let (journal, backup, old) = (
        install.home.journal(),
        install.folder(Place::Backup),
        install.old.clone(),
    );
    let at_disarm = Arc::new(Mutex::new(None::<(PhaseKind, bool)>));
    let seen = Arc::clone(&at_disarm);
    world.on_disarm = Some(Box::new(move || {
        let phase = Journal::parse(&std::fs::read(&journal).unwrap())
            .unwrap()
            .body
            .phase
            .kind();
        let whole = old
            .iter()
            .all(|(name, bytes)| std::fs::read(backup.join(name)).ok().as_ref() == Some(bytes));
        *seen.lock().unwrap() = Some((phase, whole));
    }));
    let (ended, world) = applied(&install, limits(20_000, 20_000), world);
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert_eq!(
        *at_disarm.lock().unwrap(),
        Some((PhaseKind::Committed, true)),
        "the entrance goes while Committed, with backup\\ whole"
    );
    assert!(!install.registry.holds(install.txn));
    assert!(
        install
            .in_place(Place::Backup, &install.old)
            .iter()
            .all(Option::is_none),
        "the recorded old files are deleted from backup\\"
    );
    assert_eq!(install.header().class, Class::Terminal);
}

/// RED (U-23) — **an applier that still owns its transaction goes on from the
/// disk**: `Handoff` with an entrance already there (W4) removes it and
/// reverts, nothing moved, the old build started again; a recorded holder at
/// `Armed` (W5) admits and goes on to `Committed`; a recorded holder at
/// `Moving` with some moves done (W6) declares the rollback, moves nothing
/// further forward, and rolls back (U-24). An unmarked *later* applier may not
/// use these resume rules: the journal says the earlier road was already
/// taken.
///
/// MUTATION: in `Txn::apply`, send `Armed` to `recover_from` (which reverts).
#[test]
fn reentry_at_w4_w5_w6_continues_from_the_disk() {
    let Some(install) = Install::new("w4") else {
        return;
    };
    install.arm();
    let (ended, world) = applied(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Answers),
    );
    assert_eq!(ended, Ended::Reverted, "{:?}", world.said);
    assert!(!install.registry.holds(install.txn));
    nothing_moved(&install);
    assert_eq!(world.opened, vec![(install.installed.clone(), Vec::new())]);

    let Some(install) = Install::new("w5") else {
        return;
    };
    install.arm();
    install.claim_window();
    install.write(Phase::Armed);
    let (ended, world) = applied(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Answers),
    );
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(wrote(&world).ends_with("[Moving, Trial, Committed, Retired]"));

    let Some(install) = Install::new("w6") else {
        return;
    };
    install.arm();
    install.claim_window();
    install.move_first(2);
    install.write(Phase::Moving);
    let (ended, world) = applied(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Answers),
    );
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[RollbackIntent, RolledBack, Retired]"),
        "{:?}",
        world.said
    );
    assert!(install.holds(Place::Install, &install.old));
    assert!(
        install.holds(Place::Set, &install.new),
        "nothing new was moved in"
    );
    assert!(world.launched.is_empty());
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))]
    );
}

/// RED (U-23) — **`Committed` with its cleanup cut short is finished — the
/// entrance and the recorded old files left in `backup\` removed, then
/// `Retired` — and never rolled back**: the install keeps the new set, and a
/// file in `backup\` the journal does not record is left.
///
/// (b).2 W12: "`Committed`, cleanup partly done → finish the deletions (debt,
/// never a rollback)."
///
/// MUTATION: in `recover_from`, answer `Committed` with `RollbackDeclared`.
#[test]
fn w12_finishes_the_deletions_and_never_rolls_back() {
    let Some(install) = Install::new("w12") else {
        return;
    };
    install.arm();
    install.move_first(install.inventories.forward_moves().len());
    let backup = install.folder(Place::Backup);
    std::fs::remove_file(backup.join("uninstall.cmd")).unwrap();
    std::fs::write(backup.join("stray.txt"), b"not the journal's").unwrap();
    install.write(Phase::Committed);
    let (code, world) = recovered(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::Committed,
            untried: false,
        }
    );
    assert!(
        install.holds(Place::Install, &install.new),
        "the new set stays"
    );
    assert!(!install.registry.holds(install.txn));
    let left: Vec<_> = std::fs::read_dir(&backup)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, vec![OsString::from("stray.txt")]);
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), handed())],
        "the new build opens"
    );
}

/// RED (U-23) — **a folder at the name of an old file refuses the flip
/// before any move** (U-20's decision 2): the entrance removed, `Prepared`
/// again, the old build started again.
///
/// MUTATION: in `ready_to_move`, accept any entry at an old file's name.
#[test]
fn a_non_file_at_an_old_name_refuses_before_any_move() {
    let Some(install) = Install::new("nonfile") else {
        return;
    };
    let name = install.install().join("uninstall.cmd");
    std::fs::remove_file(&name).unwrap();
    std::fs::create_dir(&name).unwrap();
    let (ended, world) = applied(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Answers),
    );
    assert_eq!(ended, Ended::Reverted, "{:?}", world.said);
    assert!(
        world
            .said
            .iter()
            .any(|line| line.contains("a folder or a link")),
        "{:?}",
        world.said
    );
    assert!(name.is_dir(), "the folder is left as it is");
    assert!(install.holds(Place::Set, &install.new));
    assert!(!install.registry.holds(install.txn));
    assert_eq!(world.opened, vec![(install.installed.clone(), Vec::new())]);
}

/// RED (U-23) — **a staged set changed after the Prepare is refused before
/// the entrance is written**: `Unverified`, `Abandoned`, nothing armed and
/// nothing moved (U-20's decision 1: the applier revalidates, reading the
/// version from the staged `folio.exe` itself).
///
/// MUTATION: in `at_handoff`, skip `staged_as_verified`.
#[test]
fn a_changed_set_is_refused_before_the_entrance() {
    let Some(install) = Install::new("changed") else {
        return;
    };
    let staged = install.folder(Place::Set).join("uninstall.cmd");
    std::fs::write(&staged, b"@rem changed after the Prepare").unwrap();
    let (ended, world) = applied(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Answers),
    );
    assert_eq!(ended, Ended::Abandoned, "{:?}", world.said);
    assert_eq!(install.on_disk().body.phase, Phase::Abandoned);
    assert!(!install.registry.holds(install.txn));
    assert!(install.holds(Place::Install, &install.old));
    assert!(world.launched.is_empty());
}

/// The recovery door as the entrance at logon starts it — no command line —
/// run to its end: its exit code, and the world.
fn recovered_at_logon(install: &Install, limits: Limits, mut world: Fake) -> (i32, Fake) {
    let road = install.road(limits);
    on_a_worker(move |worker| {
        let code = crate::update_recover::run_windows(worker, &road, None, &mut world);
        (code, world)
    })
}

/// Every forward move done by hand, the entrance armed, and the journal at
/// `phase`: a dead applier's road past its last move.
fn flipped_at(install: &Install, phase: Phase) {
    install.arm();
    install.move_first(install.inventories.forward_moves().len());
    install.write(phase);
}

/// `Stuck` after `attempts` failed rollbacks, no trial recorded.
fn stuck(attempts: u8) -> Phase {
    Phase::Stuck {
        trial: None,
        trial_started: false,
        last_error: "an earlier attempt".to_owned(),
        attempts,
        retrial: None,
    }
}

/// The name of a new member other than `folio.exe`.
fn a_new_name_besides_the_program(install: &Install) -> String {
    install
        .new
        .iter()
        .map(|(name, _)| name.clone())
        .find(|name| name != "folio.exe")
        .expect("the new set has a file besides folio.exe")
}

/// RED (U-24) — **a trial that gives no receipt by its deadline is stopped —
/// asked to quit, then, its grace run out, ended — and the moves are reversed:
/// every install file with a new digest to `rolledout\`, every old file back
/// from `backup\`; `RolledBack` is durable, the `Run` value is removed,
/// `Retired{RolledBack}` makes the class `terminal`, and the old build is
/// started again with `--update-failed <journal>`.**
///
/// §C.5: "No health → `RollingBack`"; (b).2 W9 and W11; the coordinator's
/// ruling 1: "relaunch the installed old build `folio.exe --update-failed
/// <journal>`". On BASE the applier stopped at `RollbackIntent` and started
/// nothing.
///
/// MUTATION: in `Txn::declare_rollback`, return after recording
/// `RollbackDeclared` instead of settling (U-23's stop).
#[test]
fn a_failed_health_reverses_the_moves_and_relaunches_the_old_build() {
    let Some(install) = Install::new("health") else {
        return;
    };
    let began = Instant::now();
    let (ended, world) = applied(
        &install,
        limits(20_000, 1_500),
        install.world(Trial::Silent),
    );
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        began.elapsed() >= Duration::from_millis(1_500),
        "the trial had its deadline"
    );
    assert!(
        wrote(&world).ends_with("[Armed, Moving, Trial, RollbackIntent, RolledBack, Retired]"),
        "{:?}",
        world.said
    );
    let quit = said_at(&world, "is asked to quit").expect("the trial is asked to quit");
    let end = said_at(&world, "is asked to end").expect("the silent trial is ended");
    assert!(quit < end, "{:?}", world.said);
    rolled_back_on_disk(&install);
    assert!(
        install.holds(Place::RolledOut, &install.new),
        "every new file went to rolledout\\"
    );
    assert!(
        install
            .in_place(Place::Backup, &install.old)
            .iter()
            .all(Option::is_none),
        "every old file came back from backup\\"
    );
    assert!(
        !install.registry.holds(install.txn),
        "the Run value is removed"
    );
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::RolledBack,
            untried: false,
        }
    );
    let header = install.header();
    assert_eq!(header.class, Class::Terminal);
    assert_eq!(header.outcome, HeaderOutcome::RolledBack);
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "the old build is started again with --update-failed"
    );
}

/// RED (U-24) — **a trial that dies without a receipt is rolled back at
/// once**, long before its deadline, and nothing is asked of a process that is
/// gone.
///
/// (b).2 W7: "the trial gone … → `RollbackIntent`"; W9.
///
/// MUTATION: in `Txn::watch`, answer the trial alive whatever the process list
/// says (the applier then waits out the 60 s deadline).
#[test]
fn a_dead_trial_rolls_back_at_once() {
    let Some(install) = Install::new("dead") else {
        return;
    };
    let began = Instant::now();
    let (ended, world) = applied(&install, limits(20_000, 60_000), install.world(Trial::Dies));
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        began.elapsed() < Duration::from_secs(30),
        "rolled back at once, not at the deadline: {:?}",
        began.elapsed()
    );
    assert!(
        said_at(&world, "ended without a receipt").is_some(),
        "{:?}",
        world.said
    );
    assert!(said_at(&world, "is asked to").is_none(), "{:?}", world.said);
    rolled_back_on_disk(&install);
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))]
    );
}

/// RED (U-24) — **between every two moves of the rollback, every old file is
/// in exactly one of the install and `backup\`, and every new file in exactly
/// one of `set\`, the install and `rolledout\`** (I1′), by their bytes; and
/// the watcher saw every move back.
///
/// (b).2 W9: "every step I1′"; `update_txn::Restore::Moves`: "the new files out
/// to `rolledout\` first, then the old files back from `backup\` — the new set
/// is never destroyed before the old is restored (F-7)".
///
/// MUTATION: in `update_txn::rollback_moves`, put the old files back before
/// the new ones go out.
#[test]
fn rollback_keeps_i1_prime_at_every_instant() {
    let Some(install) = Install::new("i1back") else {
        return;
    };
    let folders = [
        install.folder(Place::Install),
        install.folder(Place::Backup),
        install.folder(Place::Set),
        install.folder(Place::RolledOut),
    ];
    let (old, new) = (install.old.clone(), install.new.clone());
    let broken = Arc::new(Mutex::new(Vec::<String>::new()));
    let backs = Arc::new(Mutex::new(0usize));
    let (seen, counted) = (Arc::clone(&broken), Arc::clone(&backs));
    let mut world = install.world(Trial::Silent);
    world.on_move = Some(Box::new(move |done| {
        if done.to == Place::RolledOut || done.from == Place::Backup {
            *counted.lock().unwrap() += 1;
        }
        let count = |files: &Files, places: &[&PathBuf]| -> Vec<usize> {
            files
                .iter()
                .map(|(name, bytes)| {
                    places
                        .iter()
                        .filter(|place| {
                            std::fs::read(place.join(name)).ok().as_ref() == Some(bytes)
                        })
                        .count()
                })
                .collect()
        };
        let olds = count(&old, &[&folders[0], &folders[1]]);
        let news = count(&new, &[&folders[2], &folders[0], &folders[3]]);
        if olds.iter().chain(&news).any(|places| *places != 1) {
            seen.lock().unwrap().push(format!(
                "after `{}` {:?} → {:?}: old {olds:?}, new {news:?}",
                done.name, done.from, done.to
            ));
        }
    }));
    let (ended, world) = applied(&install, limits(20_000, 1_000), world);
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        broken.lock().unwrap().is_empty(),
        "{:?}",
        broken.lock().unwrap()
    );
    assert_eq!(
        *backs.lock().unwrap(),
        install.old.len() + install.new.len(),
        "every move back was watched"
    );
    rolled_back_on_disk(&install);
}

/// RED (U-24) — **a rollback begins from what the disk holds, never from the
/// phase**: whatever prefix of the flip a dead applier left — old files out
/// and none in, some new files in, or the whole flip — and whichever phase the
/// journal names for it (`Moving`, a `Trial` whose process is gone, or
/// `RollbackIntent`), the recovery moves out exactly the install files at new
/// digests and back exactly the old files `backup\` holds, and the old build
/// opens with `--update-failed`.
///
/// (b).2 W6: "`Moving`, any number of moves done … → `RollbackIntent`, then
/// row W9"; F-7: "reconciled by digest against the recorded inventories".
///
/// MUTATION: in `update_txn::rollback_moves`, put an old file back only where
/// the install holds the new one (a name moved out and not yet replaced is then
/// never restored).
#[test]
fn rollback_from_a_half_moved_install_uses_the_digests_not_the_phase() {
    let Some(probe) = Install::new("half-probe") else {
        return;
    };
    let old_count = probe.inventories.old_present.len();
    let all = probe.inventories.forward_moves().len();
    drop(probe);
    let nonce = Nonce::new([0x66; 32]);
    for (moved, phase) in [
        (2, "moving"),
        (old_count + 1, "moving"),
        (old_count + 1, "trial"),
        (all, "intent"),
    ] {
        let Some(install) = Install::new(&format!("half-{moved}-{phase}")) else {
            return;
        };
        install.arm();
        install.move_first(moved);
        let journal_phase = match phase {
            "moving" => Phase::Moving,
            "trial" => {
                let trial = install.start_trial();
                install.children.end(trial.pid);
                Phase::Trial {
                    nonce,
                    process: trial,
                    began_ms: now_ms(),
                }
            }
            _ => Phase::RollbackIntent {
                trial: None,
                trial_started: false,
            },
        };
        install.write(journal_phase);
        let (code, world) = recovered(
            &install,
            limits(20_000, 20_000),
            install.world(Trial::Silent),
        );
        let case = format!("{moved} moves, {phase}");
        assert_eq!(code, 0, "{case}: {:?}", world.said);
        assert_eq!(
            install.on_disk().body.phase,
            Phase::Retired {
                outcome: Outcome::RolledBack,
                untried: phase != "trial",
            },
            "{case}: {:?}",
            world.said
        );
        rolled_back_on_disk(&install);
        let moved_in = moved.saturating_sub(old_count);
        for (at, (name, bytes)) in install.new.iter().enumerate() {
            let place = if at < moved_in {
                Place::RolledOut
            } else {
                Place::Set
            };
            assert_eq!(
                std::fs::read(install.folder(place).join(name))
                    .ok()
                    .as_ref(),
                Some(bytes),
                "{case}: the new `{name}` is in {place:?}"
            );
        }
        assert_eq!(
            world.opened,
            vec![(install.installed.clone(), failed_then(&install, &handed()))],
            "{case}"
        );
    }
}

/// RED (U-24) — **a move back that fails is `Stuck`, with everything kept**:
/// the failure recorded (`Stuck{last_error, attempts: 1}`), the journal still
/// `destructive`, the `Run` value in place, every old file in exactly one of
/// the install and `backup\`, every new file in exactly one of the install and
/// `rolledout\`, the file in the way untouched — and, the install holding
/// neither whole set, the rescue copy opens with `--update-failed` and the
/// handed line.
///
/// §C.5: "rollback fails → `Stuck`, backups kept, entrance kept"; (b).2 W10.
///
/// MUTATION: in `Txn::move_back`, answer a failed move with `Ended::Stuck`
/// without recording `RollbackFailed`.
#[test]
fn a_failed_move_back_is_stuck_with_everything_kept() {
    let Some(install) = Install::new("stuck") else {
        return;
    };
    flipped_at(
        &install,
        Phase::RollbackIntent {
            trial: None,
            trial_started: false,
        },
    );
    let mut world = install.world(Trial::Silent);
    let folder = install.install();
    let taken = Arc::new(Mutex::new(None::<String>));
    let keep = Arc::clone(&taken);
    world.on_move = Some(Box::new(move |done| {
        let mut kept = keep.lock().unwrap();
        if done.to == Place::RolledOut && kept.is_none() {
            // The new file is out; something takes its name before the old
            // one comes back.
            std::fs::write(folder.join(&done.name), b"somebody else's file").unwrap();
            *kept = Some(done.name.clone());
        }
    }));
    let (code, world) = recovered(&install, limits(20_000, 20_000), world);
    assert_eq!(code, 0, "{:?}", world.said);
    let name = taken.lock().unwrap().clone().expect("a new file went out");
    let Phase::Stuck {
        attempts,
        last_error,
        ..
    } = install.on_disk().body.phase
    else {
        panic!("{:?} {:?}", install.on_disk().body.phase, world.said);
    };
    assert_eq!(attempts, 1);
    assert!(last_error.contains(&name), "{last_error}");
    assert_eq!(install.header().class, Class::Destructive);
    assert!(install.registry.holds(install.txn), "the Run value is kept");
    assert_eq!(
        std::fs::read(install.install().join(&name)).unwrap(),
        b"somebody else's file"
    );
    for (name, bytes) in &install.old {
        let places = [Place::Install, Place::Backup]
            .iter()
            .filter(|place| {
                std::fs::read(install.folder(**place).join(name))
                    .ok()
                    .as_ref()
                    == Some(bytes)
            })
            .count();
        assert_eq!(places, 1, "the old `{name}` is kept, once");
    }
    for (name, bytes) in &install.new {
        let places = [Place::Install, Place::RolledOut]
            .iter()
            .filter(|place| {
                std::fs::read(install.folder(**place).join(name))
                    .ok()
                    .as_ref()
                    == Some(bytes)
            })
            .count();
        assert_eq!(places, 1, "the new `{name}` is kept, once");
    }
    assert!(said_at(&world, "the update is incomplete").is_some());
    assert_eq!(
        world.opened,
        vec![(install.rescue.clone(), failed_then(&install, &handed()))]
    );
}

/// RED (U-42b) — **W10: a new file held open with no sharing stops the
/// rollback before anything moves back, and the reason names the held file
/// — never the collision its old file would have met.**
///
/// 0.4.6's D-9: the held file hashed to nothing, the rollback took it for
/// absent, moved the old file back onto its name and stopped at
/// `the move of folio.msix to Install: … os error 183`. The hold here is the
/// clean machine's (read, share none), on the real recovery road.
///
/// MUTATION: in `update_txn::rollback_moves`, skip the `unread` check — the
/// rollback moves and stops at the collision (`os error 183`).
#[test]
fn a_new_file_held_without_sharing_is_named_as_the_rollback_s_reason() {
    let Some(install) = Install::new("held-new") else {
        return;
    };
    flipped_at(
        &install,
        Phase::RollbackIntent {
            trial: None,
            trial_started: false,
        },
    );
    let name = a_new_name_besides_the_program(&install);
    let hold = install_flip::hold_unshared(&install.install().join(&name))
        .expect("the test holds the new file");
    let (code, world) = recovered(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    drop(hold);
    assert_eq!(code, 0, "{:?}", world.said);
    let Phase::Stuck { last_error, .. } = install.on_disk().body.phase else {
        panic!("{:?} {:?}", install.on_disk().body.phase, world.said);
    };
    assert_eq!(
        last_error,
        format!(
            "`{name}` in the install could not be read or moved out: held open by another program"
        ),
        "{:?}",
        world.said
    );
    assert!(
        install.holds(Place::Install, &install.new),
        "nothing moved back: the new set is still installed"
    );
    assert!(
        install.holds(Place::Backup, &install.old),
        "and the old one is still in the backup"
    );
}

/// RED (U-24) — **`Stuck` is tried again by every start, each failure counted,
/// and after the bound nothing more is tried**: two starts record attempts 2
/// and 3, the third records nothing and says the update is incomplete; the
/// `Run` value stays throughout, and every start still opens a Folio (the
/// rescue copy: the install holds neither whole set).
///
/// (b).2 W10: "W9 again at every logon and every start"; U-29's bound, 3.
///
/// MUTATION: in `update_txn::decide`, drop the `GiveUp` arm.
#[test]
fn stuck_is_retried_at_the_next_start_and_stops_after_the_bound() {
    let Some(install) = Install::new("bound") else {
        return;
    };
    flipped_at(&install, stuck(1));
    // Somebody else's file where a new one was: never moved, so every
    // rollback stops there.
    let foreign = install
        .install()
        .join(a_new_name_besides_the_program(&install));
    std::fs::write(&foreign, b"somebody else's file").unwrap();
    for expected in 2..=STUCK_ATTEMPT_LIMIT {
        let (code, world) = recovered(
            &install,
            limits(20_000, 20_000),
            install.world(Trial::Silent),
        );
        assert_eq!(code, 0, "{:?}", world.said);
        let Phase::Stuck { attempts, .. } = install.on_disk().body.phase else {
            panic!("{:?}", install.on_disk().body.phase);
        };
        assert_eq!(attempts, expected, "{:?}", world.said);
        assert!(install.registry.holds(install.txn));
        assert_eq!(
            world.opened,
            vec![(install.rescue.clone(), failed_then(&install, &handed()))]
        );
    }
    let journal = std::fs::read(install.home.journal()).unwrap();
    let (code, world) = recovered(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    assert_eq!(
        std::fs::read(install.home.journal()).unwrap(),
        journal,
        "at the bound nothing is recorded"
    );
    assert!(
        said_at(&world, "incomplete after 3 attempts").is_some(),
        "{:?}",
        world.said
    );
    assert!(install.registry.holds(install.txn));
    assert_eq!(
        std::fs::read(&foreign).unwrap(),
        b"somebody else's file",
        "never moved"
    );
    assert_eq!(
        world.opened,
        vec![(install.rescue.clone(), failed_then(&install, &handed()))]
    );
}

/// RED (U-24) — **a rollback that leaves `Stuck` with every new file still
/// installed starts the new build only as a trial over `Stuck` — recorded as
/// `RetrialBegan`, with `--update-failed` and the handed line after its words
/// — and that trial's receipt commits the transaction forward**: `Committed`,
/// the entrance removed, `Retired{Committed}`, the new set installed; nothing
/// else is started (the trial is the window).
///
/// U-29b's ruling 3, adopted on Windows (the coordinator's ruling 3 of U-24):
/// "`Stuck` with the new set live → the new build as a trial
/// (`RetrialBegan`), its receipt commits forward (W8)".
///
/// MUTATION: in `Txn::retry_as_trial`, return `ended` without starting the
/// trial.
#[test]
fn stuck_with_the_new_set_live_starts_it_as_a_trial_and_its_receipt_commits() {
    let Some(install) = Install::new("retrial") else {
        return;
    };
    flipped_at(&install, stuck(1));
    block_rolledout(&install);
    let (_, world) = recovered(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Answers),
    );
    assert!(
        wrote(&world).ends_with("[Stuck, Stuck, Committed, Retired]"),
        "{:?}",
        world.said
    );
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::Committed,
            untried: false,
        }
    );
    assert!(install.holds(Place::Install, &install.new));
    assert!(!install.registry.holds(install.txn));
    assert!(world.opened.is_empty(), "the trial is the window");
    assert_eq!(world.launched.len(), 1, "{:?}", world.said);
    let args = &world.launched[0];
    assert_eq!(args[0], OsString::from(cli::UPDATE_TRIAL_FLAG));
    assert_eq!(args[1], OsString::from(install.txn.to_string()));
    assert_eq!(args[3..], failed_then(&install, &handed())[..]);
}

/// A start's world: its lines, and the entrance through the real door over
/// the test's registry.
struct StartWorld {
    said: Vec<String>,
    registry: Memory,
}

impl crate::update_startup::World for StartWorld {
    fn say(&mut self, line: &str) {
        self.said.push(line.to_owned());
    }

    fn spawn_detached(&mut self, program: &Path, _args: &[OsString]) -> io::Result<()> {
        panic!("a start that retires hands nothing over: {program:?}")
    }

    fn retire_entrance(&mut self, txn: TxnId) -> Result<(), String> {
        logon_hook::disarm_in(&mut self.registry, KEY, txn.bytes())
            .map_err(|refusal| refusal.to_string())
    }

    fn mounts_under(&mut self, _folder: &Path) -> Result<Vec<PathBuf>, String> {
        Ok(Vec::new())
    }

    fn on_a_worker(&mut self, _job: crate::update_startup::OffThread) -> io::Result<()> {
        panic!("nothing is mounted on Windows")
    }
}

/// RED (U-24) — **`RolledBack` left by a dead holder is retired by the next
/// lock holder — the `Run` value removed, `Retired{RolledBack}` — and the old
/// build is started with `--update-failed <journal>`; the start that makes
/// then retires the terminal journal and deletes `H\<txn>`, and its card says
/// the previous version is restored.**
///
/// (b).2 W11: "remove the Run value (flushed); relaunch the installed
/// `folio.exe` with `--update-failed`; class `terminal`; later starts delete
/// `H\<txn>`".
///
/// MUTATION: in `Txn::finish_rollback`, skip removing the entrance.
#[test]
fn rolled_back_is_retired_at_the_next_start() {
    let Some(install) = Install::new("retired") else {
        return;
    };
    install.arm();
    install.write(Phase::RolledBack { untried: false });
    let (code, world) = recovered_at_logon(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::RolledBack,
            untried: false,
        }
    );
    assert!(
        !install.registry.holds(install.txn),
        "the Run value is gone"
    );
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "at logon, after a finished rollback, the old build opens with its card"
    );
    let folder = install.home.transaction(install.txn);
    assert!(folder.is_dir(), "the rescue folder is the next start's");

    let journal = install.home.journal();
    let argv = failed_then(&install, &[]);
    let start = crate::update_startup::Start {
        own_exe: &install.installed,
        home: &install.home,
        argv: &argv,
        trial: None,
        failed: Some(&journal),
    };
    let mut starting = StartWorld {
        said: Vec::new(),
        registry: install.registry.clone(),
    };
    let verdict = crate::update_startup::run(&start, &mut starting);
    let crate::update_startup::Verdict::Continue { failed, .. } = verdict else {
        panic!("the start continues: {:?}", starting.said);
    };
    assert!(
        matches!(failed, Some(crate::update_job::Failure::RolledBack)),
        "{failed:?}"
    );
    assert!(!folder.exists(), "H\\<txn> is deleted by the start");
    assert!(!journal.exists(), "and the journal after it");
}

/// RED (U-24) — **the trial is asked to quit before it is ended, and only the
/// process whose pid, creation time and image are all the journal's is asked
/// anything**: the recorded trial is asked to quit, given its grace, then
/// ended; another process of the same image, one recorded with another
/// creation time, and one running the same bytes from another path all run on
/// — and the rollback goes on around them.
///
/// The journal says `RollbackIntent` over an install where nothing was moved,
/// so the trial runs from `<install>\folio.exe` and the rollback moves nothing:
/// what is pinned is who is asked. (A scanner holds a just-started image for a
/// moment, and a move of it is then refused — the moves are pinned by
/// `a_failed_health_reverses_the_moves_and_relaunches_the_old_build`.)
///
/// The coordinator's ruling 1: "the process is identified by pid **and**
/// creation time **and** image path = `<install>\folio.exe` … never touch a
/// process whose identity does not match all three". The synthetic programs
/// open no window, so the ask reaches no window and the grace runs out.
///
/// MUTATION: in `update_apply::stop_trial`, ask `Ask::End` first (part A
/// red); in `install_flip::runs_from`, answer from the pid and start time
/// alone (part C red: the stranger is then asked and ended — the image is
/// checked both where the trial is judged alive and inside `ask`, and both
/// read `runs_from`).
#[test]
fn the_trial_is_asked_to_quit_before_it_is_ended_and_only_by_its_identity() {
    // A: the recorded trial, beside another process of the same image.
    let Some(install) = Install::new("ask") else {
        return;
    };
    install.arm();
    let trial = install.start_trial();
    let beside = install.start_trial();
    install.write(Phase::RollbackIntent {
        trial: Some(trial),
        trial_started: false,
    });
    let began = Instant::now();
    let (code, world) = recovered(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    let quit = said_at(&world, &format!("trial {} is asked to quit", trial.pid));
    let end = said_at(&world, &format!("trial {} is asked to end", trial.pid));
    assert!(
        quit.is_some() && end.is_some() && quit < end,
        "{:?}",
        world.said
    );
    assert!(began.elapsed() >= GRACE, "the grace ran before the end");
    assert!(!runs(trial), "the trial is ended");
    assert!(runs(beside), "the other process of its image runs on");
    rolled_back_on_disk(&install);

    // B: the journal's pid with another creation time.
    let Some(install) = Install::new("ask-time") else {
        return;
    };
    install.arm();
    let running = install.start_trial();
    install.write(Phase::RollbackIntent {
        trial: Some(TrialProcess {
            pid: running.pid,
            started: running.started + 1,
        }),
        trial_started: false,
    });
    let (code, world) = recovered(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(said_at(&world, "is asked to").is_none(), "{:?}", world.said);
    assert!(
        runs(running),
        "a process with another creation time runs on"
    );
    rolled_back_on_disk(&install);

    // C: the journal's pid and creation time, running the same bytes from
    // another path.
    let Some(install) = Install::new("ask-image") else {
        return;
    };
    install.arm();
    let elsewhere = install.install().join("elsewhere").join("folio.exe");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    std::fs::copy(&install.installed, &elsewhere).unwrap();
    let pid = install.children.start(&elsewhere, &[]);
    let stranger = TrialProcess {
        pid,
        started: install_flip::started_of(pid).expect("it runs"),
    };
    install.write(Phase::RollbackIntent {
        trial: Some(stranger),
        trial_started: false,
    });
    let (code, world) = recovered(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(said_at(&world, "is asked to").is_none(), "{:?}", world.said);
    assert!(runs(stranger), "a process of another image runs on");
    rolled_back_on_disk(&install);
}

/// What a case of [`every_phase_left_by_a_dead_applier_still_opens_folio`]
/// expects to be started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opened {
    /// The installed build, with the handed line only.
    Installed,
    /// The installed build with `--update-failed <journal>` first.
    InstalledFailed,
    /// Nothing but the trial over `Stuck` this recovery started.
    TrialOnly,
    /// The installed build — the new set, not committed — as a trial with a
    /// nonce no journal records, then `--update-failed <journal>`.
    Trial,
    /// The rescue copy with `--update-failed <journal>`: the fallback.
    Rescue,
}

/// RED (U-23; U-24, the coordinator's rulings 3 and 1) — **whatever phase a
/// dead applier left the journal in, a double-click of Folio opens a window,
/// by U-29b's rules**: the ordinary start hands itself to the rescue build
/// (`--update-recover --then-launch`), which finishes the transaction and
/// starts exactly one Folio with the handed command line —
///
/// | left at | recovery | opens |
/// |---|---|---|
/// | `Handoff` | → `Prepared` | the installed (old) build |
/// | `Armed`, the `Run` value there | value removed, → `Prepared` | the installed (old) build |
/// | `Moving`, some moves done | rolled back → `Retired{RolledBack}` | the installed (old) build, `--update-failed` |
/// | `Trial`, the trial gone, no receipt | rolled back | the installed (old) build, `--update-failed` |
/// | `Trial`, the trial alive, no receipt | waits to its deadline, stops it, rolls back | the installed (old) build, `--update-failed` |
/// | `Trial`, a matching receipt | → `Committed` → `Retired` | the installed (new) build |
/// | `Committed` | → `Retired` | the installed (new) build |
/// | `RollbackIntent` | rolled back | the installed (old) build, `--update-failed` |
/// | `RolledBack` | retired | the installed (old) build, `--update-failed` |
/// | `Stuck`, the new set installed, the rollback refused | a trial over `Stuck` | that trial only |
/// | `Stuck`, a foreign file in the install | `Stuck` again | the rescue copy, `--update-failed` (the fallback) |
/// | `RollbackIntent`, the new set installed, a running copy holding the admission | nothing recorded (`RollbackWaits`) | the installed (new) build **only as a trial**, `--update-failed` |
/// | `Stuck` at its bound, the old set whole | nothing tried (`GaveUp`) | the installed (old) build, `--update-failed` |
///
/// U-23 opened the rescue copy for every still-`destructive` phase; now that
/// the rollback exists it opens only where the install holds neither whole
/// set.
///
/// MUTATION: in `opens_now`, answer `Opens::Rescue` for every `destructive`
/// header (U-23's rule).
#[test]
fn every_phase_left_by_a_dead_applier_still_opens_folio() {
    use Opened::{Installed, InstalledFailed, Rescue, Trial as AsTrial, TrialOnly};
    let rolled_back = Some(Outcome::RolledBack);
    let committed = Some(Outcome::Committed);
    let cases: [(&str, PhaseKind, Option<Outcome>, Opened); 13] = [
        ("handoff", PhaseKind::Prepared, None, Installed),
        ("armed", PhaseKind::Prepared, None, Installed),
        ("moving", PhaseKind::Retired, rolled_back, InstalledFailed),
        (
            "trial-gone",
            PhaseKind::Retired,
            rolled_back,
            InstalledFailed,
        ),
        (
            "trial-alive",
            PhaseKind::Retired,
            rolled_back,
            InstalledFailed,
        ),
        ("trial-answered", PhaseKind::Retired, committed, Installed),
        ("committed", PhaseKind::Retired, committed, Installed),
        ("intent", PhaseKind::Retired, rolled_back, InstalledFailed),
        (
            "rolled-back",
            PhaseKind::Retired,
            rolled_back,
            InstalledFailed,
        ),
        ("stuck-new", PhaseKind::Stuck, None, TrialOnly),
        ("stuck-mix", PhaseKind::Stuck, None, Rescue),
        ("waits", PhaseKind::RollbackIntent, None, AsTrial),
        ("bound-old", PhaseKind::Stuck, None, InstalledFailed),
    ];
    for (tag, ends, outcome, opens) in cases {
        let Some(install) = Install::new(tag) else {
            return;
        };
        let nonce = Nonce::new([0x55; 32]);
        match tag {
            "handoff" => {}
            "armed" => {
                install.arm();
                install.write(Phase::Armed);
            }
            "moving" => {
                install.arm();
                install.move_first(3);
                install.write(Phase::Moving);
            }
            "trial-gone" | "trial-alive" | "trial-answered" => {
                install.arm();
                install.move_first(install.inventories.forward_moves().len());
                let trial = install.start_trial();
                if tag == "trial-gone" {
                    install.children.end(trial.pid);
                }
                if tag == "trial-answered" {
                    install.receipt(nonce, nonce, trial.pid);
                }
                install.write(Phase::Trial {
                    nonce,
                    process: trial,
                    began_ms: now_ms(),
                });
            }
            "committed" => flipped_at(&install, Phase::Committed),
            "intent" => flipped_at(
                &install,
                Phase::RollbackIntent {
                    trial: None,
                    trial_started: false,
                },
            ),
            "rolled-back" => {
                install.arm();
                install.write(Phase::RolledBack { untried: false });
            }
            "stuck-new" => {
                flipped_at(&install, stuck(1));
                block_rolledout(&install);
            }
            "waits" => flipped_at(
                &install,
                Phase::RollbackIntent {
                    trial: None,
                    trial_started: false,
                },
            ),
            "bound-old" => {
                install.arm();
                install.write(stuck(STUCK_ATTEMPT_LIMIT));
            }
            _ => {
                flipped_at(&install, stuck(1));
                let name = a_new_name_besides_the_program(&install);
                std::fs::write(install.install().join(name), b"somebody else's file").unwrap();
            }
        }
        // A running copy holds the admission shared: the rollback waits for
        // it, briefly here, and records nothing.
        let copy = (tag == "waits").then(|| {
            std::fs::write(install.home.admission(), b"").unwrap();
            install_txn::try_hold(&install.home.admission(), Hold::Shared)
                .unwrap()
                .unwrap()
        });
        let old_within = if copy.is_some() { 1_500 } else { 20_000 };
        let (code, world) = recovered(
            &install,
            limits(old_within, 1_500),
            install.world(Trial::Silent),
        );
        drop(copy);
        let phase = install.on_disk().body.phase;
        assert_eq!(phase.kind(), ends, "{tag}: {:?}", world.said);
        if let Some(outcome) = outcome {
            assert_eq!(
                phase,
                Phase::Retired {
                    outcome,
                    untried: matches!(tag, "moving" | "intent")
                },
                "{tag}"
            );
        }
        let expected = match opens {
            Installed => vec![(install.installed.clone(), handed())],
            InstalledFailed => vec![(install.installed.clone(), failed_then(&install, &handed()))],
            Rescue => vec![(install.rescue.clone(), failed_then(&install, &handed()))],
            TrialOnly | AsTrial => Vec::new(),
        };
        if opens == AsTrial {
            assert_eq!(world.opened.len(), 1, "{tag}: {:?}", world.said);
            let (program, args) = &world.opened[0];
            assert_eq!(program, &install.installed, "{tag}");
            assert_eq!(args[0], OsString::from(cli::UPDATE_TRIAL_FLAG), "{tag}");
            assert_eq!(args[1], OsString::from(install.txn.to_string()), "{tag}");
            assert_eq!(args[3..], failed_then(&install, &handed())[..], "{tag}");
        } else {
            assert_eq!(world.opened, expected, "{tag}: {:?}", world.said);
        }
        if opens == TrialOnly {
            assert_eq!(world.launched.len(), 1, "{tag}: {:?}", world.said);
            assert_eq!(world.launched[0][0], OsString::from(cli::UPDATE_TRIAL_FLAG));
        } else {
            assert_eq!(code, 0, "{tag}: {:?}", world.said);
            assert!(world.launched.is_empty(), "{tag}");
        }
        match (outcome, opens) {
            (Some(Outcome::Committed), _) | (None, AsTrial) => {
                assert!(install.holds(Place::Install, &install.new), "{tag}");
            }
            (Some(Outcome::RolledBack), _) | (None, Installed | InstalledFailed) => {
                assert!(install.holds(Place::Install, &install.old), "{tag}");
            }
            _ => {}
        }
        if matches!(ends, PhaseKind::Prepared | PhaseKind::Retired) {
            assert!(!install.registry.holds(install.txn), "{tag}");
        } else {
            assert!(install.registry.holds(install.txn), "{tag}");
        }
    }
}

/// RED (U-24, the coordinator's ruling 3; U-34) — **one rule for a live
/// applier at `Handoff`, on both platforms: the recovery leaves the handed-off
/// transaction to the live process the window's mark names — nothing
/// written, nothing waited for, nothing started (that process opens Folio);
/// a live process of the rescue image the mark does not name is not waited
/// for, and the recovery reverts and opens Folio itself.**
///
/// The mark is `H\<txn>\owner` (`update_apply::OWNER_FILE`), which the
/// applier takes before it waits for O's lock. U-24 inferred the applier from
/// an older process of the rescue image; that inference is gone — a P that
/// never took the mark is nobody's successor. U-23 let the lock go and waited
/// up to 180 s instead; that wait is removed.
///
/// MUTATION: in `recover`, leave a `Handoff` to any live process of the rescue
/// image, named or not.
#[test]
fn a_live_applier_at_handoff_is_left_alone_on_both_platforms() {
    let Some(install) = Install::new("alive") else {
        return;
    };
    let journal = std::fs::read(install.home.journal()).unwrap();
    let applier = install.children.start(&install.rescue, &[]);
    let applier = Running {
        pid: applier,
        started: install_flip::started_of(applier).expect("it runs"),
    };
    let road = |install: &Install| {
        let mut road = install.road(limits(20_000, 20_000));
        road.me = Running {
            pid: std::process::id(),
            started: u64::MAX,
        };
        road
    };
    let run = |road: Road| {
        let mut world = install.world(Trial::Silent);
        on_a_worker(move |worker| {
            let code =
                crate::update_recover::run_windows(worker, &road, Some(&handed()), &mut world);
            (code, world)
        })
    };

    // Named by the mark: left to it.
    assert!(
        crate::update_apply::take_the_window(
            None,
            &install.home,
            install.txn,
            applier,
            Instant::now() + crate::update_apply::ELECTION_WITHIN
        )
        .is_mine()
    );
    let began = Instant::now();
    let (code, world) = run(road(&install));
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "nothing is waited for: {:?}",
        began.elapsed()
    );
    assert_eq!(code, 1, "{:?}", world.said);
    assert!(
        said_at(&world, &format!("{} has the update's window", applier.pid)).is_some(),
        "{:?}",
        world.said
    );
    assert_eq!(
        std::fs::read(install.home.journal()).unwrap(),
        journal,
        "the live applier's transaction is left to it"
    );
    assert!(world.opened.is_empty(), "that applier opens Folio");
    nothing_moved(&install);

    // Not named — it never took the mark: not waited for.
    std::fs::remove_file(crate::update_apply::owner_path(&install.home, install.txn)).unwrap();
    let (code, world) = run(road(&install));
    assert_eq!(code, 0, "{:?}", world.said);
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Prepared {
            deferred_launches: 0
        }
    );
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), handed())],
        "{:?}",
        world.said
    );
}

// ── a journal write refused (U-34) ──────────────────────────────────────────

/// **The installation at `Armed`**, as a dead attempt or an applier that
/// just armed leaves it: the journal `Armed` and the `Run` value written.
fn armed(tag: &str) -> Option<Install> {
    let install = Install::new(tag)?;
    // These tests enter in the middle of one live applier's road. The mark is
    // its durable election record; an absent mark at `Armed` instead denotes
    // a later contender, which must stand down.
    install.claim_window();
    install.write(Phase::Armed);
    install.arm();
    Some(install)
}

/// RED (U-34) — **a journal write refused for a moment, because another
/// program holds `journal.json` open without delete sharing, is asked again:
/// `Moving` is recorded after the retry and the update completes.**
///
/// The clean VM's rows W4 and W12: `install_txn rename …\journal.json:
/// Access is denied (os error 5)` at `Armed` → `Moving`, the first refusal
/// final, and a person who pressed *Restart to update* had no Folio until the
/// next logon. The test first shows the harness's open is that refusal — a
/// replacing rename over the held file is refused, and the door names it as
/// one that goes away — then holds the file through the applier's first
/// write: a running copy holds the admission shared so the applier waits
/// just before `Admitted`, lets it go, and the file is let go 300 ms later.
///
/// MUTATION: in `update_apply::write_journal`, return the first failure
/// (no retry).
#[test]
fn a_journal_write_refused_for_a_moment_is_asked_again_and_the_update_completes() {
    let Some(install) = armed("refused-once") else {
        return;
    };
    let held =
        bt_platform::trust_harness::hold_without_delete_sharing(&install.home.journal()).unwrap();
    let journal = std::fs::read(install.home.journal()).unwrap();
    let refused = install_txn::durable_write(&install.home.journal(), &journal)
        .expect_err("a replacing rename over the held journal is refused");
    assert!(refused.refused_while_open(), "{refused}");

    std::fs::write(install.home.admission(), b"").unwrap();
    let running = install_txn::try_hold(&install.home.admission(), Hold::Shared)
        .unwrap()
        .unwrap();
    let applier = start(
        install.road(limits(20_000, 20_000)),
        install.txn,
        install.applier,
        install.world(Trial::Answers),
    );
    std::thread::sleep(Duration::from_millis(200));
    drop(running);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Armed,
        "nothing gets past the held journal"
    );
    drop(held);
    let (ended, world) = applier.join().unwrap();
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Moving, Trial, Committed, Retired]"),
        "{:?}",
        world.said
    );
    assert!(install.holds(Place::Install, &install.new), "the new build");
}

/// RED (U-34) — **a journal write refused for good ends the applier
/// `Failed`, and the applier still opens Folio: the installed build with
/// `--update-failed <journal>`, while the journal stays `Armed` and the `Run`
/// value stays for the next start or logon; so does a transaction lock that
/// cannot be opened.**
///
/// The owner's ruling of 2026-09-25 (every phase opens Folio) and U-23's
/// open point 6 ("the person sees Folio quit and not come back"): a failed
/// road owes what a person's start would, read from the disk — here the old
/// set whole under a `destructive` header, so the installed build with the
/// *Update incomplete.* card. The started build is recorded, never made.
///
/// MUTATION: in `update_apply_windows::apply`, tell the guard nobody is
/// waiting after `Ended::Failed` (U-23's "a failed applier owes no window").
#[test]
fn a_journal_write_refused_for_good_still_opens_the_installed_build_with_the_incomplete_card() {
    let Some(install) = armed("refused-for-good") else {
        return;
    };
    let held =
        bt_platform::trust_harness::hold_without_delete_sharing(&install.home.journal()).unwrap();
    let began = Instant::now();
    let (ended, world) = applied(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Answers),
    );
    let waited = began.elapsed();
    drop(held);
    let Ended::Failed(why) = &ended else {
        panic!("{ended:?}: {:?}", world.said);
    };
    assert!(why.contains("rename"), "{why}");
    assert!(
        waited >= crate::update_apply::JOURNAL_WRITE_WITHIN,
        "asked again for the whole bound: {waited:?}"
    );
    assert_eq!(install.on_disk().body.phase, Phase::Armed);
    assert!(install.registry.holds(install.txn), "the Run value is kept");
    nothing_moved(&install);
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "{:?}",
        world.said
    );

    // The lock itself cannot be opened: a folder where it should be.
    let Some(install) = Install::new("unlockable") else {
        return;
    };
    let journal = std::fs::read(install.home.journal()).unwrap();
    std::fs::create_dir(install.home.lock()).unwrap();
    let (ended, world) = applied(
        &install,
        limits(2_000, 20_000),
        install.world(Trial::Answers),
    );
    assert!(matches!(ended, Ended::Failed(_)), "{ended:?}");
    assert_eq!(std::fs::read(install.home.journal()).unwrap(), journal);
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "{:?}",
        world.said
    );
}

/// RED (U-34) — **a trial whose start the journal cannot record is ended
/// before anything else happens, and the road goes on as for a trial that did
/// not start: `RollbackIntent`, the rollback, and the old build with
/// `--update-failed`; `Trial` is never written and no process of the new
/// build is left running.**
///
/// The trial's pid and creation time exist only once the process does, so
/// `TrialBegan` cannot be durable before the start without a new phase; the
/// rights table gives the applier `EndTrial` over `Moving` for this one
/// case. Here the journal is held from the trial's launch until the applier
/// says the trial could not be recorded — past the whole retry bound.
///
/// MUTATION: in `Txn::trial`, return the `TrialBegan` record's error with
/// `?` (neither ending the trial nor declaring the rollback).
#[test]
fn a_trial_whose_start_cannot_be_recorded_is_ended_and_rolled_back() {
    let Some(install) = Install::new("unrecorded") else {
        return;
    };
    let mut world = install.world(Trial::Silent);
    world.hold_at_launch = true;
    let (ended, world) = applied(&install, limits(20_000, 20_000), world);
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert_eq!(world.launched.len(), 1, "one trial");
    let unrecorded = said_at(&world, "could not be recorded").expect("said");
    let asked = said_at(&world, "is asked to").expect("the trial is stopped");
    assert!(unrecorded < asked, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Armed, Moving, RollbackIntent, RolledBack, Retired]"),
        "{:?}",
        world.said
    );
    assert!(
        install_flip::running_from(&install.installed)
            .unwrap()
            .is_empty(),
        "no process of the new build runs"
    );
    rolled_back_on_disk(&install);
    assert!(!install.registry.holds(install.txn));
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "{:?}",
        world.said
    );
}

// ── the trial's gaps (U-37, U-38) ───────────────────────────────────────────

/// **D-14's shape, as the clean VM's row W14 (120 s) met it**: the journal
/// held without delete sharing from the trial's launch through the whole road
/// — `TrialBegan` refused, the trial ended, the rollback's declaration refused
/// too — so the applier ends `Failed` at `Moving` and its exit guard starts
/// the new build again as a trial with a nonce no journal records. That
/// second trial is then made as the product makes it: a process of the
/// installed program, and its receipt at its own nonce (its first pane text).
/// The handle is let go. What is returned: the installation, the second
/// trial, and the applier's world.
fn failed_with_the_journal_held(tag: &str) -> Option<(Install, TrialProcess, Fake)> {
    let install = Install::new(tag)?;
    let mut world = install.world(Trial::Silent);
    world.hold_at_launch = true;
    world.keep_held = true;
    let (ended, mut world) = applied(&install, limits(20_000, 20_000), world);
    let Ended::Failed(why) = &ended else {
        panic!("{ended:?}: {:?}", world.said);
    };
    assert!(why.contains("rename"), "{why}");
    // The rule fired: the trial it launched was ended ...
    let unrecorded = said_at(&world, "could not be recorded").expect("said");
    let asked = said_at(&world, "is asked to").expect("the trial is stopped");
    assert!(unrecorded < asked, "{:?}", world.said);
    assert_eq!(install.on_disk().body.phase, Phase::Moving);
    assert!(
        install.holds(Place::Install, &install.new),
        "the new set is live"
    );
    // ... and the guard started the new build again, as a trial nobody
    // records: the process D-14 found running.
    assert_eq!(world.opened.len(), 1, "{:?}", world.said);
    let (program, words) = &world.opened[0];
    assert_eq!(program, &install.installed);
    assert_eq!(words[0], OsString::from(cli::UPDATE_TRIAL_FLAG));
    assert_eq!(words[3], OsString::from(cli::UPDATE_FAILED_FLAG));
    let nonce = Nonce::parse(&words[2].to_string_lossy()).unwrap();
    let second = install.start_trial();
    install.receipt(nonce, nonce, second.pid);
    world.held = None;
    Some((install, second, world))
}

/// RED (U-37) — **a trial whose start the journal could not record, running
/// with its receipt written, is recorded by the next lock holder and its
/// receipt commits the update: `[Trial, Committed, Retired]`, the new set
/// kept, the entrance removed, and that trial left running as the window —
/// nothing else started.**
///
/// D-14 (the clean VM, A7, row W14 with a 120 s hold): the rule U-34 wrote
/// fired — the trial 7340 was asked to quit and ended — but the rollback's
/// declaration was refused by the same handle, the road ended `Failed` at
/// `Moving`, and the exit guard started the new build again as a trial no
/// journal records (pid 940), which nothing would ever commit or end. The
/// recovery build — here as the entrance at logon starts it, which is also
/// the line the trial's own watchdog starts it with — now records such a
/// trial rather than rolling back under it.
///
/// MUTATION: in `update_apply::before_deciding`, answer `Decide` for
/// `Survey::Adoptable` (never record it).
#[test]
fn a_running_trial_the_journal_could_not_record_is_recorded_and_committed_by_the_next_holder() {
    // The applier's world is kept: its drop ends every process of the test.
    let Some((install, second, _applier)) = failed_with_the_journal_held("d14") else {
        return;
    };
    let (code, world) = recovered_at_logon(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Trial, Committed, Retired]"),
        "{:?}",
        world.said
    );
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::Committed,
            untried: false,
        }
    );
    assert!(install.holds(Place::Install, &install.new), "the new build");
    assert!(!install.registry.holds(install.txn), "the entrance is gone");
    assert!(runs(second), "the trial runs on as the window");
    assert!(
        world.launched.is_empty(),
        "no second trial: {:?}",
        world.said
    );
    assert!(world.opened.is_empty(), "nothing started: {:?}", world.said);
}

/// RED (U-37; the macOS rehearsal's defect 9, its Windows twin) — **a
/// person's start while that unrecorded trial runs commits the update and
/// starts nothing beside it**: the recovery the start hands itself to records
/// the running trial, whose receipt commits, rather than rolling back a
/// working new build under it.
///
/// The rehearsal's D14, second half: a new start while the unrecorded trial's
/// window was open recorded a second trial, which waited for the data
/// directory the first one held and died unanswered — and the recovery rolled
/// back a healthy 0.4.7.
///
/// MUTATION: in `update_apply::before_deciding`, answer `Decide` for
/// `Survey::Adoptable` (never record it).
#[test]
fn a_start_beside_an_unrecorded_trial_commits_it_and_starts_nothing_more() {
    // The applier's world is kept: its drop ends every process of the test.
    let Some((install, second, _applier)) = failed_with_the_journal_held("defect9") else {
        return;
    };
    let (code, world) = recovered(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Trial, Committed, Retired]"),
        "{:?}",
        world.said
    );
    assert!(runs(second), "the trial runs on as the window");
    assert!(
        world.launched.is_empty(),
        "no second trial: {:?}",
        world.said
    );
    assert!(world.opened.is_empty(), "{:?}", world.said);
}

/// PIN (U-40) — **the Windows applier's trial is the process its own launch
/// created, whatever person's start of the new build runs beside it**: a
/// process of the installed program started just before the trial is never
/// recorded; it leaves at once, as a start that hands itself to the recovery
/// build does, the trial's receipt commits, and the trial is the one window.
/// The macOS road names its trial by the words it carries (LaunchServices
/// reports no pid); this is the Windows half of U-40's table, unchanged: the
/// pid is the launch's own.
///
/// The deadline is not this test's subject, and nothing it asserts waits for
/// it: the receipt the test writes ends the watch, or — under the mutation —
/// the person's end does ([`crate::update_apply::TRIAL_NOT_UNDER_TEST_MS`]). The person
/// leaves before the first assertion, so a red run is answered at once.
///
/// MUTATION: in `Txn::trial`, record the earliest-started process of the
/// installed program in place of the pid the launch answered.
#[test]
fn a_persons_start_beside_the_launch_is_never_the_recorded_trial() {
    let Some(install) = Install::new("beside") else {
        return;
    };
    let people: Arc<Mutex<Vec<u32>>> = Arc::default();
    let mut world = install.world(Trial::Silent);
    world.beside_launch = Some(Arc::clone(&people));
    let children = world.children.clone();
    let applier = start(
        install.road(limits(20_000, crate::update_apply::TRIAL_NOT_UNDER_TEST_MS)),
        install.txn,
        install.applier,
        world,
    );
    until_journal(&install, PhaseKind::Trial, &applier);
    let Phase::Trial { process, nonce, .. } = install.on_disk().body.phase else {
        unreachable!()
    };
    let person = people.lock().unwrap()[0];
    children.end(person);
    assert_ne!(process.pid, person, "the person's start is never the trial");
    install.receipt(nonce, nonce, process.pid);
    let (ended, world) = applier.join().unwrap();
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(
        world.opened.is_empty(),
        "the trial is the one window: {:?}",
        world.opened
    );
    assert!(runs(process), "the trial runs on");
}

/// PIN (U-38, the Windows twin of the macOS defect 2) — **a trial that dies
/// within its first second ends the applier's wait at once**: the applier
/// holds the trial's pid from its launch, records it, and sees it gone at its
/// next look — the rollback begins within moments, not at the 60 s deadline.
///
/// The clean VM's M9cut-style kill landed 4 ms too late to show it on the
/// Mac; here the trial ends by itself 300 ms after its launch, after `Trial`
/// is recorded.
///
/// MUTATION: in `Txn::watch`, answer the trial alive whatever the process
/// list says.
#[test]
fn a_trial_that_dies_in_its_first_second_ends_the_wait_at_once() {
    let Some(install) = Install::new("dies-soon") else {
        return;
    };
    let began = Instant::now();
    let (ended, world) = applied(
        &install,
        limits(20_000, 60_000),
        install.world(Trial::DiesSoon),
    );
    let took = began.elapsed();
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        wrote(&world).contains("Trial, RollbackIntent"),
        "recorded, then rolled back: {:?}",
        world.said
    );
    assert!(
        said_at(&world, "ended without a receipt").is_some(),
        "{:?}",
        world.said
    );
    assert!(
        took < Duration::from_secs(30),
        "at once, not the 60 s deadline: {took:?}"
    );
    rolled_back_on_disk(&install);
}

// ── the one exit guard (U-34) ───────────────────────────────────────────────

/// RED (U-34) — **an applier that never gets the transaction lock still opens
/// Folio**: `OldHeldTheLock`, the journal byte for byte as it was, and the
/// installed build started with `--update-failed <journal>` — the header is
/// `destructive` and the old set whole.
///
/// The verifier's table (`reports/U-31-W-verify.md`, "Every way a road process
/// leaves"): P's `OldHeldTheLock` left no window. Under the exit guard no
/// successor runs, so the disk names the start.
///
/// MUTATION: in `apply`, return before the guard leaves for
/// `Ended::OldHeldTheLock` (with the guard's drop disabled).
#[test]
fn an_applier_that_never_gets_the_lock_still_opens_folio() {
    let Some(install) = Install::new("lock-kept") else {
        return;
    };
    let journal = std::fs::read(install.home.journal()).unwrap();
    let held = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let (ended, world) = applied(&install, limits(600, 20_000), install.world(Trial::Answers));
    drop(held);
    assert_eq!(ended, Ended::OldHeldTheLock, "{:?}", world.said);
    assert_eq!(std::fs::read(install.home.journal()).unwrap(), journal);
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "{:?}",
        world.said
    );
}

/// **The window's mark held open by a scanner** (no delete sharing), naming a
/// process that is gone: an applier reads it as stale and claims, but every
/// replacement is refused before the rename, so the applier owns the duty
/// through `owner.lock` alone. `None` where the platform replaces an open
/// file.
fn mark_held_open(install: &Install) -> Option<std::fs::File> {
    let mark = crate::update_apply::owner_path(&install.home, install.txn);
    std::fs::create_dir_all(mark.parent().unwrap()).unwrap();
    std::fs::write(&mark, format!("{}:1", std::process::id())).unwrap();
    bt_platform::trust_harness::hold_without_delete_sharing(&mark).ok()
}

/// RED (T-UPDATE-LOCK-RACE round 4) — **an applier that owns the window only
/// through the election lock, and whose road stops at `Handoff` after it held
/// the transaction lock, opens the one window itself.** O keeps that lock
/// until its process ends, so O is gone by then, and it had stood down
/// against this applier's `owner.lock`. Driven through the product `apply`:
/// the mark's replacement is refused before the rename ([`mark_held_open`]),
/// and the journal names another applier's nonce, so the road stops at
/// `Handoff` with the transaction lock held.
///
/// MUTATION: in `ExitGuard::road_ended`, hand a lock-only duty back whether
/// or not the transaction lock was held (nothing is opened).
#[test]
fn a_lock_only_applier_that_stops_at_handoff_after_the_lock_opens_the_one_window() {
    let Some(install) = Install::new("lock-only-held") else {
        return;
    };
    let Some(scanner) = mark_held_open(&install) else {
        return;
    };
    let journal = std::fs::read(install.home.journal()).unwrap();
    let (ended, world) = match start(
        install.road(limits(20_000, 20_000)),
        install.txn,
        Nonce::new([0x55; 32]),
        install.world(Trial::Answers),
    )
    .join()
    {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    drop(scanner);
    assert!(
        said_at(&world, "owner.lock records the duty").is_some(),
        "the duty is lock-only: {:?}",
        world.said
    );
    assert!(matches!(ended, Ended::Refused(_)), "{ended:?}");
    assert_eq!(
        std::fs::read(install.home.journal()).unwrap(),
        journal,
        "the road stopped at Handoff"
    );
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "the applier opens the one window: {:?}",
        world.said
    );
    assert!(world.shown.is_empty(), "{:?}", world.shown);
}

/// RED (T-UPDATE-LOCK-RACE round 4) — **an applier that owns the window only
/// through the election lock, and whose road never held the transaction lock,
/// gives the duty back to O: it opens nothing, and O, finding the election
/// lock free and no live mark, opens the one window.** The transaction lock
/// is held by this test, standing for O, until the applier's one deadline has
/// passed; the applier's product `apply` runs through its own election.
///
/// MUTATION: in `update_apply_windows::apply`, drop the duty instead of
/// `guard.owns_window(duty)` (the applier and O each open a window).
#[test]
fn a_lock_only_applier_that_never_held_the_lock_gives_the_window_back_to_o() {
    let Some(install) = Install::new("lock-only-never") else {
        return;
    };
    let Some(scanner) = mark_held_open(&install) else {
        return;
    };
    let held = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let (ended, world) = applied(&install, limits(600, 20_000), install.world(Trial::Answers));
    drop(held);
    drop(scanner);
    assert!(
        said_at(&world, "owner.lock records the duty").is_some(),
        "the duty is lock-only: {:?}",
        world.said
    );
    assert_eq!(ended, Ended::OldHeldTheLock, "{:?}", world.said);
    assert!(
        world.opened.is_empty(),
        "the applier opens nothing: {:?}",
        world.opened
    );
    assert!(world.shown.is_empty(), "{:?}", world.shown);

    let mut starts = Starts::default();
    let left = Leaving::over(&install.home, install.txn, &install.data).leave(
        Running { pid: 1, started: 1 },
        &install.installed,
        &mut starts,
        None,
    );
    assert_eq!(left, Left::Started(install.installed.clone()));
    assert_eq!(
        starts.calls,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "O opens the one window"
    );
}

/// RED (U-34) — **a panic inside the applier's road still opens Folio**: the
/// exit guard is a `Drop`, so the panic unwinding through the road starts what
/// the disk names — here the journal still `Handoff`, the old set whole, the
/// installed build with `--update-failed` — after the transaction lock was let
/// go on the way up, and says the road left early.
///
/// The world panics in the entrance's write, inside the road; the test catches
/// the unwind and reads what was started. In the product the update doors'
/// panic hook writes the report and lets the panic unwind (`fn main`).
///
/// MUTATION: in `ExitGuard`'s `Drop`, do nothing.
#[test]
fn a_panic_inside_the_applier_still_opens_folio() {
    let Some(install) = Install::new("panics") else {
        return;
    };
    let road = install.road(limits(20_000, 20_000));
    let (txn, nonce) = (install.txn, install.applier);
    let mut world = install.world(Trial::Answers);
    world.panic_at_arm = true;
    let (panicked, world) = on_a_worker(move |worker| {
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            apply(worker, &road, txn, nonce, &mut world)
        }))
        .is_err();
        (panicked, world)
    });
    assert!(panicked, "the road panicked");
    assert_eq!(install.on_disk().body.phase.kind(), PhaseKind::Handoff);
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "{:?}",
        world.said
    );
    assert!(
        said_at(&world, "the road left early").is_some(),
        "{:?}",
        world.said
    );
    assert!(
        install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
            .unwrap()
            .is_some(),
        "the lock was let go on the way up"
    );
}

/// RED (U-34; round 2, Codex's review, blocker 4) — **the recovery run at
/// logon is the one exit that may start nothing, and only when it attempted
/// nothing**: over a `Committed` it retires, nobody is waiting and nothing is
/// started; over an `Armed` it reverts, and the old build opens — the Restart
/// before it never got a window (W11); and over a `Stuck` at its bound it
/// gives up — it attempted the transaction and failed — and the installed build
/// opens with `--update-failed`.
///
/// MUTATION: in `update_recover::run_windows`, never tell the guard that
/// nobody is waiting (the first part); in `update_apply::owed_at_logon`, owe a
/// window only after a revert or a finished rollback (the last).
#[test]
fn the_logon_run_starts_nothing_only_when_nobody_is_waiting() {
    let Some(install) = Install::new("logon-committed") else {
        return;
    };
    flipped_at(&install, Phase::Committed);
    let (_, world) = recovered_at_logon(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::Committed,
            untried: false,
        }
    );
    assert!(world.opened.is_empty(), "{:?}", world.said);

    let Some(install) = Install::new("logon-armed") else {
        return;
    };
    install.arm();
    install.write(Phase::Armed);
    let (_, world) = recovered_at_logon(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Prepared {
            deferred_launches: 0
        }
    );
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), Vec::new())],
        "{:?}",
        world.said
    );

    let Some(install) = Install::new("logon-gave-up") else {
        return;
    };
    install.arm();
    install.write(stuck(STUCK_ATTEMPT_LIMIT));
    let (_, world) = recovered_at_logon(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert!(
        matches!(install.on_disk().body.phase, Phase::Stuck { .. }),
        "given up, everything kept"
    );
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "{:?}",
        world.said
    );
}

/// RED (U-34) — **an exit whose start is refused tries the next program the
/// disk names before it gives up**: the installed build will not start, and the
/// rescue copy — O's own image, whose own home holds no journal — is started
/// with `--update-failed <journal>` instead.
///
/// The verifier's table: "spawn of the owed opening fails → none (one line
/// only)".
///
/// MUTATION: in `ExitGuard::leave`, give up at the first refused start.
#[test]
fn a_refused_start_on_the_way_out_falls_back_to_the_rescue_copy() {
    let Some(install) = Install::new("refused-start") else {
        return;
    };
    let held = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let mut world = install.world(Trial::Answers);
    world.refuse_start_of = Some(install.installed.clone());
    let (ended, world) = applied(&install, limits(600, 20_000), world);
    drop(held);
    assert_eq!(ended, Ended::OldHeldTheLock, "{:?}", world.said);
    assert_eq!(
        world.opened,
        vec![
            (install.installed.clone(), failed_then(&install, &[])),
            (install.rescue.clone(), failed_then(&install, &[])),
        ],
        "{:?}",
        world.said
    );
}

/// **A spawner that starts nothing and records every start** — O's exit
/// guard's, in the tests of the window's mark.
#[derive(Default)]
struct Starts {
    calls: Vec<(PathBuf, Vec<OsString>)>,
}

impl Spawner for Starts {
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<Running> {
        self.calls.push((program.to_path_buf(), args.to_vec()));
        Ok(Running { pid: 0, started: 0 })
    }

    fn acknowledged(&mut self, _worker: Option<&WorkerCtx>, _data: &Path) -> bool {
        true
    }
}

/// RED (U-34, round 2; Codex's review, blocker 1) — **the duty that a window
/// follows *Restart to update* is handed from the outgoing build O to its
/// applier P by one mark, never inferred from who is alive: whichever of the
/// two takes `H\<txn>\owner` first opens Folio and the other starts nothing
/// — in either order, exactly one start.**
///
/// The review's four steps, which left no window when O and P each inferred
/// the other's duty from liveness: P decides → O lets go of its claim → O
/// looks at P and finds it alive → P leaves. Here P's decision is taking the
/// mark (before it waits for O's lock, which O holds); O, leaving, finds the
/// mark taken by a live P and starts nothing; P then leaves (its lock wait
/// runs out) and its own guard opens the window. In the other order O, at its
/// end, takes the mark and starts Folio; a P that arrives afterwards finds a
/// live owner, touches nothing and starts nothing. O is a real process of the
/// installed program; each step is ordered by the test, not by timing.
///
/// MUTATION: in `update_handoff::Leaving::leave`, start whatever the mark
/// says (order A: two starts); in `update_apply_windows::apply`, run the road
/// whatever `take_the_window` answers (order B: two starts).
#[test]
fn the_window_is_handed_over_by_one_mark_and_opened_exactly_once() {
    // Order A: P takes the mark, then O leaves, then P leaves.
    let Some(install) = Install::new("duty-p-first") else {
        return;
    };
    let old = install.children.start(&install.installed, &[]);
    let old = Running {
        pid: old,
        started: install_flip::started_of(old).expect("O runs"),
    };
    let lock = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let mut road = install.road(limits(1_500, 20_000));
    road.me = crate::update_apply::this_process();
    let applier = start(
        road,
        install.txn,
        install.applier,
        install.world(Trial::Answers),
    );
    let give_up = Instant::now() + Duration::from_secs(20);
    // P's election is over: the mark is written and its lock let go.
    while crate::update_apply::window_owner(&install.home, install.txn).is_none()
        || install_txn::try_hold(
            &crate::update_apply::owner_lock_path(&install.home, install.txn),
            Hold::Exclusive,
        )
        .ok()
        .flatten()
        .is_none()
    {
        assert!(Instant::now() < give_up, "P never took the window");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        crate::update_apply::window_owner(&install.home, install.txn),
        Some(crate::update_apply::this_process()),
        "step 1: P decided — the mark is P's"
    );
    let mut starts = Starts::default();
    let left = Leaving::over(&install.home, install.txn, &install.data).leave(
        old,
        &install.installed,
        &mut starts,
        None,
    );
    assert_eq!(
        left,
        Left::NotMine(Some(std::process::id())),
        "steps 2–3: O finds P's mark"
    );
    assert!(starts.calls.is_empty(), "O starts nothing");
    let (ended, world) = applier.join().unwrap();
    drop(lock);
    assert_eq!(ended, Ended::OldHeldTheLock, "{:?}", world.said);
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "step 4: P, leaving, opens the one window"
    );

    // Order B: O leaves first; a late P finds O's mark.
    let Some(install) = Install::new("duty-o-first") else {
        return;
    };
    let old = install.children.start(&install.installed, &[]);
    let old = Running {
        pid: old,
        started: install_flip::started_of(old).expect("O runs"),
    };
    let mut starts = Starts::default();
    let left = Leaving::over(&install.home, install.txn, &install.data).leave(
        old,
        &install.installed,
        &mut starts,
        None,
    );
    assert_eq!(left, Left::Started(install.installed.clone()));
    assert_eq!(
        starts.calls,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "O's start carries --update-failed: `Handoff` is destructive"
    );
    let journal = std::fs::read(install.home.journal()).unwrap();
    let mut road = install.road(limits(1_500, 20_000));
    road.me = crate::update_apply::this_process();
    let (ended, world) = match start(
        road,
        install.txn,
        install.applier,
        install.world(Trial::Answers),
    )
    .join()
    {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(matches!(ended, Ended::Refused(_)), "{ended:?}");
    assert!(world.opened.is_empty(), "{:?}", world.said);
    assert_eq!(
        std::fs::read(install.home.journal()).unwrap(),
        journal,
        "untouched"
    );
}

/// RED (U-34, round 2; Codex's review, blocker 2) — **a start is delivered only
/// when it is acknowledged**: one created that dies before it takes the data
/// directory is not, and the rescue copy is started instead; **and when no
/// start is delivered at all — the operating system refuses both programs —
/// this process shows the failure window itself**, *Update incomplete.* and
/// the folder, without a spawn.
///
/// The acknowledgement is a Folio holding the data directory's claim
/// (`update_apply::claimed_within`, pinned on its own in
/// `update_handoff::tests::a_start_is_acknowledged_only_by_a_folio_holding_the_data_directory`);
/// here the world answers it.
///
/// MUTATION: in `ExitGuard::deliver`, take a created start as delivered
/// without its acknowledgement (the first part); in `ExitGuard::start`, give up
/// without `show_here` (the second).
#[test]
fn a_start_counts_only_when_acknowledged_and_the_last_resort_is_a_window_here() {
    let Some(install) = Install::new("dies-at-once") else {
        return;
    };
    let held = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let mut world = install.world(Trial::Answers);
    world.starts_die = 1;
    let (ended, world) = applied(&install, limits(600, 20_000), world);
    drop(held);
    assert_eq!(ended, Ended::OldHeldTheLock);
    assert_eq!(
        world.opened,
        vec![
            (install.installed.clone(), failed_then(&install, &[])),
            (install.rescue.clone(), failed_then(&install, &[])),
        ],
        "the installed build died at once; the rescue copy was started"
    );
    assert!(world.shown.is_empty(), "{:?}", world.shown);

    let Some(install) = Install::new("nothing-starts") else {
        return;
    };
    let held = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let mut world = install.world(Trial::Answers);
    world.refuse_every_start = true;
    let (ended, world) = applied(&install, limits(600, 20_000), world);
    drop(held);
    assert_eq!(ended, Ended::OldHeldTheLock);
    assert_eq!(world.opened.len(), 2, "both programs were tried");
    assert_eq!(
        world.shown,
        vec![crate::update_apply::failure_text(Some(&install.home))],
        "{:?}",
        world.said
    );
}

/// RED (U-34, rounds 4 and 5; Codex's finding 14; MARK-RACE) — **a stale
/// window's mark is taken over by exactly one of two contenders racing for
/// it** (a soak): the election is one exclusive lock around
/// read-check-replace, so the first in takes the duty and the second reads the
/// live winner, `Theirs`, or — while the winner still holds the lock, or an
/// outside scanner refuses its read — truthfully answers `Refused`; it may
/// never become another winner. Two threads race, released together by a
/// barrier, over many rounds, each round from a fresh stale mark; the two
/// contenders are both live processes (this test and a synthetic program it
/// started), so neither can be taken for a dead owner.
///
/// **The winner's duty is recorded one of two ways**, and each is asserted as
/// what it is: a replaced mark that names the winner, or — when the
/// replacement is refused before its rename — the stale mark left byte for
/// byte with the election lock still held by the winner's answer, so a third
/// contender finds `WindowHolder::Unmarked`. A scanner that opens the freshly
/// written mark without delete sharing gives the second way at random under
/// load; every tenth round holds the mark open the same way
/// (`trust_harness::hold_without_delete_sharing`), so both ways are asserted
/// in every run.
///
/// MUTATION: in `update_apply::take_the_window_within_using`, give each
/// contender a lock of its own (both answer `Mine`); or answer a replacement
/// refused before its rename with `WindowDuty::recorded` instead of
/// `WindowDuty::held`, which lets the lock go (both answer `Mine` in the first
/// scanned round).
#[test]
fn a_stale_mark_is_taken_over_by_exactly_one_contender() {
    let Some(install) = Install::new("stale-race") else {
        return;
    };
    let other = install.children.start(&install.installed, &[]);
    let contenders = [
        crate::update_apply::this_process(),
        Running {
            pid: other,
            started: install_flip::started_of(other).expect("it runs"),
        },
    ];
    let third = Running { pid: 1, started: 1 };
    let me = crate::update_apply::this_process();
    let stale = format!("{}:{}", me.pid, me.started.wrapping_add(1));
    let mark = crate::update_apply::owner_path(&install.home, install.txn);
    for round in 0..40 {
        let written = format!("{stale}{}", "0".repeat(round % 3));
        std::fs::write(&mark, &written).unwrap();
        let scanned = round % 10 == 9;
        let scanner = scanned
            .then(|| bt_platform::trust_harness::hold_without_delete_sharing(&mark).unwrap());
        // A lock-only winner keeps the lock until its answer is dropped, so
        // the loser of a scanned round waits briefly, not a whole election.
        let within = if scanned {
            Duration::from_millis(300)
        } else {
            crate::update_apply::ELECTION_WITHIN
        };
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let racers: Vec<_> = contenders
            .iter()
            .map(|who| {
                let (home, txn, who, barrier) = (
                    install.home.clone(),
                    install.txn,
                    *who,
                    Arc::clone(&barrier),
                );
                bt_platform::spawn_at_priority(
                    "bt-u34-stale-race",
                    bt_platform::ThreadPriority::BelowNormal,
                    move |_worker| {
                        barrier.wait();
                        crate::update_apply::take_the_window(
                            None,
                            &home,
                            txn,
                            who,
                            Instant::now() + within,
                        )
                    },
                )
                .expect("a racer")
            })
            .collect();
        let answers: Vec<Window> = racers
            .into_iter()
            .map(|racer| racer.join().unwrap())
            .collect();
        let on_disk =
            std::fs::read(&mark).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let mine = answers.iter().filter(|answer| answer.is_mine()).count();
        assert_eq!(mine, 1, "round {round}: {answers:?}");
        let at = answers.iter().position(Window::is_mine).unwrap();
        let winner = contenders[at];
        assert!(
            answers.contains(&Window::Theirs(winner))
                || answers
                    .iter()
                    .any(|answer| matches!(answer, Window::Refused(_))),
            "round {round}: the loser names the winner or truthfully refuses: {answers:?}"
        );
        let Window::Mine(duty) = &answers[at] else {
            unreachable!("the position is a Mine")
        };
        if scanned {
            assert!(
                duty.is_lock_backed(),
                "round {round}: a replacement the scanner refuses records no mark: {answers:?}"
            );
        }
        if duty.is_lock_backed() {
            assert_eq!(
                on_disk.as_deref().ok(),
                Some(written.as_str()),
                "round {round}: the refused replacement left the stale mark whole: {answers:?}"
            );
            assert_eq!(
                crate::update_apply::window_holder(&install.home, install.txn, third),
                Ok(Some(crate::update_apply::WindowHolder::Unmarked)),
                "round {round}: the winner's held lock is its record: {answers:?}"
            );
        } else {
            assert_eq!(
                crate::update_apply::window_owner(&install.home, install.txn),
                Some(winner),
                "round {round}: {answers:?}; the mark's bytes: {on_disk:?}"
            );
        }
        drop(answers);
        drop(scanner);
    }
}

// ── the applier decides first (U-34, round 6) ───────────────────────────────

/// The starts a recording spawner was asked for.
type Started = Vec<(PathBuf, Vec<OsString>)>;

/// O's end, on a worker of its own as `update_handoff::leave_armed` runs it:
/// `leaving`, as the process `old`, with the recording spawner.
fn old_leaves(install: &Install, old: Running, leaving: Leaving) -> JoinHandle<(Left, Started)> {
    let installed = install.installed.clone();
    start_on_a_worker(move |worker| {
        let mut starts = Starts::default();
        let left = leaving.leave(old, &installed, &mut starts, Some(worker));
        (left, starts.calls)
    })
}

/// RED (U-34, round 6; the clean VM's happy path on A5) — **O, reaching its
/// end before the applier it started has taken the window's mark, waits for
/// that applier's decision: the applier takes the mark, the update is applied
/// to `Committed`, and the one window is the trial — O starts nothing.**
///
/// On A5 O reached its end 0.8 s before its applier's first decision, won the
/// election, and the applier refused and abandoned the whole update. Here O's
/// end runs first with the applier recorded (this test process, as the
/// applier), and the applier starts half a second later, through the real
/// road.
///
/// MUTATION: in `update_handoff::Leaving::leave`, skip the wait for the
/// applier's decision.
#[test]
fn o_waits_for_the_applier_it_started_and_the_update_applies() {
    let Some(install) = Install::new("o-waits") else {
        return;
    };
    // O's identity only: its process has gone, as a real O's has once its
    // guard answers (a process of the installed image would hold it open).
    let old = Running { pid: 1, started: 1 };
    let applier = crate::update_apply::this_process();
    let leaving = Leaving::over(&install.home, install.txn, &install.data)
        .after_applier(applier, Duration::from_secs(20));
    let o = old_leaves(&install, old, leaving);
    std::thread::sleep(Duration::from_millis(500));
    let mut road = install.road(limits(20_000, 20_000));
    road.me = applier;
    let (ended, world) = applied_on(&install, road, install.world(Trial::Answers));
    let (left, starts) = o.join().unwrap();
    assert_eq!(left, Left::NotMine(Some(applier.pid)), "{starts:?}");
    assert!(starts.is_empty(), "O starts nothing");
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(world.opened.is_empty(), "the trial is the one window");
    assert!(install.holds(Place::Install, &install.new));
}

/// RED (U-34, round 6) — **an applier that dies before it takes the mark
/// leaves the duty to O, which takes it and starts Folio as soon as the
/// applier is gone** — well within its wait.
///
/// MUTATION: in `update_handoff::Leaving::leave`, wait the whole budget
/// whether or not the applier still runs (the start then comes only after it).
#[test]
fn an_applier_that_dies_before_its_mark_leaves_the_window_to_o() {
    let Some(install) = Install::new("applier-dies") else {
        return;
    };
    let old = install.children.start(&install.installed, &[]);
    let old = Running {
        pid: old,
        started: install_flip::started_of(old).expect("O runs"),
    };
    let doomed = install.children.start(&install.rescue, &[]);
    let applier = Running {
        pid: doomed,
        started: install_flip::started_of(doomed).expect("it runs"),
    };
    let leaving = Leaving::over(&install.home, install.txn, &install.data)
        .after_applier(applier, Duration::from_secs(20));
    let began = Instant::now();
    let o = old_leaves(&install, old, leaving);
    std::thread::sleep(Duration::from_millis(300));
    install.children.end(doomed);
    let (left, starts) = o.join().unwrap();
    assert_eq!(left, Left::Started(install.installed.clone()), "{starts:?}");
    assert_eq!(starts.len(), 1);
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "O did not wait out its budget: {:?}",
        began.elapsed()
    );
}

/// RED (U-34, round 6) — **an applier alive that never takes the mark loses
/// the duty to O once O's wait has passed**, and O starts Folio.
///
/// MUTATION: in `update_handoff::Leaving::leave`, wait for the applier with no
/// bound (O's end then never leaves).
#[test]
fn an_applier_that_never_takes_the_mark_loses_the_window_after_os_wait() {
    let Some(install) = Install::new("applier-silent") else {
        return;
    };
    let old = install.children.start(&install.installed, &[]);
    let old = Running {
        pid: old,
        started: install_flip::started_of(old).expect("O runs"),
    };
    let silent = install.children.start(&install.rescue, &[]);
    let applier = Running {
        pid: silent,
        started: install_flip::started_of(silent).expect("it runs"),
    };
    let leaving = Leaving::over(&install.home, install.txn, &install.data)
        .after_applier(applier, Duration::from_millis(700));
    let began = Instant::now();
    let (left, starts) = old_leaves(&install, old, leaving).join().unwrap();
    assert!(began.elapsed() >= Duration::from_millis(700), "O waited");
    assert_eq!(left, Left::Started(install.installed.clone()), "{starts:?}");
    assert_eq!(
        crate::update_apply::window_owner(&install.home, install.txn),
        Some(old)
    );
}

/// RED (U-34, round 6) — **an applier that finds the mark held by a live O
/// waits for O to leave rather than give up the update**: when O's process
/// ends, the applier takes the mark over and applies to `Committed`.
///
/// MUTATION: in `update_apply_windows::apply`, refuse at the first
/// `Theirs` (the round-5 behaviour).
#[test]
fn an_applier_finding_os_mark_waits_for_o_to_leave_and_applies() {
    let Some(install) = Install::new("applier-waits") else {
        return;
    };
    // O, alive: a process of the rescue image, which holds no install file.
    let old = install.children.start(&install.rescue, &[]);
    let old_running = Running {
        pid: old,
        started: install_flip::started_of(old).expect("O runs"),
    };
    assert!(
        crate::update_apply::take_the_window(
            None,
            &install.home,
            install.txn,
            old_running,
            Instant::now() + crate::update_apply::ELECTION_WITHIN
        )
        .is_mine()
    );
    let mut road = install.road(limits(20_000, 20_000));
    road.me = crate::update_apply::this_process();
    let applier = start(
        road,
        install.txn,
        install.applier,
        install.world(Trial::Answers),
    );
    std::thread::sleep(Duration::from_millis(600));
    install.children.end(old);
    let (ended, world) = applier.join().unwrap();
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(world.opened.is_empty(), "the trial is the window");
}

/// The applier over `road`, run to its end.
fn applied_on(install: &Install, road: Road, world: Fake) -> (Ended, Fake) {
    match start(road, install.txn, install.applier, world).join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// RED (T-UPDATE-LOCK-RACE round 2) — **the transaction-lock leg spends only
/// what remains of the applier's one wait for O**. At an already-spent
/// deadline the injected real-lock entrance is asked with exactly zero, even
/// though `road.limits.old_within` is two seconds. No clock or sleep decides
/// the assertion.
///
/// MUTATION: in `apply_under_the_lock_with`, ask for a fresh
/// `road.limits.old_within` instead of `window - now`.
#[test]
fn the_applier_waits_for_o_within_one_budget() {
    let Some(install) = Install::new("one-budget") else {
        return;
    };
    let road = install.road(limits(2_000, 20_000));
    let txn = install.txn;
    let nonce = install.applier;
    let mut world = install.world(Trial::Answers);
    let expired = Instant::now() - Duration::from_millis(1);
    let ((ended, successor, transaction_lock), asked) = on_a_worker(move |worker| {
        let mut asked = None;
        let answer = apply_under_the_lock_with(
            worker,
            &road,
            txn,
            nonce,
            expired,
            &mut world,
            |_path, within| {
                asked = Some(within);
                Ok(None)
            },
        );
        (answer, asked)
    });
    assert_eq!(asked, Some(Duration::ZERO));
    assert_eq!(ended, Ended::OldHeldTheLock);
    assert_eq!(successor, None);
    assert_eq!(
        transaction_lock,
        crate::update_apply::TransactionLock::NeverHeld
    );
}

/// RED (U-34, round 7) — **a late applier after O's wait ran out never
/// commits over O's replacement**: alive but unmarked past O's wait, it loses
/// the duty; O takes the mark and starts the installed build (its window is
/// the one the person sees); the applier, finding O's live mark, waits for O to
/// go, takes the stale mark and enters its road — which meets the running
/// Folio (here the data directory's claim it holds) and ends `Abandoned` with
/// nothing moved, its guard's start acknowledged by that Folio.
///
/// MUTATION: in `update_handoff::Leaving::leave`, never take the duty after
/// the wait (O then starts nothing, and the person has no window).
#[test]
fn a_late_applier_after_os_wait_meets_os_replacement_and_never_commits() {
    let Some(install) = Install::new("late-applier") else {
        return;
    };
    let old = install.children.start(&install.rescue, &[]);
    let old = Running {
        pid: old,
        started: install_flip::started_of(old).expect("O runs"),
    };
    let applier = crate::update_apply::this_process();
    let leaving = Leaving::over(&install.home, install.txn, &install.data)
        .after_applier(applier, Duration::from_millis(500));
    let (left, starts) = old_leaves(&install, old, leaving).join().unwrap();
    assert_eq!(left, Left::Started(install.installed.clone()), "{starts:?}");
    assert_eq!(starts.len(), 1, "O's replacement");
    // O's replacement runs and holds the data directory.
    let replacement = crate::persist::try_claim(&install.data).unwrap();
    let mut road = install.road(limits(4_000, 20_000));
    road.me = applier;
    let applier_run = start(
        road,
        install.txn,
        install.applier,
        install.world(Trial::Answers),
    );
    std::thread::sleep(Duration::from_millis(300));
    install.children.end(old.pid);
    let (ended, world) = applier_run.join().unwrap();
    drop(replacement);
    assert_eq!(ended, Ended::Abandoned, "{:?}", world.said);
    nothing_moved(&install);
    assert!(world.launched.is_empty(), "no trial: never committed");
    assert_eq!(
        world.opened.len(),
        1,
        "its guard's start, handed to the running Folio"
    );
}

// ── design revision (h): identity, deferral and the hand-back (U-37) ─────────

/// **Every new file moved in by hand, the entrance armed, the journal at
/// `Moving`**: the new set live and no trial recorded — the state a dead
/// applier, or D-14's applier, leaves.
fn moved_in(tag: &str) -> Option<Install> {
    let install = Install::new(tag)?;
    flipped_at(&install, Phase::Moving);
    assert!(install.holds(Place::Install, &install.new));
    Some(install)
}

/// The journal's phase and the words, at one process creation.
type Seen = (Phase, Vec<OsString>);

/// RED (U-35) — **when the new installed image and the rescue image are both
/// refused by the operating system, the Windows exit guard reserves one last
/// same-image trial before asking the OS for it**. The reservation is visible
/// to that third launch, and a refusal leaves it durable for recovery; there
/// is no fourth launch.
///
/// MUTATION: in `reserve_last_trial`, return the nonce without recording
/// `TrialPlanned` (the third launch sees `Moving`: the review's m4); omit
/// the last trial; or recurse after its refusal.
#[test]
fn u35_two_windows_launch_refusals_reserve_exactly_one_last_trial() {
    let Some(started) = moved_in("u35-last-trial-started") else {
        return;
    };
    let road = started.road(limits(600, 20_000));
    let mut world = started.world(Trial::Answers);
    world.refuse_starts = 2;
    // What the journal on disk said, and the words, at the moment of the third
    // process creation (the review's m4) — recorded, asserted after.
    let observed: Arc<Mutex<Option<Seen>>> = Arc::default();
    let saw = Arc::clone(&observed);
    let home = started.home.clone();
    world.before_start = Some(Box::new(move |at, _program, args| {
        if at == 2 {
            let journal = Journal::parse(&std::fs::read(home.journal()).unwrap()).unwrap();
            *saw.lock().unwrap() = Some((journal.body.phase, args.to_vec()));
        }
    }));
    let (left, world) = on_a_worker(move |worker| {
        let mut guard = ExitGuard::new(WindowsLeave {
            road: &road,
            world: &mut world,
            handed: &[],
            worker: Some(worker),
            actor: Some(Actor::Applier),
        });
        let left = guard.leave();
        drop(guard);
        (left, world)
    });
    assert_eq!(left, Left::Started(started.installed.clone()));
    let (phase, args) = observed
        .lock()
        .unwrap()
        .clone()
        .expect("a third process creation");
    let Phase::TrialStarting { nonce, .. } = phase else {
        panic!("the reservation is durable before the third launch: {phase:?}");
    };
    assert_eq!(args[0], OsString::from(cli::UPDATE_TRIAL_FLAG));
    assert_eq!(TxnId::parse(&args[1].to_string_lossy()), Ok(started.txn));
    assert_eq!(
        Nonce::parse(&args[2].to_string_lossy()),
        Ok(nonce),
        "the third launch carries the reserved nonce"
    );
    assert_eq!(world.opened.len(), 3, "the reserved trial starts");
    assert!(matches!(
        started.on_disk().body.phase,
        Phase::TrialStarting { .. }
    ));

    let Some(install) = moved_in("u35-last-trial") else {
        return;
    };
    let road = install.road(limits(600, 20_000));
    let mut world = install.world(Trial::Answers);
    world.refuse_every_start = true;
    let (left, world) = on_a_worker(move |worker| {
        let mut guard = ExitGuard::new(WindowsLeave {
            road: &road,
            world: &mut world,
            handed: &[],
            worker: Some(worker),
            actor: Some(Actor::Applier),
        });
        let left = guard.leave();
        drop(guard);
        (left, world)
    });
    assert!(matches!(left, Left::ShownHere(_)), "{left:?}");
    assert_eq!(world.opened.len(), 3, "one primary, rescue, last trial");
    assert_eq!(world.opened[0].0, install.installed);
    assert_eq!(world.opened[1].0, install.rescue);
    assert_eq!(world.opened[2].0, install.installed);
    assert!(
        world.opened[2]
            .1
            .iter()
            .any(|word| word == "--update-trial")
    );
    assert!(
        world.opened[2]
            .1
            .iter()
            .any(|word| word == "--update-failed")
    );
    assert!(matches!(
        install.on_disk().body.phase,
        Phase::TrialStarting { .. }
    ));
}

/// **The exit guard's U-35 cell, as the product reaches it**: the new set
/// live at `Moving`, the installed build and the rescue copy refused by the
/// operating system, the reserved trial's start created and acknowledged.
/// Answers the reserved nonce and every start the guard asked for.
fn reserved_by_the_guard(install: &Install) -> (Nonce, Vec<(PathBuf, Vec<OsString>)>) {
    let road = install.road(limits(600, 20_000));
    let mut world = install.world(Trial::Answers);
    world.refuse_starts = 2;
    let (left, world) = on_a_worker(move |worker| {
        let mut guard = ExitGuard::new(WindowsLeave {
            road: &road,
            world: &mut world,
            handed: &[],
            worker: Some(worker),
            actor: Some(Actor::Applier),
        });
        let left = guard.leave();
        drop(guard);
        (left, world)
    });
    assert_eq!(left, Left::Started(install.installed.clone()));
    let Phase::TrialStarting { nonce, .. } = install.on_disk().body.phase else {
        panic!("reserved: {:?}", install.on_disk().body.phase);
    };
    (nonce, world.opened)
}

/// This test process, by pid and start instant: the reserved trial a test
/// stands in for, as `commit_last_trial` names its caller.
fn this_process() -> (u32, Option<u64>) {
    let me = std::process::id();
    (me, install_flip::started_of(me))
}

/// RED (U-35 round 2, the review's B3) — **a reserved last trial that became
/// ready commits its own transaction, with no rescue-copy process at all**:
/// the guard asked for the rescue once and the operating system refused it;
/// from there, the trial's own receipt — read back under the transaction
/// lock and naming the trial exactly — makes `Committed` durable. Before the
/// receipt exists, and while a holder has the lock, it waits (`Pending`) and
/// writes nothing; asked again after the commit, it answers `Committed`.
///
/// MUTATION: take `Actor::Trial` out of `LastTrialReady`'s authors (the
/// commit is refused: B3's "keeps nothing"); or skip the lock in
/// `commit_last_trial_as` (it commits under a holder's lock).
#[test]
fn u35_a_ready_last_trial_commits_itself_without_any_rescue_process() {
    let Some(install) = moved_in("u35-commits-itself") else {
        return;
    };
    let (nonce, opened) = reserved_by_the_guard(&install);
    let (home, txn) = (install.home.clone(), install.txn);
    let (me, started) = this_process();
    let (before, held, committed, again) = on_a_worker(move |worker| {
        let before = commit_last_trial_as(worker, &home, txn, nonce, me, started);
        let receipt = Receipt {
            txn,
            nonce,
            pid: me,
            version: "0.4.7".to_owned(),
            started,
        };
        install_txn::durable_create(&home.receipt_path(txn, &nonce), &receipt.encode()).unwrap();
        let lock = install_txn::try_hold(&home.lock(), Hold::Exclusive)
            .unwrap()
            .expect("the test holds the transaction lock");
        let held = commit_last_trial_as(worker, &home, txn, nonce, me, started);
        drop(lock);
        let committed = commit_last_trial_as(worker, &home, txn, nonce, me, started);
        let again = commit_last_trial_as(worker, &home, txn, nonce, me, started);
        (before, held, committed, again)
    });
    assert_eq!(before, Ok(LastTrialCommit::Pending), "no receipt yet");
    assert_eq!(held, Ok(LastTrialCommit::Pending), "a holder has the lock");
    assert_eq!(committed, Ok(LastTrialCommit::Committed));
    assert_eq!(again, Ok(LastTrialCommit::Committed));
    assert_eq!(install.on_disk().body.phase, Phase::Committed);
    assert_eq!(install.header().outcome, HeaderOutcome::Committed);
    // The rescue copy was asked for once, by the guard, and refused; nothing
    // after that started anything.
    let rescue_starts: Vec<_> = opened
        .iter()
        .filter(|(program, _)| *program == install.rescue)
        .collect();
    assert_eq!(rescue_starts.len(), 1, "{opened:?}");
    assert_eq!(opened.len(), 3, "{opened:?}");
}

/// RED (U-35 round 2, the review's B3 and B2) — **a reserved last trial that
/// is not healthy commits nothing, and the recovery ends it and rolls back
/// once a rescue copy can run**: a receipt at the reserved nonce that names
/// another process, one that runs, is refused and the journal stays
/// `TrialStarting` (its writes stay held); the recovery handed that unready
/// instance back ends
/// exactly it — `EndTrial` over `TrialStarting` — and restores the old build,
/// and the card it leaves never says the new version was not tried (m1).
///
/// MUTATION: drop `TrialStarting` from Recovery's `EndTrial` row (the
/// recovery defers for ever, the review's B2); or record
/// `trial_started: false` over `TrialStarting`.
#[test]
fn u35_an_unready_last_trial_commits_nothing_and_the_recovery_ends_it() {
    let Some(install) = moved_in("u35-unready") else {
        return;
    };
    let (nonce, _opened) = reserved_by_the_guard(&install);
    // A receipt at the reserved name naming another process that runs — this
    // test process, which is no process of the new build — and the unready
    // instance that asks to commit on it.
    let (me, _) = this_process();
    install.receipt(nonce, nonce, me);
    let unready = install.start_trial();
    let (home, txn) = (install.home.clone(), install.txn);
    let refused = on_a_worker(move |worker| {
        commit_last_trial_as(
            worker,
            &home,
            txn,
            nonce,
            unready.pid,
            Some(unready.started),
        )
    });
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("ReceiptForAnotherProcess")),
        "{refused:?}"
    );
    assert!(matches!(
        install.on_disk().body.phase,
        Phase::TrialStarting { .. }
    ));

    let mut road = install.road(limits(5_000, 5_000));
    road.handed_back = Some(crate::update_apply::HandedBack {
        process: Running {
            pid: unready.pid,
            started: unready.started,
        },
        ready: false,
    });
    let mut world = install.world(Trial::Silent);
    let (code, world) = on_a_worker(move |worker| {
        let code = crate::update_recover::run_windows(worker, &road, None, &mut world);
        (code, world)
    });
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(!deferred(&world), "{:?}", world.said);
    assert!(said_at(&world, "is asked to").is_some(), "{:?}", world.said);
    assert!(!runs(unready), "that exact instance was ended");
    rolled_back_on_disk(&install);
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::RolledBack,
            untried: false,
        }
    );
}

/// The receipt this test process writes as the trial: its own pid and start
/// instant, as `update_trial::receipt_due` makes it.
fn own_receipt(install: &Install, nonce: Nonce) -> crate::update_trial::ReceiptJob {
    let (me, started) = this_process();
    crate::update_trial::ReceiptJob {
        path: install.home.receipt_path(install.txn, &nonce),
        bytes: Receipt {
            txn: install.txn,
            nonce,
            pid: me,
            version: "0.4.7".to_owned(),
            started,
        }
        .encode(),
    }
}

/// RED (U-35 round 3, the review's F1) — **a ready reserved trial that dies
/// before its commit does not stop the next attempt of that trial from
/// committing**: the first attempt's create-new receipt names a process that
/// has ended; the next start, its rescue refused, continues as the same
/// reserved trial; until its own receipt is on disk it waits (`Pending`, not a
/// refusal); its receipt replaces the earlier attempt's, by the same durable
/// write; and it commits — with no rescue process at all.
///
/// MUTATION: in `update_trial::write_receipt`, keep every existing receipt
/// (create-new only: the next attempt can never commit); or in
/// `commit_last_trial_as`, read an earlier attempt's receipt as a refusal.
#[test]
fn u35_a_reserved_trial_that_died_ready_does_not_stop_the_next_from_committing() {
    let Some(install) = moved_in("u35-died-ready") else {
        return;
    };
    let (nonce, _opened) = reserved_by_the_guard(&install);
    // The first attempt became ready, wrote its receipt, and died.
    let first = install.start_trial();
    install.receipt(nonce, nonce, first.pid);
    install.children.end(first.pid);
    let stale = std::fs::read(install.home.receipt_path(install.txn, &nonce)).unwrap();

    let mut world = StartsRecorded {
        said: Vec::new(),
        spawned: Vec::new(),
        refuse: true,
    };
    let crate::update_startup::Verdict::Continue {
        trial, last_trial, ..
    } = a_start(&install, &[], &mut world)
    else {
        panic!("a start whose rescue is refused continues");
    };
    assert_eq!(
        (trial, last_trial),
        (Some((install.txn, nonce)), true),
        "{:?}",
        world.said
    );

    let (home, txn) = (install.home.clone(), install.txn);
    let (me, started) = this_process();
    let job = own_receipt(&install, nonce);
    let (before, written, committed) = on_a_worker(move |worker| {
        let before = commit_last_trial_as(worker, &home, txn, nonce, me, started);
        let written = crate::update_trial::write_receipt(&job);
        let committed = commit_last_trial_as(worker, &home, txn, nonce, me, started);
        (before, written, committed)
    });
    assert_eq!(before, Ok(LastTrialCommit::Pending), "an earlier attempt's");
    assert_eq!(written, Ok(()));
    assert_ne!(
        std::fs::read(install.home.receipt_path(install.txn, &nonce)).unwrap(),
        stale
    );
    assert_eq!(committed, Ok(LastTrialCommit::Committed));
    assert_eq!(install.on_disk().body.phase, Phase::Committed);
}

/// RED (U-35 round 3, the review's F3) — **a reserved trial that cannot read
/// its own start instant gets one answer, `Unprovable`, and nothing is
/// written**: its receipt would name nobody, so neither it nor a holder can
/// ever accept it, and asking again would be for ever.
///
/// MUTATION: in `commit_last_trial_as`, go on without the start instant
/// (the protocol then refuses every turn: `Err`).
#[test]
fn u35_a_trial_that_cannot_read_its_start_instant_is_unprovable() {
    let Some(install) = moved_in("u35-unprovable") else {
        return;
    };
    let (nonce, _opened) = reserved_by_the_guard(&install);
    let (me, _) = this_process();
    install.receipt(nonce, nonce, std::process::id());
    let (home, txn) = (install.home.clone(), install.txn);
    let answer =
        on_a_worker(move |worker| commit_last_trial_as(worker, &home, txn, nonce, me, None));
    assert_eq!(answer, Ok(LastTrialCommit::Unprovable));
    assert!(matches!(
        install.on_disk().body.phase,
        Phase::TrialStarting { .. }
    ));
}

/// A start's world that records what it starts, and refuses every start
/// when `refuse` — the rescue copy the operating system will not start.
struct StartsRecorded {
    said: Vec<String>,
    spawned: Vec<PathBuf>,
    refuse: bool,
}

impl crate::update_startup::World for StartsRecorded {
    fn say(&mut self, line: &str) {
        self.said.push(line.to_owned());
    }

    fn spawn_detached(&mut self, program: &Path, _args: &[OsString]) -> io::Result<()> {
        self.spawned.push(program.to_path_buf());
        if self.refuse {
            Err(io::Error::other("the system would not start it (test)"))
        } else {
            Ok(())
        }
    }

    fn retire_entrance(&mut self, _txn: TxnId) -> Result<(), String> {
        panic!("a destructive journal is never retired by a start")
    }

    fn mounts_under(&mut self, _folder: &Path) -> Result<Vec<PathBuf>, String> {
        Ok(Vec::new())
    }

    fn on_a_worker(&mut self, _job: crate::update_startup::OffThread) -> io::Result<()> {
        panic!("nothing is mounted on Windows")
    }
}

/// A start of the installed build with `argv`, over `install`'s home.
fn a_start(
    install: &Install,
    argv: &[OsString],
    world: &mut StartsRecorded,
) -> crate::update_startup::Verdict {
    let trial = argv
        .first()
        .is_some_and(|word| word.as_os_str() == cli::UPDATE_TRIAL_FLAG)
        .then(|| cli::UpdateTrialArg {
            txn: argv[1].to_string_lossy().into_owned(),
            nonce: argv[2].to_string_lossy().into_owned(),
        });
    let failed = argv
        .iter()
        .position(|word| word.as_os_str() == cli::UPDATE_FAILED_FLAG)
        .map(|at| PathBuf::from(&argv[at + 1]));
    crate::update_startup::run(
        &crate::update_startup::Start {
            own_exe: &install.installed,
            home: &install.home,
            argv,
            trial: trial.as_ref(),
            failed: failed.as_deref(),
        },
        world,
    )
}

/// RED (U-35 round 2, the review's M1) — **at `TrialStarting` the exit guards
/// and a start read what is started from one owner, the reservation**: the
/// guard names the reserved trial (never a fresh nonce), and the start those
/// words make runs as that very trial, with nothing handed back; a start with
/// any other nonce is handed to the recovery; and a plain start whose rescue
/// copy the operating system refuses continues as the reserved trial — its
/// writes held, its card the last trial's — never as the new build plainly.
///
/// MUTATION: in `opens_now`, answer `Opens::Trial` (a fresh nonce) at
/// `TrialStarting` (its start is handed back: the review's M1 chain); or in
/// `update_startup::hand_to_rescue`, ignore the reservation (the plain start
/// writes durably before `Committed`).
#[test]
fn u35_the_guard_and_a_start_agree_on_the_reserved_trial() {
    let Some(install) = moved_in("u35-agree") else {
        return;
    };
    let (nonce, _opened) = reserved_by_the_guard(&install);
    let road = install.road(limits(600, 20_000));
    assert_eq!(
        opens_now(&road),
        Opens::LastTrial {
            txn: install.txn,
            nonce,
        }
    );
    let (program, words) = road.opening(&opens_now(&road));
    assert_eq!(program, install.installed.as_path());
    let reserved = Some((install.txn, nonce));
    let incomplete = Some(Failure::TrialIncomplete {
        folder: install.home.root().to_path_buf(),
    });

    let mut world = StartsRecorded {
        said: Vec::new(),
        spawned: Vec::new(),
        refuse: false,
    };
    let crate::update_startup::Verdict::Continue {
        trial,
        last_trial,
        failed,
        ..
    } = a_start(&install, &words, &mut world)
    else {
        panic!("the guard's start is the reserved trial: {:?}", world.said);
    };
    assert_eq!((trial, last_trial), (reserved, true));
    assert_eq!(failed, incomplete);
    assert!(world.spawned.is_empty(), "nothing is handed back");

    let fresh = Opens::Trial { txn: install.txn }.words(&install.home);
    let mut world = StartsRecorded {
        said: Vec::new(),
        spawned: Vec::new(),
        refuse: false,
    };
    assert!(matches!(
        a_start(&install, &fresh, &mut world),
        crate::update_startup::Verdict::Exit(0)
    ));
    assert_eq!(world.spawned, vec![install.rescue.clone()]);

    let mut world = StartsRecorded {
        said: Vec::new(),
        spawned: Vec::new(),
        refuse: true,
    };
    let crate::update_startup::Verdict::Continue {
        trial,
        last_trial,
        failed,
        ..
    } = a_start(&install, &[], &mut world)
    else {
        panic!("a start whose rescue is refused continues");
    };
    assert_eq!(world.spawned, vec![install.rescue.clone()]);
    assert_eq!((trial, last_trial), (reserved, true), "{:?}", world.said);
    assert_eq!(failed, incomplete);
    assert!(matches!(
        install.on_disk().body.phase,
        Phase::TrialStarting { .. }
    ));
}

/// A receipt at `nonce`'s name carrying exactly `receipt`'s fields.
fn receipt_as(install: &Install, nonce: Nonce, receipt: &Receipt) {
    install_txn::durable_create(
        &install.home.receipt_path(install.txn, &nonce),
        &receipt.encode(),
    )
    .unwrap();
}

/// The line the recovery says when it defers.
fn deferred(world: &Fake) -> bool {
    said_at(world, "BT_UPDATE_RECOVER deferred").is_some()
}

/// RED (U-37, design revision (h) H.1) — **a running trial is adopted only
/// when a receipt of this transaction, at its own nonce's name, names it
/// exactly — its pid and its start instant; a receipt naming its pid at
/// another instant (a reused pid, or an ordinary Folio that now has it), a
/// receipt without `started`, and a receipt of another transaction adopt
/// nothing, and the recovery defers with the transaction untouched; among
/// several receipts, the exact one is adopted and its receipt commits.**
///
/// Codex's review of `caea1099`, finding 1: the receipt named a pid only, so a
/// stale receipt could commit whichever process now had that pid.
///
/// MUTATION: in `update_apply::survey`, match a receipt to a running process
/// by its pid alone.
#[test]
fn adoption_needs_a_receipt_naming_the_exact_process() {
    let stale = |install: &Install, process: TrialProcess| Receipt {
        txn: install.txn,
        nonce: Nonce::new([0x61; 32]),
        pid: process.pid,
        version: "0.4.7".to_owned(),
        started: Some(process.started.wrapping_sub(1)),
    };
    type Case<'a> = (
        &'a str,
        &'a dyn Fn(&Install, TrialProcess) -> (Nonce, Receipt),
    );
    let cases: [Case<'_>; 3] = [
        ("reused", &|install, process| {
            (Nonce::new([0x61; 32]), stale(install, process))
        }),
        ("no-started", &|install, process| {
            let nonce = Nonce::new([0x62; 32]);
            (
                nonce,
                Receipt {
                    txn: install.txn,
                    nonce,
                    pid: process.pid,
                    version: "0.4.7".to_owned(),
                    started: None,
                },
            )
        }),
        ("other-txn", &|_install, process| {
            let nonce = Nonce::new([0x63; 32]);
            (
                nonce,
                Receipt {
                    txn: TxnId::new([0x7e; 16]),
                    nonce,
                    pid: process.pid,
                    version: "0.4.7".to_owned(),
                    started: Some(process.started),
                },
            )
        }),
    ];
    for (tag, receipt) in cases {
        let Some(install) = moved_in(&format!("exact-{tag}")) else {
            return;
        };
        // An ordinary start of the installed program: no trial, no receipt.
        let running = install.start_trial();
        let (nonce, carried) = receipt(&install, running);
        receipt_as(&install, nonce, &carried);
        let (_code, world) =
            recovered_at_logon(&install, limits(5_000, 5_000), install.world(Trial::Silent));
        assert_eq!(
            install.on_disk().body.phase,
            Phase::Moving,
            "{tag}: {:?}",
            world.said
        );
        assert!(deferred(&world), "{tag}: {:?}", world.said);
        assert!(runs(running), "{tag}");
        assert!(
            world.opened.is_empty() && world.launched.is_empty(),
            "{tag}"
        );
    }

    // Several receipts, none of them naming the running process exactly: no
    // adoption.
    let Some(install) = moved_in("exact-several-none") else {
        return;
    };
    let running = install.start_trial();
    receipt_as(&install, Nonce::new([0x61; 32]), &stale(&install, running));
    let no_started = Nonce::new([0x65; 32]);
    receipt_as(
        &install,
        no_started,
        &Receipt {
            txn: install.txn,
            nonce: no_started,
            pid: running.pid,
            version: "0.4.7".to_owned(),
            started: None,
        },
    );
    let (_code, world) =
        recovered_at_logon(&install, limits(5_000, 5_000), install.world(Trial::Silent));
    assert!(deferred(&world), "several, none exact: {:?}", world.said);
    assert_eq!(install.on_disk().body.phase, Phase::Moving);

    // Several receipts: a stale one, and the exact one, which commits.
    let Some(install) = moved_in("exact-several") else {
        return;
    };
    let running = install.start_trial();
    receipt_as(&install, Nonce::new([0x61; 32]), &stale(&install, running));
    let exact = Nonce::new([0x64; 32]);
    receipt_as(
        &install,
        exact,
        &Receipt {
            txn: install.txn,
            nonce: exact,
            pid: running.pid,
            version: "0.4.7".to_owned(),
            started: Some(running.started),
        },
    );
    let (code, world) =
        recovered_at_logon(&install, limits(5_000, 5_000), install.world(Trial::Silent));
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Trial, Committed, Retired]"),
        "{:?}",
        world.said
    );
    assert!(runs(running), "the adopted trial runs on");
}

/// RED (U-37, H.3 step 3; Codex's check of (h), blocker 2, sequence A) — **a
/// live process of the new build that no receipt names, even one that has not
/// taken the data directory yet, defers the recovery: nothing recorded,
/// launched or moved; at logon nothing is started; a person's start leaves
/// that process as the window; once it has gone, the next recovery decides
/// (on Windows, the rollback).**
///
/// MUTATION: in `update_apply::before_deciding`, answer `Decide` for
/// `Survey::Candidates`.
#[test]
fn a_candidate_that_has_not_claimed_the_data_directory_yet_defers_the_recovery() {
    let Some(install) = moved_in("pre-claim") else {
        return;
    };
    let candidate = install.start_trial();
    let (_code, world) =
        recovered_at_logon(&install, limits(5_000, 5_000), install.world(Trial::Silent));
    assert!(deferred(&world), "{:?}", world.said);
    assert_eq!(install.on_disk().body.phase, Phase::Moving);
    assert!(install.holds(Place::Install, &install.new), "nothing moved");
    assert!(world.opened.is_empty() && world.launched.is_empty());

    let (_code, world) = recovered(&install, limits(5_000, 5_000), install.world(Trial::Silent));
    assert!(deferred(&world), "{:?}", world.said);
    assert!(
        world.opened.is_empty(),
        "the candidate is the window: {:?}",
        world.said
    );
    assert!(runs(candidate));

    install.children.end(candidate.pid);
    let (code, world) = recovered(&install, limits(5_000, 5_000), install.world(Trial::Silent));
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(!deferred(&world), "{:?}", world.said);
    rolled_back_on_disk(&install);
}

/// RED (U-37, H.3 step 3; Codex's check of (h), blocker 2, sequence B) — **a
/// data directory whose claim cannot be asked about (`QueryDenied`) defers the
/// recovery with nothing recorded or moved; at logon nothing is started; a
/// person's start keeps U-34's delivery duty — its starts are not acknowledged
/// by a denied question, so the fallback follows and then the failure window
/// shown by the recovery itself; the handed line is never dropped.**
///
/// MUTATION: in `update_apply::before_deciding`, take a claim refusal other
/// than `Held` for a free claim (answer `Decide`).
#[test]
fn a_claim_that_cannot_be_asked_about_defers_and_a_persons_start_is_still_delivered() {
    let Some(install) = moved_in("denied") else {
        return;
    };
    let _squat = bt_platform::trust_harness::squat_the_claim(&install.data).unwrap();
    let (_code, world) =
        recovered_at_logon(&install, limits(5_000, 5_000), install.world(Trial::Silent));
    assert!(deferred(&world), "{:?}", world.said);
    assert_eq!(install.on_disk().body.phase, Phase::Moving);
    assert!(install.holds(Place::Install, &install.new), "nothing moved");
    assert!(world.opened.is_empty(), "at logon nobody waits");

    let mut world = install.world(Trial::Silent);
    world.real_ack = Some(Duration::from_millis(600));
    let (_code, world) = recovered(&install, limits(5_000, 5_000), world);
    assert!(deferred(&world), "{:?}", world.said);
    assert_eq!(install.on_disk().body.phase, Phase::Moving);
    assert_eq!(
        world.opened.len(),
        2,
        "the start and its fallback: {:?}",
        world.said
    );
    for (_, words) in &world.opened {
        assert!(
            words.ends_with(&handed()),
            "the handed line is carried: {words:?}"
        );
    }
    assert_eq!(world.shown.len(), 1, "then the failure window, here");
}

/// RED (U-37, H.3 step 3) — **a transaction folder that cannot be listed,
/// while a process of the new build runs, defers the recovery: which
/// receipts exist is not known, so nothing is adopted, decided or moved.**
///
/// MUTATION: in `update_apply::survey`, answer `Survey::Nothing` when
/// `H\<txn>` cannot be listed.
#[test]
fn an_unlistable_transaction_folder_defers_the_recovery() {
    let Some(install) = moved_in("unlistable") else {
        return;
    };
    let candidate = install.start_trial();
    let folder = install.home.transaction(install.txn);
    let aside = folder.with_extension("aside");
    std::fs::rename(&folder, &aside).unwrap();
    let (_code, world) =
        recovered_at_logon(&install, limits(5_000, 5_000), install.world(Trial::Silent));
    std::fs::rename(&aside, &folder).unwrap();
    assert!(deferred(&world), "{:?}", world.said);
    assert!(
        said_at(&world, "cannot be read").is_some(),
        "{:?}",
        world.said
    );
    assert_eq!(install.on_disk().body.phase, Phase::Moving);
    assert!(install.holds(Place::Install, &install.new));
    assert!(runs(candidate));
}

/// RED (U-37, H.3) — **the recovery's own starter — an ordinary start of the
/// installed program that handed itself over — is no candidate: with nothing
/// else running and the claim free, the recovery decides (on Windows, the
/// rollback).**
///
/// Without the exclusion every person's start over `Moving` would defer on
/// the very process that started the recovery.
///
/// MUTATION: in `Txn::before_deciding`, leave `road.starter` out of the
/// excluded processes.
#[test]
fn the_recoverys_own_starter_is_no_candidate() {
    let Some(install) = moved_in("starter") else {
        return;
    };
    let starter = install.start_trial();
    // No elapsed-time grace is part of this invariant: the starter is excluded
    // before the candidate decision, and the recovery may decide immediately.
    let mut road = install.road(limits(0, 0));
    road.starter = Some(Running {
        pid: starter.pid,
        started: starter.started,
    });
    let mut world = install.world(Trial::Silent);
    let code = on_a_worker(move |worker| {
        let handed = handed();
        let code = crate::update_recover::run_windows(worker, &road, Some(&handed), &mut world);
        (code, world)
    });
    let (code, world) = code;
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(!deferred(&world), "{:?}", world.said);
    rolled_back_on_disk(&install);
}

/// RED (U-37, H.3 step 2 and H.4 rule A3) — **the trial named by
/// `--from-trial` as unready is ended by the recovery that accepted the
/// transaction — that exact instance, by W9's stop — and the recovery then
/// decides; a ready one is a candidate and the recovery defers.**
///
/// The trial never ends itself (withdrawn (g).2's exit code 3): ownership
/// passes only when a recovery holds the transaction lock.
///
/// MUTATION: in `update_apply::before_deciding`, skip the end of an unready
/// handed-back trial.
#[test]
fn the_recovery_ends_the_unready_trial_that_handed_it_back() {
    let Some(install) = moved_in("handed-ready") else {
        return;
    };
    let ready = install.start_trial();
    let mut road = install.road(limits(5_000, 5_000));
    road.handed_back = Some(crate::update_apply::HandedBack {
        process: Running {
            pid: ready.pid,
            started: ready.started,
        },
        ready: true,
    });
    let mut world = install.world(Trial::Silent);
    let (_code, world) = on_a_worker(move |worker| {
        let code = crate::update_recover::run_windows(worker, &road, None, &mut world);
        (code, world)
    });
    assert!(
        deferred(&world),
        "a ready one is a candidate: {:?}",
        world.said
    );
    assert!(runs(ready));

    let Some(install) = moved_in("handed-unready") else {
        return;
    };
    let unready = install.start_trial();
    let mut road = install.road(limits(5_000, 5_000));
    road.handed_back = Some(crate::update_apply::HandedBack {
        process: Running {
            pid: unready.pid,
            started: unready.started,
        },
        ready: false,
    });
    let mut world = install.world(Trial::Silent);
    let (code, world) = on_a_worker(move |worker| {
        let code = crate::update_recover::run_windows(worker, &road, None, &mut world);
        (code, world)
    });
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(said_at(&world, "is asked to").is_some(), "{:?}", world.said);
    assert!(!runs(unready), "that exact instance was ended");
    rolled_back_on_disk(&install);
}

/// RED (U-37, H.3; the interleaving audit's "two holders try to adopt") —
/// **two recoveries started together over the same unrecorded, answered trial
/// serialise on the transaction lock: exactly one records and commits it, the
/// other finds the transaction decided, the trial runs on, and nothing is
/// started or launched by either.**
///
/// MUTATION: in `update_apply_windows::hold`, take the transaction lock
/// shared.
#[test]
fn two_recoveries_over_one_unrecorded_trial_commit_it_once() {
    let Some((install, second, _applier)) = failed_with_the_journal_held("contenders") else {
        return;
    };
    let one = {
        let road = install.road(limits(20_000, 20_000));
        let mut world = install.world(Trial::Silent);
        start_on_a_worker(move |worker| {
            let code = crate::update_recover::run_windows(worker, &road, None, &mut world);
            (code, world)
        })
    };
    let two = {
        let road = install.road(limits(20_000, 20_000));
        let mut world = install.world(Trial::Silent);
        start_on_a_worker(move |worker| {
            let code = crate::update_recover::run_windows(worker, &road, None, &mut world);
            (code, world)
        })
    };
    let (_, one) = one.join().unwrap();
    let (_, two) = two.join().unwrap();
    let adopted = [&one, &two]
        .iter()
        .filter(|world| said_at(world, "it is recorded").is_some())
        .count();
    assert_eq!(adopted, 1, "{:?} / {:?}", one.said, two.said);
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::Committed,
            untried: false,
        }
    );
    assert!(runs(second));
    for world in [&one, &two] {
        assert!(
            world.opened.is_empty() && world.launched.is_empty(),
            "{:?}",
            world.said
        );
    }
}

/// RED (U-37, H.4 rule A2 and the rollout contract) — **a trial hands its
/// transaction back with its exact identity and state — `--update-recover
/// --from-trial <pid>:<started>:<ready|unready>` — only to a rescue build
/// whose version is at least the one that knows the word; a rescue build of
/// 0.4.6 is handed nothing** (every update from 0.4.6 to any later version).
///
/// The rescue executable is a real program with a real `VERSIONINFO`
/// (`trust_harness::program`); the start is recorded, as a rescue build that
/// records its argv would.
///
/// MUTATION: in `update_trial::hand_back`, skip the version check.
#[test]
fn a_trial_hands_back_only_to_a_rescue_build_that_knows_the_word() {
    let Some(install) = moved_in("hand-back") else {
        return;
    };
    struct Recorded(Vec<(PathBuf, Vec<OsString>)>);
    impl crate::update_trial::Starter for Recorded {
        fn start(&mut self, program: &Path, line: &[OsString]) -> io::Result<u32> {
            self.0.push((program.to_path_buf(), line.to_vec()));
            Ok(std::process::id())
        }
    }
    let rescue = install.rescue.clone();
    for (version, handed) in [
        (FileVersion([0, 4, 6, 0]), false),
        (FileVersion([0, 4, 7, 0]), true),
    ] {
        std::fs::remove_file(&rescue).unwrap();
        bt_platform::trust_harness::program(
            &rescue,
            version,
            bt_platform::trust_harness::Behaviour::Returns,
        )
        .unwrap();
        let home = install.home.clone();
        let txn = install.txn;
        let (answer, recorded) = on_a_worker(move |worker| {
            let mut recorded = Recorded(Vec::new());
            let answer = crate::update_trial::hand_back(
                worker,
                &home,
                txn,
                false,
                crate::update_trial::FROM_TRIAL_SINCE,
                &mut recorded,
            );
            (answer, recorded.0)
        });
        if !handed {
            assert!(recorded.is_empty(), "{version:?}: {recorded:?}");
            assert_eq!(answer, None);
            continue;
        }
        let me = crate::update_apply::this_process();
        assert_eq!(
            recorded,
            vec![(
                rescue.clone(),
                vec![
                    OsString::from(cli::UPDATE_RECOVER_FLAG),
                    OsString::from(cli::FROM_TRIAL_FLAG),
                    OsString::from(format!("{}:{}:unready", me.pid, me.started)),
                ]
            )]
        );
        assert_eq!(answer, Some(me), "the recovery it started, by its identity");
        let Some(Ok(cli::UpdateDoor::Recover { handed_back, .. })) =
            cli::update_door(recorded[0].1.clone())
        else {
            panic!("the line is the recovery door's");
        };
        assert_eq!(
            handed_back,
            Some(crate::update_apply::HandedBack {
                process: me,
                ready: false
            })
        );
    }
}

// ── design revision (h), round 5 (Codex's review of 153f7cb2) ───────────────

/// **What H.3's step 3 is given, for one row of its table**: whether a process
/// of the new build runs (and whether `H\<txn>` can be listed), and the data
/// directory's claim.
#[derive(Clone, Copy, Debug)]
enum Candidate {
    Seen,
    NotSeen,
    Unlistable,
}

#[derive(Clone, Copy, Debug)]
enum Claim {
    Held,
    Free,
    Denied,
}

/// **The row's world, made**: the candidate started (and `H\<txn>` moved aside
/// for `Unlistable`, answered back by the returned guard), the claim held by
/// this process or squatted.
struct Row {
    _candidate: Option<TrialProcess>,
    _held: Option<bt_platform::instance::DataDirectoryClaim>,
    _squat: Option<bt_platform::trust_harness::Squat>,
    aside: Option<(PathBuf, PathBuf)>,
}

impl Drop for Row {
    fn drop(&mut self) {
        if let Some((folder, aside)) = self.aside.take() {
            std::fs::rename(aside, folder).unwrap();
        }
    }
}

fn make_row(install: &Install, candidate: Candidate, claim: Claim) -> Row {
    let running = match candidate {
        Candidate::NotSeen => None,
        _ => Some(install.start_trial()),
    };
    let aside = matches!(candidate, Candidate::Unlistable).then(|| {
        let folder = install.home.transaction(install.txn);
        let aside = folder.with_extension("aside");
        std::fs::rename(&folder, &aside).unwrap();
        (folder, aside)
    });
    let (held, squat) = match claim {
        Claim::Held => (
            Some(crate::persist::try_claim(&install.data).expect("the claim, held here")),
            None,
        ),
        Claim::Free => (None, None),
        Claim::Denied => (
            None,
            Some(bt_platform::trust_harness::squat_the_claim(&install.data).unwrap()),
        ),
    };
    Row {
        _candidate: running,
        _held: held,
        _squat: squat,
        aside,
    }
}

/// RED (U-37, design revision (h) H.3, the nine-row table) — **every row of
/// H.3's table ends as the table says, at logon and for a person's start: a
/// candidate seen defers and the guard starts nothing beside it; a held claim
/// defers and the guard starts nothing (it is the window); a denied claim
/// question or an unlistable folder with a free claim defers and a person's
/// start is still delivered — its starts unacknowledged, the fallback, then
/// the failure window here; only no candidate with a free claim decides.**
///
/// Codex's review of `153f7cb2`, finding 4: six of the nine cells had no test.
///
/// MUTATION: in `update_apply::before_deciding`, ask the claim before the
/// candidates (a held claim then hides a candidate: the successor is lost).
#[test]
fn the_nine_rows_of_h3_each_end_as_the_table_says() {
    use Candidate::{NotSeen, Seen, Unlistable};
    use Claim::{Denied, Free, Held};
    // The deferral the recovery says; and whether a person's start is
    // delivered (U-34's starts and the window here) or not (a Folio is the
    // window).
    let rows: [(Candidate, Claim, Option<&str>, bool); 9] = [
        (Seen, Held, Some("no receipt names it"), false),
        (Seen, Free, Some("no receipt names it"), false),
        (Seen, Denied, Some("no receipt names it"), false),
        (
            NotSeen,
            Held,
            Some("a Folio holds the data directory"),
            false,
        ),
        (NotSeen, Free, None, false),
        (NotSeen, Denied, Some("cannot be asked about"), true),
        (
            Unlistable,
            Held,
            Some("a Folio holds the data directory"),
            false,
        ),
        (Unlistable, Free, Some("cannot be read"), true),
        (Unlistable, Denied, Some("cannot be read"), true),
    ];
    for (at, (candidate, claim, deferral, delivered)) in rows.into_iter().enumerate() {
        let Some(install) = moved_in(&format!("row{at}")) else {
            return;
        };
        let row = make_row(&install, candidate, claim);
        let Some(deferral) = deferral else {
            let (code, world) =
                recovered_at_logon(&install, limits(5_000, 5_000), install.world(Trial::Silent));
            drop(row);
            assert_eq!(code, 0, "row {at}: {:?}", world.said);
            assert!(!deferred(&world), "row {at}: {:?}", world.said);
            rolled_back_on_disk(&install);
            // The same cell for a person's start, on a second installation
            // (the first one's rollback changed its state): it decides — the
            // rollback — and the exit guard delivers today's owed start, the
            // installed build with `--update-failed` and the handed line.
            let Some(install) = moved_in(&format!("row{at}-person")) else {
                return;
            };
            let (code, world) =
                recovered(&install, limits(5_000, 5_000), install.world(Trial::Silent));
            assert_eq!(code, 0, "row {at} (person): {:?}", world.said);
            assert!(!deferred(&world), "row {at} (person): {:?}", world.said);
            rolled_back_on_disk(&install);
            assert!(world.launched.is_empty(), "row {at} (person)");
            assert_eq!(
                world.opened,
                vec![(install.installed.clone(), failed_then(&install, &handed()))],
                "row {at} (person): {:?}",
                world.said
            );
            continue;
        };
        let (_code, world) =
            recovered_at_logon(&install, limits(5_000, 5_000), install.world(Trial::Silent));
        assert!(
            said_at(&world, deferral).is_some(),
            "row {at} {candidate:?}/{claim:?}: {:?}",
            world.said
        );
        assert!(world.opened.is_empty(), "row {at}: nobody waits at logon");
        assert_eq!(install.on_disk().body.phase, Phase::Moving, "row {at}");
        let mut person = install.world(Trial::Silent);
        person.real_ack = Some(Duration::from_millis(600));
        let (_code, world) = recovered(&install, limits(5_000, 5_000), person);
        drop(row);
        assert!(
            said_at(&world, deferral).is_some(),
            "row {at}: {:?}",
            world.said
        );
        assert!(world.launched.is_empty(), "row {at}: no trial launched");
        if delivered {
            assert_eq!(world.opened.len(), 2, "row {at}: {:?}", world.said);
            assert_eq!(world.shown.len(), 1, "row {at}");
        } else {
            assert!(world.opened.is_empty(), "row {at}: {:?}", world.said);
        }
        assert!(
            install.holds(Place::Install, &install.new),
            "row {at}: nothing moved"
        );
    }
}

/// RED (U-37; Codex's review of `153f7cb2`, finding 1) — **before a fresh
/// `Stuck`'s retrial the same step fails closed, and its deferral is the
/// road's end: over a held claim nothing is started beside its holder; a
/// denied claim question or a transaction folder that cannot be listed end
/// `Deferred`, and a person's start is delivered as H.3 says.**
///
/// A road declared the rollback, its first move back is refused (a foreign
/// file where the new one goes), so the journal becomes `Stuck` with the new
/// set live, and the recovery handed a person's start reaches its retrial.
///
/// MUTATION: in `Txn::retry_as_trial`, answer the earlier `ended` for
/// `Pre::Stop` (the round-4 code).
#[test]
fn a_deferral_before_a_fresh_stuck_retrial_is_the_roads_end() {
    for claim in [Claim::Held, Claim::Denied, Claim::Free] {
        let Some(install) = Install::new(&format!("stuck-{claim:?}")) else {
            return;
        };
        flipped_at(
            &install,
            Phase::RollbackIntent {
                trial: None,
                trial_started: false,
            },
        );
        block_rolledout(&install);
        let unlistable = matches!(claim, Claim::Free);
        let row = make_row(&install, Candidate::NotSeen, claim);
        let mut world = install.world(Trial::Silent);
        world.real_ack = Some(Duration::from_millis(600));
        let _candidate = unlistable.then(|| install.start_trial());
        if unlistable {
            // The folder goes the moment the rollback says it is stuck,
            // before the retrial's look.
            let folder = install.home.transaction(install.txn);
            world.on_say = Some(Box::new(move |line| {
                if line.contains("the update is incomplete") {
                    let _ = std::fs::rename(&folder, folder.with_extension("aside"));
                }
            }));
        }
        let (_code, world) = recovered(&install, limits(5_000, 5_000), world);
        if unlistable {
            let folder = install.home.transaction(install.txn);
            std::fs::rename(folder.with_extension("aside"), &folder).unwrap();
        }
        drop(row);
        let expected = match claim {
            Claim::Held => "Deferred(Held)",
            Claim::Denied => "Deferred(Denied(",
            Claim::Free => "Deferred(Unlistable(",
        };
        assert!(
            said_at(&world, expected).is_some(),
            "{claim:?}: {:?}",
            world.said
        );
        assert!(
            matches!(install.on_disk().body.phase, Phase::Stuck { .. }),
            "{claim:?}"
        );
        assert!(world.launched.is_empty(), "{claim:?}: no retrial");
        if matches!(claim, Claim::Held) {
            assert!(
                world.opened.is_empty(),
                "nothing beside the holder: {:?}",
                world.said
            );
        } else {
            assert_eq!(world.opened.len(), 2, "{claim:?}: {:?}", world.said);
            assert_eq!(world.shown.len(), 1, "{claim:?}");
        }
    }
}

/// RED (U-37, design revision (h), the rollout contract; Codex's review of
/// `153f7cb2`, finding 3(b)) — **the direct hop, as one road: a rescue build of
/// 0.4.6's shape (no adoption, no deferral) with a trial of 0.4.7 — its receipt
/// carrying `started` — commits the recorded trial by its nonce, and does not
/// adopt an unrecorded one (it rolls back under it, as 0.4.6 does); the 0.4.7
/// trial hands nothing back to a rescue build whose version is 0.4.6; and the
/// same unrecorded trial beside a rescue build of 0.4.7 is adopted.**
///
/// MUTATION: in `Txn::before_deciding`, ignore `as_046` (a 0.4.6 road that
/// adopts).
#[test]
fn the_direct_hop_from_0_4_6_ends_as_0_4_6_ends_it() {
    let exact = |install: &Install, nonce: Nonce, process: TrialProcess| {
        receipt_as(
            install,
            nonce,
            &Receipt {
                txn: install.txn,
                nonce,
                pid: process.pid,
                version: "0.4.7".to_owned(),
                started: Some(process.started),
            },
        );
    };
    let as_046 = |install: &Install| {
        let mut road = install.road(limits(5_000, 5_000));
        road.as_046 = true;
        road
    };
    let run = |road: Road, mut world: Fake| {
        on_a_worker(move |worker| {
            let code = crate::update_recover::run_windows(worker, &road, None, &mut world);
            (code, world)
        })
    };

    // Recorded: 0.4.6 commits by the nonce, whatever `started` says.
    let Some(install) = Install::new("hop-recorded") else {
        return;
    };
    flipped_at(&install, Phase::Moving);
    let trial = install.start_trial();
    let nonce = Nonce::new([0x71; 32]);
    install.write(Phase::Trial {
        nonce,
        process: trial,
        began_ms: now_ms(),
    });
    exact(&install, nonce, trial);
    let (code, world) = run(as_046(&install), install.world(Trial::Silent));
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Committed, Retired]"),
        "{:?}",
        world.said
    );

    // Unrecorded: 0.4.6 does not adopt; it rolls back, as it always did.
    let Some(install) = moved_in("hop-unrecorded") else {
        return;
    };
    let trial = install.start_trial();
    exact(&install, Nonce::new([0x72; 32]), trial);
    let (_code, world) = run(as_046(&install), install.world(Trial::Silent));
    assert!(
        !wrote(&world).contains("Trial") && wrote(&world).contains("RollbackIntent"),
        "{:?}",
        world.said
    );

    // The 0.4.7 trial hands nothing back to a 0.4.6 rescue build.
    let Some(install) = moved_in("hop-hand-back") else {
        return;
    };
    std::fs::remove_file(&install.rescue).unwrap();
    bt_platform::trust_harness::program(
        &install.rescue,
        FileVersion([0, 4, 6, 0]),
        bt_platform::trust_harness::Behaviour::Returns,
    )
    .unwrap();
    struct Refused(usize);
    impl crate::update_trial::Starter for Refused {
        fn start(&mut self, _program: &Path, _line: &[OsString]) -> io::Result<u32> {
            self.0 += 1;
            Ok(std::process::id())
        }
    }
    let (home, txn) = (install.home.clone(), install.txn);
    let starts = on_a_worker(move |worker| {
        let mut refused = Refused(0);
        let _ = crate::update_trial::hand_back(
            worker,
            &home,
            txn,
            true,
            crate::update_trial::FROM_TRIAL_SINCE,
            &mut refused,
        );
        refused.0
    });
    assert_eq!(starts, 0, "no hand-back to 0.4.6");

    // The same unrecorded trial beside a rescue build of 0.4.7: adopted.
    let trial = install.start_trial();
    exact(&install, Nonce::new([0x73; 32]), trial);
    let (code, world) = run(
        install.road(limits(5_000, 5_000)),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Trial, Committed, Retired]"),
        "{:?}",
        world.said
    );
}

// ── the real road: a trial's watch hands back to a recovery of its own ──────

/// The environment a child copy of this test binary reads its part from.
const ROAD_CHILD: &str = "BT_U37_ROAD_CHILD";
const ROAD_ROOT: &str = "BT_U37_ROAD_ROOT";
const ROAD_LINE: &str = "BT_U37_ROAD_LINE";

/// This test's own name, for its children.
const ROAD_TEST: &str = "update_apply_windows::tests::a_trial_hands_back_to_a_real_recovery_which_adopts_ends_or_defers";

/// **What the parent tells its children**, one fact a line in `road.txt`.
struct RoadPlan {
    home: PathBuf,
    installed: PathBuf,
    rescue: PathBuf,
    runner: PathBuf,
    data: PathBuf,
    txn: TxnId,
    /// `ready`, `unready` or `refused` (ready, its receipt never written).
    mode: String,
    every_ms: u64,
}

impl RoadPlan {
    fn write(&self, root: &Path) {
        let text = [
            self.home.display().to_string(),
            self.installed.display().to_string(),
            self.rescue.display().to_string(),
            self.runner.display().to_string(),
            self.data.display().to_string(),
            self.txn.to_string(),
            self.mode.clone(),
            self.every_ms.to_string(),
        ]
        .join("\n");
        std::fs::write(root.join("road.txt"), text).unwrap();
    }

    fn read(root: &Path) -> Self {
        let text = std::fs::read_to_string(root.join("road.txt")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        Self {
            home: PathBuf::from(lines[0]),
            installed: PathBuf::from(lines[1]),
            rescue: PathBuf::from(lines[2]),
            runner: PathBuf::from(lines[3]),
            data: PathBuf::from(lines[4]),
            txn: TxnId::parse(lines[5]).unwrap(),
            mode: lines[6].to_owned(),
            every_ms: lines[7].parse().unwrap(),
        }
    }
}

/// **A stand-in world with no installation of its own** (the rescue child's).
fn bare_world(home: Home) -> Fake {
    Fake {
        said: Vec::new(),
        registry: Memory::default(),
        opened: Vec::new(),
        launched: Vec::new(),
        children: Children::default(),
        trial: Trial::Silent,
        home,
        on_move: None,
        on_disarm: None,
        hold_at_launch: false,
        held: None,
        keep_held: false,
        panic_at_arm: false,
        refuse_start_of: None,
        starts_die: 0,
        refuse_every_start: false,
        refuse_starts: 0,
        shown: Vec::new(),
        on_say: None,
        before_start: None,
        real_ack: None,
        beside_launch: None,
    }
}

/// **The trial's part** (a child running from `<install>\folio.exe`): the real
/// watch over the real journal, with the product's `hand_back`, whose start is
/// the rescue runner in its recovery part; ready or not; its receipt written
/// as the product writes it, with its own start instant (unless `refused`).
fn trial_part(root: &Path) {
    let plan = RoadPlan::read(root);
    let home = Home::of(HostPlatform::Windows, &plan.installed).unwrap();
    assert_eq!(home.root(), plan.home.as_path());
    let gate: &'static crate::update_trial::Gate =
        Box::leak(Box::new(crate::update_trial::Gate::new()));
    let me = crate::update_apply::this_process();
    let ready = plan.mode != "unready";
    if ready {
        gate.ready_for_a_test();
    }
    if plan.mode == "ready" {
        let nonce = Nonce::new([0x7a; 32]);
        let receipt = Receipt {
            txn: plan.txn,
            nonce,
            pid: me.pid,
            version: "0.4.7".to_owned(),
            started: Some(me.started),
        };
        install_txn::durable_create(&home.receipt_path(plan.txn, &nonce), &receipt.encode())
            .unwrap();
    }
    struct Runner {
        runner: PathBuf,
        root: PathBuf,
    }
    impl crate::update_trial::Starter for Runner {
        fn start(&mut self, program: &Path, line: &[OsString]) -> io::Result<u32> {
            let mut said = program.display().to_string();
            for word in line {
                said.push('\n');
                said.push_str(&word.to_string_lossy());
            }
            bt_platform::quiet_command(&self.runner)
                .args(["--exact", ROAD_TEST, "--nocapture"])
                .env(ROAD_CHILD, "rescue")
                .env(ROAD_ROOT, &self.root)
                .env(ROAD_LINE, said)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|child| child.id())
        }
    }
    let every = Duration::from_millis(plan.every_ms);
    let journal = home.journal();
    let root = root.to_path_buf();
    let watched = bt_platform::spawn_at_priority(
        "bt-u37-road-trial",
        bt_platform::ThreadPriority::BelowNormal,
        move |ctx| {
            let mut runner = Runner {
                runner: plan.runner.clone(),
                root: root.clone(),
            };
            let mut hand_back = |ready: bool| {
                crate::update_trial::hand_back(
                    ctx,
                    &home,
                    plan.txn,
                    ready,
                    crate::update_trial::FROM_TRIAL_SINCE,
                    &mut runner,
                )
            };
            let mut watchdog = crate::update_trial::Watchdog {
                every,
                hand_back: &mut hand_back,
            };
            crate::update_trial::watch(
                gate,
                &journal,
                plan.txn,
                Duration::from_millis(20),
                &|| {},
                None,
                &mut watchdog,
            );
            std::fs::write(root.join("trial-decided"), b"").unwrap();
        },
    )
    .unwrap();
    drop(watched);
    // Stays up until the recovery or the parent ends it (the parent's own
    // bound is shorter; this only keeps an orphan from living for ever).
    std::thread::sleep(Duration::from_secs(600));
}

/// **The recovery's part** (a child running from the rescue runner, a copy of
/// this test binary elsewhere): the line the trial's `hand_back` built, read by
/// the recovery door's own grammar; the product's recovery over the real
/// journal and the real transaction lock; what it did, written down.
fn rescue_part(root: &Path) {
    let plan = RoadPlan::read(root);
    let started_ms = crate::update_apply::now_ms();
    let said = std::env::var(ROAD_LINE).unwrap();
    let mut words = said.lines();
    let program = PathBuf::from(words.next().unwrap());
    let line: Vec<OsString> = words.map(OsString::from).collect();
    let Some(Ok(cli::UpdateDoor::Recover {
        home: None,
        then_launch: None,
        handed_back: Some(handed_back),
    })) = cli::update_door(line.clone())
    else {
        panic!("the recovery door's line: {line:?}");
    };
    let me = crate::update_apply::this_process();
    let road = Road {
        home: Home::of(HostPlatform::Windows, &plan.installed).unwrap(),
        installed: plan.installed.clone(),
        rescue: plan.rescue.clone(),
        data: plan.data.clone(),
        policy: Policy::System,
        channel: None,
        limits: Limits {
            old_within: Duration::from_secs(10),
            trial_within_ms: 5_000,
            poll: Duration::from_millis(40),
            quit_within: Duration::from_millis(300),
            end_within: Duration::from_secs(10),
        },
        me,
        starter: install_flip::parent_of_this_process(),
        handed_back: Some(handed_back),
        layouts: own_layouts(),
        as_046: false,
    };
    let mut world = bare_world(road.home.clone());
    let (code, world) = on_a_worker(move |worker| {
        let code = crate::update_recover::run_windows(worker, &road, None, &mut world);
        (code, world)
    });
    let first = !root.join("lingered").exists();
    if first && plan.mode == "refused" {
        // The first recovery lingers two periods — past the watchdog's next
        // due time, which comes one period after the first: the watch must
        // not start another beside it.
        std::fs::write(root.join("lingered"), b"").unwrap();
        std::thread::sleep(Duration::from_millis(plan.every_ms * 2));
    }
    let ended_ms = crate::update_apply::now_ms();
    let done = format!(
        "{}\n{}:{}\n{}\n{}\n{}\n{}\n{}",
        me.pid,
        started_ms,
        ended_ms,
        code,
        program.display(),
        handed_back.word(),
        world.opened.len() + world.launched.len(),
        world.said.join(" | ")
    );
    std::fs::write(root.join(format!("account-{started_ms}-{}", me.pid)), done).unwrap();
}

/// The recoveries' accounts so far, in the order they ended.
fn rescues(root: &Path) -> Vec<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("account-"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            std::fs::read_to_string(root.join(name))
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

/// RED (U-37, design revision (h) H.2, H.3 and H.4; Codex's review of
/// `153f7cb2`, finding 3(a) and 3(c)) — **the real road, hermetic: a trial (a
/// copy of this binary running from `<install>\folio.exe`, its real watch and
/// the product's `hand_back`) hands its transaction back, with its exact
/// identity, to a recovery (another copy, started by that hand-back, reading
/// the line through the recovery door's grammar and taking the real
/// transaction lock). A ready trial whose receipt names it is recorded and
/// committed and stays running; an unready one is ended — that exact
/// instance — and the rollback follows; a ready one whose receipt the home
/// refuses is deferred to, one recovery at a time, never with a trial
/// launched beside it, and handed back no more than the watchdog's bound
/// allows.**
///
/// The "refused" part asserts invariants, never a count within a window of
/// time: it waits for the watch to say its schedule is spent (the trial's own
/// log), then checks that the four due times were each a hand-back or a skip,
/// that the cap held, and, from each recovery's recorded start and end
/// instants, that no two were alive at once.
///
/// The rescue executable on disk carries `VERSIONINFO` 0.4.7 (the product's
/// gate is asked); the start itself is of the runner copy, as a rescue build
/// that records its argv would be.
///
/// MUTATIONS: in `update_trial::watch`, hand back beside a recovery still
/// running (the "refused" part starts one more); in
/// `update_apply::before_deciding`, skip the end of an unready handed-back
/// trial (the "unready" part never ends the trial).
#[test]
fn a_trial_hands_back_to_a_real_recovery_which_adopts_ends_or_defers() {
    if let Ok(part) = std::env::var(ROAD_CHILD) {
        let root = PathBuf::from(std::env::var(ROAD_ROOT).unwrap());
        match part.as_str() {
            "trial" => trial_part(&root),
            _ => rescue_part(&root),
        }
        return;
    }
    let this = std::env::current_exe().unwrap();
    for (mode, every_ms) in [("ready", 1_500), ("unready", 1_500), ("refused", 2_000)] {
        let Some(mut install) = Install::new(&format!("road-{mode}")) else {
            return;
        };
        // The new set's `folio.exe` is a copy of this binary: the trial runs it.
        let program = install.folder(Place::Set).join("folio.exe");
        std::fs::remove_file(&program).unwrap();
        std::fs::copy(&this, &program).unwrap();
        let bytes = std::fs::read(&program).unwrap();
        for member in &mut install.inventories.new {
            if member.name == "folio.exe" {
                member.digest = Digest::new(sha256(&bytes));
                member.size = bytes.len() as u64;
            }
        }
        for (name, held) in &mut install.new {
            if name == "folio.exe" {
                held.clone_from(&bytes);
            }
        }
        // The rescue build on disk says 0.4.7: it takes a hand-back.
        std::fs::remove_file(&install.rescue).unwrap();
        bt_platform::trust_harness::program(
            &install.rescue,
            FileVersion([0, 4, 7, 0]),
            bt_platform::trust_harness::Behaviour::Returns,
        )
        .unwrap();
        flipped_at(&install, Phase::Moving);
        let root = install
            .home
            .root()
            .parent()
            .unwrap()
            .join(format!("road-{mode}"));
        std::fs::create_dir_all(&root).unwrap();
        let runner = root.join("rescue-runner.exe");
        std::fs::copy(&this, &runner).unwrap();
        RoadPlan {
            home: install.home.root().to_path_buf(),
            installed: install.installed.clone(),
            rescue: install.rescue.clone(),
            runner,
            data: install.data.clone(),
            txn: install.txn,
            mode: mode.to_owned(),
            every_ms,
        }
        .write(&root);
        let child = bt_platform::quiet_command(&install.installed)
            .args(["--exact", ROAD_TEST, "--nocapture"])
            .env(ROAD_CHILD, "trial")
            .env(ROAD_ROOT, &root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            // The watch's own lines: each due time's hand-back or skip, and
            // the end of its schedule.
            .stderr(std::fs::File::create(root.join("trial.err")).unwrap())
            .spawn()
            .unwrap();
        /// Ends the trial child by its own handle whatever this test does.
        struct Owned(std::process::Child);
        impl Drop for Owned {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut trial = Owned(child);
        let pid = trial.0.id();
        let started = install_flip::started_of(pid).unwrap();
        // A bound for a slow runner only; nothing below counts what happens
        // within a window of time.
        let give_up = Instant::now() + Duration::from_secs(300);
        let until = |done: &mut dyn FnMut() -> bool| {
            while !done() {
                assert!(
                    Instant::now() < give_up,
                    "{mode}: the road did not get there"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        match mode {
            "ready" => {
                until(&mut || root.join("trial-decided").exists());
                until(&mut || !rescues(&root).is_empty());
                assert_eq!(
                    install.on_disk().body.phase,
                    Phase::Retired {
                        outcome: Outcome::Committed,
                        untried: false,
                    }
                );
                assert!(matches!(trial.0.try_wait(), Ok(None)), "the trial stays");
                let done = rescues(&root);
                assert_eq!(done.len(), 1, "{done:?}");
                assert_eq!(done[0][3], install.rescue.display().to_string());
                assert_eq!(done[0][4], format!("{pid}:{started}:ready"));
                assert_eq!(done[0][5], "0", "nothing started or launched");
                assert!(done[0][6].contains("it is recorded"), "{done:?}");
            }
            "unready" => {
                until(&mut || !matches!(trial.0.try_wait(), Ok(None)));
                until(&mut || !rescues(&root).is_empty());
                let done = rescues(&root);
                assert_eq!(done[0][4], format!("{pid}:{started}:unready"));
                assert!(done[0][6].contains("is asked to"), "{done:?}");
                assert!(
                    matches!(
                        install.on_disk().body.phase,
                        Phase::Retired {
                            outcome: Outcome::RolledBack,
                            ..
                        }
                    ),
                    "{done:?}"
                );
            }
            _ => {
                // The schedule's end, as the watch says it; then every
                // recovery it started has written its account.
                let said = || std::fs::read_to_string(root.join("trial.err")).unwrap_or_default();
                until(&mut || said().contains("the watchdog is spent"));
                let lines = said();
                let launched = lines.matches("it is handed back to").count();
                let skipped = lines
                    .matches("the recovery handed it before still runs")
                    .count();
                until(&mut || rescues(&root).len() >= launched);
                // The invariants, not a count within a window of time: the
                // four due times are each a hand-back or a skip beside a
                // recovery still running; the cap holds; one recovery at a
                // time; each deferred to the candidate and started nothing.
                assert_eq!(launched + skipped, 4, "{lines}");
                assert!((1..=4).contains(&launched), "{lines}");
                let done = rescues(&root);
                assert_eq!(done.len(), launched, "{done:?}");
                let span = |account: &[String]| -> (u64, u64) {
                    let (from, to) = account[1].split_once(':').unwrap();
                    (from.parse().unwrap(), to.parse().unwrap())
                };
                for pair in done.windows(2) {
                    assert!(
                        span(&pair[1]).0 >= span(&pair[0]).1,
                        "never two recoveries at once: {pair:?}"
                    );
                }
                for account in &done {
                    assert_eq!(account[5], "0", "no trial launched beside it: {account:?}");
                    assert!(account[6].contains("deferred"), "{account:?}");
                    assert_eq!(account[4], format!("{pid}:{started}:ready"));
                }
                assert!(matches!(trial.0.try_wait(), Ok(None)), "the trial stays");
                assert_eq!(install.on_disk().body.phase, Phase::Moving);
            }
        }
        drop(trial);
    }
}

/// **Run `name` in a copy of this test binary** whose standard error is the
/// file `log`, opened for appending as a Folio's streams are
/// (`diagnostics::choose_resident_channel`), with `child` naming `log` in its
/// environment; answers the file's text afterwards.
pub(crate) fn said_by_a_child_whose_stderr_is_the_log(
    name: &str,
    child: &str,
    tag: &str,
) -> String {
    let folder = bt_testpath::temp_path(&format!("bt-u42d-{tag}"));
    std::fs::create_dir_all(&folder).unwrap();
    let log = folder.join("diagnostics.log");
    let stream = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .unwrap();
    let status = bt_platform::quiet_command(std::env::current_exe().expect("this test binary"))
        .args(["--exact", name, "--test-threads=1"])
        .env(child, &log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stream))
        .status()
        .expect("the child runs");
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&folder);
    assert!(status.success(), "the child failed: {text}");
    text
}

/// RED (U-42d) — **a road process's line reaches `diagnostics.log` once,
/// when its standard error is that same log.**
///
/// 0.4.6's D-11: every `BT_UPDATE_APPLY` and `BT_UPDATE_RECOVER` line was in
/// the file twice. The applier inherits the standard error of the Folio that
/// started it, which is that Folio's `diagnostics.log`, and it also appended
/// each line to the log by name. The child here is a copy of this test
/// binary whose standard error is the log, as the applier's is.
///
/// MUTATION: in `Machine::say`, write standard error too when a log is named
/// — the line is in the file twice.
#[test]
fn a_road_line_reaches_the_log_once_when_standard_error_is_that_log() {
    const CHILD: &str = "BT_U42D_WINDOWS_SAY";
    if let Some(log) = std::env::var_os(CHILD) {
        let mut machine = Machine {
            log: Some(PathBuf::from(log)),
        };
        World::say(&mut machine, "BT_UPDATE_APPLY one line, once");
        return;
    }
    let text = said_by_a_child_whose_stderr_is_the_log(
        "update_apply_windows::tests::a_road_line_reaches_the_log_once_when_standard_error_is_that_log",
        CHILD,
        "windows",
    );
    assert_eq!(
        text.matches("BT_UPDATE_APPLY one line, once").count(),
        1,
        "{text}"
    );
}

/// RED (U-42d, D-15; review finding 5) — **the applier's wait for a held
/// file sleeps one poll between two looks, so a hold let go is seen at the
/// next look, and a hold that outlasts the window refuses exactly at its
/// end.**
///
/// On the clean machine E-7's hold lasted 200 s, past the applier's 60 s, so
/// the ~61 s with no window there was the whole wait run out. The clock and
/// the pause here are the test's: each sleep moves the clock by exactly what
/// was asked, so the count of polls is exact and nothing waits.
///
/// MUTATION: in `until_let_go`, sleep what is left of the window instead of
/// `poll.min(left)`: the first sleep is the whole window.
#[test]
fn a_held_file_let_go_is_seen_at_the_next_poll() {
    let poll = Duration::from_millis(250);
    let window_length = Duration::from_secs(60);
    let start = Instant::now();
    let window = start + window_length;

    // Held for three looks, then let go.
    let clock = std::cell::Cell::new(start);
    let mut looks = 0;
    let mut sleeps = Vec::new();
    let answer = until_let_go(
        window,
        poll,
        &mut || clock.get(),
        &mut || {
            looks += 1;
            Ok(if looks <= 3 {
                vec!["folio.exe".to_owned()]
            } else {
                Vec::new()
            })
        },
        &mut |pause| {
            sleeps.push(pause);
            clock.set(clock.get() + pause);
        },
    );
    assert_eq!(answer, Ok(()));
    assert_eq!(sleeps, vec![poll; 3], "one poll between two looks");
    assert_eq!(clock.get() - start, 3 * poll, "seen at the next poll");

    // Held for good: refused when the window has passed, never after it.
    let clock = std::cell::Cell::new(start);
    let mut slept = Duration::ZERO;
    let answer = until_let_go(
        window,
        poll,
        &mut || clock.get(),
        &mut || Ok(vec!["folio.exe".to_owned()]),
        &mut |pause| {
            assert!(pause <= poll, "{pause:?} is more than one poll");
            slept += pause;
            clock.set(clock.get() + pause);
        },
    );
    assert_eq!(
        answer,
        Err("held open by another process: folio.exe".to_owned())
    );
    assert_eq!(slept, window_length, "the window, and not a poll more");
}

// ── the layout's points (U-41a1) ────────────────────────────────────────────

/// One of a layout's points on this road.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Point {
    Activate,
    ActivateBack,
    Locate,
    Live,
}

/// **A fake layout: every call the road makes of it, with the phase the
/// journal on disk stood at when it was made** — the harness U-41b and
/// U-41c build their adapters' road tests on (managed-update §6). It
/// delegates to Folio's own layout, so the road it is handed runs over the
/// real install; told to, it refuses its `Activate` with nothing moved.
#[derive(Clone, Default)]
pub(crate) struct Recorder {
    pub(crate) calls: Arc<Mutex<Vec<(Point, PhaseKind)>>>,
    pub(crate) refuse_activate: bool,
}

impl Recorder {
    fn note(&self, point: Point, site: &Site<'_>) {
        let durable = Journal::parse(&std::fs::read(site.home.journal()).unwrap())
            .unwrap()
            .body
            .phase
            .kind();
        self.calls.lock().unwrap().push((point, durable));
    }

    fn calls(&self) -> Vec<(Point, PhaseKind)> {
        self.calls.lock().unwrap().clone()
    }
}

impl ApplyPoints for Recorder {
    fn activate(
        &self,
        site: &Site<'_>,
        journal: &Journaled<'_>,
        world: &mut dyn World,
    ) -> Result<Activated, String> {
        self.note(Point::Activate, site);
        if self.refuse_activate {
            return Ok(Activated::Cut(
                "the fake layout refuses to activate (test)".to_owned(),
            ));
        }
        Ours.activate(site, journal, world)
    }

    fn activate_back(
        &self,
        site: &Site<'_>,
        journal: &Journaled<'_>,
        actor: Actor,
        moves: &[Move],
        world: &mut dyn World,
    ) -> Result<Activated, String> {
        self.note(Point::ActivateBack, site);
        Ours.activate_back(site, journal, actor, moves, world)
    }

    fn locate(&self, site: &Site<'_>) -> Result<Located, String> {
        self.note(Point::Locate, site);
        Ours.locate(site)
    }

    fn live(&self, site: &Site<'_>) -> Option<Live> {
        self.note(Point::Live, site);
        Ours.live(site)
    }
}

/// `install`'s road, over `recorder` in place of Folio's own layout.
fn recorded_road(install: &Install, recorder: &Recorder, limits: Limits) -> Road {
    Road {
        layouts: Layouts::of(Arc::new(recorder.clone())),
        ..install.road(limits)
    }
}

/// RED (U-41a1, managed-update §1.1 R1–R2) — **the road calls the layout its
/// journal names, each point once at the phase the note's table gives it:
/// `Activate` once, with `Moving` durable; `Activate` back once, with
/// `RollbackIntent` durable; the locator at every step `decide` takes; and a
/// journal naming an adapter this build has not built is refused before any
/// of its points is called or anything is moved.**
///
/// The harness is the fake layout U-41b and U-41c will hand the same road
/// ([`Recorder`]): it reads the journal on disk at each call, so "after
/// `Moving` is durable" is what the disk said when the layout was asked, not
/// what the road meant to write. It delegates to Folio's own layout, so the
/// two roads — a trial that answers and commits, a trial that dies and is
/// rolled back — are the real ones over the real install, and end exactly as
/// the W rows do. The adapter comes from the journal and never from the
/// channel (R2): the same road over a journal naming scoop refuses with the
/// channel still `Ours`.
///
/// MUTATION: in `Txn::at_armed`, make the moves with `Ours.activate` in place
/// of the layout the journal names (`self.layout`) — the recorder hears no
/// `Activate` and the sequences go red.
#[test]
fn the_road_calls_each_point_of_the_layout_the_journal_names_once_per_phase() {
    let Some(install) = Install::new("points") else {
        return;
    };
    let recorder = Recorder::default();
    let road = recorded_road(&install, &recorder, limits(20_000, 20_000));
    let (ended, world) = applied_on(&install, road, install.world(Trial::Answers));
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert_eq!(
        recorder.calls(),
        vec![
            (Point::Activate, PhaseKind::Moving),
            (Point::Locate, PhaseKind::Committed),
        ],
        "a committed road: Activate at Moving, the locator for the commit's retirement"
    );
    assert!(install.holds(Place::Install, &install.new));

    let Some(install) = Install::new("points-back") else {
        return;
    };
    let recorder = Recorder::default();
    let road = recorded_road(&install, &recorder, limits(20_000, 20_000));
    let (ended, world) = applied_on(&install, road, install.world(Trial::Dies));
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert_eq!(
        recorder.calls(),
        vec![
            (Point::Activate, PhaseKind::Moving),
            (Point::Locate, PhaseKind::RollbackIntent),
            (Point::ActivateBack, PhaseKind::RollbackIntent),
            (Point::Locate, PhaseKind::RollbackIntent),
            (Point::Locate, PhaseKind::RolledBack),
        ],
        "a rolled-back road: the locator before each of decide's steps, Activate back once"
    );
    rolled_back_on_disk(&install);

    let Some(install) = Install::new("points-scoop") else {
        return;
    };
    let named = install.on_disk().naming(Adapter::Scoop);
    install_txn::durable_write(&install.home.journal(), &named.encode()).unwrap();
    let recorder = Recorder::default();
    let road = recorded_road(&install, &recorder, limits(2_000, 2_000));
    let (ended, world) = applied_on(&install, road, install.world(Trial::Answers));
    assert!(
        matches!(&ended, Ended::Refused(why) if why.contains("Scoop")),
        "{ended:?} {:?}",
        world.said
    );
    assert!(
        recorder.calls().is_empty(),
        "no point of any layout is called"
    );
    nothing_moved(&install);
    assert!(world.launched.is_empty(), "no trial");
}

/// RED (U-41a1, managed-update §1.1 R4 and the self-update note's W6) — **a
/// layout that refuses its `Activate` leaves the road where a failed move
/// leaves it: `RollbackIntent`, the rollback over what the disk holds, and
/// `Retired{RolledBack}` with no trial ever begun — the old build opened with
/// the rolled-back card, nothing moved.**
///
/// The refusal is the layout's (`Activated::Cut`), and the road's answer to
/// it is common: it does not know whether the layout moved anything, so it
/// decides from the disk, as recovery does.
///
/// MUTATION: in `Txn::at_armed`, go on to the trial after a `Cut` — a trial
/// of the old build is started and the journal records `Trial`.
#[test]
fn a_layout_that_refuses_to_activate_is_rolled_back_untried() {
    let Some(install) = Install::new("refuses") else {
        return;
    };
    let recorder = Recorder {
        refuse_activate: true,
        ..Recorder::default()
    };
    let road = recorded_road(&install, &recorder, limits(20_000, 20_000));
    let (ended, world) = applied_on(&install, road, install.world(Trial::Answers));
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        wrote(&world).ends_with("[Armed, Moving, RollbackIntent, RolledBack, Retired]"),
        "{:?}",
        world.said
    );
    assert!(
        said_at(&world, "the fake layout refuses to activate").is_some(),
        "{:?}",
        world.said
    );
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::RolledBack,
            untried: true,
        }
    );
    assert!(world.launched.is_empty(), "no trial");
    nothing_moved(&install);
    assert_eq!(
        world.opened,
        vec![(install.installed.clone(), failed_then(&install, &[]))]
    );
    assert_eq!(recorder.calls()[0], (Point::Activate, PhaseKind::Moving));
    assert!(
        !recorder
            .calls()
            .iter()
            .any(|(point, _)| *point == Point::ActivateBack),
        "nothing to move back"
    );
}

// ── the escape hatch (0.4.8 E1) ─────────────────────────────────────────────

/// RED (E1; role #9, the Windows exit guard, sites H4 and J6 `opens_now`) —
/// **over a journal this build cannot read whole, the exit opens what an
/// unknown live set opens — the rescue copy with `--update-failed` — and
/// never the installed build plainly**: an unknown header word (its
/// envelope reads as `destructive`), an unknown body word under a
/// `destructive` header, and bytes of which nothing reads. A header that
/// reads keeps its frozen class's answer: a retired rollback over an unknown
/// body opens the installed build with its card. The journal is byte for
/// byte as it was.
///
/// MUTATION: in `opens_now`, answer `Opens::Installed { failed: false }` for
/// a journal of which nothing reads (the pre-E1 answer).
#[test]
fn the_windows_exit_opens_the_rescue_over_what_it_cannot_read_whole() {
    let Some(install) = Install::new("beyond-exit") else {
        return;
    };
    install.write(Phase::Moving);
    let known = std::fs::read(install.home.journal()).unwrap();
    let road = install.road(limits(600, 20_000));
    for (what, bytes) in crate::update_txn::beyond_inputs(&known) {
        install_txn::durable_write(&install.home.journal(), &bytes).unwrap();
        assert_eq!(opens_now(&road), Opens::Rescue, "{what}");
        let (program, words) = road.opening(&opens_now(&road));
        assert_eq!(program, install.rescue.as_path(), "{what}");
        assert_eq!(words, failed_words(&install.home).to_vec(), "{what}");
        assert_eq!(
            std::fs::read(install.home.journal()).unwrap(),
            bytes,
            "{what}"
        );
    }
    install.write(Phase::Retired {
        outcome: Outcome::RolledBack,
        untried: false,
    });
    let retired = std::fs::read(install.home.journal()).unwrap();
    let [_, (what, unknown_body), _] = crate::update_txn::beyond_inputs(&retired);
    install_txn::durable_write(&install.home.journal(), &unknown_body).unwrap();
    assert_eq!(
        opens_now(&road),
        Opens::Installed { failed: true },
        "{what}: the frozen class decides"
    );
}

/// RED (E1; role #10, the Windows lock holder, site J7 `read_journal` — the
/// recovery's `hold` and the applier's `under_the_lock`) — **a lock holder
/// stands aside from a journal this build cannot read whole**: the recovery
/// and the applier each end `Ended::StoodAside`, recording, removing,
/// moving and ending nothing, and the lock is let go. Handed a person's
/// start, the recovery owes a window; at logon it owes none
/// (`update_apply::owed_at_logon`). The applier's exit opens the rescue copy
/// (`opens_now`, role #9).
///
/// MUTATION: in `hold`, answer the pre-E1 `Ended::Left` for a journal this
/// build cannot read whole.
#[test]
fn the_windows_lock_holder_stands_aside_from_what_it_cannot_read_whole() {
    let Some(install) = Install::new("beyond-holder") else {
        return;
    };
    install.claim_window();
    install.write(Phase::Moving);
    let known = std::fs::read(install.home.journal()).unwrap();
    for (what, bytes) in crate::update_txn::beyond_inputs(&known) {
        install_txn::durable_write(&install.home.journal(), &bytes).unwrap();
        for start in [Some(handed()), None] {
            let road = install.road(limits(600, 20_000));
            let mut world = install.world(Trial::Answers);
            let waits = start.is_some();
            let recovered =
                on_a_worker(move |worker| recover(worker, &road, &mut world, start.as_deref()));
            assert!(
                matches!(recovered.ended, Ended::StoodAside(_)),
                "{what}: {:?}",
                recovered.ended
            );
            assert_eq!(recovered.successor, None, "{what}");
            assert_eq!(
                recovered.waiting, waits,
                "{what}: a window is owed to a person's start, none at logon"
            );
            assert_eq!(std::fs::read(install.home.journal()).unwrap(), bytes);
        }
        let (ended, world) = applied(&install, limits(600, 20_000), install.world(Trial::Answers));
        assert!(matches!(ended, Ended::StoodAside(_)), "{what}: {ended:?}");
        assert_eq!(
            world.opened.first().map(|(program, _)| program.clone()),
            Some(install.rescue.clone()),
            "{what}: {:?}",
            world.opened
        );
        assert_eq!(
            std::fs::read(install.home.journal()).unwrap(),
            bytes,
            "{what}"
        );
        assert!(
            install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
                .unwrap()
                .is_some(),
            "{what}: the lock is let go"
        );
        nothing_moved(&install);
        assert!(!install.registry.holds(install.txn), "{what}: no entrance");
    }
}

/// RED (E1; role #3, the trial's watchdog, site H3 `update_trial::hand_back`)
/// — **a trial hands back a journal it cannot read whole to the rescue
/// build its header acts on**: the envelope's, for an unknown header word;
/// the header's, for an unknown body word; and nobody's when nothing reads
/// (no holder could read it either). The journal is byte for byte as it was.
///
/// MUTATION: in `hand_back`, read the header alone again
/// (`Header::parse(&bytes).ok()`): an unknown header word is never handed
/// back, and nobody is asked to settle it.
#[test]
fn a_trial_hands_back_what_it_cannot_read_whole_to_the_rescue_its_header_names() {
    let Some(install) = moved_in("beyond-hand-back") else {
        return;
    };
    struct Recorded(Vec<(PathBuf, Vec<OsString>)>);
    impl crate::update_trial::Starter for Recorded {
        fn start(&mut self, program: &Path, line: &[OsString]) -> io::Result<u32> {
            self.0.push((program.to_path_buf(), line.to_vec()));
            Ok(std::process::id())
        }
    }
    std::fs::remove_file(&install.rescue).unwrap();
    bt_platform::trust_harness::program(
        &install.rescue,
        FileVersion([0, 4, 7, 0]),
        bt_platform::trust_harness::Behaviour::Returns,
    )
    .unwrap();
    let known = std::fs::read(install.home.journal()).unwrap();
    for (index, (what, bytes)) in crate::update_txn::beyond_inputs(&known)
        .into_iter()
        .enumerate()
    {
        install_txn::durable_write(&install.home.journal(), &bytes).unwrap();
        let (home, txn) = (install.home.clone(), install.txn);
        let recorded = on_a_worker(move |worker| {
            let mut recorded = Recorded(Vec::new());
            crate::update_trial::hand_back(
                worker,
                &home,
                txn,
                false,
                crate::update_trial::FROM_TRIAL_SINCE,
                &mut recorded,
            );
            recorded.0
        });
        let programs: Vec<PathBuf> = recorded.into_iter().map(|(program, _)| program).collect();
        if index < 2 {
            assert_eq!(programs, vec![install.rescue.clone()], "{what}");
        } else {
            assert!(programs.is_empty(), "{what}: {programs:?}");
        }
        assert_eq!(
            std::fs::read(install.home.journal()).unwrap(),
            bytes,
            "{what}"
        );
    }
}
