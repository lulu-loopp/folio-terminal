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
}

type MoveHook = Box<dyn FnMut(&Move) + Send>;
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
    /// **Panic in the entrance's write** (U-34): a fault inside the road.
    panic_at_arm: bool,
    /// **A program that will not start** (U-34): its start is recorded and
    /// refused.
    refuse_start_of: Option<PathBuf>,
}

impl World for Fake {
    fn say(&mut self, line: &str) {
        if line.contains("could not be recorded") {
            // The scanner lets go: the rollback's own records go through.
            self.held = None;
        }
        self.said.push(line.to_owned());
    }

    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        self.opened.push((program.to_path_buf(), args.to_vec()));
        if self.refuse_start_of.as_deref() == Some(program) {
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
            Trial::Answers => nonce,
            Trial::AnswersWithAnotherNonce => Nonce::new([0x99; 32]),
        };
        let receipt = Receipt {
            txn,
            nonce: carried,
            pid,
            version: "0.4.7".to_owned(),
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
            panic_at_arm: false,
            refuse_start_of: None,
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
            outcome: Outcome::Committed
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
            outcome: Outcome::Committed
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
            outcome: Outcome::RolledBack
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
                outcome: Outcome::RolledBack
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
            outcome: Outcome::Committed
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
    install.write(Phase::RolledBack);
    let (code, world) = recovered_at_logon(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Silent),
    );
    assert_eq!(code, 0, "{:?}", world.said);
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Retired {
            outcome: Outcome::RolledBack
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
/// MUTATION: in `opens_after`, answer `Opens::Rescue` for every `destructive`
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
                install.write(Phase::RolledBack);
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
            assert_eq!(phase, Phase::Retired { outcome }, "{tag}");
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
        crate::update_apply::take_the_window(&install.home, install.txn, applier),
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
/// MUTATION: in `update_apply_windows::opens_after`, put `Ended::Failed(_)`
/// back in the applier's arm that owes nothing.
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

/// RED (U-34) — **the recovery run at logon is the one exit that may start
/// nothing: when it did nothing a person is owed a window for**. Over a
/// `Committed` it retires, nobody is waiting and nothing is started; over an
/// `Armed` it reverts, and the old build opens — the Restart before it never
/// got a window (W11).
///
/// MUTATION: in `update_recover::run_windows`, never tell the guard that
/// nobody is waiting.
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
            outcome: Outcome::Committed
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
    while crate::update_apply::window_owner(&install.home, install.txn).is_none() {
        assert!(Instant::now() < give_up, "P never took the window");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        crate::update_apply::window_owner(&install.home, install.txn),
        Some(crate::update_apply::this_process()),
        "step 1: P decided — the mark is P's"
    );
    let mut starts = Starts::default();
    let left =
        Leaving::over(&install.home, install.txn).leave(old, &install.installed, &mut starts);
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
    let left =
        Leaving::over(&install.home, install.txn).leave(old, &install.installed, &mut starts);
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
