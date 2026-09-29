//! **Is this downloaded file the same publisher's Folio** — the Windows
//! identity check of the self-updater (0.4.6 ticket U-15;
//! `docs/plans/design/self-update-2026-09-16.md` §E "Windows", C8, revision
//! (b) F-5 and F-16, experiments E-6 and E-13).
//!
//! # The check, in order, each refusal named
//!
//! 1. **Authenticode trust** — `WinVerifyTrust` with
//!    `WINTRUST_ACTION_GENERIC_VERIFY_V2`, revocation over the whole chain but
//!    the root, and no lifetime-signing flag (the time stamp is what keeps a
//!    signature valid past its three-day certificate, so lifetime signing would
//!    refuse every release older than three days). A file with no signature is
//!    [`Refusal::Unsigned`]; any other answer but success is
//!    [`Refusal::Untrusted`] with Windows' code.
//! 2. **A required RFC 3161 time stamp** inside the signer's certificate's
//!    validity — the `1.3.6.1.4.1.311.3.3.1` countersignature `signtool /tr …
//!    /td SHA256` writes (`scripts/release/sign.ps1`), verified by
//!    `CryptVerifyTimeStampSignature` against the signature it stamps, its
//!    authority's chain trusted for time stamping at the stamped time. None:
//!    [`Refusal::NoTimestamp`]; one that does not verify:
//!    [`Refusal::TimestampInvalid`]; a time outside the leaf's validity:
//!    [`Refusal::TimestampOutsideValidity`].
//! 3. **Revocation** — read from the signer's chain as Windows built it:
//!    revoked is [`Refusal::Revoked`], and **"unknown" is
//!    [`Refusal::RevocationUnknown`]** (F-5: at download time the machine is
//!    online by construction; the next press asks again). Windows' own
//!    default policy passes an offline revocation check, which is why this is
//!    read from the chain rather than left to `WinVerifyTrust`'s answer.
//! 4. **The identity-validation OID** — the leaf's one extended key usage
//!    under `1.3.6.1.4.1.311.97.` that is not Artifact Signing's Public Trust
//!    marker ([`identity_oid`]), equal to the running build's:
//!    [`Refusal::NoIdentity`] / [`Refusal::IdentityDiffers`].
//! 5. **The subject** — the leaf's distinguished name, parsed by
//!    [`crate::msix::distinguished_name`] (type upper-cased, value case kept,
//!    order kept), equal to the running build's: [`Refusal::SubjectDiffers`].
//! 6. **`VERSIONINFO` and the machine type** equal to the offer's:
//!    [`Refusal::NoVersion`], [`Refusal::VersionDiffers`],
//!    [`Refusal::MachineDiffers`].
//! 7. **`folio.msix` only** — the manifest's `Publisher` equal to its own
//!    signer's subject ([`crate::msix::publisher_matches_subject`]),
//!    `Identity/@Version` equal to the offer's four-part version and
//!    `ProcessorArchitecture` to the offer's machine
//!    ([`Refusal::PublisherIsNotSigner`], [`Refusal::VersionDiffers`],
//!    [`Refusal::MachineDiffers`]); steps 1 to 5 as above.
//!
//! **The two Microsoft sidecars** (`conpty.dll`, `OpenConsole.exe`) get the
//! checks `package.ps1 -Sign` gives them through `sign.ps1 -VerifyOnly`:
//! Windows calls the signature valid, and it carries a time stamp —
//! [`verify_sidecar`] runs steps 1 to 3. `sign.ps1` does not check that the
//! signer is Microsoft, and neither does this: the release manifest's hashes
//! (F-4) are what carry the sidecars' identity.
//!
//! # What is compared, and what never is
//!
//! Artifact Signing certificates are valid for 72 hours and renewed daily
//! (C8, F-5), so a release is normally signed by a certificate the running
//! build has never seen. Nothing here pins a thumbprint, a key or a serial.
//! The [`Expectation`] is read **from the running `folio.exe`**
//! ([`running_identity`]) — its leaf's subject and identity OID, its machine
//! type — and never from a constant; the version is the offer's
//! ([`Expectation::for_offer`]).
//!
//! # The capability
//!
//! [`running_capability`] answers the owner's ruling of 2026-09-25 (a copy
//! may update itself only when it is signed **and** flagged): the flag is
//! `bt-app`'s build fact, passed in; the signature is this build's own
//! identity read as above. Its reader is the Windows Prepare (U-20), before
//! any download — not the update job's eligibility (U-18 decision 6). No
//! product code calls it yet.
//!
//! # Policy, and the test engine
//!
//! [`Policy::System`] is the product's: Windows' own roots and revocation.
//! [`Policy::ExclusiveRoot`] exists for tests (E-13): a chain engine made by
//! `CertCreateCertificateChainEngine` whose **only** trusted root is the one
//! it is given, in a memory store, so a certificate made inside a test
//! validates without anything being installed. Under it `WinVerifyTrust`
//! still verifies the file's digest and signature, and its one expected
//! refusal — the policy step's, because the test root is not Windows' — is
//! re-asked of the exclusive engine. **No certificate store of the system is
//! ever opened for writing, and nothing is ever installed** (pinned by
//! `trust_tests::the_machine_store_is_never_written`).
//!
//! **Worker only.** Every call reads the file, and revocation may reach the
//! network; the reads are charged to [`crate::file_reads`]' `Lane::Update`.
//! Prepare runs on `bt-update-job`. **Off Windows** every call refuses with
//! [`Refusal::Unsupported`]; the decisions are pure and compile everywhere.

use std::fmt;
use std::path::Path;

pub use crate::msix::Rdn;

/// The prefix of every Artifact Signing extended key usage (F-5).
pub const IDENTITY_PREFIX: &str = "1.3.6.1.4.1.311.97.";

/// The EKU **every** Artifact Signing Public Trust certificate carries, whoever
/// it was issued to — a type marker, not an identity (Microsoft's
/// "Artifact Signing certificate management"; measured on 0.4.5's leaf by E-6).
pub const PUBLIC_TRUST_MARKER: &str = "1.3.6.1.4.1.311.97.1.0";

/// `id-kp-codeSigning`.
pub const CODE_SIGNING: &str = "1.3.6.1.5.5.7.3.3";

/// `id-kp-timeStamping`, which a time-stamp authority's chain must be valid for.
pub const TIME_STAMPING: &str = "1.3.6.1.5.5.7.3.8";

/// The unsigned attribute an RFC 3161 time stamp travels in inside an
/// Authenticode signature (`szOID_RFC3161_counterSign`).
pub const RFC3161_COUNTERSIGNATURE: &str = "1.3.6.1.4.1.311.3.3.1";

// ── Windows' numbers, spelled here so the decisions compile everywhere ─────────

/// `CERT_TRUST_IS_REVOKED`.
pub const CHAIN_IS_REVOKED: u32 = 0x0000_0004;
/// `CERT_TRUST_REVOCATION_STATUS_UNKNOWN`.
pub const CHAIN_REVOCATION_UNKNOWN: u32 = 0x0000_0040;
/// `CERT_TRUST_IS_OFFLINE_REVOCATION`.
pub const CHAIN_OFFLINE_REVOCATION: u32 = 0x0100_0000;

/// `TRUST_E_NOSIGNATURE`.
pub const TRUST_E_NOSIGNATURE: i32 = 0x800B_0100_u32.cast_signed();
/// `CERT_E_REVOKED`.
pub const CERT_E_REVOKED: i32 = 0x800B_010C_u32.cast_signed();
/// `CERT_E_REVOCATION_FAILURE`.
pub const CERT_E_REVOCATION_FAILURE: i32 = 0x800B_010E_u32.cast_signed();
/// `CRYPT_E_REVOKED`.
pub const CRYPT_E_REVOKED: i32 = 0x8009_2010_u32.cast_signed();
/// `CRYPT_E_NO_REVOCATION_CHECK`.
pub const CRYPT_E_NO_REVOCATION_CHECK: i32 = 0x8009_2012_u32.cast_signed();
/// `CRYPT_E_REVOCATION_OFFLINE`.
pub const CRYPT_E_REVOCATION_OFFLINE: i32 = 0x8009_2013_u32.cast_signed();

/// A `VS_FIXEDFILEINFO` file version (or a package's `Identity/@Version`):
/// four numbers, compared as four numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileVersion(pub [u16; 4]);

impl FileVersion {
    /// `major.minor.patch.build` from the two halves `VS_FIXEDFILEINFO` keeps.
    #[must_use]
    pub const fn from_halves(most: u32, least: u32) -> Self {
        Self([
            (most >> 16) as u16,
            (most & 0xFFFF) as u16,
            (least >> 16) as u16,
            (least & 0xFFFF) as u16,
        ])
    }

    /// Four dot-separated numbers, as an msix manifest writes its version.
    #[must_use]
    pub fn parse_four(text: &str) -> Option<Self> {
        let mut parts = [0u16; 4];
        let mut fields = text.trim().split('.');
        for part in &mut parts {
            *part = fields.next()?.parse().ok()?;
        }
        fields.next().is_none().then_some(Self(parts))
    }
}

impl fmt::Display for FileVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d] = self.0;
        write!(f, "{a}.{b}.{c}.{d}")
    }
}

/// A PE machine type (`IMAGE_FILE_HEADER.Machine`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Machine(pub u16);

impl Machine {
    /// `IMAGE_FILE_MACHINE_AMD64`.
    pub const X64: Self = Self(0x8664);
    /// `IMAGE_FILE_MACHINE_ARM64`.
    pub const ARM64: Self = Self(0xAA64);
    /// `IMAGE_FILE_MACHINE_I386`.
    pub const X86: Self = Self(0x014C);

    /// The spelling an msix manifest's `ProcessorArchitecture` gives this
    /// machine, when it has one.
    #[must_use]
    pub const fn package_architecture(self) -> Option<&'static str> {
        match self.0 {
            0x8664 => Some("x64"),
            0xAA64 => Some("arm64"),
            0x014C => Some("x86"),
            _ => None,
        }
    }

    /// The machine type in the header of a PE image, from its first bytes
    /// (`e_lfanew` at `0x3C`, then `PE\0\0` and the file header).
    #[must_use]
    pub fn of_image_header(bytes: &[u8]) -> Option<Self> {
        if bytes.get(..2)? != b"MZ" {
            return None;
        }
        let at = u32::from_le_bytes(bytes.get(0x3C..0x40)?.try_into().ok()?) as usize;
        if bytes.get(at..at.checked_add(4)?)? != b"PE\0\0" {
            return None;
        }
        let machine = bytes.get(at + 4..at + 6)?;
        Some(Self(u16::from_le_bytes([machine[0], machine[1]])))
    }
}

impl fmt::Display for Machine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.package_architecture() {
            Some(name) => f.write_str(name),
            None => write!(f, "0x{:04X}", self.0),
        }
    }
}

/// **What a new file must be**: the running build's subject and identity OID,
/// the offer's version, and this machine's type. Made by [`running_identity`]
/// (or [`identity_of`]) from a signed file, never from constants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectation {
    /// The signer's subject, parsed: type upper-cased, value as issued, in the
    /// order Windows prints a subject (`CN` first).
    pub subject_dn: Vec<Rdn>,
    /// The leaf's full identity-validation OID ([`identity_oid`]).
    pub identity_oid: String,
    /// `VERSIONINFO`'s file version.
    pub version: FileVersion,
    /// The image's machine type.
    pub machine: Machine,
}

impl Expectation {
    /// The same identity, expecting the offer's version instead of this
    /// file's own.
    #[must_use]
    pub fn for_offer(self, version: FileVersion) -> Self {
        Self { version, ..self }
    }
}

/// **Which roots a chain may end in.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Windows' own trusted roots and revocation — the product's policy.
    System,
    /// **Tests only (E-13)**: a chain engine whose only trusted root is
    /// `root` (DER), held in a memory store, which never reaches the network;
    /// `revocation_lists` (DER CRLs) are the only revocation it knows. Nothing
    /// is installed anywhere.
    ExclusiveRoot {
        /// The one root the engine trusts.
        root: Vec<u8>,
        /// The certificate revocation lists the test world publishes.
        revocation_lists: Vec<Vec<u8>>,
    },
}

/// **A signature that passed steps 1 to 3**, with what the later steps read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    /// The signer's subject as Windows prints it.
    pub subject: String,
    /// Every extended key usage the signer's certificate carries.
    pub key_usages: Vec<String>,
    /// The RFC 3161 time, as `FILETIME` ticks (100 ns since 1601).
    pub stamped_at: u64,
}

/// **A file that passed every step** against an [`Expectation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    /// What the file is, read the way the expectation was.
    pub identity: Expectation,
    /// Its signature.
    pub signed: Signed,
}

/// **Why a file is refused**, one variant per step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// This build has no Windows arm.
    Unsupported,
    /// The file could not be read.
    Unreadable(String),
    /// Step 1: the file carries no signature.
    Unsigned,
    /// Step 1: Windows does not trust the signature; its code.
    Untrusted {
        /// The `HRESULT` Windows answered.
        code: i32,
    },
    /// Step 2: no RFC 3161 time stamp.
    NoTimestamp,
    /// Step 2: the time stamp does not verify, or its authority is not
    /// trusted for time stamping at the stamped time; Windows' code.
    TimestampInvalid {
        /// The `HRESULT` (or chain policy error) Windows answered.
        code: i32,
    },
    /// Step 2: the stamped time is outside the signer's certificate.
    TimestampOutsideValidity,
    /// Step 3: the signer's chain is revoked.
    Revoked,
    /// Step 3: revocation could not be established.
    RevocationUnknown,
    /// Step 4: the leaf carries no identity-validation OID, or more than one.
    NoIdentity {
        /// The Artifact Signing OIDs it does carry.
        found: Vec<String>,
    },
    /// Step 4: a different identity-validation OID.
    IdentityDiffers {
        /// The running build's.
        expected: String,
        /// The file's.
        found: String,
    },
    /// Step 5: a different subject.
    SubjectDiffers {
        /// The running build's, printed.
        expected: String,
        /// The file's, printed.
        found: String,
    },
    /// Step 6: no `VERSIONINFO`, or an msix manifest with no readable version.
    NoVersion,
    /// Step 6: a different version.
    VersionDiffers {
        /// The offer's.
        expected: FileVersion,
        /// The file's.
        found: FileVersion,
    },
    /// Step 6: a different machine type.
    MachineDiffers {
        /// The offer's.
        expected: String,
        /// The file's.
        found: String,
    },
    /// Step 7: the msix package's manifest cannot be read.
    ManifestUnreadable(String),
    /// Step 7: the manifest's `Publisher` is not its signer's subject.
    PublisherIsNotSigner {
        /// The manifest's `Identity/@Publisher`.
        publisher: String,
        /// The signer's subject.
        signer: String,
    },
}

impl Refusal {
    /// The refusal's name, as a diagnostic line prints it.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Unreadable(_) => "unreadable",
            Self::Unsigned => "unsigned",
            Self::Untrusted { .. } => "untrusted",
            Self::NoTimestamp => "no-timestamp",
            Self::TimestampInvalid { .. } => "timestamp-invalid",
            Self::TimestampOutsideValidity => "timestamp-outside-validity",
            Self::Revoked => "revoked",
            Self::RevocationUnknown => "revocation-unknown",
            Self::NoIdentity { .. } => "no-identity",
            Self::IdentityDiffers { .. } => "identity-differs",
            Self::SubjectDiffers { .. } => "subject-differs",
            Self::NoVersion => "no-version",
            Self::VersionDiffers { .. } => "version-differs",
            Self::MachineDiffers { .. } => "machine-differs",
            Self::ManifestUnreadable(_) => "manifest-unreadable",
            Self::PublisherIsNotSigner { .. } => "publisher-is-not-signer",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.name();
        match self {
            Self::Unreadable(why) | Self::ManifestUnreadable(why) => write!(f, "{name}: {why}"),
            Self::Untrusted { code } | Self::TimestampInvalid { code } => {
                write!(f, "{name}: 0x{:08X}", code.cast_unsigned())
            }
            Self::NoIdentity { found } => write!(f, "{name}: [{}]", found.join(", ")),
            Self::IdentityDiffers { expected, found }
            | Self::SubjectDiffers { expected, found }
            | Self::MachineDiffers { expected, found } => {
                write!(f, "{name}: expected {expected}, found {found}")
            }
            Self::VersionDiffers { expected, found } => {
                write!(f, "{name}: expected {expected}, found {found}")
            }
            Self::PublisherIsNotSigner { publisher, signer } => {
                write!(f, "{name}: publisher {publisher}, signer {signer}")
            }
            _ => f.write_str(name),
        }
    }
}

/// **The running build's own standing**, for the updater (owner ruling
/// 2026-09-25: signed *and* flagged).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Capability {
    /// Signed with an identity, and flagged: what the updater compares a new
    /// file against.
    Capable(Box<Expectation>),
    /// Built without the updater flag.
    NotFlagged,
    /// Flagged, and the running file carries no signature.
    Unsigned,
    /// Flagged and signed, and its signature or identity does not pass.
    Unidentified(Refusal),
}

// ── the decisions, pure ─────────────────────────────────────────────────────

/// **The leaf's identity-validation OID** out of its extended key usages: the
/// one under [`IDENTITY_PREFIX`] that is not [`PUBLIC_TRUST_MARKER`].
///
/// # Errors
///
/// [`Refusal::NoIdentity`] with the Artifact Signing OIDs found, when there is
/// none or more than one — an identity this cannot name is not one it can
/// compare.
pub fn identity_oid(key_usages: &[String]) -> Result<String, Refusal> {
    let artifact: Vec<String> = key_usages
        .iter()
        .filter(|oid| oid.starts_with(IDENTITY_PREFIX))
        .cloned()
        .collect();
    let mut identities = artifact.iter().filter(|oid| *oid != PUBLIC_TRUST_MARKER);
    match (identities.next(), identities.next()) {
        (Some(one), None) => Ok(one.clone()),
        _ => Err(Refusal::NoIdentity { found: artifact }),
    }
}

/// **Step 3's decision** over a chain's `TrustStatus.dwErrorStatus`: revoked,
/// unknown, or known good.
///
/// # Errors
///
/// [`Refusal::Revoked`] when any element is revoked; [`Refusal::RevocationUnknown`]
/// when revocation could not be established (unknown or offline).
pub const fn revocation_verdict(error_status: u32) -> Result<(), Refusal> {
    if error_status & CHAIN_IS_REVOKED != 0 {
        Err(Refusal::Revoked)
    } else if error_status & (CHAIN_REVOCATION_UNKNOWN | CHAIN_OFFLINE_REVOCATION) != 0 {
        Err(Refusal::RevocationUnknown)
    } else {
        Ok(())
    }
}

/// **Step 1's reading of `WinVerifyTrust`'s answer** under the system policy:
/// success, or the refusal it names.
///
/// # Errors
///
/// The step's refusal: no signature, a revoked or unknown revocation named as
/// step 3's, anything else untrusted.
pub const fn trust_verdict(code: i32) -> Result<(), Refusal> {
    match code {
        0 => Ok(()),
        TRUST_E_NOSIGNATURE => Err(Refusal::Unsigned),
        CERT_E_REVOKED | CRYPT_E_REVOKED => Err(Refusal::Revoked),
        CERT_E_REVOCATION_FAILURE | CRYPT_E_NO_REVOCATION_CHECK | CRYPT_E_REVOCATION_OFFLINE => {
            Err(Refusal::RevocationUnknown)
        }
        code => Err(Refusal::Untrusted { code }),
    }
}

/// **Step 2's time decision**: the stamped time inside the signer's
/// certificate, both ends included (all three as `FILETIME` ticks).
///
/// # Errors
///
/// [`Refusal::TimestampOutsideValidity`].
pub const fn timestamp_within(
    stamped_at: u64,
    not_before: u64,
    not_after: u64,
) -> Result<(), Refusal> {
    if not_before <= stamped_at && stamped_at <= not_after {
        Ok(())
    } else {
        Err(Refusal::TimestampOutsideValidity)
    }
}

/// **Steps 4 to 6**: the file's identity against the expectation, in order —
/// identity OID, subject, version, machine.
///
/// # Errors
///
/// The first difference, named.
pub fn compare(expected: &Expectation, found: &Expectation) -> Result<(), Refusal> {
    if found.identity_oid != expected.identity_oid {
        return Err(Refusal::IdentityDiffers {
            expected: expected.identity_oid.clone(),
            found: found.identity_oid.clone(),
        });
    }
    if found.subject_dn.is_empty() || found.subject_dn != expected.subject_dn {
        return Err(Refusal::SubjectDiffers {
            expected: crate::msix::describe_name(&expected.subject_dn),
            found: crate::msix::describe_name(&found.subject_dn),
        });
    }
    if found.version != expected.version {
        return Err(Refusal::VersionDiffers {
            expected: expected.version,
            found: found.version,
        });
    }
    if found.machine != expected.machine {
        return Err(Refusal::MachineDiffers {
            expected: expected.machine.to_string(),
            found: found.machine.to_string(),
        });
    }
    Ok(())
}

/// **Step 7's publisher decision**: the msix manifest's `Publisher` is its
/// own signer's subject ([`crate::msix::publisher_matches_subject`]).
///
/// # Errors
///
/// [`Refusal::PublisherIsNotSigner`].
pub fn publisher_verdict(publisher: &str, signer_subject: &str) -> Result<(), Refusal> {
    if crate::msix::publisher_matches_subject(publisher, signer_subject) {
        Ok(())
    } else {
        Err(Refusal::PublisherIsNotSigner {
            publisher: publisher.to_owned(),
            signer: signer_subject.to_owned(),
        })
    }
}

/// **The capability's decision**: the flag first (a build fact, no disk), then
/// the running file's own identity.
#[must_use]
pub fn capability(
    flagged: bool,
    identity: impl FnOnce() -> Result<Expectation, Refusal>,
) -> Capability {
    if !flagged {
        return Capability::NotFlagged;
    }
    match identity() {
        Ok(expectation) => Capability::Capable(Box::new(expectation)),
        Err(Refusal::Unsigned) => Capability::Unsigned,
        Err(refusal) => Capability::Unidentified(refusal),
    }
}

// ── the doors ───────────────────────────────────────────────────────────────

/// **The signature of a file**, steps 1 to 3, under `policy`.
///
/// # Errors
///
/// The step's [`Refusal`].
pub fn signature(path: &Path, policy: &Policy) -> Result<Signed, Refusal> {
    crate::file_reads::opaque(crate::file_reads::Lane::Update, || {
        arm::signature(path, policy)
    })
}

/// **What a signed `folio.exe` is** — steps 1 to 3, then its identity OID,
/// subject, `VERSIONINFO` and machine type, under `policy`.
///
/// # Errors
///
/// The step's [`Refusal`].
pub fn identity_of(path: &Path, policy: &Policy) -> Result<Expectation, Refusal> {
    crate::file_reads::opaque(crate::file_reads::Lane::Update, || {
        arm::identity_of(path, policy).map(|(identity, _)| identity)
    })
}

/// **The running `folio.exe`'s own identity**, under the system policy — the
/// [`Expectation`] a new file is held to (with [`Expectation::for_offer`]).
///
/// # Errors
///
/// The step's [`Refusal`]; [`Refusal::Unreadable`] when the running file cannot
/// be found.
pub fn running_identity() -> Result<Expectation, Refusal> {
    let path = std::env::current_exe().map_err(|error| Refusal::Unreadable(error.to_string()))?;
    identity_of(&path, &Policy::System)
}

/// **Is this downloaded `folio.exe` the same publisher's Folio, at the offer's
/// version**, under the system policy.
///
/// # Errors
///
/// The first step that refuses.
pub fn verify_release_file(path: &Path, expectation: &Expectation) -> Result<Verified, Refusal> {
    verify_release_file_under(path, expectation, &Policy::System)
}

/// [`verify_release_file`] under `policy`.
///
/// # Errors
///
/// The first step that refuses.
pub fn verify_release_file_under(
    path: &Path,
    expectation: &Expectation,
    policy: &Policy,
) -> Result<Verified, Refusal> {
    crate::file_reads::opaque(crate::file_reads::Lane::Update, || {
        let (identity, signed) = arm::identity_of(path, policy)?;
        compare(expectation, &identity)?;
        Ok(Verified { identity, signed })
    })
}

/// **Is this downloaded `folio.msix` the same publisher's package, at the
/// offer's version**, under the system policy: steps 1 to 5, then step 7.
///
/// # Errors
///
/// The first step that refuses.
pub fn verify_release_package(path: &Path, expectation: &Expectation) -> Result<Verified, Refusal> {
    verify_release_package_under(path, expectation, &Policy::System)
}

/// [`verify_release_package`] under `policy`.
///
/// # Errors
///
/// The first step that refuses.
pub fn verify_release_package_under(
    path: &Path,
    expectation: &Expectation,
    policy: &Policy,
) -> Result<Verified, Refusal> {
    crate::file_reads::opaque(crate::file_reads::Lane::Update, || {
        arm::verify_package(path, expectation, policy)
    })
}

/// **A Microsoft sidecar** (`conpty.dll`, `OpenConsole.exe`): steps 1 to 3,
/// which is what `package.ps1 -Sign` asks of them (`sign.ps1 -VerifyOnly`).
///
/// # Errors
///
/// The step's [`Refusal`].
pub fn verify_sidecar(path: &Path) -> Result<Signed, Refusal> {
    signature(path, &Policy::System)
}

/// **The `VERSIONINFO` file version of the program at `path`** (0.4.7 ticket
/// U-37): the version of a rescue copy, read by an update's trial before it
/// hands its transaction back — only a rescue build that knows `--from-trial`
/// is handed it. Read only, unsigned files included.
///
/// # Errors
///
/// [`Refusal::NoVersion`] when the file carries none; `Unsupported` off
/// Windows.
pub fn file_version(path: &Path) -> Result<FileVersion, Refusal> {
    crate::file_reads::opaque(crate::file_reads::Lane::Update, || arm::file_version(path))
}

/// **The running build's capability**: flagged (`bt-app`'s
/// `update::eligible()`, passed in) and signed with an identity.
#[must_use]
pub fn running_capability(flagged: bool) -> Capability {
    capability(flagged, running_identity)
}

/// [`running_capability`] of the file at `path`, under `policy`.
#[must_use]
pub fn capability_of(path: &Path, flagged: bool, policy: &Policy) -> Capability {
    capability(flagged, || identity_of(path, policy))
}

#[cfg(windows)]
#[path = "trust_windows.rs"]
pub(crate) mod arm;

#[cfg(not(windows))]
mod arm {
    use std::path::Path;

    use super::{Expectation, Policy, Refusal, Signed, Verified};

    pub(super) fn signature(_path: &Path, _policy: &Policy) -> Result<Signed, Refusal> {
        Err(Refusal::Unsupported)
    }

    pub(super) fn identity_of(
        _path: &Path,
        _policy: &Policy,
    ) -> Result<(Expectation, Signed), Refusal> {
        Err(Refusal::Unsupported)
    }

    pub(super) fn verify_package(
        _path: &Path,
        _expectation: &Expectation,
        _policy: &Policy,
    ) -> Result<Verified, Refusal> {
        Err(Refusal::Unsupported)
    }
    pub(super) fn file_version(_path: &Path) -> Result<super::FileVersion, Refusal> {
        Err(Refusal::Unsupported)
    }
}

#[cfg(test)]
#[path = "trust_tests.rs"]
mod tests;
