//! **How this copy was installed** — one fact, derived once at start, owned here
//! (ticket U-1; `docs/plans/design/self-update-2026-09-16.md` §D, revision
//! 2026-09-25 C2, revision (b) F-2 and F-10, and the owner's ruling of
//! 2026-09-20: *the install writes a marker file, nothing is guessed from
//! paths; managed installs do not self-update*).
//!
//! # The evidence, and where each piece lives
//!
//! | evidence | Windows | macOS |
//! |---|---|---|
//! | **the install marker** | the file [`MARKER_FILE_NAME`] beside `folio.exe` | the extended attribute [`MARKER_ATTRIBUTE`] on the `.app` bundle directory — never a file inside the sealed bundle |
//! | **scoop's own receipt** | `install.json` and `manifest.json`, which scoop writes into every version folder it installs | — |
//! | **the folder's owner** | the owner SID of the install folder, against the process token's user and default owner | the bundle's owning uid, against the effective uid |
//! | **winget's own record** | a subkey of `HKCU\…\CurrentVersion\Uninstall` with `WinGetPackageIdentifier` [`WINGET_PACKAGE_ID`], `WinGetInstallerType` `portable`, and an `InstallLocation` that contains the running executable (U-4) | — |
//!
//! winget is the one manager with no hook (a zip with a portable installer), so
//! it cannot write a marker; its own uninstall record, written at install time
//! and removed by its uninstall, stands in for one. Its `InstallLocation` is the
//! package root and the executable sits one folder below it
//! (`<InstallLocation>\folio-<version>\folio.exe`, E2), so a record counts when
//! the location **contains** the executable, both canonicalised and compared
//! component by component without case — never when a string equals another.
//!
//! The marker is written by the package manager at install time (U-2), never
//! by Folio, and it leaves with the folder or bundle it sits in. Its format,
//! version 1, is one JSON object with exactly three keys, written compactly by
//! [`Marker::encode`]:
//!
//! ```text
//! {"v":1,"manager":"scoop","uninstall_hook":true}
//! ```
//!
//! `manager` is `scoop`, `homebrew` or `winget`; `uninstall_hook` says whether
//! the manager runs `folio --uninstall-cleanup` before it removes the copy. A
//! UTF-8 byte-order mark and surrounding whitespace (a PowerShell
//! `Set-Content`'s trailing CRLF) are accepted; a missing, extra or misspelt key
//! is not, and neither is a `v` other than 1.
//!
//! # The classification
//!
//! [`classify`] is pure. **Every read that fails is `Unknown`**, and so is
//! every piece of evidence that is present but not understood: a malformed or
//! future marker, a receipt with one of its two files, a marker naming another
//! manager beside scoop's receipt. An unreadable marker is not "no marker", an
//! unreadable owner is not "somebody else", and `Unknown` is never eligible for
//! anything the updater does. With every read answered:
//!
//! * a marker → [`Channel::Managed`] by the manager it names;
//! * winget's record naming this executable, with no marker or a winget one
//!   and no scoop receipt → managed by winget, with no uninstall hook (winget
//!   runs nothing of the package's when it uninstalls, E2); a record beside a
//!   marker or receipt of another manager, or two records that both contain the
//!   executable, is `Unknown`, and a record for another location changes
//!   nothing;
//! * no marker and scoop's receipt → managed by scoop, with no known uninstall
//!   hook (a scoop install whose `post_install` never ran);
//! * neither, and the folder is this account's → [`Channel::Ours`];
//! * neither, and it is another account's → [`Channel::NotOurs`].
//!
//! Two readers today: the one `diagnostics.log` line at start, and the
//! first-run card, whose Explorer row arrives on exactly where the fact is
//! `Managed { uninstall_hook: true, .. }` (U-3, ruling 2026-09-20). The card
//! reads it through [`channel`] and hears it land through [`install_wake`].
//! The updater's eligibility is a later ticket.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bt_platform::HostPlatform;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_evidence::{self, Account, RecordValue, UninstallRecord};

/// The Windows marker's name, beside `folio.exe`.
pub const MARKER_FILE_NAME: &str = "folio-install.json";
/// The macOS marker: an extended attribute on the bundle directory, whose value
/// is the same JSON as the Windows file.
pub const MARKER_ATTRIBUTE: &str = "io.github.lulu-loopp.folio.install";
/// The marker format this build reads, and the only one.
pub const MARKER_VERSION: u64 = 1;
/// A marker longer than this is not a marker.
const MARKER_MAX_BYTES: u64 = install_evidence::ATTRIBUTE_MAX_BYTES as u64;
/// scoop's record of an install: which bucket and architecture.
pub(crate) const SCOOP_INSTALL_RECEIPT: &str = "install.json";
/// scoop's copy of the bucket manifest it installed from.
pub(crate) const SCOOP_MANIFEST_RECEIPT: &str = "manifest.json";
/// A receipt longer than this is not scoop's.
const RECEIPT_MAX_BYTES: u64 = 1 << 20;
/// Folio's package identifier in winget's source, and in the record winget
/// writes when it installs it (`packaging/winget/…/WeiyiShi.Folio.installer.yaml`).
pub const WINGET_PACKAGE_ID: &str = "WeiyiShi.Folio";
/// The installer type winget records for a zip it unpacks.
pub const WINGET_PORTABLE: &str = "portable";
/// The values of an uninstall record this reader asks for, in the order
/// [`winget`] reads them: identifier, installer type, location, source.
pub const WINGET_RECORD_VALUES: [&str; 4] = [
    "WinGetPackageIdentifier",
    "WinGetInstallerType",
    "InstallLocation",
    "WinGetSourceIdentifier",
];

/// The package managers a marker can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Manager {
    Scoop,
    Homebrew,
    Winget,
}

impl Manager {
    /// The name the marker spells.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Scoop => "scoop",
            Self::Homebrew => "homebrew",
            Self::Winget => "winget",
        }
    }
}

/// **The install marker, version 1.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    pub manager: Manager,
    pub uninstall_hook: bool,
}

/// What is wrong with bytes that are not a marker this build reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerFault {
    /// Not one JSON object with exactly `v`, `manager` and `uninstall_hook`.
    Malformed,
    /// A well-formed object whose `v` is not [`MARKER_VERSION`].
    Version(u64),
}

impl Marker {
    /// The marker's one spelling: compact, keys in this order, no newline.
    #[must_use]
    pub fn encode(self) -> String {
        format!(
            r#"{{"v":{MARKER_VERSION},"manager":"{}","uninstall_hook":{}}}"#,
            self.manager.name(),
            self.uninstall_hook
        )
    }

    /// Read a marker's bytes.
    ///
    /// # Errors
    /// [`MarkerFault::Version`] for a well-formed object of another version
    /// (asked first, so that a future marker with keys this build has never
    /// heard of is told apart from a broken one), [`MarkerFault::Malformed`]
    /// for everything else.
    pub fn parse(bytes: &[u8]) -> Result<Self, MarkerFault> {
        #[derive(serde::Deserialize)]
        struct Versioned {
            v: u64,
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct V1 {
            #[serde(rename = "v")]
            _version: u64,
            manager: Manager,
            uninstall_hook: bool,
        }
        let bytes = without_bom(bytes);
        let Versioned { v } = serde_json::from_slice(bytes).map_err(|_| MarkerFault::Malformed)?;
        if v != MARKER_VERSION {
            return Err(MarkerFault::Version(v));
        }
        let V1 {
            manager,
            uninstall_hook,
            ..
        } = serde_json::from_slice(bytes).map_err(|_| MarkerFault::Malformed)?;
        Ok(Self {
            manager,
            uninstall_hook,
        })
    }
}

fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// What the marker read found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerEvidence {
    /// The file (or attribute) is not there.
    Absent,
    Present(Marker),
    /// It is there and is not a marker this build reads.
    Faulty(MarkerFault),
    /// It could not be read.
    Unreadable(io::ErrorKind),
    /// This platform has no marker location (neither Windows nor macOS).
    NoPlace,
}

/// What the scoop receipt read found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptEvidence {
    /// Neither file is there.
    Absent,
    /// Both are there and read as scoop's.
    Present,
    /// One of the two files is there without the other.
    Partial,
    /// A file is there and does not read as scoop's.
    Malformed,
    Unreadable(io::ErrorKind),
    /// scoop installs only on Windows; nothing was read.
    NotApplicable,
}

/// **winget's record of this copy**: the subkey's name, the install location
/// it names, and the source it came from when the record says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WingetRecord {
    pub key: String,
    pub location: String,
    pub source: Option<String>,
}

/// What the winget read found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WingetEvidence {
    /// No record of Folio's names this executable (there may be records for
    /// other locations: other copies).
    None,
    /// One record names it.
    Record(WingetRecord),
    /// This many records of Folio's each contain the executable.
    Several(usize),
    /// The records could not be read, or one of Folio's is not a record this
    /// build reads (`InvalidData`).
    Unreadable(io::ErrorKind),
    /// winget installs only on Windows; nothing was read.
    NotApplicable,
}

/// What the owner read found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerEvidence {
    ThisAccount,
    AnotherAccount,
    /// The owner, or this process's own account, could not be read.
    Unreadable(io::ErrorKind),
}

/// **Everything that was read about one install folder, and what it said.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub marker: MarkerEvidence,
    pub receipt: ReceiptEvidence,
    pub owner: OwnerEvidence,
    pub winget: WingetEvidence,
}

/// **How this copy was installed.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// Unpacked by hand, by this account: the copy is its own.
    Ours,
    /// A package manager installed it and owns its updates.
    Managed {
        manager: Manager,
        /// The manager runs `--uninstall-cleanup` before removing the copy.
        uninstall_hook: bool,
    },
    /// Unpacked by hand, by another account.
    NotOurs,
    /// Some read failed, or the evidence was not understood or disagreed.
    Unknown,
}

/// **The classification** — pure, and the only place the evidence is judged.
#[must_use]
pub fn classify(evidence: &Evidence) -> Channel {
    let marker = match evidence.marker {
        MarkerEvidence::Absent => None,
        MarkerEvidence::Present(marker) => Some(marker),
        MarkerEvidence::Faulty(_) | MarkerEvidence::Unreadable(_) | MarkerEvidence::NoPlace => {
            return Channel::Unknown;
        }
    };
    let receipt = match evidence.receipt {
        ReceiptEvidence::Absent | ReceiptEvidence::NotApplicable => false,
        ReceiptEvidence::Present => true,
        ReceiptEvidence::Partial | ReceiptEvidence::Malformed | ReceiptEvidence::Unreadable(_) => {
            return Channel::Unknown;
        }
    };
    let ours = match evidence.owner {
        OwnerEvidence::ThisAccount => true,
        OwnerEvidence::AnotherAccount => false,
        OwnerEvidence::Unreadable(_) => return Channel::Unknown,
    };
    let winget = match evidence.winget {
        WingetEvidence::None | WingetEvidence::NotApplicable => false,
        WingetEvidence::Record(_) => true,
        WingetEvidence::Several(_) | WingetEvidence::Unreadable(_) => return Channel::Unknown,
    };
    if winget {
        return match (marker, receipt) {
            (None, false) => Channel::Managed {
                manager: Manager::Winget,
                uninstall_hook: false,
            },
            (Some(marker), false) if marker.manager == Manager::Winget => Channel::Managed {
                manager: Manager::Winget,
                uninstall_hook: marker.uninstall_hook,
            },
            _ => Channel::Unknown,
        };
    }
    match (marker, receipt) {
        (Some(marker), true) if marker.manager != Manager::Scoop => Channel::Unknown,
        (Some(marker), _) => Channel::Managed {
            manager: marker.manager,
            uninstall_hook: marker.uninstall_hook,
        },
        (None, true) => Channel::Managed {
            manager: Manager::Scoop,
            uninstall_hook: false,
        },
        (None, false) if ours => Channel::Ours,
        (None, false) => Channel::NotOurs,
    }
}

/// **The folder an executable is installed as**: the `.app` bundle on macOS
/// when the executable sits at `<Name>.app/Contents/MacOS/`, the executable's
/// own folder otherwise.
#[must_use]
pub fn install_root(exe: &Path, platform: HostPlatform) -> Option<PathBuf> {
    let folder = exe.parent()?;
    if platform == HostPlatform::MacOs
        && folder.file_name().is_some_and(|name| name == "MacOS")
        && let Some(contents) = folder.parent()
        && contents.file_name().is_some_and(|name| name == "Contents")
        && let Some(bundle) = contents.parent()
        && bundle
            .extension()
            .is_some_and(|extension| extension == "app")
    {
        return Some(bundle.to_path_buf());
    }
    Some(folder.to_path_buf())
}

/// **The reads**, over one install folder: the marker where `platform` keeps
/// it, scoop's receipt on Windows, and the folder's owner against `me`;
/// `winget` is what [`winget`] found, which asks of the executable rather than
/// of the folder.
///
/// Takes the folder rather than finding it, so that a test reads a temporary
/// folder through exactly the calls the product makes and never the real
/// install.
#[must_use]
pub fn read(
    root: &Path,
    platform: HostPlatform,
    me: Result<&Account, io::ErrorKind>,
    winget: WingetEvidence,
) -> Evidence {
    let marker = match platform {
        HostPlatform::Windows => match capped(&root.join(MARKER_FILE_NAME), MARKER_MAX_BYTES) {
            Ok(Some(bytes)) => marker_evidence(&bytes),
            Ok(None) => MarkerEvidence::Absent,
            Err(kind) => MarkerEvidence::Unreadable(kind),
        },
        HostPlatform::MacOs => match install_evidence::attribute(root, MARKER_ATTRIBUTE) {
            Ok(Some(bytes)) => marker_evidence(&bytes),
            Ok(None) => MarkerEvidence::Absent,
            Err(error) => MarkerEvidence::Unreadable(error.kind()),
        },
        HostPlatform::OtherUnix => MarkerEvidence::NoPlace,
    };
    let receipt = if platform == HostPlatform::Windows {
        scoop_receipt(root)
    } else {
        ReceiptEvidence::NotApplicable
    };
    let owner = match (me, install_evidence::owner_of(root)) {
        (Ok(me), Ok(owner)) if me.owns(&owner) => OwnerEvidence::ThisAccount,
        (Ok(_), Ok(_)) => OwnerEvidence::AnotherAccount,
        (Err(kind), _) => OwnerEvidence::Unreadable(kind),
        (_, Err(error)) => OwnerEvidence::Unreadable(error.kind()),
    };
    Evidence {
        marker,
        receipt,
        owner,
        winget,
    }
}

/// **winget's record of the executable at `exe`**, on Windows, from the
/// records `records` reads — [`install_evidence::uninstall_records`] in the
/// product, a list the test spells otherwise.
///
/// Only records whose `WinGetPackageIdentifier` is [`WINGET_PACKAGE_ID`] are
/// judged; every other program's record is passed over whatever it holds. Of
/// Folio's, one whose installer type is not [`WINGET_PORTABLE`] is not one this
/// install could have made and is passed over too; one whose values are not
/// strings, or whose location is missing or not absolute, is `Unreadable`,
/// since nobody can say it is not this copy's. A location counts when it
/// contains `exe`, both canonicalised through
/// [`bt_platform::instance::canonical_path`] (a link to the executable —
/// winget's own `Links\folio.exe` — resolves to the file inside the package)
/// and compared component by component without case.
#[must_use]
pub fn winget(
    exe: &Path,
    platform: HostPlatform,
    records: impl FnOnce() -> io::Result<Vec<UninstallRecord>>,
) -> WingetEvidence {
    if platform != HostPlatform::Windows {
        return WingetEvidence::NotApplicable;
    }
    let records = match records() {
        Ok(records) => records,
        Err(error) => return WingetEvidence::Unreadable(error.kind()),
    };
    let malformed = WingetEvidence::Unreadable(io::ErrorKind::InvalidData);
    let exe = bt_platform::instance::canonical_path(exe);
    let mut found = Vec::new();
    for record in records {
        let [id, kind, location, source] = record.values.as_slice() else {
            return malformed;
        };
        match id {
            RecordValue::Text(id) if id == WINGET_PACKAGE_ID => {}
            RecordValue::Absent | RecordValue::Text(_) => continue,
            RecordValue::Malformed => return malformed,
        }
        match kind {
            RecordValue::Text(kind) if kind == WINGET_PORTABLE => {}
            RecordValue::Absent | RecordValue::Text(_) => continue,
            RecordValue::Malformed => return malformed,
        }
        let RecordValue::Text(location) = location else {
            return malformed;
        };
        if !Path::new(location).is_absolute() {
            return malformed;
        }
        if contains(
            &bt_platform::instance::canonical_path(Path::new(location)),
            &exe,
        ) {
            found.push(WingetRecord {
                key: record.key,
                location: location.clone(),
                source: match source {
                    RecordValue::Text(source) => Some(source.clone()),
                    RecordValue::Absent | RecordValue::Malformed => None,
                },
            });
        }
    }
    match found.len() {
        0 => WingetEvidence::None,
        1 => WingetEvidence::Record(found.remove(0)),
        several => WingetEvidence::Several(several),
    }
}

/// Whether `inner` lies strictly inside `outer`, component by component, with
/// case folded as the Windows file system folds it.
fn contains(outer: &Path, inner: &Path) -> bool {
    let fold = |component: std::path::Component<'_>| {
        component.as_os_str().to_string_lossy().to_lowercase()
    };
    let outer: Vec<_> = outer.components().map(fold).collect();
    let inner: Vec<_> = inner.components().map(fold).collect();
    !outer.is_empty() && inner.len() > outer.len() && inner.starts_with(&outer)
}

fn marker_evidence(bytes: &[u8]) -> MarkerEvidence {
    match Marker::parse(bytes) {
        Ok(marker) => MarkerEvidence::Present(marker),
        Err(fault) => MarkerEvidence::Faulty(fault),
    }
}

/// scoop's two files, each a JSON object; the manifest names a version.
fn scoop_receipt(root: &Path) -> ReceiptEvidence {
    let install = capped(&root.join(SCOOP_INSTALL_RECEIPT), RECEIPT_MAX_BYTES);
    let manifest = capped(&root.join(SCOOP_MANIFEST_RECEIPT), RECEIPT_MAX_BYTES);
    let object = |bytes: &[u8]| {
        serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(without_bom(bytes))
            .ok()
    };
    match (install, manifest) {
        (Err(kind), _) | (_, Err(kind)) => ReceiptEvidence::Unreadable(kind),
        (Ok(None), Ok(None)) => ReceiptEvidence::Absent,
        (Ok(Some(install)), Ok(Some(manifest))) => {
            let manifest_names_a_version = object(&manifest)
                .is_some_and(|manifest| manifest.get("version").is_some_and(|v| v.is_string()));
            if object(&install).is_some() && manifest_names_a_version {
                ReceiptEvidence::Present
            } else {
                ReceiptEvidence::Malformed
            }
        }
        _ => ReceiptEvidence::Partial,
    }
}

/// One file's bytes on the install lane: `None` when it is not there, an error
/// when it cannot be read or is longer than `cap`.
fn capped(path: &Path, cap: u64) -> Result<Option<Vec<u8>>, io::ErrorKind> {
    let file = match file_reads::open(Lane::Install, path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.kind()),
    };
    let mut bytes = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.kind())?;
    if bytes.len() as u64 > cap {
        return Err(io::ErrorKind::FileTooLarge);
    }
    Ok(Some(bytes))
}

/// **The fact, and what it was derived from.** `evidence` is the executable's
/// own path's error when there was no folder to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub evidence: Result<Evidence, io::ErrorKind>,
    pub channel: Channel,
}

/// The derivation the product runs: the executable's install folder, and
/// winget's records of the executable, read and classified.
fn derive_fact(
    exe: io::Result<PathBuf>,
    platform: HostPlatform,
    me: io::Result<Account>,
    records: impl FnOnce() -> io::Result<Vec<UninstallRecord>>,
) -> Fact {
    let evidence = exe.map_err(|error| error.kind()).and_then(|exe| {
        let root = install_root(&exe, platform).ok_or(io::ErrorKind::NotFound)?;
        let winget = winget(&exe, platform, records);
        Ok(read(
            &root,
            platform,
            me.as_ref().map_err(io::Error::kind),
            winget,
        ))
    });
    let channel = evidence.as_ref().map_or(Channel::Unknown, classify);
    Fact { evidence, channel }
}

impl Fact {
    /// The one `diagnostics.log` line: the channel, then each piece of
    /// evidence. No path and no account name.
    #[must_use]
    pub fn line(&self) -> String {
        let channel = match self.channel {
            Channel::Ours => "ours".to_owned(),
            Channel::Managed {
                manager,
                uninstall_hook,
            } => format!(
                "managed by {}{}",
                manager.name(),
                if uninstall_hook {
                    " with an uninstall hook"
                } else {
                    ""
                }
            ),
            Channel::NotOurs => "another account's".to_owned(),
            Channel::Unknown => "unknown".to_owned(),
        };
        let evidence = match &self.evidence {
            Err(kind) => format!("the executable's path could not be read ({kind:?})"),
            Ok(evidence) => {
                let marker = match evidence.marker {
                    MarkerEvidence::Absent => "absent".to_owned(),
                    MarkerEvidence::Present(marker) => marker.encode(),
                    MarkerEvidence::Faulty(MarkerFault::Malformed) => "malformed".to_owned(),
                    MarkerEvidence::Faulty(MarkerFault::Version(v)) => format!("version {v}"),
                    MarkerEvidence::Unreadable(kind) => format!("unreadable ({kind:?})"),
                    MarkerEvidence::NoPlace => "not defined here".to_owned(),
                };
                let receipt = match evidence.receipt {
                    ReceiptEvidence::Absent => "absent".to_owned(),
                    ReceiptEvidence::Present => "present".to_owned(),
                    ReceiptEvidence::Partial => "partial".to_owned(),
                    ReceiptEvidence::Malformed => "malformed".to_owned(),
                    ReceiptEvidence::Unreadable(kind) => format!("unreadable ({kind:?})"),
                    ReceiptEvidence::NotApplicable => "not applicable".to_owned(),
                };
                let owner = match evidence.owner {
                    OwnerEvidence::ThisAccount => "this account".to_owned(),
                    OwnerEvidence::AnotherAccount => "another account".to_owned(),
                    OwnerEvidence::Unreadable(kind) => format!("unreadable ({kind:?})"),
                };
                let winget = match &evidence.winget {
                    WingetEvidence::None => "none".to_owned(),
                    WingetEvidence::Record(record) => format!("record {}", record.key),
                    WingetEvidence::Several(count) => format!("{count} records"),
                    WingetEvidence::Unreadable(kind) => format!("unreadable ({kind:?})"),
                    WingetEvidence::NotApplicable => "not applicable".to_owned(),
                };
                format!(
                    "marker {marker} · scoop receipt {receipt} · folder owner {owner} · winget {winget}"
                )
            }
        };
        format!("Folio: install channel {channel} — {evidence}")
    }
}

/// The fact, once derived. Written by [`begin`]'s worker and nobody else.
static FACT: OnceLock<Fact> = OnceLock::new();

/// How the worker asks the event loop for a turn once the fact has landed.
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// **How this copy was installed, or `None` while the worker is still out.**
///
/// The one accessor. A reader that cannot wait reads `None` as
/// [`Channel::Unknown`], which is never eligible for anything.
#[must_use]
pub fn channel() -> Option<Channel> {
    FACT.get().map(|fact| fact.channel)
}

/// **How the copy whose executable is `exe` was installed**, derived now on
/// the calling thread: the Windows applier's, which runs from the rescue copy
/// and asks about the installed `folio.exe` it is about to replace (U-23,
/// before the moves: `update_prepare_windows::staged_as_verified`). The start's
/// own fact is [`channel`]'s.
#[must_use]
pub(crate) fn channel_of(exe: &Path) -> Channel {
    derive_fact(
        Ok(exe.to_path_buf()),
        bt_platform::host_platform(),
        install_evidence::current_account(),
        || install_evidence::uninstall_records(&WINGET_RECORD_VALUES),
    )
    .channel
}

/// Install the event loop's wake, before [`begin`]: the first-run card waits a
/// turn for the fact, and a window with a modal due and nothing else happening
/// gets no turn unless the worker asks for one.
pub fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(wake));
}

fn wake() {
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

/// **Derive the fact once, off the window thread**, write its line, and wake
/// the loop.
///
/// The wake comes after the fact is published, never before: a turn that raced
/// the `set` would read a fact that is still missing. A kernel that will not
/// give out a thread is a launch with no channel line and no fact; the loop is
/// woken all the same, so a card waiting for the fact reads it as unknown on
/// the next turn instead of waiting for an unrelated event.
pub fn begin() {
    let spawned = bt_platform::spawn_at_priority(
        "bt-install-channel",
        bt_platform::ThreadPriority::BelowNormal,
        |_ctx| {
            let fact = FACT.get_or_init(|| {
                derive_fact(
                    std::env::current_exe(),
                    bt_platform::host_platform(),
                    install_evidence::current_account(),
                    || install_evidence::uninstall_records(&WINGET_RECORD_VALUES),
                )
            });
            crate::diagnostics::note(&fact.line());
            wake();
        },
    );
    if spawned.is_err() {
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A temporary install folder, never the real one.
    fn install_folder(tag: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "folio-install-channel-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn me() -> Account {
        install_evidence::current_account().unwrap()
    }

    /// Somebody who is not the account running the test. The owner read stays
    /// real; only the question of whose it is changes.
    fn stranger() -> Account {
        Account::named(["an account that is not this one".to_owned()])
    }

    /// The Windows reads, on any host: the marker is a file there, and a file
    /// reads the same everywhere.
    fn derived(root: &Path, me: &Account) -> (Evidence, Channel) {
        let evidence = read(root, HostPlatform::Windows, Ok(me), WingetEvidence::None);
        let channel = classify(&evidence);
        (evidence, channel)
    }

    /// The bytes scoop's `post_install` writes (U-2).
    const SCOOP_MARKER: &[u8] = br#"{"v":1,"manager":"scoop","uninstall_hook":true}"#;

    /// A machine with no uninstall records at all.
    #[allow(clippy::unnecessary_wraps)]
    fn no_records() -> io::Result<Vec<UninstallRecord>> {
        Ok(Vec::new())
    }

    /// **A winget package folder as E2 found one**: `<location>` holding the
    /// zip's own top folder, `folio-0.4.6\folio.exe` — a temporary folder,
    /// never winget's. Returns the location and the executable.
    fn winget_package(tag: &str) -> (PathBuf, PathBuf) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let location = std::env::temp_dir().join(format!(
            "bt-install-channel-winget-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&location);
        let version = location.join("folio-0.4.6");
        std::fs::create_dir_all(&version).unwrap();
        let exe = version.join("folio.exe");
        std::fs::write(&exe, b"").unwrap();
        (location, exe)
    }

    /// One uninstall record as the reader hands it over, every value a string.
    fn record(key: &str, id: &str, kind: &str, location: &str) -> UninstallRecord {
        UninstallRecord {
            key: key.to_owned(),
            values: vec![
                RecordValue::Text(id.to_owned()),
                RecordValue::Text(kind.to_owned()),
                RecordValue::Text(location.to_owned()),
                RecordValue::Text("*DefaultSource".to_owned()),
            ],
        }
    }

    /// E2's record (2026-09-27) for a package at `location`.
    fn folio_record(location: &Path) -> UninstallRecord {
        record(
            "WeiyiShi.Folio__DefaultSource",
            WINGET_PACKAGE_ID,
            WINGET_PORTABLE,
            &location.to_string_lossy(),
        )
    }

    /// The fact the product derives for `exe` on Windows, with `records` as
    /// what the registry holds.
    fn derived_with(exe: &Path, records: Vec<UninstallRecord>) -> Fact {
        derive_fact(
            Ok(exe.to_path_buf()),
            HostPlatform::Windows,
            Ok(me()),
            || Ok(records),
        )
    }

    /// RED (U-4) — **a winget record whose install location contains this
    /// executable makes the copy managed by winget, with no uninstall hook.**
    ///
    /// winget has no hook to write a marker, so its own record — written when
    /// it installs, removed when it uninstalls — is the evidence (the design
    /// note's (b).1, Q1 settled by Codex). E2: the executable sits one folder
    /// below `InstallLocation`, so the test is containment, not equality, and
    /// the record's spelling of the location differs in case from the one the
    /// file system answers. The folder is this account's own, so without the
    /// record the copy is `Ours` — the record is what decides. Another
    /// program's record, with values of any type, is passed over.
    ///
    /// MUTATION: in `classify`, `WingetEvidence::Record(_) => false`.
    #[test]
    fn a_winget_record_naming_this_target_is_managed() {
        let (location, exe) = winget_package("managed");
        let spelled = location.to_string_lossy().to_uppercase();
        let another = UninstallRecord {
            key: "Another program".to_owned(),
            values: vec![
                RecordValue::Absent,
                RecordValue::Malformed,
                RecordValue::Malformed,
                RecordValue::Absent,
            ],
        };
        let fact = derived_with(&exe, vec![another, folio_record(Path::new(&spelled))]);
        assert_eq!(
            fact.channel,
            Channel::Managed {
                manager: Manager::Winget,
                uninstall_hook: false,
            }
        );
        assert_eq!(
            fact.evidence
                .as_ref()
                .map(|evidence| evidence.winget.clone()),
            Ok(WingetEvidence::Record(WingetRecord {
                key: "WeiyiShi.Folio__DefaultSource".to_owned(),
                location: spelled.clone(),
                source: Some("*DefaultSource".to_owned()),
            }))
        );
        let line = fact.line();
        assert!(
            line.starts_with("Folio: install channel managed by winget — marker absent")
                && line.ends_with(" · winget record WeiyiShi.Folio__DefaultSource"),
            "{line}"
        );
        assert!(!line.contains(&*location.to_string_lossy()), "{line}");
        assert_eq!(derived_with(&exe, Vec::new()).channel, Channel::Ours);
        std::fs::remove_dir_all(&location).unwrap();
    }

    /// RED (U-4) — **a record of Folio's for another location changes
    /// nothing**: it is another copy, and this one is what it was without it.
    ///
    /// E2: "a record whose `InstallLocation` does not contain this executable
    /// changes nothing". The locations are a sibling package, a folder whose
    /// name merely begins with this package's name (a string prefix, not a
    /// parent), and this executable's own path, which contains nothing; beside
    /// them a record of this location from an installer that is not winget's
    /// portable one.
    ///
    /// MUTATION: in `winget`, push every portable record of Folio's whatever
    /// `contains` answers.
    #[test]
    fn a_record_for_another_target_changes_nothing() {
        let (location, exe) = winget_package("this");
        let (other, _) = winget_package("other");
        let prefixed = PathBuf::from(format!("{}-2", location.to_string_lossy()));
        let records = vec![
            folio_record(&other),
            folio_record(&prefixed),
            folio_record(&exe),
            record(
                "WeiyiShi.Folio_msix",
                WINGET_PACKAGE_ID,
                "msix",
                &location.to_string_lossy(),
            ),
        ];
        let fact = derived_with(&exe, records);
        assert_eq!(fact, derived_with(&exe, Vec::new()));
        assert_eq!(
            fact.evidence.map(|evidence| evidence.winget),
            Ok(WingetEvidence::None)
        );
        assert_eq!(fact.channel, Channel::Ours);
        std::fs::remove_dir_all(&location).unwrap();
        std::fs::remove_dir_all(&other).unwrap();
    }

    /// RED (U-4) — **winget's record beside a marker or receipt of another
    /// manager, or two records that both contain the executable, is
    /// `Unknown`**; a winget marker agreeing with the record is winget.
    ///
    /// The (b).5 row names this test. Two records for two locations are two
    /// copies and only one contains the executable; two that both contain it
    /// (a package nested in another's folder) say nothing about which manager
    /// owns this copy, and neither does a scoop marker or scoop's receipt in
    /// the folder winget's record claims.
    ///
    /// MUTATION: in `classify`, answer `Managed { Winget, false }` for a
    /// record whatever the marker and the receipt say.
    #[test]
    fn conflicting_evidence_is_unknown() {
        let (location, exe) = winget_package("two");
        let folder = exe.parent().unwrap();
        let fact = derived_with(&exe, vec![folio_record(&location), folio_record(folder)]);
        assert_eq!(
            fact.evidence
                .as_ref()
                .map(|evidence| evidence.winget.clone()),
            Ok(WingetEvidence::Several(2))
        );
        assert_eq!(fact.channel, Channel::Unknown);
        assert!(
            fact.line().ends_with(" · winget 2 records"),
            "{}",
            fact.line()
        );

        std::fs::write(folder.join(MARKER_FILE_NAME), SCOOP_MARKER).unwrap();
        assert_eq!(
            derived_with(&exe, vec![folio_record(&location)]).channel,
            Channel::Unknown
        );
        std::fs::remove_file(folder.join(MARKER_FILE_NAME)).unwrap();

        std::fs::write(folder.join(SCOOP_INSTALL_RECEIPT), br#"{"bucket":"folio"}"#).unwrap();
        std::fs::write(
            folder.join(SCOOP_MANIFEST_RECEIPT),
            br#"{"version":"0.4.6"}"#,
        )
        .unwrap();
        assert_eq!(
            derived_with(&exe, vec![folio_record(&location)]).channel,
            Channel::Unknown
        );
        std::fs::remove_file(folder.join(SCOOP_INSTALL_RECEIPT)).unwrap();
        std::fs::remove_file(folder.join(SCOOP_MANIFEST_RECEIPT)).unwrap();

        std::fs::write(
            folder.join(MARKER_FILE_NAME),
            br#"{"v":1,"manager":"winget","uninstall_hook":false}"#,
        )
        .unwrap();
        assert_eq!(
            derived_with(&exe, vec![folio_record(&location)]).channel,
            Channel::Managed {
                manager: Manager::Winget,
                uninstall_hook: false,
            }
        );
        std::fs::remove_dir_all(&location).unwrap();
    }

    /// RED (U-4) — **a registry read that fails, or a record of Folio's that
    /// is not one this build reads, is `Unknown` — kept apart from "no
    /// record", which would be `Ours`.**
    ///
    /// The coordinator's ruling 2 on E2: "A registry read that fails (access
    /// denied, malformed value types) → `Unknown`, kept apart from 'no record'
    /// in the evidence." Each case sits in this account's own folder. Off
    /// Windows the reader is never asked.
    ///
    /// MUTATION: in `winget`, map an `Err` from `records()` to
    /// `WingetEvidence::None`.
    #[test]
    fn an_unreadable_registry_is_unknown_not_none() {
        let (location, exe) = winget_package("unreadable");
        let fact = derive_fact(Ok(exe.clone()), HostPlatform::Windows, Ok(me()), || {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });
        assert_eq!(
            fact.evidence
                .as_ref()
                .map(|evidence| evidence.winget.clone()),
            Ok(WingetEvidence::Unreadable(io::ErrorKind::PermissionDenied))
        );
        assert_eq!(fact.channel, Channel::Unknown);
        assert!(
            fact.line()
                .ends_with(" · winget unreadable (PermissionDenied)"),
            "{}",
            fact.line()
        );

        let with = |location: RecordValue| UninstallRecord {
            key: "WeiyiShi.Folio__DefaultSource".to_owned(),
            values: vec![
                RecordValue::Text(WINGET_PACKAGE_ID.to_owned()),
                RecordValue::Text(WINGET_PORTABLE.to_owned()),
                location,
                RecordValue::Absent,
            ],
        };
        for malformed in [
            with(RecordValue::Malformed),
            with(RecordValue::Absent),
            with(RecordValue::Text(String::new())),
            with(RecordValue::Text("folio-0.4.6".to_owned())),
            UninstallRecord {
                key: "WeiyiShi.Folio__DefaultSource".to_owned(),
                values: vec![RecordValue::Malformed; 4],
            },
        ] {
            let fact = derived_with(&exe, vec![malformed.clone()]);
            assert_eq!(
                fact.evidence.map(|evidence| evidence.winget),
                Ok(WingetEvidence::Unreadable(io::ErrorKind::InvalidData)),
                "{malformed:?}"
            );
            assert_eq!(fact.channel, Channel::Unknown, "{malformed:?}");
        }

        let fact = derive_fact(Ok(exe), HostPlatform::MacOs, Ok(me()), || {
            panic!("the registry is asked only on Windows")
        });
        assert_eq!(
            fact.evidence.map(|evidence| evidence.winget),
            Ok(WingetEvidence::NotApplicable)
        );
        std::fs::remove_dir_all(&location).unwrap();
    }

    /// RED (U-1) — **a well-formed marker makes this copy managed, by the
    /// manager it names and with the hook it states.**
    ///
    /// The ruling of 2026-09-20: how Folio was installed is read from a marker
    /// the install wrote. The folder is this account's own, so without the
    /// marker it would be `Ours` — the marker is what decides.
    ///
    /// MUTATION: in `classify`, `(Some(marker), _) => Channel::Ours`.
    #[test]
    fn a_well_formed_marker_makes_this_copy_managed() {
        let root = install_folder("marker");
        std::fs::write(root.join(MARKER_FILE_NAME), SCOOP_MARKER).unwrap();
        let (evidence, channel) = derived(&root, &me());
        assert_eq!(
            evidence.marker,
            MarkerEvidence::Present(Marker {
                manager: Manager::Scoop,
                uninstall_hook: true,
            })
        );
        assert_eq!(
            channel,
            Channel::Managed {
                manager: Manager::Scoop,
                uninstall_hook: true,
            }
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// RED (U-1) — **scoop's own receipt, with no marker, makes the copy
    /// managed by scoop, with no uninstall hook known.**
    ///
    /// Revision (b) F-10: scoop writes `install.json` and `manifest.json` into
    /// every version folder, so a scoop install whose `post_install` never ran
    /// (a bucket manifest from before U-2) is still told apart from a hand
    /// unpack — by a record the manager wrote, not by a path.
    ///
    /// MUTATION: in `classify`, `(None, true) => Channel::Ours`.
    #[test]
    fn a_scoop_receipt_alone_makes_it_managed() {
        let root = install_folder("receipt");
        std::fs::write(
            root.join(SCOOP_INSTALL_RECEIPT),
            b"{\r\n    \"architecture\": \"64bit\",\r\n    \"bucket\": \"folio\"\r\n}",
        )
        .unwrap();
        std::fs::write(
            root.join(SCOOP_MANIFEST_RECEIPT),
            br#"{"version":"0.4.6","bin":"folio.exe"}"#,
        )
        .unwrap();
        let (evidence, channel) = derived(&root, &me());
        assert_eq!(evidence.marker, MarkerEvidence::Absent);
        assert_eq!(evidence.receipt, ReceiptEvidence::Present);
        assert_eq!(
            channel,
            Channel::Managed {
                manager: Manager::Scoop,
                uninstall_hook: false,
            }
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// RED (U-1) — **a marker that cannot be read is `Unknown`, never "no
    /// marker" and so never `Ours`.**
    ///
    /// The marker's name is taken by a folder, so opening or reading it fails
    /// on every platform. Were the failure read as absence, this copy — this
    /// account's own folder — would be `Ours`, and the updater would replace a
    /// copy whose manager may own it.
    ///
    /// MUTATION: in `read`, map the Windows arm's `Err(kind)` to
    /// `MarkerEvidence::Absent`.
    #[test]
    fn an_unreadable_marker_is_unknown() {
        let root = install_folder("unreadable");
        std::fs::create_dir(root.join(MARKER_FILE_NAME)).unwrap();
        let (evidence, channel) = derived(&root, &me());
        assert!(
            matches!(evidence.marker, MarkerEvidence::Unreadable(_)),
            "{evidence:?}"
        );
        assert_eq!(channel, Channel::Unknown);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// RED (U-1) — **with no marker and no receipt, a folder owned by another
    /// account is not ours.**
    ///
    /// Revision (b) F-2: an install is `Ours` only if its folder's owner is the
    /// account running Folio, because the recovery entrance lives in that
    /// account's own registry hive. The owner is read for real from a real
    /// folder; the account asking is one it cannot belong to, since no
    /// unprivileged test can give a folder away.
    ///
    /// MUTATION: in `read`, `(Ok(_), Ok(_)) => OwnerEvidence::ThisAccount`.
    #[test]
    fn a_folder_owned_by_another_account_is_not_ours() {
        let root = install_folder("stranger");
        let (evidence, channel) = derived(&root, &stranger());
        assert_eq!(evidence.owner, OwnerEvidence::AnotherAccount);
        assert_eq!(channel, Channel::NotOurs);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// **And the same bare folder, asked by the account that made it, is
    /// ours** — the other half of the test above, so that `NotOurs` there is
    /// the owner's doing and not a folder that could never be anybody's.
    #[test]
    fn install_channel_no_marker_and_a_folder_this_account_owns_is_ours() {
        let root = install_folder("mine");
        let (evidence, channel) = derived(&root, &me());
        assert_eq!(
            evidence,
            Evidence {
                marker: MarkerEvidence::Absent,
                receipt: ReceiptEvidence::Absent,
                owner: OwnerEvidence::ThisAccount,
                winget: WingetEvidence::None,
            }
        );
        assert_eq!(channel, Channel::Ours);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// RED (U-1) — **a malformed or future marker is `Unknown`, and never
    /// `Ours`.**
    ///
    /// Each of these sits in this account's own folder, which alone would be
    /// `Ours`. A marker this build does not understand still says a manager
    /// may own the copy.
    ///
    /// MUTATION: in `classify`, `MarkerEvidence::Faulty(_) => None` (read as
    /// absent).
    #[test]
    fn a_malformed_or_future_marker_is_unknown_and_never_ours() {
        for (bytes, fault) in [
            (
                &br#"{"v":2,"manager":"scoop","uninstall_hook":true,"channel":"beta"}"#[..],
                MarkerFault::Version(2),
            ),
            (
                br#"{"v":0,"manager":"scoop","uninstall_hook":true}"#,
                MarkerFault::Version(0),
            ),
            (
                br#"{"v":1,"manager":"apt","uninstall_hook":true}"#,
                MarkerFault::Malformed,
            ),
            (br#"{"v":1,"manager":"scoop"}"#, MarkerFault::Malformed),
            (
                br#"{"v":1,"manager":"scoop","uninstall_hook":true,"extra":1}"#,
                MarkerFault::Malformed,
            ),
            (
                br#"{"v":1,"manager":"scoop","uninstallhook":true}"#,
                MarkerFault::Malformed,
            ),
            (
                br#"{"v":1,"manager":"scoop","uninstall_hook":"yes"}"#,
                MarkerFault::Malformed,
            ),
            (br#"{"v":"1"}"#, MarkerFault::Malformed),
            (b"scoop", MarkerFault::Malformed),
            (b"", MarkerFault::Malformed),
        ] {
            let root = install_folder("faulty");
            std::fs::write(root.join(MARKER_FILE_NAME), bytes).unwrap();
            let (evidence, channel) = derived(&root, &me());
            assert_eq!(
                evidence.marker,
                MarkerEvidence::Faulty(fault),
                "{}",
                String::from_utf8_lossy(bytes)
            );
            assert_eq!(
                channel,
                Channel::Unknown,
                "{}",
                String::from_utf8_lossy(bytes)
            );
            std::fs::remove_dir_all(&root).unwrap();
        }
    }

    /// RED (U-1) — **a marker longer than a marker can be is unreadable, not
    /// read whole.**
    ///
    /// MUTATION: drop the `bytes.len() > cap` refusal in `capped`.
    #[test]
    fn install_channel_an_oversized_marker_is_unknown() {
        let root = install_folder("oversized");
        let mut bytes = SCOOP_MARKER.to_vec();
        bytes.resize(usize::try_from(MARKER_MAX_BYTES).unwrap() + 1, b' ');
        std::fs::write(root.join(MARKER_FILE_NAME), &bytes).unwrap();
        let (evidence, channel) = derived(&root, &me());
        assert_eq!(
            evidence.marker,
            MarkerEvidence::Unreadable(io::ErrorKind::FileTooLarge)
        );
        assert_eq!(channel, Channel::Unknown);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// RED (U-1) — **evidence that disagrees, or half a receipt, is
    /// `Unknown`.**
    ///
    /// A marker naming Homebrew in a folder scoop's receipt claims, and a
    /// folder with scoop's `install.json` and no `manifest.json` (or a manifest
    /// naming no version), are not answers.
    ///
    /// MUTATION: in `classify`, drop the `(Some(marker), true) if …` arm.
    #[test]
    fn install_channel_conflicting_or_partial_evidence_is_unknown() {
        let receipt = |root: &Path, manifest: &[u8]| {
            std::fs::write(root.join(SCOOP_INSTALL_RECEIPT), br#"{"bucket":"folio"}"#).unwrap();
            std::fs::write(root.join(SCOOP_MANIFEST_RECEIPT), manifest).unwrap();
        };
        let root = install_folder("conflict");
        receipt(&root, br#"{"version":"0.4.6"}"#);
        std::fs::write(
            root.join(MARKER_FILE_NAME),
            br#"{"v":1,"manager":"homebrew","uninstall_hook":false}"#,
        )
        .unwrap();
        assert_eq!(derived(&root, &me()).1, Channel::Unknown);
        std::fs::remove_dir_all(&root).unwrap();

        let root = install_folder("partial");
        std::fs::write(root.join(SCOOP_INSTALL_RECEIPT), br#"{"bucket":"folio"}"#).unwrap();
        let (evidence, channel) = derived(&root, &me());
        assert_eq!(evidence.receipt, ReceiptEvidence::Partial);
        assert_eq!(channel, Channel::Unknown);
        std::fs::remove_dir_all(&root).unwrap();

        let root = install_folder("versionless");
        receipt(&root, br#"{"bin":"folio.exe"}"#);
        let (evidence, channel) = derived(&root, &me());
        assert_eq!(evidence.receipt, ReceiptEvidence::Malformed);
        assert_eq!(channel, Channel::Unknown);
        std::fs::remove_dir_all(&root).unwrap();

        // The marker and scoop's receipt agreeing is scoop, with the marker's hook.
        let root = install_folder("agree");
        receipt(&root, br#"{"version":"0.4.6"}"#);
        std::fs::write(root.join(MARKER_FILE_NAME), SCOOP_MARKER).unwrap();
        assert_eq!(
            derived(&root, &me()).1,
            Channel::Managed {
                manager: Manager::Scoop,
                uninstall_hook: true,
            }
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// RED (U-1) — **every marker round-trips through its one spelling.**
    ///
    /// MUTATION: spell `uninstall_hook` as `hook` in `encode`.
    #[test]
    fn install_channel_the_marker_round_trips_through_encode_and_parse() {
        for manager in [Manager::Scoop, Manager::Homebrew, Manager::Winget] {
            for uninstall_hook in [false, true] {
                let marker = Marker {
                    manager,
                    uninstall_hook,
                };
                assert_eq!(Marker::parse(marker.encode().as_bytes()), Ok(marker));
            }
        }
        assert_eq!(
            Marker {
                manager: Manager::Homebrew,
                uninstall_hook: false,
            }
            .encode(),
            r#"{"v":1,"manager":"homebrew","uninstall_hook":false}"#
        );
    }

    /// RED (U-1) — **the example the design note documents parses, and so do
    /// the forms a PowerShell hook writes it in.**
    ///
    /// The example is the note's revision 2026-09-25 C2 table, verbatim. A
    /// scoop hook is PowerShell: `Set-Content` ends the line with CRLF, and
    /// Windows PowerShell 5.1's `-Encoding UTF8` starts the file with a
    /// byte-order mark. U-2's scripts are checked against these bytes.
    ///
    /// MUTATION: drop `without_bom` from `Marker::parse`.
    #[test]
    fn install_channel_the_documented_marker_example_parses() {
        let documented = r#"{"v":1,"manager":"scoop","uninstall_hook":true}"#;
        let marker = Marker {
            manager: Manager::Scoop,
            uninstall_hook: true,
        };
        let expected = Ok(marker);
        assert_eq!(Marker::parse(documented.as_bytes()), expected);
        assert_eq!(marker.encode(), documented);
        assert_eq!(
            Marker::parse(format!("{documented}\r\n").as_bytes()),
            expected
        );
        assert_eq!(
            Marker::parse(format!("\u{FEFF}{documented}\r\n").as_bytes()),
            expected
        );
        assert_eq!(
            Marker::parse(b"{ \"v\": 1, \"manager\": \"scoop\", \"uninstall_hook\": true }\n"),
            expected
        );
    }

    /// **The install folder is the bundle on macOS and the executable's folder
    /// elsewhere** — pure, over spelled paths.
    #[test]
    fn install_channel_the_install_root_is_the_bundle_on_macos() {
        let exe = Path::new("/Applications/Folio.app/Contents/MacOS/folio");
        assert_eq!(
            install_root(exe, HostPlatform::MacOs),
            Some(PathBuf::from("/Applications/Folio.app"))
        );
        assert_eq!(
            install_root(Path::new("/opt/build/folio"), HostPlatform::MacOs),
            Some(PathBuf::from("/opt/build"))
        );
        assert_eq!(
            install_root(
                Path::new("/x/Folio/Contents/MacOS/folio"),
                HostPlatform::MacOs
            ),
            Some(PathBuf::from("/x/Folio/Contents/MacOS"))
        );
        assert_eq!(
            install_root(exe, HostPlatform::Windows),
            Some(PathBuf::from("/Applications/Folio.app/Contents/MacOS"))
        );
    }

    /// **The derivation the product runs, over a temporary executable path**:
    /// the folder is found from the executable, read and classified; an
    /// executable path that cannot be read, or a platform with no marker
    /// location, is `Unknown`, and so is an account that cannot be read.
    #[test]
    fn install_channel_the_derivation_reads_the_executables_folder() {
        let root = install_folder("derive");
        std::fs::write(root.join(MARKER_FILE_NAME), SCOOP_MARKER).unwrap();
        let exe = root.join("folio.exe");
        let fact = derive_fact(Ok(exe.clone()), HostPlatform::Windows, Ok(me()), no_records);
        assert_eq!(
            fact.channel,
            Channel::Managed {
                manager: Manager::Scoop,
                uninstall_hook: true,
            }
        );
        let line = fact.line();
        assert!(
            line.starts_with(
                "Folio: install channel managed by scoop with an uninstall hook — marker {\"v\":1,"
            ),
            "{line}"
        );
        assert!(!line.contains(&*root.to_string_lossy()), "{line}");

        let fact = derive_fact(
            Ok(exe.clone()),
            HostPlatform::OtherUnix,
            Ok(me()),
            no_records,
        );
        assert_eq!(fact.channel, Channel::Unknown);
        let fact = derive_fact(
            Ok(exe),
            HostPlatform::Windows,
            Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            no_records,
        );
        assert_eq!(
            fact.evidence.map(|evidence| evidence.owner),
            Ok(OwnerEvidence::Unreadable(io::ErrorKind::PermissionDenied))
        );
        assert_eq!(fact.channel, Channel::Unknown);
        let fact = derive_fact(
            Err(io::Error::from(io::ErrorKind::NotFound)),
            HostPlatform::Windows,
            Ok(me()),
            no_records,
        );
        assert_eq!(fact.channel, Channel::Unknown);
        assert!(
            fact.line()
                .contains("the executable's path could not be read")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The scoop manifest and the cask this repository publishes (U-2): the
    /// sources `scripts/release/update-manifests.ps1` renders for a release.
    const SCOOP_MANIFEST: &str = include_str!("../../../packaging/scoop/folio.json");
    const CASK: &str = include_str!("../../../packaging/homebrew/folio.rb");

    /// The one single-quoted JSON object in `text` — the marker a hook writes,
    /// quoted as PowerShell and Ruby both quote a literal.
    fn quoted_marker(text: &str) -> &str {
        let found: Vec<_> = text.match_indices("'{").collect();
        assert_eq!(found.len(), 1, "one quoted marker in {text}");
        let start = found[0].0 + 1;
        let end = start
            + text[start..]
                .find("}'")
                .expect("the marker's closing quote")
            + 1;
        &text[start..end]
    }

    /// One hook's lines, out of the scoop manifest as scoop reads it.
    fn scoop_hook(name: &str) -> String {
        let manifest: serde_json::Value = serde_json::from_str(SCOOP_MANIFEST).unwrap();
        manifest[name]
            .as_array()
            .unwrap_or_else(|| panic!("the scoop manifest has a {name} block"))
            .iter()
            .map(|line| line.as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// RED (U-2) — **the marker each package manager writes is one this build
    /// reads as that manager's, with the hook the manifest runs.**
    ///
    /// The marker is written by scoop's `post_install` into `$dir` and by the
    /// cask's `postflight_steps` onto the bundle, from a literal in each manifest; a
    /// literal this parser refuses turns every managed copy into `Unknown`, and
    /// one that claims a hook the manifest does not run pre-ticks the Explorer
    /// row for a copy whose uninstall leaves it behind. So the literal is read
    /// out of the published sources and handed to [`Marker::parse`], and it
    /// must be [`Marker::encode`]'s own spelling. The cask says
    /// `uninstall_hook: false`: Homebrew runs a cask's `uninstall` steps on
    /// `brew upgrade` too (E-10), so its cleanup is in `zap` only.
    ///
    /// MUTATION: in `packaging/scoop/folio.json`'s `post_install`, write
    /// `"uninstall_hook":false` (or any key U-1 does not know).
    #[test]
    fn the_marker_each_package_manager_writes_reads_as_that_manager() {
        let post_install = scoop_hook("post_install");
        assert!(post_install.contains(MARKER_FILE_NAME), "{post_install}");
        let scoop = quoted_marker(&post_install);
        assert_eq!(
            Marker::parse(scoop.as_bytes()),
            Ok(Marker {
                manager: Manager::Scoop,
                uninstall_hook: true,
            })
        );
        assert_eq!(scoop.as_bytes(), SCOOP_MARKER);

        let postflight = &CASK[CASK
            .find("postflight_steps do")
            .expect("a postflight_steps block")..];
        let postflight = &postflight[..postflight.find("\n  end").unwrap()];
        assert!(postflight.contains(MARKER_ATTRIBUTE), "{postflight}");
        let homebrew = Marker {
            manager: Manager::Homebrew,
            uninstall_hook: false,
        };
        let cask = quoted_marker(postflight);
        assert_eq!(Marker::parse(cask.as_bytes()), Ok(homebrew));
        assert_eq!(cask, homebrew.encode());
        assert_eq!(scoop, Marker::parse(SCOOP_MARKER).unwrap().encode());
    }

    /// RED (U-2) — **the line each manifest hands Folio's cleanup door is one
    /// the door's own grammar accepts as the cleanup, not as a usage error.**
    ///
    /// `cli::uninstall_cleanup` answers anything after `--uninstall-cleanup`
    /// but `--purge` with its usage line and exit 1, and a hook maps exit 1 to
    /// "the uninstall goes on": a manifest that passed one more word (a
    /// `--quiet` the door does not have) would uninstall every copy having
    /// cleaned nothing, and say so only in a line scoop scrolls past. So the
    /// words are read out of the two hooks and handed to the grammar.
    ///
    /// MUTATION: in `packaging/scoop/folio.json`'s `pre_uninstall`, run
    /// `folio.exe --uninstall-cleanup --quiet`.
    #[test]
    fn the_cleanup_line_each_package_manager_runs_is_the_doors_own() {
        let pre_uninstall = scoop_hook("pre_uninstall");
        let (_, after) = pre_uninstall
            .split_once("folio.exe\"")
            .expect("the hook runs folio.exe");
        // The words end where the pipe the hook waits on begins.
        let scoop: Vec<_> = after[..after.find('|').unwrap()]
            .split_whitespace()
            .map(std::ffi::OsString::from)
            .collect();

        let zap = &CASK[CASK.find("zap script:").expect("a zap script")..];
        assert!(
            zap.contains("executable:   \"Folio.app/Contents/MacOS/folio\""),
            "{zap}"
        );
        let args = &zap[zap.find("args:").unwrap()..];
        let args = &args[args.find('[').unwrap() + 1..args.find(']').unwrap()];
        let homebrew: Vec<_> = args
            .split(',')
            .map(|word| std::ffi::OsString::from(word.trim().trim_matches('"')))
            .collect();

        for words in [scoop, homebrew] {
            assert_eq!(
                crate::cli::uninstall_cleanup(words.clone()),
                Some(Ok(crate::cli::UninstallDoor::Cleanup { purge: false })),
                "{words:?}"
            );
        }
    }
}
