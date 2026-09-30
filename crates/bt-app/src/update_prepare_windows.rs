//! **The Windows Prepare** — the update job's driver on Windows, from the press
//! to `Prepared` (0.4.6 ticket U-20; `docs/plans/design/self-update-2026-09-16.md`
//! §C.2, C4–C6, revision (b) F-4, F-8, F-12, F-17 and the W1–W2 rows).
//!
//! A press on the card hands the offer to [`WinPrepare`]. It starts one worker,
//! `bt-update-job` (`bt_platform::spawn_at_priority`, `BelowNormal`), and
//! everything below runs there, reporting to the job through the [`Poster`]
//! under the offer's transaction:
//!
//! 1. **The road** ([`eligible`]): how this copy was installed (only
//!    `Channel::Ours`, which on Windows already says the install folder may be
//!    written, C2) and the installation home `H` beside the running
//!    executable (`<install>\.folio-update\`). Then **the running build**: its
//!    signature and identity under the policy (the capability's second half,
//!    owner ruling 2026-09-25 1 — an unsigned build is not an updater:
//!    [`Stop::NotOurs`]), the digest of its image, and the list of files it
//!    shipped, read out of its own release manifest (F-4, F-8). Nothing is
//!    written before this.
//! 2. **Allocate**: `H` is made if it is not there, its transaction lock taken
//!    (held → [`Stop::Busy`]), a journal still standing is somebody else's
//!    transaction ([`Stop::Busy`]), then the journal is written at `Allocated`
//!    — the old shipped list (`Layout::Members`), the rescue path, header
//!    outcome `none` — **before** any resource is taken (F-17), then `H\<txn>\`
//!    and its `download\`, `expand\`, `set\` and `rescue\` by exclusive
//!    creation (`install_txn::durable_create_dir`). **Everything is on the
//!    installation's own volume by construction** (R-6, C6): `H` is inside the
//!    install folder, and nothing is written anywhere else — not the roaming
//!    profile, not the system's temporary folder.
//! 3. **Download** the offer's two files by its own tag into `download\` (the
//!    transport; C11), and hash the archive against `SHA256SUMS.txt`
//!    ([`Stop::Download`], [`Stop::Sums`]).
//! 4. **Reserve**: the archive's directory declares what its members add up to
//!    (`update_archive::declared_bytes`); the volume must still have room for
//!    that twice (the expansion and the staged copy) and for the rescue copy of
//!    the running executable, or the card says how many megabytes are missing
//!    ([`Stop::Space`]). The backup needs none: the flip *moves* the old files
//!    on the same volume (C4, F-7).
//! 5. **Expand** into `expand\` (`update_archive::expand`: the grammar, the
//!    bounds, the new `folio.exe`'s manifest against the offer and this build,
//!    every other member against the manifest — F-4, F-12), then **verify
//!    identity, not merely validity** (§E, U-15): the new `folio.exe` against
//!    the running build's subject and identity OID at the offer's version and
//!    this machine ([`trust::verify_release_file_under`]), `folio.msix` when
//!    the release carries one ([`trust::verify_release_package_under`]), and
//!    the two Microsoft sidecars' signatures ([`trust::signature`]). Any
//!    refusal is [`Stop::Identity`] — a release whose archive and checksum
//!    document were both replaced consistently still fails here.
//! 6. **Stage**: every member copied into `set\` (`install_txn::durable_copy`:
//!    named only once flushed, the folder flushed after), `expand\` and
//!    `download\` removed, and **`set\` verified again as it lies** — every
//!    member's digest the one measured in `expand\`, and the identity checks of
//!    step 5 once more — because what gets installed must be what was checked
//!    ([`Stop::Copy`], [`Stop::Identity`]).
//! 7. **Rescue** (C5 as F-8 replaced it): the running executable copied to
//!    `rescue\<name>`, the copy the applier and recovery run from and no step
//!    ever moves; its digest must be the running image's and its identity the
//!    running build's ([`Stop::Clone`]).
//! 8. **`Prepared`**, durably, with the inventories O measured under the lock
//!    (F-8): the old shipped list, what is present at every name of it or of
//!    the new set (name, digest, size), and the new set; `deferred_launches:
//!    0`, outcome `none` (`Journal::prepare_with`); then the staged transaction
//!    — home, journal, lock — is handed to the job with `Poster::verified`.
//!
//! **Every refusal after the journal exists abandons the transaction and
//! leaves nothing** (W1): the journal is recorded `Abandoned` (`PrepareFailed`),
//! `H\<txn>` removed, the journal removed last, the lock let go
//! (`update_prepare::abandon`). The card names the reason and says *Nothing
//! changed*: nothing in the install folder is written by any step here. A
//! Cancel is the same road, taken at the next step.
//!
//! **A later launch** is `update_prepare::at_launch`, shared with macOS (W1's
//! sweep, W2's count and its discard at the second launch). **Before any
//! resume, and before the apply** ([`staged_as_verified`]): the channel and
//! the home, the installed image still the one the journal recorded, every
//! staged member still its recorded digest, the staged executable's identity again at
//! its own version, and the rescue copy still the old image. The applier runs
//! it after O has let go and before the entrance is written (U-23); O's resume
//! at a later launch is [`resume`] ([`revalidate`], which discards on a
//! failure, then the staged `folio.exe`'s own version), which the update job
//! runs on its worker at every launch that finds a `Prepared` transaction
//! (`update_prepare::settle_at_launch`, U-33).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bt_platform::admission::WorkerCtx;
use bt_platform::exclusive_create::Directory;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_txn::{self, Hold};
use bt_platform::trust::{self, Expectation, FileVersion, Policy, Refusal};
use bt_winres::release_manifest::{self, Manifest};

use crate::install_channel::Channel;
use crate::update_archive::{self, Deadline, EmbeddedManifest, Expected, ManifestSource, Reason};
use crate::update_handoff::Staged;
use crate::update_job::{
    Driver, NotEligible, Offer, Poster, Refused, SharedTransport, Step, Stop, Transport,
    Unsupported,
};
use crate::update_prepare::{
    WORKER, abandon, digest_of, discard, fetching, finish, matches_its_sum,
};
use crate::update_txn::{
    Adapter, Digest, Event, Home, Inventories, Journal, Layout, Member, Place, TxnId,
};

/// The new release's executable, which carries its manifest.
const EXECUTABLE: &str = update_archive::EXECUTABLE;

/// The new release's package, verified by its own signature (F-5).
const PACKAGE: &str = update_archive::PACKAGE;

/// **The two Microsoft sidecars** every release carries (C4): their bytes are
/// the manifest's (F-4) and their signatures are checked as `package.ps1
/// -Sign` checks them (U-15 decision 6).
const SIDECARS: [&str; 2] = ["conpty.dll", "OpenConsole.exe"];

/// **How long the expansion may take** once it has begun: 200 MB at most is
/// expanded ([`update_archive::ARCHIVE_MAX_BYTES`]), which a disk writes in
/// seconds; the bound is for a disk or a stream that stalls.
const EXPANSION_BUDGET: Duration = Duration::from_secs(5 * 60);

// ── the seam ────────────────────────────────────────────────────────────────

/// **The two effects a Prepare takes on the volume** that a test must be able
/// to make fail — the space left, and the durable copy — the system's in the
/// product ([`System`]).
pub(crate) trait Tools: Send + Sync {
    /// How many bytes the volume `folder` is on still has for this account.
    ///
    /// # Errors
    /// Why it could not be asked, as a sentence.
    fn available(&self, folder: &Path) -> Result<u64, String>;

    /// Copy the file `from` to `to`, which does not exist: `to`'s name appears
    /// only once its bytes are flushed, and its folder is flushed after.
    ///
    /// # Errors
    /// Why not, as a sentence; nothing is left under `to`.
    fn copy(&self, from: &Path, to: &Path) -> Result<(), String>;
}

/// **The system's effects**: `install_txn::available_bytes` and
/// `install_txn::durable_copy`, the source read on `file_reads`' `Lane::Update`.
pub(crate) struct System;

impl Tools for System {
    fn available(&self, folder: &Path) -> Result<u64, String> {
        install_txn::available_bytes(folder).map_err(|failure| failure.to_string())
    }

    fn copy(&self, from: &Path, to: &Path) -> Result<(), String> {
        let mut source = file_reads::open(Lane::Update, from).map_err(|e| e.to_string())?;
        install_txn::durable_copy(&mut source, to)
            .map(drop)
            .map_err(|failure| failure.to_string())
    }
}

// ── the driver ──────────────────────────────────────────────────────────────

/// **The Windows driver** (`update_job::Driver`): the Prepare of one install
/// folder.
pub(crate) struct WinPrepare {
    /// The running `folio.exe`, `<install>\folio.exe`.
    exe: PathBuf,
    /// How this copy was installed, as the start read it.
    channel: Option<Channel>,
    /// Which roots a signature may end in: the system's, in the product.
    policy: Policy,
    tools: Arc<dyn Tools>,
}

impl WinPrepare {
    /// **The driver of the running copy**: this process's executable, the
    /// channel the start read (`install_channel::channel`), the system's trust
    /// and effects. A process that cannot name its own executable has no road
    /// to take: [`Unsupported`].
    pub(crate) fn of_this_copy() -> Box<dyn Driver> {
        match std::env::current_exe() {
            Ok(exe) => Box::new(Self {
                exe,
                channel: crate::install_channel::channel(),
                policy: Policy::System,
                tools: Arc::new(System),
            }),
            Err(_) => Box::new(Unsupported),
        }
    }

    /// A driver for the running executable `exe`, with `channel`, `policy` and
    /// `tools` — a test's.
    #[cfg(test)]
    pub(crate) fn with(
        exe: PathBuf,
        channel: Option<Channel>,
        policy: Policy,
        tools: Arc<dyn Tools>,
    ) -> Self {
        Self {
            exe,
            channel,
            policy,
            tools,
        }
    }
}

impl Driver for WinPrepare {
    fn prepare(
        &self,
        offer: &Offer,
        transport: &SharedTransport,
        post: &Poster,
    ) -> Result<(), Refused> {
        let (exe, policy, tools, transport) = (
            self.exe.clone(),
            self.policy.clone(),
            Arc::clone(&self.tools),
            Arc::clone(transport),
        );
        let (offer, post, channel) = (offer.clone(), post.clone(), self.channel);
        bt_platform::spawn_at_priority(
            WORKER,
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                let road = Road {
                    exe: &exe,
                    offer: &offer,
                    transport: transport.as_ref(),
                    post: &post,
                    tools: tools.as_ref(),
                    channel,
                    policy: &policy,
                };
                finish(worker, &post, prepare_on(worker, &road));
            },
        )
        .map(drop)
        .map_err(|_| Refused::NoWorker)
    }
}

/// Everything one Prepare works from.
struct Road<'a> {
    exe: &'a Path,
    offer: &'a Offer,
    transport: &'a dyn Transport,
    post: &'a Poster,
    tools: &'a dyn Tools,
    channel: Option<Channel>,
    policy: &'a Policy,
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

// ── step 1: the road, and the running build ─────────────────────────────────

/// **Whether the copy whose executable is `exe` may take the Windows road, its
/// home, and the adapter it takes**: installed as a channel whose adapter's
/// road is built on Windows (`update_adapter::built_on`; managed-update §1.5)
/// — in this build only [`Channel::Ours`], which on Windows is decided only
/// for a folder this account owns and may write (C2, F-2) — with an
/// installation home beside the executable.
///
/// # Errors
/// [`NotEligible::NotOurs`] / [`NotEligible::Unknown`] for another channel
/// (a managed copy whose road is not built never reaches a press, and is
/// answered as not ours here); [`NotEligible::NotWritable`] for an executable
/// with no folder.
pub(crate) fn eligible(
    exe: &Path,
    channel: Option<Channel>,
) -> Result<(Home, Adapter), NotEligible> {
    let adapter = match channel {
        Some(Channel::Unknown) | None => return Err(NotEligible::Unknown),
        Some(channel) => crate::update_adapter::of_channel(channel)
            .filter(|&adapter| {
                crate::update_adapter::built_on(adapter, bt_platform::HostPlatform::Windows)
            })
            .ok_or(NotEligible::NotOurs)?,
    };
    let home = Home::of(bt_platform::HostPlatform::Windows, exe).ok_or(NotEligible::NotWritable)?;
    Ok((home, adapter))
}

/// The stop a road refusal is reported as.
fn stop_for(why: &NotEligible) -> Stop {
    match why {
        NotEligible::NotWritable => Stop::NotWritable,
        _ => Stop::NotOurs,
    }
}

/// **The running build, as a Prepare needs it**: whom its signature names (the
/// expectation a new file is held to, at the offer's version), its image's
/// digest and length (the rescue copy is held to them), and the names it
/// shipped (F-8's old shipped list).
struct Running {
    identity: Expectation,
    digest: String,
    size: u64,
    shipped: Vec<String>,
}

/// **Read the running build at `exe`.**
///
/// # Errors
/// [`Stop::NotOurs`] for an unsigned build or one that carries no release
/// manifest — neither is an updater build; [`Stop::Identity`] for a signature
/// that does not pass (revocation unknown included: the next press asks
/// again).
fn running(exe: &Path, policy: &Policy) -> Result<Running, Stop> {
    let identity = trust::identity_of(exe, policy).map_err(|refusal| match refusal {
        Refusal::Unsigned => Stop::NotOurs,
        _ => Stop::Identity,
    })?;
    let manifest = EmbeddedManifest
        .manifest_text(exe)
        .ok()
        .and_then(|text| Manifest::parse(&text).ok())
        .ok_or(Stop::NotOurs)?;
    let mut shipped = vec![EXECUTABLE.to_owned(), PACKAGE.to_owned()];
    shipped.extend(manifest.members.into_iter().map(|member| member.name));
    let (digest, size) = digest_of(exe).map_err(|_| Stop::Identity)?;
    Ok(Running {
        identity,
        digest,
        size,
        shipped,
    })
}

/// The offer's version as `VERSIONINFO` spells it: `0.4.7` → `0.4.7.0`
/// (`package.ps1` writes the fourth part as 0).
fn file_version(version: &str) -> Option<FileVersion> {
    FileVersion::parse_four(&format!("{version}.0"))
}

// ── steps 2–8 ───────────────────────────────────────────────────────────────

/// **The Prepare, on its worker** — the module header's eight steps.
fn prepare_on(worker: &WorkerCtx, road: &Road<'_>) -> Result<Staged, Stop> {
    let (home, adapter) = eligible(road.exe, road.channel).map_err(|why| stop_for(&why))?;
    road.go_on()?;
    let running = running(road.exe, road.policy)?;
    let name = road.exe.file_name().ok_or(Stop::NotWritable)?;

    // Step 2: the home, the lock, the journal at `Allocated`.
    let txn = road.offer.txn();
    let rescue = home.rescue_copy(txn, name).ok_or(Stop::NotWritable)?;
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
        Layout::Members(Inventories {
            old_shipped: running.shipped.clone(),
            old_present: Vec::new(),
            new: Vec::new(),
        }),
    )
    .naming(adapter);
    install_txn::durable_write(&home.journal(), &allocated.encode()).map_err(|_| Stop::Journal)?;

    // From here a refusal abandons the transaction and leaves nothing.
    let staged = acquire(road, &home, &running, &rescue).and_then(|inventories| {
        let prepared = allocated
            .prepare_with(Layout::Members(inventories))
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

/// The folders of one transaction a Prepare works in, all under `H\<txn>`.
struct Folders {
    transaction: PathBuf,
    download: PathBuf,
    expand: PathBuf,
    set: PathBuf,
}

impl Folders {
    fn of(home: &Home, txn: TxnId) -> Option<Self> {
        let transaction = home.transaction(txn);
        Some(Self {
            download: transaction.join("download"),
            expand: transaction.join("expand"),
            set: home.members_folder(txn, Place::Set)?,
            transaction,
        })
    }
}

/// Steps 2 (the folders) to 7: everything the transaction acquires; the
/// inventories when all of it is in place.
fn acquire(
    road: &Road<'_>,
    home: &Home,
    running: &Running,
    rescue: &Path,
) -> Result<Inventories, Stop> {
    let folders = Folders::of(home, road.offer.txn()).ok_or(Stop::Journal)?;
    for folder in [
        Some(folders.transaction.as_path()),
        Some(folders.download.as_path()),
        Some(folders.expand.as_path()),
        Some(folders.set.as_path()),
        rescue.parent(),
    ] {
        install_txn::durable_create_dir(folder.ok_or(Stop::Journal)?).map_err(|_| Stop::Journal)?;
    }
    road.go_on()?;

    // Step 3: the two files, and the hash.
    let archive = fetch(road, &folders.download)?;
    road.post.post(Step::Staged);

    // Step 4: the space the rest needs.
    let version = road.offer.to_version();
    let expanded = update_archive::declared_bytes(&archive, version).map_err(archive_refused)?;
    let needed = expanded.saturating_mul(2).saturating_add(running.size);
    let available = road
        .tools
        .available(&folders.transaction)
        .map_err(|_| Stop::Journal)?;
    if available < needed {
        return Err(Stop::Space {
            short_by: needed - available,
        });
    }
    road.go_on()?;

    // Step 5: expand, and verify identity.
    let expectation = file_version(version)
        .map(|offered| running.identity.clone().for_offer(offered))
        .ok_or(Stop::Identity)?;
    let members = expand_release(&archive, version, &folders.expand)?;
    let _ = install_txn::durable_remove(&folders.download);
    verify_set(&folders.expand, &members, &expectation, road.policy)?;
    road.go_on()?;

    // Step 6: the set, copied, flushed, and verified again where it lies.
    let mut new = Vec::with_capacity(members.len());
    for member in &members {
        let (digest, size) = digest_of(&folders.expand.join(member)).map_err(|_| Stop::Copy)?;
        road.tools
            .copy(&folders.expand.join(member), &folders.set.join(member))
            .map_err(|_| Stop::Copy)?;
        new.push(Member {
            name: member.clone(),
            digest: Digest::parse(&digest).map_err(|_| Stop::Copy)?,
            size,
        });
    }
    let _ = install_txn::durable_remove(&folders.expand);
    still_staged(&folders.set, &new, &expectation, road.policy)?;
    road.go_on()?;

    // Step 7: the rescue copy of the running executable.
    road.tools.copy(road.exe, rescue).map_err(|_| Stop::Clone)?;
    rescue_is_the_running_build(rescue, running, road.policy)?;
    road.go_on()?;

    // Step 8's inventories, measured under the lock.
    let install = road.exe.parent().ok_or(Stop::Journal)?;
    Ok(Inventories {
        old_shipped: running.shipped.clone(),
        old_present: present(install, &running.shipped, &new)?,
        new,
    })
}

/// **Step 3**: the offer's two files into `download`, and the archive held to
/// its line in the checksum document. Answers where the archive is.
fn fetch(road: &Road<'_>, download: &Path) -> Result<PathBuf, Stop> {
    let [asset, sums] = road.offer.requests();
    let fetching = fetching(road.post);
    let fetched = |request| {
        road.transport
            .fetch(request, download, &fetching)
            .map_err(|_| road.go_on().err().unwrap_or(Stop::Download))
    };
    let archive = fetched(&asset)?;
    let sums_file = fetched(&sums)?;
    road.go_on()?;
    matches_its_sum(&archive, &sums_file, road.offer.asset())?;
    Ok(archive)
}

/// **Step 5's expansion** into the empty folder `into`, held open: the
/// members, by name, in the archive's order.
fn expand_release(archive: &Path, version: &str, into: &Path) -> Result<Vec<String>, Stop> {
    let staging = Directory::open(into).map_err(|_| Stop::Copy)?;
    let expected = Expected {
        version,
        arch: release_manifest::archive_arch(std::env::consts::ARCH),
        updater: crate::version::VERSION,
    };
    let deadline = Deadline::at(Instant::now() + EXPANSION_BUDGET);
    update_archive::expand(archive, &expected, &staging, &EmbeddedManifest, &deadline)
        .map(|expanded| expanded.members)
        .map_err(archive_refused)
}

/// **The stop the archive reader's refusal is reported as, and its one line**
/// (0.4.7 U-42c; 0.4.6's D-13: every refusal read as *The update is not
/// verified.* and no line said which check refused): a release that needs a
/// newer updater is [`Stop::TooOld`], a write into the staging folder
/// [`Stop::Copy`], every other refusal [`Stop::Identity`] — and
/// `diagnostics.log` names the refusal.
fn archive_refused(refusal: update_archive::Refusal) -> Stop {
    crate::diagnostics::note(&format!(
        "Folio: update job — the release archive is refused: {refusal}"
    ));
    stop_for_archive(&refusal.reason)
}

/// The stop an archive [`Reason`] is reported as — see [`archive_refused`].
fn stop_for_archive(reason: &Reason) -> Stop {
    match reason {
        Reason::Write(_) => Stop::Copy,
        Reason::UpdaterTooOld { .. } => Stop::TooOld,
        _ => Stop::Identity,
    }
}

/// **The signed files of a release in `folder` are this publisher's, at the
/// offer's version, for this machine** (§E, U-15): `folio.exe` held to
/// `expectation`, `folio.msix` too when the release has one, and the two
/// sidecars' signatures, each of which must be there (C4).
///
/// # Errors
/// [`Stop::Identity`], whichever refused.
fn verify_set(
    folder: &Path,
    members: &[String],
    expectation: &Expectation,
    policy: &Policy,
) -> Result<(), Stop> {
    let has = |name: &str| members.iter().any(|member| member == name);
    if !has(EXECUTABLE) || !SIDECARS.iter().all(|sidecar| has(sidecar)) {
        return Err(Stop::Identity);
    }
    trust::verify_release_file_under(&folder.join(EXECUTABLE), expectation, policy)
        .map_err(|_| Stop::Identity)?;
    if has(PACKAGE) {
        trust::verify_release_package_under(&folder.join(PACKAGE), expectation, policy)
            .map_err(|_| Stop::Identity)?;
    }
    for sidecar in SIDECARS {
        trust::signature(&folder.join(sidecar), policy).map_err(|_| Stop::Identity)?;
    }
    Ok(())
}

/// **`set\` is what was verified**: every member's digest and length the ones
/// recorded, and the identity checks again, as the files lie there.
///
/// # Errors
/// [`Stop::Identity`].
fn still_staged(
    set: &Path,
    new: &[Member],
    expectation: &Expectation,
    policy: &Policy,
) -> Result<(), Stop> {
    for member in new {
        let (digest, size) = digest_of(&set.join(&member.name)).map_err(|_| Stop::Identity)?;
        if Digest::parse(&digest).ok() != Some(member.digest) || size != member.size {
            return Err(Stop::Identity);
        }
    }
    let names: Vec<String> = new.iter().map(|member| member.name.clone()).collect();
    verify_set(set, &names, expectation, policy)
}

/// **The rescue copy is the running build**: the running image's digest and
/// length, and the running build's identity as its own signature names it.
///
/// # Errors
/// [`Stop::Clone`].
fn rescue_is_the_running_build(
    rescue: &Path,
    running: &Running,
    policy: &Policy,
) -> Result<(), Stop> {
    let (digest, size) = digest_of(rescue).map_err(|_| Stop::Clone)?;
    if digest != running.digest || size != running.size {
        return Err(Stop::Clone);
    }
    match trust::identity_of(rescue, policy) {
        Ok(identity) if identity == running.identity => Ok(()),
        _ => Err(Stop::Clone),
    }
}

/// **What is in the install folder now at every name the old build shipped or
/// the new set brings** (F-8's `old_present`): each regular file with its
/// digest and length. A name with nothing there is absent from the list; a
/// name held by a folder or a link is left out as well — the flip moves files,
/// and never one of those.
///
/// # Errors
/// [`Stop::Journal`] when a file there cannot be read.
fn present(install: &Path, shipped: &[String], new: &[Member]) -> Result<Vec<Member>, Stop> {
    let mut names: Vec<&str> = shipped.iter().map(String::as_str).collect();
    for member in new {
        if !names.contains(&member.name.as_str()) {
            names.push(&member.name);
        }
    }
    let mut present = Vec::new();
    for name in names {
        let path = install.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => {
                let (digest, size) = digest_of(&path).map_err(|_| Stop::Journal)?;
                present.push(Member {
                    name: name.to_owned(),
                    digest: Digest::parse(&digest).map_err(|_| Stop::Journal)?,
                    size,
                });
            }
            _ => {}
        }
    }
    Ok(present)
}

// ── before a resume, and before the apply ────────────────────────────────────

/// **What a staged transaction is checked against**: the installed executable
/// (the running one, for a resume in O; `<install>\folio.exe`, for the
/// applier), how that copy was installed, and the trust policy. The version
/// the staged set is held to is read from the staged `folio.exe`'s own
/// `VERSIONINFO` (U-23's decision on U-20's decision 1): the journal records
/// no tag and no `to_version`, and needs neither — every staged member's
/// digest was recorded when it had just been verified at the offer's version,
/// and it is those digests that bind the set to that offer now.
pub(crate) struct Resume<'a> {
    pub(crate) exe: &'a Path,
    pub(crate) channel: Option<Channel>,
    pub(crate) policy: &'a Policy,
}

/// **Revalidate a staged transaction before O resumes it** ((b).1 F-17: hash,
/// signature, manifest, classification) — [`staged_as_verified`]; a failure
/// discards the transaction (`Discarded`, then `update_prepare::clear`) and
/// says why.
///
/// # Errors
/// The [`Stop`] that failed; the transaction is gone.
pub(crate) fn revalidate(
    worker: &WorkerCtx,
    staged: Staged,
    resume: &Resume<'_>,
) -> Result<Staged, Stop> {
    match staged_as_verified(&staged.home, &staged.journal, resume) {
        Ok(()) => Ok(staged),
        Err(stop) => {
            let _ = discard(worker, staged, &Event::Discarded);
            Err(stop)
        }
    }
}

/// **O's resume at a later launch** (U-33; `update_prepare::Resumer`):
/// [`revalidate`], then the version the staged set installs, read from the
/// staged `folio.exe`'s own release manifest (F-4: the manifest the archive's
/// expansion held to the offer) — the offer is rebuilt from it, never from a
/// download. A set whose manifest cannot be read is discarded like any other
/// that fails revalidation.
///
/// # Errors
/// The [`Stop`] that failed; the transaction is gone.
pub(crate) fn resume(
    worker: &WorkerCtx,
    staged: Staged,
    resume: &Resume<'_>,
) -> Result<(Staged, String), Stop> {
    let staged = revalidate(worker, staged, resume)?;
    let version = staged
        .home
        .members_folder(staged.journal.txn, Place::Set)
        .and_then(|set| EmbeddedManifest.manifest_text(&set.join(EXECUTABLE)).ok())
        .and_then(|text| Manifest::parse(&text).ok())
        .map(|manifest| manifest.version);
    match version {
        Some(version) => Ok((staged, version)),
        None => {
            let _ = discard(worker, staged, &Event::Discarded);
            Err(Stop::Identity)
        }
    }
}

/// **The resumer of the running copy** (`update_job::resumer_for_this_copy`):
/// [`resume`] for the executable at `exe` under `policy`, for the channel
/// the job hands it.
#[must_use]
pub(crate) fn resumer(exe: PathBuf, policy: Policy) -> crate::update_prepare::Resumer {
    Box::new(move |worker, staged, channel| {
        resume(
            worker,
            staged,
            &Resume {
                exe: &exe,
                channel,
                policy: &policy,
            },
        )
    })
}

/// **The staged transaction `journal` of `home` is still what was verified**
/// — the check a resume in O and the applier both run, the applier under the
/// transaction lock after O has let go and before it writes the entrance
/// (U-23): this copy's channel and home ([`eligible`]); the installed image
/// still the `folio.exe` the journal recorded as present; every staged member of `set\`
/// still its recorded digest and length, and the staged set's identity checks
/// again at the staged `folio.exe`'s own version; the rescue copy still the
/// old image.
///
/// # Errors
/// The [`Stop`] that failed. Nothing is written.
pub(crate) fn staged_as_verified(
    home: &Home,
    journal: &Journal,
    resume: &Resume<'_>,
) -> Result<(), Stop> {
    let Layout::Members(inventories) = &journal.body.layout else {
        return Err(Stop::Journal);
    };
    let (found, adapter) = eligible(resume.exe, resume.channel).map_err(|why| stop_for(&why))?;
    if &found != home || adapter != journal.body.adapter {
        return Err(Stop::NotOurs);
    }
    let name = resume.exe.file_name().ok_or(Stop::NotWritable)?;
    let is = |path: &Path, member: &Member| {
        digest_of(path).is_ok_and(|(digest, size)| {
            Digest::parse(&digest).ok() == Some(member.digest) && size == member.size
        })
    };
    let old = inventories
        .old_present
        .iter()
        .find(|member| OsStr::new(&member.name) == name)
        .ok_or(Stop::Identity)?;
    if !is(resume.exe, old) {
        return Err(Stop::Identity);
    }
    let running = trust::identity_of(resume.exe, resume.policy).map_err(|_| Stop::Identity)?;
    let set = home
        .members_folder(journal.txn, Place::Set)
        .ok_or(Stop::Journal)?;
    let staged_version = trust::identity_of(&set.join(EXECUTABLE), resume.policy)
        .map_err(|_| Stop::Identity)?
        .version;
    still_staged(
        &set,
        &inventories.new,
        &running.for_offer(staged_version),
        resume.policy,
    )?;
    let rescue = home.rescue_copy(journal.txn, name).ok_or(Stop::Journal)?;
    if !is(&rescue, old) {
        return Err(Stop::Clone);
    }
    Ok(())
}

#[cfg(test)]
#[path = "update_prepare_windows_tests.rs"]
pub(crate) mod tests;
