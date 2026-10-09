//! **`notice`, as the application drives it.** Tests whose first assertion is about
//! `notice`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    CROSS_DPI, a_shell, cross_solve, facts, facts_with, quiet, squeezed_body,
};

/// PIN (real-machine bug, 2026-08-10): **a tab whose shell said nothing wears
/// nothing, however many frames it published.**
///
/// The reported window: a pinned three-pane tab, no new output anywhere, and
/// a blue dot on it the instant the user moved to the next tab. The recorded
/// ledger at the moment of the switch was `frames=95 seen=94` — the shell had
/// been silent for the whole session, and the extra frame was a chrome
/// repaint that went out after the turn's last reconciliation and before the
/// switch was paid.
///
/// Two mutations die here. Count publication instead of output and the
/// thirty-six silent frames below become thirty-six units of news; measure
/// unread against `status.published_revision` and [`quiet`]'s absurd value
/// lights the tab immediately.
#[test]
fn frames_are_not_output_and_a_silent_tab_stays_silent() {
    let mut output = 0;
    let mut seen = 0;
    for _ in 0..36 {
        // A blinking cursor, a hovered link, a repainted chrome: the window
        // publishes, the shell says nothing.
        output = output_revision(output, false, false);
        seen = seen_revision(seen, output, true);
    }
    assert_eq!(
        (output, seen),
        (0, 0),
        "a frame is not a sentence: nothing was said and nothing is owed"
    );
    assert!(!facts(output, seen, false).has_unseen_output());
    assert_eq!(
        facts(output, seen, false).claim(),
        StatusClaim::Silent,
        "the tab the user just left had nothing to report"
    );
}

/// PIN (T2 D35, the per-leaf half): work in flight suppresses the finished
/// claim of **the shell doing the work**, and of nothing else.
///
/// This is the pin that fails the moment someone re-reads D35 as a tab-wide
/// rule — "any progress anywhere means the tab has not finished". A pane
/// with a download running contributes `Silent` because *it* has not
/// finished; its quiet sibling has finished and gone unread and contributes
/// `Unread`; and the tab wears `Unread`, because a tab must never say less
/// than its panes do (D34). Suppressing tab-wide would swallow the
/// sibling's news under a download it has nothing to do with, and the user
/// would learn about it only by opening the tab to check — which is the one
/// thing the badge exists to save them.
///
/// Red gate: rewrite `fleet_claim` as `loudest_claim` over the fleet with a
/// tab-wide `any(work_in_flight)` gate in front of it and this reads
/// `Silent`.
#[test]
fn a_download_in_one_pane_does_not_silence_its_quiet_siblings_unread() {
    // Leaf A: unseen output *and* a download still running.
    let mut downloading = quiet();
    downloading.progress = Some(ProgressState::Normal(40));
    assert_eq!(
        facts_with(downloading, 12, 3, false).claim(),
        StatusClaim::Silent,
        "per leaf, the download suppresses this pane's own finished claim"
    );
    // Leaf B: quiet, and holding output nobody has read.
    assert_eq!(facts(7, 3, false).claim(), StatusClaim::Unread);

    assert_eq!(
        fleet_claim([facts_with(downloading, 12, 3, false), facts(7, 3, false)]),
        StatusClaim::Unread,
        "the suppression is per leaf; the aggregation is per tab"
    );
    // Order must not matter: `max` is commutative and the pin says so out
    // loud, because a fold that carried a suppression forward would not be.
    assert_eq!(
        fleet_claim([facts(7, 3, false), facts_with(downloading, 12, 3, false)]),
        StatusClaim::Unread
    );
    // And with no quiet sibling there is genuinely nothing finished to
    // report, so the tab is silent — D35 intact where it does apply.
    assert_eq!(
        fleet_claim([facts_with(downloading, 12, 3, false)]),
        StatusClaim::Silent
    );
}

/// PIN (T2 D35): work in flight suppresses every "finished" claim.
///
/// The mock-up's own comment is a user ruling (line 1920): "an active
/// download is still WORK IN FLIGHT: no finished-unread claim until the
/// progress ends". The ring and the breathing icon are already reporting
/// what is happening, and a dot beside them would be a third voice on one
/// fact — and a wrong one, since nothing has finished.
#[test]
fn a_session_still_working_makes_no_finished_claim() {
    let unseen = quiet();
    // Quiet and unseen: the plain unread claim.
    assert_eq!(facts_with(unseen, 9, 4, false).claim(), StatusClaim::Unread);

    // The same session, still running.
    let mut working = unseen;
    working.working = true;
    assert_eq!(
        facts_with(working, 9, 4, false).claim(),
        StatusClaim::Silent
    );

    // The same session, reporting progress — suppressed in every flavour,
    // because every one of them means a run that has not ended.
    for state in [
        ProgressState::Normal(40),
        ProgressState::Indeterminate,
        ProgressState::Paused(Some(40)),
        ProgressState::Error(Some(40)),
    ] {
        let mut in_flight = unseen;
        in_flight.progress = Some(state);
        assert_eq!(
            facts_with(in_flight, 9, 4, false).claim(),
            StatusClaim::Silent,
            "{state:?} is work in flight, not a finished claim"
        );
    }

    // A failure is suppressed by the same rule, for the same reason.
    let mut failing = unseen;
    failing.failure_exit_code = Some(1);
    assert_eq!(
        facts_with(failing, 9, 4, false).claim(),
        StatusClaim::Failed
    );
    failing.progress = Some(ProgressState::Normal(10));
    assert_eq!(
        facts_with(failing, 9, 4, false).claim(),
        StatusClaim::Silent
    );
}

/// PIN (T2): the bell is latched, so it survives what suppresses the rest.
///
/// A bell is a thing that *rang* — a past event, not a state — so a session
/// that is busy again has still rung, and the claim stands until the user
/// looks. This is the one claim the work-in-flight rule does not touch.
#[test]
fn the_bell_outlives_the_work_that_followed_it() {
    let mut ringing = quiet();
    ringing.bell = Some(bt_term::BellSource::Bel);
    ringing.working = true;
    ringing.progress = Some(ProgressState::Indeterminate);
    assert_eq!(facts_with(ringing, 9, 9, false).claim(), StatusClaim::Bell);
    // Even with nothing unread — the bell is not an unread claim.
    assert_eq!(facts_with(ringing, 9, 99, false).claim(), StatusClaim::Bell);
}

/// PIN (T2 J97): unread is "said since you last saw it", and the tab you are
/// looking at is never unread.
///
/// Red gate: without the active-tab clause the tab under the user's eyes
/// wears a dot asking them to look at it, in the window between its shell
/// speaking and the frame that carries those words being presented.
#[test]
fn unread_is_what_was_said_since_the_last_look_and_never_on_the_active_tab() {
    // Behind an inactive tab, new output is unread.
    assert!(facts(7, 3, false).has_unseen_output());
    // Caught up: nothing new.
    assert!(!facts(7, 7, false).has_unseen_output());
    // The active tab is the one being read, whatever its ledger says.
    assert!(!facts(7, 3, true).has_unseen_output());
    assert_eq!(facts(7, 0, true).claim(), StatusClaim::Silent);
    // A failure on the active tab makes no dot either — the same clause
    // covers it, because a failure is a kind of unread.
    let mut failed = quiet();
    failed.failure_exit_code = Some(2);
    assert_eq!(facts_with(failed, 7, 0, true).claim(), StatusClaim::Silent);
    assert_eq!(facts_with(failed, 7, 0, false).claim(), StatusClaim::Failed);
}

/// RED (confirmation review of `6049179a`, P1) — **a press handed to the
/// program is released to it, however the hand comes up over the capsule or
/// the strip, and the route comes off.**
///
/// The in-pane surfaces decide where a gesture *starts*. `6049179a` let them
/// decide where one ends too: the cell root (`pane_hit_context`) refuses a
/// point on the capsule or the strip, `mouse_input` stopped at the failed cell
/// lookup, and the child was given a press and never its release while
/// `MouseRoute::Forward` stayed latched.
///
/// Run: a lone terminal wearing a strip, a forwarded press on a cell, and the
/// release with the pointer on the strip's `×` — a point no cell lookup names,
/// which the owner's clamp folds into the body's first row — comes back as the
/// release bytes and clears the route. Read: the release is ended by
/// `release_owned_gesture` above every surface claim, from the clamped owner
/// cell, and a routed drag's moves are reported the same way.
///
/// Red gate: move the owned release below the cell lookup (where the forwarded
/// release used to be answered) or measure it with `pane_hit_context`, and the
/// ordering or the clamp assertion fails; remove the drag-motion station and
/// the motion assertion fails.
#[test]
fn a_forwarded_press_is_released_to_its_pane_over_the_capsule_and_the_strip() {
    let scale = seats::scale_ppm(CROSS_DPI) as f32 / 1_000_000.0;
    let mut seats = seats::Seats::lone_terminal();
    let seat = seats.terminals()[0];
    seats.set_notices(std::collections::BTreeSet::from([seat]));
    let (layout, _) = cross_solve(&seats);
    let body = seats::pane_body_viewport(&seats, &layout, seat, scale).expect("a placed pane");
    let strip = seats::pane_notice_strip(&seats, &layout, seat, scale).expect("a strip");
    // No band kind is worn by a terminal since T-INTEGRATION-INJECT-1 retired `Offer` and
    // `Added`; the seat model and the router do not tell a terminal's strip from a preview's, so
    // the release's ordering is pinned with a band that still exists.
    let bar = notice::lay_out(
        strip,
        notice::NoticeSay::band(notice::Notice::DiskChanged),
        &[90.0, 90.0],
        scale,
    );
    let close = bar.close.expect("a band has its `×`");
    let (x, y) = (
        f64::from((close[0] + close[2]) / 2.0),
        f64::from((close[1] + close[3]) / 2.0),
    );
    assert!(
        y < f64::from(body.y),
        "the strip's `×` is above the first row of cells, so no cell lookup names it"
    );
    let (clamped_x, clamped_y) = clamp_into_body(body, x, y);
    assert_eq!(
        clamped_y, 0.0,
        "the owner's clamp folds it into the first row"
    );
    assert!(clamped_x > 0.0 && clamped_x < f64::from(body.width));

    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1000h\x1b[?1006h").unwrap();
    let mut route = None;
    let pressed = route_forwarded_mouse_button(
        &mut route,
        ElementState::Pressed,
        input::MouseProtocolButton::Left,
        bt_render::GridHit { row: 3, column: 4 },
        session.terminal_modes(),
        ModifiersState::empty(),
        PressedCellTarget::Ordinary,
        a_shell(),
    );
    assert!(pressed.is_some() && matches!(route, Some(MouseRoute::Forward { .. })));
    let released = route_forwarded_mouse_button(
        &mut route,
        ElementState::Released,
        input::MouseProtocolButton::Left,
        bt_render::GridHit { row: 0, column: 9 },
        session.terminal_modes(),
        ModifiersState::empty(),
        PressedCellTarget::Ordinary,
        a_shell(),
    )
    .expect("the release is owed to the forwarded press");
    assert_eq!(
        released, b"\x1b[<0;10;1m",
        "an SGR release at the clamped cell"
    );
    assert!(route.is_none(), "and the route comes off");

    let input = squeezed_body("Runtime", "mouse_input");
    let owned = input
        .find("ifstate==ElementState::Released&&self.release_owned_gesture(button)?")
        .expect("a release is first offered to the gesture that owns it");
    for later in [
        "self.quit_card_layout()",
        "self.press_in_pane_surface(button,position)?",
        "self.chrome_mouse_input(state,button,position)?",
        "self.pane_frame_hit()",
    ] {
        let at = input
            .find(later)
            .unwrap_or_else(|| panic!("`{later}` is in the router"));
        assert!(owned < at, "the owned release is answered before `{later}`");
    }
    let release = squeezed_body("Runtime", "release_owned_gesture");
    assert!(
        release.contains("Some(MouseRoute::Forward{button:latched,owner,..})=>")
            && release.contains("self.forwarded_gesture_hit(seat)")
            && release.contains("ElementState::Released,"),
        "the forwarded release is sent from the owner's cell"
    );
    assert!(
        release.contains("self.window.mouse_route=None;returnOk(true);"),
        "and a pane with no frame still lets go of the route"
    );
    let hit = squeezed_body("Runtime", "forwarded_gesture_hit");
    assert!(
        hit.contains("self.drag_hit_in_pane(seat)?") && !hit.contains("pane_hit_context"),
        "the owner's cell is clamped into its body, not refused by the surfaces over it"
    );
    let moved = squeezed_body("Runtime", "pointer_moved");
    let routed = moved
        .find("ifmatches!(self.window.mouse_route,Some(MouseRoute::Forward{..})){returnself.forward_owned_drag_motion();}")
        .expect("a routed drag's moves go to the pane that took the press");
    let guard = moved.find("ifhit.is_none(){").expect("the cell guard");
    assert!(
        routed < guard,
        "ahead of the guard that needs a cell under the pointer"
    );
    assert!(
        squeezed_body("Runtime", "forward_owned_drag_motion")
            .contains("self.forwarded_gesture_hit(seat)"),
        "measured the same way the release is"
    );
}
