//! **Attention's door for the focus, driven** (T-DISPATCHER-WRITES-INTO-DOORS): the window event
//! loop's `Focused(true)` ends the record of a taskbar flash through
//! [`crate::Runtime::window_focused`], on a headless window.

use crate::headless::{HeadlessApplication, HeadlessFolio};
use crate::test_support::ringing_tab;
use crate::{AttentionDelivery, SeatId, TabId, TaskbarFlash, attention};
use winit::event::WindowEvent;

/// RED (62) — **a focus ends the record of a taskbar flash, through attention's door**: a window
/// whose button is flashing for a pane comes to the front, and the record of what the flash was
/// about goes with the flash.
///
/// The flash is recorded by hand, with the constructor `Runtime::raise_attention` records it
/// with: a headless window has no native handle to flash (T-NATIVE-HANDLE-BEHIND-THE-SEAM).
///
/// MUTATION: drop the `runtime.window_focused()` call from `FolioApp::on_window_event`'s focus arm
/// — the assertion after the focus goes red; write the field from the focus arm instead of
/// calling the door — the ownership census (`bt-source`'s `census` test) goes red on a second
/// writer.
#[test]
fn a_focus_clears_the_taskbar_flash_through_attentions_door() {
    let folio = HeadlessFolio::with_tabs("focus-attention-door", vec![ringing_tab(1, 1)], 0);
    let mut application = HeadlessApplication::of(folio);
    application.window_event(WindowEvent::Focused(false));
    {
        let runtime = application.runtime();
        let mut flash = None;
        TaskbarFlash::record(
            &mut flash,
            runtime.window.taskbar_reading.generation,
            vec![AttentionDelivery {
                tab: TabId(1),
                seat: SeatId(0),
                reach: attention::Reach::Flash,
                why: attention::Why::Awaiting,
                title: "pwsh".to_owned(),
                body: None,
            }],
        );
        runtime.window.taskbar_flash = flash;
    }
    assert!(
        application.runtime().window.taskbar_flash.is_some(),
        "the button is flashing"
    );

    application.window_event(WindowEvent::Focused(true));

    assert!(
        application.runtime().window.taskbar_flash.is_none(),
        "the record ends where the flash does"
    );
}
