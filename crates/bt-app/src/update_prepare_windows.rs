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
//!    executable (`<install>\.folio-update\`). Then **the running build**: the
//!    digest of its image, and the list of files it shipped, read out of its
//!    own release manifest (F-4, F-8) — a build without one is not an updater
//!    ([`Stop::NotOurs`]). Its signature decides nothing (0.4.10 plan row 102,
//!    R3: the capability is the release build's flag, `update::eligible`, and
//!    the manifest; the owner's 2026-09-25 "signed and flagged" became
//!    "flagged"). Nothing is written before this.
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
//! 3. **The release statement** (0.4.10 plan row 102): `RELEASE-STATEMENT.txt`
//!    and its signature fetched by the offer's own tag into `download\` and
//!    verified with this copy's embedded keys, none it remembers as retired —
//!    its tag and version the offer's, its `min_from` no newer than this build
//!    ([`Stop::StatementRefused`]; [`Stop::TooOld`] before the large download;
//!    `update_prepare::vouched`) — and kept in `H\<txn>\` beside `set\`
//!    (`update_prepare::kept_statement`). Then **the archive**, by the name
//!    the statement gives it, held to the statement's SHA-256 and size
//!    ([`Stop::Download`], [`Stop::Sums`]; `update_prepare::fetch_vouched_asset`).
//! 4. **Reserve**: the archive's directory declares what its members add up to
//!    (`update_archive::declared_bytes`); the volume must still have room for
//!    that twice (the expansion and the staged copy) and for the rescue copy of
//!    the running executable, or the card says how many megabytes are missing
//!    ([`Stop::Space`]). The backup needs none: the flip *moves* the old files
//!    on the same volume (C4, F-7).
//! 5. **Expand** into `expand\` (`update_archive::expand`: the grammar, the
//!    bounds, the new `folio.exe`'s manifest against the offer and this build,
//!    every other member against the manifest — F-4, F-12), then **every
//!    member, `folio.exe` and `folio.msix` included, held to the statement's
//!    member lines** ([`held_to_the_statement`]): the names the same set, each
//!    SHA-256 and size the same. Any difference is [`Stop::Identity`]. No
//!    `trust` call decides anything (R4, R9: the sidecars' Microsoft
//!    signatures are checked when the release is packaged, and their bytes are
//!    bound here twice, by the manifest and by the statement).
//! 6. **Stage**: every member copied into `set\` (`install_txn::durable_copy`:
//!    named only once flushed, the folder flushed after), `expand\` and
//!    `download\` removed, and **`set\` verified again as it lies** — the kept
//!    statement verified again and every member held to it ([`still_staged`])
//!    — because what gets installed must be what was checked ([`Stop::Copy`],
//!    [`Stop::Identity`]). Then the one reading of a signature the Prepare
//!    makes: whether the staged `folio.exe` carries our signing service's
//!    signature ([`service_signature`]), which decides no step — only whether
//!    the Ready card says the version is not signed by it (R4, R5).
//! 7. **Rescue** (C5 as F-8 replaced it): the running executable copied to
//!    `rescue\<name>`, the copy the applier and recovery run from and no step
//!    ever moves; its digest and length must be the running image's — equal
//!    bytes are the same build ([`Stop::Clone`]).
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
//! the home, the installed image still the one the journal recorded, the kept
//! release statement verified again and every staged member held to it and
//! to its recorded digest, and the rescue copy still the old image; a
//! transaction folder with no statement — one an older build prepared — is
//! refused. The applier runs it after O has let go and before the entrance is
//! written (U-23); O's resume at a later launch is [`resume`] ([`revalidate`],
//! which discards on a failure; the version is the statement's), which the update job
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
use bt_platform::trust::{self, Policy};
use bt_winres::release_manifest::{self, Manifest};
use bt_winres::release_statement::{VerifiedStatement, WINDOWS_X64};

use crate::install_channel::Channel;
use crate::update_adapter::Layouts;
use crate::update_apply::{
    JOURNAL_HELD_WITHIN, Spelling, WaitClock, WaitWithin, WallClock, failure_words,
    write_journal_on,
};
use crate::update_archive::{self, Deadline, EmbeddedManifest, Expected, ManifestSource, Reason};
use crate::update_handoff::Staged;
use crate::update_job::{
    Driver, NotEligible, Offer, Poster, Refused, SharedTransport, Step, Stop, Transport,
    Unsupported,
};
use crate::update_prepare::{
    Vouching, WORKER, abandon_on, digest_of, discard, fetch_vouched_asset, finish, journal_failed,
    journal_stop, kept_statement, note, statement_refused, vouched,
};
use crate::update_txn::{
    Adapter, Digest, Event, Home, Inventories, Journal, Layout, Member, Place, TxnId,
};

/// The new release's executable, which carries its manifest.
const EXECUTABLE: &str = update_archive::EXECUTABLE;

/// The new release's package, which the manifest cannot list (F-5): the
/// statement does.
const PACKAGE: &str = update_archive::PACKAGE;

/// **Whose signature on a staged `folio.exe` is our signing service's**
/// (0.4.10 plan row 102 §1.8, R4): the subject the package is published under
/// (`msix::PACKAGE_PUBLISHER`) and the identity-validation OID of our profile,
/// two constants recorded in code rather than read from the statement — so a
/// statement's `authenticode true` cannot stand in for the signature.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ServiceSigner {
    pub(crate) subject: &'static str,
    pub(crate) identity_oid: &'static str,
}

/// **Our signing service**, as the signature on every signed release names it.
pub(crate) const OUR_SERVICE: ServiceSigner = ServiceSigner {
    subject: bt_platform::msix::PACKAGE_PUBLISHER,
    identity_oid: "1.3.6.1.4.1.311.97.921771326.840678362.737210289.98417136",
};

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
        install_txn::available_bytes(folder).map_err(|failure| failure_words(&failure))
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
    /// What the release statement is verified with, and where its retires
    /// are remembered (`update_prepare::Vouching`).
    vouching: Vouching,
    /// Which roots a signature may end in, for the one reading that decides
    /// no step ([`service_signature`]): the system's, in the product.
    policy: Policy,
    tools: Arc<dyn Tools>,
    /// The layouts this road's Prepare is called through, by the adapter
    /// the press names ([`PreparePoint`]).
    layouts: Layouts<dyn PreparePoint>,
    /// The clock a held journal write is asked again on ([`journal_written`]):
    /// [`WallClock`] in the product; a test's counted clock
    /// (T-UPDATE-PREPARE-REVALIDATION-RESUME-FLAKE).
    clock: Arc<dyn WaitClock>,
}

impl WinPrepare {
    /// **The driver of the running copy**: this process's executable, the
    /// channel the start read (`install_channel::channel`), the embedded
    /// release keys and this copy's data folder, the system's trust and
    /// effects. A process that cannot name its own executable has no road to
    /// take: [`Unsupported`].
    pub(crate) fn of_this_copy() -> Box<dyn Driver> {
        match std::env::current_exe() {
            Ok(exe) => Box::new(Self {
                exe,
                channel: crate::install_channel::channel(),
                vouching: Vouching::of_this_copy(),
                policy: Policy::System,
                tools: Arc::new(System),
                layouts: Layouts::of(Arc::new(Ours)),
                clock: Arc::new(WallClock),
            }),
            Err(_) => Box::new(Unsupported),
        }
    }

    /// A driver for the running executable `exe`, with `channel`, `vouching`,
    /// `policy` and `tools` — a test's.
    #[cfg(test)]
    pub(crate) fn with(
        exe: PathBuf,
        channel: Option<Channel>,
        vouching: Vouching,
        policy: Policy,
        tools: Arc<dyn Tools>,
    ) -> Self {
        Self {
            exe,
            channel,
            vouching,
            policy,
            tools,
            layouts: Layouts::of(Arc::new(Ours)),
            clock: Arc::new(WallClock),
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
        let (exe, vouching, policy, tools, transport, layouts, clock) = (
            self.exe.clone(),
            self.vouching.clone(),
            self.policy.clone(),
            Arc::clone(&self.tools),
            Arc::clone(transport),
            self.layouts.clone(),
            Arc::clone(&self.clock),
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
                    vouching: &vouching,
                    policy: &policy,
                    layouts: &layouts,
                    clock: clock.as_ref(),
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
    exe: &'a Path,
    offer: &'a Offer,
    transport: &'a dyn Transport,
    post: &'a Poster,
    tools: &'a dyn Tools,
    channel: Option<Channel>,
    vouching: &'a Vouching,
    policy: &'a Policy,
    layouts: &'a Layouts<dyn PreparePoint>,
    /// The clock a held journal write is asked again on.
    clock: &'a dyn WaitClock,
}

/// **A layout's `Prepare` on the Windows road** (0.4.7 ticket U-41a1;
/// managed-update §1.1 R1, §1.3): what the journal records at `Allocated`,
/// and — on the job's worker, under the transaction lock, between the
/// journal at `Allocated` and the one at `Prepared` — everything the
/// transaction acquires, answered as the layout `Prepared` records. The road
/// around it is common: the road check, the home, the lock, both journal
/// writes, and the abandonment of a Prepare that refused (`Abandoned`, then
/// cleared: *Nothing changed.*).
pub(crate) trait PreparePoint: Send + Sync {
    /// What `Allocated` records, from the running build.
    fn allocated(&self, running: &Running) -> Layout;

    /// Everything the transaction acquires; the layout `Prepared` records.
    ///
    /// # Errors
    /// The [`Stop`] the card names; the road abandons the transaction.
    fn prepare(
        &self,
        road: &Road<'_>,
        home: &Home,
        running: &Running,
        rescue: &Path,
    ) -> Result<Layout, Stop>;
}

/// **Folio's own layout** (`Layout::Members`): the offer's zip, expanded,
/// verified and staged into `H\<txn>\set\`, and the rescue copy (U-20).
pub(crate) struct Ours;

impl PreparePoint for Ours {
    fn allocated(&self, running: &Running) -> Layout {
        Layout::Members(Inventories {
            old_shipped: running.shipped.clone(),
            old_present: Vec::new(),
            new: Vec::new(),
        })
    }

    fn prepare(
        &self,
        road: &Road<'_>,
        home: &Home,
        running: &Running,
        rescue: &Path,
    ) -> Result<Layout, Stop> {
        acquire(road, home, running, rescue).map(Layout::Members)
    }
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

/// **The running build, as a Prepare needs it**: its image's digest and
/// length (the rescue copy is held to them), and the names it shipped (F-8's
/// old shipped list).
pub(crate) struct Running {
    digest: String,
    size: u64,
    shipped: Vec<String>,
}

/// **Read the running build at `exe`.** Its signature is not read: it decides
/// nothing (R3).
///
/// # Errors
/// [`Stop::NotOurs`] for a build that carries no release manifest — it is not
/// an updater build; [`Stop::Clone`] when its image cannot be read (the
/// rescue copy is held to it).
fn running(exe: &Path) -> Result<Running, Stop> {
    let manifest = EmbeddedManifest
        .manifest_text(exe)
        .ok()
        .and_then(|text| Manifest::parse(&text).ok())
        .ok_or(Stop::NotOurs)?;
    let mut shipped = vec![EXECUTABLE.to_owned(), PACKAGE.to_owned()];
    shipped.extend(manifest.members.into_iter().map(|member| member.name));
    let (digest, size) = digest_of(exe).map_err(|_| Stop::Clone)?;
    Ok(Running {
        digest,
        size,
        shipped,
    })
}

// ── steps 2–8 ───────────────────────────────────────────────────────────────

/// **The Prepare, on its worker** — the module header's eight steps.
fn prepare_on(worker: &WorkerCtx, road: &Road<'_>) -> Result<Staged, Stop> {
    let (home, adapter) = eligible(road.exe, road.channel).map_err(|why| stop_for(&why))?;
    let layout = road.layouts.named(adapter).map_err(|_| Stop::NotOurs)?;
    road.go_on()?;
    let running = running(road.exe)?;
    let name = road.exe.file_name().ok_or(Stop::NotWritable)?;

    // Step 2: the home, the lock, the journal at `Allocated`.
    let txn = road.offer.txn();
    let rescue = home.rescue_copy(txn, name).ok_or(Stop::NotWritable)?;
    let Some(rescue_text) = rescue.to_str() else {
        return Err(journal_stop("the rescue copy's path is not Unicode"));
    };
    let rescue_text = rescue_text.to_owned();
    match install_txn::durable_create_dir(home.root()) {
        Ok(()) => {}
        Err(failure) if failure.error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(failure) => {
            return Err(journal_failed(
                "the installation home was not created",
                &failure,
            ));
        }
    }
    let lock = install_txn::try_hold(&home.lock(), Hold::Exclusive)
        .map_err(|failure| journal_failed("the transaction lock was not asked for", &failure))?
        .ok_or(Stop::Busy)?;
    if std::fs::symlink_metadata(home.journal()).is_ok() {
        return Err(crate::update_prepare::journal_there(&home));
    }
    let allocated = Journal::allocate(txn, rescue_text, layout.allocated(&running)).naming(adapter);
    journal_written(worker, road, &home, &allocated, "Allocated")?;

    // From here a refusal abandons the transaction and leaves nothing.
    let staged = layout
        .prepare(road, &home, &running, &rescue)
        .and_then(|recorded| {
            let prepared = allocated.prepare_with(recorded).map_err(|refusal| {
                journal_stop(&format!("the journal refuses Prepared: {refusal:?}"))
            })?;
            journal_written(worker, road, &home, &prepared, "Prepared")?;
            Ok(prepared)
        });
    match staged {
        Ok(journal) => Ok(Staged {
            home,
            journal,
            lock,
        }),
        Err(stop) => {
            let _ = abandon_on(worker, road.clock, &home, &allocated, &Event::PrepareFailed);
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

/// **Write `journal` durably on the Prepare's road, at `phase`**, riding a
/// hold: while another program holds `journal.json` (or the temporary renamed
/// onto it) open — an on-access scan of the file just written, an indexer
/// (`install_txn::Failure::refused_while_open`, Windows only) — the rename is
/// asked again on the road's clock until [`JOURNAL_HELD_WITHIN`] has passed,
/// and no more once the job has cancelled the transaction; each round is said
/// in `diagnostics.log` (U-34, 0.4.8 E4: the applier's own
/// `update_apply::write_journal`; T-UPDATE-PREPARE-REVALIDATION-RESUME-FLAKE:
/// a hold under load stopped a Prepare at `Stopped(Journal)`).
///
/// # Errors
/// [`Stop::Cancelled`] once the job has cancelled; otherwise
/// [`Stop::Journal`], its line naming the phase and the last failure.
fn journal_written(
    worker: &WorkerCtx,
    road: &Road<'_>,
    home: &Home,
    journal: &Journal,
    phase: &str,
) -> Result<(), Stop> {
    write_journal_on(
        WaitWithin {
            worker,
            within: JOURNAL_HELD_WITHIN,
            clock: road.clock,
        },
        Some(&road.post.cancel_flag()),
        Spelling::WithoutPath,
        &home.journal(),
        &journal.encode(),
        &mut |line| note(line),
    )
    .map_err(|unwritten| {
        // A cancel wins: once the job has cancelled, any failure of the write
        // (a hold given up on at the cancel, or another refusal met at the
        // same moment) stops at `Stop::Cancelled`, with no journal line.
        road.go_on().err().unwrap_or_else(|| {
            journal_stop(&format!(
                "the journal at {phase} was not written: {}",
                unwritten.said
            ))
        })
    })
}

/// Steps 2 (the folders) to 7: everything the transaction acquires; the
/// inventories when all of it is in place.
fn acquire(
    road: &Road<'_>,
    home: &Home,
    running: &Running,
    rescue: &Path,
) -> Result<Inventories, Stop> {
    let Some(folders) = Folders::of(home, road.offer.txn()) else {
        return Err(journal_stop("the transaction's set folder has no name"));
    };
    for folder in [
        Some(folders.transaction.as_path()),
        Some(folders.download.as_path()),
        Some(folders.expand.as_path()),
        Some(folders.set.as_path()),
        rescue.parent(),
    ] {
        let Some(folder) = folder else {
            return Err(journal_stop("the rescue copy has no folder"));
        };
        install_txn::durable_create_dir(folder)
            .map_err(|failure| journal_failed("a transaction folder was not created", &failure))?;
    }
    road.go_on()?;

    // Step 3: the statement, kept beside `set\`; then the archive it names.
    let txn = road.offer.txn();
    let (statement, fetched) = vouched(
        road.transport,
        road.offer,
        road.post,
        &folders.download,
        road.vouching,
    )?;
    let kept = kept_statement(home, txn);
    for (from, to) in fetched.iter().zip(&kept) {
        road.tools.copy(from, to).map_err(|why| {
            note(&format!("the release statement was not kept: {why}"));
            Stop::Copy
        })?;
    }
    let archive = fetch_vouched_asset(
        road.transport,
        road.offer,
        road.post,
        &folders.download,
        &statement,
    )?;
    road.post.post(Step::Staged);

    // Step 4: the space the rest needs.
    let version = road.offer.to_version();
    let expanded = update_archive::declared_bytes(&archive, version).map_err(archive_refused)?;
    let needed = expanded.saturating_mul(2).saturating_add(running.size);
    let available = road
        .tools
        .available(&folders.transaction)
        .map_err(|why| journal_stop(&format!("the space left was not answered: {why}")))?;
    if available < needed {
        return Err(Stop::Space {
            short_by: needed - available,
        });
    }
    road.go_on()?;

    // Step 5: expand, and hold every member to the statement.
    let members = expand_release(&archive, version, &folders.expand)?;
    let _ = install_txn::durable_remove(&folders.download);
    let mut new = Vec::with_capacity(members.len());
    for member in &members {
        let (digest, size) = digest_of(&folders.expand.join(member)).map_err(|_| Stop::Copy)?;
        new.push(Member {
            name: member.clone(),
            digest: Digest::parse(&digest).map_err(|_| Stop::Copy)?,
            size,
        });
    }
    held_to_the_statement(&statement, &new)?;
    road.go_on()?;

    // Step 6: the set, copied, flushed, and verified again where it lies.
    for member in &new {
        road.tools
            .copy(
                &folders.expand.join(&member.name),
                &folders.set.join(&member.name),
            )
            .map_err(|_| Stop::Copy)?;
    }
    let _ = install_txn::durable_remove(&folders.expand);
    still_staged(&folders.set, &new, &kept, road.vouching)?;
    let signed = service_signature(&folders.set.join(EXECUTABLE), road.policy, &OUR_SERVICE);
    note(&format!("the staged {EXECUTABLE} is {}", signed.as_said()));
    road.go_on()?;

    // Step 7: the rescue copy of the running executable.
    road.tools.copy(road.exe, rescue).map_err(|_| Stop::Clone)?;
    rescue_is_the_running_build(rescue, running)?;
    road.go_on()?;

    // Step 8's inventories, measured under the lock.
    let Some(install) = road.exe.parent() else {
        return Err(journal_stop("the running executable has no folder"));
    };
    Ok(Inventories {
        old_shipped: running.shipped.clone(),
        old_present: present(install, &running.shipped, &new)?,
        new,
    })
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

/// **The members of a Windows release are the ones its statement lists**
/// (0.4.10 plan row 102): their names the same set — `folio.exe` and
/// `folio.msix` among them, the two the F-4 manifest cannot list — and each
/// SHA-256 and length the same (`VerifiedStatement::members_match`).
///
/// # Errors
/// [`Stop::Identity`], after its line naming the first difference.
fn held_to_the_statement(statement: &VerifiedStatement, set: &[Member]) -> Result<(), Stop> {
    let found: Vec<release_manifest::Member> = set
        .iter()
        .map(|member| release_manifest::Member {
            name: member.name.clone(),
            sha256: member.digest.to_string(),
            size: member.size,
        })
        .collect();
    statement
        .members_match(WINDOWS_X64, &found)
        .map_err(|refusal| statement_refused(&refusal))
}

/// **`set\` is what was verified**: the statement kept beside it (`kept`, its
/// text and its signature) verified again with `vouching`, every member's
/// digest and length the ones recorded, and the members held to the kept
/// statement — as the files lie there. Answers the kept statement.
///
/// # Errors
/// [`Stop::StatementRefused`] for a kept statement that is missing or no
/// longer verifies; [`Stop::Identity`] for a member that is not the one
/// recorded or listed.
fn still_staged(
    set: &Path,
    new: &[Member],
    kept: &[PathBuf; 2],
    vouching: &Vouching,
) -> Result<VerifiedStatement, Stop> {
    let statement = vouching.verified(&kept[0], &kept[1])?;
    for member in new {
        let (digest, size) = digest_of(&set.join(&member.name)).map_err(|_| Stop::Identity)?;
        if Digest::parse(&digest).ok() != Some(member.digest) || size != member.size {
            return Err(Stop::Identity);
        }
    }
    held_to_the_statement(&statement, new)?;
    Ok(statement)
}

/// **Whether a staged `folio.exe` carries our signing service's signature**
/// (0.4.10 plan row 102 §1.8, R4) — the updater's own reading, which decides
/// no step of the Prepare: part 3 of the row gives the Ready card its one
/// sentence from it. Windows' answer under `policy` must be a valid
/// signature whose subject is `signer`'s and whose leaf carries `signer`'s
/// identity OID; any refusal — unsigned, revoked, revocation offline or
/// unknown — reads as not signed by it.
#[must_use]
pub(crate) fn service_signature(
    exe: &Path,
    policy: &Policy,
    signer: &ServiceSigner,
) -> ServiceSignature {
    match trust::signature(exe, policy) {
        Ok(signed) => {
            let subject =
                bt_platform::msix::publisher_matches_subject(signer.subject, &signed.subject);
            let profile =
                trust::identity_oid(&signed.key_usages).is_ok_and(|oid| oid == signer.identity_oid);
            if subject && profile {
                ServiceSignature::Ours
            } else {
                ServiceSignature::Another
            }
        }
        Err(trust::Refusal::Unsigned) => ServiceSignature::Unsigned,
        Err(refusal) => ServiceSignature::Refused(refusal.to_string()),
    }
}

/// **What [`service_signature`] read** — one of four answers; only
/// [`Self::Ours`] is our signing service's signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ServiceSignature {
    /// A valid signature by our service: our subject and our profile.
    Ours,
    /// A valid signature by another signer or profile.
    Another,
    /// No signature at all.
    Unsigned,
    /// A signature Windows refused, by the refusal's words.
    Refused(String),
}

impl ServiceSignature {
    /// The answer, as a diagnostics line says it.
    fn as_said(&self) -> String {
        match self {
            Self::Ours => "signed by our signing service".to_owned(),
            Self::Another => "signed, by another signer than our signing service".to_owned(),
            Self::Unsigned => "not signed".to_owned(),
            Self::Refused(refusal) => format!("not signed by our signing service ({refusal})"),
        }
    }
}

/// **The rescue copy is the running build**: the running image's digest and
/// length. Equal bytes are the same build; its signature is not read (R3).
///
/// # Errors
/// [`Stop::Clone`].
fn rescue_is_the_running_build(rescue: &Path, running: &Running) -> Result<(), Stop> {
    let (digest, size) = digest_of(rescue).map_err(|_| Stop::Clone)?;
    if digest != running.digest || size != running.size {
        return Err(Stop::Clone);
    }
    Ok(())
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
                let (digest, size) = digest_of(&path).map_err(|error| {
                    journal_stop(&format!("an installed file was not read: {error}"))
                })?;
                present.push(Member {
                    name: name.to_owned(),
                    digest: Digest::parse(&digest)
                        .map_err(|_| journal_stop("an installed file's digest is not one"))?,
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
/// applier), how that copy was installed, and what a release statement is
/// verified with. The version the staged set installs is the kept
/// statement's (0.4.10 plan row 102): the journal records no tag and no
/// `to_version`, and needs neither — the statement kept beside `set\` names
/// the version and every member, and it is verified again each time.
pub(crate) struct Resume<'a> {
    pub(crate) exe: &'a Path,
    pub(crate) channel: Option<Channel>,
    pub(crate) vouching: &'a Vouching,
}

/// **Revalidate a staged transaction before O resumes it** ((b).1 F-17: hash,
/// signature, manifest, classification) — [`staged_as_verified`]; a failure
/// discards the transaction (`Discarded`, then `update_prepare::clear`) and
/// says why. Answers the transaction and the version its set installs.
///
/// # Errors
/// The [`Stop`] that failed; the transaction is gone.
pub(crate) fn revalidate(
    worker: &WorkerCtx,
    staged: Staged,
    resume: &Resume<'_>,
) -> Result<(Staged, String), Stop> {
    match staged_as_verified(&staged.home, &staged.journal, resume) {
        Ok(version) => Ok((staged, version)),
        Err(stop) => {
            let _ = discard(worker, staged, &Event::Discarded);
            Err(stop)
        }
    }
}

/// **O's resume at a later launch** (U-33; `update_prepare::Resumer`):
/// [`revalidate`], whose version — the kept statement's — the offer is
/// rebuilt from, never from a download.
///
/// # Errors
/// The [`Stop`] that failed; the transaction is gone.
pub(crate) fn resume(
    worker: &WorkerCtx,
    staged: Staged,
    resume: &Resume<'_>,
) -> Result<(Staged, String), Stop> {
    revalidate(worker, staged, resume)
}

/// **The resumer of the running copy** (`update_job::resumer_for_this_copy`):
/// [`resume`] for the executable at `exe`, verifying with `vouching`, for the
/// channel the job hands it.
#[must_use]
pub(crate) fn resumer(exe: PathBuf, vouching: Vouching) -> crate::update_prepare::Resumer {
    Box::new(move |worker, staged, channel| {
        resume(
            worker,
            staged,
            &Resume {
                exe: &exe,
                channel,
                vouching: &vouching,
            },
        )
    })
}

/// **The staged transaction `journal` of `home` is still what was verified**
/// — the check a resume in O and the applier both run, the applier under the
/// transaction lock after O has let go and before it writes the entrance
/// (U-23): this copy's channel and home ([`eligible`]); the installed image
/// still the `folio.exe` the journal recorded as present; the release
/// statement kept in `H\<txn>\` verified again, and every staged member of
/// `set\` still its recorded digest and length and the one the statement
/// lists ([`still_staged`]); the rescue copy still the old image. Answers the
/// version the statement names. A transaction folder with no statement —
/// one an older build prepared — fails here and is refused.
///
/// # Errors
/// The [`Stop`] that failed. Nothing is written.
pub(crate) fn staged_as_verified(
    home: &Home,
    journal: &Journal,
    resume: &Resume<'_>,
) -> Result<String, Stop> {
    let Layout::Members(inventories) = &journal.body.layout else {
        return Err(journal_stop(
            "the staged journal's layout is not Folio's own",
        ));
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
    let Some(set) = home.members_folder(journal.txn, Place::Set) else {
        return Err(journal_stop(
            "the staged transaction's set folder has no name",
        ));
    };
    let statement = still_staged(
        &set,
        &inventories.new,
        &kept_statement(home, journal.txn),
        resume.vouching,
    )?;
    let Some(rescue) = home.rescue_copy(journal.txn, name) else {
        return Err(journal_stop("the staged rescue copy has no name"));
    };
    if !is(&rescue, old) {
        return Err(Stop::Clone);
    }
    Ok(statement.what_it_says().version.clone())
}

#[cfg(test)]
#[path = "update_prepare_windows_tests.rs"]
pub(crate) mod tests;
