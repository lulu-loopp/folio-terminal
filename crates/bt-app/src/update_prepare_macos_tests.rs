//! **The macOS Prepare, over real folders, real disk images and real signed
//! synthetic bundles** (macOS), and its pure parts everywhere.
//!
//! Every bundle is built here: an `arm64` executable compiled from one line of
//! C (`/usr/bin/cc`), an `Info.plist`, one resource, ad-hoc signed with
//! `codesign -s -`. Every image is made here with `hdiutil create`, every
//! attach lands under the test's own temporary folder, and every test's
//! [`fixture::Scratch`] detaches whatever the mount table still lists under it
//! before it removes it. The download is a stand-in transport that writes the
//! file it is asked for ([`Release`]); nothing leaves this machine. An ad-hoc
//! signature satisfies no Developer ID requirement, so the stand-in tools
//! ([`TestTools`]) hold a bundle to the requirement a test states — the
//! synthetic bundles' identifier — through the same `codesign --verify
//! --strict --deep --all-architectures -R` the product's check runs.

use super::*;

use std::ffi::OsStr;
use std::sync::Mutex;

use bt_persist::UpdateCheckV1;
use bt_platform::HostPlatform;

use crate::update_job::{
    Bytes, Failure, Fetching, Gathered, Job, Offer, Presenters, Request, State, Verb,
};
use crate::update_txn::{Actor, Effect, HeaderOutcome, Phase, PhaseKind, may};

/// Real images, bundles and mounts, shared with `update_startup`'s tests.
pub(crate) mod fixture {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::process::Output;

    /// The synthetic bundles' identifier.
    pub(crate) const IDENTIFIER: &str = "io.github.lulu-loopp.folio.u27-test";

    /// The requirement a test holds a synthetic bundle to, standing for the
    /// running bundle's designated requirement.
    pub(crate) const REQUIREMENT: &str = "identifier \"io.github.lulu-loopp.folio.u27-test\"";

    /// Whether this is the platform these tests run their real half on.
    pub(crate) fn on_macos() -> bool {
        bt_platform::host_platform() == bt_platform::HostPlatform::MacOs
    }

    /// Run `program` with `arguments` and answer what it did.
    pub(crate) fn answer(program: &str, arguments: &[&OsStr]) -> Output {
        bt_platform::quiet_command(program)
            .args(arguments)
            .output()
            .unwrap_or_else(|error| panic!("{program} did not start: {error}"))
    }

    /// Run `program`, which must succeed.
    pub(crate) fn run(program: &str, arguments: &[&OsStr]) -> Output {
        let output = answer(program, arguments);
        assert!(
            output.status.success(),
            "{program} {arguments:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    /// **A folder of the test's own under the temporary directory** that, when
    /// dropped, detaches every image the mount table lists under it (forced,
    /// by its mount point) and then removes it.
    pub(crate) struct Scratch {
        pub(crate) root: PathBuf,
    }

    impl Scratch {
        pub(crate) fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "bt-u27-{tag}-{}-{}",
                std::process::id(),
                bt_platform::attention_pipe::unguessable_bits() % 1_000_000
            ));
            std::fs::create_dir_all(&root).unwrap();
            // The real path: `/var` is `/private/var`, and a mount point is
            // reported by its real path.
            Self {
                root: std::fs::canonicalize(&root).unwrap(),
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            detach_all(&self.root);
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// **Detaches every image mounted under its folder when dropped** — for a
    /// test whose folder another guard removes.
    pub(crate) struct Detach(pub(crate) PathBuf);

    impl Drop for Detach {
        fn drop(&mut self) {
            detach_all(&self.0);
        }
    }

    /// Every mount under `root`, detached by its mount point, forced.
    fn detach_all(root: &Path) {
        for point in mounted(root) {
            let _ = answer(
                "/usr/bin/hdiutil",
                &[
                    OsStr::new("detach"),
                    OsStr::new("-force"),
                    point.as_os_str(),
                ],
            );
        }
    }

    /// Every mount point under `root`, from the mount table.
    pub(crate) fn mounted(root: &Path) -> Vec<PathBuf> {
        bt_platform::macos_update::mounts_under(root).unwrap_or_default()
    }

    /// **A signed synthetic bundle** `<parent>/<name>` of `version`, whose one
    /// resource says `notice`.
    pub(crate) fn bundle(parent: &Path, name: &str, version: &str, notice: &str) -> PathBuf {
        let bundle = parent.join(name);
        let macos = bundle.join("Contents").join("MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(bundle.join("Contents").join("Resources")).unwrap();
        let source = parent.join(format!(".{name}.c"));
        std::fs::write(&source, b"int main(void) { return 0; }\n").unwrap();
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
                 </dict></plist>\n"
            ),
        )
        .unwrap();
        std::fs::write(
            bundle.join("Contents").join("Resources").join("notice.txt"),
            notice,
        )
        .unwrap();
        sign(&bundle);
        bundle
    }

    /// Ad-hoc sign `bundle` again.
    pub(crate) fn sign(bundle: &Path) {
        run(
            "/usr/bin/codesign",
            &[
                OsStr::new("--force"),
                OsStr::new("--sign"),
                OsStr::new("-"),
                bundle.as_os_str(),
            ],
        );
    }

    /// `CFBundleShortVersionString` of `bundle`.
    pub(crate) fn version_of(bundle: &Path) -> String {
        let plist = bundle.join("Contents").join("Info.plist");
        let output = run(
            "/usr/bin/plutil",
            &[
                OsStr::new("-extract"),
                OsStr::new("CFBundleShortVersionString"),
                OsStr::new("raw"),
                OsStr::new("-o"),
                OsStr::new("-"),
                plist.as_os_str(),
            ],
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// **A compressed read-only image of `folder`** at `image`.
    pub(crate) fn image_of(folder: &Path, image: &Path) -> PathBuf {
        run(
            "/usr/bin/hdiutil",
            &[
                OsStr::new("create"),
                OsStr::new("-quiet"),
                OsStr::new("-volname"),
                OsStr::new("U27"),
                OsStr::new("-srcfolder"),
                folder.as_os_str(),
                OsStr::new("-fs"),
                OsStr::new("HFS+"),
                OsStr::new("-format"),
                OsStr::new("UDZO"),
                image.as_os_str(),
            ],
        );
        image.to_path_buf()
    }

    /// **A small empty image** at `image`.
    pub(crate) fn blank_image(image: &Path) -> PathBuf {
        run(
            "/usr/bin/hdiutil",
            &[
                OsStr::new("create"),
                OsStr::new("-quiet"),
                OsStr::new("-size"),
                OsStr::new("2m"),
                OsStr::new("-fs"),
                OsStr::new("HFS+"),
                OsStr::new("-volname"),
                OsStr::new("U27"),
                image.as_os_str(),
            ],
        );
        image.to_path_buf()
    }

    /// **Attach `image` read-only at a fresh mount point inside `mount_dir`**
    /// (made if missing) with no record kept — the way a dead Prepare leaves
    /// one — and answer the mount point.
    pub(crate) fn attach(image: &Path, mount_dir: &Path) -> PathBuf {
        std::fs::create_dir_all(mount_dir).unwrap();
        run(
            "/usr/bin/hdiutil",
            &[
                OsStr::new("attach"),
                OsStr::new("-nobrowse"),
                OsStr::new("-readonly"),
                OsStr::new("-noautoopen"),
                OsStr::new("-mountrandom"),
                mount_dir.as_os_str(),
                image.as_os_str(),
            ],
        );
        let points = mounted(mount_dir);
        assert_eq!(points.len(), 1, "one mount under {}", mount_dir.display());
        points[0].clone()
    }

    /// Every file under `root`, with its bytes, sorted by path.
    pub(crate) fn listing(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(base: &Path, at: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in std::fs::read_dir(at).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(base, &path, out);
                } else {
                    out.push((
                        path.strip_prefix(base).unwrap().to_path_buf(),
                        std::fs::read(&path).unwrap(),
                    ));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }
}

use fixture::{Scratch, answer, attach, blank_image, bundle, image_of, listing, mounted, on_macos};

/// The offer every test presses: `v0.4.7`, for macOS.
fn offer(txn: u8) -> Offer {
    Offer::mint(TxnId::new([txn; 16]), "v0.4.7", HostPlatform::MacOs).expect("a release tag")
}

/// Run `body` on a worker the thread door started, and wait for it.
fn on_a_worker<T: Send + 'static>(body: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
    let worker = bt_platform::spawn_at_priority(
        "bt-u27-test",
        bt_platform::ThreadPriority::BelowNormal,
        body,
    )
    .expect("the thread door starts a thread");
    match worker.join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

// ── the stand-ins ───────────────────────────────────────────────────────────

/// **The tools a test holds a Prepare to**: `codesign --verify --strict
/// --deep --all-architectures -R=<requirement>`, the real `ditto` and the real
/// rescue clone, and download folders under the test's own folder — each
/// able to be told to fail, or (the copy) to alter what it made.
struct TestTools {
    requirement: String,
    downloads: PathBuf,
    made: Mutex<Vec<PathBuf>>,
    fail_copy: bool,
    alter_copy: bool,
    fail_rescue: bool,
}

impl TestTools {
    fn new(scratch: &Scratch) -> Self {
        Self {
            requirement: fixture::REQUIREMENT.to_owned(),
            downloads: scratch.root.join("downloads"),
            made: Mutex::new(Vec::new()),
            fail_copy: false,
            alter_copy: false,
            fail_rescue: false,
        }
    }

    /// The download folders it made.
    fn folders(&self) -> Vec<PathBuf> {
        self.made.lock().unwrap().clone()
    }
}

impl Tools for TestTools {
    fn verify(&self, _worker: &WorkerCtx, bundle: &Path) -> Result<(), String> {
        let requirement = format!("-R={}", self.requirement);
        let output = answer(
            "/usr/bin/codesign",
            &[
                OsStr::new("--verify"),
                OsStr::new("--strict"),
                OsStr::new("--deep"),
                OsStr::new("--all-architectures"),
                OsStr::new(&requirement),
                bundle.as_os_str(),
            ],
        );
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn copy(&self, worker: &WorkerCtx, from: &Path, to: &Path) -> Result<(), String> {
        if self.fail_copy {
            return Err("the stand-in copy refuses".to_owned());
        }
        System.copy(worker, from, to)?;
        if self.alter_copy {
            // Between the copy and its second check: one sealed file changed.
            let notice = to.join("Contents").join("Resources").join("notice.txt");
            std::fs::write(&notice, b"changed after the copy").map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn rescue(&self, worker: &WorkerCtx, old: &Path, clone: &Path) -> Result<(), String> {
        if self.fail_rescue {
            return Err("the stand-in clone refuses".to_owned());
        }
        System.rescue(worker, old, clone)
    }

    fn download_folder(&self, _near: &Path) -> Result<PathBuf, String> {
        let mut made = self.made.lock().unwrap();
        let folder = self.downloads.join(made.len().to_string());
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        made.push(folder.clone());
        Ok(folder)
    }
}

/// **The release, as a stand-in transport**: the image's bytes and the
/// checksum document, written under the name each request asks for.
struct Release {
    image: PathBuf,
    sums: String,
    /// The file name whose fetch fails.
    refuse: Option<String>,
    fetched: Mutex<Vec<String>>,
}

impl Transport for Release {
    fn fetch(
        &self,
        request: &Request,
        into: &Path,
        fetching: &Fetching,
    ) -> Result<PathBuf, String> {
        self.fetched.lock().unwrap().push(request.file_name.clone());
        if self.refuse.as_deref() == Some(request.file_name.as_str()) {
            return Err("the stand-in server said no".to_owned());
        }
        let target = into.join(&request.file_name);
        if request.file_name.ends_with(".dmg") {
            let bytes = std::fs::read(&self.image).map_err(|e| e.to_string())?;
            std::fs::write(&target, &bytes).map_err(|e| e.to_string())?;
            let length = bytes.len() as u64;
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

/// `shasum`'s line for `image` under the offer's asset name, beside another
/// file's.
fn sums_for(image: &Path) -> String {
    let digest = bt_winres::digest::hex(&bt_winres::digest::sha256(&std::fs::read(image).unwrap()));
    format!(
        "{}  folio-0.4.7-windows-x64.zip\n{digest}  {}\n",
        "0".repeat(64),
        offer(0).asset()
    )
}

/// **A running bundle and a release**: `<scratch>/apps/<running>` at 0.4.6,
/// and an image whose `Folio.app` is 0.4.7 (`dress` may change it, re-signed
/// after), with its checksum document.
struct Scene {
    scratch: Scratch,
    apps: PathBuf,
    running: PathBuf,
    image: PathBuf,
}

impl Scene {
    fn new(tag: &str, running: &str, dress: impl FnOnce(&Path)) -> Self {
        let scratch = Scratch::new(tag);
        let apps = scratch.root.join("apps");
        std::fs::create_dir_all(&apps).unwrap();
        let running = bundle(&apps, running, "0.4.6", "the running build");
        let source = scratch.root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        let new = bundle(&source, IMAGE_BUNDLE, "0.4.7", "the new build");
        dress(&new);
        let image = image_of(&source, &scratch.root.join("release.dmg"));
        Self {
            scratch,
            apps,
            running,
            image,
        }
    }

    fn release(&self) -> Arc<Release> {
        Arc::new(Release {
            image: self.image.clone(),
            sums: sums_for(&self.image),
            refuse: None,
            fetched: Mutex::new(Vec::new()),
        })
    }

    fn home(&self) -> Home {
        Home::for_bundle(&self.running).expect("an .app with a parent")
    }
}

/// **Press Update on a job offering `v0.4.7` with `driver`**, and wait for
/// the job to reach `Verified` or `Failed` — reading the reports the way the
/// window thread does (`Job::drain_progress`).
fn press(driver: &MacPrepare, transport: SharedTransport, txn: u8) -> Job<u32> {
    let mut job = Job::with_offers(true);
    job.consider(
        Gathered {
            check: Some((
                UpdateCheckV1 {
                    latest_tag: Some("v0.4.7".to_owned()),
                    ..UpdateCheckV1::default()
                },
                true,
            )),
            channel: Some(Channel::Ours),
            running: "0.4.6",
            capable: true,
            trial: false,
            platform: HostPlatform::MacOs,
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
    job.answer_verb(Verb::Press, driver, &transport)
        .expect("the press is taken");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        job.drain_progress();
        if matches!(job.state(), State::Verified(_) | State::Failed(..)) {
            return job;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the Prepare did not finish: {:?}",
            job.state()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn driver(scene: &Scene, tools: &Arc<TestTools>) -> MacPrepare {
    MacPrepare::with(
        scene.running.clone(),
        Arc::clone(tools) as Arc<dyn Tools>,
        Some(Channel::Ours),
    )
}

/// What the Prepare left behind for the job, read off the disk.
fn journal_on_disk(home: &Home) -> Journal {
    Journal::parse(&std::fs::read(home.journal()).expect("a journal")).expect("a whole journal")
}

// ── the pure parts ──────────────────────────────────────────────────────────

/// RED (U-27) — **the checksum document's line for the offer's own asset is
/// the one the image is held to**: `shasum`'s two spellings, another file's
/// line ignored, no line or two disagreeing lines no answer.
///
/// MUTATION: in `sum_for`, take the first well-formed line whatever it names
/// (drop `name != asset`).
#[test]
fn the_checksum_line_is_the_offers_own_asset() {
    let asset = "Folio-0.4.7-macos-arm64.dmg";
    let ours = "ab".repeat(32);
    let theirs = "cd".repeat(32);
    let document = format!("{theirs}  Folio-0.4.6-macos-arm64.dmg\n{ours}  {asset}\n");
    assert_eq!(sum_for(&document, asset), Some(ours.clone()));
    let binary = format!("{} *{asset}\r\n", ours.to_uppercase());
    assert_eq!(sum_for(&binary, asset), Some(ours.clone()));
    assert_eq!(sum_for(&format!("{theirs}  other.dmg\n"), asset), None);
    let twice = format!("{ours}  {asset}\n{theirs}  {asset}\n");
    assert_eq!(sum_for(&twice, asset), None, "two answers are none");
    assert_eq!(
        sum_for(&format!("abc  {asset}\n"), asset),
        None,
        "not a sum"
    );
}

/// RED (U-27) — **O clears a transaction it gave up — the image detached
/// first — and an ordinary start detaches before it deletes** (the
/// coordinator's ruling, U-17's debt 7).
///
/// MUTATION: drop the `START_DELETES` `DetachMount` row from `EFFECT_RIGHTS`.
#[test]
fn the_detach_is_granted_where_a_transaction_is_deleted() {
    for phase in [PhaseKind::Allocated, PhaseKind::Abandoned] {
        for effect in [
            Effect::DetachMount,
            Effect::DeleteTxnDir,
            Effect::DeleteJournal,
        ] {
            assert!(may(Actor::Old, effect, phase), "O {effect:?} in {phase:?}");
        }
    }
    for phase in [
        PhaseKind::Allocated,
        PhaseKind::Prepared,
        PhaseKind::Abandoned,
        PhaseKind::Retired,
    ] {
        assert!(may(Actor::Start, Effect::DetachMount, phase), "{phase:?}");
    }
    assert!(!may(Actor::Old, Effect::DeleteTxnDir, PhaseKind::Prepared));
    for action in [
        crate::update_txn::StartAction::Retire,
        crate::update_txn::StartAction::Discard,
    ] {
        let effects = action.effects();
        let detach = effects.iter().position(|e| *e == Effect::DetachMount);
        let delete = effects.iter().position(|e| *e == Effect::DeleteTxnDir);
        assert!(
            detach.is_some() && detach < delete,
            "{action:?}: {effects:?}"
        );
    }
}

// ── the road, for real ──────────────────────────────────────────────────────

/// RED (U-27) — **a bundle named `Folio Beta.app` updates only itself**: its
/// home, its stage and its rescue carry its own name, and a sibling
/// `Folio.app` is never touched.
///
/// C.1: "on macOS every path is derived from the actual bundle basename, so a
/// `Folio Test.app` updates itself and not `/Applications/Folio.app`". The
/// image's bundle is `Folio.app`; the transaction must not take its name from
/// there. The whole road runs: download (stand-in), hash, a real attach under
/// the home, the check, `ditto`, the check again, the real clone, `Prepared`.
///
/// MUTATION: in `eligible`, find the home from the image's bundle name
/// (`Home::for_bundle(&bundle.with_file_name(IMAGE_BUNDLE))`).
#[test]
fn renamed_bundle_updates_only_itself() {
    if !on_macos() {
        return;
    }
    let scene = Scene::new("renamed", "Folio Beta.app", |_| {});
    let sibling = bundle(&scene.apps, "Folio.app", "0.4.6", "a sibling");
    let sibling_before = listing(&sibling);
    let tools = Arc::new(TestTools::new(&scene.scratch));
    let job = press(&driver(&scene, &tools), scene.release(), 0x27);
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );

    let home = scene.home();
    let txn = TxnId::new([0x27; 16]);
    assert_eq!(home.root(), scene.apps.join(".Folio Beta.app.folio-update"));
    let stage = home.stage_bundle(txn).unwrap();
    let rescue = home.rescue_bundle(txn).unwrap();
    assert!(stage.ends_with("stage/Folio Beta.app") && stage.is_dir());
    assert!(rescue.ends_with("rescue/Folio Beta.app") && rescue.is_dir());
    assert_eq!(fixture::version_of(&stage), "0.4.7");
    assert_eq!(fixture::version_of(&rescue), "0.4.6");
    assert!(!scene.apps.join(".Folio.app.folio-update").exists());
    assert_eq!(
        listing(&sibling),
        sibling_before,
        "the sibling is untouched"
    );
    assert_eq!(fixture::version_of(&scene.running), "0.4.6");

    let journal = journal_on_disk(&home);
    assert_eq!(journal.txn, txn);
    assert_eq!(
        journal.body.phase,
        Phase::Prepared {
            deferred_launches: 0
        }
    );
    assert_eq!(journal.header().outcome, HeaderOutcome::None);
    assert_eq!(journal.rescue, rescue.to_str().unwrap());
    let Layout::Bundle { old, new } = &journal.body.layout else {
        panic!("Prepared records both bundles: {:?}", journal.body.layout);
    };
    assert_eq!(
        (old.version.as_str(), new.version.as_str()),
        ("0.4.6", "0.4.7")
    );
    assert_eq!(job.staged().expect("the job holds it").journal, journal);
    assert!(mounted(home.root()).is_empty(), "the image is detached");
    assert!(tools.folders().iter().all(|folder| !folder.exists()));
}

/// RED (U-27) — **a bundle whose folder is a read-only mount — as a
/// translocated bundle's is — is refused as not writable, and nothing is
/// written or fetched.**
///
/// C7: "a translocated bundle (run from a quarantined download without being
/// moved) sits on a read-only randomized mount: Not writable, releases page".
/// The mount here is a small read-only image attached under the test's
/// folder, which is the property translocation has.
///
/// MUTATION: in `eligible`, drop the `may_write_into` answer (take every
/// folder as writable).
#[test]
fn a_translocated_bundle_is_not_writable() {
    if !on_macos() {
        return;
    }
    let scratch = Scratch::new("translocated");
    let source = scratch.root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    bundle(&source, "Folio.app", "0.4.6", "a translocated build");
    let image = image_of(&source, &scratch.root.join("translocated.dmg"));
    let point = attach(&image, &scratch.root.join("randomized"));
    let running = point.join("Folio.app");
    let before = listing(&point);

    assert_eq!(
        eligible(&running, Some(Channel::Ours)),
        Err(NotEligible::NotWritable)
    );
    let writable = bundle(&scratch.root, "Folio.app", "0.4.6", "a writable build");
    assert!(eligible(&writable, Some(Channel::Ours)).is_ok());
    assert_eq!(
        eligible(&writable, Some(Channel::NotOurs)),
        Err(NotEligible::NotOurs)
    );

    let tools = Arc::new(TestTools::new(&scratch));
    let release = Arc::new(Release {
        image: image.clone(),
        sums: sums_for(&image),
        refuse: None,
        fetched: Mutex::new(Vec::new()),
    });
    let driver = MacPrepare::with(
        running.clone(),
        Arc::clone(&tools) as Arc<dyn Tools>,
        Some(Channel::Ours),
    );
    let job = press(&driver, Arc::clone(&release) as SharedTransport, 0x28);
    assert!(
        matches!(
            job.state(),
            State::Failed(_, Failure::Stopped(Stop::NotWritable))
        ),
        "{:?}",
        job.state()
    );
    assert!(
        release.fetched.lock().unwrap().is_empty(),
        "nothing fetched"
    );
    assert!(tools.folders().is_empty(), "no download folder");
    assert_eq!(listing(&point), before, "nothing written beside it");
    assert!(!point.join(".Folio.app.folio-update").exists());
}

/// RED (U-27) — **the quarantine a bundle carries on the image is on its copy
/// in `stage/`** (§H: quarantine through the whole path).
///
/// C7: "the updater … must change nothing inside or on the copied bundle — no
/// plist edit, no attribute stripped"; `ditto` carries the attribute, and the
/// check holds the bundle's seal with it on.
///
/// Measured on macOS 26: `ditto --noextattr` and `--noqtn` both keep the
/// quarantine (the system's copy keeps it), so the mutation strips it
/// explicitly.
///
/// MUTATION: in `macos_update::copy_bundle`, after `ditto`, remove the
/// attribute from the copy (`/usr/bin/xattr -d com.apple.quarantine <to>`).
#[test]
fn quarantine_is_preserved_end_to_end() {
    if !on_macos() {
        return;
    }
    // No space: `xattr -p` prints one as `\x20`.
    const QUARANTINE: &str = "0081;66f5a1b2;FolioTest;";
    let scene = Scene::new("quarantine", "Folio.app", |new| {
        fixture::run(
            "/usr/bin/xattr",
            &[
                OsStr::new("-w"),
                OsStr::new("com.apple.quarantine"),
                OsStr::new(QUARANTINE),
                new.as_os_str(),
            ],
        );
    });
    let tools = Arc::new(TestTools::new(&scene.scratch));
    let job = press(&driver(&scene, &tools), scene.release(), 0x29);
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );
    let stage = scene.home().stage_bundle(TxnId::new([0x29; 16])).unwrap();
    let read = fixture::run(
        "/usr/bin/xattr",
        &[
            OsStr::new("-p"),
            OsStr::new("com.apple.quarantine"),
            stage.as_os_str(),
        ],
    );
    assert_eq!(String::from_utf8_lossy(&read.stdout).trim(), QUARANTINE);
}

/// RED (U-27) — **the copy is verified again where it lies**: a copy altered
/// between the two checks is refused, and the transaction leaves nothing.
///
/// §C.2 step 6: "re-verify the staged copy in place … because what gets
/// installed must be what was checked". The stand-in copy changes one sealed
/// file after `ditto`; only the second check can see it.
///
/// MUTATION: in `copy_verified`, drop the second `check` (answer `on_image`).
#[test]
fn the_copy_is_verified_twice() {
    if !on_macos() {
        return;
    }
    let scene = Scene::new("twice", "Folio.app", |_| {});
    let tools = Arc::new(TestTools {
        alter_copy: true,
        ..TestTools::new(&scene.scratch)
    });
    let job = press(&driver(&scene, &tools), scene.release(), 0x2a);
    assert!(
        matches!(
            job.state(),
            State::Failed(_, Failure::Stopped(Stop::Identity))
        ),
        "{:?}",
        job.state()
    );
    let home = scene.home();
    assert!(!home.transaction(TxnId::new([0x2a; 16])).exists());
    assert!(!home.journal().exists());
    assert!(mounted(home.root()).is_empty());
}

/// RED (U-27) — **every refusal after the journal exists detaches the image
/// and removes the transaction**: a download, a checksum, an attach, an
/// identity, a copy and a clone that fail each leave no mount, no `H/<txn>`,
/// no journal and no download.
///
/// The brief's M1 row, taken by O at once: "detach any mount under `H`,
/// delete `H/<txn>` and the journal". The identity and the copy fail with the
/// image attached; the clone fails after it came off.
///
/// MUTATION: in `abandon`, record `Abandoned` and return without `clear`.
#[test]
fn every_failure_road_detaches_and_removes_the_txn() {
    if !on_macos() {
        return;
    }
    let scene = Scene::new("roads", "Folio.app", |_| {});
    let home = scene.home();
    let garbage = scene.scratch.root.join("garbage.dmg");
    std::fs::write(&garbage, b"not a disk image").unwrap();
    let roads: [(Stop, u8); 6] = [
        (Stop::Download, 0x31),
        (Stop::Sums, 0x32),
        (Stop::Mount, 0x33),
        (Stop::Identity, 0x34),
        (Stop::Copy, 0x35),
        (Stop::Clone, 0x36),
    ];
    for (stop, txn) in roads {
        let mut release = Release {
            image: scene.image.clone(),
            sums: sums_for(&scene.image),
            refuse: None,
            fetched: Mutex::new(Vec::new()),
        };
        let mut tools = TestTools::new(&scene.scratch);
        tools.downloads = scene.scratch.root.join(format!("downloads-{txn}"));
        match stop {
            Stop::Download => release.refuse = Some(offer(txn).asset().to_owned()),
            Stop::Sums => release.sums = format!("{}  {}\n", "e".repeat(64), offer(txn).asset()),
            Stop::Mount => {
                release.image = garbage.clone();
                release.sums = sums_for(&garbage);
            }
            Stop::Identity => tools.requirement = "identifier \"another.publisher\"".to_owned(),
            Stop::Copy => tools.fail_copy = true,
            _ => tools.fail_rescue = true,
        }
        let tools = Arc::new(tools);
        let job = press(&driver(&scene, &tools), Arc::new(release), txn);
        assert_eq!(
            job.state(),
            &State::Failed(offer(txn), Failure::Stopped(stop)),
            "{stop:?}"
        );
        assert!(mounted(home.root()).is_empty(), "{stop:?}: a mount is left");
        assert!(
            !home.transaction(TxnId::new([txn; 16])).exists(),
            "{stop:?}: H/<txn> is left"
        );
        assert!(!home.journal().exists(), "{stop:?}: the journal is left");
        assert!(
            tools.folders().iter().all(|folder| !folder.exists()),
            "{stop:?}: a download is left"
        );
    }
}

// ── a later launch ──────────────────────────────────────────────────────────

/// RED (U-27) — **a prepared transaction is counted at the first launch that
/// does not resume it and discarded at the second — its image detached before
/// its folder goes — and a dead Prepare's `Allocated` one is swept.**
///
/// (b).1 F-17: "It increments `deferred_launches` at each launch that does not
/// resume, discards at 2"; M1: "detach any mount whose mount point is under
/// `H`; delete `H/<txn>` and the journal". A mount left under the
/// transaction's folder — the way a Prepare that died after its attach leaves
/// one — must not stop the deletion.
///
/// MUTATION: in `at_launch`, answer `Counted` without clearing when the count
/// reaches `Abandoned`.
#[test]
fn a_deferred_transaction_is_discarded_at_two_launches() {
    if !on_macos() {
        return;
    }
    let scene = Scene::new("deferred", "Folio.app", |_| {});
    let tools = Arc::new(TestTools::new(&scene.scratch));
    let job = press(&driver(&scene, &tools), scene.release(), 0x41);
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );
    drop(job);
    let home = scene.home();
    let txn = TxnId::new([0x41; 16]);

    let first = on_a_worker({
        let home = home.clone();
        move |worker| match at_launch(worker, &home).unwrap() {
            AtLaunch::Counted(staged) => staged.journal.body.phase,
            _ => panic!("the first launch counts"),
        }
    });
    assert_eq!(
        first,
        Phase::Prepared {
            deferred_launches: 1
        }
    );
    assert_eq!(journal_on_disk(&home).body.phase, first);

    let blank = blank_image(&scene.scratch.root.join("left.dmg"));
    attach(&blank, &home.mount_point(txn).unwrap());
    let second = on_a_worker({
        let home = home.clone();
        move |worker| matches!(at_launch(worker, &home).unwrap(), AtLaunch::Discarded)
    });
    assert!(second, "the second launch discards");
    assert!(mounted(home.root()).is_empty(), "the image is detached");
    assert!(!home.transaction(txn).exists());
    assert!(!home.journal().exists());

    // M1: a Prepare that died at `Allocated`, with its image still attached.
    let dead = TxnId::new([0x42; 16]);
    let allocated = Journal::allocate(
        dead,
        home.rescue_bundle(dead)
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        Layout::BundleIntent {
            old: BundleIdentity {
                cdhash: Cdhash::new([7; 20]),
                version: "0.4.6".to_owned(),
            },
            to_version: "0.4.7".to_owned(),
        },
    );
    std::fs::write(home.journal(), allocated.encode()).unwrap();
    attach(&blank, &home.mount_point(dead).unwrap());
    let swept = on_a_worker({
        let home = home.clone();
        move |worker| matches!(at_launch(worker, &home).unwrap(), AtLaunch::Swept)
    });
    assert!(swept);
    assert!(mounted(home.root()).is_empty());
    assert!(!home.transaction(dead).exists());
    assert!(!home.journal().exists());
}

/// RED (U-27) — **a staged transaction is revalidated before it is resumed,
/// and a staged bundle changed since it was verified is refused and the
/// transaction discarded.**
///
/// (b).1 F-17: "revalidates (hash, signature, manifest, classification) before
/// any resume". An unchanged stage passes; one resource of the staged bundle
/// changed — its seal broken — does not, and nothing of the transaction is
/// left.
///
/// MUTATION: in `still_valid`, drop the staged bundle's `check`.
#[test]
fn revalidation_before_resume_refuses_a_changed_image() {
    if !on_macos() {
        return;
    }
    let scene = Scene::new("revalidate", "Folio.app", |_| {});
    let tools = Arc::new(TestTools::new(&scene.scratch));
    let job = press(&driver(&scene, &tools), scene.release(), 0x43);
    assert!(
        matches!(job.state(), State::Verified(_)),
        "{:?}",
        job.state()
    );
    drop(job);
    let home = scene.home();
    let txn = TxnId::new([0x43; 16]);
    let running = scene.running.clone();
    let stage = home.stage_bundle(txn).unwrap();
    let answers = on_a_worker({
        let (home, tools) = (home.clone(), Arc::clone(&tools));
        move |worker| {
            let AtLaunch::Counted(staged) = at_launch(worker, &home).unwrap() else {
                panic!("the launch counts the prepared transaction");
            };
            let unchanged = revalidate(
                worker,
                *staged,
                &running,
                tools.as_ref(),
                Some(Channel::Ours),
            );
            let Ok(staged) = unchanged else {
                return (false, None);
            };
            std::fs::write(
                stage.join("Contents").join("Resources").join("notice.txt"),
                b"changed while it waited",
            )
            .unwrap();
            let changed = revalidate(
                worker,
                staged,
                &running,
                tools.as_ref(),
                Some(Channel::Ours),
            );
            (true, changed.err())
        }
    });
    assert_eq!(answers, (true, Some(Stop::Identity)));
    assert!(!home.transaction(txn).exists());
    assert!(!home.journal().exists());
    assert!(mounted(home.root()).is_empty());
}
