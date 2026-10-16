//! **The windows module's door for a re-requested close, driven**
//! (T-DISPATCHER-WRITES-INTO-DOORS): the close an exit's answer asks for is taken by the event
//! loop through [`crate::Runtime::take_close_request`], once, on a headless window.

use crate::headless::HeadlessFolio;
use crate::test_support::{buffer_saying, host_spelling, tab_with_a_preview};
use crate::{preview_edit, restore};

/// RED (T-SUMMON-DIRTY-PREVIEW) — **a close request is taken once, through the windows door**:
/// a window whose shut was asked over an unsaved edit is answered `Discard`, which re-requests
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
    let mut folio = HeadlessFolio::with_tabs("close-request-door", vec![tab], 0);
    let mut runtime = folio.runtime();
    let window = runtime.window_id();

    assert_eq!(
        runtime
            .raise_dirty_gate(restore::GateRequest::Shut)
            .expect("the gate is asked"),
        restore::GateRaise::Raised,
        "the shut asks first"
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
