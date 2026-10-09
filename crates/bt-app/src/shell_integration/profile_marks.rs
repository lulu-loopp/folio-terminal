//! The account's profile marks. Schema and extension contract:
//! `docs/shell-integration-marks.md`. All content decisions take injected bytes.

use super::*;
use crate::i18n::Text;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::collections::BTreeMap;
use std::{
    collections::BTreeSet,
    fs, io,
    sync::{Condvar, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

pub const RECORD_FILE: &str = "integration-marks.json";
pub const LEGACY_LINE: &str = r#". "$env:APPDATA\Folio\shell-integration\folio.ps1""#;
// One template, with only the two product roots admitted.
macro_rules! managed_line {
    ($root:literal) => {
        concat!(
            r#"if (Test-Path -LiteralPath "$env:APPDATA\"#,
            $root,
            r#"\shell-integration\folio.ps1" -PathType Leaf) { . "$env:APPDATA\"#,
            $root,
            r#"\shell-integration\folio.ps1" } # Folio shell integration v1"#
        )
    };
}
pub const MANAGED_LINE: &str = managed_line!("Folio");
pub const LEGACY_MANAGED_LINE: &str = managed_line!("BetterTerminal");

#[cfg(test)]
pub fn managed_line_for(data: &Path, appdata: &Path) -> io::Result<&'static str> {
    if data == appdata.join(persist::STORAGE_NAME) {
        Ok(MANAGED_LINE)
    } else if data == appdata.join(persist::PREVIOUS_STORAGE_NAME) {
        Ok(LEGACY_MANAGED_LINE)
    } else {
        Err(io::Error::other(
            "the integration script is outside Folio's data folder",
        ))
    }
}

/// Exact spellings only. Literal legacy forms are generated from known script
/// locations, never parsed out of arbitrary user code mentioning folio.ps1.
pub struct Forms {
    legacy: Vec<String>,
    managed: &'static str,
}

impl Forms {
    pub fn new(scripts: &[PathBuf]) -> Self {
        let mut legacy = vec![
            LEGACY_LINE.to_owned(),
            r#". "$env:APPDATA\BetterTerminal\shell-integration\folio.ps1""#.to_owned(),
        ];
        for script in scripts {
            let literal = script.display().to_string();
            legacy.push(format!(". \"{literal}\""));
            let quoted = format!("'{}'", literal.replace('\'', "''"));
            legacy.push(format!(". {quoted}"));
        }
        Self {
            legacy,
            managed: MANAGED_LINE,
        }
    }

    pub fn targeting(mut self, managed: &'static str) -> Self {
        self.managed = managed;
        self
    }

    pub fn owns(&self, line: &str) -> bool {
        let line = line.trim();
        [MANAGED_LINE, LEGACY_MANAGED_LINE].contains(&line)
            || self.legacy.iter().any(|known| known == line)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Migrate,
    Remove,
}

#[derive(Clone, Copy)]
enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
}

pub struct Decoded {
    pub text: String,
    encoding: Encoding,
}

impl Decoded {
    pub fn read(bytes: &[u8]) -> io::Result<Self> {
        let (text, encoding) = if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
            if body.len() % 2 != 0 {
                return Err(invalid_encoding());
            }
            let units: Vec<_> = body
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect();
            (
                String::from_utf16(&units).map_err(|_| invalid_encoding())?,
                Encoding::Utf16Le,
            )
        } else {
            let (body, encoding) = bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .map_or((bytes, Encoding::Utf8), |b| (b, Encoding::Utf8Bom));
            (
                std::str::from_utf8(body)
                    .map_err(|_| invalid_encoding())?
                    .to_owned(),
                encoding,
            )
        };
        // NUL in a BOM-less file is usually unmarked UTF-16. Never guess.
        if text.contains('\0') {
            return Err(invalid_encoding());
        }
        Ok(Self { text, encoding })
    }

    pub fn encode(&self, text: &str) -> Vec<u8> {
        match self.encoding {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Utf8Bom => [b"\xef\xbb\xbf".as_slice(), text.as_bytes()].concat(),
            Encoding::Utf16Le => [
                vec![0xff, 0xfe],
                text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            ]
            .concat(),
        }
    }
}

fn invalid_encoding() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        Text::ShellProfileEncoding.text(),
    )
}

/// None means byte-identical. Each line keeps its own terminator; removal
/// consumes only the owned line's terminator, never a neighbouring blank line.
pub fn rewrite(bytes: &[u8], forms: &Forms, action: Action) -> io::Result<Option<Vec<u8>>> {
    rewrite_recorded(bytes, forms, action, || Ok(()))
}

/// Record an exact owned mark before any replacement, using this same scan.
fn rewrite_recorded(
    bytes: &[u8],
    forms: &Forms,
    action: Action,
    record_owned: impl FnOnce() -> io::Result<()>,
) -> io::Result<Option<Vec<u8>>> {
    let decoded = Decoded::read(bytes)?;
    let mut owns_mark = false;
    let mut output = String::new();
    for raw in decoded.text.split_inclusive('\n') {
        let body = raw.strip_suffix('\n').unwrap_or(raw);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let trimmed = body.trim();
        if !forms.owns(trimmed) {
            output.push_str(raw);
            continue;
        }
        owns_mark = true;
        if action == Action::Remove {
            continue;
        }
        if trimmed == forms.managed {
            output.push_str(raw);
            continue;
        }
        let leading = body.len() - body.trim_start().len();
        output.push_str(&body[..leading]);
        output.push_str(forms.managed);
        output.push_str(&body[body.trim_end().len()..]);
        output.push_str(&raw[body.len()..]);
    }
    if owns_mark {
        record_owned()?;
    }
    let output = decoded.encode(&output);
    Ok((output != bytes).then_some(output))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRoots {
    pub claude: Vec<PathBuf>,
    pub codex: Vec<PathBuf>,
    pub copilot: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PowerShellState {
    Enabled {},
    Off { by: UserDecision, at: String },
}

impl Default for PowerShellState {
    fn default() -> Self {
        Self::Enabled {}
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDecision {
    User,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marks {
    pub version: u32,
    #[serde(default)]
    pub powershell_state: PowerShellState,
    pub powershell_profiles: Vec<PathBuf>,
    pub powershell_scripts: Vec<PathBuf>,
    pub psreadline_module_roots: Vec<PathBuf>,
    pub agent_config_roots: AgentRoots,
    pub profile_refusals: Vec<Refusal>,
}

impl Default for Marks {
    fn default() -> Self {
        Self {
            version: 2,
            powershell_state: PowerShellState::Enabled {},
            powershell_profiles: Vec::new(),
            powershell_scripts: Vec::new(),
            psreadline_module_roots: Vec::new(),
            agent_config_roots: AgentRoots::default(),
            profile_refusals: Vec::new(),
        }
    }
}

impl Marks {
    pub fn read(data: &Path) -> io::Result<Self> {
        let path = data.join(RECORD_FILE);
        super::refuse_profile_path(&path)?;
        match bt_platform::file_reads::read(bt_platform::file_reads::Lane::Settings, path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
            Ok(bytes) => {
                let raw: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                let mut marks: Self =
                    serde_json::from_value(raw.clone()).map_err(io::Error::other)?;
                if !matches!(marks.version, 1 | 2)
                    || (marks.version == 1 && raw.get("powershell_state").is_some())
                    || (marks.version == 2 && raw.get("powershell_state").is_none())
                {
                    return Err(io::Error::other(Text::ShellMarksVersion.text()));
                }
                if let PowerShellState::Off { at, .. } = &marks.powershell_state
                    && crate::seed::parse_iso8601_utc(at).is_none()
                {
                    return Err(io::Error::other(Text::ShellMarksVersion.text()));
                }
                marks.version = 2;
                if marks
                    .powershell_profiles
                    .iter()
                    .chain(&marks.powershell_scripts)
                    .chain(&marks.psreadline_module_roots)
                    .chain(&marks.agent_config_roots.claude)
                    .chain(&marks.agent_config_roots.codex)
                    .chain(&marks.agent_config_roots.copilot)
                    .chain(marks.profile_refusals.iter().map(|r| &r.path))
                    .any(|p| !p.is_absolute())
                {
                    return Err(io::Error::other(Text::ShellMarksPath.text()));
                }
                Ok(marks)
            }
        }
    }

    pub fn write(&self, data: &Path) -> io::Result<()> {
        let path = data.join(RECORD_FILE);
        super::refuse_profile_path(&path)?;
        fs::create_dir_all(data)?;
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        bt_persist::atomic_write(&path, &bytes).map_err(io::Error::other)
    }

    pub fn is_off(&self) -> bool {
        matches!(self.powershell_state, PowerShellState::Off { .. })
    }

    pub fn turn_off(&mut self, at: std::time::SystemTime) {
        if !self.is_off() {
            self.powershell_state = PowerShellState::Off {
                by: UserDecision::User,
                at: crate::seed::format_iso8601_utc(at),
            };
        }
    }

    pub fn remember(&mut self, profile: &Path, script: &Path) {
        for (list, path) in [
            (&mut self.powershell_profiles, profile),
            (&mut self.powershell_scripts, script),
        ] {
            if !list.iter().any(|p| p == path) {
                list.push(path.to_path_buf());
            }
        }
    }
}

/// The way in for a run that may not bring anything into existence — the uninstall
/// door, which runs on accounts that never started Folio.
///
/// The record and its lock live INSIDE the data root, so a root that does not exist
/// holds no marks and has no decision to keep: `None` says "read [`Marks::default`],
/// write nothing", and the root is still absent when the run ends. Writers — install,
/// enable, the Settings row — keep using [`lock`], which creates the root it guards.
pub fn lock_existing(data: &Path, asker: Asker) -> io::Result<Option<MarksLock>> {
    if !data.is_dir() {
        return Ok(None);
    }
    lock(data, asker).map(Some)
}

/// Whether a recorded `$PROFILE` path may be read and edited.
///
/// **A recorded path is data, never authority.** The record is an ordinary JSON file
/// in the data folder; anything able to write it can name any path on the machine, so
/// every recorded path is checked before it is used, on every run and not only under a
/// test sandbox: absolute, free of `..`, never a filesystem root, and the one kind of
/// file this mark can legitimately name — `$PROFILE` is always a `.ps1` script.
pub fn recorded_profile_is_usable(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        && path.parent().is_some()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ps1"))
}

/// **How long one of Folio's own writers waits for a holder in another process.**
///
/// Not a timeout on an operation, and since 2026-09-23 not a bound on our own
/// queue either: a writer of this process waits behind another writer of this
/// process in [`OURS`] with no deadline at all, because the only way that wait
/// ends is the writer ahead of it finishing — an edge, not a clock. Two
/// seconds on a CI runner fsyncing beside four thousand other tests was not
/// enough for our own writer's I/O, and a reader's busy laptop is the same
/// machine.
///
/// What this bounds is the wait *after* that queue, on the OS lock itself. By
/// then no writer of this process can be holding the file — every one of them
/// holds [`OURS`] first — so a holder is another process by construction, and
/// two seconds is long enough for a Folio that is finishing a write and short
/// enough that running out is a diagnosis rather than a delay: the case
/// [`Fate::Refused`] was written for.
pub const OUR_TURN: Duration = Duration::from_secs(2);

/// How often a writer waiting on another process's lock looks again — the same
/// twenty milliseconds the profile probe waits on its own child with, and for
/// the same reason: this is an edge somebody pressed, not a clock run, so it
/// owes nothing to the frame budget and is over before the next one.
const LOOK_AGAIN: Duration = Duration::from_millis(20);

/// **Who is asking for the record, and therefore what a busy lock means.**
///
/// One record, two askers, two answers, and the difference is not a judgement
/// the lock can make for itself — so every caller states which it is and the
/// wrong use cannot be spelled by leaving something out.
///
/// The distinction was learned from a red toast on a brand-new machine's
/// welcome card (2026-09-21): pressing `Done` with two rows on starts the
/// `$PROFILE` install on the window thread and the enable worker beside it, both
/// of them Folio's, and the loser of that meeting said *"lock acquisition failed
/// because the operation would block"* in the corner of a window whose every row
/// had in fact been written. Contention between two of our own writers is not a
/// failure; it is a queue, and a queue is something to stand in — to the end,
/// because what is ahead in it is our own finite I/O (2026-09-23).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Asker {
    /// A writer inside this running Folio — the first-run card, a Settings row,
    /// the strip's `Add to $PROFILE`, the startup migration, the enable and
    /// removal workers. It waits behind Folio's own writers with no deadline,
    /// then up to [`OUR_TURN`] for a holder in another process, and only that
    /// second wait running out is reported.
    InApp,
    /// A command-line door in another process — `--uninstall-cleanup`,
    /// `--remove-shell-integration`, `uninstall.cmd`. It refuses at once and
    /// says so, because the caller is a script with an exit code to read and
    /// nothing it can usefully wait for: whatever holds the record is a Folio
    /// that will still be holding it when the wait ends.
    Door,
}

impl Asker {
    const fn patience(self) -> Duration {
        match self {
            Self::InApp => OUR_TURN,
            Self::Door => Duration::ZERO,
        }
    }
}

/// **This process's writers of the record, one data root at a time.**
///
/// The half of the lock that knows who is ours without asking anybody: every
/// asker in this process takes its data root's place here before it touches the
/// OS lock, and gives it back only after the OS lock is gone. So while a root
/// is in [`Queue::held`], the writer ahead is one of ours; and once a writer is
/// through, anything still holding the file is another process. No pid is read
/// and the OS is not asked who holds what — it is known by construction.
static OURS: Mutex<Queue> = Mutex::new(Queue {
    held: BTreeSet::new(),
    #[cfg(test)]
    waiting: BTreeMap::new(),
});

/// Rung whenever a root leaves [`OURS`]; every waiter looks at its own root.
static NEXT: Condvar = Condvar::new();

struct Queue {
    held: BTreeSet<PathBuf>,
    /// Tests only: who is standing in the queue for a root, and since when.
    #[cfg(test)]
    waiting: BTreeMap<PathBuf, Vec<Instant>>,
}

/// Nothing panics while [`OURS`] is locked, so there is no poison to honour.
fn ours() -> MutexGuard<'static, Queue> {
    OURS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A root's place in [`OURS`], given back on drop.
struct OurTurn(PathBuf);

impl OurTurn {
    /// `InApp` stands in the queue until the root is free, however long our
    /// own writer ahead takes; `Door` refuses at once in the OS lock's own
    /// words, so a door's transcript cannot tell which half was busy.
    fn take(root: PathBuf, asker: Asker) -> io::Result<Self> {
        let mut queue = ours();
        if queue.held.contains(&root) {
            if asker == Asker::Door {
                return Err(io::Error::other(fs::TryLockError::WouldBlock));
            }
            #[cfg(test)]
            let joined = Instant::now();
            #[cfg(test)]
            queue.waiting.entry(root.clone()).or_default().push(joined);
            while queue.held.contains(&root) {
                queue = NEXT.wait(queue).unwrap_or_else(PoisonError::into_inner);
            }
            #[cfg(test)]
            if let Some(waiting) = queue.waiting.get_mut(&root)
                && let Some(mine) = waiting.iter().position(|at| *at == joined)
            {
                waiting.remove(mine);
            }
        }
        queue.held.insert(root.clone());
        Ok(Self(root))
    }
}

impl Drop for OurTurn {
    fn drop(&mut self) {
        ours().held.remove(&self.0);
        NEXT.notify_all();
    }
}

/// **The record's lock: the OS lock on `integration-marks.lock`, and this
/// process's turn at it.** Held for as long as the value lives.
///
/// Release order: the OS lock is let go explicitly ([`Drop`] below), then the
/// file is closed, then the turn is given back (field order), so the next
/// writer of ours never finds the file still held by the one ahead of it.
pub struct MarksLock {
    _file: fs::File,
    _turn: OurTurn,
}

impl Drop for MarksLock {
    /// **Unlocked, not merely closed.** Windows lets go of a closed handle's
    /// locks when it gets round to it — "the time it takes for the operating
    /// system to unlock these locks depends upon available system resources"
    /// (`LockFileEx`) — and the turn is given back the moment the fields drop.
    /// Closed without this, the next writer of ours could be through [`OURS`]
    /// while the file still reads as held, and spend [`OUR_TURN`] on a holder
    /// that is no other process at all, then refuse. An unlock that fails
    /// leaves only the close, which is what there was before.
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

/// Hold across read/modify/write AND the corresponding profile operation.
/// OS lock is released on drop/crash; the empty lock file is not a mark.
///
/// `asker` chooses between waiting and refusing; see [`Asker`]. A refusal is the
/// same `io::Error` it always was, in the same words, so the doors' transcripts
/// are byte-for-byte what they were.
pub fn lock(data: &Path, asker: Asker) -> io::Result<MarksLock> {
    fs::create_dir_all(data)?;
    let path = data.join("integration-marks.lock");
    super::refuse_profile_path(&path)?;
    let root = fs::canonicalize(data)?;
    let patience = asker.patience();
    #[cfg(test)]
    let patience = queue_watch::patience_for(&root).unwrap_or(patience);
    let turn = OurTurn::take(root, asker)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    // Anyone still holding the file now is another process: ours all stand in
    // `OURS`, and this writer is through it.
    let deadline = Instant::now() + patience;
    loop {
        match file.try_lock() {
            Ok(()) => {
                return Ok(MarksLock {
                    _file: file,
                    _turn: turn,
                });
            }
            // A door's patience is zero, so `now` is already past its deadline
            // and it leaves by the arm below with the error it always gave.
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(LOOK_AGAIN);
            }
            Err(error) => return Err(io::Error::other(error)),
        }
    }
}

/// Tests only: what the queue in [`OURS`] looks like from outside, and a
/// shorter patience for one data root so a test can outlast it quickly.
#[cfg(test)]
pub(crate) mod queue_watch {
    use super::*;

    static PATIENCE: Mutex<BTreeMap<PathBuf, Duration>> = Mutex::new(BTreeMap::new());

    /// Every [`Asker`] asking about `data` waits this long instead of its own.
    pub(crate) fn set_patience(data: &Path, patience: Duration) {
        PATIENCE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(fs::canonicalize(data).unwrap(), patience);
    }

    pub(super) fn patience_for(root: &Path) -> Option<Duration> {
        PATIENCE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(root)
            .copied()
    }

    /// When each writer now standing in `data`'s queue joined it.
    pub(crate) fn waiting(data: &Path) -> Vec<Instant> {
        let root = fs::canonicalize(data).unwrap();
        ours().waiting.get(&root).cloned().unwrap_or_default()
    }
}

/// **What Folio's own `$PROFILE` writes brought into existence**, one entry per profile file:
/// whether Folio created the file and which folders it created for it, the one copy taken before
/// its first write into a file that was there, and which edition named that file as its
/// `$PROFILE` (release read B1, M1, M5).
///
/// Recorded **before** the write it describes, under the marks lock, in a file of its own beside
/// [`RECORD_FILE`]: [`Marks`] is read by the previous version too, with `deny_unknown_fields`
/// (RULES §41: every mark keeps a format the previous version reads), so a field there would
/// make an update's rollback refuse the record. A power loss after the record and before the
/// write leaves an entry naming a file or a copy that is not there, which [`ProfileFiles::retire`]
/// clears; the other order would leave a file nobody knows is Folio's.
///
/// **What the record is used for, and what it never permits.** When Folio takes its line out of a
/// profile (Undo, the Settings remover, `--remove-shell-integration`, both uninstall verbs),
/// [`ProfileFiles::retire`] deletes the copy, and deletes the file — then the folders, each only
/// if empty — when Folio created it and nothing but whitespace is left. A file that was there
/// before, or that holds anything else, is never deleted. A recorded path is data, never
/// authority: every one is checked for its shape on reading ([`ProfileFiles::read`]).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileFiles {
    pub version: u32,
    pub profiles: Vec<ProfileFile>,
}

pub const FILES_RECORD: &str = "integration-profile-files.json";
const FILES_RECORD_VERSION: u32 = 1;

impl Default for ProfileFiles {
    fn default() -> Self {
        Self {
            version: FILES_RECORD_VERSION,
            profiles: Vec::new(),
        }
    }
}

/// One profile file's entry in [`ProfileFiles`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileFile {
    pub profile: PathBuf,
    /// The edition that named this file its `$PROFILE.CurrentUserCurrentHost` when Folio wrote
    /// into it, so a removal finds the file without asking that edition again.
    #[serde(default)]
    pub edition: Option<PowerShellEdition>,
    /// No file stood here before Folio's write.
    #[serde(default)]
    pub created_file: bool,
    /// The folders Folio created for the file, outermost first.
    #[serde(default)]
    pub created_folders: Vec<PathBuf>,
    /// The copy of the file as it stood before Folio's first write into it — at most one.
    #[serde(default)]
    pub backup: Option<PathBuf>,
    /// The SHA-256 of the bytes that copy holds, recorded with its name before it is written: the
    /// copy is deleted only while it still holds exactly those bytes, so a file of that name
    /// that Folio did not write — after a crash between the record and the copy — is never
    /// Folio's to delete (release read review F3).
    #[serde(default)]
    pub backup_sha256: Option<String>,
}

impl ProfileFile {
    fn new(profile: &Path) -> Self {
        Self {
            profile: profile.to_path_buf(),
            edition: None,
            created_file: false,
            created_folders: Vec::new(),
            backup: None,
            backup_sha256: None,
        }
    }

    /// Whether every path this entry names has the one shape it can legitimately have: the
    /// profile a usable `.ps1` ([`recorded_profile_is_usable`]); each folder an ancestor of it;
    /// the copy beside it, named `<profile>.bak-…` — so a planted record cannot name anything
    /// else for deletion.
    fn is_usable(&self) -> bool {
        let Some(name) = self.profile.file_name() else {
            return false;
        };
        let mut copy = name.to_os_string();
        copy.push(".bak-");
        recorded_profile_is_usable(&self.profile)
            && self.created_folders.iter().all(|folder| {
                folder.is_absolute()
                    && folder.parent().is_some()
                    && !folder
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
                    && self.profile.starts_with(folder)
                    && *folder != self.profile
            })
            && self.backup.as_ref().is_none_or(|backup| {
                backup.parent() == self.profile.parent()
                    && backup.file_name().is_some_and(|backup| {
                        backup
                            .to_string_lossy()
                            .starts_with(copy.to_string_lossy().as_ref())
                    })
            })
    }
}

impl ProfileFiles {
    pub fn read(data: &Path) -> io::Result<Self> {
        let path = data.join(FILES_RECORD);
        super::refuse_profile_path(&path)?;
        match bt_platform::file_reads::read(bt_platform::file_reads::Lane::Settings, path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
            Ok(bytes) => {
                let files: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                if files.version != FILES_RECORD_VERSION {
                    return Err(io::Error::other(Text::ShellMarksVersion.text()));
                }
                if !files.profiles.iter().all(ProfileFile::is_usable) {
                    return Err(io::Error::other(Text::ShellMarksPath.text()));
                }
                Ok(files)
            }
        }
    }

    /// Written only under the marks lock ([`lock`]), which has made the data root.
    pub fn write(&self, data: &Path) -> io::Result<()> {
        let path = data.join(FILES_RECORD);
        super::refuse_profile_path(&path)?;
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        bt_persist::atomic_write(&path, &bytes).map_err(io::Error::other)
    }

    /// The entry for `profile`, if there is one.
    pub fn entry(&self, profile: &Path) -> Option<&ProfileFile> {
        self.profiles.iter().find(|entry| entry.profile == profile)
    }

    fn entry_mut(&mut self, profile: &Path) -> &mut ProfileFile {
        let index = match self
            .profiles
            .iter()
            .position(|entry| entry.profile == profile)
        {
            Some(index) => index,
            None => {
                self.profiles.push(ProfileFile::new(profile));
                self.profiles.len() - 1
            }
        };
        &mut self.profiles[index]
    }

    /// The profile file a recorded edition named, for every edition the record knows.
    pub fn located(&self, edition: PowerShellEdition) -> Option<&Path> {
        self.profiles
            .iter()
            .find(|entry| entry.edition == Some(edition))
            .map(|entry| entry.profile.as_path())
    }

    /// **Record what the write about to happen will bring into existence** — the file and its
    /// missing folders when there is no file, or the one copy of a file Folio has not yet
    /// written into — and answer where that copy goes (`None`: no copy). Called under the marks
    /// lock, and written before the write.
    pub fn before_write(
        &mut self,
        profile: &Path,
        edition: Option<PowerShellEdition>,
        at: std::time::SystemTime,
    ) -> Option<PathBuf> {
        let exists = fs::symlink_metadata(profile).is_ok();
        let entry = self.entry_mut(profile);
        if edition.is_some() {
            entry.edition = edition;
        }
        if !exists {
            entry.created_file = true;
            let mut missing: Vec<PathBuf> = profile
                .ancestors()
                .skip(1)
                .filter(|folder| !folder.as_os_str().is_empty())
                .take_while(|folder| {
                    fs::symlink_metadata(folder).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                })
                .map(Path::to_path_buf)
                .collect();
            missing.reverse();
            for folder in missing {
                if folder.parent().is_some() && !entry.created_folders.contains(&folder) {
                    entry.created_folders.push(folder);
                }
            }
            return None;
        }
        if entry.created_file || entry.backup.is_some() {
            return None;
        }
        // The bytes the copy will hold — the file as it stands now, which the write checks again
        // before it copies; a file that changes in between leaves a copy whose bytes differ, and
        // that copy is never deleted.
        let Ok(Some(bytes)) = super::read_profile_for_edit(profile) else {
            return None;
        };
        let copy = super::free_backup_path(profile, at);
        entry.backup = Some(copy.clone());
        entry.backup_sha256 = Some(sha256(&bytes));
        Some(copy)
    }

    /// **A write recorded by [`Self::before_write`] did not happen**: the entry goes back to what
    /// it was, keeping any folder the attempt made (each is removed later only if empty), and a
    /// copy the attempt took is removed — it backs up nothing.
    pub fn write_failed(&mut self, previous: Option<ProfileFile>, profile: &Path) {
        let attempted = self.entry(profile).cloned();
        let mut restored = previous
            .clone()
            .unwrap_or_else(|| ProfileFile::new(profile));
        if let Some(attempted) = attempted {
            restored.edition = attempted.edition.or(restored.edition);
            for folder in attempted.created_folders {
                if !restored.created_folders.contains(&folder) && folder.is_dir() {
                    restored.created_folders.push(folder);
                }
            }
            if let Some(copy) = attempted.backup
                && previous.as_ref().and_then(|p| p.backup.as_ref()) != Some(&copy)
            {
                let _ = remove_our_copy(&copy, attempted.backup_sha256.as_deref());
            }
        }
        if restored == ProfileFile::new(profile) {
            self.profiles.retain(|entry| entry.profile != profile);
        } else {
            *self.entry_mut(profile) = restored;
        }
    }

    /// **Folio's line is out of `profile`; take back what its writes brought into existence**
    /// (release read B1, M5). Called after a removal of the line succeeded or found no line —
    /// never after a refused one. The copy is deleted: it existed to undo Folio's write, and the
    /// write is undone. The file is deleted when Folio created it and nothing but whitespace is
    /// left — an empty `$PROFILE` is itself refused under `Restricted`, so leaving it would leave
    /// every session's error — and then each folder Folio created, deepest first, if it is
    /// empty. A file holding anything else, or one that was there before, stays. Idempotent: a
    /// file already gone retires its folders; an entry with nothing left keeps only the edition.
    pub fn retire(&mut self, profile: &Path, forms: &Forms) -> io::Result<()> {
        let Some(index) = self.profiles.iter().position(|e| e.profile == profile) else {
            return Ok(());
        };
        let remaining = super::read_profile_for_edit(profile)?;
        if let Some(bytes) = &remaining {
            let decoded = Decoded::read(bytes).ok();
            if decoded
                .as_ref()
                .is_some_and(|decoded| decoded.text.lines().any(|line| forms.owns(line)))
            {
                return Ok(());
            }
            let entry = &self.profiles[index];
            if entry.created_file && decoded.is_some_and(|decoded| decoded.text.trim().is_empty()) {
                fs::remove_file(profile)?;
                remove_empty_folders(&entry.created_folders);
            }
        } else {
            remove_empty_folders(&self.profiles[index].created_folders);
        }
        let entry = &mut self.profiles[index];
        if let Some(copy) = entry.backup.take() {
            if let Err(e) = remove_our_copy(&copy, entry.backup_sha256.as_deref()) {
                entry.backup = Some(copy);
                return Err(e);
            }
            entry.backup_sha256 = None;
        }
        entry.created_file = false;
        entry.created_folders.clear();
        if entry.edition.is_none() {
            self.profiles.remove(index);
        }
        Ok(())
    }
}

/// SHA-256, lower-case hex.
fn sha256(bytes: &[u8]) -> String {
    bt_winres::digest::hex(&bt_winres::digest::sha256(bytes))
}

/// **Delete a copy Folio recorded, only while it is the copy Folio wrote**: a file at that name
/// holding exactly the recorded bytes. A name with no file is nothing to do; a file whose bytes
/// differ — or a record with no digest — is not provably Folio's and stays.
fn remove_our_copy(copy: &Path, expected: Option<&str>) -> io::Result<()> {
    let bytes = match bt_platform::file_reads::read(bt_platform::file_reads::Lane::Settings, copy) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if expected.is_some_and(|expected| sha256(&bytes) == expected) {
        fs::remove_file(copy)?;
    }
    Ok(())
}

/// Each folder, deepest first, removed only if it is empty: a folder somebody has put anything
/// in stays, whatever the reason the removal gives.
fn remove_empty_folders(folders: &[PathBuf]) {
    for folder in folders.iter().rev() {
        let _ = fs::remove_dir(folder);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fate {
    Unchanged,
    Migrated,
    Removed,
    Refused(String),
    /// This PowerShell (the report's path is the program) did not say where its `$PROFILE` is,
    /// and no record names it: a Folio line there, if there is one, stays (release read M1). Not
    /// a refusal — nothing Folio recorded was left — so it changes no exit code.
    Unlocated,
}

#[derive(Clone, Debug)]
pub struct FileReport {
    pub path: PathBuf,
    pub fate: Fate,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub files: Vec<FileReport>,
}

impl Report {
    pub fn exit_code(&self) -> i32 {
        i32::from(
            self.files
                .iter()
                .any(|f| matches!(f.fate, Fate::Refused(_))),
        )
    }
    pub fn refusals(&self) -> Vec<Refusal> {
        self.files
            .iter()
            .filter_map(|f| match &f.fate {
                Fate::Refused(reason) => Some(Refusal {
                    path: f.path.clone(),
                    reason: reason.clone(),
                }),
                _ => None,
            })
            .collect()
    }
    pub fn text(&self, refused: bool) -> String {
        self.files
            .iter()
            .filter(|f| f.fate != Fate::Unchanged)
            .filter(|f| matches!(f.fate, Fate::Refused(_)) == refused)
            .map(|f| {
                let (label, reason) = match &f.fate {
                    Fate::Unchanged => (Text::ShellProfileUnchanged, ""),
                    Fate::Migrated => (Text::ShellProfileMigrated, ""),
                    Fate::Removed => (Text::ShellProfileRemoved, ""),
                    Fate::Refused(reason) => (Text::ShellProfileRefused, reason.as_str()),
                    // The program first, then what is left there: the door's own row shape.
                    Fate::Unlocated => {
                        return format!(
                            "{}: {}\n",
                            f.path.display(),
                            Text::ShellProfileUnlocated.text()
                        );
                    }
                };
                format!(
                    "{}: {}{}{}\n",
                    label.text(),
                    f.path.display(),
                    if reason.is_empty() { "" } else { ": " },
                    reason
                )
            })
            .collect()
    }

    /// **What a window is owed by this report, or `None` when it is owed
    /// nothing.**
    ///
    /// A removal that found nothing to remove changed nothing, so there is
    /// nothing to report and silence is the honest answer. The place this is
    /// said is the report itself rather than the one call site that raises a
    /// toast, because the call site said it wrong once already: an empty
    /// successful report was turned into the words
    /// [`Text::ShellProfileNothing`]. A caller that asks the report cannot make
    /// that mistake again.
    ///
    /// **The console door is a different door and keeps its words.** Somebody
    /// who typed `--remove-shell-integration` asked a question and is owed an
    /// answer, so `main` still prints [`Text::ShellProfileNothing`] to the
    /// transcript for an empty report. Refusals and real removals say here
    /// exactly what they said before.
    ///
    /// The refusal flag comes back beside the words, because it is what chooses
    /// both the words ([`Report::text`]'s filter) and the colour they arrive in:
    /// handed over together, a caller cannot pair one report's refusal with
    /// another's tone.
    #[must_use]
    pub fn window_text(&self) -> Option<(bool, String)> {
        let refused = self.exit_code() != 0;
        let text = self.text(refused);
        (!text.is_empty()).then_some((refused, text))
    }
}

/// Pure multi-file plan, including injected refusals. A refusal never hides
/// another file's decision. The applier below consumes these same decisions.
#[cfg(test)]
pub fn plan(
    inputs: Vec<(PathBuf, Result<Vec<u8>, String>)>,
    forms: &Forms,
    action: Action,
) -> Vec<(FileReport, Option<Vec<u8>>)> {
    plan_recorded(inputs, forms, action, &mut |_| Ok(()))
}

fn plan_recorded(
    inputs: Vec<(PathBuf, Result<Vec<u8>, String>)>,
    forms: &Forms,
    action: Action,
    record_owned: &mut impl FnMut(&Path) -> io::Result<()>,
) -> Vec<(FileReport, Option<Vec<u8>>)> {
    inputs
        .into_iter()
        .map(|(path, input)| {
            let result = input.and_then(|bytes| {
                rewrite_recorded(&bytes, forms, action, || record_owned(&path))
                    .map_err(|e| e.to_string())
            });
            let (fate, bytes) = match result {
                Err(reason) => (Fate::Refused(reason), None),
                Ok(None) => (Fate::Unchanged, None),
                Ok(Some(bytes)) => (
                    if action == Action::Remove {
                        Fate::Removed
                    } else {
                        Fate::Migrated
                    },
                    Some(bytes),
                ),
            };
            (FileReport { path, fate }, bytes)
        })
        .collect()
}

/// Apply `action` to every path, then — for a removal that succeeded or found nothing — retire
/// what Folio's writes brought into existence there ([`ProfileFiles::retire`]). No copy is taken:
/// a removal puts the file back to what Folio's write found, byte for byte outside the line
/// (`shell_integration_rewrite_table_preserves_every_other_byte`), and the one copy of a file is
/// taken before Folio's first write into it ([`ProfileFiles::before_write`]).
pub fn apply_recorded(
    paths: &[PathBuf],
    forms: &Forms,
    action: Action,
    files: &mut ProfileFiles,
    mut record_owned: impl FnMut(&Path) -> io::Result<()>,
) -> Report {
    let mut report = Report::default();
    for path in paths {
        let before = super::read_profile_for_edit(path);
        let input = before
            .as_ref()
            .map(|b| b.clone().unwrap_or_default())
            .map_err(ToString::to_string);
        let (mut file, replacement) = plan_recorded(
            vec![(path.clone(), input)],
            forms,
            action,
            &mut record_owned,
        )
        .pop()
        .unwrap();
        if let Some(bytes) = replacement {
            let original = before.unwrap().unwrap_or_default();
            if let Err(e) = super::replace_profile(path, &original, &bytes, None) {
                file.fate = Fate::Refused(e.to_string());
            }
        }
        if action == Action::Remove
            && matches!(file.fate, Fate::Removed | Fate::Unchanged)
            && let Err(e) = files.retire(path, forms)
        {
            file.fate = Fate::Refused(e.to_string());
        }
        report.files.push(file);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_integration_record_failure_prevents_owned_profile_edit() {
        let root = super::super::tests::temp_dir("owned-record-failure");
        let profile = root.join("profile.ps1");
        fs::write(&profile, LEGACY_LINE).unwrap();
        let mut calls = 0;
        let report = apply_recorded(
            std::slice::from_ref(&profile),
            &Forms::new(&[]),
            Action::Migrate,
            &mut ProfileFiles::default(),
            |path| {
                calls += 1;
                assert_eq!(path, profile);
                assert_eq!(fs::read(path).unwrap(), LEGACY_LINE.as_bytes());
                Err(io::Error::other("record unavailable"))
            },
        );
        assert_eq!(calls, 1);
        assert_eq!(report.exit_code(), 1);
        assert_eq!(fs::read(&profile).unwrap(), LEGACY_LINE.as_bytes());
        assert!(report.text(true).contains("record unavailable"));
    }

    #[test]
    fn shell_integration_followup_exactly_two_managed_roots_and_legacy_install() {
        let appdata = super::super::tests::temp_dir("two-roots");
        for (root, expected) in [
            (persist::STORAGE_NAME, MANAGED_LINE),
            (persist::PREVIOUS_STORAGE_NAME, LEGACY_MANAGED_LINE),
        ] {
            let data = appdata.join(root);
            let line = managed_line_for(&data, &appdata).unwrap();
            assert_eq!(line, expected);
            let script = data.join("shell-integration").join("folio.ps1");
            let forms = Forms::new(std::slice::from_ref(&script)).targeting(line);
            let profile = appdata.join(format!("{root}.ps1"));
            let literal = format!(". \"{}\"", script.display());
            fs::write(&profile, &literal).unwrap();
            super::super::profile_runtime::install_recorded(
                &profile,
                &data,
                &script,
                line,
                std::time::UNIX_EPOCH,
                None,
                &ProfileRevision::read(&profile),
            )
            .unwrap();
            assert_eq!(fs::read_to_string(&profile).unwrap(), line);
            assert!(forms.owns(MANAGED_LINE));
            assert!(forms.owns(LEGACY_MANAGED_LINE));
            assert!(!forms.owns(&line.replace(root, "SomeoneElse")));
            assert!(!forms.owns(&format!("{line} # mine")));
            assert_eq!(
                rewrite(line.as_bytes(), &forms, Action::Migrate).unwrap(),
                None
            );
            assert_eq!(
                rewrite(line.as_bytes(), &forms, Action::Remove).unwrap(),
                Some(vec![])
            );
        }
        assert!(managed_line_for(&appdata.join("Other"), &appdata).is_err());
        assert!(managed_line_for(&appdata.join("elsewhere").join("Folio"), &appdata).is_err());
    }

    #[test]
    fn shell_integration_rewrite_table_preserves_every_other_byte() {
        let forms = Forms::new(&[]);
        for encoding in [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le] {
            let codec = Decoded {
                text: String::new(),
                encoding,
            };
            for (prefix, suffix) in [
                ("", ""),
                ("", "\n"),
                ("", "\r\n"),
                ("", "\n# tail\r\n"),
                ("# café\r\n\n", "\n# tail\r\n"),
                ("# before\n", ""),
                ("\r\n", "\r\n\n# mixed\n"),
            ] {
                let legacy = format!("{prefix}  {LEGACY_LINE}\t{suffix}");
                let managed = format!("{prefix}  {}\t{suffix}", MANAGED_LINE);
                let before = codec.encode(&legacy);
                let after = rewrite(&before, &forms, Action::Migrate).unwrap().unwrap();
                assert_eq!(after, codec.encode(&managed));
                assert_eq!(rewrite(&after, &forms, Action::Migrate).unwrap(), None);
                let trailing = suffix
                    .strip_prefix("\r\n")
                    .or_else(|| suffix.strip_prefix('\n'))
                    .unwrap_or(suffix);
                let removed = rewrite(&after, &forms, Action::Remove).unwrap().unwrap();
                assert_eq!(removed, codec.encode(&format!("{prefix}{trailing}")));
                assert_eq!(rewrite(&removed, &forms, Action::Remove).unwrap(), None);
                assert_eq!(
                    rewrite(&before, &forms, Action::Remove).unwrap(),
                    Some(removed)
                );
            }
        }
    }

    #[test]
    fn shell_integration_user_lines_and_nonexact_forms_survive() {
        let forms = Forms::new(&[]);
        for line in [
            "# my note about folio.ps1",
            r". D:\tools\folio.ps1",
            r". 'D:\tools\folio.ps1'",
            r#"Write-Host 'folio.ps1'"#,
            ". \"$env:APPDATA\\Folio\\shell-integration\\folio.ps1\" # mine",
        ] {
            for action in [Action::Remove, Action::Migrate] {
                assert_eq!(
                    rewrite(line.as_bytes(), &forms, action).unwrap(),
                    None,
                    "{line}"
                );
            }
        }
    }

    #[test]
    fn shell_integration_partial_refusal_and_encoding_refusal_are_explicit() {
        let forms = Forms::new(&[]);
        let result = plan(
            vec![
                (PathBuf::from("first"), Ok(LEGACY_LINE.as_bytes().to_vec())),
                (PathBuf::from("second"), Err("locked".into())),
            ],
            &forms,
            Action::Remove,
        );
        assert_eq!(result[0].0.fate, Fate::Removed);
        assert_eq!(result[0].1, Some(Vec::new()));
        assert_eq!(result[1].0.fate, Fate::Refused("locked".into()));
        for bytes in [
            vec![0xfe, 0xff, 0, 65],
            vec![0xff],
            vec![0xff, 0xfe, 65],
            vec![0xff, 0xfe, 0, 0xd8],
            vec![65, 0],
        ] {
            assert!(rewrite(&bytes, &forms, Action::Remove).is_err());
        }
    }

    #[test]
    fn shell_integration_all_historical_literal_spellings_are_exact() {
        let path = PathBuf::from(r"D:\old data\Folio\shell-integration\folio.ps1");
        let forms = Forms::new(&[path]);
        for line in &forms.legacy {
            assert_eq!(
                rewrite(line.as_bytes(), &forms, Action::Migrate).unwrap(),
                Some(MANAGED_LINE.as_bytes().to_vec())
            );
        }
        assert!(!forms.owns(r#". "D:\someone else\folio.ps1""#));
    }
}
