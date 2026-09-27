//! **The macOS applier over real folders, real synthetic signed bundles, real
//! locks, a real claim, a real LaunchAgent plist in a folder of the test's own
//! and real processes** (macOS); nothing here runs elsewhere.
//!
//! Every bundle is built here: an `arm64` executable compiled from a few lines
//! of C (`/usr/bin/cc`) — with an argument it stays up for twenty seconds (a
//! trial, or a process started by hand), and quits at `SIGTERM` as any process
//! does; with `stubborn <file>` it writes `term` into that file at each
//! `SIGTERM` and stays (a trial that will not quit, U-29); without one it
//! answers each line of its standard input (the running old build of E5) — an
//! `Info.plist` with `LSUIElement` (no Dock icon, no window), ad-hoc signed
//! with `codesign -s -`. Everything lives under the test's own temporary
//! folder (`TMPDIR`), the LaunchAgents folder included; no real
//! `~/Library/LaunchAgents`, no `/Applications`, no Folio. The trial's launch
//! and the relaunch after a rollback are stand-ins ([`Fake`]) that start the
//! new bundle's executable directly, record the words, or nothing at all,
//! except in E-12's one real run through `open`. Every process a test starts
//! is reaped by a thread of the test's and ended by the pid it recorded, while
//! it is not yet reaped ([`Children`]). The check of a restored bundle's
//! signature is a stand-in too: an ad-hoc bundle satisfies no Developer ID
//! requirement (U-16).

use super::*;

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::update_prepare_macos::tests::fixture::{Scratch, on_macos, run, sign, version_of};
use crate::update_txn::{
    Body, Class, Header, HeaderOutcome, Outcome, STUCK_ATTEMPT_LIMIT, StartAction, TrialSight,
};

/// The synthetic bundles' identifier.
const IDENTIFIER: &str = "io.github.lulu-loopp.folio.u28-test";

/// The main executable's place in every synthetic bundle.
const EXE: &str = "Contents/MacOS/folio";

/// The synthetic executable (see the module header).
const SOURCE: &str = "#include <fcntl.h>\n#include <signal.h>\n#include <stdio.h>\n\
#include <string.h>\n#include <unistd.h>\n\
static const char *mark;\n\
static void on_term(int signal_number) {\n\
  int fd = open(mark, O_WRONLY | O_CREAT | O_TRUNC, 0644);\n\
  (void)signal_number;\n\
  if (fd >= 0) { if (write(fd, \"term\", 4) < 0) {} close(fd); }\n\
}\n\
int main(int argc, char **argv) {\n\
  char line[256];\n\
  if (argc > 2 && strcmp(argv[1], \"stubborn\") == 0) {\n\
    mark = argv[2]; signal(SIGTERM, on_term); for (;;) pause();\n\
  }\n\
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

/// **Every process a test started**: each reaped by a thread of its own the
/// moment it ends (so a process that has ended leaves the process list, as a
/// trial LaunchServices started does), its exit status kept, and every one
/// still unreaped ended by its recorded pid when the test is over, pass or
/// fail.
#[derive(Clone, Default)]
struct Children {
    started: Arc<Mutex<Vec<u32>>>,
    ended: Arc<Mutex<Vec<(u32, ExitStatus)>>>,
    pids: Arc<Mutex<Vec<u32>>>,
}

impl Children {
    /// Start `bundle`'s executable with one argument: it stays up.
    fn start(&self, bundle: &Path, word: &str) -> u32 {
        self.start_with(bundle, &[OsStr::new(word)])
    }

    /// Start `bundle`'s executable with `words`.
    fn start_with(&self, bundle: &Path, words: &[&OsStr]) -> u32 {
        let mut child: Child = bt_platform::quiet_command(bundle.join(EXE))
            .args(words)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the synthetic executable starts");
        let pid = child.id();
        self.started.lock().unwrap().push(pid);
        let ended = self.ended.clone();
        std::thread::spawn(move || {
            if let Ok(status) = child.wait() {
                ended.lock().unwrap().push((pid, status));
            }
        });
        pid
    }

    /// How `pid` ended, once it has.
    fn status(&self, pid: u32) -> Option<ExitStatus> {
        self.ended
            .lock()
            .unwrap()
            .iter()
            .find(|(ended, _)| *ended == pid)
            .map(|(_, status)| *status)
    }

    /// How `pid` ended, waiting up to 20 s for it.
    fn ended(&self, pid: u32) -> Option<ExitStatus> {
        let give_up = Instant::now() + Duration::from_secs(20);
        while Instant::now() < give_up {
            if let Some(status) = self.status(pid) {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    /// End `pid`, one this test started and has not reaped yet.
    fn end(&self, pid: u32) {
        if self.status(pid).is_none() {
            let _ = bt_platform::quiet_command("/bin/kill")
                .arg("-9")
                .arg(pid.to_string())
                .status();
        }
    }

    /// A process this test caused to start without a handle (through `open`).
    fn record(&self, pid: u32) {
        self.pids.lock().unwrap().push(pid);
    }
}

impl Drop for Children {
    fn drop(&mut self) {
        if Arc::strong_count(&self.started) > 1 {
            return;
        }
        for pid in self.started.lock().unwrap().clone() {
            self.end(pid);
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
/// optional look (or a refusal, from the `refuse_from`-th on), a check of the
/// restored bundle that passes unless the test says why not, a launch that
/// does what the test says, and relaunches recorded.
struct Fake {
    said: Vec<String>,
    launched: Arc<Mutex<Vec<Vec<OsString>>>>,
    relaunched: Vec<(PathBuf, Vec<OsString>)>,
    on_launch: Option<LaunchHook>,
    on_exchange: Option<ExchangeHook>,
    real_open: bool,
    /// Exchanges performed or refused so far.
    exchanges: usize,
    /// The first exchange (counted from 0) that is refused.
    refuse_from: usize,
    unverified: Option<String>,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            said: Vec::new(),
            launched: Arc::default(),
            relaunched: Vec::new(),
            on_launch: None,
            on_exchange: None,
            real_open: false,
            exchanges: 0,
            refuse_from: usize::MAX,
            unverified: None,
        }
    }
}

impl Hands for Fake {
    fn say(&mut self, line: &str) {
        self.said.push(line.to_owned());
    }

    fn exchange(&mut self, live: &Path, staged: &Path) -> Result<(), String> {
        let this = self.exchanges;
        self.exchanges += 1;
        if this >= self.refuse_from {
            return Err("the exchange is refused (test)".to_owned());
        }
        if let Some(look) = &mut self.on_exchange {
            look(live, staged);
        }
        install_flip::exchange(live, staged).map_err(|failure| failure.to_string())
    }

    fn verify_restored(&mut self, _worker: &WorkerCtx, _bundle: &Path) -> Result<(), String> {
        self.unverified.clone().map_or(Ok(()), Err)
    }
}

impl World for Fake {
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

    fn relaunch(&mut self, bundle: &Path, args: &[OsString]) -> io::Result<()> {
        self.relaunched.push((bundle.to_path_buf(), args.to_vec()));
        Ok(())
    }
}

/// A stand-in world whose launch is `launch`.
fn launching(launch: LaunchHook) -> Fake {
    Fake {
        on_launch: Some(launch),
        ..Fake::default()
    }
}

/// Whether `world` said a line containing `words`.
fn said(world: &Fake, words: &str) -> bool {
    world.said.iter().any(|line| line.contains(words))
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
            nonce: Some(self.applier),
            data: self.data.clone(),
            agents: Some(self.agents.clone()),
            limits,
        }
    }

    /// The rescue build's road, as recovery: no applier's nonce.
    fn recovery(&self, limits: Limits) -> Road {
        Road {
            nonce: None,
            ..self.road(limits)
        }
    }

    /// The LaunchAgent the applier arms, armed by its door.
    fn arm(&self) {
        let rescue = self.home.rescue_executable(self.txn).unwrap();
        let _armed =
            launch_agent::arm(&self.agents, self.txn.bytes(), &rescue, self.home.root()).unwrap();
    }

    /// The installation after the exchange: the new bundle live, the old one
    /// in `stage/`.
    fn exchanged(&self) {
        install_flip::exchange(&self.installed, &self.stage()).unwrap();
    }

    /// The new build's executable wherever it is now.
    fn new_program(&self) -> PathBuf {
        if version_of(&self.installed) == "2.0" {
            self.installed.clone()
        } else {
            self.stage()
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
        quit_within: Duration::from_millis(1_000),
        end_within: Duration::from_millis(5_000),
    }
}

/// The rescue build as recovery on its own worker, over `road`.
fn recovered(road: Road, mut hands: Fake) -> (Option<Ended>, Fake) {
    on_a_worker(move |worker| {
        let ended = recover(worker, &road, &mut hands);
        (ended, hands)
    })
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
    assert_eq!(ended, Ended::LockHeld);
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
/// committed, the trial is stopped and the old bundle comes back (U-29).
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
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(world.said.iter().any(|line| line.contains("is refused")));
    assert!(
        world
            .wrote()
            .contains("[Armed, Moving, Trial, RollbackIntent, RolledBack, Retired]"),
        "{}",
        world.wrote()
    );
    let trial = children.started.lock().unwrap()[0];
    assert!(children.ended(trial).is_some(), "the trial is gone");
    assert!(said(&world, "is asked to quit"), "{:?}", world.said);
    assert!(!said(&world, "is asked to end"), "it quit when asked");
    assert_eq!(
        version_of(&install.installed),
        "1.0",
        "the old bundle is back"
    );
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

    // M6, and M9 after it (U-29).
    let install = Install::new("m6");
    install.write(Phase::Moving);
    install_flip::exchange(&install.installed, &install.stage()).unwrap();
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), Fake::default());
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        world
            .wrote()
            .contains("[RollbackIntent, RolledBack, Retired]"),
        "{}",
        world.wrote()
    );
    assert_eq!(version_of(&install.installed), "1.0", "swapped back");

    // Another applier's hand-over.
    let install = Install::new("other");
    let before = std::fs::read(install.home.journal()).unwrap();
    let mut road = install.road(limits(5_000, 5_000));
    road.nonce = Some(Nonce::new([0x45; 32]));
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

// ── U-29: the rollback, `Stuck`, and the relaunch ─────────────────────────────

/// The recovery door (`update_recover`) answers its start with this world.
impl crate::update_recover::World for Fake {
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        self.relaunched.push((program.to_path_buf(), args.to_vec()));
        Ok(())
    }
}

/// **The recovery door over `install`** on a worker, handed `then_launch`:
/// its exit code and what it asked of `hands`.
fn recover_door(install: &Install, then_launch: Vec<OsString>, hands: Fake) -> (i32, Fake) {
    let (home, installed, data, agents) = (
        install.home.clone(),
        install.installed.join(EXE),
        install.data.clone(),
        install.agents.clone(),
    );
    on_a_worker(move |worker| {
        let mut hands = hands;
        let door = crate::update_recover::Door {
            home: &home,
            installed: &installed,
            then_launch: Some(&then_launch),
            data: &data,
            agents: Some(&agents),
            limits: limits(5_000, 5_000),
        };
        let code = crate::update_recover::run(worker, &door, &mut hands);
        (code, hands)
    })
}

/// A journal at `Stuck` after `attempts` failed rollbacks.
fn stuck(attempts: u8) -> Phase {
    Phase::Stuck {
        trial: None,
        last_error: "the exchange is refused (test)".to_owned(),
        attempts,
    }
}

/// The words after a rollback, then `rest`.
fn failed_then(install: &Install, rest: &[&str]) -> Vec<OsString> {
    let mut words = failed_words(&install.home).to_vec();
    words.extend(rest.iter().map(OsString::from));
    words
}

/// A running process of `program`, by its pid, as the journal records a trial.
fn recorded(program: &Path, pid: u32) -> TrialProcess {
    let listed = install_flip::running_from(program)
        .unwrap()
        .into_iter()
        .find(|running| running.pid == pid)
        .expect("the process runs from the program");
    TrialProcess {
        pid,
        started: listed.started,
    }
}

/// RED (U-29, M9) — **a trial that sends no receipt by the deadline is asked
/// to quit, the old bundle is swapped back and verified, `RolledBack` and the
/// retirement follow, and the old build is started again with
/// `--update-failed <journal>`.**
///
/// §C.5: "N exits, crashes, or the deadline passes → P journals
/// `RollingBack` and reverses the flip from what is on disk (macOS: the same
/// `RENAME_SWAP` back). It then relaunches the restored old build with
/// `--update-failed <journal>`"; the revision's U-21 section names this test.
/// The journal is kept at `Retired{RolledBack}` for the relaunched build's
/// card; the plist and `H/<txn>` are gone.
///
/// MUTATION: in `declare_rollback`, answer `Ok(Ended::Stuck(..))` right after
/// recording `RollbackDeclared` (the rollback never runs).
#[test]
fn a_failed_health_swaps_back() {
    if !on_macos() {
        return;
    }
    let install = Install::new("health");
    let children = Children::default();
    let started = children.clone();
    let world = launching(Box::new(move |bundle, _| {
        started.start(bundle, "trial");
        Ok(())
    }));
    let (ended, world) = applied(install.road(limits(5_000, 1_500)), world);
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        world
            .wrote()
            .contains("[Armed, Moving, Trial, RollbackIntent, RolledBack, Retired]"),
        "{}",
        world.wrote()
    );
    assert!(said(&world, "no receipt by the trial's deadline"));
    assert!(said(&world, "is asked to quit"), "{:?}", world.said);
    assert_eq!(
        version_of(&install.installed),
        "1.0",
        "the old bundle is back"
    );
    let journal = install.on_disk().expect("kept for the relaunched card");
    assert_eq!(
        journal.body.phase,
        Phase::Retired {
            outcome: Outcome::RolledBack
        }
    );
    assert_eq!(journal.header().outcome, HeaderOutcome::RolledBack);
    assert!(!install.plist().exists());
    assert!(!install.home.transaction(install.txn).exists());
    assert_eq!(
        world.relaunched,
        vec![(install.installed.clone(), failed_then(&install, &[]))]
    );
    assert!(
        install_txn::try_hold(&install.home.lock(), Hold::Exclusive)
            .unwrap()
            .is_some(),
        "the lock is let go before the relaunch"
    );
}

/// RED (U-29, M7 → M9) — **a trial that dies without a receipt is rolled back
/// at once**, not at the deadline, and nothing is signalled: there is no
/// process left to stop.
///
/// W7: "P alive: P waits for the receipt until the deadline" — only while the
/// recorded trial process lives. The test ends the trial as soon as `Trial`
/// is durable; the deadline is twenty seconds away.
///
/// MUTATION: in `trial`, treat the recorded process as alive whatever
/// `still_running` answers (the rollback waits for the deadline).
#[test]
fn a_dead_trial_rolls_back_at_once() {
    if !on_macos() {
        return;
    }
    let install = Install::new("dead");
    let children = Children::default();
    let started = children.clone();
    let world = launching(Box::new(move |bundle, _| {
        started.start(bundle, "trial");
        Ok(())
    }));
    let killer = {
        let children = children.clone();
        let journal = install.home.journal();
        std::thread::spawn(move || {
            let give_up = Instant::now() + Duration::from_secs(20);
            while Instant::now() < give_up {
                let trial = std::fs::read(&journal)
                    .ok()
                    .and_then(|bytes| Journal::parse(&bytes).ok())
                    .is_some_and(|journal| journal.body.phase.kind() == PhaseKind::Trial);
                if trial {
                    for pid in children.started.lock().unwrap().clone() {
                        children.end(pid);
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    };
    let began = Instant::now();
    let (ended, world) = applied(install.road(limits(5_000, 20_000)), world);
    killer.join().unwrap();
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        began.elapsed() < Duration::from_secs(12),
        "rolled back {:?} after the start: it waited for the deadline",
        began.elapsed()
    );
    assert!(said(&world, "ended without a receipt"));
    assert!(!said(&world, "is asked to"), "{:?}", world.said);
    assert_eq!(version_of(&install.installed), "1.0");
}

/// RED (U-29, M9) — **the swap back is decided by the live identity**: with
/// the new bundle live, recovery exchanges once; with the old bundle already
/// live (the exchange never happened, or a swap back happened and the power
/// went before `RolledBack`), it exchanges nothing — and both end
/// `RolledBack` with the old bundle live and the entrance removed.
///
/// M9: "If the live identity is new, `RENAME_SWAP` back. If it is already
/// old, do not swap."
///
/// MUTATION: in `update_txn::restore`, answer `SwapBack` whenever the
/// staged bundle is the old one or the new one (whatever is live).
#[test]
fn rollback_from_the_new_live_swaps_and_from_the_old_live_does_not() {
    if !on_macos() {
        return;
    }
    let install = Install::new("new-live");
    install.exchanged();
    install.write(Phase::RollbackIntent { trial: None });
    install.arm();
    let (ended, hands) = recovered(install.recovery(limits(5_000, 5_000)), Fake::default());
    assert_eq!(ended, Some(Ended::RolledBack), "{:?}", hands.said);
    assert_eq!(hands.exchanges, 1);
    assert_eq!(version_of(&install.installed), "1.0");
    assert!(!install.plist().exists());
    assert_eq!(
        install.on_disk().unwrap().body.phase,
        Phase::Retired {
            outcome: Outcome::RolledBack
        }
    );

    let install = Install::new("old-live");
    install.write(Phase::RollbackIntent { trial: None });
    install.arm();
    let (ended, hands) = recovered(install.recovery(limits(5_000, 5_000)), Fake::default());
    assert_eq!(ended, Some(Ended::RolledBack), "{:?}", hands.said);
    assert_eq!(hands.exchanges, 0, "the old bundle was already live");
    assert_eq!(version_of(&install.installed), "1.0");
    assert!(!install.plist().exists());
}

/// RED (U-29, M9 → M10) — **a swap back that fails is `Stuck{last_error,
/// attempts: 1}`, with everything kept** — both bundles where they are, the
/// rescue clone, the entrance, the journal — and the live build is started
/// with `--update-failed`, whose start then continues past the unfinished
/// rollback with the card at *Update incomplete.*
///
/// §C.5: "Rollback itself fails → journal `Failed` with `last_error`, backups
/// kept, the RunOnce entry kept … a third outcome, named"; (b).2 W10/M10.
///
/// MUTATION: in `stuck`, answer `Ended::Stuck` without recording
/// `RollbackFailed`.
#[test]
fn a_failed_swap_back_is_stuck_with_everything_kept() {
    if !on_macos() {
        return;
    }
    let install = Install::new("stuck");
    let world = Fake {
        refuse_from: 1,
        ..Fake::default()
    };
    let (ended, world) = applied(install.road(limits(5_000, 1_000)), world);
    assert!(
        matches!(ended, Ended::Stuck(_)),
        "{ended:?}: {:?}",
        world.said
    );
    let journal = install.on_disk().unwrap();
    let Phase::Stuck {
        last_error,
        attempts,
        ..
    } = &journal.body.phase
    else {
        panic!("{:?}", journal.body.phase);
    };
    assert_eq!(*attempts, 1);
    assert!(last_error.contains("refused"), "{last_error}");
    assert_eq!(version_of(&install.installed), "2.0", "nothing moved back");
    assert_eq!(
        version_of(&install.stage()),
        "1.0",
        "the old bundle is kept"
    );
    assert!(install.home.rescue_bundle(install.txn).unwrap().exists());
    assert!(install.plist().exists(), "the entrance is kept");
    assert!(said(&world, &install.home.root().display().to_string()));
    assert_eq!(
        world.relaunched,
        vec![(install.installed.clone(), failed_then(&install, &[]))]
    );
    let header = journal.header();
    assert_eq!(header.class, Class::Destructive);
    let sent = crate::update_txn::StartView {
        journal: crate::update_txn::JournalRead::Read(header),
        lock_free: true,
        own_image: None,
        rescue_image: None,
        trial_of: None,
        sent_by_rollback: true,
    };
    assert_eq!(crate::update_txn::at_start(&sent), StartAction::Continue);
}

/// RED (U-29, M10) — **`Stuck` is tried again by the next lock holder — the
/// rescue build that a start handed itself to — and each failure counts; at
/// the bound nothing more is tried and the line names the journal's folder;
/// a retry below the bound that can finish does.** Each run starts the
/// installed build with `--update-failed` and the handed command line.
///
/// The coordinator's ruling 4 (U-29): "the rescue retries M9 at every
/// login/start, bounded … add `Stuck{attempts}` and stop after 3 with the
/// sentence naming the folder".
///
/// MUTATION: drop the `attempts >= STUCK_ATTEMPT_LIMIT` arm of
/// `update_txn::decide`.
#[test]
fn stuck_is_retried_on_the_next_start_and_stops_after_the_bound() {
    if !on_macos() {
        return;
    }
    let install = Install::new("bound");
    install.exchanged();
    install.write(stuck(1));
    install.arm();
    let handed = vec![OsString::from("--tab")];
    for attempts in 2..=STUCK_ATTEMPT_LIMIT {
        let refusing = Fake {
            refuse_from: 0,
            ..Fake::default()
        };
        let (code, hands) = recover_door(&install, handed.clone(), refusing);
        assert_eq!(code, 0, "{:?}", hands.said);
        assert_eq!(hands.exchanges, 1, "tried again");
        let Phase::Stuck { attempts: now, .. } = install.on_disk().unwrap().body.phase else {
            panic!("still Stuck");
        };
        assert_eq!(now, attempts);
        assert_eq!(
            hands.relaunched,
            vec![(
                install.installed.join(EXE),
                failed_then(&install, &["--tab"])
            )]
        );
    }
    let before = std::fs::read(install.home.journal()).unwrap();
    let (code, hands) = recover_door(&install, handed.clone(), Fake::default());
    assert_eq!(code, 0);
    assert_eq!(hands.exchanges, 0, "nothing is tried at the bound");
    assert_eq!(std::fs::read(install.home.journal()).unwrap(), before);
    assert!(
        hands
            .said
            .iter()
            .any(|line| line.contains("after 3 attempts")
                && line.contains(&*install.home.root().to_string_lossy())),
        "{:?}",
        hands.said
    );
    assert_eq!(version_of(&install.installed), "2.0");
    assert!(install.plist().exists());
    let log = std::fs::read_to_string(crate::diagnostics::log_path(&install.data)).unwrap();
    assert!(log.contains("after 3 attempts"), "the line is in the log");

    let install = Install::new("retry");
    install.exchanged();
    install.write(stuck(1));
    install.arm();
    let (code, hands) = recover_door(&install, handed, Fake::default());
    assert_eq!(code, 0, "{:?}", hands.said);
    assert_eq!(version_of(&install.installed), "1.0");
    assert!(!install.plist().exists());
    assert_eq!(
        hands.relaunched,
        vec![(
            install.installed.join(EXE),
            failed_then(&install, &["--tab"])
        )]
    );
}

/// **The ordinary start, over a macOS home, with the start's effects
/// performed for real where they are files** (the entrance through its door,
/// in the test's LaunchAgents folder).
struct StartWorld {
    agents: PathBuf,
    said: Vec<String>,
}

impl crate::update_startup::World for StartWorld {
    fn say(&mut self, line: &str) {
        self.said.push(line.to_owned());
    }

    fn spawn_detached(&mut self, program: &Path, _args: &[OsString]) -> io::Result<()> {
        panic!("a retired transaction hands nothing to {program:?}")
    }

    fn retire_entrance(&mut self, txn: TxnId) -> Result<(), String> {
        retire_entrance_in(Some(&self.agents), txn)
    }

    fn mounts_under(&mut self, _folder: &Path) -> Result<Vec<PathBuf>, String> {
        Ok(Vec::new())
    }

    fn on_a_worker(&mut self, _job: crate::update_startup::OffThread) -> io::Result<()> {
        panic!("nothing is mounted")
    }
}

/// RED (U-29, M11) — **a transaction found at `RolledBack` is retired by the
/// next lock holder — the entrance removed, `Retired{RolledBack}`, `H/<txn>`
/// gone — and the next ordinary start, sent with `--update-failed`, reads
/// *Previous version restored.* from the header and retires the journal.**
///
/// M11: "`RolledBack` … as W11–W13; the plist is removed after the terminal
/// state is durable." The power went after `RolledBack` was durable.
///
/// MUTATION: in `finish_rollback`, skip `disarm`.
#[test]
fn rolled_back_is_retired_at_the_next_start() {
    if !on_macos() {
        return;
    }
    let install = Install::new("m11");
    install.write(Phase::RolledBack);
    install.arm();
    let (ended, hands) = recovered(install.recovery(limits(5_000, 5_000)), Fake::default());
    assert_eq!(ended, Some(Ended::RolledBack), "{:?}", hands.said);
    assert_eq!(hands.exchanges, 0);
    assert!(!install.plist().exists(), "the entrance is removed");
    assert!(!install.home.transaction(install.txn).exists());
    let journal = install.home.journal();
    assert_eq!(
        install.on_disk().unwrap().body.phase,
        Phase::Retired {
            outcome: Outcome::RolledBack
        }
    );

    let exe = install.installed.join(EXE);
    let argv = failed_words(&install.home).to_vec();
    let mut world = StartWorld {
        agents: install.agents.clone(),
        said: Vec::new(),
    };
    let verdict = crate::update_startup::run(
        &crate::update_startup::Start {
            own_exe: &exe,
            home: &install.home,
            argv: &argv,
            trial: None,
            failed: Some(&journal),
        },
        &mut world,
    );
    let crate::update_startup::Verdict::Continue { failed, .. } = verdict else {
        panic!("the start continues: {:?}", world.said);
    };
    assert_eq!(failed, Some(crate::update_job::Failure::RolledBack));
    assert!(!journal.exists(), "the start retired the journal");
}

/// RED (U-29) — **the trial is asked to quit before it is ended**: a trial
/// that ignores `SIGTERM` gets it first (it writes a mark), is given
/// [`Limits::quit_within`], and only then `SIGKILL`; a recorded process whose
/// image is not the new build's is never signalled at all.
///
/// The coordinator's ruling 2 (U-29): "Ask it to quit … wait a bounded time
/// … then `SIGKILL`; never touch a process whose image is not the trial's
/// executable".
///
/// MUTATION: in `stop_trial`, ask `Ask::End` first.
#[test]
fn the_trial_is_asked_to_quit_before_it_is_killed() {
    if !on_macos() {
        return;
    }
    let install = Install::new("stubborn");
    install.exchanged();
    let children = Children::default();
    let mark = install.data.join("term-mark");
    let pid = children.start_with(
        &install.new_program(),
        &[OsStr::new("stubborn"), mark.as_os_str()],
    );
    std::thread::sleep(Duration::from_millis(200));
    let trial = recorded(&install.installed.join(EXE), pid);
    install.write(Phase::RollbackIntent { trial: Some(trial) });
    let began = Instant::now();
    let (ended, hands) = recovered(install.recovery(limits(5_000, 5_000)), Fake::default());
    assert_eq!(ended, Some(Ended::RolledBack), "{:?}", hands.said);
    assert!(
        began.elapsed() >= Duration::from_millis(1_000),
        "ended before its grace ran out"
    );
    assert_eq!(std::fs::read_to_string(&mark).unwrap(), "term");
    let quit = hands
        .said
        .iter()
        .position(|line| line.contains("is asked to quit"));
    let end = hands
        .said
        .iter()
        .position(|line| line.contains("is asked to end"));
    assert!(quit.is_some() && quit < end, "{:?}", hands.said);
    assert!(children.ended(pid).is_some(), "the stubborn trial is gone");

    // A process of the old build, recorded as the trial: not the new build's
    // image, so nothing is sent and the rollback goes on around it.
    let install = Install::new("stranger");
    install.exchanged();
    let children = Children::default();
    let stranger = children.start(&install.stage(), "by-hand");
    std::thread::sleep(Duration::from_millis(200));
    let trial = recorded(&install.stage().join(EXE), stranger);
    install.write(Phase::RollbackIntent { trial: Some(trial) });
    let (ended, hands) = recovered(install.recovery(limits(5_000, 5_000)), Fake::default());
    assert_eq!(ended, Some(Ended::RolledBack), "{:?}", hands.said);
    assert!(!hands.said.iter().any(|line| line.contains("is asked to")));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(children.status(stranger), None, "the stranger still runs");
}

/// RED (U-29) — **after `Abandoned` nothing is started again; after a revert
/// the old build is started again with no word at all.**
///
/// The coordinator's ruling 1 (U-29): "After `Abandoned` at the applier (O
/// never left, or the claim stayed held) no relaunch … After a revert at
/// admission (`Moving` → back to `Prepared`) the applier relaunches the old
/// build without a flag (it is unchanged; the transaction waits `Prepared` for
/// the deferred rule)."
///
/// MUTATION: in `apply`, give the revert's relaunch the words
/// `failed_words` gives a rollback's.
#[test]
fn no_relaunch_after_abandoned_and_a_plain_relaunch_after_a_revert() {
    if !on_macos() {
        return;
    }
    let install = Install::new("abandoned");
    let claim = bt_platform::instance::claim_data_directory(&install.data).unwrap();
    let (ended, world) = applied(install.road(limits(1_000, 5_000)), Fake::default());
    drop(claim);
    assert_eq!(ended, Ended::Abandoned, "{:?}", world.said);
    assert!(world.relaunched.is_empty(), "{:?}", world.relaunched);

    let install = Install::new("reverted");
    std::fs::write(install.home.admission(), b"").unwrap();
    let copy = install_txn::try_hold(&install.home.admission(), Hold::Shared)
        .unwrap()
        .unwrap();
    let (ended, world) = applied(install.road(limits(1_000, 5_000)), Fake::default());
    drop(copy);
    assert_eq!(ended, Ended::Reverted, "{:?}", world.said);
    assert_eq!(
        world.relaunched,
        vec![(install.installed.clone(), Vec::new())]
    );
    assert_eq!(
        install.on_disk().unwrap().body.phase,
        Phase::Prepared {
            deferred_launches: 0
        }
    );
}

/// RED (U-29, the owner's ruling of 2026-09-27) — **a finished rollback
/// leaves only what the old build reads**: the old bundle live with its own
/// identity; in the home the journal, the lock and the admission and nothing
/// else; a journal whose frozen v1 header the old build's parser reads as
/// `terminal` / `rolled_back` — its start retires it, and a trial reading it
/// knows its transaction ended; and no entrance of this transaction.
///
/// "Everything Folio writes outside its own data root … keeps a format the
/// previous version also reads, so a rollback leaves nothing the old build
/// cannot repair on its next start." The rescue clone is a copy of the old
/// build, and wrote every byte here; the header is U-10's frozen format
/// (`update_txn::tests`' frozen-header cases).
///
/// MUTATION: in `retire`, skip the removal of `H/<txn>`.
#[test]
fn rolled_back_leaves_only_what_the_old_build_reads() {
    if !on_macos() {
        return;
    }
    let install = Install::new("leaves");
    let (ended, world) = applied(install.road(limits(5_000, 1_000)), Fake::default());
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    let (installed, old) = (install.installed.clone(), install.old.clone());
    let live = on_a_worker(move |worker| {
        crate::update_prepare_macos::identity(worker, &installed).unwrap()
    });
    assert_eq!(live, old);
    let mut left: Vec<String> = std::fs::read_dir(install.home.root())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["admission", "journal.json", "lock"]);
    let bytes = std::fs::read(install.home.journal()).unwrap();
    let header = Header::parse(&bytes).expect("the frozen v1 header");
    assert_eq!(
        (header.class, header.outcome),
        (Class::Terminal, HeaderOutcome::RolledBack)
    );
    let view = crate::update_txn::StartView {
        journal: crate::update_txn::JournalRead::Read(header),
        lock_free: true,
        own_image: None,
        rescue_image: None,
        trial_of: None,
        sent_by_rollback: false,
    };
    assert_eq!(crate::update_txn::at_start(&view), StartAction::Retire);
    assert_eq!(
        crate::update_txn::trial_sight(Some(&bytes), &install.txn),
        TrialSight::Ended
    );
    let ours: Vec<_> = std::fs::read_dir(&install.agents)
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name();
            launch_agent::is_ours(&name).then_some(name)
        })
        .collect();
    assert!(ours.is_empty(), "{ours:?}");
}
