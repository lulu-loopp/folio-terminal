//! **The window's frame and the references printed in it, driven** (T-HEADLESS-RUNTIME-SEAM):
//! a press on a printed reference, the compose of a frame and the present, on a headless window.

use crate::headless::{HeadlessFolio, WindowEffect};
use crate::test_support::{hyperlink_hit, leaf_saying, tab_holding};
use std::path::{Path, PathBuf};

/// `path` as the `file:` URI a program prints for it.
fn file_uri(path: &Path) -> String {
    let spelled = path.display().to_string().replace('\\', "/");
    if spelled.starts_with('/') {
        format!("file://{spelled}")
    } else {
        format!("file:///{spelled}")
    }
}

/// The hand-offs the window made since this was last asked.
fn handed_off(folio: &HeadlessFolio) -> Vec<bt_platform::Handoff> {
    folio
        .ledger
        .take()
        .into_iter()
        .filter_map(|effect| match effect {
            WindowEffect::HandedOff(handoff) => Some(handoff),
            _ => None,
        })
        .collect()
}

/// RED (closure review r6, B-2; CC-4) — **the open door is handed the name the program printed,
/// and the reveal the same name with the doors' own resolution of it riding beside it.**
///
/// The path-verification lane answers a printed name with the doors' own transform
/// (`bt_platform::resolved_for_a_door`), the pane's ledger keeps that answer, and a `Ctrl`+click
/// on the reference hands the system the name exactly as printed — a file to its handler, a
/// folder to the file manager — with the resolved name on the target, where the macOS door takes
/// it from.
///
/// MUTATIONS: hand `bt_term::verify_path` a raw `std::fs::canonicalize` in
/// `run_path_verify_worker` — the resolved name is not the doors' (Windows keeps the verbatim
/// `\\?\` prefix); hand `open_local_path_verified` the resolved name in
/// `Runtime::activate_hyperlink` — the open door is asked about a name nobody printed.
#[test]
fn the_open_door_is_handed_the_printed_name_and_the_reveal_the_resolved_one() {
    let root = bt_testpath::temp_path("headless-printed-name");
    std::fs::create_dir_all(root.join("src").join("docs")).expect("the scratch tree");
    std::fs::write(root.join("src").join("notes.txt"), "笔记 notes").expect("a file");
    let file = root.join("src").join("..").join("src").join("notes.txt");
    let folder = root.join("src").join("..").join("src").join("docs");

    // The lane's answer, from the worker the window's questions go to.
    let (requests, task_rx) = std::sync::mpsc::channel();
    let (result_tx, results) = std::sync::mpsc::channel();
    let worker =
        std::thread::spawn(move || crate::run_path_verify_worker(task_rx, result_tx, || {}));
    let mut verdicts = Vec::new();
    for printed in [&file, &folder] {
        requests
            .send(crate::PathWorkerRequest {
                leaf: crate::test_support::probe_leaf(),
                path: printed.clone(),
            })
            .expect("the worker takes the question");
        let answer = crate::lane::receive(&results, "the path worker's answer");
        let crate::DecorationWorkerCompletion::VerifiedPath { path, verdict } = answer.completion
        else {
            panic!("the path worker answers a path question");
        };
        assert_eq!(&path, printed, "the answer is about the name asked");
        assert_eq!(
            verdict.door_ready,
            bt_platform::resolved_for_a_door(printed),
            "the resolved name is the doors' own transform"
        );
        verdicts.push(verdict);
    }
    drop(requests);
    worker.join().expect("the worker ends with its queue");

    for (printed, verdict) in [&file, &folder].into_iter().zip(verdicts) {
        let leaf = leaf_saying("参考 reference");
        let mut folio =
            HeadlessFolio::with_tabs("headless-printed-open", vec![tab_holding(leaf)], 0);
        let seat = folio.runtime().seats.identity();
        folio
            .runtime()
            .focused_mut()
            .expect("the shell")
            .session
            .complete_path_verification(printed.clone(), verdict.clone());
        folio
            .runtime()
            .activate_hyperlink(seat, hyperlink_hit(&file_uri(printed)), true)
            .expect("the reference is followed");
        let target = crate::verified_target_of(Some(&verdict));
        let expected = if verdict.directory {
            bt_platform::Handoff::RevealVerified(PathBuf::from(printed), target)
        } else {
            bt_platform::Handoff::OpenVerified(PathBuf::from(printed), target)
        };
        assert_eq!(
            handed_off(&folio),
            [expected],
            "the door is handed the printed name, and the resolved one rides on its target"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
