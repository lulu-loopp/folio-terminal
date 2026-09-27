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
//! * **the checksum**: the document's line for the offer's own asset
//!   ([`sum_for`]) and the downloaded file held to it ([`matches_its_sum`]),
//!   hashed a bounded chunk at a time ([`digest_of`]);
//! * **giving a transaction up**: `Abandoned` recorded durably, then the
//!   transaction cleared away in the protocol's order ([`abandon`],
//!   [`discard`], [`clear`] — an image mounted under the transaction is
//!   detached first, which only macOS can have);
//! * **the job owner's pass at a later launch** ([`at_launch`]): an
//!   `Allocated` transaction a dead Prepare left is swept (W1, M1), a
//!   `Prepared` one is counted (`LaunchedWithoutResume`) and discarded at its
//!   second launch (W2, M2). It reads the phase alone, so it is one function
//!   for both layouts. What *revalidating* a staged transaction means differs
//!   (a bundle's identity, a member set's digests), and each driver has its
//!   own.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_txn::{self, Hold};

use crate::update_handoff::Staged;
use crate::update_job::{Bytes, Fetching, Poster, Step, Stop};
use crate::update_txn::{
    Action, Actor, Asker, Disk, Effect, Event, Home, Journal, Located, PhaseKind, TxnId, decide,
    may,
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

/// **The downloaded `asset` is the one the checksum document `sums` names.**
///
/// # Errors
/// [`Stop::Sums`]: the document cannot be read or names no single sum for
/// `asset`, or the file hashes to another.
pub(crate) fn matches_its_sum(file: &Path, sums: &Path, asset: &str) -> Result<(), Stop> {
    let document = file_reads::read_to_string(Lane::Update, sums).map_err(|_| Stop::Sums)?;
    let expected = sum_for(&document, asset).ok_or(Stop::Sums)?;
    let (found, _) = digest_of(file).map_err(|_| Stop::Sums)?;
    if found == expected {
        Ok(())
    } else {
        Err(Stop::Sums)
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
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the launch pass and the resume are wired with the staged card of a later launch (U-23, U-28)"
    )
)]
pub(crate) enum AtLaunch {
    /// No journal: no transaction.
    Nothing,
    /// Another holder has the transaction lock; nothing was touched.
    Busy,
    /// A dead Prepare's `Allocated` transaction was swept (W1, M1).
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

/// **The job owner's pass at a launch** ((b).2's W1–W2 and M1–M2): the lock,
/// one read of the journal, and `update_txn::decide` as the job owner.
///
/// # Errors
/// A step that failed, as a sentence; whatever it left is the next launch's.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the launch pass and the resume are wired with the staged card of a later launch (U-23, U-28)"
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
