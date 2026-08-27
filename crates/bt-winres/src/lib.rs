//! **The `.res` file `folio.exe` is linked with** — its icon and its
//! `VERSIONINFO`, written out as bytes.
//!
//! # What this is for
//!
//! Two of the four places the product's version has to appear are inside the
//! executable's own resource directory: the `0.1.0` Explorer shows on the
//! Properties page, and the icon Explorer draws for the file (and that the
//! context-menu entry names as `folio.exe,0` — see
//! `bt_platform::context_menu_shape`). Neither can be a Rust constant; both are
//! a block of bytes the linker has to be handed.
//!
//! `link.exe` takes a `.res` file directly as an input, and cargo can hand it
//! one with `cargo:rustc-link-arg-bins`. So the whole of the problem is
//! producing that file — which is what this crate does, from `bt-app`'s build
//! script.
//!
//! # Why it is written here rather than taken from crates.io
//!
//! The two crates that do this — `winres` and `embed-resource` — both work by
//! *shelling out to `rc.exe`* and adding six packages to the lock file to find
//! it. That makes a release build depend on a Windows SDK component being
//! present and locatable, on top of the dependency policy in `docs/DESIGN.md`
//! §8. The RES container is a documented sequence of aligned records and
//! `VS_VERSIONINFO` is a documented tree of aligned blocks; between them they
//! are the two hundred lines below, they need no toolchain, and they are
//! covered by tests that read back what they wrote.
//!
//! # The two layouts
//!
//! **RES** (`[`ResourceFile`]) is a flat list of records. Each one is a header —
//! two sizes, a type, a name, a language and three fields nothing modern reads —
//! followed by its data, with every record beginning on a four-byte boundary.
//! The file opens with a record that is nothing but a header describing an empty
//! resource of type 0; that null record is how a consumer tells a 32-bit RES
//! from the 16-bit one it replaced, and a file without it is rejected.
//!
//! **`VS_VERSIONINFO`** ([`VersionInfo`]) is a tree of blocks that all share one
//! shape: a length, a value length, a flag saying whether the value is text, a
//! UTF-16 key, and then — after padding — a value, children, or both. The
//! subtlety that makes it worth testing rather than eyeballing is that
//! `wValueLength` is a **byte** count for a binary value and a **UTF-16
//! character** count for a text one, including its terminator in the second case
//! and not the first.

use std::fmt;

/// `RT_ICON` — one image out of an `.ico`.
const RT_ICON: u16 = 3;
/// `RT_GROUP_ICON` — the directory that says which images belong to one icon.
const RT_GROUP_ICON: u16 = 14;
/// `RT_VERSION` — the `VS_VERSIONINFO` block.
const RT_VERSION: u16 = 16;

/// US English. The language every record here is filed under.
///
/// Not the user's language and not a build option: this is the language of the
/// *resource*, and the strings in it are a company name, a product name and a
/// version number — none of which this product translates. A build that filed
/// them under a language and then ran on a machine set to another would have
/// Explorer fall back to the first table it finds, which is this one, so the
/// only thing a second table would buy is a second copy of `Folio`.
const LANG_EN_US: u16 = 0x0409;
/// The Unicode code page, which is the second half of the `040904b0` key the
/// string table is named after.
const CODEPAGE_UNICODE: u16 = 0x04B0;

/// `MOVEABLE | DISCARDABLE`, the flags `rc.exe` gives an icon image.
///
/// These three are 16-bit memory-manager hints that nothing since Win32 has
/// read. They are written as `rc.exe` writes them anyway: this crate's whole
/// claim is that it produces the file that tool produces, and a field left at a
/// plausible guess is the field that turns a byte-for-byte comparison against
/// the reference into a comparison somebody has to explain.
const MEMORY_ICON: u16 = 0x1010;
/// `MOVEABLE | PURE | DISCARDABLE`, for the group directory.
const MEMORY_ICON_GROUP: u16 = 0x1030;
/// `MOVEABLE | PURE`, for a version block.
const MEMORY_VERSION: u16 = 0x0030;

/// Why the caller's input could not be turned into resources.
///
/// One variant and not five: every way this can fail is the same fact — what the
/// build script was handed is not what it says it is — and its only response to
/// any of them is to stop with the message. Splitting it further would be
/// inventing choices for a caller that has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceError(String);

impl fmt::Display for ResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ResourceError {}

/// One image inside an `.ico`, as the group directory describes it.
///
/// The four bytes at the front are the ones the format spells oddly: a 256-pixel
/// icon is written as a width of `0`, because the field is one byte wide and
/// 256 does not fit in it. They are carried through exactly as the file gave
/// them rather than re-derived, because the group directory Explorer reads has
/// the same fields with the same spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IcoEntry {
    width: u8,
    height: u8,
    colours: u8,
    planes: u16,
    bit_count: u16,
    offset: u32,
    length: u32,
}

/// The images of an `.ico`, ready to become one `RT_GROUP_ICON` and n `RT_ICON`.
#[derive(Clone, Debug)]
pub struct IconGroup {
    entries: Vec<IcoEntry>,
    images: Vec<Vec<u8>>,
}

impl IconGroup {
    /// Read an `.ico` file.
    ///
    /// # Errors
    ///
    /// [`ResourceError`] when the bytes are not an icon file, or when a directory
    /// entry points outside them.
    pub fn parse(bytes: &[u8]) -> Result<Self, ResourceError> {
        let fail = |what: &str| ResourceError(format!("not an .ico file: {what}"));
        if bytes.len() < 6 {
            return Err(fail("shorter than a directory header"));
        }
        if read_u16(bytes, 0) != 0 || read_u16(bytes, 2) != 1 {
            return Err(fail("the directory header does not say ICON"));
        }
        let count = usize::from(read_u16(bytes, 4));
        if count == 0 {
            return Err(fail("it holds no images"));
        }
        let directory_end = 6 + count * 16;
        if bytes.len() < directory_end {
            return Err(fail("the directory runs past the end of the file"));
        }
        let mut entries = Vec::with_capacity(count);
        let mut images = Vec::with_capacity(count);
        for index in 0..count {
            let at = 6 + index * 16;
            let entry = IcoEntry {
                width: bytes[at],
                height: bytes[at + 1],
                colours: bytes[at + 2],
                planes: read_u16(bytes, at + 4),
                bit_count: read_u16(bytes, at + 6),
                length: read_u32(bytes, at + 8),
                offset: read_u32(bytes, at + 12),
            };
            let start = entry.offset as usize;
            let end = start
                .checked_add(entry.length as usize)
                .ok_or_else(|| fail("an image length overflows"))?;
            if end > bytes.len() || start < directory_end {
                return Err(fail("an image lies outside the file"));
            }
            entries.push(entry);
            images.push(bytes[start..end].to_vec());
        }
        Ok(Self { entries, images })
    }

    /// How many images this icon carries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Whether this icon carries no images. Never true for a parsed one — the
    /// parser refuses an empty directory — and present because `len` without it
    /// is a lint.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// The `GRPICONDIR` an `RT_GROUP_ICON` resource holds, given the id the
    /// first `RT_ICON` was filed under.
    ///
    /// It is the `.ico`'s own directory with one field changed: where the file
    /// says "this image is at byte 1234", the resource says "this image is
    /// resource number 3". That substitution is the whole difference between the
    /// two formats, and the reason an icon cannot simply be dropped into a `.res`
    /// whole.
    fn group_directory(&self, first_icon_id: u16) -> Vec<u8> {
        let count = u16::try_from(self.entries.len()).unwrap_or(u16::MAX);
        let mut out = Vec::with_capacity(6 + self.entries.len() * 14);
        out.extend_from_slice(&0u16.to_le_bytes()); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
        out.extend_from_slice(&count.to_le_bytes());
        for (index, entry) in self.entries.iter().enumerate() {
            out.push(entry.width);
            out.push(entry.height);
            out.push(entry.colours);
            out.push(0); // reserved
            out.extend_from_slice(&entry.planes.to_le_bytes());
            out.extend_from_slice(&entry.bit_count.to_le_bytes());
            out.extend_from_slice(&entry.length.to_le_bytes());
            let id = first_icon_id.saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
            out.extend_from_slice(&id.to_le_bytes());
        }
        out
    }
}

/// A four-part Windows version number, `major.minor.patch.build`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileVersion(pub [u16; 4]);

impl FileVersion {
    /// Read `major.minor.patch` — a Cargo version — and put `0` in the fourth
    /// field Windows insists on.
    ///
    /// A pre-release or build suffix (`0.1.0-rc.1`) is dropped, because the
    /// field it would go in does not exist: `VS_FIXEDFILEINFO` is four numbers
    /// and nothing else. The suffix still reaches the reader — it is in the
    /// `FileVersion` *string*, which is written out verbatim beside these
    /// numbers.
    ///
    /// # Errors
    ///
    /// [`ResourceError`] when the text is not three numbers separated by dots.
    pub fn parse_semver(text: &str) -> Result<Self, ResourceError> {
        let core = text
            .split(['-', '+'])
            .next()
            .expect("split always yields one part");
        let mut parts = [0u16; 4];
        let mut seen = 0;
        for (index, field) in core.split('.').enumerate() {
            if index >= 3 {
                return Err(ResourceError(format!("{text} has more than three fields")));
            }
            parts[index] = field
                .parse()
                .map_err(|_| ResourceError(format!("{text} field {index} is not a number")))?;
            seen += 1;
        }
        if seen != 3 {
            return Err(ResourceError(format!("{text} is not major.minor.patch")));
        }
        Ok(Self(parts))
    }

    /// The high and low halves `VS_FIXEDFILEINFO` stores this as.
    fn packed(self) -> (u32, u32) {
        let [major, minor, patch, build] = self.0;
        (
            (u32::from(major) << 16) | u32::from(minor),
            (u32::from(patch) << 16) | u32::from(build),
        )
    }
}

/// What Explorer's Details page shows about the file.
///
/// The strings are a list of pairs rather than named fields, because the set is
/// open: `StringFileInfo` takes whatever keys a producer writes, and the four or
/// five Windows renders specially are a convention rather than a schema. The
/// caller names what it wants said.
#[derive(Clone, Debug, Default)]
pub struct VersionInfo {
    pub file_version: FileVersion,
    pub product_version: FileVersion,
    pub strings: Vec<(String, String)>,
}

impl VersionInfo {
    /// The `VS_VERSIONINFO` block, as the bytes of an `RT_VERSION` resource.
    fn to_bytes(&self) -> Vec<u8> {
        let (file_ms, file_ls) = self.file_version.packed();
        let (product_ms, product_ls) = self.product_version.packed();
        let mut fixed = Vec::with_capacity(52);
        for word in [
            0xFEEF_04BDu32, // dwSignature
            0x0001_0000,    // dwStrucVersion: 1.0
            file_ms,
            file_ls,
            product_ms,
            product_ls,
            0x0000_003F, // dwFileFlagsMask: all six flags are meaningful
            0x0000_0000, // dwFileFlags: no debug, no pre-release, no patch
            0x0000_0004, // dwFileOS: VOS__WINDOWS32
            0x0000_0001, // dwFileType: VFT_APP
            0x0000_0000, // dwFileSubtype: none, for an application
            0x0000_0000, // dwFileDateMS
            0x0000_0000, // dwFileDateLS
        ] {
            fixed.extend_from_slice(&word.to_le_bytes());
        }

        let strings = self
            .strings
            .iter()
            .map(|(key, value)| block(key, Value::Text(value), &[]))
            .collect::<Vec<_>>();
        let table_key = format!("{LANG_EN_US:04x}{CODEPAGE_UNICODE:04x}");
        let table = block(&table_key, Value::Empty, &strings);
        let string_file_info = block("StringFileInfo", Value::Empty, &[table]);

        let mut translation = Vec::with_capacity(4);
        translation.extend_from_slice(&LANG_EN_US.to_le_bytes());
        translation.extend_from_slice(&CODEPAGE_UNICODE.to_le_bytes());
        let var = block("Translation", Value::Binary(&translation), &[]);
        let var_file_info = block("VarFileInfo", Value::Empty, &[var]);

        block(
            "VS_VERSION_INFO",
            Value::Binary(&fixed),
            &[string_file_info, var_file_info],
        )
    }
}

/// The value half of a `VS_VERSIONINFO` block.
///
/// Three cases and not `Option<&[u8]>`, because `wValueLength` counts different
/// things for each: bytes for a binary value, UTF-16 characters *including the
/// terminator* for a text one, and zero for a block that is only a parent.
enum Value<'a> {
    Empty,
    Binary(&'a [u8]),
    Text(&'a str),
}

/// One `VS_VERSIONINFO` block: header, key, value, children.
///
/// Every block this produces is padded to a four-byte boundary and its
/// `wLength` counts that padding, so a reader walking `wLength` from one
/// sibling lands exactly on the next. The alternative — lengths that stop short
/// of the padding — is legal too and is what makes hand-reading a hex dump of
/// one of these confusing; the walk is the same either way only because the
/// documented algorithm rounds up.
fn block(key: &str, value: Value<'_>, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&[0; 6]); // wLength and wValueLength, filled in below
    let (value_length, is_text) = match value {
        Value::Empty => (0u16, true),
        Value::Binary(bytes) => (u16::try_from(bytes.len()).unwrap_or(u16::MAX), false),
        // Characters, not bytes — and one more than the string has, for the
        // terminator. This is the field that is wrong in every hand-rolled
        // version block that renders as an empty Details page.
        Value::Text(text) => (
            u16::try_from(text.encode_utf16().count() + 1).unwrap_or(u16::MAX),
            true,
        ),
    };
    out[2..4].copy_from_slice(&value_length.to_le_bytes());
    out[4..6].copy_from_slice(&u16::from(is_text).to_le_bytes());
    push_utf16z(&mut out, key);
    pad_to_dword(&mut out);
    match value {
        Value::Empty => {}
        Value::Binary(bytes) => out.extend_from_slice(bytes),
        Value::Text(text) => push_utf16z(&mut out, text),
    }
    pad_to_dword(&mut out);
    for child in children {
        out.extend_from_slice(child);
        pad_to_dword(&mut out);
    }
    let length = u16::try_from(out.len()).unwrap_or(u16::MAX);
    out[0..2].copy_from_slice(&length.to_le_bytes());
    out
}

/// A whole `.res` file, built one record at a time.
#[derive(Debug, Default)]
pub struct ResourceFile {
    bytes: Vec<u8>,
}

impl ResourceFile {
    /// An empty file — which is to say, one holding only the null record every
    /// 32-bit `.res` opens with.
    #[must_use]
    pub fn new() -> Self {
        let mut file = Self { bytes: Vec::new() };
        file.push_record(0, 0, 0, &[]);
        file
    }

    /// Add an icon: its images as `RT_ICON` and its directory as
    /// `RT_GROUP_ICON` under `group_id`.
    ///
    /// `group_id` is what decides which icon an executable is *drawn* with:
    /// Explorer takes the lowest-numbered group, and `folio.exe,0` in a
    /// registered shell command means the same thing. So the application icon
    /// has to be group `1`.
    pub fn add_icon(&mut self, group_id: u16, icon: &IconGroup) {
        let first_icon_id = 1u16;
        for (index, image) in icon.images.iter().enumerate() {
            let id = first_icon_id + u16::try_from(index).unwrap_or(u16::MAX);
            self.push_record(RT_ICON, id, MEMORY_ICON, image);
        }
        self.push_record(
            RT_GROUP_ICON,
            group_id,
            MEMORY_ICON_GROUP,
            &icon.group_directory(first_icon_id),
        );
    }

    /// Add the `VS_VERSIONINFO` block, under the id Windows looks it up by.
    pub fn add_version_info(&mut self, version: &VersionInfo) {
        self.push_record(RT_VERSION, 1, MEMORY_VERSION, &version.to_bytes());
    }

    /// The file.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }

    /// One record: a fixed 32-byte header — both names given as ordinals, which
    /// is every name this crate writes — then the data, then padding.
    fn push_record(&mut self, type_id: u16, name_id: u16, memory_flags: u16, data: &[u8]) {
        let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
        self.bytes.extend_from_slice(&length.to_le_bytes());
        self.bytes.extend_from_slice(&32u32.to_le_bytes()); // header size
        self.bytes.extend_from_slice(&0xFFFFu16.to_le_bytes()); // type is an ordinal
        self.bytes.extend_from_slice(&type_id.to_le_bytes());
        self.bytes.extend_from_slice(&0xFFFFu16.to_le_bytes()); // name is an ordinal
        self.bytes.extend_from_slice(&name_id.to_le_bytes());
        self.bytes.extend_from_slice(&0u32.to_le_bytes()); // data version
        self.bytes.extend_from_slice(&memory_flags.to_le_bytes());
        // The null record is language-neutral; everything else is en-US.
        let language = if type_id == 0 { 0 } else { LANG_EN_US };
        self.bytes.extend_from_slice(&language.to_le_bytes());
        self.bytes.extend_from_slice(&0u32.to_le_bytes()); // version
        self.bytes.extend_from_slice(&0u32.to_le_bytes()); // characteristics
        self.bytes.extend_from_slice(data);
        pad_to_dword(&mut self.bytes);
    }
}

fn push_utf16z(out: &mut Vec<u8>, text: &str) {
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes());
}

fn pad_to_dword(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::{FileVersion, IconGroup, ResourceFile, VersionInfo, read_u16, read_u32};

    /// One record of a parsed `.res`, as the tests read it back.
    #[derive(Debug)]
    struct Record {
        type_id: u16,
        name_id: u16,
        language: u16,
        data: Vec<u8>,
    }

    /// Walk a `.res` the way a linker does: header, data, round up, repeat.
    ///
    /// Written as a reader rather than as a byte-for-byte expectation on
    /// purpose. A test that asserted a hex string would pass for a file whose
    /// *lengths* were wrong in a way that cancelled out, which is exactly the
    /// failure this format invites.
    fn records(bytes: &[u8]) -> Vec<Record> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let data_size = read_u32(bytes, at) as usize;
            let header_size = read_u32(bytes, at + 4) as usize;
            assert_eq!(header_size, 32, "every name here is an ordinal");
            assert_eq!(read_u16(bytes, at + 8), 0xFFFF, "type is an ordinal");
            assert_eq!(read_u16(bytes, at + 12), 0xFFFF, "name is an ordinal");
            let start = at + header_size;
            out.push(Record {
                type_id: read_u16(bytes, at + 10),
                name_id: read_u16(bytes, at + 14),
                language: read_u16(bytes, at + 22),
                data: bytes[start..start + data_size].to_vec(),
            });
            at = (start + data_size).next_multiple_of(4);
        }
        out
    }

    /// A minimal `.ico` holding `count` images of one byte each.
    fn ico(count: usize) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&u16::try_from(count).unwrap().to_le_bytes());
        let body = 6 + count * 16;
        for index in 0..count {
            out.push(u8::try_from(16 + index).unwrap()); // width
            out.push(u8::try_from(16 + index).unwrap()); // height
            out.push(0); // colours
            out.push(0); // reserved
            out.extend_from_slice(&1u16.to_le_bytes()); // planes
            out.extend_from_slice(&32u16.to_le_bytes()); // bit count
            out.extend_from_slice(&1u32.to_le_bytes()); // length
            out.extend_from_slice(&u32::try_from(body + index).unwrap().to_le_bytes());
        }
        for index in 0..count {
            out.push(u8::try_from(index).unwrap());
        }
        out
    }

    /// PIN — **the file opens with the null record, and every record after it
    /// starts on a four-byte boundary.**
    ///
    /// Red gate: drop the null record and `link.exe` reads the file as the
    /// 16-bit format it replaced and rejects it; drop the padding and every
    /// record after the first odd-sized one is read from the wrong offset. Both
    /// failures are invisible in the bytes and total at link time.
    #[test]
    fn the_container_opens_with_a_null_record_and_stays_aligned() {
        let mut file = ResourceFile::new();
        file.add_icon(1, &IconGroup::parse(&ico(3)).expect("a three-image icon"));
        file.add_version_info(&VersionInfo::default());
        let bytes = file.finish();

        assert_eq!(bytes.len() % 4, 0);
        let records = records(&bytes);
        let null = &records[0];
        assert_eq!(
            (null.type_id, null.name_id, null.data.len(), null.language),
            (0, 0, 0, 0),
            "the first record is the empty type-zero one that says 32-bit"
        );
        for record in &records[1..] {
            assert_eq!(
                record.language, 0x0409,
                "every real record is filed under one language"
            );
        }
    }

    /// PIN — **an icon becomes n images plus one directory, and the directory
    /// names them by resource id rather than by file offset.**
    ///
    /// Red gate: copy the `.ico`'s directory in unchanged and Explorer draws
    /// nothing — the offsets in it point into a file that no longer exists.
    #[test]
    fn an_icon_becomes_its_images_and_a_directory_that_points_at_them() {
        let icon = IconGroup::parse(&ico(3)).expect("a three-image icon");
        assert_eq!(icon.len(), 3);
        assert!(!icon.is_empty());
        let mut file = ResourceFile::new();
        file.add_icon(1, &icon);
        let records = records(&file.finish());

        let images = records
            .iter()
            .filter(|record| record.type_id == 3)
            .collect::<Vec<_>>();
        assert_eq!(images.len(), 3);
        for (index, image) in images.iter().enumerate() {
            assert_eq!(
                image.name_id,
                u16::try_from(index).unwrap() + 1,
                "images are numbered from one"
            );
            assert_eq!(image.data, vec![u8::try_from(index).unwrap()]);
        }

        let group = records
            .iter()
            .find(|record| record.type_id == 14)
            .expect("a group directory");
        assert_eq!(group.name_id, 1, "the application icon is group one");
        assert_eq!(read_u16(&group.data, 2), 1, "the directory says ICON");
        assert_eq!(read_u16(&group.data, 4), 3, "it counts three images");
        for index in 0..3usize {
            let at = 6 + index * 14;
            assert_eq!(group.data[at], u8::try_from(16 + index).unwrap());
            assert_eq!(read_u32(&group.data, at + 8), 1, "the image's own length");
            assert_eq!(
                read_u16(&group.data, at + 12),
                u16::try_from(index).unwrap() + 1,
                "and the resource it now lives in"
            );
        }
    }

    /// A `VS_VERSIONINFO` block, read back.
    #[derive(Debug)]
    struct Block {
        key: String,
        /// The value as text when the block said its value was text, and `None`
        /// when it was binary or absent.
        text: Option<String>,
        binary: Vec<u8>,
        children: Vec<Block>,
    }

    /// Walk a `VS_VERSIONINFO` block the way `VerQueryValue` walks it: read the
    /// key, round up, read the value, round up, and whatever is left inside
    /// `wLength` is children.
    ///
    /// Written as a reader rather than as a byte-for-byte expectation for the
    /// same reason as [`records`]: the failure this format invites is a length
    /// field that is wrong, and a hex comparison would only ever say *some*
    /// byte moved.
    fn version_block(bytes: &[u8]) -> Block {
        let length = usize::from(read_u16(bytes, 0));
        let value_length = usize::from(read_u16(bytes, 2));
        let is_text = read_u16(bytes, 4) == 1;
        let mut at = 6;
        let mut key = Vec::new();
        loop {
            let unit = read_u16(bytes, at);
            at += 2;
            if unit == 0 {
                break;
            }
            key.push(unit);
        }
        at = at.next_multiple_of(4);
        let value_bytes = if is_text {
            value_length * 2
        } else {
            value_length
        };
        let value = &bytes[at..at + value_bytes];
        let text = is_text.then(|| {
            let units = value
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take_while(|unit| *unit != 0)
                .collect::<Vec<_>>();
            String::from_utf16_lossy(&units)
        });
        at = (at + value_bytes).next_multiple_of(4);
        let mut children = Vec::new();
        while at < length {
            let child_length = usize::from(read_u16(bytes, at));
            assert!(
                child_length > 0,
                "a zero-length child would never terminate"
            );
            children.push(version_block(&bytes[at..at + child_length]));
            at = (at + child_length).next_multiple_of(4);
        }
        Block {
            key: String::from_utf16_lossy(&key),
            text,
            binary: value.to_vec(),
            children,
        }
    }

    /// PIN — **the version block reads back: the four numbers, and every string
    /// the caller asked for.**
    ///
    /// MUTATION: count `wValueLength` for a text value in bytes rather than in
    /// UTF-16 characters, or leave off its terminator, and this fails on the
    /// first string — which is the same bug that ships an executable whose
    /// Details page is blank.
    #[test]
    fn the_version_block_reads_back_as_numbers_and_strings() {
        let info = VersionInfo {
            file_version: FileVersion([0, 1, 0, 0]),
            product_version: FileVersion([0, 1, 0, 0]),
            strings: vec![
                ("ProductName".to_owned(), "Folio".to_owned()),
                ("FileVersion".to_owned(), "0.1.0".to_owned()),
            ],
        };
        let mut file = ResourceFile::new();
        file.add_version_info(&info);
        let records = records(&file.finish());
        let version = records
            .iter()
            .find(|record| record.type_id == 16)
            .expect("an RT_VERSION record");
        assert_eq!(version.name_id, 1);

        let root = version_block(&version.data);
        assert_eq!(root.key, "VS_VERSION_INFO");
        let fixed = &root.binary;
        assert_eq!(fixed.len(), 52, "VS_FIXEDFILEINFO is thirteen words");
        assert_eq!(read_u32(fixed, 0), 0xFEEF_04BD, "the signature");
        assert_eq!(read_u32(fixed, 8), 0x0000_0001, "file version major.minor");
        assert_eq!(read_u32(fixed, 12), 0x0000_0000, "and patch.build");
        assert_eq!(read_u32(fixed, 16), 0x0000_0001, "product version too");
        assert_eq!(read_u32(fixed, 36), 1, "dwFileType is VFT_APP");

        let names = root
            .children
            .iter()
            .map(|child| child.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["StringFileInfo", "VarFileInfo"]);

        let table = &root.children[0].children[0];
        assert_eq!(table.key, "040904b0", "language and code page, as one key");
        let strings = table
            .children
            .iter()
            .map(|child| (child.key.as_str(), child.text.clone().unwrap_or_default()))
            .collect::<Vec<_>>();
        assert_eq!(
            strings,
            [
                ("ProductName", "Folio".to_owned()),
                ("FileVersion", "0.1.0".to_owned()),
            ]
        );

        let var = &root.children[1].children[0];
        assert_eq!(var.key, "Translation");
        assert_eq!(
            var.binary,
            [0x09, 0x04, 0xB0, 0x04],
            "en-US and the Unicode code page, which is what the table is named"
        );
    }

    /// PIN — **a Cargo version becomes the four numbers Windows stores, and
    /// anything that is not three fields is refused rather than guessed at.**
    #[test]
    fn a_cargo_version_becomes_four_numbers() {
        assert_eq!(
            FileVersion::parse_semver("0.1.0").expect("three fields"),
            FileVersion([0, 1, 0, 0])
        );
        assert_eq!(
            FileVersion::parse_semver("12.34.56").expect("three fields"),
            FileVersion([12, 34, 56, 0])
        );
        assert_eq!(
            FileVersion::parse_semver("1.2.3-rc.1").expect("a pre-release still has three numbers"),
            FileVersion([1, 2, 3, 0]),
            "the suffix has no field to go in, and survives in the string instead"
        );
        for bad in ["0.1", "0.1.0.0", "", "x.y.z", "0.1.z"] {
            assert!(
                FileVersion::parse_semver(bad).is_err(),
                "{bad} is not a version this can pack"
            );
        }
    }

    /// PIN — **bytes that are not an icon are refused, and never read past.**
    #[test]
    fn only_an_icon_parses_as_one() {
        for bad in [
            vec![],
            vec![0, 0, 1, 0],       // truncated header
            vec![0, 0, 2, 0, 1, 0], // a cursor, not an icon
            vec![0, 0, 1, 0, 0, 0], // no images
            vec![0, 0, 1, 0, 1, 0], // a directory that is not there
        ] {
            assert!(IconGroup::parse(&bad).is_err(), "{bad:?}");
        }
        let mut past_the_end = ico(1);
        past_the_end[6 + 12] = 0xFF; // an offset far outside the file
        assert!(
            IconGroup::parse(&past_the_end).is_err(),
            "an image that lies outside the file is refused rather than read"
        );
        let mut into_the_directory = ico(1);
        into_the_directory[6 + 12] = 0; // an offset back inside the header
        assert!(
            IconGroup::parse(&into_the_directory).is_err(),
            "and so is one that points back at the directory"
        );
    }
}
