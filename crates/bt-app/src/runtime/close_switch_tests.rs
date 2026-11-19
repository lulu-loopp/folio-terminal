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
use crate::test_support::StoragePassRig;
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

/// RED (T16e; R5, review P1-1) — **a refused first put-away spends the receipt, raises no card,
/// and leaves the session's one refusal card to the message it would have lost**: the desk
/// latches the refusal and answers `Ok` from then on, so the next attention delivery that
/// reaches the desktop is not shown — and that delivery raises the "notifications refused" card
/// (DESIGN §7.1.5o ④, 2026-08-25), with the platform's words.
///
/// PLANT 87d-P38: drop the kept refusal (`App::toast_refusal_unsaid`) from
/// `Runtime::tell_the_first_put_away` — the agent's message is lost and no card says so.
#[test]
fn a_refused_first_put_away_leaves_its_card_to_the_next_message_it_would_have_lost() {
    const REFUSAL: &str = "the notification platform is not here";
    let mut run = PutAwayRun::open_with("told-refused", FirstHideToastV1::NotShown, true);
    run.app().notifications.refuse_with = Some(REFUSAL.to_owned());
    let refusal_cards = |run: &mut PutAwayRun| {
        run.window()
            .toasts
            .toasts()
            .iter()
            .filter(|card| card.body().contains(REFUSAL))
            .count()
    };
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    assert_eq!(
        run.first_hide_receipt(),
        FirstHideToastV1::Shown,
        "the receipt is spent on the platform's refusal too"
    );
    assert!(run.toasts_told().is_empty(), "nothing was shown");
    assert_eq!(refusal_cards(&mut run), 0, "and no card is raised for it");
    run.application
        .folio
        .bring_the_window_back()
        .expect("the window comes back");
    assert!(!run.window().put_away, "the window is back");
    let tab = run.window().tabs[0].id;
    let seat = run.window().tabs[0].seats.identity();
    let delivery = |title: &str| crate::AttentionDelivery {
        tab,
        seat,
        reach: crate::attention::Reach::Toast,
        why: crate::attention::Why::Awaiting,
        title: title.to_owned(),
        body: None,
    };
    run.application
        .runtime()
        .raise_attention(vec![delivery("等你 waiting")])
        .expect("the delivery is answered");
    assert!(
        run.toasts_told().is_empty(),
        "the latched desk shows nothing"
    );
    assert_eq!(
        refusal_cards(&mut run),
        1,
        "and the message it lost raises the one refusal card"
    );
    run.application
        .runtime()
        .raise_attention(vec![delivery("second")])
        .expect("the delivery is answered");
    assert_eq!(refusal_cards(&mut run), 1, "once a session");
}

/// RED (T16d; R12) — **a click on it brings the window back and opens nothing**: its launch is
/// [`BRING_BACK_LAUNCH`], which names no pane (`NotificationRoute::parse` refuses it), and the
/// door every click on one of Folio's notifications takes brings the put-away window back.
///
/// PLANT 87d-P34: hand the desk an empty launch in `Runtime::tell_the_first_put_away` — the
/// launch the click carries is not the one the route loop knows by name. This pins the launch
/// constant; the route loop's `bring-back` arm only names the click in the attention trace, which
/// no test can read, and is unpinned (review N1).
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

/// RED (T20c; R8, review P2-1) — **a hand edit of `settings.json` to Off brings a put-away window
/// back on the turn alone**: the edit arrives with the storage worker's answer
/// (`AppEvent::DocumentsAnswered`), not with a window event, and a put-away run's sole window is
/// sent none, so the turn's settle chain is the only door that spends the ask.
///
/// PLANT 87d-P39: drop `settle_bring_back_request` from `FolioApp::about_to_wait_inner`'s chain —
/// the window stays put away with the switch off.
#[test]
fn a_hand_edit_to_off_brings_a_put_away_window_back_on_the_turn_alone() {
    let mut run = PutAwayRun::open("hand-edit-off", true);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    let mut edited = run.app().settings_store.loaded().clone();
    edited.close_keeps_running = false;
    StoragePassRig::write(&run.dir, crate::persist::SETTINGS_FILE_NAME, &edited);
    run.app().settings_store.ask_reread();
    crate::lane::wait_until("the hand edit, merged", || {
        run.application
            .folio
            .take_document_answers()
            .expect("the answers are heard");
        !run.switch()
    });
    assert!(
        run.window().put_away,
        "the answer itself brings nothing back: the turn's door spends the ask"
    );
    let _ = run.application.event_loop.ledger.take();
    run.application.turn();
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Visible(true)),
        "the window is back on the glass: {effects:?}"
    );
    assert!(!run.window().put_away, "and no longer put away");
    assert!(run.sentinel_stands(), "the run goes on");
}
