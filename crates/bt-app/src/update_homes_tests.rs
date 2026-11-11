//! `update_homes`' own tests (0.4.10 plan row 68e): the sweep over homes and records of their own,
//! in scratch folders of the system's temporary directory, never the account's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::attention_ownership::PathStands;

/// **A scratch folder in the system's temporary directory, spelt with no link above it and no
/// `..` in it** (a recorded entry with either is refused, by design), removed with the test.
struct HomesScratch(PathBuf);

impl std::ops::Deref for HomesScratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for HomesScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh [`HomesScratch`].
fn homes_scratch(tag: &str) -> HomesScratch {
    let root = bt_testpath::link_free_temp_path(tag);
    std::fs::create_dir_all(&root).unwrap();
    HomesScratch(root)
}

/// **A bundle `<root>/<folder>/<name>.app` and its home**: the bundle's main executable made when
/// `bundle_there`, the home made with `files` in it. The home's path.
fn homes_fixture(
    root: &Path,
    folder: &str,
    name: &str,
    bundle_there: bool,
    files: &[&str],
) -> PathBuf {
    let bundle = root.join(folder).join(format!("{name}.app"));
    let home = Home::for_bundle(&bundle).expect("an .app with a parent");
    if bundle_there {
        let program = home.installed_program().expect("a bundle's home");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, name.as_bytes()).unwrap();
    }
    std::fs::create_dir_all(home.root()).unwrap();
    for file in files {
        std::fs::write(home.root().join(file), file.as_bytes()).unwrap();
    }
    home.root().to_path_buf()
}

/// `body` on a worker of the test's own, as the carry pass's member and the uninstall run the
/// sweep; a panic in it is the test's.
fn homes_on_a_worker(body: impl FnOnce(&WorkerCtx) + Send + 'static) {
    match bt_platform::spawn_at_priority(
        "bt-update-homes-test",
        bt_platform::ThreadPriority::BelowNormal,
        body,
    )
    .expect("the thread door starts a thread")
    .join()
    {
        Ok(()) => {}
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// The record at `file`, written as the door writes it, naming `homes`.
fn homes_record_write(file: &Path, homes: &[&Path]) {
    let record = UpdateHomesRecord {
        homes: homes.iter().map(|home| home.to_path_buf()).collect(),
        ..UpdateHomesRecord::default()
    };
    std::fs::write(file, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
}

/// The homes the record at `file` names.
fn homes_record_entries(file: &Path) -> Vec<PathBuf> {
    UpdateHomesRecord::read(file)
        .expect("a record this build reads")
        .homes
}

/// **Every entry under `root` and its bytes**, a link read as its target's name and never
/// followed: what "byte for byte" is compared on.
fn homes_tree_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if bt_platform::cleanup::is_link(&meta) {
                let target = std::fs::read_link(&path).unwrap_or_default();
                found.insert(path, target.as_os_str().as_encoded_bytes().to_vec());
            } else if meta.is_dir() {
                found.insert(path.clone(), b"<folder>".to_vec());
                folders.push(path);
            } else {
                found.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    found
}

/// A gate that must not be asked: nothing is due.
fn homes_gate_never_asked(writer: Writer) -> bool {
    panic!("the trial's gate is asked only when a write is due, not for {writer:?}")
}

/// The lines one sweep says through a fresh book, and the lines a second sweep's answer adds.
fn homes_lines_said(book: &OrphansSaid, swept: &HomesSwept) -> Vec<String> {
    let said = std::cell::RefCell::new(Vec::new());
    let _ = book.lines_answer(swept.home_lines(), &|line| said.borrow_mut().push(line));
    said.into_inner()
}

/// RED (0.4.10 plan row 68e; the member's `proved_by`) — **a recorded home whose bundle is gone
/// is removed, and every other is left.**
///
/// Three recorded homes beside three bundles: the bundle gone, the home holding Folio's three
/// files (removed, with its entry); the bundle there (left, its entry kept); the home itself not
/// there (its entry dropped). The record holds exactly the live entry afterwards, each home is
/// said once, and a second round that finds the same says nothing new.
///
/// MUTATION: the bundle's answer ignored (`plant-68e-sweep-takes-a-live-home.py`) and the live
/// home is removed.
#[test]
fn a_recorded_home_whose_bundle_is_gone_is_removed_and_every_other_is_left() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-swept");
        let gone = homes_fixture(
            &root,
            "Applications",
            "Folio",
            false,
            &["admission", "lock", "recover.log"],
        );
        let live = homes_fixture(&root, "Elsewhere", "Folio Beta", true, &["lock"]);
        let live_bundle = root.join("Elsewhere").join("Folio Beta.app");
        let missing = Home::for_bundle(&root.join("Moved").join("Folio.app"))
            .unwrap()
            .root()
            .to_path_buf();
        std::fs::create_dir_all(root.join("Moved")).unwrap();
        let record = root.join(HOMES_RECORD);
        homes_record_write(&record, &[&gone, &live, &missing]);

        let swept = sweep_recorded_homes(worker, &record, None, None, &what_stands, |_| false);
        assert_eq!(
            swept,
            HomesSwept {
                fates: vec![
                    (gone.clone(), HomeFate::Removed),
                    (live.clone(), HomeFate::LeftLive(live_bundle.clone())),
                    (missing.clone(), HomeFate::Gone),
                ],
                record: RecordState::Kept,
            }
        );
        assert!(!gone.exists(), "the home beside the gone bundle is removed");
        assert!(
            root.join("Applications").is_dir(),
            "and nothing above it is"
        );
        assert_eq!(
            std::fs::read(live.join("lock")).unwrap(),
            b"lock",
            "the live home is left"
        );
        assert_eq!(homes_record_entries(&record), std::slice::from_ref(&live));

        let book = OrphansSaid::new();
        let first = homes_lines_said(&book, &swept);
        assert_eq!(first.len(), 3, "{first:?}");
        assert!(first.contains(&"BT_UPDATE_HOMES .Folio.app.folio-update #1 removed".to_owned()));
        assert!(
            first.contains(&"BT_UPDATE_HOMES .Folio Beta.app.folio-update left (live)".to_owned())
        );
        assert!(first.contains(&"BT_UPDATE_HOMES .Folio.app.folio-update #3 gone".to_owned()));
        let again = sweep_recorded_homes(
            worker,
            &record,
            None,
            None,
            &what_stands,
            homes_gate_never_asked,
        );
        assert_eq!(
            again.fates,
            [(live.clone(), HomeFate::LeftLive(live_bundle))]
        );
        assert!(
            homes_lines_said(&book, &again).is_empty(),
            "a round that finds what was said says nothing"
        );
    });
}

/// PIN (0.4.10 plan row 68e, review N1 of 2026-11-10) — **retiring a home never removes again
/// after its folder is gone**: the emptied folder goes by `std::fs::remove_dir`, which takes no
/// tree, and what follows only flushes its parent (`install_txn::flush_directory_on_disk`). A
/// removal after it — `install_txn::durable_remove`, which is `remove_dir_all` for a folder —
/// would take whole a home a Prepare made again at the path in between. The window is a few
/// instructions wide and no test drives it, so the body is pinned by its source.
///
/// MUTATION: `install_txn::durable_remove(home.root())` put back in place of the flush
/// (`plant-68e-retire-removes-again.py`).
#[test]
fn retiring_a_home_never_removes_again_after_its_folder() {
    use crate::test_support::{free_fn_body, squeezed};
    let body = squeezed(free_fn_body("retire_home"));
    let (_, after) = body
        .split_once("std::fs::remove_dir(home.root())")
        .expect("the emptied folder goes by remove_dir");
    assert!(
        after.contains(
            "install_txn::flush_directory_on_disk(install_txn::directory_of(home.root()))"
        ),
        "its parent is flushed: {after}"
    );
    for removal in ["remove_", "_remove("] {
        assert!(
            !after.contains(removal),
            "nothing removes after the folder is gone: {after}"
        );
    }
}

/// RED (0.4.10 plan row 68e, E6) — **a home with a journal is left for its rescue**: beside a gone
/// bundle, a home holding a journal — one this build reads and one of bytes no build reads — is
/// left whole and its entry kept; nothing is asked of the trial's gate.
///
/// MUTATION: the journal not looked for (`plant-68e-sweep-takes-an-unfinished-home.py`) and the
/// home is removed.
#[test]
fn a_home_with_a_journal_is_left_for_its_rescue() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-in-flight");
        let read = homes_fixture(
            &root,
            "Applications",
            "Folio",
            false,
            &["admission", "lock"],
        );
        std::fs::write(read.join("journal.json"), br#"{"header":{"v":1}}"#).unwrap();
        let junk = homes_fixture(&root, "Applications", "Folio Beta", false, &["lock"]);
        std::fs::write(junk.join("journal.json"), [0xff, 0x00, 0x7b]).unwrap();
        let record = root.join(HOMES_RECORD);
        homes_record_write(&record, &[&read, &junk]);
        let before = homes_tree_bytes(&root);

        let swept = sweep_recorded_homes(
            worker,
            &record,
            None,
            None,
            &what_stands,
            homes_gate_never_asked,
        );
        assert_eq!(
            swept.fates,
            [
                (read.clone(), HomeFate::LeftInFlight),
                (junk.clone(), HomeFate::LeftInFlight),
            ]
        );
        assert_eq!(
            homes_tree_bytes(&root),
            before,
            "every byte of both homes and of the record"
        );
    });
}

/// RED (0.4.10 plan row 68e, E5) — **a home holding anything but Folio's three files is left
/// byte for byte**: a transaction's folder with no journal, a stranger's file, and a link where
/// the `lock` is, each in its own home beside a gone bundle; the link's target is never touched.
///
/// MUTATION: what the home holds not looked at (`plant-68e-sweep-takes-foreign-files.py`) and the
/// homes go.
#[test]
fn a_home_holding_anything_else_is_left_byte_for_byte() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-not-only-ours");
        let with_txn = homes_fixture(&root, "Applications", "Folio", false, &["lock"]);
        let txn = with_txn.join("0123456789abcdef0123456789abcdef");
        std::fs::create_dir_all(txn.join("stage")).unwrap();
        let with_file = homes_fixture(&root, "Applications", "Folio Beta", false, &["admission"]);
        std::fs::write(with_file.join("notes.txt"), b"mine").unwrap();
        let with_link = homes_fixture(&root, "Applications", "Folio Dev", false, &["admission"]);
        let outside = root.join("Outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("kept.txt"), b"kept").unwrap();
        crate::uninstall::tests::plant_directory_link(&with_link.join("lock"), &outside);
        let record = root.join(HOMES_RECORD);
        homes_record_write(&record, &[&with_txn, &with_file, &with_link]);
        let before = homes_tree_bytes(&root);

        let swept = sweep_recorded_homes(
            worker,
            &record,
            None,
            None,
            &what_stands,
            homes_gate_never_asked,
        );
        assert_eq!(
            swept.fates,
            [
                (with_txn.clone(), HomeFate::LeftNotOnlyOurs(vec![txn])),
                (
                    with_file.clone(),
                    HomeFate::LeftNotOnlyOurs(vec![with_file.join("notes.txt")])
                ),
                (
                    with_link.clone(),
                    HomeFate::LeftNotOnlyOurs(vec![with_link.join("lock")])
                ),
            ]
        );
        assert_eq!(homes_tree_bytes(&root), before, "every home byte for byte");
        assert_eq!(std::fs::read(outside.join("kept.txt")).unwrap(), b"kept");
        crate::uninstall::tests::remove_directory_link(&with_link.join("lock"));
    });
}

/// RED (0.4.10 plan row 68e) — **a home a running copy holds is left**: with its `admission`
/// held shared, as every running copy of its bundle holds it for its life, and then with its
/// `lock` held, the home is `in use` and nothing in it goes; let go, it is removed. The holds are
/// the real `install_txn::try_hold` on every platform (ruling (c) of 2026-11-10).
///
/// MUTATION: the holds not asked for (`plant-68e-sweep-ignores-the-locks.py`) and the held home
/// is removed.
#[test]
fn a_home_a_running_copy_holds_is_left() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-in-use");
        let home = homes_fixture(
            &root,
            "Applications",
            "Folio",
            false,
            &["admission", "lock"],
        );
        let record = root.join(HOMES_RECORD);
        homes_record_write(&record, &[&home]);
        let located = Home::for_bundle(&root.join("Applications").join("Folio.app")).unwrap();
        for held in [located.admission(), located.lock()] {
            let hold = if held == located.admission() {
                Hold::Shared
            } else {
                Hold::Exclusive
            };
            let holder = install_txn::try_hold(&held, hold)
                .unwrap()
                .expect("the test holds it first");
            let swept = sweep_recorded_homes(worker, &record, None, None, &what_stands, |_| false);
            assert_eq!(swept.fates, [(home.clone(), HomeFate::LeftInUse)]);
            assert!(
                home.join("admission").is_file() && home.join("lock").is_file(),
                "nothing in a held home goes"
            );
            drop(holder);
        }
        let swept = sweep_recorded_homes(worker, &record, None, None, &what_stands, |_| false);
        assert_eq!(swept.fates, [(home.clone(), HomeFate::Removed)]);
        assert!(!home.exists());
    });
}

/// **The machine with the drive a bundle was on unplugged**: what stands is this machine's, and
/// the way to a missing path's volume cannot be followed.
struct HomesAwayVolume;

impl Look for HomesAwayVolume {
    fn stands(&self, path: &Path) -> PathStands {
        what_stands(path)
    }

    fn volume_of(&self, _missing: &Path) -> Option<PathBuf> {
        None
    }
}

/// RED (0.4.10 plan row 68e; row 67's R4) — **a recorded home whose bundle is on a volume that is
/// away is left, and said**: the bundle is not gone, only unplugged.
///
/// MUTATION: the bundle judged by its absence alone (`plant-68e-home-sweep-ignores-the-volume.py`)
/// and the home is removed.
#[test]
fn a_recorded_home_on_a_volume_that_is_away_is_left_and_said() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-away");
        let home = homes_fixture(&root, "Volume", "Folio", false, &["admission", "lock"]);
        let record = root.join(HOMES_RECORD);
        homes_record_write(&record, &[&home]);
        let swept = sweep_recorded_homes(
            worker,
            &record,
            None,
            None,
            &HomesAwayVolume,
            homes_gate_never_asked,
        );
        assert_eq!(swept.fates, [(home.clone(), HomeFate::LeftOwnerUnknown)]);
        assert!(home.join("admission").is_file(), "left");
        assert_eq!(
            homes_record_entries(&record),
            std::slice::from_ref(&home),
            "entry kept"
        );
        assert_eq!(
            homes_lines_said(&OrphansSaid::new(), &swept),
            ["BT_UPDATE_HOMES .Folio.app.folio-update left (owner unknown)"]
        );
    });
}

/// RED (0.4.10 plan row 68e) — **a record entry that is not a home's path is never followed**:
/// a relative path, one through `..`, a name not `.<X>.app.folio-update`, a home's name over a
/// file, and a home's path that is a link to a folder elsewhere are each dropped from the record
/// as `refused entry`, and nothing they name is touched — the link's target holds a home's three
/// files beside a gone bundle, and keeps them.
///
/// MUTATION: the entry's shape not checked (`plant-68e-record-entry-followed.py`) and the link's
/// target is emptied.
#[test]
fn a_record_entry_that_is_not_a_homes_path_is_never_followed() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-entries");
        let elsewhere = root.join("Elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        for file in ["admission", "lock", "recover.log"] {
            std::fs::write(elsewhere.join(file), file.as_bytes()).unwrap();
        }
        std::fs::create_dir_all(root.join("Links")).unwrap();
        let linked = root.join("Links").join(".Folio.app.folio-update");
        crate::uninstall::tests::plant_directory_link(&linked, &elsewhere);
        let stranger = root.join("Applications").join("stranger");
        std::fs::create_dir_all(&stranger).unwrap();
        std::fs::write(stranger.join("lock"), b"theirs").unwrap();
        let a_file = root
            .join("Applications")
            .join(".Folio Beta.app.folio-update");
        std::fs::write(&a_file, b"a file").unwrap();
        let entries = [
            PathBuf::from("Applications").join(".Folio.app.folio-update"),
            root.join("Applications")
                .join("x")
                .join("..")
                .join(".Folio.app.folio-update"),
            root.join("Applications").join(".Folio.folio-update"),
            stranger.clone(),
            a_file.clone(),
            linked.clone(),
        ];
        let record = root.join(HOMES_RECORD);
        homes_record_write(
            &record,
            &entries.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
        );
        let before: BTreeMap<_, _> = homes_tree_bytes(&root)
            .into_iter()
            .filter(|(path, _)| *path != record)
            .collect();

        let swept = sweep_recorded_homes(worker, &record, None, None, &what_stands, |_| false);
        assert_eq!(
            swept.fates,
            entries
                .iter()
                .map(|entry| (entry.clone(), HomeFate::RefusedEntry))
                .collect::<Vec<_>>()
        );
        assert!(
            homes_record_entries(&record).is_empty(),
            "every one dropped"
        );
        let after: BTreeMap<_, _> = homes_tree_bytes(&root)
            .into_iter()
            .filter(|(path, _)| *path != record)
            .collect();
        assert_eq!(after, before, "and nothing any of them names touched");
        for file in ["admission", "lock", "recover.log"] {
            assert!(
                elsewhere.join(file).is_file(),
                "the link's target keeps {file}"
            );
        }
        crate::uninstall::tests::remove_directory_link(&linked);
    });
}

/// RED (0.4.10 plan row 68e, E4) — **a record this build cannot read is left as it is**: one of
/// a later version, one with a key this version does not have, and bytes that are not a record
/// are each left byte for byte by the sweep, by the seed and by the Prepare's entry; the sweep
/// says `record unread` once, and a second round says nothing new.
///
/// MUTATION: a record that cannot be read taken as empty and written
/// (`plant-68e-newer-record-overwritten.py`) and the newer record is replaced.
#[test]
fn a_newer_record_is_left_as_it_is() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-newer");
        let seed = homes_fixture(&root, "Applications", "Folio", true, &["lock"]);
        let record = root.join(HOMES_RECORD);
        for bytes in [
            br#"{"version":2,"homes":[]}"#.as_slice(),
            br#"{"version":1,"homes":[],"bundles":[]}"#.as_slice(),
            b"\x00not a record".as_slice(),
        ] {
            std::fs::write(&record, bytes).unwrap();
            let book = OrphansSaid::new();
            let swept = sweep_recorded_homes(
                worker,
                &record,
                Some(&seed),
                None,
                &what_stands,
                homes_gate_never_asked,
            );
            assert!(swept.fates.is_empty(), "{:?}", swept.fates);
            assert!(
                matches!(swept.record, RecordState::Unread(_)),
                "{:?}",
                swept.record
            );
            let said = homes_lines_said(&book, &swept);
            assert_eq!(said.len(), 1, "{said:?}");
            assert!(
                said[0].starts_with("BT_UPDATE_HOMES record unread ("),
                "{said:?}"
            );
            let again = sweep_recorded_homes(
                worker,
                &record,
                Some(&seed),
                None,
                &what_stands,
                homes_gate_never_asked,
            );
            assert!(homes_lines_said(&book, &again).is_empty(), "said once");
            assert!(add_home_to_record(&record, &seed).is_err());
            assert_eq!(std::fs::read(&record).unwrap(), bytes, "left byte for byte");
        }
    });
}

/// PIN (0.4.10 plan row 68e; 68d's rule, R4/R8 of 2026-11-04) — **the record keeps its keys at
/// version 1**: a key added or taken away without a new version is red here, so a build that
/// reads version 1 is never handed a shape it refuses unknowingly. The fields are read from the
/// struct's own source, not only from a written default (review N5): a key added as
/// `#[serde(default, skip_serializing_if = …)]` is absent from the default's JSON, yet every
/// version-1 build, reading with `deny_unknown_fields`, would refuse a record that carries it.
///
/// MUTATION: a key renamed at version 1 (`plant-68e-record-key-renamed.py`); an optional key
/// added at version 1 (`plant-68e-record-key-added-optional.py`).
#[test]
fn the_update_homes_record_keeps_its_keys_at_version_1() {
    use crate::test_support::{item_body, item_declaration, squeezed};
    assert_eq!(HOMES_RECORD_VERSION, 1);
    let record = bt_source::ItemQuery::type_item("UpdateHomesRecord");
    assert_eq!(
        squeezed(item_body(&record)),
        "{version:u32,homes:Vec<PathBuf>,}",
        "version 1's fields, as declared"
    );
    assert!(
        squeezed(item_declaration(&record)).contains("#[serde(deny_unknown_fields)]"),
        "version 1 refuses a key it does not know"
    );
    let written = serde_json::to_value(UpdateHomesRecord::default()).unwrap();
    let keys: Vec<&str> = written
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["homes", "version"], "version 1's keys");
    assert_eq!(written["version"], 1);
}

/// RED (0.4.10 plan row 68e, E3; the check's old-shape test) — **a home made before the record is
/// recorded by its copy's start**: this copy's own home, there and absent from the record, is
/// added once (`recorded`, then `left (live)`); a home that is not there is not added; during a
/// trial the addition is held and nothing is written.
///
/// MUTATION: the seed dropped (`plant-68e-no-seed.py`).
#[test]
fn a_home_made_before_the_record_is_recorded_by_its_copys_start() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-seed");
        let mine = homes_fixture(&root, "Applications", "Folio", true, &["admission", "lock"]);
        let record = root.join(HOMES_RECORD);

        let gate = crate::update_trial::Gate::new();
        let held =
            sweep_recorded_homes(worker, &record, Some(&mine), None, &what_stands, |writer| {
                gate.defer(true, writer)
            });
        assert_eq!(held.fates, [(mine.clone(), HomeFate::Held)]);
        assert!(!record.exists(), "nothing written during the trial");
        assert_eq!(gate.pending(), vec![Writer::OrphanSweep]);

        let seeded =
            sweep_recorded_homes(worker, &record, Some(&mine), None, &what_stands, |_| false);
        assert_eq!(seeded.fates, [(mine.clone(), HomeFate::Recorded)]);
        assert_eq!(homes_record_entries(&record), std::slice::from_ref(&mine));
        let again = sweep_recorded_homes(
            worker,
            &record,
            Some(&mine),
            None,
            &what_stands,
            homes_gate_never_asked,
        );
        assert_eq!(
            again.fates,
            [(
                mine.clone(),
                HomeFate::LeftLive(root.join("Applications").join("Folio.app"))
            )]
        );
        assert_eq!(
            homes_record_entries(&record),
            std::slice::from_ref(&mine),
            "once"
        );

        let not_made = Home::for_bundle(&root.join("Other").join("Folio.app"))
            .unwrap()
            .root()
            .to_path_buf();
        let _ = sweep_recorded_homes(
            worker,
            &record,
            Some(&not_made),
            None,
            &what_stands,
            homes_gate_never_asked,
        );
        assert_eq!(
            homes_record_entries(&record),
            std::slice::from_ref(&mine),
            "a home that is not there is not recorded"
        );
    });
}

/// RED (0.4.10 plan row 68e, E2) — **the home sweep waits for the trial's commit**: during an
/// update's trial a due removal and a due addition ask the gate once, are held and recorded as
/// pending ([`Writer::OrphanSweep`]), and nothing is written or removed; the commit's round does
/// both. A round with nothing due never asks. A round whose only due write is the record's own
/// rewrite — an entry whose home is gone — holds that rewrite as it holds a removal: the record is
/// left byte for byte, `record held` is said, and the commit's round drops the entry (review N2).
///
/// MUTATION: removed without asking the gate (`plant-68e-home-sweep-unheld.py`); the record's
/// rewrite made without asking it (`plant-68e-record-rewrite-unheld.py`).
#[test]
fn the_home_sweep_waits_for_the_trials_commit() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-trial");
        let gone = homes_fixture(&root, "Applications", "Folio", false, &["admission"]);
        let mine = homes_fixture(&root, "Elsewhere", "Folio", true, &["lock"]);
        let record = root.join(HOMES_RECORD);
        homes_record_write(&record, &[&gone]);
        let recorded = std::fs::read(&record).unwrap();

        let gate = crate::update_trial::Gate::new();
        let asked = std::cell::Cell::new(0);
        let held =
            sweep_recorded_homes(worker, &record, Some(&mine), None, &what_stands, |writer| {
                asked.set(asked.get() + 1);
                gate.defer(true, writer)
            });
        assert_eq!(asked.get(), 1, "the gate is asked once");
        assert_eq!(
            held.fates,
            [
                (mine.clone(), HomeFate::Held),
                (gone.clone(), HomeFate::Held)
            ]
        );
        assert!(gone.join("admission").is_file(), "nothing removed");
        assert_eq!(std::fs::read(&record).unwrap(), recorded, "nothing written");
        assert_eq!(gate.pending(), vec![Writer::OrphanSweep]);

        let committed =
            sweep_recorded_homes(worker, &record, Some(&mine), None, &what_stands, |_| false);
        assert_eq!(
            committed.fates,
            [
                (gone.clone(), HomeFate::Removed),
                (mine.clone(), HomeFate::Recorded),
            ]
        );
        assert!(!gone.exists());
        assert_eq!(homes_record_entries(&record), std::slice::from_ref(&mine));
        let quiet = sweep_recorded_homes(
            worker,
            &record,
            Some(&mine),
            None,
            &what_stands,
            homes_gate_never_asked,
        );
        assert_eq!(
            quiet.fates,
            [(
                mine.clone(),
                HomeFate::LeftLive(root.join("Elsewhere").join("Folio.app"))
            )]
        );

        let moved = Home::for_bundle(&root.join("Moved").join("Folio.app"))
            .unwrap()
            .root()
            .to_path_buf();
        std::fs::create_dir_all(root.join("Moved")).unwrap();
        let only_gone = root.join("only-gone.json");
        homes_record_write(&only_gone, &[&moved]);
        let before = std::fs::read(&only_gone).unwrap();
        let trial = crate::update_trial::Gate::new();
        let held = sweep_recorded_homes(worker, &only_gone, None, None, &what_stands, |writer| {
            trial.defer(true, writer)
        });
        assert_eq!(
            held,
            HomesSwept {
                fates: vec![(moved.clone(), HomeFate::Gone)],
                record: RecordState::Held,
            }
        );
        assert_eq!(
            std::fs::read(&only_gone).unwrap(),
            before,
            "the record's rewrite is held: left byte for byte"
        );
        assert_eq!(trial.pending(), vec![Writer::OrphanSweep]);
        assert!(
            held.home_lines()
                .contains(&"BT_UPDATE_HOMES record held".to_owned()),
            "{:?}",
            held.home_lines()
        );
        let dropped = sweep_recorded_homes(worker, &only_gone, None, None, &what_stands, |_| false);
        assert_eq!(dropped.record, RecordState::Kept);
        assert!(
            homes_record_entries(&only_gone).is_empty(),
            "the commit's round drops it"
        );
    });
}

/// RED (0.4.10 plan row 68e) — **the Prepare's entry keeps one entry per home**: the same home
/// twice is one entry, a second home is added after it, and the record is written through the
/// journal's own durable writer (`record_update_home` calls `add_home_to_record`).
///
/// MUTATION: the entry added whether or not it is there (`plant-68e-record-twice.py`).
#[test]
fn record_update_home_keeps_one_entry_per_home() {
    let root = homes_scratch("homes-record");
    let record = root.join(HOMES_RECORD);
    let first = root.join("Applications").join(".Folio.app.folio-update");
    let second = root.join("Elsewhere").join(".Folio.app.folio-update");
    assert_eq!(add_home_to_record(&record, &first), Ok(true));
    assert_eq!(add_home_to_record(&record, &first), Ok(false));
    assert_eq!(add_home_to_record(&record, &second), Ok(true));
    assert_eq!(homes_record_entries(&record), [first, second]);
    use crate::test_support::{free_fn_body, method_body, squeezed};
    assert!(
        squeezed(method_body("UpdateHomesRecord", "write")).contains("install_txn::durable_write("),
        "the record is written durably"
    );
    assert!(
        squeezed(free_fn_body("record_update_home")).contains("add_home_to_record(record,"),
        "the Prepare's entry is this one"
    );
}

/// PIN (0.4.10 plan row 68e; 68b's S2 shape) — **the macOS Prepare records its home before it
/// makes it**: `prepare_on` calls `record_update_home(` before
/// `install_txn::durable_create_dir(home.root())`, outside the match on its answer, so both a
/// new home and one that was there are recorded; and the product's driver,
/// `MacPrepare::of_this_copy`, hands it the data folder's record — `record: None` is a test
/// driver's alone (review N3).
///
/// MUTATION: the entry moved after the home is made
/// (`plant-68e-prepare-records-after-the-home.py`); the product's driver handed no record
/// (`plant-68e-prepare-records-nothing.py`).
#[test]
fn the_macos_prepare_records_its_home_before_making_it() {
    use crate::test_support::{item_body, squeezed};
    let body = squeezed(item_body(
        &bt_source::ItemQuery::function("prepare_on").in_module("crate::update_prepare_macos"),
    ));
    let recorded = body
        .find("update_homes::record_update_home(record,&home)")
        .expect("the macOS Prepare records its home");
    let made = body
        .find("matchinstall_txn::durable_create_dir(home.root())")
        .expect("the macOS Prepare makes its home");
    assert!(recorded < made, "recorded before it is made: {body}");
    let product = squeezed(crate::test_support::method_body(
        "MacPrepare",
        "of_this_copy",
    ));
    assert!(
        product.contains(
            "record:Some(crate::persist::storage_dir().join(crate::update_homes::HOMES_RECORD))"
        ),
        "the product's Prepare is handed the data folder's record: {product}"
    );
}

/// PIN (0.4.10 plan row 68e, ruling (b) of 2026-11-10) — **a `BT_UPDATE_HOMES` line never carries a
/// path**: over every fate the sweep answers, in a tree whose every path holds the scratch root,
/// no line holds the root, a separator, or a part of any recorded path but the home's own name;
/// and the one function that builds the lines formats no path.
#[test]
fn the_update_homes_lines_name_no_path() {
    homes_on_a_worker(|worker| {
        let root = homes_scratch("homes-no-path");
        let removed = homes_fixture(&root, "Applications", "Folio", false, &["lock"]);
        let live = homes_fixture(&root, "Applications", "Folio Beta", true, &["lock"]);
        let flight = homes_fixture(&root, "Applications", "Folio Dev", false, &["journal.json"]);
        let more = homes_fixture(&root, "Other", "Folio", false, &["notes.txt"]);
        let gone = Home::for_bundle(&root.join("Moved").join("Folio.app"))
            .unwrap()
            .root()
            .to_path_buf();
        std::fs::create_dir_all(root.join("Moved")).unwrap();
        let record = root.join(HOMES_RECORD);
        homes_record_write(
            &record,
            &[
                &removed,
                &live,
                &flight,
                &more,
                &gone,
                Path::new("relative"),
            ],
        );
        let swept = sweep_recorded_homes(worker, &record, None, None, &what_stands, |_| false);
        std::fs::write(&record, b"\x00not a record").unwrap();
        let unread = sweep_recorded_homes(worker, &record, None, None, &what_stands, |_| false);
        let lines: Vec<String> = swept
            .home_lines()
            .into_iter()
            .chain(unread.home_lines())
            .collect();
        assert_eq!(lines.len(), 7, "{lines:?}");
        let root_said = root.display().to_string();
        for line in &lines {
            assert!(!line.contains(&root_said), "{line}");
            assert!(!line.contains('/') && !line.contains('\\'), "{line}");
            for part in ["Applications", "Other", "Moved"] {
                assert!(!line.contains(part), "{line} names {part}");
            }
        }
        let body = crate::test_support::method_body("HomesSwept", "home_lines");
        assert!(
            !body.contains(".display()") && !body.contains("{home"),
            "the lines name no path: {body}"
        );
    });
}
