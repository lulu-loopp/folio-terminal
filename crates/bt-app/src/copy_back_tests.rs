//! `copy_back`'s own tests (0.4.10 plan row 68d): the words the notice and the offer say, and
//! the part the carry pass hands the window.

use super::*;

/// An offer of `document`'s copy `copy`, its bytes `bytes`.
fn offered(document: KeptDocument, copy: &str, bytes: &[u8]) -> CopyOffered {
    CopyOffered {
        document,
        copy: copy.to_owned(),
        path: PathBuf::from(copy),
        bytes: Arc::from(bytes),
        copy_version: 2,
        file_version: 1,
    }
}

/// PIN (0.4.10 plan row 68d, R6) — **the offer's heading names every offered document, once, in
/// the order offered**: one noun alone, two joined by the last separator, three with the
/// separator before.
#[test]
fn the_offer_heading_names_every_offered_document() {
    let settings = offered(
        KeptDocument::Settings,
        "settings.json.rejected-20261114-000000",
        b"",
    );
    let pins = offered(
        KeptDocument::Pins,
        "pins.json.rejected-20261114-000000",
        b"",
    );
    let layout = offered(
        KeptDocument::Session,
        "session.json.rejected-20261114-000000",
        b"",
    );
    assert_eq!(
        offer_heading(std::slice::from_ref(&settings)),
        "Bring back your newer settings?"
    );
    assert_eq!(
        offer_heading(&[settings.clone(), pins.clone()]),
        "Bring back your newer settings and pins?"
    );
    assert_eq!(
        offer_heading(&[layout, settings, pins]),
        "Bring back your newer windows and tabs, settings and pins?"
    );
}

/// PIN (row 68d, R4) — **the notice names the document and the copy it is kept in**.
#[test]
fn the_notice_names_the_document_and_its_copy() {
    let notice = NewerKept {
        document: KeptDocument::Keybindings,
        copy: "keybindings.json.rejected-20261114-101500".to_owned(),
        path: PathBuf::from("keybindings.json.rejected-20261114-101500"),
    };
    assert_eq!(
        newer_kept_sentence(&notice),
        "This Folio is older than the one that last saved your shortcuts. They are kept in \
         keybindings.json.rejected-20261114-101500."
    );
}

/// RED (row 68d) — **a notice the window has not taken is owed after a newer round**, before the
/// newer round's own; the offer is the newer round's.
///
/// MUTATION: `CopyBackAnswer::keeping_untaken` drops the untaken notices
/// (`plant-68d-untaken-notice-dropped.py`).
#[test]
fn an_untaken_notice_is_said_after_a_newer_round() {
    let notice = |document| NewerKept {
        document,
        copy: format!("{}.rejected-20261114-000000", document.kept_file_name()),
        path: PathBuf::new(),
    };
    let untaken = CopyBackAnswer {
        notices: vec![notice(KeptDocument::Settings)],
        offer: vec![offered(KeptDocument::Pins, "old", b"1")],
    };
    let newer = CopyBackAnswer {
        notices: vec![notice(KeptDocument::Profiles)],
        offer: vec![offered(KeptDocument::Pins, "new", b"2")],
    };
    let merged = newer.keeping_untaken(Some(untaken));
    assert_eq!(
        merged.notices,
        [
            notice(KeptDocument::Settings),
            notice(KeptDocument::Profiles)
        ]
    );
    assert_eq!(merged.offer, [offered(KeptDocument::Pins, "new", b"2")]);
}

/// A scratch data folder for a store, removed with the test.
struct CopyBackScratch(PathBuf);

impl Drop for CopyBackScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh [`CopyBackScratch`] holding `keybindings.json` with `bytes`, and the file's path.
fn keybindings_written(tag: &str, bytes: &[u8]) -> (CopyBackScratch, PathBuf) {
    let root = bt_testpath::temp_path(&format!("copy-back-{tag}"));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join(crate::persist::KEYBINDINGS_FILE_NAME);
    std::fs::write(&path, bytes).unwrap();
    (CopyBackScratch(root), path)
}

/// The `.rejected-` copies of `keybindings.json` beside `path`, by path.
fn kept_copies_beside(path: &std::path::Path) -> Vec<PathBuf> {
    let mut copies: Vec<PathBuf> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|copy| {
            copy.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("keybindings.json.rejected-")
        })
        .collect();
    copies.sort();
    copies
}

/// What an older build left: `keybindings.json` at version 1, the version before this build's.
const OLDER_BUILDS_WRITE: &[u8] = b"{\"schema_version\":1,\"bindings\":[]}";

/// The newer build's copy: this build's own version, one override in it.
fn newer_copy() -> Vec<u8> {
    let mut newer: bt_persist::KeybindingsV1 =
        bt_persist::parse_keybindings(OLDER_BUILDS_WRITE).unwrap();
    newer.bindings.push(bt_persist::BindingOverrideV1 {
        action: "copy".to_owned(),
        chord: Some("Ctrl+Shift+K".to_owned()),
    });
    bt_persist::serialize_keybindings(&newer).unwrap()
}

/// RED (row 68d, R7) — **Bring back keeps the older build's write as a copy of its own, then
/// writes the newer copy in its place and reads the file again**: the file holds the copy, a
/// `.rejected-` copy beside it holds what the older build wrote, the answer names it, and a
/// re-read is on its way.
///
/// MUTATION: the older build's write is not kept (`plant-68d-replaces-without-keeping.py`).
#[test]
fn bringing_back_keeps_the_older_builds_write_as_a_copy() {
    let (_scratch, path) = keybindings_written("keeps", OLDER_BUILDS_WRITE);
    let mut store = crate::persist::KeybindingsStore::at_on(
        path.clone(),
        crate::persist::StorageLane::of_its_own(),
    );
    let newer = newer_copy();
    store.bring_back(Arc::from(&newer[..]));
    store.settle();
    let kept = kept_copies_beside(&path);
    assert_eq!(kept.len(), 1, "one copy of the older build's write");
    assert_eq!(std::fs::read(&kept[0]).unwrap(), OLDER_BUILDS_WRITE);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        newer,
        "the file holds the copy"
    );
    assert_eq!(
        store.take_copy_back(),
        Some(CopyBackDone::BroughtBack {
            kept: Some(kept[0].clone())
        })
    );
    assert!(
        store.document().reading(),
        "the file is read again, as an outside edit is"
    );
}

/// RED (row 68d, R7) — **a Bring back over a file changed since the start read it writes
/// nothing**: the hand edit stays, nothing is kept beside it, and the answer says it changed.
///
/// MUTATION: the copy is written over whatever the file holds now
/// (`plant-68d-replaces-over-an-edit.py`).
#[test]
fn a_bring_back_over_a_file_changed_since_is_refused() {
    let (_scratch, path) = keybindings_written("changed", OLDER_BUILDS_WRITE);
    let mut store = crate::persist::KeybindingsStore::at_on(
        path.clone(),
        crate::persist::StorageLane::of_its_own(),
    );
    let edit = b"{\"schema_version\":1,\"bindings\":[],\"edited\":true}";
    std::fs::write(&path, edit).unwrap();
    store.bring_back(Arc::from(&newer_copy()[..]));
    store.settle();
    assert_eq!(store.take_copy_back(), Some(CopyBackDone::ChangedSince));
    assert_eq!(std::fs::read(&path).unwrap(), edit, "the edit stays");
    assert!(kept_copies_beside(&path).is_empty(), "nothing is kept");
    assert_eq!(
        not_brought_sentence(
            KeptDocument::Keybindings,
            "k.rejected-1",
            &CopyBackDone::ChangedSince
        )
        .as_deref(),
        Some(
            "Your shortcuts changed after this Folio started, so the newer ones were not brought \
             back. They are still kept in k.rejected-1."
        )
    );
}

/// RED (row 68d, R8) — **Keep current writes the document at this build's version** although
/// nothing in it changed — the next start reads that version, and a start at this build's version
/// is offered nothing (`rejected_copies::tests::a_copy_newer_than_the_older_builds_write_is_offered`).
///
/// MUTATION: Keep current sends nothing, as a store with nothing owed does
/// (`plant-68d-keep-writes-nothing.py`).
#[test]
fn keeping_current_writes_this_builds_version() {
    let (_scratch, path) = keybindings_written("keep-current", OLDER_BUILDS_WRITE);
    let mut store = crate::persist::KeybindingsStore::at_on(
        path.clone(),
        crate::persist::StorageLane::of_its_own(),
    );
    store.write_at_this_version();
    store.settle();
    let written = std::fs::read(&path).unwrap();
    assert_eq!(
        bt_persist::schema_version_of(&written),
        Some(bt_persist::KEYBINDINGS_SCHEMA_VERSION),
        "written at this build's version"
    );
    assert_eq!(
        bt_persist::parse_keybindings(&written).unwrap(),
        bt_persist::parse_keybindings(OLDER_BUILDS_WRITE).unwrap(),
        "and nothing in it changed"
    );
}
