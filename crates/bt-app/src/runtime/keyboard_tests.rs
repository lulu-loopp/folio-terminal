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
}

/// The one caret placement in `effects`, or a panic naming what was there.
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
/// emptied, ended — are the very list main `3cec1b48a` produced for the same steps (this test's
/// `MAIN_AREAS`, recorded there), so Microsoft Pinyin's placement is untouched; the caret stands
/// at each area's corner while it lasts; and the composition ending no longer takes the caret
/// with it (it is the keyboard owner's, not the composition's).
///
/// MUTATION (observed red): drop the `if self.window.ime_active` around
/// `self.window.ime_cursor.want(area)` in `Runtime::offer_ime_caret` — the forms are told before
/// the composition opens.
#[test]
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
    assert_eq!(
        areas, MAIN_AREAS,
        "the input method's forms, call for call, as main told them: {before:?} {composed:?} {after:?}"
    );
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

/// What main `3cec1b48a` told the input method for
/// [`a_compositions_input_method_calls_are_unchanged_and_the_caret_outlives_it`]'s steps.
const MAIN_AREAS: &[[i32; 4]] = &[[44, 48, 9, 22], [80, 48, 9, 22], [44, 48, 9, 22]];

/// RED (T-CARET-EVERY-LAYOUT ⑥) — **a window that opens without the focus never creates the
/// caret**; given the focus, it does.
///
/// The thread has one caret, so a background window's frames must not place one — not even the
/// first, when there is nothing of its own to take down.
///
/// MUTATION (observed red): drop `self.window.window_focused &&` from `caret_owner` in
/// `Runtime::offer_ime_caret` — the unfocused window places a caret.
#[test]
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
