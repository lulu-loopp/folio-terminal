//! **The windows module's doors, driven on a headless window**: the close an exit's answer asks
//! for is taken by the event loop through [`crate::Runtime::take_close_request`], once
//! (T-DISPATCHER-WRITES-INTO-DOORS); and every group of the window's doors to the system — its
//! place, its dress, the keyboard and the foreground, attention, the taskbar button, the system's
//! news, a page's place — is driven through the runtime's own code
//! (T-NATIVE-HANDLE-BEHIND-THE-SEAM).

use crate::headless::{
    HeadlessApplication, HeadlessFolio, HeadlessParts, Ledger, PlatformAnswers, WindowEffect,
    headless_window,
};
use crate::test_support::{
    buffer_saying, host_spelling, leaf_saying, ringing_tab, tab_holding, tab_with_a_preview,
};
use crate::{preview_edit, restore};
use winit::dpi::PhysicalPosition;
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

// ── The window's doors to the system, driven (T-NATIVE-HANDLE-BEHIND-THE-SEAM) ──────────────────
//
// One test per group of the doors that replaced the window's platform handle: the window's place
// on the desktop (read), its dress (told), the keyboard and the foreground, attention, the taskbar
// button, the system's news, and a page's place for the web engine. Each drives the runtime's own
// code over a headless window, whose questions answer what the test scripts
// (`headless::PlatformAnswers`) and whose instructions land on its ledger.

/// One headless window holding a single terminal tab, its ledger emptied.
fn one_window(tag: &str) -> HeadlessFolio {
    HeadlessFolio::with_tabs(tag, vec![tab_holding(leaf_saying("门 door"))], 0)
}

/// The window's questions answering `answers`.
fn answering(folio: &HeadlessFolio, answers: PlatformAnswers) {
    folio.window.window.answers.set(answers);
}

const RECT: bt_platform::WindowRect = bt_platform::WindowRect {
    left: 100,
    top: 50,
    right: 900,
    bottom: 650,
};

/// RED — **where the window is on the desktop is read through its own doors**: iconic and
/// cloaked each make the turn's reading hidden, a covered window reads unexposed, the snapshot
/// measures the rectangle the window door names, the work area is the display's as the door names
/// it, the glass under a point and the pointer are the door's answers.
///
/// MUTATION: make `window_is_hidden` read `false` instead of `window.is_iconic() ||
/// window.is_cloaked()` — the first two readings go red; make `window_snapshot` measure nothing
/// (`measured = None`) — the bounds go red; take the door out of `refresh_work_area` — the work
/// area stays unknown.
#[test]
fn the_windows_place_is_read_through_its_doors() {
    let mut folio = one_window("seam-door-place");
    answering(
        &folio,
        PlatformAnswers {
            iconic: true,
            ..PlatformAnswers::default()
        },
    );
    folio.runtime().observe_window_place();
    assert!(
        folio.window.observed_place.hidden,
        "an iconic window is hidden"
    );
    assert!(folio.runtime().window_is_iconic());

    answering(
        &folio,
        PlatformAnswers {
            cloaked: true,
            ..PlatformAnswers::default()
        },
    );
    folio.runtime().observe_window_place();
    assert!(
        folio.window.observed_place.hidden,
        "a cloaked window is hidden"
    );

    answering(
        &folio,
        PlatformAnswers {
            exposed: false,
            outer_rect: Some(RECT),
            work_area: Some(RECT),
            glass: crate::GlassHere::Theirs,
            pointer: Some((12, 34)),
            ..PlatformAnswers::default()
        },
    );
    folio.runtime().observe_window_place();
    let place = folio.window.observed_place;
    assert!(
        !place.hidden && !place.exposed,
        "a covered window is on the screen and unexposed: {place:?}"
    );
    let snapshot = folio.runtime().window_snapshot();
    assert_eq!(
        snapshot.placement.bounds,
        crate::persisted_window_bounds(RECT, 1.0),
        "the snapshot measures the rectangle the window door names"
    );
    folio.runtime().refresh_work_area();
    assert_eq!(
        folio.window.work_area,
        bt_layout::WorkAreaHint::Known(bt_layout::LogicalSize::px(800, 600)),
        "the work area is the display's, as the window door names it"
    );
    assert_eq!(
        folio.runtime().glass_here(PhysicalPosition::new(5.0, 6.0)),
        crate::GlassHere::Theirs
    );
    assert_eq!(
        folio.runtime().platform_pointer_now(),
        Some(PhysicalPosition::new(12.0, 34.0))
    );
}

/// RED — **the window's dress is told through its doors, with its arguments**: always-on-top,
/// the system backdrop, the frame's dark plate and the class background each land as the effect
/// the window would have had.
///
/// MUTATION: drop the `set_topmost` call from `apply_always_on_top` — `Topmost(true)` is missing;
/// make `install_theme_class_background` tell `None` — the background's colour goes red.
#[test]
fn the_windows_dress_is_told_through_its_doors() {
    let mut folio = one_window("seam-door-dress");
    folio.app.acrylic_available = true;
    folio.window.dwm_dark_mode = None;
    let mut runtime = folio.runtime();
    runtime
        .apply_always_on_top(true)
        .expect("the row is answered");
    runtime.apply_acrylic(true).expect("the row is answered");
    runtime
        .apply_window_dark_mode()
        .expect("the plate is answered");
    crate::install_theme_class_background(&folio.window.window);
    let dark = !bt_render::background_is_light(bt_render::background_rgb());
    let opaque = bt_render::window_ground().alpha >= 1.0;
    let effects = folio.ledger.take();
    let told: Vec<&WindowEffect> = effects
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                WindowEffect::Topmost(_)
                    | WindowEffect::Backdrop(_)
                    | WindowEffect::DarkMode(_)
                    | WindowEffect::ClassBackground(_)
            )
        })
        .collect();
    assert_eq!(
        told,
        [
            &WindowEffect::Topmost(true),
            &WindowEffect::Backdrop(true),
            &WindowEffect::DarkMode(dark),
            &WindowEffect::ClassBackground(opaque.then(crate::background_rgb)),
        ],
        "every instruction, in the order it was given: {effects:?}"
    );
}

/// RED — **a raised window is restored before it takes the foreground**, and the keyboard, the
/// close and a page's keyboard all go through the window's doors.
///
/// MUTATION: swap `restore_minimized_window(window)?` below the foreground in
/// `bring_owned_window_forward` — the order goes red; make `take_owned_keyboard_focus` answer `Ok`
/// without `window.take_foreground()` — `ForegroundTaken` is missing and the refusal is not said.
#[test]
fn a_raised_window_is_restored_before_it_takes_the_foreground() {
    let folio = one_window("seam-door-foreground");
    answering(
        &folio,
        PlatformAnswers {
            minimized: Some(true),
            ..PlatformAnswers::default()
        },
    );
    crate::bring_owned_window_forward(&folio.window.window).expect("the system lets it");
    assert_eq!(
        folio.ledger.take(),
        [
            WindowEffect::Minimized(false),
            WindowEffect::ForegroundTaken
        ],
        "restored, then given the foreground"
    );
}

/// RED — **the keyboard is taken through the foreground door**, and a refusal from the system is
/// said; the close is asked through the window's door; a page's find report takes the keyboard
/// back into the window while the capsule's field is focused.
///
/// MUTATION: make `take_owned_keyboard_focus` answer `Ok(())` — the refusal goes red; drop the
/// `take_keyboard_focus` call from `web_find_reported` — `KeyboardFocusTaken` is missing.
#[test]
fn the_keyboard_is_taken_through_the_foreground_door() {
    let mut folio = one_window("seam-door-keyboard");
    folio
        .runtime()
        .give_foreground_with_retry()
        .expect("the system lets it");
    assert_eq!(folio.ledger.take(), [WindowEffect::ForegroundTaken]);
    answering(
        &folio,
        PlatformAnswers {
            takes_the_foreground: false,
            ..PlatformAnswers::default()
        },
    );
    assert!(
        folio.runtime().give_foreground_with_retry().is_err(),
        "a window the system would not give the keyboard says so"
    );
    folio.runtime().request_window_close().expect("asked");
    assert_eq!(
        folio.ledger.take(),
        [WindowEffect::ForegroundTaken, WindowEffect::CloseRequested]
    );
    let seat = folio.window.tabs[0].seats.identity();
    folio
        .runtime()
        .open_search(seat)
        .expect("the capsule opens");
    folio.ledger.take();
    folio
        .runtime()
        .web_find_reported(1, 1)
        .expect("the engine's count is taken");
    assert!(
        folio
            .ledger
            .take()
            .contains(&WindowEffect::KeyboardFocusTaken),
        "the keyboard comes back into the window while the field is focused"
    );
}

/// RED — **a flash for attention goes to the window's own button**: a delivery whose reach is the
/// taskbar flashes it, once for the turn's deliveries.
///
/// MUTATION: drop `self.window.window.flash()` from `raise_attention` — `Flashed` is missing.
#[test]
fn a_flash_for_attention_goes_through_the_windows_door() {
    let mut folio = one_window("seam-door-attention");
    let tab = folio.window.tabs[0].id;
    let seat = folio.window.tabs[0].seats.identity();
    let delivery = |title: &str| crate::AttentionDelivery {
        tab,
        seat,
        reach: crate::attention::Reach::Flash,
        why: crate::attention::Why::Awaiting,
        title: title.to_owned(),
        body: None,
    };
    folio
        .runtime()
        .raise_attention(vec![delivery("等你 waiting"), delivery("second")])
        .expect("the deliveries are answered");
    let effects = folio.ledger.take();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| **effect == WindowEffect::Flashed)
            .count(),
        1,
        "one flash for the turn's deliveries: {effects:?}"
    );
}

/// RED — **the taskbar button is told each new progress through the window's door**, once, and
/// never the reading it already shows.
///
/// MUTATION: make `TaskbarMirror::show` build no button (`self.refused = true`) — nothing is told.
#[test]
fn the_taskbar_button_is_told_through_the_windows_door() {
    let folio = one_window("seam-door-taskbar");
    let mut mirror = crate::TaskbarMirror::default();
    let half = bt_platform::TaskbarProgress {
        state: bt_platform::TaskbarProgressState::Normal,
        value: Some((50, 100)),
    };
    for wanted in [bt_platform::TaskbarProgress::CLEARED, half, half] {
        mirror.show(&folio.window.window, wanted);
    }
    assert_eq!(folio.ledger.take(), [WindowEffect::TaskbarProgress(half)]);
}

/// RED — **a window is born listening to the system** through its own doors: the preference
/// broadcasts and the end-of-session question are each asked for once, as the window is built.
///
/// MUTATION: drop the `watch_session_end` call from `new_window_runtime` — `SessionEndWatched` is
/// missing.
#[test]
fn a_window_is_born_listening_to_the_system() {
    let folio = one_window("seam-door-news");
    let ledger = Ledger::default();
    let parts = HeadlessParts::new(&folio.app.gpu, 2, (800, 600), 1.0, &ledger);
    let window = headless_window(
        &folio.app,
        parts,
        vec![tab_holding(leaf_saying("新 new"))],
        0,
    );
    let effects = ledger.take();
    for asked in [
        WindowEffect::SystemSettingsWatched,
        WindowEffect::SessionEndWatched,
    ] {
        assert_eq!(
            effects.iter().filter(|effect| **effect == asked).count(),
            1,
            "{asked:?} once: {effects:?}"
        );
    }
    drop(window);
}

/// RED — **a page is opened on the place the window door names**: the page's own name in this
/// window is asked of the window, and a window that hosts no engine answers the open with its
/// refusal.
///
/// MUTATION: open the seat without asking the window for the page's place — `PageAddressed` is
/// missing.
#[test]
fn a_page_is_opened_on_the_place_the_window_door_names() {
    let mut folio = one_window("seam-door-page");
    let leaf = crate::LeafId {
        tab: folio.window.tabs[0].id,
        seat: folio.window.tabs[0].seats.identity(),
    };
    let refused = folio
        .runtime()
        .open_web_page_on(leaf, "https://example.com/页", crate::webnav::Mint::Nothing)
        .expect_err("a headless window hosts no web engine");
    assert!(
        refused.to_string().contains("no web engine"),
        "the window's own refusal: {refused:#}"
    );
    assert_eq!(
        folio.ledger.take(),
        [WindowEffect::PageAddressed(bt_platform::PageVisual {
            tab: leaf.tab.0,
            seat: leaf.seat.0,
        })]
    );
}
