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

/// A window of two shells side by side, the keyboard in the first, whose decoration requests go
/// to a channel the test reads rather than to the worker.
fn two_shells(
    tag: &str,
) -> (
    HeadlessFolio,
    bt_layout::SeatId,
    std::sync::mpsc::Receiver<crate::MathWorkerRequest>,
) {
    let tab = crate::test_support::cross_tab(1, &["键盘 focused", "旁边 beside"]);
    let beside = tab.seats.terminals()[1];
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
    let (tasks, requests) = std::sync::mpsc::channel();
    let (scale_tasks, _) = std::sync::mpsc::channel();
    let (path_tasks, _) = std::sync::mpsc::channel();
    let (_, results) = std::sync::mpsc::channel();
    folio.app.math_worker = crate::MathWorker {
        tasks,
        scale_tasks,
        path_tasks,
        results,
    };
    (folio, beside, requests)
}

/// The seats of the formulas handed to the typesetting worker since this was last asked.
fn typeset_for(
    requests: &std::sync::mpsc::Receiver<crate::MathWorkerRequest>,
) -> Vec<bt_layout::SeatId> {
    requests
        .try_iter()
        .filter_map(|request| match request {
            crate::MathWorkerRequest::Math { leaf, .. } => Some(leaf.leaf.seat),
            _ => None,
        })
        .collect()
}

/// RED (review row R5-4) — **every pane the reader can see settles its rows and typesets what it
/// printed**, not only the pane holding the keyboard. A formula already in the transcript of the
/// pane beside the focused one is scheduled and handed to the typesetting worker by the pass that
/// projects that pane; a display formula still on its live rows there gives the window a
/// stability deadline, settles when it passes, and is handed over in its turn.
///
/// MUTATIONS: drop `bt_compose::schedule` from `Runtime::redraw`'s projection of the other panes,
/// or the `dispatch_tab_decoration_tasks` after it — the lost formula is not asked for again;
/// ask only the focused pane in `Runtime::live_stability_deadline` (`self.shell()`) — the window
/// sleeps through the row it owes; settle only the focused pane in
/// `Runtime::advance_live_math_if_due` — the live formula is never typeset.
#[test]
fn every_visible_pane_settles_its_rows_and_schedules_its_own_artifacts() {
    let (mut folio, beside, requests) = two_shells("headless-every-pane");
    // The panes' first size, told and quiet: the window's own turns, a generous interval apart,
    // until the pane beside has closed the resize its layout opened.
    let mut now = std::time::Instant::now();
    folio.draw();
    for _ in 0..8 {
        if !folio.runtime().sessions[&beside]
            .session
            .resize_transaction_open()
        {
            break;
        }
        now += std::time::Duration::from_millis(250);
        folio
            .runtime()
            .turn(now, true)
            .expect("the turn is answered");
        folio.draw();
    }
    assert!(
        !folio.runtime().sessions[&beside]
            .session
            .resize_transaction_open(),
        "the fixture's own precondition: the pane beside has settled its first size"
    );
    let _ = typeset_for(&requests);

    // A formula on the live rows, typeset once the row it stands on has settled.
    folio
        .runtime()
        .sessions
        .get_mut(&beside)
        .expect("the pane beside")
        .session
        .feed(b"$$x^2$$\r\n")
        .expect("the formula is printed");
    let deadline = folio
        .runtime()
        .live_stability_deadline()
        .expect("the window wakes for the row the pane beside owes");
    folio
        .runtime()
        .turn(deadline, true)
        .expect("the turn is answered");
    folio.draw();
    let typeset = typeset_for(&requests);
    assert!(
        typeset.contains(&beside),
        "the live formula in the pane beside is handed to the typesetting worker: {typeset:?}"
    );

    // A formula in the transcript whose typesetting was lost (the worker gone with the request,
    // `DualPlaneSession::forget_work_in_flight`): nothing re-asks for it but the scan of the
    // frame that shows it, so the pass that projects the pane beside is what asks again.
    {
        let mut runtime = folio.runtime();
        let pane = runtime.sessions.get_mut(&beside).expect("the pane beside");
        let mut printed = b"\r\n$$a^2$$\r\n".to_vec();
        printed.extend(b"\r\n".repeat(64));
        pane.session.feed(&printed).expect("the formula is printed");
        pane.session.refresh_projection(&mut pane.projection);
        pane.session
            .viewport_frame(&mut pane.projection)
            .expect("the pane projects");
        pane.projection.scroll_to_top();
    }
    folio.draw();
    let asked = typeset_for(&requests);
    assert!(
        asked.contains(&beside),
        "the fixture's own precondition: the transcript's formula was asked for: {asked:?}"
    );
    folio
        .runtime()
        .sessions
        .get_mut(&beside)
        .expect("the pane beside")
        .session
        .forget_work_in_flight();
    folio.draw();
    let typeset = typeset_for(&requests);
    assert!(
        typeset.contains(&beside),
        "the frame that shows the pane beside again asks for the formula whose work was lost: \
         {typeset:?}"
    );
}

/// RED (ticket 37) — ***Restart shell* keeps the rung, and the old shell stays until its
/// replacement exists**: the replacement is born at the pane's own text size, read off the leaf
/// it replaces, and waits beside the old leaf — which goes on being the pane — while it is being
/// born.
///
/// MUTATIONS: build the replacement's view with `TextScale::ACTUAL` in `Runtime::restart_shell`
/// — a pane at 125 % comes back at 100 %; take the old leaf out of the seat before the spawn —
/// the pane is empty while its replacement is born.
#[test]
fn restart_shell_keeps_the_rung_and_the_old_leaf_until_its_replacement_exists() {
    let mut folio = HeadlessFolio::with_tabs(
        "headless-restart",
        vec![crate::test_support::tab_holding(leaf_saying(
            "旧壳 old shell",
        ))],
        0,
    );
    let mut runtime = folio.runtime();
    let seat = runtime.seats.identity();
    let leaf = runtime.sessions.get_mut(&seat).expect("the shell");
    crate::step_leaf_text_scale(leaf, crate::TextStep::Larger, 2, |scale| {
        Ok(crate::fixture_cell_metrics(
            1.0,
            scale.effective_logical_px(14.0),
        ))
    })
    .expect("the pane steps up");
    let rung = runtime.sessions[&seat].text_scale;
    let old = runtime.sessions[&seat].incarnation;
    assert!(
        !rung.is_actual(),
        "the fixture's own precondition: a pane off 100 %"
    );
    runtime
        .restart_shell(seat)
        .expect("the restart is asked for");
    let pane = runtime
        .sessions
        .get(&seat)
        .expect("the seat still holds a shell while its replacement is born");
    assert_eq!(pane.incarnation, old, "the old shell is still the pane");
    let successor = pane
        .successor
        .as_ref()
        .expect("its replacement waits beside it while it is born");
    assert_eq!(
        successor.text_scale, rung,
        "and is born at the pane's own rung"
    );
}
