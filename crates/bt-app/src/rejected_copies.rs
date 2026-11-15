//! **The copies of the data folder's documents that a read refused, and the carry pass's
//! retirement of the old ones** (0.4.10 plan row 68f; R9 of 2026-11-04, ruled yes by the owner on
//! 2026-11-14: keep the newest three copies of each document and remove the older ones; rulings
//! R1-R3 and R11 of 2026-11-14).
//!
//! A document this build will not read — damaged, newer than this build, too large — is kept
//! beside itself before anything can replace it, as `<document>.rejected-<YYYYMMDD-HHMMSS>[-n]`
//! (`bt_persist`'s keeping, `rejected_sibling`). Until this part nothing read those copies again
//! or retired them, and a file an older build cannot read is refused, and copied, at every start
//! until it is written: a data folder gathered one copy a start.
//!
//! **The member** ([`run_rejected_copies_step`], `CarryAsk::REJECTED_COPIES`, after the sign-in
//! entry and before the agents' hooks): in the data folder, and only in the process that writes
//! it, for each of the six documents a read keeps a copy of ([`KeptDocument`]):
//!
//! 1. **Which copies count**: plain files — no folder, no link — whose names are exactly the ones
//!    the namer gives (`bt_persist::rejected_stamp_of`). Every other name, however close, a
//!    `.displaced-` copy (the storage worker's own leftover, R3), and anything another program
//!    put there is never read and never removed.
//! 2. **Of copies holding the same bytes, the newest is kept** and the older ones are removed
//!    (R2): nothing is lost, and "three" is three different refusals.
//! 3. **The newest three of the rest are kept**, by their stamp and then their count, and the
//!    older ones removed, oldest first.
//! 4. **Never the copy a newer build could bring back** ([`copy_a_newer_build_can_bring_back`]):
//!    the newest copy whose version is above the version of the file this start read and which
//!    this build reads, kept even when it is the fourth. The next part (68d) offers it back.
//!
//! **And what the window is told** (row 68d, R4 and R5 of 2026-11-14), decided per document
//! before anything is removed and handed to the window in the round's part
//! ([`crate::copy_back::CopyBackAnswer`]):
//!
//! - **the notice**: this start refused a newer file and kept it as a copy, and no other copy of
//!   the document holds those bytes — a refusal an earlier copy already holds was said when that
//!   copy was made, and the older copy goes as a duplicate (the copies are the memory: "once"
//!   needs no record);
//! - **the offer**: the copy a newer build could bring back (step 4), for each document that has
//!   one — the window asks once for all of them.
//!
//! Each removal is `remove_file` on that one file. A copy that will not go is left and said; the
//! others go on. **During an update's trial nothing is removed**: the trial's gate is asked for
//! [`Writer::RefusedCopies`] once a round, and only when a removal is due, and the commit's
//! release of that writer keeps the copies the trial owed first and then asks the pass again.
//!
//! **One diagnostics line per document when what it did changes**: `BT_REJECTED_COPIES
//! <document> kept=<n> removed=<copy>[,<copy>…]`, `BT_REJECTED_COPIES <document> left <copy>
//! (<error kind>)`, `BT_REJECTED_COPIES <document> held`, `BT_REJECTED_COPIES <document> said
//! <copy>`, `BT_REJECTED_COPIES <document> offered <copy> v<copy's> over v<file's>`, or
//! `BT_REJECTED_COPIES unlisted (<error kind>)` for a folder that will not list. A line names copies by their own names, never a path.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use bt_persist::RejectedStamp;
use bt_platform::admission::WorkerCtx;

use crate::carry_pass::MemberAnswer;
use crate::copy_back::{CopyBackAnswer, CopyOffered, NewerKept};
use crate::update_orphans::OrphansSaid;
use crate::update_trial::Writer;

/// **How many copies of each document are kept** (R9 of 2026-11-14): the newest three. Held
/// equal to the inventory's declaration (`CarryRule::KeptNewest`'s `keep`) by
/// `uninstall_tests::every_owned_thing_says_how_an_update_carries_it`.
pub(crate) const KEPT_COPIES: usize = 3;

/// **A document of the data folder whose refused bytes a read keeps** — the six read through
/// `bt_persist`'s fallback chain. The two integration records and the update-homes record are
/// refused and never written, so they leave no copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeptDocument {
    Session,
    Settings,
    Keybindings,
    Profiles,
    Pins,
    UpdateCheck,
}

impl KeptDocument {
    /// Every one, in the order their lines are said.
    pub(crate) const ALL: [Self; 6] = [
        Self::Session,
        Self::Settings,
        Self::Keybindings,
        Self::Profiles,
        Self::Pins,
        Self::UpdateCheck,
    ];

    /// **The live document whose file is named `file`**, as a store names its file to
    /// `read_fault`: one of the five a build writes at a version ([`Self::current_version`]),
    /// never `update-check.json`.
    pub(crate) fn live_named(file: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|document| {
            document.kept_file_name() == file && document.current_version().is_some()
        })
    }

    /// The document's file name in the data folder, which its copies' names begin with.
    pub(crate) const fn kept_file_name(self) -> &'static str {
        match self {
            Self::Session => crate::persist::SESSION_FILE_NAME,
            Self::Settings => crate::persist::SETTINGS_FILE_NAME,
            Self::Keybindings => crate::persist::KEYBINDINGS_FILE_NAME,
            Self::Profiles => crate::persist::PROFILES_FILE_NAME,
            Self::Pins => crate::pins::PINS_FILE_NAME,
            Self::UpdateCheck => crate::update::STATE_FILE_NAME,
        }
    }

    /// **The `schema_version` this build writes the document at**, the highest its chain reads;
    /// `None` for `update-check.json`, whose copy no build offers back (its loss is a skipped
    /// release tag).
    const fn current_version(self) -> Option<u32> {
        match self {
            Self::Session => Some(bt_persist::SESSION_SCHEMA_VERSION),
            Self::Settings => Some(bt_persist::SETTINGS_SCHEMA_VERSION),
            Self::Keybindings => Some(bt_persist::KEYBINDINGS_SCHEMA_VERSION),
            Self::Profiles => Some(bt_persist::PROFILES_SCHEMA_VERSION),
            Self::Pins => Some(bt_persist::PINS_SCHEMA_VERSION),
            Self::UpdateCheck => None,
        }
    }

    /// **Whether this build's chain reads `bytes` as this document** — a copy a newer build
    /// could bring back. Never `update-check.json`'s: no build offers its copy back (its loss is a
    /// skipped release tag). `update::OfferState::load_for` reads that file at start, but it is
    /// not a store and its read is not noted here.
    fn this_build_reads(self, bytes: &[u8]) -> bool {
        match self {
            Self::Session => bt_persist::parse_session(bytes).is_ok(),
            Self::Settings => bt_persist::parse_settings(bytes).is_ok(),
            Self::Keybindings => bt_persist::parse_keybindings(bytes).is_ok(),
            Self::Profiles => bt_persist::parse_profiles(bytes).is_ok(),
            Self::Pins => bt_persist::parse_pins(bytes).is_ok(),
            Self::UpdateCheck => false,
        }
    }
}

/// **What a store read at this start, as the pass needs it**: the bytes until the pass's worker
/// has read their version, then the version alone (review N8: the version is all 68f and 68d
/// need, and the bytes are let go at the first round that reads it). `Version(None)` when there
/// was no file, no bytes, or bytes that claim no version.
#[derive(Clone, Debug)]
pub(crate) enum StartRead {
    Bytes(Arc<[u8]>),
    Version(Option<u32>),
}

/// **One document's start, as the pass reads it**: what its store read, and this start's refusal
/// of a newer file — the copy it kept, and whether this process has answered it yet.
#[derive(Clone, Debug)]
pub(crate) struct OpenedDocument {
    pub(crate) document: KeptDocument,
    pub(crate) read: StartRead,
    /// The copy this start kept of a file newer than this build (`FutureSchemaVersion`), by
    /// its name; `None` when this start refused no newer file or kept no copy of it.
    pub(crate) refused_newer: Option<String>,
    /// Whether this process has decided the notice for [`Self::refused_newer`] — said it, or
    /// found it said — so a later round of the same process does not say it again.
    pub(crate) refusal_answered: bool,
}

impl OpenedDocument {
    /// What `document`'s store read: `bytes`, or nothing.
    pub(crate) fn read(document: KeptDocument, bytes: Option<Arc<[u8]>>) -> Self {
        Self {
            document,
            read: bytes.map_or(StartRead::Version(None), StartRead::Bytes),
            refused_newer: None,
            refusal_answered: false,
        }
    }

    /// **The version this start read**, reading it from the bytes once and keeping only it.
    fn version(&mut self) -> Option<u32> {
        let version = match &self.read {
            StartRead::Bytes(bytes) => bt_persist::schema_version_of(bytes),
            StartRead::Version(version) => *version,
        };
        self.read = StartRead::Version(version);
        version
    }
}

/// **What each document's store read at this start** ([`OpenedDocument`]). Written by each
/// store's `open` on the window thread — an `Arc` clone or a name under a lock, nothing read —
/// before the start begins the carry pass; read, and its versions and answers kept, by the
/// pass's worker.
static OPENED_AT_START: Mutex<Vec<OpenedDocument>> = Mutex::new(Vec::new());

/// `document`'s entry in `opened`, made when there is none.
fn opened_entry(opened: &mut Vec<OpenedDocument>, document: KeptDocument) -> &mut OpenedDocument {
    let index = match opened.iter().position(|entry| entry.document == document) {
        Some(index) => index,
        None => {
            opened.push(OpenedDocument::read(document, None));
            opened.len() - 1
        }
    };
    &mut opened[index]
}

/// **A store's open says what it read** ([`OPENED_AT_START`]); a refusal it noted stays.
pub(crate) fn note_opened_document(document: KeptDocument, bytes: Option<Arc<[u8]>>) {
    let mut opened = OPENED_AT_START
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    opened_entry(&mut opened, document).read =
        bytes.map_or(StartRead::Version(None), StartRead::Bytes);
}

/// **A store's open refused a file newer than this build and kept it as `copy`** (row 68d, R4):
/// the member says so once ([`newer_refusal_to_say`]); the start raises nothing for it.
pub(crate) fn note_refused_newer(document: KeptDocument, copy: String) {
    let mut opened = OPENED_AT_START
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let entry = opened_entry(&mut opened, document);
    entry.refused_newer = Some(copy);
    entry.refusal_answered = false;
}

/// **A round's reading of the cell kept**: each document's version in place of its bytes, and
/// the answers given, for an entry whose refusal is still the one the round read.
fn settle_opened(read: &[OpenedDocument]) {
    let mut opened = OPENED_AT_START
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    for after in read {
        if let Some(entry) = opened
            .iter_mut()
            .find(|entry| entry.document == after.document)
        {
            if let (StartRead::Bytes(_), StartRead::Version(version)) = (&entry.read, &after.read) {
                entry.read = StartRead::Version(*version);
            }
            if entry.refused_newer == after.refused_newer {
                entry.refusal_answered |= after.refusal_answered;
            }
        }
    }
}

/// One copy found: its document, where it stands, its name and path, and its length.
struct FoundCopy {
    document: KeptDocument,
    stamp: RejectedStamp,
    name: String,
    path: PathBuf,
    length: u64,
}

/// **The copies in `folder`**, by their names and as plain files; nothing when the folder is not
/// there (a first start, or a trial that has not made it). On a worker: the carry pass's.
fn rejected_copies_listed(_worker: &WorkerCtx, folder: &Path) -> io::Result<Vec<FoundCopy>> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some((document, stamp)) = KeptDocument::ALL.into_iter().find_map(|document| {
            bt_persist::rejected_stamp_of(name, document.kept_file_name())
                .map(|stamp| (document, stamp))
        }) else {
            continue;
        };
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && !bt_platform::cleanup::is_link(&meta) => {
                found.push(FoundCopy {
                    document,
                    stamp,
                    name: name.to_owned(),
                    length: meta.len(),
                    path,
                });
            }
            _ => {}
        }
    }
    Ok(found)
}

/// **One document's copies after a round**: how many are kept, the ones removed and the ones
/// that would not go, by name, and whether the trial held the round's removals.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CopiesRetired {
    pub(crate) document: KeptDocument,
    pub(crate) kept: usize,
    pub(crate) removed: Vec<String>,
    pub(crate) left: Vec<(String, io::ErrorKind)>,
    pub(crate) held: bool,
    /// This start's refusal of a newer file, when it is said now (row 68d, R4).
    pub(crate) said: Option<NewerKept>,
    /// The copy a newer build could bring back, offered (row 68d, R5).
    pub(crate) offered: Option<CopyOffered>,
}

impl CopiesRetired {
    /// The document's lines this round; none when nothing was due.
    pub(crate) fn copies_lines(&self) -> Vec<String> {
        let document = self.document.kept_file_name();
        let mut lines = Vec::new();
        if self.held {
            lines.push(format!("BT_REJECTED_COPIES {document} held"));
        }
        if !self.removed.is_empty() {
            lines.push(format!(
                "BT_REJECTED_COPIES {document} kept={} removed={}",
                self.kept,
                self.removed.join(",")
            ));
        }
        for (name, kind) in &self.left {
            lines.push(format!(
                "BT_REJECTED_COPIES {document} left {name} ({kind:?})"
            ));
        }
        if let Some(said) = &self.said {
            lines.push(format!("BT_REJECTED_COPIES {document} said {}", said.copy));
        }
        if let Some(offered) = &self.offered {
            lines.push(format!(
                "BT_REJECTED_COPIES {document} offered {} v{} over v{}",
                offered.copy, offered.copy_version, offered.file_version
            ));
        }
        lines
    }
}

/// **The trial's gate as the retirement asks it**: once a round, at the first removal that is
/// due.
struct CopiesGate<D: FnOnce(Writer) -> bool> {
    defer: Option<D>,
    held: bool,
}

impl<D: FnOnce(Writer) -> bool> CopiesGate<D> {
    /// Whether a removal that is due may be made now.
    fn may_remove(&mut self) -> bool {
        if let Some(defer) = self.defer.take() {
            self.held = defer(Writer::RefusedCopies);
        }
        !self.held
    }
}

/// **The copy a newer build could bring back**, as an index into `copies` (newest first): the
/// newest whose `schema_version` is above the one `start` claims — the bytes the document's store
/// read at this start — and which this build reads (so at most this build's version). `None` when
/// the start read no bytes, or bytes that claim no version.
///
/// Only a downgrade makes one: a copy above the file's version exists when an older build refused
/// a newer file and then wrote its own. A normal upgrade never does, and a start at this build's
/// own version or above (`current_version`) answers `None` before any copy is read: no copy this
/// build reads stands above it, so the ordinary start reads nothing.
fn copy_a_newer_build_can_bring_back(
    document: KeptDocument,
    start: Option<u32>,
    copies: &[FoundCopy],
    bytes: &mut [Option<Option<Vec<u8>>>],
) -> Option<usize> {
    let current = document.current_version()?;
    let start = start?;
    if start >= current {
        return None;
    }
    (0..copies.len()).find(|&index| {
        copy_bytes(copies, bytes, index).is_some_and(|copy| {
            bt_persist::schema_version_of(copy).is_some_and(|version| version > start)
                && document.this_build_reads(copy)
        })
    })
}

/// **The copy only a newer build reads**, as an index into `copies` (newest first): the newest
/// whose `schema_version` is above `current`, this build's own (R9 as the owner read it: the newer
/// build's things are kept). It is the copy 68d offers back when the newer build returns, so it
/// stays whatever the count. Only its version is read, from the envelope; a copy read for it is
/// not kept in `bytes`, so asking holds at most one copy more in memory. `None` for a document no
/// build offers back (`current` is `None`) or when no copy stands above this build.
fn copy_only_a_newer_build_reads(
    copies: &[FoundCopy],
    bytes: &[Option<Option<Vec<u8>>>],
    current: Option<u32>,
) -> Option<usize> {
    let current = current?;
    (0..copies.len()).find(|&index| {
        let version = match &bytes[index] {
            Some(read) => read.as_deref().and_then(bt_persist::schema_version_of),
            None => bt_persist::read_kept_copy(&copies[index].path)
                .as_deref()
                .and_then(bt_persist::schema_version_of),
        };
        version.is_some_and(|version| version > current)
    })
}

/// **The notice this start's refusal of a newer file owes**, once a process (row 68d, R4): the
/// copy the start kept, unless another copy of the document holds the same bytes — then the
/// refusal was said when that copy was made, and the older of the two goes as a duplicate. The
/// entry is answered either way, so a later round of this process says nothing; a copy no longer
/// there (removed by hand) is answered with nothing to say.
fn newer_refusal_to_say(
    entry: &mut OpenedDocument,
    copies: &[FoundCopy],
    bytes: &mut [Option<Option<Vec<u8>>>],
) -> Option<NewerKept> {
    if entry.refusal_answered {
        return None;
    }
    let name = entry.refused_newer.clone()?;
    entry.refusal_answered = true;
    let kept = copies.iter().position(|copy| copy.name == name)?;
    let kept_bytes = copy_bytes(copies, bytes, kept)?.to_vec();
    let said_before = (0..copies.len()).any(|other| {
        other != kept
            && copies[other].length == copies[kept].length
            && copy_bytes(copies, bytes, other) == Some(kept_bytes.as_slice())
    });
    (!said_before).then(|| NewerKept {
        document: entry.document,
        copy: name,
        path: copies[kept].path.clone(),
    })
}

/// `copies[index]`'s bytes, read once a round through `bt_persist`'s bounded read; `None` when
/// they were not obtained.
fn copy_bytes<'a>(
    copies: &[FoundCopy],
    bytes: &'a mut [Option<Option<Vec<u8>>>],
    index: usize,
) -> Option<&'a [u8]> {
    bytes[index]
        .get_or_insert_with(|| bt_persist::read_kept_copy(&copies[index].path))
        .as_deref()
}

/// Why a copy is removed this round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopyRetirement {
    /// A newer copy holds the same bytes.
    Duplicate,
    /// Older than the newest [`KEPT_COPIES`].
    BeyondKept,
}

/// **Which of one document's copies (newest first) go this round, and why**: the older of two
/// holding the same bytes first, then all but the newest [`KEPT_COPIES`] of the rest. Never
/// `protected` (the copy a newer build could bring back, [`copy_a_newer_build_can_bring_back`]),
/// and never the copy only a newer build reads ([`copy_only_a_newer_build_reads`] above
/// `current`), which is looked for only once a copy is otherwise due beyond the kept ones. Neither
/// is ever the older of two equal copies: an equal newer one would answer before it.
fn copies_to_retire(
    copies: &[FoundCopy],
    bytes: &mut [Option<Option<Vec<u8>>>],
    protected: Option<usize>,
    current: Option<u32>,
) -> Vec<Option<CopyRetirement>> {
    let mut fate = vec![None; copies.len()];
    for older in 0..copies.len() {
        if Some(older) == protected {
            continue;
        }
        for newer in 0..older {
            if fate[newer].is_some() || copies[newer].length != copies[older].length {
                continue;
            }
            let Some(newer_bytes) = copy_bytes(copies, bytes, newer).map(<[u8]>::to_vec) else {
                continue;
            };
            if copy_bytes(copies, bytes, older) == Some(newer_bytes.as_slice()) {
                fate[older] = Some(CopyRetirement::Duplicate);
                break;
            }
        }
    }
    let mut survivors = 0;
    let mut newer_builds: Option<Option<usize>> = None;
    for (index, retirement) in fate.iter_mut().enumerate() {
        if retirement.is_some() {
            continue;
        }
        survivors += 1;
        if survivors <= KEPT_COPIES || Some(index) == protected {
            continue;
        }
        let newer_build = *newer_builds
            .get_or_insert_with(|| copy_only_a_newer_build_reads(copies, bytes, current));
        if Some(index) != newer_build {
            *retirement = Some(CopyRetirement::BeyondKept);
        }
    }
    fate
}

/// **One round of the retirement over `folder`** — the data folder's, or a test's own: each
/// document's copies listed, its notice and its offer decided (row 68d), judged
/// ([`copies_to_retire`], against `opened`, what each store read at this start), and removed
/// through `remove`, the duplicates then the rest, oldest first, the trial's gate allowing. One
/// answer per document that has copies; an error when the folder would not list. `opened` keeps
/// the versions read and the notices answered. On a worker: the carry pass's.
pub(crate) fn retire_rejected_copies(
    worker: &WorkerCtx,
    folder: &Path,
    opened: &mut [OpenedDocument],
    defer: impl FnOnce(Writer) -> bool,
    remove: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> Result<Vec<CopiesRetired>, io::ErrorKind> {
    let mut found = rejected_copies_listed(worker, folder).map_err(|error| error.kind())?;
    // Newest first, the order every judgement below reads them in.
    found.sort_by(|newer, older| older.stamp.cmp(&newer.stamp));
    let mut gate = CopiesGate {
        defer: Some(defer),
        held: false,
    };
    let mut answers = Vec::new();
    for document in KeptDocument::ALL {
        let (copies, rest): (Vec<FoundCopy>, Vec<FoundCopy>) = found
            .into_iter()
            .partition(|copy| copy.document == document);
        found = rest;
        if copies.is_empty() {
            continue;
        }
        let mut bytes = vec![None; copies.len()];
        let mut entry = opened.iter_mut().find(|entry| entry.document == document);
        let start = entry.as_mut().and_then(|entry| entry.version());
        let said = entry.and_then(|entry| newer_refusal_to_say(entry, &copies, &mut bytes));
        let protected = copy_a_newer_build_can_bring_back(document, start, &copies, &mut bytes);
        let offered = protected.and_then(|index| {
            let held: Arc<[u8]> = Arc::from(copy_bytes(&copies, &mut bytes, index)?);
            Some(CopyOffered {
                document,
                copy: copies[index].name.clone(),
                path: copies[index].path.clone(),
                copy_version: bt_persist::schema_version_of(&held)?,
                file_version: start?,
                bytes: held,
            })
        });
        let fate = copies_to_retire(&copies, &mut bytes, protected, document.current_version());
        let mut answer = CopiesRetired {
            document,
            kept: copies.len(),
            removed: Vec::new(),
            left: Vec::new(),
            held: false,
            said,
            offered,
        };
        'phases: for phase in [CopyRetirement::Duplicate, CopyRetirement::BeyondKept] {
            // Oldest first: the copies are newest first.
            for index in (0..copies.len()).rev() {
                if fate[index] != Some(phase) {
                    continue;
                }
                let copy: &FoundCopy = &copies[index];
                if !gate.may_remove() {
                    answer.held = true;
                    break 'phases;
                }
                match remove(&copy.path) {
                    Err(error) if error.kind() != io::ErrorKind::NotFound => {
                        answer.left.push((copy.name.clone(), error.kind()));
                    }
                    // Gone either way: removed now, or by somebody else since it was listed.
                    _ => {
                        answer.kept -= 1;
                        answer.removed.push(copy.name.clone());
                    }
                }
            }
        }
        answers.push(answer);
    }
    Ok(answers)
}

/// This process's last lines.
static REJECTED_COPIES_SAID: OrphansSaid = OrphansSaid::new();

/// **The retirement as the pass's part**: `None` unless this process writes the data folder
/// (`writer`, the rescue card's rule: two processes over one folder never both retire); else one
/// round over `folder`, its lines said through `said` when they changed, and the window's part —
/// the notices and the offer (row 68d) — answered.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rejected_copies_member(
    worker: &WorkerCtx,
    writer: bool,
    folder: &Path,
    opened: &mut [OpenedDocument],
    defer: impl FnOnce(Writer) -> bool,
    remove: &mut dyn FnMut(&Path) -> io::Result<()>,
    said: &OrphansSaid,
    say: &dyn Fn(String),
) -> Option<MemberAnswer<CopyBackAnswer>> {
    if !writer {
        return None;
    }
    let mut part = CopyBackAnswer::default();
    let lines = match retire_rejected_copies(worker, folder, opened, defer, remove) {
        Ok(answers) => {
            for answer in &answers {
                part.notices.extend(answer.said.clone());
                part.offer.extend(answer.offered.clone());
            }
            answers
                .iter()
                .flat_map(CopiesRetired::copies_lines)
                .collect()
        }
        Err(kind) => vec![format!("BT_REJECTED_COPIES unlisted ({kind:?})")],
    };
    let MemberAnswer {
        checks, changed, ..
    } = said.lines_answer(lines, say);
    Some(MemberAnswer {
        part,
        checks,
        changed,
    })
}

/// **The retirement, as a member of the start's carry pass**
/// ([`crate::carry_pass::run_carry_steps`]): the data folder, what this start's stores read, the
/// trial's gate and this machine's disk.
///
/// Nothing in a test process, whose pass must never read or remove the account's copies — the
/// tests drive [`rejected_copies_member`] over folders of their own.
pub(crate) fn run_rejected_copies_step(ctx: &WorkerCtx) -> Option<MemberAnswer<CopyBackAnswer>> {
    if cfg!(test) {
        return None;
    }
    let mut opened = OPENED_AT_START
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let answer = rejected_copies_member(
        ctx,
        crate::persist::is_storage_writer(),
        &crate::persist::storage_dir(),
        &mut opened,
        crate::update_trial::defer,
        &mut |path| std::fs::remove_file(path),
        &REJECTED_COPIES_SAID,
        &crate::diagnostics::stderr_line,
    );
    settle_opened(&opened);
    answer
}

#[cfg(test)]
#[path = "rejected_copies_tests.rs"]
mod tests;
