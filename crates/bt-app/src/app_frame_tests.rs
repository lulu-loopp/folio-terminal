//! **The crate root: frame and present.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    TITLE_FRAME, focused_frame, leaf_saying, method_body, rail_test_body, resolve_focused_pane,
    tab_holding,
};
use bt_render::LIGHT_CHROME;
use std::time::Duration;

/// PIN (T2): every `OSC 9;4` state maps to the arc the mock-up gives it.
///
/// The two states that may arrive *without* a percentage are the reason
/// this takes a `last_sweep`: states 2 and 4 change a run that is already
/// under way, so the reading already on the wire still stands, and keeping
/// it is the protocol's own answer rather than an invented number.
#[test]
fn each_progress_state_paints_its_own_arc() {
    let palette = LIGHT_CHROME;
    let arc = |state, last| ring_arc(state, last, Duration::ZERO, Motion::Full, &palette);

    let normal = arc(ProgressState::Normal(40), None);
    assert_eq!(normal.color, palette.accent);
    assert_eq!(normal.sweep_milliturns, 400);
    assert!(
        !normal.animating,
        "a determinate arc does not move by itself"
    );

    // Percent is a fraction of the whole turn, at both ends of its range.
    assert_eq!(arc(ProgressState::Normal(0), None).sweep_milliturns, 0);
    assert_eq!(arc(ProgressState::Normal(100), None).sweep_milliturns, 1000);
    // And a report beyond 100 is clamped rather than wrapped — an arc that
    // wrapped would report 130% as 30%.
    assert_eq!(arc(ProgressState::Normal(255), None).sweep_milliturns, 1000);

    // Only the arc's colour changes; the ring is not redrawn as something
    // else (mock-up lines 280-281 recolour `.arc` and nothing more).
    let failed = arc(ProgressState::Error(Some(40)), None);
    assert_eq!(failed.color, palette.status_err);
    assert_eq!(failed.sweep_milliturns, 400);
    let paused = arc(ProgressState::Paused(Some(40)), None);
    assert_eq!(paused.color, palette.status_pause);
    assert_eq!(paused.sweep_milliturns, 400);

    // A state change with no percentage keeps the reading already showing.
    assert_eq!(
        arc(ProgressState::Error(None), Some(400)).sweep_milliturns,
        400
    );
    assert_eq!(
        arc(ProgressState::Paused(None), Some(730)).sweep_milliturns,
        730
    );
    // With no reading ever taken, a full ring — so a failure is visible
    // rather than reported as a bare track.
    assert_eq!(arc(ProgressState::Error(None), None).sweep_milliturns, 1000);

    let spinning = arc(ProgressState::Indeterminate, None);
    assert_eq!(spinning.color, palette.accent);
    assert_eq!(spinning.sweep_milliturns, 243, "13 of the mock-up's 53.4");
    assert!(
        spinning.animating,
        "an indeterminate arc owes the next frame"
    );
    // Stopped, it is the same arc and no longer owes a frame.
    let still = ring_arc(
        ProgressState::Indeterminate,
        None,
        Duration::ZERO,
        Motion::Reduced,
        &palette,
    );
    assert_eq!(still.sweep_milliturns, spinning.sweep_milliturns);
    assert!(!still.animating);
}

/// PIN — §7.1.6c-4f amendment: the acrylic plate follows the scheme in
/// force, and this is the whole of the decision that makes it.
///
/// Red gate: the window used to declare nothing at all, so DWM tinted its
/// plate light whatever the scheme was — measured on a light desktop with
/// Solarized Dark at 30 %, a pane body read `(156,177,183)` instead of the
/// `(99,120,126)` the flag buys. A version of this that keyed on the theme
/// *row* rather than on the painted background would pass the first two
/// cases and fail the last two.
#[test]
fn the_dwm_plate_is_told_which_canvas_is_actually_painted() {
    const SOLARIZED_DARK: [u8; 3] = [0x00, 0x2B, 0x36];
    const FOLIO_LIGHT: [u8; 3] = [0xFA, 0xFA, 0xFA];

    // A window that has never spoken says it either way: DWM's default is
    // DWM's assumption, not this window's statement.
    assert_eq!(dwm_dark_mode_owed(None, SOLARIZED_DARK), Some(true));
    assert_eq!(dwm_dark_mode_owed(None, FOLIO_LIGHT), Some(false));
    // Said once, not said again — the whole point of remembering it.
    assert_eq!(dwm_dark_mode_owed(Some(true), SOLARIZED_DARK), None);
    assert_eq!(dwm_dark_mode_owed(Some(false), FOLIO_LIGHT), None);
    // A canvas that moved is a statement that has to move with it, in both
    // directions — this is the theme switch and the scheme switch alike.
    assert_eq!(dwm_dark_mode_owed(Some(false), SOLARIZED_DARK), Some(true));
    assert_eq!(dwm_dark_mode_owed(Some(true), FOLIO_LIGHT), Some(false));
    // The luma decides, not the row's name: a "dark scheme" file naming a
    // pale background gets the light plate that background asks for, and a
    // "light scheme" naming a near-black one gets the dark plate. This is
    // `scheme_in_force`'s rule, at `background_is_light`'s one threshold.
    assert_eq!(dwm_dark_mode_owed(None, [0xEE, 0xEE, 0xE8]), Some(false));
    assert_eq!(dwm_dark_mode_owed(None, [0x10, 0x10, 0x12]), Some(true));
    // And the threshold itself is borrowed, never restated here.
    for background in [SOLARIZED_DARK, FOLIO_LIGHT, [0x7F, 0x7F, 0x7F]] {
        assert_eq!(
            dwm_dark_mode_owed(None, background),
            Some(!bt_render::background_is_light(background)),
            "the plate and the canvas must take one decision, not two"
        );
    }
}

/// RED — **a tab whose only pane is a preview draws like any other**
/// (§7.10 ④‴, user report on `next21`).
///
/// The three funnels that answer a retained-state change —
/// `present_chrome_change`, `present_peek_overlay`,
/// `represent_on_screen_frame` — re-queue the picture already on the glass
/// and ask for a redraw; on a tab with no shell there is no such picture to
/// re-queue, ever, so all they leave behind is the bare request. Answering
/// that request with "nothing composed, nothing owed" is a preview pane
/// whose wheel moves the document in memory and never puts it on the glass.
///
/// RED GATE: drop the `|| !tab_has_a_shell` and the second assertion goes
/// red — which is exactly the shipped build, measured at ten notches and
/// zero frames.
#[test]
fn a_redraw_asked_for_by_a_tab_with_no_shell_is_always_a_present() {
    assert!(
        !a_bare_redraw_still_owes_a_present(false, true),
        "a tab with a shell and no filed debt owes nothing: everything that \
             changes such a window publishes a frame or files the debt first"
    );
    assert!(
        a_bare_redraw_still_owes_a_present(false, false),
        "a tab with no shell has only one kind of frame, so every redraw it \
             is asked for is that kind — without this its wheel scrolls a \
             document nobody draws"
    );
    assert!(
        a_bare_redraw_still_owes_a_present(true, true),
        "and the debt a chrome animation files is still the debt it always was"
    );
    assert!(a_bare_redraw_still_owes_a_present(true, false));

    // And the half a value cannot hold: that `redraw` really asks this
    // question rather than the narrower one it asked before. A funnel that
    // files no debt and a slot that is empty is the whole of the defect, and
    // it is invisible from any value this function could be handed. The
    // needle is split so that this assertion cannot find itself.
    let redraw = method_body("Runtime", "redraw");
    assert!(
        redraw.contains("a_bare_redraw_still_owes_a_present("),
        "`redraw` drops a bare request again, so a preview alone in a tab \
             stops drawing:\n{redraw}"
    );
}

/// RED (49) — **A title that changes every turn reaches the OS at most once per
/// frame interval, and the last one always arrives.**
///
/// A program that animates its title changes it faster than any taskbar can
/// show. Ten turns a quarter of a frame apart, each with a new title: the OS
/// hears at most one per frame, and once the held title's deadline comes — the
/// wake the turn folds in — it holds the tenth.
///
/// MUTATION: drop the pending value when the interval has not passed (no
/// trailing write) in `pace::LatestThrottle::offer` — the OS is left holding
/// the ninth.
#[test]
fn a_title_that_changes_every_turn_reaches_the_os_at_most_once_a_frame_and_the_last_one_arrives() {
    let start = Instant::now();
    let quarter = TITLE_FRAME / 4;
    let mut slot = TitleSlot::default();
    let mut os_holds: Option<String> = None;
    let mut writes = 0_u32;
    let mut now = start;
    for turn in 0..10_u32 {
        now = start + quarter * turn;
        slot.want(format!("building {turn}/10"));
        if let Some(title) = slot.take_due(TITLE_FRAME, now) {
            writes += 1;
            os_holds = Some(title);
        }
    }
    // The turns after the last change want nothing new; the one the deadline
    // books writes what was held.
    let due = slot.deadline().expect("the tenth title is held and booked");
    assert!(
        due > now && due <= now + TITLE_FRAME,
        "held for at most one frame"
    );
    assert_eq!(
        slot.take_due(TITLE_FRAME, due - Duration::from_millis(1)),
        None
    );
    if let Some(title) = slot.take_due(TITLE_FRAME, due) {
        writes += 1;
        os_holds = Some(title);
    }
    assert!(
        writes <= 10_u32.div_ceil(4) + 1,
        "{writes} writes for ten titles"
    );
    assert_eq!(os_holds.as_deref(), Some("building 9/10"));
    assert_eq!(slot.deadline(), None);
}

#[test]
fn resize_atomic_present_gate_rejects_the_previous_grid_frame() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
    let mut projection = session.new_projection(session.layout_key());
    let old_frame = session.viewport_frame(&mut projection).unwrap();
    let new_grid = GridSize {
        columns: std::num::NonZeroU16::new(42).unwrap(),
        rows: std::num::NonZeroU16::new(12).unwrap(),
    };
    assert!(!frame_matches_grid(&old_frame, new_grid));

    session
        .resize(NonZeroU32::new(42).unwrap(), NonZeroU32::new(12).unwrap())
        .unwrap();
    session.refresh_projection(&mut projection);
    let new_frame = session.viewport_frame(&mut projection).unwrap();
    assert!(frame_matches_grid(&new_frame, new_grid));
}

#[test]
fn pty_mode_only_update_is_presentation_equivalent_but_text_is_not() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(3).unwrap());
    let mut projection = session.new_projection(session.layout_key());
    let before = session.viewport_frame(&mut projection).unwrap();

    session.feed(b"\x1b[?2004h").unwrap();
    session.refresh_projection(&mut projection);
    let mode_only = session.viewport_frame(&mut projection).unwrap();
    assert!(presentation_equivalent(&before, &mode_only));

    session.feed(b"visible").unwrap();
    session.refresh_projection(&mut projection);
    let text = session.viewport_frame(&mut projection).unwrap();
    assert!(!presentation_equivalent(&mode_only, &text));
}

/// **A frame that says the same thing from somewhere else is not the same
/// frame.**
///
/// The dedupe above exists because a frame drawing the same picture need not
/// be published again, and that holds right up until the picture stops being
/// the only thing the frame says. It is also the answer to every question
/// about where the pointer is: `live_point_at`, the word and line selections,
/// the links and the caret all read `horizontal` off the *published* frame.
/// Drop a frame whose cells happen to match and the reader is left holding an
/// origin they have moved off, and their next click lands on a column they
/// have left.
///
/// The two frames here differ in the axis and in nothing else, on purpose:
/// building a live fixture whose cells genuinely coincide across a move is
/// hard *in this build* — a physical row past its own last column is empty
/// rather than blank (plan §5.1 clause 4), so the live plane usually gives
/// the move away. "Usually" is not "always", and it is not a property this
/// dedupe should be resting on.
///
/// MUTATION: drop `previous.horizontal == next.horizontal` from
/// `presentation_equivalent` and this passes the moved frame off as
/// unchanged.
#[test]
fn a_frame_that_says_the_same_thing_from_a_new_origin_is_not_the_old_frame() {
    use bt_viewport::horizontal::HorizontalProjection;

    let session = DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(2).unwrap());
    let mut projection = session.new_projection(session.layout_key());
    let before = session.viewport_frame(&mut projection).unwrap();
    assert!(
        presentation_equivalent(&before, &before.clone()),
        "a frame is equivalent to itself, or this test proves nothing"
    );

    let mut moved = before.clone();
    moved.horizontal = HorizontalProjection::new(ContentColumn(40), 8, ContentColumn(4));
    assert_eq!(
        before.cells, moved.cells,
        "the two differ in the axis and nowhere else"
    );
    assert!(
        !presentation_equivalent(&before, &moved),
        "same cells, different origin — a different frame, not the same one"
    );
}

/// RED (user report 2026-09-12, during `next60` acceptance) — **a hand
/// resting on a floating window lights no row of the column underneath it.**
///
/// The press half of this is `b1cf054`'s, and it was written at one caller:
/// `file_row_under`. The hover does not pass that caller — `pointer_moved`
/// asks `update_chrome_hover`, which asks `chrome_target_at`, and the docked
/// ladder does not consider floats at all — so a preview window standing
/// over a files column was opaque to the eye and transparent to the hover.
/// The row behind it lit up, and after `peek_strip::PEEK_DELAY` raised its
/// glance card on top of the window that was hiding it.
///
/// Read as text for this family's stated reason: what it guards against is a
/// *second* door onto one question, and a second door that agrees today
/// cannot be driven into disagreeing by any state machine.
///
/// Red gate: put the ladder's body back under the name `chrome_target_at`,
/// or let its float arm answer with the chrome behind the window, and the
/// first two assertions fail by name.
#[test]
fn a_hover_inside_a_floats_body_lights_no_docked_row() {
    let door = method_body("Runtime", "chrome_target_at");
    assert!(
        door.contains("self.pointer_target_at(position)?"),
        "the chrome's door is the router's answer read through, and not a \
             walk of the docked ladder that never heard of a window"
    );
    assert!(
        door.contains("PointerTarget::Float(..) => None,"),
        "and a point a window has claimed is no chrome at all — never the \
             chrome that window is covering"
    );
    let hover = method_body("Runtime", "update_chrome_hover");
    assert!(
        hover.contains("Some(PointerTarget::Float(..)) | None => None,"),
        "so the hover this window paints is read through the same claim"
    );
}

/// RED — **a tick whose only news is a decoded picture still reaches the
/// glass** (the freeze of 2026-08-28; §7.44 ③).
///
/// The defect this pins was found by photographing the machine: press play,
/// take your hand off the mouse, and the recording runs for two seconds and
/// then stops dead, while the decoder behind it goes on for as long as you
/// leave it. Two seconds is `VIDEO_BAR_IDLE_REST + VIDEO_BAR_FADE` — the
/// control bar's dwell and fade — because while the bar is up its clock and
/// its scrubber change the *chrome*, and the picture was only ever reaching
/// the glass as a passenger on that.
///
/// Two halves, because the fault had two places to live and mending one
/// without the other mends nothing:
///
/// ① **The rule.** A picture's debt alone is enough, exactly as a chrome
/// change alone is and a pane in flight alone is — and a tick carrying none
/// of the three presents nothing, which is what keeps an idle window at
/// zero.
///
/// ② **The call site asks it.** Read out of the source, because the gate is
/// a `return` inside a method that cannot be called without a window: what
/// there is to assert is that `advance_strip_animation` puts its question
/// through [`tick_owes_a_present`] and hands it the picture's debt.
///
/// RED GATE ①: return `chrome_changed || panes_owe` from
/// `tick_owes_a_present` and the first block fails. RED GATE ②: write the
/// gate back as `!self.refresh_chrome() && !panes_owe` and the second block
/// fails — which is the state the binary that froze was built from.
#[test]
fn a_video_frame_alone_is_enough_to_present() {
    // ① the rule.
    assert!(
        tick_owes_a_present(false, false, true),
        "a decoded picture and nothing else is still a frame the glass is owed"
    );
    assert!(tick_owes_a_present(true, false, false), "the chrome moved");
    assert!(
        tick_owes_a_present(false, true, false),
        "a pane is in flight"
    );
    assert!(
        !tick_owes_a_present(false, false, false),
        "and a tick with no news at all presents nothing — an idle window \
             costs what it costs because of this half"
    );

    // ② the call site asks it.
    let tick = method_body("Runtime", "advance_strip_animation");
    assert!(
        tick.contains("tick_owes_a_present(self.refresh_chrome(), panes_owe, pictures_owe)"),
        "the chrome gate asks the whole question, with the picture's debt in it"
    );
    // **The debt is now written down between two passes** (closure review
    // O4, 2026-09-18): the pictures are serviced above every gate, so what
    // reaches this gate is what the service recorded — including a frame
    // that arrived on a turn the gate refused, which nothing else would ever
    // come back for.
    assert!(
        tick.contains("std::mem::take(&mut self.window.pictures_owe_a_frame)"),
        "and the picture's debt is a name that survives as far as that gate"
    );
    let service = method_body("Runtime", "service_pictures");
    assert!(
        service.contains("if frames_arrived || boxes_moved {")
            && service.contains("self.window.pictures_owe_a_frame = true;"),
        "and the service is what writes it down"
    );
}

/// PIN (47) — **a resize present still refuses the frame composed before the resize, and is
/// silent once paid.**
///
/// The gate's own purpose, which reading the grid at validation keeps: the pane is re-solved into
/// a new rectangle, and the frame composed at the old grid is refused while the present is owed.
/// Once a present has landed nothing is owed, and a frame of any grid is admitted.
///
/// MUTATION: make `TabState::admit_resize_present` return `Ok(())` without asking
/// `frame_matches_grid` — red at the first refusal.
#[test]
fn a_resize_present_refuses_the_frame_composed_before_the_resize() {
    let mut fonts = bt_render::preview_measure_font_system();
    let metrics = bt_render::CellMetrics::measure(&mut fonts, 1.0).unwrap();
    let body = rail_test_body(797, 1.0);
    let mut tab = tab_holding(leaf_saying(
        "a line before the resize
",
    ));
    let before = focused_frame(&mut tab);
    tab.admit_resize_present(&before)
        .expect("nothing is owed before a resize");
    resolve_focused_pane(&mut tab, &metrics, body);
    tab.owe_resize_present();
    assert!(!frame_matches_grid(&before, tab.focused().unwrap().grid));
    assert!(
        tab.admit_resize_present(&before).is_err(),
        "the frame composed at the old grid is refused while the resize present is owed"
    );
    let after = focused_frame(&mut tab);
    tab.admit_resize_present(&after)
        .expect("the frame of the new grid is admitted");
    // The first frame that reaches the glass pays the debt (`Runtime::redraw`'s commit arm).
    tab.resize_present_owed = false;
    tab.admit_resize_present(&before)
        .expect("a paid debt gates nothing");
}

/// RED (51) — **A turn continues a search walk in progress under the scan's own name, and books
/// its next turn at once while one is owed; a published frame reads no slice.**
///
/// The wiring the two tests above cannot reach, since `Runtime` is not built without a window:
/// the clock stands in `turn` after the drain (so the lines the shell froze meanwhile are carried,
/// not owed) under `Station::SearchScan`, the wake fold asks for the next turn through
/// `search_walk_deadline`, and `publish_frame_inner`'s refresh is the carrying road.
///
/// MUTATION: delete the `advance_search_scan` clock from `turn` — the walk never gets past the
/// keystroke's slice, and the first assertion goes red.
#[test]
fn a_turn_walks_a_search_in_progress_and_wakes_for_it() {
    let turning = method_body("Runtime", "turn");
    let clock = turning
        .find("self.advance_search_scan()")
        .expect("`turn` advances a search walk");
    let drain = turning.find("self.drain_pty()?;").expect("`turn` drains");
    assert!(
        clock > drain,
        "the walk reads after the drain has frozen what it will"
    );
    let station = turning[..clock]
        .rfind("hang_watch::Station::")
        .map(|at| &turning[at..clock]);
    assert!(
        station.is_some_and(|text| text.starts_with("hang_watch::Station::SearchScan")),
        "the walk's slice is charged to the scan's own name"
    );
    assert!(
        turning.contains("self.search_walk_deadline(now)"),
        "the wake fold books the next slice's turn"
    );
    assert!(
        method_body("Runtime", "publish_frame_inner")
            .contains("self.refresh_search(SearchRefresh::Output)"),
        "a published frame carries the answer and reads no slice"
    );
}

/// How many times the product names `path`, and where.
fn product_names(path: &str) -> (usize, String) {
    use bt_source::{Pattern, View, needle};
    let found = crate::test_support::found(needle!(Pattern::path(path)), View::Identifiers)
        .in_the_product(crate::test_support::source());
    (found.len(), found.report(crate::test_support::source()))
}

/// GUARD — **the focused frame's hold is decided before anything is scheduled, and every frame is
/// scheduled through `bt_compose::schedule`** (design T-COMPOSE-CRATE §6.2, planted violation
/// "hold before schedule", the call-site half; the half inside the crate is
/// `bt_compose::tests::project_files_no_decoration_work_and_schedule_does`).
///
/// A guard: it reads how `publish_frame_inner` is written, because `Runtime` needs a window and
/// its order cannot be driven from a test. The product names the session's own scheduler nowhere,
/// so no site can go round the crate; and inside `publish_frame_inner` the projection comes first,
/// the hold's return second and the schedule third.
///
/// MUTATIONS: move the `bt_compose::schedule` call above the hold check in `publish_frame_inner`
/// — the order goes red; call `session.schedule_visible_artifacts` directly there — the product
/// names it and the first assertion goes red.
#[test]
fn the_hold_is_decided_before_anything_is_scheduled() {
    let (named, report) = product_names("schedule_visible_artifacts");
    assert_eq!(
        named, 0,
        "a frame is scheduled through `bt_compose::schedule` only:\n{report}"
    );
    let publish = method_body("Runtime", "publish_frame_inner");
    let projected = publish
        .find("bt_compose::project(")
        .expect("the focused frame is projected through the crate");
    let held = publish
        .find("if projected.hold_requested && self.window.last_presented_frame.is_some()")
        .expect("the hold is the projection's request and a picture already on the glass");
    let returned = held
        + publish[held..]
            .find("return Ok(false);")
            .expect("a held frame goes no further");
    let scheduled = publish
        .find("bt_compose::schedule(")
        .expect("an unheld frame is scheduled through the crate");
    assert!(
        projected < held && returned < scheduled,
        "project, then the hold, then schedule:\n{publish}"
    );
    assert_eq!(
        publish.matches("bt_compose::schedule(").count(),
        1,
        "and scheduled once:\n{publish}"
    );
}

/// GUARD — **a frame is acknowledged once, at entry to the pending slot** (design
/// T-COMPOSE-CRATE §6.2, planted violation "acknowledge only at pending-slot entry"; the state
/// half is `bt_compose::tests::the_revision_moves_once_per_frame_entered_into_the_slot`).
///
/// A guard, for the reason the one above gives. The product acknowledges in one place: inside
/// `publish_frame_inner`, below the unchanged-frame return and above the slot's `publish`. The
/// retry arm of `redraw`, which files a frame back after a present that failed, acknowledges
/// nothing; neither does anything else.
///
/// MUTATIONS: call `bt_compose::acknowledge` in the skipped-unchanged branch — two sites, and the
/// one in the branch stands above the return; call `session.record_published_frame` anywhere in
/// the product — the first assertion goes red.
#[test]
fn a_frame_is_acknowledged_once_at_entry_to_the_pending_slot() {
    let (recorded, report) = product_names("record_published_frame");
    assert_eq!(
        recorded, 0,
        "a frame is acknowledged through `bt_compose::acknowledge` only:\n{report}"
    );
    let (acknowledged, report) = product_names("bt_compose::acknowledge");
    assert_eq!(
        acknowledged, 1,
        "one acknowledgment in the product:\n{report}"
    );
    let publish = method_body("Runtime", "publish_frame_inner");
    let unchanged = publish
        .find("pty_drain_says_nothing_new(")
        .expect("the unchanged-frame skip");
    let skipped = unchanged
        + publish[unchanged..]
            .find("return Ok(false);")
            .expect("an unchanged frame goes no further");
    let at = publish
        .find("bt_compose::acknowledge(")
        .expect("the acknowledgment is the publish door's");
    let entered = publish
        .find(".publish(composed.frame, trigger)")
        .expect("the frame enters the slot");
    assert!(
        skipped < at && at < entered,
        "below the unchanged return, above the slot's entry:\n{publish}"
    );
}
