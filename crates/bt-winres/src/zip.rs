//! **The zip format's records, read field by field** — the one spelling of the
//! layout in this workspace (0.4.6 ticket U-14).
//!
//! Two readers need it and they want different things of it. `bt-pty`'s build
//! script takes two entries out of the vendored ConPTY package, a whole file in
//! memory whose hash is pinned before a byte of it is parsed
//! (`bt_pty::conpty_sidecar`). The updater's archive reader (`bt-app`'s
//! `update_archive`) reads a downloaded release archive that nothing has
//! vouched for yet, from a file, a record at a time, and refuses anything
//! outside the subset `package.ps1` writes. What they share is the records:
//! the end-of-central-directory record, the central directory's file headers
//! and the local file headers, each a fixed block of little-endian fields
//! followed by a name and an extra field (APPNOTE 6.3.10 §4.3). Both read them
//! here, so the workspace has one copy of where each field is.
//!
//! Nothing here inflates, allocates beyond a record's own name, or decides
//! whether an archive is acceptable: every function answers "what does this
//! record say" or "these bytes are not that record". CRC-32 (§4.4.7) is here
//! too, because it is a property of the format, and dependency-free.
//!
//! **ZIP64 is not read.** A field holding its sentinel (`0xFFFF` or
//! `0xFFFF_FFFF`) is returned as it is; each reader refuses it in its own
//! words.

/// The end-of-central-directory record's signature.
pub const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
/// A central directory file header's signature.
pub const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
/// A local file header's signature.
pub const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;

/// The end-of-central-directory record's fixed part.
pub const END_RECORD_BYTES: usize = 22;
/// A central directory file header's fixed part.
pub const CENTRAL_HEADER_BYTES: usize = 46;
/// A local file header's fixed part.
pub const LOCAL_HEADER_BYTES: usize = 30;

/// The longest comment an end-of-central-directory record can carry, and so
/// the furthest from the end of a file the record can start.
pub const MAX_COMMENT_BYTES: usize = u16::MAX as usize;

/// Compression method 0.
pub const STORED: u16 = 0;
/// Compression method 8.
pub const DEFLATED: u16 = 8;

/// General purpose flag bit 0: the entry is encrypted.
pub const FLAG_ENCRYPTED: u16 = 1 << 0;
/// General purpose flag bit 3: sizes and CRC follow the data in a descriptor.
pub const FLAG_DATA_DESCRIPTOR: u16 = 1 << 3;
/// General purpose flag bit 11: the name is UTF-8.
pub const FLAG_UTF8: u16 = 1 << 11;

/// **The end-of-central-directory record** (§4.3.16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End {
    /// The number of this disk.
    pub disk: u16,
    /// The disk the central directory starts on.
    pub directory_disk: u16,
    /// Central directory entries on this disk.
    pub entries_on_disk: u16,
    /// Central directory entries in all.
    pub entries: u16,
    /// The central directory's length in bytes.
    pub directory_size: u32,
    /// Where the central directory starts, from the start of the archive.
    pub directory_offset: u32,
    /// The archive comment's length.
    pub comment_length: u16,
}

/// **Where the end-of-central-directory record is in `tail`, and what it
/// says**: the last signature in `tail` whose record and comment end exactly at
/// the end of `tail`.
///
/// `tail` is the last `END_RECORD_BYTES + MAX_COMMENT_BYTES` bytes of the
/// archive (or all of it, if it is shorter), so the offset returned is
/// relative to `tail`. Requiring the comment to reach the end is what rules out
/// a signature that happens to occur inside the comment, and bytes appended
/// after the record.
///
/// # Errors
///
/// A sentence naming what is missing.
pub fn find_end(tail: &[u8]) -> Result<(usize, End), String> {
    if tail.len() < END_RECORD_BYTES {
        return Err(
            "not a zip archive: shorter than an end-of-central-directory record".to_owned(),
        );
    }
    let lowest = tail
        .len()
        .saturating_sub(END_RECORD_BYTES + MAX_COMMENT_BYTES);
    for at in (lowest..=tail.len() - END_RECORD_BYTES).rev() {
        if u32_at(tail, at)? != END_OF_CENTRAL_DIRECTORY {
            continue;
        }
        let end = End {
            disk: u16_at(tail, at + 4)?,
            directory_disk: u16_at(tail, at + 6)?,
            entries_on_disk: u16_at(tail, at + 8)?,
            entries: u16_at(tail, at + 10)?,
            directory_size: u32_at(tail, at + 12)?,
            directory_offset: u32_at(tail, at + 16)?,
            comment_length: u16_at(tail, at + 20)?,
        };
        if at + END_RECORD_BYTES + usize::from(end.comment_length) == tail.len() {
            return Ok((at, end));
        }
    }
    Err("not a zip archive: no end-of-central-directory record ends the file".to_owned())
}

/// **One central directory file header** (§4.3.12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CentralEntry {
    /// The upper byte is the host system that wrote the entry (0 MS-DOS, 3
    /// Unix, 10 NTFS …), which says how to read [`Self::external_attributes`].
    pub version_made_by: u16,
    /// The general purpose flags.
    pub flags: u16,
    /// The compression method.
    pub method: u16,
    /// The CRC-32 of the uncompressed bytes.
    pub crc32: u32,
    /// The compressed length.
    pub compressed_size: u32,
    /// The uncompressed length.
    pub size: u32,
    /// The name, as bytes: the format does not say it is text unless
    /// [`FLAG_UTF8`] is set.
    pub name: Vec<u8>,
    /// The disk the entry starts on.
    pub disk_start: u16,
    /// Host-dependent attributes: MS-DOS attributes in the low byte, a Unix
    /// mode in the upper sixteen bits when the host is Unix.
    pub external_attributes: u32,
    /// Where the entry's local header starts, from the start of the archive.
    pub local_offset: u32,
}

/// **The central directory file header at `at` in `directory`**, and the offset
/// of the one after it.
///
/// # Errors
///
/// A sentence naming the offset where the bytes are not a header, or run out.
pub fn central_entry(directory: &[u8], at: usize) -> Result<(CentralEntry, usize), String> {
    if u32_at(directory, at)? != CENTRAL_DIRECTORY_HEADER {
        return Err(format!("no central directory header at offset {at}"));
    }
    let name_length = usize::from(u16_at(directory, at + 28)?);
    let extra_length = usize::from(u16_at(directory, at + 30)?);
    let comment_length = usize::from(u16_at(directory, at + 32)?);
    let entry = CentralEntry {
        version_made_by: u16_at(directory, at + 4)?,
        flags: u16_at(directory, at + 8)?,
        method: u16_at(directory, at + 10)?,
        crc32: u32_at(directory, at + 16)?,
        compressed_size: u32_at(directory, at + 20)?,
        size: u32_at(directory, at + 24)?,
        disk_start: u16_at(directory, at + 34)?,
        external_attributes: u32_at(directory, at + 38)?,
        local_offset: u32_at(directory, at + 42)?,
        name: slice(directory, at + CENTRAL_HEADER_BYTES, name_length)?.to_vec(),
    };
    let next = at + CENTRAL_HEADER_BYTES + name_length + extra_length + comment_length;
    slice(directory, at, next - at)?;
    Ok((entry, next))
}

/// **One local file header** (§4.3.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalHeader {
    /// The general purpose flags.
    pub flags: u16,
    /// The compression method.
    pub method: u16,
    /// The CRC-32, or zero when [`FLAG_DATA_DESCRIPTOR`] is set.
    pub crc32: u32,
    /// The compressed length, or zero when [`FLAG_DATA_DESCRIPTOR`] is set.
    pub compressed_size: u32,
    /// The uncompressed length, or zero when [`FLAG_DATA_DESCRIPTOR`] is set.
    pub size: u32,
    /// The name, as bytes.
    pub name: Vec<u8>,
    /// The header's whole length — fixed part, name and extra field — which is
    /// where the entry's data starts, from the header.
    pub length: usize,
}

/// **How long the local header starting with `fixed` is**, from its fixed
/// part alone: a reader reading from a file reads the fixed part, then this
/// many bytes in all.
///
/// # Errors
///
/// A sentence when `fixed` is not the start of a local header.
pub fn local_header_length(fixed: &[u8]) -> Result<usize, String> {
    if u32_at(fixed, 0)? != LOCAL_FILE_HEADER {
        return Err("no local file header where the central directory says one is".to_owned());
    }
    Ok(LOCAL_HEADER_BYTES + usize::from(u16_at(fixed, 26)?) + usize::from(u16_at(fixed, 28)?))
}

/// **The local header at the start of `bytes`**, which holds at least
/// [`local_header_length`] of them.
///
/// # Errors
///
/// A sentence when the bytes are not a local header, or run out.
pub fn local_header(bytes: &[u8]) -> Result<LocalHeader, String> {
    let length = local_header_length(bytes)?;
    let name_length = usize::from(u16_at(bytes, 26)?);
    slice(bytes, 0, length)?;
    Ok(LocalHeader {
        flags: u16_at(bytes, 6)?,
        method: u16_at(bytes, 8)?,
        crc32: u32_at(bytes, 14)?,
        compressed_size: u32_at(bytes, 18)?,
        size: u32_at(bytes, 22)?,
        name: slice(bytes, LOCAL_HEADER_BYTES, name_length)?.to_vec(),
        length,
    })
}

/// **CRC-32 as the zip format uses it** (§4.4.7: the IEEE 802.3 polynomial,
/// reflected, `0xEDB88320`), taken in pieces.
#[derive(Clone, Copy, Debug)]
pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    /// Nothing taken yet.
    #[must_use]
    pub const fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    /// Take the next piece.
    pub fn update(&mut self, bytes: &[u8]) {
        let mut crc = self.0;
        for byte in bytes {
            crc = CRC_TABLE[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
        }
        self.0 = crc;
    }

    /// The CRC of everything taken.
    #[must_use]
    pub const fn finish(self) -> u32 {
        !self.0
    }
}

/// The CRC of `bytes` in one piece.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(bytes);
    crc.finish()
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};

/// `length` bytes at `at`, or a sentence saying they run past the end.
///
/// # Errors
///
/// The sentence.
pub fn slice(bytes: &[u8], at: usize, length: usize) -> Result<&[u8], String> {
    at.checked_add(length)
        .and_then(|end| bytes.get(at..end))
        .ok_or_else(|| format!("truncated: {length} bytes at offset {at} run past the end"))
}

/// The little-endian `u16` at `at`.
///
/// # Errors
///
/// When it runs past the end.
pub fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    let b = slice(bytes, at, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

/// The little-endian `u32` at `at`.
///
/// # Errors
///
/// When it runs past the end.
pub fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    let b = slice(bytes, at, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::{
        CentralEntry, End, central_entry, crc32, find_end, local_header, local_header_length,
    };

    /// One stored entry `a/b.txt` holding `hi`, as a zip writer lays it out:
    /// local header, data, central directory, end record with `comment`.
    fn archive(comment: &[u8]) -> Vec<u8> {
        let name = b"a/b.txt";
        let data = b"hi";
        let crc = crc32(data);
        let mut out = Vec::new();
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(data);
        let directory = out.len();
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&0x0314u16.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&0o100_644u32.wrapping_shl(16).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(name);
        let size = out.len() - directory;
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
        out.extend_from_slice(&(size as u32).to_le_bytes());
        out.extend_from_slice(&(directory as u32).to_le_bytes());
        out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
        out.extend_from_slice(comment);
        out
    }

    /// RED (U-14) — **each record reads back field for field, and an end
    /// record is only one that ends the file.**
    ///
    /// The updater's archive reader and the ConPTY build script both read the
    /// format through these functions; a field read from the wrong offset is a
    /// size, a CRC or a name that is not the archive's. The end record's
    /// comment has to reach the end of the file exactly: a signature inside the
    /// comment, or bytes appended after the record, is not an end record.
    ///
    /// MUTATION: drop the comment-reaches-the-end condition in `find_end` and
    /// the archive with a trailing byte is read.
    #[test]
    fn each_record_reads_back_and_the_end_record_ends_the_file() {
        let bytes = archive(b"");
        let (at, end) = find_end(&bytes).expect("an end record");
        assert_eq!(at, bytes.len() - 22);
        assert_eq!(
            end,
            End {
                disk: 0,
                directory_disk: 0,
                entries_on_disk: 1,
                entries: 1,
                directory_size: 53,
                directory_offset: 39,
                comment_length: 0,
            }
        );
        let (entry, next) = central_entry(&bytes, 39).expect("a central header");
        assert_eq!(next, 39 + 53);
        assert_eq!(
            entry,
            CentralEntry {
                version_made_by: 0x0314,
                flags: 0,
                method: 0,
                crc32: crc32(b"hi"),
                compressed_size: 2,
                size: 2,
                name: b"a/b.txt".to_vec(),
                disk_start: 0,
                external_attributes: 0o100_644 << 16,
                local_offset: 0,
            }
        );
        assert_eq!(local_header_length(&bytes).unwrap(), 37);
        let local = local_header(&bytes).expect("a local header");
        assert_eq!(local.name, b"a/b.txt");
        assert_eq!(
            (local.size, local.compressed_size, local.length),
            (2, 2, 37)
        );

        // The signature inside a comment is not the record.
        let mut comment = 0x0605_4b50u32.to_le_bytes().to_vec();
        comment.extend_from_slice(&[0; 16]);
        comment.extend_from_slice(&7u16.to_le_bytes());
        let bytes = archive(&comment);
        let (at, end) = find_end(&bytes).expect("the real end record");
        assert_eq!(at, bytes.len() - 22 - comment.len());
        assert_eq!(end.comment_length, 22);

        let mut appended = archive(b"");
        appended.push(0);
        assert!(find_end(&appended).is_err(), "a byte after the end record");
        assert!(central_entry(&archive(b"")[..60], 39).is_err(), "cut short");
        assert!(
            central_entry(&archive(b""), 0).is_err(),
            "not a central header"
        );
    }

    /// RED (U-14) — **CRC-32 is the zip format's**, by the check value every
    /// CRC-32/ISO-HDLC table publishes (`123456789` → `CBF43926`), and in
    /// pieces it is the same.
    ///
    /// MUTATION: seed the register with zero instead of all ones and both
    /// values differ.
    #[test]
    fn crc_32_is_the_formats_by_its_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        let mut pieces = super::Crc32::new();
        pieces.update(b"1234");
        pieces.update(b"56789");
        assert_eq!(pieces.finish(), 0xCBF4_3926);
    }
}
