//! The account's profile marks. Schema and extension contract:
//! `docs/shell-integration-marks.md`. All content decisions take injected bytes.

use super::*;
use crate::i18n::Text;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::collections::BTreeMap;
use std::{
    borrow::Cow,
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

/// **The line Folio writes for the account whose data folder is `data`**, `appdata` being
/// `%APPDATA%`: the guarded line naming the script that folder holds — [`MANAGED_LINE`] for
/// `APPDATA\Folio`, [`LEGACY_MANAGED_LINE`] for an account still on the previous name — and a
/// refusal for a folder anywhere else, where no guarded line names the script (0.4.3's rule,
/// `docs/shell-integration-marks.md`; given back to Enable and to the carry by 0.4.10 plan row 68c,
/// R3). `None` for `appdata` is a process with no `APPDATA`, whose folder is the temp fallback.
///
/// **The two roots come from two sources** — the data folder from this process's `APPDATA`, the
/// account's from the environment the probe ran in (the review's N2) — so they are compared as the
/// data-root compare of the agents' records compares them (`explorer_menu::same_path`: the resolved
/// path where both are there, otherwise the spelling with case folded and a trailing separator
/// dropped), and a root spelled in another case is not refused (the re-review's R4).
pub fn managed_line_for(data: &Path, appdata: Option<&Path>) -> io::Result<&'static str> {
    let under =
        |appdata: &Path, name: &str| crate::explorer_menu::same_path(data, &appdata.join(name));
    match appdata {
        Some(appdata) if under(appdata, persist::STORAGE_NAME) => Ok(MANAGED_LINE),
        Some(appdata) if under(appdata, persist::PREVIOUS_STORAGE_NAME) => Ok(LEGACY_MANAGED_LINE),
        _ => Err(io::Error::other(Text::ShellProfileOutsideAppData.text())),
    }
}

/// **Every line Folio has ever written into a `$PROFILE`**, by the writer and the versions that
/// wrote it (0.4.10 plan row 68c, report §1.2). Each is one whole line; no version wrote a comment
/// or a guard line of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineForm {
    /// F1 — `. "$env:APPDATA\Folio\shell-integration\folio.ps1"` ([`LEGACY_LINE`]): the appender
    /// of 0.1.0–0.4.2 (`integration_line`, `0eaa4753e`).
    AppData,
    /// F2 — F1 under the previous name, `$env:APPDATA\BetterTerminal\…`: the same appender, for an
    /// account whose data stayed there.
    AppDataPrevious,
    /// F3 — `. "<script path>"`, the literal path double-quoted: 0.1.0–0.2.3, for a script outside
    /// `%APPDATA%`.
    LiteralDoubleQuoted,
    /// F4 — `. '<script path>'`, each `'` doubled: 0.2.4–0.4.2 (`8c0b7b5d6`).
    LiteralSingleQuoted,
    /// F5 — [`MANAGED_LINE`], the guarded line: 0.4.3 on (`aa234cf68`), the current form.
    Guarded,
    /// F6 — [`LEGACY_MANAGED_LINE`], the guarded line under the previous name: 0.4.3–0.4.6
    /// (`b4f8e8ab2`), the current form for an account on that root.
    GuardedPrevious,
}

impl LineForm {
    /// The form's name in a diagnostics line.
    pub fn said_as(self) -> &'static str {
        match self {
            Self::AppData => "the 0.1-0.4.2 $env:APPDATA line",
            Self::AppDataPrevious => "the 0.1-0.4.2 $env:APPDATA line under the previous name",
            Self::LiteralDoubleQuoted => "the 0.1-0.2.3 literal line",
            Self::LiteralSingleQuoted => "the 0.2.4-0.4.2 literal line",
            Self::Guarded => "the guarded line",
            Self::GuardedPrevious => "the guarded line under the previous name",
        }
    }
}

/// Exact spellings only. Literal legacy forms are generated from known script
/// locations, never parsed out of arbitrary user code mentioning folio.ps1.
pub struct Forms {
    legacy: Vec<(LineForm, String)>,
    managed: &'static str,
}

impl Forms {
    pub fn new(scripts: &[PathBuf]) -> Self {
        let mut legacy = vec![
            (LineForm::AppData, LEGACY_LINE.to_owned()),
            (
                LineForm::AppDataPrevious,
                r#". "$env:APPDATA\BetterTerminal\shell-integration\folio.ps1""#.to_owned(),
            ),
        ];
        for script in scripts {
            let literal = script.display().to_string();
            legacy.push((LineForm::LiteralDoubleQuoted, format!(". \"{literal}\"")));
            let quoted = format!("'{}'", literal.replace('\'', "''"));
            legacy.push((LineForm::LiteralSingleQuoted, format!(". {quoted}")));
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
        self.form_of(line).is_some()
    }

    /// Which of Folio's forms `line` is, exactly, its surrounding whitespace aside.
    pub fn form_of(&self, line: &str) -> Option<LineForm> {
        let line = line.trim();
        if line == MANAGED_LINE {
            Some(LineForm::Guarded)
        } else if line == LEGACY_MANAGED_LINE {
            Some(LineForm::GuardedPrevious)
        } else {
            self.legacy
                .iter()
                .find(|(_, known)| known == line)
                .map(|(form, _)| *form)
        }
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
    /// **A BOM-less file that is not UTF-8** — a profile in the system's ANSI code page, as
    /// Windows PowerShell's `Set-Content` writes one (0.4.10 plan row 68c, R7). Held byte for byte:
    /// each byte is the character of the same number (U+0000–U+00FF), so nothing is decoded, no
    /// code page is guessed, and the bytes come back out exactly. The 0.1.0–0.4.2 appender wrote its
    /// line's UTF-8 bytes into such files, and a line is Folio's when its bytes are those
    /// ([`Decoded::line_as_folio_wrote_it`]).
    AnsiBytes,
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
        } else if bytes.starts_with(&[0xfe, 0xff]) {
            // UTF-16 big-endian: never written as bytes it is not.
            return Err(invalid_encoding());
        } else if let Some(body) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
            (
                std::str::from_utf8(body)
                    .map_err(|_| invalid_encoding())?
                    .to_owned(),
                Encoding::Utf8Bom,
            )
        } else {
            match std::str::from_utf8(bytes) {
                Ok(text) => (text.to_owned(), Encoding::Utf8),
                Err(_) => (
                    bytes.iter().copied().map(char::from).collect(),
                    Encoding::AnsiBytes,
                ),
            }
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
            Encoding::AnsiBytes => text.chars().map(held_byte).collect(),
        }
    }

    /// **A line of [`Self::text`] as Folio's writers wrote it**: the line itself, or — in a file
    /// held as bytes — its bytes read as UTF-8, which is what every Folio wrote into such a file.
    /// `None` for a line of a byte-held file whose bytes are not UTF-8: no line of Folio's.
    pub fn line_as_folio_wrote_it<'a>(&self, line: &'a str) -> Option<Cow<'a, str>> {
        match self.encoding {
            Encoding::AnsiBytes => String::from_utf8(line.chars().map(held_byte).collect())
                .ok()
                .map(Cow::Owned),
            Encoding::Utf8 | Encoding::Utf8Bom | Encoding::Utf16Le => Some(Cow::Borrowed(line)),
        }
    }

    /// **Text Folio writes, as [`Self::text`] holds it**: unchanged, or — in a file held as bytes —
    /// one character per byte of its UTF-8, so [`Self::encode`] writes exactly those bytes.
    pub fn as_held<'a>(&self, written: &'a str) -> Cow<'a, str> {
        match self.encoding {
            Encoding::AnsiBytes => Cow::Owned(written.bytes().map(char::from).collect()),
            Encoding::Utf8 | Encoding::Utf8Bom | Encoding::Utf16Le => Cow::Borrowed(written),
        }
    }

    /// Whether any line is in a form `forms` owns.
    pub fn holds_an_owned_line(&self, forms: &Forms) -> bool {
        self.text.lines().any(|line| {
            self.line_as_folio_wrote_it(line)
                .is_some_and(|line| forms.owns(&line))
        })
    }

    /// The forms of Folio's that the file's lines are in, in the order they stand.
    pub fn forms_in(&self, forms: &Forms) -> Vec<LineForm> {
        self.text
            .lines()
            .filter_map(|line| forms.form_of(&self.line_as_folio_wrote_it(line)?))
            .collect()
    }

    /// Whether a line names `folio.ps1` as code without being in one of `forms` — a Folio line
    /// somebody edited, or one Folio has no record of writing: the line the carry leaves and says.
    pub fn names_the_script_otherwise(&self, forms: &Forms) -> bool {
        self.text.lines().any(|line| {
            let written = self.line_as_folio_wrote_it(line);
            let shown = written.as_deref().unwrap_or(line);
            profile_suppresses_integration_offer(shown) && !forms.owns(shown)
        })
    }
}

/// The byte a character of a byte-held file stands for: every one of them is U+0000–U+00FF
/// ([`Encoding::AnsiBytes`]), and what Folio adds to one is put there by [`Decoded::as_held`].
fn held_byte(character: char) -> u8 {
    character as u8
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
pub(super) fn rewrite_recorded(
    bytes: &[u8],
    forms: &Forms,
    action: Action,
    record_owned: impl FnOnce() -> io::Result<()>,
) -> io::Result<Option<Vec<u8>>> {
    let decoded = Decoded::read(bytes)?;
    let mut owns_mark = false;
    let mut output = String::new();
    for raw in decoded.text.split_inclusive('\n') {
        let held = raw.strip_suffix('\n').unwrap_or(raw);
        let held = held.strip_suffix('\r').unwrap_or(held);
        let Some(body) = decoded.line_as_folio_wrote_it(held) else {
            output.push_str(raw);
            continue;
        };
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
        let line = [
            &body[..leading],
            forms.managed,
            &body[body.trim_end().len()..],
        ]
        .concat();
        output.push_str(&decoded.as_held(&line));
        output.push_str(&raw[held.len()..]);
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
                .is_some_and(|decoded| decoded.holds_an_owned_line(forms))
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
            let line = managed_line_for(&data, Some(&appdata)).unwrap();
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
        assert!(managed_line_for(&appdata.join("Other"), Some(&appdata)).is_err());
        assert!(
            managed_line_for(&appdata.join("elsewhere").join("Folio"), Some(&appdata)).is_err()
        );
        assert!(managed_line_for(&appdata.join("Folio"), None).is_err());
    }

    #[test]
    fn shell_integration_rewrite_table_preserves_every_other_byte() {
        let forms = Forms::new(&[]);
        for encoding in [
            Encoding::Utf8,
            Encoding::Utf8Bom,
            Encoding::Utf16Le,
            Encoding::AnsiBytes,
        ] {
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
            vec![0xfe, 0xff, 65, 66],
            vec![0xef, 0xbb, 0xbf, 0xff],
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
        for (_, line) in &forms.legacy {
            assert_eq!(
                rewrite(line.as_bytes(), &forms, Action::Migrate).unwrap(),
                Some(MANAGED_LINE.as_bytes().to_vec())
            );
        }
        assert!(!forms.owns(r#". "D:\someone else\folio.ps1""#));
    }

    /// An account's folders for a carry: `%APPDATA%` (the test's own folder), the data root under
    /// it by `root`'s name, made, and a `$PROFILE` path beside them, not made.
    fn carry_account(tag: &str, root: &str) -> (PathBuf, PathBuf, PathBuf) {
        let appdata = super::super::tests::temp_dir(tag);
        let data = appdata.join(root);
        fs::create_dir_all(&data).unwrap();
        let documents = appdata.join("Documents");
        fs::create_dir_all(&documents).unwrap();
        (appdata, data, documents.join("profile.ps1"))
    }

    /// The carry over one account, its one PowerShell having said `profile` is its `$PROFILE`.
    fn carry_for(
        data: &Path,
        appdata: Option<&Path>,
        profile: &Path,
    ) -> super::super::profile_runtime::ProfileLinesCarried {
        super::super::profile_runtime::carry_profile_lines(
            data,
            appdata,
            vec![(PathBuf::from("pwsh"), Some(profile.to_path_buf()))],
        )
    }

    /// The script a data root holds: the one a literal line of 0.1.0-0.4.2 named.
    fn script_in(data: &Path) -> PathBuf {
        data.join("shell-integration").join("folio.ps1")
    }

    /// One form carried in a file of its own: the file holds `line` alone, the carry brings it to
    /// `to`, says so once by both forms' names, and a second carry finds nothing to do.
    fn carries_alone(tag: &str, root: &str, line: impl Fn(&Path) -> String, from: LineForm) {
        let (appdata, data, profile) = carry_account(tag, root);
        let line = line(&data);
        let to = managed_line_for(&data, Some(&appdata)).unwrap();
        let to_form = Forms::new(&[]).form_of(to).unwrap();
        fs::write(&profile, format!("{line}\r\n")).unwrap();
        let carried = carry_for(&data, Some(&appdata), &profile);
        assert_eq!(fs::read_to_string(&profile).unwrap(), format!("{to}\r\n"));
        assert_eq!(
            carried.lines,
            [format!(
                "BT_SHELL_PROFILE {}: brought forward from {} to {}",
                profile.display(),
                from.said_as(),
                to_form.said_as()
            )]
        );
        assert_eq!(
            carry_for(&data, Some(&appdata), &profile).lines,
            Vec::<String>::new(),
            "a carried profile has nothing more to carry"
        );
        let _ = fs::remove_dir_all(appdata);
    }

    /// RED (0.4.10 plan row 68c) — **the carry pass brings Folio's old `$PROFILE` lines forward**:
    /// a profile holding the line 0.1.0-0.4.2 wrote among the person's own lines is given the
    /// guarded line for the account's root in its place, the person's lines kept, the run said
    /// once, the profile remembered in the record (so the uninstall finds it), and a second run
    /// finds nothing to do and says nothing.
    ///
    /// MUTATION: the member is not called in `run_carry_steps`
    /// (`plant-68c-member-not-in-the-pass.py`, red in the inventory's guard).
    #[test]
    fn the_carry_pass_brings_folios_old_profile_lines_forward() {
        let (appdata, data, profile) = carry_account("carry-forward", persist::STORAGE_NAME);
        let before = format!("# mine\r\nSet-Alias ll Get-ChildItem\r\n{LEGACY_LINE}\r\n$x = 1\n");
        fs::write(&profile, &before).unwrap();
        let carried = carry_for(&data, Some(&appdata), &profile);
        assert_eq!(
            fs::read_to_string(&profile).unwrap(),
            before.replace(LEGACY_LINE, MANAGED_LINE)
        );
        assert_eq!(carried.checks, 1);
        assert_eq!(
            carried.lines,
            [format!(
                "BT_SHELL_PROFILE {}: brought forward from {} to {}",
                profile.display(),
                LineForm::AppData.said_as(),
                LineForm::Guarded.said_as()
            )]
        );
        let marks = Marks::read(&data).unwrap();
        assert_eq!(marks.powershell_profiles, std::slice::from_ref(&profile));
        assert_eq!(marks.powershell_scripts, [script_in(&data)]);
        let after = fs::read(&profile).unwrap();
        assert_eq!(
            carry_for(&data, Some(&appdata), &profile),
            super::super::profile_runtime::ProfileLinesCarried {
                checks: 1,
                lines: Vec::new()
            }
        );
        assert_eq!(fs::read(&profile).unwrap(), after);
        let _ = fs::remove_dir_all(appdata);
    }

    /// RED (row 68c, F1) — `. "$env:APPDATA\Folio\shell-integration\folio.ps1"`, the appender's
    /// line of 0.1.0-0.4.2, is brought to the guarded line.
    #[test]
    fn the_0_1_appdata_line_is_brought_forward() {
        carries_alone(
            "carry-f1",
            persist::STORAGE_NAME,
            |_| LEGACY_LINE.to_owned(),
            LineForm::AppData,
        );
    }

    /// RED (row 68c, F2) — the appender's line under the previous name, for an account whose data
    /// has since moved to `Folio`, is brought to the guarded line.
    #[test]
    fn the_previous_root_appdata_line_is_brought_forward() {
        carries_alone(
            "carry-f2",
            persist::STORAGE_NAME,
            |_| LEGACY_LINE.replace("Folio", "BetterTerminal"),
            LineForm::AppDataPrevious,
        );
    }

    /// RED (row 68c, F3) — the double-quoted literal line of 0.1.0-0.2.3, naming the data root's
    /// script, is brought to the guarded line.
    #[test]
    fn the_0_1_double_quoted_literal_line_is_brought_forward() {
        carries_alone(
            "carry-f3",
            persist::STORAGE_NAME,
            |data| format!(". \"{}\"", script_in(data).display()),
            LineForm::LiteralDoubleQuoted,
        );
    }

    /// RED (row 68c, F4) — the single-quoted literal line of 0.2.4-0.4.2, each `'` of the path
    /// doubled, is brought to the guarded line.
    ///
    /// MUTATION: the single-quoted form is left out of `Forms::new`
    /// (`plant-68c-single-quote-form-forgotten.py`).
    #[test]
    fn the_0_2_4_single_quoted_literal_line_is_brought_forward() {
        carries_alone(
            "carry-f4-o'brien",
            persist::STORAGE_NAME,
            |data| {
                let script = script_in(data).display().to_string();
                assert!(script.contains('\''), "the path has a quote to double");
                format!(". '{}'", script.replace('\'', "''"))
            },
            LineForm::LiteralSingleQuoted,
        );
    }

    /// RED (row 68c, F5, R3) — **the guarded line follows the account's root**: an account still
    /// on the previous name whose profile holds the line naming `%APPDATA%\Folio` — a script that
    /// is not there — is given the line naming its own root's script; an account on `Folio`
    /// keeps the line as it is and is told nothing.
    ///
    /// MUTATION: `managed_line_for` answers `MANAGED_LINE` for every root
    /// (`plant-68c-target-always-folio.py`).
    #[test]
    fn the_guarded_line_follows_the_accounts_root() {
        carries_alone(
            "carry-f5",
            persist::PREVIOUS_STORAGE_NAME,
            |_| MANAGED_LINE.to_owned(),
            LineForm::Guarded,
        );
        let (appdata, data, profile) = carry_account("carry-f5-current", persist::STORAGE_NAME);
        let before = format!("# mine\n{MANAGED_LINE}\n");
        fs::write(&profile, &before).unwrap();
        assert_eq!(
            carry_for(&data, Some(&appdata), &profile).lines,
            Vec::<String>::new()
        );
        assert_eq!(fs::read_to_string(&profile).unwrap(), before);
        assert!(
            !data.join(RECORD_FILE).exists(),
            "a profile that is current is no reason to make a record"
        );
        let _ = fs::remove_dir_all(appdata);
    }

    /// RED (row 68c, F6) — the guarded line of 0.4.3-0.4.6 under the previous name, for an account
    /// whose data has since moved to `Folio`, is brought to the guarded line naming it.
    #[test]
    fn the_previous_roots_guarded_line_is_brought_forward() {
        carries_alone(
            "carry-f6",
            persist::STORAGE_NAME,
            |_| LEGACY_MANAGED_LINE.to_owned(),
            LineForm::GuardedPrevious,
        );
    }

    /// RED (row 68c, the owner's rule) — **a carried profile keeps every byte but Folio's line**:
    /// in every encoding a profile is read in, with a BOM or none, mixed line ends, the line's own
    /// indentation and trailing whitespace, no terminator on the last line, and the person's own
    /// lines that mention the script in a comment, the bytes after the carry are the bytes before
    /// with the one line's text replaced, and nothing else.
    ///
    /// MUTATIONS: the carried line's terminator written as `\r\n` whatever it was
    /// (`plant-68c-line-ends-normalised.py`); the file written back as UTF-8 whatever it was read
    /// as (`plant-68c-encoding-dropped.py`).
    #[test]
    fn a_carried_profile_keeps_every_byte_but_folios_line() {
        let (appdata, data, profile) = carry_account("carry-every-byte", persist::STORAGE_NAME);
        for encoding in [
            Encoding::Utf8,
            Encoding::Utf8Bom,
            Encoding::Utf16Le,
            Encoding::AnsiBytes,
        ] {
            let codec = Decoded {
                text: String::new(),
                encoding,
            };
            for (prefix, suffix) in [
                ("", ""),
                ("# folio.ps1 is loaded below\r\n\n", "\n# tail\r\n"),
                ("\r\n", "\r\n\n# mixed\n"),
                ("Set-Alias g git\n", "\r\nfunction prompt { 'PS> ' }"),
            ] {
                let text = |line: &str| format!("{prefix} \t{line}  {suffix}");
                fs::write(&profile, codec.encode(&text(LEGACY_LINE))).unwrap();
                let carried = carry_for(&data, Some(&appdata), &profile);
                assert_eq!(carried.lines.len(), 1, "{carried:?}");
                assert_eq!(
                    fs::read(&profile).unwrap(),
                    codec.encode(&text(MANAGED_LINE)),
                    "{prefix:?} {suffix:?}"
                );
            }
        }
        let _ = fs::remove_dir_all(appdata);
    }

    /// RED (row 68c, R2, R3, R12) — **a line Folio cannot prove is its own is left, and said once**:
    /// a line naming the script that is in none of Folio's forms (another folder's script, a line
    /// with a comment added); a profile Folio may not edit that holds its line; a data root outside
    /// `%APPDATA%`, where no guarded line names the script. Each file is byte for byte as it was,
    /// each run says one line for it, and no record is made for any of them; a line Folio does own
    /// beside an edited one is carried, and the edited one is said and left.
    ///
    /// MUTATION: any line naming `folio.ps1` is taken for Folio's
    /// (`plant-68c-owns-any-folio-line.py`).
    #[test]
    fn a_line_folio_cannot_prove_is_its_own_is_left_and_said_once() {
        let (appdata, data, profile) = carry_account("carry-not-ours", persist::STORAGE_NAME);
        let left = |what: &str| {
            format!(
                "BT_SHELL_PROFILE {}: left as it is: {what}",
                profile.display()
            )
        };
        let not_folios = "a line naming folio.ps1 is not in one of Folio's forms";
        let elsewhere = format!(
            ". \"{}\"",
            appdata.join("tools").join("folio.ps1").display()
        );
        let edited = format!("{LEGACY_LINE} # mine");
        for text in [format!("{elsewhere}\r\n"), format!("# x\n{edited}\n")] {
            fs::write(&profile, &text).unwrap();
            assert_eq!(
                carry_for(&data, Some(&appdata), &profile).lines,
                [left(not_folios)]
            );
            assert_eq!(fs::read_to_string(&profile).unwrap(), text);
        }
        assert!(
            !data.join(RECORD_FILE).exists(),
            "a refusal makes no record"
        );

        let text = format!("{LEGACY_LINE}\n");
        fs::write(&profile, &text).unwrap();
        let carried = carry_for(&data, None, &profile);
        assert_eq!(carried.lines.len(), 1, "{carried:?}");
        assert!(
            carried.lines[0].starts_with(&left(LineForm::AppData.said_as())),
            "{carried:?}"
        );
        assert!(
            carried.lines[0].ends_with(crate::i18n::Text::ShellProfileOutsideAppData.text()),
            "{carried:?}"
        );
        assert_eq!(fs::read_to_string(&profile).unwrap(), text);

        let writable = fs::metadata(&profile).unwrap().permissions();
        let mut read_only = writable.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&profile, read_only).unwrap();
        let carried = carry_for(&data, Some(&appdata), &profile);
        fs::set_permissions(&profile, writable).unwrap();
        assert_eq!(carried.lines.len(), 1, "{carried:?}");
        assert!(carried.lines[0].starts_with(&left("")), "{carried:?}");
        assert_eq!(fs::read_to_string(&profile).unwrap(), text);
        assert!(
            !data.join(RECORD_FILE).exists(),
            "a refusal makes no record"
        );

        fs::write(&profile, format!("{LEGACY_LINE}\n{edited}\n")).unwrap();
        let carried = carry_for(&data, Some(&appdata), &profile);
        assert_eq!(
            fs::read_to_string(&profile).unwrap(),
            format!("{MANAGED_LINE}\n{edited}\n")
        );
        assert_eq!(
            carried.lines,
            [format!(
                "BT_SHELL_PROFILE {}: brought forward from {} to {}; {not_folios}, and it is left as it is",
                profile.display(),
                LineForm::AppData.said_as(),
                LineForm::Guarded.said_as()
            )]
        );
        let _ = fs::remove_dir_all(appdata);
    }

    /// RED (row 68c, R4) — **an account that turned the integration off keeps its lines**: the
    /// record's Off is the person's decision, so a profile holding Folio's old line is left as it
    /// is, said once, and the record is not written.
    ///
    /// MUTATION: the carry ignores the record's Off (`plant-68c-off-ignored.py`).
    #[test]
    fn an_account_that_turned_the_integration_off_keeps_its_lines() {
        let (appdata, data, profile) = carry_account("carry-off", persist::STORAGE_NAME);
        let mut marks = Marks::default();
        marks.turn_off(std::time::UNIX_EPOCH);
        marks.write(&data).unwrap();
        let record = fs::read(data.join(RECORD_FILE)).unwrap();
        let text = format!("# mine\r\n{LEGACY_LINE}\r\n");
        fs::write(&profile, &text).unwrap();
        assert_eq!(
            carry_for(&data, Some(&appdata), &profile).lines,
            [format!(
                "BT_SHELL_PROFILE {}: left as it is: {}: the PowerShell integration is off for this account",
                profile.display(),
                LineForm::AppData.said_as()
            )]
        );
        assert_eq!(fs::read_to_string(&profile).unwrap(), text);
        assert_eq!(fs::read(data.join(RECORD_FILE)).unwrap(), record);
        let _ = fs::remove_dir_all(appdata);
    }

    /// RED (row 68c, R7) — **a profile in an ANSI code page is carried byte for byte**: a BOM-less
    /// file that is not UTF-8 (Windows PowerShell's `Set-Content` writes the system code page),
    /// holding the line the 0.1.0-0.4.2 appender wrote into it as UTF-8, is given the guarded line
    /// and every other byte — the code page's own, a line that is not UTF-8 — kept. Every
    /// NUL-free byte string reads and writes back as itself. The remover and Enable read the same
    /// view (R7; the review's N7): the remover takes the line and its own terminator out of such a
    /// file and keeps every other byte, Enable appends the line's ASCII and the file's newline and
    /// changes nothing before them; a GBK file (a double-byte code page, a trail byte that is `\`)
    /// is carried the same way, and so is a literal line of 0.1.0-0.2.3 whose non-ASCII script path
    /// the appender wrote as UTF-8 into an ANSI file.
    ///
    /// MUTATION: the carried text written back as UTF-8 whatever the file was read as
    /// (`plant-68c-encoding-dropped.py`).
    #[test]
    fn a_profile_in_an_ansi_code_page_is_carried_byte_for_byte() {
        let (appdata, data, profile) = carry_account("carry-ansi", persist::STORAGE_NAME);
        let around = |line: &str| {
            [
                b"# caf\xe9 \x93quoted\x94\r\n".as_slice(),
                line.as_bytes(),
                b"\r\n\xff\xfe not UTF-8\r\n# end \xe9".as_slice(),
            ]
            .concat()
        };
        fs::write(&profile, around(LEGACY_LINE)).unwrap();
        let carried = carry_for(&data, Some(&appdata), &profile);
        assert_eq!(fs::read(&profile).unwrap(), around(MANAGED_LINE));
        assert_eq!(carried.lines.len(), 1, "{carried:?}");
        let every_byte: Vec<u8> = (1..=255).collect();
        let decoded = Decoded::read(&every_byte).unwrap();
        assert_eq!(decoded.encode(&decoded.text), every_byte);
        assert_eq!(
            rewrite(&every_byte, &Forms::new(&[]), Action::Migrate).unwrap(),
            None
        );

        // The remover: the line and its own terminator out, every other byte kept.
        let without = [
            b"# caf\xe9 \x93quoted\x94\r\n".as_slice(),
            b"\xff\xfe not UTF-8\r\n# end \xe9".as_slice(),
        ]
        .concat();
        for line in [LEGACY_LINE, MANAGED_LINE] {
            assert_eq!(
                rewrite(&around(line), &Forms::new(&[]), Action::Remove).unwrap(),
                Some(without.clone()),
                "{line}"
            );
        }
        // Enable: the line's ASCII and the file's newline after the last byte, nothing before
        // them changed.
        fs::write(&profile, &without).unwrap();
        super::super::add_to_profile(&profile, MANAGED_LINE, std::time::SystemTime::now()).unwrap();
        assert_eq!(
            fs::read(&profile).unwrap(),
            [
                without.as_slice(),
                b"\r\n\r\n",
                MANAGED_LINE.as_bytes(),
                b"\r\n"
            ]
            .concat()
        );

        // GBK: 中文配置 and a character whose trail byte is `\`, around Folio's line.
        let gbk = |line: &str| {
            [
                b"# \xd6\xd0\xce\xc4\xc5\xe4\xd6\xc3 \x81\x5c\r\n".as_slice(),
                line.as_bytes(),
                b"\r\n# \xb5\xc4\r\n".as_slice(),
            ]
            .concat()
        };
        fs::write(&profile, gbk(LEGACY_LINE)).unwrap();
        let carried = carry_for(&data, Some(&appdata), &profile);
        assert_eq!(fs::read(&profile).unwrap(), gbk(MANAGED_LINE));
        assert_eq!(carried.lines.len(), 1, "{carried:?}");

        // A literal line whose script path is not ASCII, in the appender's UTF-8 bytes, inside an
        // ANSI file.
        let script = PathBuf::from(r"D:\用户 目录\Folio\shell-integration\folio.ps1");
        let literal = format!(". \"{}\"", script.display());
        assert_eq!(
            rewrite(
                &around(literal.as_str()),
                &Forms::new(&[script]),
                Action::Migrate
            )
            .unwrap(),
            Some(around(MANAGED_LINE))
        );
        let _ = fs::remove_dir_all(appdata);
    }
}
