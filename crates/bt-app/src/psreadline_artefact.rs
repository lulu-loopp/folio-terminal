//! **Folio's Windows PowerShell line editor, unpacked from its vendored package by Rust alone**
//! (T-INTEGRATION-INJECT-2-BUILD; `docs/plans/design/integration-inject-2-2026-10-15.md` §2.1).
//!
//! **This is build-script code**, on `bt-pty`'s `conpty_sidecar` model. `build.rs` reaches it by
//! `#[path]`; the crate compiles it only under `cfg(test)`, which is how a build script's checks
//! get tests at all. Everything here is a pure function of bytes: reading the package and writing
//! the files are `build.rs`'s business, and what the executable embeds is the container
//! `build.rs` writes from this one (`psreadline_module.bin` in `OUT_DIR`, read by
//! `crate::psreadline::module_files`).
//!
//! The package is Microsoft's PSReadLine 2.4.5 as the PowerShell Gallery serves it, fetched once
//! by the person who moved the pin and kept in `vendor/psreadline/` with its SHA-256, its entries
//! and its signature facts (`vendor/psreadline/README.md`). Nothing fetches it at build, test or
//! package time. The pins here are the gate every build passes; `scripts/ci/check-psreadline-artefact.ps1`
//! is the gate a pin change passes, and the place each DLL's Authenticode signature is asserted.
//!
//! **Binary-only, and that is what lets it load under every execution policy** (the note's §0.2):
//! the entries taken are the four assemblies and the licence. Upstream's `PSReadLine.psd1`,
//! `PSReadLine.psm1` and `PSReadLine.format.ps1xml` are not — a policy governs the last two, and
//! under `Restricted` or `AllSigned` the signed `.ps1xml` stops a 5.1 startup at the
//! untrusted-publisher prompt (the spike's Q7). The manifest PowerShell reads is Folio's own,
//! `assets/psreadline/PSReadLine.psd1`, whose `RootModule` is the assembly.

use bt_winres::pinned::{entry, verify_pinned};

/// The vendored package, by file name. `build.rs` joins it to `WORKSPACE/vendor/psreadline`.
pub const PACKAGE: &str = "PSReadLine.2.4.5.nupkg";

/// The SHA-256 of [`PACKAGE`]: the bytes the PowerShell Gallery served on 2026-10-17. Our hash of
/// the package is the pin, as it is for the ConPTY package; the gallery's own feed hash is not
/// relied on.
pub const PACKAGE_SHA256: &str = "cb9390e9733208456c234a7971d1ec4a917886c239502aab68f4b71aa4bba235";

/// The module's name: the directory PowerShell looks it up by on a module path.
pub const MODULE: &str = "PSReadLine";

/// The module version the package carries — the `ModuleVersion` of Folio's manifest
/// ([`MANIFEST`]) and the directory under [`MODULE`] the module is written into.
pub const VERSION: &str = "2.4.5";

/// Folio's own manifest for the module, by its name in the module directory. Its bytes are
/// `assets/psreadline/PSReadLine.psd1`; `build.rs` writes them into the module container, last.
pub const MANIFEST: &str = "PSReadLine.psd1";

/// **The artefact's name on disk**: the first twelve hex digits of [`PACKAGE_SHA256`]. Folio's
/// copy lives at `%LOCALAPPDATA%\Folio\psmodules\<artefact>\PSReadLine\2.4.5\` (the note's §2.2):
/// content-addressed, so a build carrying the same package finds the same bytes and a build
/// carrying another writes a sibling, and a directory a live shell holds open is never rewritten.
#[must_use]
pub fn artefact() -> &'static str {
    &PACKAGE_SHA256[..ARTEFACT_DIGITS]
}

/// How many hex digits of the package's SHA-256 name the artefact.
pub const ARTEFACT_DIGITS: usize = 12;

/// One file of the module taken out of the package: its entry name, which is also its path in the
/// module directory, and the hash it must have.
pub struct ModuleFile {
    pub entry: &'static str,
    pub sha256: &'static str,
}

/// **The entries taken**, in the order the module directory is written: the four assemblies and
/// the licence. The licence is not optional — PSReadLine is BSD 2-Clause, and a binary
/// distribution reproduces the notice.
pub const TAKEN: [ModuleFile; 5] = [
    ModuleFile {
        entry: "Microsoft.PowerShell.PSReadLine.dll",
        sha256: "f8e3a5b7e3e8cad2130ce10647564a2a0ea15d98db8a0cc8d589f80154c108e2",
    },
    ModuleFile {
        entry: "Microsoft.PowerShell.Pager.dll",
        sha256: "451994c3d3e38d939b4fa5f8594d0207840f45e7a210aa75f81aef85aa17c592",
    },
    ModuleFile {
        entry: "netstd/Microsoft.PowerShell.PSReadLine.Polyfiller.dll",
        sha256: "2e63dc86d9240243ccfb07da5d0b4fce2bc5ce5f9952658a7ce68c26bcd3e14c",
    },
    ModuleFile {
        entry: "net6plus/Microsoft.PowerShell.PSReadLine.Polyfiller.dll",
        sha256: "6d33ea289b405c64bf5daa9c59e459596cac59ece399d0d215ee9f3d46afd675",
    },
    ModuleFile {
        entry: "License.txt",
        sha256: "25c2fdfcdc653f65629233f5ef217a83287733d3c0299c207b0bca865508c463",
    },
];

/// **The verified module files, out of the verified package**: each of [`TAKEN`] with its bytes.
///
/// The package is checked against [`PACKAGE_SHA256`] before a byte of it is parsed, and each entry
/// against its own pin once it is inflated — the same two refusals as the ConPTY sidecar, through
/// the same `bt_winres::pinned`. Either refusal names the file it is about.
pub fn unpack(package: &[u8]) -> Result<Vec<(&'static ModuleFile, Vec<u8>)>, String> {
    unpack_pinned(package, PACKAGE_SHA256, &TAKEN)
}

/// [`unpack`] against a pin and a table handed in — the seam the tests hold the refusals with,
/// over a package they build themselves.
fn unpack_pinned<'a>(
    package: &[u8],
    pinned: &str,
    taken: &'a [ModuleFile],
) -> Result<Vec<(&'a ModuleFile, Vec<u8>)>, String> {
    verify_pinned(PACKAGE, package, pinned)?;
    taken
        .iter()
        .map(|file| {
            let bytes = entry(package, file.entry).map_err(|e| format!("{PACKAGE}: {e}"))?;
            verify_pinned(file.entry, &bytes, file.sha256)?;
            Ok((file, bytes))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ARTEFACT_DIGITS, MANIFEST, MODULE, ModuleFile, PACKAGE, PACKAGE_SHA256, TAKEN, VERSION,
        artefact, unpack, unpack_pinned,
    };
    use bt_winres::digest::{hex, sha256};
    use std::path::{Path, PathBuf};

    /// **The package's other entries, named rather than ignored**, so that a package whose layout
    /// differs from the one recorded here is a difference this crate's tests and the check script
    /// say out loud, not one a build guesses past. Upstream's three script-shaped files (the note's
    /// §1.3 item 4), the change log (the notices carry the licence; the log is not needed to run the
    /// module), the sample profile (a 2.4.5 entry the fork never carried: a `.ps1` for a person to
    /// read, which Folio does not run), and the NuGet package's own records.
    const NOT_TAKEN: [&str; 10] = [
        "PSReadLine.psd1",
        "PSReadLine.psm1",
        "PSReadLine.format.ps1xml",
        "Changes.txt",
        "SamplePSReadLineProfile.ps1",
        "PSReadLine.nuspec",
        "_rels/.rels",
        "[Content_Types].xml",
        "package/services/metadata/core-properties/0a75684263af4ef49be98091fe8b6bfd.psmdcp",
        ".signature.p7s",
    ];

    fn workspace() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn vendored_package() -> Vec<u8> {
        let path = workspace().join("vendor/psreadline").join(PACKAGE);
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    }

    /// A zip archive holding `entries`, deflated, written the way a package is: local headers,
    /// then the central directory, then the end record. Only the fields `bt_winres::zip` reads are
    /// filled in; the CRC is the real one so the archive is a real one.
    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = Vec::new();
        let mut directory = Vec::new();
        for (name, data) in entries {
            let deflated = miniz_oxide::deflate::compress_to_vec(data, 6);
            let crc = bt_winres::zip::crc32(data);
            let offset = u32::try_from(archive.len()).unwrap();
            let name_length = u16::try_from(name.len()).unwrap();
            let compressed = u32::try_from(deflated.len()).unwrap();
            let size = u32::try_from(data.len()).unwrap();
            archive.extend_from_slice(&bt_winres::zip::LOCAL_FILE_HEADER.to_le_bytes());
            archive.extend_from_slice(&20u16.to_le_bytes());
            archive.extend_from_slice(&0u16.to_le_bytes());
            archive.extend_from_slice(&bt_winres::zip::DEFLATED.to_le_bytes());
            archive.extend_from_slice(&[0; 4]);
            archive.extend_from_slice(&crc.to_le_bytes());
            archive.extend_from_slice(&compressed.to_le_bytes());
            archive.extend_from_slice(&size.to_le_bytes());
            archive.extend_from_slice(&name_length.to_le_bytes());
            archive.extend_from_slice(&0u16.to_le_bytes());
            archive.extend_from_slice(name.as_bytes());
            archive.extend_from_slice(&deflated);

            directory.extend_from_slice(&bt_winres::zip::CENTRAL_DIRECTORY_HEADER.to_le_bytes());
            directory.extend_from_slice(&20u16.to_le_bytes());
            directory.extend_from_slice(&20u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&bt_winres::zip::DEFLATED.to_le_bytes());
            directory.extend_from_slice(&[0; 4]);
            directory.extend_from_slice(&crc.to_le_bytes());
            directory.extend_from_slice(&compressed.to_le_bytes());
            directory.extend_from_slice(&size.to_le_bytes());
            directory.extend_from_slice(&name_length.to_le_bytes());
            directory.extend_from_slice(&[0; 12]);
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let directory_offset = u32::try_from(archive.len()).unwrap();
        let count = u16::try_from(entries.len()).unwrap();
        let directory_size = u32::try_from(directory.len()).unwrap();
        archive.extend_from_slice(&directory);
        archive.extend_from_slice(&bt_winres::zip::END_OF_CENTRAL_DIRECTORY.to_le_bytes());
        archive.extend_from_slice(&[0; 4]);
        archive.extend_from_slice(&count.to_le_bytes());
        archive.extend_from_slice(&count.to_le_bytes());
        archive.extend_from_slice(&directory_size.to_le_bytes());
        archive.extend_from_slice(&directory_offset.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **a package whose own hash is wrong is refused before
    /// a byte of it is read, and an entry whose hash is wrong is refused by name.**
    ///
    /// Over injected bytes, the `conpty_sidecar` way: a package this test builds, holding one
    /// entry, pinned by its real hash. The same package against another pin is refused naming the
    /// package; the same pin with the entry's bytes changed is refused naming the entry — the two
    /// gates every build of the executable passes, so a tampered DLL in `vendor/` is a build that
    /// does not finish.
    ///
    /// MUTATION: drop the `verify_pinned(file.entry, …)` line of `unpack_pinned` (skip the entry
    /// check) — the tampered entry unpacks and the second assertion goes red.
    #[test]
    fn a_wrong_package_or_entry_hash_is_refused_with_the_file_named() {
        let dll = b"MZ an assembly, as far as this test is concerned".as_slice();
        let table = [ModuleFile {
            entry: "Microsoft.PowerShell.PSReadLine.dll",
            sha256: Box::leak(hex(&sha256(dll)).into_boxed_str()),
        }];
        let package = zip_of(&[("Microsoft.PowerShell.PSReadLine.dll", dll)]);
        let pinned = hex(&sha256(&package));
        let unpacked = unpack_pinned(&package, &pinned, &table).expect("the pinned package");
        assert_eq!(unpacked.len(), 1);
        assert_eq!(unpacked[0].1, dll);

        let refusal = unpack_pinned(&package, &"0".repeat(64), &table)
            .err()
            .expect("a package whose hash is not the pin");
        assert!(refusal.contains(PACKAGE), "names the package: {refusal}");

        let tampered = b"MZ an assembly somebody changed after it was pinned".as_slice();
        let package = zip_of(&[("Microsoft.PowerShell.PSReadLine.dll", tampered)]);
        let pinned = hex(&sha256(&package));
        let refusal = unpack_pinned(&package, &pinned, &table)
            .err()
            .expect("an entry whose hash is not its pin");
        assert!(
            refusal.contains("Microsoft.PowerShell.PSReadLine.dll"),
            "names the entry: {refusal}"
        );
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **the vendored package unpacks, on any host, into
    /// exactly the pinned module files, and its layout is the recorded one.**
    ///
    /// The build script's whole job on the real package: its pin, the zip reader, the inflating
    /// and each entry's pin. And the layout, which the note's §8 said the build step would read
    /// first and record rather than guess: every file entry in the package is either taken or
    /// named as not taken, and no name is both. A package with an entry nobody has looked at —
    /// a new assembly a later version needs, say — is red here before it is a module that
    /// silently lacks it.
    ///
    /// MUTATION: drop `netstd/…Polyfiller.dll` from `TAKEN` — the layout assertion names it.
    #[test]
    fn the_vendored_package_unpacks_into_exactly_the_recorded_module() {
        let package = vendored_package();
        assert_eq!(hex(&sha256(&package)), PACKAGE_SHA256);
        let unpacked = unpack(&package).expect("the vendored package is the pinned one");
        let sizes: Vec<(&str, usize)> = unpacked
            .iter()
            .map(|(file, bytes)| {
                assert_eq!(hex(&sha256(bytes)), file.sha256);
                (file.entry, bytes.len())
            })
            .collect();
        assert_eq!(
            sizes,
            [
                ("Microsoft.PowerShell.PSReadLine.dll", 339_528),
                ("Microsoft.PowerShell.Pager.dll", 16_784),
                (
                    "netstd/Microsoft.PowerShell.PSReadLine.Polyfiller.dll",
                    16_928
                ),
                (
                    "net6plus/Microsoft.PowerShell.PSReadLine.Polyfiller.dll",
                    14_920
                ),
                ("License.txt", 1_324),
            ]
        );

        let mut in_the_package = Vec::new();
        let tail = package
            .len()
            .saturating_sub(bt_winres::zip::END_RECORD_BYTES + bt_winres::zip::MAX_COMMENT_BYTES);
        let (_, end) = bt_winres::zip::find_end(&package[tail..]).expect("an end record");
        let mut at = end.directory_offset as usize;
        for _ in 0..end.entries {
            let (central, next) = bt_winres::zip::central_entry(&package, at).expect("an entry");
            at = next;
            in_the_package.push(String::from_utf8(central.name.to_vec()).expect("a UTF-8 name"));
        }
        let mut recorded: Vec<String> = TAKEN
            .iter()
            .map(|file| file.entry.to_owned())
            .chain(NOT_TAKEN.iter().map(|name| (*name).to_owned()))
            .collect();
        in_the_package.sort();
        recorded.sort();
        assert_eq!(
            in_the_package, recorded,
            "the package's entries are exactly the taken and the named untaken ones"
        );
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **the artefact is named by the package's own hash, and
    /// Folio's manifest names the version, the assembly and nothing a policy governs.**
    ///
    /// `crate::psreadline::SHIPPED_VERSION` is bound to [`VERSION`] there; this binds [`VERSION`]
    /// to the package's name and to the manifest's `ModuleVersion`, and the manifest's shape to the
    /// binary-only rule the whole design rests on — a `.psm1`, a `FormatsToProcess` or a
    /// `NestedModules` line put back is the publisher prompt back under `AllSigned`.
    ///
    /// MUTATION: add `FormatsToProcess = 'PSReadLine.format.ps1xml'` to the manifest.
    #[test]
    fn the_manifest_and_the_artefact_name_agree_with_the_pin() {
        assert_eq!(artefact().len(), ARTEFACT_DIGITS);
        assert!(PACKAGE_SHA256.starts_with(artefact()));
        assert_eq!(PACKAGE, format!("{MODULE}.{VERSION}.nupkg"));
        let manifest =
            std::fs::read_to_string(workspace().join("assets/psreadline").join(MANIFEST))
                .expect("Folio's PSReadLine manifest");
        let settings: Vec<(String, String)> = manifest
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
            .collect();
        let value = |key: &str| {
            settings
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.trim_matches('\'').to_owned())
        };
        assert_eq!(value("ModuleVersion").as_deref(), Some(VERSION));
        assert_eq!(
            value("RootModule").as_deref(),
            Some(TAKEN[0].entry),
            "the module is its assembly"
        );
        assert_eq!(value("PowerShellVersion").as_deref(), Some("5.1"));
        for absent in [
            "NestedModules",
            "FormatsToProcess",
            "ScriptsToProcess",
            "TypesToProcess",
            "FunctionsToExport",
        ] {
            assert_eq!(
                value(absent),
                None,
                "{absent} is not in a binary-only manifest"
            );
        }
        assert!(!manifest.contains(".psm1'"), "no script module is named");
    }
}
