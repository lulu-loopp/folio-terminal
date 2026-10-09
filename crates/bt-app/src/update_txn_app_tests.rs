//! **`update_txn`, as the application drives it.** Tests whose first assertion is about
//! `update_txn`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{a_shell, frame_row_text, squeezed_body};

#[test]
fn forwarded_mouse_hit_stays_bound_to_the_presented_frame_during_an_unpresented_shift() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(6).unwrap());
    session
        .feed(b"\x1b[?1003h\x1b[?1006ha\r\nb\r\nc\r\nheader\r\nx\r\ny")
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let presented = session.viewport_frame(&mut projection).unwrap();
    assert!(frame_row_text(&presented, 3).contains("header"));

    session.feed(b"\r\nexpanded-1\r\nexpanded-2").unwrap();
    session.refresh_projection(&mut projection);
    let unpresented = session.viewport_frame(&mut projection).unwrap();
    assert!(frame_row_text(&unpresented, 1).contains("header"));
    assert!(!frame_row_text(&unpresented, 3).contains("header"));

    let stale_aim = bt_render::GridHit { row: 3, column: 0 };
    let forwarded = live_viewport_mouse_hit(&presented, stale_aim);
    let mut route = None;
    let bytes = route_forwarded_mouse_button(
        &mut route,
        ElementState::Pressed,
        input::MouseProtocolButton::Left,
        forwarded,
        session.terminal_modes(),
        ModifiersState::empty(),
        PressedCellTarget::Ordinary,
        a_shell(),
    )
    .unwrap();

    assert_eq!(bytes, b"\x1b[<0;1;4M");
    assert_eq!(
        forwarded.row, 3,
        "the stale row correctly misses live row 1"
    );
}

/// RED (57) — **With the restore card up, a press on a pane beneath changes nothing: not the
/// focus, not what the shell is sent, not the tab.**
///
/// Owner's ruling 2026-09-25: the card is a full-window gate, as the paste card is, and owns the
/// pointer too. On BASE its press arm returned only for a press that landed on the card
/// (`restore::hit` answered `Some`), and every other press went on to the chrome router — the tab
/// strip, the pane that takes the focus, the program's mouse report. Now the arm stands in the
/// router where the paste card's does and returns for every press, answering only on its two
/// buttons; the wheel under it is nobody's (one reading, `a_modal_covers_the_window`, which the
/// card is on); and nothing under it lights on a hover.
///
/// The pure half runs the card's own layout and hit test: a press on the window beside the card
/// hits nothing, so it answers nothing. The routing half is pinned on `mouse_input`,
/// `mouse_wheel` and `pointer_moved`, read through `bt_source` — no `Runtime` can be built without
/// a window.
///
/// MUTATION: put back BASE's arm (`&& let Some(target) = restore::hit(..)` in the `if let`, so the
/// arm is taken only on the card) — the arm is no longer whole and its pin goes red.
#[test]
fn with_the_restore_card_up_a_press_on_a_pane_beneath_changes_nothing() {
    let content = restore::RestoreContent {
        rows: Vec::new(),
        sub_lines: vec!["These come back as new shells.".to_owned()],
        decline_text_width: 62.0,
        restore_text_width: 47.0,
    };
    let layout = restore::layout(&content, 1200.0, 800.0, 1.0);
    // The top-left corner is the tab strip and the first pane, never the centred card.
    for (x, y) in [(10.0, 10.0), (40.0, 120.0), (1190.0, 790.0)] {
        assert_eq!(
            restore::hit(&layout, x, y),
            None,
            "({x}, {y}) is beside the card"
        );
        assert_eq!(
            restore::hit(&layout, x, y).and_then(restore::answer),
            None,
            "a press beside the card answers nothing"
        );
    }

    let press = squeezed_body("Runtime", "mouse_input");
    let arm = "iflet(Some(layout),Some(position))=(self.restore_layout(),self.window.pointer_position){ifstate==ElementState::Pressed&&button==MouseButton::Left&&letSome(answer)=restore::hit(&layout,position.x,position.y).and_then(restore::answer){self.answer_restore_prompt(answer)?;}returnOk(());}";
    let at = press
        .find(arm)
        .unwrap_or_else(|| panic!("the restore card's press arm does not swallow every press"));
    let chrome = press
        .find("self.chrome_mouse_input(")
        .expect("the chrome router is still reached from `mouse_input`");
    assert!(
        at < chrome,
        "a press reaches the tab strip, the panes and the programs before the card"
    );

    let covers = squeezed_body("Runtime", "a_modal_covers_the_window");
    assert!(
        covers.contains("||self.restore_card_is_up()"),
        "the card is not on the one reading of a modal:\n{covers}"
    );
    let wheel = squeezed_body("Runtime", "mouse_wheel");
    let nobody = wheel
        .find("ifself.a_modal_covers_the_window(){")
        .unwrap_or_else(|| panic!("a notch under a modal card is not swallowed"));
    let beneath = wheel
        .find("self.scroll_web_page(")
        .expect("the wheel still reaches a page");
    assert!(
        nobody < beneath,
        "a notch under the card scrolls what is beneath"
    );
    assert!(
        squeezed_body("Runtime", "pointer_moved")
            .contains("letfree=!self.a_modal_covers_the_window()&&"),
        "a hover under the card lights what is beneath"
    );
}
