//! **What this release contains** — the manifest `folio.exe` carries of its own
//! archive, as a value with one text form (0.4.6 ticket U-9).
//!
//! `docs/plans/design/self-update-2026-09-16.md`, revision (b), F-4: the Windows
//! archive is ten files and only two of them are signed by us, `folio.exe` and
//! `folio.msix`. `SHA256SUMS.txt` is unsigned, so a changed `uninstall.cmd` or an
//! added DLL passed every check a release made. The fix adds no new signed
//! artifact: **`folio.exe` carries the manifest of every other member**, as an
//! `RCDATA` resource named [`RESOURCE_NAME`], and its own Authenticode signature
//! signs it. `crates/bt-app/build.rs` computes it at compile time and is the
//! fact's one owner (`docs/ARCHITECTURE.md` §4); `package.ps1` refuses to pack a
//! member whose bytes differ from it, `smoke.ps1` checks the packed archive
//! against it, and the updater's archive reader (U-14) reads it out of a new,
//! never-executed `folio.exe` before anything is moved.
//!
//! # Why it is in this crate
//!
//! Three readers need one spelling of it, and this is the one crate all three
//! can reach without compiling the window: `bt-app`'s build script (a build
//! dependency already, for the icon and `VERSIONINFO`), `render-info-plist`,
//! which writes [`PROTOCOL`] and [`MIN_UPDATER`] into the macOS bundle's
//! `Info.plist` — on macOS the bundle seal already covers every file, so those
//! two keys are what F-4 asks of it — and `bt-app`'s tests. A module in `bt-app`
//! shared by `#[path]`, the way `update_eligibility.rs` is, would have left the
//! plist renderer a second copy of the two numbers.
//!
//! # The text form, v1
//!
//! ASCII, LF, one fact a line, in this order and no other, ending in a newline:
//!
//! ```text
//! folio-release-manifest 1
//! product folio
//! version 0.4.6
//! arch x64
//! archive_root folio-0.4.6
//! protocol 1
//! min_updater 0.4.6
//! members 8
//! <sha256, 64 lower-case hex digits> <size in bytes> <name>
//! …one line per member, in the archive's order
//! ```
//!
//! A line format rather than JSON because its first reader is a PowerShell
//! script and the build that writes it must not grow a parser to do so, and
//! because every refusal it needs is a line count: `members` says how many lines
//! follow, and the final newline says the last one arrived whole, so a manifest
//! cut anywhere is refused as truncated rather than read as a smaller release.
//! The first line carries the format's own version; a manifest of a format this
//! build does not know is refused by name ([`ManifestError::UnknownFormat`]),
//! which is how a later format can exist at all.
//!
//! # The member list
//!
//! Which files the archive holds is written once, in [`MEMBER_LIST`]
//! (`scripts/release/archive-members.txt`): `package.ps1` builds the archive
//! from it and `build.rs` builds the manifest from it, so the two cannot list
//! different files. Each line is a name and where the file comes from
//! ([`Source`]); the manifest lists every member whose source is not
//! [`Source::Exe`] (the file that carries the manifest cannot list its own hash)
//! or [`Source::Msix`] (signed with the same identity, and authenticated by its
//! own signature and its manifest's `Version`, F-5).

use std::fmt;

/// The resource name `folio.exe` carries the manifest under, as `RT_RCDATA`.
pub const RESOURCE_NAME: &str = "FOLIO_RELEASE_MANIFEST";

/// The format this build writes and reads.
pub const FORMAT: u32 = 1;

/// The first word of the first line.
const MAGIC: &str = "folio-release-manifest";

/// The product every Folio manifest names.
pub const PRODUCT: &str = "folio";

/// **The update protocol this release speaks**: the version of the surfaces a
/// build other than the one that wrote them reads — the journal's header, the
/// trial's receipt, the three `--update-*` argv flags and this manifest
/// (revision (b), F-8, frozen at v1 from 0.4.6 on). A running build reads its
/// successor's before it quits and refuses one it does not speak.
pub const PROTOCOL: u32 = 1;

/// **The oldest build whose updater can install this release.** A release
/// whose layout an older reader cannot take raises it (F-12), and older
/// clients are sent to the releases page. 0.4.6 is the first build with an
/// updater, so nothing older can install anything.
pub const MIN_UPDATER: &str = "0.4.6";

/// The one list of the archive's members, relative to the workspace root.
pub const MEMBER_LIST: &str = "scripts/release/archive-members.txt";

/// Why a manifest or a member list was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The first line names a format this build does not read.
    UnknownFormat(String),
    /// The text ends before the manifest does: fewer member lines than it
    /// declares, or no final newline.
    Truncated(String),
    /// Anything else that is not the grammar.
    Malformed(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat(detail) => write!(formatter, "unknown manifest format: {detail}"),
            Self::Truncated(detail) => write!(formatter, "truncated manifest: {detail}"),
            Self::Malformed(detail) => write!(formatter, "malformed manifest: {detail}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// One member of the archive, as the manifest lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// The file name inside [`Manifest::archive_root`].
    pub name: String,
    /// Its SHA-256, 64 lower-case hex digits.
    pub sha256: String,
    /// Its length in bytes.
    pub size: u64,
}

/// **The manifest v1**: what the archive a `folio.exe` came in holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// Always [`PRODUCT`].
    pub product: String,
    /// The workspace version this build was compiled at.
    pub version: String,
    /// The archive's architecture as its file name spells it (`x64`).
    pub arch: String,
    /// The one folder at the archive's root ([`archive_root`]).
    pub archive_root: String,
    /// [`PROTOCOL`] of the build that wrote it.
    pub protocol: u32,
    /// [`MIN_UPDATER`] of the build that wrote it.
    pub min_updater: String,
    /// Every member but the two signed ones, in the archive's order.
    pub members: Vec<Member>,
}

/// The header keys after the first line, in the order they are written.
const KEYS: [&str; 6] = [
    "product",
    "version",
    "arch",
    "archive_root",
    "protocol",
    "min_updater",
];

impl Manifest {
    /// The text form (see the module).
    #[must_use]
    pub fn encode(&self) -> String {
        let mut out = format!("{MAGIC} {FORMAT}\n");
        let values = [
            self.product.clone(),
            self.version.clone(),
            self.arch.clone(),
            self.archive_root.clone(),
            self.protocol.to_string(),
            self.min_updater.clone(),
        ];
        for (key, value) in KEYS.iter().zip(values) {
            out.push_str(&format!("{key} {value}\n"));
        }
        out.push_str(&format!("members {}\n", self.members.len()));
        for member in &self.members {
            out.push_str(&format!(
                "{} {} {}\n",
                member.sha256, member.size, member.name
            ));
        }
        out
    }

    /// **The manifest in `text`, or why not.** Strict: the format is the one
    /// [`Manifest::encode`] writes, and nothing it does not write is read.
    ///
    /// # Errors
    ///
    /// [`ManifestError::UnknownFormat`] for a first line naming another format,
    /// [`ManifestError::Truncated`] for text that ends early, and
    /// [`ManifestError::Malformed`] for everything else.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let Some(body) = text.strip_suffix('\n') else {
            return Err(ManifestError::Truncated(
                "the text does not end in a newline, so its last line may be cut".to_owned(),
            ));
        };
        let mut lines = body.split('\n');

        let first = lines.next().unwrap_or_default();
        let Some(format) = first
            .strip_prefix(MAGIC)
            .and_then(|rest| rest.strip_prefix(' '))
        else {
            return Err(ManifestError::Malformed(format!(
                "the first line is `{first}`, not `{MAGIC} <format>`"
            )));
        };
        if format != FORMAT.to_string() {
            return Err(ManifestError::UnknownFormat(format!(
                "`{format}`; this build reads format {FORMAT}"
            )));
        }

        let mut values = Vec::with_capacity(KEYS.len());
        for key in KEYS {
            let line = lines.next().ok_or_else(|| {
                ManifestError::Truncated(format!("it ends before its `{key}` line"))
            })?;
            values.push(keyed(line, key)?);
        }
        let [product, version, arch, archive_root, protocol, min_updater] =
            <[String; 6]>::try_from(values).expect("one value for each of the six keys");
        let protocol = number(&protocol, "protocol")?;
        let protocol = u32::try_from(protocol)
            .map_err(|_| ManifestError::Malformed(format!("protocol {protocol} is too large")))?;

        let count_line = lines.next().ok_or_else(|| {
            ManifestError::Truncated("it ends before its `members` line".to_owned())
        })?;
        let count = number(&keyed(count_line, "members")?, "members")?;

        let mut members: Vec<Member> = Vec::new();
        for line in lines {
            let member = member_line(line)?;
            if members
                .iter()
                .any(|seen| seen.name.eq_ignore_ascii_case(&member.name))
            {
                return Err(ManifestError::Malformed(format!(
                    "{} is listed twice",
                    member.name
                )));
            }
            members.push(member);
        }
        let listed = members.len() as u64;
        if listed < count {
            return Err(ManifestError::Truncated(format!(
                "it declares {count} members and holds {listed}"
            )));
        }
        if listed > count {
            return Err(ManifestError::Malformed(format!(
                "it declares {count} members and holds {listed}"
            )));
        }

        Ok(Self {
            product,
            version,
            arch,
            archive_root,
            protocol,
            min_updater,
            members,
        })
    }
}

/// The value of `key value`, which is one word.
fn keyed(line: &str, key: &str) -> Result<String, ManifestError> {
    let value = line
        .strip_prefix(key)
        .and_then(|rest| rest.strip_prefix(' '))
        .ok_or_else(|| {
            ManifestError::Malformed(format!("expected a `{key}` line, found `{line}`"))
        })?;
    if !is_word(value) {
        return Err(ManifestError::Malformed(format!(
            "`{key}` is `{value}`, which is not one printable word"
        )));
    }
    Ok(value.to_owned())
}

/// A decimal number of digits and nothing else.
fn number(text: &str, what: &str) -> Result<u64, ManifestError> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ManifestError::Malformed(format!(
            "{what} is `{text}`, not a number"
        )));
    }
    text.parse()
        .map_err(|_| ManifestError::Malformed(format!("{what} {text} is too large")))
}

/// `<sha256> <size> <name>`.
fn member_line(line: &str) -> Result<Member, ManifestError> {
    let mut fields = line.splitn(3, ' ');
    let (Some(sha256), Some(size), Some(name)) = (fields.next(), fields.next(), fields.next())
    else {
        return Err(ManifestError::Malformed(format!(
            "`{line}` is not `<sha256> <size> <name>`"
        )));
    };
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ManifestError::Malformed(format!(
            "`{sha256}` is not 64 lower-case hex digits"
        )));
    }
    let size = number(size, "a member's size")?;
    check_member_name(name)?;
    Ok(Member {
        name: name.to_owned(),
        sha256: sha256.to_owned(),
        size,
    })
}

/// One printable ASCII word: no space, no control character, not empty.
fn is_word(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_graphic())
}

/// **A name a member may have**: one printable word that Windows stores as
/// written — no separator, no character a Windows file name cannot hold, and no
/// trailing dot, which Windows drops. The grammar the updater's reader enforces
/// on the archive (U-14, F-12) is wider than this; this is what a line of this
/// file can carry unambiguously.
fn check_member_name(name: &str) -> Result<(), ManifestError> {
    let forbidden = |byte: u8| b"/\\:*?\"<>|".contains(&byte);
    if !is_word(name) || name.bytes().any(forbidden) || name.ends_with('.') {
        return Err(ManifestError::Malformed(format!(
            "`{name}` is not a member name"
        )));
    }
    Ok(())
}

/// The folder every member sits in: `folio-<version>`.
#[must_use]
pub fn archive_root(version: &str) -> String {
    format!("{PRODUCT}-{version}")
}

/// The architecture as the archive's file name spells it (`x64`), from cargo's
/// `CARGO_CFG_TARGET_ARCH`; an architecture no archive has been made for keeps
/// cargo's word.
#[must_use]
pub fn archive_arch(target_arch: &str) -> &str {
    match target_arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// **The `links` metadata key a sidecar member is exported under** by
/// `bt-pty`'s build script, and read back by `bt-app`'s as
/// `DEP_CONPTY_<KEY>`: the member's name, lower-cased, with every character
/// that is not a letter or a digit written `_` (`conpty.dll` → `conpty_dll`).
#[must_use]
pub fn sidecar_key(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Where a member of the archive comes from, as [`MEMBER_LIST`] says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `folio.exe`: the build's, and the file that carries the manifest.
    Exe,
    /// `folio.msix`: packed by `package.ps1` out of `packaging/msix/`.
    Msix,
    /// The ConPTY sidecar: written by `bt-pty`'s build script beside the
    /// executable, and exported to `bt-app`'s through `links` metadata.
    Sidecar,
    /// A file under `packaging/`.
    Packaging,
    /// A file at the workspace root.
    Documents,
}

impl Source {
    const ALL: [Self; 5] = [
        Self::Exe,
        Self::Msix,
        Self::Sidecar,
        Self::Packaging,
        Self::Documents,
    ];

    /// The word [`MEMBER_LIST`] spells it with.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Exe => "exe",
            Self::Msix => "msix",
            Self::Sidecar => "sidecar",
            Self::Packaging => "packaging",
            Self::Documents => "documents",
        }
    }

    /// **Whether the manifest lists a member from here**: every source but the
    /// two files signed with Folio's own identity.
    #[must_use]
    pub fn in_manifest(self) -> bool {
        !matches!(self, Self::Exe | Self::Msix)
    }
}

/// One line of [`MEMBER_LIST`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// The member's name in the archive.
    pub name: String,
    /// Where it comes from.
    pub source: Source,
}

/// **The archive's members, in order, out of [`MEMBER_LIST`]'s text.**
///
/// A line is `<name> <source>` with any run of spaces between; a blank line or
/// one starting with `#` says nothing. Exactly one [`Source::Exe`] and one
/// [`Source::Msix`], and no name twice.
///
/// # Errors
///
/// [`ManifestError::Malformed`] naming the line that is not the grammar.
pub fn parse_member_list(text: &str) -> Result<Vec<Listed>, ManifestError> {
    let mut listed: Vec<Listed> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [name, word] = fields[..] else {
            return Err(ManifestError::Malformed(format!(
                "{MEMBER_LIST} line {}: `{line}` is not `<name> <source>`",
                index + 1
            )));
        };
        check_member_name(name)?;
        let source = Source::ALL
            .into_iter()
            .find(|source| source.word() == word)
            .ok_or_else(|| {
                ManifestError::Malformed(format!(
                    "{MEMBER_LIST} line {}: `{word}` is not a source",
                    index + 1
                ))
            })?;
        if listed
            .iter()
            .any(|seen| seen.name.eq_ignore_ascii_case(name))
        {
            return Err(ManifestError::Malformed(format!(
                "{MEMBER_LIST}: {name} is listed twice"
            )));
        }
        listed.push(Listed {
            name: name.to_owned(),
            source,
        });
    }
    for only in [Source::Exe, Source::Msix] {
        let count = listed.iter().filter(|item| item.source == only).count();
        if count != 1 {
            return Err(ManifestError::Malformed(format!(
                "{MEMBER_LIST} lists {count} members from `{}`; the archive has one",
                only.word()
            )));
        }
    }
    Ok(listed)
}

#[cfg(test)]
mod tests {
    use super::{
        FORMAT, Manifest, ManifestError, Member, Source, archive_arch, archive_root,
        parse_member_list, sidecar_key,
    };

    fn manifest() -> Manifest {
        Manifest {
            product: "folio".to_owned(),
            version: "0.4.6".to_owned(),
            arch: "x64".to_owned(),
            archive_root: archive_root("0.4.6"),
            protocol: 1,
            min_updater: "0.4.6".to_owned(),
            members: vec![
                Member {
                    name: "conpty.dll".to_owned(),
                    sha256: "e2fe87e2258c4e46ffc5157f727218cc25f34a174902f72eb8a5b49edd9a6458"
                        .to_owned(),
                    size: 109_920,
                },
                Member {
                    name: "uninstall.cmd".to_owned(),
                    sha256: "0".repeat(64),
                    size: 0,
                },
            ],
        }
    }

    /// RED (U-9) — **a manifest survives its own text form, field for field.**
    ///
    /// `build.rs` writes the text, and `package.ps1`, `smoke.ps1` and the
    /// updater's reader parse it; what the writer puts in must be what every
    /// reader gets out, including a member of size zero, a release with no
    /// members, and the exact spelling of every header value.
    ///
    /// MUTATION: write `members` as the count minus one in `encode` and the
    /// parse is refused.
    #[test]
    fn a_manifest_survives_encode_and_parse() {
        let manifest = manifest();
        let text = manifest.encode();
        assert!(
            text.starts_with(&format!("folio-release-manifest {FORMAT}\n")),
            "{text}"
        );
        assert!(text.ends_with(" 0 uninstall.cmd\n"), "{text}");
        assert_eq!(Manifest::parse(&text), Ok(manifest));

        let empty = Manifest {
            members: Vec::new(),
            ..self::manifest()
        };
        assert_eq!(Manifest::parse(&empty.encode()), Ok(empty));
    }

    /// RED (U-9) — **a manifest of a format this build does not know is
    /// refused by name**, not read as if it were v1.
    ///
    /// The format number is the only way a later release can change the
    /// layout: a v1 reader that took a v2 manifest line by line would check a
    /// release against a list it misread.
    ///
    /// MUTATION: skip the format comparison in `parse` and the `2` case parses.
    #[test]
    fn a_manifest_of_an_unknown_format_is_refused() {
        let text = manifest().encode();
        for other in ["2", "0", "01", "1.0", ""] {
            let changed = text.replacen(
                "folio-release-manifest 1\n",
                &format!("folio-release-manifest {other}\n"),
                1,
            );
            let result = Manifest::parse(&changed);
            assert!(
                matches!(result, Err(ManifestError::UnknownFormat(_))),
                "format `{other}`: {result:?}"
            );
        }
        assert!(matches!(
            Manifest::parse(&text.replacen("folio-release-manifest", "folio-manifest", 1)),
            Err(ManifestError::Malformed(_))
        ));
    }

    /// RED (U-9) — **a manifest cut anywhere is refused, and a cut at a line
    /// boundary is refused as truncated** — never read as the manifest of a
    /// smaller release.
    ///
    /// A resource read short, or a text cut between two lines, would otherwise
    /// be a well-formed list with members missing, and a member missing from
    /// the manifest is a file in the archive that no check covers.
    ///
    /// MUTATION: drop the `listed < count` refusal and every cut between two
    /// member lines parses.
    #[test]
    fn a_truncated_manifest_is_refused() {
        let text = manifest().encode();
        for cut in 0..text.len() {
            let result = Manifest::parse(&text[..cut]);
            assert!(result.is_err(), "cut at {cut} parsed: {:?}", &text[..cut]);
            if cut > 0 && text[..cut].ends_with('\n') {
                assert!(
                    matches!(result, Err(ManifestError::Truncated(_))),
                    "a cut at a line boundary ({cut}) is truncation: {result:?}"
                );
            }
        }
        assert!(matches!(
            Manifest::parse(&format!("{text}{} 1 extra.dll\n", "0".repeat(64))),
            Err(ManifestError::Malformed(_))
        ));
    }

    /// PIN (U-9) — **every line the grammar does not write is refused.**
    #[test]
    fn a_manifest_off_the_grammar_is_refused() {
        let text = manifest().encode();
        for (from, to) in [
            ("product folio\n", "product  folio\n"),
            ("version 0.4.6\n", "version 0.4.6 \n"),
            ("arch x64\n", "architecture x64\n"),
            ("protocol 1\n", "protocol one\n"),
            ("protocol 1\n", "protocol -1\n"),
            ("min_updater 0.4.6\n", ""),
            ("members 2\n", "members +2\n"),
            (" 109920 ", " 109920x "),
            ("e2fe87e2", "E2FE87E2"),
            ("uninstall.cmd\n", "uninstall.cmd.\n"),
            ("uninstall.cmd\n", "sub/uninstall.cmd\n"),
            ("uninstall.cmd\n", "CONPTY.DLL\n"),
            ("\n", "\r\n"),
        ] {
            let changed = text.replacen(from, to, 1);
            assert_ne!(changed, text, "{from:?} is in the text");
            assert!(
                Manifest::parse(&changed).is_err(),
                "{from:?} → {to:?} parsed: {changed}"
            );
        }
    }

    /// PIN (U-9) — **the member list says where each file comes from, and has
    /// exactly one executable and one package.**
    #[test]
    fn the_member_list_says_where_each_member_comes_from() {
        let listed = parse_member_list(
            "# the archive\n\nfolio.exe   exe\nfolio.msix msix\nconpty.dll sidecar\n\
             uninstall.cmd packaging\nLICENSE-MIT documents\n",
        )
        .expect("a list of the grammar");
        let names: Vec<(&str, bool)> = listed
            .iter()
            .map(|item| (item.name.as_str(), item.source.in_manifest()))
            .collect();
        assert_eq!(
            names,
            [
                ("folio.exe", false),
                ("folio.msix", false),
                ("conpty.dll", true),
                ("uninstall.cmd", true),
                ("LICENSE-MIT", true),
            ]
        );
        assert_eq!(listed[2].source, Source::Sidecar);

        for refused in [
            "folio.msix msix\n",
            "folio.exe exe\nfolio.msix msix\nother.exe exe\n",
            "folio.exe exe\nfolio.msix msix\nx.dll somewhere\n",
            "folio.exe exe\nfolio.msix msix\nx.dll\n",
            "folio.exe exe\nfolio.msix msix\nFOLIO.EXE documents\n",
            "folio.exe exe\nfolio.msix msix\na\\b documents\n",
        ] {
            assert!(parse_member_list(refused).is_err(), "{refused}");
        }
    }

    /// PIN (U-9) — **the names the build scripts agree on**: the sidecar's
    /// metadata key, the archive's root folder and its architecture word.
    #[test]
    fn the_names_the_build_scripts_derive() {
        assert_eq!(sidecar_key("conpty.dll"), "conpty_dll");
        assert_eq!(sidecar_key("OpenConsole.exe"), "openconsole_exe");
        assert_eq!(archive_root("0.4.6"), "folio-0.4.6");
        assert_eq!(archive_arch("x86_64"), "x64");
        assert_eq!(archive_arch("aarch64"), "arm64");
        assert_eq!(archive_arch("riscv64"), "riscv64");
    }
}
