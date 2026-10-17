//! **A second button during a held gesture, driven** (T-POINTER-CAPTURE cut 4, R-8): button
//! events dispatched as the event loop dispatches them — through `FolioApp::on_window_event`,
//! the application's capture door and each window's `mouse_input` — on headless windows that
//! each draw a Markdown page, whose left press latches a rendered selection (cell C15) and whose
//! right press, with nothing held, raises the page's menu.

use super::*;
use crate::headless::{
    HeadlessApplication, HeadlessFolio, HeadlessParts, Ledger, WindowEffect, headless_window,
};
use crate::runtime::pointer::CaptureOwner;
use crate::test_support::{buffer_saying, host_spelling, seat_of, tab_with_a_preview};
use crate::{PreviewSurface, TabId, TabState};
use bt_render::{FrameSource, FrameTrigger};
use winit::event::DeviceId;

const PAGE: &str = "# Title\n\nfirst paragraph here.\n\nsecond paragraph here.\n";

/// A tab of one shell beside a Markdown page, as tab `id`, and the page's surface.
fn a_page_tab(id: u64) -> (TabState, PreviewSurface) {
    let path = host_spelling(r"C:\w\notes.md");
    let (tab, seat) = tab_with_a_preview(id, vec![buffer_saying(&path, "notes.md", PAGE)]);
    (tab, seat_of(TabId(id), seat))
}

/// One application with two windows, `1` and `2`, each holding a page tab (tab 1 and tab 2)
/// and drawn once; window 1's page's surface and window 1's own ledger beside it (window 2
/// records into a ledger of its own, so what window 1 did can be told apart).
fn two_windows_with_pages(tag: &str) -> (HeadlessApplication, PreviewSurface, Ledger) {
    let (first, surface) = a_page_tab(1);
    let folio = HeadlessFolio::with_tabs(tag, vec![first], 0);
    let first_ledger = folio.ledger.clone();
    let second = headless_window(
        &folio.app,
        HeadlessParts::new(&folio.app.gpu, 2, (1200, 800), 1.0, &Ledger::default()),
        vec![a_page_tab(2).0],
        0,
    );
    let mut application = HeadlessApplication::of(folio);
    application.folio.windows.insert(WindowId::from(2), second);
    for id in [1, 2] {
        let mut runtime = application
            .folio
            .runtime(WindowId::from(id))
            .expect("both windows are open");
        runtime
            .settle_seat_set_change()
            .expect("the window's tabs take their layout");
        runtime
            .publish_frame(FrameTrigger {
                occurred_at: Instant::now(),
                source: FrameSource::Expose,
            })
            .expect("a headless frame composes");
        runtime.redraw().expect("and reaches the glass");
    }
    (application, surface, first_ledger)
}

/// A point on window `id`'s glass over the drawn paragraph that holds `text`.
fn on_the_paragraph(
    application: &mut HeadlessApplication,
    id: u64,
    text: &str,
) -> PhysicalPosition<f64> {
    let runtime = application
        .folio
        .runtime(WindowId::from(id))
        .expect("the window is open");
    let paragraph = runtime
        .window
        .renderer
        .preview_bodies
        .iter()
        .flat_map(|body| body.paragraphs.iter())
        .find(|paragraph| {
            paragraph
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                .contains(text)
        })
        .unwrap_or_else(|| panic!("window {id} draws a paragraph holding `{text}`"));
    PhysicalPosition::new(
        f64::from(paragraph.rect[0]) + 1.0,
        f64::from(paragraph.rect[1]) + f64::from(paragraph.line_height_px) / 2.0,
    )
}

/// The pointer moved to `at` in window `id`, then `button` went `state` there, each dispatched
/// as the event loop dispatches a window event.
fn button_at(
    application: &mut HeadlessApplication,
    id: u64,
    at: PhysicalPosition<f64>,
    state: ElementState,
    button: MouseButton,
) {
    let window = WindowId::from(id);
    for event in [
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: at,
        },
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button,
        },
    ] {
        application
            .folio
            .on_window_event(&application.event_loop, window, event);
    }
}

/// Whether window `id` shows the page's menu (the terminal menu with a page's rows).
fn menu_is_up(application: &mut HeadlessApplication, id: u64) -> bool {
    application
        .folio
        .runtime(WindowId::from(id))
        .expect("the window is open")
        .window
        .term_menu
        .is_some()
}

/// Which window holds the application's capture, and whether it is a rendered selection.
fn the_slot(application: &HeadlessApplication) -> Option<(WindowId, bool)> {
    application
        .folio
        .app
        .as_ref()
        .and_then(|app| app.pointer_capture.as_ref())
        .map(|capture| {
            (
                capture.window,
                matches!(capture.owner, CaptureOwner::RenderedSelection(..)),
            )
        })
}

fn swallowed(application: &HeadlessApplication, button: MouseButton) -> bool {
    application
        .folio
        .app
        .as_ref()
        .is_some_and(|app| app.swallowed.contains(button))
}

/// The caret's byte range on window 1's page.
fn caret(application: &mut HeadlessApplication, surface: PreviewSurface) -> std::ops::Range<usize> {
    application
        .folio
        .runtime(WindowId::from(1))
        .expect("the window is open")
        .preview_pane(surface)
        .expect("the page is shown")
        .caret()
        .range()
}

/// RED (T-POINTER-CAPTURE cut 4, R-8, cells C15·b and A·b) — **a second button during a held
/// gesture opens nothing**: with a selection held on the page, a right press on it raises no menu
/// and leaves the gesture whole; its release does nothing either; the left release still ends
/// the gesture as it always did (the press is spent: the caret goes where it was pressed). With
/// nothing held, the same right press raises the page's menu, as it did before cut 4.
///
/// MUTATION: answer `Route` for another button in `press_against_the_slot` — the menu opens
/// under the held press and the first assertion fails.
#[test]
fn a_second_button_during_a_held_gesture_opens_nothing() {
    let (mut application, surface, _) = two_windows_with_pages("pointer-second-button");
    let first = on_the_paragraph(&mut application, 1, "first paragraph");
    let second = on_the_paragraph(&mut application, 1, "second paragraph");
    let before = caret(&mut application, surface);

    button_at(
        &mut application,
        1,
        second,
        ElementState::Pressed,
        MouseButton::Left,
    );
    assert_eq!(
        the_slot(&application),
        Some((WindowId::from(1), true)),
        "the fixture's own precondition: the left press holds a selection"
    );
    button_at(
        &mut application,
        1,
        second,
        ElementState::Pressed,
        MouseButton::Right,
    );
    assert!(
        !menu_is_up(&mut application, 1),
        "a right press under the held selection raises no menu"
    );
    assert_eq!(
        the_slot(&application),
        Some((WindowId::from(1), true)),
        "and the selection is still held"
    );
    button_at(
        &mut application,
        1,
        second,
        ElementState::Released,
        MouseButton::Right,
    );
    assert!(
        !menu_is_up(&mut application, 1),
        "its release does nothing either"
    );
    assert_eq!(
        the_slot(&application),
        Some((WindowId::from(1), true)),
        "nor does it end the left gesture"
    );
    assert_eq!(
        caret(&mut application, surface),
        before,
        "and nothing has moved the caret yet"
    );
    button_at(
        &mut application,
        1,
        second,
        ElementState::Released,
        MouseButton::Left,
    );
    assert_eq!(
        the_slot(&application),
        None,
        "the left release ends the gesture"
    );
    assert_ne!(
        caret(&mut application, surface),
        before,
        "and spends its press, as it always did"
    );

    button_at(
        &mut application,
        1,
        first,
        ElementState::Pressed,
        MouseButton::Right,
    );
    assert!(
        menu_is_up(&mut application, 1),
        "with nothing held, a right press raises the page's menu as before"
    );
}

/// RED (T-POINTER-CAPTURE cut 4, R-5, R-8, cells B21, B22 and B20 across windows) — **a second
/// button in another window is swallowed too**, press and release, even when its release comes
/// after the gesture has ended; a press of the held button in the other window still cancels the
/// stale capture first.
///
/// Window 1 holds a selection; a right press in window 2 raises no menu there and leaves window
/// 1's gesture whole; the left release in window 1 ends it; the right release in window 2 is
/// still swallowed; then a right press in window 2 is routed (its menu rises). Last: a left press
/// in window 2 while window 1 holds a selection cancels window 1's and latches window 2's own.
///
/// MUTATION: route a press arriving in another window than the capture's (compare the press's
/// window with the slot's in `Runtime::button_meets_the_slot`) — window 2's menu rises under
/// window 1's held press; clear `swallowed` when a capture ends — the assertion naming B22 fails;
/// skip the owner's cancel in `FolioApp::capture_before_pointer_event` (`|_this| Ok(())` for
/// `cancel_owner`) — window 1 records no repaint and the cancel's assertion fails.
#[test]
fn a_second_button_in_another_window_is_swallowed_too() {
    let (mut application, _, first_ledger) = two_windows_with_pages("pointer-second-window");
    let one = on_the_paragraph(&mut application, 1, "second paragraph");
    let two = on_the_paragraph(&mut application, 2, "first paragraph");

    button_at(
        &mut application,
        1,
        one,
        ElementState::Pressed,
        MouseButton::Left,
    );
    assert_eq!(the_slot(&application), Some((WindowId::from(1), true)));
    button_at(
        &mut application,
        2,
        two,
        ElementState::Pressed,
        MouseButton::Right,
    );
    assert!(
        !menu_is_up(&mut application, 2),
        "B21: a right press in window 2 under window 1's held press raises nothing"
    );
    assert_eq!(
        the_slot(&application),
        Some((WindowId::from(1), true)),
        "and window 1's gesture is whole"
    );
    button_at(
        &mut application,
        1,
        one,
        ElementState::Released,
        MouseButton::Left,
    );
    assert_eq!(
        the_slot(&application),
        None,
        "the left release in window 1 ends it"
    );
    assert!(
        swallowed(&application, MouseButton::Right),
        "B22: the right button is still remembered with the slot empty"
    );
    button_at(
        &mut application,
        2,
        two,
        ElementState::Released,
        MouseButton::Right,
    );
    assert!(
        !swallowed(&application, MouseButton::Right) && !menu_is_up(&mut application, 2),
        "its release in window 2 is swallowed and spends the memory"
    );
    button_at(
        &mut application,
        2,
        two,
        ElementState::Pressed,
        MouseButton::Right,
    );
    assert!(
        menu_is_up(&mut application, 2),
        "a later right press in window 2 is routed"
    );
    button_at(
        &mut application,
        2,
        two,
        ElementState::Released,
        MouseButton::Right,
    );

    // B20 across windows: the held button pressed in the other window.
    button_at(
        &mut application,
        1,
        one,
        ElementState::Pressed,
        MouseButton::Left,
    );
    assert_eq!(the_slot(&application), Some((WindowId::from(1), true)));
    first_ledger.take();
    button_at(
        &mut application,
        2,
        two,
        ElementState::Pressed,
        MouseButton::Left,
    );
    assert!(
        first_ledger.take().contains(&WindowEffect::Redraw),
        "window 1's owner ran its own cancel: the selection let go repaints its page"
    );
    assert_eq!(
        the_slot(&application),
        Some((WindowId::from(2), true)),
        "and window 2 latches its own selection in its place"
    );
}
