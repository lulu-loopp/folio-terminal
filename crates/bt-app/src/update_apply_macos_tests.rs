//! **The macOS applier over real folders, real synthetic signed bundles, real
//! locks, a real claim, a real LaunchAgent plist in a folder of the test's own
//! and real processes** (macOS); nothing here runs elsewhere.
//!
//! Every bundle is built here: an `arm64` executable compiled from a few lines
//! of C (`/usr/bin/cc`) — with an argument it stays up for twenty seconds (a
//! trial, or a process started by hand), and quits at `SIGTERM` as any process
//! does; with `stubborn <file>` it writes `ready` into that file once it keeps
//! `SIGTERM`, then `term` at each `SIGTERM`, and stays (a trial that will not
//! quit, U-29); without one it
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
static void put(const char *word) {\n\
  int fd = open(mark, O_WRONLY | O_CREAT | O_TRUNC, 0644);\n\
  if (fd >= 0) { if (write(fd, word, strlen(word)) < 0) {} close(fd); }\n\
}\n\
static void on_term(int signal_number) {\n\
  (void)signal_number;\n\
  put(\"term\");\n\
}\n\
int main(int argc, char **argv) {\n\
  char line[256];\n\
  if (argc > 2 && strcmp(argv[1], \"stubborn\") == 0) {\n\
    mark = argv[2]; signal(SIGTERM, on_term); put(\"ready\"); for (;;) pause();\n\
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
/// A launch that hands back the process standing for `open -W` (U-38).
type HeldLaunchHook = Box<dyn FnMut(&Path, &[OsString]) -> io::Result<Child> + Send>;
type ExchangeHook = Box<dyn FnMut(&Path, &Path) + Send>;
type SayHook = Box<dyn FnMut(&str) + Send>;

/// **The stand-in world**: its lines kept, the real exchange after an
/// optional look (or a refusal, from the `refuse_from`-th on), a check of the
/// restored bundle that passes unless the test says why not, a launch that
/// does what the test says, and relaunches recorded.
struct Fake {
    said: Vec<String>,
    launched: Arc<Mutex<Vec<Vec<OsString>>>>,
    relaunched: Vec<(PathBuf, Vec<OsString>)>,
    on_launch: Option<LaunchHook>,
    /// The launch, held as the product holds `open -W` (U-38).
    on_launch_held: Option<HeldLaunchHook>,
    on_exchange: Option<ExchangeHook>,
    real_open: bool,
    /// Exchanges performed or refused so far.
    exchanges: usize,
    /// The first exchange (counted from 0) that is refused.
    refuse_from: usize,
    unverified: Option<String>,
    /// Looks at every line as it is said (U-34).
    on_say: Option<SayHook>,
    /// Every start dies before it takes the data directory (U-34, round 2).
    starts_die: bool,
    /// The failure windows shown in this process (U-34, round 2).
    shown: Vec<String>,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            said: Vec::new(),
            launched: Arc::default(),
            relaunched: Vec::new(),
            on_launch: None,
            on_launch_held: None,
            on_exchange: None,
            real_open: false,
            exchanges: 0,
            refuse_from: usize::MAX,
            unverified: None,
            on_say: None,
            starts_die: false,
            shown: Vec::new(),
        }
    }
}

impl Hands for Fake {
    fn say(&mut self, line: &str) {
        if let Some(look) = &mut self.on_say {
            look(line);
        }
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

    fn launch_trial(&mut self, bundle: &Path, args: &[OsString]) -> io::Result<Launch> {
        self.launched.lock().unwrap().push(args.to_vec());
        if self.real_open {
            return (Machine { log: None }).launch_trial(bundle, args);
        }
        if let Some(launch) = &mut self.on_launch_held {
            return launch(bundle, args).map(Launch::of);
        }
        match &mut self.on_launch {
            Some(launch) => launch(bundle, args).map(|()| Launch::untracked()),
            None => Ok(Launch::untracked()),
        }
    }

    fn acknowledged(&mut self, _worker: Option<&WorkerCtx>, _data: &Path) -> bool {
        !self.starts_die
    }

    fn show_here(&mut self, text: &str) {
        self.shown.push(text.to_owned());
    }
}

impl World for Fake {
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
        install_txn::durable_write(&self.home.journal(), &self.journal_at(phase).encode()).unwrap();
    }

    /// This installation's journal at `phase`, with both identities.
    fn journal_at(&self, phase: Phase) -> Journal {
        Journal {
            txn: self.txn,
            rescue: self
                .home
                .rescue_bundle(self.txn)
                .unwrap()
                .display()
                .to_string(),
            body: Body {
                adapter: crate::update_txn::Adapter::Ours,
                phase,
                layout: Layout::Bundle {
                    old: self.old.clone(),
                    new: self.new.clone(),
                },
            },
        }
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
            starter: None,
            handed_back: None,
            layouts: own_layouts(),
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
            // As the product's trial writes it (H.1).
            started: install_flip::started_of(pid),
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
        let ended = recover(worker, &road, &mut hands, None).ended;
        (Some(ended), hands)
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
            // As the product's trial writes it (H.1).
            started: install_flip::started_of(pid),
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
            started: None,
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
            outcome: Outcome::Committed,
            untried: false,
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
            outcome: Outcome::Committed,
            untried: false,
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

/// RED (U-28, M4–M6; U-29b) — **an applier started again over its own
/// transaction goes on from what is live**: at `Armed` it admits, exchanges
/// and commits; at `Moving` with the old bundle live it removes the plist and
/// reverts to `Prepared` (M5); at `Moving` with the new bundle live it starts
/// the trial nobody started and commits on its receipt, or rolls back when it
/// cannot start one (M6, as the coordinator's ruling 1 of U-29b decides it
/// for recovery too). A hand-over to another applier is refused and nothing
/// is touched.
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

    // M6 as U-29b rules it: decided by a trial, which commits here; and a
    // trial that cannot start is rolled back (M9, U-29).
    let install = Install::new("m6");
    install.write(Phase::Moving);
    install_flip::exchange(&install.installed, &install.stage()).unwrap();
    let children = Children::default();
    let world = launching(a_healthy_trial(&install, &children, None));
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), world);
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(
        world.wrote().contains("[Trial, Committed, Retired]"),
        "{}",
        world.wrote()
    );
    assert_eq!(version_of(&install.installed), "2.0", "kept");

    let install = Install::new("m6-no-trial");
    install.write(Phase::Moving);
    install_flip::exchange(&install.installed, &install.stage()).unwrap();
    let world = launching(Box::new(|_, _| Err(io::Error::other("no open (test)"))));
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), world);
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
            starter: None,
            handed_back: None,
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
        retrial: None,
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
            outcome: Outcome::RolledBack,
            untried: false,
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
            outcome: Outcome::RolledBack,
            untried: true,
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
    // The new bundle is live and not committed: it is started again only as
    // a trial, with the card's words after the trial's (U-29b, ruling 3),
    // and never plainly. The one over `Stuck` was never found running here
    // (the stand-in `open` starts nothing), so the exit guard has no
    // successor and starts one more — as a trial too (U-34).
    assert!(
        world
            .relaunched
            .iter()
            .all(|(_, words)| words.first() == Some(&OsString::from(cli::UPDATE_TRIAL_FLAG))),
        "{:?}",
        world.relaunched
    );
    let launched = world.launched.lock().unwrap().clone();
    assert_eq!(launched.len(), 2, "the trial, then the one over Stuck");
    assert_eq!(launched[1][0], OsString::from(cli::UPDATE_TRIAL_FLAG));
    assert_eq!(launched[1][3..].to_vec(), failed_then(&install, &[]));
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
/// installed build with `--update-failed` and the handed command line — the
/// new build, still live, only as a trial (U-29b): here `open` refuses the
/// trial this run would record, so the door starts one no journal records.
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
    let no_open = || -> LaunchHook { Box::new(|_, _| Err(io::Error::other("no open (test)"))) };
    for attempts in 2..=STUCK_ATTEMPT_LIMIT {
        let refusing = Fake {
            refuse_from: 0,
            on_launch: Some(no_open()),
            ..Fake::default()
        };
        let (code, hands) = recover_door(&install, handed.clone(), refusing);
        assert_eq!(code, 0, "{:?}", hands.said);
        assert_eq!(hands.exchanges, 1, "tried again");
        let Phase::Stuck { attempts: now, .. } = install.on_disk().unwrap().body.phase else {
            panic!("still Stuck");
        };
        assert_eq!(now, attempts);
        assert_eq!(hands.relaunched.len(), 1, "{:?}", hands.relaunched);
        let (program, words) = &hands.relaunched[0];
        assert_eq!(program, &install.installed.join(EXE));
        assert_eq!(words[0], OsString::from(cli::UPDATE_TRIAL_FLAG));
        assert_eq!(words[3..].to_vec(), failed_then(&install, &["--tab"]));
    }
    let before = std::fs::read(install.home.journal()).unwrap();
    let at_the_bound = Fake {
        on_launch: Some(no_open()),
        ..Fake::default()
    };
    let (code, hands) = recover_door(&install, handed.clone(), at_the_bound);
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
    install.write(Phase::RolledBack { untried: false });
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
            outcome: Outcome::RolledBack,
            untried: false,
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
    // It keeps `SIGTERM` only once it says so: a signal before that would end
    // it at once, however loaded the machine.
    let give_up = Instant::now() + Duration::from_secs(20);
    while std::fs::read_to_string(&mark).ok().as_deref() != Some("ready") {
        assert!(
            Instant::now() < give_up,
            "the stubborn trial never got ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
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

/// RED (U-29; U-34) — **after `Abandoned`, and after a revert, the old build
/// is started again with no word at all.**
///
/// The coordinator's ruling 1 (U-29): "After a revert at admission (`Moving`
/// → back to `Prepared`) the applier relaunches the old build without a flag
/// (it is unchanged; the transaction waits `Prepared` for the deferred
/// rule)." Its "After `Abandoned` … no relaunch" is superseded by U-34: the
/// applier leaves through its exit guard, and with no successor running it
/// starts what the disk names — the journal is over, so the old build,
/// plainly.
///
/// MUTATION: in `apply`, give the revert's relaunch the words
/// `failed_words` gives a rollback's.
#[test]
fn a_plain_relaunch_after_abandoned_and_after_a_revert() {
    if !on_macos() {
        return;
    }
    let install = Install::new("abandoned");
    let claim = bt_platform::instance::claim_data_directory(&install.data).unwrap();
    let (ended, world) = applied(install.road(limits(1_000, 5_000)), Fake::default());
    drop(claim);
    assert_eq!(ended, Ended::Abandoned, "{:?}", world.said);
    assert_eq!(
        world.relaunched,
        vec![(install.installed.clone(), Vec::new())],
        "{:?}",
        world.said
    );

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

// ── U-29b: whatever the journal says, a start opens Folio ─────────────────────

/// **An ordinary start's world over a macOS home**: the entrance removed
/// through its door in the test's LaunchAgents folder, nothing mounted, and
/// the hand-over recorded — the test runs the door the line names itself.
struct Starting {
    agents: PathBuf,
    said: Vec<String>,
    spawned: Vec<(PathBuf, Vec<OsString>)>,
}

impl crate::update_startup::World for Starting {
    fn say(&mut self, line: &str) {
        self.said.push(line.to_owned());
    }

    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        self.spawned.push((program.to_path_buf(), args.to_vec()));
        // As the operating system answers: a program that is not there cannot
        // be started.
        if program.is_file() {
            Ok(())
        } else {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
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

/// **What one person's start opened**: the start itself going on (with its
/// card), or — once it handed itself over — what the recovery door started:
/// the installed program with its words, and trials through `open`.
struct Opened {
    continued: Option<Option<crate::update_job::Failure>>,
    started: Vec<(PathBuf, Vec<OsString>)>,
    trials: Vec<Vec<OsString>>,
    hands: Fake,
}

impl Opened {
    /// Every start that happened: the start's own, the door's, the trials.
    fn launches(&self) -> usize {
        usize::from(self.continued.is_some()) + self.started.len() + self.trials.len()
    }
}

/// **A person's start over `install` with `argv`, then the recovery it handed
/// itself to**: the ordinary start's own pass, the line it wrote read back by
/// the command line's own parser and the rescue clone's own home finder, and
/// the recovery door run on a worker with `hands` within `limits`. No
/// applier runs.
fn start_then_recover(install: &Install, argv: &[&str], hands: Fake, limits: Limits) -> Opened {
    let exe = install.installed.join(EXE);
    let argv: Vec<OsString> = argv.iter().map(OsString::from).collect();
    let mut starting = Starting {
        agents: install.agents.clone(),
        said: Vec::new(),
        spawned: Vec::new(),
    };
    let verdict = crate::update_startup::run(
        &crate::update_startup::Start {
            own_exe: &exe,
            home: &install.home,
            argv: &argv,
            trial: None,
            failed: None,
        },
        &mut starting,
    );
    match verdict {
        crate::update_startup::Verdict::Continue { failed, .. } => {
            assert!(starting.spawned.is_empty(), "{:?}", starting.spawned);
            return Opened {
                continued: Some(failed),
                started: Vec::new(),
                trials: Vec::new(),
                hands,
            };
        }
        crate::update_startup::Verdict::Exit(code) => assert_eq!(code, 0, "{:?}", starting.said),
    }
    assert_eq!(starting.spawned.len(), 1, "{:?}", starting.said);
    let (program, line) = starting.spawned.remove(0);
    assert_eq!(
        program,
        install.home.rescue_executable(install.txn).unwrap()
    );
    let Some(Ok(cli::UpdateDoor::Recover {
        home: Some(named),
        then_launch: Some(then_launch),
        handed_back: None,
    })) = cli::update_door(line.clone())
    else {
        panic!("the hand-over line {line:?}");
    };
    let (home, installed) = Home::of_rescue_named(HostPlatform::MacOs, &program, &named)
        .expect("the rescue clone finds the home the line names");
    let (data, agents) = (install.data.clone(), install.agents.clone());
    let (_code, hands) = on_a_worker(move |worker| {
        let mut hands = hands;
        let door = crate::update_recover::Door {
            home: &home,
            installed: &installed,
            then_launch: Some(&then_launch),
            data: &data,
            agents: Some(&agents),
            limits,
            starter: None,
            handed_back: None,
        };
        let code = crate::update_recover::run(worker, &door, &mut hands);
        (code, hands)
    });
    let trials = hands.launched.lock().unwrap().clone();
    Opened {
        continued: None,
        started: hands.relaunched.clone(),
        trials,
        hands,
    }
}

/// **A trial recorded in `install`'s journal, as a dead applier leaves it**:
/// the new bundle live, the entrance armed, the new build's process started
/// and recorded by its pid and start instant, `Trial` begun now — its receipt
/// on disk when `answered`, and the process ended again unless `alive`.
fn a_recorded_trial(install: &Install, children: &Children, alive: bool, answered: bool) {
    install.exchanged();
    install.arm();
    let pid = children.start(&install.installed, "trial");
    std::thread::sleep(Duration::from_millis(200));
    let process = recorded(&install.installed.join(EXE), pid);
    let nonce = Nonce::new([0x6e; 32]);
    install.write(Phase::Trial {
        nonce,
        process,
        began_ms: now_ms(),
    });
    if answered {
        install.receipt(nonce, nonce, pid);
    }
    if !alive {
        children.end(pid);
        assert!(children.ended(pid).is_some(), "the trial is gone");
    }
}

/// **What `decide` leaves a holder to do over `install` now**, from its disk
/// as the door reads it; `None` with no journal.
fn left_to_do(install: &Install) -> Option<Action> {
    let journal = install.on_disk()?;
    let (installed, stage) = (install.installed.clone(), install.stage());
    let (live, staged) = on_a_worker(move |worker| {
        (
            crate::update_prepare_macos::identity(worker, &installed).ok(),
            crate::update_prepare_macos::identity(worker, &stage).ok(),
        )
    });
    let trial_alive = match &journal.body.phase {
        Phase::Trial { process, .. }
        | Phase::Stuck {
            trial: Some(process),
            ..
        } => install_flip::still_running(Running {
            pid: process.pid,
            started: process.started,
        }),
        _ => false,
    };
    Some(decide(&Disk {
        journal: &journal,
        asker: Asker::Rescue,
        entrance: install.plist().exists(),
        located: Located::Bundle {
            live,
            stage: staged,
        },
        receipt: None,
        trial_alive,
        now_ms: now_ms(),
    }))
}

/// What the table expects one start to open.
#[derive(Clone, Copy, Debug)]
enum Expect {
    /// The start went on itself: it retired a terminal journal.
    ItselfWent,
    /// The installed program, with `--update-failed <journal>` first when
    /// `failed`, then the handed `--tab`.
    Installed { failed: bool },
    /// The new build as a trial through `open`, its words then
    /// `--update-failed <journal>` when `failed`, then `--tab`.
    Trial { failed: bool },
}

/// RED (U-29b) — **every phase a dead applier can leave still opens Folio:
/// the start runs, exactly one launch follows — the old build plainly or
/// with `--update-failed`, the committed new build plainly, the new build
/// before `Committed` only as a trial — and the disk ends where `decide` has
/// nothing left for a holder but to leave, retire, or wait for a live
/// trial.**
///
/// The coordinator's rulings 1 and 2 (U-29b): "One recovery function for
/// every phase"; "A window always opens … exactly one bundle is started". On
/// BASE the ordinary start handed `Handoff`, `Armed`, `Exchanging` and
/// `Trial` to a rescue build that answered "not recovery's" and started
/// nothing. Real synthetic bundles, a real trial process, the real start's
/// pass, its line parsed back, and the real recovery door; the one
/// stand-in is `open` (a trial through it starts the new executable here).
///
/// MUTATION: in `update_recover::run`, recover only a `destructive` header
/// whose outcome is decided (`&& header.outcome != HeaderOutcome::None`).
#[test]
fn every_phase_left_by_a_dead_applier_still_opens_folio() {
    if !on_macos() {
        return;
    }
    type Setup = fn(&Install, &Children);
    let healthy = |install: &Install, children: &Children| {
        launching(a_healthy_trial(install, children, None))
    };
    let plain = |_: &Install, _: &Children| Fake::default();
    let refusing_then_healthy = |install: &Install, children: &Children| Fake {
        refuse_from: 0,
        ..launching(a_healthy_trial(install, children, None))
    };
    type MakeHands = fn(&Install, &Children) -> Fake;
    let cases: Vec<(&str, Setup, MakeHands, Expect, &str)> = vec![
        (
            "handoff",
            |_, _| {},
            plain,
            Expect::Installed { failed: false },
            "1.0",
        ),
        (
            "armed",
            |install, _| {
                install.write(Phase::Armed);
                install.arm();
            },
            plain,
            Expect::Installed { failed: false },
            "1.0",
        ),
        (
            "exchanging-old-live",
            |install, _| {
                install.write(Phase::Moving);
                install.arm();
            },
            plain,
            Expect::Installed { failed: false },
            "1.0",
        ),
        (
            "exchanging-new-live",
            |install, _| {
                install.write(Phase::Moving);
                install.arm();
                install.exchanged();
            },
            healthy,
            Expect::Trial { failed: false },
            "2.0",
        ),
        (
            "trial-alive-receipt",
            |install, children| a_recorded_trial(install, children, true, true),
            plain,
            Expect::Installed { failed: false },
            "2.0",
        ),
        (
            "trial-alive-none",
            |install, children| a_recorded_trial(install, children, true, false),
            plain,
            Expect::Installed { failed: true },
            "1.0",
        ),
        (
            "trial-dead-receipt",
            |install, children| a_recorded_trial(install, children, false, true),
            plain,
            Expect::Installed { failed: false },
            "2.0",
        ),
        (
            "trial-dead-none",
            |install, children| a_recorded_trial(install, children, false, false),
            plain,
            Expect::Installed { failed: true },
            "1.0",
        ),
        (
            "rollback-intent",
            |install, _| {
                install.exchanged();
                install.write(Phase::RollbackIntent { trial: None });
                install.arm();
            },
            plain,
            Expect::Installed { failed: true },
            "1.0",
        ),
        (
            "stuck-old-live",
            |install, _| {
                install.write(stuck(1));
                install.arm();
            },
            plain,
            Expect::Installed { failed: true },
            "1.0",
        ),
        (
            "stuck-new-live",
            |install, _| {
                install.exchanged();
                install.write(stuck(1));
                install.arm();
            },
            refusing_then_healthy,
            Expect::Trial { failed: true },
            "2.0",
        ),
        (
            "rolled-back",
            |install, _| {
                install.write(Phase::RolledBack { untried: false });
                install.arm();
            },
            plain,
            Expect::Installed { failed: true },
            "1.0",
        ),
        (
            "committed-with-debt",
            |install, _| {
                install.exchanged();
                install.write(Phase::Committed);
                install.arm();
            },
            plain,
            Expect::Installed { failed: false },
            "2.0",
        ),
        (
            "abandoned",
            |install, _| {
                install.write(Phase::Abandoned);
                install.arm();
            },
            plain,
            Expect::ItselfWent,
            "1.0",
        ),
    ];
    for (tag, setup, hands, expect, live) in cases {
        let install = Install::new(&format!("every-{tag}"));
        let children = Children::default();
        setup(&install, &children);
        let opened = start_then_recover(
            &install,
            &["--tab"],
            hands(&install, &children),
            limits(5_000, 1_500),
        );
        let said = &opened.hands.said;
        assert_eq!(opened.launches(), 1, "{tag}: one launch: {said:?}");
        let tab = OsString::from("--tab");
        let with_card = |failed: bool| {
            let mut words = if failed {
                failed_words(&install.home).to_vec()
            } else {
                Vec::new()
            };
            words.push(tab.clone());
            words
        };
        match expect {
            Expect::ItselfWent => assert!(opened.continued.is_some(), "{tag}"),
            Expect::Installed { failed } => assert_eq!(
                opened.started,
                vec![(install.installed.join(EXE), with_card(failed))],
                "{tag}: {said:?}"
            ),
            Expect::Trial { failed } => {
                let words = &opened.trials[0];
                assert_eq!(
                    words[..2].to_vec(),
                    [
                        OsString::from(cli::UPDATE_TRIAL_FLAG),
                        OsString::from(install.txn.to_string())
                    ],
                    "{tag}"
                );
                assert_eq!(words[3..].to_vec(), with_card(failed), "{tag}");
            }
        }
        assert_eq!(version_of(&install.installed), live, "{tag}: {said:?}");
        assert!(!install.plist().exists(), "{tag}: the entrance is gone");
        match left_to_do(&install) {
            None => assert!(matches!(expect, Expect::ItselfWent), "{tag}"),
            Some(action) => assert!(
                matches!(
                    action,
                    Action::Leave | Action::Retire { .. } | Action::AwaitReceipt { .. }
                ),
                "{tag}: {action:?} is left to do: {said:?}"
            ),
        }
    }
}

/// RED (U-29b) — **the new bundle, live and not committed, is never started
/// plainly**: at `Stuck`'s bound with `open` refusing, while a running copy
/// keeps the rollback waiting, after the applier's own swap back failed, and
/// when the recovery cannot read the journal's body — every start of it
/// carries the trial's words.
///
/// The coordinator's rulings 2 and 3: "never the new build as an ordinary
/// start before `Committed`"; "`Stuck` with the new bundle live starts the
/// new build as a trial (rule 2), not plainly". U-29's report, open point 1:
/// `Stuck` relaunched whatever was live, the untrialled new build included,
/// its writers not held back.
///
/// MUTATION: in `opens_now`, answer `Opens::Installed { failed }` whatever
/// is live.
#[test]
fn a_new_live_bundle_is_never_started_plainly_before_committed() {
    if !on_macos() {
        return;
    }
    let is_trial =
        |words: &[OsString]| words.first() == Some(&OsString::from(cli::UPDATE_TRIAL_FLAG));
    let no_open = || -> LaunchHook { Box::new(|_, _| Err(io::Error::other("no open (test)"))) };

    // `Stuck` at its bound: nothing is tried, and `open` refuses the trial
    // this run would record — the door's own start is one.
    let install = Install::new("plain-bound");
    install.exchanged();
    install.write(stuck(STUCK_ATTEMPT_LIMIT));
    install.arm();
    let hands = Fake {
        on_launch: Some(no_open()),
        ..Fake::default()
    };
    let opened = start_then_recover(&install, &["--tab"], hands, limits(5_000, 1_000));
    assert_eq!(opened.launches(), 2, "the refused trial, then the door's");
    assert_eq!(opened.started.len(), 1, "{:?}", opened.hands.said);
    assert!(is_trial(&opened.started[0].1), "{:?}", opened.started);
    assert_eq!(opened.hands.exchanges, 0);

    // A running copy keeps the admission: the swap back waits.
    let install = Install::new("plain-waits");
    install.exchanged();
    install.write(Phase::RollbackIntent { trial: None });
    install.arm();
    std::fs::write(install.home.admission(), b"").unwrap();
    let copy = install_txn::try_hold(&install.home.admission(), Hold::Shared)
        .unwrap()
        .unwrap();
    let opened = start_then_recover(&install, &["--tab"], Fake::default(), limits(500, 1_000));
    drop(copy);
    assert!(
        opened
            .hands
            .said
            .iter()
            .any(|line| line.contains("RollbackWaits")),
        "{:?}",
        opened.hands.said
    );
    assert_eq!(opened.launches(), 1);
    assert!(is_trial(&opened.started[0].1), "{:?}", opened.started);
    assert_eq!(version_of(&install.installed), "2.0");

    // The applier itself: no receipt, and its swap back refused.
    let install = Install::new("plain-applier");
    let world = Fake {
        refuse_from: 1,
        ..Fake::default()
    };
    let (ended, world) = applied(install.road(limits(5_000, 800)), world);
    assert!(matches!(ended, Ended::Stuck(_)), "{ended:?}");
    // The stand-in `open` starts nothing, so no trial is found running and
    // the exit guard starts one more — as a trial (U-34), never plainly.
    assert!(
        world.relaunched.iter().all(|(_, words)| is_trial(words)),
        "{:?}",
        world.relaunched
    );
    let launched = world.launched.lock().unwrap().clone();
    assert_eq!(launched.len(), 2);
    assert!(launched.iter().all(|words| is_trial(words)), "{launched:?}");

    // A body this build cannot read: which bundle is live cannot be told,
    // and a trial is safe for either.
    let install = Install::new("plain-unread");
    install.exchanged();
    install.arm();
    let bytes = String::from_utf8(install.journal_at(Phase::Moving).encode()).unwrap();
    let unread = bytes.replace("\"layout\":\"Bundle\"", "\"layout\":\"Later\"");
    assert_ne!(unread, bytes);
    install_txn::durable_write(&install.home.journal(), unread.as_bytes()).unwrap();
    let opened = start_then_recover(&install, &["--tab"], Fake::default(), limits(5_000, 1_000));
    assert_eq!(opened.launches(), 1);
    assert!(is_trial(&opened.started[0].1), "{:?}", opened.started);
}

/// RED (U-29b) — **a receipt that recovery finds commits the transaction
/// forward** — the trial's own, after the applier died with it on disk; and
/// the one of a trial started over a `Stuck` whose new bundle is live — then
/// the old bundle, the entrance and `H/<txn>` go (W8/M8); a receipt of
/// another trial commits nothing and the rollback follows.
///
/// The coordinator's rulings 1 and 3: "`Trial` without a receipt … a receipt
/// with the matching nonce → `Committed` and the W8 cleanup"; "if that trial
/// then produces a receipt, the lock holder writes `Committed` (the
/// transaction recovers forward, W8/M8)". On BASE `Trial` was not recovery's,
/// and a receipt in `Stuck` was refused by rule.
///
/// MUTATION: in `update_txn::decide`, drop the `Commit` arm of a `Stuck`
/// with a retrial.
#[test]
fn a_receipt_found_by_recovery_commits_forward() {
    if !on_macos() {
        return;
    }
    let install = Install::new("forward-trial");
    let children = Children::default();
    a_recorded_trial(&install, &children, false, true);
    let opened = start_then_recover(&install, &["--tab"], Fake::default(), limits(5_000, 1_000));
    let said = &opened.hands.said;
    assert!(
        said.iter()
            .any(|line| line.contains("wrote [Committed, Retired]")),
        "{said:?}"
    );
    assert_eq!(version_of(&install.installed), "2.0");
    assert!(
        !install.home.transaction(install.txn).exists(),
        "the old bundle and H/<txn>"
    );
    assert!(!install.plist().exists());
    let bytes = std::fs::read(install.home.journal()).unwrap();
    assert_eq!(
        crate::update_txn::trial_sight(Some(&bytes), &install.txn),
        TrialSight::Committed,
        "a trial still watching is released"
    );

    // Another trial's receipt: nothing is committed on it.
    let install = Install::new("forward-other");
    let children = Children::default();
    a_recorded_trial(&install, &children, false, false);
    let Phase::Trial { nonce, process, .. } = install.on_disk().unwrap().body.phase else {
        panic!("a trial");
    };
    install.receipt(nonce, Nonce::new([0x99; 32]), process.pid);
    let opened = start_then_recover(&install, &["--tab"], Fake::default(), limits(5_000, 1_000));
    assert_eq!(
        version_of(&install.installed),
        "1.0",
        "{:?}",
        opened.hands.said
    );
    assert_eq!(
        install.on_disk().unwrap().body.phase,
        Phase::Retired {
            outcome: Outcome::RolledBack,
            untried: false,
        }
    );

    // The trial started over `Stuck`, found answered by the next holder.
    let install = Install::new("forward-stuck");
    install.exchanged();
    install.arm();
    let children = Children::default();
    let pid = children.start(&install.installed, "trial");
    std::thread::sleep(Duration::from_millis(200));
    let process = recorded(&install.installed.join(EXE), pid);
    let retrial = Nonce::new([0x72; 32]);
    let stuck = install
        .journal_at(stuck(STUCK_ATTEMPT_LIMIT))
        .advance(&Event::RetrialBegan {
            nonce: retrial,
            process,
            began_ms: now_ms(),
        })
        .unwrap();
    install_txn::durable_write(&install.home.journal(), &stuck.encode()).unwrap();
    install.receipt(retrial, retrial, pid);
    // It answered and quit before the next holder came: only its receipt is
    // left to go on.
    children.end(pid);
    assert!(children.ended(pid).is_some());
    let opened = start_then_recover(&install, &["--tab"], Fake::default(), limits(5_000, 1_000));
    let said = &opened.hands.said;
    assert!(
        said.iter()
            .any(|line| line.contains("wrote [Committed, Retired]")),
        "{said:?}"
    );
    assert_eq!(opened.hands.exchanges, 0, "never swapped back");
    assert_eq!(version_of(&install.installed), "2.0");
    assert_eq!(
        opened.started,
        vec![(install.installed.join(EXE), vec![OsString::from("--tab")])],
        "the committed build, plainly"
    );
}

/// RED (U-29b) — **a recovery that fails still opens Folio, and the start it
/// makes says *Update incomplete.* and names the folder**: a transaction
/// lock that cannot be opened (the old build live: it is started with
/// `--update-failed`), and a journal body it cannot read (a trial, with the
/// same card); the journal is kept.
///
/// The coordinator's ruling 2: "If recovery itself fails (any error road),
/// the bundle that is live is started under that same rule with
/// `--update-failed <journal>` so the card says *Update incomplete* and names
/// the folder; the journal is kept." The start the door makes is run through
/// the command line's own parser and the ordinary start's pass.
///
/// MUTATION: in `recover`, answer `waiting: false` for a start's recovery
/// that failed (the exit guard then starts nothing).
#[test]
fn recovery_failure_still_opens_with_the_incomplete_card() {
    if !on_macos() {
        return;
    }
    let card_of = |install: &Install, words: &[OsString]| {
        let request = cli::parse(words.to_vec()).expect("the words parse");
        let exe = install.installed.join(EXE);
        let mut starting = Starting {
            agents: install.agents.clone(),
            said: Vec::new(),
            spawned: Vec::new(),
        };
        let verdict = crate::update_startup::run(
            &crate::update_startup::Start {
                own_exe: &exe,
                home: &install.home,
                argv: words,
                trial: request.update_trial.as_ref(),
                failed: request.update_failed.as_deref(),
            },
            &mut starting,
        );
        let crate::update_startup::Verdict::Continue { failed, trial, .. } = verdict else {
            panic!("the start it makes goes on: {:?}", starting.said);
        };
        (failed, trial)
    };
    let incomplete = |install: &Install| crate::update_job::Failure::Incomplete {
        folder: install.home.root().to_path_buf(),
    };

    // A folder where the transaction lock should be: the lock cannot be
    // opened, so nothing can be recorded.
    let install = Install::new("fail-unlockable");
    let journal = std::fs::read(install.home.journal()).unwrap();
    std::fs::create_dir(install.home.lock()).unwrap();
    let opened = start_then_recover(&install, &["--tab"], Fake::default(), limits(5_000, 1_000));
    assert!(
        opened.hands.said.iter().any(|line| line.contains("Failed")),
        "{:?}",
        opened.hands.said
    );
    assert_eq!(opened.launches(), 1);
    let (program, words) = &opened.started[0];
    assert_eq!(program, &install.installed.join(EXE));
    assert_eq!(*words, failed_then(&install, &["--tab"]));
    assert_eq!(
        std::fs::read(install.home.journal()).unwrap(),
        journal,
        "kept"
    );
    let (card, trial) = card_of(&install, words);
    assert_eq!(card, Some(incomplete(&install)));
    assert_eq!(trial, None, "the old build, plainly");

    let install = Install::new("fail-unread");
    let bytes = String::from_utf8(install.journal_at(Phase::Armed).encode()).unwrap();
    let unread = bytes.replace("\"layout\":\"Bundle\"", "\"layout\":\"Later\"");
    install_txn::durable_write(&install.home.journal(), unread.as_bytes()).unwrap();
    let opened = start_then_recover(&install, &["--tab"], Fake::default(), limits(5_000, 1_000));
    assert!(
        opened
            .hands
            .said
            .iter()
            .any(|line| line.contains("Refused")),
        "{:?}",
        opened.hands.said
    );
    assert_eq!(opened.launches(), 1);
    let words = &opened.started[0].1;
    assert_eq!(
        std::fs::read(install.home.journal()).unwrap(),
        unread.as_bytes()
    );
    let (card, trial) = card_of(&install, words);
    assert_eq!(card, Some(incomplete(&install)));
    assert_eq!(trial.map(|(txn, _)| txn), Some(install.txn), "held back");
}

/// RED (U-29b) — **`Stuck`'s bound still holds when the new bundle is live**:
/// at three failed rollbacks nothing is swapped back again, however many
/// starts come; each start has the new build started as a trial over it
/// (recorded, so its receipt could still commit forward), and the count
/// stays where it was.
///
/// The coordinator's ruling 3 with U-29's ruling 4: the trial started over
/// `Stuck` is not a rollback attempt, and it gives the bound no way round.
///
/// MUTATION: in `update_txn::next`, let `RetrialBegan` record `attempts: 0`.
#[test]
fn the_stuck_bound_still_holds_when_the_new_bundle_is_live() {
    if !on_macos() {
        return;
    }
    let install = Install::new("bound-new-live");
    install.exchanged();
    install.write(stuck(STUCK_ATTEMPT_LIMIT));
    install.arm();
    let children = Children::default();
    let silent = |children: &Children| -> LaunchHook {
        let children = children.clone();
        Box::new(move |bundle, _| {
            children.start(bundle, "trial");
            Ok(())
        })
    };
    let attempts_now = |install: &Install| match install.on_disk().unwrap().body.phase {
        Phase::Stuck { attempts, .. } => attempts,
        other => panic!("{other:?}"),
    };
    for run in 0..3 {
        if run == 2 {
            // The trials the first run started are gone before the third.
            for pid in children.started.lock().unwrap().clone() {
                children.end(pid);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let hands = launching(silent(&children));
        let opened = start_then_recover(&install, &["--tab"], hands, limits(5_000, 1_000));
        let said = &opened.hands.said;
        assert_eq!(
            opened.hands.exchanges, 0,
            "run {run}: never swapped back: {said:?}"
        );
        assert_eq!(attempts_now(&install), STUCK_ATTEMPT_LIMIT, "run {run}");
        assert_eq!(opened.launches(), 1, "run {run}: {said:?}");
        let words = opened
            .trials
            .first()
            .or_else(|| opened.started.first().map(|(_, words)| words))
            .unwrap();
        assert_eq!(
            words[0],
            OsString::from(cli::UPDATE_TRIAL_FLAG),
            "run {run}"
        );
        assert_eq!(version_of(&install.installed), "2.0");
        assert!(install.plist().exists(), "run {run}: the entrance is kept");
    }
    let Phase::Stuck { retrial, .. } = install.on_disk().unwrap().body.phase else {
        panic!("still Stuck");
    };
    assert!(retrial.is_some(), "the trial over Stuck is recorded");
}

// ── a journal write that fails (U-34) ───────────────────────────────────────

/// **The home made unwritable** — no new file can be made in it, so every
/// journal write fails at its temporary (the macOS form of a refused write:
/// a rename here is never refused for an open target) — and the permissions
/// it had, to put back.
fn unwritable(home: &Home) -> std::fs::Permissions {
    let before = std::fs::metadata(home.root()).unwrap().permissions();
    let mut shut = before.clone();
    shut.set_readonly(true);
    std::fs::set_permissions(home.root(), shut).unwrap();
    before
}

/// RED (U-34) — **an applier whose road fails still opens Folio: the live
/// bundle with `--update-failed <journal>`, the journal and the LaunchAgent
/// kept for the next start or login** — a transaction lock that cannot be
/// opened, and a journal write that fails at `Armed` → `Moving`.
///
/// The owner's ruling of 2026-09-25 (every phase opens Folio), made the
/// applier's by U-34: a failed road owes what a person's start would, read
/// from the disk — here the old bundle live under a `destructive` header, so
/// the *Update incomplete.* card. On Windows the same write is asked again
/// while another program holds the journal open; a macOS rename never is.
///
/// MUTATION: in `update_apply_macos::apply`, tell the guard nobody is waiting
/// after `Ended::Failed` (U-23's "a failed applier owes no window").
#[test]
fn a_failed_applier_still_opens_the_live_bundle_with_the_incomplete_card() {
    if !on_macos() {
        return;
    }
    let install = Install::new("fail-unlockable");
    let journal = std::fs::read(install.home.journal()).unwrap();
    std::fs::create_dir(install.home.lock()).unwrap();
    let (ended, world) = applied(install.road(limits(1_000, 5_000)), Fake::default());
    assert!(matches!(ended, Ended::Failed(_)), "{ended:?}");
    assert_eq!(std::fs::read(install.home.journal()).unwrap(), journal);
    assert_eq!(
        world.relaunched,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "{:?}",
        world.said
    );

    let install = Install::new("fail-write");
    install.write(Phase::Armed);
    install.arm();
    std::fs::write(install.home.lock(), b"").unwrap();
    std::fs::write(install.home.admission(), b"").unwrap();
    let before = unwritable(&install.home);
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), Fake::default());
    std::fs::set_permissions(install.home.root(), before).unwrap();
    assert!(
        matches!(ended, Ended::Failed(_)),
        "{ended:?}: {:?}",
        world.said
    );
    assert_eq!(install.on_disk().unwrap().body.phase, Phase::Armed);
    assert!(install.plist().exists(), "the LaunchAgent is kept");
    assert_eq!(version_of(&install.installed), "1.0", "nothing exchanged");
    assert_eq!(
        world.relaunched,
        vec![(install.installed.clone(), failed_then(&install, &[]))],
        "{:?}",
        world.said
    );
}

/// RED (U-34) — **a trial whose start the journal cannot record is ended
/// before anything else happens, and the road goes on as for a trial that did
/// not start: `RollbackIntent`, the swap back, and the old bundle with
/// `--update-failed`; `Trial` is never written and the trial does not run
/// on.**
///
/// The trial is found only once it runs (LaunchServices reports no pid), so
/// `TrialBegan` cannot be durable before the start without a new phase; the
/// rights table gives the lock holder `EndTrial` over `Moving` for this one
/// case. The home is made unwritable from the trial's launch until the
/// applier says the trial could not be recorded.
///
/// MUTATION: in `Txn::watch_trial`, answer `Watched::Unrecorded` with
/// `Err(why)` whatever the phase (the trial runs on, unrecorded).
#[test]
fn a_trial_whose_start_cannot_be_recorded_is_ended_and_swapped_back() {
    if !on_macos() {
        return;
    }
    let install = Install::new("unrecorded");
    let children = Children::default();
    let started = children.clone();
    let root = install.home.root().to_path_buf();
    let shut: Arc<Mutex<Option<std::fs::Permissions>>> = Arc::default();
    let at_launch = Arc::clone(&shut);
    let mut world = launching(Box::new(move |bundle, _| {
        started.start(bundle, "trial");
        let before = std::fs::metadata(&root).unwrap().permissions();
        let mut closed = before.clone();
        closed.set_readonly(true);
        std::fs::set_permissions(&root, closed).unwrap();
        *at_launch.lock().unwrap() = Some(before);
        Ok(())
    }));
    let root = install.home.root().to_path_buf();
    let at_say = Arc::clone(&shut);
    world.on_say = Some(Box::new(move |line| {
        if line.contains("could not be recorded")
            && let Some(before) = at_say.lock().unwrap().take()
        {
            std::fs::set_permissions(&root, before).unwrap();
        }
    }));
    let (ended, world) = applied(install.road(limits(5_000, 20_000)), world);
    if let Some(before) = shut.lock().unwrap().take() {
        std::fs::set_permissions(install.home.root(), before).unwrap();
    }
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        world
            .wrote()
            .contains("[Armed, Moving, RollbackIntent, RolledBack, Retired]"),
        "{}",
        world.wrote()
    );
    let unrecorded = world
        .said
        .iter()
        .position(|line| line.contains("could not be recorded"))
        .expect("said");
    let asked = world
        .said
        .iter()
        .position(|line| line.contains("is asked to quit"))
        .expect("the trial is stopped");
    assert!(unrecorded < asked, "{:?}", world.said);
    assert_eq!(
        version_of(&install.installed),
        "1.0",
        "the old bundle is back"
    );
    let trial = children.started.lock().unwrap()[0];
    assert!(children.ended(trial).is_some(), "the trial does not run on");
    assert_eq!(
        world.relaunched,
        vec![(install.installed.clone(), failed_then(&install, &[]))]
    );
}

/// TWIN (U-32; the Windows checklist's E-7 and W14) — **what another program
/// holds open never stops the macOS exchange: a file inside the installed
/// bundle and the journal itself, each held open for the whole road, and the
/// update commits.**
///
/// On Windows a file held without sharing refuses its move (E-7: nothing
/// moves, `Prepared`, the old build reopened) and a journal held without
/// delete sharing refuses the rename over it (W14: asked again for 2 s). On
/// macOS neither exists: the exchange is one `renamex_np(RENAME_SWAP)` of the
/// two bundle directories, which an open file inside either does not stop,
/// and a `rename(2)` replaces a target another process has open
/// (`install_txn::Failure::refused_while_open` is `false` there). The held
/// descriptor of the old `Info.plist` still reads the old bytes afterwards —
/// the file moved to `stage/` and was removed only with it.
///
/// No MUTATION: this is the twin the addendum asks to be checked, and it
/// holds on BASE.
#[test]
fn what_another_program_holds_open_never_stops_the_macos_exchange() {
    use std::io::Read;
    if !on_macos() {
        return;
    }
    let install = Install::new("held-open");
    let plist = install.installed.join("Contents").join("Info.plist");
    let before = std::fs::read(&plist).unwrap();
    let mut held_plist = std::fs::File::open(&plist).unwrap();
    let held_journal = std::fs::File::open(install.home.journal()).unwrap();
    let children = Children::default();
    let world = launching(a_healthy_trial(&install, &children, None));
    let (ended, world) = applied(install.road(limits(5_000, 5_000)), world);
    assert_eq!(ended, Ended::Committed, "{:?}", world.said);
    assert!(
        world
            .wrote()
            .contains("[Armed, Moving, Trial, Committed, Retired]"),
        "{}",
        world.wrote()
    );
    assert_eq!(version_of(&install.installed), "2.0");
    let mut still = Vec::new();
    held_plist.read_to_end(&mut still).unwrap();
    assert_eq!(
        still, before,
        "the held file is the old one, still readable"
    );
    drop(held_journal);
}

/// TWIN (U-32; the Windows checklist's D-14) — **a home that stays unwritable
/// from the trial's launch to the end of the road leaves the new build
/// running as a trial with the *Update incomplete.* card, the journal at
/// `Moving` and the LaunchAgent armed for the next start or login.**
///
/// D-14 on Windows: the journal held for 120 s right after the moves; the
/// trial could not be recorded, was asked to quit, and the road ended
/// `Failed` with the new build started again as a trial. The macOS road does
/// the same, by the same rules: `TrialBegan` cannot be written, the trial is
/// ended (U-34), `RollbackDeclared` cannot be written either, so the road
/// ends `Failed`, and its exit guard starts what the disk names — the new
/// bundle is live and nothing is committed, so it is started only as a trial
/// (U-29b), with `--update-failed` because the header is `destructive`. The
/// reader is told the update is incomplete and where; the trial's writes are
/// held back; the next start or login finds `Moving` with the new bundle live
/// and decides it by a trial again (M6).
///
/// No MUTATION of its own: the rules it exercises are pinned by
/// `a_trial_whose_start_cannot_be_recorded_is_ended_and_swapped_back` and
/// `a_new_live_bundle_is_never_started_plainly_before_committed`; this is
/// the twin the addendum asks to be checked.
#[test]
fn a_home_unwritable_after_the_exchange_leaves_the_new_build_as_a_trial_with_the_card() {
    if !on_macos() {
        return;
    }
    let install = Install::new("unwritable-long");
    let children = Children::default();
    let started = children.clone();
    let root = install.home.root().to_path_buf();
    let shut: Arc<Mutex<Option<std::fs::Permissions>>> = Arc::default();
    let at_launch = Arc::clone(&shut);
    let world = launching(Box::new(move |bundle, _| {
        started.start(bundle, "trial");
        let before = std::fs::metadata(&root).unwrap().permissions();
        let mut closed = before.clone();
        closed.set_readonly(true);
        std::fs::set_permissions(&root, closed).unwrap();
        *at_launch.lock().unwrap() = Some(before);
        Ok(())
    }));
    let (ended, world) = applied(install.road(limits(5_000, 20_000)), world);
    if let Some(before) = shut.lock().unwrap().take() {
        std::fs::set_permissions(install.home.root(), before).unwrap();
    }
    assert!(
        matches!(ended, Ended::Failed(_)),
        "{ended:?}: {:?}",
        world.said
    );
    assert_eq!(install.on_disk().unwrap().body.phase, Phase::Moving);
    assert!(install.plist().exists(), "the LaunchAgent is kept");
    assert_eq!(
        version_of(&install.installed),
        "2.0",
        "the new bundle is live"
    );
    let trial = children.started.lock().unwrap()[0];
    assert!(
        children.ended(trial).is_some(),
        "the unrecorded trial is ended"
    );
    let [(bundle, words)] = world.relaunched.as_slice() else {
        panic!("one start on the way out: {:?}", world.relaunched);
    };
    assert_eq!(bundle, &install.installed);
    assert_eq!(words.len(), 5, "{words:?}");
    assert_eq!(words[0], OsString::from(cli::UPDATE_TRIAL_FLAG));
    assert_eq!(words[1], OsString::from(install.txn.to_string()));
    assert_eq!(&words[3..], failed_then(&install, &[]).as_slice());
}

// ── the trial's gaps (U-37, U-38) ───────────────────────────────────────────

/// **The macOS D14 shape through the road** (the rehearsal's D14, first half):
/// the home made unwritable at the trial's launch and kept so — `TrialBegan`
/// refused, the trial ended, `RollbackDeclared` refused — so the applier ends
/// `Failed` at `Moving` and its exit guard starts the new bundle again as a
/// trial with a nonce no journal records (the stand-in records the start).
/// That second trial is then made as the product makes it: a process of the
/// installed executable and its receipt at its own nonce. The home is
/// writable again. What is returned: the installation, the processes, the
/// second trial and the applier's world.
fn failed_with_the_home_shut(tag: &str) -> (Install, Children, TrialProcess, Fake) {
    let install = Install::new(tag);
    let children = Children::default();
    let started = children.clone();
    let root = install.home.root().to_path_buf();
    let shut: Arc<Mutex<Option<std::fs::Permissions>>> = Arc::default();
    let at_launch = Arc::clone(&shut);
    let world = launching(Box::new(move |bundle, _| {
        started.start(bundle, "trial");
        let before = std::fs::metadata(&root).unwrap().permissions();
        let mut closed = before.clone();
        closed.set_readonly(true);
        std::fs::set_permissions(&root, closed).unwrap();
        *at_launch.lock().unwrap() = Some(before);
        Ok(())
    }));
    let (ended, world) = applied(install.road(limits(5_000, 20_000)), world);
    if let Some(before) = shut.lock().unwrap().take() {
        std::fs::set_permissions(install.home.root(), before).unwrap();
    }
    assert!(
        matches!(ended, Ended::Failed(_)),
        "{ended:?}: {:?}",
        world.said
    );
    assert!(said(&world, "is asked to quit"), "{:?}", world.said);
    assert_eq!(install.on_disk().unwrap().body.phase, Phase::Moving);
    let [(bundle, words)] = world.relaunched.as_slice() else {
        panic!("one start on the way out: {:?}", world.relaunched);
    };
    assert_eq!(bundle, &install.installed);
    let nonce = trial_nonce(words);
    let pid = children.start(&install.installed, "trial");
    install.receipt(nonce, nonce, pid);
    let second = recorded(&install.installed.join(EXE), pid);
    (install, children, second, world)
}

/// RED (U-37) — **a trial whose start the journal could not record, running
/// with its receipt written, is recorded by the next lock holder and its
/// receipt commits the update: `[Trial, Committed, Retired]`, the new bundle
/// kept, the LaunchAgent removed, and that trial left running as the
/// window.**
///
/// The rehearsal's D14 (round 4): the trial 56947 could not be recorded and
/// was ended; `RollbackDeclared` failed on the same unwritable home; the
/// guard started 56955 as a trial no journal records, which nothing would
/// ever commit or end. The recovery build — here as the LaunchAgent starts it
/// at login, which is also the line the trial's own watchdog starts it with —
/// now records such a trial.
///
/// MUTATION: in `update_apply::before_deciding`, answer `Decide` for
/// `Survey::Adoptable` (never record it).
#[test]
fn a_running_trial_the_journal_could_not_record_is_recorded_and_committed_by_the_next_holder() {
    if !on_macos() {
        return;
    }
    let (install, children, second, _applier) = failed_with_the_home_shut("d14");
    let (ended, world) = recovered(install.recovery(limits(5_000, 5_000)), Fake::default());
    assert_eq!(ended, Some(Ended::Committed), "{:?}", world.said);
    assert!(
        world.wrote().ends_with("[Trial, Committed, Retired]"),
        "{}",
        world.wrote()
    );
    assert_eq!(version_of(&install.installed), "2.0", "the new bundle");
    assert!(!install.plist().exists(), "the LaunchAgent is removed");
    assert!(
        children.status(second.pid).is_none(),
        "the trial runs on as the window"
    );
    assert!(world.launched.lock().unwrap().is_empty(), "no second trial");
}

/// RED (U-37; the rehearsal's defect 9) — **a person's start while that
/// unrecorded trial runs commits the update and starts nothing beside it**:
/// the recovery the start hands itself to records the running trial, whose
/// receipt commits, and its exit guard leaves that trial as the window.
///
/// The rehearsal's D14, second half: `open -n` while 56955 was open; the
/// recovery recorded a second trial (58221), which waited for the data
/// directory 56955 held and left unanswered, and the recovery declared the
/// rollback of a healthy 0.4.7 — `RollbackIntent`, `RollbackWaits`.
///
/// MUTATION: in `update_apply::before_deciding`, answer `Decide` for
/// `Survey::Adoptable` (never record it).
#[test]
fn a_start_beside_an_unrecorded_trial_commits_it_and_starts_nothing_more() {
    if !on_macos() {
        return;
    }
    let (install, children, second, _applier) = failed_with_the_home_shut("defect9");
    let (code, world) = recover_door(&install, Vec::new(), Fake::default());
    assert_eq!(code, 0, "{:?}", world.said);
    assert!(
        world.wrote().ends_with("[Trial, Committed, Retired]"),
        "{}",
        world.wrote()
    );
    assert!(children.status(second.pid).is_none(), "the trial runs on");
    assert!(world.launched.lock().unwrap().is_empty(), "no second trial");
    assert!(world.relaunched.is_empty(), "{:?}", world.relaunched);
}

/// RED (U-38; the rehearsal's defect 2) — **a trial that ends before the
/// process list shows it ends the applier's wait at once**: its launch —
/// `open -W`, which returns when the application it opened ends — is over
/// and nothing was seen, so the road goes on as for a trial that gave no
/// receipt, well inside its deadline.
///
/// The rehearsal's M9cut and M11-rolledback: the trial killed before it was
/// seen, and 89.8 s without a window before `no receipt by the trial's
/// deadline`. The stand-in for `open -W` here is a program that returns at
/// once, and nothing of the new bundle ever runs.
///
/// MUTATION: in `Txn::watch_trial`, pass `&mut || false` as `launch_over`.
#[test]
fn a_trial_that_ends_before_it_is_seen_ends_the_wait_at_once() {
    if !on_macos() {
        return;
    }
    let install = Install::new("unseen");
    let world = Fake {
        on_launch_held: Some(Box::new(|_, _| {
            bt_platform::quiet_command("/usr/bin/true")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        })),
        ..Fake::default()
    };
    let began = Instant::now();
    let (ended, world) = applied(install.road(limits(5_000, 30_000)), world);
    let took = began.elapsed();
    assert_eq!(ended, Ended::RolledBack, "{:?}", world.said);
    assert!(
        said(&world, "ended before it could be seen"),
        "{:?}",
        world.said
    );
    assert!(took < Duration::from_secs(15), "at once: {took:?}");
    assert_eq!(version_of(&install.installed), "1.0", "swapped back");
}

// ── design revision (h): deferral, the hand-back and the launch (U-37) ───────

/// **The exchange done, the entrance armed, the journal at `Moving`**: the
/// new bundle live, the old one in `stage/`, no trial recorded.
fn exchanged_at_moving(tag: &str) -> Install {
    let install = Install::new(tag);
    install.arm();
    install.exchanged();
    install.write(Phase::Moving);
    assert_eq!(version_of(&install.installed), "2.0");
    install
}

/// A process of the installed (new) bundle's executable, running, recorded
/// by its pid and start instant.
fn a_candidate(install: &Install, children: &Children) -> TrialProcess {
    let pid = children.start(&install.installed, "trial");
    recorded(&install.installed.join(EXE), pid)
}

/// RED (U-37, H.3 step 3; Codex's check of (h), blocker 2, sequence A) — **a
/// live process of the new bundle that no receipt names, even one that has
/// not taken the data directory yet, defers the recovery: no trial launched
/// beside it, nothing recorded or swapped; a person's start leaves it as the
/// window; once it has gone, the recovery begins its own recorded trial.**
///
/// The rehearsal's defect 9 was this shape: a second trial beside the first.
///
/// MUTATION: in `update_apply::before_deciding`, answer `Decide` for
/// `Survey::Candidates`.
#[test]
fn a_candidate_that_has_not_claimed_the_data_directory_yet_defers_the_recovery() {
    if !on_macos() {
        return;
    }
    let install = exchanged_at_moving("pre-claim");
    let children = Children::default();
    let candidate = a_candidate(&install, &children);
    let (ended, world) = recovered(install.recovery(limits(5_000, 3_000)), Fake::default());
    assert!(
        matches!(ended, Some(Ended::Deferred(_))),
        "{ended:?}: {:?}",
        world.said
    );
    assert_eq!(install.on_disk().unwrap().body.phase, Phase::Moving);
    assert!(
        world.launched.lock().unwrap().is_empty(),
        "no trial beside it"
    );
    assert_eq!(version_of(&install.installed), "2.0", "nothing swapped");

    let (_code, world) = recover_door(&install, Vec::new(), Fake::default());
    assert!(said(&world, "deferred"), "{:?}", world.said);
    assert!(world.relaunched.is_empty(), "the candidate is the window");
    assert!(children.status(candidate.pid).is_none());

    children.end(candidate.pid);
    assert!(children.ended(candidate.pid).is_some());
    let (_ended, world) = recovered(install.recovery(limits(5_000, 1_000)), Fake::default());
    assert!(!said(&world, "deferred"), "{:?}", world.said);
    assert_eq!(
        world.launched.lock().unwrap().len(),
        1,
        "its own recorded trial"
    );
}

/// RED (U-37, H.3 step 3; Codex's check of (h), blocker 2, sequence B) — **a
/// data directory whose claim cannot be asked about defers the recovery; at
/// login nothing is started; a person's start keeps U-34's delivery duty —
/// its start unacknowledged, it shows the failure window itself.**
///
/// The claim's name is squatted the platform's own way
/// (`trust_harness::squat_the_claim`); the stand-in's acknowledgement is
/// refused as a denied question refuses it (`claimed_within`, round 4).
///
/// MUTATION: in `update_apply::before_deciding`, take a claim refusal other
/// than `Held` for a free claim (answer `Decide`).
#[test]
fn a_claim_that_cannot_be_asked_about_defers_and_a_persons_start_is_still_delivered() {
    if !on_macos() {
        return;
    }
    let install = exchanged_at_moving("denied");
    let _squat = bt_platform::trust_harness::squat_the_claim(&install.data).unwrap();
    let (ended, world) = recovered(install.recovery(limits(5_000, 3_000)), Fake::default());
    assert!(
        matches!(ended, Some(Ended::Deferred(_))),
        "{ended:?}: {:?}",
        world.said
    );
    assert!(world.launched.lock().unwrap().is_empty());
    assert_eq!(install.on_disk().unwrap().body.phase, Phase::Moving);

    let world = Fake {
        starts_die: true,
        ..Fake::default()
    };
    let (_code, world) = recover_door(&install, args(&["--cwd", "/x"]), world);
    assert!(said(&world, "deferred"), "{:?}", world.said);
    assert_eq!(world.relaunched.len(), 1, "{:?}", world.said);
    assert!(
        world.relaunched[0].1.ends_with(&args(&["--cwd", "/x"])),
        "the handed line is carried"
    );
    assert_eq!(world.shown.len(), 1, "then the failure window, here");
    assert!(world.launched.lock().unwrap().is_empty());
}

/// RED (U-37, H.3 step 3) — **a transaction folder that cannot be listed,
/// while a process of the new bundle runs, defers the recovery.**
///
/// The folder is made unlistable (no read permission) but passable, so the
/// bundle in `stage/` is still found.
///
/// MUTATION: in `update_apply::survey`, answer `Survey::Nothing` when
/// `H/<txn>` cannot be listed.
#[test]
fn an_unlistable_transaction_folder_defers_the_recovery() {
    if !on_macos() {
        return;
    }
    let install = exchanged_at_moving("unlistable");
    let children = Children::default();
    let _candidate = a_candidate(&install, &children);
    let folder = install.home.transaction(install.txn);
    let before = std::fs::metadata(&folder).unwrap().permissions();
    // Write and pass, no read: listed by nobody, passed through by all.
    run("/bin/chmod", &[OsStr::new("300"), folder.as_os_str()]);
    let (ended, world) = recovered(install.recovery(limits(5_000, 3_000)), Fake::default());
    std::fs::set_permissions(&folder, before).unwrap();
    assert!(
        matches!(ended, Some(Ended::Deferred(_))),
        "{ended:?}: {:?}",
        world.said
    );
    assert!(said(&world, "cannot be read"), "{:?}", world.said);
    assert!(world.launched.lock().unwrap().is_empty());
    assert_eq!(install.on_disk().unwrap().body.phase, Phase::Moving);
}

/// RED (U-37, H.3) — **the recovery's own starter is no candidate**: with it
/// running from the installed bundle's executable and nothing else, the
/// recovery decides — over `Moving` with the new bundle live, its own
/// recorded trial.
///
/// MUTATION: in `Txn::before_deciding`, leave `road.starter` out of the
/// excluded processes.
#[test]
fn the_recoverys_own_starter_is_no_candidate() {
    if !on_macos() {
        return;
    }
    let install = exchanged_at_moving("starter");
    let children = Children::default();
    let starter = a_candidate(&install, &children);
    let mut road = install.recovery(limits(5_000, 1_000));
    road.starter = Some(Running {
        pid: starter.pid,
        started: starter.started,
    });
    let (_ended, world) = recovered(road, Fake::default());
    assert!(!said(&world, "deferred"), "{:?}", world.said);
    assert_eq!(
        world.launched.lock().unwrap().len(),
        1,
        "its own recorded trial"
    );
}

/// RED (U-37, H.4 rule A2, macOS) — **a trial hands its transaction back, with
/// the home and `--from-trial <pid>:<started>:<ready|unready>`, only to a
/// rescue clone whose `CFBundleShortVersionString` is at least the version
/// that knows the word.**
///
/// The synthetic rescue clone says 1.0; the threshold is set on either side
/// of it (the product's is 0.4.7).
///
/// MUTATION: in `update_trial::hand_back`, skip the version check.
#[test]
fn a_trial_hands_back_only_to_a_rescue_build_that_knows_the_word() {
    if !on_macos() {
        return;
    }
    let install = Install::new("hand-back");
    install.write(Phase::Moving);
    struct Recorded(Vec<(PathBuf, Vec<OsString>)>);
    impl crate::update_trial::Starter for Recorded {
        fn start(&mut self, program: &Path, line: &[OsString]) -> io::Result<u32> {
            self.0.push((program.to_path_buf(), line.to_vec()));
            Ok(std::process::id())
        }
    }
    for (since, handed) in [((2, 0, 0), false), ((1, 0, 0), true)] {
        let home = install.home.clone();
        let txn = install.txn;
        let recorded = on_a_worker(move |worker| {
            let mut recorded = Recorded(Vec::new());
            let _ = crate::update_trial::hand_back(worker, &home, txn, true, since, &mut recorded);
            recorded.0
        });
        if !handed {
            assert!(recorded.is_empty(), "{since:?}: {recorded:?}");
            continue;
        }
        let me = crate::update_apply::this_process();
        assert_eq!(recorded.len(), 1);
        assert_eq!(
            recorded[0].0,
            install.home.rescue_executable(install.txn).unwrap()
        );
        assert_eq!(
            recorded[0].1,
            vec![
                OsString::from(cli::UPDATE_RECOVER_FLAG),
                install.home.root().as_os_str().to_owned(),
                OsString::from(cli::FROM_TRIAL_FLAG),
                OsString::from(format!("{}:{}:ready", me.pid, me.started)),
            ]
        );
    }
}

/// RED (U-38; design revision (h) H.5 rule L1) — **the launch is over only
/// when `open` exited; an `open` ended by a signal is unknown — never over —
/// and the trial is then waited for as before.**
///
/// A launch killed from outside before LaunchServices showed the application
/// said nothing about the application; taking it for its end began a rollback
/// beside a trial that might still appear.
///
/// MUTATION: in `Launch::over`, answer `true` for any status.
#[test]
fn a_launch_ended_by_a_signal_is_unknown_not_over() {
    if !on_macos() {
        return;
    }
    let exited = bt_platform::quiet_command("/usr/bin/false")
        .spawn()
        .unwrap();
    let mut launch = Launch::of(exited);
    let give_up = Instant::now() + Duration::from_secs(10);
    while !launch.over() {
        assert!(Instant::now() < give_up, "an exit is over");
        std::thread::sleep(Duration::from_millis(10));
    }

    let helper = bt_platform::quiet_command("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = helper.id();
    let mut launch = Launch::of(helper);
    assert!(!launch.over(), "still running");
    let _ = bt_platform::quiet_command("/bin/kill")
        .arg("-9")
        .arg(pid.to_string())
        .status();
    std::thread::sleep(Duration::from_millis(300));
    for _ in 0..5 {
        assert!(!launch.over(), "a signal is unknown");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The words of a command line.
fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

// ── design revision (h), round 5 (Codex's review of 153f7cb2) ───────────────

/// A receipt at `nonce`'s name carrying exactly `receipt`'s fields.
fn receipt_as(install: &Install, nonce: Nonce, receipt: &Receipt) {
    install_txn::durable_create(
        &install.home.receipt_path(install.txn, &nonce),
        &receipt.encode(),
    )
    .unwrap();
}

/// RED (U-37, design revision (h) H.1 R3–R4, on macOS) — **a running process
/// of the new bundle is adopted only when a receipt names it exactly — pid and
/// start instant; a reused pid at another instant, a receipt without
/// `started`, another transaction's receipt and several receipts none of
/// which names it exactly adopt nothing and the recovery defers; the exact one
/// among several commits.**
///
/// MUTATION: in `update_apply::survey`, match a receipt to a running process
/// by its pid alone.
#[test]
fn adoption_needs_a_receipt_naming_the_exact_process() {
    if !on_macos() {
        return;
    }
    let receipt = |install: &Install, nonce: Nonce, txn: TxnId, pid: u32, started: Option<u64>| {
        receipt_as(
            install,
            nonce,
            &Receipt {
                txn,
                nonce,
                pid,
                version: "2.0".to_owned(),
                started,
            },
        );
    };
    for case in [
        "reused",
        "no-started",
        "other-txn",
        "several-none",
        "several-exact",
    ] {
        let install = exchanged_at_moving(&format!("exact-{case}"));
        let children = Children::default();
        let running = a_candidate(&install, &children);
        let other = TxnId::new([0x7e; 16]);
        let (a, b) = (Nonce::new([0x61; 32]), Nonce::new([0x62; 32]));
        match case {
            "reused" => receipt(
                &install,
                a,
                install.txn,
                running.pid,
                Some(running.started - 1),
            ),
            "no-started" => receipt(&install, a, install.txn, running.pid, None),
            "other-txn" => receipt(&install, a, other, running.pid, Some(running.started)),
            "several-none" => {
                receipt(
                    &install,
                    a,
                    install.txn,
                    running.pid,
                    Some(running.started - 1),
                );
                receipt(&install, b, install.txn, running.pid, None);
            }
            _ => {
                receipt(
                    &install,
                    a,
                    install.txn,
                    running.pid,
                    Some(running.started - 1),
                );
                receipt(&install, b, install.txn, running.pid, Some(running.started));
            }
        }
        let (ended, world) = recovered(install.recovery(limits(5_000, 3_000)), Fake::default());
        if case == "several-exact" {
            assert_eq!(ended, Some(Ended::Committed), "{case}: {:?}", world.said);
            assert!(
                children.status(running.pid).is_none(),
                "the adopted trial runs on"
            );
        } else {
            assert!(
                matches!(ended, Some(Ended::Deferred(_))),
                "{case}: {ended:?} {:?}",
                world.said
            );
            assert_eq!(
                install.on_disk().unwrap().body.phase,
                Phase::Moving,
                "{case}"
            );
        }
        assert!(world.launched.lock().unwrap().is_empty(), "{case}");
    }
}

/// The claim of the data directory, as a row of H.3's table wants it.
enum MacClaim {
    Held,
    Free,
    Denied,
}

/// **The row's world**: the claim held here or squatted, and `H/<txn>` made
/// unlistable (and put back when dropped).
struct MacRow {
    _held: Option<bt_platform::instance::DataDirectoryClaim>,
    _squat: Option<bt_platform::trust_harness::Squat>,
    shut: Option<PathBuf>,
}

impl Drop for MacRow {
    fn drop(&mut self) {
        if let Some(folder) = self.shut.take() {
            run("/bin/chmod", &[OsStr::new("755"), folder.as_os_str()]);
        }
    }
}

fn mac_row(install: &Install, claim: &MacClaim, unlistable: bool) -> MacRow {
    let (held, squat) = match claim {
        MacClaim::Held => (
            Some(crate::persist::try_claim(&install.data).expect("the claim, held here")),
            None,
        ),
        MacClaim::Free => (None, None),
        MacClaim::Denied => (
            None,
            Some(bt_platform::trust_harness::squat_the_claim(&install.data).unwrap()),
        ),
    };
    let shut = unlistable.then(|| {
        let folder = install.home.transaction(install.txn);
        run("/bin/chmod", &[OsStr::new("300"), folder.as_os_str()]);
        folder
    });
    MacRow {
        _held: held,
        _squat: squat,
        shut,
    }
}

/// RED (U-37, design revision (h) H.3, the nine-row table, on macOS) — **every
/// row of H.3's table ends as the table says, at login and for a person's
/// start** (see the Windows test of the same name): a candidate seen, or a
/// held claim, defers and nothing is started beside it; a denied claim
/// question or an unlistable folder with a free claim defers and a person's
/// start is delivered — here unacknowledged, then the failure window; only no
/// candidate with a free claim decides (over `Moving` with the new bundle
/// live, the recovery's own recorded trial).
///
/// MUTATION: in `update_apply::before_deciding`, ask the claim before the
/// candidates.
#[test]
fn the_nine_rows_of_h3_each_end_as_the_table_says() {
    if !on_macos() {
        return;
    }
    // (candidate seen, unlistable, claim, the deferral said, delivered)
    let rows: [(bool, bool, MacClaim, Option<&str>, bool); 9] = [
        (
            true,
            false,
            MacClaim::Held,
            Some("no receipt names it"),
            false,
        ),
        (
            true,
            false,
            MacClaim::Free,
            Some("no receipt names it"),
            false,
        ),
        (
            true,
            false,
            MacClaim::Denied,
            Some("no receipt names it"),
            false,
        ),
        (
            false,
            false,
            MacClaim::Held,
            Some("a Folio holds the data directory"),
            false,
        ),
        (false, false, MacClaim::Free, None, false),
        (
            false,
            false,
            MacClaim::Denied,
            Some("cannot be asked about"),
            true,
        ),
        (
            true,
            true,
            MacClaim::Held,
            Some("a Folio holds the data directory"),
            false,
        ),
        (true, true, MacClaim::Free, Some("cannot be read"), true),
        (true, true, MacClaim::Denied, Some("cannot be read"), true),
    ];
    for (at, (seen, unlistable, claim, deferral, delivered)) in rows.into_iter().enumerate() {
        let install = exchanged_at_moving(&format!("row{at}"));
        let children = Children::default();
        if seen {
            a_candidate(&install, &children);
        }
        let row = mac_row(&install, &claim, unlistable);
        let Some(deferral) = deferral else {
            let (_ended, world) =
                recovered(install.recovery(limits(5_000, 1_000)), Fake::default());
            drop(row);
            assert!(!said(&world, "deferred"), "row {at}: {:?}", world.said);
            assert_eq!(
                world.launched.lock().unwrap().len(),
                1,
                "row {at}: its own trial"
            );
            // The same cell for a person's start, on a second installation:
            // it decides by starting its own recorded trial (the handed line
            // after its words); that trial never answers, so the rollback
            // follows, and the exit guard delivers the old build with its card
            // and the handed line.
            let install = exchanged_at_moving(&format!("row{at}-person"));
            let (code, world) = recover_door(&install, args(&["--cwd", "/x"]), Fake::default());
            assert!(
                !said(&world, "deferred"),
                "row {at} (person): {:?}",
                world.said
            );
            let launched = world.launched.lock().unwrap().clone();
            assert_eq!(launched.len(), 1, "row {at} (person): its own trial");
            assert!(
                launched[0].ends_with(&args(&["--cwd", "/x"])),
                "row {at} (person): {launched:?}"
            );
            assert_eq!(code, 0, "row {at} (person): {:?}", world.said);
            assert_eq!(
                world.relaunched,
                vec![(
                    install.installed.join(EXE),
                    failed_then(&install, &["--cwd", "/x"])
                )],
                "row {at} (person): {:?}",
                world.said
            );
            continue;
        };
        let (ended, world) = recovered(install.recovery(limits(5_000, 3_000)), Fake::default());
        assert!(
            matches!(ended, Some(Ended::Deferred(_))),
            "row {at}: {ended:?}"
        );
        assert!(said(&world, deferral), "row {at}: {:?}", world.said);
        let person = Fake {
            starts_die: true,
            ..Fake::default()
        };
        let (_code, world) = recover_door(&install, args(&["--cwd", "/x"]), person);
        drop(row);
        assert!(said(&world, deferral), "row {at}: {:?}", world.said);
        assert!(
            world.launched.lock().unwrap().is_empty(),
            "row {at}: no trial"
        );
        if delivered {
            assert_eq!(world.relaunched.len(), 1, "row {at}: {:?}", world.said);
            assert_eq!(world.shown.len(), 1, "row {at}");
        } else {
            assert!(
                world.relaunched.is_empty(),
                "row {at}: {:?}",
                world.relaunched
            );
        }
        assert_eq!(
            install.on_disk().unwrap().body.phase,
            Phase::Moving,
            "row {at}"
        );
        assert_eq!(
            version_of(&install.installed),
            "2.0",
            "row {at}: nothing swapped"
        );
    }
}

/// RED (U-37; Codex's review of `153f7cb2`, finding 1, on macOS) — **before a
/// fresh `Stuck`'s retrial the same step fails closed, and its deferral is the
/// road's end: over a held claim nothing is started beside its holder; a
/// denied claim question or an unlistable folder end `Deferred` and a
/// person's start is delivered.**
///
/// The rollback was declared and its swap back is refused, so the journal
/// becomes `Stuck` with the new bundle live, and the recovery handed a
/// person's start reaches its retrial.
///
/// MUTATION: in `Txn::retry_as_trial`, answer the earlier `ended` for
/// `Pre::Stop` (the round-4 code).
#[test]
fn a_deferral_before_a_fresh_stuck_retrial_is_the_roads_end() {
    if !on_macos() {
        return;
    }
    for (tag, claim, unlistable, expected) in [
        ("held", MacClaim::Held, false, "Deferred(Held)"),
        ("denied", MacClaim::Denied, false, "Deferred(Denied("),
        ("unlistable", MacClaim::Free, true, "Deferred(Unlistable("),
    ] {
        let install = Install::new(&format!("stuck-{tag}"));
        install.arm();
        install.exchanged();
        install.write(Phase::RollbackIntent { trial: None });
        let children = Children::default();
        let row = mac_row(&install, &claim, false);
        let mut world = Fake {
            refuse_from: 0,
            starts_die: true,
            ..Fake::default()
        };
        if unlistable {
            a_candidate(&install, &children);
            let folder = install.home.transaction(install.txn);
            world.on_say = Some(Box::new(move |line| {
                if line.contains("the update is incomplete") {
                    run("/bin/chmod", &[OsStr::new("300"), folder.as_os_str()]);
                }
            }));
        }
        let (_code, world) = recover_door(&install, args(&["--cwd", "/x"]), world);
        if unlistable {
            let folder = install.home.transaction(install.txn);
            run("/bin/chmod", &[OsStr::new("755"), folder.as_os_str()]);
        }
        drop(row);
        assert!(said(&world, expected), "{tag}: {:?}", world.said);
        assert!(
            matches!(install.on_disk().unwrap().body.phase, Phase::Stuck { .. }),
            "{tag}"
        );
        assert!(
            world.launched.lock().unwrap().is_empty(),
            "{tag}: no retrial"
        );
        if tag == "held" {
            assert!(world.relaunched.is_empty(), "{tag}: {:?}", world.relaunched);
        } else {
            assert_eq!(world.relaunched.len(), 1, "{tag}: {:?}", world.said);
            assert_eq!(world.shown.len(), 1, "{tag}");
        }
    }
}

/// RED (U-42d) — **the macOS applier's line reaches `diagnostics.log` once,
/// when its standard error is that same log** — the Windows test's twin
/// (`update_apply_windows::tests::a_road_line_reaches_the_log_once_when_standard_error_is_that_log`).
///
/// MUTATION: in `Machine::say`, write standard error too when a log is named.
#[test]
fn a_road_line_reaches_the_log_once_when_standard_error_is_that_log() {
    const CHILD: &str = "BT_U42D_MACOS_SAY";
    if let Some(log) = std::env::var_os(CHILD) {
        let mut machine = Machine {
            log: Some((PathBuf::from(log), String::new())),
        };
        Hands::say(&mut machine, "BT_UPDATE_APPLY one line, once");
        return;
    }
    let text = crate::update_apply_windows::tests::said_by_a_child_whose_stderr_is_the_log(
        "update_apply_macos::tests::a_road_line_reaches_the_log_once_when_standard_error_is_that_log",
        CHILD,
        "macos",
    );
    assert_eq!(
        text.matches("BT_UPDATE_APPLY one line, once").count(),
        1,
        "{text}"
    );
}
