//! **The macOS Prepare** — the update job's driver on macOS, from the press to
//! `Prepared` (0.4.6 ticket U-27; `docs/plans/design/self-update-2026-09-16.md`
//! C7 steps 1–3, C.1, revision (b) F-3, F-17 and the M1–M2 rows, and revision
//! (c)).
//!
//! A press on the card hands the offer to [`MacPrepare`]. It starts one
//! worker, `bt-update-job` (`bt_platform::spawn_at_priority`, `BelowNormal`),
//! and everything below runs there, reporting to the job through the
//! [`Poster`] under the offer's transaction:
//!
//! 1. **The road** ([`eligible`]): the running bundle's installation home
//!    (`update_txn::Home::for_bundle`, beside the bundle — its own name, so a
//!    `Folio Beta.app` updates only itself), how this copy was installed
//!    (only `Channel::Ours`), and whether the folder the bundle stands in may
//!    be written: a translocated bundle, run from its read-only randomized
//!    mount, or a folder this account may not write, is
//!    [`NotEligible::NotWritable`] → [`Stop::NotWritable`], the releases page.
//!    Nothing is written before this answer.
//! 2. **Allocate**: the home is made if it is not there, its transaction lock
//!    taken (held → [`Stop::Busy`]), a journal still standing is somebody
//!    else's transaction ([`Stop::Busy`]), then the journal is written at
//!    `Allocated` — the running bundle's identity and the offer's version
//!    (`Layout::BundleIntent`), header outcome `none` — **before** any
//!    resource is taken (F-17), then `H/<txn>/` and its `stage/`, `rescue/`
//!    and `mnt/` through `install_txn::durable_create_dir`.
//! 3. **Download** the offer's two files by its own tag into a fresh
//!    item-replacement folder on the bundle's volume (the transport; C11),
//!    and hash the image against `SHA256SUMS-macos.txt`
//!    ([`Stop::Download`], [`Stop::Sums`]).
//! 4. **Attach** the image under `H/<txn>/mnt` with
//!    `bt_platform::macos_update::with_image`, which detaches it on every
//!    road ([`Stop::Mount`]). In its body: the mounted `Folio.app` is checked
//!    ([`check`]: the running bundle's designated requirement, strict, all
//!    architectures, nested code, through `bt_platform::macos_identity`; its
//!    `CFBundleShortVersionString` against the offer; its main executable's
//!    architecture against the offer's), copied with `ditto` into
//!    `stage/<Bundle>.app` — the running bundle's own name — and **checked
//!    again** as it lies there ([`Stop::Identity`], [`Stop::Copy`]). The
//!    downloaded folder is removed whatever happened.
//! 5. **Rescue**: the running bundle is cloned to `rescue/<Bundle>.app` and
//!    proved the same code (`macos_update::rescue_clone`; [`Stop::Clone`]).
//! 6. **`Prepared`**, durably, with both bundles' identities
//!    (`Layout::Bundle`, `deferred_launches: 0`, outcome `none`), and the
//!    staged transaction — home, journal, lock — handed to the job with
//!    `Poster::verified`.
//!
//! **Every refusal after the journal exists abandons the transaction and
//! leaves nothing**: the journal is recorded `Abandoned` (`PrepareFailed`),
//! then every image under `H/<txn>` is detached, `H/<txn>` removed, and the
//! journal removed last ([`clear`], each effect held to `update_txn`'s rights
//! table for O). The job's card names the reason; nothing installed moved.
//!
//! **The updater changes nothing inside or on the copied bundle** (C7): the
//! stage is the image's bundle as `ditto` carries it — seal, stapled ticket
//! and quarantine included.
//!
//! **A later launch** ([`at_launch`]): the job owner takes the lock and asks
//! `update_txn::decide` — `Allocated` is swept (M1: detach, then delete
//! `H/<txn>`, then the journal), `Prepared` is counted (M2 as W2,
//! `LaunchedWithoutResume`) and discarded at its second launch. **Before any
//! resume** ([`revalidate`]): the channel, the folder, the running bundle
//! (still the one the journal names), and the staged bundle (the identity
//! check, and the version and cdhash the journal recorded); a failure
//! discards. Neither has a product caller yet: the card that resumes a
//! staged job at a later launch is U-28's.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_txn::{self, Hold};
use bt_platform::macos_update::{self, Failed};

use crate::install_channel::Channel;
use crate::update_handoff::Staged;
use crate::update_job::{
    Bytes, Driver, Fetching, MACOS_ARCHITECTURE, NotEligible, Offer, Poster, Refused,
    SharedTransport, Step, Stop, Transport, Unsupported,
};
use crate::update_txn::{
    Action, Actor, Asker, BundleIdentity, Cdhash, Disk, Effect, Event, Home, Journal, Layout,
    Located, PhaseKind, TxnId, decide, may,
};

/// The worker a Prepare runs on — one per press, one job per process (R.3).
pub(crate) const WORKER: &str = "bt-update-job";

/// **The bundle's name on the release image** (`scripts/release/macos/dmg.sh`
/// stages `Folio.app` beside a link to `/Applications`).
pub(crate) const IMAGE_BUNDLE: &str = "Folio.app";

/// How much more of the image arrives between two progress reports to the
/// job: the card's bar moves in steps of this, and the job's inbox holds a few
/// dozen reports for a release image rather than one per chunk.
const REPORT_EVERY: u64 = 1024 * 1024;

// ── the seam ────────────────────────────────────────────────────────────────

/// **The checks and the effects a Prepare takes outside its home** — the
/// system's in the product ([`System`]), a test's stand-ins in the tests (an
/// ad-hoc signed synthetic bundle satisfies no Developer ID requirement, so a
/// test states the requirement it holds the bundle to).
pub(crate) trait Tools: Send + Sync {
    /// Whether the bundle at `bundle` is this publisher's Folio: its signature
    /// valid, strictly, for all architectures and nested code, and the running
    /// bundle's designated requirement satisfied.
    ///
    /// # Errors
    /// Why not, as a sentence.
    fn verify(&self, worker: &WorkerCtx, bundle: &Path) -> Result<(), String>;

    /// Copy the bundle `from` to `to` (which does not exist), changing nothing.
    ///
    /// # Errors
    /// Why not, as a sentence.
    fn copy(&self, worker: &WorkerCtx, from: &Path, to: &Path) -> Result<(), String>;

    /// Clone the running bundle `old` to `clone` and prove it the same code.
    ///
    /// # Errors
    /// Why not, as a sentence.
    fn rescue(&self, worker: &WorkerCtx, old: &Path, clone: &Path) -> Result<(), String>;

    /// A fresh, empty folder on `near`'s volume for the download.
    ///
    /// # Errors
    /// Why not, as a sentence.
    fn download_folder(&self, near: &Path) -> Result<PathBuf, String>;
}

/// **The system's tools**: `bt_platform::macos_identity` (the running
/// process's designated requirement, `codesign --verify --strict --deep
/// --all-architectures -R`, and Gatekeeper's assessment recorded through its
/// decision, F-9), `macos_update::copy_bundle` (`ditto`),
/// `macos_update::rescue_clone` and `macos_update::item_replacement_directory`.
pub(crate) struct System;

impl Tools for System {
    fn verify(&self, _worker: &WorkerCtx, bundle: &Path) -> Result<(), String> {
        use bt_platform::macos_identity;
        let requirement = macos_identity::running_requirement().map_err(|r| r.to_string())?;
        let verified =
            macos_identity::verify_bundle(bundle, &requirement).map_err(|r| r.to_string())?;
        macos_identity::identity_decision(Ok(verified), macos_identity::assess(bundle))
            .map(drop)
            .map_err(|refusal| refusal.to_string())
    }

    fn copy(&self, worker: &WorkerCtx, from: &Path, to: &Path) -> Result<(), String> {
        macos_update::copy_bundle(worker, from, to).map_err(|refusal| refusal.to_string())
    }

    fn rescue(&self, _worker: &WorkerCtx, old: &Path, clone: &Path) -> Result<(), String> {
        macos_update::rescue_clone(old, clone)
            .map(drop)
            .map_err(|refusal| refusal.to_string())
    }

    fn download_folder(&self, near: &Path) -> Result<PathBuf, String> {
        macos_update::item_replacement_directory(near).map_err(|refusal| refusal.to_string())
    }
}

// ── the driver ──────────────────────────────────────────────────────────────

/// **The macOS driver** (`update_job::Driver`): the Prepare of one bundle.
pub(crate) struct MacPrepare {
    /// The running bundle, `<parent>/<Name>.app`.
    bundle: PathBuf,
    tools: Arc<dyn Tools>,
    /// How this copy was installed, as the start read it.
    channel: Option<Channel>,
}

impl MacPrepare {
    /// **The driver of the running copy**: the bundle this process's
    /// executable sits in, the system's tools, and the channel the start read
    /// (`install_channel::channel`). A process that is not running from a
    /// bundle has no road to take: [`Unsupported`].
    pub(crate) fn of_this_copy() -> Box<dyn Driver> {
        let bundle = std::env::current_exe()
            .ok()
            .and_then(|exe| running_bundle(&exe));
        match bundle {
            Some(bundle) => Box::new(Self {
                bundle,
                tools: Arc::new(System),
                channel: crate::install_channel::channel(),
            }),
            None => Box::new(Unsupported),
        }
    }

    /// A driver for `bundle`, with `tools` and `channel` — a test's.
    #[cfg(test)]
    pub(crate) fn with(bundle: PathBuf, tools: Arc<dyn Tools>, channel: Option<Channel>) -> Self {
        Self {
            bundle,
            tools,
            channel,
        }
    }
}

/// The bundle an executable at `<X>.app/Contents/MacOS/<name>` is in.
fn running_bundle(exe: &Path) -> Option<PathBuf> {
    let root = crate::install_channel::install_root(exe, bt_platform::HostPlatform::MacOs)?;
    (root.extension()? == "app").then_some(root)
}

impl Driver for MacPrepare {
    fn prepare(
        &self,
        offer: &Offer,
        transport: &SharedTransport,
        post: &Poster,
    ) -> Result<(), Refused> {
        let (bundle, tools, transport) = (
            self.bundle.clone(),
            Arc::clone(&self.tools),
            Arc::clone(transport),
        );
        let (offer, post, channel) = (offer.clone(), post.clone(), self.channel);
        bt_platform::spawn_at_priority(
            WORKER,
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                let road = Road {
                    bundle: &bundle,
                    offer: &offer,
                    transport: transport.as_ref(),
                    post: &post,
                    tools: tools.as_ref(),
                    channel,
                };
                match prepare_on(worker, &road) {
                    Ok(staged) => {
                        // Cancelled between the last look and the report: the
                        // job has moved on, so the transaction is given up.
                        if let Err(staged) = post.verified(staged) {
                            let _ = discard(worker, *staged, &Event::Discarded);
                        }
                    }
                    Err(Stop::Cancelled) => {}
                    Err(stop) => post.post(Step::Stopped(stop)),
                }
            },
        )
        .map(drop)
        .map_err(|_| Refused::NoWorker)
    }
}

/// Everything one Prepare works from.
struct Road<'a> {
    bundle: &'a Path,
    offer: &'a Offer,
    transport: &'a dyn Transport,
    post: &'a Poster,
    tools: &'a dyn Tools,
    channel: Option<Channel>,
}

impl Road<'_> {
    /// [`Stop::Cancelled`] once the job has cancelled this transaction.
    fn go_on(&self) -> Result<(), Stop> {
        if self.post.cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    }
}

// ── step 1: the road ────────────────────────────────────────────────────────

/// **Whether `bundle` may take the macOS road, and its home**: an `.app` with
/// a parent, installed as [`Channel::Ours`], standing in a folder this process
/// may write that is not on a read-only mount (a translocated bundle is: C7).
///
/// # Errors
/// [`NotEligible::NotWritable`] for a read-only or unwritable folder, or one
/// that cannot be asked; [`NotEligible::NotOurs`] / [`NotEligible::Unknown`]
/// for a channel other than `Ours` (a managed copy never reaches a press, and
/// is answered as not ours here).
pub(crate) fn eligible(bundle: &Path, channel: Option<Channel>) -> Result<Home, NotEligible> {
    match channel {
        Some(Channel::Ours) => {}
        Some(Channel::NotOurs | Channel::Managed { .. }) => return Err(NotEligible::NotOurs),
        Some(Channel::Unknown) | None => return Err(NotEligible::Unknown),
    }
    let home = Home::for_bundle(bundle).ok_or(NotEligible::NotWritable)?;
    let folder = bundle.parent().ok_or(NotEligible::NotWritable)?;
    match bt_platform::install_evidence::may_write_into(folder) {
        Ok(true) => Ok(home),
        Ok(false) | Err(_) => Err(NotEligible::NotWritable),
    }
}

/// The stop a road refusal is reported as.
fn stop_for(why: &NotEligible) -> Stop {
    match why {
        NotEligible::NotWritable => Stop::NotWritable,
        _ => Stop::NotOurs,
    }
}

// ── the road, steps 2–6 ─────────────────────────────────────────────────────

/// **The Prepare, on its worker** — the module header's six steps.
fn prepare_on(worker: &WorkerCtx, road: &Road<'_>) -> Result<Staged, Stop> {
    let home = eligible(road.bundle, road.channel).map_err(|why| stop_for(&why))?;
    road.go_on()?;
    // The running bundle's identity is read before anything is written: a
    // bundle with none cannot be cloned, and so cannot be updated.
    let old = identity(worker, road.bundle).map_err(|_| Stop::Clone)?;

    // Step 2: the home, the lock, the journal at `Allocated`.
    let txn = road.offer.txn();
    let (Some(rescue), Some(stage), Some(mount)) = (
        home.rescue_bundle(txn),
        home.stage_bundle(txn),
        home.mount_point(txn),
    ) else {
        return Err(Stop::NotWritable);
    };
    let rescue_text = rescue.to_str().ok_or(Stop::Journal)?.to_owned();
    match install_txn::durable_create_dir(home.root()) {
        Ok(()) => {}
        Err(failure) if failure.error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(Stop::Journal),
    }
    let lock = install_txn::try_hold(&home.lock(), Hold::Exclusive)
        .map_err(|_| Stop::Journal)?
        .ok_or(Stop::Busy)?;
    if std::fs::symlink_metadata(home.journal()).is_ok() {
        return Err(Stop::Busy);
    }
    let allocated = Journal::allocate(
        txn,
        rescue_text,
        Layout::BundleIntent {
            old: old.clone(),
            to_version: road.offer.to_version().to_owned(),
        },
    );
    install_txn::durable_write(&home.journal(), &allocated.encode()).map_err(|_| Stop::Journal)?;

    // From here a refusal abandons the transaction and leaves nothing.
    let staged = acquire(worker, road, &home, txn, [&rescue, &stage, &mount]).and_then(|new| {
        let prepared = allocated
            .prepare_with(Layout::Bundle { old, new })
            .map_err(|_| Stop::Journal)?;
        install_txn::durable_write(&home.journal(), &prepared.encode())
            .map_err(|_| Stop::Journal)?;
        Ok(prepared)
    });
    match staged {
        Ok(journal) => Ok(Staged {
            home,
            journal,
            lock,
        }),
        Err(stop) => {
            let _ = abandon(worker, &home, &allocated, &Event::PrepareFailed);
            drop(lock);
            Err(stop)
        }
    }
}

/// Steps 2 (the folders) to 5: everything the transaction acquires; the
/// new bundle's identity when all of it is in place.
fn acquire(
    worker: &WorkerCtx,
    road: &Road<'_>,
    home: &Home,
    txn: TxnId,
    [rescue, stage, mount]: [&Path; 3],
) -> Result<BundleIdentity, Stop> {
    let folders = [
        Some(home.transaction(txn)),
        stage.parent().map(Path::to_path_buf),
        rescue.parent().map(Path::to_path_buf),
        Some(mount.to_path_buf()),
    ];
    for folder in folders {
        let folder = folder.ok_or(Stop::Journal)?;
        install_txn::durable_create_dir(&folder).map_err(|_| Stop::Journal)?;
    }
    road.go_on()?;

    // Step 3: the two files, into a folder of their own, and the hash.
    let downloads = road
        .tools
        .download_folder(road.bundle)
        .map_err(|_| Stop::Download)?;
    let staged = fetch_and_attach(worker, road, &downloads, mount, stage);
    let _ = install_txn::durable_remove(&downloads);
    let new = staged?;
    road.go_on()?;

    // Step 5: the rescue clone of the running bundle.
    road.tools
        .rescue(worker, road.bundle, rescue)
        .map_err(|_| Stop::Clone)?;
    road.go_on()?;
    Ok(new)
}

/// Steps 3 and 4: fetch, hash, attach, check, copy, check again.
fn fetch_and_attach(
    worker: &WorkerCtx,
    road: &Road<'_>,
    downloads: &Path,
    mount: &Path,
    stage: &Path,
) -> Result<BundleIdentity, Stop> {
    let [asset, sums] = road.offer.requests();
    let reported = Arc::new(AtomicU64::new(0));
    let post = road.post.clone();
    let fetching = Fetching {
        report: Arc::new(move |bytes: Bytes| {
            let last = reported.load(Ordering::Relaxed);
            let whole = bytes.total == Some(bytes.received);
            if whole || bytes.received >= last.saturating_add(REPORT_EVERY) {
                reported.store(bytes.received, Ordering::Relaxed);
                post.post(Step::Received(bytes));
            }
        }),
        cancelled: road.post.cancel_flag(),
    };
    let fetched = |request| {
        road.transport
            .fetch(request, downloads, &fetching)
            .map_err(|_| road.go_on().err().unwrap_or(Stop::Download))
    };
    let image = fetched(&asset)?;
    let sums_file = fetched(&sums)?;
    road.go_on()?;
    matches_its_sum(&image, &sums_file, road.offer.asset())?;
    road.post.post(Step::Staged);

    let used = macos_update::with_image(worker, &image, mount, |point| {
        copy_verified(worker, road, &point.join(IMAGE_BUNDLE), stage)
    });
    // A refused detach is debt, never a reason to undo the body's work (R-11):
    // the mount point lies under the home, where the next sweep finds it.
    match used.outcome {
        Ok(new) => Ok(new),
        Err(Failed::Attach(_)) => Err(Stop::Mount),
        Err(Failed::Body(stop)) => Err(stop),
    }
}

/// **C7 step 3, inside the attached image**: the mounted bundle is checked,
/// copied into `stage/`, and the copy checked again — "what gets installed
/// must be what was checked" (§C.2 step 6).
fn copy_verified(
    worker: &WorkerCtx,
    road: &Road<'_>,
    mounted: &Path,
    stage: &Path,
) -> Result<BundleIdentity, Stop> {
    let on_image = check(worker, road.tools, mounted, road.offer.to_version())?;
    road.go_on()?;
    road.tools
        .copy(worker, mounted, stage)
        .map_err(|_| Stop::Copy)?;
    let copied = check(worker, road.tools, stage, road.offer.to_version())?;
    if copied != on_image {
        return Err(Stop::Identity);
    }
    Ok(copied)
}

/// **A bundle is the offered Folio**: [`Tools::verify`], its
/// `CFBundleShortVersionString` equal to `version`, and its main executable
/// carrying the offer's architecture ([`MACOS_ARCHITECTURE`]). Answers its
/// identity — cdhash and version — for the journal.
///
/// # Errors
/// [`Stop::Identity`], whichever of them failed.
pub(crate) fn check(
    worker: &WorkerCtx,
    tools: &dyn Tools,
    bundle: &Path,
    version: &str,
) -> Result<BundleIdentity, Stop> {
    tools.verify(worker, bundle).map_err(|_| Stop::Identity)?;
    let code = macos_update::code_identity(worker, bundle).map_err(|_| Stop::Identity)?;
    let found = macos_update::short_version(worker, bundle).map_err(|_| Stop::Identity)?;
    if found != version {
        return Err(Stop::Identity);
    }
    let architectures =
        macos_update::architectures(&code.executable).map_err(|_| Stop::Identity)?;
    if !architectures.iter().any(|name| name == MACOS_ARCHITECTURE) {
        return Err(Stop::Identity);
    }
    Ok(BundleIdentity {
        cdhash: Cdhash::parse(&code.cdhash).map_err(|_| Stop::Identity)?,
        version: found,
    })
}

/// A signed bundle's identity as the journal records it: its main
/// executable's cdhash and its `CFBundleShortVersionString`.
pub(crate) fn identity(worker: &WorkerCtx, bundle: &Path) -> Result<BundleIdentity, String> {
    let code = macos_update::code_identity(worker, bundle).map_err(|r| r.to_string())?;
    let version = macos_update::short_version(worker, bundle).map_err(|r| r.to_string())?;
    Ok(BundleIdentity {
        cdhash: Cdhash::parse(&code.cdhash).map_err(|refusal| refusal.to_string())?,
        version,
    })
}

// ── the checksum ────────────────────────────────────────────────────────────

/// **The SHA-256 a checksum document gives for `asset`**, lowercase: the
/// document is `shasum`'s format (`<64 hex>  <name>`, or `<64 hex> *<name>`
/// for a binary-mode line), one file a line, as `docs/RELEASING.md` and
/// `scripts/release/macos/checksums.sh` write it. `None` when no line names
/// `asset`, or two lines name it with different sums.
pub(crate) fn sum_for(document: &str, asset: &str) -> Option<String> {
    let mut found: Option<String> = None;
    for line in document.lines() {
        let Some((sum, rest)) = line.trim_end_matches('\r').split_once(' ') else {
            continue;
        };
        let name = rest
            .strip_prefix(' ')
            .or_else(|| rest.strip_prefix('*'))
            .unwrap_or(rest);
        let well_formed = sum.len() == 64 && sum.bytes().all(|byte| byte.is_ascii_hexdigit());
        if name != asset || !well_formed {
            continue;
        }
        let sum = sum.to_ascii_lowercase();
        match &found {
            Some(earlier) if *earlier != sum => return None,
            _ => found = Some(sum),
        }
    }
    found
}

/// **The image is the one the checksum document names** — hashed a bounded
/// chunk at a time on `file_reads`' `Lane::Update`.
fn matches_its_sum(image: &Path, sums: &Path, asset: &str) -> Result<(), Stop> {
    use std::io::Read;
    let document = file_reads::read_to_string(Lane::Update, sums).map_err(|_| Stop::Sums)?;
    let expected = sum_for(&document, asset).ok_or(Stop::Sums)?;
    let mut file = file_reads::open(Lane::Update, image).map_err(|_| Stop::Sums)?;
    let mut hasher = bt_winres::digest::Sha256::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk).map_err(|_| Stop::Sums)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    if bt_winres::digest::hex(&hasher.finish()) == expected {
        Ok(())
    } else {
        Err(Stop::Sums)
    }
}

// ── giving a transaction up ─────────────────────────────────────────────────

/// **Record `event` (→ `Abandoned`) over `journal` and clear the
/// transaction away.** When the abandonment cannot be written, the journal on
/// the disk is still `journal`'s phase, and the clearing is done under it —
/// O may clear an `Allocated` transaction (M1) as well as an `Abandoned` one.
fn abandon(
    worker: &WorkerCtx,
    home: &Home,
    journal: &Journal,
    event: &Event,
) -> Result<(), String> {
    let phase = match journal.advance(event) {
        Ok(abandoned) => match install_txn::durable_write(&home.journal(), &abandoned.encode()) {
            Ok(()) => abandoned.body.phase.kind(),
            Err(_) => journal.body.phase.kind(),
        },
        Err(refusal) => return Err(format!("{refusal:?}")),
    };
    clear(worker, home, journal.txn, phase)
}

/// **Give a staged transaction up** (`Discarded`, or the second launch's
/// `LaunchedWithoutResume`): recorded, cleared, and its lock let go.
fn discard(worker: &WorkerCtx, staged: Staged, event: &Event) -> Result<(), String> {
    let cleared = abandon(worker, &staged.home, &staged.journal, event);
    // The lock goes with the rest of the staged transaction, once it is clear.
    drop(staged);
    cleared
}

/// **Clear a transaction O has given up away, in the protocol's order**:
/// every image mounted under `H/<txn>` detached (`macos_update::
/// detach_all_under` — a read-only volume inside the folder would stop its
/// deletion halfway), then `H/<txn>`, then the journal, last. Each effect is
/// the rights table's to allow O in `phase` (`update_txn::EFFECT_RIGHTS`).
/// Off macOS there is no image to detach.
pub(crate) fn clear(
    worker: &WorkerCtx,
    home: &Home,
    txn: TxnId,
    phase: PhaseKind,
) -> Result<(), String> {
    for effect in [
        Effect::DetachMount,
        Effect::DeleteTxnDir,
        Effect::DeleteJournal,
    ] {
        if !may(Actor::Old, effect, phase) {
            return Err(format!("O may not {effect:?} in {phase:?}"));
        }
        let done = match effect {
            Effect::DetachMount
                if bt_platform::host_platform() == bt_platform::HostPlatform::MacOs =>
            {
                macos_update::detach_all_under(worker, &home.transaction(txn))
                    .map_err(|refusal| refusal.to_string())
            }
            Effect::DetachMount => Ok(()),
            Effect::DeleteTxnDir => install_txn::durable_remove(&home.transaction(txn))
                .map_err(|failure| failure.to_string()),
            _ => {
                install_txn::durable_remove(&home.journal()).map_err(|failure| failure.to_string())
            }
        };
        done?;
    }
    Ok(())
}

// ── a later launch ──────────────────────────────────────────────────────────

/// **What the job owner of a launch did about the transaction it found.**
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the launch pass and the resume are wired with the staged card of a later launch (U-28)"
    )
)]
pub(crate) enum AtLaunch {
    /// No journal: no transaction.
    Nothing,
    /// Another holder has the transaction lock; nothing was touched.
    Busy,
    /// A dead Prepare's `Allocated` transaction was swept (M1).
    Swept,
    /// A `Prepared` transaction counted this launch and is kept, lock held:
    /// it may still be resumed (revalidating) in this launch.
    Counted(Box<Staged>),
    /// A `Prepared` transaction reached its second launch and was discarded.
    Discarded,
    /// Nothing here is the job owner's to do: a journal it cannot read, or
    /// one in a phase that belongs to the applier or recovery.
    Left,
}

/// **The job owner's pass at a launch** ((b).2's M1 and M2): the lock, one
/// read of the journal, and `update_txn::decide` as the job owner.
///
/// # Errors
/// A step that failed, as a sentence; whatever it left is the next launch's.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the launch pass and the resume are wired with the staged card of a later launch (U-28)"
    )
)]
pub(crate) fn at_launch(worker: &WorkerCtx, home: &Home) -> Result<AtLaunch, String> {
    if std::fs::symlink_metadata(home.journal()).is_err() {
        return Ok(AtLaunch::Nothing);
    }
    let Some(lock) = install_txn::try_hold(&home.lock(), Hold::Exclusive)
        .map_err(|failure| failure.to_string())?
    else {
        return Ok(AtLaunch::Busy);
    };
    let bytes = match file_reads::read(Lane::UpdateJournal, home.journal()) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(AtLaunch::Nothing),
        Err(error) => return Err(error.to_string()),
    };
    let Ok(journal) = Journal::parse(&bytes) else {
        return Ok(AtLaunch::Left);
    };
    // A job owner's answer is read from the phase alone (`decide`'s first
    // arm); the rest of the description is what an owner of no destructive
    // phase sees: no entrance, no receipt, no trial.
    let disk = Disk {
        journal: &journal,
        asker: Asker::JobOwner,
        entrance: false,
        located: Located::Bundle {
            live: None,
            stage: None,
        },
        receipt: None,
        trial_alive: false,
        now_ms: 0,
    };
    match decide(&disk) {
        Action::Sweep => {
            clear(worker, home, journal.txn, PhaseKind::Allocated)?;
            Ok(AtLaunch::Swept)
        }
        Action::CountDeferredLaunch => {
            let counted = journal
                .advance(&Event::LaunchedWithoutResume)
                .map_err(|refusal| format!("{refusal:?}"))?;
            install_txn::durable_write(&home.journal(), &counted.encode())
                .map_err(|failure| failure.to_string())?;
            if counted.body.phase.kind() == PhaseKind::Abandoned {
                clear(worker, home, counted.txn, PhaseKind::Abandoned)?;
                drop(lock);
                Ok(AtLaunch::Discarded)
            } else {
                Ok(AtLaunch::Counted(Box::new(Staged {
                    home: home.clone(),
                    journal: counted,
                    lock,
                })))
            }
        }
        _ => Ok(AtLaunch::Left),
    }
}

/// **Revalidate a staged transaction before it is resumed** ((b).1 F-17: hash,
/// signature, manifest, classification): this copy's channel and folder
/// ([`eligible`]), the running bundle still the one the journal names, and
/// the staged bundle — [`check`] again, and the cdhash and version the journal
/// recorded at `Prepared`. A failure discards the transaction (`Discarded`,
/// then [`clear`]) and says why.
///
/// # Errors
/// The [`Stop`] that failed; the transaction is gone.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the launch pass and the resume are wired with the staged card of a later launch (U-28)"
    )
)]
pub(crate) fn revalidate(
    worker: &WorkerCtx,
    staged: Staged,
    bundle: &Path,
    tools: &dyn Tools,
    channel: Option<Channel>,
) -> Result<Staged, Stop> {
    match still_valid(worker, &staged, bundle, tools, channel) {
        Ok(()) => Ok(staged),
        Err(stop) => {
            let _ = discard(worker, staged, &Event::Discarded);
            Err(stop)
        }
    }
}

fn still_valid(
    worker: &WorkerCtx,
    staged: &Staged,
    bundle: &Path,
    tools: &dyn Tools,
    channel: Option<Channel>,
) -> Result<(), Stop> {
    let Layout::Bundle { old, new } = &staged.journal.body.layout else {
        return Err(Stop::Journal);
    };
    let home = eligible(bundle, channel).map_err(|why| stop_for(&why))?;
    if home != staged.home {
        return Err(Stop::NotOurs);
    }
    if identity(worker, bundle).map_err(|_| Stop::Identity)? != *old {
        return Err(Stop::Identity);
    }
    let stage = staged
        .home
        .stage_bundle(staged.journal.txn)
        .ok_or(Stop::Journal)?;
    if check(worker, tools, &stage, &new.version)? != *new {
        return Err(Stop::Identity);
    }
    Ok(())
}

#[cfg(test)]
#[path = "update_prepare_macos_tests.rs"]
pub(crate) mod tests;
