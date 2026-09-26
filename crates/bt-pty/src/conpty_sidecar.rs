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
//! reading the package costs no new package. SHA-256 is FIPS 180-4, written out below for the same
//! reason; the tests below hold it to the standard's own examples.

/// The vendored package, by file name. `build.rs` joins it to `WORKSPACE/vendor/conpty`.
pub const PACKAGE: &str = "Microsoft.Windows.Console.ConPTY.1.25.260710002-preview.nupkg";

/// The SHA-256 of [`PACKAGE`], pinned to the official release asset.
pub const PACKAGE_SHA256: &str = "05fe9b571ea4fb198f5012405cb39a132cf23eee50feaa496524c149b2502692";

/// One file the sidecar is made of: where it sits in the package, the hash it must have, and the
/// paths, relative to each destination directory, that it is written to.
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

const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;

/// **The contents of the entry called `name` in the zip archive `archive`.**
///
/// Stored and deflated entries are read. An encrypted entry, another compression method, or a
/// ZIP64 archive (whose sizes do not fit the fields read here) is an error that says which.
pub fn zip_entry(archive: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let end = end_of_central_directory(archive)?;
    let entries = u16_at(archive, end + 10)?;
    let directory_size = u32_at(archive, end + 12)?;
    let directory_offset = u32_at(archive, end + 16)?;
    if entries == u16::MAX || directory_size == u32::MAX || directory_offset == u32::MAX {
        return Err("a ZIP64 archive, which this reader does not read".to_owned());
    }

    let mut at = directory_offset as usize;
    for _ in 0..entries {
        if u32_at(archive, at)? != CENTRAL_DIRECTORY_HEADER {
            return Err(format!("no central directory header at offset {at}"));
        }
        let flags = u16_at(archive, at + 8)?;
        let method = u16_at(archive, at + 10)?;
        let compressed_size = u32_at(archive, at + 20)?;
        let size = u32_at(archive, at + 24)?;
        let name_length = usize::from(u16_at(archive, at + 28)?);
        let extra_length = usize::from(u16_at(archive, at + 30)?);
        let comment_length = usize::from(u16_at(archive, at + 32)?);
        let local_offset = u32_at(archive, at + 42)?;
        let entry_name = slice(archive, at + 46, name_length)?;
        at += 46 + name_length + extra_length + comment_length;
        if entry_name != name.as_bytes() {
            continue;
        }

        if flags & 1 != 0 {
            return Err(format!("{name} is encrypted"));
        }
        if compressed_size == u32::MAX || size == u32::MAX || local_offset == u32::MAX {
            return Err(format!(
                "{name} is a ZIP64 entry, which this reader does not read"
            ));
        }
        let local = local_offset as usize;
        if u32_at(archive, local)? != LOCAL_FILE_HEADER {
            return Err(format!("no local file header for {name} at offset {local}"));
        }
        let data_start = local
            + 30
            + usize::from(u16_at(archive, local + 26)?)
            + usize::from(u16_at(archive, local + 28)?);
        let data = slice(archive, data_start, compressed_size as usize)?;
        let contents = match method {
            STORED => data.to_vec(),
            DEFLATED => miniz_oxide::inflate::decompress_to_vec_with_limit(data, size as usize)
                .map_err(|e| format!("{name} does not inflate: {:?}", e.status))?,
            other => return Err(format!("{name} uses compression method {other}")),
        };
        if contents.len() != size as usize {
            return Err(format!(
                "{name} inflated to {} bytes, but its directory entry says {size}",
                contents.len()
            ));
        }
        return Ok(contents);
    }
    Err(format!("no entry named {name}"))
}

/// The offset of the end-of-central-directory record: the last occurrence of its signature within
/// the 22-byte record plus the longest comment a zip can carry.
fn end_of_central_directory(archive: &[u8]) -> Result<usize, String> {
    const RECORD: usize = 22;
    if archive.len() < RECORD {
        return Err(
            "not a zip archive: shorter than an end-of-central-directory record".to_owned(),
        );
    }
    let lowest = archive.len().saturating_sub(RECORD + usize::from(u16::MAX));
    (lowest..=archive.len() - RECORD)
        .rev()
        .find(|&at| u32_at(archive, at) == Ok(END_OF_CENTRAL_DIRECTORY))
        .ok_or_else(|| "not a zip archive: no end-of-central-directory record".to_owned())
}

fn slice(bytes: &[u8], at: usize, length: usize) -> Result<&[u8], String> {
    at.checked_add(length)
        .and_then(|end| bytes.get(at..end))
        .ok_or_else(|| format!("truncated: {length} bytes at offset {at} run past the end"))
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    let b = slice(bytes, at, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    let b = slice(bytes, at, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Lower-case hex, two digits a byte.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// **SHA-256 of `message`** (FIPS 180-4 §6.2).
pub fn sha256(message: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Padding: a one bit, zeros up to 56 bytes mod 64, then the length in bits, big-endian.
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&(message.len() as u64).wrapping_mul(8).to_be_bytes());

    for block in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (word, bytes) in w.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16]
                .wrapping_add(s0)
                .wrapping_add(w[t - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for (k, w) in K.iter().zip(w) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(*k)
                .wrapping_add(w);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (word, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *word = word.wrapping_add(value);
        }
    }

    let mut digest = [0u8; 32];
    for (bytes, word) in digest.chunks_exact_mut(4).zip(state) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::{PACKAGE, PACKAGE_SHA256, SIDECAR, hex, sha256, unpack, verify_pinned, zip_entry};

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

    /// RED (67) — **The hash the pins are checked with is SHA-256, by the standard's own examples.**
    ///
    /// The digest is written out in the build script rather than taken from a crate, so it is held
    /// to the FIPS 180-4 examples: the empty message, the one-block `abc`, the two-block 448-bit
    /// message whose padding spills into a second block, and a million `a`s, which crosses many
    /// blocks.
    ///
    /// MUTATION: rotate by 7 instead of 6 in the first `Σ1` term. (On a Windows target the build
    /// script refuses the package first, naming it; elsewhere this is what goes red.)
    #[test]
    fn the_digest_is_sha_256_by_the_standard_s_own_examples() {
        let million_a = vec![b'a'; 1_000_000];
        let examples: [(&[u8], &str); 4] = [
            (
                b"",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (
                &million_a,
                "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
            ),
        ];
        for (message, digest) in examples {
            assert_eq!(hex(&sha256(message)), digest, "{} bytes", message.len());
        }
    }
}
