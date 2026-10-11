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
//!    (`Channel::Ours`, or a Homebrew copy at the app target its Caskroom
//!    records, whose marks the layout reads here: [`PreparePoint::carried`]),
//!    and whether the folder the bundle stands in may be written: a
//!    translocated bundle, run from its read-only randomized mount, or a
//!    folder this account may not write, is
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
//! journal removed last (`update_prepare::clear`, each effect held to
//! `update_txn`'s rights table for O). The job's card names the reason;
//! nothing installed moved.
//!
//! **The updater changes nothing inside or on the copied bundle** (C7): the
//! stage is the image's bundle as `ditto` carries it — seal, stapled ticket
//! and quarantine included.
//!
//! **A later launch** (`update_prepare::at_launch`, shared with the Windows
//! Prepare): the job owner takes the lock and asks `update_txn::decide` —
//! `Allocated` is swept (M1: detach, then delete `H/<txn>`, then the journal),
//! `Prepared` is counted (M2 as W2, `LaunchedWithoutResume`) and discarded at
//! its second launch. **Before any resume** ([`revalidate`]): the channel, the
//! folder, the running bundle (still the one the journal names), and the
//! staged bundle (the identity check, and the version and cdhash the journal
//! recorded); a failure discards. The update job runs both at every launch
//! that finds a transaction waiting (`update_prepare::settle_at_launch`,
//! U-33), and a set that passes is [`resume`]d: the verified card again, for
//! the version the journal recorded.
//!
//! The worker, the progress reports, the checksum, the abandonment and the
//! launch pass are `update_prepare`'s: the same on both platforms.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_platform::admission::WorkerCtx;
use bt_platform::install_txn::{self, Hold};
use bt_platform::macos_update::{self, Failed};

use crate::install_channel::Channel;
use crate::update_adapter::Layouts;
use crate::update_archive;
use crate::update_handoff::Staged;
use crate::update_job::{
    Driver, MACOS_ARCHITECTURE, NotEligible, Offer, Poster, Refused, SharedTransport, Step, Stop,
    Transport, Unsupported,
};
#[cfg(test)]
pub(crate) use crate::update_prepare::{AtLaunch, at_launch, sum_for};
use crate::update_prepare::{WORKER, abandon, discard, fetching, finish, matches_its_sum};
use crate::update_txn::{
    Adapter, BundleIdentity, Carried, Cdhash, Event, Home, Journal, Layout, TxnId,
};

/// **The bundle's name on the release image** (`scripts/release/macos/dmg.sh`
/// stages `Folio.app` beside a link to `/Applications`).
pub(crate) const IMAGE_BUNDLE: &str = "Folio.app";

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
    /// The layouts this road's Prepare is called through, by the adapter
    /// the press names ([`PreparePoint`]).
    layouts: Layouts<dyn PreparePoint>,
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
                layouts: own_layouts(),
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
            layouts: own_layouts(),
        }
    }

    /// This driver with `ours` in place of the road's own layout — a test's
    /// fake layout, which records or refuses (U-41a1).
    #[cfg(test)]
    pub(crate) fn laid_out(self, ours: Arc<dyn PreparePoint>) -> Self {
        Self {
            layouts: Layouts::of(ours),
            ..self
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
        let (bundle, tools, transport, layouts) = (
            self.bundle.clone(),
            Arc::clone(&self.tools),
            Arc::clone(transport),
            self.layouts.clone(),
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
                    layouts: &layouts,
                };
                finish(worker, &post, prepare_on(worker, &road));
            },
        )
        .map(drop)
        .map_err(|_| Refused::NoWorker)
    }
}

/// Everything one Prepare works from.
pub(crate) struct Road<'a> {
    bundle: &'a Path,
    offer: &'a Offer,
    transport: &'a dyn Transport,
    post: &'a Poster,
    tools: &'a dyn Tools,
    channel: Option<Channel>,
    layouts: &'a Layouts<dyn PreparePoint>,
}

/// **A layout's `Prepare` on the macOS road** (0.4.7 ticket U-41a1;
/// managed-update §1.1 R1, §1.3): what the journal records at `Allocated`
/// from the running bundle's identity, and — on the job's worker, under the
/// transaction lock, between the journal at `Allocated` and the one at
/// `Prepared` — everything the transaction acquires, answered as the layout
/// `Prepared` records. The road around it is common: the road check, the
/// running bundle's identity, the home, the lock, both journal writes, and
/// the abandonment of a Prepare that refused (`Abandoned`, then cleared:
/// *Nothing changed.*).
pub(crate) trait PreparePoint: Send + Sync {
    /// **What travels with the copy** (managed-update §1.1's "what
    /// bookkeeping travels with it", M1), read from the running bundle
    /// before anything is written: the manager's marks `Allocated` records,
    /// or none. A precondition of the layout's road that does not hold
    /// (§1.5) is refused here, before the journal exists.
    ///
    /// # Errors
    /// The [`Stop`] the card names; nothing was written.
    fn carried(&self, road: &Road<'_>) -> Result<Option<Carried>, Stop>;

    /// What `Allocated` records: the running bundle's identity `old`, and
    /// what the offer names.
    fn allocated(&self, road: &Road<'_>, old: &BundleIdentity) -> Layout;

    /// Everything the offer's transaction acquires, into `rescue`, `stage`
    /// and `mount`, with what [`PreparePoint::carried`] answered carried
    /// onto the staged set; the layout `Prepared` records.
    ///
    /// # Errors
    /// The [`Stop`] the card names; the road abandons the transaction.
    fn prepare(
        &self,
        worker: &WorkerCtx,
        road: &Road<'_>,
        home: &Home,
        places: [&Path; 3],
        old: BundleIdentity,
        carried: Option<&Carried>,
    ) -> Result<Layout, Stop>;
}

/// **Folio's own layout** (`Layout::BundleIntent` → `Layout::Bundle`): the
/// offer's image, checked, its bundle copied into `stage/` and checked again,
/// and the rescue clone (U-27).
pub(crate) struct Ours;

impl PreparePoint for Ours {
    fn carried(&self, _road: &Road<'_>) -> Result<Option<Carried>, Stop> {
        Ok(None)
    }

    fn allocated(&self, road: &Road<'_>, old: &BundleIdentity) -> Layout {
        Layout::BundleIntent {
            old: old.clone(),
            to_version: road.offer.to_version().to_owned(),
        }
    }

    fn prepare(
        &self,
        worker: &WorkerCtx,
        road: &Road<'_>,
        home: &Home,
        places: [&Path; 3],
        old: BundleIdentity,
        _carried: Option<&Carried>,
    ) -> Result<Layout, Stop> {
        acquire(worker, road, home, road.offer.txn(), places).map(|new| Layout::Bundle { old, new })
    }
}

/// **Homebrew's layout** (0.4.8 ticket D1, U-41b; managed-update §2.2, §4):
/// [`Ours`] at the app target Homebrew recorded, with the cask's marks
/// carried. Before anything is written, the running bundle must be the app
/// Homebrew's record names (R-H2, `install_channel::homebrew_record`) and
/// the marks that exist are read (M1); a copy that fails is refused as one the
/// road does not update ([`Stop::NotOurs`]). After the staged bundle's
/// second identity check and the rescue clone, the running bundle's marks
/// are read again and must still be the recorded bytes (M2), and they are
/// written onto `stage/` and read back equal
/// (`bt_platform::macos_update::carry_attributes`); either failing is
/// [`Stop::Copy`], and the road abandons: *Nothing changed.*
pub(crate) struct Homebrew;

impl PreparePoint for Homebrew {
    fn carried(&self, road: &Road<'_>) -> Result<Option<Carried>, Stop> {
        let marks = crate::install_channel::homebrew_record(road.bundle).map_err(|why| {
            crate::diagnostics::note(&format!(
                "Folio: update job — this Homebrew copy is not updated here: {why}"
            ));
            Stop::NotOurs
        })?;
        Ok(Some(carried_of(marks.marks)))
    }

    fn allocated(&self, road: &Road<'_>, old: &BundleIdentity) -> Layout {
        Ours.allocated(road, old)
    }

    fn prepare(
        &self,
        worker: &WorkerCtx,
        road: &Road<'_>,
        home: &Home,
        places: [&Path; 3],
        old: BundleIdentity,
        carried: Option<&Carried>,
    ) -> Result<Layout, Stop> {
        let layout = Ours.prepare(worker, road, home, places, old, None)?;
        if let Some(carried) = carried {
            let [_, stage, _] = places;
            let still = crate::install_channel::homebrew_marks(road.bundle)
                .map(carried_of)
                .map_err(|_| Stop::Copy)?;
            if still != *carried {
                return Err(Stop::Copy);
            }
            carry(stage, carried)?;
        }
        road.go_on()?;
        Ok(layout)
    }
}

/// The journal's record of a Homebrew copy's marks.
fn carried_of(marks: crate::install_channel::HomebrewMarks) -> Carried {
    Carried {
        install: marks.marker,
        caskroom: marks.caskroom,
    }
}

/// **The recorded marks written onto the bundle at `bundle`** and read back
/// equal (M1).
///
/// # Errors
/// [`Stop::Copy`]: one was not written, not read back equal, or not flushed.
fn carry(bundle: &Path, carried: &Carried) -> Result<(), Stop> {
    let mut attributes = Vec::new();
    if let Some(install) = &carried.install {
        attributes.push((crate::install_channel::MARKER_ATTRIBUTE, install.as_slice()));
    }
    if let Some(caskroom) = &carried.caskroom {
        attributes.push((
            crate::install_channel::CASKROOM_ATTRIBUTE,
            caskroom.as_slice(),
        ));
    }
    macos_update::carry_attributes(bundle, &attributes).map_err(|refusal| {
        crate::diagnostics::note(&format!(
            "Folio: update job — the staged bundle did not take the copy's marks: {refusal}"
        ));
        Stop::Copy
    })
}

/// **Whether the marks `recorded` are still on the running bundle and on
/// the staged one** (M2, at a later launch's revalidation).
fn still_carried(bundle: &Path, stage: &Path, recorded: &Carried) -> Result<(), Stop> {
    for side in [bundle, stage] {
        let found = crate::install_channel::homebrew_marks(side)
            .map(carried_of)
            .map_err(|_| Stop::Copy)?;
        if found != *recorded {
            return Err(Stop::Copy);
        }
    }
    Ok(())
}

/// **The layouts of this road** as the product has them: [`Ours`], and
/// [`Homebrew`]'s.
fn own_layouts() -> Layouts<dyn PreparePoint> {
    Layouts::of(Arc::new(Ours) as Arc<dyn PreparePoint>).with_homebrew(Arc::new(Homebrew))
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

/// **Whether `bundle` may take the macOS road, its home, and the adapter it
/// takes**: an `.app` with a parent, installed as a channel whose adapter's
/// road is built on macOS (`update_adapter::built_on`; managed-update §1.5 —
/// [`Channel::Ours`] and Homebrew's, whose own precondition is its
/// layout's [`PreparePoint::carried`]), standing in a folder this process
/// may write that is not on a read-only mount (a translocated bundle is: C7).
///
/// # Errors
/// [`NotEligible::NotWritable`] for a read-only or unwritable folder, or one
/// that cannot be asked; [`NotEligible::NotOurs`] / [`NotEligible::Unknown`]
/// for another channel (a managed copy whose road is not built never reaches
/// a press, and is answered as not ours here).
pub(crate) fn eligible(
    bundle: &Path,
    channel: Option<Channel>,
) -> Result<(Home, Adapter), NotEligible> {
    let adapter = match channel {
        Some(Channel::Unknown) | None => return Err(NotEligible::Unknown),
        Some(channel) => crate::update_adapter::of_channel(channel)
            .filter(|&adapter| {
                crate::update_adapter::built_on(adapter, bt_platform::HostPlatform::MacOs)
            })
            .ok_or(NotEligible::NotOurs)?,
    };
    let home = Home::for_bundle(bundle).ok_or(NotEligible::NotWritable)?;
    let folder = bundle.parent().ok_or(NotEligible::NotWritable)?;
    match bt_platform::install_evidence::may_write_into(folder) {
        Ok(true) => Ok((home, adapter)),
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
    let (home, adapter) = eligible(road.bundle, road.channel).map_err(|why| stop_for(&why))?;
    let layout = road.layouts.named(adapter).map_err(|_| Stop::NotOurs)?;
    road.go_on()?;
    // What travels with the copy, and the layout's own precondition, before
    // anything is written (managed-update §1.5, M1).
    let carried = layout.carried(road)?;
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
        return Err(crate::update_prepare::journal_there(&home));
    }
    let allocated = Journal::allocate(txn, rescue_text, layout.allocated(road, &old))
        .naming(adapter)
        .carrying(carried);
    install_txn::durable_write(&home.journal(), &allocated.encode()).map_err(|_| Stop::Journal)?;

    // From here a refusal abandons the transaction and leaves nothing.
    let places = [rescue.as_path(), stage.as_path(), mount.as_path()];
    let staged = layout
        .prepare(
            worker,
            road,
            &home,
            places,
            old,
            allocated.body.marker.as_ref(),
        )
        .and_then(|recorded| {
            let prepared = allocated
                .prepare_with(recorded)
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
    let fetching = fetching(road.post);
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
/// carrying the offer's architecture ([`MACOS_ARCHITECTURE`]); and **this
/// build may update itself to it** — its sealed `FolioUpdateProtocol` this
/// build's ([`spoken`], 0.4.8 E1-a2) and its sealed `FolioMinUpdater` no
/// newer than this build (0.4.7 U-42c): the Windows archive reader's
/// `protocol` and `min_updater` rules, in that order. Answers its identity —
/// cdhash and version — for the journal.
///
/// # Errors
/// [`Stop::TooOld`] when the bundle needs a newer updater (with one line in
/// `diagnostics.log`); [`Stop::Identity`] for every other check — another
/// protocol (with one line), a missing or unreadable `FolioUpdateProtocol`
/// or `FolioMinUpdater` included.
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
    spoken(
        macos_update::update_protocol(worker, bundle).map_err(|refusal| refusal.to_string()),
        &mut crate::diagnostics::note,
    )?;
    let needs = macos_update::min_updater(worker, bundle).map_err(|_| Stop::Identity)?;
    let needed = crate::update::Version::parse(&needs).ok_or(Stop::Identity)?;
    let this = crate::update::Version::parse(crate::version::VERSION).ok_or(Stop::Identity)?;
    if this < needed {
        crate::diagnostics::note(&format!(
            "Folio: update job — the new bundle needs an updater of {needs} or later, and this is {}",
            crate::version::VERSION
        ));
        return Err(Stop::TooOld);
    }
    Ok(BundleIdentity {
        cdhash: Cdhash::parse(&code.cdhash).map_err(|_| Stop::Identity)?,
        version: found,
    })
}

/// **The bundle speaks this build's update protocol**: its sealed
/// `FolioUpdateProtocol` — `sealed`, or why it could not be read — is
/// [`release_manifest::PROTOCOL`]: the rule and the words of the Windows
/// archive reader's `protocol` check (`update_archive`'s `offered`,
/// [`update_archive::Reason::Protocol`]). A key that is missing or
/// unreadable, or a value that is not a number, is refused as a malformed
/// manifest is there ([`update_archive::Reason::Manifest`]). Every refusal is
/// one line, handed to `note` (`diagnostics.log` in the product), as every
/// archive refusal is on Windows.
///
/// Defence in depth for a hop, which the frozen surfaces already keep: it
/// runs in Prepare, so it protects no downgrade, and no journal read.
///
/// # Errors
/// [`Stop::Identity`], after its line.
///
/// [`release_manifest::PROTOCOL`]: bt_winres::release_manifest::PROTOCOL
fn spoken(sealed: Result<String, String>, note: &mut dyn FnMut(&str)) -> Result<(), Stop> {
    let reason = match sealed {
        Err(why) => update_archive::Reason::Manifest(format!(
            "its FolioUpdateProtocol could not be read: {why}"
        )),
        Ok(sealed) => match sealed.parse::<u32>() {
            Ok(protocol) if protocol == bt_winres::release_manifest::PROTOCOL => return Ok(()),
            Ok(protocol) => update_archive::Reason::Protocol(protocol),
            Err(_) => update_archive::Reason::Manifest(format!(
                "its FolioUpdateProtocol `{sealed}` is not a number"
            )),
        },
    };
    note(&format!(
        "Folio: update job — the new bundle is refused: {reason}"
    ));
    Err(Stop::Identity)
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

// ── a later launch ──────────────────────────────────────────────────────────

/// **Revalidate a staged transaction before it is resumed** ((b).1 F-17: hash,
/// signature, manifest, classification): this copy's channel and folder
/// ([`eligible`]), the running bundle still the one the journal names, and
/// the staged bundle — [`check`] again, and the cdhash and version the journal
/// recorded at `Prepared`. A failure discards the transaction (`Discarded`,
/// then `update_prepare::clear`) and says why.
///
/// # Errors
/// The [`Stop`] that failed; the transaction is gone.
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

/// **O's resume at a later launch** (U-33; `update_prepare::Resumer`):
/// [`revalidate`], then the version the staged bundle installs — the one the
/// journal recorded at `Prepared`, which the revalidation has just held the
/// staged bundle to.
///
/// # Errors
/// The [`Stop`] that failed; the transaction is gone.
pub(crate) fn resume(
    worker: &WorkerCtx,
    staged: Staged,
    bundle: &Path,
    tools: &dyn Tools,
    channel: Option<Channel>,
) -> Result<(Staged, String), Stop> {
    let staged = revalidate(worker, staged, bundle, tools, channel)?;
    match &staged.journal.body.layout {
        Layout::Bundle { new, .. } => {
            let version = new.version.clone();
            Ok((staged, version))
        }
        _ => {
            let _ = discard(worker, staged, &Event::Discarded);
            Err(Stop::Journal)
        }
    }
}

/// **The resumer of the running copy** (`update_job::resumer_for_this_copy`):
/// [`resume`] for the bundle this process's executable sits in, with the
/// system's tools; a process not running from a bundle cannot revalidate
/// anything (`update_prepare::no_resume`).
#[must_use]
pub(crate) fn resumer_of_this_copy() -> crate::update_prepare::Resumer {
    let bundle = std::env::current_exe()
        .ok()
        .and_then(|exe| running_bundle(&exe));
    match bundle {
        Some(bundle) => Box::new(move |worker, staged, channel| {
            resume(worker, staged, &bundle, &System, channel)
        }),
        None => crate::update_prepare::no_resume(),
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
    let (home, adapter) = eligible(bundle, channel).map_err(|why| stop_for(&why))?;
    if home != staged.home || adapter != staged.journal.body.adapter {
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
    // Homebrew's record still names this bundle (R-H2), and the marks the
    // press recorded are still on both bundles (M2).
    if adapter == Adapter::Homebrew {
        crate::install_channel::homebrew_record(bundle).map_err(|_| Stop::NotOurs)?;
    }
    if let Some(recorded) = &staged.journal.body.marker {
        still_carried(bundle, &stage, recorded)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "update_prepare_macos_tests.rs"]
pub(crate) mod tests;
