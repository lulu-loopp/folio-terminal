//! **The update homes beside a macOS bundle that this account's copies made, and the carry pass's
//! sweep of the ones whose bundle is gone** (0.4.10 plan row 68e; R5 of 2026-11-04, R7 of
//! 2026-11-09, rulings E1–E10 of 2026-11-10).
//!
//! The macOS updater keeps a bundle's transactions beside it, in the home
//! `<parent>/.<Bundle>.folio-update/` ([`Home::for_bundle`]), which is found from the bundle's own
//! path and nothing else. A commit or a rollback retires the transaction's `<txn>/` folder and then
//! its journal, and leaves the home with `admission`, `lock` and, when the rescue found no data
//! folder, `recover.log`, ready for the bundle's next update. Once the bundle has moved or is gone,
//! nothing names that home: the uninstall finds only the running copy's own (`Home::of`), and
//! Homebrew's zap runs the cleanup from the Caskroom copy, beside which there is none.
//!
//! **So the homes are recorded**, in the data folder's [`HOMES_RECORD`]
//! (`{"version":1,"homes":[…]}`, [`UpdateHomesRecord`]), under `ProfileFiles`' rule
//! (`shell_integration::profile_marks`): **an entry is written before the home it names** — by the
//! macOS Prepare, before it makes the home ([`record_update_home`]; the other order would leave a
//! home nobody knows is Folio's) — and **a recorded path is data, never authority**: an entry is
//! acted on only when it is the home [`Home::for_bundle`] names for the bundle its own name gives,
//! and never through a link. The carry pass records this copy's own home when the record lacks it,
//! so the homes 0.4.6–0.4.9 made at today's paths are in it too (E3). **A record this build cannot
//! read** — a newer version, a key it does not know, bytes that are not one — **is left byte for
//! byte**, and nothing is added to it (E4).
//!
//! **The sweep** ([`sweep_recorded_homes`]), a member of the start's carry pass
//! ([`run_update_homes_step`]) and of both uninstall verbs: a recorded home whose bundle's program
//! is gone from a volume that is there (`attention_ownership::gone_by`), and which holds nothing
//! but Folio's own `admission`, `lock` and `recover.log`, plain files, is removed with its entry —
//! the three files, then the emptied folder, never a tree (E5). Every other home is left and said:
//! one whose bundle is there (`live`) or that the machine will not say about (`owner unknown`), one
//! with a journal, whatever its phase (`in flight`: the rescue's, E6), one holding anything else
//! (`not only ours`), and one whose `lock` or `admission` somebody holds (`in use`: a copy still
//! running from a moved bundle holds `admission` for its life). Removals and the record's own
//! writes are held during an update's trial by [`Writer::OrphanSweep`] (E2), asked once a round
//! and only when one is due.
//!
//! **One diagnostics line per home when its fate changes**: `BT_UPDATE_HOMES <home name>[ #<n>]
//! <removed|gone|recorded|left (live)|left (owner unknown)|left (in flight)|left (not only
//! ours)|left (in use)|held|refused (<stage>)|refused entry>`, `BT_UPDATE_HOMES record <unread|not
//! written> (<reason>)`, `BT_UPDATE_HOMES record held`, or `BT_UPDATE_HOMES none`. A line names the home's own name, never its
//! path; `#<n>` is its place in the round when two homes of one name are said.
//!
//! **macOS only, by the host's answer rather than by `cfg`** (E8, `update_orphans`' shape): the
//! Windows home is `<install>/.folio-update`, inside the install folder, and goes with that folder
//! on every road, so no Windows copy writes a record. The module and its tests are compiled and run
//! on every platform; the member answers `None` off macOS.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use bt_platform::HostPlatform;
use bt_platform::admission::WorkerCtx;
use bt_platform::install_txn::{self, Hold};
use serde::{Deserialize, Serialize};

use crate::attention_ownership::{Look, gone_by, what_stands};
use crate::carry_pass::{LastSaid, MemberAnswer};
use crate::update_orphans::OrphansSaid;
use crate::update_trial::Writer;
use crate::update_txn::{Home, MACOS_HOME_SUFFIX};

/// **The record's file name**, in the data folder.
pub(crate) const HOMES_RECORD: &str = "update-homes.json";

/// The record's version: its keys are pinned at it
/// (`tests::the_update_homes_record_keeps_its_keys_at_version_1`, 68d's rule).
const HOMES_RECORD_VERSION: u32 = 1;

/// **The record of the update homes this account's copies made** ([`HOMES_RECORD`]): a version and
/// the homes' paths, unique, in the order they were recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateHomesRecord {
    version: u32,
    homes: Vec<PathBuf>,
}

impl Default for UpdateHomesRecord {
    fn default() -> Self {
        Self {
            version: HOMES_RECORD_VERSION,
            homes: Vec::new(),
        }
    }
}

impl UpdateHomesRecord {
    /// **The record at `file`**: an empty one when there is none; why not, in words that carry no
    /// path, when it is there and this build cannot read it — another version, a key it does not
    /// know, bytes that are not one record. Such a record is never written over.
    pub(crate) fn read(file: &Path) -> Result<Self, String> {
        let bytes = match bt_platform::file_reads::read(bt_platform::file_reads::Lane::Update, file)
        {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("read: {}", error.kind())),
        };
        // Its category alone: serde's own words may quote a value, and a value may be a path.
        let record: Self = serde_json::from_slice(&bytes).map_err(|error| {
            format!(
                "shape: {}",
                match error.classify() {
                    serde_json::error::Category::Io => "unreadable",
                    serde_json::error::Category::Syntax => "not JSON",
                    serde_json::error::Category::Data => "not this version's keys",
                    serde_json::error::Category::Eof => "cut short",
                }
            )
        })?;
        if record.version != HOMES_RECORD_VERSION {
            return Err(format!("version {}", record.version));
        }
        Ok(record)
    }

    /// **The record written to `file`** through the journal's own durable writer, so an entry is
    /// on the medium before the home it names is made.
    fn write(&self, file: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|_| "write: a recorded path is not Unicode".to_owned())?;
        install_txn::durable_write(file, &bytes)
            .map_err(|failure| format!("write: {}", failure.error.kind()))
    }
}

/// **`home` added to the record at `file`**: `Ok(true)` when it was written, `Ok(false)` when it
/// was there already; why not when the record cannot be read or written — and a record that cannot
/// be read is left as it is.
pub(crate) fn add_home_to_record(file: &Path, home: &Path) -> Result<bool, String> {
    let mut record = UpdateHomesRecord::read(file)?;
    if record.homes.iter().any(|recorded| recorded == home) {
        return Ok(false);
    }
    record.homes.push(home.to_path_buf());
    record.write(file)?;
    Ok(true)
}

/// **The one reader-and-writer at a time of this process's record**: the Prepare's entry on the
/// update job's worker, the carry pass's sweep on its own. One process writes a data folder's
/// record — the data folder's claim makes the running copy the only Folio on it, and the uninstall
/// refuses while any Folio holds it.
static HOMES_RECORD_LOCK: Mutex<()> = Mutex::new(());

/// The Prepare's last word about a record it could not write.
static RECORD_NOT_WRITTEN: LastSaid = LastSaid::new();

/// **The macOS Prepare's entry for the home it is about to make** (step 2, before
/// `install_txn::durable_create_dir`), in the record at `record`. A record that cannot be read or
/// written is said once and the Prepare goes on (E4): the update's own start records the same home
/// ([`run_update_homes_step`]), so only a bundle deleted before any later start stays unrecorded,
/// and a refusal here would stop a downgraded build from updating forward over a newer record.
pub(crate) fn record_update_home(record: &Path, home: &Home) {
    let added = {
        let _record = HOMES_RECORD_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        add_home_to_record(record, home.root())
    };
    if let Err(reason) = added {
        RECORD_NOT_WRITTEN.say_if_new(
            format!("BT_UPDATE_HOMES record not written ({reason})"),
            &crate::diagnostics::stderr_line,
        );
    }
}

/// **What the sweep did with one recorded home.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HomeFate {
    /// Its bundle is gone, and so are its three files, the folder and its entry.
    Removed,
    /// Not there, on a volume that is: its entry is dropped.
    Gone,
    /// This copy's own home, added to the record this round.
    Recorded,
    /// Its bundle, this one, is there.
    LeftLive(PathBuf),
    /// The machine will not say about it or its bundle (a drive that is away among them).
    LeftOwnerUnknown,
    /// It holds a journal: a transaction that is the rescue's.
    LeftInFlight,
    /// It holds these, which are not Folio's three files, so it is left whole.
    LeftNotOnlyOurs(Vec<PathBuf>),
    /// Somebody holds its `lock` or its `admission`.
    LeftInUse,
    /// Due, and held for the update's commit.
    Held,
    /// Looking at it or removing it failed, at the stage named.
    Refused(String),
    /// The entry names no home ([`home_of_entry`]), or a home that is a link or a file: dropped,
    /// and the path it names never touched.
    RefusedEntry,
}

impl HomeFate {
    fn home_fate_said(&self) -> String {
        match self {
            Self::Removed => "removed".to_owned(),
            Self::Gone => "gone".to_owned(),
            Self::Recorded => "recorded".to_owned(),
            Self::LeftLive(_) => "left (live)".to_owned(),
            Self::LeftOwnerUnknown => "left (owner unknown)".to_owned(),
            Self::LeftInFlight => "left (in flight)".to_owned(),
            Self::LeftNotOnlyOurs(_) => "left (not only ours)".to_owned(),
            Self::LeftInUse => "left (in use)".to_owned(),
            Self::Held => "held".to_owned(),
            Self::Refused(stage) => format!("refused ({stage})"),
            Self::RefusedEntry => "refused entry".to_owned(),
        }
    }

    /// Whether the entry leaves the record with this fate.
    fn drops_the_entry(&self) -> bool {
        matches!(self, Self::Removed | Self::Gone | Self::RefusedEntry)
    }
}

/// **What became of the record itself in a sweep.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecordState {
    /// Read, and written where it changed.
    Kept,
    /// There, and this build cannot read it: left byte for byte.
    Unread(String),
    /// Changed, and its rewrite held for the update's commit, as a removal is: left byte for byte,
    /// and the commit's round drops the same entries again.
    Held,
    /// Changed, and the write failed: the next round finds the same homes again.
    NotWritten(String),
}

/// **One sweep's answer**: every recorded home's fate, in the record's order, and the record's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HomesSwept {
    pub(crate) fates: Vec<(PathBuf, HomeFate)>,
    pub(crate) record: RecordState,
}

impl HomesSwept {
    /// **The sweep's lines** — a home's own name and never its path, the record's state by its
    /// reason alone (whose words carry no path either).
    pub(crate) fn home_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        match &self.record {
            RecordState::Kept => {}
            RecordState::Unread(reason) => {
                lines.push(format!("BT_UPDATE_HOMES record unread ({reason})"));
            }
            RecordState::NotWritten(reason) => {
                lines.push(format!("BT_UPDATE_HOMES record not written ({reason})"));
            }
            RecordState::Held => lines.push("BT_UPDATE_HOMES record held".to_owned()),
        }
        let name_of = |home: &Path| {
            home.file_name().map_or_else(
                || "(no name)".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            )
        };
        for (place, (home, fate)) in self.fates.iter().enumerate() {
            let name = name_of(home);
            let shared = self
                .fates
                .iter()
                .filter(|(other, _)| name_of(other) == name)
                .count()
                > 1;
            let said = if shared {
                format!("{name} #{}", place + 1)
            } else {
                name
            };
            lines.push(format!("BT_UPDATE_HOMES {said} {}", fate.home_fate_said()));
        }
        if lines.is_empty() {
            lines.push("BT_UPDATE_HOMES none".to_owned());
        }
        lines
    }
}

/// **The trial's gate as the sweep asks it**: once a round, at the first write that is due.
struct HomesGate<D: FnOnce(Writer) -> bool> {
    defer: Option<D>,
    held: bool,
}

impl<D: FnOnce(Writer) -> bool> HomesGate<D> {
    /// Whether a write that is due may be made now.
    fn may_write(&mut self) -> bool {
        if let Some(defer) = self.defer.take() {
            self.held = defer(Writer::OrphanSweep);
        }
        !self.held
    }
}

/// **The home a recorded entry names, or `None` when it names none** — the locator's own inverse:
/// an absolute path with no `..`, whose name is `.<X>.app.folio-update` and which is exactly the
/// home [`Home::for_bundle`] names for `<its parent>/<X>.app`.
fn home_of_entry(entry: &Path) -> Option<Home> {
    if !entry.is_absolute()
        || entry
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    let name = entry.file_name()?.to_str()?;
    let bundle_name = name.strip_prefix('.')?.strip_suffix(MACOS_HOME_SUFFIX)?;
    let home = Home::for_bundle(&entry.parent()?.join(bundle_name))?;
    (home.root() == entry).then_some(home)
}

/// What a home holds, as the sweep reads it.
enum HomeHolds {
    /// Nothing but Folio's three files, each a plain file.
    OnlyOurs,
    /// A journal: a transaction not yet retired.
    AJournal,
    /// These besides.
    MoreThanOurs(Vec<PathBuf>),
}

/// **The three files a home keeps between transactions**: its `admission`, its `lock`, and the
/// rescue's `recover.log`.
fn home_files_between_transactions(home: &Home) -> [PathBuf; 3] {
    [home.admission(), home.lock(), home.recover_log()]
}

/// **What the home holds**, listed without following a link — a worker's read: on the carry
/// pass's worker, or the uninstall door's.
fn what_the_home_holds(_worker: &WorkerCtx, home: &Home) -> io::Result<HomeHolds> {
    let ours = home_files_between_transactions(home);
    let journal = home.journal();
    let mut journal_there = false;
    let mut more = Vec::new();
    for entry in std::fs::read_dir(home.root())? {
        let entry = entry?;
        let path = entry.path();
        if path == journal {
            journal_there = true;
        } else if !(entry.file_type()?.is_file() && ours.contains(&path)) {
            more.push(path);
        }
    }
    Ok(if journal_there {
        HomeHolds::AJournal
    } else if more.is_empty() {
        HomeHolds::OnlyOurs
    } else {
        more.sort();
        HomeHolds::MoreThanOurs(more)
    })
}

/// The fate a home that is not only Folio's three files has.
fn fate_of_holds(holds: io::Result<HomeHolds>) -> Option<HomeFate> {
    match holds {
        Ok(HomeHolds::OnlyOurs) => None,
        Ok(HomeHolds::AJournal) => Some(HomeFate::LeftInFlight),
        Ok(HomeHolds::MoreThanOurs(more)) => Some(HomeFate::LeftNotOnlyOurs(more)),
        Err(error) => Some(HomeFate::Refused(format!("list: {}", error.kind()))),
    }
}

/// **A home whose bundle is gone, retired**: its `lock` and its `admission` taken, never waited
/// for (`in use` when somebody holds either); what it holds read again under them; its three
/// files removed while they are held, the holds let go, then the emptied folder — `remove_dir`,
/// which takes no tree — and its parent flushed (`install_txn::flush_directory_on_disk`). Nothing
/// removes again after `remove_dir`: a home made again at the path in between is a new one, and
/// never a tree this function takes.
fn retire_home(worker: &WorkerCtx, home: &Home) -> HomeFate {
    let hold = |path: &Path| match install_txn::try_hold(path, Hold::Exclusive) {
        Ok(Some(held)) => Ok(held),
        Ok(None) => Err(HomeFate::LeftInUse),
        Err(failure) => Err(HomeFate::Refused(format!("lock: {}", failure.error.kind()))),
    };
    let lock = match hold(&home.lock()) {
        Ok(held) => held,
        Err(fate) => return fate,
    };
    let admission = match hold(&home.admission()) {
        Ok(held) => held,
        Err(fate) => return fate,
    };
    let holds = what_the_home_holds(worker, home);
    if let Some(fate) = fate_of_holds(holds) {
        return fate;
    }
    for file in home_files_between_transactions(home) {
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return HomeFate::Refused(format!("remove: {}", error.kind())),
        }
    }
    drop(admission);
    drop(lock);
    if let Err(error) = std::fs::remove_dir(home.root()) {
        return HomeFate::Refused(format!("remove: {}", error.kind()));
    }
    match install_txn::flush_directory_on_disk(install_txn::directory_of(home.root())) {
        Ok(()) => HomeFate::Removed,
        Err(failure) => HomeFate::Refused(format!("flush: {}", failure.error.kind())),
    }
}

/// **One recorded entry, judged** — in the order of the module's rule, stopping at the first
/// answer.
///
/// The gate's bound is a `where` clause: the thread guard reads a function's parameters at the
/// first parenthesis after `fn` (`window_waits_tests::signature`), which a `FnOnce(Writer)` bound
/// in the generics would be.
fn judge_recorded_home<D>(
    worker: &WorkerCtx,
    entry: &Path,
    look: &(impl Look + ?Sized),
    gate: &mut HomesGate<D>,
) -> HomeFate
where
    D: FnOnce(Writer) -> bool,
{
    let Some(home) = home_of_entry(entry) else {
        return HomeFate::RefusedEntry;
    };
    match std::fs::symlink_metadata(entry) {
        Ok(meta) if meta.is_dir() && !bt_platform::cleanup::is_link(&meta) => {}
        Ok(_) => return HomeFate::RefusedEntry,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return match gone_by(entry, look) {
                Ok(true) => HomeFate::Gone,
                _ => HomeFate::LeftOwnerUnknown,
            };
        }
        Err(error) => return HomeFate::Refused(format!("look: {}", error.kind())),
    }
    let (Some(bundle), Some(program)) = (home.installed_bundle(), home.installed_program()) else {
        return HomeFate::RefusedEntry;
    };
    match gone_by(&program, look) {
        Ok(true) => {}
        Ok(false) => return HomeFate::LeftLive(bundle),
        Err(_) => return HomeFate::LeftOwnerUnknown,
    }
    let holds = what_the_home_holds(worker, &home);
    if let Some(fate) = fate_of_holds(holds) {
        return fate;
    }
    if !gate.may_write() {
        return HomeFate::Held;
    }
    retire_home(worker, &home)
}

/// **The sweep of the record at `record`**: `seed`, this copy's own home, recorded when it is a
/// folder there and the record lacks it; `skip`, the home the uninstall has just removed whole as
/// this copy's own, neither judged nor touched in the record; every other entry judged
/// ([`judge_recorded_home`]); the record written once when an entry was added or dropped, the
/// trial's gate allowing. A record this build cannot read is left byte for byte and judges
/// nothing. On a worker: the carry pass's, or the uninstall door's.
pub(crate) fn sweep_recorded_homes(
    worker: &WorkerCtx,
    record: &Path,
    seed: Option<&Path>,
    skip: Option<&Path>,
    look: &(impl Look + ?Sized),
    defer: impl FnOnce(Writer) -> bool,
) -> HomesSwept {
    let mut read = match UpdateHomesRecord::read(record) {
        Ok(read) => read,
        Err(reason) => {
            return HomesSwept {
                fates: Vec::new(),
                record: RecordState::Unread(reason),
            };
        }
    };
    let mut gate = HomesGate {
        defer: Some(defer),
        held: false,
    };
    let mut fates = Vec::new();
    let mut changed = false;
    let mut seeded = None;
    if let Some(seed) = seed
        && !read.homes.iter().any(|recorded| recorded == seed)
        && std::fs::symlink_metadata(seed)
            .is_ok_and(|meta| meta.is_dir() && !bt_platform::cleanup::is_link(&meta))
    {
        if gate.may_write() {
            read.homes.push(seed.to_path_buf());
            changed = true;
            seeded = Some(seed);
        } else {
            fates.push((seed.to_path_buf(), HomeFate::Held));
        }
    }
    let mut kept = Vec::new();
    for entry in std::mem::take(&mut read.homes) {
        if kept.contains(&entry) {
            changed = true;
            continue;
        }
        if skip == Some(entry.as_path()) {
            kept.push(entry);
            continue;
        }
        let fate = if seeded == Some(entry.as_path()) {
            HomeFate::Recorded
        } else {
            judge_recorded_home(worker, &entry, look, &mut gate)
        };
        if fate.drops_the_entry() {
            changed = true;
        } else {
            kept.push(entry.clone());
        }
        fates.push((entry, fate));
    }
    read.homes = kept;
    // The record's own rewrite is a write of the member too: held during a trial like a removal,
    // the dropped entries are dropped again by the commit's round.
    let record = if !changed {
        RecordState::Kept
    } else if !gate.may_write() {
        RecordState::Held
    } else {
        match read.write(record) {
            Ok(()) => RecordState::Kept,
            Err(reason) => RecordState::NotWritten(reason),
        }
    };
    HomesSwept { fates, record }
}

/// This process's last lines.
static UPDATE_HOMES_SAID: OrphansSaid = OrphansSaid::new();

/// **The sweep, as a member of the start's carry pass** ([`crate::carry_pass::run_carry_steps`]):
/// the data folder's record, this copy's own home as its seed, this machine's disk and the trial's
/// gate.
///
/// Nothing off macOS, where no copy writes a record; nothing in a test process, whose pass must
/// never read or write the account's record or its homes — the tests drive
/// [`sweep_recorded_homes`] over folders of their own.
pub(crate) fn run_update_homes_step(ctx: &WorkerCtx) -> Option<MemberAnswer<()>> {
    if cfg!(test) || bt_platform::host_platform() != HostPlatform::MacOs {
        return None;
    }
    let record = crate::persist::storage_dir().join(HOMES_RECORD);
    let this_copys = std::env::current_exe()
        .ok()
        .and_then(|exe| Home::of(HostPlatform::MacOs, &exe));
    let swept = {
        let _record = HOMES_RECORD_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        sweep_recorded_homes(
            ctx,
            &record,
            this_copys.as_ref().map(Home::root),
            None,
            &what_stands,
            crate::update_trial::defer,
        )
    };
    Some(UPDATE_HOMES_SAID.lines_answer(swept.home_lines(), &crate::diagnostics::stderr_line))
}

#[cfg(test)]
#[path = "update_homes_tests.rs"]
mod tests;
