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
    opened: &[OpenedDocument],
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
            &[],
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
            member_round(worker, &folder, &[], &said),
            Vec::<String>::new()
        );
        let quiet =
            retire_rejected_copies(worker, &folder, &[], copies_gate_never_asked, &mut |_| {
                panic!("nothing is due")
            })
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
/// storage worker's `.displaced-` copy, a name close to a document's, a folder with a copy's
/// name, and on Unix a link with one are never removed and never counted among the three.
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
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            folder.join("settings.json.rejected-20261101-000000"),
            folder.join("settings.json.rejected-20261102-000002"),
        )
        .unwrap();
        let before = names_in(&folder);
        let retired =
            retire_rejected_copies(worker, &folder, &[], copies_gate_never_asked, &mut |path| {
                panic!("{} is removed", path.display())
            })
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
        let retired = retire_rejected_copies(worker, &folder, &[], |_| false, &mut |path| {
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
        let retired = retire_rejected_copies(worker, &folder, &[], |_| false, &mut |path| {
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
            let opened = [(KeptDocument::Settings, start.clone())];
            let retired =
                retire_rejected_copies(worker, &folder, &opened, |_| false, &mut |path| {
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
            &[],
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

        let committed = retire_rejected_copies(worker, &folder, &[], |_| false, &mut |path| {
            std::fs::remove_file(path)
        })
        .unwrap();
        assert!(committed.iter().all(|answer| answer.removed.len() == 1));
        retire_rejected_copies(worker, &folder, &[], copies_gate_never_asked, &mut |_| {
            panic!("nothing is due")
        })
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
        let retired = retire_rejected_copies(worker, &folder, &[], |_| false, &mut |path| {
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
            &[],
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
        assert_eq!(member_round(worker, &folder, &[], &said).len(), 1);
        assert_eq!(names_in(&folder).len(), 3);
    });
}
