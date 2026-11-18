//! **What a release vouches for, signed with our own key** — the release statement the
//! updater trusts a release by (0.4.10 plan row 102, T-UPDATES-TRUST-OUR-OWN-KEY, rulings
//! R1–R12 of 2026-11-18).
//!
//! Until 0.4.10 the updater trusted a downloaded release by the Authenticode signature of its
//! files: the new `folio.exe`'s signer had to carry the running build's subject and
//! certificate-profile identity, with a revocation check that answered. The autumn of 2026
//! showed what that rests on: the 0.4.8 certificate went onto its issuer's revocation list, so
//! every 0.4.8 copy refuses itself, and the signing account was suspended, so no release could
//! be signed at all. A release is now vouched for by a **statement** signed with **our own
//! Ed25519 key**, whose public half every build embeds ([`RELEASE_KEYS`]); the Authenticode
//! signature stays what it is for Windows itself (SmartScreen, Smart App Control) and stops
//! being a condition of an update.
//!
//! The name is not "manifest": that word already means the F-4 manifest `folio.exe` carries of
//! its own archive ([`crate::release_manifest`]), which stays.
//!
//! # The two files, published beside the assets of every release
//!
//! [`STATEMENT_FILE`], `RELEASE-STATEMENT.txt`, and [`SIGNATURE_FILE`],
//! `RELEASE-STATEMENT.txt.sig`: exactly [`SIGNATURE_BYTES`] raw bytes, an Ed25519 signature
//! (RFC 8032, pure, no pre-hash) over the statement's bytes. Raw bytes so that anyone can check
//! a release by hand with stock OpenSSL and a published public key:
//!
//! ```text
//! openssl pkeyutl -verify -pubin -inkey <public.der> -keyform DER -rawin \
//!     -in RELEASE-STATEMENT.txt -sigfile RELEASE-STATEMENT.txt.sig
//! ```
//!
//! # The text form, v1
//!
//! The F-4 manifest's rules: ASCII, LF, one fact a line, in this order and no other, every list
//! counted, ending in a newline — so a statement cut anywhere is refused as truncated rather
//! than read as a smaller release, and a format this build does not know is refused by name.
//!
//! ```text
//! folio-release-statement 1
//! product folio
//! version 0.4.10
//! tag v0.4.10-preview
//! date 2026-11-25
//! key <the signing key's id: 16 lower-case hex digits>
//! min_from 0.4.6
//! authenticode false
//! retires 0
//! <a key id the statement retires>        …one line each
//! assets 2
//! <sha256> <size> windows-x64 folio-0.4.10-windows-x64-unsigned.zip
//! <sha256> <size> macos-arm64 Folio-0.4.10-macos-arm64.dmg
//! members windows-x64 10
//! <sha256> <size> folio.exe                …every member of that asset's archive
//! ```
//!
//! * `version` is three numbers, each written without a leading zero, and `tag` is
//!   `v<version>` or `v<version>-preview` — the release grammar the update job mints an offer
//!   with — so the two cannot name different releases. The tag grammar is narrower than what the
//!   update check reads (`-rc.1`, build metadata) on purpose: `v<version>` and
//!   `v<version>-preview` are the only tags Folio has ever had, and a release under any other
//!   word needs a new statement format, which an older build refuses by name.
//! * `key` is the signer's [`KeyId`]. It is inside the signed bytes, so it cannot be swapped:
//!   the verifier uses exactly that key and never tries each key in turn.
//! * `min_from` is the oldest version that may update to this release by this statement — the
//!   F-4 manifest's `min_updater`, copied by the release script so the fact keeps one owner. A
//!   compatibility field, not a defence: whoever holds the key writes it.
//! * `authenticode` says whether the Windows archive's `folio.exe` and `folio.msix` carry our
//!   signing service's signature. Information for the reader; no decision of the updater rests
//!   on the claim (R4).
//! * `date` is information too. Nothing decides on a date (`bt_app::update`'s rule).
//! * `retires`: key ids this statement retires (see [`verify_statement`]).
//! * `assets`: the release's files by platform, each with its SHA-256 and size. The updater
//!   takes the name of its platform's file from here rather than composing it from the tag.
//! * `members <platform>`: every member of that asset's archive, `folio.exe` and `folio.msix`
//!   included — the two files the F-4 manifest cannot list, the one that carries it among them.
//!
//! # The keys
//!
//! [`RELEASE_KEYS`] are two: the live key every release is signed with, and a spare kept off
//! every machine that signs, so that a leaked or lost live key can be retired without a flag
//! day: the next release is signed with the spare and retires the live key, every copy that
//! reads it refuses the live key from then on, and that release embeds a new pair.
//! **Retirement is one-way, by position in [`RELEASE_KEYS`]**: a key may retire only the keys
//! before it — the spare may retire the live key, and the live key retires nothing. Otherwise a
//! leaked live key, the one that sits on a disk and signs every release, could retire the
//! offline spare on every copy that read it, and the one road out of the leak would be closed.
//! The price: if the spare leaks, no statement can tell an old build to refuse it — but a leaked
//! spare was the worst case already, since it may retire the live key; old copies stay exposed
//! until they update to a build that embeds a new pair. The updater remembers what was retired
//! in the data folder (`bt_persist::ReleaseKeysV1`). The private halves never enter this
//! repository, the trace folder or CI; a release is signed with OpenSSL outside them.
//!
//! [`TEST_KEYS`] are a throwaway pair whose private halves are committed beside this crate's
//! fixtures, for the tests that must sign (a tampered statement, a retire). They protect
//! nothing; [`embeds_test_keys`] says whether a build embeds them, and the release check
//! refuses such a build.

use std::fmt;

use crate::digest::{hex, sha256};
use crate::release_manifest::{ManifestError, Member, is_word, member_line, number};

/// The statement's file name on a release page.
pub const STATEMENT_FILE: &str = "RELEASE-STATEMENT.txt";

/// The signature's file name on a release page.
pub const SIGNATURE_FILE: &str = "RELEASE-STATEMENT.txt.sig";

/// An Ed25519 signature's length; the signature file is exactly this.
pub const SIGNATURE_BYTES: usize = 64;

/// **The most bytes a statement may have.** A release lists a few assets and a few dozen
/// members, a few kilobytes; a reader stops reading here.
pub const STATEMENT_MAX_BYTES: usize = 64 * 1024;

/// The format this build writes and reads.
pub const FORMAT: u32 = 1;

/// The first word of the first line.
const MAGIC: &str = "folio-release-statement";

/// The Windows archive's platform word.
pub const WINDOWS_X64: &str = "windows-x64";

/// The macOS disk image's platform word.
pub const MACOS_ARM64: &str = "macos-arm64";

// ── the keys ────────────────────────────────────────────────────────────────

/// **A key's id**: the first eight bytes of the SHA-256 of its 32-byte public key, written as
/// sixteen lower-case hex digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId([u8; 8]);

impl KeyId {
    /// The id of the public key `public`.
    #[must_use]
    pub fn of_public(public: &[u8; 32]) -> Self {
        let digest = sha256(public);
        let mut id = [0u8; 8];
        id.copy_from_slice(&digest[..8]);
        Self(id)
    }

    /// The id `text` spells — sixteen lower-case hex digits — or `None`.
    #[must_use]
    pub fn parse_key_id(text: &str) -> Option<Self> {
        if text.len() != 16
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return None;
        }
        let mut id = [0u8; 8];
        for (at, byte) in id.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).ok()?;
        }
        Some(Self(id))
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex(&self.0))
    }
}

/// **One embedded public key**, and which slot it fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseKey {
    /// `live` or `spare`: for what a diagnostic says, never for a decision.
    pub slot: &'static str,
    /// The 32-byte Ed25519 public key.
    pub public: [u8; 32],
}

impl ReleaseKey {
    /// This key's id.
    #[must_use]
    pub fn key_id(&self) -> KeyId {
        KeyId::of_public(&self.public)
    }
}

/// **The keys every build trusts a release statement by**: the live key, then the spare.
///
/// Both are accepted unless remembered as retired. Generated 2026-11-18 with OpenSSL; the
/// private halves live outside every repository, synced folder and CI, encrypted, and are
/// asked for by OpenSSL's own prompt when a release is signed (rulings R2, R12). Changing
/// either is a flag day for every copy that has not read a statement retiring it.
pub const RELEASE_KEYS: [ReleaseKey; 2] = [
    ReleaseKey {
        slot: "live",
        public: [
            0x47, 0x9a, 0xc1, 0x67, 0x57, 0x98, 0xe5, 0x45, 0x83, 0xa1, 0x16, 0x3e, 0x61, 0x0c,
            0x86, 0x54, 0x3b, 0x4d, 0x83, 0x6b, 0xf0, 0x9f, 0x40, 0x7a, 0x52, 0xfa, 0x36, 0x2f,
            0x78, 0x3d, 0x58, 0x9a,
        ],
    },
    ReleaseKey {
        slot: "spare",
        public: [
            0x57, 0x81, 0xdb, 0xbf, 0x90, 0x1c, 0xb0, 0x53, 0x45, 0xe9, 0x80, 0x00, 0xe0, 0xb5,
            0x9b, 0xea, 0xd9, 0x6d, 0x1a, 0x24, 0x55, 0xb9, 0x29, 0x73, 0xae, 0xfa, 0x25, 0x02,
            0xb7, 0x62, 0xdf, 0x59,
        ],
    },
];

/// **The throwaway test pair** (`fixtures/release-key/TEST-ONLY-*.pem`, made with OpenSSL on
/// 2026-11-18, private halves committed because they protect nothing): what the tests that
/// must sign a statement sign with. No release is ever signed with them, and
/// [`embeds_test_keys`] is how a build that embedded them by mistake is found out.
pub const TEST_KEYS: [ReleaseKey; 2] = [
    ReleaseKey {
        slot: "live",
        public: [
            0xbe, 0xb9, 0xb6, 0x50, 0x59, 0x64, 0xbc, 0xa2, 0x42, 0x6d, 0x70, 0xc2, 0xb7, 0x06,
            0xce, 0x0f, 0xfa, 0x57, 0xcb, 0x1a, 0x1b, 0x90, 0xd9, 0x58, 0x70, 0x7d, 0xc4, 0xe3,
            0x91, 0x58, 0x45, 0x45,
        ],
    },
    ReleaseKey {
        slot: "spare",
        public: [
            0x6b, 0xcc, 0x91, 0x89, 0x11, 0x28, 0xbf, 0x2c, 0xcb, 0xc4, 0x1e, 0x08, 0xe3, 0x6f,
            0xb1, 0x9e, 0x7e, 0x88, 0x56, 0xe5, 0x1c, 0x6c, 0x82, 0x71, 0x46, 0x16, 0x9f, 0x26,
            0xa1, 0xb2, 0x46, 0x2f,
        ],
    },
];

/// **Whether this build embeds a test key among [`RELEASE_KEYS`]** — a build no release may be
/// made of, since the test keys' private halves are public.
#[must_use]
pub fn embeds_test_keys() -> bool {
    KeySet::EMBEDDED.holds_a_test_key()
}

/// **The keys a statement is verified against**: [`Self::EMBEDDED`] in the product and the
/// release check; [`TEST_KEYS`] in the tests that sign.
#[derive(Clone, Copy, Debug)]
pub struct KeySet<'k> {
    keys: &'k [ReleaseKey],
}

impl<'k> KeySet<'k> {
    /// The keys this build embeds.
    pub const EMBEDDED: KeySet<'static> = KeySet {
        keys: &RELEASE_KEYS,
    };

    /// A set of `keys`.
    #[must_use]
    pub const fn of(keys: &'k [ReleaseKey]) -> Self {
        Self { keys }
    }

    /// The key whose id is `id`.
    #[must_use]
    pub fn key_named(&self, id: KeyId) -> Option<&'k ReleaseKey> {
        self.keys.iter().find(|key| key.key_id() == id)
    }

    /// Where the key whose id is `id` stands in the set: the live key first.
    #[cfg(feature = "release-key")]
    fn place_in_set(&self, id: KeyId) -> Option<usize> {
        self.keys.iter().position(|key| key.key_id() == id)
    }

    /// Whether the set holds one of [`TEST_KEYS`].
    #[must_use]
    pub fn holds_a_test_key(&self) -> bool {
        self.keys
            .iter()
            .any(|key| TEST_KEYS.iter().any(|test| test.public == key.public))
    }
}

// ── the statement ───────────────────────────────────────────────────────────

/// One release file, as the statement lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatementAsset {
    /// Its SHA-256, 64 lower-case hex digits.
    pub sha256: String,
    /// Its length in bytes.
    pub size: u64,
    /// The platform it is for ([`WINDOWS_X64`], [`MACOS_ARM64`]).
    pub platform: String,
    /// Its file name on the release page.
    pub name: String,
}

/// **The statement v1** (see the module for each field).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    /// Always `folio`.
    pub product: String,
    /// Three numbers.
    pub version: String,
    /// `v<version>` or `v<version>-preview`.
    pub tag: String,
    /// `YYYY-MM-DD`, information only.
    pub date: String,
    /// The signing key's id.
    pub key: KeyId,
    /// The oldest version that may update to this release by this statement.
    pub min_from: String,
    /// Whether the Windows archive's two files of ours carry our service's signature.
    pub authenticode: bool,
    /// The key ids this statement retires.
    pub retires: Vec<KeyId>,
    /// The release's files.
    pub assets: Vec<StatementAsset>,
    /// The platform whose asset is an archive with [`Self::members`].
    pub members_of: String,
    /// Every member of that archive, in its order.
    pub members: Vec<Member>,
}

/// **Why a statement, its signature or what it vouches for is refused**, one variant per
/// check, each named for a diagnostic line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatementError {
    /// The first line names a format this build does not read.
    UnknownFormat(String),
    /// The text ends before the statement does.
    Truncated(String),
    /// Anything else that is not the grammar.
    Malformed(String),
    /// More than [`STATEMENT_MAX_BYTES`].
    TooLarge {
        /// The length found.
        bytes: usize,
    },
    /// The signature file is not [`SIGNATURE_BYTES`] long.
    SignatureLength {
        /// The length found.
        found: usize,
    },
    /// The statement names a key this build does not embed.
    UnknownKey(KeyId),
    /// The statement is signed by a key a statement read earlier retired.
    RetiredKey(KeyId),
    /// The signature does not verify with the key the statement names.
    BadSignature(KeyId),
    /// The statement retires a key this build does not embed.
    RetiresUnknownKey(KeyId),
    /// The statement retires a key its signer may not retire: one at or after the signer's
    /// place in the key set (the live key retires nothing).
    RetireNotAllowed(KeyId),
    /// The keys a release is checked with include a test key, whose private half is public.
    TestKey(KeyId),
    /// The statement is for another tag than the one asked for.
    OtherTag {
        /// The tag asked for.
        expected: String,
        /// The statement's.
        found: String,
    },
    /// The statement is for another version than the one asked for.
    OtherVersion {
        /// The version asked for.
        expected: String,
        /// The statement's.
        found: String,
    },
    /// The release needs a newer build to update to it than this one.
    TooOld {
        /// The statement's `min_from`.
        needs: String,
        /// This build's version.
        this: String,
    },
    /// This build's version is not three numbers.
    RunningVersionUnreadable(String),
    /// The statement lists no file for this platform.
    NoAsset(String),
    /// A file is not the one the statement lists.
    AssetDiffers(String),
    /// An archive's members are not the ones the statement lists.
    MembersDiffer(String),
    /// A file could not be read.
    Unreadable(String),
    /// An archive could not be read as one.
    Archive(String),
}

impl StatementError {
    /// The refusal's name, as a diagnostic line prints it.
    #[must_use]
    pub const fn refusal_name(&self) -> &'static str {
        match self {
            Self::UnknownFormat(_) => "unknown-format",
            Self::Truncated(_) => "truncated",
            Self::Malformed(_) => "malformed",
            Self::TooLarge { .. } => "too-large",
            Self::SignatureLength { .. } => "signature-length",
            Self::UnknownKey(_) => "unknown-key",
            Self::RetiredKey(_) => "retired-key",
            Self::BadSignature(_) => "bad-signature",
            Self::RetiresUnknownKey(_) => "retires-unknown-key",
            Self::RetireNotAllowed(_) => "retire-not-allowed",
            Self::TestKey(_) => "test-key",
            Self::OtherTag { .. } => "other-tag",
            Self::OtherVersion { .. } => "other-version",
            Self::TooOld { .. } => "too-old",
            Self::RunningVersionUnreadable(_) => "running-version-unreadable",
            Self::NoAsset(_) => "no-asset",
            Self::AssetDiffers(_) => "asset-differs",
            Self::MembersDiffer(_) => "members-differ",
            Self::Unreadable(_) => "unreadable",
            Self::Archive(_) => "archive",
        }
    }
}

impl fmt::Display for StatementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.refusal_name();
        match self {
            Self::UnknownFormat(detail)
            | Self::Truncated(detail)
            | Self::Malformed(detail)
            | Self::RunningVersionUnreadable(detail)
            | Self::NoAsset(detail)
            | Self::AssetDiffers(detail)
            | Self::MembersDiffer(detail)
            | Self::Unreadable(detail)
            | Self::Archive(detail) => write!(formatter, "{name}: {detail}"),
            Self::TooLarge { bytes } => write!(
                formatter,
                "{name}: {bytes} bytes, above {STATEMENT_MAX_BYTES}"
            ),
            Self::SignatureLength { found } => {
                write!(formatter, "{name}: {found} bytes, not {SIGNATURE_BYTES}")
            }
            Self::UnknownKey(id)
            | Self::RetiredKey(id)
            | Self::BadSignature(id)
            | Self::RetiresUnknownKey(id)
            | Self::RetireNotAllowed(id)
            | Self::TestKey(id) => write!(formatter, "{name}: key {id}"),
            Self::OtherTag { expected, found } | Self::OtherVersion { expected, found } => {
                write!(formatter, "{name}: expected {expected}, found {found}")
            }
            Self::TooOld { needs, this } => {
                write!(formatter, "{name}: needs {needs} or later, this is {this}")
            }
        }
    }
}

impl std::error::Error for StatementError {}

impl From<ManifestError> for StatementError {
    fn from(error: ManifestError) -> Self {
        match error {
            ManifestError::UnknownFormat(detail) => Self::UnknownFormat(detail),
            ManifestError::Truncated(detail) => Self::Truncated(detail),
            ManifestError::Malformed(detail) => Self::Malformed(detail),
        }
    }
}

/// The header keys after the first line, in the order they are written.
const HEADER: [&str; 7] = [
    "product",
    "version",
    "tag",
    "date",
    "key",
    "min_from",
    "authenticode",
];

/// **Three numbers**, `major.minor.patch`, compared as three numbers; each is written without a
/// leading zero, so a version has one spelling (`0.4.10`, never `0.04.10`).
fn version_triple(text: &str) -> Option<[u64; 3]> {
    let mut parts = [0u64; 3];
    let mut fields = text.split('.');
    for part in &mut parts {
        let field = fields.next()?;
        if field.is_empty()
            || !field.bytes().all(|byte| byte.is_ascii_digit())
            || (field.len() > 1 && field.starts_with('0'))
        {
            return None;
        }
        *part = field.parse().ok()?;
    }
    fields.next().is_none().then_some(parts)
}

/// Whether `tag` is `v<version>` or `v<version>-preview` — the only two tag forms Folio has ever
/// released under (see the module).
fn tag_names_version(tag: &str, version: &str) -> bool {
    tag.strip_prefix('v')
        .map(|rest| rest.strip_suffix("-preview").unwrap_or(rest))
        .is_some_and(|named| named == version)
}

/// `YYYY-MM-DD`, by shape.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(at, byte)| match at {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// A platform word: lower-case letters, digits and `-`.
fn is_platform(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The value of `key value`, which is one word.
fn header_value(line: &str, key: &str) -> Result<String, StatementError> {
    let value = line
        .strip_prefix(key)
        .and_then(|rest| rest.strip_prefix(' '))
        .ok_or_else(|| {
            StatementError::Malformed(format!("expected a `{key}` line, found `{line}`"))
        })?;
    if !is_word(value) {
        return Err(StatementError::Malformed(format!(
            "`{key}` is `{value}`, which is not one printable word"
        )));
    }
    Ok(value.to_owned())
}

/// The next line, or a truncation naming what was expected.
fn next_line<'t>(
    lines: &mut impl Iterator<Item = &'t str>,
    what: &str,
) -> Result<&'t str, StatementError> {
    lines
        .next()
        .ok_or_else(|| StatementError::Truncated(format!("it ends before its {what}")))
}

/// `<count>` lines of a list, each read by `read`.
fn counted<'t, T>(
    lines: &mut impl Iterator<Item = &'t str>,
    count: u64,
    what: &str,
    mut read: impl FnMut(&str) -> Result<T, StatementError>,
) -> Result<Vec<T>, StatementError> {
    let mut items = Vec::new();
    for at in 0..count {
        let line = lines.next().ok_or_else(|| {
            StatementError::Truncated(format!("it declares {count} {what} and holds {at}"))
        })?;
        items.push(read(line)?);
    }
    Ok(items)
}

/// `<sha256> <size> <platform> <name>`.
fn asset_line(line: &str) -> Result<StatementAsset, StatementError> {
    let Some((sha256, rest)) = line.split_once(' ') else {
        return Err(StatementError::Malformed(format!(
            "`{line}` is not `<sha256> <size> <platform> <name>`"
        )));
    };
    let Some((size, rest)) = rest.split_once(' ') else {
        return Err(StatementError::Malformed(format!(
            "`{line}` is not `<sha256> <size> <platform> <name>`"
        )));
    };
    let Some((platform, name)) = rest.split_once(' ') else {
        return Err(StatementError::Malformed(format!(
            "`{line}` is not `<sha256> <size> <platform> <name>`"
        )));
    };
    if !is_platform(platform) {
        return Err(StatementError::Malformed(format!(
            "`{platform}` is not a platform word"
        )));
    }
    let member = member_line(&format!("{sha256} {size} {name}"))?;
    Ok(StatementAsset {
        sha256: member.sha256,
        size: member.size,
        platform: platform.to_owned(),
        name: member.name,
    })
}

impl Statement {
    /// **The text form** (see the module).
    #[must_use]
    pub fn statement_text(&self) -> String {
        let mut out = format!("{MAGIC} {FORMAT}\n");
        let values = [
            self.product.clone(),
            self.version.clone(),
            self.tag.clone(),
            self.date.clone(),
            self.key.to_string(),
            self.min_from.clone(),
            self.authenticode.to_string(),
        ];
        for (key, value) in HEADER.iter().zip(values) {
            out.push_str(&format!("{key} {value}\n"));
        }
        out.push_str(&format!("retires {}\n", self.retires.len()));
        for id in &self.retires {
            out.push_str(&format!("{id}\n"));
        }
        out.push_str(&format!("assets {}\n", self.assets.len()));
        for asset in &self.assets {
            out.push_str(&format!(
                "{} {} {} {}\n",
                asset.sha256, asset.size, asset.platform, asset.name
            ));
        }
        out.push_str(&format!(
            "members {} {}\n",
            self.members_of,
            self.members.len()
        ));
        for member in &self.members {
            out.push_str(&format!(
                "{} {} {}\n",
                member.sha256, member.size, member.name
            ));
        }
        out
    }

    /// **The statement in `bytes`, or why not.** Strict: printable ASCII and LF only, no more
    /// than [`STATEMENT_MAX_BYTES`], the form [`Self::statement_text`] writes and nothing it
    /// does not.
    ///
    /// # Errors
    ///
    /// [`StatementError::TooLarge`]; [`StatementError::UnknownFormat`] for a first line naming
    /// another format; [`StatementError::Truncated`] for text that ends early; and
    /// [`StatementError::Malformed`] for everything else, a statement retiring its own signer
    /// and a `min_from` above its own version included.
    pub fn read_statement(bytes: &[u8]) -> Result<Self, StatementError> {
        if bytes.len() > STATEMENT_MAX_BYTES {
            return Err(StatementError::TooLarge { bytes: bytes.len() });
        }
        if let Some(at) = bytes
            .iter()
            .position(|byte| !(byte.is_ascii_graphic() || *byte == b' ' || *byte == b'\n'))
        {
            return Err(StatementError::Malformed(format!(
                "byte {at} is 0x{:02x}, and a statement is printable ASCII and LF",
                bytes[at]
            )));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|error| StatementError::Malformed(error.to_string()))?;
        let Some(body) = text.strip_suffix('\n') else {
            return Err(StatementError::Truncated(
                "the text does not end in a newline, so its last line may be cut".to_owned(),
            ));
        };
        let mut lines = body.split('\n');

        let first = next_line(&mut lines, "first line")?;
        let Some(format) = first
            .strip_prefix(MAGIC)
            .and_then(|rest| rest.strip_prefix(' '))
        else {
            return Err(StatementError::Malformed(format!(
                "the first line is `{first}`, not `{MAGIC} <format>`"
            )));
        };
        if format != FORMAT.to_string() {
            return Err(StatementError::UnknownFormat(format!(
                "`{format}`; this build reads format {FORMAT}"
            )));
        }

        let mut values = Vec::with_capacity(HEADER.len());
        for key in HEADER {
            let line = next_line(&mut lines, &format!("`{key}` line"))?;
            values.push(header_value(line, key)?);
        }
        let [product, version, tag, date, key, min_from, authenticode] =
            <[String; 7]>::try_from(values).expect("one value for each of the seven keys");
        if product != crate::release_manifest::PRODUCT {
            return Err(StatementError::Malformed(format!(
                "the product is `{product}`, not `{}`",
                crate::release_manifest::PRODUCT
            )));
        }
        let Some(own) = version_triple(&version) else {
            return Err(StatementError::Malformed(format!(
                "the version `{version}` is not three numbers"
            )));
        };
        if !tag_names_version(&tag, &version) {
            return Err(StatementError::Malformed(format!(
                "the tag `{tag}` does not name the version {version}"
            )));
        }
        if !is_date(&date) {
            return Err(StatementError::Malformed(format!(
                "the date `{date}` is not YYYY-MM-DD"
            )));
        }
        let key = KeyId::parse_key_id(&key).ok_or_else(|| {
            StatementError::Malformed(format!("the key `{key}` is not 16 lower-case hex digits"))
        })?;
        let Some(from) = version_triple(&min_from) else {
            return Err(StatementError::Malformed(format!(
                "min_from `{min_from}` is not three numbers"
            )));
        };
        if from > own {
            return Err(StatementError::Malformed(format!(
                "min_from {min_from} is above the statement's own version {version}"
            )));
        }
        let authenticode = match authenticode.as_str() {
            "true" => true,
            "false" => false,
            other => {
                return Err(StatementError::Malformed(format!(
                    "authenticode is `{other}`, not true or false"
                )));
            }
        };

        let count = number(
            &header_value(next_line(&mut lines, "`retires` line")?, "retires")?,
            "retires",
        )?;
        let retires = counted(&mut lines, count, "retired keys", |line| {
            KeyId::parse_key_id(line)
                .ok_or_else(|| StatementError::Malformed(format!("`{line}` is not a key id")))
        })?;
        for (at, id) in retires.iter().enumerate() {
            if *id == key {
                return Err(StatementError::Malformed(format!(
                    "the statement retires {id}, the key that signs it"
                )));
            }
            if retires[..at].contains(id) {
                return Err(StatementError::Malformed(format!("{id} is retired twice")));
            }
        }

        let count = number(
            &header_value(next_line(&mut lines, "`assets` line")?, "assets")?,
            "assets",
        )?;
        let assets = counted(&mut lines, count, "assets", asset_line)?;
        if assets.is_empty() {
            return Err(StatementError::Malformed(
                "the statement lists no asset".to_owned(),
            ));
        }
        for (at, asset) in assets.iter().enumerate() {
            if assets[..at].iter().any(|earlier| {
                earlier.platform == asset.platform || earlier.name.eq_ignore_ascii_case(&asset.name)
            }) {
                return Err(StatementError::Malformed(format!(
                    "{} {} is listed twice",
                    asset.platform, asset.name
                )));
            }
        }

        let line = next_line(&mut lines, "`members` line")?;
        let Some((members_of, count)) = line
            .strip_prefix("members ")
            .and_then(|rest| rest.split_once(' '))
        else {
            return Err(StatementError::Malformed(format!(
                "expected `members <platform> <count>`, found `{line}`"
            )));
        };
        if !assets.iter().any(|asset| asset.platform == members_of) {
            return Err(StatementError::Malformed(format!(
                "the members are of {members_of}, which no asset is for"
            )));
        }
        let count = number(count, "members")?;
        let members = counted(&mut lines, count, "members", |line| {
            member_line(line).map_err(StatementError::from)
        })?;
        for (at, member) in members.iter().enumerate() {
            if members[..at]
                .iter()
                .any(|earlier| earlier.name.eq_ignore_ascii_case(&member.name))
            {
                return Err(StatementError::Malformed(format!(
                    "{} is listed twice",
                    member.name
                )));
            }
        }
        if let Some(extra) = lines.next() {
            return Err(StatementError::Malformed(format!(
                "a line after the last member: `{extra}`"
            )));
        }

        Ok(Self {
            product,
            version,
            tag,
            date,
            key,
            min_from,
            authenticode,
            retires,
            assets,
            members_of: members_of.to_owned(),
            members,
        })
    }
}

/// **A statement whose signature verified** with a key of the set it was checked against, and
/// whose retires that set allows — made only by [`verify_statement`], and the one type that
/// decides anything about a release ([`Self::vouches_for`], [`Self::asset_for`],
/// [`Self::members_match`]). A [`Statement`] that was merely read decides nothing, so a caller
/// cannot hold a release to a statement it never verified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedStatement(Statement);

impl VerifiedStatement {
    /// What the statement says.
    #[must_use]
    pub const fn what_it_says(&self) -> &Statement {
        &self.0
    }

    /// **This statement vouches for the release `offered` names, and this build may update to
    /// it**: the tag and the version the offer's, then `min_from` no newer than this build.
    ///
    /// # Errors
    ///
    /// [`StatementError::OtherTag`], [`StatementError::OtherVersion`],
    /// [`StatementError::RunningVersionUnreadable`], [`StatementError::TooOld`], in that order.
    pub fn vouches_for(&self, offered: &OfferedRelease<'_>) -> Result<(), StatementError> {
        let statement = &self.0;
        if statement.tag != offered.tag {
            return Err(StatementError::OtherTag {
                expected: offered.tag.to_owned(),
                found: statement.tag.clone(),
            });
        }
        if statement.version != offered.version {
            return Err(StatementError::OtherVersion {
                expected: offered.version.to_owned(),
                found: statement.version.clone(),
            });
        }
        let this = version_triple(offered.running)
            .ok_or_else(|| StatementError::RunningVersionUnreadable(offered.running.to_owned()))?;
        let needs = version_triple(&statement.min_from).ok_or_else(|| {
            StatementError::Malformed(format!(
                "min_from `{}` is not three numbers",
                statement.min_from
            ))
        })?;
        if this < needs {
            return Err(StatementError::TooOld {
                needs: statement.min_from.clone(),
                this: offered.running.to_owned(),
            });
        }
        Ok(())
    }

    /// **The file this statement lists for `platform`.**
    ///
    /// # Errors
    ///
    /// [`StatementError::NoAsset`].
    pub fn asset_for(&self, platform: &str) -> Result<&StatementAsset, StatementError> {
        self.0
            .assets
            .iter()
            .find(|asset| asset.platform == platform)
            .ok_or_else(|| StatementError::NoAsset(platform.to_owned()))
    }

    /// **The members `found` of `platform`'s archive are exactly the statement's**: the
    /// statement lists the members of that platform's archive ([`Statement::members_of`]), no
    /// name is found twice (compared without case, as Windows stores names: an archive holding a
    /// name twice is read one way by this check and another by an extractor), and the names are
    /// the same (as written), each with the statement's SHA-256 and size. Order is not compared;
    /// the archive reader holds the order to the archive's own records.
    ///
    /// # Errors
    ///
    /// [`StatementError::MembersDiffer`], naming the platform, or the first member found twice,
    /// missing, unlisted or different.
    pub fn members_match(&self, platform: &str, found: &[Member]) -> Result<(), StatementError> {
        let statement = &self.0;
        if statement.members_of != platform {
            return Err(StatementError::MembersDiffer(format!(
                "the statement lists the members of {}, not of {platform}",
                statement.members_of
            )));
        }
        for (at, member) in found.iter().enumerate() {
            if found[..at]
                .iter()
                .any(|earlier| earlier.name.eq_ignore_ascii_case(&member.name))
            {
                return Err(StatementError::MembersDiffer(format!(
                    "{} is in the archive twice",
                    member.name
                )));
            }
        }
        for listed in &statement.members {
            match found.iter().find(|member| member.name == listed.name) {
                None => {
                    return Err(StatementError::MembersDiffer(format!(
                        "{} is listed and not in the archive",
                        listed.name
                    )));
                }
                Some(member) if member.sha256 != listed.sha256 || member.size != listed.size => {
                    return Err(StatementError::MembersDiffer(format!(
                        "{} is {} bytes with SHA-256 {}, and the statement lists {} bytes with {}",
                        member.name, member.size, member.sha256, listed.size, listed.sha256
                    )));
                }
                Some(_) => {}
            }
        }
        if let Some(extra) = found.iter().find(|member| {
            !statement
                .members
                .iter()
                .any(|listed| listed.name == member.name)
        }) {
            return Err(StatementError::MembersDiffer(format!(
                "{} is in the archive and not listed",
                extra.name
            )));
        }
        Ok(())
    }
}

/// **The file `asset` lists is the one whose bytes hash to `sha256` and number `size`.**
///
/// # Errors
///
/// [`StatementError::AssetDiffers`].
pub fn asset_matches(
    asset: &StatementAsset,
    sha256: &str,
    size: u64,
) -> Result<(), StatementError> {
    if asset.sha256 == sha256 && asset.size == size {
        Ok(())
    } else {
        Err(StatementError::AssetDiffers(format!(
            "{} is {size} bytes with SHA-256 {sha256}, and the statement lists {} bytes with {}",
            asset.name, asset.size, asset.sha256
        )))
    }
}

/// **The release an update was offered**, as the statement is held to it.
#[derive(Clone, Copy, Debug)]
pub struct OfferedRelease<'a> {
    /// The offer's tag, verbatim.
    pub tag: &'a str,
    /// The version the tag names.
    pub version: &'a str,
    /// This build's version, held to `min_from`.
    pub running: &'a str,
}

/// **The fixture statement for `key`**: a fixed release (`0.0.1`, two assets, one member) that
/// names `key` — what each slot's committed fixture holds, signed with that slot's key, so a
/// test can prove the embedded key verifies a statement made outside this crate.
#[must_use]
pub fn fixture_statement(key: &ReleaseKey) -> String {
    let digest_of = |text: &str| hex(&sha256(text.as_bytes()));
    Statement {
        product: crate::release_manifest::PRODUCT.to_owned(),
        version: "0.0.1".to_owned(),
        tag: "v0.0.1-preview".to_owned(),
        date: "2026-11-18".to_owned(),
        key: key.key_id(),
        min_from: "0.0.1".to_owned(),
        authenticode: false,
        retires: Vec::new(),
        assets: vec![
            StatementAsset {
                sha256: digest_of("the fixture's Windows archive"),
                size: 29,
                platform: WINDOWS_X64.to_owned(),
                name: "folio-0.0.1-windows-x64-unsigned.zip".to_owned(),
            },
            StatementAsset {
                sha256: digest_of("the fixture's macOS image"),
                size: 25,
                platform: MACOS_ARM64.to_owned(),
                name: "Folio-0.0.1-macos-arm64.dmg".to_owned(),
            },
        ],
        members_of: WINDOWS_X64.to_owned(),
        members: vec![Member {
            name: "folio.exe".to_owned(),
            sha256: digest_of("the fixture's folio.exe"),
            size: 23,
        }],
    }
    .statement_text()
}

// ── the signature ───────────────────────────────────────────────────────────

/// **The statement in `statement`, verified by `signature` with the key it names among
/// `keys`, and none of `retired`.**
///
/// In order: the statement is read ([`Statement::read_statement`]); the key it names must be
/// one of `keys` ([`StatementError::UnknownKey`]) and not remembered as retired
/// ([`StatementError::RetiredKey`]); the signature must be [`SIGNATURE_BYTES`] long and verify
/// over the statement's bytes with that key ([`StatementError::BadSignature`]); every key the
/// statement retires must be one of `keys` ([`StatementError::RetiresUnknownKey`]); and each must
/// stand before its signer in `keys` ([`StatementError::RetireNotAllowed`]) — retirement is
/// one-way: the spare may retire the live key, and the live key retires nothing (see the
/// module). A retire of the signer itself is the grammar's to refuse.
///
/// What the caller does with a verified statement's [`Statement::retires`] is to remember
/// them, so that a later statement signed by one of them is refused here.
///
/// # Errors
///
/// The first check that refuses.
#[cfg(feature = "release-key")]
pub fn verify_statement(
    statement: &[u8],
    signature: &[u8],
    keys: &KeySet<'_>,
    retired: &[KeyId],
) -> Result<VerifiedStatement, StatementError> {
    let read = Statement::read_statement(statement)?;
    let key = keys
        .key_named(read.key)
        .ok_or(StatementError::UnknownKey(read.key))?;
    if retired.contains(&read.key) {
        return Err(StatementError::RetiredKey(read.key));
    }
    if signature.len() != SIGNATURE_BYTES {
        return Err(StatementError::SignatureLength {
            found: signature.len(),
        });
    }
    let signature = ed25519_compact::Signature::from_slice(signature)
        .map_err(|_| StatementError::BadSignature(read.key))?;
    ed25519_compact::PublicKey::new(key.public)
        .verify(statement, &signature)
        .map_err(|_| StatementError::BadSignature(read.key))?;
    if let Some(unknown) = read
        .retires
        .iter()
        .find(|id| keys.key_named(**id).is_none())
    {
        return Err(StatementError::RetiresUnknownKey(*unknown));
    }
    let signer = keys
        .place_in_set(read.key)
        .ok_or(StatementError::UnknownKey(read.key))?;
    if let Some(later) = read
        .retires
        .iter()
        .find(|id| keys.place_in_set(**id).is_none_or(|at| at >= signer))
    {
        return Err(StatementError::RetireNotAllowed(*later));
    }
    Ok(VerifiedStatement(read))
}

// ── a published release, checked whole ──────────────────────────────────────

/// **Every member of the archive `archive`**, each under the one folder `root`, with its
/// SHA-256 and size: the root's own directory entry is allowed and not counted, and any other
/// entry outside it, or below a further folder, is refused — and so is a name the archive holds
/// twice (without case, as Windows stores names), whose bytes this read and an extractor would
/// take from different entries.
///
/// # Errors
///
/// [`StatementError::Archive`], naming the entry or the record.
#[cfg(feature = "inflate")]
pub fn archive_members(archive: &[u8], root: &str) -> Result<Vec<Member>, StatementError> {
    use crate::zip;

    let refused = |detail: String| StatementError::Archive(detail);
    let tail_start = archive
        .len()
        .saturating_sub(zip::END_RECORD_BYTES + zip::MAX_COMMENT_BYTES);
    let (_, end) = zip::find_end(&archive[tail_start..]).map_err(refused)?;
    let mut at = end.directory_offset as usize;
    let mut members = Vec::new();
    for _ in 0..end.entries {
        let (entry, next) = zip::central_entry(archive, at).map_err(refused)?;
        at = next;
        let name = String::from_utf8(entry.name.clone())
            .map_err(|_| refused("an entry's name is not UTF-8".to_owned()))?;
        if name == format!("{root}/") {
            continue;
        }
        let member = name
            .strip_prefix(root)
            .and_then(|rest| rest.strip_prefix('/'))
            .filter(|member| !member.is_empty() && !member.contains('/'))
            .ok_or_else(|| refused(format!("{name} is not a file directly under {root}/")))?;
        if members
            .iter()
            .any(|earlier: &Member| earlier.name.eq_ignore_ascii_case(member))
        {
            return Err(refused(format!("{member} is in the archive twice")));
        }
        let bytes = crate::pinned::entry(archive, &name).map_err(refused)?;
        members.push(Member {
            name: member.to_owned(),
            sha256: hex(&sha256(&bytes)),
            size: bytes.len() as u64,
        });
    }
    Ok(members)
}

/// **A release as published, checked whole against its statement** — what the release script
/// runs over the package directory before the upload, and the release workflow over the assets
/// it downloads from the published page (ruling R10): the statement and its signature verified
/// with `keys` ([`verify_statement`], nothing remembered as retired); its tag `tag`; every
/// asset it lists, read through `read` by its name, the size and SHA-256 it lists; and the
/// archive the members are of, every member exactly the statement's ([`archive_members`] under
/// `folio-<version>`, [`Statement::members_match`]).
///
/// # Errors
///
/// The first check that refuses; a file `read` cannot give is [`StatementError::Unreadable`].
#[cfg(all(feature = "release-key", feature = "inflate"))]
pub fn check_published(
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    tag: &str,
    keys: &KeySet<'_>,
) -> Result<VerifiedStatement, StatementError> {
    let statement_bytes = read(STATEMENT_FILE).map_err(StatementError::Unreadable)?;
    let signature = read(SIGNATURE_FILE).map_err(StatementError::Unreadable)?;
    let verified = verify_statement(&statement_bytes, &signature, keys, &[])?;
    let statement = verified.what_it_says();
    if statement.tag != tag {
        return Err(StatementError::OtherTag {
            expected: tag.to_owned(),
            found: statement.tag.clone(),
        });
    }
    for asset in &statement.assets {
        let bytes = read(&asset.name).map_err(StatementError::Unreadable)?;
        asset_matches(asset, &hex(&sha256(&bytes)), bytes.len() as u64)?;
        if asset.platform == statement.members_of {
            let root = crate::release_manifest::archive_root(&statement.version);
            verified.members_match(&asset.platform, &archive_members(&bytes, &root)?)?;
        }
    }
    Ok(verified)
}

/// **The release check as the release script and the release workflow run it**: a set of keys
/// holding one of [`TEST_KEYS`] — whose private halves are public — vouches for nothing
/// ([`StatementError::TestKey`]), whatever the release; otherwise [`check_published`].
///
/// # Errors
///
/// [`StatementError::TestKey`]; then as [`check_published`].
#[cfg(all(feature = "release-key", feature = "inflate"))]
pub fn release_check(
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    tag: &str,
    keys: &KeySet<'_>,
) -> Result<VerifiedStatement, StatementError> {
    if let Some(test) = keys
        .keys
        .iter()
        .find(|key| TEST_KEYS.iter().any(|test| test.public == key.public))
    {
        return Err(StatementError::TestKey(test.key_id()));
    }
    check_published(read, tag, keys)
}

#[cfg(test)]
#[path = "release_statement_tests.rs"]
mod tests;
