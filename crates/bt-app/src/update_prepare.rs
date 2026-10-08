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
//! * **the worker's last word** ([`finish`]): one report, posted once the
//!   worker has let go of the transaction or handed it to the job — a Cancel
//!   included, so its lock is known free when its report is read;
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
//!   own ([`Resumer`]). The whole pass as the update job runs it at a launch
//!   — the phase's answer, then the resume of a counted set with the offer
//!   rebuilt from its own version — is [`settle_at_launch`] (U-33).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_txn::{self, Hold};

use bt_platform::HostPlatform;

use crate::install_channel::Channel;
use crate::update_handoff::Staged;
use crate::update_job::{Bytes, Fetching, Landed, Offer, Poster, Step, Stop};
use crate::update_txn::{
    Action, Actor, Asker, Disk, Effect, Event, Home, Journal, Located, PhaseKind, Role, TxnId,
    decide, may,
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

// ── the worker's last word ──────────────────────────────────────────────────

/// **End a Prepare's worker with its one last report**, posted only once the
/// worker holds nothing of the transaction: `Verified` hands the staged
/// transaction — lock and all — to the job; every other end has already let
/// the lock go (`abandon`, then the lock dropped, in `prepare_on`), and says
/// [`Step::Stopped`] with why. A Cancel ends the same way, with
/// [`Stop::Cancelled`]: the job has moved on, so the report is stale to it and
/// revives nothing, but whoever reads it knows the transaction is gone and its
/// lock free — the journal's removal is not that sign, since the lock is let
/// go only after it (the journal goes last, under the lock).
pub(crate) fn finish(worker: &WorkerCtx, post: &Poster, prepared: Result<Staged, Stop>) {
    match prepared {
        Ok(staged) => {
            // Cancelled between the last look and the report: the job has
            // moved on, so the transaction is given up.
            if let Err(staged) = post.verified(staged) {
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

/// **Why a press finds a journal already there** (both Prepares, step 2): the
/// installation is another transaction's ([`Stop::Busy`]), or that journal is
/// one this build cannot read whole — another Folio's update is not finished,
/// and the build that wrote it finishes it ([`Stop::Newer`], 0.4.8 E1,
/// `update_txn::Role::JobOwner`). A journal that cannot be read at all is
/// [`Stop::Busy`] as before.
pub(crate) fn journal_there(home: &Home) -> Stop {
    match file_reads::read(Lane::UpdateJournal, home.journal()) {
        Ok(bytes) => match Role::JobOwner.sight(&bytes) {
            crate::update_txn::Sight::Known(_) => Stop::Busy,
            crate::update_txn::Sight::Header { .. }
            | crate::update_txn::Sight::Envelope { .. }
            | crate::update_txn::Sight::Unreadable(_) => Stop::Newer,
        },
        Err(_) => Stop::Busy,
    }
}

/// **The job owner's pass at a launch** ((b).2's W1–W2 and M1–M2): the lock,
/// one read of the journal, and `update_txn::decide` as the job owner.
///
/// # Errors
/// A step that failed, as a sentence; whatever it left is the next launch's.
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
    // A journal this build cannot read whole is left to the build that wrote
    // it (E1, `update_txn::Role::JobOwner`): the offer still shows, and the
    // press says so (`update_job::Stop::Newer`).
    let journal = match Role::JobOwner.sight(&bytes) {
        crate::update_txn::Sight::Known(journal) => journal,
        crate::update_txn::Sight::Header { .. }
        | crate::update_txn::Sight::Envelope { .. }
        | crate::update_txn::Sight::Unreadable(_) => return Ok(AtLaunch::Left),
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

/// **How this copy revalidates a staged transaction before O resumes it**
/// (F-17), on the job's worker, given how this copy was installed: the staged
/// transaction back with **the version its set installs**, read from the set
/// itself — the staged `folio.exe`'s own release manifest on Windows, the
/// staged bundle's recorded version on macOS — never from a download. A
/// failure has already discarded the transaction (`Discarded`, then
/// [`clear`]) and says why. `update_job::resumer_for_this_copy` is the
/// product's; a test holds the real revalidation to its own trust root.
pub(crate) type Resumer =
    Box<dyn FnOnce(&WorkerCtx, Staged, Option<Channel>) -> Result<(Staged, String), Stop> + Send>;

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
/// on the job's worker: [`at_launch`], then —
///
/// * `Swept`, `Discarded`, `Nothing`, `Left`, or a step that failed →
///   [`Landed::Ordinary`]: the launch offers as usual;
/// * `Busy` → [`Landed::Busy`]: another holder has the transaction, and this
///   launch offers nothing;
/// * `Counted` with `offers` off → the count is recorded and the lock let go
///   ([`Landed::Ordinary`]): a build whose job never offers shows no card, and
///   the second launch discards;
/// * `Counted` with `offers` on → `resume` revalidates the staged set for
///   `channel` and answers its version; the offer is minted again from that
///   version under the transaction's own identity, for `platform`'s files —
///   [`Landed::Resumed`], the verified card of this launch. A revalidation
///   that fails has discarded the set ([`Landed::Ordinary`]).
///
/// What it did is said in one `diagnostics.log` line, with no path.
pub(crate) fn settle_at_launch(
    worker: &WorkerCtx,
    home: &Home,
    offers: bool,
    resume: Resumer,
    channel: Option<Channel>,
    platform: HostPlatform,
) -> Landed {
    let say = |line: &str| crate::diagnostics::note(&format!("Folio: update job — {line}"));
    match at_launch(worker, home) {
        Ok(AtLaunch::Counted(staged)) if !offers => {
            drop(staged);
            say("a verified update is kept for a later launch; offers are off in this build");
            Landed::Ordinary
        }
        Ok(AtLaunch::Counted(staged)) => {
            let txn = staged.journal.txn;
            match resume(worker, *staged, channel) {
                Ok((staged, version)) => match Offer::mint(txn, &format!("v{version}"), platform) {
                    Some(offer) => Landed::Resumed(offer, Box::new(staged)),
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
        Ok(AtLaunch::Busy) => Landed::Busy,
        Ok(AtLaunch::Swept) => {
            say("an unfinished download of an earlier launch is cleared");
            Landed::Ordinary
        }
        Ok(AtLaunch::Discarded) => {
            say("the update prepared at an earlier launch is discarded at its second launch");
            Landed::Ordinary
        }
        Ok(AtLaunch::Nothing | AtLaunch::Left) => Landed::Ordinary,
        Err(failure) => {
            say(&format!(
                "an earlier launch's update is kept for the next launch: {failure}"
            ));
            Landed::Ordinary
        }
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
                },
                layout: Layout::Members(Inventories {
                    old_shipped: vec!["folio.exe".to_owned()],
                    old_present: Vec::new(),
                    new: Vec::new(),
                }),
                adapter: Adapter::Ours,
            },
        }
        .encode();
        for (what, bytes) in beyond_inputs(&known) {
            install_txn::durable_write(&home.journal(), &bytes).unwrap();
            let at = home.clone();
            let landed = bt_platform::spawn_at_priority(
                "bt-update-prepare-test",
                bt_platform::ThreadPriority::BelowNormal,
                move |worker| matches!(at_launch(worker, &at), Ok(AtLaunch::Left)),
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
}
