//! **The keyboard's door for the blur, driven** (T-DISPATCHER-WRITES-INTO-DOORS): the window
//! event loop's `Focused(false)` reaches the keyboard's two window facts through
//! [`crate::Runtime::blurred`], on a headless window.

use crate::headless::{HeadlessApplication, HeadlessFolio};
use crate::keyhint::KEY_HINT_DELAY;
use crate::test_support::ringing_tab;
use std::time::{Duration, Instant};
use winit::event::{Ime, Modifiers, WindowEvent};
use winit::keyboard::ModifiersState;

/// This host's command modifier: Control, or Command on a Mac — `input::is_command_chord`'s
/// answer, not a second decision.
fn command() -> ModifiersState {
    [ModifiersState::CONTROL, ModifiersState::SUPER]
        .into_iter()
        .find(|modifiers| crate::input::is_command_chord(*modifiers))
        .expect("one of the two is the host's command modifier")
}

/// RED — **a blur clears the input method's context and takes the key-hint card down, through
/// the keyboard's door**: a window holding the command modifier long enough for its card, with
/// an input method enabled, loses the focus; afterwards neither the card nor the input method is
/// this window's, and both were cleared where the keyboard writes them.
///
/// MUTATION: drop the `runtime.blurred()` call from `FolioApp::on_window_event`'s blur arm — both
/// assertions after the blur go red; write either field from the blur arm instead of calling the
/// door — the ownership census (`bt-source`'s `census` test) goes red on a second writer.
#[test]
fn a_blur_clears_the_ime_and_the_key_hint_through_the_keyboards_door() {
    let folio = HeadlessFolio::with_tabs("blur-keyboard-door", vec![ringing_tab(1, 1)], 0);
    let mut application = HeadlessApplication::of(folio);
    application.window_event(WindowEvent::Focused(true));
    application.window_event(WindowEvent::Ime(Ime::Enabled));
    application.window_event(WindowEvent::ModifiersChanged(Modifiers::from(command())));
    application
        .runtime()
        .advance_key_hint_if_due(Instant::now() + KEY_HINT_DELAY + Duration::from_millis(1))
        .expect("the hold comes due");
    {
        let runtime = application.runtime();
        assert!(runtime.window.ime_active, "the input method is enabled");
        assert_eq!(
            runtime.window.key_hint.active(),
            Some(command()),
            "the card for the held modifier is up"
        );
    }

    application.window_event(WindowEvent::Focused(false));

    let runtime = application.runtime();
    assert!(
        !runtime.window.ime_active,
        "the input method's context left with the keyboard"
    );
    assert_eq!(
        runtime.window.key_hint.active(),
        None,
        "the card left with the keyboard"
    );
}
