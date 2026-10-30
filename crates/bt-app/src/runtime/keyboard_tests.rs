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
/// MUTATIONS: `ime_native_facts` reads `bt_platform::ime_observation::snapshot` directly instead of
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

// ── the system caret on every layout (T-CARET-EVERY-LAYOUT, plan row 87) ──
//
// The headless window's caret door records every `ImeSystemCaret::update` as `ImeCaret(x, y)`
// and every `destroy` as `ImeCaretDestroyed`; its `set_ime_cursor_area` records `ImeArea`. The
// layout gate that used to sit inside `bt_platform::ImeSystemCaret::update` is below this seam
// and is gone with its code; what these tests drive is the runtime's half: a caret wanted
// whenever a text owner holds the focused window's keyboard, composing or not, and taken down
// when none does.

/// The caret and input-method effects the window was told since this was last asked, in order.
fn caret_effects(ledger: &crate::headless::Ledger) -> Vec<WindowEffect> {
    ledger
        .take()
        .into_iter()
        .filter(|effect| {
            matches!(
                effect,
                WindowEffect::ImeCaret(..)
                    | WindowEffect::ImeCaretDestroyed
                    | WindowEffect::ImeArea(..)
            )
        })
        .collect()
}

/// One expose frame composed and drawn, then one turn of the window at `now` — every road that
/// offers a caret, and the flush that tells it.
fn paint_and_turn(runtime: &mut crate::Runtime<'_, crate::headless::Headless>, now: Instant) {
    runtime
        .publish_frame(bt_render::FrameTrigger {
            occurred_at: Instant::now(),
            source: bt_render::FrameSource::Expose,
        })
        .expect("a headless frame composes");
    runtime.redraw().expect("and reaches the glass");
    runtime.turn(now, true).expect("the turn is answered");
}

/// The turns of one test, a whole caret interval and more apart, so no area is held for its
/// 60 Hz slot.
struct Turns(Instant);

impl Turns {
    fn start() -> Self {
        Self(Instant::now())
    }

    fn next(&mut self) -> Instant {
        self.0 += Duration::from_millis(250);
        self.0
    }

    /// Count on from the later of this test's last turn and the real clock, after a step the
    /// product times with `Instant::now()` itself (`Ime::Enabled` flushes the area at the real
    /// instant): every turn after this is a whole interval and more past both that flush and the
    /// turns before it, however long the steps took on a loaded machine (review F2).
    #[cfg(windows)]
    fn after_the_real_flush(&mut self) {
        self.0 = self.0.max(Instant::now());
    }
}

/// The one caret placement in `effects`, or a panic naming what was there.
#[cfg(windows)]
fn the_one_caret(effects: &[WindowEffect]) -> (i32, i32) {
    let placed: Vec<(i32, i32)> = effects
        .iter()
        .filter_map(|effect| match effect {
            WindowEffect::ImeCaret(x, y) => Some((*x, *y)),
            _ => None,
        })
        .collect();
    let [at] = placed.as_slice() else {
        panic!("one caret placement: {effects:?}");
    };
    *at
}

/// RED (T-CARET-EVERY-LAYOUT ①) — **a shell holding the keyboard has a system caret without any
/// composition**, at the very point the input method is told once one starts.
///
/// An English layout never composes, so before this ticket no `Ime::Enabled` ever came, the
/// offer returned on `!ime_active`, and Magnifier, the emoji panel and every `GetGUIThreadInfo`
/// reader found no caret in Folio. Now the frame's offer wants the grid's caret into the caret's
/// own slot and the turn places it — and the input method's forms are still told nothing until a
/// composition opens; when one does, its area's corner is the caret's.
///
/// MUTATION (observed red): restore the `if !self.window.ime_active { return; }` head of
/// `Runtime::offer_ime_caret` — no `ImeCaret` before the composition.
#[test]
#[cfg(windows)]
fn the_shell_cursor_has_a_system_caret_without_a_composition() {
    let mut folio = HeadlessFolio::with_tabs(
        "caret-every-layout-shell",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    paint_and_turn(&mut folio.runtime(), turns.next());
    let before = caret_effects(&folio.ledger);
    assert!(
        !before
            .iter()
            .any(|effect| matches!(effect, WindowEffect::ImeArea(..))),
        "no composition, so the input method's forms are told nothing: {before:?}"
    );
    let caret = the_one_caret(&before);

    folio
        .runtime()
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    let started = caret_effects(&folio.ledger);
    let Some(WindowEffect::ImeArea([x, y, _, _])) = started.first() else {
        panic!("a composition opening tells the input method its area: {started:?}");
    };
    assert_eq!(
        (*x, *y),
        caret,
        "the caret stood where the composition's area is"
    );
    assert!(
        !started
            .iter()
            .any(|effect| matches!(effect, WindowEffect::ImeCaret(..))),
        "the caret already stands there and is not placed again: {started:?}"
    );
}

/// RED (T-CARET-EVERY-LAYOUT ②) — **focus loss takes the caret down, and the window's later
/// turns do not bring it back** until the focus returns.
///
/// A Win32 caret is the thread's: a background window that placed one would take the focused
/// window's away. So the blur destroys it, and the offers of a window without the focus — its
/// frames still compose — want nothing.
///
/// MUTATION (observed red): drop `self.window.window_focused &&` from `caret_owner` in
/// `Runtime::offer_ime_caret` — the frame after the blur places the caret again.
#[test]
#[cfg(windows)]
fn focus_loss_takes_the_caret_and_the_next_turns_do_not_bring_it_back() {
    let folio = HeadlessFolio::with_tabs(
        "caret-every-layout-blur",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let ledger = folio.ledger.clone();
    let mut application = HeadlessApplication::of(folio);
    let mut turns = Turns::start();
    application.window_event(WindowEvent::Focused(true));
    paint_and_turn(&mut application.runtime(), turns.next());
    the_one_caret(&caret_effects(&ledger));

    application.window_event(WindowEvent::Focused(false));
    for _ in 0..3 {
        paint_and_turn(&mut application.runtime(), turns.next());
    }
    assert_eq!(
        caret_effects(&ledger),
        vec![WindowEffect::ImeCaretDestroyed],
        "the blur destroyed the caret once, and nothing placed it again"
    );

    application.window_event(WindowEvent::Focused(true));
    paint_and_turn(&mut application.runtime(), turns.next());
    the_one_caret(&caret_effects(&ledger));
}

/// RED (T-CARET-EVERY-LAYOUT ③) — **an owner without a caret takes it down**: the settings sheet
/// (a modal, `ImeCaretSource::Nowhere`) holds the keyboard, and the caret that stood at the
/// prompt is destroyed — once, not on every turn the sheet stays up. The sheet closing brings it
/// back.
///
/// MUTATION (observed red): drop the `self.vacate_system_caret()` call from
/// `Runtime::offer_ime_caret` — the caret stays standing at the prompt under the sheet.
#[test]
#[cfg(windows)]
fn an_owner_without_a_caret_takes_it_down_once() {
    let mut folio = HeadlessFolio::with_tabs(
        "caret-every-layout-modal",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    paint_and_turn(&mut folio.runtime(), turns.next());
    the_one_caret(&caret_effects(&folio.ledger));

    folio
        .runtime()
        .toggle_settings_panel()
        .expect("the sheet opens");
    for _ in 0..3 {
        paint_and_turn(&mut folio.runtime(), turns.next());
    }
    assert_eq!(
        caret_effects(&folio.ledger),
        vec![WindowEffect::ImeCaretDestroyed],
        "the sheet took the caret down once"
    );

    folio
        .runtime()
        .toggle_settings_panel()
        .expect("the sheet closes");
    paint_and_turn(&mut folio.runtime(), turns.next());
    the_one_caret(&caret_effects(&folio.ledger));
}

/// RED (T-CARET-EVERY-LAYOUT ④) — **a field holding the keyboard moves the caret to the field**:
/// the command palette opens over the prompt, and the caret leaves the grid for the palette's
/// own caret line — the point the input method is told when a composition starts there.
///
/// MUTATION (observed red): want the system caret only for `ImeCaretSource::TerminalCursor` in
/// `Runtime::offer_ime_caret` — the caret stays at the prompt.
#[test]
#[cfg(windows)]
fn a_field_holding_the_keyboard_moves_the_caret_to_the_field() {
    let mut folio = HeadlessFolio::with_tabs(
        "caret-every-layout-field",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    paint_and_turn(&mut folio.runtime(), turns.next());
    let at_the_prompt = the_one_caret(&caret_effects(&folio.ledger));

    folio
        .runtime()
        .open_command_palette()
        .expect("the palette opens");
    paint_and_turn(&mut folio.runtime(), turns.next());
    paint_and_turn(&mut folio.runtime(), turns.next());
    let in_the_palette = the_one_caret(&caret_effects(&folio.ledger));
    assert_ne!(in_the_palette, at_the_prompt, "the caret left the grid");

    folio
        .runtime()
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    let started = caret_effects(&folio.ledger);
    let Some(WindowEffect::ImeArea([x, y, _, _])) = started.first() else {
        panic!("a composition in the palette tells its area: {started:?}");
    };
    assert_eq!(
        (*x, *y),
        in_the_palette,
        "the caret stands where the palette's composition is placed"
    );
}

/// RED (T-CARET-EVERY-LAYOUT ⑤) — **a composition's input-method calls are what they were**, and
/// the caret outlives it.
///
/// The IMM32 forms (`ImeArea`) a whole composition at the prompt produces — opened, typed into,
/// emptied, ended — are the very list main `3cec1b48a` produced for the same steps, so Microsoft
/// Pinyin's placement is untouched. Recorded there once (this file over main's product code):
/// `[[44, 48, 9, 22], [80, 48, 9, 22], [44, 48, 9, 22]]` — three calls, the second four cells
/// to the right of the first, the third back at the first; asserted as that shape rather than
/// those pixels (review F3), so a change to the headless metrics does not read as an IME change; the caret stands
/// at each area's corner while it lasts; and the composition ending no longer takes the caret
/// with it (it is the keyboard owner's, not the composition's).
///
/// MUTATION (observed red): drop the `if self.window.ime_active` around
/// `self.window.ime_cursor.want(area)` in `Runtime::offer_ime_caret` — the forms are told before
/// the composition opens.
#[test]
#[cfg(windows)]
fn a_compositions_input_method_calls_are_unchanged_and_the_caret_outlives_it() {
    let mut folio = HeadlessFolio::with_tabs(
        "caret-every-layout-pinyin",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    paint_and_turn(&mut folio.runtime(), turns.next());
    let before = caret_effects(&folio.ledger);

    let mut composed = Vec::new();
    folio
        .runtime()
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    turns.after_the_real_flush();
    composed.extend(caret_effects(&folio.ledger));
    folio
        .runtime()
        .ime_input(Ime::Preedit("ni你".to_owned(), Some((5, 5))))
        .expect("a pre-edit is answered");
    folio
        .runtime()
        .turn(turns.next(), true)
        .expect("the turn is answered");
    composed.extend(caret_effects(&folio.ledger));
    folio
        .runtime()
        .ime_input(Ime::Preedit(String::new(), None))
        .expect("an emptied pre-edit is answered");
    folio
        .runtime()
        .turn(turns.next(), true)
        .expect("the turn is answered");
    composed.extend(caret_effects(&folio.ledger));
    folio
        .runtime()
        .ime_input(Ime::Disabled)
        .expect("an end is answered");
    paint_and_turn(&mut folio.runtime(), turns.next());
    let after = caret_effects(&folio.ledger);

    let areas: Vec<[i32; 4]> = before
        .iter()
        .chain(&composed)
        .chain(&after)
        .filter_map(|effect| match effect {
            WindowEffect::ImeArea(area) => Some(*area),
            _ => None,
        })
        .collect();
    let [opened, typed, emptied] = areas.as_slice() else {
        panic!(
            "the input method's forms, call for call, as main told them — opened, typed into, \
             emptied: {before:?} {composed:?} {after:?}"
        );
    };
    let [x, y, width, height] = *opened;
    assert_eq!(
        *typed,
        [x + 4 * width, y, width, height],
        "typed into: the area four cells on, past \"ni你\" (n, i, and one wide character)"
    );
    assert_eq!(emptied, opened, "emptied: the area back at the prompt");
    let placed: Vec<(i32, i32)> = before
        .iter()
        .chain(&composed)
        .filter_map(|effect| match effect {
            WindowEffect::ImeCaret(x, y) => Some((*x, *y)),
            _ => None,
        })
        .collect();
    let mut corners: Vec<(i32, i32)> = areas.iter().map(|area| (area[0], area[1])).collect();
    corners.dedup();
    assert_eq!(
        placed, corners,
        "the caret stood at every corner the forms were told, before and during the composition"
    );
    assert!(
        !composed
            .iter()
            .chain(&after)
            .any(|effect| matches!(effect, WindowEffect::ImeCaretDestroyed)),
        "the composition ending leaves the caret standing: {composed:?} {after:?}"
    );
}

/// RED (T-CARET-EVERY-LAYOUT ⑥) — **a window that opens without the focus never creates the
/// caret**; given the focus, it does.
///
/// The thread has one caret, so a background window's frames must not place one — not even the
/// first, when there is nothing of its own to take down.
///
/// MUTATION (observed red): drop `self.window.window_focused &&` from `caret_owner` in
/// `Runtime::offer_ime_caret` — the unfocused window places a caret.
#[test]
#[cfg(windows)]
fn an_unfocused_window_never_creates_the_caret() {
    let folio = HeadlessFolio::with_tabs(
        "caret-every-layout-unfocused",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let ledger = folio.ledger.clone();
    let mut application = HeadlessApplication::of(folio);
    let mut turns = Turns::start();
    application.window_event(WindowEvent::Focused(false));
    ledger.take();
    for _ in 0..3 {
        paint_and_turn(&mut application.runtime(), turns.next());
    }
    assert_eq!(
        caret_effects(&ledger),
        Vec::new(),
        "a window without the focus places no caret and has none to take down"
    );

    application.window_event(WindowEvent::Focused(true));
    paint_and_turn(&mut application.runtime(), turns.next());
    the_one_caret(&caret_effects(&ledger));
}

/// RED (T-CARET-EVERY-LAYOUT ⑦) — **a hosted page holding the keyboard has no caret of ours**:
/// its engine keeps its own, and one of ours created over it could take it away. The caret that
/// stood at the prompt is destroyed when the keyboard goes to the page seat.
///
/// MUTATION (observed red): drop `&& !self.page_holds_the_keyboard()` from `caret_owner` in
/// `Runtime::offer_ime_caret` — the caret stays standing at the prompt (the page rung has no
/// area of its own to move it to).
#[test]
#[cfg(windows)]
fn a_page_holding_the_keyboard_has_no_caret_of_ours() {
    let (tab, page_seat) = crate::test_support::tab_with_a_preview(1, Vec::new());
    let mut folio = HeadlessFolio::with_tabs("caret-every-layout-page", vec![tab], 0);
    let mut turns = Turns::start();
    paint_and_turn(&mut folio.runtime(), turns.next());
    the_one_caret(&caret_effects(&folio.ledger));

    {
        let mut runtime = folio.runtime();
        let leaf = crate::LeafId {
            tab: runtime.id,
            seat: page_seat,
        };
        runtime.window.web.insert(
            leaf,
            crate::webhost::WebSeat::shown_without_an_engine(crate::webhost::WebBounds {
                x: 0,
                y: 0,
                width: 200,
                height: 200,
            }),
        );
        assert!(
            runtime.seats.set_focus(page_seat),
            "the page seat takes the focus"
        );
        assert!(
            runtime.page_holds_the_keyboard(),
            "the fixture's own precondition"
        );
    }
    for _ in 0..2 {
        paint_and_turn(&mut folio.runtime(), turns.next());
    }
    assert_eq!(
        caret_effects(&folio.ledger),
        vec![WindowEffect::ImeCaretDestroyed],
        "the page's keyboard took our caret down"
    );
}

/// Whether a resize waits at the PTY gate in any pane of the window.
fn a_resize_waits_at_the_pty_gate(folio: &HeadlessFolio) -> bool {
    folio
        .window
        .tabs
        .iter()
        .flat_map(|tab| tab.leaves())
        .any(|(_, leaf)| leaf.pending_pty_resize.is_some())
}

/// RED (T-CARET-EVERY-LAYOUT, review F4) — **a resize the PTY gate releases leaves no caret owed
/// at the turn's own instant.**
///
/// The release publishes the pane's frame, and that frame's offer wants the caret. The turn tells
/// the caret area after the gate, so the same turn answers that want. Told before the gate, the
/// want outlived the flush, `ImeCursorSlot::deadline` read it as owed at once, and the turn handed
/// the loop a wake at its own instant — a wait that is no wait.
///
/// MUTATION (observed red): move the `flush_ime_cursor_area` station in `Runtime::turn` back above
/// `flush_pending_pty_resize` — the turn hands the loop `ImeCursor` at its own instant, and the
/// caret's slot still owes it.
#[test]
fn a_resize_the_pty_gate_releases_leaves_no_caret_owed_at_the_turns_instant() {
    let mut folio = HeadlessFolio::with_tabs(
        "caret-every-layout-pty-gate",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    folio.draw();
    assert!(
        a_resize_waits_at_the_pty_gate(&folio),
        "the fixture's own precondition: the new pane's size waits at the gate"
    );
    let now = Instant::now() + Duration::from_secs(1);
    let wake = folio
        .runtime()
        .turn(now, true)
        .expect("the turn is answered");
    assert!(
        !a_resize_waits_at_the_pty_gate(&folio),
        "the turn's gate released the resize"
    );
    if let Some((owner, at)) = wake {
        assert!(
            at > now,
            "the turn handed the loop {owner:?}'s instant {} us in the past",
            now.saturating_duration_since(at).as_micros()
        );
    }
    assert_eq!(
        folio.window.system_caret.deadline(now),
        None,
        "nothing is owed to the system caret after the turn"
    );
}

/// RED (T-CARET-EVERY-LAYOUT, review F5, ruled 2026-10-27) — **the system caret is fed on Windows
/// alone.** On Windows a shell holding the keyboard has its caret placed. Elsewhere the platform's
/// `ImeSystemCaret` is a no-op, so the runtime places none, takes none down, keeps nothing in the
/// caret's slot and books no wake for it.
///
/// The expectation reads [`super::SYSTEM_CARET_FED`], and the last assertion pins that constant to
/// `cfg!(windows)`. So the Mac half can be driven on Windows: plant the constant `false` and every
/// behaviour assertion holds, with only the pin red.
///
/// MUTATIONS (observed red, on Windows, with the constant planted `false`): drop
/// `SYSTEM_CARET_FED &&` from `may_hold_the_caret` in `Runtime::offer_ime_caret` — a caret is
/// placed and its slot holds the area. Plant the constant `false` alone — the pin goes red.
#[test]
fn the_system_caret_is_fed_on_windows_alone() {
    let mut folio = HeadlessFolio::with_tabs(
        "caret-every-layout-platform",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    paint_and_turn(&mut folio.runtime(), turns.next());
    let effects = caret_effects(&folio.ledger);
    let placed = effects
        .iter()
        .filter(|effect| matches!(effect, WindowEffect::ImeCaret(..)))
        .count();
    let fed = super::SYSTEM_CARET_FED;
    if fed {
        assert_eq!(placed, 1, "fed: the shell's caret is placed: {effects:?}");
    } else {
        assert_eq!(
            effects,
            Vec::new(),
            "not fed: no caret is placed or taken down"
        );
        let now = turns.next();
        let wake = folio
            .runtime()
            .turn(now, true)
            .expect("the turn is answered");
        assert_eq!(
            folio.window.system_caret.last_told(),
            None,
            "not fed: the caret's slot told nothing"
        );
        assert_eq!(
            folio.window.system_caret.deadline(now),
            None,
            "not fed: the caret's slot owes nothing"
        );
        assert!(
            !matches!(wake, Some((crate::wake_owner::WakeOwner::ImeCursor, _))),
            "not fed: no wake is booked for the caret: {wake:?}"
        );
    }
    assert_eq!(
        fed,
        cfg!(windows),
        "the system caret is fed on Windows and nowhere else"
    );
}

// ── T-SETTINGS-FIELDS-IME-POSITION: the settings dialog's text fields ─────────────────────────

/// The settings target of the summoned terminal's startup-command row — the one field the
/// dialog has outside the profile editor.
fn quake_command_field() -> crate::settings::SettingsTarget {
    crate::settings::SettingsTarget::Field(crate::settings::SettingsRow::QuakeCommand)
}

/// The text `target`'s field holds, and the composition standing in it.
fn field_text(folio: &HeadlessFolio, target: crate::settings::SettingsTarget) -> (String, String) {
    let field = folio
        .window
        .settings
        .text_field(target)
        .expect("the fixture's field");
    (field.text().to_owned(), field.preedit().to_owned())
}

/// A window with the profile editor up on profile 0, its Advanced group open (the arguments and
/// the environment table are in it), holding one environment row of the reader's own, and the
/// focus on `focus`.
fn editor_on(tag: &str, focus: crate::settings::SettingsTarget) -> HeadlessFolio {
    use crate::settings::SettingsCategory;
    use crate::text_field::TextField;
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab_holding(leaf_saying("PS> "))], 0);
    {
        let mut runtime = folio.runtime();
        let mut stored = runtime.app.settings_store.loaded().clone();
        stored.advanced_open = vec![SettingsCategory::Profiles.key().to_owned()];
        runtime.app.settings_store.store(stored);
        runtime.toggle_settings_panel().expect("the sheet opens");
        runtime
            .window
            .settings
            .select_category(SettingsCategory::Profiles);
        runtime
            .window
            .settings
            .open_editor(crate::settings::ProfileEditor {
                index: 0,
                name: TextField::holding("Shell"),
                program: TextField::holding("pwsh.exe"),
                args: TextField::holding("-NoLogo"),
                env: vec![(TextField::holding("NAME"), TextField::holding("value"))],
                refusal: None,
                program_refusal: None,
            });
        runtime.window.settings.press(focus);
    }
    assert!(
        folio.runtime().settings_field_holds_the_keyboard(),
        "the fixture's own precondition: {focus:?} holds the keyboard"
    );
    folio
}

/// The carets the frame's overlay struck inside `target`'s field box: one-physical-pixel accent
/// bars, read off what the headless glass was handed — the painter's picture, not a second
/// derivation of it.
fn painted_carets(
    folio: &mut HeadlessFolio,
    target: crate::settings::SettingsTarget,
) -> Vec<[f32; 4]> {
    let mut runtime = folio.runtime();
    let layout = runtime.settings_layout().expect("the dialog is up");
    let (field, _) = layout
        .text_field_box(target)
        .expect("the layout placed the field");
    let thickness = layout.scale().round().max(1.0);
    let field_band_slack = 8.0 * layout.scale();
    let accent = bt_render::chrome_palette().accent;
    runtime
        .window
        .renderer
        .overlay
        .0
        .iter()
        .flat_map(|layer| layer.quads.iter())
        .filter(|quad| {
            quad.color == accent
                && (quad.alpha - 1.0).abs() < f32::EPSILON
                && ((quad.rect[2] - quad.rect[0]) - thickness).abs() < 0.01
                && quad.rect[0] > field[0]
                && quad.rect[2] < field[2]
                // A bar and not a pixel: a switch's rounded corners strike one-pixel accent
                // squares too. Within the field's band, give or take the arrival's travel.
                && quad.rect[3] - quad.rect[1] > field_band_slack
                && quad.rect[1] >= field[1] - field_band_slack
                && quad.rect[3] <= field[3] + field_band_slack
        })
        .map(|quad| quad.rect)
        .collect()
}

/// The `x` of the caret the frame struck in `target`'s field, rounded as `ime_cursor_area_of`
/// rounds a corner. Only the `x`: the dialog's arrival travels its layers four pixels down, so
/// the frame's `y` is the layout's only once the arrival is over.
fn the_painted_caret_x(folio: &mut HeadlessFolio, target: crate::settings::SettingsTarget) -> i32 {
    let painted = painted_carets(folio, target);
    let [caret] = painted.as_slice() else {
        panic!("one caret struck in {target:?}: {painted:?}");
    };
    caret[0].round() as i32
}

/// Whether `(x, y)` lies in `target`'s field box as the layout placed it.
fn inside_the_field(
    folio: &mut HeadlessFolio,
    target: crate::settings::SettingsTarget,
    (x, y): (i32, i32),
) -> bool {
    let layout = folio.runtime().settings_layout().expect("the dialog is up");
    let (field, _) = layout.text_field_box(target).expect("the field's box");
    let (x, y) = (x as f32, y as f32);
    x > field[0] && x < field[2] && y > field[1] && y < field[3]
}

/// RED (T-SETTINGS-FIELDS-IME-POSITION ①) — **a composition in a settings field is drawn there,
/// and its commit is stored**.
///
/// Before the ticket the dialog's fields were the `Modal` rung, which swallows every composition:
/// the pre-edit was never drawn and the commit — which winit delivers only as `Ime::Commit`, no
/// `WM_CHAR` following it — was dropped, so Chinese could not be typed into Settings at all. Now
/// the pre-edit stands in the field and is drawn spliced in at the caret, and the commit is an
/// insert written through to `settings.json` as a typed character is.
///
/// MUTATION (observed red): make `Runtime::ime_input`'s `ImeOwner::SettingsField` arm
/// `return Ok(())` without calling `settings_field_ime` — no pre-edit, no commit; or hand the
/// painter `quake_command()` instead of the field's `composed()` in `runtime/math.rs` — the
/// pre-edit is not drawn.
#[test]
fn a_composition_in_a_settings_field_is_drawn_there_and_its_commit_is_stored() {
    let mut folio = HeadlessFolio::with_tabs(
        "settings-field-ime-text",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    folio
        .runtime()
        .open_settings_on_row(crate::settings::SettingsRow::QuakeCommand)
        .expect("the dialog opens on the field");
    let target = quake_command_field();
    let (before, _) = field_text(&folio, target);
    assert_eq!(
        crate::runtime::keyboard::ime_owner(folio.runtime().keyboard_owner()),
        crate::ImeOwner::SettingsField,
        "a settings field holding the keyboard is its own rung"
    );

    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    runtime
        .ime_input(Ime::Preedit("ni".to_owned(), Some((2, 2))))
        .expect("a pre-edit is answered");
    assert_eq!(
        field_text(&folio, target),
        (before.clone(), "ni".to_owned()),
        "the pre-edit stands in the field and is not in its text"
    );
    paint_and_turn(&mut folio.runtime(), Instant::now());
    let drawn = format!("{before}ni");
    assert!(
        folio
            .runtime()
            .window
            .renderer
            .overlay
            .0
            .iter()
            .flat_map(|layer| layer.labels.iter())
            .any(|label| label.text == drawn),
        "the field draws its text with the pre-edit spliced in at the caret: {drawn:?}"
    );

    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Preedit(String::new(), None))
        .expect("the clearing pre-edit is answered");
    runtime
        .ime_input(Ime::Commit("你".to_owned()))
        .expect("a commit is answered");
    runtime
        .ime_input(Ime::Disabled)
        .expect("an end is answered");
    let committed = format!("{before}你");
    assert_eq!(
        field_text(&folio, target),
        (committed.clone(), String::new()),
        "the commit is in the field and the pre-edit is gone"
    );
    assert_eq!(
        folio
            .runtime()
            .app
            .settings_store
            .loaded()
            .quake_startup_command,
        committed,
        "and it is stored, as a typed character is"
    );
}

/// RED (T-SETTINGS-FIELDS-IME-POSITION ②) — **a settings field holding the keyboard tells its
/// caret to both takers**: the system caret leaves the prompt for the very bar the painter
/// struck in the field, and a composition opening there tells the input method the same corner.
///
/// MUTATION (observed red): answer `ImeCaretSource::Nowhere` for `ImeOwner::SettingsField` in
/// `ime_caret_source` — the caret is taken down and no area is told.
#[test]
#[cfg(windows)]
fn a_settings_field_holding_the_keyboard_tells_its_caret_to_both_takers() {
    let mut folio = HeadlessFolio::with_tabs(
        "settings-field-ime-caret",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    paint_and_turn(&mut folio.runtime(), turns.next());
    let at_the_prompt = the_one_caret(&caret_effects(&folio.ledger));

    folio
        .runtime()
        .open_settings_on_row(crate::settings::SettingsRow::QuakeCommand)
        .expect("the dialog opens on the field");
    paint_and_turn(&mut folio.runtime(), turns.next());
    paint_and_turn(&mut folio.runtime(), turns.next());
    let target = quake_command_field();
    let in_the_field = the_one_caret(&caret_effects(&folio.ledger));
    assert_ne!(in_the_field, at_the_prompt, "the caret left the prompt");
    assert_eq!(
        in_the_field.0,
        the_painted_caret_x(&mut folio, target),
        "the caret stands on the bar the painter struck"
    );
    assert!(
        inside_the_field(&mut folio, target, in_the_field),
        "and inside the field's box: {in_the_field:?}"
    );

    folio
        .runtime()
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    let started = caret_effects(&folio.ledger);
    let Some(WindowEffect::ImeArea([x, y, _, _])) = started.first() else {
        panic!("a composition in the field tells its area: {started:?}");
    };
    assert_eq!(
        (*x, *y),
        in_the_field,
        "the composition is placed where the caret stands"
    );
}

/// RED (T-SETTINGS-FIELDS-IME-POSITION ③) — **typing in a settings field moves the caret, and
/// `Home` brings it back**, each time onto the bar the painter struck.
///
/// MUTATION (observed red): in `Runtime::settings_field_caret_line` measure the field's whole
/// shown text rather than what stands in front of the caret — after `Home` the caret stays at
/// the end of the text.
#[test]
#[cfg(windows)]
fn typing_in_a_settings_field_moves_the_caret_and_home_brings_it_back() {
    use crate::headless::key_down;
    use winit::keyboard::{Key, KeyCode, NamedKey};
    let mut folio = HeadlessFolio::with_tabs(
        "settings-field-ime-typing",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    folio
        .runtime()
        .open_settings_on_row(crate::settings::SettingsRow::QuakeCommand)
        .expect("the dialog opens on the field");
    paint_and_turn(&mut folio.runtime(), turns.next());
    paint_and_turn(&mut folio.runtime(), turns.next());
    let target = quake_command_field();
    let opened = the_one_caret(&caret_effects(&folio.ledger));

    for _ in 0..3 {
        folio
            .runtime()
            .keyboard_input(
                &key_down(Key::Character("a".into()), KeyCode::KeyA, Some("a")),
                false,
            )
            .expect("a letter is answered");
    }
    paint_and_turn(&mut folio.runtime(), turns.next());
    let typed = the_one_caret(&caret_effects(&folio.ledger));
    assert!(
        typed.0 > opened.0,
        "three letters move the caret right: {opened:?} {typed:?}"
    );
    assert_eq!(typed.1, opened.1, "along the line");
    assert_eq!(typed.0, the_painted_caret_x(&mut folio, target));

    folio
        .runtime()
        .keyboard_input(
            &key_down(Key::Named(NamedKey::Home), KeyCode::Home, None),
            false,
        )
        .expect("Home is answered");
    paint_and_turn(&mut folio.runtime(), turns.next());
    let home = the_one_caret(&caret_effects(&folio.ledger));
    assert!(home.0 < typed.0, "Home moves it left: {home:?} {typed:?}");
    assert!(
        home.0 <= opened.0,
        "to the start of the text: {home:?} {opened:?}"
    );
    assert_eq!(home.0, the_painted_caret_x(&mut folio, target));
}

/// RED (T-SETTINGS-FIELDS-IME-POSITION ④) — **a field losing the keyboard to a control that is
/// not a field takes the caret down; closing the dialog hands it back to the prompt** (ruling
/// 2026-10-30: the terminal does not hold the keyboard while the dialog is up, so the caret
/// stands nowhere until it closes — T-CARET-EVERY-LAYOUT ③'s rule).
///
/// MUTATION (observed red): drop the `text_field(target).is_some()` clause from
/// `Runtime::settings_field_holds_the_keyboard` — the control after the field keeps the caret.
#[test]
#[cfg(windows)]
fn leaving_the_field_takes_the_caret_down_and_closing_the_dialog_hands_it_to_the_prompt() {
    use crate::headless::key_down;
    use winit::keyboard::{Key, KeyCode, NamedKey};
    let mut folio = HeadlessFolio::with_tabs(
        "settings-field-ime-leave",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    paint_and_turn(&mut folio.runtime(), turns.next());
    let at_the_prompt = the_one_caret(&caret_effects(&folio.ledger));

    folio
        .runtime()
        .open_settings_on_row(crate::settings::SettingsRow::QuakeCommand)
        .expect("the dialog opens on the field");
    paint_and_turn(&mut folio.runtime(), turns.next());
    let in_the_field = the_one_caret(&caret_effects(&folio.ledger));
    assert_ne!(in_the_field, at_the_prompt);

    folio
        .runtime()
        .keyboard_input(
            &key_down(Key::Named(NamedKey::Tab), KeyCode::Tab, None),
            false,
        )
        .expect("Tab is answered");
    let walked_to = folio.window.settings.focus();
    assert!(
        walked_to.is_some_and(|target| folio.window.settings.text_field(target).is_none()),
        "the fixture's own precondition: Tab walked to a control that is not a field: {walked_to:?}"
    );
    for _ in 0..3 {
        paint_and_turn(&mut folio.runtime(), turns.next());
    }
    assert_eq!(
        caret_effects(&folio.ledger),
        vec![WindowEffect::ImeCaretDestroyed],
        "the control took the caret down, once"
    );

    folio
        .runtime()
        .toggle_settings_panel()
        .expect("the sheet closes");
    paint_and_turn(&mut folio.runtime(), turns.next());
    assert_eq!(
        the_one_caret(&caret_effects(&folio.ledger)),
        at_the_prompt,
        "the dialog closing hands the caret back to the prompt"
    );
}

/// RED (T-SETTINGS-FIELDS-IME-POSITION ⑤) — **a composition carried from one settings field to
/// another lands in neither** (§7.1.5a″): begun in the profile's program, its field left for the
/// arguments by a press, its ending arriving there before any pass has run — the commit belongs
/// to the field it began in, which no longer holds the keyboard, and the clearing pre-edit takes
/// the letters drawn in the program off. A pass between the press and the ending cancels the
/// composition outright (⑧).
///
/// MUTATION (observed red): compose `CompositionOrigin::Settings(None)` in
/// `Runtime::composition_origin_now` — two fields compare equal and the commit lands in the
/// arguments.
#[test]
fn a_composition_carried_from_one_settings_field_to_another_lands_in_neither() {
    use crate::settings::{SettingsRow, SettingsTarget};
    let program = SettingsTarget::Field(SettingsRow::ProfileProgram);
    let args = SettingsTarget::Field(SettingsRow::ProfileArgs);
    let mut folio = editor_on("settings-field-ime-carried", program);

    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    runtime
        .ime_input(Ime::Preedit("ni".to_owned(), Some((2, 2))))
        .expect("a pre-edit is answered");
    assert_eq!(
        field_text(&folio, program).1,
        "ni",
        "composing in the program"
    );

    folio.window.settings.press(args);
    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Preedit(String::new(), None))
        .expect("the clearing pre-edit is answered");
    runtime
        .ime_input(Ime::Commit("你".to_owned()))
        .expect("the late commit is answered");
    assert_eq!(
        field_text(&folio, args),
        ("-NoLogo".to_owned(), String::new()),
        "the commit did not land in the field the composition did not begin in"
    );
    assert_eq!(
        field_text(&folio, program),
        ("pwsh.exe".to_owned(), String::new()),
        "nor in the one it left, whose letters are off the glass"
    );
}

/// RED (T-SETTINGS-FIELDS-IME-POSITION ⑥) — **every field of the profile editor tells its caret**:
/// the name, the program and the arguments (a row's field) and an environment row's two cells
/// (the table's) — each onto the bar the painter struck in that box.
///
/// MUTATION (observed red): answer `None` for `EnvName` / `EnvValue` in
/// `SettingsLayout::text_field_box` — the environment cells move no caret.
#[test]
#[cfg(windows)]
fn every_field_kind_of_the_profile_editor_has_its_caret() {
    use crate::settings::{SettingsRow, SettingsTarget};
    let targets = [
        SettingsTarget::Field(SettingsRow::ProfileName),
        SettingsTarget::Field(SettingsRow::ProfileProgram),
        SettingsTarget::Field(SettingsRow::ProfileArgs),
        SettingsTarget::EnvName(0),
        SettingsTarget::EnvValue(0),
    ];
    let mut folio = editor_on("settings-field-ime-kinds", targets[0]);
    let mut turns = Turns::start();
    let mut seen = Vec::new();
    for target in targets {
        // The press, and the chrome rebuilt as the pointer's road rebuilds it.
        folio.window.settings.press(target);
        folio.runtime().refresh_chrome();
        paint_and_turn(&mut folio.runtime(), turns.next());
        let effects = caret_effects(&folio.ledger);
        let caret = the_one_caret(&effects);
        assert!(
            inside_the_field(&mut folio, target, caret),
            "{target:?}: the caret {caret:?} is in its own box"
        );
        assert_eq!(
            caret.0,
            the_painted_caret_x(&mut folio, target),
            "{target:?}: on the bar the painter struck"
        );
        seen.push(caret);
    }
    seen.dedup();
    assert_eq!(
        seen.len(),
        targets.len(),
        "five fields, five places: {seen:?}"
    );
}

/// RED (T-SETTINGS-FIELDS-IME-POSITION ⑦) — **a card over the dialog takes its field off the
/// rung**: with the uninstaller's card up, the keys are the card's, so the caret is taken down
/// and a composition's commit reaches neither the field nor the shell behind the dialog.
///
/// MUTATION (observed red): drop `card_above` from `Runtime::settings_field_holds_the_keyboard` —
/// the field keeps the caret and takes the commit under the card.
///
/// The commit half runs everywhere (review F2): the system caret is Windows's alone, the rung and
/// the commit are not.
#[test]
fn a_card_over_the_dialog_takes_the_field_off_the_rung() {
    let mut folio = HeadlessFolio::with_tabs(
        "settings-field-ime-card",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    folio
        .runtime()
        .open_settings_on_row(crate::settings::SettingsRow::QuakeCommand)
        .expect("the dialog opens on the field");
    paint_and_turn(&mut folio.runtime(), turns.next());
    #[cfg(windows)]
    the_one_caret(&caret_effects(&folio.ledger));
    let target = quake_command_field();
    let (before, _) = field_text(&folio, target);

    folio.window.settings.raise_uninstall_card();
    for _ in 0..2 {
        paint_and_turn(&mut folio.runtime(), turns.next());
    }
    #[cfg(windows)]
    assert_eq!(
        caret_effects(&folio.ledger),
        vec![WindowEffect::ImeCaretDestroyed],
        "the card took the caret down"
    );
    assert_ne!(
        crate::runtime::keyboard::ime_owner(folio.runtime().keyboard_owner()),
        crate::ImeOwner::SettingsField,
        "the field is off the rung under the card"
    );
    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    runtime
        .ime_input(Ime::Preedit("ni".to_owned(), Some((2, 2))))
        .expect("a pre-edit is answered");
    runtime
        .ime_input(Ime::Preedit(String::new(), None))
        .expect("the clearing pre-edit is answered");
    runtime
        .ime_input(Ime::Commit("你".to_owned()))
        .expect("a commit is answered");
    assert_eq!(
        field_text(&folio, target),
        (before, String::new()),
        "the field under the card took nothing"
    );
}

/// RED (review of T-SETTINGS-FIELDS-IME-POSITION, F1 — ⑧) — **moving the focus from one settings
/// field to another cancels the composition left behind** (§7.1.5a″): the two fields are one
/// rung, so the kinds alone would leave the program drawing the letters typed for it while the
/// candidate list followed the caret to the arguments, and a composition carried on would commit
/// there. The pass after the press cancels it — the letters are off the program and nothing is
/// being composed — and what the reader types next is a composition of the arguments' own.
///
/// MUTATION (observed red): drop the `moved_to_another_field` clause from
/// `Runtime::settle_composition_owner` — the program keeps its pre-edit and the composition
/// stays live.
#[test]
fn a_press_into_another_settings_field_cancels_the_composition_left_behind() {
    use crate::settings::{SettingsRow, SettingsTarget};
    let program = SettingsTarget::Field(SettingsRow::ProfileProgram);
    let args = SettingsTarget::Field(SettingsRow::ProfileArgs);
    let mut folio = editor_on("settings-field-ime-moved", program);
    let mut turns = Turns::start();

    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    runtime
        .ime_input(Ime::Preedit("ni".to_owned(), Some((2, 2))))
        .expect("a pre-edit is answered");
    assert_eq!(
        field_text(&folio, program).1,
        "ni",
        "composing in the program"
    );
    assert_eq!(folio.window.composing, Some(crate::ImeOwner::SettingsField));

    folio.window.settings.press(args);
    paint_and_turn(&mut folio.runtime(), turns.next());
    assert_eq!(
        field_text(&folio, program),
        ("pwsh.exe".to_owned(), String::new()),
        "the pass after the press took the program's letters off"
    );
    assert_eq!(
        folio.window.composing, None,
        "and the composition is cancelled, not carried"
    );

    let mut runtime = folio.runtime();
    runtime
        .ime_input(Ime::Preedit("nih".to_owned(), Some((3, 3))))
        .expect("a pre-edit is answered");
    runtime
        .ime_input(Ime::Preedit(String::new(), None))
        .expect("the clearing pre-edit is answered");
    runtime
        .ime_input(Ime::Commit("你好".to_owned()))
        .expect("a commit is answered");
    assert_eq!(
        field_text(&folio, program),
        ("pwsh.exe".to_owned(), String::new()),
        "nothing lands in the field that was left"
    );
    assert_eq!(
        field_text(&folio, args),
        ("-NoLogo你好".to_owned(), String::new()),
        "a composition begun after the cancel is the arguments' own"
    );
}

/// RED (review of T-SETTINGS-FIELDS-IME-POSITION, F2 — ⑨) — **a composition in a settings field
/// is placed at the field's caret, on every platform**: the input method's area
/// (`set_ime_cursor_area`) is what a Mac's candidate list follows, and it is the bar the painter
/// struck. ② proves the same with the system caret, which only Windows has.
///
/// MUTATION (observed red): answer `ImeCaretSource::Nowhere` for `ImeOwner::SettingsField` in
/// `ime_caret_source` — no area is told.
#[test]
fn a_composition_in_a_settings_field_is_placed_at_its_caret_on_every_platform() {
    let mut folio = HeadlessFolio::with_tabs(
        "settings-field-ime-area",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let mut turns = Turns::start();
    folio
        .runtime()
        .open_settings_on_row(crate::settings::SettingsRow::QuakeCommand)
        .expect("the dialog opens on the field");
    paint_and_turn(&mut folio.runtime(), turns.next());
    paint_and_turn(&mut folio.runtime(), turns.next());
    caret_effects(&folio.ledger);
    let target = quake_command_field();

    folio
        .runtime()
        .ime_input(Ime::Enabled)
        .expect("a start is answered");
    let started = caret_effects(&folio.ledger);
    let Some(&WindowEffect::ImeArea([x, y, _, _])) = started
        .iter()
        .find(|effect| matches!(effect, WindowEffect::ImeArea(..)))
    else {
        panic!("a composition in the field tells its area: {started:?}");
    };
    assert_eq!(
        x,
        the_painted_caret_x(&mut folio, target),
        "the area stands on the bar the painter struck"
    );
    assert!(
        inside_the_field(&mut folio, target, (x, y)),
        "and inside the field's box: {:?}",
        (x, y)
    );
}
