//! **What the two Prepares share** — the update job's drivers on macOS
//! (`update_prepare_macos`, U-27) and Windows (`update_prepare_windows`, U-20),
//! from the press to `Prepared` (`docs/plans/design/self-update-2026-09-16.md`
//! §C.2, revision (b) F-17 and the W1–W2 / M1–M2 rows).
//!
//! Everything here is the same on both platforms, and was the macOS driver's
//! until the Windows one arrived to share it rather than copy it:
//!
//! * **the worker** both run on ([`WORKER`]) and the progress a fetch reports
//!   to the job ([`fetching`]);
//! * **the release statement** (0.4.10 plan row 102, part 1b): the offer's
//!   `RELEASE-STATEMENT.txt` and its signature fetched by tag and verified with
//!   this copy's embedded keys, none it remembers as retired ([`Vouching`],
//!   [`vouched`]) — the one thing a Prepare trusts, on both platforms; what it
//!   retires remembered in the data folder's `release-keys.json`, whose one
//!   writer this is; the platform's file then fetched by the name the
//!   statement gives and held to its SHA-256 and size
//!   ([`fetch_vouched_asset`]), hashed a bounded chunk at a time
//!   ([`digest_of`]). No signature of the operating system's decides anything
//!   here;
//! * **the worker's last word** ([`finish`]): one report, posted once the
//!   worker has let go of the transaction or handed it to the job — a Cancel
//!   included, so its lock is known free when its report is read;
//! * **giving a transaction up**: `Abandoned` recorded durably, then the
//!   transaction cleared away in the protocol's order ([`abandon`],
//!   [`discard`], [`clear`] — an image mounted under the transaction is
//!   detached first, which only macOS can have);
//! * **the job owner's pass at a later launch** ([`at_launch`]): an
//!   `Allocated` transaction a dead Prepare left is swept (W1, M1), a
//!   `Prepared` one is kept and offered again — counted
//!   (`LaunchedWithoutResume`) only when the run before ended orderly without
//!   pressing Restart for it, and discarded at the second such launch (W2,
//!   M2); after a restart that did not happen it is offered with the card that
//!   says so and counted not at all (`MissedRestartOffered`; 0.4.8 E3,
//!   `update_txn::launch_event`). It reads the phase and how the run before
//!   ended, so it is one function for both layouts. What *revalidating* a
//!   staged transaction means differs (a bundle's identity, a member set's
//!   digests), and each driver has its own ([`Resumer`]). The whole pass as the
//!   update job runs it at a launch — the phase's answer, then the resume of a
//!   kept set with the offer rebuilt from its own version — is
//!   [`settle_at_launch`] (U-33).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_txn::{self, Hold};

use bt_platform::HostPlatform;
use bt_winres::release_statement::{
    KeyId, KeySet, OfferedRelease, SIGNATURE_BYTES, SIGNATURE_FILE, STATEMENT_FILE,
    STATEMENT_MAX_BYTES, StatementError, VerifiedStatement, verify_statement,
};

use crate::install_channel::Channel;
use crate::update_apply::{
    JOURNAL_HELD_WITHIN, Spelling, WaitClock, WaitWithin, WallClock, failure_words,
    write_journal_on,
};
use crate::update_handoff::Staged;
use crate::update_job::{
    Bytes, Fetching, Landed, Offer, Poster, SignatureNote, Step, Stop, Transport,
};
use crate::update_txn::{
    Action, Actor, Asker, Disk, Effect, Event, Home, Journal, Located, PhaseKind, PreviousRun,
    Role, TxnId, decide, launch_event, may,
};

/// The worker a Prepare runs on — one per press, one job per process (R.3).
pub(crate) const WORKER: &str = "bt-update-job";

/// How much more of the asset arrives between two progress reports to the
/// job: the card's bar moves in steps of this, and the job's inbox holds a few
/// dozen reports for a release file rather than one per chunk.
const REPORT_EVERY: u64 = 1024 * 1024;

/// **What a fetch for `post`'s transaction reports to, and asks**: the bytes
/// so far, as [`Step::Received`] every [`REPORT_EVERY`] and once at the end;
/// and the job's cancel flag, which the download door stops on.
pub(crate) fn fetching(post: &Poster) -> Fetching {
    let reported = Arc::new(AtomicU64::new(0));
    let report = post.clone();
    Fetching {
        report: Arc::new(move |bytes: Bytes| {
            let last = reported.load(Ordering::Relaxed);
            let whole = bytes.total == Some(bytes.received);
            if whole || bytes.received >= last.saturating_add(REPORT_EVERY) {
                reported.store(bytes.received, Ordering::Relaxed);
                report.post(Step::Received(bytes));
            }
        }),
        cancelled: post.cancel_flag(),
    }
}

// ── the release statement ───────────────────────────────────────────────────

/// **The file a copy remembers retired release keys in**, in its data folder
/// (`bt_persist::ReleaseKeysV1`, 0.4.10 plan row 102, ruling R8).
pub(crate) const RELEASE_KEYS_FILE_NAME: &str = "release-keys.json";

/// **What a release statement is verified with on a copy, and where what it
/// retires is remembered**: the keys this build embeds
/// (`release_statement::RELEASE_KEYS`) and the data folder's
/// [`RELEASE_KEYS_FILE_NAME`]. A Prepare, a resume and the applier each verify
/// with one; only a Prepare remembers ([`Vouching::remember`]).
#[derive(Clone, Debug)]
pub(crate) struct Vouching {
    keys: KeySet<'static>,
    memory: PathBuf,
}

/// **What `release-keys.json` holds, as a statement check reads it**: the
/// document — or, for a file this build refuses, the default carrying the ids
/// the refused file retired (`bt_persist::read_release_keys_keeping`) — and
/// whether a write may replace the file.
struct Remembered {
    document: bt_persist::ReleaseKeysV1,
    /// **Not over a newer build's document**, nor over a file whose bytes
    /// were neither read nor kept: only a missing file, one this build read,
    /// or one whose refused bytes were kept beside it may be written over.
    writable: bool,
}

impl Vouching {
    /// **The product's**: the embedded keys, remembered in `data`.
    #[must_use]
    pub(crate) fn in_data_folder(data: &Path) -> Self {
        Self {
            keys: KeySet::EMBEDDED,
            memory: data.join(RELEASE_KEYS_FILE_NAME),
        }
    }

    /// **The running copy's**: [`Self::in_data_folder`] of this process's data
    /// folder (`persist::storage_dir`).
    #[must_use]
    pub(crate) fn of_this_copy() -> Self {
        Self::in_data_folder(&crate::persist::storage_dir())
    }

    /// **A test's**: the throwaway test keys (`release_statement::TEST_KEYS`),
    /// remembered in `memory`.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn under_test_keys(memory: PathBuf) -> Self {
        Self {
            keys: KeySet::of(&bt_winres::release_statement::TEST_KEYS),
            memory,
        }
    }

    /// **The statement at `statement`, signed by the 64 bytes at `signature`,
    /// verified** with these keys and none remembered as retired
    /// (`release_statement::verify_statement`). Each file is refused on its
    /// size before it is read.
    ///
    /// # Errors
    /// [`Stop::StatementRefused`], after its line ([`statement_refused`]).
    pub(crate) fn verified(
        &self,
        statement: &Path,
        signature: &Path,
    ) -> Result<VerifiedStatement, Stop> {
        let text = statement_file_bytes(statement, STATEMENT_MAX_BYTES as u64)?;
        let signed = statement_file_bytes(signature, SIGNATURE_BYTES as u64)?;
        let retired = self.retires_on_disk().retired_ids();
        verify_statement(&text, &signed, &self.keys, &retired)
            .map_err(|refusal| statement_refused(&refusal))
    }

    /// **Remember the keys `statement` retires**, so this copy refuses them
    /// from now on (§1.6): written only when the list grows, and never over a
    /// file a write may not replace ([`Remembered::writable`]). A retire that
    /// is not remembered is said in one line and the Prepare goes on: the
    /// statement verified, and the next release's statement retires again.
    pub(crate) fn remember(&self, statement: &VerifiedStatement) {
        let retires = &statement.what_it_says().retires;
        if retires.is_empty() {
            return;
        }
        let mut remembered = self.retires_on_disk();
        if !remembered
            .document
            .remember_retired(retires.iter().map(ToString::to_string))
        {
            return;
        }
        if !remembered.writable {
            note(
                "a retired release key is not remembered: release-keys.json is not this build's to replace",
            );
            return;
        }
        match bt_persist::write_release_keys_atomic(&self.memory, &remembered.document) {
            Ok(()) => note(&format!(
                "release keys retired from now on: {}",
                remembered.document.retired.join(" ")
            )),
            Err(error) => note(&format!("a retired release key is not remembered: {error}")),
        }
    }

    /// What `release-keys.json` holds now, read the way every document of a
    /// trial is (`update_trial::keeping`).
    fn retires_on_disk(&self) -> Remembered {
        let (document, report) =
            bt_persist::read_release_keys_keeping(&self.memory, crate::update_trial::keeping());
        crate::update_trial::owe_copy(&report, &self.memory, |path| {
            let _ = bt_persist::read_release_keys(path);
        });
        let writable = match &report {
            bt_persist::ReadReport::NotFound | bt_persist::ReadReport::Loaded => true,
            bt_persist::ReadReport::FellBackToDefaults { reason, kept } => {
                note(&format!("release-keys.json is refused: {reason:?}"));
                kept.is_some()
                    && !matches!(
                        reason,
                        bt_persist::FallbackReason::FutureSchemaVersion { .. }
                    )
            }
        };
        Remembered { document, writable }
    }
}

impl Remembered {
    /// The ids remembered, each one a key id is spelled as; any other entry is
    /// left out.
    fn retired_ids(&self) -> Vec<KeyId> {
        self.document
            .retired
            .iter()
            .filter_map(|id| KeyId::parse_key_id(id))
            .collect()
    }
}

/// **The bytes of a statement's file, at most `most` of them**: one open and
/// one read bounded on the bytes themselves (`most + 1` asked for, more than
/// `most` refused), so a server that answers a statement with an archive is
/// not read whole — nor a link in the file's place, whose own size says
/// nothing of its target's (the 1b+2 review's N11).
fn statement_file_bytes(path: &Path, most: u64) -> Result<Vec<u8>, Stop> {
    use std::io::Read as _;
    let file = file_reads::open(Lane::Update, path)
        .map_err(|error| refused_with(&format!("its file is not there: {error}")))?;
    let mut bytes = Vec::new();
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| refused_with(&format!("its file was not read: {error}")))?;
    if bytes.len() as u64 > most {
        return Err(refused_with(&format!("its file is more than {most} bytes")));
    }
    Ok(bytes)
}

/// [`Stop::StatementRefused`], and its line.
fn refused_with(why: &str) -> Stop {
    note(&format!("the release statement is refused: {why}"));
    Stop::StatementRefused
}

/// **The stop a statement's refusal is reported as, and its one line**: a
/// release that needs a newer build is [`Stop::TooOld`] (before the large
/// download); a file that is not the one the statement lists, [`Stop::Sums`];
/// a member set that is not the statement's, [`Stop::Identity`]; every other
/// refusal — the signature, the key, a retired key, the tag, the version, no
/// file for this platform — [`Stop::StatementRefused`].
pub(crate) fn statement_refused(refusal: &StatementError) -> Stop {
    note(&format!("the release statement is refused: {refusal}"));
    match refusal {
        StatementError::TooOld { .. } => Stop::TooOld,
        StatementError::AssetDiffers(_) => Stop::Sums,
        StatementError::MembersDiffer(_) => Stop::Identity,
        _ => Stop::StatementRefused,
    }
}

/// **Where a staged transaction keeps the statement it was prepared from**:
/// `H\<txn>\RELEASE-STATEMENT.txt` and its `.sig`, beside `set\` — read
/// again before a resume and before the apply.
#[must_use]
pub(crate) fn kept_statement(home: &Home, txn: TxnId) -> [PathBuf; 2] {
    let transaction = home.transaction(txn);
    [STATEMENT_FILE, SIGNATURE_FILE].map(|name| transaction.join(name))
}

/// [`Stop::Cancelled`] once the job has cancelled `post`'s transaction,
/// otherwise `stop`.
fn unless_cancelled(post: &Poster, stop: Stop) -> Stop {
    if post.cancelled() {
        Stop::Cancelled
    } else {
        stop
    }
}

/// **The first step after `Allocated`, on both platforms**: the offer's
/// statement and its signature fetched by tag into `into`, verified
/// ([`Vouching::verified`]), what it retires remembered, and held to the offer
/// — its tag and version the offer's, its `min_from` no newer than this build
/// (`VerifiedStatement::vouches_for`). Answers the statement and where its two
/// files are. The fetch reports no progress: the card's bar is the asset's.
///
/// # Errors
/// [`Stop::Download`], [`Stop::Cancelled`]; [`Stop::TooOld`],
/// [`Stop::StatementRefused`] ([`statement_refused`]).
pub(crate) fn vouched(
    transport: &dyn Transport,
    offer: &Offer,
    post: &Poster,
    into: &Path,
    vouching: &Vouching,
) -> Result<(VerifiedStatement, [PathBuf; 2]), Stop> {
    let quiet = Fetching {
        report: Arc::new(|_| {}),
        cancelled: post.cancel_flag(),
    };
    let [statement, signature] = offer.requests().map(|request| {
        transport
            .fetch(&request, into, &quiet)
            .map_err(|_| unless_cancelled(post, Stop::Download))
    });
    let files = [statement?, signature?];
    if post.cancelled() {
        return Err(Stop::Cancelled);
    }
    let verified = vouching.verified(&files[0], &files[1])?;
    vouching.remember(&verified);
    verified
        .vouches_for(&OfferedRelease {
            tag: offer.tag(),
            version: offer.to_version(),
            running: crate::version::VERSION,
        })
        .map_err(|refusal| statement_refused(&refusal))?;
    Ok((verified, files))
}

/// **The platform's file, by the name `statement` gives it**, fetched by the
/// offer's tag into `into` (reporting to the card), and held to the
/// statement's SHA-256 and size for the offer's platform
/// (`VerifiedStatement::asset_matches`). Answers where it is.
///
/// # Errors
/// [`Stop::StatementRefused`] when the statement lists no file for the
/// platform; [`Stop::Download`], [`Stop::Cancelled`]; [`Stop::Sums`] for a file
/// that is not the one listed.
pub(crate) fn fetch_vouched_asset(
    transport: &dyn Transport,
    offer: &Offer,
    post: &Poster,
    into: &Path,
    statement: &VerifiedStatement,
) -> Result<PathBuf, Stop> {
    let platform = offer.release_platform();
    let name = statement
        .asset_for(platform)
        .map_err(|refusal| statement_refused(&refusal))?
        .name
        .clone();
    let file = transport
        .fetch(&offer.asset_request(&name), into, &fetching(post))
        .map_err(|_| unless_cancelled(post, Stop::Download))?;
    if post.cancelled() {
        return Err(Stop::Cancelled);
    }
    let (digest, size) = digest_of(&file).map_err(|_| Stop::Sums)?;
    statement
        .asset_matches(platform, &digest, size)
        .map_err(|refusal| statement_refused(&refusal))?;
    Ok(file)
}

/// **A file's SHA-256 and length**, lowercase hex — read a bounded chunk at a
/// time on `file_reads`' `Lane::Update`.
///
/// # Errors
/// The read's, as the operating system gave it.
pub(crate) fn digest_of(path: &Path) -> std::io::Result<(String, u64)> {
    use std::io::Read;
    let mut file = file_reads::open(Lane::Update, path)?;
    let mut hasher = bt_winres::digest::Sha256::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut length = 0u64;
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
        length += read as u64;
    }
    Ok((bt_winres::digest::hex(&hasher.finish()), length))
}

// ── the journal's writes ────────────────────────────────────────────────────

/// **One line of the update job's in `diagnostics.log`**.
pub(crate) fn note(line: &str) {
    crate::diagnostics::note(&format!("Folio: update job — {line}"));
}

/// **[`Stop::Journal`], and its one `diagnostics.log` line**: what the
/// Prepare could not do with its home, its folders or its journal, never a
/// path (T-UPDATE-PREPARE-REVALIDATION-RESUME-FLAKE: every such stop was
/// silent, so a failure seen once could not be told from any other) — on
/// both Prepares (T-PLATFORM-ONLY-TESTS-CFG-GATED gave the macOS one its
/// lines).
pub(crate) fn journal_stop(what: &str) -> Stop {
    note(&format!("the Prepare stops at its journal: {what}"));
    Stop::Journal
}

/// [`journal_stop`] for a failure of the install door, in
/// `update_apply::failure_words`.
pub(crate) fn journal_failed(what: &str, failure: &install_txn::Failure) -> Stop {
    journal_stop(&format!("{what}: {}", failure_words(failure)))
}

/// **Write `journal` durably as the job owner, riding a hold**: while another
/// program holds `journal.json` open (a scanner, an indexer: `install_txn::
/// Failure::refused_while_open`, Windows only) the rename is asked again on
/// the wall clock until [`JOURNAL_HELD_WITHIN`] has passed, each round said in
/// `diagnostics.log` (U-34, 0.4.8 E4 — the applier's
/// `update_apply::write_journal`; T-UPDATE-PREPARE-REVALIDATION-RESUME-FLAKE).
/// Read and paused on `clock`: [`WallClock`] for the writes whose callers
/// hand none (a launch's count, [`abandon`]); a Prepare's road's own for its
/// abandonment ([`abandon_on`]).
///
/// # Errors
/// The last failure, in `update_apply::failure_words`.
fn journal_written_by_the_owner(
    worker: &WorkerCtx,
    clock: &dyn WaitClock,
    home: &Home,
    journal: &Journal,
) -> Result<(), String> {
    write_journal_on(
        WaitWithin {
            worker,
            within: JOURNAL_HELD_WITHIN,
            clock,
        },
        None,
        Spelling::WithoutPath,
        &home.journal(),
        &journal.encode(),
        &mut |line| note(line),
    )
    .map_err(|unwritten| unwritten.said)
}

// ── the worker's last word ──────────────────────────────────────────────────

/// **End a Prepare's worker with its one last report**, posted only once the
/// worker holds nothing of the transaction: `Verified` hands the staged
/// transaction — lock and all — to the job, with what the Ready card says of
/// its signature ([`SignatureNote`]); every other end has already let
/// the lock go (`abandon`, then the lock dropped, in `prepare_on`), and says
/// [`Step::Stopped`] with why. A Cancel ends the same way, with
/// [`Stop::Cancelled`]: the job has moved on, so the report is stale to it and
/// revives nothing, but whoever reads it knows the transaction is gone and its
/// lock free — the journal's removal is not that sign, since the lock is let
/// go only after it (the journal goes last, under the lock).
pub(crate) fn finish(
    worker: &WorkerCtx,
    post: &Poster,
    prepared: Result<(Staged, SignatureNote), Stop>,
) {
    match prepared {
        Ok((staged, note)) => {
            // Cancelled between the last look and the report: the job has
            // moved on, so the transaction is given up.
            if let Err(staged) = post.verified(staged, note) {
                let _ = discard(worker, *staged, &Event::Discarded);
                post.post(Step::Stopped(Stop::Cancelled));
            }
        }
        Err(stop) => post.post(Step::Stopped(stop)),
    }
}

// ── giving a transaction up ─────────────────────────────────────────────────

/// **Record `event` (→ `Abandoned`) over `journal` and clear the
/// transaction away.** When the abandonment cannot be written, the journal on
/// the disk is still `journal`'s phase, and the clearing is done under it —
/// O may clear an `Allocated` transaction (W1, M1) as well as an `Abandoned`
/// one.
///
/// # Errors
/// The event cannot be recorded, or a step of [`clear`] failed; what is left
/// is the next launch's to sweep.
pub(crate) fn abandon(
    worker: &WorkerCtx,
    home: &Home,
    journal: &Journal,
    event: &Event,
) -> Result<(), String> {
    abandon_on(worker, &WallClock, home, journal, event)
}

/// [`abandon`], its held write asked again on `clock` — a Prepare's road's
/// (T-UPDATE-PREPARE-REVALIDATION-RESUME-FLAKE).
///
/// # Errors
/// As [`abandon`].
pub(crate) fn abandon_on(
    worker: &WorkerCtx,
    clock: &dyn WaitClock,
    home: &Home,
    journal: &Journal,
    event: &Event,
) -> Result<(), String> {
    let phase = match journal.advance(event) {
        Ok(abandoned) => match journal_written_by_the_owner(worker, clock, home, &abandoned) {
            Ok(()) => abandoned.body.phase.kind(),
            Err(_) => journal.body.phase.kind(),
        },
        Err(refusal) => return Err(format!("{refusal:?}")),
    };
    clear(worker, home, journal.txn, phase)
}

/// **Give a staged transaction up** (`Discarded`, or the second launch's
/// `LaunchedWithoutResume`): recorded, cleared, and its lock let go.
///
/// # Errors
/// As [`abandon`].
pub(crate) fn discard(worker: &WorkerCtx, staged: Staged, event: &Event) -> Result<(), String> {
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
///
/// # Errors
/// The first effect that O may not do in `phase`, or that failed.
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
                bt_platform::macos_update::detach_all_under(worker, &home.transaction(txn))
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
pub(crate) enum AtLaunch {
    /// No journal: no transaction.
    Nothing,
    /// Another holder has the transaction lock; nothing was touched.
    Busy,
    /// A dead Prepare's `Allocated` transaction was swept (W1, M1).
    Swept,
    /// A `Prepared` transaction is kept, lock held — this launch counted
    /// against it when the run before ended orderly, and nothing was written
    /// when it did not: it may still be resumed (revalidating) in this launch.
    Kept(Box<Staged>),
    /// **A `Prepared` transaction whose restart did not happen** (0.4.8 E3):
    /// the mark spent (`MissedRestartOffered`), nothing counted, the lock
    /// held; resumed, its card says the restart did not happen.
    RestartMissed(Box<Staged>),
    /// A `Prepared` transaction reached its second counted launch and was
    /// discarded.
    Discarded,
    /// Nothing here is the job owner's to do: a journal it cannot read, or
    /// one in a phase that belongs to the applier or recovery.
    Left,
}

/// **Why a press finds a journal already there** (both Prepares, step 2): the
/// installation is another transaction's ([`Stop::Busy`]), or that journal is
/// one this build cannot read whole — another Folio's update is not finished,
/// and the build that wrote it finishes it ([`Stop::Newer`], 0.4.8 E1,
/// `update_txn::Role::JobOwner`) — a journal file that could not be read
/// included (E1 round 2). One gone since the press looked is [`Stop::Busy`]:
/// another holder had it.
pub(crate) fn journal_there(home: &Home) -> Stop {
    match Role::JobOwner.sight_of_read(file_reads::read(Lane::UpdateJournal, home.journal())) {
        Some(crate::update_txn::Sight::Known(_)) | None => Stop::Busy,
        Some(
            crate::update_txn::Sight::Header { .. }
            | crate::update_txn::Sight::Envelope { .. }
            | crate::update_txn::Sight::Unreadable(_),
        ) => Stop::Newer,
    }
}

/// **The job owner's pass at a launch** ((b).2's W1–W2 and M1–M2): the lock,
/// one read of the journal, and `update_txn::decide` as the job owner; over a
/// `Prepared` journal, what `previous` — how the run before this launch ended
/// — makes of this launch (`update_txn::launch_event`, 0.4.8 E3).
///
/// # Errors
/// A step that failed, as a sentence; whatever it left is the next launch's.
pub(crate) fn at_launch(
    worker: &WorkerCtx,
    home: &Home,
    previous: PreviousRun,
) -> Result<AtLaunch, String> {
    if std::fs::symlink_metadata(home.journal()).is_err() {
        return Ok(AtLaunch::Nothing);
    }
    let Some(lock) = install_txn::try_hold(&home.lock(), Hold::Exclusive)
        .map_err(|failure| failure.to_string())?
    else {
        return Ok(AtLaunch::Busy);
    };
    // A journal this build cannot read whole — or whose file could not be
    // read (E1 round 2) — is left to the build that wrote it (E1,
    // `update_txn::Role::JobOwner`): the offer still shows, and the press
    // says so (`update_job::Stop::Newer`).
    let journal =
        match Role::JobOwner.sight_of_read(file_reads::read(Lane::UpdateJournal, home.journal())) {
            None => return Ok(AtLaunch::Nothing),
            Some(crate::update_txn::Sight::Known(journal)) => journal,
            Some(
                crate::update_txn::Sight::Header { .. }
                | crate::update_txn::Sight::Envelope { .. }
                | crate::update_txn::Sight::Unreadable(_),
            ) => return Ok(AtLaunch::Left),
        };
    // A job owner's answer is read from the phase alone (`decide`'s first
    // arm); the rest of the description is what an owner of no destructive
    // phase sees: no entrance, no receipt, no trial.
    let disk = Disk {
        journal: &journal,
        asker: Asker::JobOwner,
        entrance: false,
        located: Located::Members(Vec::new()),
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
            let Some(event) = launch_event(&journal.body.phase, previous) else {
                // The run before did not end orderly: no deferral to count,
                // and nothing is written.
                return Ok(AtLaunch::Kept(Box::new(Staged {
                    home: home.clone(),
                    journal,
                    lock,
                })));
            };
            let counted = journal
                .advance(&event)
                .map_err(|refusal| format!("{refusal:?}"))?;
            journal_written_by_the_owner(worker, &WallClock, home, &counted)?;
            if counted.body.phase.kind() == PhaseKind::Abandoned {
                clear(worker, home, counted.txn, PhaseKind::Abandoned)?;
                drop(lock);
                return Ok(AtLaunch::Discarded);
            }
            let staged = Box::new(Staged {
                home: home.clone(),
                journal: counted,
                lock,
            });
            Ok(if event == Event::MissedRestartOffered {
                AtLaunch::RestartMissed(staged)
            } else {
                AtLaunch::Kept(staged)
            })
        }
        _ => Ok(AtLaunch::Left),
    }
}

/// **How this copy revalidates a staged transaction before O resumes it**
/// (F-17), on the job's worker, given how this copy was installed: the staged
/// transaction back with **the version its set installs**, read from what was
/// verified — the kept release statement on Windows (0.4.10 plan row 102), the
/// staged bundle's recorded version on macOS — never from a download, and
/// what the Ready card says of the set's signature, read again
/// ([`SignatureNote`], row 102 part 3). A
/// failure has already discarded the transaction (`Discarded`, then
/// [`clear`]) and says why. `update_job::resumer_for_this_copy` is the
/// product's; a test holds the real revalidation to its own trust root.
pub(crate) type Resumer = Box<
    dyn FnOnce(&WorkerCtx, Staged, Option<Channel>) -> Result<(Staged, String, SignatureNote), Stop>
        + Send,
>;

/// **The resumer of a copy that cannot revalidate** — a platform with no
/// release, a copy that cannot name its own executable or bundle: whatever it
/// finds staged is discarded, since nothing here could ever install it.
#[must_use]
pub(crate) fn no_resume() -> Resumer {
    Box::new(|worker, staged, _| {
        let _ = discard(worker, staged, &Event::Discarded);
        Err(Stop::NotOurs)
    })
}

/// **This launch's job-owner pass, whole** (U-33; (b).2's W1–W2 and M1–M2),
/// on the job's worker: [`at_launch`] with `previous`, how the run before this
/// launch ended (the product's is `persist::previous_run_ended_orderly`, 0.4.8
/// E3), then —
///
/// * `Swept`, `Discarded`, `Nothing`, `Left`, or a step that failed →
///   [`Landed::Ordinary`]: the launch offers as usual;
/// * `Busy` → [`Landed::Busy`]: another holder has the transaction, and this
///   launch offers nothing;
/// * `Kept` or `RestartMissed` with `offers` off → whatever was recorded stays
///   and the lock is let go ([`Landed::Ordinary`]): a build whose job never
///   offers shows no card, and the second counted launch discards;
/// * `Kept` or `RestartMissed` with `offers` on → `resume` revalidates the
///   staged set for `channel` and answers its version; the offer is minted
///   again from that version under the transaction's own identity, for
///   `platform`'s files — [`Landed::Resumed`], the verified card of this
///   launch, which after a restart that did not happen says so. A
///   revalidation that fails has discarded the set ([`Landed::Ordinary`]).
///
/// What it did is said in one `diagnostics.log` line, with no path.
pub(crate) fn settle_at_launch(
    worker: &WorkerCtx,
    home: &Home,
    offers: bool,
    resume: Resumer,
    channel: Option<Channel>,
    platform: HostPlatform,
    previous: PreviousRun,
) -> Landed {
    let say = note;
    let (staged, restart_missed) = match at_launch(worker, home, previous) {
        Ok(AtLaunch::Kept(staged)) => (staged, false),
        Ok(AtLaunch::RestartMissed(staged)) => (staged, true),
        Ok(AtLaunch::Busy) => return Landed::Busy,
        Ok(AtLaunch::Swept) => {
            say("an unfinished download of an earlier launch is cleared");
            return Landed::Ordinary;
        }
        Ok(AtLaunch::Discarded) => {
            say("the update prepared at an earlier launch is discarded at its second launch");
            return Landed::Ordinary;
        }
        Ok(AtLaunch::Nothing | AtLaunch::Left) => return Landed::Ordinary,
        Err(failure) => {
            say(&format!(
                "an earlier launch's update is kept for the next launch: {failure}"
            ));
            return Landed::Ordinary;
        }
    };
    if restart_missed {
        say(
            "the restart of the update prepared at an earlier launch did not happen; it is offered again",
        );
    } else if previous == PreviousRun::Unfinished {
        say(
            "the run before did not reach its clean exit; the update prepared at an earlier launch is kept and not counted",
        );
    }
    if !offers {
        drop(staged);
        say("a verified update is kept for a later launch; offers are off in this build");
        return Landed::Ordinary;
    }
    let txn = staged.journal.txn;
    match resume(worker, *staged, channel) {
        Ok((staged, version, note)) => match Offer::mint(txn, &format!("v{version}"), platform) {
            Some(offer) => Landed::Resumed {
                offer,
                staged: Box::new(staged),
                restart_missed,
                note,
            },
            None => {
                let _ = discard(worker, staged, &Event::Discarded);
                say(&format!(
                    "the update prepared at an earlier launch names no release ({version}) and is discarded"
                ));
                Landed::Ordinary
            }
        },
        Err(stop) => {
            say(&format!(
                "the update prepared at an earlier launch is discarded: {}",
                stop.why()
            ));
            Landed::Ordinary
        }
    }
}

/// **Release statements a test publishes** (0.4.10 plan row 102, part 1b):
/// built from what the test serves and signed by one of the throwaway test
/// keys (`release_statement::TEST_KEYS`, whose private halves are public and
/// protect nothing) or by a key no build embeds. The seeds stay here, in test
/// code only; `tests::the_test_seeds_are_the_test_keys` holds each to its
/// public half.
#[cfg(test)]
pub(crate) mod test_statements {
    use bt_winres::digest::{hex, sha256};
    use bt_winres::release_manifest::Member;
    use bt_winres::release_statement::{KeyId, Statement, StatementAsset};

    /// **Who signs a test's statement.**
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Signer {
        /// The live test key, `TEST_KEYS[0]`.
        Live,
        /// The spare test key, `TEST_KEYS[1]`, which may retire the live one.
        Spare,
        /// A key no build embeds.
        Foreign,
    }

    /// The seeds of `TEST_KEYS`, live then spare (RFC 8032 private halves).
    const TEST_KEY_SEEDS: [[u8; 32]; 2] = [
        [
            0x10, 0x28, 0xb5, 0x4e, 0x7c, 0xc4, 0x77, 0xc9, 0xc6, 0x99, 0x8a, 0xf8, 0x19, 0x6f,
            0x23, 0x61, 0x52, 0x0e, 0xb9, 0xae, 0x44, 0x10, 0x0e, 0x3e, 0xe3, 0x19, 0x80, 0x93,
            0x1a, 0x4e, 0x57, 0xe5,
        ],
        [
            0x5d, 0x60, 0xcf, 0xb8, 0xc3, 0xf3, 0x9c, 0x85, 0x78, 0x3e, 0xfb, 0xb7, 0x30, 0x6c,
            0xad, 0xc7, 0x89, 0x13, 0x44, 0xbb, 0xd8, 0x65, 0x8e, 0x2f, 0xc3, 0x66, 0x9f, 0xb8,
            0xa6, 0xf6, 0xac, 0x20,
        ],
    ];

    /// A seed whose public half no build embeds.
    const FOREIGN_SEED: [u8; 32] = [0x77; 32];

    /// `signer`'s key pair.
    pub(crate) fn pair_of(signer: Signer) -> ed25519_compact::KeyPair {
        let seed = match signer {
            Signer::Live => TEST_KEY_SEEDS[0],
            Signer::Spare => TEST_KEY_SEEDS[1],
            Signer::Foreign => FOREIGN_SEED,
        };
        ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed))
    }

    /// `signer`'s key id.
    pub(crate) fn key_of(signer: Signer) -> KeyId {
        KeyId::of_public(&pair_of(signer).pk)
    }

    /// **A statement of the release `tag`** listing one file — `platform`'s,
    /// called `name`, holding `bytes` — whose archive's members are `members`;
    /// `min_from 0.4.6`, retiring nothing, by the live test key.
    pub(crate) fn statement_of(
        tag: &str,
        (platform, name, bytes): (&str, &str, &[u8]),
        members: Vec<Member>,
    ) -> Statement {
        let version = tag
            .strip_prefix('v')
            .map(|rest| rest.strip_suffix("-preview").unwrap_or(rest))
            .expect("a release tag")
            .to_owned();
        Statement {
            product: "folio".to_owned(),
            version,
            tag: tag.to_owned(),
            date: "2026-11-25".to_owned(),
            key: key_of(Signer::Live),
            min_from: "0.4.6".to_owned(),
            authenticode: false,
            retires: Vec::new(),
            assets: vec![StatementAsset {
                sha256: hex(&sha256(bytes)),
                size: bytes.len() as u64,
                platform: platform.to_owned(),
                name: name.to_owned(),
            }],
            members_of: platform.to_owned(),
            members,
        }
    }

    /// **`statement`'s text and its signature by `signer`**, the text naming
    /// `signer`'s key: the two files a release publishes.
    pub(crate) fn signed_statement(statement: &Statement, signer: Signer) -> (Vec<u8>, Vec<u8>) {
        let mut named = statement.clone();
        named.key = key_of(signer);
        let text = named.statement_text().into_bytes();
        let signature = pair_of(signer).sk.sign(&text, None).to_vec();
        (text, signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update_txn::{Adapter, Body, Inventories, Layout, Phase, beyond_inputs};

    /// RED (E1; role #16, the job owner, site J11 `at_launch`, and the press,
    /// both Prepares' `journal_there`) — **the job owner leaves a journal this
    /// build cannot read whole to the build that wrote it, and the press says
    /// so**: at a launch it is `AtLaunch::Left` (the offer still shows) and
    /// the journal is byte for byte as it was, the lock let go; pressing
    /// Update then answers `Stop::Newer`, whose card says that a newer
    /// Folio's update is not finished — not *Another update is in
    /// progress.*, which a journal this build reads still answers.
    ///
    /// MUTATION: in `journal_there`, answer `Stop::Busy` whatever the
    /// journal is (the pre-E1 press).
    #[test]
    fn the_job_owner_leaves_what_it_cannot_read_whole_and_the_press_says_why() {
        let root = bt_testpath::temp_path("bt-update-prepare-beyond");
        let _ = std::fs::remove_dir_all(&root);
        let home = Home::at(root.join("home"));
        let txn = TxnId::new([0x4b; 16]);
        std::fs::create_dir_all(home.transaction(txn)).unwrap();
        let known = crate::update_txn::Journal {
            txn,
            rescue: "rescue".to_owned(),
            body: Body {
                phase: Phase::Prepared {
                    deferred_launches: 0,
                    restart_missed: false,
                },
                layout: Layout::Members(Inventories {
                    old_shipped: vec!["folio.exe".to_owned()],
                    old_present: Vec::new(),
                    new: Vec::new(),
                }),
                adapter: Adapter::Ours,
                marker: None,
                unkept: None,
            },
        }
        .encode();
        for (what, bytes) in beyond_inputs(&known) {
            install_txn::durable_write(&home.journal(), &bytes).unwrap();
            let at = home.clone();
            let landed = bt_platform::spawn_at_priority(
                "bt-update-prepare-test",
                bt_platform::ThreadPriority::BelowNormal,
                move |worker| {
                    matches!(
                        at_launch(worker, &at, crate::update_txn::PreviousRun::Orderly),
                        Ok(AtLaunch::Left)
                    )
                },
            )
            .unwrap()
            .join()
            .unwrap();
            assert!(landed, "{what}: left to the build that wrote it");
            assert_eq!(std::fs::read(home.journal()).unwrap(), bytes, "{what}");
            assert!(
                install_txn::try_hold(&home.lock(), Hold::Exclusive)
                    .unwrap()
                    .is_some(),
                "{what}: the lock is let go"
            );
            assert_eq!(journal_there(&home), Stop::Newer, "{what}");
        }
        let paint = crate::update_card::paint(&crate::update_job::State::Failed(
            None,
            crate::update_job::Failure::Stopped(Stop::Newer),
        ))
        .expect("a failed job has a card");
        assert_eq!(
            paint.heading.as_deref(),
            Some("An update by a newer Folio is not finished.")
        );
        assert_eq!(
            paint.detail.as_deref(),
            Some("It finishes when you next sign in.")
        );

        install_txn::durable_write(&home.journal(), &known).unwrap();
        assert_eq!(
            journal_there(&home),
            Stop::Busy,
            "a journal this build reads"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (0.4.8 E3, #13) — **a staged update survives a power cut and a
    /// plain relaunch; the launch after a restart that did not happen offers
    /// it again and says so; only a deliberate deferral counts**: over a set
    /// one deliberate *Later* has already counted, a launch after a run that
    /// did not end orderly keeps it, writing nothing; a launch after an
    /// orderly quit counts and discards it, as ever; a set the road put back
    /// (`restart_missed`) is kept, its mark spent and nothing counted, and
    /// [`settle_at_launch`] resumes it as the card that says the restart did
    /// not happen.
    ///
    /// MUTATION: in `at_launch`, record `LaunchedWithoutResume` for every
    /// `Prepared` launch (the pre-E3 count: the power cut's relaunch discards
    /// the staged update).
    #[test]
    fn a_power_cut_relaunch_keeps_the_staged_update_and_a_missed_restart_says_so() {
        let root = bt_testpath::temp_path("bt-update-prepare-停电");
        let _ = std::fs::remove_dir_all(&root);
        let home = Home::at(root.join("更新 home"));
        let txn = TxnId::new([0x4c; 16]);
        let staged = |deferred_launches, restart_missed| {
            std::fs::create_dir_all(home.transaction(txn)).unwrap();
            let bytes = crate::update_txn::Journal {
                txn,
                rescue: "rescue".to_owned(),
                body: Body {
                    phase: Phase::Prepared {
                        deferred_launches,
                        restart_missed,
                    },
                    layout: Layout::Members(Inventories {
                        old_shipped: vec!["folio.exe".to_owned()],
                        old_present: Vec::new(),
                        new: Vec::new(),
                    }),
                    adapter: Adapter::Ours,
                    marker: None,
                    unkept: None,
                },
            }
            .encode();
            install_txn::durable_write(&home.journal(), &bytes).unwrap();
            bytes
        };
        let launch = |previous: PreviousRun| {
            let at = home.clone();
            bt_platform::spawn_at_priority(
                "bt-update-prepare-test",
                bt_platform::ThreadPriority::BelowNormal,
                move |worker| match at_launch(worker, &at, previous) {
                    Ok(AtLaunch::Kept(staged)) => format!("kept {:?}", staged.journal.body.phase),
                    Ok(AtLaunch::RestartMissed(staged)) => {
                        format!("missed {:?}", staged.journal.body.phase)
                    }
                    Ok(AtLaunch::Discarded) => "discarded".to_owned(),
                    Ok(_) => "something else".to_owned(),
                    Err(failure) => failure,
                },
            )
            .unwrap()
            .join()
            .unwrap()
        };

        let once = staged(1, false);
        assert_eq!(
            launch(PreviousRun::Unfinished),
            "kept Prepared { deferred_launches: 1, restart_missed: false }",
            "a power cut's relaunch keeps the staged update"
        );
        assert_eq!(
            std::fs::read(home.journal()).unwrap(),
            once,
            "nothing is written"
        );
        assert_eq!(
            launch(PreviousRun::Orderly),
            "discarded",
            "a second deliberate deferral discards, as ever"
        );
        assert!(!home.journal().exists());

        staged(1, true);
        assert_eq!(
            launch(PreviousRun::Orderly),
            "missed Prepared { deferred_launches: 1, restart_missed: false }",
            "offered again, nothing counted"
        );
        assert_eq!(
            crate::update_txn::Journal::parse(&std::fs::read(home.journal()).unwrap())
                .map(|journal| journal.body.phase),
            Ok(Phase::Prepared {
                deferred_launches: 1,
                restart_missed: false
            }),
            "the mark is spent"
        );

        staged(0, true);
        let at = home.clone();
        let landed = bt_platform::spawn_at_priority(
            "bt-update-prepare-test",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| match settle_at_launch(
                worker,
                &at,
                true,
                Box::new(|_, staged, _| Ok((staged, "0.4.9".to_owned(), SignatureNote::Nothing))),
                Some(Channel::Ours),
                HostPlatform::Windows,
                PreviousRun::Unfinished,
            ) {
                Landed::Resumed {
                    offer,
                    restart_missed,
                    ..
                } => Some((offer.tag().to_owned(), restart_missed)),
                _ => None,
            },
        )
        .unwrap()
        .join()
        .unwrap();
        assert_eq!(landed, Some(("v0.4.9".to_owned(), true)));
        let _ = std::fs::remove_dir_all(&root);
    }
    // ── the release statement (0.4.10 plan row 102, part 1b) ─────────────────

    use bt_winres::release_statement::{RELEASE_KEYS, Statement, TEST_KEYS, WINDOWS_X64};
    use test_statements::{Signer, key_of, pair_of, signed_statement, statement_of};

    /// The archive name every statement here lists.
    const ARCHIVE: &str = "folio-0.4.7-windows-x64.zip";

    /// **A release, as a stand-in transport**: each file it serves, by name,
    /// and every name it was asked for, in order.
    struct Served {
        files: Vec<(String, Vec<u8>)>,
        asked: std::sync::Mutex<Vec<String>>,
    }

    impl Served {
        /// `statement` signed by `signer`, and `asset` under the archive's name.
        fn of(statement: &Statement, signer: Signer, asset: &[u8]) -> Self {
            let (text, signature) = signed_statement(statement, signer);
            Self {
                files: vec![
                    (STATEMENT_FILE.to_owned(), text),
                    (SIGNATURE_FILE.to_owned(), signature),
                    (ARCHIVE.to_owned(), asset.to_vec()),
                ],
                asked: std::sync::Mutex::default(),
            }
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl Transport for Served {
        fn fetch(
            &self,
            request: &crate::update_job::Request,
            into: &Path,
            _: &Fetching,
        ) -> Result<PathBuf, String> {
            self.asked.lock().unwrap().push(request.file_name.clone());
            let (name, bytes) = self
                .files
                .iter()
                .find(|(name, _)| *name == request.file_name)
                .ok_or_else(|| "the stand-in server has no such file".to_owned())?;
            let target = into.join(name);
            std::fs::write(&target, bytes).map_err(|error| error.to_string())?;
            Ok(target)
        }
    }

    /// **A folder of the test's own**: `download\` and the data folder whose
    /// `release-keys.json` the test keys are remembered against.
    struct Fold {
        root: PathBuf,
    }

    impl Fold {
        fn new(tag: &str) -> Self {
            let root = bt_testpath::temp_path(&format!("bt-update-vouch-{tag}"));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("download")).unwrap();
            std::fs::create_dir_all(root.join("data")).unwrap();
            Self { root }
        }

        fn download(&self) -> PathBuf {
            self.root.join("download")
        }

        fn memory(&self) -> PathBuf {
            self.root.join("data").join(RELEASE_KEYS_FILE_NAME)
        }

        fn vouching(&self) -> Vouching {
            Vouching::under_test_keys(self.memory())
        }
    }

    impl Drop for Fold {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// The offer every test here presses: `v0.4.7`, for Windows.
    fn the_offer() -> Offer {
        Offer::mint(TxnId::new([0x5a; 16]), "v0.4.7", HostPlatform::Windows).unwrap()
    }

    /// A statement of `v0.4.7` listing `asset` as the Windows archive.
    fn statement_for(asset: &[u8]) -> Statement {
        statement_of("v0.4.7", (WINDOWS_X64, ARCHIVE, asset), Vec::new())
    }

    /// [`vouched`], then [`fetch_vouched_asset`], over `served`, for the
    /// offer of `v0.4.7`.
    fn through(fold: &Fold, served: &Served) -> Result<PathBuf, Stop> {
        through_for(fold, served, &the_offer())
    }

    /// [`through`], for `offer`.
    fn through_for(fold: &Fold, served: &Served, offer: &Offer) -> Result<PathBuf, Stop> {
        let post = Poster::of_a_test(offer.txn());
        let vouching = fold.vouching();
        let (statement, _) = vouched(served, offer, &post, &fold.download(), &vouching)?;
        fetch_vouched_asset(served, offer, &post, &fold.download(), &statement)
    }

    /// PIN (0.4.10 plan row 102, part 1b) — **the test seeds are the test
    /// keys' private halves, and the foreign seed is no key a build embeds**:
    /// every signature a test makes is the one its claim says.
    #[test]
    fn the_test_seeds_are_the_test_keys() {
        assert_eq!(*pair_of(Signer::Live).pk, TEST_KEYS[0].public);
        assert_eq!(*pair_of(Signer::Spare).pk, TEST_KEYS[1].public);
        let foreign = *pair_of(Signer::Foreign).pk;
        assert!(
            TEST_KEYS
                .iter()
                .chain(RELEASE_KEYS.iter())
                .all(|key| key.public != foreign)
        );
    }

    /// RED (0.4.10 plan row 102, part 1b) — **a statement the live key signed
    /// admits the file it lists, fetched by the name it gives**: the
    /// statement, its signature, then that file — and nothing is remembered
    /// when nothing is retired.
    ///
    /// MUTATION: fetch the archive by a composed name in `fetch_vouched_asset`
    /// (`plant-102-1b-composed-name.py`).
    #[test]
    fn a_vouched_statement_admits_the_file_it_names() {
        let fold = Fold::new("admits");
        let asset = b"the archive".to_vec();
        let mut statement = statement_for(&asset);
        statement.assets[0].name = "folio-0.4.7-windows-x64-unsigned.zip".to_owned();
        let mut served = Served::of(&statement, Signer::Live, &asset);
        served.files[2].0.clone_from(&statement.assets[0].name);
        let file = through(&fold, &served).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), asset);
        assert_eq!(
            served.asked(),
            [
                STATEMENT_FILE,
                SIGNATURE_FILE,
                "folio-0.4.7-windows-x64-unsigned.zip"
            ]
        );
        assert!(!fold.memory().exists(), "nothing retired, nothing written");
    }

    /// RED (0.4.10 plan row 102, part 1b) — **a statement signed by a key this
    /// build does not embed is refused before the large download**:
    /// `StatementRefused`, and the archive is never asked for.
    ///
    /// MUTATION: report a refused statement as a failed download in
    /// `Vouching::verified` (`plant-102-1b-unverified-as-download.py`).
    #[test]
    fn a_statement_by_a_key_this_build_does_not_embed_is_refused() {
        let fold = Fold::new("foreign");
        let asset = b"the archive".to_vec();
        let served = Served::of(&statement_for(&asset), Signer::Foreign, &asset);
        assert_eq!(through(&fold, &served), Err(Stop::StatementRefused));
        assert_eq!(served.asked(), [STATEMENT_FILE, SIGNATURE_FILE]);
    }

    /// RED (0.4.10 plan row 102, part 1b) — **a release whose `min_from` is
    /// newer than this build is too old to update to, before the large
    /// download**: `TooOld`, the archive never asked for.
    ///
    /// MUTATION: drop the `TooOld` arm of `statement_refused`
    /// (`plant-102-1b-too-old-unnamed.py`).
    #[test]
    fn a_statement_that_needs_a_newer_build_is_too_old_before_the_download() {
        let fold = Fold::new("too-old");
        let asset = b"the archive".to_vec();
        let mut statement = statement_of("v9.9.9", (WINDOWS_X64, ARCHIVE, &asset), Vec::new());
        statement.min_from = "9.9.9".to_owned();
        let served = Served::of(&statement, Signer::Live, &asset);
        let offer = Offer::mint(TxnId::new([0x5b; 16]), "v9.9.9", HostPlatform::Windows).unwrap();
        assert_eq!(through_for(&fold, &served, &offer), Err(Stop::TooOld));
        assert_eq!(served.asked(), [STATEMENT_FILE, SIGNATURE_FILE]);
    }

    /// RED (0.4.10 plan row 102, part 1b) — **a statement for another release
    /// than the offer's is refused**: the next release's statement, served
    /// under this offer's tag, vouches for nothing here.
    ///
    /// MUTATION: skip `vouches_for` in `vouched`
    /// (`plant-102-1b-any-release.py`).
    #[test]
    fn a_statement_for_another_release_is_refused() {
        let fold = Fold::new("another");
        let asset = b"the archive".to_vec();
        let statement = statement_of("v0.4.8", (WINDOWS_X64, ARCHIVE, &asset), Vec::new());
        let served = Served::of(&statement, Signer::Live, &asset);
        assert_eq!(through(&fold, &served), Err(Stop::StatementRefused));
        assert_eq!(served.asked(), [STATEMENT_FILE, SIGNATURE_FILE]);
    }

    /// RED (0.4.10 plan row 102, part 1b) — **a file that is not the one the
    /// statement lists is refused as `Sums`**: other bytes of the same length.
    ///
    /// MUTATION: skip `asset_matches` in `fetch_vouched_asset`
    /// (`plant-102-1b-asset-unchecked.py`).
    #[test]
    fn a_file_the_statement_does_not_list_is_refused() {
        let fold = Fold::new("sums");
        let statement = statement_for(b"the archive");
        let served = Served::of(&statement, Signer::Live, b"an archive!");
        assert_eq!(through(&fold, &served), Err(Stop::Sums));
    }

    /// RED (0.4.10 plan row 102, part 1b) — **the retire lines are remembered
    /// after a verified statement**: the spare key's statement retiring the
    /// live key leaves `release-keys.json` listing it.
    ///
    /// MUTATION: return before the write in `Vouching::remember`
    /// (`plant-102-1b-retire-forgotten.py`).
    #[test]
    fn the_retire_lines_are_remembered_after_a_verified_statement() {
        let fold = Fold::new("retires");
        let asset = b"the archive".to_vec();
        let mut statement = statement_for(&asset);
        statement.retires = vec![key_of(Signer::Live)];
        through(&fold, &Served::of(&statement, Signer::Spare, &asset)).unwrap();
        let (remembered, _) = bt_persist::read_release_keys(&fold.memory());
        assert_eq!(remembered.retired, [key_of(Signer::Live).to_string()]);
    }

    /// RED (0.4.10 plan row 102, part 1b) — **a key remembered as retired is
    /// refused**: once `release-keys.json` lists the live key, its statement
    /// is `StatementRefused`, and the spare's still passes.
    ///
    /// MUTATION: verify with no retired ids in `Vouching::verified`
    /// (`plant-102-1b-retired-ignored.py`).
    #[test]
    fn a_remembered_retired_key_is_refused() {
        let fold = Fold::new("retired");
        let mut keys = bt_persist::ReleaseKeysV1::default();
        keys.remember_retired([key_of(Signer::Live).to_string()]);
        bt_persist::write_release_keys_atomic(&fold.memory(), &keys).unwrap();
        let asset = b"the archive".to_vec();
        let statement = statement_for(&asset);
        assert_eq!(
            through(&fold, &Served::of(&statement, Signer::Live, &asset)),
            Err(Stop::StatementRefused)
        );
        assert!(through(&fold, &Served::of(&statement, Signer::Spare, &asset)).is_ok());
    }

    /// RED (0.4.10 plan row 102, part 1b) — **a newer build's
    /// `release-keys.json` is never written over**: a retire this build reads
    /// leaves the file byte for byte as it was, and what it retired still
    /// holds.
    ///
    /// MUTATION: write whatever the read reported in `Vouching::remember`
    /// (`plant-102-1b-newer-overwritten.py`).
    #[test]
    fn a_newer_release_keys_file_is_never_written_over() {
        let fold = Fold::new("newer");
        let newer = format!(
            "{{\"schema_version\": 2, \"retired\": [\"{}\"], \"trusted\": []}}",
            key_of(Signer::Foreign)
        );
        std::fs::write(fold.memory(), &newer).unwrap();
        let asset = b"the archive".to_vec();
        let mut statement = statement_for(&asset);
        statement.retires = vec![key_of(Signer::Live)];
        through(&fold, &Served::of(&statement, Signer::Spare, &asset)).unwrap();
        assert_eq!(std::fs::read_to_string(fold.memory()).unwrap(), newer);
    }

    /// RED (0.4.10 plan row 102, part 1b) — **a statement file longer than any
    /// statement is refused on its size**, read no further than one byte past
    /// the bound (the 1b+2 review's N11).
    ///
    /// MUTATION: drop the size check in `statement_file_bytes`
    /// (`plant-102-1b-unbounded.py`).
    #[test]
    fn an_oversized_statement_file_is_refused_on_its_size() {
        let fold = Fold::new("oversized");
        let statement = fold.download().join(STATEMENT_FILE);
        std::fs::write(&statement, vec![b'a'; STATEMENT_MAX_BYTES + 1]).unwrap();
        assert_eq!(
            statement_file_bytes(&statement, STATEMENT_MAX_BYTES as u64),
            Err(Stop::StatementRefused)
        );
    }
}
