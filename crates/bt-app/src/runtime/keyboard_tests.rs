//! **The input method's composition events, driven** (T-IME-BRACKET-FACTS): `Runtime::ime_input`
//! on a headless window whose input-method facts door answers what the test scripted.

use crate::headless::{HeadlessFolio, WindowEffect};
use crate::ime_report::{ImeKind, NativeFacts};
use crate::test_support::{leaf_saying, tab_holding};
use winit::event::Ime;

/// How many times the window's input-method facts were read since this was last asked.
fn facts_read(folio: &HeadlessFolio) -> usize {
    folio
        .ledger
        .take()
        .into_iter()
        .filter(|effect| *effect == WindowEffect::ImeFactsRead)
        .count()
}

/// T-IME-BRACKET-FACTS (F4 design note §4.1): **a broken bracket's line carries the facts the
/// window's facts door answers at the break**, and the door is asked at the break and nowhere
/// else on the road. The headless window has no native handle, so a reading that went round the
/// door would answer default facts (every field `None`) and read nothing from the door.
///
/// The composition is a Chinese and mixed-script pre-edit (`ni你`, five bytes), restarted while
/// live (the line read back from the runtime's own bracket door), then torn down and ended twice,
/// the second end unpaired — through `ime_input`, as winit delivers it.
///
/// MUTATION: `ime_native_facts` reads `native_window(..)` and `snapshot` instead of
/// `WindowHost::ime_facts` (or answers `NativeFacts::default()`) — no read, default facts — red.
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

    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    runtime
        .ime_input(Ime::Preedit("ni你".to_owned(), Some((5, 5))))
        .expect("a pre-edit is answered");
    assert_eq!(facts_read(&folio), 0, "an unbroken bracket reads no facts");

    let restart = folio
        .runtime()
        .ime_bracket_line(ImeKind::Enabled, 0)
        .expect("a start inside a live composition is named");
    assert!(
        restart.contains("shape=restarted-inside-a-live-composition live_preedit_bytes=5 "),
        "{restart}"
    );
    assert!(
        restart.ends_with(&format!(" native={scripted:?} window={window:?}")),
        "the restart's line carries the facts the window's door answered: {restart}"
    );
    assert_eq!(
        facts_read(&folio),
        1,
        "the break read the window's facts once"
    );

    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Preedit(String::new(), None))
        .expect("an emptied pre-edit is answered");
    runtime
        .ime_input(Ime::Disabled)
        .expect("an end is answered");
    assert_eq!(facts_read(&folio), 0, "a paired end reads no facts");
    folio
        .runtime()
        .ime_input(Ime::Disabled)
        .expect("an unpaired end is answered");
    assert_eq!(
        facts_read(&folio),
        1,
        "the unpaired end, through ime_input, read the window's facts through its door"
    );
}
