//! **The window's frame and the references printed in it, driven** (T-HEADLESS-RUNTIME-SEAM):
//! a press on a printed reference, the compose of a frame and the present, on a headless window.

use crate::computation_lane::{self, fixtures::Watched};
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

    // The lane's answer, from the share the window's questions go to.
    let mut watched = computation_lane::fixtures::watched(crate::lane::Executor::scripted(
        std::sync::Arc::new(crate::lane::Gate::default()),
    ));
    let mut verdicts = Vec::new();
    for printed in [&file, &folder] {
        let asked = watched
            .lane
            .verify(crate::PathWorkerRequest {
                leaf: crate::test_support::probe_leaf(),
                path: printed.clone(),
            })
            .ok()
            .expect("the lane takes the question");
        let terminal = watched.terminal(
            computation_lane::Share::Path,
            asked,
            computation_lane::fixtures::nobody,
        );
        let answer = computation_lane::fixtures::answer(terminal).expect("the path share answers");
        let crate::DecorationWorkerCompletion::VerifiedPath { path, verdict } = answer.completion
        else {
            panic!("the path share answers a path question");
        };
        assert_eq!(&path, printed, "the answer is about the name asked");
        assert_eq!(
            verdict.door_ready,
            bt_platform::resolved_for_a_door(printed),
            "the resolved name is the doors' own transform"
        );
        verdicts.push(verdict);
    }

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
/// to a computation lane the test watches.
fn two_shells(tag: &str) -> (HeadlessFolio, bt_layout::SeatId, Watched) {
    let tab = crate::test_support::cross_tab(1, &["键盘 focused", "旁边 beside"]);
    let beside = tab.seats.terminals()[1];
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
    let watched = computation_lane::fixtures::watched(crate::lane::Executor::scripted(
        std::sync::Arc::new(crate::lane::Gate::default()),
    ));
    folio.app.math_worker = std::sync::Arc::clone(&watched.lane);
    (folio, beside, watched)
}

/// How many formulas and pictures the window has handed the lane: every request it admitted,
/// answered or not (nothing here drains them).
fn asked(watched: &Watched) -> usize {
    watched.lane.held(computation_lane::Share::Decoration)
}

/// The panes the next `count` answers of the lane were asked for, as the lane answers them.
fn answered_for(watched: &mut Watched, count: usize) -> Vec<bt_layout::SeatId> {
    (0..count)
        .map(|_| {
            let (_, _, terminal, _) = watched.next_ended(
                computation_lane::Share::Decoration,
                computation_lane::fixtures::nobody,
            );
            computation_lane::fixtures::answer(terminal)
                .expect("the lane answers a formula")
                .leaf
                .leaf
                .seat
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
    let (mut folio, beside, mut watched) = two_shells("headless-every-pane");
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
    let before = asked(&watched);

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
    assert!(
        asked(&watched) > before,
        "the live formula in the pane beside is handed to the typesetting lane"
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
        pane.session.refresh_projection(&mut pane.view.projection);
        pane.session
            .viewport_frame(&mut pane.view.projection)
            .expect("the pane projects");
        pane.view.projection.scroll_to_top();
    }
    let before = asked(&watched);
    folio.draw();
    assert!(
        asked(&watched) > before,
        "the fixture's own precondition: the transcript's formula was asked for"
    );
    folio
        .runtime()
        .sessions
        .get_mut(&beside)
        .expect("the pane beside")
        .session
        .forget_work_in_flight();
    let before = asked(&watched);
    folio.draw();
    assert!(
        asked(&watched) > before,
        "the frame that shows the pane beside again asks for the formula whose work was lost"
    );
    let all = asked(&watched);
    let seats = answered_for(&mut watched, all);
    assert!(
        seats.iter().all(|seat| *seat == beside),
        "every formula the window asked for was the pane beside's: {seats:?}"
    );
}

/// RED (ticket 37) — ***Restart shell* keeps the rung, and the old shell stays until its
/// replacement exists**: the replacement is born at the pane's own text size, read off the leaf
/// it replaces, and waits beside the old leaf — which goes on being the pane — while it is being
/// born.
///
/// MUTATIONS: build the replacement's view with `TextScale::ACTUAL` in `Runtime::restart_shell`
/// — a pane at 125 % comes back at 100 %; take the old leaf out of the seat before the spawn —
/// the pane is empty while its replacement is born; drop `self.sessions.insert(seat, spawned)`
/// from `Runtime::replace_restarted_shell` — the old shell stays the pane after its replacement
/// landed.
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
    let rung = runtime.sessions[&seat].view.text_scale;
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
        successor.view.text_scale, rung,
        "and is born at the pane's own rung"
    );

    // Its landing: the replacement takes the seat by the insert that drops the old shell.
    let born = successor.incarnation;
    let successor = runtime
        .sessions
        .get_mut(&seat)
        .and_then(|pane| pane.take_successor())
        .expect("the replacement");
    runtime
        .replace_restarted_shell(seat, *successor)
        .expect("the replacement lands");
    let pane = runtime.sessions.get(&seat).expect("the seat holds a shell");
    assert_eq!(
        pane.incarnation, born,
        "the replacement is the pane once it has landed, and the old shell is gone"
    );
    assert_eq!(pane.view.text_scale, rung, "at the pane's own rung");
}

/// RED (T-CWD-FROM-PROCESS, round 2) — **the window draws "Folder unknown" on the foot of a
/// column opened on the home folder for a pane's folder nobody could say**: read off the chrome
/// labels the headless window's glass was handed for the frame, so every layer between the
/// column's state and the drawn strip is on the road — the foot's dressing
/// (`Runtime::dress_files_feet`), the tree content carrying it, and the seat painter hanging it
/// (`FootStrip::notice`). A column whose root is known draws no such label.
///
/// MUTATIONS (observed red): drop `tree.foot_notice = words.notice` in `dress_files_feet`; hand
/// the column's `FootStrip` an empty `notice` in the seat painter — either way no label says it.
#[test]
fn a_column_on_a_folder_nobody_could_say_draws_folder_unknown_on_its_foot() {
    let root = bt_testpath::temp_path("files-foot-unknown");
    let mut folio = HeadlessFolio::with_tabs(
        "files-foot-unknown",
        vec![crate::test_support::tab_with_a_files_column(
            1,
            &root.display().to_string(),
        )],
        0,
    );
    let unknown = crate::i18n::Text::FilesFolderUnknown.text();
    let drawn = |folio: &mut HeadlessFolio| -> bool {
        folio.draw();
        let runtime = folio.runtime();
        let labels = &runtime.window.renderer.chrome.1;
        labels.iter().any(|label| label.text == unknown)
    };
    assert!(!drawn(&mut folio), "a known root draws no such label");

    {
        let mut runtime = folio.runtime();
        let seat = runtime
            .seats
            .files_seat()
            .expect("the tab has a files column");
        runtime
            .files
            .get_mut(&seat)
            .expect("the column's state")
            .folder_unknown = true;
        runtime.refresh_chrome();
    }
    assert!(
        drawn(&mut folio),
        "the column's foot draws \"Folder unknown\""
    );
}

/// RED (T-CWD-FROM-PROCESS, round 2) — **the root menu takes the shells' folders once, as it
/// opens**: it is laid out every frame it is up, and a pane's folder is read on an ask. A shell
/// that reports another folder while the menu is open changes nothing on it; the next opening
/// reads again.
///
/// MUTATION (observed red): `root_choices` reading every pane through `LeafSession::folder` (the
/// per-frame read) — the open menu offers the new folder.
#[test]
fn the_root_menu_reads_the_shells_folders_once_as_it_opens() {
    let root = bt_testpath::temp_path("root-menu-once");
    let first = root.join("first 一");
    let second = root.join("second 二");
    let mut folio = HeadlessFolio::with_tabs(
        "root-menu-once",
        vec![crate::test_support::tab_with_a_files_column(
            1,
            &root.display().to_string(),
        )],
        0,
    );
    let mut runtime = folio.runtime();
    let column = runtime
        .seats
        .files_seat()
        .expect("the tab has a files column");
    let terminal = runtime.seats.terminals()[0];
    let report = |folder: &Path| format!("\u{1b}]7;{}\u{7}", file_uri(folder));
    let say = |runtime: &mut crate::Runtime<'_, crate::headless::Headless>, folder: &Path| {
        runtime
            .sessions
            .get_mut(&terminal)
            .expect("the terminal pane")
            .session
            .feed(report(folder).as_bytes())
            .expect("the report is read");
    };
    let offers = |runtime: &crate::Runtime<'_, crate::headless::Headless>, folder: &Path| {
        runtime
            .root_choices(column)
            .iter()
            .any(|choice| Path::new(&choice.path) == folder)
    };
    say(&mut runtime, &first);
    runtime.toggle_root_menu(column).expect("the menu opens");
    assert!(
        offers(&runtime, &first),
        "the open menu offers where the shell stands"
    );

    say(&mut runtime, &second);
    assert!(
        !offers(&runtime, &second) && offers(&runtime, &first),
        "a frame of the open menu does not read the shell again"
    );

    runtime.toggle_root_menu(column).expect("the menu shuts");
    runtime
        .toggle_root_menu(column)
        .expect("the menu opens again");
    assert!(
        offers(&runtime, &second),
        "the next opening reads the shell again"
    );
}
