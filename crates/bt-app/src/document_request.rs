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
//! cannot exchange is refused for every request ((b)1). **Per-file order** holds
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
//! Until the merge tickets (T-STORE-MERGE-*), the four stores send `Restore`
//! requests ((b)16 item 1): a crossing hand edit is put back and kept, its
//! bytes are stored for the merge, and the store stays owed against a file it
//! will not write over. Nothing is merged yet, and nothing is lost.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

use bt_persist::{
    FallbackReason, KeybindingsV1, PinsV1, ProfilesV1, SettingsV1, StableRead, Staging,
};
use bt_platform::admission::WorkerCtx;

use crate::persist::{DocumentWrites, Revision, StorageLane};

/// **Which semantic policy a request asks for** ((c)3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The asker merges: a displaced file stays on disk, named in the answer,
    /// for its merge.
    // The four stores join `Merge` with the merge tickets; until then only the
    // tests that drive the merge's bookkeeping send it.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "T-STORAGE-WORKER-REQUEST until 2026-10-31: T-STORE-MERGE-SETTINGS sends it"
        )
    )]
    Merge,
    /// The asker has no merge: a displaced file is put back under its name and
    /// the answer is a conflict.
    Restore,
}

/// **What a request does with a target that is a link** ((c)3 item 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkPolicy {
    /// Refuse: nothing is written ((b)2, owner ruling Q5).
    Refuse,
    /// Write the link's name the plain way, as the preview's save does today.
    /// Honoured for a link only: a volume that cannot exchange is refused
    /// whatever the policy.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "T-STORAGE-WORKER-REQUEST until 2026-10-31: S-6a's preview save sends it"
        )
    )]
    PlainWriter,
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
    /// Opaque to the worker and echoed on the answer: the preview's body
    /// revision, `0` for a store.
    pub(crate) asker: u64,
    /// The kept files the asker holds obligations for; a durable landing of
    /// `version` removes those [`retired_by`] it.
    pub(crate) kept: Vec<KeptFile>,
    pub(crate) answer: mpsc::Sender<DocumentAnswer>,
}

/// **One stable read of a document**: the bytes go to `answer` with the
/// request's generation, on the worker, which parses them there.
pub(crate) struct ReadJob {
    pub(crate) which: PathBuf,
    pub(crate) generation: u64,
    pub(crate) answer: Box<dyn FnOnce(u64, StableRead) + Send>,
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
    /// a later durable landing retires it.
    Landed {
        bytes: Arc<[u8]>,
        kept: Option<PathBuf>,
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
}

impl Refusal {
    /// **Whether this refusal is a fault at its first occurrence** ((b)7): a
    /// link and a volume that cannot exchange do not change by trying again.
    /// Every other refusal counts in the streak.
    pub(crate) fn is_said_at_once(&self) -> bool {
        matches!(self, Self::Link | Self::ExchangeUnavailable(_))
    }

    fn described(&self) -> String {
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
            Self::Disk(error) | Self::NoWorker(error) => error.clone(),
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

/// **Serve one [`DocumentJob`] on the storage worker** and answer it.
pub(crate) fn serve_document(worker: &WorkerCtx, job: DocumentJob) {
    serve_document_between(worker, job, &mut |_| {});
}

/// [`serve_document`], with `between` run at each [`Step`].
pub(crate) fn serve_document_between(
    worker: &WorkerCtx,
    job: DocumentJob,
    between: &mut dyn FnMut(Step),
) {
    let served = conditional_write(worker, &job, between);
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
pub(crate) fn serve_read(worker: &WorkerCtx, job: ReadJob) {
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

/// §3.2's steps 1–4, as revised.
fn conditional_write(
    worker: &WorkerCtx,
    job: &DocumentJob,
    between: &mut dyn FnMut(Step),
) -> Served {
    let read = bt_persist::read_stable(worker, &job.which);
    between(Step::Read);
    match precondition(read, job.expected.as_deref()) {
        Precondition::Holds => {}
        Precondition::Moved(bytes) => return Served::plainly(Outcome::Moved { bytes }),
        Precondition::Raced => return Served::plainly(Outcome::MovedDuringRead),
        Precondition::Unread(error) => {
            return Served::plainly(Outcome::Refused {
                cause: Refusal::Unread(error),
            });
        }
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
        Some(expected) => exchange(worker, job, expected, &temp, between),
    }
}

/// The first file under a name that holds none: a rename that refuses any
/// entry already there, then the directory flushed.
fn first_write(worker: &WorkerCtx, job: &DocumentJob, temp: &Path) -> Served {
    match bt_platform::rename_without_replacing_durably(worker, temp, &job.which) {
        Ok(()) => Served::plainly(Outcome::Landed {
            bytes: Arc::clone(&job.bytes),
            kept: None,
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
    match bt_platform::replace_keeping_displaced(worker, temp, &job.which) {
        Ok(displaced) => settle_exchange(worker, job, expected, &displaced.path, None, between),
        Err(ExchangeRefusal::NotDurable { displaced, error }) => settle_exchange(
            worker,
            job,
            expected,
            &displaced.path,
            Some(error.to_string()),
            between,
        ),
        Err(ExchangeRefusal::Link) => match job.link_policy {
            LinkPolicy::Refuse => refused(Refusal::Link),
            LinkPolicy::PlainWriter => {
                let _ = std::fs::remove_file(temp);
                Served::plainly(
                    match bt_persist::atomic_replace_keeping_metadata(&job.which, &job.bytes) {
                        Ok(()) => Outcome::Landed {
                            bytes: Arc::clone(&job.bytes),
                            kept: None,
                        },
                        Err(error) => Outcome::Refused {
                            cause: Refusal::Disk(error.to_string()),
                        },
                    },
                )
            }
        },
        Err(ExchangeRefusal::ExchangeUnavailable(error)) => {
            refused(Refusal::ExchangeUnavailable(error.to_string()))
        }
        Err(ExchangeRefusal::BackupNameTaken(path)) => refused(Refusal::BackupNameTaken(path)),
        Err(ExchangeRefusal::Failed(error)) => refused(Refusal::Disk(error.to_string())),
    }
}

/// **§3.2 step 4: what the exchange displaced, read and judged.** The bytes
/// the precondition expected are Folio's own and go — unless the exchange is not
/// known to be durable, when the displaced copy is the only durable copy of the
/// file's last state and stays, retired by a later landing (carry-forward 5).
/// Anything else is a hand edit that crossed: kept for a merge, or put back.
pub(crate) fn settle_exchange(
    worker: &WorkerCtx,
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
            },
            not_durable,
            left: None,
        };
    }
    let outcome = match job.mode {
        Mode::Merge => Outcome::Displaced {
            bytes: theirs,
            kept: displaced.to_path_buf(),
        },
        Mode::Restore => restore(worker, job, displaced, theirs),
    };
    Served {
        outcome,
        not_durable,
        left: None,
    }
}

/// **`Restore`'s second exchange** ((c)3): only while the file still holds what
/// was written, the displaced file goes back under its name. What that exchange
/// displaces in turn is Folio's own write, and goes; anything else, or a second
/// exchange that is refused, leaves both files and names the one beside it.
fn restore(worker: &WorkerCtx, job: &DocumentJob, displaced: &Path, theirs: Arc<[u8]>) -> Outcome {
    let crossed = |kept: &Path| Outcome::Conflict {
        theirs: Arc::clone(&theirs),
        kept: Some(kept.to_path_buf()),
    };
    match bt_persist::read_stable(worker, &job.which) {
        StableRead::Bytes(bytes) if *bytes == *job.bytes => {}
        _ => return crossed(displaced),
    }
    let back = match bt_platform::replace_keeping_displaced(worker, displaced, &job.which) {
        Ok(back)
        | Err(bt_platform::ExchangeRefusal::NotDurable {
            displaced: back, ..
        }) => back,
        Err(_) => return crossed(displaced),
    };
    match bt_persist::read_stable(worker, &back.path) {
        StableRead::Bytes(bytes) if *bytes == *job.bytes => {
            let _ = std::fs::remove_file(&back.path);
            Outcome::Conflict {
                theirs: Arc::clone(&theirs),
                kept: None,
            }
        }
        _ => crossed(&back.path),
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

/// Which of several entries under one key a reader puts in force.
#[derive(Clone, Copy)]
enum InForce {
    First,
    Last,
}

/// **A press's delta over a list of entries named by a key** ((b)6, (b)8's
/// units): every key whose entry in force differs between `before` and `after`
/// takes `after`'s in `desired` — replaced where `desired` has it in force,
/// added after its nearest predecessor in `after` otherwise, removed when
/// `after` has none — and, when the press reordered the keys both sides hold,
/// those entries take `after`'s order in the places they hold in `desired`.
/// Entries `desired` carries that the projection never had (a line a reader
/// refused, a shadowed duplicate) keep their place.
fn apply_keyed_press<E, K>(
    desired: &mut Vec<E>,
    before: &[E],
    after: &[E],
    key: impl Fn(&E) -> K,
    in_force: InForce,
) where
    E: Clone + PartialEq,
    K: PartialEq + Clone,
{
    if desired.as_slice() == before {
        *desired = after.to_vec();
        return;
    }
    let position = |list: &[E], wanted: &K| -> Option<usize> {
        let mut found = list
            .iter()
            .enumerate()
            .filter(|(_, entry)| key(entry) == *wanted);
        match in_force {
            InForce::First => found.next(),
            InForce::Last => found.last(),
        }
        .map(|(index, _)| index)
    };
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
            (Some(at), None) => {
                desired.remove(at);
            }
            (None, Some(entry)) => {
                let at = position(after, name).unwrap_or(0);
                let anchor = after[..at]
                    .iter()
                    .rev()
                    .find_map(|earlier| position(desired, &key(earlier)));
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

    /// The unit is the action, the last line about it in force (the reader's
    /// rule). A press in another platform's dialect than `desired`'s is the
    /// whole document: its lines cannot share one `writing_platform`, and their
    /// translation is the keybindings merge's (T-STORE-MERGE-KEYBINDINGS).
    fn apply_press(desired: &mut Self, before: &Self, after: &Self) {
        if desired == before || desired.writing_platform != after.writing_platform {
            *desired = after.clone();
            return;
        }
        desired.schema_version = after.schema_version;
        apply_keyed_press(
            &mut desired.bindings,
            &before.bindings,
            &after.bindings,
            |binding| binding.action.clone(),
            InForce::Last,
        );
    }
}

impl StoredDocument for ProfilesV1 {
    fn to_bytes(&self) -> Result<Vec<u8>, String> {
        bt_persist::serialize_profiles(self).map_err(|error| error.to_string())
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, FallbackReason> {
        bt_persist::parse_profiles(bytes)
    }

    /// The unit is the row, by id, the first in force (the installer's rule).
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
            InForce::First,
        );
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
            InForce::First,
        );
    }
}

/// **What a stable read came to, parsed on the worker.**
#[derive(Debug)]
pub(crate) enum ReadAnswer<T> {
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
    /// A read put a new document in force (the projection moved).
    pub(crate) taken: bool,
    /// Whether the newest read found a file that would not read — `None` when
    /// no read was answered. A stale read's verdict is not said over a newer
    /// one's.
    pub(crate) unreadable: Option<bool>,
}

/// **One re-read's news, as a test names it.**
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// The file is the one Folio left, or holds the document in force, or a
    /// change made here kept it from being taken.
    Unchanged,
    /// A new document is in force.
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
        } else if self.taken {
            Heard::Changed
        } else {
            Heard::Unchanged
        }
    }
}

/// **A file's bytes, kept for the merge** (T-STORE-MERGE-*): a `Moved`,
/// `Displaced` or `Conflict` answer, or a re-read that found the file changed
/// while a change of Folio's was not yet on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingMerge<T> {
    /// The revision the file was compared with — the merge's base: for a
    /// `Displaced` answer, the revision before the exchange ((b)3).
    pub(crate) base: Revision<T>,
    /// What the file held; `None`: no file.
    pub(crate) theirs: Option<Arc<[u8]>>,
    /// Where the person's bytes are kept beside the file (`Displaced`).
    pub(crate) kept: Option<PathBuf>,
}

/// One request a store has on the worker.
enum Outstanding<T> {
    Document(InFlight<T>),
    Read {
        answer: mpsc::Receiver<(u64, ReadAnswer<T>)>,
    },
}

/// The one document write a store has in flight.
struct InFlight<T> {
    generation: u64,
    document: T,
    bytes: Arc<[u8]>,
    /// The revision the write is conditional on.
    base: Revision<T>,
    answer: mpsc::Receiver<DocumentAnswer>,
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
    merges: Vec<PendingMerge<T>>,
    kept: Vec<KeptFile>,
    writes: DocumentWrites,
    writer_of_record: bool,
    deferral: crate::update_trial::Writer,
    lane: StorageLane,
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
            mode: Mode::Restore,
            outstanding: VecDeque::new(),
            read_again: false,
            send_again: false,
            merges: Vec::new(),
            kept: Vec::new(),
            writes: DocumentWrites::new(),
            writer_of_record,
            deferral,
            lane,
        }
    }

    /// The document readers read.
    pub(crate) fn projection(&self) -> &T {
        &self.projection
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
            self.writes.rearm();
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
    /// processed. A worker that cannot take the request is a refusal, said now.
    pub(crate) fn send_owed(&mut self) {
        if !self.writer_of_record || crate::update_trial::defer(self.deferral) {
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
        let (answer, answered) = mpsc::channel();
        let job = DocumentJob {
            which: self.path.clone(),
            generation: self.generation,
            version: self.version,
            expected: self.revision.bytes.clone(),
            bytes: Arc::clone(&bytes),
            mode: self.mode,
            link_policy: LinkPolicy::Refuse,
            asker: 0,
            kept: self.kept.clone(),
            answer,
        };
        if !self.lane.send_document(job) {
            self.refused(&Refusal::NoWorker(
                crate::persist::NO_STORAGE_WORKER.to_owned(),
            ));
            return;
        }
        self.outstanding.push_back(Outstanding::Document(InFlight {
            generation: self.generation,
            document: self.desired.clone(),
            bytes,
            base: self.revision.clone(),
            answer: answered,
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
        let job = ReadJob {
            which: self.path.clone(),
            generation: self.generation,
            answer: Box::new(move |generation, read| {
                let _ = answer.send((generation, ReadAnswer::<T>::of(read)));
            }),
        };
        if !self.lane.send_read(job) {
            eprintln!(
                "BT_PERSIST {} was not read again: {}",
                self.file,
                crate::persist::NO_STORAGE_WORKER
            );
            return;
        }
        self.outstanding
            .push_back(Outstanding::Read { answer: answered });
    }

    /// **Take every answer the worker has given, in the order it served them**,
    /// and send what is owed after them. Never waits.
    pub(crate) fn take_answers(&mut self) -> ReadNews {
        let mut news = ReadNews::default();
        let mut landed = false;
        while let Some(request) = self.outstanding.front() {
            let answered = match request {
                Outstanding::Document(flight) => match flight.answer.try_recv() {
                    Ok(answer) => Some(Ok(answer)),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some(Err(())),
                },
                Outstanding::Read { answer } => match answer.try_recv() {
                    Ok((generation, read)) => {
                        self.outstanding.pop_front();
                        self.on_read(generation, read, &mut news);
                        continue;
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
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
        self.after_answers(landed);
        news
    }

    /// Wait for the answer to every request this store has on the worker now,
    /// then take them, and send what is owed after them — a test's way to let
    /// the worker finish. A request sent after them is not waited for.
    #[cfg(test)]
    pub(crate) fn settle(&mut self) -> ReadNews {
        let mut news = ReadNews::default();
        let mut landed = false;
        for _ in 0..self.outstanding.len() {
            match self.outstanding.pop_front() {
                Some(Outstanding::Document(flight)) => {
                    let answer = flight.answer.recv().map_err(|_| ());
                    landed |= self.on_document(flight, answer);
                }
                Some(Outstanding::Read { answer }) => {
                    if let Ok((generation, read)) = answer.recv() {
                        self.on_read(generation, read, &mut news);
                    }
                }
                None => break,
            }
        }
        self.after_answers(landed);
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
    fn after_answers(&mut self, landed: bool) {
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
        if !writing && (self.send_again || (landed && self.owed())) {
            self.send_owed();
        }
    }

    /// **One write's answer** — every one is processed, whatever its generation
    /// ((b)4). Answers whether it landed.
    fn on_document(&mut self, flight: InFlight<T>, answer: Result<DocumentAnswer, ()>) -> bool {
        let file = self.file;
        let Ok(answer) = answer else {
            self.refused(&Refusal::NoWorker(
                crate::persist::STORAGE_WORKER_GONE.to_owned(),
            ));
            return false;
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
            Outcome::Landed { bytes, kept } => {
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
                self.keep_for_the_merge(PendingMerge {
                    base: flight.base,
                    theirs: bytes,
                    kept: None,
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
                });
                self.revision.landed(flight.bytes, flight.document);
                self.writes.record(file, Ok(()));
                return true;
            }
            Outcome::Conflict { theirs, kept } => {
                eprintln!(
                    "BT_PERSIST {file} was edited while it was written; the edit was put back{}",
                    kept.as_ref().map_or_else(String::new, |kept| format!(
                        " and {} is left beside it",
                        kept.display()
                    ))
                );
                self.keep_for_the_merge(PendingMerge {
                    base: flight.base,
                    theirs: Some(theirs),
                    kept: None,
                });
            }
            Outcome::Refused { cause } => self.refused(&cause),
        }
        false
    }

    /// A refused write: the streak, or at once ((b)7).
    fn refused(&mut self, cause: &Refusal) {
        let error = cause.described();
        if cause.is_said_at_once() {
            self.writes.refused_for_good(self.file, error);
        } else {
            self.writes.record(self.file, Err(error));
        }
    }

    /// Store a file's bytes for the merge. A newer file with no kept copy
    /// replaces an older one: the merge reconciles what the file holds now.
    fn keep_for_the_merge(&mut self, merge: PendingMerge<T>) {
        if merge.kept.is_none()
            && let Some(last) = self.merges.last_mut()
            && last.kept.is_none()
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
            ReadAnswer::MovedDuringRead => {
                eprintln!(
                    "BT_PERSIST {file} changed while it was read (generation {generation}); the                      next pass reads it again"
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
                return;
            }
            ReadAnswer::Absent => (None, T::default()),
            ReadAnswer::Parsed { bytes, document } => (Some(bytes), document),
        };
        news.unreadable = Some(false);
        if self.is_folios_own_write(bytes.as_ref()) {
            return;
        }
        if self.desired != self.revision.document {
            // A change of Folio's is not on disk — in flight, refused, or a
            // process that does not write here — and a re-read does not undo it.
            // The file's bytes wait for the merge.
            eprintln!(
                "BT_PERSIST {file} changed on disk while a change made here is not on it; the \
                 change made here stays"
            );
            self.keep_for_the_merge(PendingMerge {
                base: self.revision.clone(),
                theirs: bytes,
                kept: None,
            });
            return;
        }
        let read_bytes = match bytes {
            Some(bytes) => bt_persist::DocumentBytes::Loaded(bytes),
            None => bt_persist::DocumentBytes::Absent,
        };
        if self.revision.adopt(document.clone(), read_bytes) {
            self.projection = document.clone();
            self.desired = document;
            self.version += 1;
            self.merges.clear();
            news.taken = true;
        }
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "T-STORAGE-WORKER-REQUEST until 2026-10-31: T-STORE-MERGE-SETTINGS takes them"
        )
    )]
    pub(crate) fn take_merges(&mut self) -> Vec<PendingMerge<T>> {
        std::mem::take(&mut self.merges)
    }

    /// **A merge folded a file's bytes into `desired`** ((c)1, (d)1): `merged`
    /// is the new `desired`, at a new version. A kept file the merge took its
    /// units from is retired by a landing of this version or a later one. A
    /// file that moved (not a displaced copy) is what the file holds: it becomes
    /// the revision, as `theirs` parsed.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "T-STORAGE-WORKER-REQUEST until 2026-10-31: T-STORE-MERGE-SETTINGS folds"
        )
    )]
    pub(crate) fn fold(&mut self, merge: PendingMerge<T>, merged: T, theirs: T) {
        self.desired = merged;
        self.version += 1;
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
}
