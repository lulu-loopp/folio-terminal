//! **The Windows Prepare, over real folders, real archives and real signed
//! files** (Windows), and what it does elsewhere.
//!
//! Every file is made here, under the test's own temporary folder: an install
//! folder holding the running build — a small x64 program carrying its own
//! release manifest, signed and time-stamped by U-15's test root
//! (`bt_platform::trust_harness`) — and a release archive in `package.ps1`'s
//! layout (`update_archive`'s own test writer) whose `folio.exe` and sidecars
//! the same root signed, with its checksum document. The product's trust
//! checks run for real under that root's exclusive-root policy; the download
//! is a stand-in transport that writes the file it is asked for ([`Release`]);
//! nothing leaves this machine, and nothing is installed anywhere.

use super::*;

use std::sync::Mutex;
use std::sync::mpsc;

use bt_persist::UpdateCheckV1;
use bt_platform::HostPlatform;
use bt_platform::trust_harness::{Behaviour, IDENTITY, OTHER_IDENTITY, SUBJECT, TestCa};
use bt_winres::digest::{hex, sha256};
use bt_winres::release_manifest::{PROTOCOL, RESOURCE_NAME, archive_root};

use crate::i18n::Text;
use crate::update_archive::tests::{item_under, zip_of};
use crate::update_job::{
    Applied, Bytes, Failure, Fetching, Gathered, Job, Presenters, Progress, Request, State, Verb,
};
use crate::update_prepare::{AtLaunch, at_launch, sum_for};
use crate::update_txn::{Class, Header, HeaderOutcome, Phase, PhaseKind};

/// The running build's `VERSIONINFO`, and the offer's.
const RUNNING: FileVersion = FileVersion([0, 4, 6, 0]);
const OFFERED: FileVersion = FileVersion([0, 4, 7, 0]);
/// The tag every test offers, and the version it names.
const TAG: &str = "v0.4.7";
const TO: &str = "0.4.7";

/// The text members of a test release, after the two sidecars.
const TEXT_MEMBERS: [&str; 3] = ["folio-here.cmd", "uninstall.cmd", "LICENSE-MIT"];

/// The offer every test presses: `v0.4.7`, for Windows.
fn offer(txn: u8) -> Offer {
    Offer::mint(TxnId::new([txn; 16]), TAG, HostPlatform::Windows).expect("a release tag")
}

/// Run `body` on a worker the thread door started, and wait for it.
fn on_a_worker<T: Send + 'static>(body: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
    let worker = bt_platform::spawn_at_priority(
        "bt-u20-test",
        bt_platform::ThreadPriority::BelowNormal,
        body,
    )
    .expect("the thread door starts a thread");
    match worker.join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

// ── the files ───────────────────────────────────────────────────────────────

/// **One build's files, written into `folder`**: the two sidecars (small
/// programs the test root signed), the text members saying `says`, and
/// `folio.exe` — a program at `version`, signed for the test publisher under
/// `identity`, carrying the release manifest of the others (F-4). Answers
/// every file with its bytes, `folio.exe` first.
fn build(
    ca: &TestCa,
    folder: &Path,
    version: FileVersion,
    spelled: &str,
    identity: &str,
    says: &str,
) -> Vec<(String, Vec<u8>)> {
    build_that(
        ca,
        folder,
        version,
        spelled,
        identity,
        says,
        Behaviour::Returns,
    )
}

/// [`build`], whose `folio.exe` does what `behaviour` says when it is run —
/// the Windows applier's tests start it (U-23).
pub(crate) fn build_that(
    ca: &TestCa,
    folder: &Path,
    version: FileVersion,
    spelled: &str,
    identity: &str,
    says: &str,
    behaviour: Behaviour,
) -> Vec<(String, Vec<u8>)> {
    build_needing(
        ca,
        folder,
        (version, spelled),
        identity,
        says,
        behaviour,
        crate::version::VERSION,
    )
}

/// [`build_that`], whose manifest's `min_updater` is `needs` (U-42c).
fn build_needing(
    ca: &TestCa,
    folder: &Path,
    (version, spelled): (FileVersion, &str),
    identity: &str,
    says: &str,
    behaviour: Behaviour,
    needs: &str,
) -> Vec<(String, Vec<u8>)> {
    let mut members = Vec::new();
    for sidecar in SIDECARS {
        let path = folder.join(sidecar);
        ca.signed_program(&path, FileVersion([1, 0, 0, 0]), SUBJECT, IDENTITY, &[]);
        members.push((sidecar.to_owned(), std::fs::read(&path).unwrap()));
    }
    for name in TEXT_MEMBERS {
        let bytes = format!("@rem {name}: {says}\r\n").repeat(20).into_bytes();
        std::fs::write(folder.join(name), &bytes).unwrap();
        members.push((name.to_owned(), bytes));
    }
    let manifest = Manifest {
        product: release_manifest::PRODUCT.to_owned(),
        version: spelled.to_owned(),
        arch: release_manifest::archive_arch(std::env::consts::ARCH).to_owned(),
        archive_root: archive_root(spelled),
        protocol: PROTOCOL,
        min_updater: needs.to_owned(),
        members: members
            .iter()
            .map(|(name, bytes)| release_manifest::Member {
                name: name.clone(),
                sha256: hex(&sha256(bytes)),
                size: bytes.len() as u64,
            })
            .collect(),
    };
    let exe = folder.join(EXECUTABLE);
    ca.signed_program_that(
        &exe,
        version,
        SUBJECT,
        identity,
        &[(RESOURCE_NAME, manifest.encode().as_bytes())],
        behaviour,
    );
    let mut files = vec![(EXECUTABLE.to_owned(), std::fs::read(&exe).unwrap())];
    files.extend(members);
    files
}

/// **The release archive of `files`** under the offer's root, in the layout
/// `package.ps1` writes.
fn archive_of(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let root = archive_root(TO);
    let items: Vec<_> = files
        .iter()
        .map(|(name, bytes)| item_under(&root, name, bytes))
        .collect();
    zip_of(&items)
}

/// `SHA256SUMS.txt` for `archive` under the offer's asset name, beside another
/// file's line.
fn sums_for(archive: &[u8]) -> String {
    format!(
        "{}  Folio-0.4.7-macos-arm64.dmg\n{}  {}\n",
        "0".repeat(64),
        hex(&sha256(archive)),
        offer(0).asset()
    )
}

/// A folder of the test's own under the temporary directory, removed when
/// dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// **An install folder running 0.4.6, and a release of 0.4.7**, both signed by
/// one test root.
struct Scene {
    scratch: Scratch,
    ca: TestCa,
    install: PathBuf,
    exe: PathBuf,
    archive: Vec<u8>,
}

impl Scene {
    /// The scene, or `None` off Windows, where no test root can sign.
    fn new(tag: &str) -> Option<Self> {
        let ca = TestCa::new().ok()?;
        let root = std::env::temp_dir().join(format!(
            "bt-u20-{tag}-{}-{}",
            std::process::id(),
            bt_platform::attention_pipe::unguessable_bits() % 1_000_000
        ));
        let scratch = Scratch(root.clone());
        let install = root.join("Folio");
        let release = root.join("release");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(&release).unwrap();
        build(
            &ca,
            &install,
            RUNNING,
            "0.4.6",
            IDENTITY,
            "the running build",
        );
        let archive = archive_of(&build(
            &ca,
            &release,
            OFFERED,
            TO,
            IDENTITY,
            "the new build",
        ));
        Some(Self {
            exe: install.join(EXECUTABLE),
            scratch,
            ca,
            install,
            archive,
        })
    }

    /// Another build of 0.4.7, in a folder of its own, signed by the same root
    /// for `identity`, its text members saying `says`.
    fn another_release(&self, folder: &str, identity: &str, says: &str) -> Vec<(String, Vec<u8>)> {
        let path = self.scratch.0.join(folder);
        std::fs::create_dir_all(&path).unwrap();
        build(&self.ca, &path, OFFERED, TO, identity, says)
    }

    fn home(&self) -> Home {
        Home::of(HostPlatform::Windows, &self.exe).expect("an executable in a folder")
    }

    fn driver(&self, tools: TestTools) -> WinPrepare {
        WinPrepare::with(
            self.exe.clone(),
            Some(Channel::Ours),
            self.ca.policy(),
            Arc::new(tools),
        )
    }

    /// Every file of the install folder but the installation home, with its
    /// bytes, by name.
    fn installed(&self) -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(&self.install)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.file_name() != Some(OsStr::new(crate::update_txn::WINDOWS_HOME)))
            .map(|path| {
                (
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    std::fs::read(&path).unwrap(),
                )
            })
            .collect();
        files.sort();
        files
    }

    /// **Nothing of transaction `txn` is left**: no folder, no journal, and
    /// the transaction lock free.
    fn left_nothing(&self, txn: u8) {
        let home = self.home();
        assert!(
            !home.transaction(TxnId::new([txn; 16])).exists(),
            "the transaction's folder is removed"
        );
        assert!(!home.journal().exists(), "the journal is removed");
        assert!(
            install_txn::try_hold(&home.lock(), Hold::Exclusive)
                .unwrap()
                .is_some(),
            "the lock is let go"
        );
    }
}

// ── the stand-ins ───────────────────────────────────────────────────────────

/// **The release, as a stand-in transport**: the archive's bytes and its
/// checksum document, written under the name each request asks for.
struct Release {
    archive: Vec<u8>,
    sums: String,
    /// The file name whose fetch fails.
    refuse: Option<String>,
    /// Where each file was asked to go.
    into: Mutex<Vec<PathBuf>>,
    /// Told when the archive's fetch begins, and waited on before it goes on.
    gate: Option<(mpsc::Sender<()>, Mutex<mpsc::Receiver<()>>)>,
}

impl Release {
    fn of(archive: Vec<u8>) -> Self {
        Self {
            sums: sums_for(&archive),
            archive,
            refuse: None,
            into: Mutex::new(Vec::new()),
            gate: None,
        }
    }
}

impl Transport for Release {
    fn fetch(
        &self,
        request: &Request,
        into: &Path,
        fetching: &Fetching,
    ) -> Result<PathBuf, String> {
        self.into.lock().unwrap().push(into.to_path_buf());
        if self.refuse.as_deref() == Some(request.file_name.as_str()) {
            return Err("the stand-in server said no".to_owned());
        }
        let target = into.join(&request.file_name);
        if request.file_name.ends_with(".zip") {
            if let Some((entered, go)) = &self.gate {
                entered.send(()).unwrap();
                go.lock().unwrap().recv().unwrap();
            }
            std::fs::write(&target, &self.archive).map_err(|e| e.to_string())?;
            let length = self.archive.len() as u64;
            (fetching.report)(Bytes {
                received: length,
                total: Some(length),
            });
        } else {
            std::fs::write(&target, &self.sums).map_err(|e| e.to_string())?;
        }
        Ok(target)
    }
}

/// How the stand-in effects copy.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Copying {
    /// The product's durable copy.
    Real,
    /// A disk that took half of this member's bytes and said it took them all.
    ShortWrite(&'static str),
    /// A disk that is full: the copy is refused and nothing is left.
    DiskFull,
    /// A device that refuses the file's flush: the durable copy removes its
    /// temporary and refuses, and nothing is left.
    FlushRefused,
    /// The rescue copy is refused.
    RescueRefused,
    /// The rescue copy lands, and one byte of it is not the running image's.
    RescueAltered,
}

/// **The effects a test holds a Prepare to**: the space the volume reports
/// (the real answer unless a test says otherwise) and the copy, which a test
/// may tell to lie or to fail.
struct TestTools {
    available: Option<u64>,
    copying: Copying,
}

impl TestTools {
    fn real() -> Self {
        Self {
            available: None,
            copying: Copying::Real,
        }
    }

    fn copying(copying: Copying) -> Self {
        Self {
            copying,
            ..Self::real()
        }
    }
}

impl Tools for TestTools {
    fn available(&self, folder: &Path) -> Result<u64, String> {
        self.available.map_or_else(|| System.available(folder), Ok)
    }

    fn copy(&self, from: &Path, to: &Path) -> Result<(), String> {
        let rescue = to.parent().and_then(Path::file_name) == Some(OsStr::new("rescue"));
        match self.copying {
            Copying::ShortWrite(name) if to.file_name() == Some(OsStr::new(name)) => {
                let bytes = std::fs::read(from).map_err(|e| e.to_string())?;
                std::fs::write(to, &bytes[..bytes.len() / 2]).map_err(|e| e.to_string())
            }
            Copying::DiskFull => {
                Err("There is not enough space on the disk. (os error 112)".into())
            }
            Copying::FlushRefused => Err("install_txn flush-file: the device refused".into()),
            Copying::RescueRefused if rescue => Err("the stand-in copy refuses".into()),
            Copying::RescueAltered if rescue => {
                System.copy(from, to)?;
                let mut bytes = std::fs::read(to).map_err(|e| e.to_string())?;
                let last = bytes.len() - 1;
                bytes[last] ^= 0xFF;
                std::fs::write(to, bytes).map_err(|e| e.to_string())
            }
            _ => System.copy(from, to),
        }
    }
}

/// **Press Update on a job offering `v0.4.7` with `driver`**, and wait for
/// the job to reach `Verified` or `Failed` — reading the reports the way the
/// window thread does (`Job::drain_progress`).
fn press(driver: &WinPrepare, transport: Arc<Release>, txn: u8) -> Job<u32> {
    press_with(driver, transport, txn)
}

/// [`press`], with any transport.
fn press_with(driver: &WinPrepare, transport: SharedTransport, txn: u8) -> Job<u32> {
    let mut job = offered(txn);
    job.answer_verb(Verb::Press, driver, &transport)
        .expect("the press is taken");
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        job.drain_progress();
        if matches!(job.state(), State::Verified(_) | State::Failed(..)) {
            return job;
        }
        assert!(
            Instant::now() < deadline,
            "the Prepare did not finish: {:?}",
            job.state()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// **Wait for the cancelled Prepare of transaction `txn` to say its last
/// word** (`Stopped(Cancelled)`, `update_prepare::finish`), reading the job's
/// inbox without applying it, and answer every report taken, in order. The
/// last word is the sign the worker holds nothing of the transaction; the
/// journal's removal is not (the lock is let go after it).
fn last_word(job: &mut Job<u32>, txn: u8) -> Vec<Progress> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut taken = Vec::new();
    loop {
        taken.extend(job.take_reports());
        if taken.iter().any(|report| {
            report.txn == TxnId::new([txn; 16]) && report.step == Step::Stopped(Stop::Cancelled)
        }) {
            return taken;
        }
        assert!(
            Instant::now() < deadline,
            "the cancelled Prepare never said its last word: {taken:?}"
        );
        std::thread::yield_now();
    }
}

/// A job that offers `v0.4.7` under transaction `txn`.
fn offered(txn: u8) -> Job<u32> {
    let mut job = Job::with_offers(true);
    job.consider(
        Gathered {
            check: Some((
                UpdateCheckV1 {
                    latest_tag: Some(TAG.to_owned()),
                    ..UpdateCheckV1::default()
                },
                true,
            )),
            channel: Some(Channel::Ours),
            running: "0.4.6",
            capable: true,
            trial: false,
            platform: HostPlatform::Windows,
        },
        &Presenters {
            visited: &[1],
            open: &[1],
            quake: None,
        },
        || TxnId::new([txn; 16]),
    );
    assert!(
        matches!(job.state(), State::Available(_)),
        "{:?}",
        job.state()
    );
    job
}

/// What the Prepare left behind for the job, read off the disk.
fn journal_on_disk(home: &Home) -> Journal {
    Journal::parse(&std::fs::read(home.journal()).expect("a journal")).expect("a whole journal")
}

/// The inventories a `Prepared` journal carries.
fn inventories(journal: &Journal) -> &Inventories {
    match &journal.body.layout {
        Layout::Members(inventories) => inventories,
        other => panic!("a Windows transaction records members, not {other:?}"),
    }
}

/// **The job failed with `stop`, and its card says so and that nothing
/// changed.**
fn failed_with(job: &Job<u32>, txn: u8, stop: Stop) {
    assert_eq!(
        job.state(),
        &State::Failed(Some(offer(txn)), Failure::Stopped(stop)),
    );
    let paint = crate::update_card::paint(job.state()).expect("a failed card");
    assert_eq!(
        paint.detail.as_deref(),
        Some(Text::UpdateCardNothingChanged.text()),
        "{stop:?}: the card says nothing changed"
    );
}

/// Off Windows there is no test root: **the Windows Prepare refuses before it
/// writes anything** — the running build's identity cannot be read there
/// (`trust` refuses by name), so no home is made.
fn refused_off_windows() {
    assert_ne!(bt_platform::host_platform(), HostPlatform::Windows);
    let root = std::env::temp_dir().join(format!(
        "bt-u20-elsewhere-{}-{}",
        std::process::id(),
        bt_platform::attention_pipe::unguessable_bits() % 1_000_000
    ));
    let _scratch = Scratch(root.clone());
    std::fs::create_dir_all(&root).unwrap();
    let exe = root.join(EXECUTABLE);
    std::fs::write(&exe, b"not a program").unwrap();
    let driver = WinPrepare::with(
        exe,
        Some(Channel::Ours),
        Policy::System,
        Arc::new(TestTools::real()),
    );
    let job = press(&driver, Arc::new(Release::of(Vec::new())), 1);
    assert_eq!(
        job.state(),
        &State::Failed(Some(offer(1)), Failure::Stopped(Stop::Identity))
    );
    assert!(
        !root.join(crate::update_txn::WINDOWS_HOME).exists(),
        "nothing was written"
    );
}

// ── the tests ───────────────────────────────────────────────────────────────

/// RED (U-20) — **an archive and a checksum document replaced together,
/// consistently, still do not pass: hash agreement alone never admits a
/// release; the identity of the running build's signer does.**
///
/// The review's counterexample (F-4, F-16): `SHA256SUMS.txt` is unsigned, so
/// whoever can replace the archive can replace its line too. Here the attacker
/// ships a `folio.exe` validly signed under the same trusted root — a valid
/// signature, a trusted chain, a time stamp — but for another identity, and it
/// carries a manifest that lists a changed `uninstall.cmd`, so the archive
/// reader's own checks all agree with it. The Prepare stops at the identity
/// check with *Nothing changed*, and before any file of the install is
/// touched. Beside it, the genuine `folio.exe` with that changed `.cmd` is
/// refused by the manifest it signs (F-4), and the genuine release passes —
/// the fixture is not refused for a reason of its own.
///
/// MUTATION: in `verify_set`, skip `trust::verify_release_file_under` for
/// `folio.exe` — the attacker's release is `Verified`.
#[test]
fn mutated_asset_hash_pair_refuses_before_swap() {
    let Some(scene) = Scene::new("mutated") else {
        return refused_off_windows();
    };
    let before = scene.installed();

    let attacker = scene.another_release("attacker", OTHER_IDENTITY, "the attacker's command");
    let replaced = Release::of(archive_of(&attacker));
    assert_eq!(
        sum_for(&replaced.sums, offer(0).asset()),
        Some(hex(&sha256(&replaced.archive))),
        "the pair agrees: the checksum line is the replaced archive's"
    );
    let job = press(&scene.driver(TestTools::real()), Arc::new(replaced), 1);
    failed_with(&job, 1, Stop::Identity);
    scene.left_nothing(1);
    assert_eq!(scene.installed(), before, "nothing installed was changed");

    // The genuine executable, with the attacker's `.cmd`: its own manifest
    // refuses the member.
    let mut genuine = std::fs::read_dir(scene.scratch.0.join("release"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    genuine.sort_by_key(|(name, _)| name != EXECUTABLE);
    let cmd = attacker
        .iter()
        .find(|(name, _)| name == "uninstall.cmd")
        .unwrap()
        .clone();
    for file in &mut genuine {
        if file.0 == cmd.0 {
            file.1.clone_from(&cmd.1);
        }
    }
    let job = press(
        &scene.driver(TestTools::real()),
        Arc::new(Release::of(archive_of(&genuine))),
        2,
    );
    failed_with(&job, 2, Stop::Identity);
    scene.left_nothing(2);
    assert_eq!(scene.installed(), before);

    let job = press(
        &scene.driver(TestTools::real()),
        Arc::new(Release::of(scene.archive.clone())),
        3,
    );
    assert!(
        matches!(job.state(), State::Verified(_)),
        "the genuine release passes: {:?}",
        job.state()
    );
    assert_eq!(
        scene.installed(),
        before,
        "a Prepare changes nothing installed"
    );
}

/// RED (U-42c) — **a release that needs a newer updater than this build
/// fails as "too old to update itself", not as "not verified"; nothing
/// changes.**
///
/// 0.4.6's D-13: the first clean-machine pair ran 0.4.5 against a release
/// whose manifest said `min_updater 0.4.6`; the card said *The update is not
/// verified.* and no line said which check refused. The release here is
/// signed by the test root like the genuine one and differs only in its
/// manifest's `min_updater`.
///
/// MUTATION: in `stop_for_archive`, drop the `UpdaterTooOld` arm — the job
/// fails with `Stop::Identity`.
#[test]
fn a_release_that_needs_a_newer_updater_says_this_version_is_too_old() {
    let Some(scene) = Scene::new("too-old") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    let folder = scene.scratch.0.join("needs-newer");
    std::fs::create_dir_all(&folder).unwrap();
    let release = build_needing(
        &scene.ca,
        &folder,
        (OFFERED, TO),
        IDENTITY,
        "a release for newer updaters",
        Behaviour::Returns,
        "99.0.0",
    );
    let job = press(
        &scene.driver(TestTools::real()),
        Arc::new(Release::of(archive_of(&release))),
        1,
    );
    failed_with(&job, 1, Stop::TooOld);
    let paint = crate::update_card::paint(job.state()).expect("a failed card");
    assert_eq!(
        paint.heading.as_deref(),
        Some(Text::UpdateFailedTooOld.text())
    );
    scene.left_nothing(1);
    assert_eq!(scene.installed(), before, "nothing installed was changed");
}

/// RED (U-20) — **a copy that lands short, a disk that is full and a flush
/// the device refuses never reach `Prepared`: the transaction is removed and
/// the card says nothing changed.**
///
/// §C.2 step 6: the verified tree is copied into `set\`, flushed, and
/// re-verified where it lies, because what gets installed must be what was
/// checked. A disk that accepted half of `uninstall.cmd` and said it took it
/// all leaves a file whose signature nobody checks — only its digest against
/// the one measured before the copy tells. A full disk and a refused flush are
/// refusals of the copy itself (`install_txn`'s own test holds that a refused
/// flush leaves no file under the name).
///
/// MUTATION: in `still_staged`, skip the digest comparison — the short write
/// is `Verified`.
#[test]
fn short_write_disk_full_and_flush_failure_never_verify() {
    let Some(scene) = Scene::new("short") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    for (txn, copying, stop) in [
        (1, Copying::ShortWrite("uninstall.cmd"), Stop::Identity),
        (2, Copying::DiskFull, Stop::Copy),
        (3, Copying::FlushRefused, Stop::Copy),
    ] {
        let job = press(
            &scene.driver(TestTools::copying(copying)),
            Arc::new(Release::of(scene.archive.clone())),
            txn,
        );
        failed_with(&job, txn, stop);
        scene.left_nothing(txn);
        assert_eq!(scene.installed(), before, "nothing installed was changed");
    }
}

/// RED (U-20) — **every file a Prepare writes is inside the installation's
/// own folder, on its volume: the download, the expansion, the staged set and
/// the rescue copy — nothing lands in the roaming profile or the system's
/// temporary folder.**
///
/// R-6 and C6: the flip is a rename only if the staged set is on the
/// destination volume, and a download cache under `persist::storage_dir()`
/// would put the release in the roaming profile. The transport is asked to
/// write into `H\<txn>\download\`; after `Prepared` only `set\` and `rescue\`
/// remain under `H\<txn>`, and every path under the home resolves to the
/// install folder's volume.
///
/// MUTATION: in `acquire`, fetch into `std::env::temp_dir()` instead of
/// `folders.download`.
#[test]
fn staging_is_always_on_the_destination_volume() {
    let Some(scene) = Scene::new("volume") else {
        return refused_off_windows();
    };
    let release = Arc::new(Release::of(scene.archive.clone()));
    let job = press(&scene.driver(TestTools::real()), Arc::clone(&release), 1);
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );

    let home = scene.home();
    let transaction = home.transaction(TxnId::new([1; 16]));
    assert!(
        home.root().starts_with(&scene.install),
        "H is in the install folder"
    );
    for into in release.into.lock().unwrap().iter() {
        assert_eq!(into, &transaction.join("download"), "the download is H's");
    }
    let mut left: Vec<String> = std::fs::read_dir(&transaction)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        ["rescue", "set"],
        "the download and expansion are gone"
    );

    let volume = |path: &Path| {
        std::fs::canonicalize(path)
            .unwrap()
            .components()
            .next()
            .map(|component| component.as_os_str().to_ascii_lowercase())
    };
    let install_volume = volume(&scene.install);
    let mut stack = vec![home.root().to_path_buf()];
    let mut seen = 0;
    while let Some(path) = stack.pop() {
        assert_eq!(volume(&path), install_volume, "{}", path.display());
        seen += 1;
        if path.is_dir() {
            stack.extend(std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
        }
    }
    assert!(
        seen > 10,
        "the home and the transaction were walked ({seen})"
    );
}

/// RED (U-20) — **a Cancel while the archive downloads leaves no
/// transaction**: no `H\<txn>`, no journal, the lock free, the install as it
/// was, and the job idle.
///
/// §C.2: "every failure here deletes the transaction directory, releases the
/// lock". A Cancel is the job's word, not a failure the driver reports, so
/// it takes the same road at the driver's next step — its only report the
/// last word, stale to the job — and must still leave nothing: a journal at `Allocated` left behind would be
/// swept at the next launch, but the lock and the folder would stand until
/// then.
///
/// MUTATION: in `prepare_on`'s error arm, skip `abandon` when the stop is
/// `Stop::Cancelled`.
#[test]
fn cancel_leaves_no_transaction() {
    let Some(scene) = Scene::new("cancel") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    let (entered, entered_seen) = mpsc::channel();
    let (go, go_seen) = mpsc::channel();
    let release = Arc::new(Release {
        gate: Some((entered, Mutex::new(go_seen))),
        ..Release::of(scene.archive.clone())
    });
    let driver = scene.driver(TestTools::real());
    let mut job = offered(1);
    let transport: SharedTransport = release;
    job.answer_verb(Verb::Press, &driver, &transport)
        .expect("the press is taken");
    entered_seen
        .recv_timeout(Duration::from_secs(60))
        .expect("the archive's fetch began");
    assert!(
        scene.home().journal().exists(),
        "the transaction exists while it downloads"
    );
    job.answer_verb(Verb::Cancel, &driver, &transport)
        .expect("Cancel is on the download's card");
    assert_eq!(job.state(), &State::Idle);
    go.send(()).unwrap();

    let reports = last_word(&mut job, 1);
    scene.left_nothing(1);
    assert_eq!(scene.installed(), before);
    for report in reports {
        assert_eq!(job.apply(report), Applied::Stale, "{report:?}");
    }
    assert_eq!(
        job.state(),
        &State::Idle,
        "no report revives a cancelled job"
    );
}

/// RED (cancel-lock) — **a cancelled Prepare says its last word only once it
/// holds nothing: when the report is read, the folder and the journal are
/// gone and the transaction lock is free.**
///
/// The journal goes last under the lock and the lock is let go after it, a
/// directory flush later, so a reader that took the journal's removal as the
/// sign found the lock still held — 160 times in 160 on a quiet machine when
/// looked at at once, and once on main's CI (847b65c3) through a 20 ms poll.
/// The worker's one report (`update_prepare::finish`) is the sign; every round
/// here looks at the lock the instant the report is read.
///
/// MUTATION: in `update_prepare::finish`, post nothing for
/// `Err(Stop::Cancelled)` (the last word never comes), or post it before
/// `prepare_on` returns (the lock is still held when it is read).
#[test]
fn a_cancelled_prepare_reports_only_after_it_lets_the_lock_go() {
    let Some(scene) = Scene::new("lastword") else {
        return refused_off_windows();
    };
    let driver = scene.driver(TestTools::real());
    let home = scene.home();
    for txn in 1..=12u8 {
        let (entered, entered_seen) = mpsc::channel();
        let (go, go_seen) = mpsc::channel();
        let release = Arc::new(Release {
            gate: Some((entered, Mutex::new(go_seen))),
            ..Release::of(scene.archive.clone())
        });
        let mut job = offered(txn);
        let transport: SharedTransport = release;
        job.answer_verb(Verb::Press, &driver, &transport)
            .expect("the press is taken");
        entered_seen
            .recv_timeout(Duration::from_secs(60))
            .expect("the archive's fetch began");
        job.answer_verb(Verb::Cancel, &driver, &transport)
            .expect("Cancel is on the download's card");
        go.send(()).unwrap();

        last_word(&mut job, txn);
        assert!(
            install_txn::try_hold(&home.lock(), Hold::Exclusive)
                .unwrap()
                .is_some(),
            "round {txn}: the lock is let go before the last word"
        );
        scene.left_nothing(txn);
    }
}

/// RED (U-20) — **a `Prepared` transaction survives the first launch that does
/// not resume it, counted, and is discarded at the second** — its set, its
/// rescue copy and its journal all gone, the install untouched.
///
/// (b).1 F-17 and W2: startup never deletes `Prepared`; the job owner counts
/// `deferred_launches` at each launch that does not resume and discards at
/// [`crate::update_txn::DEFERRED_LAUNCH_LIMIT`]. The pass is the one
/// `update_prepare::at_launch` both platforms share, over a Windows journal
/// the real Prepare wrote.
///
/// MUTATION: set `DEFERRED_LAUNCH_LIMIT` to 1 — the first relaunch discards.
#[test]
fn a_deferred_transaction_survives_the_first_relaunch() {
    let Some(scene) = Scene::new("deferred") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    let job = press(
        &scene.driver(TestTools::real()),
        Arc::new(Release::of(scene.archive.clone())),
        1,
    );
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );
    drop(job);

    let home = scene.home();
    let first = {
        let home = home.clone();
        on_a_worker(move |worker| match at_launch(worker, &home).unwrap() {
            AtLaunch::Counted(staged) => staged.journal.body.phase,
            _ => panic!("the first launch counts"),
        })
    };
    assert_eq!(
        first,
        Phase::Prepared {
            deferred_launches: 1
        }
    );
    let on_disk = journal_on_disk(&home);
    assert_eq!(
        on_disk.body.phase,
        Phase::Prepared {
            deferred_launches: 1
        }
    );
    assert_eq!(on_disk.header().class, Class::Deferred);
    let set = home
        .members_folder(TxnId::new([1; 16]), Place::Set)
        .unwrap();
    assert!(set.join(EXECUTABLE).exists(), "the staged set survives");

    let second = {
        let home = home.clone();
        on_a_worker(move |worker| matches!(at_launch(worker, &home).unwrap(), AtLaunch::Discarded))
    };
    assert!(second, "the second launch discards");
    scene.left_nothing(1);
    assert_eq!(scene.installed(), before);
}

/// RED (U-20) — **before a staged set is resumed it is checked again, and a
/// set that changed since `Prepared` is refused and discarded**; an unchanged
/// one passes.
///
/// (b).1 F-17: "revalidates (hash, signature, manifest, classification)
/// before any resume". Between the Prepare and a resume the files in `set\`
/// sat in a folder anyone who can write the install folder can write; a
/// resume that trusted the journal's word for them would install whatever is
/// there now.
///
/// MUTATION: in `staged_as_verified`, skip `still_staged`.
#[test]
fn revalidation_before_resume_refuses_a_changed_set() {
    let Some(scene) = Scene::new("revalidate") else {
        return refused_off_windows();
    };
    let job = press(
        &scene.driver(TestTools::real()),
        Arc::new(Release::of(scene.archive.clone())),
        1,
    );
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );
    drop(job);

    // One later launch: the transaction is counted, then revalidated twice
    // before any resume — unchanged, and after one staged file changed.
    let home = scene.home();
    let set = home
        .members_folder(TxnId::new([1; 16]), Place::Set)
        .unwrap();
    let (unchanged, kept, changed) = {
        let (home, exe, policy) = (home.clone(), scene.exe.clone(), scene.ca.policy());
        on_a_worker(move |worker| {
            let AtLaunch::Counted(staged) = at_launch(worker, &home).unwrap() else {
                panic!("a prepared transaction is counted");
            };
            let resume = Resume {
                exe: &exe,
                channel: Some(Channel::Ours),
                policy: &policy,
            };
            let staged = revalidate(worker, *staged, &resume);
            let unchanged = staged.as_ref().map(drop).map_err(|stop| *stop);
            let kept = home.journal().exists();
            std::fs::write(
                set.join("uninstall.cmd"),
                b"@rem changed after the Prepare\r\n",
            )
            .unwrap();
            let changed = match staged {
                Ok(staged) => revalidate(worker, staged, &resume).map(drop),
                Err(stop) => Err(stop),
            };
            (unchanged, kept, changed)
        })
    };
    assert_eq!(unchanged, Ok(()), "an unchanged set passes");
    assert!(kept, "and is kept");
    assert_eq!(changed, Err(Stop::Identity), "a changed one is refused");
    scene.left_nothing(1);
}

/// RED (U-20) — **every road a Prepare can fail by removes its transaction,
/// lets the lock go, leaves the install as it was, and has the card say which
/// reason and that nothing changed** — a refused download, a checksum
/// mismatch, too little space (the card naming the shortfall), an archive
/// that is not a release, a refused copy and a refused rescue copy.
///
/// §C.2's last paragraph and W1: "Every failure here deletes the transaction
/// directory, releases the lock, and reports `Failed(…)` with *Nothing
/// installed was changed.*, which is true."
///
/// MUTATION: in `update_prepare::abandon`, record `Abandoned` and return
/// without `clear`.
#[test]
fn every_failure_road_removes_the_transaction_and_says_nothing_changed() {
    let Some(scene) = Scene::new("roads") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    let genuine = || Release::of(scene.archive.clone());
    let roads: Vec<(u8, Release, TestTools, Stop)> = vec![
        (
            1,
            Release {
                refuse: Some(offer(0).asset().to_owned()),
                ..genuine()
            },
            TestTools::real(),
            Stop::Download,
        ),
        (
            2,
            Release {
                sums: sums_for(b"another archive"),
                ..genuine()
            },
            TestTools::real(),
            Stop::Sums,
        ),
        (
            3,
            genuine(),
            TestTools {
                available: Some(1_000),
                copying: Copying::Real,
            },
            Stop::Space {
                short_by: 2 * declared(&scene.archive)
                    + std::fs::metadata(&scene.exe).unwrap().len()
                    - 1_000,
            },
        ),
        (
            4,
            Release::of(b"not an archive".to_vec()),
            TestTools::real(),
            Stop::Identity,
        ),
        (
            5,
            genuine(),
            TestTools::copying(Copying::DiskFull),
            Stop::Copy,
        ),
        (
            6,
            genuine(),
            TestTools::copying(Copying::RescueRefused),
            Stop::Clone,
        ),
    ];
    for (txn, release, tools, stop) in roads {
        let job = press(&scene.driver(tools), Arc::new(release), txn);
        failed_with(&job, txn, stop);
        scene.left_nothing(txn);
        assert_eq!(
            scene.installed(),
            before,
            "{stop:?}: nothing installed was changed"
        );
        if let Stop::Space { short_by } = stop {
            let paint = crate::update_card::paint(job.state()).unwrap();
            assert_eq!(
                paint.heading,
                Some(crate::i18n::update_failed_space(
                    &short_by.div_ceil(1_000_000).to_string()
                )),
                "the card names the shortfall"
            );
        }
    }
}

/// What the scene's archive declares its members add up to.
fn declared(archive: &[u8]) -> u64 {
    let root = std::env::temp_dir().join(format!(
        "bt-u20-declared-{}-{}",
        std::process::id(),
        bt_platform::attention_pipe::unguessable_bits() % 1_000_000
    ));
    let _scratch = Scratch(root.clone());
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("release.zip");
    std::fs::write(&path, archive).unwrap();
    update_archive::declared_bytes(&path, TO).unwrap()
}

/// RED (U-20) — **the applier's copy is the running build, checked, named by
/// the journal, and never one of the files the flip moves**; a copy that is
/// not byte for byte the running image stops the Prepare.
///
/// C5 as F-8 replaced it: the applier and recovery run from
/// `H\<txn>\rescue\folio.exe`, a copy of the running build that no step moves
/// and that is deleted only with the transaction — so the Run entrance and the
/// journal's `rescue` always name a program that is there, whatever the flip
/// has moved. The header's `rescue` names it, and `Home::of_rescue` finds the
/// home and the installed program from it (F-2); the flip's moves are between
/// the install folder, `backup\` and `set\`, never `rescue\`.
///
/// MUTATION: make `rescue_is_the_running_build` answer `Ok(())` — the altered
/// copy is `Verified`.
#[test]
fn the_applier_copy_is_verified_and_never_moved() {
    let Some(scene) = Scene::new("applier") else {
        return refused_off_windows();
    };
    let job = press(
        &scene.driver(TestTools::real()),
        Arc::new(Release::of(scene.archive.clone())),
        1,
    );
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );
    let home = scene.home();
    let txn = TxnId::new([1; 16]);
    let rescue = home.rescue_copy(txn, OsStr::new(EXECUTABLE)).unwrap();
    assert_eq!(
        std::fs::read(&rescue).unwrap(),
        std::fs::read(&scene.exe).unwrap(),
        "the rescue copy is the running image"
    );
    let journal = journal_on_disk(&home);
    let header = Header::parse(&std::fs::read(home.journal()).unwrap()).unwrap();
    assert_eq!(
        header.rescue,
        rescue.to_str().unwrap(),
        "the header names it"
    );
    assert_eq!(header.outcome, HeaderOutcome::None);
    assert_eq!(
        journal.body.phase,
        Phase::Prepared {
            deferred_launches: 0
        }
    );
    assert_eq!(
        Home::of_rescue(HostPlatform::Windows, &rescue),
        Some((home.clone(), scene.exe.clone())),
        "and the rescue build finds the home and the install from its own path"
    );
    let inventories = inventories(&journal);
    let rescue_folder = rescue.parent().unwrap();
    for step in inventories.forward_moves() {
        for place in [step.from, step.to] {
            let folder = home
                .members_folder(txn, place)
                .unwrap_or_else(|| scene.install.clone());
            assert_ne!(folder, rescue_folder, "{step:?} moves the rescue copy");
        }
    }
    assert!(
        inventories
            .old_present
            .iter()
            .any(|member| member.name == EXECUTABLE),
        "the running executable is in the old inventory"
    );
    assert_eq!(
        inventories.new.len(),
        1 + SIDECARS.len() + TEXT_MEMBERS.len(),
        "the new set is the release's"
    );
    drop(job);
    on_a_worker({
        let home = home.clone();
        move |worker| {
            if let AtLaunch::Counted(staged) = at_launch(worker, &home).unwrap() {
                let _ = crate::update_prepare::discard(worker, *staged, &Event::Discarded);
            }
        }
    });
    scene.left_nothing(1);

    let job = press(
        &scene.driver(TestTools::copying(Copying::RescueAltered)),
        Arc::new(Release::of(scene.archive.clone())),
        2,
    );
    failed_with(&job, 2, Stop::Clone);
    scene.left_nothing(2);
}

// ── a release feed (U-30b) ──────────────────────────────────────────────────

/// **The page's stand-in**: a transport that counts every request and
/// fetches nothing — a press that reaches it has contacted github.com.
#[derive(Default)]
struct Page(std::sync::atomic::AtomicU32);

impl Page {
    fn calls(&self) -> u32 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Transport for Page {
    fn fetch(&self, request: &Request, _: &Path, _: &Fetching) -> Result<PathBuf, String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(format!("the page was asked for {}", request.file_name))
    }
}

/// The `file:` URL of `path`, as the product mints one (`webnav`).
fn url_of(path: &Path) -> String {
    crate::webnav::file_url_of_local_path(&path.to_string_lossy()).expect("an absolute path")
}

/// **A release feed in `folder`**: `archive` under the offer's asset name and
/// `sums` as `SHA256SUMS.txt`, beside a `releases.json` in the GitHub
/// releases list's shape naming them as release [`TAG`] by their `file:`
/// URLs; answers the feed the folder's URL names, as the command line gives
/// it.
fn feed_of(folder: &Path, archive: &[u8], sums: &str) -> crate::update::Feed {
    std::fs::create_dir_all(folder).unwrap();
    let asset = offer(0).asset().to_owned();
    std::fs::write(folder.join(&asset), archive).unwrap();
    std::fs::write(folder.join("SHA256SUMS.txt"), sums).unwrap();
    let list = serde_json::json!([{
        "tag_name": TAG,
        "name": format!("Folio {TO}"),
        "draft": false,
        "prerelease": false,
        "assets": [
            {
                "name": asset,
                "browser_download_url": url_of(&folder.join(&asset)),
                "size": archive.len(),
            },
            {
                "name": "SHA256SUMS.txt",
                "browser_download_url": url_of(&folder.join("SHA256SUMS.txt")),
                "size": sums.len(),
            },
        ],
    }]);
    std::fs::write(folder.join(crate::update::FEED_LIST), list.to_string()).unwrap();
    crate::update::Feed::at(&format!("{}/", url_of(folder)))
}

/// RED (U-30b) — **with a release feed, a press copies the feed's asset and
/// its checksum document instead of downloading them, and the Prepare holds
/// the copy to its sum exactly as it holds a download.**
///
/// First the copy alone, on every platform: the asset lands in the download
/// folder byte for byte, its bytes reported against the length the list
/// gives. Then, where a test root can sign, the whole Windows Prepare over a
/// feed of the genuine release reaches `Verified`, and over the same archive
/// with a checksum line that is not its own stops at `Sums` with *Nothing
/// changed*. The page's stand-in is never asked.
///
/// MUTATION: make `transport_for` answer `page` whatever `feed` is (the page
/// is asked and the press fails at `Download`); or skip `matches_its_sum` in
/// the Windows `fetch` (the wrong sum is `Verified`).
#[test]
fn the_download_copies_the_feed_asset_and_verifies_its_sum() {
    let root = std::env::temp_dir().join(format!(
        "bt-u30b-copy-{}-{}",
        std::process::id(),
        bt_platform::attention_pipe::unguessable_bits() % 1_000_000
    ));
    let _scratch = Scratch(root.clone());
    let archive = b"a release archive, by the feed".repeat(5_000);
    let feed = feed_of(&root.join("feed"), &archive, &sums_for(&archive));
    let page = Arc::new(Page::default());
    let transport = crate::update_job::transport_for(Some(&feed), page.clone());
    let into = root.join("download");
    std::fs::create_dir_all(&into).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fetching = Fetching {
        report: Arc::new({
            let seen = Arc::clone(&seen);
            move |bytes| seen.lock().unwrap().push(bytes)
        }),
        cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let [asset, _] = offer(0).requests();
    let copied = transport
        .fetch(&asset, &into, &fetching)
        .expect("the feed's asset is copied");
    assert_eq!(copied, into.join(offer(0).asset()));
    assert_eq!(std::fs::read(&copied).unwrap(), archive);
    let length = archive.len() as u64;
    assert_eq!(
        seen.lock().unwrap().last(),
        Some(&Bytes {
            received: length,
            total: Some(length),
        })
    );
    assert_eq!(page.calls(), 0, "github.com was contacted");

    let Some(scene) = Scene::new("feed-sum") else {
        return;
    };
    let before = scene.installed();
    let genuine = feed_of(
        &scene.scratch.0.join("feed"),
        &scene.archive,
        &sums_for(&scene.archive),
    );
    let job = press_with(
        &scene.driver(TestTools::real()),
        crate::update_job::transport_for(Some(&genuine), page.clone()),
        1,
    );
    assert!(
        matches!(job.state(), State::Verified(_)),
        "the feed's genuine release passes: {:?}",
        job.state()
    );
    drop(job);
    on_a_worker({
        let home = scene.home();
        move |worker| {
            if let AtLaunch::Counted(staged) = at_launch(worker, &home).unwrap() {
                let _ = crate::update_prepare::discard(worker, *staged, &Event::Discarded);
            }
        }
    });
    scene.left_nothing(1);

    let wrong = feed_of(
        &scene.scratch.0.join("wrong-sum"),
        &scene.archive,
        &sums_for(b"another archive"),
    );
    let job = press_with(
        &scene.driver(TestTools::real()),
        crate::update_job::transport_for(Some(&wrong), page.clone()),
        2,
    );
    failed_with(&job, 2, Stop::Sums);
    scene.left_nothing(2);
    assert_eq!(scene.installed(), before, "nothing installed was changed");
    assert_eq!(page.calls(), 0, "github.com was contacted");
}

/// RED (U-30b) — **a feed cannot deliver a build the running build's signer
/// did not sign: an archive signed by another identity, with its own
/// consistent checksum line, is refused as `Identity`.**
///
/// The reason a visible flag is acceptable (the coordinator's ruling 2): the
/// feed replaces only where the two files come from. U-20's counterexample,
/// delivered through a feed — a `folio.exe` validly signed under the same
/// trusted root for another identity, its sums agreeing — stops at the
/// identity check with *Nothing changed*, before any installed file is
/// touched; the page is never asked.
///
/// MUTATION: in `verify_set`, skip `trust::verify_release_file_under` for
/// `folio.exe` — the feed's foreign build is `Verified`.
#[test]
fn a_feed_asset_signed_by_another_signer_is_refused_as_identity() {
    let Some(scene) = Scene::new("feed-identity") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    let foreign =
        archive_of(&scene.another_release("foreign", OTHER_IDENTITY, "another signer's command"));
    let feed = feed_of(&scene.scratch.0.join("feed"), &foreign, &sums_for(&foreign));
    let page = Arc::new(Page::default());
    let job = press_with(
        &scene.driver(TestTools::real()),
        crate::update_job::transport_for(Some(&feed), page.clone()),
        1,
    );
    failed_with(&job, 1, Stop::Identity);
    scene.left_nothing(1);
    assert_eq!(scene.installed(), before, "nothing installed was changed");
    assert_eq!(page.calls(), 0, "github.com was contacted");
}

// ── a later launch, through the product's road (U-33) ───────────────────────

/// **The start's world for a later launch**: nothing is started, no entrance
/// is removed and nothing is mounted; what it would say is kept.
#[derive(Default)]
struct Quiet(Vec<String>);

impl crate::update_startup::World for Quiet {
    fn say(&mut self, line: &str) {
        self.0.push(line.to_owned());
    }

    fn spawn_detached(&mut self, program: &Path, _: &[std::ffi::OsString]) -> std::io::Result<()> {
        panic!("a later launch started {}", program.display())
    }

    fn retire_entrance(&mut self, _: TxnId) -> Result<(), String> {
        panic!("a later launch of a waiting transaction removed an entrance")
    }

    fn mounts_under(&mut self, _: &Path) -> Result<Vec<PathBuf>, String> {
        Ok(Vec::new())
    }

    fn on_a_worker(&mut self, _: crate::update_startup::OffThread) -> std::io::Result<()> {
        panic!("a later launch handed a retirement to a worker")
    }
}

/// What a later launch left: the job after its pass landed, whether the day's
/// check was started, and the one line the job said.
struct Launched {
    job: Job<u32>,
    checked: Arc<std::sync::atomic::AtomicBool>,
    said: Option<String>,
}

/// **A later launch of the copy whose executable is `exe`, down the product's
/// road**: the ordinary start's pass (`update_startup::run`, which decides
/// what it leaves for the job owner), the job built the way `create` builds
/// it (`Job::after_start` with that answer, `resume`, and a check that records
/// being started), then the job asked to consider on `gathered` the way the
/// window thread asks it — every `AppEvent::UpdateJobOffer` — until its pass
/// has landed. The window `1` is open and was the last visited.
fn launch(exe: &Path, resume: crate::update_prepare::Resumer, gathered: &Gathered) -> Launched {
    let home = Home::of(HostPlatform::Windows, exe).expect("an executable in a folder");
    let start = crate::update_startup::Start {
        own_exe: exe,
        home: &home,
        argv: &[],
        trial: None,
        failed: None,
    };
    let mut world = Quiet::default();
    let crate::update_startup::Verdict::Continue { waiting, .. } =
        crate::update_startup::run(&start, &mut world)
    else {
        panic!("a start with a waiting transaction continues");
    };
    assert!(world.0.is_empty(), "the start said {:?}", world.0);
    let checked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut job = Job::with_offers(true).after_start(waiting, resume, {
        let checked = Arc::clone(&checked);
        move || checked.store(true, std::sync::atomic::Ordering::SeqCst)
    });
    let presenters = Presenters {
        visited: &[1],
        open: &[1],
        quake: None,
    };
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut said = None;
    loop {
        if let Some(line) = job.consider(gathered.clone(), &presenters, || TxnId::new([0xAA; 16])) {
            said = Some(line);
        }
        if job.state() != &State::Pending(crate::update_job::Pending::AwaitingTransaction) {
            return Launched { job, checked, said };
        }
        assert!(Instant::now() < deadline, "the launch pass never landed");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// What a launch has gathered by the time the channel is read: the channel,
/// and a check that has not settled yet (`check`) or that knows `v0.4.7`.
fn at_launch_gathered(knows_the_release: bool) -> Gathered {
    Gathered {
        check: knows_the_release.then(|| {
            (
                UpdateCheckV1 {
                    latest_tag: Some(TAG.to_owned()),
                    ..UpdateCheckV1::default()
                },
                true,
            )
        }),
        channel: Some(Channel::Ours),
        running: "0.4.6",
        capable: true,
        trial: false,
        platform: HostPlatform::Windows,
    }
}

/// The product's resumer for `scene`'s running build, under the scene's root.
fn resumer_of(scene: &Scene) -> crate::update_prepare::Resumer {
    resumer(scene.exe.clone(), scene.ca.policy())
}

/// **Download, Later, close**: the real Prepare of `v0.4.7` under
/// transaction `1` reaches `Verified`, and the process ends — the job and the
/// lock it held are let go, the journal stays at `Prepared`.
fn prepared_and_closed(scene: &Scene) {
    let job = press(
        &scene.driver(TestTools::real()),
        Arc::new(Release::of(scene.archive.clone())),
        1,
    );
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );
    drop(job);
}

/// **An install folder whose earlier launch died during a download**: a
/// running build at `<root>/Folio/folio.exe` and transaction `3`'s journal at
/// `Allocated`, as the Windows Prepare writes it, with part of an archive in
/// `H\<txn>\download\`. No signature is needed: nothing here is verified.
fn allocated_scene(tag: &str) -> (Scratch, PathBuf, Home, TxnId) {
    let root = std::env::temp_dir().join(format!(
        "bt-u33-{tag}-{}-{}",
        std::process::id(),
        bt_platform::attention_pipe::unguessable_bits() % 1_000_000
    ));
    let scratch = Scratch(root.clone());
    let install = root.join("Folio");
    std::fs::create_dir_all(&install).unwrap();
    let exe = install.join(EXECUTABLE);
    std::fs::write(&exe, b"the running build").unwrap();
    let home = Home::of(HostPlatform::Windows, &exe).expect("an executable in a folder");
    let txn = TxnId::new([3; 16]);
    let rescue = home
        .rescue_copy(txn, OsStr::new(EXECUTABLE))
        .expect("a rescue path");
    let allocated = Journal::allocate(
        txn,
        rescue.to_str().expect("a UTF-8 path").to_owned(),
        Layout::Members(Inventories {
            old_shipped: vec![EXECUTABLE.to_owned()],
            old_present: Vec::new(),
            new: Vec::new(),
        }),
    );
    std::fs::create_dir_all(home.root()).unwrap();
    install_txn::durable_write(&home.journal(), &allocated.encode()).unwrap();
    let download = home.transaction(txn).join("download");
    std::fs::create_dir_all(&download).unwrap();
    std::fs::write(
        download.join(offer(0).asset()),
        b"half of an archive".repeat(1_000),
    )
    .unwrap();
    (scratch, exe, home, txn)
}

/// RED (U-33) — **a launch after "download, Later, close" shows the verified
/// card again, from the staged set and without any download: `Verified` with
/// the staged version, in the last active window, the staged transaction held
/// — and it does not start the day's check.**
///
/// W2 and F-17: the job owner "resumes (revalidating) or discards after 2
/// launches". On the clean VM nothing ran the pass: the journal stayed at
/// `Prepared` for ever, the start offered v0.4.7 again as new, and every
/// press ended `Busy` (`U-31-W-verify.md`). Here the real Prepare stages the
/// release, the process ends, and a later launch goes down the product's road
/// — the start's pass, `Job::after_start`, `Job::consider` — with no
/// transport at all: the card can only come from the set on disk. The offer
/// is the transaction's own and names the staged `folio.exe`'s version; the
/// launch is counted; and the 24-hour rule holds: the launch that resumed does
/// not check, so the same release is not offered twice.
///
/// MUTATION: in `Job::consider`, skip `self.launch_pass(…)` (take the
/// ordinary road) — the job waits for the check and never shows the card.
#[test]
fn a_later_launch_shows_the_verified_card_from_the_staged_set_without_downloading() {
    let Some(scene) = Scene::new("resume") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    prepared_and_closed(&scene);

    let launched = launch(&scene.exe, resumer_of(&scene), &at_launch_gathered(false));
    let job = &launched.job;
    let State::Verified(offer) = job.state() else {
        panic!("the verified card comes back: {:?}", job.state());
    };
    assert_eq!(
        offer.txn(),
        TxnId::new([1; 16]),
        "the transaction's own offer"
    );
    assert_eq!(offer.to_version(), TO, "the staged folio.exe's version");
    assert_eq!(job.card_window(), Some(1), "in the last active window");
    assert_eq!(
        crate::update_card::paint(job.state()).map(|paint| paint.verbs),
        Some(vec![
            crate::update_card::CardVerb::Restart,
            crate::update_card::CardVerb::Later
        ]),
        "the Restart to update card"
    );
    let staged = job.staged().expect("the staged transaction is the job's");
    assert_eq!(staged.journal.txn, TxnId::new([1; 16]));
    let home = scene.home();
    assert!(
        install_txn::try_hold(&home.lock(), Hold::Exclusive)
            .unwrap()
            .is_none(),
        "the job holds the transaction lock"
    );
    assert_eq!(
        journal_on_disk(&home).body.phase,
        Phase::Prepared {
            deferred_launches: 1
        },
        "the launch is counted"
    );
    assert!(
        !launched.checked.load(std::sync::atomic::Ordering::SeqCst),
        "a launch that resumed a staged set starts no check (the 24-hour rule)"
    );
    assert!(
        launched
            .said
            .as_deref()
            .is_some_and(|line| line.contains("verified again")),
        "{:?}",
        launched.said
    );
    assert_eq!(scene.installed(), before, "nothing installed was changed");
}

/// RED (U-33) — **a staged set that fails revalidation at a later launch is
/// discarded — its folder, its rescue copy and its journal gone, the lock let
/// go — and the launch checks and offers as usual.**
///
/// F-17: revalidation (hash, signature, manifest, classification) comes
/// before any resume, and a failure discards. A staged member changed between
/// the Prepare and the launch; the product's road refuses it and clears the
/// transaction, so a later press is not refused `Busy`.
///
/// MUTATION: in `update_prepare::settle_at_launch`, answer
/// `Landed::Resumed` without calling `resume` (the changed set is shown as
/// verified).
#[test]
fn a_later_launch_discards_a_staged_set_that_fails_revalidation() {
    let Some(scene) = Scene::new("resume-refused") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    prepared_and_closed(&scene);
    let set = scene
        .home()
        .members_folder(TxnId::new([1; 16]), Place::Set)
        .unwrap();
    std::fs::write(
        set.join("uninstall.cmd"),
        b"@rem changed after the Prepare\r\n",
    )
    .unwrap();

    let launched = launch(&scene.exe, resumer_of(&scene), &at_launch_gathered(false));
    assert_eq!(
        launched.job.state(),
        &State::Pending(crate::update_job::Pending::AwaitingCheck),
        "an ordinary launch, waiting for its check"
    );
    assert!(launched.job.staged().is_none());
    assert!(
        launched.checked.load(std::sync::atomic::Ordering::SeqCst),
        "a launch that discarded checks as usual"
    );
    scene.left_nothing(1);
    assert_eq!(scene.installed(), before, "nothing installed was changed");
}

/// RED (U-33) — **the second launch that finds the same staged set discards
/// it, even though the first showed its card: nothing of it is left, and that
/// launch checks as usual.**
///
/// W2: "discards after 2 launches" — a launch counts whether or not the card
/// it shows is pressed. Download, Later, close; a launch shows the verified
/// card again; Later, close; the next launch finds `deferred_launches` at its
/// limit and clears the transaction.
///
/// MUTATION: in `update_prepare::settle_at_launch`, treat
/// `AtLaunch::Discarded` as a resume of the set the journal names (or set
/// `DEFERRED_LAUNCH_LIMIT` to 3) — the second launch shows the card again.
#[test]
fn the_second_launch_that_finds_a_staged_set_discards_it() {
    let Some(scene) = Scene::new("resume-twice") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    prepared_and_closed(&scene);

    let first = launch(&scene.exe, resumer_of(&scene), &at_launch_gathered(false));
    assert!(
        matches!(first.job.state(), State::Verified(_)),
        "{:?}",
        first.job.state()
    );
    drop(first);

    let second = launch(&scene.exe, resumer_of(&scene), &at_launch_gathered(false));
    assert!(
        !matches!(second.job.state(), State::Verified(_)),
        "{:?}",
        second.job.state()
    );
    assert!(second.job.staged().is_none());
    assert!(
        second.checked.load(std::sync::atomic::Ordering::SeqCst),
        "a launch that discarded checks as usual"
    );
    scene.left_nothing(1);
    assert_eq!(scene.installed(), before, "nothing installed was changed");
}

/// RED (U-33) — **a launch after a download died at `Allocated` sweeps it
/// while the lock is free: `H\<txn>` and the journal are gone, the install is
/// untouched, and the launch checks and offers as usual.**
///
/// W1 and F-17: at `Allocated`, "if the lock is free: delete `H\<txn>` and the
/// journal. Old starts." On the clean VM about 120 MB and the journal stayed
/// for ever and every later press was refused `Busy`, because nothing ran the
/// job owner's pass. The transaction here is the Windows Prepare's
/// `Allocated` shape; the launch goes down the product's road.
///
/// MUTATION: in `update_startup::run`, answer `waiting: None` for every class
/// (the start leaves nothing for the job owner) — the journal and `H\<txn>`
/// stay.
#[test]
fn a_later_launch_sweeps_a_download_that_died_at_allocated() {
    let (_scratch, exe, home, txn) = allocated_scene("sweep");
    let launched = launch(
        &exe,
        crate::update_prepare::no_resume(),
        &at_launch_gathered(false),
    );
    assert!(
        !home.transaction(txn).exists(),
        "the transaction's folder is removed"
    );
    assert!(!home.journal().exists(), "the journal is removed");
    assert!(
        install_txn::try_hold(&home.lock(), Hold::Exclusive)
            .unwrap()
            .is_some(),
        "the lock is let go"
    );
    assert_eq!(std::fs::read(&exe).unwrap(), b"the running build");
    assert_eq!(
        launched.job.state(),
        &State::Pending(crate::update_job::Pending::AwaitingCheck)
    );
    assert!(
        launched.checked.load(std::sync::atomic::Ordering::SeqCst),
        "the launch checks as usual"
    );
}

/// RED (U-33) — **while another holder has the transaction lock, a launch
/// changes nothing and offers nothing, even with a newer release known.**
///
/// The launch pass's `Busy`: the transaction is somebody else's now (another
/// Folio of this install, or its applier), so the job owner here neither
/// sweeps nor counts it, and no card is raised this launch — a card whose
/// press could only be refused `Busy`.
///
/// MUTATION: in `Job::launch_pass`, answer `Landed::Busy` as an ordinary
/// launch (`check(); Pass::Ordinary`) — `v0.4.7` is offered.
#[test]
fn a_later_launch_that_finds_the_lock_held_changes_nothing_and_offers_nothing() {
    let (_scratch, exe, home, txn) = allocated_scene("busy");
    let journal = std::fs::read(home.journal()).unwrap();
    let holder = install_txn::try_hold(&home.lock(), Hold::Exclusive)
        .unwrap()
        .expect("the lock is free before the test holds it");
    let mut launched = launch(
        &exe,
        crate::update_prepare::no_resume(),
        &at_launch_gathered(true),
    );
    assert_eq!(launched.job.state(), &State::Idle);
    assert_eq!(launched.job.card_window(), None, "no card");
    assert_eq!(
        launched.said.as_deref(),
        Some("Folio: update job — no offer: another update holds this installation")
    );
    // The check settles later in the same launch: still no offer.
    launched.job.consider(
        at_launch_gathered(true),
        &Presenters {
            visited: &[1],
            open: &[1],
            quake: None,
        },
        || TxnId::new([0xAA; 16]),
    );
    assert_eq!(launched.job.state(), &State::Idle);
    assert_eq!(std::fs::read(home.journal()).unwrap(), journal);
    assert!(home.transaction(txn).join("download").exists());
    drop(holder);
}

// ── the layout's Prepare (U-41a1) ───────────────────────────────────────────

/// What the road asked of a layout's Prepare, and the journal on disk when
/// it asked: `None` where there was none yet.
type PrepareCall = (
    &'static str,
    Option<(PhaseKind, crate::update_txn::Adapter)>,
);

/// **A fake layout's Prepare** — the harness U-41b and U-41c build theirs on
/// (managed-update §6): every call recorded with the journal on disk,
/// delegated to Folio's own layout unless it is told to refuse with `refuse`.
#[derive(Clone, Default)]
struct PrepareRecorder {
    calls: Arc<Mutex<Vec<PrepareCall>>>,
    refuse: Option<Stop>,
}

impl PrepareRecorder {
    fn note(&self, point: &'static str, home: &Home) {
        let on_disk = std::fs::read(home.journal()).ok().map(|bytes| {
            let journal = Journal::parse(&bytes).unwrap();
            (journal.body.phase.kind(), journal.body.adapter)
        });
        self.calls.lock().unwrap().push((point, on_disk));
    }

    fn calls(&self) -> Vec<PrepareCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl PreparePoint for PrepareRecorder {
    fn allocated(&self, running: &Running) -> Layout {
        self.calls.lock().unwrap().push(("allocated", None));
        Ours.allocated(running)
    }

    fn prepare(
        &self,
        road: &Road<'_>,
        home: &Home,
        running: &Running,
        rescue: &Path,
    ) -> Result<Layout, Stop> {
        self.note("prepare", home);
        match &self.refuse {
            Some(stop) => Err(*stop),
            None => Ours.prepare(road, home, running, rescue),
        }
    }
}

/// RED (U-41a1, managed-update §1.1 R1–R2, §1.3) — **the press calls the
/// Prepare of the layout its channel names once, with the journal at
/// `Allocated` on disk and naming that adapter, and records what the layout
/// answers at `Prepared`.**
///
/// The fake layout delegates to Folio's own, so the road is the real U-20
/// Prepare over a real signed release; what the harness adds is the proof
/// that the road reaches the layout through its adapter, at the one boundary
/// the note gives `Prepare`, and that the adapter the press chose is in the
/// journal before anything is acquired.
///
/// MUTATION: in `prepare_on`, acquire through `Ours` in place of the layout
/// the adapter names (`layout.prepare` → `Ours.prepare`) — the recorder hears
/// no `prepare`.
#[test]
fn the_press_calls_the_prepare_of_the_layout_its_adapter_names_once_at_allocated() {
    let Some(scene) = Scene::new("layout-prepare") else {
        return refused_off_windows();
    };
    let recorder = PrepareRecorder::default();
    let driver = scene
        .driver(TestTools::real())
        .laid_out(Arc::new(recorder.clone()));
    let job = press(&driver, Arc::new(Release::of(scene.archive.clone())), 1);
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );
    assert_eq!(
        recorder.calls(),
        vec![
            ("allocated", None),
            (
                "prepare",
                Some((PhaseKind::Allocated, crate::update_txn::Adapter::Ours))
            ),
        ]
    );
    let journal = journal_on_disk(&scene.home());
    assert_eq!(
        journal.body.phase,
        Phase::Prepared {
            deferred_launches: 0
        }
    );
    assert_eq!(journal.body.adapter, crate::update_txn::Adapter::Ours);
    assert!(
        !inventories(&journal).new.is_empty(),
        "the layout's answer is recorded"
    );
}

/// RED (U-41a1, managed-update §3.2 F4/F12's journal column) — **a layout
/// that refuses its Prepare abandons the transaction: the card says its stop
/// and that nothing changed, and nothing of the transaction is left — no
/// folder, no journal, the lock free — with the install untouched.**
///
/// The abandonment is the road's, common to every layout: `Abandoned`, then
/// cleared (`update_prepare::abandon`), as for every other Prepare failure.
///
/// MUTATION: in `prepare_on`, acquire through `Ours` in place of the layout
/// the adapter names — the refusal is never met and the job is `Verified`.
#[test]
fn a_layout_that_refuses_its_prepare_abandons_and_leaves_nothing() {
    let Some(scene) = Scene::new("layout-refuses") else {
        return refused_off_windows();
    };
    let before = scene.installed();
    let recorder = PrepareRecorder {
        refuse: Some(Stop::Copy),
        ..PrepareRecorder::default()
    };
    let driver = scene
        .driver(TestTools::real())
        .laid_out(Arc::new(recorder.clone()));
    let job = press(&driver, Arc::new(Release::of(scene.archive.clone())), 2);
    failed_with(&job, 2, Stop::Copy);
    scene.left_nothing(2);
    assert_eq!(scene.installed(), before, "nothing installed was changed");
    assert_eq!(
        recorder.calls().last(),
        Some(&(
            "prepare",
            Some((PhaseKind::Allocated, crate::update_txn::Adapter::Ours))
        ))
    );
}
