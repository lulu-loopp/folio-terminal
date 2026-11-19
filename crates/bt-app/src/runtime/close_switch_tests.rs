//! **The switch `Keep running when closed`, its import door, and the first put-away's
//! notification** (0.4.10 plan row 87, part 87d; T-TRAY-87D-SETTINGS-AND-TEXTS, the coordinator's
//! rulings R1-R15 of 2026-11-19).
//!
//! On the put-away's own fixture ([`PutAwayRun`]): a headless application with one window, its
//! storage in a scratch folder of the test's own, its desk recording what it is handed
//! (`headless::HeadlessToasts`) — no toast reaches the machine running the suite.

use super::PutAwayRun;
use crate::headless::WindowEffect;
use crate::notify::{BRING_BACK_LAUNCH, NotificationRoute};
use crate::settings::{SettingsRow, SettingsTarget};
use crate::settings_bundle::SettingChange;
use bt_persist::FirstHideToastV1;
use winit::event::WindowEvent;

impl PutAwayRun {
    /// [`PutAwayRun::open`] with the switch on, and the receipt and the `Notifications` row as
    /// given.
    fn open_with(tag: &str, receipt: FirstHideToastV1, terminal_notifications: bool) -> Self {
        let mut run = Self::open(tag, true);
        let mut settings = run.app().settings_store.loaded().clone();
        settings.first_hide_toast = receipt;
        settings.terminal_notifications = terminal_notifications;
        let _ = run.app().settings_store.store(settings);
        run
    }

    /// Every notification the desk was handed, as `(title, body, launch)`.
    fn toasts_told(&mut self) -> Vec<(String, String, String)> {
        self.app().notifications.handed_over.clone()
    }

    fn first_hide_receipt(&mut self) -> FirstHideToastV1 {
        self.app().settings_store.loaded().first_hide_toast
    }

    fn switch(&mut self) -> bool {
        self.app().settings_store.loaded().close_keeps_running
    }

    /// A press on the row's picker, item `index` (`0` is `On`, `1` is `Off`).
    fn press_keep_running(&mut self, index: usize) {
        let id = self.application.id;
        self.application
            .folio
            .runtime(id)
            .expect("the window")
            .apply_settings_choice(SettingsTarget::Choice(
                SettingsRow::CloseKeepsRunning,
                index,
            ))
            .expect("the press is answered");
    }

    /// One event the loop delivers to a window, hidden or not, so the event's door runs.
    fn an_event(&mut self) -> Vec<WindowEffect> {
        self.application.window_event(WindowEvent::Focused(false));
        self.application.event_loop.ledger.take()
    }
}

/// The one notification the first put-away raises, as the desk is handed it.
fn still_running_toast() -> (String, String, String) {
    (
        crate::i18n::Text::ToastStillRunningTitle.text().to_owned(),
        crate::i18n::Text::ToastStillRunningBody.text().to_owned(),
        BRING_BACK_LAUNCH.to_owned(),
    )
}

/// RED (T16; R12, R5) — **the first put-away on this machine says Folio is still running, once
/// ever**: the receipt is spent as the notification is handed to the desk, so the next put-away,
/// after the window came back, says nothing.
///
/// PLANT 87d-P18: drop the receipt's spend from `Runtime::tell_the_first_put_away` — the second
/// put-away says it again.
#[test]
fn the_first_put_away_says_folio_is_still_running_once_ever() {
    let mut run = PutAwayRun::open_with("told-once", FirstHideToastV1::NotShown, true);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    assert_eq!(
        run.toasts_told(),
        vec![still_running_toast()],
        "said at the first put-away"
    );
    assert_eq!(
        run.first_hide_receipt(),
        FirstHideToastV1::Shown,
        "and the receipt is spent"
    );
    run.application
        .folio
        .route_notification_launches(vec![BRING_BACK_LAUNCH.to_owned()])
        .expect("the click is answered");
    assert!(!run.window().put_away, "the window is back");
    let _ = run.application.event_loop.ledger.take();
    let _ = run.close();
    assert!(run.window().put_away, "put away a second time");
    assert_eq!(
        run.toasts_told(),
        vec![still_running_toast()],
        "and nothing more is said"
    );
}

/// RED (T16b; the coordinator's item 12 of 2026-11-14) — **the `Notifications` row silences it
/// like every other toast, and leaves it owed**: nothing is handed to the desk and the receipt
/// still says it was never shown.
///
/// PLANT 87d-P32: drop the `interruption` term from `notify::first_hide_toast_owed` — the
/// notification is handed over with the row off.
#[test]
fn with_notifications_off_the_first_put_away_says_nothing_and_stays_owed() {
    let mut run = PutAwayRun::open_with("told-silenced", FirstHideToastV1::NotShown, false);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    assert!(run.toasts_told().is_empty(), "nothing reaches the desk");
    assert_eq!(
        run.first_hide_receipt(),
        FirstHideToastV1::NotShown,
        "and it is still owed"
    );
}

/// **A machine that was told says nothing**: the receipt is what is read, not this run's memory.
#[test]
fn a_machine_that_was_told_is_not_told_again() {
    let mut run = PutAwayRun::open_with("told-before", FirstHideToastV1::Shown, true);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    assert!(run.toasts_told().is_empty(), "nothing reaches the desk");
}

/// RED (T16d; R12) — **a click on it brings the window back and opens nothing**: its launch is
/// [`BRING_BACK_LAUNCH`], which names no pane (`NotificationRoute::parse` refuses it), and the
/// door every click on one of Folio's notifications takes brings the put-away window back.
///
/// PLANT 87d-P34: hand the desk an empty launch in `Runtime::tell_the_first_put_away` — the
/// launch the click carries is not the one the route loop knows by name.
#[test]
fn a_click_on_it_brings_the_window_back_and_opens_nothing() {
    let mut run = PutAwayRun::open_with("told-click", FirstHideToastV1::NotShown, true);
    let tabs = run.window().tabs.len();
    let _ = run.close();
    let told = run.toasts_told();
    let (_, _, launch) = told.first().expect("the first put-away was told");
    assert_eq!(
        launch, BRING_BACK_LAUNCH,
        "the click carries the bring-back launch"
    );
    assert_eq!(
        NotificationRoute::parse(launch),
        None,
        "which names no pane"
    );
    run.application
        .folio
        .route_notification_launches(vec![launch.clone()])
        .expect("the click is answered");
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Visible(true)),
        "the window is back on the glass: {effects:?}"
    );
    assert!(
        effects.contains(&WindowEffect::ForegroundTaken),
        "and given the keyboard: {effects:?}"
    );
    assert!(!run.window().put_away, "and no longer put away");
    assert_eq!(run.window().tabs.len(), tabs, "no tab is opened");
}

/// RED (T19; R1, R3) — **the row is the switch the close reads, from the next close**: `Off`
/// pressed on a run whose switch was on, and the last window's close ends the run; `On` pressed
/// on one whose switch was off, and it puts the window away. Nothing waits for a restart.
///
/// PLANT 87d-P35: drop the row's arm from `Runtime::apply_settings_choice_announcing` — the press
/// stores nothing and each close answers the old way.
#[test]
fn the_rows_press_is_the_switch_the_next_close_reads() {
    let mut run = PutAwayRun::open("row-off", true);
    run.press_keep_running(1);
    assert!(!run.switch(), "Off is stored");
    let _ = run.close();
    assert!(!run.window().put_away, "nothing is put away");
    assert!(
        run.window().leaving.is_some(),
        "the window is on its way out"
    );
    assert!(!run.sentinel_stands(), "and the run has ended");

    let mut run = PutAwayRun::open("row-on", false);
    run.press_keep_running(0);
    assert!(run.switch(), "On is stored");
    let _ = run.close();
    assert!(run.window().put_away, "the window is put away");
    assert!(run.sentinel_stands(), "and the run keeps running");
}

/// RED (T20; R8) — **turning the switch off while the window is put away brings it back**, by
/// the row, an import and a hand edit alike (the last two enter through the one door,
/// `Runtime::apply_imported_setting`): off is the old behaviour exactly, and a window left hidden
/// with its icon gone would be reachable only by a second launch. Spent at the event's door.
///
/// PLANT 87d-P36: drop the ask (`App::ask_for_the_put_away_windows`) from
/// `Runtime::apply_close_keeps_running` — the window stays put away with the switch off.
#[test]
fn turning_the_switch_off_brings_a_put_away_window_back() {
    type Road = fn(&mut PutAwayRun);
    let row: Road = |run| run.press_keep_running(1);
    let import: Road = |run| {
        let id = run.application.id;
        run.application
            .folio
            .runtime(id)
            .expect("the window")
            .apply_imported_setting(SettingChange::CloseKeepsRunning(false))
            .expect("the change is put in force");
    };
    for (index, (road, off)) in [("the row", row), ("an import or a hand edit", import)]
        .into_iter()
        .enumerate()
    {
        let mut run = PutAwayRun::open(&format!("off-{index}"), true);
        let _ = run.close();
        assert!(
            run.window().put_away,
            "{road}: the fixture's window is put away"
        );
        off(&mut run);
        assert!(!run.switch(), "{road}: Off is stored");
        let effects = run.an_event();
        assert!(
            effects.contains(&WindowEffect::Visible(true)),
            "{road}: the window is back on the glass: {effects:?}"
        );
        assert!(!run.window().put_away, "{road}: and no longer put away");
        assert!(run.sentinel_stands(), "{road}: the run goes on");
    }
}

/// **Turning it on moves no window**: a press on `On` with the window on the glass changes only
/// what the next close does.
#[test]
fn turning_the_switch_on_moves_no_window() {
    let mut run = PutAwayRun::open("on-moves-nothing", false);
    run.press_keep_running(0);
    let effects = run.an_event();
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, WindowEffect::Visible(_))),
        "no window is shown or hidden: {effects:?}"
    );
    assert!(!run.window().put_away);
}
