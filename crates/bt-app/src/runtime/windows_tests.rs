//! **The windows module's door for a re-requested close, driven**
//! (T-DISPATCHER-WRITES-INTO-DOORS): the close an exit's answer asks for is taken by the event
//! loop through [`crate::Runtime::take_close_request`], once, on a headless window.

use crate::headless::{HeadlessApplication, HeadlessFolio, HeadlessParts, headless_window};
use crate::test_support::{buffer_saying, host_spelling, ringing_tab, tab_with_a_preview};
use crate::{preview_edit, restore};
use winit::event::WindowEvent;
use winit::window::WindowId;

/// RED (T-SUMMON-DIRTY-PREVIEW) — **a close request is taken once, through the windows door**:
/// a window the OS asked to close over an unsaved edit is answered `Discard`, which re-requests
/// its own close; the event loop's take hands that close over, and a second take finds nothing —
/// the next event closes nothing.
///
/// MUTATION: make `take_close_request` read the request without taking it
/// (`self.window.window_close_requested`) — the second take's assertion goes red; write the field
/// from `FolioApp::on_window_event` instead of calling the door — the ownership census
/// (`bt-source`'s `census` test) goes red on a second writer.
#[test]
fn a_close_request_is_taken_once_through_the_windows_door() {
    let path = host_spelling(r"C:\w\notes.md");
    let mut buffer = buffer_saying(&path, "notes.md", "one\n");
    let mut caret = preview_edit::EditCaret::default();
    let body = buffer.content().cloned().expect("the head landed");
    caret.place(&body, 4, false);
    assert!(
        buffer.edit_by_caret(&mut caret, |content, caret| {
            preview_edit::insert(content, caret, "typed by hand")
        }),
        "the keystroke landed"
    );
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let folio = HeadlessFolio::with_tabs("close-request-door", vec![tab], 0);
    let mut application = HeadlessApplication::of(folio);
    let window = application.id;

    application.window_event(WindowEvent::CloseRequested);
    let mut runtime = application.runtime();
    assert_eq!(
        runtime.window.dirty_gate.request(),
        Some(&restore::GateRequest::Shut),
        "the OS close asks first"
    );
    assert_eq!(
        runtime.take_close_request(),
        None,
        "nothing is requested while the question is up"
    );
    runtime
        .answer_dirty_gate(restore::GateAnswer::Discard)
        .expect("the answer is spent");

    assert_eq!(
        runtime.take_close_request(),
        Some(window),
        "Discard re-requests this window's close"
    );
    assert_eq!(
        runtime.take_close_request(),
        None,
        "and the request is spent by the taking"
    );
}

/// One application with two ordinary windows, `1` and `2`, so that closing either is a window
/// going and not the run ending; window `1`'s question is up, asking about `request`.
fn two_windows_asking(tag: &str, request: restore::GateRequest) -> HeadlessApplication {
    let folio = HeadlessFolio::with_tabs(tag, vec![ringing_tab(1, 1)], 0);
    let second = headless_window(
        &folio.app,
        HeadlessParts::new(&folio.app.gpu, 2, (1200, 800), 1.0, &folio.ledger),
        vec![ringing_tab(2, 1)],
        0,
    );
    let mut application = HeadlessApplication::of(folio);
    application.folio.windows.insert(WindowId::from(2), second);
    application.runtime().window.dirty_gate.open(request);
    application
}

/// Whether the application has told window `id` to go.
fn is_leaving(application: &mut HeadlessApplication, id: WindowId) -> bool {
    application
        .folio
        .windows
        .get_mut(id)
        .is_some_and(|window| window.leaving.is_some())
}

/// RED (T-SUMMON-DIRTY-PREVIEW) — **the event loop spends a re-requested close on the window it
/// names**: answered `Discard`, a window's own shut closes that window at the tail of the next
/// event it dispatches; the run's end answered in one window closes the *other* window it names,
/// and the answering window stays.
///
/// MUTATION: drop the `runtime.take_close_request()` call from the tail of
/// `FolioApp::on_window_event` (`let requested = None;`) — the first case's first assertion goes
/// red; match nothing in place of `requested.filter(|closing| *closing != window_id)` — the
/// second case's first assertion goes red.
#[test]
fn the_dispatcher_closes_the_window_a_close_request_names() {
    let one = WindowId::from(1);
    let two = WindowId::from(2);

    // Its own close.
    let mut application = two_windows_asking("close-tail-self", restore::GateRequest::Shut);
    application
        .runtime()
        .answer_dirty_gate(restore::GateAnswer::Discard)
        .expect("the answer is spent");
    application.window_event(WindowEvent::Focused(true));
    assert!(
        is_leaving(&mut application, one),
        "the window whose shut was answered goes"
    );
    assert!(
        !is_leaving(&mut application, two),
        "and the other window stays"
    );

    // Another window's close, re-run from the window where it was answered.
    let mut application = two_windows_asking(
        "close-tail-other",
        restore::GateRequest::ShutWithTheRun(two),
    );
    application
        .runtime()
        .answer_dirty_gate(restore::GateAnswer::Discard)
        .expect("the answer is spent");
    application.window_event(WindowEvent::Focused(true));
    assert!(
        is_leaving(&mut application, two),
        "the window the answer names goes"
    );
    assert!(
        !is_leaving(&mut application, one),
        "and the window that answered stays"
    );
}
