//! **The macOS applier over real folders, real synthetic signed bundles, real
//! locks, a real claim, a real LaunchAgent plist in a folder of the test's own
//! and real processes** (macOS); nothing here runs elsewhere.
//!
//! Every bundle is built here: an `arm64` executable compiled from a few lines
//! of C (`/usr/bin/cc`) — with an argument it stays up for twenty seconds (a
//! trial, or a process started by hand), without one it answers each line of
//! its standard input (the running old build of E5) — an `Info.plist` with
//! `LSUIElement` (no Dock icon, no window), ad-hoc signed with `codesign -s -`.
//! Everything lives under the test's own temporary folder (`TMPDIR`), the
//! LaunchAgents folder included; no real `~/Library/LaunchAgents`, no
//! `/Applications`, no Folio. The trial's launch is a stand-in ([`Fake`]) that
//! starts the new bundle's executable directly or nothing at all, except in
//! E-12's one real run through `open`. Every process a test starts is ended by
//! the handle or the pid it recorded ([`Children`]).

use super::*;

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::update_prepare_macos::tests::fixture::{Scratch, on_macos, run, sign, version_of};
use crate::update_txn::{Body, HeaderOutcome, Outcome};

/// The synthetic bundles' identifier.
const IDENTIFIER: &str = "io.github.lulu-loopp.folio.u28-test";

/// The main executable's place in every synthetic bundle.
const EXE: &str = "Contents/MacOS/folio";

/// The synthetic executable (see the module header).
const SOURCE: &str = "#include <stdio.h>\n#include <unistd.h>\n\
int main(int argc, char **argv) {\n\
  char line[256];\n\
  (void)argv;\n\
  if (argc > 1) { sleep(20); return 0; }\n\
  while (fgets(line, sizeof line, stdin)) { printf(\"ok %s\", line); fflush(stdout); }\n\
  return 0;\n\
}\n";

/// **A signed synthetic bundle** `<parent>/<name>` of `version`.
fn bundle(parent: &Path, name: &str, version: &str) -> PathBuf {
    let bundle = parent.join(name);
    let macos = bundle.join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    let source = parent.join(format!(".{name}.c"));
    std::fs::write(&source, SOURCE).unwrap();
    run(
        "/usr/bin/cc",
        &[
            OsStr::new("-arch"),
            OsStr::new("arm64"),
            OsStr::new("-o"),
            macos.join("folio").as_os_str(),
            source.as_os_str(),
        ],
    );
    std::fs::remove_file(&source).unwrap();
    std::fs::write(
        bundle.join("Contents").join("Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\
             <key>CFBundleExecutable</key><string>folio</string>\
             <key>CFBundleIdentifier</key><string>{IDENTIFIER}</string>\
             <key>CFBundlePackageType</key><string>APPL</string>\
             <key>CFBundleShortVersionString</key><string>{version}</string>\
             <key>LSUIElement</key><true/>\
             </dict></plist>\n"
        ),
    )
    .unwrap();
    sign(&bundle);
    bundle
}

/// **Every process a test started**, ended by its own handle (or recorded pid)
/// when the test is over, pass or fail.
#[derive(Clone, Default)]
struct Children {
    handles: Arc<Mutex<Vec<Child>>>,
    pids: Arc<Mutex<Vec<u32>>>,
}

impl Children {
    /// Start `bundle`'s executable with one argument: it stays up.
    fn start(&self, bundle: &Path, word: &str) -> u32 {
        let child = bt_platform::quiet_command(bundle.join(EXE))
            .arg(word)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the synthetic executable starts");
        let pid = child.id();
        self.handles.lock().unwrap().push(child);
        pid
    }

    /// A process this test caused to start without a handle (through `open`).
    fn record(&self, pid: u32) {
        self.pids.lock().unwrap().push(pid);
    }
}

impl Drop for Children {
    fn drop(&mut self) {
        if Arc::strong_count(&self.handles) > 1 {
            return;
        }
        for child in self.handles.lock().unwrap().iter_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        for pid in self.pids.lock().unwrap().iter() {
            // A process this test started through `open`, by the pid the
            // journal recorded for it.
            let _ = bt_platform::quiet_command("/bin/kill")
                .arg(pid.to_string())
                .status();
        }
    }
}

type LaunchHook = Box<dyn FnMut(&Path, &[OsString]) -> io::Result<()> + Send>;
type ExchangeHook = Box<dyn FnMut(&Path, &Path) + Send>;

/// **The stand-in world**: its lines kept, the real exchange after an
/// optional look, and a launch that does what the test says.
#[derive(Default)]
struct Fake {
    said: Vec<String>,
    launched: Arc<Mutex<Vec<Vec<OsString>>>>,
    on_launch: Option<LaunchHook>,
    on_exchange: Option<ExchangeHook>,
    real_open: bool,
}

impl World for Fake {
    fn say(&mut self, line: &str) {
        self.said.push(line.to_owned());
    }

    fn exchange(&mut self, live: &Path, staged: &Path) -> Result<(), String> {
        if let Some(look) = &mut self.on_exchange {
            look(live, staged);
        }
        install_flip::exchange(live, staged).map_err(|failure| failure.to_string())
    }

    fn launch_trial(&mut self, bundle: &Path, args: &[OsString]) -> io::Result<()> {
        self.launched.lock().unwrap().push(args.to_vec());
        if self.real_open {
            return (Machine { log: None }).launch_trial(bundle, args);
        }
        match &mut self.on_launch {
            Some(launch) => launch(bundle, args),
            None => Ok(()),
        }
    }
}

/// A stand-in world whose launch is `launch`.
fn launching(launch: LaunchHook) -> Fake {
    Fake {
        on_launch: Some(launch),
        ..Fake::default()
    }
}

impl Fake {
    fn wrote(&self) -> &str {
        self.said
            .iter()
            .find(|line| line.contains(" wrote "))
            .map_or("", String::as_str)
    }
}

/// **One installation**, under a scratch folder: the installed bundle (1.0),
/// its home, the transaction's staged (2.0) and rescue bundles, the journal,
/// a data directory and a LaunchAgents folder.
struct Install {
    /// Removed (and anything mounted under it detached) with the installation.
    _scratch: Scratch,
    installed: PathBuf,
    home: Home,
    txn: TxnId,
    applier: Nonce,
    old: BundleIdentity,
    new: BundleIdentity,
    data: PathBuf,
    agents: PathBuf,
}

impl Install {
    fn new(tag: &str) -> Self {
        let scratch = Scratch::new(&format!("u28-{tag}"));
        let root = scratch.root.clone();
        let apps = root.join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        let installed = bundle(&apps, "Folio.app", "1.0");
        let home = Home::for_bundle(&installed).unwrap();
        let txn = TxnId::new([0x5c; 16]);
        let stage = home.stage_bundle(txn).unwrap();
        let rescue = home.rescue_bundle(txn).unwrap();
        std::fs::create_dir_all(stage.parent().unwrap()).unwrap();
        std::fs::create_dir_all(rescue.parent().unwrap()).unwrap();
        bundle(stage.parent().unwrap(), "Folio.app", "2.0");
        bundle(rescue.parent().unwrap(), "Folio.app", "1.0");
        let (old, new) = {
            let (installed, stage) = (installed.clone(), stage.clone());
            on_a_worker(move |worker| {
                (
                    crate::update_prepare_macos::identity(worker, &installed).unwrap(),
                    crate::update_prepare_macos::identity(worker, &stage).unwrap(),
                )
            })
        };
        assert_ne!(old, new);
        let data = root.join("data").join("Folio");
        std::fs::create_dir_all(&data).unwrap();
        let agents = root.join("LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        let install = Self {
            _scratch: scratch,
            installed,
            home,
            txn,
            applier: Nonce::new([0x44; 32]),
            old,
            new,
            data,
            agents,
        };
        install.write(Phase::Handoff {
            applier: install.applier,
        });
        install
    }

    /// The journal, durably at `phase`, with both identities.
    fn write(&self, phase: Phase) {
        let journal = Journal {
            txn: self.txn,
            rescue: self
                .home
                .rescue_bundle(self.txn)
                .unwrap()
                .display()
                .to_string(),
            body: Body {
                phase,
                layout: Layout::Bundle {
                    old: self.old.clone(),
                    new: self.new.clone(),
                },
            },
        };
        install_txn::durable_write(&self.home.journal(), &journal.encode()).unwrap();
    }

    fn on_disk(&self) -> Option<Journal> {
        std::fs::read(self.home.journal())
            .ok()
            .map(|bytes| Journal::parse(&bytes).unwrap())
    }

    fn road(&self, limits: Limits) -> Road {
        Road {
            home: self.home.clone(),
            txn: self.txn,
            nonce: self.applier,
            data: self.data.clone(),
            agents: Some(self.agents.clone()),
            limits,
        }
    }

    fn stage(&self) -> PathBuf {
        self.home.stage_bundle(self.txn).unwrap()
    }

    fn plist(&self) -> PathBuf {
        self.agents.join(launch_agent::file_name(self.txn.bytes()))
    }

    /// The receipt the trial writes, as `update_trial` writes it: the real
    /// encoder, created durably and never over an existing one.
    fn receipt(&self, nonce: Nonce, carried: Nonce, pid: u32) {
        let receipt = Receipt {
            txn: self.txn,
            nonce: carried,
            pid,
            version: "2.0".to_owned(),
        };
        install_txn::durable_create(&self.home.receipt_path(self.txn, &nonce), &receipt.encode())
            .unwrap();
    }
}

/// Short limits for a test.
fn limits(old_within_ms: u64, trial_within_ms: u64) -> Limits {
    Limits {
        old_within: Duration::from_millis(old_within_ms),
        trial_within_ms,
        poll: Duration::from_millis(40),
    }
}

/// The trial's nonce, from the arguments the launch was given.
fn trial_nonce(args: &[OsString]) -> Nonce {
    assert_eq!(args[0], OsString::from(cli::UPDATE_TRIAL_FLAG));
    Nonce::parse(&args[2].to_string_lossy()).unwrap()
}

/// Run `body` on a worker the thread door started, and wait for it.
fn on_a_worker<T: Send + 'static>(body: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
    match start_on_a_worker(body).join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

fn start_on_a_worker<T: Send + 'static>(
    body: impl FnOnce(&WorkerCtx) -> T + Send + 'static,
) -> JoinHandle<T> {
    bt_platform::spawn_at_priority(
        "bt-u28-test",
        bt_platform::ThreadPriority::BelowNormal,
        body,
    )
    .expect("the thread door starts a thread")
}

/// The applier on its own worker, started now.
fn start(road: Road, mut world: Fake) -> JoinHandle<(Ended, Fake)> {
    start_on_a_worker(move |worker| {
        let ended = apply(worker, &road, &mut world);
        (ended, world)
    })
}

fn applied(road: Road, world: Fake) -> (Ended, Fake) {
    match start(road, world).join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// A launch that starts the new bundle's executable and writes the receipt
/// for the trial's nonce, carrying `carried` (the trial's own when `None`).
fn a_healthy_trial(install: &Install, children: &Children, carried: Option<Nonce>) -> LaunchHook {
    let home = install.home.clone();
    let txn = install.txn;
    let children = children.clone();
    Box::new(move |bundle, args| {
        let nonce = trial_nonce(args);
        let pid = children.start(bundle, "trial");
        let receipt = Receipt {
            txn,
            nonce: carried.unwrap_or(nonce),
            pid,
            version: "2.0".to_owned(),
        };
        install_txn::durable_create(&home.receipt_path(txn, &nonce), &receipt.encode())
            .expect("the receipt");
        Ok(())
    })
}

/// Wait until the journal on disk satisfies `until`, or panic after 20 s.
fn journal_reaches(install: &Install, until: impl Fn(&Journal) -> bool) -> Journal {
    let give_up = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(journal) = install.on_disk().filter(|journal| until(journal)) {
            return journal;
        }
        assert!(Instant::now() < give_up, "the journal never got there");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// RED (U-28, E5 / E-12) — **the exchange of the installed bundle with the
/// staged one leaves no instant at which the launch path holds no bundle, and
/// a process already running from the old bundle keeps working after it.**
///
/// §C.4: "one `renamex_np(…, RENAME_SWAP)` … there is no interrupted state";
/// §H's running-bundle swap. A watcher asks for the installed executable and
/// its `Info.plist` as fast as it can for the whole exchange and never misses
/// either. The old build's process answers a line before and after. What the
/// kernel then names as that process's image is printed (E-12's record): it is
/// the old file, now at `stage/`, never the new one at the launch path.
///
/// MUTATION: in `install_txn`'s macOS arm, perform `Replace::Swap` as three
/// renames through a name beside `a` (a → aside, b → a, aside → b).
#[test]
fn swap_leaves_no_instant_without_a_bundle() {
    if !on_macos() {
        return;
    }
    let install = Install::new("swap");
    let (installed, stage) = (install.installed.clone(), install.stage());
    let mut old = bt_platform::quiet_command(installed.join(EXE))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut answers = BufReader::new(old.stdout.take().unwrap());
    let mut ask = |old: &mut Child, word: &str| {
        writeln!(old.stdin.as_mut().unwrap(), "{word}").unwrap();
        let mut line = String::new();
        answers.read_line(&mut line).unwrap();
        line
    };
    assert_eq!(ask(&mut old, "before"), "ok before\n");
    let running = install_flip::running_from(&installed.join(EXE)).unwrap();
    assert!(running.iter().any(|process| process.pid == old.id()));

    let stop = Arc::new(AtomicBool::new(false));
    let (looks, misses) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let watcher = {
        let (stop, looks, misses, installed) = (
            stop.clone(),
            looks.clone(),
            misses.clone(),
            installed.clone(),
        );
        std::thread::spawn(move || {
            let (exe, plist) = (installed.join(EXE), installed.join("Contents/Info.plist"));
            while !stop.load(Ordering::Relaxed) {
                if std::fs::symlink_metadata(&exe).is_err()
                    || std::fs::symlink_metadata(&plist).is_err()
                {
                    misses.fetch_add(1, Ordering::Relaxed);
                }
                looks.fetch_add(1, Ordering::Relaxed);
            }
        })
    };
    std::thread::sleep(Duration::from_millis(50));
    install_flip::exchange(&installed, &stage).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    stop.store(true, Ordering::Relaxed);
    watcher.join().unwrap();
    assert!(looks.load(Ordering::Relaxed) > 100);
    assert_eq!(
        misses.load(Ordering::Relaxed),
        0,
        "the launch path held no bundle at some instant"
    );
    assert_eq!(version_of(&installed), "2.0");
    assert_eq!(version_of(&stage), "1.0");
    assert_eq!(
        ask(&mut old, "after"),
        "ok after\n",
        "the old image still runs"
    );
    let at_launch_path = install_flip::running_from(&installed.join(EXE)).unwrap();
    let at_stage = install_flip::running_from(&stage.join(EXE)).unwrap();
    println!(
        "E-12: after the exchange, the old process {} is listed at the launch path: {}; at stage/: {}",
        old.id(),
        at_launch_path.iter().any(|p| p.pid == old.id()),
        at_stage.iter().any(|p| p.pid == old.id())
    );
    assert!(!at_launch_path.iter().any(|process| process.pid == old.id()));
    assert!(at_stage.iter().any(|process| process.pid == old.id()));
    let _ = old.kill();
    let _ = old.wait();
}

/// RED (U-28) — **the applier waits for O: for the transaction lock O holds
/// until it exits, then for the data directory's claim, which it takes and
/// lets go at once; a claim still held at the end of the wait abandons the
/// transaction with nothing moved, and a lock never had writes nothing.**
///
/// §C.4: "P first waits for O to be gone … polls
/// `claim_data_directory` until it succeeds, then immediately releases it …
/// If 60 s pass, P journals `Failed`, deletes staging, and exits without
/// touching anything."
///
/// MUTATION: in `wait_for_the_claim`, answer `Ok(())` for a held claim.
#[test]
fn p_waits_for_o_and_releases_the_claim() {
    if !on_macos() {
        return;
    }
    // O alive: its lock, then its claim, let go one after the other.
    let install = Install::new("wait");
    let lock = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let claim = bt_platform::instance::claim_data_directory(&install.data).unwrap();
    let released = Arc::new(Mutex::new(None::<Instant>));
    let children = Children::default();
    let mut world = launching(a_healthy_trial(&install, &children, None));
    let data = install.data.clone();
    let (released_seen, swapped_at) = (released.clone(), Arc::new(Mutex::new(None)));
    let swapped = swapped_at.clone();
    world.on_exchange = Some(Box::new(move |_, _| {
        *swapped.lock().unwrap() = Some(Instant::now());
        assert!(
            bt_platform::instance::claim_data_directory(&data).is_some(),
            "the applier let the claim go"
        );
        assert!(released_seen.lock().unwrap().is_some());
    }));
    let applier = start(install.road(limits(10_000, 10_000)), world);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        install.on_disk().unwrap().body.phase.kind(),
        PhaseKind::Handoff,
        "nothing is written while O holds the lock"
    );
    drop(lock);
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !install.plist().exists(),
        "nothing is armed while O holds its claim"
    );
    *released.lock().unwrap() = Some(Instant::now());
    drop(claim);
    let (ended, _) = applier.join().unwrap();
    assert_eq!(ended, Ended::Committed);
    assert!(swapped_at.lock().unwrap().is_some());
    drop(children);

    // O never lets go of its claim: abandoned, cleared, nothing moved.
    let install = Install::new("stayed");
    let claim = bt_platform::instance::claim_data_directory(&install.data).unwrap();
    let (ended, world) = applied(install.road(limits(1_200, 10_000)), Fake::default());
    drop(claim);
    assert_eq!(ended, Ended::Abandoned, "{:?}", world.said);
    assert!(
        world
            .said
            .iter()
            .any(|line| line.contains("did not let go"))
    );
    assert!(install.on_disk().is_none());
    assert!(!install.home.transaction(install.txn).exists());
    assert!(!install.plist().exists());
    assert_eq!(version_of(&install.installed), "1.0");

    // O never lets go of its lock: nothing is written at all.
    let install = Install::new("held");
    let before = std::fs::read(install.home.journal()).unwrap();
    let lock = install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
        .unwrap()
        .unwrap();
    let (ended, _) = applied(install.road(limits(800, 10_000)), Fake::default());
    drop(lock);
    assert_eq!(ended, Ended::OldHeldTheLock);
    assert_eq!(std::fs::read(install.home.journal()).unwrap(), before);
    assert_eq!(version_of(&install.stage()), "2.0");
}

/// RED (U-28, M4) — **`Armed` is on the disk, with its plist, before
/// anything is exchanged; the exchange happens under `Moving`.**
///
/// (b).2: "Every destructive step is preceded by a durable journal state that
/// names it"; M4: the LaunchAgent plist durable, then the admission and the
/// process check, then `Exchanging`. A running copy holds the admission shared
/// while the test reads the disk: the journal says `Armed` and the plist
/// names the rescue executable and the home, and no bundle has moved. At the
/// exchange itself the journal says `Moving`.
///
/// MUTATION: in `at_handoff`, go on to `at_armed` before recording
/// `Armed` (record it after the call returns).
#[test]
fn armed_is_durable_before_exchanging() {
    if !on_macos() {
        return;
    }
    let install = Install::new("armed");
    std::fs::write(install.home.admission(), b"").unwrap();
    let copy = install_txn::try_hold(&install.home.admission(), Hold::Shared)
        .unwrap()
        .unwrap();
    let children = Children::default();
    let mut world = launching(a_healthy_trial(&install, &children, None));
    let journal = install.home.journal();
    let at_exchange = Arc::new(Mutex::new(None));
    let seen = at_exchange.clone();
    world.on_exchange = Some(Box::new(move |_, _| {
        let phase = Journal::parse(&std::fs::read(&journal).unwrap())
            .unwrap()
            .body
            .phase
            .kind();
        *seen.lock().unwrap() = Some(phase);
    }));
    let applier = start(install.road(limits(10_000, 10_000)), world);
    journal_reaches(&install, |journal| journal.body.phase == Phase::Armed);
    let plist = std::fs::read_to_string(install.plist()).expect("the plist is there at Armed");
    let rescue = install.home.rescue_executable(install.txn).unwrap();
    assert!(plist.contains(&*rescue.to_string_lossy()));
    assert!(plist.contains(&*install.home.root().to_string_lossy()));
    assert!(plist.contains("--update-recover"));
    assert_eq!(version_of(&install.installed), "1.0", "nothing moved yet");
    drop(copy);
    let (ended, world) = applier.join().unwrap();
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert_eq!(*at_exchange.lock().unwrap(), Some(PhaseKind::Moving));
    assert!(
        world
            .wrote()
            .contains("[Armed, Moving, Trial, Committed, Retired]"),
        "{}",
        world.wrote()
    );
}

/// RED (U-28) — **a receipt at the trial's name that carries another nonce
/// is refused, and the deadline then declares the rollback**: nothing is
/// committed and the old bundle stays in `stage/`.
///
/// (b).2: "`Committed` … only on a receipt whose `txn` and `nonce` match."
///
/// MUTATION: in `trial`, give the receipt read the trial's own nonce before it
/// is offered to the journal.
#[test]
fn a_receipt_for_another_nonce_is_refused() {
    if !on_macos() {
        return;
    }
    let install = Install::new("nonce");
    let children = Children::default();
    let world = launching(a_healthy_trial(
        &install,
        &children,
        Some(Nonce::new([0x99; 32])),
    ));
    let (ended, world) = applied(install.road(limits(5_000, 1_500)), world);
    assert_eq!(ended, Ended::RollbackIntent, "{:?}", world.said);
    assert!(world.said.iter().any(|line| line.contains("is refused")));
    let journal = install.on_disk().unwrap();
    let Phase::RollbackIntent { trial: Some(trial) } = journal.body.phase else {
        panic!("{:?}", journal.body.phase);
    };
    assert!(
        children
            .handles
            .lock()
            .unwrap()
            .iter()
            .any(|child| child.id() == trial.pid)
    );
    assert_eq!(
        version_of(&install.stage()),
        "1.0",
        "the old bundle is kept"
    );
    assert_eq!(version_of(&install.installed), "2.0", "U-29 swaps back");
    assert!(install.plist().exists());
}

/// RED (U-28) — **`Committed` is written only while the journal says
/// `Trial`, on a receipt that matches**: a receipt that arrives before any
/// process of the new build is listed still waits for `Trial` (with the
/// receipt's own pid) to be durable first.
///
/// "Who may write what": "`Committed` is written only by the lock holder,
/// only while the journal says `Trial`, and only on a receipt whose `txn` and
/// `nonce` match." The launch here starts nothing and writes the receipt with
/// this test process's pid.
///
/// MUTATION: in `trial`, never record `TrialBegan` (filter every process
/// found out): the receipt then has no `Trial` to be accepted in.
#[test]
fn committed_is_written_only_on_a_matching_receipt_while_trial() {
    if !on_macos() {
        return;
    }
    let install = Install::new("commit");
    let (home, txn) = (install.home.clone(), install.txn);
    let world = launching(Box::new(move |_, args| {
        let nonce = trial_nonce(args);
        let receipt = Receipt {
            txn,
            nonce,
            pid: std::process::id(),
            version: "2.0".to_owned(),
        };
        install_txn::durable_create(&home.receipt_path(txn, &nonce), &receipt.encode()).unwrap();
        Ok(())
    }));
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), world);
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(
        world
            .wrote()
            .contains("[Armed, Moving, Trial, Committed, Retired]"),
        "{}",
        world.wrote()
    );
    let journal = install
        .on_disk()
        .expect("the journal is kept for the trial's watch");
    assert_eq!(
        journal.body.phase,
        Phase::Retired {
            outcome: Outcome::Committed
        }
    );
    assert_eq!(journal.header().outcome, HeaderOutcome::Committed);
}

/// RED (U-28, M8) — **the old bundle leaves `stage/` only once `Committed`
/// is durable**, then the plist and `H/<txn>` go; the journal stays at
/// `Retired{Committed}` for the trial's watch, and the lock is free.
///
/// M8: "`Committed` durable, then remove the plist, delete `stage/Folio.app`
/// (the old bundle), then the rescue clone." A watcher reads the journal at
/// the first instant it finds `stage/` empty.
///
/// MUTATION: in `trial`, remove the old bundle from `stage/` before recording
/// `ReceiptAccepted`.
#[test]
fn the_old_bundle_is_removed_only_after_committed() {
    if !on_macos() {
        return;
    }
    let install = Install::new("removed");
    let children = Children::default();
    let world = launching(a_healthy_trial(&install, &children, None));
    let (stage, journal) = (install.stage(), install.home.journal());
    let watcher = std::thread::spawn(move || {
        let give_up = Instant::now() + Duration::from_secs(30);
        while Instant::now() < give_up {
            let bytes = std::fs::read(&journal).ok();
            if std::fs::symlink_metadata(&stage).is_err() {
                return bytes.map(|bytes| Journal::parse(&bytes).unwrap().body.phase.kind());
            }
        }
        None
    });
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), world);
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    let seen = watcher.join().unwrap();
    assert!(
        matches!(seen, Some(PhaseKind::Committed | PhaseKind::Retired)),
        "the journal said {seen:?} when the old bundle went"
    );
    assert_eq!(version_of(&install.installed), "2.0");
    assert!(!install.plist().exists());
    assert!(!install.home.transaction(install.txn).exists());
    assert_eq!(
        install.on_disk().unwrap().body.phase,
        Phase::Retired {
            outcome: Outcome::Committed
        }
    );
    assert!(
        install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
            .unwrap()
            .is_some()
    );
}

/// RED (U-28, M7) — **no pid and no receipt by the deadline, or a trial that
/// ends without one, journals `RollbackIntent` and stops**: nothing is
/// swapped back (U-29's), the plist stays armed, and the lock is let go.
///
/// W7/M7: "Then → `RollbackIntent`." The trial of the second half is started
/// and ended by the test before it can answer.
///
/// MUTATION: in `trial`, at the deadline, answer `Ended::RollbackIntent`
/// without recording `RollbackDeclared`.
#[test]
fn the_deadline_journals_rollback_intent_and_stops() {
    if !on_macos() {
        return;
    }
    let install = Install::new("deadline");
    let (ended, world) = applied(install.road(limits(5_000, 1_200)), Fake::default());
    assert_eq!(ended, Ended::RollbackIntent, "{:?}", world.said);
    assert_eq!(
        install.on_disk().unwrap().body.phase,
        Phase::RollbackIntent { trial: None }
    );
    assert_eq!(
        install.on_disk().unwrap().header().outcome,
        HeaderOutcome::RolledBack
    );
    assert_eq!(version_of(&install.installed), "2.0");
    assert_eq!(version_of(&install.stage()), "1.0");
    assert!(install.plist().exists());
    assert!(
        install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
            .unwrap()
            .is_some()
    );

    let install = Install::new("died");
    let children = Children::default();
    let started = children.clone();
    let world = launching(Box::new(move |bundle, _| {
        started.start(bundle, "trial");
        Ok(())
    }));
    let killer = {
        let children = children.clone();
        let install_journal = install.home.journal();
        std::thread::spawn(move || {
            let give_up = Instant::now() + Duration::from_secs(20);
            while Instant::now() < give_up {
                let trial = std::fs::read(&install_journal)
                    .ok()
                    .and_then(|bytes| Journal::parse(&bytes).ok())
                    .is_some_and(|journal| journal.body.phase.kind() == PhaseKind::Trial);
                if trial {
                    for child in children.handles.lock().unwrap().iter_mut() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    };
    let (ended, world) = applied(install.road(limits(5_000, 10_000)), world);
    killer.join().unwrap();
    assert_eq!(ended, Ended::RollbackIntent, "{:?}", world.said);
    assert!(matches!(
        install.on_disk().unwrap().body.phase,
        Phase::RollbackIntent { trial: Some(_) }
    ));
    assert!(
        world
            .said
            .iter()
            .any(|line| line.contains("ended without a receipt"))
    );
}

/// RED (U-28) — **the receipt completes the transaction whichever process
/// the applier recorded as the trial**: here the process list first finds a
/// process of the new version that a person started by hand, and the receipt
/// then arrives carrying another pid — the trial's nonce and the transaction
/// are what the journal asks of it, never the pid.
///
/// §C.5: "Any Folio of `to_version` starting from this installation
/// satisfies it, … so a race with a manual launch **completes** the
/// transaction instead of failing it." Since F-14 a start without the trial's
/// nonce hands itself to the rescue build and writes no receipt (U-12); what
/// remains of the sentence is this: a hand-started process of the new build
/// in the list neither fails the trial nor stands in the way of its receipt.
///
/// MUTATION: in `trial`, accept a receipt only when its pid is the recorded
/// trial process's.
#[test]
fn a_manual_launch_of_the_new_version_completes_the_transaction() {
    if !on_macos() {
        return;
    }
    let install = Install::new("manual");
    let children = Children::default();
    let world = Fake::default();
    let launched = world.launched.clone();
    let applier = start(install.road(limits(5_000, 10_000)), world);
    let give_up = Instant::now() + Duration::from_secs(20);
    let args = loop {
        if let Some(args) = launched.lock().unwrap().first().cloned() {
            break args;
        }
        assert!(Instant::now() < give_up, "no launch");
        std::thread::sleep(Duration::from_millis(10));
    };
    let by_hand = children.start(&install.installed, "by-hand");
    let journal = journal_reaches(&install, |journal| {
        journal.body.phase.kind() == PhaseKind::Trial
    });
    let Phase::Trial { process, .. } = journal.body.phase else {
        unreachable!()
    };
    assert_eq!(
        process.pid, by_hand,
        "the list found the hand-started process"
    );
    install.receipt(trial_nonce(&args), trial_nonce(&args), std::process::id());
    let (ended, world) = applier.join().unwrap();
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
}

/// RED (U-28, M4–M6) — **an applier started again over its own transaction
/// goes on from what is live**: at `Armed` it admits, exchanges and commits;
/// at `Moving` with the old bundle live it removes the plist and reverts to
/// `Prepared` (M5); at `Moving` with the new bundle live it declares the
/// rollback without swapping (M6). A hand-over to another applier is refused
/// and nothing is touched.
///
/// M5/M6: "recovery decides by reading the installed bundle's version, not by
/// trusting the phase" (§C.4).
///
/// MUTATION: in `at_moving_again`, revert whatever is live.
#[test]
#[expect(
    non_snake_case,
    reason = "the M-rows are named as the design note names them"
)]
fn reentry_at_M4_M5_M6_continues_from_the_live_identity() {
    if !on_macos() {
        return;
    }
    // M4.
    let install = Install::new("m4");
    let rescue = install.home.rescue_executable(install.txn).unwrap();
    let armed = launch_agent::arm(
        &install.agents,
        install.txn.bytes(),
        &rescue,
        install.home.root(),
    )
    .unwrap();
    let journal = install.on_disk().unwrap();
    let armed_journal = journal.advance(&Event::Armed(armed)).unwrap();
    install_txn::durable_write(&install.home.journal(), &armed_journal.encode()).unwrap();
    let children = Children::default();
    let world = launching(a_healthy_trial(&install, &children, None));
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), world);
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(
        world
            .wrote()
            .contains("[Moving, Trial, Committed, Retired]")
    );
    assert_eq!(version_of(&install.installed), "2.0");

    // M5.
    let install = Install::new("m5");
    install.write(Phase::Moving);
    let rescue = install.home.rescue_executable(install.txn).unwrap();
    let _plist = launch_agent::arm(
        &install.agents,
        install.txn.bytes(),
        &rescue,
        install.home.root(),
    )
    .unwrap();
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), Fake::default());
    assert_eq!(ended, Ended::Reverted, "{:?}", world.said);
    assert_eq!(
        install.on_disk().unwrap().body.phase,
        Phase::Prepared {
            deferred_launches: 0
        }
    );
    assert!(!install.plist().exists());
    assert_eq!(version_of(&install.installed), "1.0");
    assert_eq!(version_of(&install.stage()), "2.0");

    // M6.
    let install = Install::new("m6");
    install.write(Phase::Moving);
    install_flip::exchange(&install.installed, &install.stage()).unwrap();
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), Fake::default());
    assert_eq!(ended, Ended::RollbackIntent, "{:?}", world.said);
    assert_eq!(
        install.on_disk().unwrap().body.phase,
        Phase::RollbackIntent { trial: None }
    );
    assert_eq!(version_of(&install.installed), "2.0", "no swap back here");

    // Another applier's hand-over.
    let install = Install::new("other");
    let before = std::fs::read(install.home.journal()).unwrap();
    let mut road = install.road(limits(5_000, 5_000));
    road.nonce = Nonce::new([0x45; 32]);
    let (ended, _) = applied(road, Fake::default());
    assert!(matches!(ended, Ended::Refused(_)), "{ended:?}");
    assert_eq!(std::fs::read(install.home.journal()).unwrap(), before);
    assert_eq!(version_of(&install.installed), "1.0");
}

/// RED (U-28, E-12) — **the trial is started through LaunchServices (`open
/// -n -a`), found by its image in the process list, and its receipt
/// commits.**
///
/// §C.5: "P launches `open -n -a <parent>/<Name>.app --args --update-health
/// <journal>`" — the trial word is `--update-trial <txn> <nonce>` (U-12's
/// frozen v1 flag). `open` reports no pid, so the one the journal records is
/// the one the list found: a process of the installed executable that
/// started after the launch. The synthetic bundle is `LSUIElement`: no Dock
/// icon, no window. The test writes the receipt the trial would, and ends the
/// process by the pid the journal recorded.
///
/// MUTATION: in `trial`, take no pid from the process list (only a
/// receipt's).
#[test]
fn the_trial_is_launched_through_launch_services_and_found_by_its_image() {
    if !on_macos() {
        return;
    }
    let install = Install::new("open");
    let children = Children::default();
    let world = Fake {
        real_open: true,
        ..Fake::default()
    };
    let launched = world.launched.clone();
    let applier = start(install.road(limits(5_000, 20_000)), world);
    let journal = journal_reaches(&install, |journal| {
        journal.body.phase.kind() == PhaseKind::Trial
    });
    let Phase::Trial { process, nonce, .. } = journal.body.phase else {
        unreachable!()
    };
    children.record(process.pid);
    let listed = install_flip::running_from(&install.installed.join(EXE)).unwrap();
    assert!(listed.iter().any(|running| running.pid == process.pid));
    assert_eq!(trial_nonce(&launched.lock().unwrap()[0]), nonce);
    println!(
        "E-12: `open -n -a` started the trial as pid {}",
        process.pid
    );
    install.receipt(nonce, nonce, process.pid);
    let (ended, world) = applier.join().unwrap();
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
}

/// RED (U-28, U-27's finding 3) — **an ordinary start that retires a
/// finished macOS transaction removes its LaunchAgent**: the plist the
/// applier armed is gone after the retirement's `RemoveEntrance`, a second
/// retirement finds nothing and succeeds, and a start with no home folder to
/// look in has nothing to remove.
///
/// Until U-28 no macOS transaction reached `Armed`, and
/// `update_startup::Machine::retire_entrance` answered only Windows.
///
/// MUTATION: `retire_entrance_in` answers `Ok(())` without calling
/// `launch_agent::disarm`.
#[test]
fn an_ordinary_start_retires_the_macos_entrance() {
    if !on_macos() {
        return;
    }
    let scratch = Scratch::new("u28-retire");
    let agents = scratch.root.join("LaunchAgents");
    std::fs::create_dir_all(&agents).unwrap();
    let txn = TxnId::new([0x5d; 16]);
    let _armed = launch_agent::arm(
        &agents,
        txn.bytes(),
        &scratch.root.join("rescue").join(EXE),
        &scratch.root.join(".Folio.app.folio-update"),
    )
    .unwrap();
    let plist = agents.join(launch_agent::file_name(txn.bytes()));
    assert!(plist.exists());
    retire_entrance_in(Some(&agents), txn).unwrap();
    assert!(!plist.exists());
    retire_entrance_in(Some(&agents), txn).unwrap();
    retire_entrance_in(None, txn).unwrap();
}
