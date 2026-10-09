//! **The crate root: settings panel.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::method_body;
use bt_render::{DARK_CHROME, LIGHT_CHROME};

/// PIN (T2 D32/D33): each claim wears the mock-up's own colour, and a
/// silent session draws no dot at all.
///
/// Presence-versus-absence is the point: the mock-up keeps `.unreaddot` in
/// the DOM always and shows it by class (its comment at line 249 records
/// why), but what lands on screen is still nothing when there is nothing to
/// say. A dot drawn in the tab's own colour would be a smudge, not a state.
#[test]
fn each_claim_wears_its_own_colour_and_silence_draws_nothing() {
    for palette in [LIGHT_CHROME, DARK_CHROME] {
        assert_eq!(StatusClaim::Silent.dot(&palette), None);
        let ink = |claim: StatusClaim| claim.dot(&palette).map(|dot| dot.ink);
        assert_eq!(ink(StatusClaim::Unread), Some(palette.accent));
        assert_eq!(ink(StatusClaim::Bell), Some(palette.status_warn));
        assert_eq!(ink(StatusClaim::Failed), Some(palette.status_err));
        // Every speaking claim differs from every other in at least one of the two axes —
        // a taxonomy that collapses is not a taxonomy. Three of the four differ in ink;
        // the fourth pair shares `--warn` and is told apart by its fill (red line 3).
        let drawn = [
            StatusClaim::Unread,
            StatusClaim::Bell,
            StatusClaim::Failed,
            StatusClaim::Awaiting,
        ]
        .map(|claim| claim.dot(&palette).expect("a speaking claim has a dot"));
        for (index, dot) in drawn.iter().enumerate() {
            for other in &drawn[index + 1..] {
                assert_ne!(dot, other, "two claims cannot arrive as the same badge");
            }
        }
    }
}

/// RED GATE (user reports and rulings, 2026-08-29 and 2026-08-30) — **a
/// press outside an open dropdown closes it and still lands.**
///
/// The first report: open `General ▸ Language`, click the dialog's own blank
/// background, and the list stays standing — while every other popup in this
/// window goes away on a press outside it. The ruling is one rule for all of
/// them, and the dialog is where it had to be asked separately: it is a modal
/// with a dispatch of its own, so a press inside it never reaches
/// `mouse_input`'s popup arms.
///
/// The second report is what that first fix cost: with a list open, the `×`
/// needed two presses. So the gate no longer ends every press — only the two
/// that land on the dialog's ground (`settings::target_is_ground`: the
/// panel's blank and the scrim). A press on a control takes the list down
/// and then goes on to that control.
///
/// **What is asserted is the position of the gate**, because the position is
/// what makes "the list is already gone when the verb runs" true: the
/// question stands between the hit test and every verb, and its ground arm
/// returns. Judged by `settings::popup_press`, whose own answers are pinned
/// beside it in `settings.rs`
/// (`a_press_outside_an_open_dropdown_closes_it_and_still_lands` there, and
/// `a_press_inside_the_dropdown_still_picks` for the reverse).
///
/// Read off the source for `a_right_press_on_a_tab_raises_its_menu_and_leaves_the_active_tab_alone`'s
/// reason: raising this dialog needs a `Runtime`, and a `Runtime` needs a
/// GPU device, a swapchain and a live window.
///
/// MUTATIONS that must turn it red:
/// ① delete the gate — the first assertion, which is the 08-29 report;
/// ② drop the `DismissAndLand` arm, or make it return like its neighbour —
///    the third and fourth, which are the 08-30 report;
/// ③ let the ground arm fall through instead of returning, so a press on the
///    scrim shuts the dialog under an open list — the fifth;
/// ④ move the gate below `SettingsPanel::press_verb`, where the focus has
///    already moved and the scrim has already answered — the sixth and seventh;
/// ⑤ judge it against a rectangle of its own instead of the hit test's
///    answer — the second, which pins that `hit` is asked first.
#[test]
fn a_press_outside_an_open_dropdown_closes_it_and_still_lands() {
    let router = method_body("Runtime", "settings_mouse_input");

    // ① the gate exists, and it asks the one door both popups leave by.
    let gate = router
        .find("settings::popup_press(self.window.settings.popup_up(), target)")
        .expect(
            "a press outside the dialog's open popup is judged by \
                 `settings::popup_press`, not by a rule written here",
        );
    // ② off the hit test's own answer — the geometry is `settings::hit`'s
    // and this router measures nothing.
    let hit = router
        .find("let target = settings::hit(layout,")
        .expect("the router hit-tests the press once");
    assert!(
        hit < gate,
        "the gate reads the hit test's answer, which is where the popup's \
             own rectangle was already asked"
    );
    // ③ the popup goes away on both of the outside answers.
    // The focus move and every verb of the dialog's own — the scrim's close among them — are
    // `SettingsPanel::press_verb`'s (F-SWEEP-048 round 2), so its call is where both start.
    let press = router
        .find("self.window.settings.press_verb(target, content)")
        .expect("the router moves the focus to what was pressed, and runs its verb");
    let arm = &router[gate..press];
    let land = arm
        .find("settings::PopupPress::DismissAndLand(popup)")
        .expect("a press outside the list that landed on a control has an arm");
    let ground = arm
        .find("settings::PopupPress::DismissOnly(popup)")
        .expect("a press outside the list that landed on ground has an arm");
    assert_eq!(
        arm.matches("self.window.settings.close_popup(popup)")
            .count(),
        2,
        "both outside answers put the popup away: {arm}"
    );
    assert!(
        land < ground,
        "the arms read in the order the ruling does: land, then the ground \
             that ends the gesture"
    );
    // ④ and the one that landed on a control does NOT end the press — the
    // × that used to need pressing twice (user report 2026-08-30).
    assert!(
        !arm[land..ground].contains("return Ok(());"),
        "a press that landed on a control goes on to that control: {arm}"
    );
    // ⑤ while the one that landed on ground does, which is what leaves the
    // dialog standing under a press on its own scrim.
    assert!(
        arm[ground..].contains("return Ok(());"),
        "a press on the dialog's ground is the whole gesture: {arm}"
    );
    // ⑥ before the focus moves.
    assert!(
        gate < press,
        "the gate stands above `SettingsPanel::press_verb`"
    );
    // ⑦ and before every verb, the scrim's close included — which `press_verb` makes first.
    let verbs = press;
    assert!(
        gate < verbs,
        "a press on the scrim while a list is open shuts the list, not the \
             dialog — one layer per gesture, which is what the first Esc does"
    );
}
