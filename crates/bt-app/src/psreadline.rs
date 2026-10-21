//! The PSReadLine a Windows PowerShell pane of Folio's runs, and the copy an earlier Folio left in
//! the account's Documents — `docs/plans/design/integration-inject-2-2026-10-15.md`,
//! T-INTEGRATION-INJECT-2-BUILD (owner rulings of 2026-10-15).
//!
//! # What a 5.1 pane runs
//!
//! Microsoft's signed PSReadLine [`SHIPPED_VERSION`], taken out of the PowerShell Gallery package
//! pinned in `vendor/psreadline/` by the build script and embedded here ([`module_files`]). A
//! Windows PowerShell pane that gets Folio's loader is given a module path whose first entry holds
//! only this module (`shell_integration::declare_psreadline_module`), so the host's own import —
//! before any profile runs — loads it, under every execution policy: the module is binary-only.
//! `folio.ps1` repairs its resize anchor by reflection on Folio's resize chord, as it does pwsh 7's
//! in-box 2.4.5. Nothing is written outside Folio's own folders and nobody is asked anything: the
//! invitation, the Terminal page's install row, the out-of-band probe, the Documents installer
//! and the launch-time upgrade are retired (Q3).
//!
//! # The copy an earlier Folio wrote
//!
//! Folio 0.1.0–0.4.7 offered to install its fork of PSReadLine (`2.4.6-bt.*`) into
//! `<Documents>\WindowsPowerShell\Modules\PSReadLine\2.4.6\`, where every Windows PowerShell host
//! of the account loads it. That copy stays where it is (Q4): Windows Terminal and the other hosts
//! keep what they have. While one is there the Terminal page offers its removal
//! ([`legacy_copy_present`], [`begin_legacy_removal`]), and the uninstall removes it
//! (`uninstall::Remover::PsReadLine`). Which copy is Folio's is decided as before — the nine files
//! byte for byte the fork Folio last shipped, or the fork's `-bt.` build stamp in the assembly —
//! except that the bytes themselves are gone from the executable: what is kept is their SHA-256
//! ([`LEGACY_FILES`]) and the stamp ([`PATCHED_BUILD`]).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::i18n::Text;

/// **The PSReadLine a Windows PowerShell 5.1 pane of Folio's runs** (T-INTEGRATION-INJECT-2-BUILD;
/// owner ruling Q1, 2026-10-15): Microsoft's signed 2.4.5 from the PowerShell Gallery, the version
/// pwsh 7 ships in-box, so both editions run one editor and `folio.ps1` repairs its anchor one way.
///
/// **The one Rust literal of this version**, bound by test to the pinned table's package and to
/// Folio's own manifest (`assets/psreadline/PSReadLine.psd1`), so a pin moved without it — or it
/// without the pin — is red, not a module whose directory and manifest disagree.
pub const SHIPPED_VERSION: &str = "2.4.5";

/// **The artefact's name**: the first twelve hex digits of the pinned package's SHA-256, from the
/// build script (`src/psreadline_artefact.rs`'s `artefact`) — the directory under `psmodules` this
/// build's module is written into.
pub const ARTEFACT: &str = env!("FOLIO_PSREADLINE_ARTEFACT");

/// The module's files as the build script wrote them: rows of a length-prefixed path and
/// length-prefixed bytes ([`module_rows`]). Empty off Windows.
static MODULE_CONTAINER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/psreadline_module.bin"));

static MODULE_FILES: std::sync::LazyLock<Vec<(&'static str, &'static [u8])>> =
    std::sync::LazyLock::new(|| module_rows(MODULE_CONTAINER));

/// **The module this executable carries** — the four assemblies, the licence and Folio's
/// manifest, each by its path `PSReadLine/2.4.5/<file>` under the artefact's directory, the
/// manifest last — read out of the build script's container once. Empty off Windows: Windows
/// PowerShell is a Windows fact, and so are the bytes.
#[must_use]
pub fn module_files() -> &'static [(&'static str, &'static [u8])] {
    &MODULE_FILES
}

/// The rows of a container the build script wrote: each the path's length as a little-endian
/// `u32`, the path, the bytes' length the same way, the bytes. The container is this
/// executable's own, written by its build script, so a row that does not read is a broken build.
fn module_rows(mut container: &'static [u8]) -> Vec<(&'static str, &'static [u8])> {
    fn part(container: &mut &'static [u8]) -> &'static [u8] {
        let (length, rest) = container
            .split_first_chunk::<4>()
            .expect("the build script's container: a length");
        let length = u32::from_le_bytes(*length) as usize;
        let (part, rest) = rest.split_at(length);
        *container = rest;
        part
    }
    let mut rows = Vec::new();
    while !container.is_empty() {
        let name = std::str::from_utf8(part(&mut container))
            .expect("the build script's container: a path is UTF-8");
        rows.push((name, part(&mut container)));
    }
    rows
}

/// **The module's assembly**, by its file name in a version directory — every PSReadLine's,
/// Folio's 2.4.5 and the earlier fork alike: the file a shell maps when it loads the module, and
/// so the one whose deletion tells a held artefact from a free one
/// (`shell_integration::prune_psreadline_artefacts`), and the one whose version resource carries
/// the fork's build stamp.
pub const ASSEMBLY: &str = "Microsoft.PowerShell.PSReadLine.dll";

/// The module's name: the directory a module path is searched for it by.
pub const MODULE_NAME: &str = "PSReadLine";

/// What is directly in `directory`, and which of it is itself a directory — the installed-module
/// IO door's listing (`installed_disk`), which the pruning of Folio's own artefacts reads with
/// too. Names, not content.
pub fn directory_entries(directory: &Path) -> io::Result<Vec<(OsString, bool)>> {
    installed_disk::entries(directory)
}

/// Windows' script execution policy, as `Get-ExecutionPolicy` reports it — the Profiles page's
/// `$PROFILE` rows decide with it (`shell_integration::policy_cause`).
///
/// The six names are kept apart, because a row puts the policy's own name on screen and "the
/// policy is Blocked" is not a sentence Windows would ever write.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExecutionPolicy {
    Restricted,
    AllSigned,
    RemoteSigned,
    Unrestricted,
    Bypass,
    /// No policy is set anywhere, which on a client Windows behaves as `Restricted` — but
    /// `Get-ExecutionPolicy` prints `Undefined`, and this is the name that goes on screen.
    Undefined,
    /// PowerShell did not answer, or answered something this build has no name for.
    #[default]
    Unknown,
}

impl ExecutionPolicy {
    #[must_use]
    /// A policy's name in any case, as PowerShell reads it — a probe prints the canonical
    /// spelling, and a row's own `-ExecutionPolicy bypass` is the same policy.
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        [
            Self::Restricted,
            Self::AllSigned,
            Self::RemoteSigned,
            Self::Unrestricted,
            Self::Bypass,
            Self::Undefined,
        ]
        .into_iter()
        .find(|policy| policy.name().eq_ignore_ascii_case(text))
        .unwrap_or(Self::Unknown)
    }

    /// `Get-ExecutionPolicy`'s own word.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Restricted => "Restricted",
            Self::AllSigned => "AllSigned",
            Self::RemoteSigned => "RemoteSigned",
            Self::Unrestricted => "Unrestricted",
            Self::Bypass => "Bypass",
            Self::Undefined => "Undefined",
            Self::Unknown => "Unknown",
        }
    }

    /// Whether this policy refuses an unsigned **script** — the profile, a `.ps1`.
    #[must_use]
    pub fn blocks_script(self) -> bool {
        matches!(self, Self::Restricted | Self::AllSigned)
    }
}

// ── the copy an earlier Folio wrote into Documents (owner ruling Q4) ────────

/// The fork's module version — the `ModuleVersion` every `-bt` build Folio shipped carries, and the
/// directory under `PSReadLine` it was written into.
pub const PATCHED_VERSION: &str = "2.4.6";

/// **The build stamp of the last fork Folio shipped** (0.4.7): the `ProductVersion` string in the
/// assembly's Win32 version resource, and the whole of how one Folio-patched PSReadLine was told
/// from another. A copy stamped with it but not byte for byte [`LEGACY_FILES`] has been edited
/// since Folio wrote it, and is somebody's own module now.
pub const PATCHED_BUILD: &str = "2.4.6-bt.2";

/// What every Folio-patched build's `ProductVersion` begins with: a string only this project's
/// builds put there, so it is a claim about *who wrote the file*.
#[must_use]
pub fn family_prefix() -> String {
    format!("{PATCHED_VERSION}-bt.")
}

/// The file whose version resource carries the build stamp.
const BUILD_STAMP_FILE: &str = ASSEMBLY;

/// Where a per-user module for Windows PowerShell 5.1 lives, under Documents.
pub const MODULE_RELATIVE_PATH: &str = r"WindowsPowerShell\Modules\PSReadLine";

/// **The nine files of the last fork Folio shipped, by SHA-256** — `assets/psreadline/2.4.6/` of
/// 0.4.7, reduced to what the legacy copy's check needs (T-INTEGRATION-INJECT-2-BUILD step 3): a
/// directory whose nine files hash to these is Folio's own copy byte for byte, which a removal
/// may take. The bytes left the executable with the fork.
pub const LEGACY_FILES: [(&str, &str); 9] = [
    (
        "Changes.txt",
        "f9bd56cea86549fcee75c669271d5841552f1b7bb391b7945fbab021160fda5b",
    ),
    (
        "License.txt",
        "8e76479b22a7cd790f1e131df20b92feeab92b09c684d1c86e715048596c88b4",
    ),
    (
        "Microsoft.PowerShell.PSReadLine.dll",
        "833545030dbaf76fd86189046927526aed839d302212338975d4af4d1355e73b",
    ),
    (
        "Microsoft.PowerShell.Pager.dll",
        "451994c3d3e38d939b4fa5f8594d0207840f45e7a210aa75f81aef85aa17c592",
    ),
    (
        "PSReadLine.format.ps1xml",
        "c8df0ec28190f01867cb2aa88c20aef18282bcc6cd85c42972278d70bed839d1",
    ),
    (
        "PSReadLine.psd1",
        "bb8acae6c8dac3a793d4ae225ddd253e4e57c1095d9a29e03bda029bd3fe8d40",
    ),
    (
        "PSReadLine.psm1",
        "7c6f4220231b4f7cf4e6685119fc51735a9edf41786d3e7dab41bbb1530dbd6e",
    ),
    (
        "net6plus/Microsoft.PowerShell.PSReadLine.Polyfiller.dll",
        "86bc889ed027d935d4c66b27782e277a5717ad941f8bae4fc069c3eff95065ee",
    ),
    (
        "netstd/Microsoft.PowerShell.PSReadLine.Polyfiller.dll",
        "f5bed4df0c30b8653254abe15e5451cb5451bbb5ce44242f7bb3e91ed3460788",
    ),
];

/// The directory the fork was written into, under a Documents folder.
#[must_use]
pub fn module_directory(documents: &Path) -> PathBuf {
    documents.join(MODULE_RELATIVE_PATH).join(PATCHED_VERSION)
}

/// Where this machine's Documents folder is, asked of Windows — the known folder, redirection
/// honoured — or the sandbox `BT_PSREADLINE_DOCUMENTS` names.
///
/// `BT_PSREADLINE_DOCUMENTS=<dir>` moves the whole Documents root, read and remove together, so
/// the legacy copy's row and the uninstall can be exercised for real inside a scratch directory
/// and never against a developer's own `Documents\WindowsPowerShell\Modules`. Folio's panes name
/// the same folder in their module path (`shell_integration::compose_module_path`).
#[must_use]
pub fn documents_directory() -> Option<PathBuf> {
    if let Some(sandbox) = std::env::var_os("BT_PSREADLINE_DOCUMENTS") {
        let sandbox = PathBuf::from(sandbox);
        if !sandbox.as_os_str().is_empty() {
            return Some(sandbox);
        }
    }
    #[cfg(windows)]
    {
        bt_platform::documents_directory()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Historical roots must have exactly the shape the fork's installer wrote.
pub fn documents_for_module_root(root: &Path) -> Option<PathBuf> {
    let documents = root
        .ancestors()
        .find(|parent| module_directory(parent) == root)?;
    Some(documents.to_owned())
}

/// Who owns the module directory under a Documents root.
///
/// Four answers and not a `bool`: [`Self::OlderBuild`] (the 2026-08-18 ruling) and
/// [`Self::Foreign`] (audit 3, E-1) are each a different sentence from "Folio wrote exactly this"
/// and from "nothing of Folio's is here".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InstalledCopy {
    /// Nothing is there, or what is there is the fork's own work in a state the hashes will not
    /// vouch for: an interrupted write, or an edit somebody made to it. Not Folio's to remove.
    #[default]
    None,
    /// Byte for byte the fork Folio last shipped ([`LEGACY_FILES`]).
    ThisBuild,
    /// A `2.4.6-bt.*` that is not that one — an older Folio's work.
    OlderBuild,
    /// **A module Folio did not write.** Never touched.
    Foreign,
}

/// Which copy of the fork is under `documents`.
///
/// **Byte identity first** — the nine files hash to [`LEGACY_FILES`] — and the family stamp only
/// where identity cannot answer: a build whose bytes are not the last fork's. **A copy stamped
/// [`PATCHED_BUILD`] but not byte-identical is `None`**: an edited `psm1` beside Folio's DLL is
/// somebody's own module now, and the strongest answer available about it overrules the weaker.
#[must_use]
pub fn installed_copy(documents: &Path) -> InstalledCopy {
    installed_copy_against(documents, &LEGACY_FILES, PATCHED_BUILD)
}

/// [`installed_copy`] against a table and a stamp handed in — the seam its tests hold the
/// identity rule with, over files they write themselves.
fn installed_copy_against(
    documents: &Path,
    table: &[(&str, &str)],
    stamped: &str,
) -> InstalledCopy {
    let root = module_directory(documents);
    if !installed_disk::is_dir(&root) {
        return InstalledCopy::None;
    }
    if matches_table(&root, table) {
        return InstalledCopy::ThisBuild;
    }
    match installed_build(documents) {
        Some(build) if build == stamped => InstalledCopy::None,
        Some(_) => InstalledCopy::OlderBuild,
        None => unclaimed_or_foreign(&root, table),
    }
}

/// Whether every file of `table` under `root` hashes to its pin.
fn matches_table(root: &Path, table: &[(&str, &str)]) -> bool {
    table
        .iter()
        .all(|(name, pinned)| file_matches(&root.join(name), pinned))
}

fn file_matches(path: &Path, pinned: &str) -> bool {
    installed_disk::read(path)
        .is_ok_and(|bytes| bt_winres::digest::hex(&bt_winres::digest::sha256(&bytes)) == pinned)
}

/// Whether a directory that carries no `-bt` stamp is **free** or **somebody else's** (audit 3,
/// E-1). **The assembly decides**: whoever wrote it wrote the module PowerShell loads; with no
/// assembly at all, a leaf holding nothing but the fork's names is an interrupted write of
/// Folio's, and anything else in it belongs to somebody. An assembly that cannot be read has no
/// established owner, and an unestablished owner is never Folio.
fn unclaimed_or_foreign(root: &Path, table: &[(&str, &str)]) -> InstalledCopy {
    let assembly = root.join(BUILD_STAMP_FILE);
    let pinned = table
        .iter()
        .find(|(name, _)| *name == BUILD_STAMP_FILE)
        .map(|(_, pinned)| *pinned);
    match installed_disk::read(&assembly) {
        Ok(bytes)
            if pinned.is_some_and(|pinned| {
                bt_winres::digest::hex(&bt_winres::digest::sha256(&bytes)) == pinned
            }) =>
        {
            InstalledCopy::None
        }
        Ok(_) => InstalledCopy::Foreign,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if nothing_but_our_names(root, table) {
                InstalledCopy::None
            } else {
                InstalledCopy::Foreign
            }
        }
        Err(_) => InstalledCopy::Foreign,
    }
}

/// Whether every entry under `root` is one of `table`'s names, walked and not sampled; the only
/// directories the fork put anything in are `net6plus` and `netstd`.
fn nothing_but_our_names(root: &Path, table: &[(&str, &str)]) -> bool {
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let Ok(entries) = installed_disk::entries(&root.join(&relative)) else {
            return false;
        };
        for (name, is_directory) in entries {
            let child = relative.join(&name);
            let ours = table.iter().any(|(named, _)| {
                let named = Path::new(named);
                if is_directory {
                    named.starts_with(&child) && named != child
                } else {
                    named == child
                }
            });
            if !ours {
                return false;
            }
            if is_directory {
                pending.push(child);
            }
        }
    }
    true
}

/// The `ProductVersion` stamped into the module under `documents`, if it belongs to **Folio's own
/// patch family** (`-bt.`); `None` for no directory, no version resource, and a stock module.
#[must_use]
pub fn installed_build(documents: &Path) -> Option<String> {
    let stamp = module_directory(documents).join(BUILD_STAMP_FILE);
    let build = installed_disk::product_version(&stamp)?;
    build.starts_with(&family_prefix()).then_some(build)
}

/// The installed-module IO door. Tests replace all four operations on their own thread; neither a
/// real account's module nor its version resource is touched.
mod installed_disk {
    use super::*;

    pub(super) trait Disk {
        fn is_dir(&self, path: &Path) -> bool;
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;
        fn product_version(&self, path: &Path) -> Option<String>;
        /// What is directly in `directory`, and which of it is itself a directory. **Names, not
        /// content** — no file is opened, so this door carries no lane.
        fn entries(&self, directory: &Path) -> std::io::Result<Vec<(OsString, bool)>>;
    }

    pub(super) struct System;

    impl Disk for System {
        fn is_dir(&self, path: &Path) -> bool {
            path.is_dir()
        }
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            // Through the read ledger: this is the door that ran away on 2026-09-20, and the
            // tripwire has to be able to see it.
            bt_platform::file_reads::read(bt_platform::file_reads::Lane::Settings, path)
        }
        fn product_version(&self, path: &Path) -> Option<String> {
            file_product_version(path)
        }
        fn entries(&self, directory: &Path) -> std::io::Result<Vec<(OsString, bool)>> {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let is_directory = entry.file_type()?.is_dir();
                entries.push((entry.file_name(), is_directory));
            }
            Ok(entries)
        }
    }

    #[cfg(test)]
    thread_local! {
        pub(super) static OVERRIDE: std::cell::RefCell<Option<std::rc::Rc<dyn Disk>>> =
            const { std::cell::RefCell::new(None) };
    }

    fn with<R>(f: impl FnOnce(&dyn Disk) -> R) -> R {
        #[cfg(test)]
        if let Some(disk) = OVERRIDE.with(|slot| slot.borrow().clone()) {
            return f(disk.as_ref());
        }
        f(&System)
    }

    pub(super) fn is_dir(path: &Path) -> bool {
        with(|disk| disk.is_dir(path))
    }
    pub(super) fn read(path: &Path) -> std::io::Result<Vec<u8>> {
        with(|disk| disk.read(path))
    }
    pub(super) fn product_version(path: &Path) -> Option<String> {
        with(|disk| disk.product_version(path))
    }
    pub(super) fn entries(directory: &Path) -> std::io::Result<Vec<(OsString, bool)>> {
        with(|disk| disk.entries(directory))
    }
}

/// A file's `ProductVersion`, asked of Windows — and `None` everywhere else: this crate builds on
/// hosts that have no version resources at all, and a machine that cannot be asked has no fork on
/// it either.
fn file_product_version(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        bt_platform::file_product_version(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Delete the fork, and only if [`installed_copy`] says a Folio build wrote it.
///
/// The check is inside rather than at the call site, because a delete guarded from outside is a
/// delete the next caller performs unguarded. **The nine names go, and the directories only while
/// they are empty** (audit 3, E-1): on a machine an older Folio had already written over,
/// PowerShellGet's `PSGetModuleInfo.xml`, `en-US\` and catalog stand beside Folio's files, and
/// they are not Folio's to take.
pub fn remove_from(documents: &Path) -> io::Result<Removed> {
    let root = module_directory(documents);
    match installed_copy(documents) {
        InstalledCopy::None => return Ok(Removed::Nothing),
        InstalledCopy::Foreign => return Ok(Removed::NotOurs),
        InstalledCopy::ThisBuild | InstalledCopy::OlderBuild => {}
    }
    for (name, _) in LEGACY_FILES {
        match std::fs::remove_file(root.join(name)) {
            Ok(()) => {}
            // An interrupted install leaves some of the nine missing, and a removal that refused
            // over one absent file would be a module this product could not take off a machine
            // it half-wrote.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    for name in ["net6plus", "netstd"] {
        let _ = std::fs::remove_dir(root.join(name));
    }
    if std::fs::remove_dir(&root).is_ok() {
        return Ok(Removed::Took { left: Vec::new() });
    }
    let left = installed_disk::entries(&root).map_or_else(
        |_| Vec::new(),
        |entries| {
            entries
                .into_iter()
                .map(|(name, _)| root.join(name))
                .collect()
        },
    );
    Ok(Removed::Took { left })
}

/// What [`remove_from`] did to the module directory: *removed*, *not present*, and *left standing
/// because it is not Folio's* — three different lines for the door that runs unattended
/// (`folio --uninstall-cleanup`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Removed {
    /// Folio's own files went. `left` is what stood beside them and stayed, because Folio did not
    /// write it — empty when the directory itself went.
    Took { left: Vec<PathBuf> },
    /// Nothing of Folio's was there to take.
    Nothing,
    /// A module Folio did not write. It is exactly where it was.
    NotOurs,
}

// ── the Terminal page's removal row for that copy (Q4) ──────────────────────

/// **Whether an earlier Folio's copy is in a Documents root the remover would act on** — the
/// worker-observed answer the Terminal page reads ([`legacy_copy_present`]). Published by the launch's
/// observation ([`begin_legacy_observation`]) and by every removal ([`begin_legacy_removal`]);
/// `false` until the first observation answers, so the row never stands before it is known.
static LEGACY_PRESENT: AtomicBool = AtomicBool::new(false);

/// **Every press's answer, addressed to the window that pressed** — each its own, filed once, and
/// taken by that window on its next wake ([`take_legacy_removals`]); an answer for a window that
/// has since closed is delivered nowhere.
static LEGACY_REMOVALS: Mutex<Vec<(winit::window::WindowId, LegacyRemoval)>> =
    Mutex::new(Vec::new());

/// Whether the Terminal page shows the legacy copy's removal row. Reading Settings never touches the
/// disk.
#[must_use]
pub fn legacy_copy_present() -> bool {
    LEGACY_PRESENT.load(Ordering::Acquire)
}

/// **Every Documents root an earlier Folio may have written the fork under**: the one Windows
/// names now (or the sandbox), and each root the marks record holds (`psreadline_module_roots`,
/// written by 0.4.3–0.4.7 before their first byte), which is how a copy written to a Documents
/// that has since been redirected is found. The marks record is only read; nothing writes its
/// PSReadLine entry any more.
fn legacy_roots(documents: Option<PathBuf>, data: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = documents.into_iter().collect();
    if let Ok(marks) = crate::shell_integration::profile_marks::Marks::read(data) {
        for root in marks.psreadline_module_roots {
            if let Some(documents) = documents_for_module_root(&root)
                && !roots.contains(&documents)
            {
                roots.push(documents);
            }
        }
    }
    roots
}

/// **The row's predicate: a root whose copy the remover would take** — [`installed_copy`] at
/// [`InstalledCopy::ThisBuild`] or [`InstalledCopy::OlderBuild`], the two [`remove_from`] acts on,
/// and nothing weaker. A root merely named by the marks record, or a directory that merely
/// exists, is not a row: a row with nothing to remove is the INJECT follow-up F8 defect.
#[must_use]
pub fn legacy_removable(roots: &[PathBuf]) -> bool {
    roots.iter().any(|documents| {
        matches!(
            installed_copy(documents),
            InstalledCopy::ThisBuild | InstalledCopy::OlderBuild
        )
    })
}

/// **What a press on the row did.**
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LegacyRemoval {
    /// Folio's files went from these module directories; what stood beside them and stayed is
    /// listed (somebody else's).
    Removed {
        paths: Vec<PathBuf>,
        left: Vec<PathBuf>,
    },
    /// There was nothing of Folio's to take any more — somebody removed it since the row was
    /// drawn. Said nowhere: the row goes, and nothing changed.
    Nothing,
    /// A delete failed; the operating system's words, and the directory.
    Refused { path: PathBuf, message: String },
    /// The worker would not start or stopped part way.
    Unfinished(Text),
}

impl LegacyRemoval {
    /// What a window is owed: whether it is a refusal, and the sentence; `None` for
    /// [`Self::Nothing`].
    #[must_use]
    pub fn window_text(&self) -> Option<(bool, String)> {
        match self {
            Self::Removed { .. } => {
                Some((false, Text::LegacyPsReadLineRemovedToast.text().to_owned()))
            }
            Self::Nothing => None,
            Self::Refused { path, message } => Some((
                true,
                crate::i18n::psreadline_remove_failed(&path.display().to_string(), message),
            )),
            Self::Unfinished(sentence) => Some((true, sentence.text().to_owned())),
        }
    }
}

/// **The removal over `roots`**, every removable one: [`remove_from`] for each, and the first
/// failure ends the run with its directory named.
fn remove_legacy_copies(roots: &[PathBuf]) -> LegacyRemoval {
    let mut paths = Vec::new();
    let mut left = Vec::new();
    for documents in roots {
        let path = module_directory(documents);
        match remove_from(documents) {
            Ok(Removed::Took { left: stayed }) => {
                paths.push(path);
                left.extend(stayed);
            }
            Ok(Removed::Nothing | Removed::NotOurs) => {}
            Err(error) => {
                return LegacyRemoval::Refused {
                    path,
                    message: error.to_string(),
                };
            }
        }
    }
    if paths.is_empty() {
        LegacyRemoval::Nothing
    } else {
        LegacyRemoval::Removed { paths, left }
    }
}

fn publish_legacy(present: bool) {
    LEGACY_PRESENT.store(present, Ordering::Release);
}

/// **One press's answer, filed once**: by the worker as soon as the removal is known, or by the
/// worker's end when it did not get that far — never by both, so a panic after the answer was
/// filed leaves the true answer standing (`profile_runtime::ClickAnswer`'s rule).
struct PressAnswer {
    window: winit::window::WindowId,
    filed: std::sync::atomic::AtomicBool,
}

impl PressAnswer {
    fn file(&self, answer: LegacyRemoval) {
        if !self.filed.swap(true, Ordering::AcqRel) {
            if let Ok(mut answers) = LEGACY_REMOVALS.lock() {
                answers.push((self.window, answer));
            }
            crate::shell_integration::wake_for_profile_answers();
        }
    }
}

/// **Observe the legacy copy once, on a worker nobody waits for** — at launch, so the Terminal page
/// can say whether there is one without reading the disk. A start the thread door refuses, or a
/// panic, is said once and answered as no copy: the row stays hidden, and the next launch asks
/// again.
pub fn begin_legacy_observation() {
    let _ = crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal)
        .start_worker(
            "psreadline-legacy-observation",
            crate::lane::Answered::new(
                "the earlier PSReadLine copy's observation",
                crate::diagnostics::note,
                |_| publish_legacy(false),
            ),
            |_| {
                let present = legacy_removable(&legacy_roots(
                    documents_directory(),
                    &crate::persist::storage_dir(),
                ));
                publish_legacy(present);
                if present {
                    crate::shell_integration::wake_for_profile_answers();
                }
            },
        );
}

/// **The Terminal page's "remove the earlier PSReadLine copy", pressed in `window`**: started at the
/// thread door, below normal; the worker removes every removable copy, publishes whether one is
/// left, and answers the window that pressed ([`take_legacy_removals`]). A start the door refused,
/// or a panic, is said once and answered as an unfinished removal, which the window raises as a
/// refusal; the row stays, so the same click asks again.
pub fn begin_legacy_removal(window: winit::window::WindowId) {
    start_legacy_removal(
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
        crate::diagnostics::note,
        window,
        crate::persist::storage_dir(),
    );
}

/// [`begin_legacy_removal`] with the executor, the voice and the data folder handed in; `true`
/// when the worker started.
pub(crate) fn start_legacy_removal(
    executor: &crate::lane::Executor,
    say: impl Fn(&str) + Send + Sync + 'static,
    window: winit::window::WindowId,
    data: PathBuf,
) -> bool {
    let answer = std::sync::Arc::new(PressAnswer {
        window,
        filed: std::sync::atomic::AtomicBool::new(false),
    });
    let ended = std::sync::Arc::clone(&answer);
    executor
        .start_worker(
            "psreadline-legacy-removal",
            crate::lane::Answered::new("the earlier PSReadLine copy's removal", say, move |end| {
                ended.file(LegacyRemoval::Unfinished(crate::i18n::unfinished_change(
                    end.started(),
                )));
            }),
            move |_| {
                let roots = legacy_roots(documents_directory(), &data);
                let removal = remove_legacy_copies(&roots);
                publish_legacy(legacy_removable(&roots));
                answer.file(removal);
            },
        )
        .is_ok()
}

/// Every press's answer filed since the last take, each with the window that pressed.
pub fn take_legacy_removals() -> Vec<(winit::window::WindowId, LegacyRemoval)> {
    LEGACY_REMOVALS
        .lock()
        .map(|mut answers| std::mem::take(&mut *answers))
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = bt_testpath::temp_path(&format!("folio-psreadline-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **the version Folio's panes run is one number in
    /// four places: [`SHIPPED_VERSION`], the pinned package, Folio's manifest and the directory
    /// the module is written into.**
    ///
    /// PowerShell resolves a module's version from the manifest's `ModuleVersion`, and finds a
    /// version on a module path by its directory; a manifest that said one number in a directory
    /// named for another is a module `Get-Module -ListAvailable` reports one way and loads the
    /// other. The table is the build script's (`psreadline_artefact`), read as values; the manifest
    /// is the bytes this executable embeds. On Windows the embedded table is the module itself —
    /// six files, the manifest last — and off Windows it is empty (the note's §5.4: the bytes are
    /// the Windows executable's only).
    ///
    /// MUTATION: set `SHIPPED_VERSION` to `"2.4.6"` alone — the first assertion names it.
    #[test]
    fn the_shipped_version_is_the_package_the_manifest_and_the_directory() {
        use crate::psreadline_artefact::{MANIFEST, MODULE, PACKAGE, VERSION, artefact};
        assert_eq!(
            SHIPPED_VERSION, VERSION,
            "SHIPPED_VERSION is the pinned table's version"
        );
        assert_eq!(PACKAGE, format!("{MODULE}.{SHIPPED_VERSION}.nupkg"));
        assert_eq!(MODULE, MODULE_NAME);
        assert_eq!(ARTEFACT, artefact());
        let module_files = module_files();
        if bt_platform::host_platform() != bt_platform::HostPlatform::Windows {
            assert!(module_files.is_empty(), "no PSReadLine bytes off Windows");
            return;
        }
        let directory = format!("{MODULE}/{SHIPPED_VERSION}/");
        assert_eq!(module_files.len(), 6);
        assert_eq!(module_files[0].0, format!("{directory}{ASSEMBLY}"));
        for (path, bytes) in module_files {
            assert!(path.starts_with(&directory), "{path} is under {directory}");
            assert!(!bytes.is_empty(), "{path} is not empty");
        }
        let (last, manifest) = module_files[module_files.len() - 1];
        assert_eq!(
            last,
            format!("{directory}{MANIFEST}"),
            "the manifest is written last"
        );
        let manifest = String::from_utf8_lossy(manifest);
        assert!(
            manifest.contains(&format!("ModuleVersion = '{SHIPPED_VERSION}'")),
            "the embedded manifest declares {SHIPPED_VERSION}"
        );
    }

    thread_local! {
        /// The version resources this thread's legacy-copy questions are answered, by path.
        static STAMPS: std::cell::RefCell<Vec<(PathBuf, String)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// A disk for the legacy copy's tests: the real files under the test's own directory, and a
    /// version resource answered by name ([`STAMPS`]) — the stamp a fork's assembly would carry,
    /// which a fixture cannot write into bytes of its own.
    struct StampedDisk;

    impl installed_disk::Disk for StampedDisk {
        fn is_dir(&self, path: &Path) -> bool {
            installed_disk::System.is_dir(path)
        }
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            installed_disk::System.read(path)
        }
        fn product_version(&self, path: &Path) -> Option<String> {
            STAMPS.with(|stamps| {
                stamps
                    .borrow()
                    .iter()
                    .find(|(stamped, _)| stamped == path)
                    .map(|(_, stamp)| stamp.clone())
            })
        }
        fn entries(&self, directory: &Path) -> std::io::Result<Vec<(OsString, bool)>> {
            installed_disk::System.entries(directory)
        }
    }

    /// **While it is held, this thread's legacy-copy questions read the test's own files and the
    /// stamps it names** ([`installed_disk`]'s seam); the last one dropped, the real disk again.
    /// Stamps accumulate, so a test that lays two copies holds two guards.
    pub(crate) struct LegacyDisk;

    impl Drop for LegacyDisk {
        fn drop(&mut self) {
            installed_disk::OVERRIDE.with(|slot| *slot.borrow_mut() = None);
            STAMPS.with(|stamps| stamps.borrow_mut().clear());
        }
    }

    /// The disk with each `(documents, stamp)`'s assembly answering `stamp`, beside the stamps
    /// already given on this thread.
    pub(crate) fn stamped(copies: &[(&Path, &str)]) -> LegacyDisk {
        STAMPS.with(|stamps| {
            stamps
                .borrow_mut()
                .extend(copies.iter().map(|(documents, stamp)| {
                    (
                        module_directory(documents).join(BUILD_STAMP_FILE),
                        (*stamp).to_owned(),
                    )
                }));
        });
        installed_disk::OVERRIDE
            .with(|slot| *slot.borrow_mut() = Some(std::rc::Rc::new(StampedDisk)));
        LegacyDisk
    }

    /// **The fork's nine names laid down under `documents`**, each holding bytes of the test's own
    /// (`tag` and the name), and the table those bytes hash to — the copy an earlier Folio wrote,
    /// as far as a test can write one without the fork's bytes.
    pub(crate) fn lay_legacy_copy(documents: &Path, tag: &str) -> Vec<(&'static str, String)> {
        let root = module_directory(documents);
        LEGACY_FILES
            .iter()
            .map(|(name, _)| {
                let path = root.join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                let bytes = format!("{tag} {name}");
                std::fs::write(&path, &bytes).unwrap();
                (
                    *name,
                    bt_winres::digest::hex(&bt_winres::digest::sha256(bytes.as_bytes())),
                )
            })
            .collect()
    }

    /// A copy an older Folio wrote, under `documents`, whose assembly carries `stamp`; answers the
    /// disk that says so.
    pub(crate) fn an_older_folio_copy_in(documents: &Path, stamp: &str) -> LegacyDisk {
        lay_legacy_copy(documents, "older");
        stamped(&[(documents, stamp)])
    }

    /// **The copy an earlier Folio wrote, and the record it kept** — what 0.4.3–0.4.7's
    /// `install_recorded` left: an older Folio's copy under `documents` and its root in the marks
    /// record under `data`. The uninstall's tests seed the account with it.
    pub(crate) fn recorded_older_copy_in(documents: &Path, data: &Path) -> LegacyDisk {
        use crate::shell_integration::profile_marks::Marks;
        let disk = an_older_folio_copy_in(documents, "2.4.6-bt.1");
        let mut marks = Marks::read(data).unwrap_or_default();
        let root = module_directory(documents);
        if !marks.psreadline_module_roots.contains(&root) {
            marks.psreadline_module_roots.push(root);
        }
        marks.write(data).unwrap();
        disk
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **the legacy copy is Folio's by its nine hashes, or by
    /// an older Folio's stamp, and by nothing weaker** — the rule that decides the Terminal page's
    /// removal row and the uninstall, over an injected disk answer.
    ///
    /// Byte for byte the table is [`InstalledCopy::ThisBuild`] (no stamp needed); an older
    /// `-bt.` stamp over other bytes is [`InstalledCopy::OlderBuild`]; the last fork's own stamp
    /// over bytes that are not its own is an edit (`None`); a module with no stamp and somebody's
    /// assembly is [`InstalledCopy::Foreign`]; a leaf holding only the fork's names and no assembly
    /// is an interrupted write (`None`); nothing at all is `None`.
    ///
    /// MUTATION: answer `ThisBuild` from the stamp alone (drop `matches_table`) — the edited copy
    /// stops being `None`.
    #[test]
    fn the_legacy_copy_is_folios_by_its_hashes_or_an_older_stamp_and_nothing_weaker() {
        let root = temp_dir("legacy-identity");
        let identical = root.join("identical");
        let table = lay_legacy_copy(&identical, "identical");
        let table: Vec<(&str, &str)> = table.iter().map(|(n, h)| (*n, h.as_str())).collect();
        let older = root.join("older");
        lay_legacy_copy(&older, "older");
        let edited = root.join("edited");
        lay_legacy_copy(&edited, "edited");
        let foreign = root.join("foreign");
        lay_legacy_copy(&foreign, "foreign");
        let interrupted = root.join("interrupted");
        lay_legacy_copy(&interrupted, "interrupted");
        std::fs::remove_file(module_directory(&interrupted).join(ASSEMBLY)).unwrap();
        let empty = root.join("empty");
        let _disk = stamped(&[(&older, "2.4.6-bt.anchorfix"), (&edited, PATCHED_BUILD)]);
        let classify = |documents: &Path| installed_copy_against(documents, &table, PATCHED_BUILD);
        assert_eq!(classify(&identical), InstalledCopy::ThisBuild);
        assert_eq!(classify(&older), InstalledCopy::OlderBuild);
        assert_eq!(
            classify(&edited),
            InstalledCopy::None,
            "an edit is not Folio's"
        );
        assert_eq!(classify(&foreign), InstalledCopy::Foreign);
        assert_eq!(classify(&interrupted), InstalledCopy::None);
        assert_eq!(classify(&empty), InstalledCopy::None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **the removal row stands only where its remover would
    /// act** (the note's §5.3): a root holding an older Folio's copy is removable; a root the marks
    /// record merely names, a root holding a stock module or somebody's own, and a root holding
    /// nothing are not.
    ///
    /// MUTATION: decide the row on the directory's presence (`installed_disk::is_dir`), as the
    /// INJECT follow-up F8 decided on a substring — the stock module's root raises a row with
    /// nothing to remove.
    #[test]
    fn the_removal_row_stands_only_where_the_remover_would_act() {
        let root = temp_dir("legacy-row");
        let older = root.join("older");
        let stock = root.join("stock");
        lay_legacy_copy(&stock, "stock");
        let named = root.join("named-only");
        let _disk = an_older_folio_copy_in(&older, "2.4.6-bt.1");
        assert!(legacy_removable(std::slice::from_ref(&older)));
        assert!(legacy_removable(&[named.clone(), older.clone()]));
        assert!(
            !legacy_removable(std::slice::from_ref(&stock)),
            "a stock 2.4.6 is not Folio's"
        );
        assert!(
            !legacy_removable(std::slice::from_ref(&named)),
            "a name is not a copy"
        );
        assert!(!legacy_removable(&[]));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **the marks record's roots are where an earlier Folio
    /// wrote, and they are read, never written**: a recorded root of the fork's own shape adds its
    /// Documents to the candidates beside the one Windows names, a root of any other shape adds
    /// nothing, and the record is byte for byte what it was.
    ///
    /// MUTATION: drop the marks record from `legacy_roots` — the redirected copy is not found.
    #[test]
    fn the_marks_record_names_where_an_earlier_folio_wrote_and_is_left_as_it_is() {
        use crate::shell_integration::profile_marks::Marks;
        let root = temp_dir("legacy-roots");
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let redirected = root.join("redirected documents 文档");
        let marks = Marks {
            psreadline_module_roots: vec![
                module_directory(&redirected),
                root.join("not a module root"),
            ],
            ..Marks::default()
        };
        marks.write(&data).unwrap();
        let before =
            std::fs::read(data.join(crate::shell_integration::profile_marks::RECORD_FILE)).unwrap();
        let named = root.join("named");
        assert_eq!(
            legacy_roots(Some(named.clone()), &data),
            [named.clone(), redirected.clone()]
        );
        assert_eq!(
            legacy_roots(Some(redirected.clone()), &data),
            std::slice::from_ref(&redirected),
            "one root once"
        );
        assert_eq!(
            std::fs::read(data.join(crate::shell_integration::profile_marks::RECORD_FILE)).unwrap(),
            before
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (audit 3, E-1; kept by T-INTEGRATION-INJECT-2-BUILD) — **the removal takes the fork's
    /// nine files and leaves everything Folio did not write**, and a copy Folio did not write is
    /// left whole: PowerShellGet's record beside an older Folio's copy stays, and a foreign module
    /// answers `NotOurs` with every byte where it was.
    ///
    /// MUTATION: `remove_dir_all` the version leaf once the guard says yes — the sidecars go.
    #[test]
    fn a_removal_takes_folios_nine_files_and_leaves_what_folio_never_wrote() {
        let root = temp_dir("legacy-removal");
        let ours = root.join("ours");
        let foreign = root.join("foreign");
        lay_legacy_copy(&foreign, "foreign");
        let _disk = an_older_folio_copy_in(&ours, "2.4.6-bt.anchorfix");
        let sidecar = module_directory(&ours).join("PSGetModuleInfo.xml");
        std::fs::write(&sidecar, "<Objs/>").unwrap();

        let removed = remove_from(&ours).unwrap();
        assert_eq!(
            removed,
            Removed::Took {
                left: vec![sidecar.clone()]
            }
        );
        for (name, _) in LEGACY_FILES {
            assert!(!module_directory(&ours).join(name).exists(), "{name} went");
        }
        assert!(sidecar.is_file(), "somebody else's record stays");

        assert_eq!(remove_from(&foreign).unwrap(), Removed::NotOurs);
        for (name, _) in LEGACY_FILES {
            assert!(
                module_directory(&foreign).join(name).is_file(),
                "{name} stays"
            );
        }
        assert_eq!(
            remove_from(&root.join("nothing")).unwrap(),
            Removed::Nothing
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **a press removes every removable copy and says so; a
    /// copy gone since the row was drawn says nothing; a refusal names the directory** — the
    /// removal worker's run, over the test's own roots.
    ///
    /// MUTATION: answer `Removed` for an empty run — the vanished copy's press raises a toast about
    /// nothing.
    #[test]
    fn a_press_removes_every_copy_and_a_vanished_one_says_nothing() {
        let root = temp_dir("legacy-press");
        let first = root.join("first");
        let second = root.join("second");
        lay_legacy_copy(&first, "first");
        lay_legacy_copy(&second, "second");
        let _disk = stamped(&[(&first, "2.4.6-bt.1"), (&second, "2.4.6-bt.anchorfix")]);
        let roots = [first.clone(), second.clone()];
        match remove_legacy_copies(&roots) {
            LegacyRemoval::Removed { paths, left } => {
                assert_eq!(paths, [module_directory(&first), module_directory(&second)]);
                assert!(left.is_empty());
            }
            other => panic!("both copies are removed: {other:?}"),
        }
        assert!(!legacy_removable(&roots), "and nothing is left to offer");
        let again = remove_legacy_copies(&roots);
        assert_eq!(again, LegacyRemoval::Nothing);
        assert_eq!(again.window_text(), None, "nothing to say");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The queue of press answers is the process's; the two tests that read it take turns.
    static PRESS_ANSWERS: Mutex<()> = Mutex::new(());

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **a press whose worker the thread door refused is
    /// answered, to the window that pressed, once**: an unfinished removal in the sentence that
    /// says nothing was changed, raised as a refusal, and the refusal said once. The body never
    /// runs, so no Documents folder is looked at.
    ///
    /// MUTATIONS: the removal's `Answered` answering nothing — the queue stays empty; file the
    /// answer without its window — the window's filter finds nothing; its voice dropped — no line.
    #[test]
    fn a_refused_press_is_answered_to_its_window() {
        let _turn = PRESS_ANSWERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = take_legacy_removals();
        let gate = std::sync::Arc::new(crate::lane::Gate::default());
        gate.refuse_next_start();
        let said = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let lines = std::sync::Arc::clone(&said);
        let window = winit::window::WindowId::from(43_u64);
        let started = start_legacy_removal(
            &crate::lane::Executor::scripted(gate),
            move |line| lines.lock().unwrap().push(line.to_owned()),
            window,
            temp_dir("legacy-refused"),
        );
        assert!(!started, "the door refused the start");
        let answers = take_legacy_removals();
        assert_eq!(
            answers,
            vec![(window, LegacyRemoval::Unfinished(Text::ChangeNotStarted))],
            "one answer, addressed to the window that pressed"
        );
        assert_eq!(
            answers[0].1.window_text(),
            Some((true, Text::ChangeNotStarted.text().to_owned())),
            "and the window raises it as a refusal"
        );
        let said = said.lock().unwrap().clone();
        assert_eq!(said.len(), 1, "the refused start is said once: {said:?}");
        assert!(said[0].contains(crate::lane::REFUSED_START), "{said:?}");
        assert!(take_legacy_removals().is_empty(), "taken once");
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **a press is answered once**: by the worker when the
    /// removal is known, or by the worker's end when it did not get that far — never both, so a
    /// panic after the answer was filed leaves the true answer standing.
    ///
    /// MUTATION: `PressAnswer::file` filing every time (no `filed` swap) — the end's unfinished
    /// removal follows the true answer.
    #[test]
    fn a_press_answered_and_then_ended_keeps_its_one_answer() {
        let _turn = PRESS_ANSWERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = take_legacy_removals();
        let window = winit::window::WindowId::from(44_u64);
        let answer = PressAnswer {
            window,
            filed: std::sync::atomic::AtomicBool::new(false),
        };
        answer.file(LegacyRemoval::Nothing);
        answer.file(LegacyRemoval::Unfinished(Text::ChangeStopped));
        assert_eq!(
            take_legacy_removals(),
            vec![(window, LegacyRemoval::Nothing)],
            "one answer, the first"
        );
    }

    /// PIN — **the legacy table names the fork's nine files once each**, the assembly whose stamp
    /// decides among them included, each by a SHA-256 of lower-case hex.
    #[test]
    fn the_legacy_table_names_the_forks_nine_files_once_each() {
        let mut names: Vec<&str> = LEGACY_FILES.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 9);
        assert!(names.contains(&BUILD_STAMP_FILE));
        assert!(names.contains(&"License.txt"));
        for (name, hash) in LEGACY_FILES {
            assert!(
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{name}: {hash}"
            );
        }
        assert!(PATCHED_BUILD.starts_with(&family_prefix()));
    }
}
