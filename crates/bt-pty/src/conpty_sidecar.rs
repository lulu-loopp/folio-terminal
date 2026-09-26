//! **The ConPTY sidecar, unpacked from its vendored NuGet package by Rust alone.**
//!
//! **This is build-script code.** `build.rs` reaches it by `#[path]`; the library compiles it only
//! under `cfg(test)`, which is how a build script's checks get tests at all, and ships none of it.
//! Everything here is a pure function of bytes: reading the package and writing the files are
//! `build.rs`'s business.
//!
//! It used to be a Windows PowerShell 5.1 script, which tied the build to a Windows *host* even
//! though the sidecar is a property of the Windows *target*: a cross build of
//! `x86_64-pc-windows-msvc` from Linux or macOS panicked asking for `SystemRoot`
//! (`docs/DESIGN.md`, 2026-09-26). The pins, the two entries and the files they land as are the
//! script's, unchanged; `vendor/conpty/README.md` is where they are published.
//!
//! The archive reader is the stored-and-deflated subset of the zip format that the package uses:
//! the end-of-central-directory record, the central directory, and each local header, with the
//! inflating done by `miniz_oxide`, which is already in `Cargo.lock` through `flate2` and `png`, so
//! reading the package costs no new package. SHA-256 is `bt_winres::digest`, the workspace's one
//! written-out copy of FIPS 180-4, which `bt-app`'s build script hashes the release archive's
//! members with too; the standard's own examples hold it there.

use std::path::{Path, PathBuf};

pub use bt_winres::digest::{hex, sha256};
use bt_winres::release_manifest::sidecar_key;
use bt_winres::zip;

/// The vendored package, by file name. `build.rs` joins it to `WORKSPACE/vendor/conpty`.
pub const PACKAGE: &str = "Microsoft.Windows.Console.ConPTY.1.25.260710002-preview.nupkg";

/// The SHA-256 of [`PACKAGE`], pinned to the official release asset.
pub const PACKAGE_SHA256: &str = "05fe9b571ea4fb198f5012405cb39a132cf23eee50feaa496524c149b2502692";

/// One file the sidecar is made of: where it sits in the package, the hash it must have, and the
/// paths, relative to each destination directory, that it is written to. The first of those is
/// the one beside the executable, which is the file the release archive carries under that name.
pub struct SidecarFile {
    pub entry: &'static str,
    pub sha256: &'static str,
    pub targets: &'static [&'static str],
}

/// The two x64 files, in the order the retired script wrote them.
///
/// `OpenConsole.exe` is written twice: beside the DLL, where the application directory is, and
/// below `x64/`, which is the architecture-host layout the package's native `.targets` expects.
pub const SIDECAR: [SidecarFile; 2] = [
    SidecarFile {
        entry: "runtimes/win-x64/native/conpty.dll",
        sha256: "e2fe87e2258c4e46ffc5157f727218cc25f34a174902f72eb8a5b49edd9a6458",
        targets: &["conpty.dll"],
    },
    SidecarFile {
        entry: "build/native/runtimes/x64/OpenConsole.exe",
        sha256: "2525c351aa136d555e5df9a3c9d6ce9be43f785e37e3c993b8f23b3f0a53c7fa",
        targets: &["OpenConsole.exe", "x64/OpenConsole.exe"],
    },
];

/// **What `build.rs` tells the build scripts of the packages that depend on this one** (the
/// `links = "conpty"` metadata, 0.4.6 ticket U-9): for each sidecar file, the key
/// [`sidecar_key`] makes of the name it has beside the executable, and where in `profile_dir` it
/// was written under that name. `bt-app`'s build script reads each back as `DEP_CONPTY_<KEY>` and
/// hashes the file into the release manifest `folio.exe` carries.
pub fn exported(profile_dir: &Path) -> Vec<(String, PathBuf)> {
    SIDECAR
        .iter()
        .map(|file| {
            let beside_the_executable = file.targets[0];
            (
                sidecar_key(beside_the_executable),
                profile_dir.join(beside_the_executable),
            )
        })
        .collect()
}

/// **Refuses `bytes` unless their SHA-256 is `pinned`, and the refusal names `name`.**
///
/// `pinned` is lower-case hex, the way every pin in this repository is written.
pub fn verify_pinned(name: &str, bytes: &[u8], pinned: &str) -> Result<(), String> {
    let actual = hex(&sha256(bytes));
    if actual == pinned {
        Ok(())
    } else {
        Err(format!(
            "{name}: SHA-256 is {actual}, but the pinned official asset's is {pinned}"
        ))
    }
}

/// **The verified sidecar, out of the verified package**: each of [`SIDECAR`] with its bytes.
///
/// The package is checked against [`PACKAGE_SHA256`] before a byte of it is parsed, and each entry
/// against its own pin once it is inflated. Either refusal names the file it is about.
pub fn unpack(package: &[u8]) -> Result<Vec<(&'static SidecarFile, Vec<u8>)>, String> {
    verify_pinned(PACKAGE, package, PACKAGE_SHA256)?;
    SIDECAR
        .iter()
        .map(|file| {
            let bytes = zip_entry(package, file.entry).map_err(|e| format!("{PACKAGE}: {e}"))?;
            verify_pinned(file.entry, &bytes, file.sha256)?;
            Ok((file, bytes))
        })
        .collect()
}

/// **The contents of the entry called `name` in the zip archive `archive`.**
///
/// Stored and deflated entries are read. An encrypted entry, another compression method, or a
/// ZIP64 archive (whose sizes do not fit the fields read here) is an error that says which. The
/// records are read through `bt_winres::zip`, the workspace's one copy of the format's layout,
/// which the updater's archive reader reads a release archive with too (0.4.6 ticket U-14).
pub fn zip_entry(archive: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let tail_start = archive
        .len()
        .saturating_sub(zip::END_RECORD_BYTES + zip::MAX_COMMENT_BYTES);
    let (_, end) = zip::find_end(&archive[tail_start..])?;
    if end.entries == u16::MAX || end.directory_size == u32::MAX || end.directory_offset == u32::MAX
    {
        return Err("a ZIP64 archive, which this reader does not read".to_owned());
    }

    let mut at = end.directory_offset as usize;
    for _ in 0..end.entries {
        let (entry, next) = zip::central_entry(archive, at)?;
        at = next;
        if entry.name != name.as_bytes() {
            continue;
        }

        if entry.flags & zip::FLAG_ENCRYPTED != 0 {
            return Err(format!("{name} is encrypted"));
        }
        if entry.compressed_size == u32::MAX
            || entry.size == u32::MAX
            || entry.local_offset == u32::MAX
        {
            return Err(format!(
                "{name} is a ZIP64 entry, which this reader does not read"
            ));
        }
        let local = entry.local_offset as usize;
        let header = archive
            .get(local..)
            .ok_or_else(|| format!("{name}: its local header at {local} is past the end"))
            .and_then(|rest| {
                zip::local_header(rest).map_err(|error| format!("{name}: {error} (offset {local})"))
            })?;
        let data = zip::slice(
            archive,
            local + header.length,
            entry.compressed_size as usize,
        )?;
        let contents = match entry.method {
            zip::STORED => data.to_vec(),
            zip::DEFLATED => {
                miniz_oxide::inflate::decompress_to_vec_with_limit(data, entry.size as usize)
                    .map_err(|e| format!("{name} does not inflate: {:?}", e.status))?
            }
            other => return Err(format!("{name} uses compression method {other}")),
        };
        if contents.len() != entry.size as usize {
            return Err(format!(
                "{name} inflated to {} bytes, but its directory entry says {}",
                contents.len(),
                entry.size
            ));
        }
        return Ok(contents);
    }
    Err(format!("no entry named {name}"))
}

#[cfg(test)]
mod tests {
    use super::{
        PACKAGE, PACKAGE_SHA256, SIDECAR, exported, hex, sha256, unpack, verify_pinned, zip_entry,
    };
    use bt_winres::release_manifest::{MEMBER_LIST, Source, parse_member_list, sidecar_key};

    fn vendored_package() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/conpty")
            .join(PACKAGE);
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    }

    /// RED (67) — **A file one byte away from its pin is refused, and the refusal names the file.**
    ///
    /// The pins are the whole reason the sidecar is taken from a vendored package rather than from
    /// whatever ConPTY the machine ships: they are what makes `conpty.dll` Microsoft's signed build
    /// and not a file somebody dropped into `vendor/`. When the check moved out of a PowerShell
    /// script into the build script's own Rust, it became a function that can be called on its own,
    /// and this is the call: the real `conpty.dll` out of the real package passes, and the same
    /// bytes with one of them changed do not, with `conpty.dll`'s entry named in what the build
    /// prints. The package itself is held to its own pin the same way, through `unpack`, the
    /// function `build.rs` calls.
    ///
    /// MUTATION: make `verify_pinned` return `Ok(())` without hashing.
    #[test]
    fn a_file_one_byte_away_from_its_pin_is_refused_by_name() {
        let package = vendored_package();
        let dll = &SIDECAR[0];
        let mut bytes = zip_entry(&package, dll.entry).expect("the package holds conpty.dll");
        assert_eq!(verify_pinned(dll.entry, &bytes, dll.sha256), Ok(()));

        let middle = bytes.len() / 2;
        bytes[middle] ^= 1;
        let refusal = verify_pinned(dll.entry, &bytes, dll.sha256)
            .expect_err("a changed byte must not pass the pin");
        assert!(
            refusal.contains("runtimes/win-x64/native/conpty.dll"),
            "the refusal names the file: {refusal}"
        );

        let mut package = package;
        let last = package.len() - 1;
        package[last] ^= 1;
        let refusal = unpack(&package)
            .err()
            .expect("a changed package must not unpack");
        assert!(
            refusal.contains(PACKAGE),
            "the refusal names the package: {refusal}"
        );
    }

    /// RED (67) — **The vendored package unpacks, on any host, into exactly the pinned sidecar.**
    ///
    /// This is the build script's whole job run end to end on the real package: its pin, the zip
    /// reader over its central directory, the inflating, and each entry's pin. The targets are the
    /// retired script's, file for file, which is what keeps a Windows build byte-identical to
    /// before.
    ///
    /// MUTATION: return the compressed bytes of a deflated entry instead of inflating them. (On a
    /// Windows target the build script itself refuses first, with the entry named; elsewhere this
    /// is what goes red.)
    #[test]
    fn the_vendored_package_unpacks_into_exactly_the_pinned_sidecar() {
        let sidecar = unpack(&vendored_package()).expect("the vendored package is the pinned one");
        let written: Vec<(&str, &str, usize)> = sidecar
            .iter()
            .flat_map(|(file, bytes)| {
                assert_eq!(hex(&sha256(bytes)), file.sha256);
                file.targets
                    .iter()
                    .map(move |target| (*target, file.sha256, bytes.len()))
            })
            .collect();
        assert_eq!(
            written,
            [
                (
                    "conpty.dll",
                    "e2fe87e2258c4e46ffc5157f727218cc25f34a174902f72eb8a5b49edd9a6458",
                    109_920
                ),
                (
                    "OpenConsole.exe",
                    "2525c351aa136d555e5df9a3c9d6ce9be43f785e37e3c993b8f23b3f0a53c7fa",
                    1_063_224
                ),
                (
                    "x64/OpenConsole.exe",
                    "2525c351aa136d555e5df9a3c9d6ce9be43f785e37e3c993b8f23b3f0a53c7fa",
                    1_063_224
                ),
            ]
        );
        assert_eq!(hex(&sha256(&vendored_package())), PACKAGE_SHA256);
    }

    /// RED (U-9) — **the build script exports exactly the sidecar files the release archive lists,
    /// under the keys the manifest's build reads, at the paths beside the executable.**
    ///
    /// `bt-app`'s build script hashes the two ConPTY files into the manifest `folio.exe` carries,
    /// and it finds them only through this package's `links` metadata. So the set exported here has
    /// to be the set `scripts/release/archive-members.txt` calls `sidecar` — a file exported that
    /// the archive does not carry, or one the archive carries that is not exported, is a manifest
    /// `build.rs` cannot complete — and each path has to be the copy beside the executable, which
    /// is the one `package.ps1` packs, and not the `x64/` mirror.
    ///
    /// MUTATION: export `file.targets.last()` instead of `targets[0]` and the `OpenConsole.exe`
    /// path is `x64/OpenConsole.exe`; export only `SIDECAR[..1]` and the two lists differ.
    #[test]
    fn the_sidecar_is_exported_under_the_keys_the_manifest_reads() {
        let list = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(MEMBER_LIST),
        )
        .expect("the release member list is in the workspace");
        let listed: Vec<String> = parse_member_list(&list)
            .expect("the release member list parses")
            .into_iter()
            .filter(|item| item.source == Source::Sidecar)
            .map(|item| item.name)
            .collect();
        assert!(!listed.is_empty(), "the archive carries the sidecar");

        let profile = std::path::Path::new("PROFILE");
        let expected: Vec<(String, std::path::PathBuf)> = listed
            .iter()
            .map(|name| (sidecar_key(name), profile.join(name)))
            .collect();
        assert_eq!(exported(profile), expected);
    }
}
