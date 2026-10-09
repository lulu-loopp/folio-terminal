//! **The crate root: floats and menus.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use std::time::Duration;

/// PIN — an empty family contributes no layer, so the order above is about
/// what is *on screen* and not about eight always-present slots.
///
/// The stack is built every frame with most of it empty; if flattening
/// emitted placeholders, the renderer would be handed — and diff — a list of
/// blank layers that changed every time a family came and went.
#[test]
fn a_family_with_nothing_in_it_adds_nothing_to_the_overlay() {
    assert!(OverlayStack::default().flattened().is_empty());
    let only_float = OverlayStack {
        float: vec![marks::OverlayLayer::default()].into(),
        ..OverlayStack::default()
    };
    assert_eq!(only_float.flattened().len(), 1);
}

/// Pin (a) of the peek raster defect: every peek pixel that reaches the renderer is one the
/// flyout draws. The chain the app runs — the renderer's box, the worker's resample, the
/// thumbnail slot, the overlay — is walked end to end here, so a future edit that hands the
/// renderer a native decode again fails on the resident byte count and on the texture key.
#[test]
fn the_peek_overlay_carries_display_sized_pixels_under_a_display_sized_key() {
    // A decode far larger than any flyout: 1024x768 in a 640x480 pane.
    let (native_width_px, native_height_px) = (1024_u32, 768_u32);
    let native_rgba: Arc<[u8]> =
        Arc::from(vec![
            0x40_u8;
            native_width_px as usize * native_height_px as usize * 4
        ]);
    let content_key = "image:0123456789abcdef0123456789abcdef".to_owned();

    let (display_width_px, display_height_px) =
        bt_render::peek_thumbnail_extent(640.0, 480.0, 8.0, 1.0, native_width_px, native_height_px)
            .expect("the pane can host the flyout");
    assert!(
        display_width_px < native_width_px && display_height_px < native_height_px,
        "the 40% cap is what makes the flyout smaller than its decode",
    );

    let target: PeekThumbnailTarget = (content_key.clone(), display_width_px, display_height_px);
    let task = peek_scale_task(
        &target,
        Arc::clone(&native_rgba),
        native_width_px,
        native_height_px,
    );
    let thumbnail = PeekThumbnail::from_scaled(bt_term::scale_inline_image(&task));
    let overlay = thumbnail.overlay(
        bt_render::SeatViewport {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        },
        PhysicalPosition::new(120.0, 90.0),
    );
    assert_eq!(
        (overlay.width_px, overlay.height_px),
        (display_width_px, display_height_px),
    );
    assert_eq!(
        overlay.rgba.len(),
        display_width_px as usize * display_height_px as usize * 4,
        "the resident bytes the renderer uploads are the display box, not the decode",
    );
    assert!(
        overlay.rgba.len() * 16 < native_rgba.len(),
        "the defect uploaded {} bytes where {} suffice",
        native_rgba.len(),
        overlay.rgba.len(),
    );
    assert_eq!(
        overlay.key,
        bt_term::display_texture_key(&content_key, display_width_px, display_height_px),
        "the display size is part of the texture identity, so the shared LRU can never \
             serve a raster sized for another box",
    );
    assert!(
        thumbnail.matches(&target),
        "the slot answers the question the hover asked, so a raster for another box is \
             never presented as this one",
    );
}

/// PIN — **§7.1.6e (2026-08-20): the turn is not merely hidden where it says
/// something false, it is never started.**
///
/// The angle the `˅` is drawn at is one half of the ruling and `seats` owns
/// it; this is the other half. A tween aimed at 180° under
/// [`profiles::MenuSide::Beside`] would run a clock for 140ms, ask for a
/// repaint on every frame of it, and arrive at a number no surface is going
/// to draw — a whole animation whose only observable effect is the work it
/// costs. So the criterion is applied to the *target*, and it is the same one
/// statement of it the arrow reads: the side decides, and nothing else does.
///
/// `None` — a vertical window whose rail is folded away — is not a third
/// rule. There is no button on screen, so there is no arrow to aim.
#[test]
fn the_chevron_s_turn_is_only_aimed_where_the_picker_hangs_below() {
    assert!(
        chevron_turn_target(true, Some(profiles::MenuSide::Below)),
        "the strip's arrow turns over, and the ruling kept that arm"
    );
    assert!(
        !chevron_turn_target(true, Some(profiles::MenuSide::Beside)),
        "a rail's picker opens to the side, so its arrow has nowhere true to \
             turn — and no clock to run getting there"
    );
    assert!(
        !chevron_turn_target(false, Some(profiles::MenuSide::Below)),
        "a shut menu aims the arrow back down wherever it hangs"
    );
    assert!(
        !chevron_turn_target(true, None),
        "and a window with no new-tab button on screen has no arrow to aim"
    );
}

/// **PIN — every popup says which surface it grew out of, and only the two
/// the tab list itself raises can name the sidebar.**
///
/// Two, since 丙2: the `˅`'s profile list hangs off whichever surface carries
/// the `+`, and a tab's context menu hangs off the row a right press landed
/// on — which is the same surface, by the same [`tab_surface`] answer. The
/// other seven are raised by a press somewhere in the panes, and if any of
/// *them* ever answered [`PopupOwner::Tabs`] the rail would be held open by a
/// menu standing in the middle of the stage — the 2026-08-15 flyout report
/// with a different panel in it.
///
/// Both directions matter and the loop below is what keeps them apart: drop
/// `Popup::Tab` back onto the `Stage` line and the sidebar retracts out from
/// under a menu it is still drawing (the 2026-08-25 report); move any of the
/// seven onto the `Tabs` line and the rail is held open by a pane's `⌄`.
///
/// Mutation: either move, and the loop goes red at that popup on all three
/// surfaces.
#[test]
fn only_the_two_menus_the_tab_list_raises_belong_to_a_tab_surface() {
    const OF_THE_TAB_LIST: [Popup; 2] = [Popup::Profile, Popup::Tab];
    for surface in [TabSurface::Strip, TabSurface::Rail, TabSurface::FocusColumn] {
        for popup in OF_THE_TAB_LIST {
            assert_eq!(
                popup_owner(popup, surface),
                PopupOwner::Tabs(surface),
                "{popup:?} is raised on whichever surface carries the tabs"
            );
        }
        for popup in Popup::ALL
            .into_iter()
            .filter(|popup| !OF_THE_TAB_LIST.contains(popup))
        {
            assert_eq!(
                popup_owner(popup, surface),
                PopupOwner::Stage,
                "{popup:?} is raised inside the stage and belongs to no tab \
                     surface"
            );
        }
    }
}

/// PIN — the profile picker's arrow turns over across 140ms on
/// `cubic-bezier(.2,0,0,1)`, and it is the turn that is drawn rather than
/// its two ends.
///
/// `.chevbtn svg { transition: transform 140ms cubic-bezier(.2,0,0,1) }`
/// (mock-up 415-418). Both halves of that declaration are load-bearing and
/// both are pinned here against the ways they get quietly deleted: cut the
/// duration to nothing and the arrow arrives before the first sample, so
/// there is no midpoint left to read; swap the curve for `linear` and the
/// midpoint lands at half a turn instead of where this curve actually puts
/// it, which — because `.2,0,0,1` front-loads almost everything and then
/// crawls — is nearly nine tenths of the way over.
#[test]
fn the_profile_chevron_turns_over_across_a_hundred_and_forty_milliseconds() {
    assert_eq!(
        CHEVRON_TURN,
        Duration::from_millis(140),
        "`transition: transform 140ms` (mock-up 417)"
    );
    assert_eq!(
        GRAB_EASE,
        [0.2, 0.0, 0.0, 1.0],
        "`cubic-bezier(.2,0,0,1)` (mock-up 417)"
    );

    let now = Instant::now();
    let mut turn = ChevronTurn::default();
    assert_eq!(
        turn.sample(now, Motion::Full),
        (0.0, false),
        "an untouched picker's arrow points down and is not moving"
    );

    turn.retarget(true, now, Motion::Full);
    assert_eq!(turn.sample(now, Motion::Full), (0.0, true));

    // The middle of the transition is a real place, and it is where this
    // curve puts it rather than where a straight line would.
    let (halfway, moving) = turn.sample(now + Duration::from_millis(70), Motion::Full);
    assert!(moving);
    assert!(
        halfway > 0.0 && halfway < 1.0,
        "70ms into a 140ms turn the arrow is partway over, saw {halfway}"
    );
    let eased = cubic_bezier(0.5, GRAB_EASE);
    assert!(
        (halfway - eased).abs() < 1e-3,
        "the turn is drawn on its own curve: expected {eased}, saw {halfway}"
    );
    assert!(
        (halfway - 0.5).abs() > 0.2,
        "halfway in time is not halfway over on this curve — saw {halfway}, \
             which is what `linear` would have given"
    );

    // It only ever goes forwards, and it stops.
    let mut last = 0.0;
    for step in 0..=14 {
        let (at, _) = turn.sample(now + Duration::from_millis(step * 10), Motion::Full);
        assert!(at >= last, "the arrow does not turn back on its way over");
        last = at;
    }
    assert!(
        turn.sample(now + Duration::from_millis(139), Motion::Full)
            .1
    );
    assert_eq!(
        turn.sample(now + Duration::from_millis(140), Motion::Full),
        (1.0, false),
        "at 140ms the arrow has arrived and owes no more frames"
    );
    assert_eq!(
        turn.sample(now + Duration::from_secs(9), Motion::Full),
        (1.0, false)
    );
}

/// PIN — a turn reversed mid-flight carries on from the angle the arrow is
/// actually at.
///
/// This is what a CSS transition does to a property whose target changes
/// while it is running, and it is the whole difference between a control
/// that turns and one that flickers: clicking the picker twice quickly must
/// not snap the arrow to an end it never reached and then run back from
/// there. Red gate: the sample taken at the instant of the reversal is the
/// same number on both sides of it.
#[test]
fn the_chevron_reverses_from_where_the_arrow_actually_is() {
    let now = Instant::now();
    let mut turn = ChevronTurn::default();
    turn.retarget(true, now, Motion::Full);

    let reversed_at = now + Duration::from_millis(40);
    let (mid, _) = turn.sample(reversed_at, Motion::Full);
    assert!(mid > 0.0 && mid < 1.0, "caught mid-turn, saw {mid}");

    turn.retarget(false, reversed_at, Motion::Full);
    let (restarted, moving) = turn.sample(reversed_at, Motion::Full);
    assert!(moving);
    assert!(
        (restarted - mid).abs() < 1e-6,
        "the arrow jumped from {mid} to {restarted} when it was told to come back"
    );

    // And from there it goes the other way, all the way home.
    let mut last = restarted;
    for step in 1..=14 {
        let (at, _) = turn.sample(reversed_at + Duration::from_millis(step * 10), Motion::Full);
        assert!(
            at <= last,
            "the reversed turn went further over instead of coming back"
        );
        last = at;
    }
    assert_eq!(
        turn.sample(reversed_at + CHEVRON_TURN, Motion::Full),
        (0.0, false)
    );

    // Told again what it is already doing, it does not restart: a caller
    // that re-reports the same state must not stretch the transition.
    let mut steady = ChevronTurn::default();
    steady.retarget(true, now, Motion::Full);
    let at = now + Duration::from_millis(100);
    let (before, _) = steady.sample(at, Motion::Full);
    steady.retarget(true, at, Motion::Full);
    assert_eq!(steady.sample(at, Motion::Full).0, before);
    assert_eq!(
        steady.sample(now + CHEVRON_TURN, Motion::Full),
        (1.0, false)
    );
}

/// PIN — `@media (prefers-reduced-motion: reduce) { .chevbtn svg {
/// transition: none } }` (mock-up 420).
///
/// `none` is not a faster transition: there are no intermediate frames at
/// all, the arrow is simply already over, and — the half that actually
/// costs something — nothing asks to be woken up to draw the frames that do
/// not exist.
#[test]
fn reduced_motion_turns_the_chevron_over_with_no_frames_in_between() {
    let now = Instant::now();
    let mut turn = ChevronTurn::default();
    turn.retarget(true, now, Motion::Reduced);
    assert_eq!(
        turn.sample(now, Motion::Reduced),
        (1.0, false),
        "under reduced motion the arrow is over the instant the list is up"
    );
    for step in 0..=14 {
        assert_eq!(
            turn.sample(now + Duration::from_millis(step * 10), Motion::Reduced),
            (1.0, false),
            "and there is never a frame of it on the way"
        );
    }
    turn.retarget(false, now + Duration::from_millis(50), Motion::Reduced);
    assert_eq!(
        turn.sample(now + Duration::from_millis(50), Motion::Reduced),
        (0.0, false)
    );
}

/// Which of them the float is asking for — hover, and the gesture that
/// outlives it.
#[test]
fn the_grip_keeps_its_arrow_for_the_whole_pull() {
    use float::FloatPart;
    assert_eq!(
        float_grasp(None, Some(FloatPart::Grip), true),
        Some(FloatGrasp::Grip),
        "hovering the grip is the whole bug: an arrow was showing where a resize lives"
    );
    assert_eq!(
        float_grasp(None, Some(FloatPart::Head), true),
        Some(FloatGrasp::Head)
    );
    for part in [
        FloatPart::Body,
        FloatPart::Foot,
        FloatPart::Dock,
        FloatPart::Close,
        FloatPart::Row(0),
    ] {
        assert_eq!(
            float_grasp(None, Some(part), true),
            None,
            "the rest of the window is an ordinary arrow"
        );
    }
    // The pull survives the pointer leaving the grip it began on — the
    // corner stops at the 200×150 floor while the hand keeps going.
    assert_eq!(
        float_grasp(Some(FloatDragKind::Resize), Some(FloatPart::Body), true),
        Some(FloatGrasp::Grip)
    );
    assert_eq!(
        float_grasp(Some(FloatDragKind::Move { grab: [0.0, 0.0] }), None, true),
        Some(FloatGrasp::Carrying)
    );
    // **A peek's header is a handle too** (user ruling 2026-08-27, §7.29).
    // The sentence that stood here — "a peek is not a window you were told
    // you had: its header is not a handle" — was overturned on the day the
    // gesture it denied became the product's way of keeping a glance: since
    // 2026-08-12 six pixels on that header promote the peek, and a header
    // that does that while advertising nothing is §7.21's complaint exactly.
    assert_eq!(
        float_grasp(None, Some(FloatPart::Head), false),
        Some(FloatGrasp::Head),
        "the one header that turns a glimpse into a window must look like one"
    );
    // The grip is still the pinned window's alone, and a peek is drawn
    // without one — so this is belt and braces rather than a rule with a
    // surface behind it.
    assert_eq!(float_grasp(None, Some(FloatPart::Grip), false), None);
    // **But the carry a peek's header turns into wears the closed fist**
    // (rule ③, 2026-08-12). The gesture is asked before the hover and before
    // `pinned`, which is what makes this true without a special case: by the
    // time the drag exists the window has been promoted anyway, and even a
    // frame where the two disagreed would be answered by the drag.
    assert_eq!(
        float_grasp(
            Some(FloatDragKind::Move { grab: [60.0, 12.0] }),
            Some(FloatPart::Head),
            false
        ),
        Some(FloatGrasp::Carrying),
        "a peek being carried off is a hand that has closed on something"
    );
}

/// User ruling 2026-08-12, which overturns `M2-tiny-window-priority.md`
/// §3.3: a float may stand on the rail and on the pane heads — the overlay
/// order already draws it over them — and may never stand on the caption,
/// whose buttons and drag band have to be reachable at every moment.
#[test]
fn a_float_may_stand_anywhere_in_the_window_except_the_title_bar() {
    let caption = |scale: f32| (bt_render::WINDOW_TITLE_BAR_LOGICAL_PX * scale).round();
    let rect = float_viewport_rect(2200, 1400, 2.0);
    assert_eq!(
        rect,
        [0.0, caption(2.0), 2200.0, 1400.0],
        "the rail's own column, the pane heads and every edge of the client \
             area are ground a float may cover; only the caption is removed"
    );
    assert_eq!(
        float_viewport_rect(800, 600, 1.0),
        [0.0, 40.0, 800.0, 600.0],
        "and the strip is the title bar's own 40 logical px, not a number of \
             this function's own"
    );
    // The floor under the tiny-window ladder: a client area shorter than its
    // own caption still answers with a rectangle that is the right way up,
    // because every clamp downstream subtracts from these two edges.
    let tiny = float_viewport_rect(120, 20, 2.0);
    assert!(
        tiny[1] < tiny[3],
        "a viewport must never come back inverted: {tiny:?}"
    );
}

/// RED (review row D7) — **a menu's rows are the rows its host can carry
/// out.**
///
/// A floating tree got the docked column's whole face — `Rename`, `Delete`
/// and, on a folder, `New file…` and `New folder…` — while
/// `open_files_row_rename`, `open_files_row_new` and `delete_files_row` each
/// answer only `RowHost::Column`. Four rows of a six-row menu did nothing at
/// all, with no field, no card and no explanation. The refusals are right; it
/// was the menu that was not told.
///
/// Red gate: hand `profiles::file_menu` the subject alone and every
/// assertion in the first loop fails by name.
#[test]
fn a_menus_rows_are_the_rows_its_host_can_perform() {
    use profiles::FileMenuRow as Row;
    let on = |host| {
        file_menu_powers(Some(&FileMenuTreeRow {
            host,
            key: "/notes.md".to_owned(),
        }))
    };
    let column = on(RowHost::Column(SeatId(1)));
    let float = on(RowHost::Float(7));
    assert!(column.writes_rows, "a docked column owns the rows it draws");
    assert!(
        !float.writes_rows,
        "a float has no box to measure an editor into and no column to \
             report a refusal on"
    );
    let writes = |row: &Row| {
        matches!(
            row,
            Row::Rename | Row::Delete | Row::NewFile | Row::NewFolder
        )
    };
    for subject in [
        profiles::FileMenuSubject::File,
        profiles::FileMenuSubject::Folder { expanded: false },
        profiles::FileMenuSubject::Folder { expanded: true },
        profiles::FileMenuSubject::Root,
    ] {
        let docked = profiles::file_menu(subject, column).rows;
        let floating = profiles::file_menu(subject, float).rows;
        assert!(
            docked.iter().any(writes),
            "{subject:?} on a column offers verbs that write"
        );
        assert!(
            !floating.iter().any(writes),
            "{subject:?} on a float offers none of them"
        );
        // And nothing *else* is taken away: the two faces differ by exactly
        // the rows the float cannot perform, so a floating tree still opens,
        // folds, starts a shell and hands out its path.
        assert_eq!(
            floating,
            docked
                .iter()
                .copied()
                .filter(|row| !writes(row))
                .collect::<Vec<_>>(),
            "{subject:?}"
        );
    }
    // The gap `Delete` stands in goes with it rather than being left behind
    // as a rule with nothing above it.
    assert_eq!(
        profiles::file_menu(profiles::FileMenuSubject::File, float).lone_separator_after,
        None,
        "no lone row, no lone rule"
    );
    // A face with no tree row behind it has no host to ask about, and its
    // list is the same either way — neither carries a row that writes.
    for subject in [
        profiles::FileMenuSubject::Document,
        profiles::FileMenuSubject::FoldedPath { levels: 3 },
    ] {
        assert_eq!(
            profiles::file_menu(subject, column).rows,
            profiles::file_menu(subject, float).rows,
            "{subject:?}"
        );
    }
    assert!(
        !file_menu_powers(None).writes_rows,
        "and a menu with no row behind it is handed the powerless set"
    );
}

// ── the `⌄` ruling (2026-08-16) ─────────────────────────────────────────

/// PIN (**the ruling's own point**) — the tab strip's `⌄` and the pane
/// head's `⌄` are driven by **one call, one policy and one pair of
/// constants**.
///
/// The ruling is "两处 ⌄ 语义完全对齐", and the failure it guards against is
/// not a wrong delay: it is two implementations that agree in the build that
/// wrote them and drift in the one after. [`ChevronGates::observe`] is the
/// only function in this program that starts either clock — it takes both
/// buttons' states and cannot be called for one of them — so "the two agree"
/// is a fact about the type rather than a promise about two call sites.
///
/// Red gate: give either gate its own `observe` at its own call site and
/// this test still passes, but the *shape* it is asserting is gone — so the
/// assertion is written against the pair, driving both through one script
/// and demanding identical deadlines at every step.
#[test]
fn both_chevrons_are_driven_by_one_policy_and_one_pair_of_constants() {
    use profiles::{ChevronAction, ChevronPointer};
    let start = Instant::now();
    let mut gates = ChevronGates::default();

    // A rest on the strip's chevron while the pointer is nowhere near the
    // pane head's: one clock runs and the other does not.
    gates.observe(
        (ChevronPointer::Button, false),
        (ChevronPointer::Away, false),
        (ChevronPointer::Away, false),
        start,
    );
    assert_eq!(
        gates.deadline(),
        Some(start + profiles::CHEVRON_HOVER_OPEN_DELAY)
    );
    assert_eq!(gates.pane.deadline(), None, "an idle chevron owes nothing");
    assert_eq!(
        gates
            .profile
            .due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(ChevronAction::Open)
    );

    // The mirrored situation gives the mirrored answer at the same instant,
    // which is the whole claim.
    let mut mirrored = ChevronGates::default();
    mirrored.observe(
        (ChevronPointer::Away, false),
        (ChevronPointer::Button, false),
        (ChevronPointer::Away, false),
        start,
    );
    assert_eq!(mirrored.profile, gates.pane);
    assert_eq!(mirrored.pane, gates.profile);
    assert_eq!(mirrored.deadline(), gates.deadline());

    // Both graces run on the same 150, and the earliest deadline is the one
    // the loop is told about.
    let mut leaving = ChevronGates::default();
    leaving.observe(
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, false),
        start,
    );
    assert_eq!(
        leaving.deadline(),
        Some(start + profiles::CHEVRON_LEAVE_GRACE)
    );
    assert_eq!(
        leaving.profile.due(start + profiles::CHEVRON_LEAVE_GRACE),
        Some(ChevronAction::Close)
    );
    assert_eq!(
        leaving.pane.due(start + profiles::CHEVRON_LEAVE_GRACE),
        Some(ChevronAction::Close)
    );

    // And a press — every door that is not a pointer move — stops both.
    leaving.clear();
    assert_eq!(leaving.deadline(), None);

    // **The pin is one bit on the one type, and the three gates answer it
    // alike** (owner ruling 2026-09-23). Every menu pinned, the hand gone from
    // all three: no gate owes anything and the loop is told of no wake-up (A3).
    // Then each menu goes, and each gate is back to the gate it was before any
    // of this — no fourth state, and no gate remembering how its menu opened.
    let mut pinned = ChevronGates::default();
    for popup in [Popup::Profile, Popup::Pane, Popup::File] {
        pinned.gate(popup).expect("a `⌄` governs this menu").pin();
    }
    pinned.observe(
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        start,
    );
    assert_eq!(pinned.profile, pinned.pane, "one policy for the pin too");
    assert_eq!(pinned.pane, pinned.rail, "and the rail's pill is in it");
    assert_eq!(
        pinned.deadline(),
        None,
        "a pinned menu registers no wake deadline"
    );
    for popup in [Popup::Profile, Popup::Pane, Popup::File] {
        pinned.menu_gone(popup);
    }
    assert_eq!(pinned, ChevronGates::default());
    for popup in [
        Popup::Root,
        Popup::GraphFilter,
        Popup::Preview,
        Popup::GitMenu,
        Popup::TermMenu,
        Popup::Tab,
        Popup::Palette,
    ] {
        assert!(
            pinned.gate(popup).is_none(),
            "{popup:?} is raised by no `⌄` and has no pin"
        );
    }
}

/// PIN (**menu-openers hover, actions click**) — user ruling, 2026-09-10.
///
/// The preview rail's `Open` pill expands
/// [`profiles::FileMenuSubject::Document`] and does nothing else, so by the
/// owner's principle of this day — 「展开菜单的控件 hover 就开,执行动作的控
/// 件必须点」 — it is a menu-opener and it rests open. It had been click-only
/// since it was drawn, for no reason but that it is spelled with a word
/// instead of with a `⌄`: the 2026-08-16 ruling drew its boundary around the
/// *glyph*, and this one redraws it around the *behaviour*.
///
/// So the pill is enrolled in the very clock the two `⌄` already run on
/// rather than given a second one, and that is what is asserted: the rail
/// gate is driven through the same [`ChevronGates::observe`] and answers the
/// same verbs at the same instants as the strip's. A second clock at 250ms
/// would pass a test that only checked the pill; it cannot pass one written
/// as an equality against the chevron beside it.
///
/// Red gate: give the pill its own `Duration` or its own `observe` call and
/// the equalities fail; delete the rail arm and the `Open` answers vanish.
#[test]
fn the_rails_open_pill_rests_open_on_the_chevrons_own_clock() {
    use profiles::{ChevronAction, ChevronPointer};
    let start = Instant::now();

    // A rest on the pill and a rest on the strip's `⌄`, told to the gates in
    // one call: the same deadline, the same verb, the same instant.
    let mut resting = ChevronGates::default();
    resting.observe(
        (ChevronPointer::Button, false),
        (ChevronPointer::Away, false),
        (ChevronPointer::Button, false),
        start,
    );
    assert_eq!(
        resting.rail, resting.profile,
        "one policy: the pill's clock and the chevron's are the same state"
    );
    assert_eq!(
        resting.rail.deadline(),
        Some(start + profiles::CHEVRON_HOVER_OPEN_DELAY)
    );
    assert_eq!(
        resting.rail.due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(ChevronAction::Open),
        "resting on `Open` for the ruling's quarter second raises the \
             document menu"
    );

    // A hand that left before the rest matured has raised nothing and owes
    // nothing — leaving a shut control clears the clock outright rather than
    // pausing it, so coming back starts the quarter second again from zero.
    let mut left_early = resting;
    left_early.observe(
        (ChevronPointer::Away, false),
        (ChevronPointer::Away, false),
        (ChevronPointer::Away, false),
        start + profiles::CHEVRON_HOVER_OPEN_DELAY - Duration::from_millis(1),
    );
    assert_eq!(left_early.rail.deadline(), None);
    assert_eq!(
        left_early
            .rail
            .due(start + profiles::CHEVRON_HOVER_OPEN_DELAY * 4),
        None,
        "no menu is ever raised by a rest the hand did not finish"
    );

    // The pointer moving into the menu the pill opened keeps it up: on the
    // surface, no clock runs in either direction.
    let mut on_menu = ChevronGates::default();
    on_menu.observe(
        (ChevronPointer::Away, false),
        (ChevronPointer::Away, false),
        (ChevronPointer::Surface, true),
        start,
    );
    assert_eq!(
        on_menu.rail.deadline(),
        None,
        "a hand on the menu is a hand still dealing with the pill"
    );

    // And leaving both of them runs the chevrons' own 150ms grace.
    let mut leaving = ChevronGates::default();
    leaving.observe(
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        start,
    );
    assert_eq!(leaving.rail, leaving.profile);
    assert_eq!(
        leaving.rail.due(start + profiles::CHEVRON_LEAVE_GRACE),
        Some(ChevronAction::Close)
    );
}

/// PIN (**E61 — one popup at a time**, now including both `⌄` menus).
///
/// The list is the rule: whatever is being raised, every other popup goes.
/// Six openers used to carry six hand-copied runs of `self.x = None` and no
/// two of them agreed — the pane menu left the preview switcher up, the root
/// menu left the file menu up — so what is pinned here is not a set of pairs
/// but the *completeness*: for each popup, `others()` is exactly the rest of
/// the list, and adding one without listing it breaks this test rather than
/// shipping a pair that can be up together.
///
/// Red gate: drop one arm of `ALL` and the count assertion goes red; return
/// a hand-written subset from `others` and the "every other popup" assertion
/// names the one that got away.
#[test]
fn opening_any_popup_closes_every_other_one() {
    assert_eq!(
        Popup::ALL.len(),
        10,
        "nine popups and the command palette, and this list is the rule"
    );
    for keep in Popup::ALL {
        let closed: Vec<Popup> = keep.others().collect();
        assert_eq!(
            closed.len(),
            Popup::ALL.len() - 1,
            "{keep:?} closes every popup but itself"
        );
        assert!(
            !closed.contains(&keep),
            "{keep:?} must not close the popup it is raising — that is what                  makes a toggle possible through the same door as an open"
        );
        for other in Popup::ALL {
            assert_eq!(
                other != keep,
                closed.contains(&other),
                "{keep:?} against {other:?}"
            );
        }
    }
    // The two chevron menus are on the list, which is the ruling's own
    // requirement: a hover-opening surface that could coexist with another
    // popup would be a menu a pointer drops on top of an open one.
    assert!(Popup::ALL.contains(&Popup::Profile));
    assert!(Popup::ALL.contains(&Popup::Pane));
    // And so is the git context menu (v2 (4)) — it is raised by a right
    // press, which is a gesture no other popup answers, so it is exactly the
    // one that could otherwise have come up on top of an open menu.
    assert!(Popup::ALL.contains(&Popup::GitMenu));
    // And the terminal's own menu (ticket #62), which is the one raised
    // *inside a pane* — the surface every other popup on this list is drawn
    // over, and therefore the one that could otherwise have come up
    // underneath an open menu rather than on top of it.
    assert!(Popup::ALL.contains(&Popup::TermMenu));
    // And a tab's own menu (丙2), which is the one raised *on the tab list* —
    // the surface the profile picker's own list hangs beside — and therefore
    // the one that could otherwise have come up next to an open picker
    // rather than instead of it.
    assert!(Popup::ALL.contains(&Popup::Tab));
}
