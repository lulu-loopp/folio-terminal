//! The archive reader over archives built here, in `package.ps1`'s layout
//! (0.4.6 ticket U-14).
//!
//! **The manifest seam.** The product reads the manifest out of the staged
//! `folio.exe` through [`EmbeddedManifest`]. A test cannot build an executable
//! carrying the manifest of an archive it made — the only executables it can
//! reach carry the manifest of the build that compiled them — so these tests
//! hand the reader a [`Given`] manifest source: the text the test built, and a
//! record of the path it was asked about, which each test holds to the staged
//! executable's bytes. The door itself is held on the real `folio.exe` this
//! workspace builds, by
//! [`the_manifest_is_read_out_of_a_built_folio_exe_without_running_it`], which
//! also runs a whole archive of this build's own members through both doors.

use super::{
    ARCHIVE_MAX_BYTES, ARCHIVE_MAX_FILES, Deadline, EmbeddedManifest, Expanded, Expected, Inflate,
    Inflated, ManifestSource, Miniz, NAME_MAX_BYTES, NameRefusal, Reason, Refusal, expand,
    expand_from, member_name,
};
use bt_platform::exclusive_create::Directory;
use bt_winres::digest::{hex, sha256};
use bt_winres::release_manifest::{
    MEMBER_LIST, MIN_UPDATER, Manifest, Member, PROTOCOL, Source, parse_member_list,
};
use bt_winres::zip::crc32;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

const VERSION: &str = "0.4.6";
const ROOT: &str = "folio-0.4.6";

fn expected() -> Expected<'static> {
    Expected {
        version: VERSION,
        arch: "x64",
        updater: MIN_UPDATER,
    }
}

/// One entry of an archive under construction, with every field a test may
/// make lie.
#[derive(Clone)]
struct Item {
    entry: String,
    data: Vec<u8>,
    method: u16,
    /// The uncompressed size both headers declare; the data's length unless a
    /// test says otherwise.
    declared: Option<u32>,
    /// The CRC both headers declare; the data's unless a test says otherwise.
    crc: Option<u32>,
    /// The local header's name, when it is not the central one.
    local_name: Option<String>,
    made_by: u16,
    external: u32,
}

fn item(name: &str, data: &[u8]) -> Item {
    Item {
        entry: format!("{ROOT}/{name}"),
        data: data.to_vec(),
        // `ZipFile.CreateFromDirectory` stores an empty file and deflates the
        // rest.
        method: if data.is_empty() { 0 } else { 8 },
        declared: None,
        crc: None,
        local_name: None,
        made_by: 0x0014,
        external: 0,
    }
}

/// **An archive in the layout `package.ps1`'s `CreateFromDirectory` writes
/// under pwsh 7**: local header and data for each item in order, then the
/// central directory, then the end record; no data descriptors, no extra
/// fields, no comment, forward slashes, version 2.0 made on MS-DOS
/// (`0x0014`), as read back from a real one (report U-14).
fn zip_of(items: &[Item]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for item in items {
        let compressed = if item.method == 8 {
            miniz_oxide::deflate::compress_to_vec(&item.data, 6)
        } else {
            item.data.clone()
        };
        let size = item.declared.unwrap_or(item.data.len() as u32);
        let crc = item.crc.unwrap_or_else(|| crc32(&item.data));
        let local_name = item.local_name.as_deref().unwrap_or(&item.entry);
        let offset = out.len() as u32;

        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&item.method.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(local_name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(local_name.as_bytes());
        out.extend_from_slice(&compressed);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&item.made_by.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&item.method.to_le_bytes());
        central.extend_from_slice(&[0; 4]);
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(item.entry.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&item.external.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(item.entry.as_bytes());
    }
    let directory_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(items.len() as u16).to_le_bytes());
    out.extend_from_slice(&(items.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&directory_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// The members `scripts/release/archive-members.txt` lists, in its order.
fn listed() -> Vec<(String, Source)> {
    parse_member_list(include_str!("../../../scripts/release/archive-members.txt"))
        .expect("the member list parses")
        .into_iter()
        .map(|item| (item.name, item.source))
        .collect()
}

/// Synthetic bytes for a member: text for the text members, a few kilobytes
/// of patterned bytes for the rest, so every one of them deflates.
fn bytes_of(name: &str) -> Vec<u8> {
    if name.ends_with(".cmd") || name.ends_with(".md") || name.starts_with("LICENSE") {
        format!("@rem {name} of the test release\r\n")
            .repeat(40)
            .into_bytes()
    } else {
        (0..4096u32)
            .map(|i| (i.wrapping_mul(31) ^ name.len() as u32) as u8)
            .collect()
    }
}

/// **A release in `package.ps1`'s layout**: every member of the one list,
/// with synthetic bytes, and the manifest `build.rs` would embed for them.
fn release() -> (Vec<Item>, Manifest) {
    let mut items = Vec::new();
    let mut members = Vec::new();
    for (name, source) in listed() {
        let data = bytes_of(&name);
        if source.in_manifest() {
            members.push(Member {
                name: name.clone(),
                sha256: hex(&sha256(&data)),
                size: data.len() as u64,
            });
        }
        items.push(item(&name, &data));
    }
    let manifest = Manifest {
        product: "folio".to_owned(),
        version: VERSION.to_owned(),
        arch: "x64".to_owned(),
        archive_root: ROOT.to_owned(),
        protocol: PROTOCOL,
        min_updater: MIN_UPDATER.to_owned(),
        members,
    };
    (items, manifest)
}

fn named<'a>(items: &'a mut [Item], name: &str) -> &'a mut Item {
    items
        .iter_mut()
        .find(|item| item.entry == format!("{ROOT}/{name}"))
        .unwrap_or_else(|| panic!("{name} is in the release"))
}

/// **The test's manifest source**: the text it was given, and every path it
/// was asked about with the bytes that were there.
struct Given {
    text: String,
    asked: RefCell<Vec<(PathBuf, Vec<u8>)>>,
}

impl Given {
    fn new(manifest: &Manifest) -> Self {
        Self {
            text: manifest.encode(),
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl ManifestSource for Given {
    fn manifest_text(&self, executable: &Path) -> Result<String, String> {
        let bytes = std::fs::read(executable).map_err(|error| error.to_string())?;
        self.asked
            .borrow_mut()
            .push((executable.to_path_buf(), bytes));
        Ok(self.text.clone())
    }
}

/// A fresh folder with an empty `staging` in it.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "bt-app-update-archive-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("staging")).unwrap();
    root
}

fn far() -> Instant {
    Instant::now() + Duration::from_secs(600)
}

/// Write `zip` beside the staging folder and read it through [`expand`] — the
/// product's own open on `file_reads`' lane, its own inflater and clock.
fn run(root: &Path, zip: &[u8], source: &dyn ManifestSource) -> Result<Expanded, Refusal> {
    let archive = root.join("release.zip");
    std::fs::write(&archive, zip).unwrap();
    let staging = Directory::open(&root.join("staging")).unwrap();
    expand(
        &archive,
        &expected(),
        &staging,
        source,
        &Deadline::at(far()),
    )
}

/// The names in the staging folder, sorted, `~` temporaries included.
fn staged(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("staging"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// No member of the release stands under its own name.
fn nothing_final(root: &Path) -> bool {
    staged(root).iter().all(|name| name.starts_with('~'))
}

fn refusal(member: &str, reason: Reason) -> Refusal {
    Refusal {
        member: Some(format!("{ROOT}/{member}")),
        reason,
    }
}

/// RED (U-14) — **a release packed the way `package.ps1` packs one is
/// accepted: every member of the one list lands in the staging folder under
/// its own name with its own bytes, and the manifest was read from the staged
/// `folio.exe`.**
///
/// The fixture is built from `scripts/release/archive-members.txt` — the list
/// `package.ps1` packs from and `build.rs` builds the manifest from — in the
/// layout `ZipFile.CreateFromDirectory(…, $true)` writes under pwsh 7: one
/// root `folio-<version>/`, forward slashes, deflate for every non-empty file,
/// no directory entry for the root. A second archive adds the root's own
/// directory entry, which F-12 allows once and does not count, and an empty
/// stored member is expanded as well as a deflated one.
///
/// MUTATION: skip the final rename loop in `expand_from` and the members stay
/// under their `~` names.
#[test]
fn packaged_zip_root_is_accepted() {
    let root = scratch("accepted");
    let (items, manifest) = release();
    let given = Given::new(&manifest);
    let expanded = run(&root, &zip_of(&items), &given).expect("the packaged release is accepted");

    let names: Vec<String> = listed().into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        expanded.members, names,
        "every member, in the archive's order"
    );
    assert_eq!(expanded.manifest, manifest);
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        staged(&root),
        sorted,
        "each under its own name, and nothing else"
    );
    for item in &items {
        let name = item.entry.strip_prefix(&format!("{ROOT}/")).unwrap();
        assert_eq!(
            std::fs::read(root.join("staging").join(name)).unwrap(),
            item.data,
            "{name}"
        );
    }
    let asked = given.asked.borrow();
    assert_eq!(asked.len(), 1, "the manifest is read once");
    assert_eq!(asked[0].0, root.join("staging").join("~folio.exe"));
    assert_eq!(
        asked[0].1,
        bytes_of("folio.exe"),
        "from the staged executable"
    );
    drop(asked);

    // The root's own directory entry, once, and an empty member.
    let root = scratch("accepted-with-root");
    let (mut items, mut manifest) = release();
    let mut folder = item("", b"");
    folder.external = 0x10;
    items.insert(0, folder);
    items.push(item("empty.txt", b""));
    manifest.members.push(Member {
        name: "empty.txt".to_owned(),
        sha256: hex(&sha256(b"")),
        size: 0,
    });
    let expanded = run(&root, &zip_of(&items), &Given::new(&manifest))
        .expect("the root's directory entry is allowed once");
    assert_eq!(
        expanded.members.last().map(String::as_str),
        Some("empty.txt")
    );
    assert_eq!(
        std::fs::read(root.join("staging").join("empty.txt")).unwrap(),
        b""
    );
}

/// RED (U-14) — **one changed byte in `uninstall.cmd` is refused by name, and
/// nothing of the release stands under its own name.**
///
/// The review's counterexample for F-4: `package.ps1` signs only `folio.exe`
/// and `folio.msix`, so before the manifest a changed `uninstall.cmd` passed
/// every check a release made. Here the archive is internally consistent —
/// the same size, the CRC recomputed — and only the manifest `folio.exe`
/// carries tells the bytes apart.
///
/// MUTATION: skip the SHA-256 comparison after `expand_member` and the changed
/// member is staged.
#[test]
fn a_changed_cmd_member_is_refused() {
    let root = scratch("changed-cmd");
    let (mut items, manifest) = release();
    let cmd = named(&mut items, "uninstall.cmd");
    let middle = cmd.data.len() / 2;
    cmd.data[middle] ^= 0x20;
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("changed");
    assert_eq!(refused, refusal("uninstall.cmd", Reason::Digest));
    assert!(
        refused
            .to_string()
            .starts_with("folio-0.4.6/uninstall.cmd: "),
        "the refusal names the member: {refused}"
    );
    assert!(nothing_final(&root), "{:?}", staged(&root));
}

/// RED (U-14) — **a trailing dot or space, a device name, a stream separator,
/// a separator variant and every other name outside the grammar are refused,
/// each as its own class; and such a name in an archive is refused before a
/// byte is written.**
///
/// F-12: with no allowlist of names, the Windows namespace has to be the
/// grammar. `uninstall.cmd.` and `uninstall.cmd ` both land as
/// `uninstall.cmd`; `NUL.txt` and `com1.dll` open devices; `a:b` writes a
/// stream of `a`; `~` spells 8.3 aliases (`FOLIO~1.EXE`).
///
/// MUTATION: drop the device-name check in `member_name` and the `NUL.txt`
/// archive is expanded.
#[test]
fn trailing_dot_space_and_device_names_are_refused() {
    let long = "a".repeat(NAME_MAX_BYTES + 1);
    let table: [(&str, NameRefusal); 22] = [
        ("", NameRefusal::Empty),
        (&long, NameRefusal::TooLong(NAME_MAX_BYTES + 1)),
        ("uninstall.cmd.", NameRefusal::TrailingDotOrSpace),
        ("uninstall.cmd ", NameRefusal::TrailingDotOrSpace),
        ("..", NameRefusal::TrailingDotOrSpace),
        ("CON", NameRefusal::Device),
        ("con.txt", NameRefusal::Device),
        ("NUL.txt", NameRefusal::Device),
        ("Aux.md", NameRefusal::Device),
        ("prn", NameRefusal::Device),
        ("com1.dll", NameRefusal::Device),
        ("LPT9", NameRefusal::Device),
        ("COM0.cmd", NameRefusal::Device),
        ("conpty.dll:evil", NameRefusal::Colon),
        ("sub/uninstall.cmd", NameRefusal::Separator),
        ("sub\\uninstall.cmd", NameRefusal::Separator),
        ("FOLIO~1.EXE", NameRefusal::Character('~')),
        ("two words.md", NameRefusal::Character(' ')),
        ("notice\u{e9}.md", NameRefusal::Character('\u{e9}')),
        ("a*b", NameRefusal::Character('*')),
        (".hidden", NameRefusal::DotName),
        ("folio-install.json", NameRefusal::Reserved),
    ];
    for (name, class) in table {
        assert_eq!(member_name(name), Err(class), "{name:?}");
    }
    for fine in [
        "folio.exe",
        "THIRD-PARTY-NOTICES.md",
        "vcruntime140_1.dll",
        "CONSOLE.dll",
        "com10.dll",
        "nul-free.txt",
        &"a".repeat(NAME_MAX_BYTES),
    ] {
        assert!(member_name(fine).is_ok(), "{fine:?}");
    }

    for (name, class) in [
        ("NUL.txt", NameRefusal::Device),
        ("uninstall.cmd.", NameRefusal::TrailingDotOrSpace),
        ("uninstall.cmd ", NameRefusal::TrailingDotOrSpace),
    ] {
        let root = scratch("bad-name");
        let (mut items, manifest) = release();
        items.push(item(name, b"x"));
        let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err(name);
        assert_eq!(refused, refusal(name, Reason::Name(class)), "{name:?}");
        assert!(staged(&root).is_empty(), "{name:?}: nothing written");
    }
}

/// A decompressor that records what it produced, one slot per member.
struct Counting {
    inner: Miniz,
    produced: Rc<RefCell<Vec<usize>>>,
    slot: usize,
}

impl Inflate for Counting {
    fn inflate(&mut self, input: &[u8], output: &mut [u8]) -> Result<Inflated, String> {
        let step = self.inner.inflate(input, output)?;
        self.produced.borrow_mut()[self.slot] += step.produced;
        Ok(step)
    }
}

/// RED (U-14) — **a member whose headers and manifest claim a small size but
/// whose stream inflates larger is stopped at that size and refused; the
/// decompressor is never given room for more than one byte past it.**
///
/// A zip bomb is a small declared size over a large stream. The reader counts
/// the decompressor's own output, not the header's word for it, and sizes the
/// output buffer so the decompressor cannot run past the member's size by more
/// than the one byte that shows it trying: 1,000 bytes declared everywhere,
/// 5,000 in the stream, and at most 1,001 ever produced and 1,000 written.
///
/// MUTATION: give the inflater the whole chunk (`CHUNK_BYTES`) instead of
/// `size - produced + 1` and it produces all 5,000 bytes.
#[test]
fn a_lying_uncompressed_size_is_stopped_at_the_cap() {
    let root = scratch("lying-size");
    let (mut items, mut manifest) = release();
    let payload: Vec<u8> = (0..5000u32).map(|i| (i % 97) as u8).collect();
    let cmd = named(&mut items, "uninstall.cmd");
    cmd.data = payload.clone();
    cmd.declared = Some(1000);
    cmd.crc = Some(crc32(&payload[..1000]));
    let listed = manifest
        .members
        .iter_mut()
        .find(|member| member.name == "uninstall.cmd")
        .unwrap();
    listed.size = 1000;
    listed.sha256 = hex(&sha256(&payload[..1000]));

    let produced = Rc::new(RefCell::new(Vec::new()));
    let zip = zip_of(&items);
    let staging = Directory::open(&root.join("staging")).unwrap();
    let mut counting = || -> Box<dyn Inflate> {
        produced.borrow_mut().push(0);
        Box::new(Counting {
            inner: Miniz::new(),
            produced: Rc::clone(&produced),
            slot: produced.borrow().len() - 1,
        })
    };
    let refused = expand_from(
        std::io::Cursor::new(zip),
        &expected(),
        &staging,
        &Given::new(&manifest),
        &Deadline::at(far()),
        &mut counting,
    )
    .expect_err("the stream passes its size");
    assert_eq!(
        refused,
        refusal("uninstall.cmd", Reason::Oversize { bound: 1000 })
    );
    let for_cmd = *produced.borrow().last().unwrap();
    assert!(for_cmd <= 1001, "the decompressor produced {for_cmd} bytes");
    let written = std::fs::metadata(root.join("staging").join("~uninstall.cmd"))
        .unwrap()
        .len();
    assert!(written <= 1000, "{written} bytes written");
    assert!(nothing_final(&root), "{:?}", staged(&root));
}

/// RED (U-14) — **the four reserved names are never payload, spelled in any
/// case.**
///
/// F-12: `folio-install.json`, `install.json`, `manifest.json` and
/// `.folio-update` are what the updater reads to decide whether this copy may
/// update at all (the install marker, scoop's receipt, the transaction's
/// home). An archive that carried one would be writing that evidence.
///
/// MUTATION: drop the `RESERVED` check in `member_name` and `install.json` is
/// refused only as unlisted, after `folio.exe` was expanded.
#[test]
fn a_reserved_name_is_never_payload() {
    for name in [
        "folio-install.json",
        "install.json",
        "Manifest.JSON",
        ".folio-update",
    ] {
        assert_eq!(member_name(name), Err(NameRefusal::Reserved), "{name}");
        let root = scratch("reserved");
        let (mut items, manifest) = release();
        items.push(item(name, b"{}"));
        let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err(name);
        assert_eq!(refused, refusal(name, Reason::Name(NameRefusal::Reserved)));
        assert!(staged(&root).is_empty(), "{name}: nothing written");
    }
}

/// RED (U-14) — **an entry whose local header names something other than its
/// central entry is refused**, before anything is written.
///
/// A reader that trusts the central directory and one that walks the local
/// headers would expand two different archives from the same bytes (F-12:
/// "local and central ZIP names must agree"); the other fields are held the
/// same way.
///
/// MUTATION: drop the name comparison in `local_header` and the archive is
/// accepted with `uninstall.cmd`'s central name.
#[test]
fn a_local_name_that_disagrees_with_the_central_one_is_refused() {
    let root = scratch("local-name");
    let (mut items, manifest) = release();
    named(&mut items, "uninstall.cmd").local_name = Some(format!("{ROOT}/uninstall.bat"));
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("disagrees");
    assert_eq!(
        refused,
        refusal("uninstall.cmd", Reason::LocalDisagrees("name"))
    );
    assert!(staged(&root).is_empty());

    let root = scratch("local-size");
    let (items, manifest) = release();
    let mut zip = zip_of(&items);
    // The local header's size field of the first entry, `folio.exe`.
    zip[22] ^= 1;
    let refused = run(&root, &zip, &Given::new(&manifest)).expect_err("disagrees");
    assert_eq!(
        refused,
        refusal("folio.exe", Reason::LocalDisagrees("size"))
    );
    assert!(staged(&root).is_empty());
}

/// RED (U-14) — **a member the manifest does not list is refused by name**
/// — an added `version.dll` beside `folio.exe` is exactly what an unsigned
/// `SHA256SUMS.txt` let through (F-4) — and nothing is expanded but the
/// executable the manifest came from.
///
/// MUTATION: skip the unlisted check and `version.dll` is staged.
#[test]
fn an_unlisted_member_is_refused() {
    let root = scratch("unlisted");
    let (mut items, manifest) = release();
    items.push(item("version.dll", b"MZ not ours"));
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("unlisted");
    assert_eq!(refused, refusal("version.dll", Reason::Unlisted));
    assert_eq!(staged(&root), ["~folio.exe"]);
}

/// RED (U-14) — **a member the manifest lists and the archive lacks is
/// refused by name**, and so is one whose declared size is not the listed
/// one, before any member is expanded.
///
/// MUTATION: skip the missing check and the release without `TRADEMARK.md`
/// is accepted.
#[test]
fn a_missing_member_is_refused() {
    let root = scratch("missing");
    let (mut items, manifest) = release();
    items.retain(|item| item.entry != format!("{ROOT}/TRADEMARK.md"));
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("missing");
    assert_eq!(refused, refusal("TRADEMARK.md", Reason::Missing));
    assert_eq!(staged(&root), ["~folio.exe"]);

    let root = scratch("resized");
    let (mut items, manifest) = release();
    let notice = named(&mut items, "LICENSE-MIT");
    notice.data.push(b'\n');
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("resized");
    let listed = bytes_of("LICENSE-MIT").len() as u64;
    assert_eq!(
        refused,
        refusal(
            "LICENSE-MIT",
            Reason::SizeDiffers {
                listed,
                archive: listed + 1
            }
        )
    );
    assert_eq!(staged(&root), ["~folio.exe"]);
}

/// A decompressor that takes a second of a fake clock per call and produces
/// one byte, forever.
struct Slow {
    clock: Rc<Cell<Instant>>,
}

impl Inflate for Slow {
    fn inflate(&mut self, _: &[u8], output: &mut [u8]) -> Result<Inflated, String> {
        self.clock.set(self.clock.get() + Duration::from_secs(1));
        output[0] = 0;
        Ok(Inflated {
            consumed: 0,
            produced: 1,
            done: false,
        })
    }
}

/// RED (U-14) — **the deadline ends a slow expansion inside the member, not
/// after it.**
///
/// F-12's time bound through expansion: a stream can be slow to inflate as
/// well as large, and a deadline checked only between members would wait for
/// the member. The fake decompressor costs a second of a fake clock per call;
/// with thirty seconds left the reader stops within one call of the deadline,
/// in `folio.exe`, the first member it expands.
///
/// MUTATION: check the deadline only at the start of `expand_member` and the
/// expansion runs until the member's size is reached.
#[test]
fn the_time_bound_fires_on_a_slow_inflater() {
    let root = scratch("slow");
    let (items, manifest) = release();
    let start = Instant::now();
    let clock = Rc::new(Cell::new(start));
    let now = {
        let clock = Rc::clone(&clock);
        move || clock.get()
    };
    let deadline = Deadline::on(start + Duration::from_secs(30), &now);
    let staging = Directory::open(&root.join("staging")).unwrap();
    let refused = expand_from(
        std::io::Cursor::new(zip_of(&items)),
        &expected(),
        &staging,
        &Given::new(&manifest),
        &deadline,
        &mut || -> Box<dyn Inflate> {
            Box::new(Slow {
                clock: Rc::clone(&clock),
            })
        },
    )
    .expect_err("the deadline passes");
    assert_eq!(refused, refusal("folio.exe", Reason::Deadline));
    let spent = clock.get() - start;
    assert!(
        spent <= Duration::from_secs(31),
        "stopped {spent:?} after the start"
    );
    assert!(nothing_final(&root));
}

/// RED (U-14) — **duplicates by case, aliases, streams, links, folders and
/// traversal are refused, each by name, before a byte is written** (§F's
/// `duplicate_case_alias_stream_link_and_traversal_entries_are_refused`).
///
/// MUTATION: compare names with case in the duplicate check and `CONPTY.DLL`
/// beside `conpty.dll` is accepted as a second member.
#[test]
fn duplicate_case_alias_stream_link_and_traversal_entries_are_refused() {
    let unix_link = {
        let mut link = item("conpty-link.dll", b"conpty.dll");
        link.made_by = 0x031E;
        link.external = 0o120_777 << 16;
        link
    };
    let reparse = {
        let mut reparse = item("reparse.dll", b"x");
        reparse.external = 0x400;
        reparse
    };
    let folder = {
        let mut folder = item("sub/", b"");
        folder.external = 0x10;
        folder
    };
    let mut outside = item("x", b"x");
    outside.entry = "folio-0.4.5/evil.dll".to_owned();
    let mut traversal = item("x", b"x");
    traversal.entry = format!("{ROOT}/../evil.dll");
    let mut absolute = item("x", b"x");
    absolute.entry = "C:/Windows/evil.dll".to_owned();
    let mut backslash = item("x", b"x");
    backslash.entry = format!("{ROOT}\\conpty.dll");
    let mut root_twice = item("", b"");
    root_twice.entry = format!("{ROOT}/");

    let cases: Vec<(Item, String, Reason)> = vec![
        (
            item("CONPTY.DLL", b"x"),
            format!("{ROOT}/CONPTY.DLL"),
            Reason::Duplicate,
        ),
        (
            item("FOLIO~1.EXE", b"x"),
            format!("{ROOT}/FOLIO~1.EXE"),
            Reason::Name(NameRefusal::Character('~')),
        ),
        (
            item("conpty.dll:evil", b"x"),
            format!("{ROOT}/conpty.dll:evil"),
            Reason::Name(NameRefusal::Colon),
        ),
        (unix_link, format!("{ROOT}/conpty-link.dll"), Reason::Link),
        (reparse, format!("{ROOT}/reparse.dll"), Reason::Link),
        (
            folder,
            format!("{ROOT}/sub/"),
            Reason::Name(NameRefusal::Separator),
        ),
        (
            outside,
            "folio-0.4.5/evil.dll".to_owned(),
            Reason::OutsideRoot,
        ),
        (
            traversal,
            format!("{ROOT}/../evil.dll"),
            Reason::Name(NameRefusal::Separator),
        ),
        (
            absolute,
            "C:/Windows/evil.dll".to_owned(),
            Reason::OutsideRoot,
        ),
        (
            backslash,
            format!("{ROOT}\\conpty.dll"),
            Reason::Name(NameRefusal::Separator),
        ),
    ];
    for (extra, entry, reason) in cases {
        let root = scratch("aliases");
        let (mut items, manifest) = release();
        items.push(extra);
        let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err(&entry);
        assert_eq!(
            refused,
            Refusal {
                member: Some(entry.clone()),
                reason
            },
            "{entry}"
        );
        assert!(staged(&root).is_empty(), "{entry}: nothing written");
    }

    // The root's own entry, twice.
    let root = scratch("root-twice");
    let (mut items, manifest) = release();
    let mut first = item("", b"");
    first.entry = format!("{ROOT}/");
    items.insert(0, first);
    items.push(root_twice);
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("twice");
    assert_eq!(refused.reason, Reason::RootTwice);
}

/// RED (U-14) — **the bounds are refused before the disk is touched**: sizes
/// declared past [`ARCHIVE_MAX_BYTES`], and more than [`ARCHIVE_MAX_FILES`]
/// files (§F's `expanded_size_limit_precedes_disk_exhaustion`).
///
/// MUTATION: drop the declared-total check in `directory` and the archive is
/// refused only when the first member overflows, after `~folio.exe` exists.
#[test]
fn expanded_size_limit_precedes_disk_exhaustion() {
    let root = scratch("too-large");
    let (mut items, manifest) = release();
    let huge = u32::try_from(ARCHIVE_MAX_BYTES).unwrap();
    named(&mut items, "LICENSE-APACHE").declared = Some(huge);
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("too large");
    assert!(
        matches!(refused.reason, Reason::TooLarge(bytes) if bytes > ARCHIVE_MAX_BYTES),
        "{refused}"
    );
    assert!(staged(&root).is_empty());

    let root = scratch("too-many");
    let (mut items, manifest) = release();
    let already = items.len();
    for index in 0..=(ARCHIVE_MAX_FILES - already) {
        items.push(item(&format!("extra{index}.dll"), b"x"));
    }
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("too many");
    assert_eq!(refused.reason, Reason::TooManyFiles(ARCHIVE_MAX_FILES + 1));
    assert!(staged(&root).is_empty());
}

/// RED (U-14) — **a manifest for another product, version, architecture or
/// root, of another protocol, or needing a newer updater, is refused naming
/// `folio.exe` and the field.**
///
/// F-4: the manifest's header is held to the offer, so a validly signed
/// `folio.exe` of another release cannot vouch for this archive; F-12: a
/// release whose layout this reader cannot take says so with `min_updater`.
///
/// MUTATION: skip the `arch` comparison in `offered` and the arm64 manifest
/// is accepted.
#[test]
fn a_manifest_for_another_release_or_a_newer_updater_is_refused() {
    let (items, manifest) = release();
    let zip = zip_of(&items);
    let changed = |change: &dyn Fn(&mut Manifest)| {
        let mut other = manifest.clone();
        change(&mut other);
        other
    };
    let cases: [(Manifest, Reason); 6] = [
        (
            changed(&|m| m.arch = "arm64".to_owned()),
            Reason::ManifestSays {
                field: "arch",
                found: "arm64".to_owned(),
                expected: "x64".to_owned(),
            },
        ),
        (
            changed(&|m| m.version = "0.4.7".to_owned()),
            Reason::ManifestSays {
                field: "version",
                found: "0.4.7".to_owned(),
                expected: VERSION.to_owned(),
            },
        ),
        (
            changed(&|m| m.product = "other".to_owned()),
            Reason::ManifestSays {
                field: "product",
                found: "other".to_owned(),
                expected: "folio".to_owned(),
            },
        ),
        (
            changed(&|m| m.archive_root = "folio-0.4.7".to_owned()),
            Reason::ManifestSays {
                field: "archive_root",
                found: "folio-0.4.7".to_owned(),
                expected: ROOT.to_owned(),
            },
        ),
        (
            changed(&|m| m.protocol = PROTOCOL + 1),
            Reason::Protocol(PROTOCOL + 1),
        ),
        (
            changed(&|m| m.min_updater = "9.0.0".to_owned()),
            Reason::UpdaterTooOld {
                needs: "9.0.0".to_owned(),
            },
        ),
    ];
    for (other, reason) in cases {
        let root = scratch("offer");
        let refused = run(&root, &zip, &Given::new(&other)).expect_err("another release");
        assert_eq!(refused, refusal("folio.exe", reason));
        assert_eq!(staged(&root), ["~folio.exe"]);
    }

    let root = scratch("unparsed");
    let broken = Given {
        text: manifest.encode().replace("members 8\n", "members 9\n"),
        asked: RefCell::new(Vec::new()),
    };
    let refused = run(&root, &zip, &broken).expect_err("a truncated manifest");
    assert!(
        matches!(&refused.reason, Reason::Manifest(detail) if detail.contains("truncated")),
        "{refused}"
    );
}

/// RED (U-14) — **a damaged member is refused: a stream cut short, bytes after
/// its end, and a CRC that is not the archive's** — including `folio.msix`,
/// which the manifest does not list.
///
/// MUTATION: skip the CRC comparison in `Sink::finish` and the changed
/// `folio.msix` is staged.
#[test]
fn a_damaged_member_is_refused_by_its_end_or_its_crc() {
    let root = scratch("crc");
    let (mut items, manifest) = release();
    let msix = named(&mut items, "folio.msix");
    msix.crc = Some(crc32(&msix.data) ^ 1);
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("crc");
    assert_eq!(refused, refusal("folio.msix", Reason::Crc));
    assert!(nothing_final(&root));

    let root = scratch("short");
    let (mut items, manifest) = release();
    let msix = named(&mut items, "folio.msix");
    let data = msix.data.clone();
    msix.data.extend_from_slice(&data);
    msix.declared = Some(u32::try_from(data.len() * 2 + 10).unwrap());
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("short");
    assert!(
        matches!(refused.reason, Reason::Truncated { produced, .. } if produced == data.len() as u64 * 2),
        "{refused}"
    );
    assert!(nothing_final(&root));
}

/// RED (U-14) — **a member's name that already exists in the staging folder is
/// never replaced**: the expansion is refused at the rename, the existing file
/// is untouched, and so is an existing temporary name.
///
/// MUTATION: rename with replacement in `exclusive_create::rename_new` and the
/// existing `TRADEMARK.md` is overwritten.
#[test]
fn an_existing_name_in_staging_is_never_replaced() {
    let root = scratch("existing");
    std::fs::write(root.join("staging").join("TRADEMARK.md"), b"there first").unwrap();
    let (items, manifest) = release();
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("existing");
    assert_eq!(refused.member.as_deref(), Some("folio-0.4.6/TRADEMARK.md"));
    assert!(matches!(refused.reason, Reason::Write(_)), "{refused}");
    assert_eq!(
        std::fs::read(root.join("staging").join("TRADEMARK.md")).unwrap(),
        b"there first"
    );
    let finals: Vec<String> = staged(&root)
        .into_iter()
        .filter(|name| !name.starts_with('~'))
        .collect();
    assert_eq!(
        finals,
        ["TRADEMARK.md"],
        "the members renamed before it went back"
    );

    let root = scratch("existing-temporary");
    std::fs::write(root.join("staging").join("~folio.exe"), b"there first").unwrap();
    let refused = run(&root, &zip_of(&items), &Given::new(&manifest)).expect_err("existing");
    assert_eq!(refused.member.as_deref(), Some("folio-0.4.6/folio.exe"));
    assert!(matches!(refused.reason, Reason::Write(_)), "{refused}");
    assert_eq!(
        std::fs::read(root.join("staging").join("~folio.exe")).unwrap(),
        b"there first"
    );
}

/// RED (U-14) — **the manifest is read out of the `folio.exe` this workspace
/// builds, as a data file, and is the manifest `build.rs` embedded; and an
/// archive of this build's own members goes through both doors whole.**
///
/// E-14's experiment, run on every Windows test job: `LoadLibraryExW` with
/// `LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE` maps the built
/// executable, which is never started, and `FindResourceW` finds
/// `FOLIO_RELEASE_MANIFEST`. Then the real producer end to end: the members
/// `archive-members.txt` lists, from where `build.rs` hashed them (the
/// executable and the ConPTY files beside it, the checkout's `packaging/` and
/// root), packed in `package.ps1`'s layout, expanded through
/// [`EmbeddedManifest`] and the exclusive-create door. `folio.msix` is not in
/// a debug build; synthetic bytes stand in, which the manifest does not list.
/// Off Windows no build carries a manifest and the door has no loader.
///
/// MUTATION: read the resource with `FindResourceW(…, RT_RCDATA)` under
/// another name and the door answers `NotFound`.
#[test]
fn the_manifest_is_read_out_of_a_built_folio_exe_without_running_it() {
    let this = std::env::current_exe().unwrap();
    let built = this.parent().and_then(Path::parent).unwrap().to_path_buf();
    let Some(path) = option_env!("FOLIO_RELEASE_MANIFEST") else {
        assert_ne!(
            bt_platform::host_platform(),
            bt_platform::HostPlatform::Windows,
            "a Windows build always embeds the release manifest"
        );
        let error = EmbeddedManifest
            .manifest_text(&this)
            .expect_err("no loader off Windows");
        assert!(error.contains("Windows"), "{error}");
        return;
    };
    let embedded = std::fs::read_to_string(path).unwrap();
    let executable = built.join("folio.exe");
    let text = EmbeddedManifest
        .manifest_text(&executable)
        .unwrap_or_else(|error| panic!("{}: {error}", executable.display()));
    assert_eq!(text, embedded, "the door reads what build.rs embedded");
    let manifest = Manifest::parse(&text).unwrap();

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let list = std::fs::read_to_string(workspace.join(MEMBER_LIST)).unwrap();
    let mut items = Vec::new();
    for listed in parse_member_list(&list).unwrap() {
        let data = match listed.source {
            Source::Exe => std::fs::read(&executable).unwrap(),
            Source::Msix => b"not a package; the manifest does not list it".to_vec(),
            Source::Sidecar => std::fs::read(built.join(&listed.name)).unwrap(),
            Source::Packaging => {
                std::fs::read(workspace.join("packaging").join(&listed.name)).unwrap()
            }
            Source::Documents => std::fs::read(workspace.join(&listed.name)).unwrap(),
        };
        let mut member = item(&listed.name, &data);
        member.entry = format!("{}/{}", manifest.archive_root, listed.name);
        if listed.source == Source::Exe {
            // A debug `folio.exe` is about 90 MB; stored, the test does not
            // spend its time in a debug-built deflater.
            member.method = 0;
        }
        items.push(member);
    }
    let root = scratch("built");
    let archive = root.join("release.zip");
    std::fs::write(&archive, zip_of(&items)).unwrap();
    let staging = Directory::open(&root.join("staging")).unwrap();
    let expanded = expand(
        &archive,
        &Expected {
            version: &manifest.version,
            arch: &manifest.arch,
            updater: MIN_UPDATER,
        },
        &staging,
        &EmbeddedManifest,
        &Deadline::at(far()),
    )
    .unwrap_or_else(|refusal| panic!("this build's own release: {refusal}"));
    assert_eq!(expanded.manifest, manifest);
    assert_eq!(expanded.members.len(), items.len());
    assert!(!staged(&root).iter().any(|name| name.starts_with('~')));
}
