//! **The keyboard's doors, driven on a headless window.** The blur (T-DISPATCHER-WRITES-INTO-DOORS):
//! the window event loop's `Focused(false)` reaches the keyboard's two window facts through
//! [`crate::Runtime::blurred`]. The input method's composition events (T-IME-BRACKET-FACTS):
//! `Runtime::ime_input` on a window whose input-method facts door answers what the test scripted,
//! and the lines it says read back from the window's `diagnostics.log` door (T-DIAGNOSTICS-TEST-SINK).

use crate::headless::{HeadlessApplication, HeadlessFolio, WindowEffect};
use crate::ime_report::NativeFacts;
use crate::keyhint::KEY_HINT_DELAY;
use crate::test_support::{leaf_saying, ringing_tab, tab_holding};
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

/// What the window was told since this was last asked: how many times its input-method facts
/// were read, and the `diagnostics.log` lines its runtime said through the window's door, as
/// they were written.
fn told(folio: &HeadlessFolio) -> (usize, Vec<String>) {
    let mut facts_read = 0;
    let mut noted = Vec::new();
    for effect in folio.ledger.take() {
        match effect {
            WindowEffect::ImeFactsRead => facts_read += 1,
            WindowEffect::Noted(line) => noted.push(line),
            _ => {}
        }
    }
    (facts_read, noted)
}

/// T-IME-BRACKET-FACTS (F4 design note §4.1), T-DIAGNOSTICS-TEST-SINK: **a broken bracket's line
/// carries the facts the window's facts door answers at the break, and is the line `ime_input`
/// says** — read back from the window's `diagnostics.log` door, as it was written. The facts door
/// is asked at the break and nowhere else on the road. The headless window has no native handle,
/// so a reading that went round the door would answer default facts (every field `None`) and
/// read nothing from the door.
///
/// The composition is a Chinese and mixed-script pre-edit (`ni你`, five bytes), restarted while
/// live, then torn down and ended twice, the second end unpaired — through `ime_input`, as winit
/// delivers it.
///
/// MUTATIONS: `ime_native_facts` reads `native_window(..)` and `snapshot` instead of
/// `WindowHost::ime_facts` (or answers `NativeFacts::default()`) — no read, default facts — red;
/// `ime_input` formats a line of its own (the pairing line without its window, or any other
/// words) or says it to `diagnostics::note` rather than the window's door — the restart's line is
/// not the line written, red.
#[test]
fn a_restart_inside_a_live_composition_writes_the_windows_native_facts() {
    let mut folio = HeadlessFolio::with_tabs(
        "headless-ime-bracket-facts",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let scripted = NativeFacts {
        hwnd: Some(0x1234),
        focused_hwnd: Some(0x1234),
        focus_matches: Some(true),
        context: Some(false),
        imm_is_ime: Some(true),
        hkl_low: Some(2052),
        tsf_profile_type: Some(1),
        ..NativeFacts::default()
    };
    folio.window.window.ime_facts.set(scripted);
    let window = folio.window.window.id;
    folio.ledger.take();
    let broken = |shape: &str, live: usize| {
        format!(
            "Folio: the input method's composition bracket is broken — shape={shape} \
             live_preedit_bytes={live} focused=false native={scripted:?} window={window:?}"
        )
    };

    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    runtime
        .ime_input(Ime::Preedit("ni你".to_owned(), Some((5, 5))))
        .expect("a pre-edit is answered");
    assert_eq!(
        told(&folio),
        (0, Vec::new()),
        "an unbroken bracket reads no facts and says nothing"
    );

    folio
        .runtime()
        .ime_input(Ime::Enabled)
        .expect("a start inside a live composition is answered");
    assert_eq!(
        told(&folio),
        (1, vec![broken("restarted-inside-a-live-composition", 5)]),
        "the restart read the window's facts once and said the line that carries them"
    );

    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Preedit(String::new(), None))
        .expect("an emptied pre-edit is answered");
    runtime
        .ime_input(Ime::Disabled)
        .expect("an end is answered");
    assert_eq!(
        told(&folio),
        (0, Vec::new()),
        "a paired end reads no facts and says nothing"
    );
    folio
        .runtime()
        .ime_input(Ime::Disabled)
        .expect("an unpaired end is answered");
    assert_eq!(
        told(&folio),
        (1, vec![broken("ended-without-a-start", 0)]),
        "the unpaired end, through ime_input, read the window's facts through its door and said so"
    );
}

/// T-DIAGNOSTICS-TEST-SINK — **a focus change says the IME observation line through the
/// window's door**: the window gains the focus and loses it, through the event loop's own focus
/// events; each says one `IME observation` line, with its reason, the facts the window's facts
/// door answered and the window it was seen on.
///
/// MUTATION: `emit_ime_observation` says its line to `diagnostics::note` rather than the window's
/// door — no line is noted, red; it says the report's line without the window (a line of its
/// own) — red.
#[test]
fn a_focus_change_says_the_ime_observation_line_through_the_window() {
    let folio = HeadlessFolio::with_tabs("headless-ime-observation", vec![ringing_tab(1, 1)], 0);
    let scripted = NativeFacts {
        hwnd: Some(0x5678),
        context: Some(true),
        hkl_low: Some(1033),
        ..NativeFacts::default()
    };
    folio.window.window.ime_facts.set(scripted);
    let window = folio.window.window.id;
    let mut application = HeadlessApplication::of(folio);
    application.window_event(WindowEvent::Focused(true));
    application.window_event(WindowEvent::Focused(false));

    let noted: Vec<String> = application
        .runtime()
        .window
        .window
        .ledger
        .take()
        .into_iter()
        .filter_map(|effect| match effect {
            WindowEffect::Noted(line) => Some(line),
            _ => None,
        })
        .collect();
    let [gained, lost] = noted.as_slice() else {
        panic!("one line for each focus change: {noted:#?}");
    };
    for (line, reason, focused) in [(gained, "focus-gain", true), (lost, "focus-loss", false)] {
        assert!(
            line.starts_with(&format!("Folio: IME observation — reason={reason} at_ms=")),
            "{line}"
        );
        assert!(
            line.contains(&format!(" focused={focused} terminal=true web_host=false ")),
            "{line}"
        );
        assert!(
            line.ends_with(&format!(
                " native={scripted:?} native_none=unknown mode_source=imm-compat window={window:?}"
            )),
            "the line carries the facts the window's door answered, and the window: {line}"
        );
    }
}
