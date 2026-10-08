//! **The crate root: keyboard and IME.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    RESTORE_CARD_RUNG, calls_of, chevron_button, hand_leaves, method_body, paste_leaf, paste_tab,
    peek_open, reader_names, source, squeezed_body, the_three_chevrons,
};
use std::time::Duration;
use winit::keyboard::{Key, NamedKey};

/// PIN (user report, 2026-08-16) — **an open cursor range is a target clause,
/// not a caret.** winit reports `(start, end)` for the run Microsoft Pinyin
/// has marked as the clause under conversion; the first cut read `start`
/// as the caret and drew `ra|n`. A collapsed range is the caret and is
/// kept; an open one yields `None`, which the composer draws at the end
/// of the composition.
#[test]
fn an_open_cursor_range_is_a_clause_and_the_caret_goes_to_the_end() {
    assert_eq!(super::preedit_caret_byte(Some((3, 3))), Some(3));
    assert_eq!(super::preedit_caret_byte(Some((0, 0))), Some(0));
    assert_eq!(
        super::preedit_caret_byte(Some((2, 3))),
        None,
        "the clause `n` of `ran`"
    );
    assert_eq!(
        super::preedit_caret_byte(Some((0, 3))),
        None,
        "the whole word as clause"
    );
    assert_eq!(super::preedit_caret_byte(None), None);
}

/// PIN (T2 D31): the breath is the mock-up's keyframes on the mock-up's
/// curve — full at the ends, `.28` at the middle, and eased between.
///
/// The shape matters as much as the endpoints: a linear ramp between the
/// same two values is a flicker, and `ease-in-out` is what makes it read as
/// breathing. So the curve is checked for its defining property — that it
/// travels *slowly at the turns and quickly in between* — rather than only
/// at the keyframes, where a linear ramp would agree exactly.
#[test]
fn the_working_breath_runs_the_mock_ups_keyframes_on_its_own_curve() {
    let period = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS);
    let at = |fraction: f32| breathe_opacity(period.mul_f32(fraction), Motion::Full);

    assert!((at(0.0) - 1.0).abs() < 1e-3, "the breath starts full");
    assert!(
        (at(0.5) - WINDOW_TAB_BREATHE_MIN_OPACITY).abs() < 1e-3,
        "the trough is the keyframe's own .28"
    );
    assert!((at(1.0) - 1.0).abs() < 1e-3, "and it returns to full");
    // Cyclic: the second breath is the first one.
    assert!((at(0.25) - at(1.25)).abs() < 1e-3);

    // Never outside the keyframes it interpolates.
    for step in 0..=100 {
        let value = at(step as f32 / 100.0);
        assert!(
            (WINDOW_TAB_BREATHE_MIN_OPACITY..=1.0).contains(&value),
            "the breath left its keyframes at {step}%: {value}"
        );
    }

    // `ease-in-out` is flat at both ends of each half and steepest in the
    // middle of it. Over the first half-breath, the middle fifth must cover
    // more ground than the opening fifth — which is exactly what a linear
    // ramp (equal everywhere) fails.
    let opening = at(0.0) - at(0.1);
    let middle = at(0.2) - at(0.3);
    assert!(
        middle > opening * 2.0,
        "the breath must ease: opening {opening}, middle {middle}"
    );
}

#[test]
fn cursor_blink_resets_flips_and_stays_visible_while_unfocused() {
    let start = Instant::now();
    let mut blink = CursorBlink::new(start, Motion::Full);
    assert!(blink.visible());
    assert_eq!(blink.deadline(), Some(start + CURSOR_BLINK_PHASE));

    assert!(blink.advance(start + CURSOR_BLINK_PHASE));
    assert!(!blink.visible(), "the first phase boundary hides the caret");
    let input_at = start + CURSOR_BLINK_PHASE + Duration::from_millis(10);
    assert!(
        blink.reset(input_at, Motion::Full),
        "input reveals a hidden caret"
    );
    assert!(blink.visible());
    assert_eq!(blink.deadline(), Some(input_at + CURSOR_BLINK_PHASE));

    let unfocused_at = input_at + Duration::from_millis(20);
    blink.set_focused(false, unfocused_at, Motion::Full);
    assert!(blink.visible(), "the unfocused outline is always visible");
    assert_eq!(
        blink.deadline(),
        None,
        "unfocused cursors do not wake the loop"
    );
    assert!(!blink.advance(unfocused_at + Duration::from_secs(60)));
    assert!(blink.visible());

    let refocused_at = unfocused_at + Duration::from_secs(61);
    blink.set_focused(true, refocused_at, Motion::Full);
    assert!(blink.visible());
    assert_eq!(blink.deadline(), Some(refocused_at + CURSOR_BLINK_PHASE));
}

#[test]
fn cursor_blink_deadline_is_registered_with_the_event_loop_wake_set() {
    let start = Instant::now();
    let blink = CursorBlink::new(start, Motion::Full);
    let later = start + Duration::from_secs(10);
    assert_eq!(
        earliest_deadline([Some(later), blink.deadline(), None]),
        blink.deadline()
    );
}

#[test]
fn ime_commit_is_the_exact_utf8_pty_payload() {
    assert_eq!(
        ime_commit_bytes("你好世界"),
        vec![
            0xe4, 0xbd, 0xa0, 0xe5, 0xa5, 0xbd, 0xe4, 0xb8, 0x96, 0xe7, 0x95, 0x8c,
        ]
    );
}

#[test]
fn committed_utf8_projects_as_alacritty_wide_lead_and_spacer_cells() {
    let mut session = DualPlaneSession::with_quotas_and_cell_height(
        NonZeroU32::new(8).unwrap(),
        NonZeroU32::new(2).unwrap(),
        DEFAULT_STAGING_QUOTA,
        DEFAULT_FROZEN_LINE_QUOTA,
        std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
    );
    session.feed(&ime_commit_bytes("A你B")).unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();

    assert_eq!(frame.cells[0].text, "A");
    assert_eq!(frame.cells[1].text, "你");
    assert!(
        frame.cells[1]
            .style
            .flags
            .contains(bt_transcript::CellFlags::WIDE_CHAR)
    );
    assert!(frame.cells[2].wide_spacer);
    assert_eq!(frame.cells[3].text, "B");
}

#[test]
fn ime_cursor_area_throttle_coalesces_to_sixty_hz_and_flushes_the_last_area() {
    let start = Instant::now();
    let first = ImeCursorArea {
        x: 10,
        y: 20,
        width: 9,
        height: 22,
    };
    let latest = ImeCursorArea { x: 30, ..first };
    let mut throttle = ImeCursorThrottle::default();

    assert_eq!(throttle.offer(first, start), Some(first));
    assert_eq!(
        throttle.offer(latest, start + Duration::from_millis(3)),
        None
    );
    assert_eq!(throttle.flush_due(start + Duration::from_millis(15)), None);
    assert_eq!(
        throttle.flush_due(start + IME_CURSOR_AREA_INTERVAL),
        Some(latest)
    );
    assert_eq!(throttle.deadline(), None);
}

/// AppKit's own conversion, written down: a caret rectangle in **window**
/// points — top-left origin, y down, because winit's view is flipped — to
/// the **screen** rectangle `firstRectForCharacterRange:` has to answer
/// with, whose origin is the *bottom* left and whose y grows upwards from
/// the zero screen's bottom-left corner.
///
/// `window_origin` is the window content's bottom-left in that same global
/// space, which is what `-[NSWindow convertRectToScreen:]` adds. No screen's
/// height appears in it, and that is the point of the test below.
fn caret_screen_origin(
    window_origin: (f64, f64),
    content_height_pt: f64,
    scale: f64,
    area: ImeCursorArea,
) -> (f64, f64) {
    let left_pt = f64::from(area.x) / scale;
    let top_pt = f64::from(area.y) / scale;
    let height_pt = f64::from(area.height) / scale;
    (
        window_origin.0 + left_pt,
        window_origin.1 + (content_height_pt - (top_pt + height_pt)),
    )
}

/// The same rectangle read the way a screenshot reads it: down from the
/// **zero** screen's top-left. The flip constant is `NSScreen.screens[0]`'s
/// height and never the window's own screen's — the same constant
/// `macos_impl::flip_height` is built on.
fn screenshot_top(zero_screen_height_pt: f64, screen_origin_y: f64, height_pt: f64) -> f64 {
    zero_screen_height_pt - (screen_origin_y + height_pt)
}

/// **Why a window that moved has to be told, even though nothing it computes
/// changed** (user report 2026-09-14, `docs/DESIGN.md` §13.16 ⑥).
///
/// One caret, one window-relative rectangle, two displays: the zero screen,
/// and a second one whose origin is not `(0, 0)` and whose height is not the
/// zero screen's. The window-relative answer is a single number in both
/// places — that is the contract [`Runtime::apply_ime_cursor_area`] keeps —
/// and the **screen** rectangle the input method has to be given differs by
/// the whole of the move. So an answer cached while the window stood on one
/// display is wrong by that difference on the other, and re-deriving it is
/// not something this program can do by arithmetic: it can only ask the
/// platform to ask again.
///
/// Red gate: flip with the window's own screen height instead of the zero
/// screen's and the last assertion moves by the difference between them —
/// the candidate list stranded mid-window, which is the report.
#[test]
fn the_same_caret_is_two_screen_rectangles_on_two_displays() {
    let area = ImeCursorArea {
        x: 976 + 71,
        y: 40 + 646,
        width: 9,
        height: 22,
    };
    let scale = 2.0;
    let content_height_pt = 600.0;

    // Zero screen: 1512x982 points, origin (0, 0) by definition.
    let zero_screen_height = 982.0;
    let on_the_zero_screen = caret_screen_origin((100.0, 200.0), content_height_pt, scale, area);
    assert_eq!(on_the_zero_screen, (100.0 + 523.5, 200.0 + (600.0 - 354.0)));

    // A second display of a different size, parked to the right and hanging
    // below the zero screen's bottom edge: 2560x1440 points with its origin
    // at (1512, -458). The window is carried to it unchanged.
    let on_the_second_screen = caret_screen_origin(
        (1512.0 + 100.0, -458.0 + 200.0),
        content_height_pt,
        scale,
        area,
    );
    assert_eq!(
        (
            on_the_second_screen.0 - on_the_zero_screen.0,
            on_the_second_screen.1 - on_the_zero_screen.1,
        ),
        (1512.0, -458.0),
        "the caret moved by exactly the window's move and by nothing else",
    );

    // And read back the way the screen reads it, the flip is the **zero**
    // screen's height at both stops — the second display's 1440 never enters
    // the arithmetic, however tall it is.
    assert_eq!(
        screenshot_top(zero_screen_height, on_the_second_screen.1, 11.0),
        zero_screen_height + 458.0 - 200.0 - 246.0 - 11.0,
    );
}

/// **A move re-arms the rectangle the input method cached, and keeps the
/// clock** (user report 2026-09-14).
///
/// The suppression in [`ImeCursorThrottle::offer`] is what makes a still
/// caret free, and it is exactly what has to be lifted when the window
/// itself moves: the area is equal, the screen rectangle it converts to is
/// not. [`ImeCursorThrottle::rearm`] forgets the area alone, so a drag —
/// which emits a move per frame — still costs at most one call per 60Hz
/// slot rather than one per move.
///
/// Red gate: use `reset` instead and the last assertion fails, because a
/// dropped clock lets every move of a drag through.
#[test]
fn a_window_move_re_arms_the_caret_rectangle_without_dropping_the_clock() {
    let start = Instant::now();
    let area = ImeCursorArea {
        x: 1047,
        y: 686,
        width: 9,
        height: 22,
    };
    let mut throttle = ImeCursorThrottle::default();

    assert_eq!(throttle.offer(area, start), Some(area));
    assert_eq!(throttle.last_sent(), Some(area));
    assert_eq!(
        throttle.offer(area, start + IME_CURSOR_AREA_INTERVAL),
        None,
        "a caret that has not moved is not worth a call",
    );

    // The window is dragged to the second display. Same rectangle, and it
    // has to reach the platform anyway.
    throttle.rearm();
    assert_eq!(
        throttle.offer(area, start + IME_CURSOR_AREA_INTERVAL),
        Some(area),
    );

    // The rest of the drag is one move per frame, and the clock is still
    // standing: they coalesce into the one flush the interval allows.
    let moved_again = start + IME_CURSOR_AREA_INTERVAL + Duration::from_millis(3);
    throttle.rearm();
    assert_eq!(throttle.offer(area, moved_again), None);
    throttle.rearm();
    assert_eq!(throttle.offer(area, moved_again), None);
    assert_eq!(
        throttle.flush_due(start + IME_CURSOR_AREA_INTERVAL * 2),
        Some(area),
    );
}

// ── the input method's caret area, once a turn at most (0.4.5 ticket 63) ──
//
// `Runtime` cannot be built without a window, so the policy is run on the real
// `ImeCursorSlot` the window keeps — the same `want` every offer calls and the
// same `take_due` `Runtime::flush_ime_cursor_area` spends — and the facts about
// the runtime a slot cannot show (who calls the platform, where the turn and
// `Ime::Enabled` flush) are read through `bt_source`.

/// A caret line box on row `row` of a pane whose cells are 9x22 pixels.
fn caret_on_row(row: i32) -> ImeCursorArea {
    ImeCursorArea {
        x: 976 + 71,
        y: 40 + 22 * row,
        width: 9,
        height: 22,
    }
}

/// RED (63) — **Ten caret moves in one turn tell the system the cursor area
/// once.**
///
/// `Window::set_ime_cursor_area` is answered by the input method on the window
/// thread, and the owner's stall reports caught one call holding it for 85 ms
/// and one for 3,138 ms. Until ticket 63 every offer could call it at once — an
/// `Enabled` and a `Preedit` in one turn paid twice. Now an offer only wants: ten
/// moves in one turn leave the tenth area wanted, the turn's flush tells the
/// system that one, and nothing is left held. And the platform is reached from
/// one function only, which the turn calls once, after the last offer of the turn.
///
/// MUTATION: call `self.apply_ime_cursor_area(area, "sent")` in
/// `Runtime::offer_ime_caret` at every move — red (the offer becomes a caller of
/// the platform call).
#[test]
fn ten_caret_moves_in_one_turn_tell_the_system_the_cursor_area_once() {
    let start = Instant::now();
    let mut slot = ImeCursorSlot::default();
    for row in 0..10 {
        slot.want(caret_on_row(row));
    }
    let mut told = Vec::new();
    told.extend(slot.take_due(start));
    assert_eq!(told, vec![caret_on_row(9)], "one call, with the tenth area");
    assert_eq!(slot.deadline(start), None, "and nothing is held");

    let callers = calls_of("Runtime", "apply_ime_cursor_area").in_the_product(source());
    assert_eq!(
        reader_names(&callers),
        vec!["flush_ime_cursor_area".to_owned()],
        "{}",
        callers.report(source())
    );
    assert_eq!(callers.len(), 1, "{}", callers.report(source()));
    let flushes = calls_of("Runtime", "flush_ime_cursor_area").in_the_product(source());
    assert_eq!(
        reader_names(&flushes),
        vec!["ime_input".to_owned(), "turn".to_owned()],
        "{}",
        flushes.report(source())
    );
    assert_eq!(flushes.len(), 2, "{}", flushes.report(source()));
    let offer = method_body("Runtime", "offer_ime_caret");
    assert!(
        offer.contains("self.window.ime_cursor.want(area)"),
        "an offer says which area it wants"
    );
    let turn = method_body("Runtime", "turn");
    let flushed = turn
        .find("self.flush_ime_cursor_area(now)")
        .expect("the turn tells the system");
    for before in [
        "self.drain_pty()",
        "self.offer_ime_caret(None)",
        "self.flush_title(now)",
    ] {
        let at = turn.find(before).expect(before);
        assert!(at < flushed, "`{before}` runs before the turn's flush");
    }
}

/// RED (63) — **An unchanged area is not told again; a changed one is, on the
/// next flush.**
///
/// The field rungs offer their caret on every turn and the grid on every frame,
/// so a caret standing still is wanted over and over. Twenty turns a whole
/// interval apart, each wanting the area already told: no call. Then the caret
/// moves: the next flush tells it. A second move inside the same 60Hz slot is
/// held, booked in the wake fold, and told when its deadline comes.
///
/// MUTATION: drop the equality check in `pace::LatestThrottle::offer` — the
/// standing caret is told on every turn.
#[test]
fn an_unchanged_ime_cursor_area_is_not_told_again_and_a_changed_one_is_on_the_next_flush() {
    let start = Instant::now();
    let mut slot = ImeCursorSlot::default();
    slot.want(caret_on_row(3));
    assert_eq!(slot.take_due(start), Some(caret_on_row(3)));
    let mut calls = 0;
    for turn in 1..=20_u32 {
        slot.want(caret_on_row(3));
        calls += usize::from(
            slot.take_due(start + IME_CURSOR_AREA_INTERVAL * turn)
                .is_some(),
        );
    }
    assert_eq!(calls, 0, "a caret standing still costs no call");

    let moved_at = start + IME_CURSOR_AREA_INTERVAL * 21;
    slot.want(caret_on_row(4));
    assert_eq!(slot.take_due(moved_at), Some(caret_on_row(4)));
    assert_eq!(slot.last_told(), Some(caret_on_row(4)));

    let again = moved_at + Duration::from_millis(3);
    slot.want(caret_on_row(5));
    assert_eq!(slot.take_due(again), None, "held for its slot");
    let due = slot.deadline(again).expect("the held area books a wake-up");
    assert_eq!(due, moved_at + IME_CURSOR_AREA_INTERVAL);
    assert_eq!(slot.take_due(due), Some(caret_on_row(5)));
    assert_eq!(slot.deadline(due), None);
}

/// RED (63) — **Enabling the IME tells the area before the first preedit.**
///
/// winit sends `Ime::Enabled` from inside `WM_IME_STARTCOMPOSITION`, before the
/// first `WM_IME_COMPOSITION`; the candidate list opens at whatever area the
/// input method holds then. With every other offer waiting for the turn's tail,
/// the `Enabled` arm is the one place that tells the system at once: after the
/// slot is reset (a composition starting owes the system its area, whatever it
/// was told before) and after both the grid's frame and a field have said which
/// area they want. The slot half: a reset slot tells its first area at once, even
/// a millisecond after the last call.
///
/// MUTATION: drop `self.flush_ime_cursor_area(..)` from the `Ime::Enabled` arm
/// of `Runtime::ime_input` (the first area waits for the turn) — red.
#[test]
fn enabling_the_ime_tells_the_area_before_the_first_preedit() {
    let input = method_body("Runtime", "ime_input");
    let arm_at = input
        .find("Ime::Enabled => {")
        .expect("ime_input answers Enabled");
    let arm_end = input[arm_at..]
        .find("Ime::Preedit(text, cursor_range) => {")
        .map(|end| arm_at + end)
        .expect("the Preedit arm follows");
    let arm = &input[arm_at..arm_end];
    let mut previous = 0;
    for step in [
        "self.window.ime_cursor.reset()",
        "self.publish_frame(",
        "self.offer_ime_caret(None)",
        "self.flush_ime_cursor_area(",
    ] {
        let at = arm
            .find(step)
            .unwrap_or_else(|| panic!("the Enabled arm runs `{step}`"));
        assert!(
            at >= previous,
            "`{step}` is out of order in the Enabled arm"
        );
        previous = at;
    }

    let start = Instant::now();
    let mut slot = ImeCursorSlot::default();
    slot.want(caret_on_row(3));
    assert_eq!(slot.take_due(start), Some(caret_on_row(3)));
    slot.reset();
    let enabled = start + Duration::from_millis(1);
    slot.want(caret_on_row(3));
    assert_eq!(
        slot.take_due(enabled),
        Some(caret_on_row(3)),
        "a reset slot tells its first area at once, even the same area"
    );
}

/// **A window that moved wants the area it last told again, and a newer wanted
/// area wins** (ticket 63 keeps the 2026-09-14 re-arm on the one road).
#[test]
fn a_moved_window_wants_its_last_told_area_again_unless_a_newer_one_is_wanted() {
    let start = Instant::now();
    let mut slot = ImeCursorSlot::default();
    assert_eq!(slot.rearm(), None, "nothing told, nothing to say again");
    slot.want(caret_on_row(3));
    assert_eq!(slot.take_due(start), Some(caret_on_row(3)));
    assert_eq!(slot.rearm(), Some(caret_on_row(3)));
    let later = start + IME_CURSOR_AREA_INTERVAL;
    assert_eq!(slot.take_due(later), Some(caret_on_row(3)));
    slot.want(caret_on_row(4));
    assert_eq!(slot.rearm(), Some(caret_on_row(4)));
    assert_eq!(
        slot.take_due(later + IME_CURSOR_AREA_INTERVAL),
        Some(caret_on_row(4))
    );
}

/// **Focus follows the visible view** (焦点跟随可见视图, 2026-08-19) — the
/// whole of the reported bug, in the one judgement that had been missing.
///
/// A press on a docked Git page gave the *column* the keyboard, and the
/// router under it asked only whether the seat was a files column before
/// handing every key to the tree. So `↑`/`↓` walked a list nobody could see
/// and `Enter` opened a preview of a file the reader had never chosen:
/// nothing reached a shell, which is why it was silent, and state changed
/// under the user, which is why it was a bug.
///
/// **Drawn, not merely turned to.** The argument is the frame's own record
/// of which columns put a Git page on the glass. A column whose `view` is
/// `Git` while the master switch is off — or one that is collapsed, or not
/// laid out — is showing its tree, and its keys are the tree's; that column
/// is simply not in the map, which is why this needs no second condition.
///
/// Red gate: answer `GitPage` from `FilesLeafState::view` instead and a
/// column with the panel switched off answers its arrows with a page that is
/// not on screen — the same class of bug, pointing the other way.
#[test]
fn a_columns_keys_belong_to_the_page_that_is_on_the_glass() {
    let (page, tree) = (bt_layout::SeatId(1), bt_layout::SeatId(2));
    let mut drawn = BTreeMap::new();
    drawn.insert(page, git_panel::GitPanelContent::default());

    assert_eq!(column_keyboard(page, &drawn), ColumnKeyboard::GitPage);
    assert_eq!(
        column_keyboard(tree, &drawn),
        ColumnKeyboard::Tree,
        "a column beside it that is on its tree is still the tree's"
    );
    assert_eq!(
        column_keyboard(page, &BTreeMap::new()),
        ColumnKeyboard::Tree,
        "and so is the same column on a frame that drew no Git page at all"
    );
}

/// **One composition, one write** (M1-8; `docs/DESIGN.md` §13.16 ③).
///
/// X-3 measured `你好` reaching the child exactly once on macOS 26.6 with
/// winit 0.30.13 — the duplicate-commit hazard that platform is known for
/// did not reproduce — and named the detector to keep: **a duplicate is an
/// `Ime::Commit(t)` followed by a `KeyboardInput` whose text is `t`**, with
/// the payload twice on the wire. This is that detector as a test, and it
/// matters more since M1-7 than it did before: the character arm of
/// [`input::keyboard_bytes`] used to refuse everything outside ASCII, so a
/// composed character could not have been typed twice even if the key had
/// arrived. The guard is gone (it was swallowing `ü ä ö ß`), and what holds
/// the line now is that **no key arrives under a commit**: while a
/// composition is in flight the physical key is `NamedKey::Process`, which
/// this function answers `None` for on its first arm.
///
/// MUTATION: let `Process` fall through to the character arm and the second
/// assertion goes red — the wire then carries `你好` twice for one commit.
#[test]
fn one_composition_commits_its_characters_exactly_once() {
    let committed = ime_commit_bytes("你好");
    assert_eq!(committed, vec![0xe4, 0xbd, 0xa0, 0xe5, 0xa5, 0xbd]);
    // The key the commit came out of, which is every key pressed while a
    // composition is live.
    let under_the_commit = input::legacy_bytes(
        &Key::Named(NamedKey::Process),
        ModifiersState::empty(),
        false,
    );
    assert_eq!(under_the_commit, None, "no key writes under a commit");
    let mut wire = committed.clone();
    wire.extend(under_the_commit.unwrap_or_default());
    assert_eq!(
        wire.windows(committed.len())
            .filter(|window| *window == committed.as_slice())
            .count(),
        1,
        "one commit puts the characters on the wire once",
    );
}

/// **The preview publishes the caret of the surface its letters go to**
/// (M1-8; `docs/DESIGN.md` §13.16 ②; measured on the Mac).
///
/// Three readers, one door. `ime_owner` calls the rung `Preview` when
/// `preview_keyboard_surface` answers, `edit_preview` inserts the commit
/// into whatever that door names, and the caret the candidate list hangs
/// from was measured from `preview_edit_focus` instead — a narrower door.
/// A preview seat holding the keyboard without the quick edit's focus was
/// therefore routed, inserted into, and given no caret, and the list stayed
/// wherever it had last been put. On Windows that is a list standing at a
/// stale rectangle; on a Mac, where nothing else has ever placed one, it is
/// a list at the corner of the window (X-3).
///
/// A source pin because the three readers are the invariant: a behavioural
/// test would fix one of them and leave the other two free to part company
/// again.
///
/// MUTATION: send any of the three back to `preview_edit_focus` and this
/// goes red.
#[test]
fn the_preview_publishes_the_caret_of_the_surface_its_letters_go_to() {
    let body = |name: &str| method_body("Runtime", name);
    assert!(
        body("keyboard_owner").contains("preview: self.preview_keyboard_surface().is_some()"),
        "the rung a composition is routed by",
    );
    assert!(
        body("preview_ime_cursor_area").contains("let surface = self.preview_keyboard_surface()?;"),
        "the caret the candidate list is hung from",
    );
    assert!(
        body("edit_preview").contains("let Some(surface) = self.preview_keyboard_surface() else {"),
        "the buffer a commit is inserted into",
    );
}

/// **A composition belongs to the field it was typed into** (user report
/// 2026-09-12; `docs/DESIGN.md` §7.1.5a″).
///
/// The report: `gif` typed into the command palette with an input method on,
/// the row picked, the palette gone — and the candidate list still floating
/// over the picture it had opened, attached to nothing. Nothing in this
/// window had ever ended a composition, and Windows only ends one on a blur.
///
/// MUTATION: answer `false` when the two rungs differ and both assertions go
/// red — which is the window as the reporter found it.
#[test]
fn closing_the_palette_cancels_a_composition_typed_into_it() {
    let typing = KeyboardOwner {
        palette: true,
        menu_or_dialog: true,
        ..KeyboardOwner::default()
    };
    assert_eq!(ime_owner(typing), ImeOwner::Palette);
    assert!(
        !composition_outlived_its_field(Some(ImeOwner::Palette), ime_owner(typing)),
        "while the box is up the letters are its own",
    );
    // The palette is closed: `menu_or_dialog` and `palette` go down
    // together, and the keyboard is back where it was.
    let closed = KeyboardOwner::default();
    assert_eq!(ime_owner(closed), ImeOwner::Shell);
    assert!(
        composition_outlived_its_field(Some(ImeOwner::Palette), ime_owner(closed)),
        "and the moment the box is gone the composition has no field",
    );
}

/// **The composition does not follow the keyboard to another owner**
/// (§7.1.5a″) — whichever pair of rungs it is.
///
/// Said over every rung rather than over the reported one, because what
/// makes a composition stale is not which gesture moved the keyboard: it is
/// that the keyboard is somewhere else. The shell is in the walk on purpose
/// — the destination the report's letters would otherwise have reached.
///
/// MUTATION: compare against `ImeOwner::Shell` instead of against the rung
/// now holding the keyboard and every pair that does not involve a shell
/// goes red.
#[test]
fn a_composition_does_not_follow_the_keyboard_to_another_owner() {
    for started_in in ImeOwner::ALL {
        assert!(
            !composition_outlived_its_field(Some(started_in), started_in),
            "{started_in:?} keeps its own composition while it keeps the keyboard",
        );
        for now in ImeOwner::ALL.into_iter().filter(|rung| *rung != started_in) {
            assert!(
                composition_outlived_its_field(Some(started_in), now),
                "a composition begun in {started_in:?} may not arrive at {now:?}",
            );
        }
    }
    assert!(
        !composition_outlived_its_field(None, ImeOwner::Shell),
        "and a window composing nothing has nothing to cancel",
    );
}

/// PIN (user report + ruling, 2026-08-13) — **the caret blinks exactly when
/// typing would land in it.**
///
/// The report: click a files tree or a preview and the terminal's caret goes
/// on blinking behind you. `bt_render::seat_caret` already had the right
/// *shape* for it — a caret that is not the keyboard's is steady and faded,
/// which is what an unfocused pane's has always been — and the wrong
/// *question*: it was handed "is this the focused pane" rather than "is this
/// where the next keystroke goes".
///
/// MUTATIONS, one per owner:
/// ① drop the `files_tree` clause — clicking a tree leaves the shell blinking,
///    which is the report verbatim;
/// ② drop the `preview` clause — the same, one leaf along, and it also covers
///    read-only browsing, where the arrows scroll the document;
/// ③ drop `menu_or_dialog` — a menu that swallows every character key stands
///    over a caret still claiming them.
#[test]
fn only_a_shell_with_the_keyboard_gets_a_blinking_caret() {
    assert!(
        keyboard_owner_is_a_shell(KeyboardOwner::default()),
        "with nothing else holding it, the shell has the keyboard"
    );
    for owner in [
        KeyboardOwner {
            files_tree: true,
            ..KeyboardOwner::default()
        },
        KeyboardOwner {
            preview: true,
            ..KeyboardOwner::default()
        },
        KeyboardOwner {
            menu_or_dialog: true,
            ..KeyboardOwner::default()
        },
    ] {
        assert!(
            !keyboard_owner_is_a_shell(owner),
            "{owner:?} owns the keyboard, so no caret may claim it"
        );
    }
    // And the moment the owner hands it back, it is the shell's again — the
    // predicate is read per frame, so there is no state to un-set.
    assert!(keyboard_owner_is_a_shell(KeyboardOwner {
        rename: false,
        files_tree: false,
        graph_search: false,
        git_prompt: false,
        search: false,
        preview: false,
        menu_or_dialog: false,
        palette: false,
    }));
}

/// PIN (user report, 2026-08-25) — **a popup that takes the keyboard takes
/// the keys too**, or a caret stands frozen over a shell that is receiving
/// the characters.
///
/// The report: one tab whose caret could be typed into and never blinked.
/// The freeze is `advance_cursor_blink_if_due`'s and the fade is
/// `bt_render::seat_caret`'s, and both are read off one predicate —
/// [`keyboard_owner_is_a_shell`] — so a caret that will not blink is the
/// window saying "the keyboard is somewhere else". It was: `menu_or_dialog`
/// counted all eight [`Popup`]s, while `keyboard_input`'s ladder named six.
/// The two it left out are the files column's root menu and the commit
/// graph's branch filter, and neither draws anything at all once the tab it
/// was raised on is behind you — so the window could sit in "a menu owns
/// the keyboard" with no menu on screen, freezing the caret while every
/// keystroke went to the shell.
///
/// RED EVIDENCE (2026-08-25): with `popup_takes_the_key` transcribing the
/// old ladder — `File | Pane | GitMenu | TermMenu | Profile | Preview` —
/// this test fails on `Root` and on `GraphFilter` with "the caret says the
/// keyboard has left every shell, so the keystroke must not reach one".
///
/// MUTATIONS that must turn it red:
/// ① drop an arm from [`PopupsUp::holds`] (it will not compile) or answer
///    `false` in one — that popup stops taking the key while the caret goes
///    on being frozen for it;
/// ② give the ladder its own list again, of any length but eight.
#[test]
fn a_popup_that_takes_the_keyboard_takes_the_keystroke_too() {
    assert!(
        popup_takes_the_key(PopupsUp::default()).is_none(),
        "with no popup up, the keystroke is the shell's"
    );
    for popup in Popup::ALL {
        let up = PopupsUp::only(popup);
        assert_eq!(
            up.any(),
            Some(popup),
            "{popup:?} is raised, so the window has to be able to say so"
        );
        // The caret's question and the keystroke's, answered off the same
        // reading — the whole of the fix, stated as an equality rather than
        // as two lists that have to be kept in step by hand.
        let owner = KeyboardOwner {
            menu_or_dialog: popup_takes_the_key(up).is_some(),
            ..KeyboardOwner::default()
        };
        assert!(
            !keyboard_owner_is_a_shell(owner),
            "{popup:?}: the caret says the keyboard has left every shell, so \
                 the keystroke must not reach one"
        );
    }
}

/// PIN (user report, 2026-08-12) — **a composition goes where the keyboard
/// is, and only the shell has a PTY.**
///
/// The report, with a screenshot: click into the preview's edit surface,
/// type Chinese, and the candidate list floats over the document while
/// `ni'hao` arrives at the shell prompt underneath. `keyboard_input` had a
/// twelve-rung ladder deciding who owns a keystroke; `ime_input` had two
/// rungs and then wrote every commit to the PTY, so composed text was the
/// one kind of typing that ignored `InputOwner` entirely.
///
/// The last loop is the load-bearing one. "Zero PTY writes when the preview
/// or the tree owns the keyboard" cannot be asserted without a window, but
/// it has an exact structural twin that can: the owner that gets the
/// composition must be the same owner whose caret is allowed to blink,
/// because a blink *means* "typing lands here" (2026-08-13's ruling). One
/// predicate, both questions — so the two can never drift into a window
/// where a caret blinks in a shell that is not receiving the characters.
///
/// MUTATIONS that must turn it red:
/// ① answer `Shell` for `preview` — the reported bug, exactly;
/// ② answer `Shell` for `files_tree` — the same bug one surface over;
/// ③ put `preview` above `files_tree` — the ladder stops being
///    `keyboard_input`'s and the two disagree about a focused tree in a
///    preview's tab.
#[test]
fn a_composition_goes_where_the_keyboard_is_and_only_the_shell_has_a_pty() {
    assert_eq!(ime_owner(KeyboardOwner::default()), ImeOwner::Shell);
    assert_eq!(
        ime_owner(KeyboardOwner {
            preview: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::Preview,
        "the edit surface takes what is typed into it"
    );
    assert_eq!(
        ime_owner(KeyboardOwner {
            files_tree: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::FilesTree,
        "a column has nothing to type into — and nothing to type through"
    );
    assert_eq!(
        ime_owner(KeyboardOwner {
            menu_or_dialog: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::Modal
    );
    // **And the palette stands above that rung** (DESIGN.md §7.55). It is a
    // popup, so `menu_or_dialog` is true the whole time it is up; if it
    // fell to `Modal` with the rest of them the box would swallow every
    // composition and a reader could not type a Chinese file name into the
    // one surface in this product whose entire gesture is typing.
    assert_eq!(
        ime_owner(KeyboardOwner {
            palette: true,
            menu_or_dialog: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::Palette,
        "the palette is a popup and a text field, and the field wins"
    );
    assert_eq!(
        ime_caret_source(ImeOwner::Palette),
        ImeCaretSource::Field,
        "so its candidate list hangs off its own caret"
    );
    assert_eq!(
        ime_owner(KeyboardOwner {
            rename: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::Rename
    );

    // The order is `keyboard_input`'s order, rung for rung.
    assert_eq!(
        ime_owner(KeyboardOwner {
            files_tree: true,
            preview: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::FilesTree,
        "the tree's rung stands above the preview's, as it does for keys"
    );
    // T4 (v2 (3)) — the graph's search field stands between them: it lives on
    // a preview seat, so a composition routed by `preview` alone would land
    // in that seat's own edit surface while the caret was in the toolbar.
    assert_eq!(
        ime_owner(KeyboardOwner {
            graph_search: true,
            preview: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::GraphSearch,
    );
    assert_eq!(
        ime_owner(KeyboardOwner {
            rename: true,
            menu_or_dialog: true,
            files_tree: true,
            graph_search: true,
            git_prompt: true,
            search: true,
            preview: true,
            palette: true,
        }),
        ImeOwner::Rename,
        "and the editor is the topmost of all of them"
    );
    // v2 (4) — the branch prompt stands **above** the popup rung, and that
    // is the whole reason it is a rung of its own: a popup swallows every
    // character key, which is right for a menu of verbs and wrong for a menu
    // you are typing a branch name into.
    assert_eq!(
        ime_owner(KeyboardOwner {
            git_prompt: true,
            menu_or_dialog: true,
            files_tree: true,
            graph_search: true,
            preview: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::GitPrompt,
        "the prompt inside a popup takes what is composed into it"
    );
    // §7.1.5d — **the capsule's field is under every one of them and over
    // the shell**, which is the only place it can be: it can only ever be
    // open on a terminal seat, so it can never be up at the same time as any
    // rung above it, and the rung below is the shell it stands on. That last
    // half is the one the rung exists for: without it a query composed in
    // Chinese would arrive at somebody's prompt.
    assert_eq!(
        ime_owner(KeyboardOwner {
            search: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::Search,
    );
    assert_eq!(
        ime_owner(KeyboardOwner {
            search: true,
            menu_or_dialog: true,
            ..KeyboardOwner::default()
        }),
        ImeOwner::Modal,
        "a dialog over the capsule takes everything, as it does over every surface"
    );
    // And a capsule that is up with the hands back on the terminal (B81)
    // owns nothing at all: the bit is only set while the caret is in it.
    assert_eq!(
        ime_owner(KeyboardOwner::default()),
        ImeOwner::Shell,
        "an unfocused capsule is not an owner"
    );

    // **The whole of "zero PTY writes", stated as a property.**
    //
    // Seven bits since the search capsule joined the ladder (§7.1.5d): the
    // sweep is over every combination there is, so a rung added without a
    // matching arm in `keyboard_owner_is_a_shell` fails here rather than in
    // a screenshot of a query appearing at somebody's prompt.
    for bits in 0..256u16 {
        let owner = KeyboardOwner {
            rename: bits & 1 != 0,
            menu_or_dialog: bits & 2 != 0,
            files_tree: bits & 4 != 0,
            preview: bits & 8 != 0,
            graph_search: bits & 16 != 0,
            git_prompt: bits & 32 != 0,
            search: bits & 64 != 0,
            palette: bits & 128 != 0,
        };
        assert_eq!(
            matches!(ime_owner(owner), ImeOwner::Shell),
            keyboard_owner_is_a_shell(owner),
            "{owner:?}: a composition reaches the PTY exactly when a caret \
                 may blink, and never on any other terms"
        );
    }
}

/// PIN (T-REMOTE-INPUT-PACKET, owner evidence 2026-09-15) — **a commit that
/// arrived with no composition in front of it is still what the reader
/// typed**, and it is typed once.
///
/// # The second shape of injected text
///
/// A program that types for you reaches this window in one of two shapes,
/// and only one of them is a key. The phone keyboard's is
/// ([`input::injected_logical_key`] carries that half). Windows' own **Win+H
/// voice typing**, in a window with no text store — which is every window
/// this program has — uses the *other* one: it opens an IMM composition and
/// goes straight to its result, with no pre-edit anywhere in it. Measured in
/// a bare Win32 window on 2026-09-15:
///
/// ```text
/// WM_IME_STARTCOMPOSITION
/// WM_IME_COMPOSITION lParam=0x0800 (GCS_RESULTSTR) "你可以听到我说话吗"
/// WM_IME_ENDCOMPOSITION
/// ```
///
/// winit reads `GCS_RESULTSTR` the same way whatever else is in `lParam`
/// (`event_loop.rs`'s `WM_IME_COMPOSITION` arm), so what this window is
/// handed is `Ime::Preedit("")` followed by `Ime::Commit(sentence)` — the
/// identical pair Microsoft Pinyin ends a word with, which is why that half
/// needs no new road: **a commit is already unconditional here.** This test
/// is what keeps it that way. The tempting guard — "only commit while
/// something is being composed" — is one this file has the vocabulary to
/// write (the window's own `preedit` and `composing` are both in reach)
/// and it would silently delete every dictated sentence, because a
/// dictation never composes anything.
///
/// **And once.** winit answers `WM_IME_COMPOSITION` with `Value(0)` rather
/// than `DefWindowProc`, so the `WM_IME_CHAR`/`WM_CHAR` tail the probe saw
/// is never generated under it; and a `WM_CHAR` with no key press under it
/// is dropped by winit's own builder ("Received a CHAR message but no
/// `event_info` was available"). So the commit door and the key door cannot
/// both fire for one sentence — which is also why the injected-text rewrite
/// belongs at the top of the *key* ladder and nowhere near this one.
///
/// MUTATIONS: put any `if` or any early `return` between the commit arm and
/// its write — Win+H goes silent again while every Chinese IME keeps
/// working, which is exactly the shape of bug that survives a release; move
/// the injected-key rewrite out of `keyboard_input` and the last assertion
/// goes red.
#[test]
fn a_commit_with_no_composition_in_front_of_it_still_reaches_the_child() {
    let door = method_body("Runtime", "ime_input");
    let commit = door
        .find("Ime::Commit(text) => {")
        .expect("the composition door has a commit arm");
    let write = door[commit..]
        .find("write_pty_input(")
        .expect("and that arm ends in the write into the child");
    let statements = door[commit..commit + write]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect::<Vec<_>>();
    assert!(
        !statements.iter().any(|line| line.starts_with("if ")),
        "a condition stands between a commit and the child, and a dictated \
             sentence composes nothing for it to be true of:\n{statements:#?}"
    );
    // A `return` *statement*, which is the only way out of this arm short of
    // the write — `return_to_live_for_input` is a call and spells the same
    // six letters, which is why this is asked of the start of the line.
    assert!(
        !statements.iter().any(|line| line.starts_with("return")),
        "the commit arm can now decline to type what it was given:\n{statements:#?}"
    );

    // The other shape's road, and the assertion that the two stay apart: the
    // key that carries text is rewritten in the *key* ladder, so no sentence
    // can arrive through both doors.
    let ladder = method_body("Runtime", "keyboard_input");
    assert!(
        ladder.contains("input::injected_logical_key("),
        "a press that carries text and names no key has no rung again"
    );
    assert!(
        !door.contains("injected_logical_key"),
        "the composition door does not also rewrite keys"
    );
}

/// PIN (user report, 2026-08-17) — **every rung says where its caret comes
/// from, and the sweep proves the ladder can produce no other rung.**
///
/// The bug was a hole in a mapping nobody had written down: two surfaces
/// published a caret rectangle and the other six published nothing, so a
/// query composed in the search capsule got its candidate list wherever the
/// window's last published caret happened to be. The cure is one total
/// decision, and totality is the thing to hold: a rung added tomorrow either
/// names a source here or fails to compile, and a rung the ladder can reach
/// that is missing from [`ImeOwner::ALL`] fails below.
///
/// The three answers are the three kinds of caret this window has, and each
/// is asserted by name rather than by "not the others":
///
/// - the **grid's**, which only a composed frame can produce, so it is
///   published from `publish_frame_inner` and nowhere else;
/// - a **field's**, measured off a layout the window keeps — the capsule, the
///   graph's toolbar, the branch prompt, the tab editor, the preview's body;
/// - **nowhere**, which is a popup of verbs and a files column: both swallow
///   compositions outright, so there is no caret to follow and the candidate
///   window is left where it is rather than dragged somewhere nothing is
///   being typed.
///
/// MUTATIONS: send `ImeOwner::Search` to `Nowhere` and the capsule's rung goes
/// red — which is the reported bug, stated; send `Modal` to `Field` and the
/// popup's does; drop a variant from `ALL` and the sweep catches it.
#[test]
fn every_rung_of_the_ime_ladder_says_where_its_caret_comes_from() {
    for owner in ImeOwner::ALL {
        let source = ime_caret_source(owner);
        let expected = match owner {
            ImeOwner::Shell => ImeCaretSource::TerminalCursor,
            ImeOwner::Modal | ImeOwner::FilesTree => ImeCaretSource::Nowhere,
            ImeOwner::Rename
            | ImeOwner::GraphSearch
            | ImeOwner::GitPrompt
            | ImeOwner::Preview
            | ImeOwner::Search
            | ImeOwner::Palette => ImeCaretSource::Field,
        };
        assert_eq!(
            source, expected,
            "{owner:?} hangs the candidate window off {source:?}"
        );
    }
    // The capsule is the rung the report was about, and it is a field: the
    // one answer that is *not* "the terminal's cursor" for a surface standing
    // on a terminal.
    assert_eq!(
        ime_caret_source(ImeOwner::Search),
        ImeCaretSource::Field,
        "a query composed in the capsule is composed in the capsule"
    );
    // Seven bits, exactly as the sweep above: every rung the ladder can
    // reach has to be in the table this test walks, or the table is proving
    // something about a smaller ladder than the one that ships.
    for bits in 0..256u16 {
        let owner = ime_owner(KeyboardOwner {
            rename: bits & 1 != 0,
            menu_or_dialog: bits & 2 != 0,
            files_tree: bits & 4 != 0,
            preview: bits & 8 != 0,
            graph_search: bits & 16 != 0,
            git_prompt: bits & 32 != 0,
            search: bits & 64 != 0,
            palette: bits & 128 != 0,
        });
        assert!(
            ImeOwner::ALL.contains(&owner),
            "{owner:?} is a rung the ladder produces and the caret table does \
                 not list"
        );
    }
}

/// PIN (user report, 2026-08-17) — **the rectangle the capsule's rung hands
/// winit stands inside the capsule's own field box.**
///
/// [`search::Capsule::caret_line`] is asserted where it is written; what is
/// asserted here is the last step, which is the window's: rounding a line box
/// to the whole-pixel origin-and-size pair `set_ime_cursor_area` takes must
/// not push the rectangle out of the field, and the **size must stay the
/// line's** — a height rounded to zero is a candidate window with no
/// clearance to stand clear of, which is how it ends up over the field it is
/// supposed to sit under.
///
/// MUTATIONS: floor the origin and ceil the far edge and the containment
/// assertion goes red on a fractional scale; use the caret's width for the
/// height and the "under the field" assertion does.
#[test]
fn the_capsule_hands_the_ime_a_box_inside_its_own_field() {
    for scale in [1.0f32, 1.25, 1.5, 2.0] {
        let capsule = search::lay_out([0.0, 0.0, 900.0, 600.0], None, scale, 36.0 * scale);
        for caret_x in [0.0f32, 3.5, 17.0, 61.25, 10_000.0] {
            let line = capsule.caret_line(caret_x, scale);
            let area = ime_cursor_area_of(line);
            let right = area.x + i32::try_from(area.width).expect("a caret is not that wide");
            let bottom = area.y + i32::try_from(area.height).expect("nor that tall");
            assert!(
                f64::from(area.x) >= f64::from(capsule.field[0]).floor()
                    && f64::from(right) <= f64::from(capsule.field[2]).ceil(),
                "{scale}x, prefix {caret_x}: {area:?} is not in {:?}",
                capsule.field
            );
            assert!(
                f64::from(area.y) >= f64::from(capsule.field[1]).floor()
                    && f64::from(bottom) <= f64::from(capsule.field[3]).ceil(),
                "{scale}x, prefix {caret_x}: {area:?} does not sit on the \
                     field's own line"
            );
            assert_eq!(
                area.height,
                (capsule.field[3].round() - capsule.field[1].round()).max(1.0) as u32,
                "{scale}x: the candidate window is placed clear of the whole \
                     field, not of the caret's hairline"
            );
        }
    }
}

/// RED (35) — **Esc closes a pinned menu, and the pin goes with it**, for the
/// pane head's `⌄` and the tab strip's.
///
/// A pinned menu closes on three things and Esc is the first. The key reaches
/// the menu through its own closer — `close_pane_menu`, `close_profile_menu`
/// (which goes through `close_popup`) — and each closer drops its gate's pin
/// through [`ChevronGates::menu_gone`]; a pin left behind would make the next
/// rest on another pane head open nothing while no menu was up. So after Esc the gate is
/// unpinned, and a following rest opens a peek whose grace runs again.
///
/// No `Runtime` can be built without a window here, so the door is held by
/// name and the machine is driven through the calls the door makes.
///
/// MUTATION: drop `self.window.chevrons.menu_gone(Popup::Pane)` from
/// `close_pane_menu` (or `menu_gone(popup)` from `close_popup`'s arms) and the
/// source half goes red; make `ChevronGate::menu_gone` keep the pin and the
/// machine half does.
#[test]
fn esc_closes_a_pinned_menu() {
    let keys = method_body("Runtime", "keyboard_input");
    assert!(keys.contains("self.close_pane_menu()?"));
    assert!(keys.contains("self.close_profile_menu()?"));
    assert!(
        method_body("Runtime", "close_pane_menu")
            .contains("self.window.chevrons.menu_gone(Popup::Pane)"),
        "the pane menu's closer takes the pin with the menu"
    );
    assert!(
        method_body("Runtime", "close_profile_menu").contains("self.close_popup(Popup::Profile)"),
        "the picker's closer goes through the one arm"
    );
    assert_eq!(
        method_body("Runtime", "close_popup")
            .matches("self.window.chevrons.menu_gone(popup)")
            .count(),
        3,
        "and that arm drops the pin for each of the three menus a `⌄` governs"
    );

    let start = Instant::now();
    for (popup, control) in the_three_chevrons().into_iter().take(2) {
        let mut gates = peek_open(popup, start);
        press_pins_a_peek(
            press_spends_itself_closing(chevron_button(control), Some(control)),
            gates.gate(popup),
        );
        // Esc.
        gates.menu_gone(popup);
        assert!(
            gates.gate(popup).is_some_and(|gate| !gate.is_pinned()),
            "{popup:?}: Esc takes the pin with the menu"
        );
        assert_eq!(
            gates,
            ChevronGates::default(),
            "{popup:?}: Esc leaves the gates as they were before any menu"
        );
        // So the next rest opens a peek, and the hand leaving it runs the grace.
        let later = start + Duration::from_secs(2);
        let mut again = peek_open(popup, later);
        hand_leaves(&mut again, popup, later);
        assert_eq!(
            again.deadline(),
            Some(later + profiles::CHEVRON_LEAVE_GRACE),
            "{popup:?}: the next rest-open is a peek, and its grace runs"
        );
    }
}

/// RED (0.4.4 ticket 02) — **a file dropped while the card is up is refused**, because the card is
/// a modal rung of `KeyboardOwner` (owner's ruling 2026-09-23 made it modal; the design note's
/// `a_pending_paste_question_does_not_refuse_a_drop` was written for the withdrawn strip and is
/// withdrawn with it).
///
/// MUTATION: take `self.paste_card_seat().is_some()` out of `keyboard_owner`'s `menu_or_dialog`.
#[test]
fn a_drop_while_the_card_is_up_is_refused() {
    let owner = squeezed_body("Runtime", "keyboard_owner");
    assert!(
        owner.contains("||self.paste_card_seat().is_some()"),
        "the card is not a rung of the keyboard owner:\n{owner}"
    );
    let modal = KeyboardOwner {
        menu_or_dialog: true,
        ..KeyboardOwner::default()
    };
    assert!(modal.is_modal());
    assert_eq!(
        ime_owner(modal),
        ImeOwner::Modal,
        "a composition goes nowhere"
    );
    let (_tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    let promised = Some(PasteOffer {
        landing: DropLanding::SeatCentre {
            target: target.seat,
        },
        target,
    });
    assert_eq!(
        paste_offer_is_kept(GlassHere::Ours, promised, promised, true, modal.is_modal()),
        None,
        "a path was written under the card"
    );
}

/// RED (57) — **With the restore card up, Ctrl+V pastes nothing into the shell.**
///
/// Seen on the clean VM on 2026-09-23 (ticket 03's run): with "Reopen your other tabs?" up at
/// launch, `Ctrl+V` pasted into the pane under the card, because the card's rung stood below the
/// clipboard rung and caught only `Enter`. A paste has three roads to the terminal's clipboard
/// door — the chord, the terminal menu's *Paste* (a right press, which the card lets past it) and
/// the macOS Edit menu's *Paste* — and all three now defer to one list: the card has a rung above
/// the clipboard rung that returns for every key, it is on the list of surfaces above that rung,
/// and the door itself asks that list before the clipboard is read.
///
/// No `Runtime` can be built without a window and a GPU, so the claim is pinned on the bodies the
/// bytes would have to pass through, read through `bt_source`, as ticket 02 pinned the paste
/// card's modality.
///
/// MUTATION: take `|| self.restore_card_is_up()` out of
/// `a_surface_above_the_clipboard_rung_holds_the_keyboard` — the first assertion goes red, and the
/// terminal menu's *Paste* writes under the card again.
#[test]
fn with_the_restore_card_up_ctrl_v_pastes_nothing_into_the_shell() {
    let defers = squeezed_body(
        "Runtime",
        "a_surface_above_the_clipboard_rung_holds_the_keyboard",
    );
    assert!(
        defers.contains("||self.restore_card_is_up()"),
        "the restore card is not on the list of surfaces above the clipboard rung:\n{defers}"
    );
    // The chord is the clipboard rung's own.
    assert!(input::is_paste_shortcut(
        &Key::Character("v".into()),
        winit::keyboard::ModifiersState::CONTROL
    ));
    let ladder = squeezed_body("Runtime", "keyboard_input");
    let rung = ladder
        .find(RESTORE_CARD_RUNG)
        .unwrap_or_else(|| panic!("the restore card's rung is not whole"));
    let paste = ladder
        .find("self.paste_from_clipboard()?;")
        .expect("the clipboard rung is still a way out of `keyboard_input`");
    assert!(
        rung < paste,
        "`Ctrl+V` reaches the clipboard rung before the card's"
    );
    // The door every clipboard road shares asks the list before it reads the clipboard.
    let door = squeezed_body("Runtime", "paste_from_clipboard_into");
    let asked = door
        .find("ifself.a_surface_above_the_clipboard_rung_holds_the_keyboard(){returnOk(());}")
        .unwrap_or_else(|| {
            panic!("the clipboard door does not ask who holds the keyboard:\n{door}")
        });
    let read = door
        .find("bt_platform::clipboard_payload()")
        .expect("the door reads the clipboard");
    let applied = door
        .find("self.apply_clipboard_payload(target,payload)")
        .expect("the door applies its clipboard payload");
    assert!(
        asked < read && read < applied,
        "the question is asked before the read and the apply follows the read"
    );
    let apply = squeezed_body("Runtime", "apply_clipboard_payload");
    let recipient = apply
        .find("leaf.paste_recipient.clone()")
        .expect("the apply step resolves the recipient");
    let delivered = apply
        .find("self.deliver_paste(")
        .expect("the apply step delivers the paste");
    assert!(
        recipient < delivered,
        "delivery follows recipient resolution"
    );
    // And the terminal menu's Paste goes through that door, not around it.
    assert!(
        squeezed_body("Runtime", "run_term_menu_row")
            .contains("profiles::TermMenuRow::Paste=>self.paste_from_clipboard_into(seat),"),
        "the terminal menu pastes through a door of its own"
    );
}

/// RED (57) — **With the restore card up, a printable key reaches no shell.**
///
/// A letter reaches a shell two ways: as a key the encoder turns into bytes, and — for Chinese,
/// Japanese, Korean — as a composition committed to the shell's input. The card's rung returns
/// before the IME rung, the shortcut table and the encoder, and answers no `Key::Character`; and
/// the card is a rung of `KeyboardOwner`, so a composition resolves to `ImeOwner::Modal` and goes
/// nowhere, no caret blinks in the shell under it, and a drop is refused under it.
///
/// MUTATION: take `|| self.restore_card_is_up()` out of `keyboard_owner`'s `menu_or_dialog` — the
/// first assertion goes red (or take the rung's `return Ok(())` out — the rung is no longer whole).
#[test]
fn with_the_restore_card_up_a_printable_key_reaches_no_shell() {
    let owner = squeezed_body("Runtime", "keyboard_owner");
    assert!(
        owner.contains("||self.restore_card_is_up()"),
        "the restore card is not a rung of the keyboard owner:\n{owner}"
    );
    let modal = KeyboardOwner {
        menu_or_dialog: true,
        ..KeyboardOwner::default()
    };
    assert!(!keyboard_owner_is_a_shell(modal), "no caret blinks");
    assert_eq!(
        ime_owner(modal),
        ImeOwner::Modal,
        "a composition goes nowhere"
    );
    assert!(modal.is_modal(), "a drop is refused");

    let ladder = squeezed_body("Runtime", "keyboard_input");
    let rung = ladder
        .find(RESTORE_CARD_RUNG)
        .unwrap_or_else(|| panic!("the restore card's rung is not whole"));
    assert!(
        !RESTORE_CARD_RUNG.contains("Key::Character"),
        "the card answers a printable key"
    );
    for road in [
        "input::is_ime_owned_key(",
        "self.copy_selection()?;",
        "self.app.shortcuts.lookup(",
        "input::keyboard_bytes(",
        "self.send_user_input(",
    ] {
        let at = ladder
            .find(road)
            .unwrap_or_else(|| panic!("`{road}` is no longer a way out of `keyboard_input`"));
        assert!(rung < at, "`{road}` is reached before the card's rung");
    }
}
