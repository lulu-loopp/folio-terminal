//! `rejected_copies`' own tests (0.4.10 plan row 68f): the retirement over data folders of their
//! own, in scratch folders of the system's temporary directory, never the account's.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;

/// A scratch data folder, removed with the test.
struct CopiesScratch(PathBuf);

impl std::ops::Deref for CopiesScratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for CopiesScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh [`CopiesScratch`].
fn copies_scratch(tag: &str) -> CopiesScratch {
    let root = bt_testpath::temp_path(&format!("rejected-copies-{tag}"));
    std::fs::create_dir_all(&root).unwrap();
    CopiesScratch(root)
}

/// `body` on a worker of the test's own, as the carry pass runs the member; a panic in it is the
/// test's.
fn copies_on_a_worker(body: impl FnOnce(&WorkerCtx) + Send + 'static) {
    match bt_platform::spawn_at_priority(
        "bt-rejected-copies-test",
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

/// `<document>.rejected-202611<day>-0000<second>` in `folder`, holding `bytes`; its name.
fn copy_written(folder: &Path, document: &str, day: u32, second: u32, bytes: &[u8]) -> String {
    let name = format!("{document}.rejected-202611{day:02}-0000{second:02}");
    std::fs::write(folder.join(&name), bytes).unwrap();
    name
}

/// The names in `folder`, sorted.
fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A gate that must not be asked: nothing is due.
fn copies_gate_never_asked(writer: Writer) -> bool {
    panic!("the trial's gate is asked only when a removal is due, not for {writer:?}")
}

/// The disk's own removal, recording the order of the names it was handed.
fn removal_recorded(order: &RefCell<Vec<String>>) -> impl FnMut(&Path) -> io::Result<()> + '_ {
    move |path| {
        order
            .borrow_mut()
            .push(path.file_name().unwrap().to_string_lossy().into_owned());
        std::fs::remove_file(path)
    }
}

/// One round of the member over `folder`, as the writer, with `opened` and no trial; the lines it
/// said through `said`.
fn member_round(
    worker: &WorkerCtx,
    folder: &Path,
    opened: &mut [OpenedDocument],
    said: &OrphansSaid,
) -> Vec<String> {
    let lines = RefCell::new(Vec::new());
    let answer = rejected_copies_member(
        worker,
        true,
        folder,
        opened,
        |_| false,
        &mut |path| std::fs::remove_file(path),
        said,
        &|line| lines.borrow_mut().push(line),
    );
    assert!(answer.is_some(), "the writer's member answers");
    lines.into_inner()
}

/// RED (0.4.10 plan row 68f, R9; the member's `proved_by` and old-shape test) — **of five copies
/// of one document, the newest three are kept and the two older ones removed, oldest first**,
/// said in one line; a second round finds nothing due and says nothing.
///
/// MUTATION: the copies are judged oldest first, so the oldest three are kept
/// (`plant-68f-keeps-the-oldest.py`).
#[test]
fn copies_beyond_the_newest_three_are_removed_oldest_first() {
    copies_on_a_worker(|worker| {
        let folder = copies_scratch("newest-three");
        std::fs::write(folder.join("settings.json"), b"{\"schema_version\":1}").unwrap();
        let names: Vec<String> = (1..=5)
            .map(|day| {
                copy_written(
                    &folder,
                    "settings.json",
                    day,
                    0,
                    format!("{day}").as_bytes(),
                )
            })
            .collect();
        let order = RefCell::new(Vec::new());
        let retired = retire_rejected_copies(
            worker,
            &folder,
            &mut [],
            |_| false,
            &mut removal_recorded(&order),
        )
        .unwrap();
        assert_eq!(
            retired,
            [CopiesRetired {
                document: KeptDocument::Settings,
                kept: 3,
                removed: vec![names[0].clone(), names[1].clone()],
                left: Vec::new(),
                held: false,
                said: None,
                offered: None,
            }]
        );
        assert_eq!(
            *order.borrow(),
            [names[0].clone(), names[1].clone()],
            "oldest first"
        );
        let mut left = names[2..].to_vec();
        left.push("settings.json".to_owned());
        left.sort();
        assert_eq!(names_in(&folder), left);
        assert_eq!(
            retired[0].copies_lines(),
            [format!(
                "BT_REJECTED_COPIES settings.json kept=3 removed={},{}",
                names[0], names[1]
            )]
        );

        let said = OrphansSaid::new();
        assert_eq!(
            member_round(worker, &folder, &mut [], &said),
            Vec::<String>::new()
        );
        let quiet = retire_rejected_copies(
            worker,
            &folder,
            &mut [],
            copies_gate_never_asked,
            &mut |_| panic!("nothing is due"),
        )
        .unwrap();
        assert_eq!(quiet[0].kept, 3);
        assert!(
            quiet[0].copies_lines().is_empty(),
            "a quiet round says nothing"
        );
    });
}

/// RED (0.4.10 plan row 68f, R3) — **only the names the namer gives, on plain files, are
/// counted**: a copy name with a date that does not exist, another word after the stamp, the
/// storage worker's `.displaced-` copy, a name close to a document's, and a folder with a copy's
/// name are never removed and never counted among the three. (A link is refused by the shared
/// `bt_platform::cleanup::is_link`, beside the plain-file test; a test making one would decide
/// its platform, which only the listed files may.)
///
/// MUTATION: the stamp's date and time are not checked, so a copy name with a day that does not
/// exist is counted (`plant-68f-any-name-counted.py`, with `bt_persist`'s half).
#[test]
fn only_folios_own_copy_names_are_counted() {
    copies_on_a_worker(|worker| {
        let folder = copies_scratch("own-names");
        for day in 1..=3 {
            copy_written(
                &folder,
                "settings.json",
                day,
                0,
                format!("{day}").as_bytes(),
            );
        }
        let strangers = [
            "settings.json.rejected-20261131-000000",
            "settings.json.rejected-20261101-000000-mine",
            "settings.json.rejected-20261101-000000.bak",
            "settings.json.displaced-20261101-000000",
            "my-settings.json.rejected-20261101-000000",
            "settings.prev.json.rejected-20261101-000000",
        ];
        for stranger in strangers {
            std::fs::write(folder.join(stranger), b"not ours").unwrap();
        }
        std::fs::create_dir(folder.join("settings.json.rejected-20261102-000001")).unwrap();
        let before = names_in(&folder);
        let retired = retire_rejected_copies(
            worker,
            &folder,
            &mut [],
            copies_gate_never_asked,
            &mut |path| panic!("{} is removed", path.display()),
        )
        .unwrap();
        assert_eq!(retired.len(), 1, "one document has copies: {retired:?}");
        assert_eq!(
            retired[0].kept, 3,
            "three copies of settings.json, no stranger"
        );
        assert_eq!(names_in(&folder), before, "nothing removed");
    });
}

/// RED (0.4.10 plan row 68f, R9) — **each document keeps its own three**: four copies of
/// `settings.json`, four of `session.json` and two of `pins.json` lose one, one and none.
///
/// MUTATION: the copies of every document are counted together
/// (`plant-68f-documents-pooled.py`).
#[test]
fn each_document_keeps_its_own_three() {
    copies_on_a_worker(|worker| {
        let folder = copies_scratch("own-three");
        let mut oldest = Vec::new();
        for (document, count, second) in [
            ("settings.json", 4, 1),
            ("session.json", 4, 2),
            ("pins.json", 2, 3),
        ] {
            for day in 1..=count {
                let name = copy_written(
                    &folder,
                    document,
                    day,
                    second,
                    format!("{document}{day}").as_bytes(),
                );
                if day == 1 {
                    oldest.push(name);
                }
            }
        }
        let retired = retire_rejected_copies(worker, &folder, &mut [], |_| false, &mut |path| {
            std::fs::remove_file(path)
        })
        .unwrap();
        let said: Vec<(KeptDocument, usize, Vec<String>)> = retired
            .into_iter()
            .map(|answer| (answer.document, answer.kept, answer.removed))
            .collect();
        assert_eq!(
            said,
            [
                (KeptDocument::Session, 3, vec![oldest[1].clone()]),
                (KeptDocument::Settings, 3, vec![oldest[0].clone()]),
                (KeptDocument::Pins, 2, Vec::new()),
            ]
        );
        assert_eq!(names_in(&folder).len(), 8);
    });
}

/// RED (0.4.10 plan row 68f, R2) — **of copies holding the same bytes, the newest is kept**: three
/// copies, the oldest holding the newest's bytes, keep two; a copy of the same length and other
/// bytes is not one of them.
///
/// MUTATION: identical copies are not looked for (`plant-68f-no-dedupe.py`).
#[test]
fn an_older_copy_holding_the_same_bytes_is_removed() {
    copies_on_a_worker(|worker| {
        let folder = copies_scratch("same-bytes");
        let oldest = copy_written(&folder, "profiles.json", 1, 0, b"{\"schema_version\":9}");
        let middle = copy_written(&folder, "profiles.json", 2, 0, b"{\"schema_version\":8}");
        let newest = copy_written(&folder, "profiles.json", 3, 0, b"{\"schema_version\":9}");
        let retired = retire_rejected_copies(worker, &folder, &mut [], |_| false, &mut |path| {
            std::fs::remove_file(path)
        })
        .unwrap();
        assert_eq!(retired[0].removed, [oldest]);
        assert_eq!(retired[0].kept, 2);
        assert_eq!(names_in(&folder), [middle, newest]);
    });
}

/// RED (0.4.10 plan row 68f, R4/R5's copy) — **the copy a newer build could bring back is never
/// removed**: of five copies — three newer than this build, then two it reads — the newer of the
/// two is above the version of the file this start read, and is kept as the fourth; with a start
/// read at this build's own version, or no start read at all, both are retired as any other.
///
/// MUTATION: the copy is not looked for (`plant-68f-offered-copy-removed.py`).
#[test]
fn a_copy_a_newer_build_can_bring_back_is_never_removed() {
    copies_on_a_worker(|worker| {
        let readable = bt_persist::serialize_settings(&bt_persist::SettingsV1::default()).unwrap();
        let current = bt_persist::SETTINGS_SCHEMA_VERSION;
        let future = |n: u32| format!("{{\"schema_version\":{},\"n\":{n}}}", current + 1);
        let older_start: Arc<[u8]> = Arc::from(&b"{\"schema_version\":1}"[..]);
        let same_start: Arc<[u8]> = Arc::from(readable.as_slice());
        for (start, kept) in [
            (Some(older_start), true),
            (Some(same_start), false),
            (None, false),
        ] {
            let folder = copies_scratch("bring-back");
            let older_readable = copy_written(&folder, "settings.json", 1, 0, &readable);
            let mut newer = readable.clone();
            newer.extend_from_slice(b"\n");
            let brought_back = copy_written(&folder, "settings.json", 2, 0, &newer);
            for day in 3..=5 {
                copy_written(&folder, "settings.json", day, 0, future(day).as_bytes());
            }
            let mut opened = [OpenedDocument::noted(KeptDocument::Settings, start.clone())];
            let retired =
                retire_rejected_copies(worker, &folder, &mut opened, |_| false, &mut |path| {
                    std::fs::remove_file(path)
                })
                .unwrap();
            if kept {
                assert_eq!(
                    retired[0].removed,
                    [older_readable],
                    "the newest readable copy above the start's version is the one kept"
                );
                assert!(folder.join(&brought_back).is_file(), "kept as the fourth");
                assert_eq!(retired[0].kept, 4);
            } else {
                assert_eq!(
                    retired[0].removed,
                    [older_readable, brought_back],
                    "start {start:?}"
                );
            }
        }
    });
}

/// RED (0.4.10 plan row 68f, review N1 and N2) — **each document's protected copy is read by its
/// own parser**: for each of the five documents a store opens, of five copies — three above this
/// build, then two holding the document's default as its own `serialize_*` writes it — the newer of
/// the two is kept as the fourth when the start read the version before this build's; and a start
/// at this build's own version answers that no copy is protected without reading one. The
/// profiles get one more copy, newer than the kept one: `{"schema_version":1}`, which the pins
/// parser reads (any version-1 object) and the profiles' own refuses — it is not the protected copy
/// and goes beyond the three.
///
/// MUTATION: one document's arm of `this_build_reads` calls another document's parser
/// (`plant-68f-wrong-parser.py`; `plant-68f-profiles-read-by-pins.py`, the one pair whose wrong
/// parser reads the default); the early return is dropped, so a start at this build's version
/// reads every copy (`plant-68f-reads-on-a-normal-start.py`).
#[test]
fn each_documents_protected_copy_is_read_by_its_own_parser() {
    copies_on_a_worker(|worker| {
        let documents = [
            (
                KeptDocument::Session,
                bt_persist::serialize_session(&bt_persist::SessionV1::default()).unwrap(),
            ),
            (
                KeptDocument::Settings,
                bt_persist::serialize_settings(&bt_persist::SettingsV1::default()).unwrap(),
            ),
            (
                KeptDocument::Keybindings,
                bt_persist::serialize_keybindings(&bt_persist::KeybindingsV1::default()).unwrap(),
            ),
            (
                KeptDocument::Profiles,
                bt_persist::serialize_profiles(&bt_persist::ProfilesV1::default()).unwrap(),
            ),
            (
                KeptDocument::Pins,
                bt_persist::serialize_pins(&bt_persist::PinsV1::default()).unwrap(),
            ),
        ];
        for (document, readable) in documents {
            let name = document.kept_file_name();
            let current = document.current_version().unwrap();
            let future = |n: u32| format!("{{\"schema_version\":{},\"n\":{n}}}", current + 1);
            let folder = copies_scratch("own-parser");
            let older_readable = copy_written(&folder, name, 1, 0, &readable);
            let mut newer = readable.clone();
            newer.extend_from_slice(b"\n");
            let brought_back = copy_written(&folder, name, 2, 0, &newer);
            let decoy = (document == KeptDocument::Profiles)
                .then(|| copy_written(&folder, name, 2, 1, b"{\"schema_version\":1}"));
            for day in 3..=5 {
                copy_written(&folder, name, day, 0, future(day).as_bytes());
            }
            let older_start: Arc<[u8]> =
                Arc::from(format!("{{\"schema_version\":{}}}", current - 1).as_bytes());
            let mut opened = [OpenedDocument::noted(document, Some(older_start))];
            let retired =
                retire_rejected_copies(worker, &folder, &mut opened, |_| false, &mut |path| {
                    std::fs::remove_file(path)
                })
                .unwrap();
            let mut expected_removed = vec![older_readable];
            expected_removed.extend(decoy);
            assert_eq!(
                retired[0].removed, expected_removed,
                "{name}: the newest copy its own parser reads, above the start's version, is kept"
            );
            assert!(
                folder.join(&brought_back).is_file(),
                "{name}: kept as the fourth"
            );
            assert_eq!(retired[0].kept, 4, "{name}");

            let copies = rejected_copies_listed(worker, &folder).unwrap();
            let mut bytes = vec![None; copies.len()];
            assert_eq!(
                copy_a_newer_build_can_bring_back(
                    document,
                    bt_persist::schema_version_of(&readable),
                    &copies,
                    &mut bytes,
                ),
                None,
                "{name}: nothing stands above this build's own version"
            );
            assert!(
                bytes.iter().all(Option::is_none),
                "{name}: a start at this build's version reads no copy"
            );
        }
    });
}

/// RED (0.4.10 plan row 68f, fold re-review F2) — **the newest copy only a newer build reads is
/// kept whatever the count**: at an ordinary start (the file at this build's version), of six
/// `settings.json` copies — three this build reads, then one above this build's version, then an
/// old one, then an older one above — the newer one above is kept as the fourth and the other two
/// go: of two above, the older goes as any other.
///
/// MUTATION: the copy only a newer build reads is not looked for
/// (`plant-68f-newer-copy-retired.py`).
#[test]
fn the_newest_copy_only_a_newer_build_reads_is_kept() {
    copies_on_a_worker(|worker| {
        let current = bt_persist::SETTINGS_SCHEMA_VERSION;
        let readable = bt_persist::serialize_settings(&bt_persist::SettingsV1::default()).unwrap();
        let start: Arc<[u8]> = Arc::from(readable.as_slice());
        let above = |n: u32| format!("{{\"schema_version\":{},\"n\":{n}}}", current + 1);
        let folder = copies_scratch("newer-build");
        let oldest_above = copy_written(&folder, "settings.json", 1, 0, above(1).as_bytes());
        let oldest = copy_written(&folder, "settings.json", 2, 0, b"{\"schema_version\":1}");
        let newer_build = copy_written(&folder, "settings.json", 3, 0, above(3).as_bytes());
        for day in 4..=6 {
            let mut bytes = readable.clone();
            bytes.extend(std::iter::repeat_n(b' ', day as usize));
            copy_written(&folder, "settings.json", day, 0, &bytes);
        }
        let mut opened = [OpenedDocument::noted(KeptDocument::Settings, Some(start))];
        let retired =
            retire_rejected_copies(worker, &folder, &mut opened, |_| false, &mut |path| {
                std::fs::remove_file(path)
            })
            .unwrap();
        assert_eq!(
            retired[0].removed,
            [oldest_above, oldest],
            "only the newest copy above this build's version is kept beyond the three"
        );
        assert!(folder.join(&newer_build).is_file(), "kept as the fourth");
        assert_eq!(retired[0].kept, 4);
    });
}

/// RED (0.4.10 plan row 68f, R1) — **during an update's trial nothing is removed**: the gate is
/// asked once for `Writer::RefusedCopies` when removals are due across two documents, holds them
/// all, and says so; the commit's round removes them; a round with nothing due asks no gate.
///
/// MUTATION: the removals do not ask the trial's gate (`plant-68f-unheld.py`).
#[test]
fn the_retirement_waits_for_the_trials_commit() {
    copies_on_a_worker(|worker| {
        let folder = copies_scratch("trial");
        for day in 1..=4 {
            copy_written(
                &folder,
                "settings.json",
                day,
                0,
                format!("s{day}").as_bytes(),
            );
            copy_written(
                &folder,
                "keybindings.json",
                day,
                0,
                format!("k{day}").as_bytes(),
            );
        }
        let before = names_in(&folder);
        let gate = crate::update_trial::Gate::new();
        let asked = std::cell::Cell::new(0);
        let held = retire_rejected_copies(
            worker,
            &folder,
            &mut [],
            |writer| {
                asked.set(asked.get() + 1);
                gate.defer(true, writer)
            },
            &mut |path| panic!("{} is removed during a trial", path.display()),
        )
        .unwrap();
        assert_eq!(asked.get(), 1, "the gate is asked once a round");
        assert!(
            held.iter()
                .all(|answer| answer.held && answer.removed.is_empty())
        );
        assert_eq!(
            held[0].copies_lines(),
            ["BT_REJECTED_COPIES settings.json held"]
        );
        assert_eq!(names_in(&folder), before, "nothing removed");
        assert_eq!(gate.pending(), vec![Writer::RefusedCopies]);

        let committed = retire_rejected_copies(worker, &folder, &mut [], |_| false, &mut |path| {
            std::fs::remove_file(path)
        })
        .unwrap();
        assert!(committed.iter().all(|answer| answer.removed.len() == 1));
        retire_rejected_copies(
            worker,
            &folder,
            &mut [],
            copies_gate_never_asked,
            &mut |_| panic!("nothing is due"),
        )
        .unwrap();
    });
}

/// RED (0.4.10 plan row 68f) — **a copy that will not go is left and said, and the others still
/// go**: of six copies, the oldest refuses its removal; the other two due are removed after it.
///
/// MUTATION: the round stops at the first refusal (`plant-68f-stops-at-a-refusal.py`).
#[test]
fn a_copy_that_will_not_go_is_left_and_said() {
    copies_on_a_worker(|worker| {
        let folder = copies_scratch("will-not-go");
        let names: Vec<String> = (1..=6)
            .map(|day| copy_written(&folder, "pins.json", day, 0, format!("{day}").as_bytes()))
            .collect();
        let stubborn = folder.join(&names[0]);
        let retired = retire_rejected_copies(worker, &folder, &mut [], |_| false, &mut |path| {
            if path == stubborn {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                std::fs::remove_file(path)
            }
        })
        .unwrap();
        assert_eq!(
            retired[0].copies_lines(),
            [
                format!(
                    "BT_REJECTED_COPIES pins.json kept=4 removed={},{}",
                    names[1], names[2]
                ),
                format!(
                    "BT_REJECTED_COPIES pins.json left {} (PermissionDenied)",
                    names[0]
                ),
            ]
        );
        assert!(stubborn.is_file());
    });
}

/// RED (0.4.10 plan row 68f) — **only the process that writes the data folder retires copies**:
/// another one's member answers nothing, removes nothing and says nothing; the writer's removes
/// and says.
///
/// MUTATION: every process retires (`plant-68f-any-process-retires.py`).
#[test]
fn only_the_data_folders_writer_retires_copies() {
    copies_on_a_worker(|worker| {
        let folder = copies_scratch("writer-only");
        for day in 1..=4 {
            copy_written(&folder, "session.json", day, 0, format!("{day}").as_bytes());
        }
        let said = OrphansSaid::new();
        let lines = RefCell::new(Vec::new());
        let answer = rejected_copies_member(
            worker,
            false,
            &folder,
            &mut [],
            copies_gate_never_asked,
            &mut |path| {
                panic!(
                    "{} is removed by a process that is not the writer",
                    path.display()
                )
            },
            &said,
            &|line| lines.borrow_mut().push(line),
        );
        assert_eq!(answer, None);
        assert!(lines.borrow().is_empty());
        assert_eq!(names_in(&folder).len(), 4);
        assert_eq!(member_round(worker, &folder, &mut [], &said).len(), 1);
        assert_eq!(names_in(&folder).len(), 3);
    });
}

/// `document`'s start, which refused a newer file and kept it as `copy`.
fn refused_newer(document: KeptDocument, copy: &str) -> OpenedDocument {
    let mut entry = OpenedDocument::noted(document, None);
    entry.refused_newer = Some(copy.to_owned());
    entry
}

/// `document`'s start, which read bytes claiming `version`.
fn read_at_version(document: KeptDocument, version: u32) -> OpenedDocument {
    let bytes = format!("{{\"schema_version\":{version}}}");
    OpenedDocument::noted(document, Some(Arc::from(bytes.as_bytes())))
}

/// The copies a round said, by name.
fn said_copies(retired: &[CopiesRetired]) -> Vec<String> {
    retired
        .iter()
        .filter_map(|answer| answer.said.as_ref().map(|said| said.copy.clone()))
        .collect()
}

/// One round over `folder` with `opened`, no trial, the disk's own removal.
fn round_over(
    worker: &WorkerCtx,
    folder: &Path,
    opened: &mut [OpenedDocument],
) -> Vec<CopiesRetired> {
    retire_rejected_copies(worker, folder, opened, |_| false, &mut |path| {
        std::fs::remove_file(path)
    })
    .unwrap()
}

/// RED (0.4.10 plan row 68d, R4) — **a newer document an older build refused is said once**:
/// the first refusal is said, naming its copy; a later round of the same process says nothing;
/// the next start's refusal of the same bytes says nothing, and the older identical copy goes,
/// one copy left; a refusal of other bytes is said again.
///
/// MUTATION: the notice is said whatever the other copies hold
/// (`plant-68d-said-every-start.py`).
#[test]
fn a_newer_document_refused_is_said_once() {
    copies_on_a_worker(|worker| {
        let folder = copies_scratch("said-once");
        let newer = format!(
            "{{\"schema_version\":{}}}",
            bt_persist::SETTINGS_SCHEMA_VERSION + 1
        );
        let first = copy_written(&folder, "settings.json", 1, 0, newer.as_bytes());
        let mut opened = [refused_newer(KeptDocument::Settings, &first)];
        let retired = round_over(worker, &folder, &mut opened);
        assert_eq!(
            said_copies(&retired),
            std::slice::from_ref(&first),
            "the first refusal"
        );
        assert!(
            retired[0]
                .copies_lines()
                .contains(&format!("BT_REJECTED_COPIES settings.json said {first}")),
            "and its line"
        );
        assert!(
            said_copies(&round_over(worker, &folder, &mut opened)).is_empty(),
            "a later round of the same process"
        );

        let second = copy_written(&folder, "settings.json", 2, 0, newer.as_bytes());
        let mut opened = [refused_newer(KeptDocument::Settings, &second)];
        let retired = round_over(worker, &folder, &mut opened);
        assert!(
            said_copies(&retired).is_empty(),
            "the same bytes at the next start were said when the first copy was made"
        );
        assert_eq!(retired[0].removed, [first], "the older identical copy goes");
        assert_eq!(names_in(&folder), [second]);

        let other = format!(
            "{{\"schema_version\":{},\"another\":true}}",
            bt_persist::SETTINGS_SCHEMA_VERSION + 1
        );
        let third = copy_written(&folder, "settings.json", 3, 0, other.as_bytes());
        let mut opened = [refused_newer(KeptDocument::Settings, &third)];
        assert_eq!(
            said_copies(&round_over(worker, &folder, &mut opened)),
            [third],
            "other bytes are another refusal"
        );
    });
}

/// RED (row 68d, R5) — **the copy offered is the newest one above the version the start read
/// that this build reads**: a copy still newer than this build, one that will not parse and one
/// at the start's own version are passed over, and the offer names the copy and both versions in
/// its line; a start at this build's version is offered nothing.
///
/// MUTATION: a copy at the start's own version counts as above it
/// (`plant-68d-offers-an-older-copy.py`).
#[test]
fn a_copy_newer_than_the_older_builds_write_is_offered() {
    copies_on_a_worker(|worker| {
        let current = bt_persist::SETTINGS_SCHEMA_VERSION;
        let readable = bt_persist::serialize_settings(&bt_persist::SettingsV1::default()).unwrap();
        let mut at_the_start: serde_json::Value = serde_json::from_slice(&readable).unwrap();
        at_the_start["schema_version"] = serde_json::json!(current - 1);
        let at_the_start = serde_json::to_vec(&at_the_start).unwrap();
        assert!(
            KeptDocument::Settings.this_build_reads(&at_the_start),
            "the start's own version is one this build reads"
        );
        for (start, offered) in [(current - 1, true), (current, false)] {
            let folder = copies_scratch("offered");
            let copy = copy_written(&folder, "settings.json", 1, 0, &readable);
            copy_written(
                &folder,
                "settings.json",
                2,
                0,
                format!("{{\"schema_version\":{}}}", current + 1).as_bytes(),
            );
            copy_written(
                &folder,
                "settings.json",
                3,
                0,
                format!("{{\"schema_version\":{current},\"git_panel\":\"no\"}}").as_bytes(),
            );
            copy_written(&folder, "settings.json", 4, 0, &at_the_start);
            let mut opened = [read_at_version(KeptDocument::Settings, start)];
            let retired = round_over(worker, &folder, &mut opened);
            let offer = retired[0].offered.as_ref();
            if offered {
                let offer = offer.expect("a copy above the start's version is offered");
                assert_eq!(offer.copy, copy);
                assert_eq!((offer.copy_version, offer.file_version), (current, start));
                assert_eq!(&*offer.bytes, &readable[..]);
                assert!(retired[0].copies_lines().contains(&format!(
                    "BT_REJECTED_COPIES settings.json offered {copy} v{current} over v{start}"
                )));
            } else {
                assert_eq!(offer, None, "a start at this build's version");
            }
        }
    });
}

/// RED (row 68d, R5) — **the offer reads the version the start read, not the file now**: a file
/// written at this build's version after the store opened (the layout's autosave, a press) still
/// leaves the copy offered at this start.
///
/// MUTATION: the member reads the file's version now (`plant-68d-reads-the-file-now.py`).
#[test]
fn the_offer_reads_the_version_the_start_read() {
    copies_on_a_worker(|worker| {
        let current = bt_persist::SETTINGS_SCHEMA_VERSION;
        let readable = bt_persist::serialize_settings(&bt_persist::SettingsV1::default()).unwrap();
        let folder = copies_scratch("start-version");
        std::fs::write(folder.join("settings.json"), &readable).unwrap();
        let copy = copy_written(&folder, "settings.json", 1, 0, &readable);
        let mut opened = [read_at_version(KeptDocument::Settings, current - 1)];
        let retired = round_over(worker, &folder, &mut opened);
        assert_eq!(
            retired[0].offered.as_ref().map(|offer| offer.copy.as_str()),
            Some(copy.as_str())
        );
        assert!(
            matches!(opened[0].read, StartRead::Version(Some(version)) if version == current - 1),
            "the start's version is kept in place of its bytes"
        );
    });
}

/// PIN (row 68d; 68b's S2 shape) — **the window thread lists and reads no refused copy**: the
/// start (`Runtime::create`), the refusal's note (`persist::read_fault`), the adoption of the
/// pass's round and the card's verbs take what the pass's worker read, and a Bring back's disk
/// work is the storage worker's.
///
/// MUTATION: the Bring back reads the copy again on the window
/// (`plant-68d-copy-read-on-the-window.py`).
#[test]
fn the_window_thread_lists_no_rejected_copy() {
    use crate::test_support::{free_fn_body, method_body, squeezed};
    let window_thread = [
        ("Runtime::create", method_body("Runtime", "create")),
        (
            "FolioApp::adopt_carried",
            method_body("FolioApp", "adopt_carried"),
        ),
        ("read_fault", free_fn_body("read_fault")),
        ("note_opened_document", free_fn_body("note_opened_document")),
        ("note_refused_newer", free_fn_body("note_refused_newer")),
        (
            "Runtime::answer_copy_back_card",
            method_body("Runtime", "answer_copy_back_card"),
        ),
        (
            "Runtime::bring_back_copy",
            method_body("Runtime", "bring_back_copy"),
        ),
        (
            "Runtime::keep_current_document",
            method_body("Runtime", "keep_current_document"),
        ),
        (
            "Runtime::say_copy_back_answers",
            method_body("Runtime", "say_copy_back_answers"),
        ),
        ("layout_to_bring_back", free_fn_body("layout_to_bring_back")),
    ];
    for (name, body) in window_thread {
        let body = squeezed(body);
        for read in [
            "read_dir(",
            "read_kept_copy(",
            "rejected_copies_listed(",
            "retire_rejected_copies(",
            "keep_beside_as_rejected(",
            "symlink_metadata(",
            "std::fs::",
        ] {
            assert!(
                !body.contains(read),
                "{name} reads or writes a refused copy on the window thread: {read}"
            );
        }
    }
}

/// RED (row 68d, R4; review N7) — **a kept copy whose bytes cannot be read is said**: a failed
/// read is no evidence that the refusal was said before, and the start said nothing.
///
/// MUTATION: a copy that could not be read counts as said before
/// (`plant-68d-unread-copy-not-said.py`).
#[test]
fn a_kept_copy_that_cannot_be_read_is_said() {
    let name = "settings.json.rejected-20261115-000001";
    let copies = [FoundCopy {
        document: KeptDocument::Settings,
        stamp: bt_persist::rejected_stamp_of(name, "settings.json").expect("the namer's name"),
        name: name.to_owned(),
        path: bt_testpath::temp_path("rejected-copies-never-made").join(name),
        length: 10,
    }];
    let mut bytes = vec![None; copies.len()];
    let mut entry = refused_newer(KeptDocument::Settings, name);
    let said = newer_refusal_to_say(&mut entry, &copies, &mut bytes);
    assert_eq!(said.map(|notice| notice.copy).as_deref(), Some(name));
    assert!(entry.refusal_answered, "and answered, once");
}
