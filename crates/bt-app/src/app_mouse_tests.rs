//! **The crate root: mouse and drag.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    CARDS_AT_200, PtyPresentationHarness, TARGET, a_local_file, a_local_folder, a_shell, at,
    calls_of, cards_column, centre, every_wheel_situation, flush_test_wheel, found, hand_leaves,
    hyperlink_hit, in_product, method_body, no_directories, peek_open, reader_names, source,
    squeezed_body, wheel_pane_at_top,
};
use bt_source::{Needle, Pattern, View};
use std::time::Duration;
use winit::keyboard::{Key, NamedKey};

// ── dragging a peek's header keeps it (user ruling 2026-08-12) ──────────

/// PIN — **a peek's header press becomes a carry at the same six pixels
/// every other drag begins at, and the window does not move when it does.**
///
/// The ruling's rule ①, as arithmetic. Two halves, and the second is the one
/// that is easy to get wrong: the grab offset has to be the one measured at
/// the *press*, because by the time the latch trips the pointer has already
/// travelled six pixels. A grab measured at the promotion would put the
/// window's corner six pixels from where the hand is actually holding it, and
/// the window would visibly jump on the frame it was picked up — which is
/// exactly the "无缝" the ruling asks for, stated as its opposite.
///
/// The check is run through the real [`float::float_dragged_to`], because
/// "the window does not jump" is a claim about the frame that function
/// produces and not about the offset in isolation.
///
/// Mutation that must re-redden it: measure the grab at the promotion
/// (`grab: [position.x as f32 - frame[0], …]` inside `promoted`) instead of
/// carrying the press's own.
#[test]
fn a_peeks_header_press_becomes_a_carry_without_the_window_moving() {
    const SCALE: f64 = 1.0;
    // Room enough that the drag clamp never has an opinion.
    let viewport = [0.0_f32, 0.0, 4000.0, 4000.0];
    let frame = [200.0_f32, 120.0, 464.0, 420.0];
    let press = at(260.0, 132.0);

    let mut held = FloatHeadPress::armed(7, press, frame);
    assert_eq!(held.grab, [60.0, 12.0], "the offset inside the frame");

    // Still a press: a hand holding still is not a hand carrying anything.
    for (dx, dy) in [(0.0, 0.0), (3.0, 0.0), (0.0, -4.0), (3.5, 3.5)] {
        assert!(
            held.promoted(at(press.x + dx, press.y + dy), SCALE)
                .is_none(),
            "({dx}, {dy}) is inside the six pixels and promotes nothing"
        );
    }

    // And now it travels. Six pixels is the threshold every other press in
    // this product is measured by, and it is not re-declared here.
    let travel = (DRAG_THRESHOLD_LOGICAL_PX, 2.0);
    let moved = at(press.x + travel.0, press.y + travel.1);
    let Some(FloatDragKind::Move { grab }) = held.promoted(moved, SCALE) else {
        panic!("crossing the threshold turns the press into a carry");
    };
    assert_eq!(grab, [60.0, 12.0], "carrying the press's own offset");
    assert!(
        held.promoted(at(press.x + 400.0, press.y), SCALE).is_none(),
        "and it promotes once, not once per move"
    );

    // **The window is where the hand has taken it, and nowhere else.** Fed
    // the pointer it was pressed at, the carry reproduces the frame exactly;
    // fed the pointer it promoted at, it has moved by the travel and by
    // nothing else.
    assert_eq!(
        float::float_dragged_to(frame, [press.x as f32, press.y as f32], grab, viewport, 1.0),
        frame,
        "the promotion itself moves the window not at all"
    );
    assert_eq!(
        float::float_dragged_to(frame, [moved.x as f32, moved.y as f32], grab, viewport, 1.0),
        [
            frame[0] + travel.0 as f32,
            frame[1] + travel.1 as f32,
            frame[2] + travel.0 as f32,
            frame[3] + travel.1 as f32,
        ],
        "and afterwards it has moved exactly as far as the hand"
    );
}

/// PIN — the threshold is the *hand's*, so it scales with the display.
///
/// J113's rule reaching one more press: six logical pixels is a distance a
/// finger travels, and the same gesture on a 200% screen covers twice the
/// device pixels. A peek that needed twice the resolve to keep on a HiDPI
/// display would be reporting something about monitors that is really about
/// hands.
#[test]
fn the_peek_header_threshold_is_logical_pixels_like_every_other_drag() {
    let frame = [200.0_f32, 120.0, 464.0, 420.0];
    let press = at(260.0, 132.0);
    let just_over = DRAG_THRESHOLD_LOGICAL_PX;

    let mut hidpi = FloatHeadPress::armed(7, press, frame);
    assert!(
        hidpi
            .promoted(at(press.x + just_over, press.y), 2.0)
            .is_none(),
        "six device pixels at 200% is three logical ones — still a press"
    );
    assert!(
        hidpi
            .promoted(at(press.x + just_over * 2.0, press.y), 2.0)
            .is_some(),
        "twelve device pixels is the same six the hand travelled"
    );
}

/// PIN (user ruling, 2026-08-15): **"a wheel notch belongs to the pane it is
/// over — one law for the main screen and the alternate screen alike; what
/// the pane is doing decides only whether the notch scrolls our buffer or is
/// spoken to its child."**
///
/// The bug it replaces (user report, same day). The previous ruling had two
/// halves — scrolling follows the pointer, bytes follow the keyboard — and
/// the second half quietly ate the first on the alternate screen. An
/// unfocused pane running a full-screen program was refused by both
/// forwarding routes for being unfocused and fell through to
/// [`WheelRoute::Local`], which for an alternate screen means scrolling a
/// scrollback that does not exist. So hovering an unfocused pane running
/// Claude Code and turning the wheel produced **nothing at all**: no bytes,
/// no motion, no clue. The main screen had no such hole, which is exactly
/// why the two screens now answer to one law instead of two.
///
/// Asserted over the **whole fact space** against an oracle written in a
/// different shape from the implementation — a match on the tuple, arms in
/// the ruling's order rather than the code's — because the failure was a
/// hole in a chain of `if`s and a hole is found by covering the space, not
/// by sampling it. Focus does not appear, here or in the signature: that
/// absence *is* the ruling, and the fact that a `wheel_route` which consults
/// focus no longer compiles is the strongest pin available.
///
/// MUTATION: return [`WheelRoute::Local`] instead of [`WheelRoute::Nothing`]
/// for the mute alternate-screen case — the shipped behaviour for an
/// unfocused pane — and the sweep goes red. Gate the [`WheelRoute::MouseReport`]
/// arm on `!modes.alternate_screen`, which is the shape "bytes only to the
/// main screen" would take, and it goes red on every tracked TUI.
///
/// EXTENDED (37) — **the text-size rung stands ahead of this table**: the sweep is run a second
/// time with the gesture's exact modifier and with its near misses, through
/// `wheel_steps_text_size`, the decision `Runtime::mouse_wheel` asks before the math-block pan
/// and `wheel_route`. Not red on base — the decision did not exist there, and `mouse_wheel` cannot
/// be driven without a window — so its order in `mouse_wheel` is pinned separately
/// (`text_size_tests::ctrl_wheel_over_a_formula_steps_the_pane_and_does_not_pan_the_formula`).
///
/// MUTATION: build `input::text_size_wheel_held_on` on `modifiers.control_key()` alone — the
/// `Ctrl+Alt` near miss and the Mac's Control go red; drop the `y != 0` half of
/// `wheel_steps_text_size` — nothing here, but the horizontal-only case in
/// `text_size_tests::six_twenty_pixel_reports_are_one_step_and_a_reversal_at_the_top_steps_down_once`.
#[test]
fn a_wheel_notch_belongs_to_the_pane_it_is_over_on_either_screen() {
    use bt_term::{MouseTracking, TerminalModes};
    let mut situations = 0;
    for (shift, modes, scrolled) in every_wheel_situation() {
        situations += 1;
        let tracks = modes.mouse_tracking != MouseTracking::Off;
        // The ruling, restated as a table rather than as the chain of `if`s
        // under test. Same answers or the implementation is wrong; same
        // *text* and this test would be worth nothing.
        let expected = match (
            shift,
            modes.alternate_screen,
            modes.alternate_scroll,
            tracks,
            scrolled,
        ) {
            // Displaced into local review, and sticky until it rests again:
            // forwarding to pixels the user is not looking at is a lie.
            (_, true, _, _, true) => WheelRoute::Local,
            // The child asked for mouse reports. Both screens, focused or
            // not — this is the clause the alternate screen used to be
            // denied.
            (false, _, _, true, _) => WheelRoute::MouseReport,
            // Shift is the explicit local override, everywhere.
            (true, _, _, _, _) => WheelRoute::Local,
            // No tracking, but the alternate screen asked for the emulation.
            (false, true, true, false, _) => WheelRoute::ArrowKeys,
            // A program that owns the screen and asked for neither keeps its
            // wheel; there is no scrollback under it to move instead.
            (false, true, false, false, _) => WheelRoute::Nothing,
            // An ordinary shell: our own buffer, exact subpixels.
            (false, false, _, false, _) => WheelRoute::Local,
        };
        assert_eq!(
            wheel_route(shift, modes, scrolled),
            expected,
            "shift={shift} modes={modes:?} scrolled={scrolled}"
        );
    }
    assert_eq!(
        situations, 64,
        "the sweep really did cover every situation the route turns on"
    );

    // 主副屏同一律, said outright: with the child asking for reports and the
    // view at rest, which screen it is on changes nothing. This is the one
    // sentence the old code could not have satisfied.
    let plain = TerminalModes {
        alternate_screen: false,
        alternate_scroll: false,
        sgr_mouse: true,
        mouse_tracking: MouseTracking::Drag,
        focus_reporting: false,
        keyboard: bt_term::KeyboardProtocol::default(),
    };
    assert_eq!(
        wheel_route(false, plain, false),
        wheel_route(
            false,
            TerminalModes {
                alternate_screen: true,
                ..plain
            },
            false
        ),
        "a tracked main screen and a tracked alternate screen answer alike"
    );

    // **And the text-size rung ahead of it** (ticket 37, `docs/RULES.md` row 28): the same
    // sixty-four situations, each turned with the gesture's own modifier added and with each of
    // its near misses. With the exact modifier — `Ctrl` alone on Windows, `⌘` alone on a Mac —
    // every situation is a size step, whatever the pane is doing: a tracked program does not get
    // the report, a displaced review does not scroll, a mute full-screen program does not keep
    // it. With `Ctrl+Alt`, with Shift beside it, or with the other platform's key, every
    // situation answers exactly what the matrix above says it answers, which is the original
    // table kept unchanged for every gesture that is not a size gesture.
    use bt_platform::HostPlatform::{MacOs, Windows};
    use winit::keyboard::ModifiersState as Held;
    let notch = MouseScrollDelta::LineDelta(0.0, 1.0);
    let mut turned = 0;
    for (shift, modes, scrolled) in every_wheel_situation() {
        let shift_held = if shift { Held::SHIFT } else { Held::empty() };
        for (platform, exact, near_misses) in [
            (
                Windows,
                Held::CONTROL,
                [Held::CONTROL | Held::ALT, Held::SUPER],
            ),
            (MacOs, Held::SUPER, [Held::SUPER | Held::ALT, Held::CONTROL]),
        ] {
            let answer = |held: Held| {
                (!wheel_steps_text_size(held, platform, notch))
                    .then(|| wheel_route(held.shift_key(), modes, scrolled))
            };
            turned += 1;
            assert_eq!(
                answer(exact | shift_held),
                if shift {
                    Some(wheel_route(shift, modes, scrolled))
                } else {
                    None
                },
                "{platform:?} exact modifier, shift={shift} modes={modes:?} scrolled={scrolled}"
            );
            for held in near_misses {
                assert_eq!(
                    answer(held | shift_held),
                    Some(wheel_route(shift, modes, scrolled)),
                    "{platform:?} {held:?} keeps the original route, shift={shift}                      modes={modes:?} scrolled={scrolled}"
                );
            }
        }
    }
    assert_eq!(turned, 128, "every situation, on both platforms");
}

/// **`Shift` does not gain a third meaning; a pane gains a second axis**
/// (horizontal scroll plan §1c, the entry gesture).
///
/// The whole sweep, as a table rather than as the chain under test — and the
/// half of it that matters is the first eight rows. **With no column axis,
/// every combination answers exactly what it answered before this existed**,
/// which is the executable form of "nothing about the wheel changes for a
/// wrapping pane". §7.1.6b′'s objection to a third meaning for `Shift` is
/// answered here and not in prose: the key's meaning is a function of what
/// the pane has, and where the pane has nothing it means what it always did.
///
/// MUTATION: drop the `has_column_axis` guard — make `shift` alone decide —
/// and the first eight rows go red at once: a full-screen program's local
/// review, which is `Shift`'s standing job, starts scrolling an axis that is
/// not there instead of the rows the reader was reviewing.
#[test]
fn shift_names_the_axis_only_where_the_pane_has_two() {
    let mut situations = 0;
    for has_column_axis in [false, true] {
        for shift in [false, true] {
            for sideways in [false, true] {
                for shift_only_rows in [false, true] {
                    situations += 1;
                    let expected = match (has_column_axis, shift, sideways, shift_only_rows) {
                        // One axis, one answer — the answer it has always
                        // given.
                        (false, _, _, _) => WheelAxis::Rows,
                        // The axis the pane has to offer: rows it displaced
                        // above itself that nothing but this key reaches,
                        // before the columns anything can reach.
                        (true, true, _, true) => WheelAxis::Rows,
                        // Two axes: the key that already means "this
                        // window's own view" says which of its axes, and a
                        // report that already points sideways needs no key
                        // at all.
                        (true, true, _, false) | (true, false, true, _) => WheelAxis::Columns,
                        (true, false, false, _) => WheelAxis::Rows,
                    };
                    assert_eq!(
                        wheel_axis(shift, sideways, shift_only_rows, has_column_axis),
                        expected,
                        "shift={shift} sideways={sideways} \
                             shift_only_rows={shift_only_rows} \
                             axis={has_column_axis}"
                    );
                }
            }
        }
    }
    assert_eq!(situations, 16, "the sweep covered every situation");
}

/// PIN (owner report, Mac mini, 0.4.1): **a typeset formula pushed rows off
/// the top of an alternate-screen pane, the chip under it said `21 rows
/// above · Shift+wheel`, and on a Mac that gesture did nothing whatsoever.**
///
/// Every decision in this window was already right about it: [`wheel_route`]
/// read the `Shift`, ruled [`WheelRoute::Local`], and handed the notch to the
/// local row scroll exactly as it does on Windows. **The number was wrong, not
/// the routing** — macOS rewrites `Shift` plus a vertical wheel into a
/// horizontal scroll event before an application ever sees it, so the row
/// scroll was reading a `y` that the desktop had emptied, and twenty-one rows
/// stayed where they were.
///
/// Asserted as a **cross-platform equality** rather than as a shape, because
/// the shape is the easy half and the hard half is the sign: the same hand
/// movement has to queue the same report on both desktops, or the fix trades
/// a dead gesture for a backwards one. [`upright_wheel`]'s own doc says why
/// the swap is sign-preserving; this is that claim in a form that fails.
///
/// **And the platform is one of the facts** (0.4.2 release review, X-5). The
/// first landing of this rule ran it everywhere, which is why ⑥ is here and
/// why it is the half of the test that cannot be checked by using the
/// product: the desktop that needs the repair is not the desktop the
/// regression lands on. `platform_swaps_shift_wheel` is passed as a value for
/// exactly that reason, so both answers are reachable from one machine.
///
/// MUTATION: negate the copy in [`upright_wheel`] — `LineDelta(0.0, -x)`, the
/// shape "macOS must surely have flipped it too" would take — and ① goes red
/// on the equality rather than on the shape. Drop the `!shift` guard and ④
/// goes red: an ordinary tilt wheel and a trackpad's second finger start
/// scrolling the document up and down. Drop the `y == 0.0` guard and ⑤ goes
/// red, taking every diagonal trackpad flick with it. Drop the
/// `!platform_swaps_shift_wheel` guard — the shipped shape the review caught
/// — and ⑥ goes red alone, with every other numbered block still green.
#[test]
fn a_mac_reports_shift_wheel_sideways_and_this_window_stands_it_back_up() {
    use bt_term::{MouseTracking, TerminalModes};
    // The desktop, as the value [`upright_wheel`] takes it: the one that
    // performs AppKit's swap, and every other one.
    const MAC: bool = true;
    const ELSEWHERE: bool = false;
    let rows_of = |delta: MouseScrollDelta| match delta {
        MouseScrollDelta::LineDelta(_, y) => f64::from(y),
        MouseScrollDelta::PixelDelta(at) => at.y,
    };
    let alternate = TerminalModes {
        alternate_screen: true,
        alternate_scroll: true,
        sgr_mouse: true,
        mouse_tracking: MouseTracking::Off,
        focus_reporting: false,
        keyboard: bt_term::KeyboardProtocol::default(),
    };

    // ① The rewrite undone, and the equality that is the whole of the fix.
    let mac = upright_wheel(MouseScrollDelta::LineDelta(3.0, 0.0), true, MAC);
    let windows = upright_wheel(MouseScrollDelta::LineDelta(0.0, 3.0), true, MAC);
    assert_eq!(
        mac, windows,
        "one hand movement, one queued report, whichever desktop reported it"
    );
    assert_eq!(mac, MouseScrollDelta::LineDelta(0.0, 3.0));
    assert!(
        !wheel_points_sideways(mac),
        "and nothing downstream can still read it as a sideways gesture"
    );
    // The turn that goes back still goes back: a copy, never a negation.
    assert_eq!(
        upright_wheel(MouseScrollDelta::LineDelta(-3.0, 0.0), true, MAC),
        MouseScrollDelta::LineDelta(0.0, -3.0)
    );
    assert_eq!(
        upright_wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(-48.0, 0.0)),
            true,
            MAC
        ),
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -48.0)),
        "a trackpad's precise report is stood up by the same rule"
    );

    // ② The chip's own gesture, end to end: `Shift` on an alternate screen is
    // the local override, the pane has rows displaced above it, and the notch
    // is spent on those rows — with or without a second axis to be tempted by.
    assert_eq!(
        wheel_route(true, alternate, false),
        WheelRoute::Local,
        "Shift over a full-screen program is this window's view, as ever"
    );
    for has_column_axis in [false, true] {
        assert_eq!(
            wheel_axis(true, wheel_points_sideways(mac), true, has_column_axis),
            WheelAxis::Rows,
            "the axis the pane has to offer is the rows it displaced \
                 (column axis={has_column_axis})"
        );
    }
    assert_eq!(
        rows_of(mac),
        3.0,
        "and the rows move by the turn the hand made, not by nothing"
    );

    // ③ The same key over a long line on a pane with nothing displaced still
    // means sideways — the promise the wrapping setting's own line makes.
    assert_eq!(
        wheel_axis(true, wheel_points_sideways(mac), false, true),
        WheelAxis::Columns,
        "Shift+wheel still scrolls a long line sideways"
    );

    // ④ A report that points sideways on its own is untouched and unrouted:
    // a tilt wheel and a trackpad's second finger come without the key.
    let tilt = upright_wheel(MouseScrollDelta::LineDelta(3.0, 0.0), false, MAC);
    assert_eq!(
        tilt,
        MouseScrollDelta::LineDelta(3.0, 0.0),
        "no key, no rewrite — this hand really is going sideways"
    );
    assert!(wheel_points_sideways(tilt));
    for shift_only_rows in [false, true] {
        assert_eq!(
            wheel_axis(false, true, shift_only_rows, true),
            WheelAxis::Columns,
            "a sideways report is taken at its word whatever the pane holds"
        );
    }

    // ⑤ And everything else the key is held over passes through as it stands.
    // The diagonals are the ones that matter: a trackpad flick is never
    // exactly straight, and a rule that read "mostly sideways" would eat one.
    for untouched in [
        MouseScrollDelta::LineDelta(0.0, 3.0),
        MouseScrollDelta::LineDelta(3.0, 1.0),
        MouseScrollDelta::LineDelta(0.0, 0.0),
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 40.0)),
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(40.0, 2.0)),
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.0)),
    ] {
        assert_eq!(
            upright_wheel(untouched, true, MAC),
            untouched,
            "only a report with no vertical component at all is a rewrite"
        );
    }

    // ⑥ **And nowhere but a Mac is touched by any of it** (release review,
    // X-5). A tilt wheel and a trackpad's second finger are sold on every
    // desktop, and on the ones that do not perform AppKit's swap a hand
    // holding `Shift` over one of them is going sideways and means it. The
    // shipped rule rewrote exactly this report — the one shape no test then
    // held — and turned a reader's sideways gesture into a vertical one.
    for sideways in [
        MouseScrollDelta::LineDelta(3.0, 0.0),
        MouseScrollDelta::LineDelta(-3.0, 0.0),
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(48.0, 0.0)),
    ] {
        assert_eq!(
            upright_wheel(sideways, true, ELSEWHERE),
            sideways,
            "a desktop that never swapped the axes has nothing to undo"
        );
        assert!(
            wheel_points_sideways(upright_wheel(sideways, true, ELSEWHERE)),
            "and the gesture reaches the column arithmetic still sideways"
        );
        assert_eq!(
            wheel_zoom_notches(upright_wheel(sideways, true, ELSEWHERE)),
            0.0,
            "a sideways report carries no detent for the zoom chord to spend, \
                 so Ctrl+Shift over a page still scrolls nothing and zooms nothing"
        );
        assert_ne!(
            upright_wheel(sideways, true, MAC),
            sideways,
            "while the one desktop that does swap them still has it undone"
        );
    }
}

/// **The predicate the gesture turns on is the predicate the bar is drawn
/// by** — said as an equality, because two predicates is how a window ends up
/// scrolling sideways with nothing on screen to say that it did.
///
/// `column_bar` declines exactly when the axis has nowhere to go, and
/// [`wheel_axis`] is told the same fact by the same expression. This walks a
/// pane from "everything fits" to "one column over" and asserts the two flip
/// together.
#[test]
fn the_gesture_and_the_foot_bar_agree_about_whether_there_is_an_axis() {
    const BODY: [f32; 4] = [0.0, 0.0, 800.0, 600.0];
    const VIEWPORT: u32 = 80;
    for extent in [0, 1, VIEWPORT - 1, VIEWPORT, VIEWPORT + 1, 400] {
        let axis = bt_viewport::horizontal::HorizontalProjection::new(
            ContentColumn(extent),
            VIEWPORT,
            ContentColumn(0),
        );
        let gesture = wheel_axis(true, false, false, axis.max_x_origin().0 > 0);
        let bar = termscroll::column_bar(
            BODY,
            axis.content_extent().0,
            axis.viewport_columns(),
            0,
            1.0,
        );
        assert_eq!(
            gesture == WheelAxis::Columns,
            bar.is_some(),
            "an extent of {extent} columns behind a {VIEWPORT}-column pane"
        );
    }
}

/// **RED — one `Alt`+wheel over a card writes its entry, its rail decision,
/// its aim and its route into `BT_MOUSE_TRACE`** (T-WHEEL-TRACE, §7.60).
///
/// The gap this closes is the whole of why the resize report could not be
/// settled by reading: `mouse_wheel`, `rail_contains`, `scroll_rail` and
/// `aim_focus_card_window` wrote **nothing at all**, so a reader whose
/// `Alt`+wheel had stopped aiming could hand back a trace naming every click
/// they had made and not one notch.
///
/// **What a test in this file can hold, and what it cannot.** A `Runtime` is
/// a live window and a real GPU device — `WindowRenderer::new` is called at
/// exactly one place in this program, on the window-creation path — and
/// nothing in this workspace builds one headlessly. So this drives the
/// road's *arithmetic*: the burst two half reports merge into, `column_notch`
/// on a held `Alt`, the column the aim walks, the mini seat under the point,
/// and `CardAim::spend` — and writes what they produced through the very
/// builders the stations call, into a real trace at a temporary path. That
/// the stations call them **at every exit** is
/// [`super::mouse_trace_station_tests`]' half of the pair, and it is a pin
/// on this file's own text because no value in the program can answer "is
/// there a `return` here with nothing written beside it".
///
/// MUTATION: take any `key=` out of a builder and the assertion naming it
/// fails; give `CardAim::spend` `round` where it has `trunc` and the first
/// half detent stops being carried — `steps=0` becomes `steps=1` and the two
/// `card_skip` numbers on the first `wheel_aim` line part company.
#[test]
fn one_alt_wheel_over_a_card_writes_its_entry_its_rail_its_aim_and_its_route() {
    const HEADER: &str = "# BT_MOUSE_TRACE_V1 elapsed_ms event field=value…";

    let path = std::env::temp_dir().join(format!(
        "{}.log",
        bt_testpath::unique_name("bt-wheel-trace-alt-wheel")
    ));
    let _ = std::fs::remove_file(&path);
    let trace = trace::Trace::create(&path, HEADER);
    let write = |message: String| mouse_trace::emit(Some(&trace), || message);

    // A driver that speaks half a detent at a time — the shape a
    // high-resolution wheel and a precision touchpad both have. Two reports
    // merge into one burst exactly as `queue_wheel` merges them, and
    // `flush_wheel` spends the merged one.
    let half = MouseScrollDelta::LineDelta(0.0, 0.5);
    let flushed = WheelBurst::of(half)
        .plus(half)
        .expect("two reports in one currency merge")
        .delta();

    // The window it lands on: three cards on a 200% display, run to the end
    // of the list, the pointer on the last card's terminal mini — the very
    // fixture the scale test above aims with.
    let (_, scale) = CARDS_AT_200;
    let column = cards_column(CARDS_AT_200, 3, 0.0);
    let column = cards_column(CARDS_AT_200, 3, column.max_scroll);
    let mini = column.cards[2].mini;
    let point = [(mini[0] + mini[2]) / 2.0, mini[3] - 8.0 * scale];
    let tree = LayoutNode::seat(bt_layout::Seat::new(SeatId(1), SeatKind::Terminal));
    let seat = seats::focus_mini_seats(&tree, mini, scale)
        .into_iter()
        .find(|seat| {
            seat.kind == SeatKind::Terminal && seats::rect_holds(seat.rect, point[0], point[1])
        })
        .expect("the fixture aims at a terminal mini");
    let at = (f64::from(point[0]), f64::from(point[1]));

    // ① The entry: every number a resize can put out of step, read once.
    write(
        mouse_trace::WheelEntry {
            pointer: Some(at),
            pointer_last_seen: Some(at),
            swapchain: (560, 1000),
            inner: (560, 1000),
            metrics_scale: f64::from(scale),
            flushed,
            notches: wheel_zoom_notches(flushed),
            events: 2,
            routings: 1,
            alt: true,
            shift: false,
            ctrl: false,
            focus_mode: true,
        }
        .line(),
    );

    // ② The rail's decision, with the column as it is *painted* beside the
    // column the aim *walks*. The two heights differ here on purpose: this
    // is the exact shape of the disagreement §7.60 exists to make visible,
    // and `agree=0` is the one field that says so at a glance.
    let painted = cards_column((960.0, scale), 3, column.max_scroll);
    assert_ne!(
        column, painted,
        "the fixture's two heights really do solve two different columns, \
             which is what makes the `agree` assertion below mean anything"
    );
    write(
        mouse_trace::WheelRail {
            contains: true,
            point: at,
            rail_scroll: column.max_scroll,
            strip_rail: None,
            aim_height: CARDS_AT_200.0,
            aim: Some(&column),
            paint_height: 960.0,
            paint: Some(&painted),
        }
        .line(),
    );

    // ③ The aim. `Alt` under the hand is what makes the notch an aim at all.
    assert_eq!(
        column_notch(ModifiersState::ALT),
        ColumnNotch::Aim,
        "the fixture holds the modifier the column reads"
    );
    let target = LeafId {
        tab: TabId(1),
        seat: seat.id,
    };
    let mut carried: Option<CardAim> = None;
    let aim_line = |index: usize,
                    before: Option<CardAim>,
                    after: Option<CardAim>,
                    steps: i32,
                    skip_before: usize,
                    skip_after: usize| {
        mouse_trace::WheelAim {
            index,
            tab: format!("{:?}", TabId(1)),
            seat: format!("{:?}", seat.id),
            at_before: before.map(|held| format!("{:?}/{:?}", held.at.tab, held.at.seat)),
            carried_before: before.map(|held| held.carried.delta()),
            steps,
            carried_after: after.map(|held| held.carried.delta()),
            skip_before,
            skip_after,
        }
        .line()
    };

    // Half a detent is half a row, and half a row is not a row: it is
    // carried, and the window under the pointer does not move.
    let before = carried;
    let steps = CardAim::spend(&mut carried, target, half);
    assert_eq!(steps, 0, "half a detent moves no row");
    write(aim_line(2, before, carried, steps, 0, 0));

    // The other half completes it, **because the carry is filed under this
    // seat's own identity** — which is the fact the stale-target reading of
    // the report turns on.
    let before = carried;
    let steps = CardAim::spend(&mut carried, target, half);
    assert_eq!(steps, 1, "two halves at the same seat are one whole detent");
    write(aim_line(2, before, carried, steps, 0, 1));

    // ④ And the one word that says which surface took it home.
    write("wheel_route taken=rail-aim".to_owned());

    let written = std::fs::read_to_string(&path).expect("the trace was written");
    let lines: Vec<&str> = written.lines().collect();
    assert_eq!(lines[0], HEADER, "the run names its own format first");
    // A `fn` and not a closure: what it hands back is borrowed from the file
    // and not from its own argument, which is the one shape a closure's
    // inferred signature cannot spell.
    fn only<'a>(lines: &[&'a str], written: &str, event: &str) -> &'a str {
        let mut found = lines.iter().filter(|line| line.contains(event));
        let first = found
            .next()
            .unwrap_or_else(|| panic!("`{event}` is in the trace:\n{written}"));
        assert!(
            found.next().is_none(),
            "`{event}` is written once:\n{written}"
        );
        first
    }

    let entry = only(&lines, &written, "mouse_wheel ");
    for key in [
        "flushed_delta=lines:0,1",
        "notches=1",
        "metrics_scale=2",
        "swapchain_size=560x1000",
        "inner_size=560x1000",
        "alt=1",
        "shift=0",
        "ctrl=0",
        "focus_mode=1",
        "events=2",
        "routings=1",
    ] {
        assert!(entry.contains(key), "the entry carries `{key}`: {entry}");
    }
    assert!(
        !entry.contains("pointer=none") && !entry.contains("pointer_last_seen=none"),
        "and both readings of the pointer, because a window whose hand has \
             not moved since a resize is told apart by exactly those two: {entry}"
    );

    let rail = only(&lines, &written, "wheel_rail ");
    for key in [
        "contains=1",
        "rail_run=none",
        "aim_height=1000",
        "aim_cards=3",
        "aim_viewport=",
        "aim_body=",
        "aim_max_scroll=",
        "paint_height=960",
        "paint_cards=3",
        "paint_viewport=",
        "agree=0",
    ] {
        assert!(rail.contains(key), "the rail line carries `{key}`: {rail}");
    }

    let aims: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| line.contains("wheel_aim "))
        .collect();
    assert_eq!(aims.len(), 2, "one line per spend:\n{written}");
    for key in [
        "leave=carried",
        "index=2",
        "tab=TabId(1)",
        "at_before=none",
        "carried_before=none",
        "steps=0",
        "carried_after=lines:0,0.5",
        "card_skip_before=0",
        "card_skip_after=0",
    ] {
        assert!(
            aims[0].contains(key),
            "the carried notch carries `{key}`: {}",
            aims[0]
        );
    }
    for key in [
        "leave=aimed",
        "at_before=TabId(1)/",
        "carried_before=lines:0,0.5",
        "steps=1",
        "carried_after=lines:0,0",
        "card_skip_before=0",
        "card_skip_after=1",
    ] {
        assert!(
            aims[1].contains(key),
            "the notch that completed a detent carries `{key}`: {}",
            aims[1]
        );
    }
    assert!(
        only(&lines, &written, "wheel_route ").ends_with("taken=rail-aim"),
        "and the road ends in one word:\n{written}"
    );

    let _ = std::fs::remove_file(&path);
}

/// PIN — **one clicking rule, two kinds of reference** (§7.1.5g, user ruling
/// 2026-08-20).
///
/// The reported inconsistency: an inline image reference opened its preview
/// on a plain click while an OSC 8 hyperlink needed `Ctrl` and then *left* —
/// the same terminal answering the same gesture two ways. The fix is not two
/// corrected `if control`s but one [`ClickIntent`] both arms read, and this
/// test is what says so: the picture and the link are asked the same
/// question and their answers move together.
///
/// MUTATION: give either arm its own `if control` back, invert it, and the
/// pairing below goes red while each arm on its own still looks sensible —
/// which is exactly how the two rules drifted apart the first time.
#[test]
fn a_plain_click_stays_in_this_window_and_ctrl_hands_it_to_the_system() {
    let picture = Path::new(r"C:\shots\img0.jpg");
    for (control, intent) in [(false, ClickIntent::Here), (true, ClickIntent::System)] {
        assert_eq!(ClickIntent::of(control), intent);
        let link = hyperlink_activation(
            control,
            true,
            "file:///C:/notes/plan.md",
            bt_transcript::paths::PathNamer::ThisWindow,
            &|_| Some(a_local_file()),
        );
        let reference = local_image_activation(control, true, Some(picture));
        let (link_stays, reference_stays) = (
            matches!(link, HyperlinkActivation::Preview(_, None)),
            matches!(reference, LocalImageActivation::Preview(_)),
        );
        let (link_leaves, reference_leaves) = (
            matches!(link, HyperlinkActivation::External(_)),
            matches!(reference, LocalImageActivation::External(_)),
        );
        assert_eq!(
            (link_stays, link_leaves),
            (reference_stays, reference_leaves),
            "a link and a picture answer one rule: Ctrl {control}"
        );
        assert_eq!(
            link_stays,
            intent == ClickIntent::Here,
            "and that rule is the intent: Ctrl {control}"
        );
    }
    // The picture arm's two rows, unchanged by the link's arriving beside it.
    assert_eq!(
        local_image_activation(false, true, Some(picture)),
        LocalImageActivation::Preview(picture.to_path_buf())
    );
    assert_eq!(
        local_image_activation(true, true, Some(picture)),
        LocalImageActivation::External(picture.to_path_buf())
    );
}

#[test]
fn hyperlink_hover_delay_and_departure_are_event_driven() {
    let start = Instant::now();
    let link = hyperlink_hit("file:///actual-target");
    let mut hover = HyperlinkHover::default();

    // The underline is the immediate affordance: a fresh candidate republishes right away and
    // is the underline target long before the tooltip deadline; only the status text waits.
    assert!(hover.observe(Some(link.clone()), start));
    assert_eq!(hover.underline_target(), Some(&link));
    assert!(hover.active.is_none(), "tooltip must not appear instantly");
    assert!(!hover.activate_if_due(
        start + Duration::from_millis(299),
        bt_transcript::paths::PathNamer::ThisWindow,
        &no_directories
    ));
    assert!(hover.activate_if_due(
        start + Duration::from_millis(300),
        bt_transcript::paths::PathNamer::ThisWindow,
        &no_directories
    ));
    assert_eq!(
        hover.status_text(80).as_deref(),
        Some("file:///actual-target")
    );
    assert!(hover.observe(None, start + Duration::from_millis(301)));
    assert!(hover.active.is_none());
    assert!(hover.underline_target().is_none());
    assert!(hover.show_at.is_none());

    hover.show_blocked(link);
    assert_eq!(
        hover.status_text(80).as_deref(),
        Some("file:///actual-target · blocked")
    );
    assert_eq!(
        hover.status_text(20).as_deref(),
        Some("file:///a… · blocked"),
        "narrow chrome keeps the real target prefix and the blocked verdict visible"
    );
}

/// RED (gesture audit 2026-08-26, 丙3) — **the hover line says where `Ctrl`
/// would send a printed local path.**
///
/// The audit's finding, in its own words: over a readable file or folder,
/// `terminal_link_answers_a_press` answers yes *with or without* `Ctrl`, so
/// the finger cursor is identical either way and 「`Ctrl` 只是静悄悄换了目的
/// 地」. The status line was already being drawn under that same pointer and
/// was printing the address and nothing else. This is the clause that makes
/// the second destination visible, and the reason it is a clause and not a
/// new surface is that the surface was already there.
///
/// A **web** address used to be deliberately silent here, on the ground
/// that `Ctrl` lit its finger and bare clicks did not. That ground went
/// away on 2026-08-29 — a plain click now opens the address on the seat —
/// so the row takes the clause for the reason the file row has it: two
/// destinations under one unchanging finger. It is the file row's own
/// clause, because `Ctrl` reaches the same call, this machine's registered
/// handler for the address.
///
/// MUTATIONS: ① fold the folder into the file's sentence and the second
/// assertion goes red — a folder does not open in a default app, it is
/// shown in Explorer; ② print the aside unconditionally and the narrow
/// assertion goes red with the address truncated to fit a lesson.
#[test]
fn the_hover_line_says_where_control_would_send_a_local_path() {
    fn settled(uri: &str, directory: bool) -> HyperlinkHover {
        let start = Instant::now();
        let mut hover = HyperlinkHover::default();
        hover.observe(Some(hyperlink_hit(uri)), start);
        assert!(hover.activate_if_due(
            start + Duration::from_millis(300),
            bt_transcript::paths::PathNamer::ThisWindow,
            &|_: &Path| Some(if directory {
                a_local_folder()
            } else {
                a_local_file()
            })
        ));
        hover
    }

    assert_eq!(
        settled("file:///C:/notes/readme.md", false)
            .status_text(120)
            .as_deref(),
        Some("file:///C:/notes/readme.md · Ctrl+click opens in default app")
    );
    assert_eq!(
        settled("file:///C:/notes", true)
            .status_text(120)
            .as_deref(),
        Some("file:///C:/notes · Ctrl+click shows it in Explorer"),
        "a folder is shown in Explorer and the line says the verb that runs"
    );
    assert_eq!(
        settled("https://example.test/page", false)
            .status_text(120)
            .as_deref(),
        Some("https://example.test/page · Ctrl+click opens in default app"),
        "a web address already lights its own finger under Ctrl"
    );
    assert_eq!(
        settled("file:///C:/notes/readme.md", false)
            .status_text(30)
            .as_deref(),
        Some("file:///C:/notes/readme.md"),
        "an address cut short to make room for an aside is the wrong trade"
    );
    // And a refusal is an answer to the very press the aside was offering,
    // so the two are never printed together.
    let mut refused = settled("file:///C:/notes/readme.md", false);
    refused.show_blocked(hyperlink_hit("file:///C:/notes/readme.md"));
    assert_eq!(
        refused.status_text(120).as_deref(),
        Some("file:///C:/notes/readme.md · blocked")
    );
}

/// RED (user report, 2026-09-05) — **this line's budget is counted in grid
/// cells, in every language.**
///
/// 「 · Ctrl+点击在资源管理器中显示」 is fifteen characters and twenty-six
/// cells. Counted as characters, the aside was printed onto a grid that had
/// no room for it, and the row that draws the line then dropped whatever ran
/// off the left — the head of the address, which is the one fact this line
/// exists to carry. The cell-shrinking half of the same defect is
/// `bt_render`'s `a_translated_status_line_is_laid_out_in_cells_and_not_in_characters`.
///
/// MUTATIONS: ① count the aside in characters and the 40-column case takes
/// an aside eleven cells too wide for it; ② count the address in characters
/// and the truncated Chinese path comes back over budget; ③ cut the address
/// at a byte or a character rather than a cluster and the ellipsis lands
/// after half a glyph.
#[test]
fn the_hover_line_spends_grid_cells_and_not_characters() {
    fn settled(uri: &str, directory: bool) -> HyperlinkHover {
        let start = Instant::now();
        let mut hover = HyperlinkHover::default();
        hover.observe(Some(hyperlink_hit(uri)), start);
        assert!(hover.activate_if_due(
            start + Duration::from_millis(300),
            bt_transcript::paths::PathNamer::ThisWindow,
            &|_: &Path| Some(if directory {
                a_local_folder()
            } else {
                a_local_file()
            })
        ));
        hover
    }
    let cells = |line: &str| bt_unicode::text_width(line);

    // ① The reported hover, in the language it was reported in. A grid wide
    // enough for the address and the aside prints both, and the whole line
    // fits the grid it was measured against.
    let folder = settled("file:///D:/Demo", true);
    let wide = folder
        .status_text_in(60, i18n::Lang::Chinese)
        .expect("a settled hover has a line");
    assert_eq!(wide, "file:///D:/Demo · Ctrl+点击在资源管理器中显示");
    assert!(
        cells(&wide) <= 60,
        "{} cells on a 60-column grid",
        cells(&wide)
    );

    // ② One column short of that, the aside goes whole rather than eating
    // the head of the address. The nineteen *characters* of the same aside
    // would have fit, which is the whole of the reported defect.
    assert_eq!(cells(&wide), 45);
    assert_eq!(wide.chars().count(), 34);
    assert_eq!(
        folder.status_text_in(44, i18n::Lang::Chinese).as_deref(),
        Some("file:///D:/Demo"),
        "an address cut short to make room for an aside is the wrong trade"
    );
    // The English column of the same table is its own width, measured the
    // same way.
    assert_eq!(
        folder.status_text_in(49, i18n::Lang::English).as_deref(),
        Some("file:///D:/Demo · Ctrl+click shows it in Explorer")
    );
    assert_eq!(
        folder.status_text_in(48, i18n::Lang::English).as_deref(),
        Some("file:///D:/Demo")
    );

    // ③ An address of ideographs is cut to cells and at a cluster boundary,
    // ellipsis included.
    let deep = settled("file:///D:/文档/项目/笔记/读我.md", false);
    let cut = deep
        .status_text_in(20, i18n::Lang::Chinese)
        .expect("a settled hover has a line");
    assert!(cells(&cut) <= 20, "{cut:?} is {} cells wide", cells(&cut));
    assert_eq!(cut, "file:///D:/文档/项…");

    // ④ The verdict is still printed on a grid too narrow for anything else,
    // and it too is measured in cells.
    let mut refused = settled("file:///D:/Demo", true);
    refused.show_blocked(hyperlink_hit("file:///D:/Demo"));
    assert_eq!(
        refused.status_text_in(3, i18n::Lang::Chinese).as_deref(),
        Some("已"),
        "three cells hold one and a half ideographs, so they hold one"
    );
    assert_eq!(
        refused.status_text_in(60, i18n::Lang::Chinese).as_deref(),
        Some("file:///D:/Demo · 已拦截")
    );
}

#[test]
fn unavailable_system_wheel_setting_uses_the_windows_default() {
    assert_eq!(
        recoverable_wheel_scroll_amount(Err("injected SPI failure".to_owned())),
        bt_platform::WheelScrollAmount::Lines(3)
    );
}

#[test]
fn wheel_accumulator_preserves_fractional_residue_across_events() {
    // Eight trackpad ticks of 0.375 units (binary-exact) must add up to exactly 3 whole
    // units drained, never 0 (per-event truncation) and never 4 (double counting).
    let mut remainder = 0.0;
    let mut drained = 0;
    for _ in 0..8 {
        remainder += 0.375;
        drained += drain_whole_units(&mut remainder, 1.0);
    }
    assert_eq!(drained, 3);
    assert_eq!(remainder, 0.0);

    // Whole-line wheel notches with a 17px cell drain exactly one line per 17px, residue 8.
    let mut pixels = 25.0;
    assert_eq!(drain_whole_units(&mut pixels, 17.0), 1);
    assert!((pixels - 8.0).abs() < 1e-9);
}

/// PIN — a run of notches inside one turn of the loop is **one** notch, and
/// it is the same notch arithmetically.
///
/// The coalescing this pins is worth having only if it changes nothing but
/// the count: a burst that scrolled a different distance from the notches
/// that made it would be a wheel that behaves differently on a busy machine,
/// which is the opposite of the point. So the two halves are asserted
/// together — the burst is one delta, and that delta is the sum.
#[test]
fn wheel_burst_collapses_a_run_of_notches_without_changing_the_distance() {
    let mut burst = WheelBurst::of(MouseScrollDelta::LineDelta(0.0, -1.0));
    for _ in 0..4 {
        burst = burst
            .plus(MouseScrollDelta::LineDelta(0.0, -1.0))
            .expect("a line delta merges into a line burst");
    }
    assert_eq!(
        burst.delta(),
        MouseScrollDelta::LineDelta(0.0, -5.0),
        "five detents in one turn are five lines, spent once"
    );

    // The same for the currency a trackpad speaks, on both axes: a tilt
    // wheel's horizontal travel is summed beside the vertical, not dropped.
    let mut pixels = WheelBurst::of(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
        3.0, -12.5,
    )));
    pixels = pixels
        .plus(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
            -1.0, -7.5,
        )))
        .expect("a pixel delta merges into a pixel burst");
    assert_eq!(
        pixels.delta(),
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(2.0, -20.0))
    );

    // **The two currencies do not add.** A device that changes its mind
    // mid-burst is told to spend what is held first, rather than have its
    // lines quietly reinterpreted as pixels.
    assert_eq!(
        WheelBurst::of(MouseScrollDelta::LineDelta(0.0, -1.0)).plus(MouseScrollDelta::PixelDelta(
            PhysicalPosition::new(0.0, -1.0)
        )),
        None
    );
    assert_eq!(
        WheelBurst::of(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
            0.0, -1.0
        )))
        .plus(MouseScrollDelta::LineDelta(0.0, -1.0)),
        None
    );
}

#[test]
fn wheel_flush_that_moves_the_view_publishes_exactly_one_frame() {
    let mut pane = wheel_pane_at_top();
    let before = pane.last_presented.clone().unwrap();
    assert!(flush_test_wheel(&mut pane, -1.0));
    assert_eq!(pane.publications, 1);
    let after = pane.pending.pending_frame().unwrap();
    assert_ne!(before.viewport_origin, after.viewport_origin);
    assert!(pane.present_pending());
    assert!(!pane.present_pending());
}

#[test]
fn wheel_flush_at_a_clamp_still_publishes_changed_content() {
    let mut pane = wheel_pane_at_top();
    // New live cells still belong to the frame at the bottom clamp.
    pane.session.feed(b"\x1b[3J\x1b[2J\x1b[Hnew").unwrap();
    pane.publish_expose_frame();
    pane.present_pending();
    pane.publications = 0;
    pane.session.feed(b" content").unwrap();
    assert!(!flush_test_wheel(&mut pane, -1.0));
    assert_eq!(pane.publications, 1);
}

#[test]
fn wheel_accumulator_truncates_symmetrically_and_never_flips_sign_on_reversal() {
    // +0.6 then -0.7: neither direction has accrued a whole unit, so nothing drains and the
    // residue nets out — reversal must not manufacture a step from opposite-sign residue.
    let mut remainder = 0.0;
    remainder += 0.6;
    assert_eq!(drain_whole_units(&mut remainder, 1.0), 0);
    remainder += -0.7;
    assert_eq!(drain_whole_units(&mut remainder, 1.0), 0);
    assert!((remainder + 0.1).abs() < 1e-9);

    // A full downward unit drains as -1 with the same magnitude rules as upward.
    let mut down = -1.4;
    assert_eq!(drain_whole_units(&mut down, 1.0), -1);
    assert!((down + 0.4).abs() < 1e-9);
}

/// RED (0.4.4 ticket 11) — **a pan enters the wheel road as pixels, and a finger moving down
/// scrolls back into the history.**
///
/// The producer is the real one: `bt_platform::PanTrack`, the arithmetic the Windows touch door
/// runs on every `GID_PAN` message, fed a synthetic flick — a begin, a finger moving down, and
/// the system's inertia after it lifted. Its steps go through [`pan_on_the_wheel_road`], merge in
/// a [`WheelBurst`] as [`Runtime::queue_wheel`] merges them, and the burst is spent on a real
/// terminal pane with the arithmetic the local wheel route uses on a `PixelDelta`
/// (`event_subpixels = y × SUBPIXELS_PER_PX`, then `scroll_by_subpixels`). The opening step puts
/// the pointer where the pan went down and turns no wheel; every later step is travel, one pixel
/// for one pixel, in the currency a precision touchpad speaks — so the content follows the
/// finger, which is the sign a trackpad already has on this road.
///
/// MUTATION: negate `y` in `pan_on_the_wheel_road` and the burst reads -46 and the pane does not
/// move off the live bottom; drop the opening point and the first assertion goes red.
#[test]
fn a_pan_enters_the_wheel_road_as_pixels() {
    let mut track = bt_platform::PanTrack::default();
    let window_origin = (300, 200);
    let records = [
        (true, false, (640, 500)),
        (false, false, (640, 510)),
        (false, false, (641, 530)),
        (false, false, (641, 540)),
        (false, false, (641, 546)),
        (false, true, (641, 546)),
    ];
    let steps: Vec<_> = records
        .iter()
        .filter_map(|&(begins, ends, (x, y))| {
            track.step(
                begins,
                ends,
                (x, y),
                (x - window_origin.0, y - window_origin.1),
            )
        })
        .collect();
    let entries: Vec<_> = steps.into_iter().map(pan_on_the_wheel_road).collect();
    assert_eq!(
        entries[0],
        (Some(PhysicalPosition::new(340.0, 300.0)), None),
        "a pan opens by putting the pointer where it went down, in client pixels, and turns no \
         wheel: the first pan message performs no panning"
    );
    let mut burst: Option<WheelBurst> = None;
    for (pointer, wheel) in &entries[1..] {
        assert_eq!(
            *pointer, None,
            "the pointer is put down once per pan, not followed"
        );
        let delta = wheel.expect("every later step of this flick moved");
        assert!(
            matches!(delta, MouseScrollDelta::PixelDelta(_)),
            "a pan is travel in pixels, never lines: {delta:?}"
        );
        burst = Some(match burst {
            None => WheelBurst::of(delta),
            Some(held) => held.plus(delta).expect("pixels merge into a pixel burst"),
        });
    }
    let travel = burst.expect("the flick moved").delta();
    assert_eq!(
        travel,
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(1.0, 46.0)),
        "the whole flick, inertia included, is its last point minus its first, and a finger \
         moving down is positive y — the wheel road's travel back up the document"
    );

    let mut pane = PtyPresentationHarness::new(20, 3);
    pane.feed_drain(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven");
    pane.publish_expose_frame();
    pane.present_pending();
    assert_eq!(
        pane.projection.scroll_offset_subpixels(),
        0,
        "the pane starts live"
    );
    let MouseScrollDelta::PixelDelta(position) = travel else {
        unreachable!("asserted above");
    };
    let mut remainder = position.y * bt_viewport::SUBPIXELS_PER_PX as f64;
    pane.projection
        .scroll_by_subpixels(drain_whole_units(&mut remainder, 1.0));
    assert!(
        pane.projection.scroll_offset_subpixels() > 0,
        "a finger sliding down pulls the history into view, as the content follows the finger"
    );
}

/// RED (0.4.4 ticket 11) — **a parked pan is spent through the wheel's own entrance, and the
/// wake that carries it is answered once.**
///
/// Ticket 11's second blocking criterion is a second scroll road beside the wheel's. The runtime
/// half of the pan needs a window and a GPU, so this pins its shape by reading its items through
/// `bt-source`: the method that spends parked pans moves the pointer through `pointer_moved` and
/// turns the wheel through `queue_wheel` — the two doors a mouse uses — and scrolls nothing
/// itself; and the wake the touch door sends is answered in one place, by that method, in every
/// window.
///
/// MUTATION: spend the travel with `scroll_view_exact_in` instead of `queue_wheel` and the first
/// assertion goes red; answer `TouchPanned` with `Ok(())` and the last one does.
#[test]
fn a_parked_pan_is_spent_through_the_wheels_own_entrance() {
    let spend = squeezed_body("Runtime", "spend_parked_pans");
    assert!(
        spend.contains("self.queue_wheel(delta)?"),
        "the travel enters the wheel road where a notch does"
    );
    assert!(
        spend.contains("self.flush_wheel()?;self.pointer_moved(position)?;"),
        "the opening point is a pointer move, after what the wheel already held is spent"
    );
    assert!(
        !spend.contains("scroll"),
        "and nothing here scrolls anything itself: {spend}"
    );
    let answers = found(
        Needle::new(Pattern::text(concat!("AppEvent::TouchPanned", " =>"))),
        View::Raw,
    );
    assert_eq!(answers.len(), 1, "the pan wake is answered once");
    assert!(
        source().union()[answers.spans()[0].start()..].starts_with(concat!(
            "AppEvent::TouchPanned",
            " => self.for_each_window(|runtime| runtime.spend_parked_pans()),"
        )),
        "and it is answered by spending every window's parked pans"
    );
}

/// RED — **a button coming up is answered even after the pointer has left
/// the window** (user report, 2026-09-10: "a zoomed picture pans left and
/// right but not up and down").
///
/// A pan is put down inside `chrome_mouse_input`, and so is every other
/// gesture this window holds; all of them are held on purpose while the hand
/// works outside their own box. [`Runtime::mouse_input`] reached that router
/// only through the live pointer, which winit clears on `CursorLeft` — so a
/// release that arrived after the hand had left the window ended nothing,
/// the picture stayed in the hand, and the reader's next deliberate drag
/// began from a gesture that had never finished.
///
/// **Why it read as one axis and not as both.** A preview pane fills the
/// window's height, so a pan upwards or downwards leaves the window through
/// the top or bottom edge within a stroke or two, while a pan sideways
/// travels into the terminal pane beside it and never leaves at all. Up and
/// down was therefore the axis whose releases went missing. The arithmetic
/// is the same on both axes and always was —
/// [`a_picture_taller_than_its_body_is_carried_up_and_down`] is that half.
///
/// MUTATION: answer `live` for both arms and the first case fails.
#[test]
fn a_release_reaches_the_router_after_the_pointer_has_left() {
    let gone = None;
    let seen = Some(PhysicalPosition::new(1040.0, 620.0));
    let here = Some(PhysicalPosition::new(880.0, 410.0));
    assert_eq!(
        button_router_position(ElementState::Released, gone, seen),
        seen,
        "a release the hand has already carried out of the window is \
             answered from where the hand was last seen, so the gesture it \
             ends is ended"
    );
    assert_eq!(
        button_router_position(ElementState::Pressed, gone, seen),
        None,
        "a press is not: it asks where the hand is, and a window the \
             pointer has left has no answer"
    );
    for state in [ElementState::Pressed, ElementState::Released] {
        assert_eq!(
            button_router_position(state, here, seen),
            here,
            "and while the hand is in the window both answer from it, so \
                 the memory is a fallback and never a second opinion"
        );
    }
    assert_eq!(
        button_router_position(ElementState::Released, gone, None),
        None,
        "a window the pointer has never been in answers nothing at all"
    );
}

#[test]
fn a_wheel_event_is_counted_in_detents_however_the_driver_reports_it() {
    assert_eq!(
        wheel_zoom_notches(MouseScrollDelta::LineDelta(0.0, 1.0)),
        1.0
    );
    assert_eq!(
        wheel_zoom_notches(MouseScrollDelta::LineDelta(0.0, -2.0)),
        -2.0
    );
    assert_eq!(
        wheel_zoom_notches(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
            0.0, 120.0
        ))),
        1.0
    );
}

/// RED GATE (丙2, the gesture audit of 2026-08-26) — **a right press on a
/// tab raises that tab's menu, and moves nothing.**
///
/// Two claims, and the second is the one the whole menu rests on. A right
/// press is a way of *asking about* a tab, not of choosing one: the menu's
/// subject is the tab under the pointer whichever tab is in front, and a
/// press that activated the tab on its way would tear the reader's view away
/// from what they were doing in order to answer a question about somewhere
/// else. `TermMenuState::seat` states the same ruling one surface over,
/// where it reads "a right press does not move the focus
/// (`chrome_mouse_input` answers only the left button)".
///
/// **What makes it true is a structure rather than a promise**, and that is
/// what is read here: the opener sits in `mouse_input`, above the chrome
/// router, and returns; the chrome router — which owns `press_tab`,
/// `activate_tab` and the click chain — takes nothing but `Left` and
/// `Middle`. So there is no arrangement of these two functions in which a
/// right press reaches an activation.
///
/// It is read off the source because the alternative does not exist: raising
/// this menu needs a `Runtime`, and a `Runtime` needs a GPU device, a
/// swapchain and a live window. `a_tab_switch_leaves_no_menu_standing` reads
/// the source for exactly that reason, in exactly this shape.
///
/// MUTATIONS that must turn it red:
/// ① route the opener through `press_tab`, or add an `activate_tab` beside
///    it — the second assertion names it;
/// ② let `chrome_mouse_input` fall through on the right button — the third;
/// ③ move the opener below the chrome router, where the strip would have
///    already answered — the fourth.
#[test]
fn a_right_press_on_a_tab_raises_its_menu_and_leaves_the_active_tab_alone() {
    let router = method_body("Runtime", "mouse_input");
    let opener = router
        .find("self.open_tab_menu_at(tab, position)?;")
        .expect("a right press on a tab raises the tab's own menu");
    // ① it is a right press on a tab, and nothing else opens it.
    let arm = &router[router[..opener]
        .rfind("if state == ElementState::Pressed")
        .expect("the opener is guarded by a press")..opener];
    assert!(
        arm.contains("button == MouseButton::Right"),
        "the tab menu is raised by a right press: {arm}"
    );
    assert!(
        arm.contains("seats::ChromeTarget::Tab(index)"),
        "on the target the tab list answers with — the one `chrome_target_at` \
             raises from the strip, the rail and the focus column alike: {arm}"
    );
    // ② and the arm does nothing else.
    for moved in ["activate_tab", "press_tab", "focus", "tab_clicks"] {
        assert!(
            !arm.contains(moved),
            "the arm that raises a tab's menu also calls `{moved}` — a right \
                 press asks about a tab and must not choose one"
        );
    }
    // ③ the router that owns activation never sees the right button at all.
    let chrome = method_body("Runtime", "chrome_mouse_input");
    assert!(
        chrome
            .contains("if button != MouseButton::Left {\n            return Ok(false);\n        }"),
        "`chrome_mouse_input` still turns every non-left button away before \
             it reaches `press_tab`"
    );
    // ④ and the opener runs before that router is ever asked.
    let router_call = router
        .find("self.chrome_mouse_input(state, button, position)?")
        .expect("`mouse_input` hands presses to the chrome router");
    assert!(
        opener < router_call,
        "the tab menu's opener stands above the chrome router, where an open \
             rail lies over the panes (Q179) and the tab list is what
             `chrome_target_at` answers first"
    );
}

/// **PIN (user report, 2026-08-23): a pane being carried to another tab has
/// to be able to read the rail's names.**
///
/// Under `Sidebar: Icons` the rail rests as a 46px strip of icons. Its rows
/// are already drop targets in that state — [`Runtime::tab_run`] hands the
/// survey whatever the rail currently measures, so nothing about the landing
/// needed writing — but a parked row *is* an icon and nothing else, and a
/// hand cannot tell which tab it is about to hand the pane to. The rail's own
/// zone is the answer that already existed: reach the left edge and the panel
/// rolls out over the terminal, names and all.
///
/// **What was broken was the door, not the zone.** `pointer_moved` gives the
/// pointer to whichever gesture is in flight — "a gesture in flight is not a
/// hover" — and [`Runtime::drive_drag`] answers `true` for every move of a
/// drag, so everything below it went unasked for the whole gesture:
/// `update_chrome_hover`, and the [`Runtime::drive_rail_zone`] inside it.
/// The rail therefore stayed parked at exactly the moment its names were
/// wanted.
///
/// **The zone is not a hover, and that is why it belongs above the returns**
/// — beside the two questions that already carry this argument in this very
/// function. `drive_web_pointer` and `observe_chevrons` are both facts about
/// *where the pointer is*, and a fact about where the pointer is does not
/// stop being true because something is being carried. The mock-up says the
/// same by construction: `evalRailZone` is a `document`-level `pointermove`
/// listener, and neither it nor `railBusy` has a drag clause in it — so
/// dragging across the left edge rolls the panel out there too, and its
/// `stripEl()` then hands the drop engine the wider rectangle.
///
/// Read as **text**, for
/// [`both_pointer_doors_tell_the_chevron_clocks_where_the_hand_is`]' reason
/// and only that one: what went wrong is *an asker that never runs*, and no
/// state machine can be driven into a state nobody ever puts it in. What the
/// answer buys is then asserted against the geometry itself.
///
/// Red gate: put the call back under `drive_drag` — which is where it was —
/// or delete it, and the ordering assertions fail by name.
#[test]
fn the_rail_zone_is_asked_before_a_gesture_can_swallow_the_move() {
    let body = method_body("Runtime", "pointer_moved");

    let asked = body
        .find("self.drive_rail_zone(Some(position));")
        .expect("the move door asks the rail's zone where the hand is");
    let swallowed = body
        .find("if self.drive_drag(position)? {")
        .expect("a drag in flight consumes the move");
    assert!(
        asked < swallowed,
        "the zone is asked before a drag swallows the move — otherwise the \
             icon rail stays parked for the whole gesture, which is precisely \
             when a hand needs to read its names (user report 2026-08-23)"
    );
    let first_return = body
        .find("return Ok(());")
        .expect("some branch of this function owns the pointer outright");
    assert!(
        asked < first_return,
        "and before *every* branch that owns the pointer, not merely the \
             drag: a divider, a float and a selection all travel across the \
             left edge too, and the rail's zone is a question about where the \
             pointer is rather than a hover"
    );

    // What the open panel buys, and the whole of why the report is a bug:
    // the row grows a run of title to be read by. The rows are hit in both
    // states — this is about telling them apart, not about reaching them.
    let icon_rail = |open: f32| seats::RailState {
        layout: seats::TabLayoutMode::Vertical,
        mode: seats::RailMode::Icons,
        open,
        text_opacity: open,
        ..seats::RailState::default()
    };
    let trailers = vec![seats::TabTrailer::default(); 3];
    let title_run = |open: f32| {
        let geometry = seats::rail_geometry(
            600.0,
            1.0,
            seats::FOLIO_BAR,
            &trailers,
            0,
            0.0,
            icon_rail(open),
        )
        .expect("an icon rail is on screen in a vertical layout");
        let title = geometry.tabs[0].title;
        title[1] - title[0]
    };
    assert!(
        title_run(1.0) > title_run(0.0) + 100.0,
        "an opened icon rail gives every row a name to aim at ({} against \
             {} logical px)",
        title_run(1.0),
        title_run(0.0)
    );
    // And the stage does not move while it opens (Q179): the panel is an
    // overlay, so the coordinates the hand is aiming with do not jump under
    // it at the moment the names arrive. This is what makes opening the rail
    // mid-drag safe at all.
    assert_eq!(
        icon_rail(0.0).terminal_inset_logical_px(),
        icon_rail(1.0).terminal_inset_logical_px(),
        "the panel opens over the terminal, so nothing being aimed at moves"
    );
}

#[test]
fn anchored_mouse_forwarding_uses_live_viewport_rows_and_clamps_frozen_rows() {
    for mouse in [
        UserInputKind::MouseButton,
        UserInputKind::MouseWheel,
        UserInputKind::MouseMotion,
    ] {
        assert!(!mouse.returns_view_to_live(), "{mouse:?}");
    }
    assert!(UserInputKind::Keyboard.returns_view_to_live());

    // The other question this enum answers, and the one member that answers it differently.
    // A pointer sweeping across a pane is not a reply to a program waiting for one, and the
    // shape of that fact is a `None` nobody can spell around rather than a rule in a comment.
    for answering in [
        UserInputKind::Keyboard,
        UserInputKind::Ime,
        UserInputKind::Paste,
        UserInputKind::FilesRow,
        UserInputKind::MouseButton,
        UserInputKind::MouseWheel,
    ] {
        assert!(answering.answer_kind().is_some(), "{answering:?}");
    }
    assert_eq!(UserInputKind::MouseMotion.answer_kind(), None);

    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session
        .feed(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive")
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let bottom_frame = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(bottom_frame.scroll_offset_rows, 0);

    let physical_hit = bt_render::GridHit { row: 3, column: 5 };
    assert_eq!(
        live_viewport_mouse_hit(&bottom_frame, physical_hit),
        physical_hit
    );

    projection.scroll_by_rows(2);
    session.refresh_projection(&mut projection);
    let anchored_frame = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(anchored_frame.scroll_offset_rows, 2);

    let live_hit = live_viewport_mouse_hit(&anchored_frame, physical_hit);
    assert_eq!(live_hit, bt_render::GridHit { row: 1, column: 5 });
    assert_eq!(
        input::mouse_bytes(
            true,
            input::MouseProtocolButton::Left,
            input::MouseProtocolEvent::Press,
            live_hit.row,
            live_hit.column,
            ModifiersState::empty(),
        ),
        b"\x1b[<0;6;2M"
    );

    assert_eq!(
        live_viewport_mouse_hit(&anchored_frame, bt_render::GridHit { row: 1, column: 5 },),
        bt_render::GridHit { row: 0, column: 5 }
    );
}

#[test]
fn expanded_presented_row_map_drives_forwarded_mouse_row_and_column() {
    let start = Instant::now();
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(12).unwrap());
    session
        .feed_at(
            b"\x1b[?1049h$$x^2$$\r\nbarrier1\r\nbarrier2\r\nbottom",
            start,
        )
        .unwrap();
    assert_eq!(
        session.advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
        1
    );
    let mut task = session.take_live_worker_task().unwrap();
    let raster = render_live_detection_task(&MathEngine::new(), &mut task, foreground_rgb())
        .expect("test formula rasterizes through the production live worker entry");
    let ink_height_px = raster.height_px;
    assert!(session.complete_live_worker_result(task, Ok(raster)));

    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let cell_height = 18 * bt_viewport::SUBPIXELS_PER_PX;
    assert!(frame.row_map[0].height_subpixels > cell_height);
    assert_eq!(frame.math_blocks[0].artifact.render_scale_milli, 1000);
    // **The box is the ink plus whole cell rows of breathing** (owner's
    // ruling 2026-09-15 ①). The option still asks for a quarter of a cell
    // and the band still answers symmetrically, but what a quarter-row
    // request buys is rounded out to whole rows — the rows the ink needs,
    // one blank row above and one below — so the block sits in the grid
    // instead of a quarter of a line clear of the text around it. This
    // restates that rule for the reason it restated the old one: the
    // arithmetic lives in `bt_term` and this crate cannot call it.
    let ink = i64::from(ink_height_px) * bt_viewport::SUBPIXELS_PER_PX;
    let ink_rows = (ink + cell_height - 1) / cell_height;
    let band = (ink_rows + 2) * cell_height;
    let padding = (band - ink) / 2;
    assert!(
        padding >= cell_height,
        "a quarter of a row is not a row: {padding} against a {cell_height} cell"
    );
    assert_eq!(
        frame.math_blocks[0].artifact.height_subpixels,
        ink + 2 * padding,
        "display box height is alpha-tight ink plus whole cell rows of breathing"
    );
    assert_eq!(
        frame.math_blocks[0].artifact.vertical_padding_subpixels,
        padding
    );

    let target = frame.row_map[2];
    let target_y = target.top_subpixels + target.height_subpixels / 2;
    let visual_hit = bt_render::GridHit {
        row: frame
            .visual_row_at(target_y)
            .expect("expanded logical row remains hittable in the presented frame"),
        column: 5,
    };
    let forwarded = live_viewport_mouse_hit(&frame, visual_hit);
    assert_eq!(forwarded, bt_render::GridHit { row: 2, column: 5 });
    assert_eq!(
        input::mouse_bytes(
            true,
            input::MouseProtocolButton::Left,
            input::MouseProtocolEvent::Press,
            forwarded.row,
            forwarded.column,
            ModifiersState::empty(),
        ),
        b"\x1b[<0;6;3M"
    );
}

#[test]
fn a_verified_target_under_a_tracked_press_is_ours_on_the_primary_screen() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    // Exactly what Claude Code turns on, on the screen it prints its paths to.
    session
        .feed(b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h")
        .unwrap();
    let modes = session.terminal_modes();
    assert!(!modes.alternate_screen);
    assert_ne!(modes.mouse_tracking, MouseTracking::Off);
    let hit = bt_render::GridHit { row: 1, column: 2 };

    for modifiers in [ModifiersState::empty(), ModifiersState::CONTROL] {
        let mut route = None;
        assert!(
            route_forwarded_mouse_button(
                &mut route,
                ElementState::Pressed,
                input::MouseProtocolButton::Left,
                hit,
                modes,
                modifiers,
                PressedCellTarget::Ours,
                a_shell(),
            )
            .is_none(),
            "a press on a mark this window painted writes nothing to the child"
        );
        assert!(
            route.is_none(),
            "and leaves the route for `begin_local_selection` to claim"
        );
        // The release of that pair is the local drag's, and finds no forward
        // latched to answer either.
        assert!(
            route_forwarded_mouse_button(
                &mut route,
                ElementState::Released,
                input::MouseProtocolButton::Left,
                hit,
                modes,
                modifiers,
                PressedCellTarget::Ours,
                a_shell(),
            )
            .is_none()
        );
    }
}

#[test]
fn a_tracked_press_on_a_plain_cell_is_still_the_programs() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1003h\x1b[?1006h").unwrap();
    let mut route = None;
    assert_eq!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            bt_render::GridHit { row: 1, column: 2 },
            session.terminal_modes(),
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        ),
        Some(b"\x1b[<0;3;2M".to_vec())
    );
    assert!(matches!(route, Some(MouseRoute::Forward { .. })));
}

/// The window half of the prompt-start retirement (DESIGN §7.1.5i): nothing
/// here learns anything new, because `route_forwarded_mouse_button` has always
/// asked `session.terminal_modes()` afresh for every press. The mode going off
/// in `bt-term` *is* the whole delivery mechanism — this pins that the router
/// really does read it that late, and that the press the user makes on the
/// recovered prompt writes not one byte to the shell.
#[test]
fn a_prompt_start_that_retires_tracking_takes_the_next_press_back_from_the_child() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    // A program comes up, turns the set on, and dies without resetting it.
    session
        .feed(b"\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h")
        .unwrap();
    session.feed(b"\x1b[?1049l").unwrap();
    let hit = bt_render::GridHit { row: 1, column: 2 };
    let mut route = None;
    assert_eq!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        ),
        Some(b"\x1b[<0;3;2M".to_vec()),
        "the leftover mode is still routing presses at a program that is gone"
    );

    // PSReadLine draws the next prompt.
    session.feed(b"\x1b]133;A\x1b\\").unwrap();
    assert_eq!(session.terminal_modes().mouse_tracking, MouseTracking::Off);
    let mut recovered = None;
    assert!(
        route_forwarded_mouse_button(
            &mut recovered,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        )
        .is_none(),
        "and afterwards the press is this window's, to select with"
    );
    assert!(recovered.is_none());
}

/// **The press that was in the air when the prompt arrived.** A forward is a
/// latch: press sets `MouseRoute::Forward`, release clears it. The retirement
/// can land between the two, and the question is only whether the latch can be
/// left stuck — a route that never cleared would send every later release to a
/// program, and swallow every local drag, for as long as the pane lived.
///
/// It cannot: the `Released` arm keys off the latch itself, not off the modes,
/// so it fires and clears whatever the modes now say.
///
/// **And what it emits is the release owed to *that* press, in *that* press's
/// encoding.** The latch carries the encoding the press went out in, so a
/// button held across the prompt comes up as an SGR release (`\e[<0;3;2m`)
/// matching the SGR press the program already read. Reading `modes.sgr_mouse`
/// again at release time would answer a press the program saw in one protocol
/// with a release in another — `1006` is gone by then, so the pair would have
/// been split across SGR and X10, six bytes the program cannot match to
/// anything it is holding. A half-open button is a worse leftover than the one
/// this section is about.
#[test]
fn a_press_already_forwarded_when_the_prompt_arrives_is_released_in_its_own_encoding() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1003h\x1b[?1006h").unwrap();
    let hit = bt_render::GridHit { row: 1, column: 2 };
    let mut route = None;
    assert!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        )
        .is_some()
    );
    assert!(
        matches!(route, Some(MouseRoute::Forward { sgr: true, .. })),
        "the latch remembers the press went out in SGR"
    );

    session.feed(b"\x1b]133;A\x1b\\").unwrap();
    assert_eq!(session.terminal_modes().mouse_tracking, MouseTracking::Off);
    assert!(
        !session.terminal_modes().sgr_mouse,
        "and the encoding the modes would answer with is gone"
    );

    assert_eq!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Released,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        ),
        Some(b"\x1b[<0;3;2m".to_vec()),
        "the release the forwarded press is owed, in the encoding that press \
             went out in"
    );
    assert!(
        route.is_none(),
        "and the latch is open again — the one thing that must not survive"
    );
}

/// The same latch in the other direction, which is what makes it a latch and
/// not a special case for the prompt: a program that turns `1006` **on**
/// while a button is down still gets the X10 release its X10 press is owed.
///
/// Nothing on this side knows whether the child would rather have the new
/// encoding, and the child cannot be asked mid-click. What it does know is
/// which bytes it already sent, and a release that pairs with those is the
/// only release that closes the button it opened.
#[test]
fn a_release_keeps_the_press_encoding_when_the_program_switches_protocol_mid_click() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    // Tracking without 1006: presses go out X10.
    session.feed(b"\x1b[?1000h").unwrap();
    let hit = bt_render::GridHit { row: 1, column: 2 };
    let mut route = None;
    assert_eq!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        ),
        Some(vec![0x1b, b'[', b'M', b' ', b'#', b'"'])
    );
    assert!(matches!(
        route,
        Some(MouseRoute::Forward { sgr: false, .. })
    ));

    session.feed(b"\x1b[?1006h").unwrap();
    assert!(session.terminal_modes().sgr_mouse);

    assert_eq!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Released,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        ),
        Some(vec![0x1b, b'[', b'M', b'#', b'#', b'"']),
        "the release answers the press, not the modes"
    );
    assert!(route.is_none());
}

/// **The moves in the middle of the click belong to the press too.** The
/// release arm has answered in the press's encoding since 2026-08-21; motion
/// argued its way out of the same latch on the grounds that the only way to
/// lose `1006` mid-click was the prompt-start retirement (§7.1.5i), which
/// takes `1000`/`1002`/`1003` with it and so returns at the tracking guard
/// before any encoding question is asked.
///
/// That is true of the retirement and false in general: `\e[?1006l` is one
/// mode of its own, and a program that drops it while keeping its tracking
/// level leaves motion reading a `sgr_mouse` its own press did not go out
/// in. The drag then arrives at the child as an SGR press, a run of X10
/// six-byte moves, and an SGR release — one gesture spelled in two
/// protocols, with the moves the half the program cannot pair to the button
/// it is holding.
///
/// MUTATION: put `modes.sgr_mouse` back into the `Forward` arm of
/// [`route_forwarded_mouse_motion`] and this goes red.
#[test]
fn a_forwarded_drag_keeps_its_press_encoding_when_the_program_drops_sgr_mid_drag() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1002h\x1b[?1006h").unwrap();
    let hit = bt_render::GridHit { row: 1, column: 2 };
    let mut route = None;
    assert_eq!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        ),
        Some(b"\x1b[<0;3;2M".to_vec())
    );

    // The program drops the encoding on its own, without touching tracking:
    // the retirement's "it all goes at once" is not available here.
    session.feed(b"\x1b[?1006l").unwrap();
    let modes = session.terminal_modes();
    assert!(!modes.sgr_mouse);
    assert_eq!(
        modes.mouse_tracking,
        MouseTracking::Drag,
        "tracking survives, so the motion path is still live"
    );

    assert_eq!(
        route_forwarded_mouse_motion(route.as_ref(), modes, ModifiersState::empty()),
        Some((true, input::MouseProtocolButton::Left)),
        "the moves belong to the press, and go out spelled the way it was"
    );
    let moved = bt_render::GridHit { row: 2, column: 3 };
    let (sgr, button) =
        route_forwarded_mouse_motion(route.as_ref(), modes, ModifiersState::empty()).unwrap();
    assert_eq!(
        input::mouse_bytes(
            sgr,
            button,
            input::MouseProtocolEvent::Motion,
            moved.row,
            moved.column,
            ModifiersState::empty(),
        ),
        b"\x1b[<32;4;3M",
        "not the X10 six bytes the current modes would have spelled"
    );
}

/// The same latch the other way round, which is what makes it a latch: a
/// program that turns `1006` **on** while a button is down still gets X10
/// moves, because X10 is what its press went out in.
#[test]
fn a_forwarded_drag_keeps_its_press_encoding_when_the_program_adds_sgr_mid_drag() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1002h").unwrap();
    let hit = bt_render::GridHit { row: 1, column: 2 };
    let mut route = None;
    assert_eq!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        ),
        Some(vec![0x1b, b'[', b'M', b' ', b'#', b'"'])
    );

    session.feed(b"\x1b[?1006h").unwrap();
    let modes = session.terminal_modes();
    assert!(modes.sgr_mouse);
    assert_eq!(
        route_forwarded_mouse_motion(route.as_ref(), modes, ModifiersState::empty()),
        Some((false, input::MouseProtocolButton::Left))
    );
}

/// And the boundary the latch does not cross: **a mode change is a fact
/// about the next gesture.** With no button down there is no press to be
/// owed anything, so a bare tracking move reads the modes as they are — and
/// the press that starts the next drag latches whatever they say then.
#[test]
fn a_hover_with_no_press_in_flight_reads_the_modes_as_they_are() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1003h").unwrap();
    assert_eq!(
        route_forwarded_mouse_motion(None, session.terminal_modes(), ModifiersState::empty()),
        Some((false, input::MouseProtocolButton::None))
    );

    session.feed(b"\x1b[?1006h").unwrap();
    assert_eq!(
        route_forwarded_mouse_motion(None, session.terminal_modes(), ModifiersState::empty()),
        Some((true, input::MouseProtocolButton::None)),
        "no latch is held open, so the new encoding takes effect at once"
    );

    // Shift is the window's, and `Click` tracking hears presses only — the
    // two refusals the motion path had before it had a latch.
    assert_eq!(
        route_forwarded_mouse_motion(None, session.terminal_modes(), ModifiersState::SHIFT),
        None
    );
    let mut click_only =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    click_only.feed(b"\x1b[?1000h\x1b[?1006h").unwrap();
    let modes = click_only.terminal_modes();
    assert_eq!(modes.mouse_tracking, MouseTracking::Click);
    assert_eq!(
        route_forwarded_mouse_motion(None, modes, ModifiersState::empty()),
        None
    );
    assert_eq!(
        route_forwarded_mouse_motion(
            Some(&MouseRoute::Forward {
                button: input::MouseProtocolButton::Left,
                sgr: true,
                owner: a_shell(),
            }),
            modes,
            ModifiersState::empty(),
        ),
        None,
        "a click-tracking program asked for presses, not for the drag between them"
    );
}

/// RED (T-RESET-MODES) — **the pane recovery action turns a dead program's mouse
/// stream off and makes the next modified key use the legacy encoder.**
///
/// The two consumers the incident was about, read the way the window reads
/// them: pointer-motion routing and the key encoder, each given the session's
/// modes after the reset.
///
/// MUTATION: make `DualPlaneSession::reset_program_modes` skip the adapter
/// reset; both postconditions stay in their program-owned state and go red.
#[test]
fn pane_menu_reset_terminal_modes_stops_mouse_motion_and_restores_legacy_keys() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session
        .feed("主屏 primary\x1b[?1003h\x1b[?1006h\x1b[>1u\x1b[>4;2m\x1b[?1h".as_bytes())
        .unwrap();
    assert!(
        route_forwarded_mouse_motion(None, session.terminal_modes(), ModifiersState::empty())
            .is_some()
    );

    session
        .reset_program_modes(bt_term::PtyTransport::Unix)
        .unwrap();
    let modes = session.terminal_modes();
    assert_eq!(
        route_forwarded_mouse_motion(None, modes, ModifiersState::empty()),
        None,
        "pointer motion is Folio's again"
    );
    let enter = Key::Named(NamedKey::Enter);
    let encoded = input::keyboard_bytes(
        &enter,
        &enter,
        winit::keyboard::KeyLocation::Standard,
        ModifiersState::SHIFT,
        session.application_cursor_mode(),
        modes.keyboard,
        input::KeyOrigin {
            platform: bt_platform::HostPlatform::OtherUnix,
            physical_key: winit::keyboard::PhysicalKey::Unidentified(
                winit::keyboard::NativeKeyCode::Unidentified,
            ),
            text_with_all_modifiers: None,
            virtual_key_of_scan_code: |_| None,
            virtual_key_is_dead: |_| false,
            conpty: bt_pty::ConPtyKind::NotConPty,
            shifted_character: input::ShiftedCharacter::Known(None),
        },
    );
    assert_eq!(encoded, Some(vec![b'\r']), "Shift+Enter is legacy again");
}

/// PIN (user ruling, 2026-08-20 — the repeal §7.1.5f wrote its own warrant
/// for) — **the same verified target on the alternate screen is ours too.**
///
/// This test is the 2026-08-16 one turned around, kept rather than replaced
/// so the reversal is legible in one place. Gate ③ read "primary screen
/// only", and its stated reason was that there was nothing to argue about on
/// an alternate screen because nothing scanned one. That premise is gone:
/// OSC 8 spans and bare URLs are found on the alternate screen and wear this
/// window's underline there, so §7.1.5f's own core sentence — *a mark that
/// answers a hover and not a click is the window lying about what it drew* —
/// now holds on both screens. The ruling that wrote gate ③ said in the same
/// breath that the next slice teaching the scanner about alt screens would
/// repeal it; this is that slice.
///
/// MUTATION: put `!modes.alternate_screen` back into
/// [`press_belongs_to_the_window`] and this goes red on both modifiers and
/// on both edges of the click.
#[test]
fn the_same_verified_target_on_the_alternate_screen_is_the_windows_too() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session
        .feed(b"\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h")
        .unwrap();
    let modes = session.terminal_modes();
    assert!(modes.alternate_screen);
    assert_ne!(modes.mouse_tracking, MouseTracking::Off);
    let hit = bt_render::GridHit { row: 1, column: 2 };

    for modifiers in [ModifiersState::empty(), ModifiersState::CONTROL] {
        let mut route = None;
        assert!(
            route_forwarded_mouse_button(
                &mut route,
                ElementState::Pressed,
                input::MouseProtocolButton::Left,
                hit,
                modes,
                modifiers,
                PressedCellTarget::Ours,
                a_shell(),
            )
            .is_none(),
            "a press on a mark this window painted writes nothing to the child, \
                 on the alternate screen as on the primary one"
        );
        assert!(
            route.is_none(),
            "and leaves the route for `begin_local_selection` to claim"
        );
        assert!(
            route_forwarded_mouse_button(
                &mut route,
                ElementState::Released,
                input::MouseProtocolButton::Left,
                hit,
                modes,
                modifiers,
                PressedCellTarget::Ours,
                a_shell(),
            )
            .is_none(),
            "and the release of that pair finds no forward latched to answer"
        );
    }
}

/// The other half of the repeal, and the half that keeps it narrow: an
/// *ordinary* cell on the alternate screen is still the program's whole
/// mouse. vim, yazi and lazygit lose nothing but the cells this window has
/// drawn a promise on.
#[test]
fn a_tracked_press_on_a_plain_cell_is_still_the_programs_on_the_alternate_screen() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1049h\x1b[?1003h\x1b[?1006h").unwrap();
    let modes = session.terminal_modes();
    assert!(modes.alternate_screen);
    let mut route = None;
    assert_eq!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            bt_render::GridHit { row: 1, column: 2 },
            modes,
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        ),
        Some(b"\x1b[<0;3;2M".to_vec())
    );
    assert!(matches!(route, Some(MouseRoute::Forward { .. })));
}

/// PIN — **an OSC 8 link printed on the alternate screen really is found
/// there**, which is the fact that repealed gate ③ and the fact the route
/// test above can only assume.
///
/// Driven from bytes through a real session rather than from a hand-made
/// `PressedCellTarget`, because the claim being pinned is about detection,
/// not about an enum: `Runtime::pressed_cell_target` answers `Ours` for
/// exactly what `ViewportFrame::hyperlink_at` finds here (plus the verified
/// image list, which needs a decode worker and so cannot be reached from a
/// unit test). If a future slice stopped scanning the alternate screen this
/// goes red *here*, at the premise, instead of leaving the ruling above
/// standing on nothing.
#[test]
fn an_osc_8_link_on_the_alternate_screen_is_found_and_keeps_its_press() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(24).unwrap(), NonZeroU32::new(3).unwrap());
    // Claude Code's own shape: alternate screen, the four tracking modes it
    // turns on, and a link printed into it.
    session
        .feed(b"\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h")
        .unwrap();
    session
        .feed(b"\x1b]8;;file:///D:/Developer/folio-terminal/test.md\x1b\\test.md\x1b]8;;\x1b\\")
        .unwrap();
    let modes = session.terminal_modes();
    assert!(modes.alternate_screen);
    assert_ne!(modes.mouse_tracking, MouseTracking::Off);

    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let hit = bt_render::GridHit { row: 0, column: 3 };
    let link = frame
        .hyperlink_at(hit.row, hit.column)
        .expect("the alternate screen is scanned, so the label is a link there");
    assert_eq!(link.uri, "file:///D:/Developer/folio-terminal/test.md");

    // Same reading `pressed_cell_target` makes of that cell, and the press
    // that follows from it.
    let target = PressedCellTarget::Ours;
    let mut route = None;
    assert!(
        route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            modes,
            ModifiersState::empty(),
            target,
            a_shell(),
        )
        .is_none(),
        "the press stays here, so `begin_local_selection` gets to arm the link"
    );
    assert!(route.is_none());
}

#[test]
fn shift_keeps_a_tracked_press_local_on_every_cell_and_on_both_screens() {
    for enter_alt in [b"".as_slice(), b"\x1b[?1049h"] {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
        session.feed(enter_alt).unwrap();
        session.feed(b"\x1b[?1003h\x1b[?1006h").unwrap();
        for target in [PressedCellTarget::Ordinary, PressedCellTarget::Ours] {
            let mut route = None;
            assert!(
                route_forwarded_mouse_button(
                    &mut route,
                    ElementState::Pressed,
                    input::MouseProtocolButton::Left,
                    bt_render::GridHit { row: 1, column: 2 },
                    session.terminal_modes(),
                    ModifiersState::SHIFT,
                    target,
                    a_shell(),
                )
                .is_none()
            );
            assert!(route.is_none());
        }
    }
}

/// Gate ② of §7.1.5f, on both screens since 2026-08-20 — the repeal took the
/// screen out of the rule, so the *button* is now the only thing still
/// narrowing it and it is asked on the alternate screen too.
#[test]
fn only_a_left_press_can_be_taken_from_a_tracking_program_on_either_screen() {
    for enter_alt in [b"".as_slice(), b"\x1b[?1049h"] {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
        session.feed(enter_alt).unwrap();
        session.feed(b"\x1b[?1003h\x1b[?1006h").unwrap();
        for button in [
            input::MouseProtocolButton::Right,
            input::MouseProtocolButton::Middle,
        ] {
            let mut route = None;
            assert!(
                route_forwarded_mouse_button(
                    &mut route,
                    ElementState::Pressed,
                    button,
                    bt_render::GridHit { row: 1, column: 2 },
                    session.terminal_modes(),
                    ModifiersState::empty(),
                    PressedCellTarget::Ours,
                    a_shell(),
                )
                .is_some(),
                "there is no local verb on the other two buttons to trade the hole for"
            );
        }
    }
}

/// PIN (ticket #62, item 2) — **a right press raises this window's menu
/// unless the program is tracking the mouse, and then `Shift`+right does.**
///
/// The second half of the test is the load-bearing one: the two answers are
/// asserted to be *complementary* for the right button, against
/// [`route_forwarded_mouse_button`] itself rather than against a remembered
/// copy of its rule. One press can be claimed by exactly one of them, and
/// the failure this forbids is the silent one — a press that opens no menu
/// *and* sends no report, which reads to the user as a dead button.
///
/// MUTATION: drop the `shift` arm and the tracking case never opens a menu
/// again; drop the `Off` arm and an idle shell's right-click goes down the
/// pipe as a mouse report nothing asked for.
#[test]
fn a_right_press_is_the_windows_unless_the_program_tracks_and_then_shift_takes_it_back() {
    let mut idle = DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    idle.feed(b"ready").unwrap();
    assert_eq!(idle.terminal_modes().mouse_tracking, MouseTracking::Off);
    assert!(right_press_raises_terminal_menu(
        idle.terminal_modes(),
        ModifiersState::empty()
    ));

    // Every screen a tracking program can be on, because the rule is about
    // who asked for the mouse and not about which screen is up.
    for enter_alt in [b"".as_slice(), b"\x1b[?1049h"] {
        let mut tracking =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
        tracking.feed(enter_alt).unwrap();
        tracking.feed(b"\x1b[?1000h\x1b[?1006h").unwrap();
        let modes = tracking.terminal_modes();
        assert_ne!(modes.mouse_tracking, MouseTracking::Off);

        assert!(
            !right_press_raises_terminal_menu(modes, ModifiersState::empty()),
            "a bare right press belongs to the program that asked for it"
        );
        assert!(
            right_press_raises_terminal_menu(modes, ModifiersState::SHIFT),
            "and Shift is how the window takes it back (WT / VS Code)"
        );

        // Complementary, checked against the forwarder rather than restated:
        // exactly one of the two claims each press.
        for modifiers in [ModifiersState::empty(), ModifiersState::SHIFT] {
            let mut route = None;
            let forwarded = route_forwarded_mouse_button(
                &mut route,
                ElementState::Pressed,
                input::MouseProtocolButton::Right,
                bt_render::GridHit { row: 1, column: 2 },
                modes,
                modifiers,
                PressedCellTarget::Ordinary,
                a_shell(),
            )
            .is_some();
            assert_ne!(
                forwarded,
                right_press_raises_terminal_menu(modes, modifiers),
                "a right press is the menu's or the program's, never both and never neither"
            );
        }
    }

    // And with nothing tracking, the forwarder declines whatever the
    // modifiers say — so the menu is free to take every one of them.
    for modifiers in [ModifiersState::empty(), ModifiersState::SHIFT] {
        let mut route = None;
        assert!(
            route_forwarded_mouse_button(
                &mut route,
                ElementState::Pressed,
                input::MouseProtocolButton::Right,
                bt_render::GridHit { row: 1, column: 2 },
                idle.terminal_modes(),
                modifiers,
                PressedCellTarget::Ordinary,
                a_shell(),
            )
            .is_none()
        );
        assert!(right_press_raises_terminal_menu(
            idle.terminal_modes(),
            modifiers
        ));
    }
}

#[test]
fn stationary_double_click_stays_strictly_paired_across_tui_repaints() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(6).unwrap());
    session.feed(b"\x1b[?1000h\x1b[?1006hready").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let hit = bt_render::GridHit { row: 3, column: 8 };
    let mut route = None;
    let mut captured_user_input = Vec::new();

    for repaint in [
        b"\x1b[Hfirst repaint\r\na\r\nb\r\nc\r\nd\r\ne\r\nf".as_slice(),
        b"\x1b[Hsecond repaint\r\ng\r\nh\r\ni\r\nj\r\nk\r\nl",
    ] {
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(frame.scroll_offset_rows, 0);
        let live_hit = live_viewport_mouse_hit(&frame, hit);
        for state in [ElementState::Pressed, ElementState::Released] {
            captured_user_input.push(
                route_forwarded_mouse_button(
                    &mut route,
                    state,
                    input::MouseProtocolButton::Left,
                    live_hit,
                    session.terminal_modes(),
                    ModifiersState::empty(),
                    PressedCellTarget::Ordinary,
                    a_shell(),
                )
                .expect("tracked click must produce one PTY write per edge"),
            );
        }
        assert!(route.is_none());
        session.feed(repaint).unwrap();
        session.refresh_projection(&mut projection);
    }

    assert_eq!(
        captured_user_input,
        [
            b"\x1b[<0;9;4M".to_vec(),
            b"\x1b[<0;9;4m".to_vec(),
            b"\x1b[<0;9;4M".to_vec(),
            b"\x1b[<0;9;4m".to_vec(),
        ]
    );
}

#[test]
fn the_pointer_keeps_one_shape_for_the_whole_drag() {
    // K113. "The cursor changing shape mid-drag would say something happened
    // when nothing did" (mock-up 1710-1711).
    use winit::window::CursorIcon;
    assert_eq!(
        pointer_cursor(false, None, None, false, false, None),
        CursorIcon::Default
    );
    assert_eq!(
        pointer_cursor(false, None, Some(bt_layout::Axis::Row), false, false, None),
        CursorIcon::EwResize
    );
    assert_eq!(
        pointer_cursor(false, None, Some(bt_layout::Axis::Col), false, false, None),
        CursorIcon::NsResize
    );
    for axis in [None, Some(bt_layout::Axis::Row), Some(bt_layout::Axis::Col)] {
        for grasp in [
            None,
            Some(FloatGrasp::Grip),
            Some(FloatGrasp::Head),
            Some(FloatGrasp::Carrying),
        ] {
            for over_link in [false, true] {
                assert_eq!(
                    pointer_cursor(true, grasp, axis, over_link, false, None),
                    CursorIcon::Default,
                    "a tab drag crossing a divider, a float or a link must \
                         not flicker into another shape"
                );
            }
        }
    }
}

/// A markdown link says it answers a press the way every reader on the desk
/// says it (user ruling, 2026-08-13) — and gives way to a window standing
/// over it, because what a float covers is not what you are aiming at.
#[test]
fn a_markdown_link_wears_the_pointing_finger() {
    use winit::window::CursorIcon;
    assert_eq!(
        pointer_cursor(false, None, None, true, false, None),
        CursorIcon::Pointer
    );
    assert_eq!(
        pointer_cursor(false, None, Some(bt_layout::Axis::Row), true, false, None),
        CursorIcon::Pointer,
        "and over a divider it is still the link the pointer is on"
    );
    assert_eq!(
        pointer_cursor(false, Some(FloatGrasp::Head), None, true, false, None),
        CursorIcon::Grab,
        "but a window over the document takes the pointer with it"
    );
}

/// PIN — **the finger appears exactly where a press would do something**
/// (§7.1.5g, user rulings 2026-08-20).
///
/// The underline is a fact about the output and is already there without any
/// modifier; the hand is the narrower sentence, and after the plain-click
/// ruling it is no longer a fact about `Ctrl` at all. A link to a readable
/// file wears it plainly, because a plain press opens it — and since
/// 2026-08-21 so does a folder, because a plain press points the files column
/// at it, and since 2026-08-23 so does a page, because a plain press draws it
/// on the seat. **Since 2026-08-29 so does a web address**, for the same
/// reason and by the same reading: the row's plain half stopped being empty,
/// so the finger over it stopped being a promise the release declines —
/// which is 7.1.5f's complaint, and this row was the last place it was still
/// true. It must be the *same* expression the verb is spent from: a
/// pointer promising a press the release then declines is 7.1.5f's complaint
/// wearing a different shape.
///
/// MUTATION: make the hand `control && over_hyperlink` again — the shape it
/// had before the ruling — and the readable-file row goes red without `Ctrl`,
/// which is a window that opens a file it said it would not.
#[test]
fn a_terminal_hyperlink_wears_the_finger_where_a_press_would_answer() {
    use winit::window::CursorIcon;
    let folder = Path::new(r"C:\repo\docs");
    let is_directory: &dyn Fn(&Path) -> Option<bt_term::PathVerdict> = &|path| {
        Some(if path == folder {
            a_local_folder()
        } else {
            a_local_file()
        })
    };
    // rows: the URI, then the hand plainly and the hand under Ctrl.
    for (uri, plainly, under_control) in [
        ("file:///C:/notes.md", true, true),
        ("https://example.test/path", true, true),
        ("file:///C:/repo/docs", true, true),
        ("file:///C:/page.html", true, true),
        // The row that still needs the modifier to light anything at all:
        // nothing inside this window plainly, and under `Ctrl` a refusal —
        // which is an answer, and so wears the hand exactly as the rows
        // that go somewhere do. The finger has never promised a
        // destination; it promises that the press is spent on something.
        ("mailto:person@example.test", false, true),
    ] {
        for (control, expected) in [(false, plainly), (true, under_control)] {
            assert_eq!(
                terminal_link_answers_a_press(
                    control,
                    Some(uri),
                    bt_transcript::paths::PathNamer::ThisWindow,
                    is_directory
                ),
                expected,
                "the hand over {uri:?} with Ctrl {control}"
            );
            // One expression, not two agreeing ones.
            assert_eq!(
                terminal_link_answers_a_press(
                    control,
                    Some(uri),
                    bt_transcript::paths::PathNamer::ThisWindow,
                    is_directory
                ),
                hyperlink_activation(
                    control,
                    true,
                    uri,
                    bt_transcript::paths::PathNamer::ThisWindow,
                    is_directory
                ) != HyperlinkActivation::None,
                "the shape and the verb are the same reading: {uri:?}, Ctrl {control}"
            );
            assert_eq!(
                pointer_cursor(
                    false,
                    None,
                    None,
                    terminal_link_answers_a_press(
                        control,
                        Some(uri),
                        bt_transcript::paths::PathNamer::ThisWindow,
                        is_directory
                    ),
                    false,
                    None
                ),
                if expected {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                },
                "and the shape the window sets says the same: {uri:?}, Ctrl {control}"
            );
        }
    }
    // No link under the pointer is no hand, whatever is held down.
    for control in [false, true] {
        assert!(!terminal_link_answers_a_press(
            control,
            None,
            bt_transcript::paths::PathNamer::ThisWindow,
            is_directory
        ));
    }
}

/// The two shapes a pinned float names for itself: `nwse-resize` on
/// `.fly-resize` (mock-up 1708) and `grab`/`grabbing` on the header
/// (704-705). The grip is the only resize target there is — the mock-up
/// gives the float no single-axis edges, and neither does `FloatPart`.
#[test]
fn a_pinned_float_wears_the_diagonal_arrow_on_its_grip() {
    use winit::window::CursorIcon;
    assert_eq!(
        pointer_cursor(false, Some(FloatGrasp::Grip), None, false, false, None),
        CursorIcon::NwseResize
    );
    assert_eq!(
        pointer_cursor(false, Some(FloatGrasp::Head), None, false, false, None),
        CursorIcon::Grab
    );
    assert_eq!(
        pointer_cursor(false, Some(FloatGrasp::Carrying), None, false, false, None),
        CursorIcon::Grabbing
    );
    for axis in [Some(bt_layout::Axis::Row), Some(bt_layout::Axis::Col)] {
        assert_eq!(
            pointer_cursor(false, Some(FloatGrasp::Grip), axis, false, false, None),
            CursorIcon::NwseResize,
            "the window is drawn over the divider, so the divider is not what you are aiming at"
        );
    }
}

// ── U4: the engine tabs and panes share (J111-J122) ──

/// A drag carrying `source`, with `landing` last surveyed. The carry is a
/// tab's when the source is a tab, because that pairing is the engine's
/// invariant rather than a choice any caller makes.
fn drag_of(source: DragSource, landing: Option<DropLanding>) -> Drag {
    Drag {
        carry: match source {
            DragSource::Tab(tab) => DragCarry::Tab(TabCarry {
                grab: 0.0,
                origin: 0,
                offset: 0.0,
                moved: false,
                home: tab,
            }),
            DragSource::Pane(_) | DragSource::Row(_) => DragCarry::Pane,
        },
        source,
        pointer: PhysicalPosition::new(0.0, 0.0),
        landing,
        // None, because none of U4's fixtures is a text write: the offer is
        // read off a live window and the release compares it against a
        // second reading, and `paste_offer_survives` is where that pair is
        // tested against every way it can come apart.
        paste_offer: None,
        home: None,
        spring: SpringGate::default(),
        autoscroll_ticked_at: None,
        seam: None,
    }
}

/// The tab every pane in these fixtures is taken out of — the one on screen,
/// which is what `begin_pane_drag` records.
const HELD_IN: TabId = TabId(1);

/// A pane of [`HELD_IN`], by seat.
fn held_pane(seat: u64) -> DragSource {
    DragSource::Pane(LeafId {
        tab: HELD_IN,
        seat: bt_layout::SeatId(seat),
    })
}

/// J113 — one threshold, and it belongs to the latch rather than to either
/// press that owns one.
///
/// The tab's half of this is already pinned by
/// `travelling_past_six_pixels_abandons_the_delayed_switch`; what is new is
/// that a pane head crosses the *same* six pixels, measured the same way, at
/// every scale.
///
/// Red gate: give the pane its own constant — any other number — and the
/// middle assertion of each pass fails, because 6.0 is the only radius that
/// is short of at 5px and past at 7px.
#[test]
fn a_pane_head_and_a_tab_cross_the_same_six_pixels() {
    for scale in [1.0_f64, 1.5, 2.0] {
        let origin = PhysicalPosition::new(100.0, 200.0);
        let mut pane = DragLatch::new(origin);
        let mut tab = TabPress::armed(TabId(1), origin, Instant::now());
        let short = PhysicalPosition::new(100.0 + 5.0 * scale, 200.0);
        let far = PhysicalPosition::new(100.0 + 7.0 * scale, 200.0);
        assert!(
            !pane.travelled(short, scale),
            "5 logical px is still a press"
        );
        assert!(!tab.travelled(short, scale), "and the tab agrees");
        assert!(pane.travelled(far, scale), "7 logical px is a drag");
        assert!(tab.travelled(far, scale), "and the tab agrees");
        assert!(
            !pane.travelled(PhysicalPosition::new(900.0, 900.0), scale),
            "the drag starts once, for a pane exactly as for a tab"
        );
    }
    // Euclidean, not per-axis: a diagonal hand travels as far as a straight
    // one. 4/4 is 5.66 and short; 5/5 is 7.07 and past.
    let mut latch = DragLatch::new(PhysicalPosition::new(0.0, 0.0));
    assert!(!latch.travelled(PhysicalPosition::new(4.0, 4.0), 1.0));
    assert!(latch.travelled(PhysicalPosition::new(5.0, 5.0), 1.0));
}

/// J120 — a release that landed nowhere goes home, and it is not the same
/// answer as a release that landed.
///
/// Red gate: the mock-up's own behaviour is `DragRelease::Commit` for both
/// arms — `!d.target` falls in beside `d.target.reordered` and returns
/// without undoing the live reorder (7202-7208). Return `Commit` for `None`
/// here and this is the assertion that says the ruling was dropped.
#[test]
fn a_release_that_landed_nowhere_sends_the_gesture_home_rather_than_committing() {
    assert_eq!(release_verdict(None), DragRelease::Home);
    assert_eq!(
        release_verdict(Some(DropLanding::StripReorder { slot: 3 })),
        DragRelease::Commit,
        "the strip already applied it — committing is letting it stand"
    );
}

/// J116/K124 — the ghost stands down when the landing is already showing the
/// user what it means.
///
/// "The tab itself is the feedback" (mock-up 6837). A tab reordering in the
/// strip is holding the slot it would take, so a second label saying the same
/// name under the pointer is the drag telling you twice — and the one in the
/// strip is the one telling you *where*. A pane has no such stand-in: nothing
/// on screen moves for it, so the ghost is the entire report.
///
/// Red gate: make `shows_itself` answer `false` and the first assertion
/// fails; drop the `!` from `ghost_is_shown` and the second does.
#[test]
fn the_ghost_yields_to_a_landing_that_is_already_showing_itself() {
    let tab = drag_of(
        DragSource::Tab(TabId(1)),
        Some(DropLanding::StripReorder { slot: 0 }),
    );
    assert!(
        !tab.ghost_is_shown(),
        "the reordering tab is its own feedback"
    );
    let pane = drag_of(held_pane(1), None);
    assert!(
        pane.ghost_is_shown(),
        "nothing else on screen has moved for a pane, so the ghost is all there is"
    );
    assert!(
        drag_of(DragSource::Tab(TabId(1)), None).ghost_is_shown(),
        "a tab with nowhere to land is carried by the ghost like anything else"
    );
}

/// J111/J118 — one drag, two things it can be carrying, and a pane carries
/// nothing that would let it be sent home.
///
/// This is the guard `settle_home` reads, stated where it can be seen: a
/// pane's J120 is a no-op *because* there is no carry to unwind, and there is
/// no carry because the tree was never touched. The two are the same fact.
///
/// Red gate: fold `TabCarry`'s four fields onto every drag — one struct, four
/// `Option`s or four zeros — and `tab_carry()` answers `Some` for a pane.
/// `settle_home` then reads `origin: 0` off it and walks whichever tab
/// happens to be in slot 0 across the strip, on a gesture that was carrying a
/// pane and never named a tab at all.
#[test]
fn a_pane_drag_carries_no_slot_and_no_offset_so_it_has_no_way_home() {
    let pane = drag_of(held_pane(7), None);
    assert_eq!(pane.tab(), None);
    assert!(
        pane.tab_carry().is_none(),
        "nothing moved, so nothing has to move back"
    );
    let tab = drag_of(DragSource::Tab(TabId(4)), None);
    assert_eq!(tab.tab(), Some(TabId(4)));
    assert!(tab.tab_carry().is_some());
}

// ── U5: what the pointer has found (K123-K135) ──

/// **K135 — never onto yourself, in any zone.**
///
/// Splitting a pane against itself and swapping it with itself are both the
/// identity, so the honest report of a gesture that would do nothing is that
/// there is nothing under the pointer. Note it is the *whole* pane that goes
/// dead and not only its middle: `drag.leafId === leafId` is tested after the
/// zone is computed and ignores it (7101).
///
/// Red gate: drop the identity test and a pane held over its own left third
/// answers `SeatEdge`, which U7 would turn into a split of a seat against
/// itself.
#[test]
fn a_pane_has_no_landing_anywhere_on_itself() {
    let mine = bt_layout::SeatId(3);
    let other = bt_layout::SeatId(4);
    let held = DragSource::Pane(LeafId {
        tab: HELD_IN,
        seat: mine,
    });
    for aim in [
        seats::LayoutAim::SeatEdge(mine, seats::DropEdge::Left),
        seats::LayoutAim::SeatEdge(mine, seats::DropEdge::Bottom),
        seats::LayoutAim::SeatCentre(mine),
    ] {
        assert_eq!(
            landing_for_aim(&held, HELD_IN, aim),
            None,
            "a pane held over its own rectangle has no landing: {aim:?}"
        );
    }
    assert_eq!(
        landing_for_aim(&held, HELD_IN, seats::LayoutAim::SeatCentre(other)),
        Some(DropLanding::SeatCentre { target: other }),
        "a neighbour is a target like any other"
    );
    assert_eq!(
        landing_for_aim(
            &DragSource::Tab(TabId(1)),
            HELD_IN,
            seats::LayoutAim::SeatCentre(mine)
        ),
        Some(DropLanding::SeatCentre { target: mine }),
        "a tab is not any pane, so no pane is its own"
    );
}

/// **K130/G83 — the rim belongs to no seat, so it can never be your own.**
///
/// Dragging your own pane out to the rim is precisely how you ask for it to
/// sit beside everything else, which is the gesture G82 exists to give and
/// the one the root split had no edge to offer before.
///
/// Red gate: run the identity test before the match instead of inside its two
/// seat arms and the rim goes dead for pane drags — the only source that has
/// any use for it.
#[test]
fn the_rim_is_no_ones_pane() {
    for edge in seats::DropEdge::NEAREST_FIRST {
        assert_eq!(
            landing_for_aim(&held_pane(1), HELD_IN, seats::LayoutAim::Rim(edge)),
            Some(DropLanding::RootRim { edge })
        );
    }
}

// ── §7.1.6k: spring-loaded, a pane carried over the tab list ──

/// **K135 is about a *pane*, and a pane is a tab and a seat** (§7.1.6k).
///
/// The spring can leave the tab in the hand and the tab on the screen apart
/// for the rest of a gesture, and seat ids are minted per tab out of each
/// tab's own counter — so two tabs both have a `SeatId(3)`, and comparing the
/// numbers alone would answer "that is the one in your hand" about a pane in
/// another room.
///
/// Red gate: let `pane_here` ignore the tab and answer on the seat alone —
/// which is what this file did until §7.1.6k — and the second assertion goes
/// `None`: a perfectly good neighbour in the tab you sprang to is refused
/// because the pane you are carrying happens to share its number.
#[test]
fn a_pane_is_only_its_own_in_the_tab_that_is_showing() {
    let seat = bt_layout::SeatId(3);
    let held = DragSource::Pane(LeafId {
        tab: TabId(1),
        seat,
    });
    assert_eq!(
        landing_for_aim(&held, TabId(1), seats::LayoutAim::SeatCentre(seat)),
        None,
        "at home it is K135's own pane and there is nothing under the pointer"
    );
    assert_eq!(
        landing_for_aim(&held, TabId(2), seats::LayoutAim::SeatCentre(seat)),
        Some(DropLanding::SeatCentre { target: seat }),
        "in another tab that number is somebody else's pane"
    );
    assert_eq!(held.pane_here(TabId(1)), Some(seat));
    assert_eq!(
        held.pane_here(TabId(2)),
        None,
        "a pane whose tab is not being drawn has no rectangle here to be \
             held over"
    );
}

/// **§7.1.6k — letting go on a tab is its own verdict.**
///
/// Three landings the strip can answer and three different things a release
/// does with them: a reorder was applied live and is kept, a tear-out edits
/// the run, and an adopt edits two trees that may both be off screen.
///
/// Red gate: send `StripAdopt` to `DragRelease::Home` — where every landing
/// starts life — and the pane silently goes back where it came from after a
/// tab has spent the whole gesture saying it would take it.
#[test]
fn letting_go_on_a_tab_adopts_and_letting_go_between_them_extracts() {
    assert_eq!(
        release_verdict(Some(DropLanding::StripAdopt { tab: TabId(4) })),
        DragRelease::Adopt { tab: TabId(4) }
    );
    assert_eq!(
        release_verdict(Some(DropLanding::StripExtract { slot: 1 })),
        DragRelease::Extract { slot: 1 }
    );
    assert_eq!(release_verdict(None), DragRelease::Home);
    assert!(
        !DropLanding::StripAdopt { tab: TabId(4) }.shows_itself(),
        "no stand-in is drawn for it, so the ghost stays in the hand"
    );
    assert_eq!(
        DropLanding::StripAdopt { tab: TabId(4) }.layout_aim(),
        None,
        "it is not aimed into the layout, so no dock box is planned"
    );
    assert_eq!(DropLanding::StripAdopt { tab: TabId(4) }.aimed_at(), None);
    assert_eq!(
        DropLanding::StripAdopt { tab: TabId(4) }
            .caption(&held_pane(1), Some(bt_layout::SeatKind::Terminal)),
        "",
        "and the tab it lights up has already said where"
    );
}

/// **The two strip landings yield the ghost; the three layout ones do not.**
///
/// The mock-up sets `drag.ghost.style.opacity = "0"` inside the strip and
/// says why beside the *pane* case: "the preview in the strip is the ghost
/// now" (6792). Out over the layout the preview is a box drawn somewhere
/// else, saying something the ghost does not, so both are on screen at once.
///
/// Red gate: give `StripExtract` the layout's answer and a pane torn towards
/// the strip is labelled twice, once under the pointer and once in the slot;
/// give a rim the strip's answer and the hand goes empty over the layout.
#[test]
fn only_the_strip_takes_the_ghosts_place() {
    assert!(DropLanding::StripReorder { slot: 0 }.shows_itself());
    assert!(DropLanding::StripExtract { slot: 2 }.shows_itself());
    assert!(
        !DropLanding::RootRim {
            edge: seats::DropEdge::Left
        }
        .shows_itself()
    );
    assert!(
        !DropLanding::SeatEdge {
            target: bt_layout::SeatId(1),
            edge: seats::DropEdge::Top
        }
        .shows_itself()
    );
    assert!(
        !DropLanding::SeatCentre {
            target: bt_layout::SeatId(1)
        }
        .shows_itself()
    );
}

/// **U5 commits nothing it did not already commit.**
///
/// **U7 — the commit table, all five landings** (mock-up 7202-7231).
///
/// Four answers, and each one is a different relationship to work: the strip
/// reorder is *kept* (it happened live, slot by slot), the three landings in
/// the layout are *performed* now against the tree the plan built (the tree
/// was untouched all gesture), the tear-out is performed now against the
/// *strip*, and only an empty hand goes home having decided nothing.
///
/// `StripExtract` used to sit with the empty hand, on the argument that a
/// torn-out pane needs a tab to arrive in and a tab was a tree *and a shell*.
/// Panes own sessions now: the pane arrives carrying the one it was already
/// running (N157/K123), so the landing carries its slot into the verdict and
/// the release performs it.
///
/// Red gate: send `StripExtract` back to `Home` and the tear-out is silently
/// abandoned behind a caret the strip drew; send any of the three layout
/// landings to `Home` and the drop goes missing behind a preview that
/// promised it.
#[test]
fn the_release_table_keeps_the_strip_performs_the_layout_and_sends_the_rest_home() {
    assert_eq!(
        release_verdict(Some(DropLanding::StripReorder { slot: 3 })),
        DragRelease::Commit,
        "the strip already did it"
    );
    for landing in [
        DropLanding::RootRim {
            edge: seats::DropEdge::Bottom,
        },
        DropLanding::SeatEdge {
            target: bt_layout::SeatId(2),
            edge: seats::DropEdge::Right,
        },
        DropLanding::SeatCentre {
            target: bt_layout::SeatId(2),
        },
    ] {
        assert_eq!(
            release_verdict(Some(landing)),
            DragRelease::Land,
            "{landing:?} is a drop, and letting go performs it"
        );
    }
    assert_eq!(
        release_verdict(Some(DropLanding::StripExtract { slot: 1 })),
        DragRelease::Extract { slot: 1 },
        "N157: the pane arrives in the strip carrying its own shell, at the slot \
             the survey drew the caret in"
    );
    assert_eq!(release_verdict(None), DragRelease::Home);
}

/// PIN — **a report the platform put on the x axis is sideways, and the
/// larger component is the gesture** (user ruling, 2026-09-07).
///
/// A tilt wheel and a touchpad's second finger reach this window with no
/// modifier and no phase to tell them apart, and this is the whole of how
/// they are recognised. The tie-break matters because [`WheelBurst`] merges
/// consecutive reports: a run that is mostly down and a little sideways
/// arrives as one `LineDelta` carrying both, and a predicate of `x != 0.0`
/// would spend that run on the axis the hand did not mean.
///
/// MUTATION: drop the magnitude comparison and the merged cases go red — an
/// ordinary downward scroll with a pixel of tilt in it stops scrolling down.
#[test]
fn a_wheel_report_is_sideways_when_the_platform_put_it_on_the_x_axis() {
    let lines = |x: f32, y: f32| wheel_points_sideways(MouseScrollDelta::LineDelta(x, y));
    let pixels = |x: f64, y: f64| {
        wheel_points_sideways(MouseScrollDelta::PixelDelta(PhysicalPosition::new(x, y)))
    };

    // A tilt wheel, which is all this window ever sees of one.
    assert!(lines(-1.0, 0.0));
    assert!(lines(1.0, 0.0));
    // An ordinary notch, which is what every mouse on the desk sends.
    assert!(!lines(0.0, -3.0));
    // A merged run: the larger component is the gesture and the smaller one
    // is the hand not being straight.
    assert!(!lines(0.5, -3.0));
    assert!(lines(-3.0, 0.5));
    // A tie goes to the axis every surface in this window has.
    assert!(!lines(2.0, -2.0));
    // Nothing at all is not sideways, which keeps a dead report off an axis
    // it would then have to be clamped on.
    assert!(!lines(0.0, 0.0));
    // A trackpad speaks pixels, and the same reading answers it.
    assert!(pixels(-40.0, 3.0));
    assert!(!pixels(3.0, -40.0));
}

/// **A press on a link that never travelled opens it; one that travelled is
/// a selection** (user ruling: 点=留窗内, and a drag is not a click).
///
/// The same six logical pixels every other press on this desk is measured
/// against ([`DragLatch`]), and the same `click_no_drag` shape the terminal's
/// own hyperlinks are activated through.
///
/// MUTATION: activate the link at the press and a drag that begins on one
/// opens a file instead of selecting a sentence.
#[test]
fn a_press_on_a_link_opens_it_only_if_the_hand_held_still() {
    let origin = PhysicalPosition::new(100.0, 100.0);
    let mut still = DragLatch::new(origin);
    assert!(!still.travelled(PhysicalPosition::new(102.0, 101.0), 1.0));
    assert!(
        preview_press_opens_its_link(&still),
        "two pixels is a hand holding still, and the link answers",
    );
    let mut dragged = DragLatch::new(origin);
    assert!(dragged.travelled(PhysicalPosition::new(120.0, 100.0), 1.0));
    assert!(
        !preview_press_opens_its_link(&dragged),
        "twenty is a drag, and the drag is the selection's",
    );
}

/// **One address, said once** (user ruling 2026-08-25) — a page's foot is the
/// hover line and nothing else.
///
/// The band and the address row above it were both printing the page's own
/// URL, which §7.7 booked as a debt on the day the row landed. The strip
/// stays (it is where the hover line stands, and a band that came and went
/// under the pointer would move the page's bottom edge every time the
/// pointer crossed a link); what leaves is the echo.
///
/// MUTATION: return `shown_address(url)` at rest and the first assertion goes
/// red on the very duplication the ruling retired.
#[test]
fn a_pages_foot_says_nothing_until_the_pointer_is_over_a_link() {
    assert_eq!(
        page_foot_lead("https://example.com/manual", ""),
        "",
        "at rest the address row above is the one that says where this is"
    );

    assert_eq!(
        page_foot_lead("http://127.0.0.1:5173/", "http://127.0.0.1:5173/download"),
        "http://127.0.0.1:5173/download",
        "and a hovered link is still written out in full, resolved"
    );

    // A target this window will not follow is named and stamped — the same
    // words the terminal's own hover line uses (2026-08-20).
    let refused = page_foot_lead("https://example.com/", "mailto:someone@example.com");
    assert!(
        refused.starts_with("mailto:someone@example.com"),
        "the refused target is still said in full: {refused}"
    );
    assert!(
        refused.ends_with(i18n::Text::HyperlinkBlockedSuffix.text()),
        "and stamped: {refused}"
    );

    // The band spells a local file the way the row does (2026-08-25, the
    // earlier ruling of the same day). `webnav::address_bar` refuses `file:`
    // from every door, so this one is named as a path *and* stamped.
    let local = page_foot_lead(
        "https://example.com/",
        "file:///D:/Developer/notes%20and%20more.html",
    );
    assert!(
        local.starts_with(r"D:\Developer\notes and more.html"),
        "a path, not a URI: {local}"
    );
}

/// RED (adversarial review 2026-09-11, row D1) — **a right press inside a
/// floating window never names a row of the column underneath it.**
///
/// `float_hit_at` claims every point inside a float's frame, but
/// `file_row_under` consumed the claim for one part only — a tree row — and
/// let a head, a foot, a rail, a body and a body the tenant declined fall
/// through to the docked ladder, which does not consider floats at all. So
/// a right press on a preview float's text, at a y that happened to land on
/// a docked row, produced that row's menu: a face of verbs with no file name
/// on it, drawn on top of the float that was hiding the row it was about,
/// with `Delete` among them since `4031215`.
///
/// **Read at the router since the report of 2026-09-12**, which is where the
/// rule moved: `file_row_under` had it for the press alone, and the same
/// hole was still open for the hover. What is left to pin here is that this
/// door consumes the router's answer rather than keeping a second opinion
/// about floats of its own.
///
/// Red gate: make the router answer `None` for a part instead of naming it
/// and the ordering assertion fails; drop `file_row_under`'s float arm and
/// the last one does.
#[test]
fn a_press_inside_a_float_never_names_a_docked_row() {
    let router = method_body("Runtime", "pointer_target_at");
    let claim = router
        .find("self.float_hit_at(position)")
        .expect("the float is asked first, because a float is drawn over the columns");
    let docked = router
        .find("self.docked_chrome_target_at(position)")
        .expect("the docked chrome is asked second");
    assert!(
        claim < docked,
        "the topmost window answers before the panes behind it"
    );
    assert!(
        router[claim..docked].contains("return Some(PointerTarget::Float(id, part));"),
        "the float is asked about the *point*, not about one part of it: an \
             answer that named only some parts would let every other part of an \
             opaque window fall through to the chrome behind it"
    );
    let body = method_body("Runtime", "file_row_under");
    assert!(
        !body.contains(["self.float_hit_at", "("].concat().as_str()),
        "the menu's door asks the one router and never the windows directly: \
             a second reading of the floats is a second rule to forget, and \
             forgetting it once is what left the hover transparent"
    );
    assert!(
        body.contains("Some(PointerTarget::Float(..)) => return None,"),
        "and a part this door has no answer for ends the question, rather \
             than passing it to the row hidden under the window"
    );
}

/// RED (the same report, the other side of it) — **a window's own rows still
/// hover, and still peek.**
///
/// The fix must not be "a float swallows the pointer": a floating tree is a
/// list you read with the mouse, and its rows have answered a resting hand
/// with a glance since P150. What the window claims it also answers for.
///
/// Red gate: have the router return `None` for a claimed point instead of
/// the part, and the first two assertions fail by name.
#[test]
fn a_hover_over_a_floating_trees_row_is_that_rows_hover() {
    let router = method_body("Runtime", "pointer_target_at");
    assert!(
        router.contains("Some(PointerTarget::Float(id, part))"),
        "the router carries the part the window answered with, rather than \
             the bare fact that something is in the way"
    );
    let rows = method_body("Runtime", "row_under");
    assert!(
        rows.contains("Some((RowHost::Float(id), index))"),
        "so a row of a floating tree is still that float's row"
    );
    let peek = method_body("Runtime", "peek_row");
    assert!(
        peek.contains("RowHost::Column(_) | RowHost::Float(_) =>"),
        "and the glance card resolves it on either host, exactly as it did"
    );
}

/// RED (the same report) — **the pane a window is standing on is not the
/// hovered pane.**
///
/// `.pane:hover` is asked separately from the chrome target, against the
/// seat layout, because over a terminal's body the chrome answer is `None`
/// and that is most of a pane. The seat layout is the *docked* geometry and
/// knows nothing about the windows drawn over it — so a pane a float was
/// covering went on wearing the hover, and with it the four head controls
/// that are only drawn while the hand is inside that pane.
///
/// Red gate: drop the float subtraction and the first assertion fails by
/// name; drop the panel's and the second does.
#[test]
fn the_pane_under_a_float_is_not_the_hovered_pane() {
    let hover = method_body("Runtime", "update_chrome_hover");
    let pane = hover
        .find("seats::pane_at(")
        .expect("`.pane:hover` is resolved against the seat layout");
    assert!(
        hover[..pane].contains("!matches!(target, Some(PointerTarget::Float(..)))"),
        "the window's claim is subtracted from `.pane:hover` too, because \
             the seat layout cannot subtract it for itself"
    );
    assert!(
        hover[..pane].contains("!self.panel_covers(position)"),
        "beside the open rail's own subtraction and not instead of it"
    );
}

/// RED (the same report, the root of it) — **the press and the hover ask one
/// router.**
///
/// This is the finding rather than the symptom. `b1cf054` put the rule at a
/// caller, so it held for the gesture that went through that caller and for
/// no other; the hover, the `.pane:hover` question and every "is the pointer
/// on *that* button" in this file kept reading a ladder that has never heard
/// of a floating window. The rule now lives in `pointer_target_at`, and the
/// only places left that ask a float about a point are the float's own
/// gestures — which is what this asserts, by name, so that the next door
/// onto this question cannot be opened quietly.
///
/// Red gate: call `float_hit_at` from anywhere else, or the ladder from
/// anywhere but the router, and this names the place.
#[test]
fn the_press_and_the_hover_ask_one_router() {
    // The needle used to be assembled rather than written out, because a
    // literal spelled in full here is itself a line of `main.rs` and the scan
    // would find its own needle. It is a question about *calls* now, and a
    // name inside a string literal is one token and not a call.
    assert_eq!(
        reader_names(&calls_of("Runtime", "float_hit_at")),
        [
            "close_hover_floats_except",
            "drive_float_hover",
            "mouse_wheel",
            "pointer_target_at",
            "press_float",
            "scroll_float_git_page",
            "scroll_float_tree",
        ],
        "a float is asked about a pointer *target* in one place — the \
             router — and everywhere else only about its own gestures: which \
             window the peek is, the window's own hover, its press, its wheel, \
             and the two scrollers that move rows under a still hand. The rail's \
             own pill was the eighth name here until 2026-09-13: it needed a \
             part no `ChromeTarget` could stand for, and `PopoverTrigger` is \
             that part with a name, read off the router like everything else"
    );
    assert_eq!(
        in_product(&calls_of("Runtime", "docked_chrome_target_at")),
        1,
        "and the docked ladder is reached through the router and nowhere else"
    );
    assert!(
        method_body("Runtime", "pointer_target_at")
            .contains("self.docked_chrome_target_at(position)"),
        "which is the router"
    );
}

/// RED (T-STRIP-HOVER-THROUGH, owner's report on 0.4.6, 2026-10-03) — **a
/// pane's notice strip owns the points it is drawn on, and the router says so.**
///
/// The strip is staged in `Layered::Notice`, over every pane's own chrome and
/// under every floating window. The router used to go from the floats straight
/// to the docked ladder, which knows nothing about strips, so a hand on the
/// strip's `×` was — to the chrome hover, the files flyout's trigger, the `⌄`'s
/// rest clock and the press router — a hand on whatever the band was covering,
/// while the strip's own hover and press walked a second list beside it.
///
/// Red gate: drop the strip's arm from `pointer_target_at` (or move it below
/// the ladder) and the ordering assertions fail; give `drive_notice_hover` or
/// `press_notice` back their own walk of `notice_layouts` and the last ones do.
#[test]
fn a_panes_notice_strip_is_asked_between_the_floats_and_the_docked_chrome() {
    let router = squeezed_body("Runtime", "pointer_target_at");
    let float = router
        .find("self.float_hit_at(position)")
        .expect("the floats are asked first");
    let strip = router
        .find("self.docked_notice_at(position)")
        .expect("a pane's strip is asked by the router");
    let docked = router
        .find("self.docked_chrome_target_at(position)")
        .expect("and the docked ladder last");
    assert!(
        float < strip && strip < docked,
        "the pointer is asked in the order the glass is painted: a window, then \
         a strip, then the chrome the strip covers"
    );
    assert!(
        router[strip..docked].contains(".map(|(seat,element)|PointerTarget::Notice(seat,element))")
            && router[strip..docked].contains("returnclaim;"),
        "and the strip's claim is the whole answer, so nothing under it is asked"
    );
    assert_eq!(
        reader_names(&calls_of("Runtime", "docked_notice_at")),
        ["pane_hit_context", "pointer_target_at"],
        "a pane's strip is placed in the order by the router, and read as a fact only by the cells' hover root, which cannot ask the router"
    );
    let at = squeezed_body("Runtime", "notice_at");
    assert!(
        at.contains("self.pointer_target_at(position)?"),
        "the strip a gesture lands on is the strip the router names"
    );
    for gesture in ["drive_notice_hover", "press_notice"] {
        let body = squeezed_body("Runtime", gesture);
        assert!(
            body.contains("self.notice_at("),
            "`{gesture}` asks the one router which strip is under the pointer"
        );
        assert!(
            !body.contains("notice_layouts"),
            "`{gesture}` keeps no second list of strips beside the router"
        );
    }
}

/// RED (independent review of `d0e62ef1`, 2026-10-04) — **every pointer reader
/// below the in-pane surfaces takes their claim from the router**: the wheel,
/// a press of any button (a rendered page's context menu included), the tip,
/// the root the cells' hovers draw from, the files flyout's reference trigger,
/// a playing video's bar and a hosted page's own hover.
///
/// Red gates: drop the wheel's in-pane station and a notch on the strip's frame
/// scrolls what is under it; let `press_in_pane_surface` take the left button
/// only and a right click on a pill opens the page menu under it; read the tip
/// off the flat list again and a hidden control's tip speaks through the layer
/// above it; drop the root's gate and a link under the capsule underlines.
#[test]
fn every_pointer_reader_below_the_in_pane_surfaces_reads_their_claim() {
    let wheel = squeezed_body("Runtime", "mouse_wheel");
    let station = wheel
        .find("letSome(surface)=self.in_pane_surface_at(position)")
        .expect("wheel: a notch asks the router whether a surface inside a pane owns it");
    for below in [
        "self.point_is_on_the_web_page(position)",
        "self.preview_surface_at(position)",
        "seats::files_body_at(",
    ] {
        assert!(
            wheel[station..].contains(below),
            "wheel: `{below}` is asked below the in-pane surfaces"
        );
    }
    let answer = &wheel[station..wheel.len().min(station + 200)];
    assert!(
        answer.contains("returnOk(());"),
        "wheel: a notch on a claimed point is nobody's"
    );
    let door = squeezed_body("Runtime", "press_in_pane_surface");
    assert!(
        door.contains("ifbutton==MouseButton::Left{") && door.ends_with("Ok(true)}"),
        "press: every button on a claimed point is taken, only the left one acts"
    );
    let moved = squeezed_body("Runtime", "pointer_moved");
    assert!(
        moved.contains("self.owned_tooltip_anchor_at(position)"),
        "tip: the pane-level tip is read through the router's owner"
    );
    let owned = squeezed_body("Runtime", "owned_tooltip_anchor_at");
    for arm in [
        "Some(InPaneSurface::SearchCapsule)=>capsule_control",
        "Some(InPaneSurface::NoticeStrip)=>false",
        "None=>!capsule_control",
    ] {
        assert!(owned.contains(arm), "tip: `{arm}`");
    }
    let root = squeezed_body("Runtime", "pane_hit_context");
    assert!(
        root.contains("self.search_part_at(position).is_some()")
            && root.contains("self.docked_notice_at(position).is_some()"),
        "cells: the root every cell hover draws from is gated by the in-pane surfaces"
    );
    let trigger = squeezed_body("Runtime", "float_trigger_at");
    assert!(
        trigger.contains(
            "Some(PointerTarget::Float(..)|PointerTarget::Search(_)|PointerTarget::Notice(..),)=>None,"
        ),
        "flyout: a reference under a claimed point raises no files card"
    );
    assert!(
        moved.contains("self.in_pane_surface_at(position).is_none();")
            && moved.contains("self.note_video_hover(on_the_picture.then_some(position));"),
        "video: a hand on a claimed point is not on the picture"
    );
    assert!(
        squeezed_body("Runtime", "drive_web_pointer")
            .contains("self.in_pane_surface_at(position).is_none()"),
        "page: a hand on a claimed point is not on the hosted page"
    );
}

/// RED (final review of `f5d0fc2b`) — **a forwarded gesture is delivered to
/// the shell it was handed to, and to no other.**
///
/// The route used to record a button and an encoding and nothing else, and its
/// moves and release went to whichever pane held the focus *then*. Run: the
/// press records its owner (tab, seat, incarnation); a focus moved to another
/// pane does not change it; and the owner's liveness — the paste address's own
/// rule — is false for a pane that closed, a shell restarted in the same seat
/// and a tab no longer on top. Read: the release and the drag's moves take the
/// seat from the route, ask whether the owner is live, and drop the route
/// without a byte when it is not.
///
/// Red gates: take the release's seat from `focused_leaf` again (focus moved
/// mid-drag) and the owner assertion fails; drop the liveness question from the
/// release or from the moves (owner closed mid-drag) and the drop assertion
/// naming that door fails.
#[test]
fn a_forwarded_gesture_is_delivered_to_the_shell_it_was_handed_to() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1002h\x1b[?1006h").unwrap();
    let owner = PasteTarget {
        tab: TabId(7),
        seat: SeatId(2),
        incarnation: 41,
    };
    let mut route = None;
    route_forwarded_mouse_button(
        &mut route,
        ElementState::Pressed,
        input::MouseProtocolButton::Left,
        bt_render::GridHit { row: 1, column: 1 },
        session.terminal_modes(),
        ModifiersState::empty(),
        PressedCellTarget::Ordinary,
        owner,
    )
    .expect("a tracked press is forwarded");
    // The focus moves to seat 3 while the button is held: nothing about the
    // route names the focus, so the gesture is still seat 2's.
    assert!(
        matches!(route, Some(MouseRoute::Forward { owner: recorded, .. }) if recorded == owner),
        "the press records the shell it was handed to"
    );
    assert!(
        paste_target_is_live(TabId(7), Some(41), owner),
        "while it stands, it is owed"
    );
    assert!(
        !paste_target_is_live(TabId(7), None, owner),
        "a closed pane is owed nothing"
    );
    assert!(
        !paste_target_is_live(TabId(7), Some(42), owner),
        "nor a shell restarted in the same seat"
    );
    assert!(
        !paste_target_is_live(TabId(8), Some(41), owner),
        "nor a pane whose tab is no longer on top"
    );

    let release = squeezed_body("Runtime", "release_owned_gesture");
    let motion = squeezed_body("Runtime", "forward_owned_drag_motion");
    for (door, body) in [("release", &release), ("motion", &motion)] {
        assert!(
            !body.contains("focused_leaf"),
            "{door}: the gesture is delivered to its recorded owner, not to the focus"
        );
        assert!(
            body.contains("letseat=owner.seat;"),
            "{door}: the seat is the route's"
        );
        assert!(
            body.contains(
                "ifself.live_paste_target(owner).is_none(){self.window.mouse_route=None;"
            ),
            "{door}: a gone owner drops the route with nothing sent"
        );
    }
    assert!(
        release.contains("self.send_mouse_input_to(seat,&bytes,"),
        "release: written into the owner's own pipe"
    );
    // A selection drag is addressed the same way.
    assert!(
        squeezed_body("Runtime", "extend_local_selection").contains(
            "ifself.live_paste_target(owner).is_none(){self.window.mouse_route=None;returnOk(());}letseat=owner.seat;"
        ),
        "selection: its moves go to its own shell, and a gone shell lets it go"
    );
    assert!(
        squeezed_body("Runtime", "mouse_input").contains(
            "letSome(owner)=self.paste_target(hit_seat)else{returnOk(());};self.begin_local_selection(owner,hit)"
        ),
        "selection: the press records its shell"
    );
    let press = squeezed_body("Runtime", "mouse_input");
    assert!(
        press.contains("letSome(owner)=self.paste_target(self.focused_leaf)else{returnOk(());};"),
        "the press records the shell it is handed to"
    );
}

/// RED (final review of `f5d0fc2b`) — **only the button that started a
/// forwarded gesture ends it**, and another button pressed and let go while it
/// is held is not forwarded under it.
///
/// Run: left down forwarded; right down and up (over an overlay, or anywhere);
/// left up. The child hears exactly a left press and a left release, and the
/// route stands until the left release and comes off there. Read: the owned
/// release hands a different button's release on down the ordinary road, and
/// the cell road takes no second button while a forwarded gesture is latched.
///
/// Red gates: drop the latched-button test from the release arm, or let a
/// second press overwrite the route, and the chord's byte list is wrong; drop
/// the owned release's button test and its assertion fails.
#[test]
fn a_chord_under_a_forwarded_gesture_leaves_it_whole() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1002h\x1b[?1006h").unwrap();
    let modes = session.terminal_modes();
    let at = bt_render::GridHit { row: 1, column: 2 };
    let mut route = None;
    let mut heard: Vec<Vec<u8>> = Vec::new();
    let mut step = |state, button| {
        if let Some(bytes) = route_forwarded_mouse_button(
            &mut route,
            state,
            button,
            at,
            modes,
            ModifiersState::empty(),
            PressedCellTarget::Ordinary,
            a_shell(),
        ) {
            heard.push(bytes);
        }
        matches!(
            route,
            Some(MouseRoute::Forward {
                button: input::MouseProtocolButton::Left,
                ..
            })
        )
    };
    assert!(step(
        ElementState::Pressed,
        input::MouseProtocolButton::Left
    ));
    assert!(
        step(ElementState::Pressed, input::MouseProtocolButton::Right),
        "a second button's press does not take the route"
    );
    assert!(
        step(ElementState::Released, input::MouseProtocolButton::Right),
        "and its release does not end the left gesture"
    );
    assert!(
        !step(ElementState::Released, input::MouseProtocolButton::Left),
        "the left release ends it"
    );
    assert_eq!(
        heard,
        [b"\x1b[<0;3;2M".to_vec(), b"\x1b[<0;3;2m".to_vec()],
        "the child hears exactly left down and left up"
    );
    assert!(route.is_none());

    let release = squeezed_body("Runtime", "release_owned_gesture");
    assert!(
        release.contains("ifprotocol_mouse_button(button)!=Some(latched){returnOk(false);}"),
        "the owned release hands another button's release on as an event of its own"
    );
    assert!(
        release.contains("ifbutton!=MouseButton::Left{returnOk(false);}"),
        "and a selection, begun by the left button, is ended only by it"
    );
    let road = squeezed_body("Runtime", "mouse_input");
    assert!(
        road.contains(
            "ifmatches!(self.window.mouse_route,Some(MouseRoute::Forward{..})){returnOk(());}match state"
                .replace(' ', "")
                .as_str()
        ),
        "the cell road begins no selection under a latched forwarded gesture"
    );
}

/// RED (final review of `f5d0fc2b`) — **a band painted above the panes owns the
/// wheel over its own area, whether or not it scrolls.**
///
/// Yielding the in-pane claim to the menus left the notch to fall through them:
/// a notch on a pane menu over a strip, or over plain terminal, scrolled the
/// terminal beneath. Every band of `OVER_IN_PANE_TOP_FIRST` now declares
/// ([`OverInPane::wheel`], an exhaustive match) whether its own station
/// answers the notch or it swallows it, and `mouse_wheel` asks the topmost one
/// before the hosted page and every pane — with a strip under the menu or not.
///
/// Red gates: remove the station, or declare a menu `OwnStation` (there is no
/// menu station for it to reach), and the assertion naming it fails.
#[test]
fn a_band_painted_above_the_panes_owns_the_wheel_over_its_area() {
    for band in OVER_IN_PANE_TOP_FIRST {
        let expected = match band {
            OverInPane::Palette | OverInPane::Float => OverWheel::OwnStation,
            _ => OverWheel::Swallow,
        };
        assert_eq!(band.wheel(), expected, "{band:?}");
    }
    let wheel = squeezed_body("Runtime", "mouse_wheel");
    let station = wheel
        .find("letSome(band)=self.topmost_band_over_in_pane_at(position){ifband.wheel()==OverWheel::Swallow{")
        .expect("the wheel asks the topmost band over the panes what it does with a notch");
    assert!(
        wheel[station..wheel.len().min(station + 260)].contains("returnOk(());"),
        "a band that swallows ends the notch there"
    );
    for (band, own) in [
        (
            OverInPane::Palette,
            "palette::wheel_part(&layout,position.x,position.y)",
        ),
        (OverInPane::Float, "self.float_hit_at(position)"),
    ] {
        let at = wheel
            .find(own)
            .unwrap_or_else(|| panic!("{band:?} has a station"));
        assert!(
            at > station,
            "{band:?}'s own station is below the gate that lets it through"
        );
    }
    for beneath in [
        "self.point_is_on_the_web_page(position)",
        "seats::files_body_at(",
        "self.preview_surface_at(position)",
        "self.in_pane_surface_at(position)",
    ] {
        let at = wheel
            .find(beneath)
            .unwrap_or_else(|| panic!("`{beneath}` is on the wheel's road"));
        assert!(
            at > station,
            "`{beneath}` is asked only after the bands above it"
        );
    }
    assert!(
        squeezed_body("Runtime", "topmost_band_over_in_pane_at")
            .contains("OVER_IN_PANE_TOP_FIRST.into_iter().find("),
        "the topmost band is read off the paint-ordered list"
    );
}

/// PIN — K144's leading space, read off the screen the shell drew.
///
/// The rule the mock-up could state by looking at its own model of the input
/// line, restated as something a real terminal can answer: the character
/// immediately left of the cursor. A prompt that already ends in a space
/// wants no second one; a half-typed word wants one; column zero has nothing
/// in front of it to be welded to.
#[test]
fn the_space_in_front_is_decided_by_what_the_cursor_is_standing_after() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
    assert!(
        !input_line_needs_a_space_first(&session),
        "column zero has nothing in front of it"
    );
    session.feed(b"PS D:\\work> ").unwrap();
    assert!(
        !input_line_needs_a_space_first(&session),
        "a prompt that ends in a space has already left the gap"
    );
    session.feed(b"cat").unwrap();
    assert!(
        input_line_needs_a_space_first(&session),
        "a half-typed command would otherwise have the path welded to it"
    );
    session.feed(b" ").unwrap();
    assert!(
        !input_line_needs_a_space_first(&session),
        "and typing the space yourself does not earn a second one"
    );
}

/// PIN (**the pointer has two doors and both reach the `⌄` clocks**) — user
/// report, 2026-08-21.
///
/// A hover-opened profile menu would not close once the pointer came to rest
/// in the title bar's empty band, the stretch between the `⌄` and the gear.
/// That band is `HTCAPTION` — `seats`'s
/// `the_title_bar_band_right_of_the_chevron_is_win32s_and_not_the_chevrons`
/// pins both halves of why — so winit reports the hand arriving there as one
/// `CursorLeft` and then nothing at all. `pointer_moved` was the only door
/// that told the gates anything, so the grace was never started, and a grace
/// that is never started never runs out: the menu stood over the terminal
/// until a click or an Esc took it down.
///
/// The fix is that `pointer_left` asks the same question, with the position
/// it has just cleared — `None`, which fails both hit tests and is therefore
/// `Away` for both chevrons without a single special case.
///
/// Read as **text**, for `mouse_trace_station_tests`' reason and only that
/// reason: what went wrong is *a door that says nothing*, and no state
/// machine can be driven into a state that nobody ever puts it in. What the
/// absence then means is asserted below against the machine itself.
///
/// Red gate: delete the call from `pointer_left` and this fails by name;
/// hand either door a bare position again and the `Some`/`None` assertion
/// for that door fails.
#[test]
fn both_pointer_doors_tell_the_chevron_clocks_where_the_hand_is() {
    let body = |name: &str| method_body("Runtime", name);
    assert!(
        body("pointer_moved").contains("self.observe_chevrons(Some(position)"),
        "the move door hands the gates the position it was handed"
    );
    assert!(
        body("pointer_left").contains("self.observe_chevrons(None,"),
        "the leave door hands them the absence — without it a menu opened \
             from the strip's `⌄` never closes over the title bar's drag band"
    );
    assert!(
        body("turn").contains("self.window.chevrons.deadline(),"),
        "and the loop is woken for the grace: a clock nobody calls `due` on \
             is a menu that closes on the next thing to twitch"
    );

    // What the absence means once it reaches the gates: both menus up, the
    // hand on neither, and one grace — the same 150ms a hand that merely
    // stepped off the button gets. This is why the fix is not "close the
    // menu when the cursor leaves the window": the leave is reported as a
    // *position*, and the policy that reads it is the one policy.
    let start = Instant::now();
    let mut left = ChevronGates::default();
    left.observe(
        (profiles::ChevronPointer::Away, true),
        (profiles::ChevronPointer::Away, true),
        (profiles::ChevronPointer::Away, true),
        start,
    );
    assert_eq!(left.deadline(), Some(start + profiles::CHEVRON_LEAVE_GRACE));
    assert_eq!(
        left.profile
            .due(start + profiles::CHEVRON_LEAVE_GRACE - Duration::from_millis(1)),
        None,
        "the grace is not skipped for a hand that has left the client rect"
    );
    for gate in [left.profile, left.pane, left.rail] {
        assert_eq!(
            gate.due(start + profiles::CHEVRON_LEAVE_GRACE),
            Some(profiles::ChevronAction::Close),
            "every hover-opening control answers a departure with the same \
                 verb at the same instant"
        );
    }
}

/// RED (35, owner follow-up 2026-09-23) — **a right click on a pane head
/// opens its menu pinned; leaving keeps it; Esc, a click elsewhere or a second
/// right click on the head close it.**
///
/// A right click is a click. The menu it raises used to close 150ms after the
/// hand left, because the pane gate governs the pane menu whoever raised it;
/// now the right-press arm pins what it opened, and a second right press on
/// the same head (its body, `×`, folder or `⌄`) is that menu's own button
/// press: it pins a peek and closes a pinned menu. A right press on another
/// head is a click elsewhere, and moves the menu.
///
/// MUTATION: delete `self.pin_the_chevron_menu_a_press_opened(Popup::Pane)`
/// after the right-press `open_pane_menu` in `mouse_input` and the source half
/// goes red; make `a_right_press_is_on_the_pane_menus_head` ignore the seat and
/// the other-head assertion does.
#[test]
fn a_right_click_opens_the_pane_menu_pinned_and_a_second_one_closes_it() {
    let router = method_body("Runtime", "mouse_input");
    assert!(
        router.contains("self.pin_the_chevron_menu_a_press_opened(Popup::Pane);"),
        "the right press that raises the pane menu pins it"
    );
    assert!(router.contains("a_right_press_is_on_the_pane_menus_head("));

    // Opened pinned; the hand leaving owes nothing, for as long as it is away.
    let start = Instant::now();
    let mut gates = ChevronGates::default();
    gates.gate(Popup::Pane).expect("governed").pin();
    for step in 1..=10u32 {
        let now = start + profiles::CHEVRON_LEAVE_GRACE * step;
        hand_leaves(&mut gates, Popup::Pane, now);
        assert_eq!(
            gates.deadline(),
            None,
            "a right-clicked menu never closes on leave"
        );
    }

    // A second right press on the same head, on any part of it: closed.
    let menu = Some(SeatId(4));
    for part in [
        seats::ChromeTarget::PaneHeader(SeatId(4)),
        seats::ChromeTarget::PaneClose(SeatId(4)),
        seats::ChromeTarget::PaneFiles(SeatId(4)),
        seats::ChromeTarget::PaneMenu(SeatId(4)),
    ] {
        assert!(a_right_press_is_on_the_pane_menus_head(menu, Some(part)));
    }
    assert_eq!(
        press_pins_a_peek(OwnPress::Spent, gates.gate(Popup::Pane)),
        OwnPress::Spent,
        "a second right press on the head closes the pinned menu"
    );
    gates.menu_gone(Popup::Pane);
    assert_eq!(gates, ChevronGates::default());

    // On a peek the same press pins it, as the `⌄` does.
    let mut peek = peek_open(Popup::Pane, start);
    assert_eq!(
        press_pins_a_peek(OwnPress::Spent, peek.gate(Popup::Pane)),
        OwnPress::Pinned
    );

    // Another head, the terminal, or no menu at all: a click elsewhere.
    assert!(!a_right_press_is_on_the_pane_menus_head(
        menu,
        Some(seats::ChromeTarget::PaneHeader(SeatId(5)))
    ));
    assert!(!a_right_press_is_on_the_pane_menus_head(menu, None));
    assert!(!a_right_press_is_on_the_pane_menus_head(
        None,
        Some(seats::ChromeTarget::PaneHeader(SeatId(4)))
    ));

    // Esc and a click elsewhere go through the closers that drop the pin
    // (`esc_closes_a_pinned_menu`, `a_click_elsewhere_closes_a_pinned_menu`).
    assert!(
        method_body("Runtime", "close_pane_menu")
            .contains("self.window.chevrons.menu_gone(Popup::Pane)")
    );
}

/// RED (35) — **only a press pins, and every door that takes a `⌄` menu away
/// drops its pin.**
///
/// The pin is written in two places and cleared in one function per gate.
/// Written: by the press rule (a press on a peek's own button,
/// `press_on_its_own_trigger`) and right after each button's opener in its
/// press arm (a press on a closed button). The rest-open in
/// `advance_chevrons` goes through the same openers and never pins, which is
/// the whole difference between a peek and a pinned menu. Cleared: by every
/// closer, row run and replacing opener of the three menus — a closing path
/// that forgot the bit would leave a pin behind that the next rest-open would
/// inherit, and while it stood a rest on another pane head would open nothing.
///
/// MUTATION: delete any one `menu_gone` call named below, or any one pinning
/// call, and that assertion goes red by name.
#[test]
fn only_a_press_pins_and_every_way_a_chevron_menu_goes_drops_the_pin() {
    let chrome = method_body("Runtime", "chrome_mouse_input");
    assert!(chrome.contains("self.pin_the_chevron_menu_a_press_opened(Popup::Pane)"));
    assert!(chrome.contains("self.pin_the_chevron_menu_a_press_opened(Popup::Profile)"));
    assert!(
        method_body("Runtime", "press_on_its_own_trigger").contains("press_pins_a_peek("),
        "a press on a peek's own button pins it through the one rule"
    );
    let advance = method_body("Runtime", "advance_chevrons");
    assert!(
        !advance.contains("pin"),
        "a rest-open is a peek: the clock never pins what it opens"
    );
    for (method, call) in [
        (
            "close_pane_menu",
            "self.window.chevrons.menu_gone(Popup::Pane)",
        ),
        (
            "run_pane_menu_row",
            "self.window.chevrons.menu_gone(Popup::Pane)",
        ),
        (
            "open_pane_menu",
            "self.window.chevrons.menu_gone(Popup::Pane)",
        ),
        (
            "toggle_profile_menu",
            "self.window.chevrons.menu_gone(Popup::Profile)",
        ),
        (
            "place_float",
            "self.window.chevrons.menu_gone(Popup::Profile)",
        ),
        (
            "close_file_menu",
            "self.window.chevrons.menu_gone(Popup::File)",
        ),
    ] {
        assert!(
            method_body("Runtime", method).contains(call),
            "{method} takes a `⌄` menu away or replaces it, and must drop its pin"
        );
    }
    // The submenu's own doors read no pin (B3): `Split with` opens and closes
    // exactly as it did, peeked or pinned.
    for method in ["drive_pane_menu_hover", "set_pane_submenu"] {
        assert!(
            !method_body("Runtime", method).contains("pin"),
            "{method} does not know whether the menu is pinned"
        );
    }
}

/// PIN (**a hand that has left the window is in no pane**) — user report
/// with two screenshots, 2026-08-24.
///
/// The head's hover run — the rule, `</>`, the pop-out, the lock, the `×`,
/// and on a terminal head the `⌄` and the folder beside it — is drawn off
/// `seat_pointer.pane_hover`, which is a *different* fact from
/// `seat_pointer.hover`: the first is which pane the pointer is in, the
/// second is which control it is on. [`Self::pointer_left`] cleared only the
/// second, so every one of those marks stayed lit over a window the hand had
/// left — and the hand leaves on every window resize, because the border it
/// is dragging is non-client. That is what made the run look as though it
/// came and went with the window's *width*: it was stuck on from the last
/// hover for the whole drag, and gone again the moment the pointer came back
/// down somewhere else.
///
/// `docs/DESIGN.md` §7.1.6/§7.7 state the ladder the same way for both
/// heads — 「pane 没被指着就一枚都不画」 — and three comments inside
/// `pointer_left` itself already refuse exactly this for the controls that
/// hang off `hover`: "a `×` still lit after the pointer has left the window
/// is a button claiming to be under a pointer that is not there".
///
/// The wiring is read as **text** for `both_pointer_doors_tell_the_chevron_
/// clocks_where_the_hand_is`' reason and only that reason: what went wrong
/// is a door that does not say something, and no painter can be driven into
/// a state nobody ever puts it in. What the door leaves behind is then
/// asserted against the type itself.
///
/// Red gate: drop the `left_the_window` call from `pointer_left` — or let it
/// go back to clearing `hover` alone — and this fails by name. What the
/// cleared state then means at the painter is `seats`' own
/// `the_three_buttons_do_not_fade_with_the_pane_and_the_tools_do` and
/// `the_rule_fades_in_with_the_group_it_introduces`, driven there directly.
#[test]
fn a_pointer_that_has_left_the_window_is_standing_in_no_pane() {
    assert!(
        method_body("Runtime", "pointer_left")
            .contains("self.window.seat_pointer.left_the_window()"),
        "the leave door drops the control the pointer was on AND the pane it \
             was standing in, through the one function that owns both — without \
             the second, every head's hover run stays lit over a window the hand \
             has left, which is what a window resize does on every drag of its \
             own border"
    );
    // The state that reaches the painter, asserted against the type rather
    // than against the text: a departure leaves neither fact behind.
    let mut pointer = seats::ChromePointer {
        hover: Some(seats::ChromeTarget::PaneClose(SeatId(1))),
        pane_hover: Some(SeatId(1)),
        ..seats::ChromePointer::default()
    };
    assert!(
        pointer.left_the_window(),
        "something was lit and is not now"
    );
    assert_eq!((pointer.hover, pointer.pane_hover), (None, None));
    assert!(
        !pointer.left_the_window(),
        "and a second departure changes no picture"
    );
}

/// PIN (user report, 2026-08-19) — **one hover panel at a time: while any of
/// them is on the glass, no other one's clock runs.**
///
/// The report was a screenshot with a pane head's `⌄` menu and a files
/// flyout overlapping. Neither had a bug in it — the flyout's leftward
/// closing grace is 420ms, the chevron's rest is 250, and 250 is inside 420,
/// so a hand walking off one onto the other holds both open by arithmetic.
/// [`Popup`]'s own list is the precedent and this is the same shape one
/// register down.
///
/// Driven over **every ordered pair**, because "at most one" is a claim
/// about all of them and a rule that happened to be right for the pair in
/// the screenshot is not the rule. And the three claims are separate:
///
/// 1. Nothing up means every clock may run — a window with a free hand pays
///    nothing for this rule.
/// 2. Something up means only *that* one's clock may run. The panel already
///    on the glass is never blocked by itself, which is what lets the same
///    gate be asked on every pointer move without shutting the thing it is
///    protecting.
/// 3. `Menu` wins whenever it is up, because a press outranks a hover — the
///    ruling's 点击开启的菜单优先级高于 hover 浮层.
///
/// Red gate: return `true` unconditionally from `free` and claim 2 names the
/// pair that got through; sort `ALL` with `Menu` anywhere but first and
/// claim 3 goes red on the pairs a press is supposed to win.
#[test]
fn only_one_hover_panel_may_be_on_the_glass_at_a_time() {
    assert_eq!(
        HoverFloat::ALL.len(),
        4,
        "four hover panels, and this list is the rule"
    );
    assert_eq!(
        HoverFloat::ALL[0],
        HoverFloat::Menu,
        "a menu is a press's answer and outranks every hover panel"
    );
    for keep in HoverFloat::ALL {
        let closed: Vec<HoverFloat> = keep.others().collect();
        assert_eq!(closed.len(), HoverFloat::ALL.len() - 1);
        assert!(!closed.contains(&keep));
    }

    // Nothing up: every clock is free to run.
    assert_eq!(HoverFloat::holding(|_| false), None);
    for who in HoverFloat::ALL {
        assert!(who.free(|_| false), "{who:?} has nothing to wait for");
    }

    // One up: that one, and only that one, may go on running.
    for held in HoverFloat::ALL {
        assert_eq!(
            HoverFloat::holding(|who| who == held),
            Some(held),
            "{held:?} holds the glass on its own"
        );
        for who in HoverFloat::ALL {
            assert_eq!(
                who.free(|other| other == held),
                who == held,
                "{held:?} is up, so {who:?} must {} arm",
                if who == held { "still" } else { "not" }
            );
        }
    }

    // Two up at once — which is the state this rule exists to make
    // unreachable, and which a frame in the middle of a hand-off can still
    // pass through. Whoever leads the list is the one holding it, so the
    // answer is never "both may continue".
    for a in HoverFloat::ALL {
        for b in HoverFloat::ALL {
            if a == b {
                continue;
            }
            let held = HoverFloat::holding(|who| who == a || who == b)
                .expect("with two up, one of them holds it");
            assert!(held == a || held == b, "{a:?} and {b:?}: {held:?}");
            let free: Vec<HoverFloat> = HoverFloat::ALL
                .into_iter()
                .filter(|who| who.free(|other| other == a || other == b))
                .collect();
            assert_eq!(free, vec![held], "{a:?} against {b:?}");
            if a == HoverFloat::Menu || b == HoverFloat::Menu {
                assert_eq!(held, HoverFloat::Menu, "a press wins: {a:?} against {b:?}");
            }
        }
    }
}

/// RED GATE (user ruling 2026-09-07, `docs/DESIGN.md` §7.58) — **a file row
/// inside the folder card raises a glance, and it is the only row on the
/// glass that may.**
///
/// The report: a folder path printed in a pane opens the folder card, and a
/// hand resting on `report.md` inside it was answered by nothing at all —
/// while the same name in the files column, in a pinned window, or printed in
/// the output, answers with a preview. Nothing about the glance was wrong.
/// What refused was the rule above: the folder card is a
/// [`float::FloatMode::Peek`], so [`HoverFloat::Flyout`] was holding the
/// glass, and no glance may arm under a panel.
///
/// **The exception is a rule about independence, not a hole in the list.**
/// [`HoverFloat`] exists because two hover surfaces that know nothing of one
/// another end up half-covering each other; a glance a flyout's own row
/// raised knows a great deal about it — it is placed against that flyout's
/// frame ([`file_peek::PeekAnchor::row_in_a_card`]), it holds it open while
/// the hand is in it ([`Runtime::pointer_is_in_the_peeks_own_glance`]), and
/// it dies with it ([`Runtime::forget_dead_float_gestures`]). Two rectangles,
/// one region — the same thing [`file_peek::corridor`] already says about a
/// card and its row.
///
/// MUTATIONS that must turn this red:
///
/// * [`glance_may_arm`] delegating to [`HoverFloat::free`] again — ② goes
///   red, which is the defect exactly.
/// * the exception widened to any row — ① goes red: a files column under an
///   open folder card starts arming cards behind it.
/// * the exception widened past `Flyout` to whoever holds the glass — ③ goes
///   red and a press stops outranking a hover.
#[test]
fn a_row_inside_the_folder_card_may_raise_a_glance_while_that_card_holds_the_glass() {
    // ① The standing exclusion, untouched: a row anywhere *else* arms
    // nothing while a peek flyout is up.
    assert!(
        !glance_may_arm(Some(HoverFloat::Flyout), false),
        "a files column row under an open folder card still arms nothing"
    );
    // ② The ruling.
    assert!(
        glance_may_arm(Some(HoverFloat::Flyout), true),
        "but the folder card's own file row raises its glance"
    );
    // ③ A press outranks every hover, and the exception does not reach it.
    for inside in [false, true] {
        assert!(
            !glance_may_arm(Some(HoverFloat::Menu), inside),
            "a row under an open menu is a row under a menu ({inside})"
        );
    }
    // ④ Everything else is [`HoverFloat::free`] verbatim, which is what
    // makes this an exception rather than a second policy.
    let mut glass: Vec<Option<HoverFloat>> = vec![None];
    glass.extend(HoverFloat::ALL.map(Some));
    for held in glass {
        assert_eq!(
            glance_may_arm(held, false),
            HoverFloat::Glance.free(|who| held == Some(who)),
            "off the flyout, the list decides on its own: {held:?}"
        );
    }
    assert!(glance_may_arm(None, false), "a free hand on a free glass");
    assert!(
        glance_may_arm(Some(HoverFloat::Glance), true),
        "and a glance never blocks itself, wherever the row is"
    );
    assert!(
        !glance_may_arm(Some(HoverFloat::LayoutPeek), true),
        "the flyout is the one panel this exception is about"
    );
}

/// RED GATE (user ruling 2026-09-07, §7.58) — **the folder card stays open
/// while the glance it raised is showing, and that glance opens nothing
/// further.**
///
/// Two halves of one sentence, and both are facts about *where a question is
/// asked* rather than about a value, which is why they are read as text —
/// [`the_rail_zone_is_asked_before_a_gesture_can_swallow_the_move`]'s reason
/// exactly: what goes wrong is an asker that never runs, and no state machine
/// can be driven into a state nobody puts it in.
///
/// * **The pair stays up together.** The glance stands *outside* the folder
///   card, so walking into it is walking off the card by
///   [`float::peek_reach`]'s arithmetic, and the card's 220ms starts under a
///   hand that is reading the very thing it opened. The glance's own frame
///   has to count as part of the peek's region, beside the trigger and the
///   root menu that are already on that list.
/// * **One level only.** A hand inside a glance is inside the glance,
///   whatever rows are drawn under it — so
///   [`Runtime::observe_file_peek`] answers [`file_peek::Life::Held`] before
///   it ever looks at the row the pointer is over, and a glance can therefore
///   never raise a second one.
///
/// RED GATE: drop `pointer_is_in_the_peeks_own_glance` from
/// `drive_float_hover` and the first half fails by name; move the `Held` arm
/// below the dwell and the second does.
#[test]
fn the_folder_card_stays_up_under_the_glance_it_raised_and_that_glance_raises_nothing() {
    let body_of = |name: &str| method_body("Runtime", name);

    let hover = body_of("drive_float_hover");
    let asked = hover
        .find("self.pointer_is_in_the_peeks_own_glance(position)")
        .expect("the peek's region includes the glance its own row raised");
    let released = hover
        .find("self.window.float.release(")
        .expect("and otherwise the pointer has left and the grace starts");
    assert!(
        asked < released,
        "the glance is counted as part of the peek before the peek is let \
             go — otherwise reaching into the card the folder card just opened \
             starts the folder card's dismissal"
    );

    let observe = body_of("observe_file_peek");
    let held = observe
        .find("Some(file_peek::Life::Held) => return self.keep_file_peek(),")
        .expect("a pointer inside the card is inside the card");
    for later in [
        "self.dwell_file_peek(host, now);",
        "self.armed_file_peek(host, index, now)",
    ] {
        let at = observe
            .find(later)
            .unwrap_or_else(|| panic!("{later} is how a row takes a card"));
        assert!(
            held < at,
            "the card's own face is answered before {later} — one level \
                 only: a row drawn under a glance raises nothing"
        );
    }

    // And the card goes when the window that raised it does: a glance left
    // standing where its folder card used to be is about a place the reader
    // can no longer see, and has no frame left to be placed against.
    let forget = body_of("forget_dead_float_gestures");
    assert!(
        forget.contains("Some(RowHost::Float(id)) = self.window.file_peek.as_ref()"),
        "a glance raised inside a float is one of that float's gestures"
    );
}

// ── U6: the plan, its cache, and the word in the box (M148, M155, L137) ──

fn inputs_of(landing: DropLanding, source: &DragSource) -> PlanInputs {
    PlanInputs {
        landing,
        source: source.clone(),
        tree: LayoutNode::seat(bt_layout::Seat::new(
            bt_layout::SeatId(1),
            bt_layout::SeatKind::Terminal,
        )),
        cargo: None,
        viewport: LogicalRect::from_px(1600, 900),
        scale_ppm: 1_000_000,
    }
}

/// **The cache is over a computation, and its key is every input the
/// computation has.**
///
/// "Re-plan only when the landing moves" is the behaviour, but the landing is
/// not the whole question: a window resized under a still pointer, or a DPI
/// change mid-drag, changes the layout the plan describes without the pointer
/// moving at all. Keying on the landing alone leaves the promise on screen
/// describing a layout that no longer exists — a stale promise is exactly what
/// M148 is about, arrived at from the other direction.
#[test]
fn the_plan_stands_while_its_question_does_and_falls_when_anything_moves() {
    let pane = DragSource::Pane(LeafId {
        tab: TabId(1),
        seat: SeatId(1),
    });
    let landing = DropLanding::SeatEdge {
        target: bt_layout::SeatId(2),
        edge: seats::DropEdge::Right,
    };
    let base = inputs_of(landing, &pane);
    assert_eq!(base, inputs_of(landing, &pane), "the same question, twice");

    let moved_zone = inputs_of(
        DropLanding::SeatEdge {
            target: bt_layout::SeatId(2),
            edge: seats::DropEdge::Left,
        },
        &pane,
    );
    assert_ne!(
        base, moved_zone,
        "the other side of the same pane is a new plan"
    );

    let mut resized = inputs_of(landing, &pane);
    resized.viewport = LogicalRect::from_px(1200, 900);
    assert_ne!(
        base, resized,
        "a resize re-plans without the pointer moving"
    );

    let mut rescaled = inputs_of(landing, &pane);
    rescaled.scale_ppm = 1_500_000;
    assert_ne!(base, rescaled, "and so does a DPI change");

    let mut edited = inputs_of(landing, &pane);
    edited.tree = LayoutNode::split(
        bt_layout::SplitId(1),
        Axis::Row,
        LayoutNode::seat(bt_layout::Seat::new(
            bt_layout::SeatId(1),
            bt_layout::SeatKind::Terminal,
        )),
        LayoutNode::seat(bt_layout::Seat::new(
            bt_layout::SeatId(2),
            bt_layout::SeatKind::Terminal,
        )),
    );
    assert_ne!(base, edited, "a tree that changed under the drag re-plans");

    let carried = inputs_of(landing, &DragSource::Tab(TabId(7)));
    assert_ne!(
        base, carried,
        "the same zone means a different thing depending on what is in the hand"
    );
}

/// **L137, as B4 leaves it — the centre says its name, and the name is a
/// fact about what is in the hand.**
///
/// The centre's box is the same rectangle an edge's is; a pane trades places
/// with the target and a whole tab takes its place outright, and the geometry
/// cannot tell you which. Every other zone answers with nothing, because its
/// shape has already spoken.
///
/// **The third row is gone** (user ruling 2026-08-25). Between 2026-08-23 and
/// this ruling a pane the spring had left in another tab said `Replace pane`
/// — it arrived as a subtree, and a subtree at a centre evicted. The ruling
/// makes every pane's centre a trade, so the word no longer turns on a
/// distinction a reader cannot see: **a pane swaps, a tab replaces**, at home
/// and away alike.
///
/// Red gate: reintroduce the subtree test and a foreign pane's box goes back
/// to promising an eviction the commit no longer performs.
#[test]
fn only_the_centre_says_a_word_and_it_depends_on_what_is_in_the_hand() {
    let target = bt_layout::SeatId(2);
    // The pane under the pointer, which a *pane* and a *tab* say their word
    // without consulting: a terminal's middle is the one kind a row now
    // reads (the 2026-09-16 ruling), so aiming these two at one is what
    // states that the other two sources are untouched by it.
    const TERM: Option<bt_layout::SeatKind> = Some(bt_layout::SeatKind::Terminal);
    let pane = DragSource::Pane(LeafId {
        tab: TabId(1),
        seat: SeatId(1),
    });
    let elsewhere = DragSource::Pane(LeafId {
        tab: TabId(9),
        seat: SeatId(1),
    });
    let tab = DragSource::Tab(TabId(1));
    assert_eq!(
        DropLanding::SeatCentre { target }.caption(&pane, TERM),
        "Swap panes",
        "a pane trades places with the target (L138)"
    );
    assert_eq!(
        DropLanding::SeatCentre { target }.caption(&elsewhere, TERM),
        "Swap panes",
        "B4: and so does a pane whose tab is not the one on screen — where it \
             came from is not something the box may say two different words about"
    );
    assert_eq!(
        DropLanding::SeatCentre { target }.caption(&tab, TERM),
        "Replace pane",
        "a whole tab still takes the target's place outright (L139)"
    );
    for landing in [
        DropLanding::SeatEdge {
            target,
            edge: seats::DropEdge::Right,
        },
        DropLanding::RootRim {
            edge: seats::DropEdge::Top,
        },
        DropLanding::StripExtract { slot: 0 },
        DropLanding::StripReorder { slot: 0 },
    ] {
        assert_eq!(
            landing.caption(&pane, TERM),
            "",
            "{landing:?} draws its own meaning"
        );
        assert_eq!(landing.caption(&tab, TERM), "");
    }
}

/// A landing's aim is the one it was read off — and the strip's two never had
/// one, which is what keeps the dock box off the strip entirely.
#[test]
fn a_landings_aim_is_the_aim_it_came_from() {
    let seat = bt_layout::SeatId(4);
    for aim in [
        seats::LayoutAim::Rim(seats::DropEdge::Bottom),
        seats::LayoutAim::SeatEdge(seat, seats::DropEdge::Left),
        seats::LayoutAim::SeatCentre(seat),
    ] {
        let landing = landing_for_aim(&DragSource::Tab(TabId(1)), TabId(1), aim)
            .expect("a tab may land on any of them");
        assert_eq!(landing.layout_aim(), Some(aim));
    }
    assert_eq!(DropLanding::StripExtract { slot: 2 }.layout_aim(), None);
    assert_eq!(DropLanding::StripReorder { slot: 2 }.layout_aim(), None);
}

/// **M147 — only the aimed-at pane can be traced**, and the rim has none.
#[test]
fn a_refusal_points_at_a_pane_only_when_the_gesture_had_one() {
    let seat = bt_layout::SeatId(3);
    assert_eq!(
        DropLanding::SeatEdge {
            target: seat,
            edge: seats::DropEdge::Top,
        }
        .aimed_at(),
        Some(seat)
    );
    assert_eq!(
        DropLanding::SeatCentre { target: seat }.aimed_at(),
        Some(seat)
    );
    assert_eq!(
        DropLanding::RootRim {
            edge: seats::DropEdge::Top,
        }
        .aimed_at(),
        None,
        "the rim asked to divide the whole layout, so there is no pane to name"
    );
}

// ── P81-P88: the row drag and its verb table ───────────────────────────

fn file_row(name: &str) -> RowPayload {
    RowPayload {
        kind: RowPayloadKind::File,
        path: PathBuf::from("C:\\work").join(name),
        name: name.to_owned(),
    }
}

fn folder_row(name: &str) -> RowPayload {
    RowPayload {
        kind: RowPayloadKind::Folder,
        path: PathBuf::from("C:\\work").join(name),
        name: name.to_owned(),
    }
}

/// PIN — **a file row's four cells, as the 2026-09-16 ruling leaves them.**
///
/// P82's table with one cell moved: edges split out a preview, a preview's
/// centre shows it there, **a terminal's centre pastes its path**, and every
/// other centre refuses honestly.
///
/// **The terminal cell is the ruling, and the ruling is a revision.** The
/// 2026-07-17 pass cut path insertion off this gesture because it "made one
/// gesture speak two languages (space verbs vs text verbs)"; the 2026-09-16
/// pass puts it back on the finding that the objection was about *silence*
/// rather than about the two languages — the drop target now says which
/// language it speaks before the hand opens, with the split preview on the
/// outer band and `Paste path` on the middle. The two are still two
/// languages; they are no longer two languages a reader has to guess
/// between.
///
/// The refusal on a *files* centre is this build's own addition and the
/// 2026-08-13 ruling: the mock-up offers "Save into this folder" there, but
/// only for a payload carrying `drag.save`, which is a flag a **terminal
/// artifact** has and a tree row does not. A file dragged out of the tree is
/// already on the disk.
///
/// Mutation: make the `SeatCentre` arm answer `Retarget` for any target kind
/// — the terminal would open in a preview it has not got and the files
/// column would accept a file, and two of these assertions fail at once.
/// Mutation: answer `PastePath` for a files centre too — the third
/// assertion's neighbour fails, and a drop on a column would type into a
/// shell it is not.
#[test]
fn a_file_row_splits_at_an_edge_shows_in_a_preview_pastes_into_a_terminal() {
    for landing in [
        DropLanding::SeatEdge {
            target: TARGET,
            edge: seats::DropEdge::Left,
        },
        DropLanding::RootRim {
            edge: seats::DropEdge::Bottom,
        },
    ] {
        assert_eq!(
            row_verb(
                RowPayloadKind::File,
                landing,
                Some(bt_layout::SeatKind::Terminal)
            ),
            RowVerb::Split,
            "an edge is a space verb whatever pane it was aimed at: {landing:?}"
        );
    }
    assert_eq!(
        row_verb(
            RowPayloadKind::File,
            centre(),
            Some(bt_layout::SeatKind::Preview)
        ),
        RowVerb::Retarget(TARGET),
        "a preview's centre shows it here"
    );
    assert_eq!(
        row_verb(
            RowPayloadKind::File,
            centre(),
            Some(bt_layout::SeatKind::Terminal)
        ),
        RowVerb::PastePath(TARGET),
        "2026-09-16: a terminal's centre pastes the path, into the pane the \
             pointer named and not into the one holding the keyboard"
    );
    assert_eq!(
        row_verb(
            RowPayloadKind::File,
            centre(),
            Some(bt_layout::SeatKind::Files)
        ),
        RowVerb::Refused,
        "2026-08-13: a tree row carries no save verb, so a tree's centre refuses too"
    );
    assert_eq!(
        row_verb(
            RowPayloadKind::File,
            centre(),
            Some(bt_layout::SeatKind::Placeholder)
        )
        .content_target(),
        None,
        "a leaf this build cannot read is not a shell to type into either"
    );
}

/// PIN — **S3: a folder carries place verbs, and only place verbs.**
///
/// The mirror image of the file's table and the same shape: edges split out
/// a tree, a *tree's* centre re-roots that column ("Root this tree here",
/// 7187-7202), and everything else refuses. A folder on a preview's centre
/// is refused for the file-on-a-tree case's reason read backwards: a preview
/// shows a document, and a folder is not one.
///
/// Mutation: let the folder's `Retarget` arm accept `SeatKind::Preview` as
/// well — the third assertion fails, and a preview pane would be asked to
/// open a directory as a file.
#[test]
fn a_folder_row_splits_at_an_edge_roots_a_tree_and_is_refused_everywhere_else() {
    assert_eq!(
        row_verb(
            RowPayloadKind::Folder,
            DropLanding::SeatEdge {
                target: TARGET,
                edge: seats::DropEdge::Right,
            },
            Some(bt_layout::SeatKind::Preview)
        ),
        RowVerb::Split
    );
    assert_eq!(
        row_verb(
            RowPayloadKind::Folder,
            centre(),
            Some(bt_layout::SeatKind::Files)
        ),
        RowVerb::Retarget(TARGET),
        "S3's third verb: the centre of a tree re-roots it"
    );
    assert_eq!(
        row_verb(
            RowPayloadKind::Folder,
            centre(),
            Some(bt_layout::SeatKind::Preview)
        ),
        RowVerb::Refused,
        "a page is not a place"
    );
    assert_eq!(
        row_verb(
            RowPayloadKind::Folder,
            centre(),
            Some(bt_layout::SeatKind::Terminal)
        ),
        RowVerb::Refused
    );
    assert_eq!(
        row_verb(RowPayloadKind::Folder, centre(), None),
        RowVerb::Refused,
        "a centre aimed at a seat the tree does not have is not a verb either"
    );
}

/// PIN — **L141/L142: a row's centre says its name**, and since the
/// 2026-09-16 ruling there are three names to say.
///
/// The centre box is the same blue rectangle for five different outcomes,
/// and the three a row can ask for move nothing at all — so the shape has
/// *even less* to say than it does for a pane swap. An edge, a rim and the
/// strip stay silent, because their shapes already spoke.
///
/// **`Paste path` is the one the new ruling rests on.** The 2026-07-17
/// ruling cut this zone because one gesture must not mean two families of
/// thing *without feedback*; the word on the box is that feedback, so a
/// caption that went missing here would not be a cosmetic loss — it would be
/// the reason the zone was reopened, gone.
///
/// Mutation: give every row kind one shared caption — the second assertion
/// fails, and a folder over a tree would be promising to open a document.
/// Mutation: read the caption off the payload alone instead of off
/// [`row_verb`] — the terminal and the preview promise the same thing, and
/// one of the two boxes is lying.
#[test]
fn a_rows_centre_says_which_of_its_three_verbs_it_means() {
    const PREVIEW: Option<bt_layout::SeatKind> = Some(bt_layout::SeatKind::Preview);
    const TERMINAL: Option<bt_layout::SeatKind> = Some(bt_layout::SeatKind::Terminal);
    const FILES: Option<bt_layout::SeatKind> = Some(bt_layout::SeatKind::Files);
    let file = DragSource::Row(file_row("notes.md"));
    let folder = DragSource::Row(folder_row("src"));
    // A row's cargo is neither shape the flag is about — it has no tree of
    // its own and it is not a seat of this one — so its verbs are read off
    // the payload and the pane, and the flag says nothing here either way.
    assert_eq!(centre().caption(&file, PREVIEW), "Open in this preview");
    assert_eq!(
        centre().caption(&folder, FILES),
        "Root the files column here"
    );
    assert_eq!(
        centre().caption(&file, TERMINAL),
        "Paste path",
        "the 2026-09-16 ruling: the middle of a terminal says what it will do"
    );
    // And every centre the table refuses says nothing, which is the half
    // that keeps the word from wandering onto a box that will not happen.
    for (source, kind) in [
        (&file, FILES),
        (&folder, PREVIEW),
        (&folder, TERMINAL),
        (&file, None),
    ] {
        assert_eq!(
            centre().caption(source, kind),
            "",
            "a refused centre is traced and wordless: {kind:?}"
        );
    }
    for landing in [
        DropLanding::SeatEdge {
            target: TARGET,
            edge: seats::DropEdge::Top,
        },
        DropLanding::RootRim {
            edge: seats::DropEdge::Left,
        },
        DropLanding::StripExtract { slot: 0 },
    ] {
        assert_eq!(
            landing.caption(&file, TERMINAL),
            "",
            "{landing:?} draws its own meaning"
        );
    }
}

/// PIN — **K135 belongs to panes, and a row is nobody's pane.**
///
/// The "never onto yourself" test is an identity comparison against the
/// *seat* in the hand, and a row has none: every zone of every pane is open
/// to it, including the centre of the very column it was dragged out of —
/// where it is then refused by the verb table rather than by the aim. Two
/// different sentences about two different things, and folding them would
/// make a file dropped on its own tree silently indistinguishable from a
/// file dropped on open air.
///
/// Mutation: have `DragSource::pane()` answer `Some` for a row — the row
/// loses whichever pane that names, and the drag develops a blind spot
/// nothing in the table explains.
#[test]
fn a_row_is_no_ones_pane_so_every_zone_is_open_to_it() {
    let row = DragSource::Row(file_row("main.rs"));
    assert_eq!(row.pane(), None);
    for aim in [
        seats::LayoutAim::SeatCentre(TARGET),
        seats::LayoutAim::SeatEdge(TARGET, seats::DropEdge::Left),
        seats::LayoutAim::Rim(seats::DropEdge::Top),
    ] {
        assert!(
            landing_for_aim(&row, TabId(1), aim).is_some(),
            "a row has a landing in every zone: {aim:?}"
        );
    }
}

/// PIN — a row payload travels as its identity (P87).
///
/// "a FILE travels as its identity (the path); **the drop target decides the
/// verb** — reference(terminal), view(preview), split(edge)". So the payload
/// carries the path and the name and nothing about what will happen to it,
/// and the two are not interchangeable: two files called `main.rs` in two
/// folders are two payloads.
///
/// Mutation: key `RowPayload`'s equality on the name alone — the second
/// assertion fails, and a drop would open whichever `main.rs` the pool
/// happened to hold.
#[test]
fn two_files_with_one_name_are_two_payloads() {
    let a = RowPayload {
        kind: RowPayloadKind::File,
        path: PathBuf::from("C:\\a\\main.rs"),
        name: "main.rs".to_owned(),
    };
    let b = RowPayload {
        kind: RowPayloadKind::File,
        path: PathBuf::from("C:\\b\\main.rs"),
        name: "main.rs".to_owned(),
    };
    assert_eq!(a.name, b.name);
    assert_ne!(a, b, "the path is the identity, not the name");
    assert_ne!(
        DragSource::Row(a.clone()),
        DragSource::Row(b),
        "and the drag carries the identity"
    );
    assert_eq!(DragSource::Row(a.clone()).row(), Some(&a));
    assert_eq!(
        DragSource::Pane(LeafId {
            tab: TabId(1),
            seat: TARGET
        })
        .row(),
        None
    );
}

/// PIN — **the two landings are spent by the row's own two commits.**
///
/// [`release_verdict`] is a fact about the *landing* and stays one — the run
/// gains an entry, or an entry is aimed at, whatever the hand holds — so the
/// payload is read one step further in. That split is what lets a row reuse
/// the whole strip: the verdict, the stand-in, the spring and the clamp were
/// all written about a landing.
///
/// Mutation: send a row's verdict through `commit_pane_extract`, whose
/// `let DragSource::Pane(..) else` answers `false`, and the release goes to
/// J120's settle — a ghost that lands nowhere, which is report #200 again.
#[test]
fn a_rows_two_landings_are_spent_by_its_own_two_commits() {
    assert_eq!(
        release_verdict(Some(DropLanding::StripExtract { slot: 2 })),
        DragRelease::Extract { slot: 2 }
    );
    assert_eq!(
        release_verdict(Some(DropLanding::StripAdopt { tab: TabId(4) })),
        DragRelease::Adopt { tab: TabId(4) }
    );
    let extract = method_body("Runtime", "commit_strip_extract");
    assert!(
        extract.contains("self.commit_row_into_new_tab(&payload, slot)"),
        "a row over the padding makes a tab out of a path:\n{extract}"
    );
    assert!(
        extract.contains("self.commit_pane_extract(drag, slot)"),
        "and a pane still tears out the way N157 says:\n{extract}"
    );
    let adopt = method_body("Runtime", "commit_strip_adopt");
    assert!(
        adopt.contains("self.commit_row_into_tab(&payload, target)"),
        "a row on an entry is opened in that tab:\n{adopt}"
    );
    assert!(
        adopt.contains("self.commit_pane_adopt(drag, target)"),
        "and a pane is still adopted the way §7.1.6k says:\n{adopt}"
    );
}

/// PIN — **§7.1.6b‴'s band moves itself under a row now**, and ③'s copy of
/// the refusal went with ③.
///
/// The auto-scroll used to turn a row away in its own words: *"没有任何 tab
/// 面会接一个文件行,所以也没有任何 tab 面有理由为它挪动自己"*. The premise is
/// what changed, so the gate is deleted rather than inverted — this function
/// asks nothing about what is in the hand, which is the shape it had before
/// ③ needed saying.
///
/// Mutation: put the gate back and a file carried to the foot of a card
/// column taller than the window can only ever reach the cards already on
/// screen.
#[test]
fn the_edge_autoscroll_no_longer_asks_what_is_in_the_hand() {
    let text = method_body("Runtime", "drag_autoscroll_aim");
    assert!(
        !text.contains("DragSource::"),
        "every payload the surfaces accept is offered the band, and they \
             accept all three now:\n{text}"
    );
}

/// **A pin is not a permission, at either end** (`plan.md` §3「钉不是授权」).
///
/// Twice, because a pin passes through two moments and a check at only one of
/// them is a hole: a target that fails now never reaches `pins.json`, and a
/// target already in the file is asked again every time somebody presses it —
/// which is the only check that catches a row written by an older build,
/// edited by hand, or pinned before the policy tightened.
///
/// Red gate: return `Some(target.to_owned())` from `switcher_row_destination`
/// and every one of the refused strings below is navigated to.
#[test]
fn a_pinned_page_is_asked_at_the_pin_and_asked_again_at_the_press() {
    for hostile in [
        "javascript:alert(1)",
        "data:text/html,<h1>x</h1>",
        "file:///C:/Windows/System32/drivers/etc/hosts",
        "view-source:http://localhost:5173/",
        "about:blank",
    ] {
        assert!(
            !switcher_pin_is_allowed(bt_persist::PinKind::Url, hostile),
            "a page this window will not go to never reaches pins.json: {hostile}"
        );
        assert_eq!(
            switcher_row_destination(hostile),
            None,
            "and a row hand-edited into it does not navigate: {hostile}"
        );
    }
    // The two that are allowed, and the second is the point of asking twice:
    // it is already in the file and is checked again anyway.
    assert!(switcher_pin_is_allowed(
        bt_persist::PinKind::Url,
        "http://localhost:5173/app?tab=logs#top"
    ));
    assert_eq!(
        switcher_row_destination("http://localhost:5173/app?tab=logs#top"),
        Some("http://localhost:5173/app?tab=logs#top".to_owned()),
        "query and fragment carried through the gate untouched"
    );
    // A file pin is not asked, because whether a path is still there is a
    // filesystem question this store deliberately does not ask.
    assert!(switcher_pin_is_allowed(
        bt_persist::PinKind::File,
        r"C:\work\notes.md"
    ));
}

/// RED — **the speaker is a second channel and not a sixth claim** (user
/// ruling 2026-08-27; §7.23 ⑩, under §7.1.5b's taxonomy).
///
/// §7.1.5b's ladder is seven readings of one question — *does this session
/// want you* — resolved by taking the loudest, and the whole discipline of the
/// dot is that one dot carries one assertion. A sound is not an answer to that
/// question: it is not about a session, it wants nothing, and it is neither
/// more nor less urgent than an unread exit code. A tab can be audible *and*
/// awaiting a reply, and both are true at once — so folding audio into
/// [`StatusClaim`] would mean choosing between two things that do not compete.
///
/// So the mark is a control in the trailing run, beside the pin and the `×`,
/// with its own shape and its own press; and the dot's discipline is kept
/// precisely by leaving it alone. This asserts both: the ladder is the five it
/// was, and the press goes to the tab.
///
/// **The press does not mute**, and that is a ruling rather than an omission.
/// A muted tab looks exactly like a paused one, and the only way back is to
/// find the same small glyph again — while the controls that actually stop the
/// sound are under the video, where they have been all along. What a reader
/// wants when they hear something unexpected is to *see* it.
///
/// RED GATE ①: add an audible variant to [`StatusClaim`] and the first block
/// fails. RED GATE ②: make the speaker's arm mute instead of switching and the
/// last block fails.
#[test]
fn the_speaker_is_a_second_channel_and_takes_you_to_the_sound() {
    assert_eq!(
        loudest_claim([StatusClaim::Awaiting, StatusClaim::Unread]),
        StatusClaim::Awaiting,
        "the ladder still resolves by loudness"
    );
    assert_eq!(
        loudest_claim([StatusClaim::Silent]),
        StatusClaim::Silent,
        "and a silent session still makes no claim, however loud its page is"
    );
    let router = method_body("Runtime", "chrome_mouse_input");
    let at = router
        .find(concat!("ChromeTarget::TabSpeaker", "(index) => {"))
        .expect("the speaker has a press");
    let arm = &router[at..at + 200];
    assert!(
        arm.contains("self.activate_tab("),
        "the speaker takes you to the tab that is making the sound:\n{arm}"
    );
    for silencer in ["SetIsMuted", "put_IsMuted", "set_muted"] {
        assert!(
            !arm.contains(silencer),
            "the speaker does not silence anything: it names {silencer}"
        );
    }
}
