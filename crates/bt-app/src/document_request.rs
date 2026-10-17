//! **The storage worker's one request protocol for a document**, and the store
//! half that every hand-editable document shares
//! (`docs/plans/design/settings-reread-2026-10-11.md` §3.2 as revised by
//! (b)1–(b)4, (b)6, (b)9, (b)10, (b)16, (c)1, (c)3 and (d)1; ticket
//! T-STORAGE-WORKER-REQUEST).
//!
//! # The request
//!
//! One worker, `persist::SessionWriter`'s `session-writer`, takes two kinds of
//! document request on its one queue, in order:
//!
//! - [`DocumentJob`]: put `bytes` under `which`, **only if** the file still
//!   holds `expected` (`None` means *no file*). The worker reads the file while
//!   it stands still (`bt_persist::read_stable`), stages and flushes a sibling,
//!   and exchanges the two through `bt_platform::replace_keeping_displaced`
//!   (the first write, through `bt_platform::rename_without_replacing_durably`),
//!   then reads what the exchange displaced. It answers [`Outcome::Landed`],
//!   [`Outcome::Moved`] (the precondition saw another file; nothing written),
//!   [`Outcome::MovedDuringRead`], [`Outcome::Displaced`] (a hand edit crossed
//!   the exchange: Folio's bytes are on disk and the person's are kept beside
//!   them), [`Outcome::Conflict`] (the `Restore` policy put the person's bytes
//!   back) or [`Outcome::Refused`]. Every answer echoes `generation`, `version`
//!   and `asker`.
//! - [`ReadJob`]: read `which` while it stands still, and hand the bytes to the
//!   asker's own answer, which parses them on the worker.
//!
//! **Two policies, chosen per request** ((c)3): `Merge` keeps a displaced file
//! for the asker's merge; `Restore` exchanges again, conditional on the target
//! still holding what was written, so the person's file is back under its name,
//! and answers `Conflict`. **A link** is refused unless the request's
//! `link_policy` is `PlainWriter`, which writes it the plain way; a volume that
//! cannot exchange is refused unless the request's `no_exchange` is
//! `PlainWriter` — the preview's save (T-PREVIEW-NO-EXCHANGE-RULING, owner
//! ruling 2026-10-15); every store refuses ((b)1). The plain road reads the file
//! again just before it writes and writes only while it still holds `expected`,
//! so a writer that crossed is not written over. **Per-file order** holds
//! because there is one queue: every request naming one path, whichever store or
//! buffer queued it, is served in the order it was queued.
//!
//! The worker answers on the request's own channel and only then wakes the loop
//! (`AppEvent::DocumentsAnswered`, `persist::wake_documents`): the answer first,
//! the wake after.
//!
//! # The store half: [`DocumentState`]
//!
//! Each of `settings.json`, `keybindings.json`, `profiles.json` and `pins.json`
//! holds one. It keeps:
//!
//! - the **revision** (`persist::Revision`): the bytes Folio last read from the
//!   file or placed there, and their document;
//! - the **projection**: the document readers read, which presses produce;
//! - **`desired`**, with its **version**: the document Folio wants on disk. A
//!   press applies to it exactly the units it changed in the projection
//!   ([`StoredDocument::apply_press`]), so a value the file carries and a door
//!   left out is never written back as a change ((b)6). The version rises at
//!   every change to `desired` ((d)1);
//! - the requests on the worker, in the order they were sent.
//!
//! **Owed** ⇔ this process writes the file and `desired` differs from the
//! revision's document. One write is in flight at a time; a change while one is
//! is sent when its answer has been processed (the pending slot is `desired`
//! itself, so the newest change wins). **Every answer is processed, in the order
//! the worker served it**: revision transitions, kept files and diagnostics for
//! every one; a generation only keeps a stale answer's visible effect from
//! landing after a newer one's ((b)4).
//!
//! **One policy for all four stores.** Every store sends `Merge` requests
//! (T-STORE-MERGE-SETTINGS, T-STORE-MERGE-KEYBINDINGS,
//! T-STORE-MERGE-PROFILES-PINS): a displaced file is kept for its merge. And a
//! re-read that found the file changed stores it for the merge.
//!
//! **The merge's seam** ([`DocumentState::merge_stored`]): every file stored
//! for the merge, in the order it was stored, is merged by the document's own
//! merge against its base and folded into `desired` ([`DocumentState::fold`],
//! (c)1 and (d)1). Nothing is merged on the worker, and nothing here knows a
//! document's units: the merge is handed in.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::SystemTime;

use bt_persist::{
    BindingOverrideV1, FallbackReason, KeybindingsV1, PinsV1, ProfilesV1, SettingsV1, StableRead,
    Staging,
};
use bt_platform::admission::WorkerCtx;

use crate::persist::{DocumentWrites, OnTheWorker, Reply, Revision, StorageLane};
use crate::quit::{CopyStanding, Standing};
use crate::settings_bundle::{LineRead, read_lines};

/// **Which semantic policy a request asks for** ((c)3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The asker merges: a displaced file stays on disk, named in the answer,
    /// for its merge.
    Merge,
    /// The asker has no merge: a displaced file is put back under its name and
    /// the answer is a conflict. No store sends it — all four merge — and it is
    /// the preview's save's policy on this worker (design note (c)3, J5 S-6a;
    /// `preview::PreviewBuffer::request_save`).
    Restore,
}

/// **What a request does with a target that is a link** ((c)3 item 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkPolicy {
    /// Refuse: nothing is written ((b)2, owner ruling Q5).
    Refuse,
    /// Write the link's name the plain way — the preview's save, until the
    /// owner's ruling on links (J5 Q5(b), `Refuse`) is built for it. Honoured
    /// for a link only: a volume that cannot exchange is refused whatever the
    /// policy.
    PlainWriter,
}

/// **What a request does on a volume that cannot exchange two files in one
/// step** ((b)1; hand-off item 6). The four stores refuse. The preview's save
/// writes plainly (owner ruling 2026-10-15, T-PREVIEW-NO-EXCHANGE-RULING); its
/// one switch is `preview::ON_A_VOLUME_THAT_CANNOT_EXCHANGE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoExchange {
    /// Nothing is written; the answer is `Refused(ExchangeUnavailable)`.
    Refuse,
    /// Write the file the plain way, still only while it holds `expected`, and
    /// name the drive on the landing ([`Outcome::Landed`]'s `ordinary_on`).
    PlainWriter,
}

/// **A drive, as a landing names it**: the identity of the volume the file is
/// on (`bt_platform::volume_identity`), or `None` when the volume would not
/// say. Two files on one drive name the same drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Drive(pub(crate) Option<u64>);

/// **The volume a request's file is on, as the worker meets it**: its exchange,
/// its identity and the plain road's rename. The product's is [`TheVolume`]; a test scripts one that
/// answers as a drive without the exchange does.
pub(crate) trait Volume {
    /// `bt_platform::replace_keeping_displaced`'s contract.
    fn exchange(
        &self,
        worker: &WorkerCtx,
        temp: &Path,
        target: &Path,
    ) -> Result<bt_platform::Displaced, bt_platform::ExchangeRefusal>;
    /// The drive `file` is on.
    fn drive(&self, file: &Path) -> Drive;
    /// The plain road's one step on the file: `staged`, already written and
    /// flushed, takes `target`'s name by one rename
    /// (`bt_persist::commit_staged`). The error in the system's words.
    fn commit(&self, staged: &Path, target: &Path) -> Result<(), String>;
}

/// **The operating system's volume**: the exchange door and the volume's own
/// identity.
pub(crate) struct TheVolume;

impl Volume for TheVolume {
    fn exchange(
        &self,
        worker: &WorkerCtx,
        temp: &Path,
        target: &Path,
    ) -> Result<bt_platform::Displaced, bt_platform::ExchangeRefusal> {
        bt_platform::replace_keeping_displaced(worker, temp, target)
    }

    fn drive(&self, file: &Path) -> Drive {
        Drive(bt_platform::volume_identity(file))
    }

    fn commit(&self, staged: &Path, target: &Path) -> Result<(), String> {
        bt_persist::commit_staged(staged, target).map_err(|error| error.to_string())
    }
}

/// **A file the worker left beside a document, and the version whose landing
/// retires it** ((d)1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeptFile {
    pub(crate) path: PathBuf,
    /// The version of `desired` that holds what the file kept. A landing of any
    /// version at or after it removes the file; nothing else does.
    pub(crate) version: u64,
}

/// **The kept files a landing of version `landed` retires** ((d)1, rule 4):
/// every one whose version is at or before it. `desired` has one line of
/// descent, so every version from `m` on derives from the merge that took the
/// kept file's units in.
pub(crate) fn retired_by(kept: &[KeptFile], landed: u64) -> impl Iterator<Item = &KeptFile> {
    kept.iter().filter(move |file| file.version <= landed)
}

/// **One document on its way to the disk, conditional on what the file holds.**
pub(crate) struct DocumentJob {
    /// The file.
    pub(crate) which: PathBuf,
    pub(crate) generation: u64,
    /// The version of `desired` these bytes are.
    pub(crate) version: u64,
    /// What the file must hold for the write to go ahead; `None` is *no file*.
    pub(crate) expected: Option<Arc<[u8]>>,
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) mode: Mode,
    pub(crate) link_policy: LinkPolicy,
    pub(crate) no_exchange: NoExchange,
    /// Opaque to the worker and echoed on the answer: the preview's body
    /// revision, `0` for a store.
    pub(crate) asker: u64,
    /// The kept files the asker holds obligations for; a durable landing of
    /// `version` removes those [`retired_by`] it.
    pub(crate) kept: Vec<KeptFile>,
    pub(crate) answer: mpsc::Sender<DocumentAnswer>,
}

/// **One copy of a document into the data directory's `recovered` folder** — a
/// quit's recovery phase (design note 2026-10-11, revision (c)2): written
/// create-new, the file flushed and the folder flushed, exactly as a stop that
/// cannot ask keeps a preview edit ([`crate::preview::keep_in_recovered`]), so the
/// next start's sentence about the folder covers it unchanged.
pub(crate) struct RecoveredJob {
    pub(crate) folder: PathBuf,
    /// The document's file name, which the copy's name ends in.
    pub(crate) name: &'static str,
    pub(crate) bytes: Vec<u8>,
    /// The instant the copy's name is stamped with.
    pub(crate) at: SystemTime,
    /// Where the copy is, or why there is none.
    pub(crate) answer: mpsc::Sender<Result<PathBuf, String>>,
}

/// **Serve one [`RecoveredJob`] on the storage worker** and answer it. The job
/// is lent: its road stands until the worker has published its terminal.
pub(crate) fn serve_recovered(worker: &WorkerCtx, job: &RecoveredJob) {
    let _ = worker;
    let kept = crate::preview::keep_in_recovered(&job.folder, job.name, &job.bytes, job.at);
    let _ = job.answer.send(kept);
}

/// **One stable read of a document**: the bytes go to `answer` with the
/// request's generation, on the worker, which parses them there. The answer is
/// called through the job, which the worker lets go only after the request's
/// terminal is published — so a parse that panics leaves its road standing until
/// its `Failed` is there to read.
pub(crate) struct ReadJob {
    pub(crate) which: PathBuf,
    pub(crate) generation: u64,
    pub(crate) answer: Box<dyn Fn(u64, StableRead) + Send>,
}

/// **What became of one [`DocumentJob`].**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentAnswer {
    pub(crate) generation: u64,
    pub(crate) version: u64,
    pub(crate) asker: u64,
    pub(crate) outcome: Outcome,
    /// The exchange happened and a directory flush after it failed: the target
    /// holds the new bytes, which are not known to be on the device
    /// (`ExchangeRefusal::NotDurable`).
    pub(crate) not_durable: Option<String>,
    /// The kept files this landing removed.
    pub(crate) retired: Vec<PathBuf>,
    /// A file the worker left on disk that no rule removes, and why — said in
    /// `stderr`, never read back.
    pub(crate) left: Option<(PathBuf, String)>,
}

/// **The answer's verdict.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The file holds `bytes`. `kept` names the displaced copy of the bytes the
    /// file held before, kept because the exchange is not known to be durable;
    /// a later durable landing retires it. `ordinary_on` names the drive when
    /// the bytes went there by an ordinary write because it cannot exchange two
    /// files in one step (`NoExchange::PlainWriter`): the staged copy took the
    /// name by one rename, so a crash leaves the old file or the new one, never
    /// a part of either; on a drive with no journal (FAT, exFAT) an interruption
    /// at that rename can leave the file only under the copy's name beside it,
    /// or the folder's entry damaged, which the system's disk check can usually
    /// repair; and a writer that wrote the file between the end of the last
    /// check of its bytes and the rename is replaced unseen.
    Landed {
        bytes: Arc<[u8]>,
        kept: Option<PathBuf>,
        ordinary_on: Option<Drive>,
    },
    /// The precondition saw another file — `None`: no file, where one was
    /// expected — and nothing was written.
    Moved { bytes: Option<Arc<[u8]>> },
    /// The file changed while the precondition read it; nothing was written.
    MovedDuringRead,
    /// `Merge`: a hand edit crossed the exchange. The file holds Folio's bytes;
    /// the person's, `bytes`, are under `kept`.
    Displaced { bytes: Arc<[u8]>, kept: PathBuf },
    /// `Restore`: a hand edit crossed the exchange, and its bytes, `theirs`, are
    /// back under the file's name. `kept` names a file left beside it when the
    /// putting back was itself crossed or refused.
    Conflict {
        theirs: Arc<[u8]>,
        kept: Option<PathBuf>,
    },
    /// Nothing was written.
    Refused { cause: Refusal },
}

/// **Why a write was refused.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The file is a symbolic link or has a second name, and the request
    /// refuses links.
    Link,
    /// The volume cannot exchange two files in one step.
    ExchangeUnavailable(String),
    /// The fresh name for the displaced copy was taken; tried again at the next
    /// trigger.
    BackupNameTaken(PathBuf),
    /// The file is there and its bytes could not be read, so it is not written
    /// over.
    Unread(String),
    /// The disk's own error.
    Disk(String),
    /// No worker took the request, or the worker ended before answering it.
    NoWorker(String),
    /// The worker failed on the request — its task panicked — and went on
    /// (T-STORAGE-WORKER-ON-THE-SEAM, R2); the words are
    /// `persist::storage_task_failed`'s.
    Failed(String),
}

impl Refusal {
    /// **Whether this refusal is a fault at its first occurrence** ((b)7): a
    /// link and a volume that cannot exchange do not change by trying again.
    /// Every other refusal counts in the streak.
    pub(crate) fn is_said_at_once(&self) -> bool {
        matches!(self, Self::Link | Self::ExchangeUnavailable(_))
    }

    /// The refusal in words, as `diagnostics.log` and a pane that was not
    /// saved say it.
    pub(crate) fn described(&self) -> String {
        match self {
            Self::Link => "it is a link or has a second name".to_owned(),
            Self::ExchangeUnavailable(error) => {
                format!("its volume cannot exchange two files in one step: {error}")
            }
            Self::BackupNameTaken(path) => {
                format!(
                    "the name for its displaced copy is taken: {}",
                    path.display()
                )
            }
            Self::Unread(error) => format!("it could not be read: {error}"),
            Self::Disk(error) | Self::NoWorker(error) | Self::Failed(error) => error.clone(),
        }
    }
}

/// The moments in a write at which a crossing writer can land — a test's seam
/// into [`serve_document_between`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The precondition has been read.
    Read,
    /// The exchange has happened, before its displaced bytes are read.
    Exchanged,
}

/// **Serve one [`DocumentJob`] on the storage worker** and answer it. The job is
/// lent: its road stands until the worker has published its terminal.
pub(crate) fn serve_document(worker: &WorkerCtx, job: &DocumentJob) {
    serve_document_between(worker, job, &mut |_| {});
}

/// [`serve_document`], with `between` run at each [`Step`].
pub(crate) fn serve_document_between(
    worker: &WorkerCtx,
    job: &DocumentJob,
    between: &mut dyn FnMut(Step),
) {
    serve_document_on(worker, &TheVolume, job, between);
}

/// [`serve_document_between`] on `volume`. The job is lent, as there.
pub(crate) fn serve_document_on(
    worker: &WorkerCtx,
    volume: &dyn Volume,
    job: &DocumentJob,
    between: &mut dyn FnMut(Step),
) {
    let served = conditional_write(worker, volume, job, between);
    let retired = match (&served.outcome, &served.not_durable) {
        (Outcome::Landed { .. }, None) => retire(worker, &job.kept, job.version),
        _ => Vec::new(),
    };
    let _ = job.answer.send(DocumentAnswer {
        generation: job.generation,
        version: job.version,
        asker: job.asker,
        outcome: served.outcome,
        not_durable: served.not_durable,
        retired,
        left: served.left,
    });
}

/// **Serve one [`ReadJob`]**: the stable read, handed to the asker's answer.
pub(crate) fn serve_read(worker: &WorkerCtx, job: &ReadJob) {
    let read = bt_persist::read_stable(worker, &job.which);
    (job.answer)(job.generation, read);
}

/// What one write came to, before it is addressed.
#[derive(Debug)]
pub(crate) struct Served {
    pub(crate) outcome: Outcome,
    pub(crate) not_durable: Option<String>,
    pub(crate) left: Option<(PathBuf, String)>,
}

impl Served {
    fn plainly(outcome: Outcome) -> Self {
        Self {
            outcome,
            not_durable: None,
            left: None,
        }
    }
}

/// What the precondition read decides.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Precondition {
    Holds,
    Moved(Option<Arc<[u8]>>),
    Raced,
    Unread(String),
}

/// **The precondition, decided** (§3.2 step 1; carry-forward 3): `expected:
/// None` is *no file*, and a file whose bytes were not obtained satisfies no
/// precondition at all — Folio never writes over a file it has not read.
pub(crate) fn precondition(read: StableRead, expected: Option<&[u8]>) -> Precondition {
    match (read, expected) {
        (StableRead::Absent, None) => Precondition::Holds,
        (StableRead::Absent, Some(_)) => Precondition::Moved(None),
        (StableRead::Bytes(bytes), Some(expected)) if *bytes == *expected => Precondition::Holds,
        (StableRead::Bytes(bytes), _) => Precondition::Moved(Some(bytes)),
        (StableRead::Unobtained(error), _) => Precondition::Unread(error),
        (StableRead::MovedDuringRead, _) => Precondition::Raced,
    }
}

/// **What a precondition that does not hold answers**: `None` when it holds and
/// the write may go ahead; otherwise the answer, nothing written.
fn unless_it_holds(read: StableRead, expected: Option<&[u8]>) -> Option<Served> {
    match precondition(read, expected) {
        Precondition::Holds => None,
        Precondition::Moved(bytes) => Some(Served::plainly(Outcome::Moved { bytes })),
        Precondition::Raced => Some(Served::plainly(Outcome::MovedDuringRead)),
        Precondition::Unread(error) => Some(Served::plainly(Outcome::Refused {
            cause: Refusal::Unread(error),
        })),
    }
}

/// §3.2's steps 1–4, as revised.
fn conditional_write(
    worker: &WorkerCtx,
    volume: &dyn Volume,
    job: &DocumentJob,
    between: &mut dyn FnMut(Step),
) -> Served {
    let read = bt_persist::read_stable(worker, &job.which);
    between(Step::Read);
    if let Some(answer) = unless_it_holds(read, job.expected.as_deref()) {
        return answer;
    }
    let staging = if job.expected.is_some() {
        Staging::Replacing
    } else {
        Staging::New
    };
    let temp = match bt_persist::stage_beside(&job.which, &job.bytes, staging) {
        Ok(temp) => temp,
        Err(error) => {
            return Served::plainly(Outcome::Refused {
                cause: Refusal::Disk(error.to_string()),
            });
        }
    };
    match &job.expected {
        None => first_write(worker, job, &temp),
        Some(expected) => exchange(worker, volume, job, expected, &temp, between),
    }
}

/// The first file under a name that holds none: a rename that refuses any
/// entry already there, then the directory flushed.
fn first_write(worker: &WorkerCtx, job: &DocumentJob, temp: &Path) -> Served {
    match bt_platform::rename_without_replacing_durably(worker, temp, &job.which) {
        Ok(()) => Served::plainly(Outcome::Landed {
            bytes: Arc::clone(&job.bytes),
            kept: None,
            ordinary_on: None,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(temp);
            Served::plainly(match bt_persist::read_stable(worker, &job.which) {
                StableRead::Bytes(bytes) => Outcome::Moved { bytes: Some(bytes) },
                StableRead::Unobtained(error) => Outcome::Refused {
                    cause: Refusal::Unread(error),
                },
                StableRead::Absent | StableRead::MovedDuringRead => Outcome::MovedDuringRead,
            })
        }
        // The rename took the staged file: what failed was a flush after it.
        Err(error) if !temp.exists() => Served {
            outcome: Outcome::Landed {
                bytes: Arc::clone(&job.bytes),
                kept: None,
                ordinary_on: None,
            },
            not_durable: Some(error.to_string()),
            left: None,
        },
        Err(error) => {
            let _ = std::fs::remove_file(temp);
            Served::plainly(Outcome::Refused {
                cause: Refusal::Disk(error.to_string()),
            })
        }
    }
}

/// The exchange, keeping what it displaced, and what follows each answer.
fn exchange(
    worker: &WorkerCtx,
    volume: &dyn Volume,
    job: &DocumentJob,
    expected: &[u8],
    temp: &Path,
    between: &mut dyn FnMut(Step),
) -> Served {
    use bt_platform::ExchangeRefusal;
    let refused = |cause| {
        let _ = std::fs::remove_file(temp);
        Served::plainly(Outcome::Refused { cause })
    };
    match volume.exchange(worker, temp, &job.which) {
        Ok(displaced) => settle_exchange(
            worker,
            volume,
            job,
            expected,
            &displaced.path,
            None,
            between,
        ),
        Err(ExchangeRefusal::NotDurable { displaced, error }) => settle_exchange(
            worker,
            volume,
            job,
            expected,
            &displaced.path,
            Some(error.to_string()),
            between,
        ),
        Err(ExchangeRefusal::Link) => match job.link_policy {
            LinkPolicy::Refuse => refused(Refusal::Link),
            LinkPolicy::PlainWriter => plainly(worker, volume, job, expected, temp, None),
        },
        Err(ExchangeRefusal::ExchangeUnavailable(error)) => match job.no_exchange {
            NoExchange::Refuse => refused(Refusal::ExchangeUnavailable(error.to_string())),
            NoExchange::PlainWriter => plainly(
                worker,
                volume,
                job,
                expected,
                temp,
                Some(volume.drive(&job.which)),
            ),
        },
        Err(ExchangeRefusal::BackupNameTaken(path)) => refused(Refusal::BackupNameTaken(path)),
        Err(ExchangeRefusal::Failed(error)) => refused(Refusal::Disk(error.to_string())),
    }
}

/// **The plain write** a request's policy asks for where the exchange refused:
/// the sibling already staged and flushed is the copy that goes in — nothing
/// writes the body again. What a rename can carry is carried onto it first
/// (`bt_platform::carry_metadata`: the attribute word on Windows; owner, mode
/// and extended attributes on Unix); then the file is read again, and only
/// while it still holds `expected` does the staged copy take its name by one
/// rename ([`Volume::commit`]), with no displaced copy to read back. A writer
/// that crossed before that read is answered as the precondition answers it —
/// `Moved` with its bytes — and is not written over; the staged copy goes. One
/// that writes between the end of that read and the rename is replaced unseen:
/// that span is the plain road's whole window, which the exchange road does
/// not have. `ordinary_on`: the drive, when the road was taken because the drive
/// cannot exchange.
fn plainly(
    worker: &WorkerCtx,
    volume: &dyn Volume,
    job: &DocumentJob,
    expected: &[u8],
    temp: &Path,
    ordinary_on: Option<Drive>,
) -> Served {
    bt_platform::carry_metadata(temp, &job.which);
    if let Some(answer) =
        unless_it_holds(bt_persist::read_stable(worker, &job.which), Some(expected))
    {
        let _ = std::fs::remove_file(temp);
        return answer;
    }
    Served::plainly(match volume.commit(temp, &job.which) {
        Ok(()) => Outcome::Landed {
            bytes: Arc::clone(&job.bytes),
            kept: None,
            ordinary_on,
        },
        Err(error) => Outcome::Refused {
            cause: Refusal::Disk(error),
        },
    })
}

/// **§3.2 step 4: what the exchange displaced, read and judged.** The bytes
/// the precondition expected are Folio's own and go — unless the exchange is not
/// known to be durable, when the displaced copy is the only durable copy of the
/// file's last state and stays, retired by a later landing (carry-forward 5).
/// Anything else is a hand edit that crossed: kept for a merge, or put back.
pub(crate) fn settle_exchange(
    worker: &WorkerCtx,
    volume: &dyn Volume,
    job: &DocumentJob,
    expected: &[u8],
    displaced: &Path,
    not_durable: Option<String>,
    between: &mut dyn FnMut(Step),
) -> Served {
    between(Step::Exchanged);
    let theirs = match bt_persist::read_stable(worker, displaced) {
        StableRead::Bytes(bytes) => bytes,
        other => {
            return Served {
                outcome: Outcome::Landed {
                    bytes: Arc::clone(&job.bytes),
                    kept: None,
                    ordinary_on: None,
                },
                not_durable,
                left: Some((
                    displaced.to_path_buf(),
                    format!("what the exchange displaced could not be read back: {other:?}"),
                )),
            };
        }
    };
    if *theirs == *expected {
        let kept = if not_durable.is_some() {
            Some(displaced.to_path_buf())
        } else {
            let _ = std::fs::remove_file(displaced);
            None
        };
        return Served {
            outcome: Outcome::Landed {
                bytes: Arc::clone(&job.bytes),
                kept,
                ordinary_on: None,
            },
            not_durable,
            left: None,
        };
    }
    match job.mode {
        Mode::Merge => Served {
            outcome: Outcome::Displaced {
                bytes: theirs,
                kept: displaced.to_path_buf(),
            },
            not_durable,
            left: None,
        },
        Mode::Restore => {
            let mut restored = restore(worker, volume, job, displaced, theirs);
            restored.not_durable = restored.not_durable.or(not_durable);
            restored
        }
    }
}

/// **`Restore`'s second exchange** ((c)3): only while the file still holds what
/// was written, the displaced file goes back under its name. What that exchange
/// displaces in turn is judged by [`settle_restoring_exchange`]; a file that
/// moved again before it, or a second exchange that is refused, leaves both
/// files and names the one beside it.
fn restore(
    worker: &WorkerCtx,
    volume: &dyn Volume,
    job: &DocumentJob,
    displaced: &Path,
    theirs: Arc<[u8]>,
) -> Served {
    let crossed = |kept: &Path| {
        Served::plainly(Outcome::Conflict {
            theirs: Arc::clone(&theirs),
            kept: Some(kept.to_path_buf()),
        })
    };
    match bt_persist::read_stable(worker, &job.which) {
        StableRead::Bytes(bytes) if *bytes == *job.bytes => {}
        _ => return crossed(displaced),
    }
    match volume.exchange(worker, displaced, &job.which) {
        Err(
            bt_platform::ExchangeRefusal::Link
            | bt_platform::ExchangeRefusal::ExchangeUnavailable(_)
            | bt_platform::ExchangeRefusal::BackupNameTaken(_)
            | bt_platform::ExchangeRefusal::Failed(_),
        ) => crossed(displaced),
        answer => settle_restoring_exchange(worker, job, theirs, answer),
    }
}

/// **What the restoring exchange displaced, judged** ((c)3; review round 2).
/// The file holds the person's bytes again. What came back is Folio's own
/// write, and goes — unless the exchange is not known to be durable
/// (`NotDurable`): then the restore landed and the displaced copy is kept as an
/// obligation, never deleted, exactly as the first exchange's would be. Bytes
/// that are not Folio's are a third writer's, and stay beside the file.
pub(crate) fn settle_restoring_exchange(
    worker: &WorkerCtx,
    job: &DocumentJob,
    theirs: Arc<[u8]>,
    answer: Result<bt_platform::Displaced, bt_platform::ExchangeRefusal>,
) -> Served {
    let (back, not_durable) = match answer {
        Ok(back) => (back, None),
        Err(bt_platform::ExchangeRefusal::NotDurable { displaced, error }) => {
            (displaced, Some(error.to_string()))
        }
        Err(refused) => {
            return Served {
                outcome: Outcome::Conflict { theirs, kept: None },
                not_durable: None,
                left: Some((
                    job.which.clone(),
                    format!("the putting back was refused: {refused:?}"),
                )),
            };
        }
    };
    let ours = matches!(
        bt_persist::read_stable(worker, &back.path),
        StableRead::Bytes(bytes) if *bytes == *job.bytes
    );
    if ours && not_durable.is_none() {
        let _ = std::fs::remove_file(&back.path);
        return Served::plainly(Outcome::Conflict { theirs, kept: None });
    }
    Served {
        outcome: Outcome::Conflict {
            theirs,
            kept: Some(back.path),
        },
        not_durable,
        left: None,
    }
}

/// **Remove the kept files a durable landing of `landed` retires**, and answer
/// which went (a file already gone counts as gone).
fn retire(worker: &WorkerCtx, kept: &[KeptFile], landed: u64) -> Vec<PathBuf> {
    let _ = worker;
    retired_by(kept, landed)
        .filter(|file| match std::fs::remove_file(&file.path) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        })
        .map(|file| file.path.clone())
        .collect()
}

/// **One of the four hand-editable documents, as the store half needs it.**
pub(crate) trait StoredDocument: Clone + PartialEq + Default + Send + 'static {
    /// The document as the bytes its file holds.
    fn to_bytes(&self) -> Result<Vec<u8>, String>;
    /// Bytes read as the file is read, without a disk.
    fn from_bytes(bytes: &[u8]) -> Result<Self, FallbackReason>;
    /// **Apply to `desired` exactly the units a press changed** between
    /// `before` and `after`, the projection on each side of it ((b)6).
    fn apply_press(desired: &mut Self, before: &Self, after: &Self);
    /// **The units in which `desired` differs from `on_disk`**, named the way the
    /// document names them — what `diagnostics.log` says a change that was not
    /// saved was made of.
    fn units(desired: &Self, on_disk: &Self) -> Vec<String>;
}

/// The top-level fields of a document's file shape that differ between `a` and
/// `b`, by name.
fn fields_differing<T: serde::Serialize>(a: &T, b: &T) -> Vec<String> {
    use serde_json::Value;
    let (Ok(Value::Object(a)), Ok(Value::Object(b))) =
        (serde_json::to_value(a), serde_json::to_value(b))
    else {
        return Vec::new();
    };
    let mut names: Vec<String> = a
        .iter()
        .filter(|(name, value)| b.get(*name) != Some(*value))
        .map(|(name, _)| name.clone())
        .collect();
    names.extend(b.keys().filter(|name| !a.contains_key(*name)).cloned());
    names
}

/// The keys under which two lists of entries hold different entries, in the
/// order the keys first appear.
fn keys_differing<E, K>(a: &[E], b: &[E], key: impl Fn(&E) -> K) -> Vec<K>
where
    E: PartialEq,
    K: PartialEq,
{
    let mut keys: Vec<K> = Vec::new();
    for entry in a.iter().chain(b) {
        let name = key(entry);
        if keys.contains(&name) {
            continue;
        }
        let under = |list: &[E]| -> Vec<usize> {
            list.iter()
                .enumerate()
                .filter(|(_, entry)| key(entry) == name)
                .map(|(index, _)| index)
                .collect()
        };
        let (in_a, in_b) = (under(a), under(b));
        let same = in_a.len() == in_b.len()
            && in_a
                .iter()
                .zip(&in_b)
                .all(|(&left, &right)| a[left] == b[right]);
        if !same {
            keys.push(name);
        }
    }
    keys
}

impl StoredDocument for SettingsV1 {
    fn to_bytes(&self) -> Result<Vec<u8>, String> {
        bt_persist::serialize_settings(self).map_err(|error| error.to_string())
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, FallbackReason> {
        bt_persist::parse_settings(bytes)
    }

    /// The unit is the field ((b)8).
    fn apply_press(desired: &mut Self, before: &Self, after: &Self) {
        if desired == before {
            *desired = after.clone();
            return;
        }
        if let Some(merged) = fields_changed(desired, before, after) {
            *desired = merged;
        }
    }

    /// The unit is the field.
    fn units(desired: &Self, on_disk: &Self) -> Vec<String> {
        fields_differing(desired, on_disk)
    }
}

/// `desired` with every top-level field that differs between `before` and
/// `after` taken from `after` — a document's fields, by name, through the shape
/// its file is written in. `None` when one of the three is not an object, which
/// no settings document is.
fn fields_changed<T>(desired: &T, before: &T, after: &T) -> Option<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    use serde_json::Value;
    let (Ok(Value::Object(before)), Ok(Value::Object(after)), Ok(Value::Object(mut desired))) = (
        serde_json::to_value(before),
        serde_json::to_value(after),
        serde_json::to_value(desired),
    ) else {
        return None;
    };
    for (name, value) in &after {
        if before.get(name) != Some(value) {
            desired.insert(name.clone(), value.clone());
        }
    }
    for name in before.keys() {
        if !after.contains_key(name) {
            desired.remove(name);
        }
    }
    serde_json::from_value(Value::Object(desired)).ok()
}

/// **A document's own reader, as a press needs it**: which entry under a key is
/// in force, and which entries go with it when a press takes it away — the one
/// rule the document's readers use, never a second one in the press.
trait InForce<E, K> {
    /// The entry under `key` its readers put in force.
    fn in_force(&self, list: &[E], key: &K) -> Option<usize>;
    /// The entries that go with it when it goes.
    fn alongside(&self, list: &[E], key: &K) -> Vec<usize>;
    /// **Where an entry the press adds under `key` goes, when the list already
    /// holds entries under it that are not in force**: after the last of them,
    /// so the reader's rule makes the new entry the one in force. `None`: no
    /// such entries, and the entry goes after its nearest predecessor.
    fn last_under(&self, list: &[E], key: &K) -> Option<usize>;
}

/// **The first usable entry under a key is the one in force** — the installer's
/// rule for profiles (`usable`: `profiles::installable`, its own test), the
/// menus' for pins (every pin row is usable) — and **the entries under the key
/// after it are its duplicates, which go with it**: they were never in force,
/// and none of them becomes the live entry because the one in force went
/// (T-STORE-MERGE-PROFILES-PINS, review rounds 3–5). An unusable entry before
/// it is refused: it is not in force, does not go with it, and stays.
struct First<F, U> {
    key: F,
    usable: U,
}

impl<E, K: PartialEq, F: Fn(&E) -> K, U: Fn(&E) -> bool> InForce<E, K> for First<F, U> {
    fn in_force(&self, list: &[E], key: &K) -> Option<usize> {
        list.iter()
            .position(|entry| (self.key)(entry) == *key && (self.usable)(entry))
    }

    fn alongside(&self, list: &[E], key: &K) -> Vec<usize> {
        let Some(in_force) = self.in_force(list, key) else {
            return Vec::new();
        };
        (in_force + 1..list.len())
            .filter(|&at| (self.key)(&list[at]) == *key)
            .collect()
    }

    /// An entry the press adds goes after its nearest predecessor: the reader
    /// takes the first usable entry under the key wherever it stands, and a
    /// refused entry before it stays refused.
    fn last_under(&self, _: &[E], _: &K) -> Option<usize> {
        None
    }
}

/// **A shortcut document's lines as its readers take them**
/// (`settings_bundle::read_lines`): an action's line in force is the one that
/// `Reads`, and its `Shadowed` lines go with it.
struct ShortcutLines {
    dialect: bt_persist::KeybindingsPlatformV1,
    platform: bt_platform::HostPlatform,
}

impl ShortcutLines {
    fn lines(
        &self,
        list: &[BindingOverrideV1],
        action: &str,
        wanted: fn(&LineRead) -> bool,
    ) -> Vec<usize> {
        list.iter()
            .zip(read_lines(list, self.dialect, self.platform))
            .enumerate()
            .filter(|(_, (line, read))| line.action == action && wanted(read))
            .map(|(index, _)| index)
            .collect()
    }
}

impl InForce<BindingOverrideV1, String> for ShortcutLines {
    fn in_force(&self, list: &[BindingOverrideV1], action: &String) -> Option<usize> {
        self.lines(list, action, |read| matches!(read, LineRead::Reads(_)))
            .into_iter()
            .next()
    }

    fn alongside(&self, list: &[BindingOverrideV1], action: &String) -> Vec<usize> {
        self.lines(list, action, |read| *read == LineRead::Shadowed)
    }

    /// An action whose only lines do not read: the line a press adds goes after
    /// them, so the later readable line shadows them and they are silent.
    fn last_under(&self, list: &[BindingOverrideV1], action: &String) -> Option<usize> {
        list.iter().rposition(|line| line.action == *action)
    }
}

/// **The pins menus' reader**: a pin's identity is its category and target as
/// the file spells them; the first row of an identity is the pin, and every
/// row of a category this build names is usable — a row of one it cannot name
/// has an identity no press asks for, and is never taken.
fn pin_reader() -> First<
    impl Fn(&bt_persist::PinEntryV1) -> (String, String),
    impl Fn(&bt_persist::PinEntryV1) -> bool,
> {
    First {
        key: |pin: &bt_persist::PinEntryV1| (pin.kind.clone(), pin.target.clone()),
        usable: |_: &bt_persist::PinEntryV1| true,
    }
}

/// **The one removal road**: the entry under `name` that `reader` puts in
/// force, and every entry its reader says goes with it (`alongside`) —
/// for profiles and pins its duplicates after it, never a refused row before
/// it; for keybindings the lines it shadows. A press's delta takes an entry
/// away by this, and so does an unpin.
fn take_away<E, K>(list: &mut Vec<E>, name: &K, reader: &impl InForce<E, K>) {
    let Some(at) = reader.in_force(list, name) else {
        return;
    };
    let mut gone = reader.alongside(list, name);
    gone.push(at);
    gone.sort_unstable();
    gone.dedup();
    for index in gone.into_iter().rev() {
        list.remove(index);
    }
}

/// **Unpin**: the pin `(kind, target)` and its duplicates go, by the menus'
/// reader ([`take_away`]) — `pins::toggle_pin`'s removal.
pub(crate) fn unpin(pins: &mut Vec<bt_persist::PinEntryV1>, kind: &str, target: &str) {
    take_away(pins, &(kind.to_owned(), target.to_owned()), &pin_reader());
}

/// **A press's delta over a list of entries named by a key** ((b)6, (b)8's
/// units): every key whose entry in force differs between `before` and `after`
/// takes `after`'s in `desired` — replaced where `desired` has it in force,
/// added after its nearest predecessor in `after` otherwise, removed when
/// `after` has none — and, when the press reordered the keys both sides hold,
/// those entries take `after`'s order in the places they hold in `desired`.
/// Entries `desired` carries that the projection never had (a line a reader
/// refused, a refused profile row) keep their place — except what the reader
/// says goes with an entry in force (`alongside`: a keybindings line's earlier
/// lines about its action; a profile's or a pin's later duplicates), which is
/// removed with it ([`take_away`]).
///
/// `in_force` is the document's own reader's answer to "which entry under this
/// key is in force": the one rule its readers use, never a second one here.
fn apply_keyed_press<E, K>(
    desired: &mut Vec<E>,
    before: &[E],
    after: &[E],
    key: impl Fn(&E) -> K,
    reader: &impl InForce<E, K>,
) where
    E: Clone + PartialEq,
    K: PartialEq + Clone,
{
    if desired.as_slice() == before {
        *desired = after.to_vec();
        return;
    }
    let position = |list: &[E], wanted: &K| -> Option<usize> { reader.in_force(list, wanted) };
    let mut keys: Vec<K> = Vec::new();
    for entry in before.iter().chain(after) {
        let name = key(entry);
        if !keys.contains(&name) {
            keys.push(name);
        }
    }
    for name in &keys {
        let was = position(before, name).map(|index| &before[index]);
        let is = position(after, name).map(|index| &after[index]);
        if was == is {
            continue;
        }
        match (position(desired, name), is) {
            (Some(at), Some(entry)) => desired[at] = entry.clone(),
            (Some(_), None) => take_away(desired, name, reader),
            (None, Some(entry)) => {
                let at = position(after, name).unwrap_or(0);
                let anchor = reader.last_under(desired, name).or_else(|| {
                    after[..at]
                        .iter()
                        .rev()
                        .find_map(|earlier| position(desired, &key(earlier)))
                });
                desired.insert(anchor.map_or(0, |index| index + 1), entry.clone());
            }
            (None, None) => {}
        }
    }
    // The order of the keys both sides of the press hold.
    let common = |list: &[E]| -> Vec<K> {
        let mut seen: Vec<K> = Vec::new();
        for entry in list {
            let name = key(entry);
            if position(before, &name).is_some()
                && position(after, &name).is_some()
                && !seen.contains(&name)
            {
                seen.push(name);
            }
        }
        seen
    };
    let order = common(after);
    if common(before) != order {
        let slots: Vec<usize> = order
            .iter()
            .filter_map(|name| position(desired, name))
            .collect::<std::collections::BTreeSet<usize>>()
            .into_iter()
            .collect();
        let entries: Vec<E> = order
            .iter()
            .filter_map(|name| position(desired, name).map(|index| desired[index].clone()))
            .collect();
        for (slot, entry) in slots.into_iter().zip(entries) {
            desired[slot] = entry;
        }
    }
}

impl StoredDocument for KeybindingsV1 {
    fn to_bytes(&self) -> Result<Vec<u8>, String> {
        bt_persist::serialize_keybindings(self).map_err(|error| error.to_string())
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, FallbackReason> {
        bt_persist::parse_keybindings(bytes)
    }

    /// The unit is the action, and the line in force is the one the readers
    /// put in force (`settings_bundle::read_lines`, the one selection of the
    /// launch, an import and the merge): the last line about the action that
    /// reads. A press that takes an action's departure away removes that line
    /// and the earlier lines it shadows, which would otherwise be the last about
    /// the action at the next start.
    ///
    /// **A press is laid in the press's dialect** — the one the store writes in
    /// (`KeybindingsStore::store_on`), which every press records. `desired` in
    /// another dialect (a file written on the other platform, before any merge
    /// carried it here) is first carried into it the way the merge carries a
    /// side (`keybindings_merge::in_dialect`): each line read in its own dialect
    /// and spelled in this one, what is not in force — an action this build has
    /// no row for, a shadowed line, a line this dialect cannot read — kept
    /// verbatim in place. Then the press is a delta, so nothing the projection
    /// lacks is lost to a dialect difference.
    fn apply_press(desired: &mut Self, before: &Self, after: &Self) {
        // A press always records the dialect it is written in; a document that
        // records none is a v1 file, never a press.
        let Some(here) = crate::settings_bundle::dialect_of(after.writing_platform) else {
            *desired = after.clone();
            return;
        };
        if desired == before {
            *desired = after.clone();
            return;
        }
        if desired.writing_platform != after.writing_platform {
            *desired = crate::keybindings_merge::in_dialect(desired, here);
        }
        desired.schema_version = after.schema_version;
        apply_keyed_press(
            &mut desired.bindings,
            &before.bindings,
            &after.bindings,
            |binding| binding.action.clone(),
            &ShortcutLines {
                dialect: desired.writing_platform,
                platform: here,
            },
        );
    }

    /// The unit is the action.
    fn units(desired: &Self, on_disk: &Self) -> Vec<String> {
        keys_differing(&desired.bindings, &on_disk.bindings, |binding| {
            binding.action.clone()
        })
    }
}

impl StoredDocument for ProfilesV1 {
    fn to_bytes(&self) -> Result<Vec<u8>, String> {
        bt_persist::serialize_profiles(self).map_err(|error| error.to_string())
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, FallbackReason> {
        bt_persist::parse_profiles(bytes)
    }

    /// The unit is the row, by id: the first row the installer can start
    /// (`profiles::installable`, the installer's rule). A removal takes it and
    /// its duplicates after it, and leaves a refused row before it ([`First`]).
    fn apply_press(desired: &mut Self, before: &Self, after: &Self) {
        if desired == before {
            *desired = after.clone();
            return;
        }
        desired.schema_version = after.schema_version;
        apply_keyed_press(
            &mut desired.profiles,
            &before.profiles,
            &after.profiles,
            |row| row.id.clone(),
            &First {
                key: |row: &bt_persist::ProfileEntryV1| row.id.clone(),
                usable: crate::profiles::installable(),
            },
        );
    }

    /// The unit is the row, by id.
    fn units(desired: &Self, on_disk: &Self) -> Vec<String> {
        keys_differing(&desired.profiles, &on_disk.profiles, |row| row.id.clone())
    }
}

impl StoredDocument for PinsV1 {
    fn to_bytes(&self) -> Result<Vec<u8>, String> {
        bt_persist::serialize_pins(self).map_err(|error| error.to_string())
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, FallbackReason> {
        bt_persist::parse_pins(bytes)
    }

    /// The unit is the pin, by its category and target, the first in force (the
    /// menus' rule); a top-level key of the file's own the press changed is a
    /// unit too.
    fn apply_press(desired: &mut Self, before: &Self, after: &Self) {
        if desired == before {
            *desired = after.clone();
            return;
        }
        desired.schema_version = after.schema_version;
        for (name, value) in &after.extra {
            if before.extra.get(name) != Some(value) {
                desired.extra.insert(name.clone(), value.clone());
            }
        }
        for name in before.extra.keys() {
            if !after.extra.contains_key(name) {
                desired.extra.remove(name);
            }
        }
        apply_keyed_press(
            &mut desired.pins,
            &before.pins,
            &after.pins,
            |pin| (pin.kind.clone(), pin.target.clone()),
            &pin_reader(),
        );
    }

    /// The unit is the pin, by its category and target, and each top-level key
    /// of the file's own.
    fn units(desired: &Self, on_disk: &Self) -> Vec<String> {
        let mut units: Vec<String> = keys_differing(&desired.pins, &on_disk.pins, |pin| {
            format!("{} {}", pin.kind, pin.target)
        });
        units.extend(
            desired
                .extra
                .keys()
                .chain(on_disk.extra.keys())
                .filter(|name| desired.extra.get(*name) != on_disk.extra.get(*name))
                .cloned(),
        );
        units.dedup();
        units
    }
}

/// **What a stable read came to, parsed on the worker.**
#[derive(Debug)]
pub(crate) enum ReadAnswer<T> {
    /// No worker took the read; the request door completed it itself.
    NotServed(String),
    Absent,
    Unobtained(String),
    MovedDuringRead,
    Parsed {
        bytes: Arc<[u8]>,
        document: T,
    },
    Refused {
        bytes: Arc<[u8]>,
        reason: FallbackReason,
    },
}

impl<T: StoredDocument> ReadAnswer<T> {
    /// The read, parsed through the document's own chain.
    pub(crate) fn of(read: StableRead) -> Self {
        match read {
            StableRead::Absent => Self::Absent,
            StableRead::Unobtained(error) => Self::Unobtained(error),
            StableRead::MovedDuringRead => Self::MovedDuringRead,
            StableRead::Bytes(bytes) => match T::from_bytes(&bytes) {
                Ok(document) => Self::Parsed { bytes, document },
                Err(reason) => Self::Refused { bytes, reason },
            },
        }
    }
}

/// **A re-read's news for a reader** ([`DocumentState::take_answers`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReadNews {
    /// Whether the newest read found a file that would not read — `None` when
    /// no read was answered. A stale read's verdict is not said over a newer
    /// one's.
    pub(crate) unreadable: Option<bool>,
}

/// **One re-read's news, as a test names it.**
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// The file is the one Folio left, or the pass put nothing new in force.
    Unchanged,
    /// The pass's merge put a new document in force (a store's `reread`).
    Changed,
    /// The file would not read, and what was in force stays.
    Unreadable,
}

#[cfg(test)]
impl ReadNews {
    /// What a test asks of one pass's news.
    pub(crate) fn heard(self) -> Heard {
        if self.unreadable == Some(true) {
            Heard::Unreadable
        } else {
            Heard::Unchanged
        }
    }
}

/// **A file's bytes, kept for the merge** (T-STORE-MERGE-*): a `Moved`,
/// `Displaced` or `Conflict` answer, or a re-read that found the file changed
/// while a change of Folio's was not yet on disk — or, for a store that merges
/// what it re-reads, any re-read that found the file changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingMerge<T> {
    /// The revision the file was compared with — the merge's base: for a
    /// `Displaced` answer, the revision before the exchange ((b)3).
    pub(crate) base: Revision<T>,
    /// What the file held; `None`: no file.
    pub(crate) theirs: Option<Arc<[u8]>>,
    /// Where the person's bytes are kept beside the file (`Displaced`).
    pub(crate) kept: Option<PathBuf>,
    /// The document the exchange wrote (`Displaced`): the merge's `O` ((c)1).
    /// `None` for every other answer, whose `O` is `desired`.
    pub(crate) written: Option<T>,
}

/// **What one merge pass replaced in one file** — the data the pass's card
/// (§3.6, T-STORE-NOTICE) names: the file, every value the file won a
/// conflict over, and every value whose door refused it ("not applied"),
/// each named the way the reader knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReplacedInFile {
    pub(crate) file: &'static str,
    pub(crate) values: Vec<String>,
    pub(crate) not_applied: Vec<String>,
}

/// **What one storage pass's merges replaced**: one entry per file that lost a
/// value of Folio's to the file — none for a clean hand edit (§3.6, owner
/// ruling Q2). T-STORE-NOTICE raises the pass's one card from it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MergeNotice {
    pub(crate) files: Vec<ReplacedInFile>,
}

/// One request a store has on the worker.
enum Outstanding<T> {
    Document(InFlight<T>),
    Read {
        answer: OnTheWorker<(u64, ReadAnswer<T>)>,
    },
}

/// The one document write a store has in flight.
struct InFlight<T> {
    generation: u64,
    document: T,
    bytes: Arc<[u8]>,
    /// The revision the write is conditional on.
    base: Revision<T>,
    answer: OnTheWorker<DocumentAnswer>,
}

/// **The store half one hand-editable document holds** — see the module
/// header.
pub(crate) struct DocumentState<T> {
    file: &'static str,
    path: PathBuf,
    revision: Revision<T>,
    projection: T,
    desired: T,
    version: u64,
    generation: u64,
    mode: Mode,
    outstanding: VecDeque<Outstanding<T>>,
    /// A re-read was asked for while one was on the worker: one more is sent
    /// when it has been answered.
    read_again: bool,
    /// A press arrived while a write was in flight: the document is sent when
    /// that write's answer has been processed, whatever it was.
    send_again: bool,
    /// The last write was refused: the next ear pass that reads this document
    /// sends it once more (C5's "next ear event"; presses send it anyway).
    retry_on_ear: bool,
    /// **Held for the merge** (review round 2; (c)3): a hand edit crossed a
    /// write and was put back, so the revision is the person's file, and
    /// nothing is written over it — not by a press, an ear pass or a quit —
    /// until the merge folds it in. `desired` keeps taking presses meanwhile.
    held_for_merge: bool,
    merges: Vec<PendingMerge<T>>,
    kept: Vec<KeptFile>,
    writes: DocumentWrites,
    writer_of_record: bool,
    deferral: crate::update_trial::Writer,
    lane: StorageLane,
    /// **Why the last write was refused**, until the next write is sent: a
    /// quit's documents phase reads a refusal as a terminal answer.
    refusal: Option<String>,
    /// **This document's copy into `recovered`** in a quit's recovery phase, once
    /// sent: its answer on the way, or the answer.
    recovery: Option<RecoveryCopy>,
}

/// A quit's copy of a document, on the storage worker or answered.
enum RecoveryCopy {
    Sent(OnTheWorker<Result<PathBuf, String>>),
    Answered(Result<PathBuf, String>),
}

impl<T: StoredDocument> DocumentState<T> {
    /// A store's document as it opened: `revision` is what the read had in
    /// hand, and the projection and `desired` are its document.
    pub(crate) fn opened(
        file: &'static str,
        path: PathBuf,
        revision: Revision<T>,
        writer_of_record: bool,
        deferral: crate::update_trial::Writer,
        lane: StorageLane,
    ) -> Self {
        Self {
            file,
            path,
            projection: revision.document.clone(),
            desired: revision.document.clone(),
            revision,
            version: 0,
            generation: 0,
            mode: Mode::Merge,
            outstanding: VecDeque::new(),
            read_again: false,
            send_again: false,
            retry_on_ear: false,
            held_for_merge: false,
            merges: Vec::new(),
            kept: Vec::new(),
            writes: DocumentWrites::new(),
            writer_of_record,
            deferral,
            lane,
            refusal: None,
            recovery: None,
        }
    }

    /// The document readers read.
    pub(crate) fn projection(&self) -> &T {
        &self.projection
    }

    /// The document Folio wants on disk ((b)6).
    pub(crate) fn desired(&self) -> &T {
        &self.desired
    }

    /// What the file is known to hold.
    #[cfg(test)]
    pub(crate) fn revision(&self) -> &Revision<T> {
        &self.revision
    }

    /// The version of [`Self::desired`].
    #[cfg(test)]
    pub(crate) fn version(&self) -> u64 {
        self.version
    }

    /// The kept files this store holds obligations for.
    #[cfg(test)]
    pub(crate) fn kept(&self) -> &[KeptFile] {
        &self.kept
    }

    /// The file bytes stored for the merge.
    #[cfg(test)]
    pub(crate) fn merges(&self) -> &[PendingMerge<T>] {
        &self.merges
    }

    /// How many requests this store has sent.
    #[cfg(test)]
    pub(crate) fn sent(&self) -> u64 {
        self.generation
    }

    /// Send this store's writes with `mode`.
    #[cfg(test)]
    pub(crate) fn sending(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// How many writes of this document the reader has asked for.
    pub(crate) fn write_generation(&self) -> u64 {
        self.writes.generation()
    }

    /// **Owed** ((b)6): this process writes the file, and `desired` is not what
    /// the file is known to hold.
    pub(crate) fn owed(&self) -> bool {
        self.writer_of_record && self.desired != self.revision.document
    }

    /// **A press**: `next` is the projection now. Answers whether it changed.
    /// The units it changed go into `desired`, which is handed to the worker
    /// unless `held`. A press that changed nothing still sends a document that
    /// is owed — the gesture somebody makes when a change did not take (review
    /// row R4-10).
    pub(crate) fn store(&mut self, next: T, held: bool) -> bool {
        let changed = self.projection != next;
        if !changed && !self.owed() {
            return false;
        }
        self.writes.requested();
        if changed {
            T::apply_press(&mut self.desired, &self.projection, &next);
            self.version += 1;
            self.projection = next;
        }
        if !held {
            self.send_owed();
        }
        changed
    }

    /// **The projection is `projection`**, and `desired` does not move: what a
    /// reader put in force from the document and what the document says are
    /// two facts (a refused line is in one and not the other, (b)8).
    pub(crate) fn project(&mut self, projection: T) {
        self.projection = projection;
    }

    /// **The request door: hand the owed document to the storage worker.**
    ///
    /// Nothing when this process does not write the file (§6), while an update's
    /// trial holds its writes, or when nothing is owed. One write in flight: a
    /// call while one is marks the document to be sent when that answer has been
    /// processed. Nothing while the document is held for the merge. A request no
    /// worker takes is completed here as refused, and the loop is woken for it
    /// (C2): the answer reaches the store the way the worker's would.
    pub(crate) fn send_owed(&mut self) {
        if !self.writer_of_record
            || self.held_for_merge
            || crate::update_trial::defer(self.deferral)
        {
            return;
        }
        if self
            .outstanding
            .iter()
            .any(|request| matches!(request, Outstanding::Document(_)))
        {
            self.send_again = true;
            return;
        }
        self.send_again = false;
        if self.desired == self.revision.document {
            return;
        }
        let bytes: Arc<[u8]> = match self.desired.to_bytes() {
            Ok(bytes) => bytes.into(),
            Err(error) => {
                self.writes.record(self.file, Err(error));
                return;
            }
        };
        self.generation += 1;
        self.retry_on_ear = false;
        self.refusal = None;
        let (answer, answered) = mpsc::channel();
        let refusal = answer.clone();
        let job = DocumentJob {
            which: self.path.clone(),
            generation: self.generation,
            version: self.version,
            expected: self.revision.bytes.clone(),
            bytes: Arc::clone(&bytes),
            mode: self.mode,
            link_policy: LinkPolicy::Refuse,
            no_exchange: NoExchange::Refuse,
            asker: 0,
            kept: self.kept.clone(),
            answer,
        };
        let answer = match self.lane.send_document(job) {
            Ok(request) => OnTheWorker::admitted(answered, request),
            Err(refused) => {
                let _ = refusal.send(DocumentAnswer {
                    generation: self.generation,
                    version: self.version,
                    asker: 0,
                    outcome: Outcome::Refused {
                        cause: Refusal::NoWorker(refused.said(crate::persist::NO_STORAGE_WORKER)),
                    },
                    not_durable: None,
                    retired: Vec::new(),
                    left: None,
                });
                self.lane.wake();
                OnTheWorker::answered_at_the_door(answered)
            }
        };
        self.outstanding.push_back(Outstanding::Document(InFlight {
            generation: self.generation,
            document: self.desired.clone(),
            bytes,
            base: self.revision.clone(),
            answer,
        }));
    }

    /// **The pass's request door: ask the worker to read the file again.** One
    /// read on the worker at a time; a second ask while one is out is served by
    /// one more read when it is answered.
    pub(crate) fn ask_read(&mut self) {
        if self
            .outstanding
            .iter()
            .any(|request| matches!(request, Outstanding::Read { .. }))
        {
            self.read_again = true;
            return;
        }
        self.read_again = false;
        self.generation += 1;
        let (answer, answered) = mpsc::channel();
        let refusal = answer.clone();
        let job = ReadJob {
            which: self.path.clone(),
            generation: self.generation,
            answer: Box::new(move |generation, read| {
                let _ = answer.send((generation, ReadAnswer::<T>::of(read)));
            }),
        };
        let answer = match self.lane.send_read(job) {
            Ok(request) => OnTheWorker::admitted(answered, request),
            Err(refused) => {
                let _ = refusal.send((
                    self.generation,
                    ReadAnswer::NotServed(refused.said(crate::persist::NO_STORAGE_WORKER)),
                ));
                self.lane.wake();
                OnTheWorker::answered_at_the_door(answered)
            }
        };
        self.outstanding.push_back(Outstanding::Read { answer });
    }

    /// **Take every answer the worker has given, in the order it served them**,
    /// and send what is owed after them. Never waits.
    pub(crate) fn take_answers(&mut self) -> ReadNews {
        let mut news = ReadNews::default();
        let mut landed = false;
        let mut heard = false;
        while let Some(request) = self.outstanding.front_mut() {
            let answered = match request {
                Outstanding::Document(flight) => match flight.answer.hear() {
                    Reply::Answered(answer) => Some(Ok(answer)),
                    Reply::Waiting => None,
                    Reply::Failed(words) => Some(Err(Refusal::Failed(words))),
                    Reply::Gone => Some(Err(Refusal::NoWorker(
                        crate::persist::STORAGE_WORKER_GONE.to_owned(),
                    ))),
                },
                Outstanding::Read { answer } => match answer.hear() {
                    Reply::Answered((generation, read)) => {
                        self.outstanding.pop_front();
                        heard = true;
                        self.on_read(generation, read, &mut news);
                        continue;
                    }
                    Reply::Waiting => None,
                    Reply::Failed(words) => {
                        self.outstanding.pop_front();
                        eprintln!("BT_PERSIST {} was not read again: {words}", self.file);
                        continue;
                    }
                    Reply::Gone => {
                        self.outstanding.pop_front();
                        eprintln!(
                            "BT_PERSIST {} was not read again: {}",
                            self.file,
                            crate::persist::STORAGE_WORKER_GONE
                        );
                        continue;
                    }
                },
            };
            let Some(answered) = answered else {
                break;
            };
            let Some(Outstanding::Document(flight)) = self.outstanding.pop_front() else {
                break;
            };
            landed |= self.on_document(flight, answered);
        }
        self.after_answers(landed, heard);
        news
    }

    /// Wait for the answer to every request this store has on the worker now,
    /// then take them, and send what is owed after them — a test's way to let
    /// the worker finish. A request sent after them is not waited for.
    #[cfg(test)]
    pub(crate) fn settle(&mut self) -> ReadNews {
        let mut news = ReadNews::default();
        let mut landed = false;
        let mut heard = false;
        let file = self.file;
        for _ in 0..self.outstanding.len() {
            match self.outstanding.pop_front() {
                Some(Outstanding::Document(mut flight)) => {
                    let answer = match flight.answer.within_patience(file) {
                        Reply::Answered(answer) => Ok(answer),
                        Reply::Failed(words) => Err(Refusal::Failed(words)),
                        Reply::Waiting | Reply::Gone => Err(Refusal::NoWorker(
                            crate::persist::STORAGE_WORKER_GONE.to_owned(),
                        )),
                    };
                    landed |= self.on_document(flight, answer);
                }
                Some(Outstanding::Read { mut answer }) => {
                    if let Reply::Answered((generation, read)) = answer.within_patience(file) {
                        heard = true;
                        self.on_read(generation, read, &mut news);
                    }
                }
                None => break,
            }
        }
        self.after_answers(landed, heard);
        news
    }

    /// A read answer handed in as if the worker had served it now — a test's
    /// way to put a read between two others.
    #[cfg(test)]
    pub(crate) fn hear_read(&mut self, read: StableRead) -> ReadNews {
        let mut news = ReadNews::default();
        self.generation += 1;
        self.on_read(self.generation, ReadAnswer::of(read), &mut news);
        news
    }

    /// What follows a batch of answers: one more read if one was asked for
    /// meanwhile; the document again if a press arrived while it was in flight,
    /// or a landing left `desired` ahead of the file (a press during the write).
    ///
    /// And once after an ear pass's read when the last write was refused (C5).
    fn after_answers(&mut self, landed: bool, heard: bool) {
        let reading = self
            .outstanding
            .iter()
            .any(|request| matches!(request, Outstanding::Read { .. }));
        if self.read_again && !reading {
            self.ask_read();
        }
        let writing = self
            .outstanding
            .iter()
            .any(|request| matches!(request, Outstanding::Document(_)));
        let ear_retry = heard && self.retry_on_ear && self.owed();
        if !writing && (self.send_again || (landed && self.owed()) || ear_retry) {
            self.send_owed();
        }
    }

    /// **One write's answer** — every one is processed, whatever its generation
    /// ((b)4). Answers whether it landed. `Err` is a write that ended without an
    /// answer: the worker gone with it, or failed on it.
    fn on_document(
        &mut self,
        flight: InFlight<T>,
        answer: Result<DocumentAnswer, Refusal>,
    ) -> bool {
        let file = self.file;
        let answer = match answer {
            Ok(answer) => answer,
            Err(cause) => {
                self.refused(&cause);
                return false;
            }
        };
        if let Some((path, why)) = &answer.left {
            eprintln!(
                "BT_PERSIST {file}: {} is left beside it: {why}",
                path.display()
            );
        }
        for retired in &answer.retired {
            self.kept.retain(|kept| kept.path != *retired);
        }
        match answer.outcome {
            Outcome::Landed { bytes, kept, .. } => {
                self.revision.landed(bytes, flight.document);
                self.writes.record(file, Ok(()));
                if let Some(error) = &answer.not_durable {
                    eprintln!(
                        "BT_PERSIST {file} was written and is not known to be durable: {error}"
                    );
                }
                if let Some(path) = kept {
                    // The copy of what the file held stays until a later landing
                    // is durable (carry-forward 5).
                    self.kept.push(KeptFile {
                        path,
                        version: answer.version + 1,
                    });
                }
                return true;
            }
            Outcome::Moved { bytes } => {
                eprintln!(
                    "BT_PERSIST {file} changed on disk; the change is kept in memory and not \
                     written over it (generation {})",
                    flight.generation
                );
                // (c)1's `Moved` row: the base is the revision as it stands
                // when the answer is processed, in worker order.
                self.keep_for_the_merge(PendingMerge {
                    base: self.revision.clone(),
                    theirs: bytes,
                    kept: None,
                    written: None,
                });
            }
            Outcome::MovedDuringRead => {
                eprintln!("BT_PERSIST {file} was being written by another program; not written");
            }
            Outcome::Displaced { bytes, kept } => {
                eprintln!(
                    "BT_PERSIST {file} was edited while it was written; the edit is kept as {}",
                    kept.display()
                );
                // Stored against the revision the write was conditional on, and
                // only then does the revision become what the file holds ((c)1).
                self.keep_for_the_merge(PendingMerge {
                    base: flight.base,
                    theirs: Some(bytes),
                    kept: Some(kept),
                    written: Some(flight.document.clone()),
                });
                self.revision.landed(flight.bytes, flight.document);
                self.writes.record(file, Ok(()));
                return true;
            }
            Outcome::Conflict { theirs, kept } => {
                self.keep_for_the_merge(PendingMerge {
                    base: flight.base,
                    theirs: Some(Arc::clone(&theirs)),
                    kept: None,
                    written: None,
                });
                // (c)3: the revision is the file the person now has — a file
                // that will not parse is its bytes beside the default document,
                // as at start — and the owed write is held until the merge.
                self.revision = Revision {
                    document: T::from_bytes(&theirs).unwrap_or_default(),
                    bytes: Some(theirs),
                };
                if !std::mem::replace(&mut self.held_for_merge, true) {
                    eprintln!(
                        "BT_PERSIST {file} changed on disk while Folio had unsaved changes; held until merged"
                    );
                }
                match (kept, &answer.not_durable) {
                    (Some(path), Some(error)) => {
                        eprintln!(
                            "BT_PERSIST {file} was put back and is not known to be durable: {error}"
                        );
                        self.kept.push(KeptFile {
                            path,
                            version: answer.version + 1,
                        });
                    }
                    (Some(path), None) => {
                        eprintln!("BT_PERSIST {file}: {} is left beside it", path.display());
                    }
                    (None, _) => {}
                }
            }
            Outcome::Refused { cause } => self.refused(&cause),
        }
        false
    }

    /// A refused write: the streak, or at once ((b)7).
    fn refused(&mut self, cause: &Refusal) {
        self.retry_on_ear = true;
        let error = cause.described();
        self.refusal = Some(error.clone());
        if cause.is_said_at_once() {
            self.writes.refused_for_good(self.file, error);
        } else {
            self.writes.record(self.file, Err(error));
        }
    }

    /// Store a file's bytes for the merge. A newer file with no kept copy
    /// replaces an older one measured against the same base: the merge
    /// reconciles what the file holds now. Against another base it is stored
    /// after it, and the merges run in order, each folding into `desired` what
    /// the next one is measured with.
    ///
    /// **A store that merges holds its writes until the merge has folded the
    /// file in**: a write sent now would be conditional on a file that is no
    /// longer there, and would answer `Moved` (§3.3: an owed edit waits).
    fn keep_for_the_merge(&mut self, merge: PendingMerge<T>) {
        if self.mode == Mode::Merge {
            self.held_for_merge = true;
        }
        if merge.kept.is_none()
            && let Some(last) = self.merges.last_mut()
            && last.kept.is_none()
            && last.base == merge.base
        {
            *last = merge;
            return;
        }
        self.merges.push(merge);
    }

    /// **One re-read's answer** (§3.1, revision (b)9; carry-forwards 1, 2, 4).
    fn on_read(&mut self, generation: u64, read: ReadAnswer<T>, news: &mut ReadNews) {
        let file = self.file;
        let (bytes, document) = match read {
            // The file was being written; the ear has a stamp for the write.
            ReadAnswer::NotServed(why) => {
                eprintln!("BT_PERSIST {file} was not read again: {why}");
                return;
            }
            ReadAnswer::MovedDuringRead => {
                eprintln!(
                    "BT_PERSIST {file} changed while it was read (generation {generation}); the next pass reads it again"
                );
                return;
            }
            ReadAnswer::Unobtained(error) => {
                eprintln!("BT_PERSIST {file} could not be read: {error}");
                news.unreadable = Some(true);
                return;
            }
            ReadAnswer::Refused { bytes, reason } => {
                if self.is_folios_own_write(Some(&bytes)) {
                    news.unreadable = Some(false);
                    return;
                }
                // No copy is kept mid-run (defect 4): the file is the person's,
                // under its own name, and nothing will replace it.
                eprintln!("BT_PERSIST {file} would not parse: {reason:?}");
                news.unreadable = Some(true);
                if self.held_for_merge {
                    self.held_file_moved(Some(bytes), T::default());
                }
                return;
            }
            ReadAnswer::Absent => (None, T::default()),
            ReadAnswer::Parsed { bytes, document } => (Some(bytes), document),
        };
        news.unreadable = Some(false);
        if self.is_folios_own_write(bytes.as_ref()) {
            return;
        }
        if self.held_for_merge {
            self.held_file_moved(bytes, document);
            return;
        }
        // A file that changed is stored for the merge, which folds it in and
        // puts it in force through the document's doors.
        self.keep_for_the_merge(PendingMerge {
            base: self.revision.clone(),
            theirs: bytes,
            kept: None,
            written: None,
        });
    }

    /// **A held document's file changed again**: the file is stored for the
    /// merge against the revision as it stands now — (c)1's `Moved` row, whose
    /// base is always the current revision, answers being processed in worker
    /// order (a `Displaced` before it has moved the revision to the document
    /// written) — and the revision follows the file, the bytes the next merge
    /// must not write over. The hold stays: only the merge releases it.
    fn held_file_moved(&mut self, bytes: Option<Arc<[u8]>>, document: T) {
        self.keep_for_the_merge(PendingMerge {
            base: self.revision.clone(),
            theirs: bytes.clone(),
            kept: None,
            written: None,
        });
        self.revision = Revision { bytes, document };
    }

    /// **Whether a read found the file Folio left**: the revision's bytes, or
    /// the bytes of the write on its way (carry-forward 4). `None` is no file.
    fn is_folios_own_write(&self, bytes: Option<&Arc<[u8]>>) -> bool {
        let read = match bytes {
            Some(bytes) => bt_persist::DocumentBytes::Loaded(Arc::clone(bytes)),
            None => bt_persist::DocumentBytes::Absent,
        };
        if self.revision.holds(&read) {
            return true;
        }
        let Some(bytes) = bytes else {
            return false;
        };
        self.outstanding.iter().any(|request| match request {
            Outstanding::Document(flight) => flight.bytes == *bytes,
            Outstanding::Read { .. } => false,
        })
    }

    /// **The file's bytes stored for the merge**, taken by it.
    pub(crate) fn take_merges(&mut self) -> Vec<PendingMerge<T>> {
        std::mem::take(&mut self.merges)
    }

    /// Whether a file's bytes wait for the merge.
    pub(crate) fn merge_waits(&self) -> bool {
        !self.merges.is_empty()
    }

    /// **Whether a re-read of this file is still to be answered** — one on the
    /// worker, or one asked while it was and owed after it ([`Self::ask_read`]).
    /// The storage pass's card waits while any store is reading
    /// (`MergeNotice::gather`, T-STORE-NOTICE).
    pub(crate) fn reading(&self) -> bool {
        self.read_again
            || self
                .outstanding
                .iter()
                .any(|request| matches!(request, Outstanding::Read { .. }))
    }

    /// **The merge's seam: every file stored for the merge, merged and
    /// folded**, in the order the store kept them ((c)1's table).
    ///
    /// For each, `merge(base, ours, theirs)` answers the merged document and
    /// what it has to say about it:
    /// - `base` is the revision the file was compared with — for a displaced
    ///   file, the revision before the exchange ((b)3);
    /// - `ours` is `desired` — for a displaced file, the document written; the
    ///   presses that arrived after it was sent are then laid over the merged
    ///   document by their unit delta ((c)1), neither lost nor counted twice;
    /// - `theirs` is the file's document; no file is the default document.
    ///
    /// Each result is folded ([`Self::fold`]). A file whose bytes will not
    /// parse is not merged (§3.3): a file that moved stays stored, and the
    /// store stays held, until a later read finds it parsing; a displaced copy
    /// that will not parse stays on disk beside the file under its own name,
    /// said once in `stderr`, never merged and never removed — like a
    /// `.rejected-` copy — and with nothing of it left to merge, `desired` and
    /// the revision (the document written, (c)1) stay as they are. **The hold
    /// is released whenever nothing waits for the merge**, so the next trigger
    /// writes conditionally on what the file is known to hold.
    pub(crate) fn merge_stored<R>(
        &mut self,
        mut merge: impl FnMut(&T, &T, &T) -> (T, R),
    ) -> Vec<R> {
        let file = self.file;
        let mut said = Vec::new();
        for pending in self.take_merges() {
            let theirs = match &pending.theirs {
                None => T::default(),
                Some(bytes) => match T::from_bytes(bytes) {
                    Ok(document) => document,
                    Err(reason) => {
                        match &pending.kept {
                            Some(kept) => eprintln!(
                                "BT_PERSIST {file}: the edit kept as {} would not parse \
                                 ({reason:?}); it stays beside the file and is not merged",
                                kept.display()
                            ),
                            None => self.merges.push(pending),
                        }
                        continue;
                    }
                },
            };
            let ours = pending
                .written
                .clone()
                .unwrap_or_else(|| self.desired.clone());
            let (mut merged, about) = merge(&pending.base.document, &ours, &theirs);
            if let Some(written) = &pending.written {
                T::apply_press(&mut merged, written, &self.desired);
            }
            self.fold(pending, merged, theirs);
            said.push(about);
        }
        self.held_for_merge = !self.merges.is_empty();
        said
    }

    /// **A merge folded a file's bytes into `desired`** ((c)1, (d)1): `merged`
    /// is the new `desired`, at a new version. A kept file the merge took its
    /// units from is retired by a landing of this version or a later one. A
    /// file that moved (not a displaced copy) is what the file holds: it becomes
    /// the revision, as `theirs` parsed. With nothing left for the merge, the
    /// hold is released, and the next trigger writes `desired` conditional on
    /// what the file is known to hold.
    pub(crate) fn fold(&mut self, merge: PendingMerge<T>, merged: T, theirs: T) {
        self.desired = merged;
        self.version += 1;
        if self.merges.is_empty() {
            // Nothing is left for the merge: what was held is resolved.
            self.held_for_merge = false;
        }
        match merge.kept {
            Some(path) => self.kept.push(KeptFile {
                path,
                version: self.version,
            }),
            None => {
                let bytes = match merge.theirs {
                    Some(bytes) => bt_persist::DocumentBytes::Loaded(bytes),
                    None => bt_persist::DocumentBytes::Absent,
                };
                self.revision.adopt(theirs, bytes);
            }
        }
    }

    /// Take the sentence a refused write owes the reader.
    pub(crate) fn take_fault(&mut self) -> Option<String> {
        self.writes.take_fault()
    }

    /// **Whether this run owes the file a write it may make**: [`Self::owed`],
    /// and no update's trial holding its writes back.
    fn owed_now(&self) -> bool {
        self.owed() && !crate::update_trial::defer(self.deferral)
    }
}

/// **A hand-editable document as a quit's documents and recovery phases, and a
/// stop that cannot ask, see it** (design note 2026-10-11, revisions (c)2, (d)2
/// and (d)3). Each of the four stores answers for its own document; the quit
/// asks them all the same questions in the same turn.
pub(crate) trait QuitDocument {
    /// The file's name, as the card and `diagnostics.log` say it.
    fn file(&self) -> &'static str;
    /// **The documents phase begins**: a quit is a trigger an owed edit is
    /// retried on (§4.1), so the owed document is handed over once more, whatever
    /// an earlier write was answered, and nothing of an earlier quit's copy is
    /// kept.
    fn quit_begin(&mut self);
    /// **One turn of the documents phase**: what the storage worker answered is
    /// taken, a file a merge can fold is merged (purely — no door runs, the
    /// windows are leaving), and what is owed with nothing on the worker is sent.
    /// Answers the `diagnostics.log` line of what this turn merged, if it merged
    /// anything — said once, whether the quit completes or is cancelled.
    fn quit_turn(&mut self) -> Option<String>;
    /// Where the document stands ([`Standing`]).
    fn standing(&self) -> Standing;
    /// **Hand the owed document to the storage worker as a copy into `folder`**,
    /// once per quit.
    fn keep_copy(&mut self, folder: &Path, at: SystemTime);
    /// Where the copy stands ([`CopyStanding`]); `Waiting` before it is sent.
    fn copy_standing(&mut self) -> CopyStanding;
    /// **Drop the owed change for this run** — the person chose to leave without
    /// it — and answer the `diagnostics.log` line that names the file and its
    /// units, or `None` when nothing was owed.
    fn drop_owed(&mut self) -> Option<String>;
    /// **The owed document's bytes**, for a stop that cannot ask: `None` when
    /// nothing is owed, `Err` when the document would not serialize.
    fn owed_bytes(&self) -> Option<Result<Vec<u8>, String>>;
}

impl<T: StoredDocument> QuitDocument for DocumentState<T> {
    fn file(&self) -> &'static str {
        self.file
    }

    fn quit_begin(&mut self) {
        self.refusal = None;
        self.recovery = None;
        // A write already on the worker is answered first: its answer decides.
        if self.standing() == Standing::Unsent {
            self.send_owed();
        }
    }

    fn quit_turn(&mut self) -> Option<String> {
        let _ = self.take_answers();
        if self.standing() == Standing::Unsent {
            self.send_owed();
        }
        None
    }

    fn standing(&self) -> Standing {
        if !self.owed_now() {
            return Standing::Settled;
        }
        if self
            .outstanding
            .iter()
            .any(|request| matches!(request, Outstanding::Document(_)))
        {
            return Standing::Waiting;
        }
        if self.held_for_merge || !self.merges.is_empty() {
            let unparsable = self.merges.iter().any(|merge| {
                merge
                    .theirs
                    .as_deref()
                    .is_some_and(|bytes| T::from_bytes(bytes).is_err())
            });
            return Standing::Terminal(if unparsable {
                "it changed on disk, and what it holds now will not parse".to_owned()
            } else {
                "it changed on disk, and the change made here waits for a merge".to_owned()
            });
        }
        if let Some(refusal) = &self.refusal {
            return Standing::Terminal(format!("its write was refused: {refusal}"));
        }
        Standing::Unsent
    }

    fn keep_copy(&mut self, folder: &Path, at: SystemTime) {
        if self.recovery.is_some() {
            return;
        }
        let bytes = match self.desired.to_bytes() {
            Ok(bytes) => bytes,
            Err(error) => {
                self.recovery = Some(RecoveryCopy::Answered(Err(error)));
                return;
            }
        };
        let (answer, answered) = mpsc::channel();
        let job = RecoveredJob {
            folder: folder.to_path_buf(),
            name: self.file,
            bytes,
            at,
            answer,
        };
        self.recovery = Some(match self.lane.send_recovered(job) {
            Ok(request) => RecoveryCopy::Sent(OnTheWorker::admitted(answered, request)),
            Err(refused) => {
                RecoveryCopy::Answered(Err(refused.said(crate::persist::NO_STORAGE_WORKER)))
            }
        });
    }

    fn copy_standing(&mut self) -> CopyStanding {
        if let Some(RecoveryCopy::Sent(answer)) = &mut self.recovery {
            let answered = match answer.hear() {
                Reply::Answered(answered) => answered,
                Reply::Waiting => return CopyStanding::Waiting,
                Reply::Failed(words) => Err(words),
                Reply::Gone => Err(crate::persist::STORAGE_WORKER_GONE.to_owned()),
            };
            self.recovery = Some(RecoveryCopy::Answered(answered));
        }
        match &self.recovery {
            Some(RecoveryCopy::Answered(Ok(copy))) => CopyStanding::Kept(copy.clone()),
            Some(RecoveryCopy::Answered(Err(error))) => CopyStanding::Refused(error.clone()),
            Some(RecoveryCopy::Sent(_)) | None => CopyStanding::Waiting,
        }
    }

    fn drop_owed(&mut self) -> Option<String> {
        if !self.owed_now() {
            return None;
        }
        let units = T::units(&self.desired, &self.revision.document);
        self.desired = self.revision.document.clone();
        self.version += 1;
        self.merges.clear();
        self.held_for_merge = false;
        self.send_again = false;
        let what = if units.is_empty() {
            "the whole document".to_owned()
        } else {
            units.join(", ")
        };
        Some(format!(
            "{} quit without a change to {} that could not be saved: {what}",
            crate::APP_NAME,
            self.file
        ))
    }

    fn owed_bytes(&self) -> Option<Result<Vec<u8>, String>> {
        self.owed_now().then(|| self.desired.to_bytes())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, mpsc};

    use super::{DocumentAnswer, DocumentJob, LinkPolicy, Mode, Outcome, ReadJob};
    use crate::persist::{OnTheWorker, Reply, StorageLane};

    /// A write of `bytes` to `which`, answered on `answer`.
    fn write(which: &Path, answer: mpsc::Sender<DocumentAnswer>) -> DocumentJob {
        DocumentJob {
            which: which.to_path_buf(),
            generation: 2,
            version: 1,
            expected: None,
            bytes: Arc::from(&b"{\"\xe5\x86\x99\": 1}"[..]),
            mode: Mode::Restore,
            link_policy: LinkPolicy::Refuse,
            no_exchange: super::NoExchange::Refuse,
            asker: 0,
            kept: Vec::new(),
            answer,
        }
    }

    /// RED (T-STORAGE-WORKER-ON-THE-SEAM, R2; was T-PANIC-ROWS-HONEST's "a storage worker panic
    /// ends every owed request") — **a panic in one storage request is that request's `Failed`,
    /// and the worker goes on.**
    ///
    /// Two requests are admitted: a re-read, whose answer (the store's parse, on the worker)
    /// panics, and behind it a write. The product's panic hook ends the process for a panic it is
    /// not told is contained, so each request runs inside `lane::task`
    /// (`contained_panic::contain`): the hook's decision at the panic is yes, the read ends
    /// `Failed` in its own words, and the worker serves the write behind it, which lands. Each
    /// answer is followed by its wake, and a request made after them is admitted: the worker
    /// is still there. (The worker's own end — every request still owed ending `WorkerGone`,
    /// then one wake — is the lane contract's `ADeadWorkerIsObservable`, run on this lane
    /// through `persist::contract_adapter`.)
    ///
    /// MUTATIONS: ① serve the job without its `lane::task` in `SessionWriter::start`'s loop — the
    /// panic is the worker's end, which wakes the loop once, and the write behind the read is
    /// never answered; ② publish `Done` for a failed task — the read is heard gone (its road
    /// closed, no answer on it), not failed.
    #[test]
    fn a_panic_in_one_storage_request_fails_it_and_the_worker_goes_on() {
        let which = bt_testpath::temp_path("panic-rows-存储");
        let _ = std::fs::remove_file(&which);
        let (woke, wakes) = mpsc::channel::<()>();
        let lane = StorageLane::with_a_worker_and_wake(move || {
            let _ = woke.send(());
        });

        let (told, decision) = mpsc::channel();
        let (open, gate) = mpsc::channel::<bool>();
        let (read_answer, read_answered) = mpsc::channel();
        let admitted = lane
            .send_read(ReadJob {
                which: which.clone(),
                generation: 1,
                answer: Box::new(move |generation, _read| {
                    let _ = told.send(crate::the_hook_lets_this_panic_unwind());
                    if crate::lane::receive(&gate, "the test's word to fail the parse") {
                        panic!("scripted: the document would not parse 解析失败");
                    }
                    let _ = read_answer.send(generation);
                }),
            })
            .expect("the read is admitted");
        let mut read = OnTheWorker::admitted(read_answered, admitted);
        assert!(
            crate::lane::receive(&decision, "the hook's decision inside the read's answer"),
            "the worker serves where the resident hook lets a panic unwind"
        );
        let (write_answer, write_answered) = mpsc::channel::<DocumentAnswer>();
        let admitted = lane
            .send_document(write(&which, write_answer))
            .expect("the write is admitted behind it");
        let mut written = OnTheWorker::admitted(write_answered, admitted);
        open.send(true).expect("the worker waits at the gate");

        crate::lane::wait_for_a_wake(&wakes, "the read's end");
        crate::lane::wait_for_a_wake(&wakes, "the write's answer");
        match read.hear() {
            Reply::Failed(words) => assert!(
                words.contains("scripted: the document would not parse 解析失败"),
                "the read failed in its own words: {words}"
            ),
            heard => panic!("the read is that request's failure, not {heard:?}"),
        }
        match written.hear() {
            Reply::Answered(answer) => assert!(
                matches!(answer.outcome, Outcome::Landed { .. }),
                "the write behind it landed: {:?}",
                answer.outcome
            ),
            heard => panic!("the worker went on to the write, not {heard:?}"),
        }
        assert!(which.exists(), "and the write is on the disk");

        assert!(
            lane.send_read(ReadJob {
                which: which.clone(),
                generation: 3,
                answer: Box::new(|_, _| {}),
            })
            .is_ok(),
            "a request after the failure is admitted: the worker is still there"
        );
        let _ = std::fs::remove_file(&which);
    }
}
