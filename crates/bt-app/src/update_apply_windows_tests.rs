//! **The Windows applier and recovery over real install folders, real signed
//! synthetic programs, real locks, a real claim, real moves and real
//! processes** (Windows); off Windows nothing here runs.
//!
//! Every file is made here, under the test's own temporary folder
//! (`%TEMP%\bt-u23-*`, removed when the test is over): an install folder
//! holding the running 0.4.6 build and a staged 0.4.7 set in its home's
//! `set\`, both written by U-20's release writer
//! (`update_prepare_windows::tests::build_that`) and signed by U-15's test root
//! (`bt_platform::trust_harness`), a rescue copy of the running `folio.exe`,
//! and the journal. Each `folio.exe` is a small program that imports nothing,
//! opens no window and stays up until it is ended: the trial, the old build
//! still running (E-7) and an applier still alive are real processes of it,
//! and every one this test starts is ended by the handle it recorded
//! ([`Children`]). No Folio is ever started: a start the applier or the
//! recovery asks for is recorded ([`Fake::opened`]).
//!
//! The entrance goes through the real `bt_platform::logon_hook::arm_in` and
//! `disarm_in` over a registry held in memory ([`Memory`]), under a key of
//! the test's own: the real `Run` key is never written.

use super::*;

use std::collections::BTreeMap;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bt_platform::logon_hook::{self, Registry};
use bt_platform::trust::FileVersion;
use bt_platform::trust_harness::{Behaviour, IDENTITY, TestCa};
use bt_winres::digest::sha256;

use crate::update_prepare_windows::tests::build_that;
use crate::update_txn::{Body, Class, Header, HeaderOutcome, Member, Outcome, PhaseKind, Receipt};

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
}

impl World for Fake {
    fn say(&mut self, line: &str) {
        self.said.push(line.to_owned());
    }

    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        self.opened.push((program.to_path_buf(), args.to_vec()));
        Ok(())
    }

    fn arm(&mut self, txn: TxnId, rescue: &Path) -> Result<Armed, String> {
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
        let txn = TxnId::parse(&args[1].to_string_lossy()).unwrap();
        let nonce = Nonce::parse(&args[2].to_string_lossy()).unwrap();
        let carried = match self.trial {
            Trial::Silent => return Ok(pid),
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
            "bt-u23-{tag}-{}-{}",
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
            recover_within: Duration::from_secs(30),
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
/// for the trial.
fn limits(old_within_ms: u64, trial_ms: u64) -> Limits {
    Limits {
        old_within: Duration::from_millis(old_within_ms),
        trial_within_ms: trial_ms,
        poll: Duration::from_millis(40),
    }
}

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
    assert!(world.opened.is_empty(), "O stayed: nothing is started");
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

/// RED (U-23) — **a move that fails leaves the disk in I1′, and the applier
/// journals `RollbackIntent` and stops**: no trial, the entrance kept for the
/// rollback, nothing started.
///
/// (b).2 W6 and the coordinator's ruling 3: "a move that fails leaves the
/// disk in I1′ and the applier journals `RollbackIntent` and stops".
///
/// MUTATION: in `at_armed`, go on to the next move after a failed one.
#[test]
fn a_failed_move_journals_rollback_intent_and_stops() {
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
    assert_eq!(ended, Ended::RollbackIntent, "{:?}", world.said);
    assert_eq!(
        install.on_disk().body.phase,
        Phase::RollbackIntent { trial: None }
    );
    assert_eq!(install.header().outcome, HeaderOutcome::RolledBack);
    assert!(
        install.holds(Place::Backup, &install.old),
        "every old file is in backup\\"
    );
    assert!(
        install.holds(Place::Set, &install.new),
        "every new file is still in set\\"
    );
    assert!(world.launched.is_empty(), "no trial");
    assert!(world.opened.is_empty(), "nothing started");
    assert!(
        install.registry.holds(install.txn),
        "the entrance stays for the rollback"
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
    assert_eq!(ended, Ended::RollbackIntent, "{:?}", world.said);
    let said = world
        .said
        .iter()
        .filter(|line| line.contains("is refused"))
        .count();
    assert_eq!(said, 1, "{:?}", world.said);
    let Phase::RollbackIntent { trial: Some(trial) } = install.on_disk().body.phase else {
        panic!("{:?}", install.on_disk().body.phase);
    };
    assert!(
        install_flip::still_running(Running {
            pid: trial.pid,
            started: trial.started
        }),
        "the trial it recorded is the process it started"
    );
    assert!(wrote(&world).ends_with("[Armed, Moving, Trial, RollbackIntent]"));
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
/// declares the rollback and moves nothing more.
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
    let before: Vec<_> = [Place::Install, Place::Backup, Place::Set]
        .iter()
        .map(|place| install.in_place(*place, &install.old))
        .collect();
    let (ended, world) = applied(
        &install,
        limits(20_000, 20_000),
        install.world(Trial::Answers),
    );
    assert_eq!(ended, Ended::RollbackIntent, "{:?}", world.said);
    assert_eq!(
        install.on_disk().body.phase,
        Phase::RollbackIntent { trial: None }
    );
    let after: Vec<_> = [Place::Install, Place::Backup, Place::Set]
        .iter()
        .map(|place| install.in_place(*place, &install.old))
        .collect();
    assert_eq!(before, after, "nothing more is moved");
    assert!(world.launched.is_empty());
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

/// RED (U-23) — **a recovery that finds `Handoff` while an applier may still
/// be alive — a process of the rescue image started before it — lets the
/// lock go and waits; once that process is gone, `Handoff` is a dead
/// applier's and is reverted.**
///
/// The owner's ruling of 2026-09-25 (3): a start during an apply waits for
/// it; the coordinator's of 2026-09-27: a dead applier's `Handoff` goes back
/// to `Prepared`.
///
/// MUTATION: `applier_alive` answers `false`.
#[test]
fn a_live_applier_is_waited_for_and_a_dead_one_reverted() {
    let Some(install) = Install::new("alive") else {
        return;
    };
    let applier = install.children.start(&install.rescue, &[]);
    let mut road = install.road(limits(20_000, 20_000));
    road.me = Running {
        pid: std::process::id(),
        started: u64::MAX,
    };
    let world = install.world(Trial::Silent);
    let recovery = start_on_a_worker(move |worker| {
        let mut world = world;
        let code = crate::update_recover::run_windows(worker, &road, Some(&handed()), &mut world);
        (code, world)
    });
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(
        install.on_disk().body.phase.kind(),
        PhaseKind::Handoff,
        "the live applier's transaction is left to it"
    );
    install.children.end(applier);
    let (code, world) = recovery.join().unwrap();
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(
        world.said.iter().any(|line| line.contains("still alive")),
        "{:?}",
        world.said
    );
    assert_eq!(
        install.on_disk().body.phase,
        Phase::Prepared {
            deferred_launches: 0
        }
    );
    assert_eq!(world.opened, vec![(install.installed.clone(), handed())]);
}

/// RED (U-23, the coordinator's requirement of 2026-09-27) — **whatever
/// phase a dead applier left the journal in, a double-click of Folio opens a
/// window**: the ordinary start hands itself to the rescue build
/// (`--update-recover --then-launch`), which recovers what it may and starts a
/// Folio with the handed command line —
///
/// | left at | recovery | opens |
/// |---|---|---|
/// | `Handoff` | → `Prepared` | the installed (old) build |
/// | `Armed`, the `Run` value there | value removed, → `Prepared` | the installed (old) build |
/// | `Moving`, some moves done | → `RollbackIntent` | the rescue copy (the old build), with `--update-failed` |
/// | `Trial`, the trial gone, no receipt | → `RollbackIntent` | the rescue copy |
/// | `Trial`, the trial alive, no receipt | waits to its deadline, → `RollbackIntent` | the rescue copy |
/// | `Trial`, a matching receipt | → `Committed` → `Retired` | the installed (new) build |
/// | `Committed` | → `Retired` | the installed (new) build |
///
/// On BASE the rescue build said "the update is not finished" for every
/// destructive phase and started nothing.
///
/// MUTATION: in `update_recover::opens`, answer `None` for a `destructive`
/// header (the rollback phases then open nothing).
#[test]
fn every_phase_left_by_a_dead_applier_still_opens_folio() {
    struct Case {
        tag: &'static str,
        ends: PhaseKind,
        opens_rescue: bool,
        installed_is_new: bool,
    }
    let cases = [
        Case {
            tag: "handoff",
            ends: PhaseKind::Prepared,
            opens_rescue: false,
            installed_is_new: false,
        },
        Case {
            tag: "armed",
            ends: PhaseKind::Prepared,
            opens_rescue: false,
            installed_is_new: false,
        },
        Case {
            tag: "moving",
            ends: PhaseKind::RollbackIntent,
            opens_rescue: true,
            installed_is_new: false,
        },
        Case {
            tag: "trial-gone",
            ends: PhaseKind::RollbackIntent,
            opens_rescue: true,
            installed_is_new: true,
        },
        Case {
            tag: "trial-alive",
            ends: PhaseKind::RollbackIntent,
            opens_rescue: true,
            installed_is_new: true,
        },
        Case {
            tag: "trial-answered",
            ends: PhaseKind::Retired,
            opens_rescue: false,
            installed_is_new: true,
        },
        Case {
            tag: "committed",
            ends: PhaseKind::Retired,
            opens_rescue: false,
            installed_is_new: true,
        },
    ];
    for case in cases {
        let Some(install) = Install::new(case.tag) else {
            return;
        };
        let all = install.inventories.forward_moves().len();
        let nonce = Nonce::new([0x55; 32]);
        match case.tag {
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
                install.move_first(all);
                let trial = install.start_trial();
                if case.tag == "trial-gone" {
                    install.children.end(trial.pid);
                }
                if case.tag == "trial-answered" {
                    install.receipt(nonce, nonce, trial.pid);
                }
                install.write(Phase::Trial {
                    nonce,
                    process: trial,
                    began_ms: now_ms(),
                });
            }
            _ => {
                install.arm();
                install.move_first(all);
                install.write(Phase::Committed);
            }
        }
        let began = Instant::now();
        let (code, world) = recovered(
            &install,
            limits(20_000, 1_500),
            install.world(Trial::Silent),
        );
        assert_eq!(code, 0, "{}: {:?}", case.tag, world.said);
        assert_eq!(
            install.on_disk().body.phase.kind(),
            case.ends,
            "{}: {:?}",
            case.tag,
            world.said
        );
        let opened = if case.opens_rescue {
            let mut words = crate::update_apply_macos::failed_words(&install.home).to_vec();
            words.extend(handed());
            (install.rescue.clone(), words)
        } else {
            (install.installed.clone(), handed())
        };
        assert_eq!(world.opened, vec![opened], "{}: {:?}", case.tag, world.said);
        let expected = if case.installed_is_new {
            &install.new
        } else {
            &install.old
        };
        assert!(
            case.tag == "moving" || install.holds(Place::Install, expected),
            "{}",
            case.tag
        );
        if case.tag == "trial-alive" {
            assert!(
                began.elapsed() >= Duration::from_millis(1_000),
                "the recovery waited while the trial lived"
            );
        }
        if matches!(case.ends, PhaseKind::Prepared | PhaseKind::Retired) {
            assert!(!install.registry.holds(install.txn), "{}", case.tag);
        }
    }
}
