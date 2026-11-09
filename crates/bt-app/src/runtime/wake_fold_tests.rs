//! **No clock in the wake fold is left in the past by the turn it comes due on**
//! (T-WINDOW-THREAD-UNDER-LOAD).
//!
//! The loop parks with `ControlFlow::WaitUntil(the fold's earliest instant)`, and an instant that
//! is already due when it parks is no wait at all: winit returns at once and the loop turns again,
//! thousands of times a second, until something else moves the clock. The owner's log of
//! 2026-10-18 held a deadline fixed in the past for minutes at a time while the window thread
//! turned some 13 500 times a second. Every case below drives one named entry of the fold to its
//! due instant and through the turn that sees it due, and then asks the fold that turn returned:
//! nothing in it may be at or before the turn's own instant, whichever clock it is — the earliest
//! instant is the one the loop parks until, so "the earliest is ahead" is "every entry is ahead".

use std::time::{Duration, Instant};

use crate::headless::{Headless, HeadlessFolio, WindowEffect};
use crate::test_support::{card_over, cross_tab, leaf_saying, tab_holding};
use crate::wake_owner::WakeOwner;
use crate::{Motion, PeekClock, RowHost, Runtime, TabId, foreground_program};

/// **The class's one assertion**: the wake a turn at `now` booked is ahead of `now`, or absent.
fn books_nothing_due(wake: Option<(WakeOwner, Instant)>, now: Instant) {
    if let Some((owner, at)) = wake {
        assert!(
            at > now,
            "the turn handed the loop {owner:?}'s instant {} us in the past — a wait that is no \
             wait, and a loop that spins on it",
            now.saturating_duration_since(at).as_micros()
        );
    }
}

/// A turn of the window at `now`, as the loop takes it, and the wake it booked.
fn turn(folio: &mut HeadlessFolio, now: Instant) -> Option<(WakeOwner, Instant)> {
    folio
        .runtime()
        .turn(now, true)
        .expect("the turn is answered")
}

/// RED — **a frame candidate's foreground-program cadence books no wake, and a turn asks the
/// worker nothing for a pane that publishes nothing.**
///
/// The cadence is a gate on the publish of a pane's own output: `dispatch_tab_decoration_tasks`
/// asks `Cadence::should_request` with the publish's instant. The fold booked its five-second
/// instant for every pane of every tab, and no turn spends it, so a frame candidate (an agent's
/// boxed TUI) in a background tab left that instant in the past and the loop spun on it until
/// the tab published — the entry the owner's blocks fit. Two halves, both held: the turn at the
/// due instant books nothing stale, and the pane's cadence is exactly where it was — no request
/// minted for a pane in a tab that is not on screen, as shipped (a request mints by moving the
/// cadence's next instant on; this fixture's pane has no shell, so nothing would leave the
/// window either way, and the cadence is the counter).
///
/// MUTATIONS: book `foreground_program::deadline` over every leaf in `Runtime::turn`'s fold — the
/// turn hands the loop `ForegroundProgram`; ask `foreground_program::request` for every pane on
/// every turn — the background pane's cadence moves on.
#[test]
fn a_background_frame_candidates_cadence_books_no_wake_and_asks_nothing() {
    let mut framed = tab_holding(leaf_saying("ab │ cd\r\nab │ cd\r\nab │ cd\r\n"));
    framed.id = TabId(2);
    let front = tab_holding(leaf_saying("plain\r\n"));
    let mut folio = HeadlessFolio::with_tabs("wake-fold-foreground", vec![front, framed], 1);
    // The pane's first size, told and quiet: the window's own turns, a generous interval apart,
    // until the resize its layout opened has closed — a pane decorates nothing before that, and
    // the capture that finds the frame candidate is a decoration pass.
    let mut now = Instant::now();
    folio.draw();
    for _ in 0..8 {
        if !folio.window.tabs[1]
            .leaves()
            .any(|(_, leaf)| leaf.session.resize_transaction_open())
        {
            break;
        }
        now += Duration::from_millis(250);
        turn(&mut folio, now);
        folio.draw();
    }
    // Two frames: the first capture finds the candidate, the next publish arms its cadence.
    folio.draw();
    folio.draw();
    assert!(
        folio.window.tabs[1]
            .leaves()
            .any(|(_, leaf)| leaf.session.foreground_program_probe_is_armed()),
        "the fixture's own precondition: the boxed rows are a frame candidate"
    );
    let due = folio.window.tabs[1]
        .leaves()
        .find_map(|(_, leaf)| foreground_program::deadline(leaf))
        .expect("the framed pane's first published frame arms its cadence");
    folio
        .runtime()
        .activate_tab(0, false)
        .expect("the plain tab comes forward");
    // The window's own clocks, without the application's: the turn runs five seconds ahead of
    // the real clock the heartbeat's allowance is taken on, and the web engine's warm-up — a
    // deferrable unit, asked only while that allowance lasts — would be judged against an
    // allowance from another instant.
    let wake = folio
        .runtime()
        .turn(due, false)
        .expect("the turn is answered");
    books_nothing_due(wake, due);
    let after = folio.window.tabs[1]
        .leaves()
        .find_map(|(_, leaf)| foreground_program::deadline(leaf));
    assert_eq!(
        after,
        Some(due),
        "a turn mints no request for a pane in a tab not on screen: its cadence stays put"
    );
}

/// RED — **a pane menu's matured rest is spent when the pointer has left the window.**
///
/// The rest on a heading matures in `advance_pane_menu`, which asked the pointer which row it is
/// on; with the pointer gone (`CursorLeft` clears it) the answer was nothing and the hold was left
/// standing, an instant in the past, until the next pointer move over the window.
///
/// MUTATION: drop the `menu.submenu_hold_until = None` in `advance_pane_menu` — the turn hands the
/// loop `PaneMenu`.
#[test]
fn a_pane_menus_matured_rest_is_spent_with_the_pointer_gone() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-pane-menu",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let seat = *folio.window.tabs[0]
        .sessions
        .keys()
        .next()
        .expect("the one pane");
    folio
        .runtime()
        .open_pane_menu(seat, [40.0, 40.0])
        .expect("the menu opens");
    let due = Instant::now() + Duration::from_millis(250);
    folio
        .window
        .pane_menu
        .as_mut()
        .expect("the menu is up")
        .submenu_hold_until = Some(due);
    assert!(folio.window.pointer_position.is_none());
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **a terminal menu's matured rest is spent when its child cannot open** — a class guard
/// over a synthetic state.
///
/// `set_term_submenu(true)` declines a menu whose pane is not alone on the stage (`lone` false),
/// and the hold that asked it was left standing. The product cannot reach that pairing today —
/// the `Split with` heading is drawn only when `lone` — so the state is set by hand: the guard
/// is that `advance_term_menu` spends a matured hold whatever its roads do next, as the pane
/// menu's and the tab menu's do on roads the product does reach.
///
/// MUTATION: drop the `menu.submenu_hold_until = None` in `advance_term_menu`.
#[test]
fn a_terminal_menus_matured_rest_is_spent_when_its_child_cannot_open() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-term-menu",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let seat = *folio.window.tabs[0]
        .sessions
        .keys()
        .next()
        .expect("the one pane");
    folio
        .runtime()
        .open_term_menu_at(seat, winit::dpi::PhysicalPosition::new(40.0, 40.0))
        .expect("the menu opens");
    let due = Instant::now() + Duration::from_millis(250);
    {
        let runtime = folio.runtime();
        let menu = runtime.window.term_menu.as_mut().expect("the menu is up");
        menu.lone = false;
        menu.submenu_hold_until = Some(due);
    }
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **a tab menu's matured rest is spent when there is no other window to list.**
///
/// `set_tab_submenu(true)` declines while no other window exists, and the hold that asked it was
/// left standing.
///
/// MUTATION: drop the `menu.submenu_hold_until = None` in `advance_tab_menu`.
#[test]
fn a_tab_menus_matured_rest_is_spent_with_no_other_window() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-tab-menu",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let tab = folio.window.tabs[0].id;
    folio
        .runtime()
        .open_tab_menu_at(tab, winit::dpi::PhysicalPosition::new(40.0, 10.0))
        .expect("the menu opens");
    let due = Instant::now() + Duration::from_millis(250);
    folio
        .window
        .tab_menu
        .as_mut()
        .expect("the menu is up")
        .submenu_hold_until = Some(due);
    assert!(folio.runtime().other_window_ids().is_empty());
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **a standing credential's ten minutes are booked no earlier than the turn that spends
/// them.**
///
/// `settle_attention` runs behind the strip's tick and the window's frame gate; a credential due
/// while the gate is shut was booked raw, an instant in the past for as long as the gate stayed
/// shut.
///
/// MUTATION: book `attention_ledger_deadline` raw in `Runtime::turn`'s fold — the turn hands the
/// loop `AttentionCredential`.
#[test]
fn a_due_credential_behind_the_strips_gate_is_booked_for_the_turn_the_gate_admits() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-credential",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let armed = Instant::now();
    let due = armed + bt_workbench::attention::expiry::WAIT_TTL;
    {
        let runtime = folio.runtime();
        let leaf = runtime.window.tabs[0]
            .sessions
            .values_mut()
            .next()
            .expect("the one pane");
        leaf.session.attention.clock.arm(
            &bt_workbench::attention::WaitSlot::Level(bt_workbench::attention::WaitKind::Agent),
            armed,
        );
        // The strip ticked a moment ago, so its gate is shut at `due`.
        runtime.window.strip_animation_ticked_at = Some(due - Duration::from_millis(1));
    }
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **the session's autosave is not booked while a quit holds the storage worker.**
///
/// Its spender does not run while the quit's documents and their copies are on the worker; its
/// instant, booked anyway, was one the loop spun on for the whole of that phase.
///
/// MUTATION: book `session_store.deadline()` under `application_clocks` alone in the fold — the
/// turn hands the loop `SessionSave`.
#[test]
fn the_autosave_is_not_booked_while_a_quit_holds_the_storage_worker() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-autosave",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    // A session that differs from the one the window records, so the window's own record on a
    // turn is a change; after that the document holds still and the store owes one write, due
    // one quiet window after the last change.
    let mut session = folio.app.session_store.loaded().clone();
    session.schema_version = session.schema_version.wrapping_add(1);
    folio.app.session_store.record(session, Instant::now());
    folio.app.quit = Some(crate::quit::Quit::begin(Vec::new()));
    assert!(
        folio
            .app
            .quit
            .as_ref()
            .is_some_and(crate::quit::Quit::holds_the_storage_worker)
    );
    let mut due = Instant::now();
    for _ in 0..4 {
        turn(&mut folio, due);
        let next = folio
            .app
            .session_store
            .deadline()
            .expect("a changed session owes a write");
        if next == due {
            break;
        }
        due = next;
    }
    assert_eq!(
        folio.app.session_store.deadline(),
        Some(due),
        "the fixture's own precondition: the document holds still and its write is due"
    );
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **a shown glance card with no buffer behind it is taken down on its turn.**
///
/// The card is a view of a buffer; with none the paint draws nothing, and a card still reported
/// shown owed a frame no frame could pay — booked at the last present's frame, an instant in the
/// past on every turn once nothing presented.
///
/// MUTATION: drop the bufferless-card arm from `advance_file_peek` — the second turn hands the
/// loop `FilePeek`.
#[test]
fn a_shown_glance_card_with_no_buffer_is_taken_down_on_its_turn() {
    let tab = cross_tab(1, &["一 one"]);
    let shell = tab.seats.terminals()[0];
    let mut folio = HeadlessFolio::with_tabs("wake-fold-file-peek", vec![tab], 0);
    folio.draw();
    let shown = Instant::now();
    folio.window.file_peek = Some(card_over(
        RowHost::Terminal(shell),
        std::path::Path::new("nowhere.txt"),
        PeekClock::Shown(shown),
    ));
    let first = shown + Duration::from_millis(200);
    books_nothing_due(turn(&mut folio, first), first);
    let second = first + Duration::from_millis(200);
    books_nothing_due(turn(&mut folio, second), second);
    assert!(folio.window.file_peek.is_none(), "the card is down");
}

/// RED — **the storage watch's re-read is not booked while a quit holds the storage worker.**
///
/// The autosave's twin: the watch's spender (`advance_storage_watch`) does not run while the
/// quit's documents and their copies are on the worker, and its quiet instant, booked anyway,
/// was one the loop spun on for the whole of that phase. The settings file changes, a turn hears
/// it and books the quiet window; then a quit takes the worker and the window comes due.
///
/// MUTATION: book `storage_watch.deadline()` under `application_clocks` alone in the fold — the
/// turn hands the loop `StorageWatch`.
#[test]
fn the_storage_watch_is_not_booked_while_a_quit_holds_the_storage_worker() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-storage-watch",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let listener = folio.app.storage_watch.listener_on(Instant::now);
    listener(
        std::path::Path::new("storage"),
        bt_platform::DirChange::Named(&[std::ffi::OsString::from(
            crate::persist::SETTINGS_FILE_NAME,
        )]),
    );
    let heard = Instant::now();
    books_nothing_due(turn(&mut folio, heard), heard);
    let due = folio
        .app
        .storage_watch
        .deadline()
        .expect("the turn heard the change and booked its quiet window");
    folio.app.quit = Some(crate::quit::Quit::begin(Vec::new()));
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **an idle turn inside the last present's frame asks the frame gate nothing**
/// (T-ADVANCERS-ASK-ONLY-WITH-WORK).
///
/// Row 12's idle census: after every caret frame the window turned once more for nothing. The
/// turn inside the frame the glass was showing ran seven advancers that asked the gate before
/// they had looked at their own work; each refusal booked `RefusedFrame` one interval on, and
/// that wake drew nothing — `pace_skipped=7` on every present, a third of an idle window's turns.
/// A window with nothing moving, turned inside its last present's frame, refuses nothing: no
/// debt, no refusal counted, no `RefusedFrame` handed to the loop. Every gated advancer runs in
/// the turn, so a new one that asks before it holds work turns this red as well.
///
/// MUTATION: ask `self.animation_frame_is_due()` at the head of `Runtime::advance_toasts` — the
/// turn refuses once and hands the loop `RefusedFrame`.
#[test]
fn an_idle_turn_inside_the_last_frame_asks_the_gate_nothing() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-idle-ask",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let presented = folio
        .window
        .last_present_at
        .expect("the fixture's draw reached the glass");
    // A running window's present is a turn's, and that turn ticked the strip on its way to it:
    // the strip's own rate is spent until a frame on, as it is after the caret's turn. The
    // fixture's first paint is an expose with no turn before it, so its strip has never ticked.
    folio.window.strip_animation_ticked_at = Some(presented);
    let inside = presented + Duration::from_millis(1);
    assert!(
        inside < presented + folio.window.frame_clock.interval(),
        "the fixture's own precondition: the turn is inside the frame the glass is showing"
    );
    folio.window.frame_clock.take_skipped();
    let wake = turn(&mut folio, inside);
    assert!(
        !folio.window.frame_clock.owes_a_frame(),
        "an idle turn owes no frame"
    );
    assert_eq!(
        folio.window.frame_clock.take_skipped(),
        0,
        "and the gate turned nobody away"
    );
    assert!(
        !matches!(wake, Some((WakeOwner::RefusedFrame, _))),
        "and the loop is handed no refused frame: {wake:?}"
    );
}

/// The seven advancers that have news of their own and ask the frame gate through
/// [`Runtime::paced_frame`], each with work planted for it at `at`.
#[derive(Clone, Copy, Debug)]
enum Advancer {
    MathTools,
    Tooltip,
    KeyHint,
    CardHint,
    Toasts,
    FilePeek,
    Float,
}

impl Advancer {
    const SEVEN: [Self; 7] = [
        Self::MathTools,
        Self::Tooltip,
        Self::KeyHint,
        Self::CardHint,
        Self::Toasts,
        Self::FilePeek,
        Self::Float,
    ];

    /// One turn's run of this advancer, as `Runtime::turn` runs it.
    fn run_alone(self, runtime: &mut Runtime<'_, Headless>, now: Instant) {
        match self {
            Self::MathTools => runtime.advance_math_tools_if_due(now),
            Self::Tooltip => runtime.advance_tooltip_if_due(now),
            Self::KeyHint => runtime.advance_key_hint_if_due(now),
            Self::CardHint => runtime.advance_card_hint(now),
            Self::Toasts => runtime.advance_toasts(now),
            Self::FilePeek => runtime.advance_file_peek(now),
            Self::Float => runtime.advance_float(now),
        }
        .expect("the advance is answered");
    }

    /// Put a journey in flight that owes this advancer frames from `at` on, with no news left
    /// for a turn after `at` to carry: the frame it owes is a debt, and a debt is what asks.
    fn plant_work(self, runtime: &mut Runtime<'_, Headless>, at: Instant) {
        match self {
            // The formula marks arriving, with no pointer in the window to move their ink.
            Self::MathTools => {
                runtime.window.math_tools = Some(
                    crate::formula_tools::FormulaToolFollow::arriving(&a_bands_marks(), None, at),
                );
            }
            // A tab's tip, promoted at `at` and fading in.
            Self::Tooltip => {
                let tip = (
                    crate::tooltip::TooltipAnchorId::Tab(0),
                    crate::tooltip::TipFace::Chrome,
                );
                let settled = at
                    .checked_sub(tip.1.intent_delay())
                    .expect("the process is older than a tip's intent");
                runtime.window.tooltip.observe(Some(tip), settled);
                assert!(
                    runtime.window.tooltip.activate_if_due(at),
                    "the fixture's own precondition: the tip comes up at its intent"
                );
            }
            // The command modifier held for its 800ms, its card promoted at `at` and fading in.
            Self::KeyHint => {
                let held = the_hosts_command_modifier();
                let pressed = at
                    .checked_sub(crate::keyhint::KEY_HINT_DELAY)
                    .expect("the process is older than a hold");
                runtime.window.key_hint.observe(held, true, pressed);
                assert!(
                    runtime.window.key_hint.activate_if_due(at),
                    "the fixture's own precondition: the card comes up at the end of the hold"
                );
            }
            // The Cards column's bubble raised at `at`, its nudge moving.
            Self::CardHint => {
                runtime.window.card_hint.observe(true, true, at);
            }
            // A notice raised at `at` and fading in, on a glass that has not drawn it.
            Self::Toasts => {
                runtime.window.toasts.raise(
                    crate::toast::ToastKind::Info,
                    crate::toast::ToastAnchor::Window,
                    None,
                    "动 arriving",
                    None,
                    Motion::Full,
                    at,
                );
            }
            // A glance card that matured at `at` (its own turn, which is news and goes through)
            // and whose first frame has not reached the glass: the fade owes it.
            Self::FilePeek => {
                runtime.window.file_peek = Some(card_over(
                    RowHost::Column(bt_layout::SeatId(1)),
                    std::path::Path::new("一瞥 glance.txt"),
                    PeekClock::Settling(at),
                ));
                runtime.window.frame_clock.open(None, at);
                Self::FilePeek.run_alone(runtime, at);
                assert!(
                    runtime
                        .window
                        .file_peek
                        .as_ref()
                        .is_some_and(|peek| peek.clock.is_shown()),
                    "the fixture's own precondition: the card is up"
                );
                runtime.window.file_peek_drawn_opacity = None;
            }
            // A pinned float opened at `at`, its entrance under way; the turn at `at` takes the
            // news its opening makes (its first size), so a later turn has none.
            Self::Float => {
                runtime.window.float.open(
                    crate::float::FloatMode::Pinned,
                    None,
                    a_files_tenant(),
                    [100.0, 100.0, 364.0, 400.0],
                    None,
                    at,
                );
                runtime.window.frame_clock.open(None, at);
                Self::Float.run_alone(runtime, at);
            }
        }
    }
}

/// A band's three boxes, as the renderer hands them for a block on screen.
fn a_bands_marks() -> bt_render::MathToolBoxes {
    bt_render::MathToolBoxes {
        anchor: bt_viewport::MathBlockAnchor::History {
            run: None,
            start: bt_transcript::TranscriptId(1),
            end: bt_transcript::TranscriptId(1),
        },
        display: bt_viewport::MathBlockDisplay::Rendered,
        block: [40.0, 10.0, 300.0, 59.0],
        source: [230.0, 25.0, 249.0, 44.0],
        copy: [251.0, 25.0, 270.0, 44.0],
    }
}

/// A files float's tenant over a folder that is never read.
fn a_files_tenant() -> crate::float::FloatTenant {
    crate::float::FloatTenant::Files(Box::new(crate::float::FloatFiles {
        files: crate::seats::FilesLeafState {
            root: "C:/x".to_owned(),
            ..crate::seats::FilesLeafState::default()
        },
        width: bt_layout::LogicalPx::px(240),
        ..crate::float::FloatFiles::default()
    }))
}

/// This host's command modifier: Control, or Command on a Mac — `input::is_command_chord`'s
/// answer, not a second decision.
fn the_hosts_command_modifier() -> winit::keyboard::ModifiersState {
    use winit::keyboard::ModifiersState;
    [ModifiersState::CONTROL, ModifiersState::SUPER]
        .into_iter()
        .find(|modifiers| crate::input::is_command_chord(*modifiers))
        .expect("one of the two is the host's command modifier")
}

/// How many frames the window was asked for since the last take of its ledger.
fn frames_asked_since(folio: &HeadlessFolio) -> usize {
    folio
        .ledger
        .take()
        .into_iter()
        .filter(|effect| *effect == WindowEffect::Redraw)
        .count()
}

/// RED — **an advancer with work still gets its frame** (T-ADVANCERS-ASK-ONLY-WITH-WORK).
///
/// Asking only with work must not become asking never. Each of the seven, with a journey in
/// flight that owes it frames, run alone on a turn the frame gate refuses: it asks, the refusal
/// is counted once, the frame clock owes the frame — the debt the wake fold books as
/// `RefusedFrame` at the next frame boundary, which is how the refused frame comes back — and no
/// frame is asked for. On the turn the gate admits, it is not refused again, and it asks the
/// window for its frame.
///
/// MUTATIONS: answer `news` alone from `Runtime::paced_frame` (a debt never asks) — every row's
/// refused turn owes nothing; let a debt ask the gate and then never draw — every row's admitted
/// turn asks the window for no frame.
#[test]
fn an_advancer_with_work_on_a_refused_turn_still_owes_its_frame() {
    for advancer in Advancer::SEVEN {
        let mut folio = HeadlessFolio::with_tabs(
            "wake-fold-advancer-work",
            vec![tab_holding(leaf_saying("工作 work\r\n"))],
            0,
        );
        folio.app.motion = Motion::Full;
        folio.draw();
        let at = folio
            .window
            .last_present_at
            .expect("the fixture's draw reached the glass");
        let interval = folio.window.frame_clock.interval();
        advancer.plant_work(&mut folio.runtime(), at);
        // The planting's own frames are the fixture's, not the turns' below.
        folio.ledger.take();

        // A turn inside the frame the glass is showing: the gate refuses, and the advancer asks.
        let refused = at + Duration::from_millis(1);
        let mut runtime = folio.runtime();
        runtime.window.frame_clock.open(Some(at), refused);
        assert!(
            !runtime.window.frame_clock.admits(),
            "{advancer:?}: the fixture's own precondition: the gate refuses this turn"
        );
        runtime.window.frame_clock.take_skipped();
        advancer.run_alone(&mut runtime, refused);
        assert!(
            runtime.window.frame_clock.owes_a_frame(),
            "{advancer:?}: an advancer with work on a refused turn owes its frame"
        );
        assert_eq!(
            runtime.window.frame_clock.take_skipped(),
            1,
            "{advancer:?}: and asked the gate once"
        );
        assert_eq!(
            frames_asked_since(&folio),
            0,
            "{advancer:?}: and the refused turn asks the window for no frame"
        );

        // The frame boundary the debt books: the gate admits, and nothing is refused again.
        let admitted = at + interval;
        let mut runtime = folio.runtime();
        runtime.window.frame_clock.open(Some(at), admitted);
        assert!(
            runtime.window.frame_clock.admits(),
            "{advancer:?}: the fixture's own precondition: the gate admits the booked turn"
        );
        advancer.run_alone(&mut runtime, admitted);
        assert!(
            !runtime.window.frame_clock.owes_a_frame(),
            "{advancer:?}: the admitted turn is not refused again"
        );
        assert!(
            frames_asked_since(&folio) >= 1,
            "{advancer:?}: and the admitted turn asks the window for the frame it owed"
        );
    }
}

/// RED — **the formula copy tick running out is news, and is not paced** (T-ADVANCERS-ASK-ONLY-
/// WITH-WORK, ruling R2).
///
/// Behind the gate, a turn inside the frame the glass is showing either asked on every turn a
/// band had marks or spent its news where it could not draw it. Above it, the spent tick is
/// retired on the turn that sees it run out, refused or not, and that turn asks the gate nothing.
///
/// MUTATION: move `retire_spent_math_copy` in `Runtime::advance_math_tools_if_due` back below
/// the frame gate — the refused turn leaves the tick standing.
#[test]
fn a_spent_copy_tick_is_retired_on_a_refused_turn() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-copy-tick",
        vec![tab_holding(leaf_saying("复制 copy\r\n"))],
        0,
    );
    folio.draw();
    let at = folio
        .window
        .last_present_at
        .expect("the fixture's draw reached the glass");
    let said = at
        .checked_sub(crate::FOOT_REVEAL_FEEDBACK)
        .expect("the process is older than a copy's acknowledgement");
    let mut runtime = folio.runtime();
    runtime.window.math_copied = Some((a_bands_marks().anchor, said));
    let refused = at + Duration::from_millis(1);
    runtime.window.frame_clock.open(Some(at), refused);
    runtime.window.frame_clock.take_skipped();
    runtime
        .advance_math_tools_if_due(refused)
        .expect("the advance is answered");
    assert!(
        runtime.window.math_copied.is_none(),
        "the spent tick is retired on the turn that sees it run out"
    );
    assert_eq!(
        runtime.window.frame_clock.take_skipped(),
        0,
        "and news asks the gate nothing"
    );
}

/// RED — **the formula hover moving is news, and is not paced** (T-ADVANCERS-ASK-ONLY-WITH-WORK,
/// ruling R2).
///
/// Behind the gate, the hit test that notices the pointer has left a mark ran only on an
/// admitted turn; observed and then held back, the observation would be spent on a turn that
/// cannot draw it, and the admitted turn after it would find nothing to say. Above it, a band
/// whose marks have landed and whose ink still names a mark the pointer has left has the ink put
/// out on the turn that sees it, refused or not: no gate asked, and the window asked for the
/// frame.
///
/// MUTATION: observe the hover in `Runtime::advance_math_tools_if_due` only on a turn the gate
/// admits — the refused turn leaves the ink on the mark.
#[test]
fn a_stale_formula_hover_is_put_out_on_a_refused_turn() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-formula-hover",
        vec![tab_holding(leaf_saying("悬停 hover\r\n"))],
        0,
    );
    folio.draw();
    let at = folio
        .window
        .last_present_at
        .expect("the fixture's draw reached the glass");
    let landed = at
        .checked_sub(Duration::from_secs(1))
        .expect("the process is older than a band's arrival");
    let refused = at + Duration::from_millis(1);
    {
        let runtime = folio.runtime();
        let mut marks =
            crate::formula_tools::FormulaToolFollow::arriving(&a_bands_marks(), None, landed);
        marks.observe_hovered(Some(crate::formula_tools::FormulaTool::CopyLatex));
        runtime.window.math_tools = Some(marks);
        assert!(
            !runtime.math_tools_owe_frames(refused),
            "the fixture's own precondition: the marks have landed and owe no frame"
        );
        runtime.window.frame_clock.open(Some(at), refused);
        runtime.window.frame_clock.take_skipped();
    }
    folio.ledger.take();
    let mut runtime = folio.runtime();
    runtime
        .advance_math_tools_if_due(refused)
        .expect("the advance is answered");
    assert_eq!(
        runtime
            .window
            .math_tools
            .as_ref()
            .and_then(|marks| marks.hovered()),
        None,
        "the ink leaves the mark the pointer is not on, on the turn that sees it"
    );
    assert_eq!(
        runtime.window.frame_clock.take_skipped(),
        0,
        "and news asks the gate nothing"
    );
    assert!(
        frames_asked_since(&folio) >= 1,
        "and the window is asked for the frame that shows it"
    );
}
