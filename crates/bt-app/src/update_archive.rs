//! **The Windows release archive, read as something nobody has vouched for
//! yet** — the updater's archive reader
//! (`docs/plans/design/self-update-2026-09-16.md`, revision 2026-09-25 (b):
//! F-12, F-4 and experiment E-14; this module is (b).5's ticket U-14).
//!
//! Prepare (U-20) downloads `folio-<version>-windows-x64.zip` through
//! `bt_platform::http::https_download` and hands it here with an empty,
//! per-transaction staging directory. [`expand`] either writes every member of
//! the release into that directory under its own name, each one checked, or
//! refuses — naming the member and the reason — and leaves nothing under a
//! member's name. Nothing in the installation is touched either way.
//!
//! # What is checked, in the order it is checked
//!
//! 1. **The records, before a byte is inflated.** The end record, then the
//!    whole central directory, then every local header against its central
//!    entry: the same name byte for byte, the same method, flags, CRC and sizes
//!    (F-12: "local and central ZIP names must agree"). One disk, no ZIP64, no
//!    encryption, no data descriptor, stored or deflated only, no two entries'
//!    data overlapping, the directory ending where the end record starts.
//! 2. **Every name, by the grammar** ([`member_name`]): each entry is
//!    `folio-<version>/<name>` under the one root the offer's version names,
//!    `<name>` one component of letters, digits, `.`, `-` and `_`, no longer
//!    than [`NAME_MAX_BYTES`], not a device name, not ending in a dot or a
//!    space, not one of the [`RESERVED`] names, and no two names equal as
//!    Windows compares them. The root's own directory entry is allowed once and
//!    not counted (F-12); any other directory, and any link or reparse entry,
//!    is refused.
//! 3. **The bounds.** At most [`ARCHIVE_MAX_FILES`] files, and the sizes the
//!    directory declares add up to no more than [`ARCHIVE_MAX_BYTES`] — refused
//!    before anything is written, so the bound comes before the disk does.
//! 4. **`folio.exe`, then its manifest (E-14).** The executable is expanded
//!    first, under a temporary name, and the release manifest is read out of it
//!    through a [`ManifestSource`] — in the product
//!    [`EmbeddedManifest`], which is `bt_platform::pe_resource::read_rcdata`:
//!    `LoadLibraryExW` as a data file, so the new build is never run. The
//!    manifest's `product`, `version`, `arch` and `archive_root` must be the
//!    offer's, its `protocol` this build's, and its `min_updater` no newer than
//!    this build.
//! 5. **The members against the manifest (F-4).** Every member but
//!    `folio.exe` and `folio.msix` must be listed, every listed member present,
//!    and each one's declared size the listed size — refused before any of them
//!    is expanded.
//! 6. **Each member, as it inflates.** The output is counted as the
//!    decompressor produces it, and the decompressor is never given room for
//!    more than one byte past the member's size: a stream that would pass it
//!    is stopped there, before the write, and refused (F-12: "byte counters on
//!    the decompressor's actual output, checked before each write"). A stream
//!    that ends short, has bytes after its end, or fails its CRC is refused;
//!    a listed member's SHA-256 must be the manifest's.
//! 7. **The deadline**, checked before each member and before each chunk of
//!    one: a slow or adversarial stream ends at the caller's deadline, not at
//!    its own.
//!
//! # Writing
//!
//! Each member is written through `bt_platform::exclusive_create`: created
//! only where nothing of its name exists, relative to the staging directory's
//! held handle, never through a link. It is written as `~<name>` — a name the
//! grammar can never produce, so it collides with no member — and only when
//! every member has passed every check is each renamed to its own name, again
//! never over an existing one; if one of those renames is refused, the ones
//! already made are renamed back. **A refused expansion therefore leaves
//! nothing under a member's name**; its temporary files stay in the staging
//! directory, which is per-transaction and which the caller removes.
//!
//! # What it deliberately does not do
//!
//! It does not check a signature: `folio.exe`'s identity is U-15's, and the
//! manifest is only as good as that check, which Prepare makes before it
//! trusts what this returns. `folio.msix` is authenticated by its own
//! signature and version (F-5). It does not flush what it wrote to disk:
//! durability belongs to Prepare's journal write (U-11's door), which follows
//! it.
//!
//! **Nothing here is called from the product yet**: U-20's Windows Prepare is
//! its caller.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the reader lands before its caller: U-20's Windows Prepare calls it ((b).5)"
    )
)]

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Instant;

use bt_platform::exclusive_create::Directory;
use bt_platform::file_reads::{self, Lane};
use bt_winres::digest::{Sha256, hex};
use bt_winres::release_manifest::{self, Manifest};
use bt_winres::zip;
use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

use crate::install_channel::{MARKER_FILE_NAME, SCOOP_INSTALL_RECEIPT, SCOOP_MANIFEST_RECEIPT};
use crate::update::Version;

/// **The most files an archive may hold** (C4, F-12). The root's own
/// directory entry, if there is one, is not counted.
pub(crate) const ARCHIVE_MAX_FILES: usize = 32;

/// **The most the members may add up to, expanded** (C4): the download's own
/// ceiling, `bt_platform::https_download::DOWNLOAD_CEILING_LIMIT`.
pub(crate) const ARCHIVE_MAX_BYTES: u64 = bt_platform::https_download::DOWNLOAD_CEILING_LIMIT;

/// **The longest member name** (F-12's name-length bound), in bytes. The
/// longest today is `THIRD-PARTY-NOTICES.md`, 22; the bound keeps a staged
/// path far inside `MAX_PATH` under any install folder a person picks.
pub(crate) const NAME_MAX_BYTES: usize = 64;

/// The longest central directory read: 33 entries of the longest name and a
/// generous extra field each fit many times over.
const DIRECTORY_MAX_BYTES: u32 = 64 * 1024;

/// The longest manifest read out of an executable.
const MANIFEST_MAX_BYTES: usize = 64 * 1024;

/// One read from the archive, and the most one inflate call may produce.
const CHUNK_BYTES: usize = 64 * 1024;

/// The executable, which carries the manifest and is not listed in it.
pub(crate) const EXECUTABLE: &str = "folio.exe";

/// The package, signed with the same identity and not listed in the manifest.
pub(crate) const PACKAGE: &str = "folio.msix";

/// The folder an update transaction lives in (revision (b), (b).3).
const UPDATE_HOME: &str = ".folio-update";

/// **Names that are never payload** (F-12): the install marker and scoop's two
/// receipts, which say how *this* copy was installed (`install_channel`), and
/// the update's own folder. An archive that carried one would be rewriting the
/// evidence the updater reads to decide it may update at all.
pub(crate) const RESERVED: [&str; 4] = [
    MARKER_FILE_NAME,
    SCOOP_INSTALL_RECEIPT,
    SCOOP_MANIFEST_RECEIPT,
    UPDATE_HOME,
];

/// What a member is written as until every member has passed: `~` is a
/// character the grammar refuses, so no member can be called this.
const TEMPORARY_PREFIX: &str = "~";

/// The general purpose flags a member may carry: bits 1 and 2 (the deflate
/// compressor's level hint) and 11 (the name is UTF-8). Anything else —
/// encryption, a data descriptor, a patched or masked entry — is outside the
/// subset `package.ps1` writes.
const ALLOWED_FLAGS: u16 = 0b0000_1000_0000_0110;

// ───────────────────────────── the name grammar ─────────────────────────────

/// **A member name that passed the grammar**: one component, spelled the way
/// Windows stores it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalName(String);

impl CanonicalName {
    /// The name as the archive spells it.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The name as Windows compares it: ASCII without case. The grammar is
    /// ASCII, so there is no other normalization for two names to differ by.
    fn key(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

/// **Why a name is not a member name** — each class of F-12's grammar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NameRefusal {
    /// Nothing after the root.
    Empty,
    /// Longer than [`NAME_MAX_BYTES`].
    TooLong(usize),
    /// A `/` or a `\`: a folder below the root, or a separator variant.
    Separator,
    /// A `:`, which on NTFS names a stream of another file.
    Colon,
    /// A dot or a space at the end, which Windows drops, so two names would
    /// land as one.
    TrailingDotOrSpace,
    /// A character outside letters, digits, `.`, `-` and `_` — which includes
    /// a space, `~` (an 8.3 short-name alias is spelled with one), anything
    /// Windows forbids in a name, and anything that is not ASCII.
    Character(char),
    /// One of [`RESERVED`].
    Reserved,
    /// Starting with a dot.
    DotName,
    /// A device name — `CON`, `PRN`, `AUX`, `NUL`, `COM0`–`COM9`, `LPT0`–`LPT9`
    /// — with or without an extension, which Windows opens as the device.
    Device,
}

impl fmt::Display for NameRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "an empty name"),
            Self::TooLong(bytes) => {
                write!(formatter, "a name of {bytes} bytes, over {NAME_MAX_BYTES}")
            }
            Self::Separator => write!(formatter, "a path separator in the name"),
            Self::Colon => write!(formatter, "a stream separator `:` in the name"),
            Self::TrailingDotOrSpace => write!(formatter, "a name ending in a dot or a space"),
            Self::Character(character) => {
                write!(formatter, "the character {character:?} in the name")
            }
            Self::Reserved => write!(formatter, "a name reserved for the installation"),
            Self::DotName => write!(formatter, "a name starting with a dot"),
            Self::Device => write!(formatter, "a Windows device name"),
        }
    }
}

/// **The member name `name`, or the class of F-12's grammar it breaks.**
///
/// The grammar is a character set and not an allowlist of names: a later
/// release may add a member without this build learning its name, as long as
/// the name is one component of letters, digits, `.`, `-` and `_`. A release
/// that needs more declares it with a newer `min_updater`.
pub(crate) fn member_name(name: &str) -> Result<CanonicalName, NameRefusal> {
    if name.is_empty() {
        return Err(NameRefusal::Empty);
    }
    if name.len() > NAME_MAX_BYTES {
        return Err(NameRefusal::TooLong(name.len()));
    }
    if name.contains(['/', '\\']) {
        return Err(NameRefusal::Separator);
    }
    if name.contains(':') {
        return Err(NameRefusal::Colon);
    }
    if name.ends_with(['.', ' ']) {
        return Err(NameRefusal::TrailingDotOrSpace);
    }
    if let Some(character) = name.chars().find(|character| {
        !(character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
    }) {
        return Err(NameRefusal::Character(character));
    }
    if RESERVED
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(name))
    {
        return Err(NameRefusal::Reserved);
    }
    if name.starts_with('.') {
        return Err(NameRefusal::DotName);
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix)
            .is_some_and(|digit| digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit())
    };
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT")
    {
        return Err(NameRefusal::Device);
    }
    Ok(CanonicalName(name.to_owned()))
}

// ───────────────────────────── refusals ─────────────────────────────

/// **Why an archive was refused**, naming the member when there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    /// The entry the refusal is about, as the archive spells it, if it is
    /// about one.
    pub(crate) member: Option<String>,
    pub(crate) reason: Reason,
}

/// The reason half of a [`Refusal`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    /// The archive could not be read.
    Unreadable(String),
    /// The bytes are not the zip subset this reads.
    NotAnArchive(String),
    /// A ZIP64 field.
    Zip64,
    /// More than one disk.
    MultiDisk,
    /// More than [`ARCHIVE_MAX_FILES`] files.
    TooManyFiles(usize),
    /// A central directory longer than this reads.
    DirectoryTooLarge(u32),
    /// A name that is not UTF-8.
    NotText,
    /// A name the grammar refuses.
    Name(NameRefusal),
    /// An entry outside the one root the offer names.
    OutsideRoot,
    /// The root's directory entry more than once.
    RootTwice,
    /// Two entries whose names Windows would take as one.
    Duplicate,
    /// General purpose flags outside [`ALLOWED_FLAGS`].
    Flags(u16),
    /// A compression method other than stored or deflated.
    Method(u16),
    /// A link or a reparse entry.
    Link,
    /// A directory other than the root.
    Directory,
    /// The local header says something other than the central directory.
    LocalDisagrees(&'static str),
    /// Two entries' bytes overlap, or an entry runs into the directory.
    Overlaps,
    /// More than [`ARCHIVE_MAX_BYTES`], declared or produced.
    TooLarge(u64),
    /// No `folio.exe` to read the manifest from.
    NoExecutable,
    /// The manifest could not be read or parsed.
    Manifest(String),
    /// The manifest names another release than the one offered.
    ManifestSays {
        field: &'static str,
        found: String,
        expected: String,
    },
    /// The release needs a newer updater than this build.
    UpdaterTooOld { needs: String },
    /// The release speaks another update protocol.
    Protocol(u32),
    /// A member the manifest does not list.
    Unlisted,
    /// A member the manifest lists and the archive does not hold.
    Missing,
    /// The archive declares another size than the manifest lists.
    SizeDiffers { listed: u64, archive: u64 },
    /// The decompressor produced more than the member's size.
    Oversize { bound: u64 },
    /// The member ended before its size.
    Truncated { expected: u64, produced: u64 },
    /// The deflate stream is damaged.
    Corrupt(String),
    /// The member's CRC-32 is not the archive's.
    Crc,
    /// The member's SHA-256 is not the manifest's.
    Digest,
    /// The deadline passed.
    Deadline,
    /// Writing into the staging directory failed.
    Write(String),
}

impl fmt::Display for Reason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(detail) => write!(formatter, "the archive cannot be read: {detail}"),
            Self::NotAnArchive(detail) => write!(formatter, "not a release archive: {detail}"),
            Self::Zip64 => write!(formatter, "a ZIP64 field"),
            Self::MultiDisk => write!(formatter, "an archive of more than one disk"),
            Self::TooManyFiles(count) => {
                write!(formatter, "{count} files, over {ARCHIVE_MAX_FILES}")
            }
            Self::DirectoryTooLarge(bytes) => {
                write!(formatter, "a central directory of {bytes} bytes")
            }
            Self::NotText => write!(formatter, "a name that is not UTF-8"),
            Self::Name(refusal) => write!(formatter, "{refusal}"),
            Self::OutsideRoot => write!(formatter, "outside the release's folder"),
            Self::RootTwice => write!(formatter, "the release's folder listed twice"),
            Self::Duplicate => write!(formatter, "a second entry of the same name"),
            Self::Flags(flags) => write!(formatter, "the entry flags {flags:#06x}"),
            Self::Method(method) => write!(formatter, "compression method {method}"),
            Self::Link => write!(formatter, "a link"),
            Self::Directory => write!(formatter, "a folder"),
            Self::LocalDisagrees(field) => write!(
                formatter,
                "its local header and the central directory disagree on the {field}"
            ),
            Self::Overlaps => write!(formatter, "bytes shared with another entry"),
            Self::TooLarge(bytes) => write!(
                formatter,
                "{bytes} bytes expanded, over {ARCHIVE_MAX_BYTES}"
            ),
            Self::NoExecutable => write!(formatter, "no {EXECUTABLE}"),
            Self::Manifest(detail) => write!(formatter, "the release manifest: {detail}"),
            Self::ManifestSays {
                field,
                found,
                expected,
            } => write!(
                formatter,
                "the release manifest says {field} {found}, the offer {expected}"
            ),
            Self::UpdaterTooOld { needs } => {
                write!(
                    formatter,
                    "the release needs an updater of {needs} or later"
                )
            }
            Self::Protocol(protocol) => write!(
                formatter,
                "the release speaks update protocol {protocol}, this build {}",
                release_manifest::PROTOCOL
            ),
            Self::Unlisted => write!(formatter, "not listed in the release manifest"),
            Self::Missing => write!(
                formatter,
                "listed in the release manifest and not in the archive"
            ),
            Self::SizeDiffers { listed, archive } => write!(
                formatter,
                "{archive} bytes in the archive, {listed} in the release manifest"
            ),
            Self::Oversize { bound } => {
                write!(formatter, "it inflates past its {bound} bytes")
            }
            Self::Truncated { expected, produced } => {
                write!(formatter, "{produced} bytes of {expected}")
            }
            Self::Corrupt(detail) => write!(formatter, "damaged: {detail}"),
            Self::Crc => write!(formatter, "its CRC-32 is not the archive's"),
            Self::Digest => write!(formatter, "its SHA-256 is not the release manifest's"),
            Self::Deadline => write!(formatter, "the deadline passed"),
            Self::Write(detail) => write!(formatter, "it cannot be staged: {detail}"),
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.member {
            Some(member) => write!(formatter, "{member}: {}", self.reason),
            None => write!(formatter, "{}", self.reason),
        }
    }
}

fn refused(reason: Reason) -> Refusal {
    Refusal {
        member: None,
        reason,
    }
}

fn refused_at(member: &str, reason: Reason) -> Refusal {
    Refusal {
        member: Some(member.to_owned()),
        reason,
    }
}

// ───────────────────────────── seams ─────────────────────────────

/// **What the release is supposed to be**: the offer's version and this
/// build's architecture and version.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Expected<'a> {
    /// The offered version, as the manifest spells it (no `v`).
    pub(crate) version: &'a str,
    /// The architecture word the archive's name carries (`x64`).
    pub(crate) arch: &'a str,
    /// This build's version, held to the manifest's `min_updater`.
    pub(crate) updater: &'a str,
}

/// **Where the release manifest comes from**: the staged executable's own
/// resource in the product ([`EmbeddedManifest`]); the text a test built in
/// its tests, because the executables a test can build carry the manifest of
/// the build that made them rather than of the archive the test made.
pub(crate) trait ManifestSource {
    /// The manifest's text, out of the executable at `executable`.
    fn manifest_text(&self, executable: &Path) -> Result<String, String>;
}

/// **The E-14 door**: the `RCDATA` resource
/// [`release_manifest::RESOURCE_NAME`], read from the executable as a data file
/// — never run.
pub(crate) struct EmbeddedManifest;

impl ManifestSource for EmbeddedManifest {
    fn manifest_text(&self, executable: &Path) -> Result<String, String> {
        let bytes = bt_platform::pe_resource::read_rcdata(
            executable,
            release_manifest::RESOURCE_NAME,
            MANIFEST_MAX_BYTES,
        )
        .map_err(|error| error.to_string())?;
        String::from_utf8(bytes).map_err(|_| "it is not UTF-8".to_owned())
    }
}

/// What one call of an [`Inflate`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Inflated {
    /// Input bytes it took.
    pub(crate) consumed: usize,
    /// Output bytes it wrote.
    pub(crate) produced: usize,
    /// Whether the stream's end was reached.
    pub(crate) done: bool,
}

/// **A deflate decompressor, a bounded step at a time.** The product's is
/// [`Miniz`]; a test hands in one that is slow, to hold the deadline.
pub(crate) trait Inflate {
    /// Inflate from `input` into `output`, which is never larger than the
    /// member has room left for plus one byte.
    fn inflate(&mut self, input: &[u8], output: &mut [u8]) -> Result<Inflated, String>;
}

/// `miniz_oxide`'s streaming inflater over raw deflate.
pub(crate) struct Miniz(Box<InflateState>);

impl Miniz {
    pub(crate) fn new() -> Self {
        Self(InflateState::new_boxed(DataFormat::Raw))
    }
}

impl Inflate for Miniz {
    fn inflate(&mut self, input: &[u8], output: &mut [u8]) -> Result<Inflated, String> {
        let result = inflate(&mut self.0, input, output, MZFlush::None);
        let done = match result.status {
            Ok(MZStatus::StreamEnd) => true,
            // `Buf` is "no progress possible with what it was given": the
            // caller sees nothing consumed and nothing produced, and decides.
            Ok(_) | Err(MZError::Buf) => false,
            Err(error) => return Err(format!("the deflate stream fails with {error:?}")),
        };
        Ok(Inflated {
            consumed: result.bytes_consumed,
            produced: result.bytes_written,
            done,
        })
    }
}

/// **When the expansion must have ended**, and the clock it is read on.
pub(crate) struct Deadline<'a> {
    at: Instant,
    now: &'a dyn Fn() -> Instant,
}

impl Deadline<'static> {
    /// `at`, on the monotonic clock.
    pub(crate) fn at(at: Instant) -> Self {
        Self {
            at,
            now: &Instant::now,
        }
    }
}

impl<'a> Deadline<'a> {
    /// `at`, on the clock `now` — a test's.
    pub(crate) fn on(at: Instant, now: &'a dyn Fn() -> Instant) -> Self {
        Self { at, now }
    }

    fn check(&self, member: Option<&str>) -> Result<(), Refusal> {
        if (self.now)() >= self.at {
            return Err(Refusal {
                member: member.map(str::to_owned),
                reason: Reason::Deadline,
            });
        }
        Ok(())
    }
}

/// **What an accepted archive held**: its manifest, and the members now in the
/// staging directory under their own names, in the archive's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Expanded {
    pub(crate) manifest: Manifest,
    pub(crate) members: Vec<String>,
}

// ───────────────────────────── the reader ─────────────────────────────

/// **Expand the release archive at `archive` into `staging`, or refuse.**
///
/// The archive's bytes are read on `file_reads`' `Lane::Update`. See the
/// module for every check and its order.
pub(crate) fn expand(
    archive: &Path,
    expected: &Expected<'_>,
    staging: &Directory,
    manifest: &dyn ManifestSource,
    deadline: &Deadline<'_>,
) -> Result<Expanded, Refusal> {
    let file = file_reads::open(Lane::Update, archive)
        .map_err(|error| refused(Reason::Unreadable(error.to_string())))?;
    expand_from(file, expected, staging, manifest, deadline, &mut || {
        Box::new(Miniz::new())
    })
}

/// One entry of the archive that is a member, with where its data is.
struct Located {
    /// The entry's full name, `folio-<version>/<name>`.
    entry: String,
    name: CanonicalName,
    central: zip::CentralEntry,
    data_start: u64,
}

/// [`expand`] over any seekable reader, with the decompressor a parameter.
fn expand_from<R: Read + Seek>(
    mut archive: R,
    expected: &Expected<'_>,
    staging: &Directory,
    manifest_source: &dyn ManifestSource,
    deadline: &Deadline<'_>,
    new_inflater: &mut dyn FnMut() -> Box<dyn Inflate>,
) -> Result<Expanded, Refusal> {
    deadline.check(None)?;
    let root = release_manifest::archive_root(expected.version);
    let members = directory(&mut archive, &root)?;

    let executable = members
        .iter()
        .find(|member| member.name.as_str() == EXECUTABLE)
        .ok_or_else(|| refused(Reason::NoExecutable))?;
    let mut total = 0u64;
    expand_member(
        &mut archive,
        executable,
        u64::from(executable.central.size),
        &mut total,
        staging,
        deadline,
        new_inflater,
    )?;

    let text = manifest_source
        .manifest_text(&staging.path().join(temporary(EXECUTABLE)))
        .map_err(|detail| refused_at(&executable.entry, Reason::Manifest(detail)))?;
    let manifest = Manifest::parse(&text)
        .map_err(|error| refused_at(&executable.entry, Reason::Manifest(error.to_string())))?;
    offered(&manifest, expected, &root).map_err(|reason| refused_at(&executable.entry, reason))?;

    for member in &members {
        let name = member.name.as_str();
        if name == EXECUTABLE || name == PACKAGE {
            continue;
        }
        let listed = manifest
            .members
            .iter()
            .find(|listed| listed.name == name)
            .ok_or_else(|| refused_at(&member.entry, Reason::Unlisted))?;
        let archive_size = u64::from(member.central.size);
        if listed.size != archive_size {
            return Err(refused_at(
                &member.entry,
                Reason::SizeDiffers {
                    listed: listed.size,
                    archive: archive_size,
                },
            ));
        }
    }
    for listed in &manifest.members {
        if !members
            .iter()
            .any(|member| member.name.as_str() == listed.name)
        {
            return Err(refused_at(
                &format!("{root}/{}", listed.name),
                Reason::Missing,
            ));
        }
    }

    for member in &members {
        let name = member.name.as_str();
        if name == EXECUTABLE {
            continue;
        }
        let digest = expand_member(
            &mut archive,
            member,
            u64::from(member.central.size),
            &mut total,
            staging,
            deadline,
            new_inflater,
        )?;
        if let Some(listed) = manifest.members.iter().find(|listed| listed.name == name)
            && listed.sha256 != digest
        {
            return Err(refused_at(&member.entry, Reason::Digest));
        }
    }

    for (index, member) in members.iter().enumerate() {
        let name = member.name.as_str();
        if let Err(error) = staging.rename_new(&temporary(name), name) {
            // The members already renamed go back to their temporary names, so
            // a refusal leaves none of them under its own. Each name was just
            // this call's, so the way back is free unless something outside
            // it is writing into the staging directory.
            for done in members[..index].iter().rev() {
                let done = done.name.as_str();
                let _ = staging.rename_new(done, &temporary(done));
            }
            return Err(refused_at(&member.entry, Reason::Write(error.to_string())));
        }
    }
    Ok(Expanded {
        manifest,
        members: members
            .into_iter()
            .map(|member| member.name.as_str().to_owned())
            .collect(),
    })
}

fn temporary(name: &str) -> String {
    format!("{TEMPORARY_PREFIX}{name}")
}

/// The manifest's header against the offer and this build.
fn offered(manifest: &Manifest, expected: &Expected<'_>, root: &str) -> Result<(), Reason> {
    for (field, found, wanted) in [
        (
            "product",
            manifest.product.as_str(),
            release_manifest::PRODUCT,
        ),
        ("version", manifest.version.as_str(), expected.version),
        ("arch", manifest.arch.as_str(), expected.arch),
        ("archive_root", manifest.archive_root.as_str(), root),
    ] {
        if found != wanted {
            return Err(Reason::ManifestSays {
                field,
                found: found.to_owned(),
                expected: wanted.to_owned(),
            });
        }
    }
    if manifest.protocol != release_manifest::PROTOCOL {
        return Err(Reason::Protocol(manifest.protocol));
    }
    let needs = Version::parse(&manifest.min_updater).ok_or_else(|| {
        Reason::Manifest(format!(
            "min_updater {} is not a version",
            manifest.min_updater
        ))
    })?;
    let this = Version::parse(expected.updater).ok_or_else(|| {
        Reason::Manifest(format!(
            "this build's version {} is not a version",
            expected.updater
        ))
    })?;
    if this < needs {
        return Err(Reason::UpdaterTooOld {
            needs: manifest.min_updater.clone(),
        });
    }
    Ok(())
}

/// `length` bytes at `at`.
fn read_at<R: Read + Seek>(archive: &mut R, at: u64, length: usize) -> Result<Vec<u8>, Refusal> {
    archive
        .seek(SeekFrom::Start(at))
        .map_err(|error| refused(Reason::Unreadable(error.to_string())))?;
    let mut bytes = vec![0; length];
    archive.read_exact(&mut bytes).map_err(|error| {
        refused(if error.kind() == io::ErrorKind::UnexpectedEof {
            Reason::NotAnArchive(format!("{length} bytes at offset {at} run past the end"))
        } else {
            Reason::Unreadable(error.to_string())
        })
    })?;
    Ok(bytes)
}

/// **Steps 1–3 of the module**: the records, the names and the bounds. Returns
/// the members in the archive's order.
fn directory<R: Read + Seek>(archive: &mut R, root: &str) -> Result<Vec<Located>, Refusal> {
    let length = archive
        .seek(SeekFrom::End(0))
        .map_err(|error| refused(Reason::Unreadable(error.to_string())))?;
    let tail_length = length.min((zip::END_RECORD_BYTES + zip::MAX_COMMENT_BYTES) as u64);
    let tail_start = length - tail_length;
    let tail = read_at(archive, tail_start, tail_length as usize)?;
    let (at, end) = zip::find_end(&tail).map_err(|detail| refused(Reason::NotAnArchive(detail)))?;
    let end_offset = tail_start + at as u64;

    if end.entries == u16::MAX || end.directory_size == u32::MAX || end.directory_offset == u32::MAX
    {
        return Err(refused(Reason::Zip64));
    }
    if end.disk != 0 || end.directory_disk != 0 || end.entries_on_disk != end.entries {
        return Err(refused(Reason::MultiDisk));
    }
    if usize::from(end.entries) > ARCHIVE_MAX_FILES + 1 {
        return Err(refused(Reason::TooManyFiles(usize::from(end.entries))));
    }
    if end.directory_size > DIRECTORY_MAX_BYTES {
        return Err(refused(Reason::DirectoryTooLarge(end.directory_size)));
    }
    if u64::from(end.directory_offset) + u64::from(end.directory_size) != end_offset {
        return Err(refused(Reason::NotAnArchive(
            "the central directory does not end where the end record starts".to_owned(),
        )));
    }
    let bytes = read_at(
        archive,
        u64::from(end.directory_offset),
        end.directory_size as usize,
    )?;

    let mut members: Vec<Located> = Vec::new();
    let mut root_seen = false;
    let mut spans: Vec<(u64, u64, String)> = Vec::new();
    let mut cursor = 0;
    for _ in 0..end.entries {
        let (central, next) = zip::central_entry(&bytes, cursor)
            .map_err(|detail| refused(Reason::NotAnArchive(detail)))?;
        cursor = next;
        let entry = std::str::from_utf8(&central.name)
            .map_err(|_| refused_at(&String::from_utf8_lossy(&central.name), Reason::NotText))?
            .to_owned();
        let fail = |reason| refused_at(&entry, reason);

        if central.disk_start != 0 {
            return Err(fail(Reason::MultiDisk));
        }
        if central.compressed_size == u32::MAX
            || central.size == u32::MAX
            || central.local_offset == u32::MAX
        {
            return Err(fail(Reason::Zip64));
        }
        if central.flags & !ALLOWED_FLAGS != 0 {
            return Err(fail(Reason::Flags(central.flags)));
        }
        if is_link(&central) {
            return Err(fail(Reason::Link));
        }

        let Some(rest) = entry
            .strip_prefix(root)
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            return Err(fail(if entry.contains('\\') {
                Reason::Name(NameRefusal::Separator)
            } else {
                Reason::OutsideRoot
            }));
        };
        let data_start = local_header(archive, &central, &entry)?;
        let data_end = data_start + u64::from(central.compressed_size);
        spans.push((u64::from(central.local_offset), data_end, entry.clone()));

        if rest.is_empty() {
            if root_seen {
                return Err(fail(Reason::RootTwice));
            }
            if central.size != 0 || central.compressed_size != 0 {
                return Err(fail(Reason::Directory));
            }
            root_seen = true;
            continue;
        }
        let name = member_name(rest).map_err(|refusal| fail(Reason::Name(refusal)))?;
        if is_directory(&central) {
            return Err(fail(Reason::Directory));
        }
        if central.method != zip::STORED && central.method != zip::DEFLATED {
            return Err(fail(Reason::Method(central.method)));
        }
        if central.method == zip::STORED && central.compressed_size != central.size {
            return Err(fail(Reason::Corrupt(
                "a stored entry whose two sizes differ".to_owned(),
            )));
        }
        if members.iter().any(|member| member.name.key() == name.key()) {
            return Err(fail(Reason::Duplicate));
        }
        members.push(Located {
            entry: entry.clone(),
            name,
            central,
            data_start,
        });
        if members.len() > ARCHIVE_MAX_FILES {
            return Err(refused(Reason::TooManyFiles(members.len())));
        }
    }
    if cursor != bytes.len() {
        return Err(refused(Reason::NotAnArchive(
            "the central directory holds more than its entries".to_owned(),
        )));
    }

    spans.sort_by_key(|span| span.0);
    for pair in spans.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(refused_at(&pair[1].2, Reason::Overlaps));
        }
    }
    if let Some(last) = spans.last()
        && last.1 > u64::from(end.directory_offset)
    {
        return Err(refused_at(&last.2, Reason::Overlaps));
    }

    let declared: u64 = members
        .iter()
        .map(|member| u64::from(member.central.size))
        .sum();
    if declared > ARCHIVE_MAX_BYTES {
        return Err(refused(Reason::TooLarge(declared)));
    }
    Ok(members)
}

/// **The local header of `central`, held to it**; returns where its data
/// starts.
fn local_header<R: Read + Seek>(
    archive: &mut R,
    central: &zip::CentralEntry,
    entry: &str,
) -> Result<u64, Refusal> {
    let at = u64::from(central.local_offset);
    let fixed = read_at(archive, at, zip::LOCAL_HEADER_BYTES)?;
    let length = zip::local_header_length(&fixed)
        .map_err(|detail| refused_at(entry, Reason::NotAnArchive(detail)))?;
    let bytes = read_at(archive, at, length)?;
    let local = zip::local_header(&bytes)
        .map_err(|detail| refused_at(entry, Reason::NotAnArchive(detail)))?;
    for (field, agrees) in [
        ("name", local.name == central.name),
        ("method", local.method == central.method),
        ("flags", local.flags == central.flags),
        ("CRC", local.crc32 == central.crc32),
        (
            "compressed size",
            local.compressed_size == central.compressed_size,
        ),
        ("size", local.size == central.size),
    ] {
        if !agrees {
            return Err(refused_at(entry, Reason::LocalDisagrees(field)));
        }
    }
    Ok(at + length as u64)
}

/// A symlink by its Unix mode, or a reparse point by its Windows attribute.
fn is_link(central: &zip::CentralEntry) -> bool {
    const HOST_UNIX: u16 = 3;
    const S_IFMT: u32 = 0o170_000;
    const S_IFLNK: u32 = 0o120_000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let unix_link = central.version_made_by >> 8 == HOST_UNIX
        && (central.external_attributes >> 16) & S_IFMT == S_IFLNK;
    unix_link || central.external_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// A directory by its Unix mode or its MS-DOS attribute.
fn is_directory(central: &zip::CentralEntry) -> bool {
    const HOST_UNIX: u16 = 3;
    const S_IFMT: u32 = 0o170_000;
    const S_IFDIR: u32 = 0o040_000;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    let unix_directory = central.version_made_by >> 8 == HOST_UNIX
        && (central.external_attributes >> 16) & S_IFMT == S_IFDIR;
    unix_directory || central.external_attributes & FILE_ATTRIBUTE_DIRECTORY != 0
}

/// **Step 6 for one member**: inflate it into `~<name>` in `staging`, never
/// producing more than `size` bytes, and return its SHA-256.
fn expand_member<R: Read + Seek>(
    archive: &mut R,
    member: &Located,
    size: u64,
    total: &mut u64,
    staging: &Directory,
    deadline: &Deadline<'_>,
    new_inflater: &mut dyn FnMut() -> Box<dyn Inflate>,
) -> Result<String, Refusal> {
    let entry = member.entry.as_str();
    deadline.check(Some(entry))?;
    let file = staging
        .create_new(&temporary(member.name.as_str()))
        .map_err(|error| refused_at(entry, Reason::Write(error.to_string())))?;
    archive
        .seek(SeekFrom::Start(member.data_start))
        .map_err(|error| refused_at(entry, Reason::Unreadable(error.to_string())))?;

    let mut sink = Sink {
        file,
        size,
        produced: 0,
        total,
        sha: Sha256::new(),
        crc: zip::Crc32::new(),
        entry,
    };
    let mut input = vec![0u8; CHUNK_BYTES];
    let mut remaining = u64::from(member.central.compressed_size);
    let read_chunk = |archive: &mut R, input: &mut [u8], remaining: &mut u64| {
        let take = (*remaining).min(input.len() as u64) as usize;
        archive.read_exact(&mut input[..take]).map_err(|error| {
            refused_at(
                entry,
                if error.kind() == io::ErrorKind::UnexpectedEof {
                    Reason::NotAnArchive("the data runs past the end".to_owned())
                } else {
                    Reason::Unreadable(error.to_string())
                },
            )
        })?;
        *remaining -= take as u64;
        Ok::<usize, Refusal>(take)
    };

    if member.central.method == zip::STORED {
        while remaining > 0 {
            deadline.check(Some(entry))?;
            let taken = read_chunk(archive, &mut input, &mut remaining)?;
            sink.write(&input[..taken])?;
        }
    } else {
        let mut inflater = new_inflater();
        let mut output = vec![0u8; CHUNK_BYTES];
        let (mut start, mut end) = (0, 0);
        loop {
            deadline.check(Some(entry))?;
            if start == end && remaining > 0 {
                end = read_chunk(archive, &mut input, &mut remaining)?;
                start = 0;
            }
            // Room for the rest of the member and one byte more: the one byte
            // is how a stream that would pass its size is seen doing so.
            let room = (size - sink.produced + 1).min(CHUNK_BYTES as u64) as usize;
            let step = inflater
                .inflate(&input[start..end], &mut output[..room])
                .map_err(|detail| refused_at(entry, Reason::Corrupt(detail)))?;
            start += step.consumed;
            sink.write(&output[..step.produced])?;
            if step.done {
                if start < end || remaining > 0 {
                    return Err(refused_at(
                        entry,
                        Reason::Corrupt("bytes after the end of its deflate stream".to_owned()),
                    ));
                }
                break;
            }
            if step.consumed == 0 && step.produced == 0 {
                return Err(refused_at(
                    entry,
                    if start == end && remaining == 0 {
                        Reason::Truncated {
                            expected: size,
                            produced: sink.produced,
                        }
                    } else {
                        Reason::Corrupt("the decompressor makes no progress".to_owned())
                    },
                ));
            }
        }
    }
    sink.finish(member.central.crc32)
}

/// Where a member's bytes go: counted, bounded and hashed before each write.
struct Sink<'a> {
    file: File,
    size: u64,
    produced: u64,
    total: &'a mut u64,
    sha: Sha256,
    crc: zip::Crc32,
    entry: &'a str,
}

impl Sink<'_> {
    fn write(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
        let length = bytes.len() as u64;
        if self.produced + length > self.size {
            return Err(refused_at(
                self.entry,
                Reason::Oversize { bound: self.size },
            ));
        }
        if *self.total + length > ARCHIVE_MAX_BYTES {
            return Err(refused_at(
                self.entry,
                Reason::TooLarge(*self.total + length),
            ));
        }
        self.file
            .write_all(bytes)
            .map_err(|error| refused_at(self.entry, Reason::Write(error.to_string())))?;
        self.sha.update(bytes);
        self.crc.update(bytes);
        self.produced += length;
        *self.total += length;
        Ok(())
    }

    fn finish(self, crc32: u32) -> Result<String, Refusal> {
        if self.produced != self.size {
            return Err(refused_at(
                self.entry,
                Reason::Truncated {
                    expected: self.size,
                    produced: self.produced,
                },
            ));
        }
        if self.crc.finish() != crc32 {
            return Err(refused_at(self.entry, Reason::Crc));
        }
        drop(self.file);
        Ok(hex(&self.sha.finish()))
    }
}

#[cfg(test)]
#[path = "update_archive_tests.rs"]
mod tests;
