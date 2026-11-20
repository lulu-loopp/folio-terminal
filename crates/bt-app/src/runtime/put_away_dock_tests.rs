//! **On macOS the last window's close puts it away too, and the Dock brings it back** (0.4.10
//! plan row 87, part 87c; T-TRAY-87C-MACOS, the coordinator's rulings R1-R14 of 2026-11-20).
//!
//! The Mac's arms of `put_away_tests`, on that module's fixture run as a Mac
//! ([`PutAwayRun::on_the_platform`]): the close's rules and the first-hide toast asked of
//! `HostPlatform::MacOs`, and the loop with no notification area. They run on every host — the
//! platform is a value (R1) — and natively on a Mac.
//!
//! What only AppKit can answer — that `orderOut:` keeps the window, that a Dock click arrives as a
//! reopen, that the Window menu and the Dock tile list no window it is told to leave out — is the
//! hand check's (the report's §4); what this file holds is what the application does with each.

use super::{CARRY_ASKS, PutAwayRun};
use crate::carry_pass::CarryAsk;
use crate::headless::WindowEffect;
use bt_platform::HostPlatform::MacOs;
use bt_platform::close_road::OwnCloseRoad;
use bt_platform::{AppDelegateEvent, AppDelegateEventKind, AppDelegateOrigin};
use winit::event::WindowEvent;

impl PutAwayRun {
    /// [`PutAwayRun::start_the_run`] on a Mac.
    fn on_a_mac(tag: &str, keeps_running: bool) -> Self {
        Self::start_the_run(tag, keeps_running).on_the_platform(MacOs)
    }

    /// One event the application delegate parked, answered as the loop's turn answers it
    /// (`FolioApp::answer_one_delegate_event`), without the process-wide inbox.
    fn hand_the_delegate(&mut self, origin: AppDelegateOrigin, kind: AppDelegateEventKind) {
        self.application
            .folio
            .answer_one_delegate_event(
                &self.application.event_loop,
                AppDelegateEvent { origin, kind },
            )
            .expect("the delegate's event is answered");
    }

    /// A click on the Dock icon, or `open -a Folio`: AppKit's reopen.
    fn click_the_dock(&mut self) {
        self.hand_the_delegate(
            AppDelegateOrigin::Reopen,
            AppDelegateEventKind::Reopen {
                had_visible_windows: false,
            },
        );
    }

    /// A press on the Dock tile's menu row `row` (`menubar::DOCK`).
    fn press_on_the_dock(&mut self, row: &'static str) {
        self.hand_the_delegate(
            AppDelegateOrigin::Dock,
            AppDelegateEventKind::MenuChosen(bt_platform::menu::MenuChoice::Verb(row)),
        );
    }

    /// The close, then the ledger emptied and the carry asks forgotten: a put-away run, ready for
    /// a road back.
    fn put_away_on_a_mac(tag: &str) -> Self {
        let mut run = Self::on_a_mac(tag, true);
        let _ = run.close();
        assert!(run.window().put_away, "the fixture's window is put away");
        let _ = run.application.event_loop.ledger.take();
        CARRY_ASKS.with_borrow_mut(Vec::clear);
        run
    }
}

/// RED (T1, the Mac's arm; R1, R2 of 2026-11-20) — **with the switch on, a Mac's last window's
/// close puts it away**, by the red button and by the last pane's `Cmd+W` (the final tab's close)
/// alike: hidden and still the registry's, its shells untold, no Recent row, the run's sentinel
/// standing and no exit; and nothing that is Windows' — no icon put up, no first-hide toast.
///
/// PLANT 87c-P13: answer `false` for macOS from `HostPlatform::a_put_away_run_has_a_way_back` —
/// the Mac's last close files its window into Recent with the switch on.
/// PLANT 87c-P62: read the machine in `Runtime::tell_the_first_put_away`
/// (`bt_platform::host_platform()` for its `platform`) — on a Windows runner the Mac's put-away
/// tells the Windows toast.
#[test]
fn with_the_switch_on_a_macs_last_close_puts_it_away() {
    type Road = fn(&mut PutAwayRun) -> Vec<WindowEffect>;
    let roads: [(&str, Road); 2] = [
        ("the red button", PutAwayRun::close),
        ("the last pane's Cmd+W", |run| {
            run.close_by(OwnCloseRoad::FinalTab)
        }),
    ];
    for (index, (road, close)) in roads.into_iter().enumerate() {
        let mut run = PutAwayRun::on_a_mac(&format!("mac-on-{index}"), true);
        let mut effects = close(&mut run);
        run.turn_the_tray().expect("the icon's turn");
        effects.extend(run.application.event_loop.ledger.take());
        assert!(
            effects.contains(&WindowEffect::Visible(false)),
            "{road}: the window leaves the glass: {effects:?}"
        );
        assert!(
            !effects.contains(&WindowEffect::Exit),
            "{road}: the loop is not asked to exit: {effects:?}"
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, WindowEffect::TrayShown(_))),
            "{road}: a Mac puts no icon up: {effects:?}"
        );
        let window = run.window();
        assert!(window.put_away, "{road}: the window is put away");
        assert!(window.leaving.is_none(), "{road}: and not on its way out");
        assert_eq!(
            window.tabs[0].sessions.len(),
            2,
            "{road}: both its shells stay, untold"
        );
        assert!(
            run.app().recent.is_empty(),
            "{road}: no Recent row is filed"
        );
        assert!(
            run.app().notifications.handed_over.is_empty(),
            "{road}: a Mac is told nothing: the Dock icon stays where it is"
        );
        assert!(run.sentinel_stands(), "{road}: the run is still running");
    }
}

/// RED (T2, the Mac's arm; R2's off arm) — **with the switch off, a Mac's last close is the close
/// it always was**: the window is filed into Recent and let go, and the application stays in the
/// Dock — no exit, the run's sentinel standing, nothing put away.
#[test]
fn with_the_switch_off_a_macs_last_close_files_the_window_and_the_application_stays() {
    let mut run = PutAwayRun::on_a_mac("mac-off", false);
    let effects = run.close();
    assert!(
        !effects.contains(&WindowEffect::Exit),
        "the application stays: {effects:?}"
    );
    let window = run.window();
    assert!(!window.put_away, "nothing is put away");
    assert!(window.leaving.is_some(), "the window is on its way out");
    assert_eq!(run.app().recent.len(), 1, "its tab is filed in Recent");
    assert!(run.sentinel_stands(), "and the run goes on, in the Dock");
}

/// RED (T4, the Mac's arm; R4) — **with the switch on, a Mac's last window whose final shell
/// exited is filed and let go, and the application stays**: the close the shells' exit asked for
/// is no put-away, on a Mac as on Windows, and a Mac's run does not end at a close.
#[test]
fn a_macs_last_window_whose_shells_exited_is_filed_and_the_application_stays() {
    let mut run = PutAwayRun::on_a_mac("mac-exited", true);
    let effects = run.close_by(OwnCloseRoad::ShellsExited);
    assert!(
        !effects.contains(&WindowEffect::Exit),
        "the application stays: {effects:?}"
    );
    let window = run.window();
    assert!(!window.put_away, "nothing is put away");
    assert!(window.leaving.is_some(), "the window is on its way out");
    assert_eq!(run.app().recent.len(), 1, "its tab is filed in Recent");
    assert!(run.sentinel_stands(), "and the run goes on, in the Dock");
}

/// RED (T6, the Mac's arm; R10, R13, R4 of 2026-11-20) — **a click on the Dock icon brings the
/// put-away window back as it was**: on the glass and given the keyboard, with the same tabs as
/// the session writes them (the resume records among them), the same pane ids and the same rows,
/// one round of the carry pass for the agents' hooks, and no window opened.
///
/// PLANT 87c-P50: drop the door from `FolioApp::a_window_for_the_delegate` (the window the reader
/// was last in, answered as before) — the reopen raises the put-away window, which
/// `raise_for_a_launch` refuses, and nothing comes back.
#[test]
fn a_reopen_brings_the_put_away_window_back_as_it_was() {
    let mut run = PutAwayRun::on_a_mac("mac-reopen", true);
    let (tabs, seats, texts) = run.what_the_window_holds();
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    let _ = run.application.event_loop.ledger.take();
    CARRY_ASKS.with_borrow_mut(Vec::clear);
    run.click_the_dock();
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Visible(true)),
        "the window is back on the glass: {effects:?}"
    );
    assert!(
        effects.contains(&WindowEffect::ForegroundTaken),
        "and given the keyboard: {effects:?}"
    );
    assert!(
        !effects.contains(&WindowEffect::WindowOpened),
        "and no second window is opened: {effects:?}"
    );
    assert!(!run.window().put_away, "no longer put away");
    assert_eq!(run.application.folio.windows.len(), 1, "one window");
    assert_eq!(
        run.what_the_window_holds(),
        (tabs, seats, texts),
        "the same tabs as the session writes them, the same panes, the same rows"
    );
    assert_eq!(
        CARRY_ASKS.with_borrow_mut(std::mem::take),
        vec![CarryAsk::HOOKS],
        "one round for the agents' hooks"
    );
}

/// RED (R4 of 2026-11-20, the hand-over) — **a file Finder hands over while the run is put away
/// opens in the window brought back**: the door first, then the preview, and no second window.
///
/// PLANT 87c-P51: take the file's window in `FolioApp::open_one_path_for_the_delegate` as the
/// window the reader was last in (no door) — the preview opens in the hidden window, which stays
/// put away.
#[test]
fn a_file_handed_over_while_put_away_opens_in_the_window_brought_back() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-file");
    let file = run.dir.join("notes.md");
    std::fs::write(&file, "handed over\n").expect("the file is written");
    run.hand_the_delegate(
        AppDelegateOrigin::OpenUrls,
        AppDelegateEventKind::OpenPaths(vec![file]),
    );
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Visible(true)),
        "the window is back on the glass: {effects:?}"
    );
    assert!(!run.window().put_away, "no longer put away");
    assert!(
        !effects.contains(&WindowEffect::WindowOpened),
        "and the file did not open a second window: {effects:?}"
    );
}

/// RED (R3 of 2026-11-20) — **the Dock menu's New tab brings the put-away window back and lands its
/// tab there**: a tab needs a window, and the window the reader was last in is the put-away one.
///
/// PLANT 87c-P53: run the Dock row on the window the reader was last in without the door
/// (`answer_a_menu_row`'s Dock arm) — the tab lands in the hidden window, which stays put away.
#[test]
fn the_docks_new_tab_brings_the_put_away_window_back_and_lands_in_it() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-dock-tab");
    let tabs = run.window().tabs.len();
    run.press_on_the_dock("new-tab");
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Visible(true)),
        "the window is back on the glass: {effects:?}"
    );
    assert!(!run.window().put_away, "no longer put away");
    assert_eq!(run.window().tabs.len(), tabs + 1, "and the tab is in it");
}

/// RED (R3 of 2026-11-20; 87b's item 8) — **the Dock menu's New window opens a window of its own
/// and leaves the put-away one away**: one verb, one thing. The window is recorded to open, and the
/// put-away window is neither shown nor given the keyboard nor asked to be.
///
/// PLANT: drop the `a_dock_row_opens_a_window_of_its_own` term from `answer_a_menu_row`'s Dock arm
/// — New window brings the put-away window back as well.
#[test]
fn the_docks_new_window_leaves_the_put_away_window_away() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-dock-window");
    run.press_on_the_dock("new-window");
    let effects = run.application.event_loop.ledger.take();
    assert_eq!(
        run.app().pending_new_windows.len(),
        1,
        "a window is recorded to open"
    );
    assert!(run.window().put_away, "the put-away window stays away");
    assert!(
        !effects.contains(&WindowEffect::Visible(true))
            && !effects.contains(&WindowEffect::ForegroundTaken),
        "and is neither shown nor given the keyboard: {effects:?}"
    );
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            WindowEffect::Noted(line) if line.contains("a raise reached the put-away window")
        )),
        "nor raised and refused: {effects:?}"
    );
}

/// RED (R4 of 2026-11-20; the report's §1.0 point 1) — **a put-away window is given the keyboard
/// only by the door**: a launch's raise that reaches it takes no foreground and shows nothing — on
/// macOS taking the foreground orders a window in (`makeKeyAndOrderFront:`) — and says one line.
///
/// PLANT 87c-P52: drop the put-away guard from `FolioApp::raise_for_a_launch` — the raise takes the
/// foreground of the hidden window.
#[test]
fn a_put_away_window_is_never_raised_but_through_the_door() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-raise");
    let id = run.application.id;
    run.application.folio.raise_for_a_launch(id);
    let effects = run.application.event_loop.ledger.take();
    assert!(
        !effects.contains(&WindowEffect::ForegroundTaken)
            && !effects.contains(&WindowEffect::Visible(true)),
        "the hidden window is not given the keyboard: {effects:?}"
    );
    assert!(run.window().put_away, "and stays put away");
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            WindowEffect::Noted(line) if line.contains("a raise reached the put-away window")
        )),
        "the refused raise is said: {effects:?}"
    );
}

/// RED (R5 of 2026-11-20) — **a put-away window leaves the system's lists of the application's
/// windows, and comes back to them on the way back**: AppKit's Window menu and the Dock tile's
/// list would otherwise offer a row that orders it in behind the door.
///
/// PLANT 87c-P54: drop the `set_listed(true)` from `Runtime::bring_the_window_back_on_the_glass` —
/// the window comes back and stays off the lists.
#[test]
fn a_put_away_window_leaves_the_window_list_and_comes_back_to_it() {
    let mut run = PutAwayRun::on_a_mac("mac-list", true);
    let effects = run.close();
    assert!(
        effects.contains(&WindowEffect::WindowListed(false)),
        "off the lists while away: {effects:?}"
    );
    run.click_the_dock();
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::WindowListed(true)),
        "and back on them with the window: {effects:?}"
    );
}

/// RED (brief test 5, R14 of 2026-11-20; GitHub issue #5's road for a put-away window) — **news
/// that a put-away window is visible again composes nothing**: AppKit raises `Occluded(false)` when
/// a window is ordered in, and its arm publishes an expose frame; for a window that is put away the
/// publish gate answers it with no frame and nothing filed in the slot, so no picture is drawn
/// for a window nobody can see and no turn is asked to draw one.
///
/// PLANT: 87a's P15 (drop the `put_away` gate at the top of `Runtime::publish_frame_inner`) — the
/// news composes a frame for the put-away window.
#[test]
fn a_put_away_windows_visible_news_composes_nothing() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-occluded");
    let composed = run.window().composed_terminal_frames;
    let overwrites = run.window().pending_frames.overwrites();
    run.application.window_event(WindowEvent::Occluded(false));
    assert_eq!(
        run.window().composed_terminal_frames,
        composed,
        "no frame is composed for a window off the glass"
    );
    assert_eq!(
        run.window().pending_frames.overwrites(),
        overwrites,
        "and nothing is filed in the slot"
    );
    assert!(run.window().put_away, "the news brings nothing back");
}

/// RED (T20, the Mac's arm; R8 of T-TRAY-87D) — **turning the switch off on a Mac brings the
/// put-away window back on the loop's next turn**, once, with no notification area to take an
/// icon down from: Off is the old close exactly, and no window is left hidden.
///
/// PLANT 87c-P61: move the bring-back of `FolioApp::settle_tray`'s Off arm under the
/// notification-area term — on a Mac the window stays put away with the switch off.
#[test]
fn turning_the_switch_off_on_a_mac_brings_the_put_away_window_back() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-off-turn");
    run.switch(false);
    run.application
        .folio
        .about_to_wait_inner(&run.application.event_loop);
    let effects = run.application.event_loop.ledger.take();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| **effect == WindowEffect::Visible(true))
            .count(),
        1,
        "the window is back on the glass, once: {effects:?}"
    );
    assert!(!run.window().put_away, "and no longer put away");
    assert!(run.sentinel_stands(), "the run goes on");
}

/// The step of the quit under way.
fn the_quits_step(run: &mut PutAwayRun) -> crate::quit::QuitStep {
    run.app()
        .quit
        .as_ref()
        .map(crate::quit::Quit::step)
        .expect("a quit is under way")
}

/// RED (T10m; 87e's R7 on a Mac; the coordinator's item 9 of 2026-11-20) — **a person's quit of a
/// put-away Mac run asks on the window brought back**: `Cmd+Q` from the menu bar of another
/// window's focus, the Dock tile's Quit, AppKit's quit with no system reason — while a command
/// runs, the one door back is taken first, and the card stands on the window on the glass.
///
/// PLANT: 87e-P37 (drop the bring-back from `FolioApp::begin_quit_if_asked`) — the card stands on
/// a window nobody can see.
#[test]
fn a_persons_quit_of_a_put_away_mac_run_asks_on_the_window_brought_back() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-quit-asks");
    run.a_command_runs();
    run.application
        .folio
        .begin_the_systems_quit_for(bt_platform::QuitReason::NoneGiven);
    run.application
        .folio
        .begin_quit_if_asked()
        .expect("the quit begins");
    assert_eq!(
        the_quits_step(&mut run),
        crate::quit::QuitStep::Ask,
        "a person's quit asks while a command runs"
    );
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Visible(true)),
        "the window is shown: {effects:?}"
    );
    assert!(!run.window().put_away, "it is back on the glass");
    assert!(
        run.application.runtime().quit_card_layout().is_some(),
        "and the card stands on it"
    );
}

/// RED (T14m; 87e's R9 on a Mac) — **a log-out reaches a put-away Mac run and leaves without
/// asking**, a command running or not: nobody is there to answer, so no card is put up and the
/// window is not shown for one; the quit goes on to its documents, and its way out writes the
/// layout, the put-away window in it.
///
/// PLANT: 87e-P42 (drop the begin-and-answer block from `FolioApp::begin_the_systems_quit_for`) —
/// the log-out stands asking.
#[test]
fn a_macs_log_out_writes_a_put_away_runs_layout_and_leaves() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-log-out");
    run.a_command_runs();
    run.application
        .folio
        .begin_the_systems_quit_for(bt_platform::QuitReason::LogOut);
    assert_eq!(
        the_quits_step(&mut run),
        crate::quit::QuitStep::Documents,
        "the log-out asks nothing"
    );
    assert!(
        run.application.runtime().quit_card_layout().is_none(),
        "no card is put up"
    );
    assert!(
        !run.application
            .event_loop
            .ledger
            .take()
            .contains(&WindowEffect::Visible(true)),
        "and the window is not shown for one"
    );
    assert!(run.window().put_away, "it is still put away");
}

/// The loop's activity fold, as its turn runs it after every window's turn
/// (`FolioApp::settle_the_put_away_activity`), its effects taken. The fold alone and not the whole
/// turn: the fixture's shell stands for a process no fixture starts, and a turn's reaper would read
/// its exit (`test_support::land_a_live_shell`); where the turn runs the fold is pinned by
/// [`the_turn_settles_the_activity_after_every_windows_turn`].
fn a_turn(run: &mut PutAwayRun) -> Vec<WindowEffect> {
    run.application
        .folio
        .settle_the_put_away_activity(&run.application.event_loop);
    run.application.event_loop.ledger.take()
}

/// How many times `effect` is among `effects`.
fn times_seen(effects: &[WindowEffect], effect: &WindowEffect) -> usize {
    effects.iter().filter(|seen| *seen == effect).count()
}

/// RED (TA1; R14 of 2026-11-12, R6, R7 of 2026-11-20) — **a put-away run that works holds the
/// activity, once, and lets it go when the window comes back**: begun on the first turn after the
/// close with a command running, not begun again on the next turn, ended on the turn after the
/// Dock brings the window back.
///
#[test]
fn a_put_away_run_that_works_holds_the_activity_until_the_window_is_back() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-activity");
    run.a_command_runs();
    let first = a_turn(&mut run);
    assert_eq!(
        times_seen(&first, &WindowEffect::ActivityHeld),
        1,
        "the first turn begins it: {first:?}"
    );
    let second = a_turn(&mut run);
    assert_eq!(
        times_seen(&second, &WindowEffect::ActivityHeld),
        0,
        "the next turn holds the one begun: {second:?}"
    );
    assert_eq!(
        times_seen(&second, &WindowEffect::ActivityLetGo),
        0,
        "{second:?}"
    );
    run.click_the_dock();
    assert!(!run.window().put_away, "the Dock brought the window back");
    let back = a_turn(&mut run);
    assert_eq!(
        times_seen(&back, &WindowEffect::ActivityLetGo),
        1,
        "the turn after the window is back lets it go: {back:?}"
    );
    assert_eq!(
        times_seen(&back, &WindowEffect::ActivityHeld),
        0,
        "{back:?}"
    );
}

/// RED (TA2; R7, item 19 of 2026-11-14) — **a put-away run with nothing running holds none, and a
/// shown run that works holds none**: the activity is the put-away run's, and only while work runs
/// (`crate::running_work`, the quit's reader).
///
/// PLANT 87c-P58: answer `a_window_is_put_away` alone from
/// `put_away_activity::the_activity_is_wanted` — the idle put-away run holds one.
/// The shown half stands on the fold's own reading as well: it reads no work while nothing is put
/// away, so 87c-P55 (the term dropped from `the_activity_is_wanted` alone) is red in that
/// function's own test and leaves this one green.
#[test]
fn an_idle_put_away_run_and_a_shown_run_hold_no_activity() {
    let mut idle = PutAwayRun::put_away_on_a_mac("mac-activity-idle");
    let turn = a_turn(&mut idle);
    assert_eq!(
        times_seen(&turn, &WindowEffect::ActivityHeld),
        0,
        "nothing runs, nothing is held: {turn:?}"
    );

    let mut shown = PutAwayRun::on_a_mac("mac-activity-shown", true);
    shown.a_command_runs();
    let _ = shown.application.event_loop.ledger.take();
    let turn = a_turn(&mut shown);
    assert_eq!(
        times_seen(&turn, &WindowEffect::ActivityHeld),
        0,
        "a window on the glass is App Nap's to judge: {turn:?}"
    );
}

/// RED (TA3) — **the work's end lets the activity go while the window stays away**, and the run's
/// end lets it go for good: after `App::finish` no turn begins it again.
///
/// PLANT 87c-P57: drop `put_away_activity.let_go_for_good()` from `App::finish` — the activity is
/// held past the run's end.
#[test]
fn the_works_end_and_the_runs_end_let_the_activity_go() {
    let mut run = PutAwayRun::put_away_on_a_mac("mac-activity-ends");
    run.a_command_runs();
    let _ = a_turn(&mut run);
    run.the_command_ends();
    let ended = a_turn(&mut run);
    assert_eq!(
        times_seen(&ended, &WindowEffect::ActivityLetGo),
        1,
        "the command's end lets it go: {ended:?}"
    );
    assert!(run.window().put_away, "and the window stays away");

    run.a_command_runs();
    let again = a_turn(&mut run);
    assert_eq!(
        times_seen(&again, &WindowEffect::ActivityHeld),
        1,
        "{again:?}"
    );
    run.app().finish();
    let finished = run.application.event_loop.ledger.take();
    assert_eq!(
        times_seen(&finished, &WindowEffect::ActivityLetGo),
        1,
        "the run's end lets it go: {finished:?}"
    );
    let after = a_turn(&mut run);
    assert_eq!(
        times_seen(&after, &WindowEffect::ActivityHeld),
        0,
        "and no turn after the end begins it: {after:?}"
    );
}

/// RED (TA4) — **the turn runs the activity fold after every window's turn, beside the run's
/// icon**: what the windows' attention passes settled on this turn (an agent's turn heard) is read
/// on this turn, and the fold is never skipped by a turn that reaches the icon.
///
/// PLANT 87c-P56: drop the `settle_the_put_away_activity` call from `FolioApp::about_to_wait_inner`
/// — no turn begins the activity.
#[test]
fn the_turn_settles_the_activity_after_every_windows_turn() {
    let turn = crate::test_support::squeezed_body("FolioApp", "about_to_wait_inner");
    let windows = turn
        .find("forindexin0..self.windows.len()")
        .expect("the windows take their turns");
    let icon = turn
        .find("self.tell_the_tray();")
        .expect("the run's icon is told");
    let activity = turn
        .find("self.settle_the_put_away_activity(event_loop);")
        .expect("the activity is settled");
    assert!(windows < icon && icon < activity, "{turn}");
}
