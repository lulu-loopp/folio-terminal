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

use crate::update_apply::{Left, Window};
use crate::update_handoff::{Leaving, Spawner};
use crate::update_prepare_windows::tests::build_that;
use crate::update_txn::{
    Body, Class, Header, HeaderOutcome, Member, Outcome, PhaseKind, Receipt, STUCK_ATTEMPT_LIMIT,
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
    /// The failure windows shown in this process (U-34, round 2).
    shown: Vec<String>,
    /// Looks at every line as it is said (U-37).
    on_say: Option<SayHook>,
    /// **Acknowledge a start only as the product does** (U-37, H.3): a Folio
    /// holding the data directory, asked through `update_apply::claimed_within`
    /// for this long — a denied claim question is no acknowledgement.
    real_ack: Option<Duration>,
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
        self.opened.push((program.to_path_buf(), args.to_vec()));
        if self.refuse_every_start || self.refuse_start_of.as_deref() == Some(program) {
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
        let root = std::env::temp_dir().join(format!(
            "bt-u24-{tag}-{}-{}",
            std::process::id(),
            bt_platform::attention_pipe::unguessable_bits() % 1_000_000
        ));
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
            shown: Vec::new(),
            on_say: None,
            real_ack: None,
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

/// Wait until the journal on disk is at `phase`.
fn until_journal(install: &Install, phase: PhaseKind) {
    let give_up = Instant::now() + Duration::from_secs(30);
    while install.on_disk().body.phase.kind() != phase {
        assert!(
            Instant::now() < give_up,
            "the journal never got to {phase:?}"
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
    until_journal(&install, PhaseKind::Armed);
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

/// RED (U-23) — **an applier started again over its own transaction goes on
/// from the disk**: `Handoff` with an entrance already there (W4) removes it
/// and reverts, nothing moved, the old build started again; `Armed` (W5)
/// admits and goes on to `Committed`; `Moving` with some moves done (W6)
/// declares the rollback, moves nothing further forward, and rolls back
/// (U-24).
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
            _ => Phase::RollbackIntent { trial: None },
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
    flipped_at(&install, Phase::RollbackIntent { trial: None });
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
    flipped_at(&install, Phase::RollbackIntent { trial: None });
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
    install.write(Phase::RollbackIntent { trial: Some(trial) });
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
            "intent" => flipped_at(&install, Phase::RollbackIntent { trial: None }),
            "rolled-back" => {
                install.arm();
                install.write(Phase::RolledBack { untried: false });
            }
            "stuck-new" => {
                flipped_at(&install, stuck(1));
                block_rolledout(&install);
            }
            "waits" => flipped_at(&install, Phase::RollbackIntent { trial: None }),
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
    assert_eq!(
        crate::update_apply::take_the_window(
            &install.home,
            install.txn,
            applier,
            Instant::now() + crate::update_apply::ELECTION_WITHIN
        ),
        Window::Mine
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

/// RED (U-34, rounds 4 and 5; Codex's finding 14) — **a stale window's mark is
/// taken over by exactly one of two contenders racing for it** (a soak): the
/// election is one exclusive lock around read-check-replace, so the first in
/// replaces the stale value and the second reads the live winner, `Theirs`. Two threads race, released together by a
/// barrier, over many rounds, each round from a fresh stale mark; the two
/// contenders are both live processes (this test and a synthetic program it
/// started), so neither can be taken for a dead owner.
///
/// MUTATION: in `update_apply::take_the_window_within`, skip the lock (check,
/// then `durable_write`) — both contenders then answer `Mine` in some round.
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
    let me = crate::update_apply::this_process();
    let stale = format!("{}:{}", me.pid, me.started.wrapping_add(1));
    let mark = crate::update_apply::owner_path(&install.home, install.txn);
    for round in 0..40 {
        std::fs::write(&mark, format!("{stale}{}", "0".repeat(round % 3))).unwrap();
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
                            &home,
                            txn,
                            who,
                            Instant::now() + crate::update_apply::ELECTION_WITHIN,
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
        let mine = answers
            .iter()
            .filter(|answer| **answer == Window::Mine)
            .count();
        assert_eq!(mine, 1, "round {round}: {answers:?}");
        let winner = contenders[answers.iter().position(|a| *a == Window::Mine).unwrap()];
        assert!(
            answers.contains(&Window::Theirs(winner)),
            "round {round}: the loser names the winner: {answers:?}"
        );
        assert_eq!(
            crate::update_apply::window_owner(&install.home, install.txn),
            Some(winner),
            "round {round}"
        );
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
    assert_eq!(
        crate::update_apply::take_the_window(
            &install.home,
            install.txn,
            old_running,
            Instant::now() + crate::update_apply::ELECTION_WITHIN
        ),
        Window::Mine
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

/// RED (U-34, rounds 7 and 8; Codex's finding 15) — **the applier's wait for O
/// is one budget**: a wait for O's mark that spent most of `old_within` leaves
/// only the rest for the election's lock (held here by the test, so the
/// election has to wait) and then for O's transaction lock (held too), so the
/// applier ends `OldHeldTheLock` at the one deadline — not an election's 5 s
/// later, nor a fresh `old_within` later.
///
/// MUTATIONS: in `take_the_window`, wait the full `ELECTION_WITHIN`; in
/// `apply_under_the_lock`, wait for the lock a fresh `road.limits.old_within`.
#[test]
fn the_applier_waits_for_o_within_one_budget() {
    let Some(install) = Install::new("one-budget") else {
        return;
    };
    let old = install.children.start(&install.rescue, &[]);
    let old_running = Running {
        pid: old,
        started: install_flip::started_of(old).expect("O runs"),
    };
    assert_eq!(
        crate::update_apply::take_the_window(
            &install.home,
            install.txn,
            old_running,
            Instant::now() + crate::update_apply::ELECTION_WITHIN
        ),
        Window::Mine
    );
    let held = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let mut road = install.road(limits(2_000, 20_000));
    road.me = crate::update_apply::this_process();
    let began = Instant::now();
    let applier = start(
        road,
        install.txn,
        install.applier,
        install.world(Trial::Answers),
    );
    std::thread::sleep(Duration::from_millis(1_500));
    // The election's lock, taken between two of the applier's elections (each
    // holds it for one read), and held: the next election has to wait for it.
    let election = loop {
        if let Some(held) = install_txn::try_hold(
            &crate::update_apply::owner_lock_path(&install.home, install.txn),
            Hold::Exclusive,
        )
        .unwrap()
        {
            break held;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let (ended, world) = applier.join().unwrap();
    let took = began.elapsed();
    drop(election);
    drop(held);
    install.children.end(old);
    assert_eq!(ended, Ended::OldHeldTheLock, "{:?}", world.said);
    // The deadline is 2 s; what follows it is the guard's leave in this
    // world, which waits for nothing.
    assert!(
        took < Duration::from_millis(2_400),
        "the election and the lock got only what the mark left of the 2 s: {took:?}"
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
    let mut road = install.road(limits(5_000, 5_000));
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
        flipped_at(&install, Phase::RollbackIntent { trial: None });
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
        shown: Vec::new(),
        on_say: None,
        real_ack: None,
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
