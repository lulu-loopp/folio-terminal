//! **The crate root, unsorted.** Tests of items `main.rs` owns that the theme sort of
//! `docs/plans/bt-app-split-inventory-2026-09-15.md` §0.3 places under no theme;
//! written in the crate root's scope (`use super::*`), with their shared fixtures from
//! [`crate::test_support`].

use super::*;
use crate::test_support::{PtyPresentationHarness, assert_close, at, host_path};
use std::time::Duration;

/// PIN (user report, 2026-08-25) — **`BT_PROBE_INPUT=` opens no probe and
/// kills nothing.**
///
/// The report was `Folio stopped: read BT_PROBE_INPUT : The system cannot
/// find the path specified. (os error 3)` — a window that never appeared
/// because a variable had been cleared rather than removed. The ruling is
/// this program's own and already written down for `BT_PTY_DUMP`: an emptied
/// variable is off.
///
/// Red gate: read the value into a `PathBuf` without asking whether it names
/// anything, and the empty string becomes a file to open.
#[test]
fn an_emptied_probe_variable_is_off_and_not_a_nameless_file() {
    assert!(
        super::probe_input(None)
            .expect("an unset variable is not a failure")
            .is_none()
    );
    assert!(
        super::probe_input(Some(std::ffi::OsString::new()))
            .expect("an emptied variable is off, not a file this run cannot open")
            .is_none(),
    );
    let named = std::env::temp_dir().join(format!(
        "{}.vt",
        bt_testpath::unique_name("folio-probe-input")
    ));
    std::fs::write(&named, b"\x1b[2J").expect("write a fixture into the scratch directory");
    assert_eq!(
        super::probe_input(Some(named.clone().into_os_string()))
            .expect("a variable that names a readable file is read")
            .as_deref(),
        Some(b"\x1b[2J".as_slice()),
        "and a variable that does name a file still feeds it in"
    );
    let _ = std::fs::remove_file(&named);
}

/// PIN (T2 D41): the accessibility preference is read in the right
/// direction.
///
/// Win32 and CSS spell this setting with opposite polarity —
/// `SPI_GETCLIENTAREAANIMATION` is `TRUE` when animation is *wanted*, while
/// `prefers-reduced-motion: reduce` matches when it is *not* — and the
/// inversion is invisible on any machine left at the default. Getting it
/// backwards would force animation on exactly the users who asked for none
/// and strip it from everyone else, and no screenshot review would catch
/// it. So the mapping is a named function with a test rather than a `!` at
/// a call site.
#[test]
fn the_reduced_motion_preference_is_read_in_the_right_direction() {
    assert_eq!(
        Motion::from_client_area_animation(Some(true)),
        Motion::Full,
        "TRUE means the system wants animation"
    );
    assert_eq!(
        Motion::from_client_area_animation(Some(false)),
        Motion::Reduced,
        "FALSE is the accessibility setting turned on"
    );
    assert_eq!(
        Motion::from_client_area_animation(None),
        Motion::Full,
        "a failed read is not a request for less motion"
    );
    // The default a `Motion` takes when nothing has asked is the same one a
    // failed read gets, so the two cannot drift apart.
    assert_eq!(Motion::default(), Motion::Full);
}

/// PIN (T2 D41): with animations off the breath holds one value instead of
/// stopping at whatever opacity it happened to be passing through.
///
/// The mock-up spells the replacement out (line 1927): `.ticon.working {
/// opacity: .6 }`. "Working" still has to be legible when nothing may move,
/// so the answer is a held value, not a still frame and not full opacity.
#[test]
fn reduced_motion_holds_the_breath_at_one_value() {
    for fraction in [0.0_f32, 0.25, 0.5, 0.75, 1.0, 3.7] {
        let held = breathe_opacity(
            Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS).mul_f32(fraction),
            Motion::Reduced,
        );
        assert!((held - WINDOW_TAB_BREATHE_REDUCED_OPACITY).abs() < 1e-6);
    }
    // And it is genuinely quieter than a mark that is not working at all,
    // which is what makes it still say something.
    const { assert!(WINDOW_TAB_BREATHE_REDUCED_OPACITY < 1.0) };
}

/// PIN (`docs/DESIGN.md` §7.1.5b, 2026-07-18; the clock ruled by the owner
/// 2026-09-20) — **the waiting dot breathes with the halo and never goes out.**
///
/// The mock-up wrote `.unreaddot.await { animation: fcpulse .9s infinite }`
/// (`ui-mockup.html:346`) and never defined `@keyframes fcpulse`, so the number
/// in it was never a curve anybody had seen. The owner ruled the missing curve
/// to be the halo's: *two things saying one fact breathe together*, so the dot
/// rides the very same 1.7s breath at the very same phase and differs only in
/// what it does with it — a glow goes out, a badge does not.
///
/// Red gate: give the dot a period or a phase of its own and the first pair of
/// assertions part company; let it ramp from zero like the halo and the floor
/// assertion goes red; answer anything but a flat 1.0 under `Reduced` and the
/// last does — which would be the accessibility setting deleting a claim.
#[test]
fn the_waiting_dot_breathes_with_the_halo_and_never_goes_out() {
    let period = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS);
    let at = |fraction: f32| wait_pulse(period.mul_f32(fraction), Motion::Full);

    // One clock: brightest at the same instant, faintest at the same instant.
    assert!(
        (at(0.5).halo - 1.0).abs() < 1e-6 && (at(0.5).dot - 1.0).abs() < 1e-6,
        "both are full at the one keyframe the mock-up writes: {:?}",
        at(0.5)
    );
    assert!(
        at(0.0).halo.abs() < 1e-6 && (at(0.0).dot - WINDOW_TAB_BREATHE_MIN_OPACITY).abs() < 1e-6,
        "and at the trough the glow is out while the badge is merely faint: {:?}",
        at(0.0)
    );
    assert!(
        (at(0.25).dot - at(0.75).dot).abs() < 1e-6 && (at(2.5).dot - at(0.5).dot).abs() < 1e-6,
        "symmetric about the keyframe, and `infinite`"
    );

    // One sample: the dot is a function of the halo's own number at every
    // phase, which is what stops the two drifting.
    for step in 0..=64 {
        let pulse = at(step as f32 / 64.0);
        assert!(
            (0.0..=1.0).contains(&pulse.halo),
            "the halo stays in gamut at phase {step}/64: {pulse:?}"
        );
        assert!(
            (WINDOW_TAB_BREATHE_MIN_OPACITY..=1.0).contains(&pulse.dot),
            "and the dot never goes out at phase {step}/64: {pulse:?}"
        );
        assert!(
            (pulse.dot
                - (WINDOW_TAB_BREATHE_MIN_OPACITY
                    + (1.0 - WINDOW_TAB_BREATHE_MIN_OPACITY) * pulse.halo))
                .abs()
                < 1e-6,
            "one breath, two faces of it: {pulse:?}"
        );
    }

    // Reduced motion: each channel's own value with the animation stood down,
    // and neither of them is "not waiting".
    for fraction in [0.0_f32, 0.25, 0.5, 0.75, 1.0, 3.7] {
        let pulse = wait_pulse(period.mul_f32(fraction), Motion::Reduced);
        assert_eq!(
            (pulse.halo, pulse.dot),
            (0.0, 1.0),
            "an animation turned off leaves the element as it is written: a \
             keyframe set with no 0% frame leaves no shadow, and `.unreaddot` \
             is an opaque dot"
        );
    }
}

/// PIN (T2 D41): the indeterminate arc turns once per its own period, and
/// stands still — rather than vanishing — when animation is off.
#[test]
fn the_indeterminate_arc_turns_once_a_period_and_holds_still_when_asked() {
    let period = Duration::from_millis(WINDOW_TAB_RING_SPIN_PERIOD_MS);
    assert_eq!(
        indeterminate_start_milliturns(Duration::ZERO, Motion::Full),
        0
    );
    assert_eq!(
        indeterminate_start_milliturns(period.mul_f32(0.25), Motion::Full),
        250
    );
    assert_eq!(
        indeterminate_start_milliturns(period.mul_f32(0.5), Motion::Full),
        500
    );
    // A whole turn returns to the start rather than running off the end.
    assert_eq!(indeterminate_start_milliturns(period, Motion::Full), 0);
    assert_eq!(
        indeterminate_start_milliturns(period.mul_f32(7.25), Motion::Full),
        250
    );
    // Stopped, it holds at noon — and it is still an arc. A ring with no
    // arc at all would be reporting 0%, which is a different claim.
    for fraction in [0.0_f32, 0.3, 0.75, 9.1] {
        assert_eq!(
            indeterminate_start_milliturns(period.mul_f32(fraction), Motion::Reduced),
            0
        );
    }
}

/// PIN (T2): the arc eases to a new reading instead of snapping to it, and
/// stops owing frames the moment it arrives.
///
/// `.pring .arc { transition: stroke-dashoffset .3s ease }` (line 279).
/// The "stops owing frames" half is what keeps an idle window idle: a tween
/// that never reports itself finished is a 60fps loop that never ends.
#[test]
fn the_arc_eases_to_a_new_reading_and_then_stands_down() {
    let started = Instant::now();
    let tween = SweepTween {
        from: 200,
        to: 700,
        started,
    };
    let duration = Duration::from_millis(WINDOW_TAB_RING_SWEEP_TRANSITION_MS);

    let (at_start, moving) = tween.sample(started);
    assert_eq!(at_start, 200, "it begins where the arc already was");
    assert!(moving);

    let (midway, moving) = tween.sample(started + duration / 2);
    assert!(moving);
    assert!(
        (200..=700).contains(&midway),
        "the tween left its endpoints: {midway}"
    );

    let (arrived, moving) = tween.sample(started + duration);
    assert_eq!(arrived, 700, "it arrives exactly, not nearly");
    assert!(!moving, "an arrived tween owes no further frames");
    let (still_there, moving) = tween.sample(started + duration * 4);
    assert_eq!(still_there, 700);
    assert!(!moving);

    // `ease` leaves quickly and arrives slowly, so by the halfway point it
    // is already past halfway. A linear ramp would sit exactly on 450.
    assert!(
        midway > 450,
        "the arc must use CSS `ease`, which front-loads its travel: {midway}"
    );
}

/// PIN (T2): the two CSS timing functions are solved, not approximated.
///
/// Both are checked against their defining points — the endpoints every
/// curve shares, and the midpoint value that tells them apart. `ease` and
/// `ease-in-out` are symmetric only in the second case, and a solver that
/// silently returned one for the other would pass every endpoint test.
#[test]
fn the_css_timing_curves_are_the_real_beziers() {
    for curve in [EASE, EASE_IN_OUT] {
        assert_eq!(cubic_bezier(0.0, curve), 0.0);
        assert_eq!(cubic_bezier(1.0, curve), 1.0);
        // Out of range in either direction is clamped, not extrapolated.
        assert_eq!(cubic_bezier(-1.0, curve), 0.0);
        assert_eq!(cubic_bezier(2.0, curve), 1.0);
        // Monotonic: time only moves forward, so the curve must too.
        let mut previous = 0.0_f32;
        for step in 0..=200 {
            let value = cubic_bezier(step as f32 / 200.0, curve);
            assert!(value >= previous - 1e-4, "{curve:?} went backwards");
            previous = value;
        }
    }
    // `ease-in-out` is symmetric about its centre and therefore passes
    // through exactly .5 at half time.
    assert!((cubic_bezier(0.5, EASE_IN_OUT) - 0.5).abs() < 1e-3);
    // `ease` is not symmetric: it is already well past half by half time,
    // which is the whole difference between the two and the reason both
    // exist rather than one standing in for the other.
    assert!(cubic_bezier(0.5, EASE) > 0.75);
}

#[test]
fn theme_mode_resolution_covers_every_os_theme_input() {
    use bt_persist::ThemeModeV1::{Dark, Light, System};
    use winit::window::Theme::{Dark as OsDark, Light as OsLight};

    for (mode, os_theme, expected) in [
        (System, Some(OsDark), Theme::Dark),
        (System, Some(OsLight), Theme::Light),
        (System, None, Theme::Dark),
        (Light, Some(OsDark), Theme::Light),
        (Light, Some(OsLight), Theme::Light),
        (Light, None, Theme::Light),
        (Dark, Some(OsDark), Theme::Dark),
        (Dark, Some(OsLight), Theme::Dark),
        (Dark, None, Theme::Dark),
    ] {
        assert_eq!(resolve_theme_mode(mode, os_theme), expected);
    }
}

/// The mutant: the window this fix replaced, which reprojected on every
/// animation tick no matter what the picture on the glass already said.
fn always_recompose(_: PictureOnGlass) -> bool {
    false
}

/// The same six hundred ticks under the mutant, so the assertion above is
/// known to have teeth: the count it asserts is zero was six hundred.
#[test]
fn the_unconditional_reprojection_this_replaced_costs_one_projection_a_tick() {
    let mut harness = PtyPresentationHarness::new(80, 24);
    harness.feed_drain(b"$ sleep 30\r\n");
    harness.present_pending();
    let projections_before = harness.viewport_frames;

    for _ in 0..600 {
        harness.chrome_tick(always_recompose);
    }

    assert_eq!(harness.viewport_frames - projections_before, 600);
}

/// The whole point of zooming about the pointer: whatever pixel of the
/// picture the hand was on stays under the hand.
#[test]
fn zooming_about_a_point_leaves_that_point_where_it_was() {
    // Read off `image_destination`'s own arithmetic: a picture point `u`
    // from the picture's centre lands at `centre + pan + u * scale`.
    let anchor_at =
        |scale: f32, pan: [f32; 2], u: [f32; 2]| [pan[0] + u[0] * scale, pan[1] + u[1] * scale];
    let (old, new) = (0.4_f32, 1.7_f32);
    let pan = [37.0, -12.0];
    // The pointer, measured from the body's centre.
    let point = [180.0, -95.0];
    let u = [(point[0] - pan[0]) / old, (point[1] - pan[1]) / old];
    assert_eq!(anchor_at(old, pan, u), point);

    let moved = zoom_about(point, old, new, pan);
    let after = anchor_at(new, moved, u);
    assert_close(after[0], point[0], "the anchor did not move sideways");
    assert_close(after[1], point[1], "nor down");

    assert_eq!(
        zoom_about(point, old, old, pan),
        pan,
        "and a zoom that changes nothing moves nothing"
    );
}

/// **The bare wheel is the column's, whatever else is held down** (user
/// ruling 2026-08-21).
///
/// The list this window puts under the pointer is the thing with content out
/// of sight, so the gesture every other list in the product answers is the
/// one this one answers too. `Shift` is named here on purpose: it is spent
/// twice already on the wheel — the preview body's other axis, and
/// [`wheel_route`]'s "this notch is mine, not the program's" — so a reader
/// holding it over the column gets the list, not a third meaning.
#[test]
fn a_bare_notch_over_the_column_is_the_lists() {
    for held in [
        ModifiersState::empty(),
        ModifiersState::SHIFT,
        ModifiersState::CONTROL,
        ModifiersState::CONTROL.union(ModifiersState::SHIFT),
        ModifiersState::SUPER,
    ] {
        assert_eq!(
            column_notch(held),
            ColumnNotch::List,
            "{held:?} over the card column has to scroll the card column"
        );
    }
}

/// PIN — every surface that says the product's name out loud says the same
/// name, and none of them still says the old one.
///
/// A rename is not one edit, it is seven, spread over five modules and two
/// shell scripts, and six of the seven are sentences that *embed* the name
/// rather than reading it: `&'static str` constants cannot interpolate
/// [`APP_NAME`], because `concat!` takes literals and nothing else. So the
/// thing that makes them one decision is this test rather than the type
/// system, and it is written as both halves on purpose — the positive one
/// (each surface names the product) would pass a sentence that named it
/// twice, once under each brand.
///
/// The old spelling is checked for by name because that is what a half-done
/// rename leaves behind, and because there is no other way to state "and
/// nothing here still says the thing we stopped saying".
///
/// Red gate: rename any one of these and leave the others, in either
/// direction, and this names the surface that moved. It is what the
/// BetterTerminal → Folio rename was carried out under.
#[test]
fn every_surface_that_names_the_product_says_the_same_name() {
    let banner = banner_line("something failed to start");
    let surfaces: [(&str, &str); 7] = [
        ("the window title bar", APP_NAME),
        ("the startup trace title", WINDOW_TITLE),
        (
            "a pane this build cannot draw",
            seats::placeholder_seat_notice(),
        ),
        (
            "the settings dialog's startup row",
            settings::SettingsRow::DefaultProfile
                .literal_description(&settings::SettingsValues::sample())
                .expect("the startup row's sentence is a literal"),
        ),
        ("the restore prompt", restore::sub_text()),
        ("this terminal's own banner", &banner),
        (
            "the TERM_PROGRAM this terminal declares",
            bt_pty::TERM_PROGRAM,
        ),
    ];
    for (surface, text) in surfaces {
        assert!(
            text.contains(APP_NAME),
            "{surface} must name the product ({APP_NAME:?}): {text:?}"
        );
        assert!(
            !text.contains("BetterTerminal"),
            "{surface} still carries the name this product had before \
                 2026-08-13: {text:?}"
        );
    }
    // The storage directory is the eighth surface and is deliberately not
    // touched here: reading it would relocate the directory of whoever is
    // running the tests. `persist::tests` pins its two names against
    // `APP_NAME` without going near `%APPDATA%`.
}

// ── T5: the drag's own clocks and rulings ──

/// PIN (§7.1.6b″) — **"in flight" is a fraction of the journey, not a
/// distance, and a hand is always the whole of it.**
///
/// Three claims, and each one is a way the obvious implementation goes
/// wrong:
///
/// * A tab *in the hand* answers `1.0` even when the pointer has carried it
///   back to exactly its own slot. `offset != 0.0` gets this wrong, and the
///   frame it gets wrong is the one where a card you are still holding drops
///   back into the list under its neighbours.
/// * A settling tab answers the *remaining fraction* and not `from`. Two
///   tabs displaced by one slot and by five are one event at two magnitudes;
///   a shadow keyed on the distance is five times heavier for the second.
/// * **Reduced motion answers `0.0`**, because there is no slide: with no
///   animation the tab is simply in its slot, and lifting a thing that is
///   not moving would be inventing a state the reader asked not to see.
///
/// Red gate: derive the answer from `sample().0` and the first assertion
/// goes red; drop the `motion` filter in `FlipTween::remaining` and the last
/// one does.
#[test]
fn a_flight_is_how_much_of_the_journey_is_left_and_a_hand_is_all_of_it() {
    let now = Instant::now();
    let resting = FlipTween::default();

    assert_eq!(
        resting.flight(now, Motion::Full, None),
        0.0,
        "a tab nothing has touched is not in flight"
    );
    assert_eq!(
        resting.flight(now, Motion::Full, Some(0.0)),
        1.0,
        "a tab carried back to its own slot is still in the hand"
    );

    // Two tabs displaced by very different distances, sampled at the same
    // instant of the same tween: one event, one answer.
    let mut near = FlipTween::default();
    let mut far = FlipTween::default();
    near.displace(12.0, now, Motion::Full);
    far.displace(600.0, now, Motion::Full);
    let quarter = now + TAB_FLIP / 4;
    assert!(
        (near.flight(quarter, Motion::Full, None) - far.flight(quarter, Motion::Full, None)).abs()
            < 1e-6,
        "a one-slot swap and a five-slot swap are the same flight"
    );
    assert!(
        near.flight(quarter, Motion::Full, None) < 1.0,
        "and it has already begun to fade"
    );
    assert_eq!(
        near.flight(now + TAB_FLIP, Motion::Full, None),
        0.0,
        "it is over when the tween is"
    );

    let mut reduced = FlipTween::default();
    reduced.displace(120.0, now, Motion::Reduced);
    assert_eq!(
        reduced.flight(quarter, Motion::Reduced, None),
        0.0,
        "there is no slide to be in the middle of when the machine has been \
             asked not to animate"
    );
}

#[test]
fn a_flip_runs_the_displacement_down_to_nothing_on_the_grab_curve() {
    // K117/K118. One motion, the base span, cubic-bezier(.2, 0, 0, 1).
    let now = Instant::now();
    let mut flip = FlipTween::default();
    assert_eq!(flip.sample(now, Motion::Full), (0.0, false));
    flip.displace(-96.0, now, Motion::Full);
    let (start, moving) = flip.sample(now, Motion::Full);
    assert!((start + 96.0).abs() < 1e-3, "it starts where the tab was");
    assert!(moving);
    let (mid, moving) = flip.sample(now + Duration::from_millis(80), Motion::Full);
    assert!(moving);
    assert!(
        mid > -96.0 && mid < 0.0,
        "and travels the whole way in between"
    );
    assert!(
        mid.abs() < 48.0,
        "the curve leaves fast: half the time is well past half the distance"
    );
    // The mock-up writes `.16s` at 6570; the archive answers the base span,
    // because a tab sliding one slot along a row is one interaction (§7.18).
    assert_eq!(TAB_FLIP, bt_render::MOTION_BASE);
    assert!(
        flip.sample(now + TAB_FLIP - Duration::from_millis(1), Motion::Full)
            .1,
        "still moving one millisecond short of the end"
    );
    assert_eq!(
        flip.sample(now + TAB_FLIP, Motion::Full),
        (0.0, false),
        "and at the end the tab is simply in its slot"
    );
}

#[test]
fn the_landing_wash_runs_out_over_its_own_two_hundred_milliseconds() {
    // K121. Only the `from` is a design value, so only how much of it is left
    // is a state.
    let now = Instant::now();
    let mut landing = LandTween::default();
    assert_eq!(landing.sample(now, Motion::Full), (0.0, false));
    landing.start(now, Motion::Full);
    assert_eq!(landing.sample(now, Motion::Full), (1.0, true));
    let mut last = 1.0;
    for step in 1..20 {
        let (left, moving) = landing.sample(now + Duration::from_millis(step * 10), Motion::Full);
        assert!(left <= last, "the wash only ever fades");
        assert!(moving);
        last = left;
    }
    assert_eq!(
        TAB_LAND,
        Duration::from_millis(200),
        "`animation: tab-land .2s` (mock-up 967)"
    );
    assert!(
        landing
            .sample(now + Duration::from_millis(199), Motion::Full)
            .1
    );
    assert_eq!(
        landing.sample(now + Duration::from_millis(200), Motion::Full),
        (0.0, false)
    );
}

/// PIN (user ruling 2026-08-25) — **the `…` chip's list runs deepest first,
/// and its rows point where they say.**
///
/// Windows Explorer's own breadcrumb `…` is the reference the ruling handed
/// over: 「由深到浅排,最近的隐藏级在最上,一路到根方向」. The geometry hands
/// over the *fold* order, which is the other way round, so the turn has to
/// happen somewhere and this is where.
///
/// **Asked with three levels**, because a fixture with two is symmetrical
/// under the very mistake this pin exists to catch.
///
/// RED EVIDENCE (2026-08-25): the chip did not build a list at all — it
/// raised the file menu on `folded.last()`, one folder, unnamed.
///
/// MUTATIONS: drop the `.rev()`; take the name from one segment and the
/// folder from another.
#[test]
fn the_folded_levels_read_from_the_deepest_towards_the_root() {
    let path = &host_path(r"D:\Developer\folio-terminal\test-assets\huge.txt");
    // What `preview_rail_geometry` folds: the middle, nearest the root
    // first — the folders between the top of the host's row (a drive crumb
    // on Windows, none off it) and the file.
    let segments = crumb_segments(path);
    let at = |name: &str| {
        segments
            .iter()
            .position(|(segment, _)| segment == name)
            .expect("every folder of the path is a segment")
    };
    let levels = folded_levels(
        path,
        &[at("Developer"), at("folio-terminal"), at("test-assets")],
    );
    assert_eq!(
        levels
            .iter()
            .map(|level| level.name.as_str())
            .collect::<Vec<_>>(),
        vec!["test-assets", "folio-terminal", "Developer"],
    );
    assert_eq!(
        levels
            .iter()
            .map(|level| level.folder.clone())
            .collect::<Vec<_>>(),
        vec![
            host_path(r"D:\Developer\folio-terminal\test-assets"),
            host_path(r"D:\Developer\folio-terminal"),
            host_path(r"D:\Developer"),
        ],
        "every row goes to the place it names"
    );
    assert!(folded_levels(path, &[]).is_empty());
}

/// RAIL (user ruling 2026-08-24) — **a path reads as its segments, each one
/// naming the place it leads.**
///
/// The two things about this walk a reader can see and a re-parse would get
/// wrong: the drive and its separator are **one** segment (a row that drew
/// both would offer a folder called `\`), and every segment's target is the
/// path built *up to and including it* rather than a prefix guessed from the
/// name.
///
/// MUTATIONS:
/// ① give `RootDir` a segment of its own — the row grows a `\` between the
///    drive and the first folder, and pressing the drive stands the column
///    at `D:` (the process's current directory on that drive, which is not
///    where the reader pointed);
/// ② rebuild each target by joining the names — a path holding `..` or a
///    folder literally called `D:` lands somewhere else, which the last case
///    catches;
/// ③ drop the last segment because it is a file — the row loses the one
///    part the ruling says is bold, and the tail assertion goes red.
///
/// Windows only: a drive, its separator and `\` are Windows path grammar. The
/// row off Windows is `app_windows_tests::a_mac_breadcrumb_starts_at_the_home_crumb_or_at_a_name`.
#[cfg(windows)]
#[test]
fn a_path_reads_as_segments_that_each_name_where_they_lead() {
    let walked = crumb_segments(Path::new(r"D:\Developer\Folio\docs\DESIGN.md"));
    let names: Vec<&str> = walked.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec!["D:", "Developer", "Folio", "docs", "DESIGN.md"],
        "the drive and its separator are one segment, and the file is the last"
    );
    let targets: Vec<PathBuf> = walked.into_iter().map(|(_, target)| target).collect();
    assert_eq!(
        targets,
        vec![
            PathBuf::from(r"D:\"),
            PathBuf::from(r"D:\Developer"),
            PathBuf::from(r"D:\Developer\Folio"),
            PathBuf::from(r"D:\Developer\Folio\docs"),
            PathBuf::from(r"D:\Developer\Folio\docs\DESIGN.md"),
        ],
        "pressing the drive stands the column at its root, not at the \
             process's directory on that drive"
    );
    // A relative path keeps what it was written with: resolving `..` here
    // would be this walk inventing a place the reader never named.
    let relative = crumb_segments(Path::new(r"..\sibling\notes.md"));
    assert_eq!(
        relative
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["..", "sibling", "notes.md"]
    );
    assert_eq!(
        relative.last().expect("a tail").1,
        PathBuf::from(r"..\sibling\notes.md")
    );
    // A path that begins at a root with no drive still has a top: `\` is the
    // only name that level of the tree has.
    let rooted = crumb_segments(Path::new(r"\srv\share"));
    assert_eq!(
        rooted
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![std::path::MAIN_SEPARATOR_STR, "srv", "share"]
    );
}

/// PIN — **P86: letting go where you picked it up is a clean "never mind".**
///
/// The user's report of 2026-07-17 is what this is for: without a home
/// rectangle, retracting a drag lands it on the source's own edge zone and
/// splits a pane anyway, so there is no gesture at all for "actually, no".
/// It is K135's sentence generalised from a seat identity to a rectangle,
/// which is what a payload that is not a pane needs — and it is the seam an
/// inline image drag plugs its `srcRect` into unchanged (P86's second half).
///
/// Mutation: make the bounds exclusive on the far edges — a release on the
/// row's own last pixel column stops being a retraction, which is the one
/// place a hand that has travelled exactly six pixels tends to end up.
#[test]
fn a_payload_let_go_over_its_own_ground_lands_nowhere() {
    let home = Some([100.0, 200.0, 300.0, 220.0]);
    assert!(over_home_ground(home, at(150.0, 210.0)), "inside");
    assert!(over_home_ground(home, at(100.0, 200.0)), "its own corner");
    assert!(over_home_ground(home, at(300.0, 220.0)), "and its far one");
    assert!(!over_home_ground(home, at(301.0, 210.0)), "just outside");
    assert!(
        !over_home_ground(None, at(150.0, 210.0)),
        "a source with no home ground retracts nowhere: a tab goes back to its slot"
    );
}
