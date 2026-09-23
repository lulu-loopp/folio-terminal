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

pub fn managed_line_for(data: &Path, appdata: &Path) -> io::Result<&'static str> {
    if data == appdata.join(persist::STORAGE_NAME) {
        Ok(MANAGED_LINE)
    } else if data == appdata.join(persist::PREVIOUS_STORAGE_NAME) {
        Ok(LEGACY_MANAGED_LINE)
    } else {
        Err(io::Error::other(Text::ShellProfileScriptLocation.text()))
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
/// Field order is the release order: the file (and with it the OS lock) is
/// closed first and the turn given back second, so the next writer of ours
/// never finds the file still held by the one ahead of it.
pub struct MarksLock {
    _file: fs::File,
    _turn: OurTurn,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fate {
    Unchanged,
    Migrated,
    Removed,
    Refused(String),
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
    /// [`Text::ShellProfileNothing`], and the first-run card's PowerShell row
    /// left off presses the Settings page's own `Off`, which runs a removal —
    /// so a brand-new machine's first sight of Folio was a corner toast about a
    /// `$PROFILE` line it had never had. A caller that asks the report cannot
    /// make that mistake again.
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

#[cfg(test)]
pub fn apply(paths: &[PathBuf], forms: &Forms, action: Action) -> Report {
    apply_recorded(paths, forms, action, |_| Ok(()))
}

pub fn apply_recorded(
    paths: &[PathBuf],
    forms: &Forms,
    action: Action,
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
            let result =
                super::replace_profile(path, &original, &bytes, std::time::SystemTime::now());
            if let Err(e) = result {
                file.fate = Fate::Refused(e.to_string());
            }
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
