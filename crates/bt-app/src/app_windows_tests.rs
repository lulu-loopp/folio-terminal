//! **The crate root: windows and session.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    EndSessionHome, ResizeGateHarness, TITLE_FRAME, calls_of, claim_after_a_look, cross_tab, facts,
    facts_with, free_calls_of, grid_of, item_body, latched, launch_plan_on_disk, method_body,
    on_the_window_thread, placement, quiet, reader_names, record_as_the_app_does, shells_document,
    source, tab_with_a_files_column, the_system_asks, waiting,
};
use bt_source::ItemQuery;
use std::time::Duration;

/// PIN (user ruling 2026-08-19) — **the quiet refusals of a name that
/// cannot exist.**
///
/// A name that cannot exist is a fact about the draft and is refused in
/// silence. The other half this pin used to carry — a file renamed into its
/// own name with different capitals is not a collision — is a question about
/// the file's identity now, and is pinned on a real volume by
/// `a_case_only_rename_never_replaces_a_different_file` (B-AUDIT-046).
///
/// Red gate: drop `is_control` and a name carrying a tab character reaches
/// `fs::rename`.
#[test]
fn a_name_windows_will_not_take_is_refused_before_the_filesystem_is_asked() {
    for name in ["notes.md", ".gitignore", "a b c.txt", "笔记.md"] {
        assert_eq!(
            bt_platform::judge_file_name(name, bt_platform::HostPlatform::Windows),
            None,
            "{name} is a name a file can have"
        );
    }
    for name in [
        r"a\b", "a/b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "a\tb",
    ] {
        assert!(
            bt_platform::judge_file_name(name, bt_platform::HostPlatform::Windows).is_some(),
            "{name:?} is not"
        );
    }
}

/// RED (B-ENDSESSION) — **a shell the system ends after the question keeps its pane in the
/// document, and the next start revives it as an ordinary saved leaf.**
///
/// Both halves of rule 2: `App::record_session` asks whether the system's end holds the document
/// before it hands the store anything (its own text, read through `bt_source`), and what the
/// held document brings back at the next start — through the real reader, `plan_launch` and
/// `revive_plan` — is both shells of both pinned tabs, each in its folder.
///
/// MUTATION: drop `|| session_end::holds_the_document()` from `App::record_session`'s guard.
#[test]
fn a_shell_the_system_ends_after_the_freeze_keeps_its_pane_in_the_document() {
    let door = method_body("App", "record_session");
    let asked = door
        .find("session_end::holds_the_document()")
        .expect("the one door onto the document asks whether the system's end holds it");
    let handed = door
        .find(["self.session_store", ".record("].concat().as_str())
        .expect("the door still hands the store a document");
    assert!(
        asked < handed,
        "asked before the store is handed anything:\n{door}"
    );

    on_the_window_thread();
    let home = EndSessionHome::new("shell-ended");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    assert_eq!(the_system_asks(), Some(1));
    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    for end in session_end::take() {
        session_end::settle(end, &mut store, false);
    }

    let (plan, shapes) = launch_plan_on_disk(&home.session());
    assert_eq!(plan.open.len(), 2, "both pinned tabs open");
    let both = (
        vec![bt_layout::SeatKind::Terminal, bt_layout::SeatKind::Terminal],
        vec![Some(home.0.clone()), Some(home.0.clone())],
    );
    assert_eq!(
        shapes,
        vec![both.clone(), both],
        "each tab comes back with both shells, each in its folder"
    );
}

/// PIN (T2 J97): painting is seeing, so leaving a tab you have been reading
/// does not retroactively invent unread output.
///
/// Red gate, and the subtlest bug in this whole片. Suppressing the dot on
/// the active tab is only half the rule — it hides the claim without
/// answering it. If the ledger itself stops advancing while the tab is
/// watched, then everything the user sat and read piles up behind it, and
/// the moment they switch away the tab they *just left* lights up claiming
/// to hold output they had been staring at.
#[test]
fn painting_is_seeing_so_a_tab_you_read_owes_nothing_when_you_leave() {
    // Painted: the ledger takes the whole of what was said.
    assert_eq!(seen_revision(3, 90, true), 90);
    // Not painted: it holds still, which is what makes new output count.
    assert_eq!(seen_revision(3, 90, false), 3);

    // The whole sequence the bug lives in: open a tab, read a while, leave.
    let mut output = 0;
    let mut seen = 0;
    for _ in 0..40 {
        output = output_revision(output, true, false);
        seen = seen_revision(seen, output, true);
    }
    assert_eq!((output, seen), (40, 40));
    assert!(
        !facts(output, seen, false).has_unseen_output(),
        "a tab just switched away from holds nothing unread"
    );
    // And the very next thing it says does count.
    let output = output_revision(output, true, false);
    assert!(
        facts(output, seen, false).has_unseen_output(),
        "output after the switch is unread"
    );
}

/// PIN: **a shell that speaks behind a tab lights it** — the claim the whole
/// badge exists to make.
///
/// The counterpart to the test above, and the direction the old rule could
/// never satisfy: only the focused pane of the tab on screen ever published a
/// frame, so a ledger measured against publication stood still for exactly
/// the shells a badge is for. Output is counted where every leaf is drained,
/// so a background tab keeps an honest account of itself.
#[test]
fn a_shell_that_speaks_behind_a_tab_lights_it() {
    let seen = 7;
    let output = output_revision(7, true, false);
    assert_eq!(output, 8);
    assert!(facts(output, seen, false).has_unseen_output());
    assert_eq!(facts(output, seen, false).claim(), StatusClaim::Unread);
    // And it stays lit until those cells are painted — not until some tab
    // happens to become active, and not on a turn of the loop.
    assert_eq!(seen_revision(seen, output, false), 7);
    assert_eq!(seen_revision(seen, output, true), 8);
}

/// PIN: **a reprint we asked for is not the program speaking.**
///
/// A leaf told its new size answers with a redrawn prompt, and a badge that
/// counted it would light every background tab every time the window
/// changed shape. The exemption is bounded by the leaf's own resize
/// transaction, so the byte after quiescence counts again — and the price,
/// stated in [`output_revision`], is that a shell printing *only* inside its
/// own resize window goes unremarked.
#[test]
fn a_reprint_we_asked_for_is_not_the_program_speaking() {
    assert_eq!(
        output_revision(4, true, true),
        4,
        "bytes arriving inside the transaction are the echo of our own question"
    );
    assert_eq!(
        facts(output_revision(4, true, true), 4, false).claim(),
        StatusClaim::Silent
    );
    // The transaction closes, and the shell's next word is news again.
    assert_eq!(output_revision(4, true, false), 5);
    assert!(facts(5, 4, false).has_unseen_output());
}

/// PIN: **every painted pane is seen, not only the one holding the
/// keyboard.**
///
/// Three panes on screen are three panes the user is reading. Squaring only
/// the focused leaf leaves its siblings holding a backlog they earned while
/// in plain sight, and the tab lights up for them the moment it is left.
///
/// The second half is the same rule refusing to overreach: a pane that did
/// not fit on screen was not painted, so its backlog is real and survives.
#[test]
fn every_painted_pane_is_seen_not_only_the_one_holding_the_keyboard() {
    let panes = [(5_u64, 0_u64), (9, 0), (2, 0)];
    let painted: Vec<u64> = panes
        .iter()
        .map(|&(output, seen)| seen_revision(seen, output, true))
        .collect();
    assert_eq!(painted, vec![5, 9, 2]);
    assert_eq!(
        fleet_claim(
            panes
                .iter()
                .zip(&painted)
                .map(|(&(output, _), &seen)| facts(output, seen, false))
        ),
        StatusClaim::Silent,
        "a tab every pane of which was on the glass owes nothing when you leave it"
    );
    let overflowed = seen_revision(0, 3, false);
    assert_eq!(overflowed, 0);
    assert_eq!(
        fleet_claim([facts(3, overflowed, false)]),
        StatusClaim::Unread,
        "a pane that never made it to the glass is honestly unread"
    );
}

/// PIN: **output that never reached the glass survives the switch.**
///
/// The honest converse of the bug being fixed. Bytes drained on the same turn
/// the user leaves are in the shell's screen but were never presented, and
/// the ledger says so. Squaring a departing tab wholesale — the obvious way
/// to kill a stale dot — would swallow exactly this.
#[test]
fn output_that_never_reached_the_glass_survives_the_switch() {
    let output = output_revision(0, true, false);
    let seen = seen_revision(0, output, false);
    assert_eq!((output, seen), (1, 0));
    assert_eq!(facts(output, seen, false).claim(), StatusClaim::Unread);
}

/// PIN (T2, user ruling — "watching is consuming"): a latch that arrives on
/// the tab the user is already reading is spent on arrival.
///
/// The bell is the case that matters, because it is the one claim the
/// work-in-flight rule never suppresses: without this it would sit on the
/// focused tab indefinitely, since nothing else in the taxonomy retires a
/// latch except switching away and back. A terminal you are looking at does
/// not need a dot repeating what it just showed you.
#[test]
fn a_latch_arriving_on_the_watched_tab_is_spent_on_arrival() {
    assert!(attention_is_consumed(true, true));
    assert_eq!(
        claim_after_a_look(latched(), true, true),
        StatusClaim::Silent,
        "the tab in front of the user wears no dot"
    );
    // Both latches go, not just the loud one — a failure read on screen is
    // as read as a bell heard on screen.
    let mut failed_only = quiet();
    failed_only.failure_exit_code = Some(1);
    assert_eq!(
        claim_after_a_look(failed_only, true, true),
        StatusClaim::Silent
    );
}

/// PIN (T2, user ruling): an unfocused window consumes nothing.
///
/// This is the half that makes the rule safe. The active tab is still the
/// active tab when the user alt-tabs away, so clearing on "active" alone
/// would eat every bell that rang while they were gone — which is precisely
/// the moment a bell is doing its job. Nobody is reading a background
/// window, so nothing in it is read.
#[test]
fn a_bell_that_rings_while_the_user_is_away_is_still_waiting() {
    assert!(!attention_is_consumed(true, false));
    // Both latches survive, but only the bell can *show* on the tab that is
    // on screen: a failure is a kind of unread, and unread is suppressed on
    // the active tab whatever the window is doing. So the bell is not
    // merely the loudest surviving claim here — it is the only one this
    // ruling can be about, which is why the ruling is about the bell.
    let mut bell_only = quiet();
    bell_only.bell = Some(bt_term::BellSource::Bel);
    for status in [latched(), bell_only] {
        assert_eq!(
            claim_after_a_look(status, true, false),
            StatusClaim::Bell,
            "an unfocused window keeps its bell"
        );
    }
    // And a bell on the *active* tab really can show a dot at all — the
    // active-tab suppression covers unread and failure, never the bell.
    assert_eq!(
        facts_with(bell_only, 50, 50, true).claim(),
        StatusClaim::Bell
    );
}

/// PIN (T2, user ruling): a tab nobody is looking at is unchanged — its
/// latches wait for the activation that answers them.
///
/// The whole point of the taxonomy is the tabs you *cannot* see, so the
/// rule above must not reach them under any combination of focus. Only
/// `TabState::mark_seen`, on activation, retires these.
#[test]
fn an_inactive_tabs_latches_wait_for_the_activation_that_answers_them() {
    for window_is_focused in [true, false] {
        assert!(
            !attention_is_consumed(false, window_is_focused),
            "focus alone consumes nothing on a tab that is not on screen"
        );
        assert_eq!(
            claim_after_a_look(latched(), false, window_is_focused),
            StatusClaim::Failed,
            "an unwatched tab keeps its claim whatever the window is doing"
        );
    }
    // The full truth table, so the rule cannot drift into a one-sided test:
    // it is an `and`, and each half is load-bearing on its own.
    assert!(attention_is_consumed(true, true));
    assert!(!attention_is_consumed(true, false));
    assert!(!attention_is_consumed(false, true));
    assert!(!attention_is_consumed(false, false));
}

/// PIN (T2 D34/C36): the same ladder, walked over a *fleet* — the tab's dot
/// is the loudest claim its member shells make, each of them having made its
/// own.
///
/// Red gate: `mark_state` used to build a one-element iterator out of the
/// focused leaf's facts, which is the placeholder this replaces. Against it
/// a tab holding a quiet focused pane and a screaming background one wore
/// nothing at all.
#[test]
fn a_tab_wears_the_loudest_claim_its_fleet_makes() {
    let mut rang = quiet();
    rang.bell = Some(bt_term::BellSource::Bel);
    let mut failed = quiet();
    failed.failure_exit_code = Some(1);

    // Quiet focused pane, background pane that rang: the tab rings.
    assert_eq!(
        fleet_claim([facts(4, 4, false), facts_with(rang, 9, 4, false)]),
        StatusClaim::Bell
    );
    // And the failure outranks the bell wherever in the fleet it sits.
    assert_eq!(
        fleet_claim([
            facts_with(rang, 9, 4, false),
            facts_with(failed, 9, 4, false)
        ]),
        StatusClaim::Failed
    );
    // A tab with no shells at all claims nothing rather than panicking. It
    // is not a state this build can reach — seats and sessions are created
    // and destroyed together — but a `max` over an empty iterator is
    // exactly where a fold would have unwrapped.
    assert_eq!(fleet_claim([]), StatusClaim::Silent);
}

/// PIN (§7.1.5b) — **a place in the attention queue is the ladder's top
/// rung, and the only rung that breathes.**
///
/// The taxonomy's severity order, said as `Ord` so `max` is the aggregation:
/// 等你回答 > 未读·失败 > bell > 未读·完成. And the half of it that F3 exists
/// to make true: `Bell` and `Awaiting` are **the same warn dot** and only the
/// second pulses, because the colour is the claim and the motion is the
/// queue (mock-up 345-346, two consecutive lines).
///
/// The last assertion is the one worth stating: a place outranks even the
/// suppressions. A shell that is *still working* and *has already been
/// looked at* has nothing to report on the output ledger — and it is still
/// standing there waiting for you, which no fact about output can answer.
///
/// Red gate: move `Awaiting` below `Failed` in the enum and the ladder
/// assertion goes red; let `pulses` match `Bell` too and the second does;
/// read `awaiting` after the `unread` cascade in `claim` and the last does.
#[test]
fn a_place_in_the_queue_outranks_every_claim_and_is_the_only_one_that_breathes() {
    let palette = bt_render::chrome_palette();
    assert!(StatusClaim::Awaiting > StatusClaim::Failed);
    assert!(StatusClaim::Failed > StatusClaim::Bell);
    assert!(StatusClaim::Bell > StatusClaim::Unread);
    assert!(StatusClaim::Unread > StatusClaim::Silent);
    assert_eq!(
        loudest_claim([StatusClaim::Failed, StatusClaim::Awaiting]),
        StatusClaim::Awaiting,
        "and `max` over that order is the tab's own aggregation"
    );

    let standing = StatusClaim::Awaiting.dot(&palette).expect("a warn dot");
    let rang = StatusClaim::Bell.dot(&palette).expect("a warn dot");
    assert_eq!(
        standing.ink, rang.ink,
        "one warn, two claims — the mock-up writes `--warn` for both"
    );
    // **And the ink is where the sameness stops** (`attention` plan red line 3, ruled
    // 2026-08-25). Until this line the two differed only in that one of them breathed, which
    // meant that with the system's animation setting off they arrived as the same pixels —
    // one dot, two assertions, which is the taxonomy's own rule broken from the inside. The
    // second axis is a *fill*, so it survives reduced motion, a repaint and a screenshot.
    assert_ne!(
        standing.hollow, rang.hollow,
        "two claims that share an ink must not also share a shape"
    );
    assert!(
        rang.hollow && !standing.hollow,
        "filled is a state that is standing, hollow is an event that rang — and the bell is \
             the one event in the ladder"
    );
    assert!(StatusClaim::Awaiting.pulses());
    assert!(
        !StatusClaim::Bell.pulses(),
        "§7.1.5b: bell 的橙点明确不脉动"
    );
    assert!(!StatusClaim::Failed.pulses() && !StatusClaim::Unread.pulses());

    let mut busy_and_read = quiet();
    busy_and_read.working = true;
    assert_eq!(
        facts_with(busy_and_read, 0, 0, true).claim(),
        StatusClaim::Silent,
        "the output ledger has nothing to say about a shell you are watching \
             work — which is what makes the next line a claim about the queue"
    );
    assert_eq!(
        waiting(busy_and_read, true).claim(),
        StatusClaim::Awaiting,
        "看一眼阻塞的 agent 不解除阻塞"
    );
}

/// RED GATE (same ruling) — **the theme row writes the settings file, and
/// the session file no longer carries a theme at all.**
///
/// The store is the whole of the defect: `apply_theme_mode` used to mark the
/// *session* dirty, which is how `settings.json`'s `theme_mode` came to be a
/// field the Settings page drew and no code on either side of it ever wrote
/// or read. Two files holding one fact is the shape the ruling forbids, so
/// this is asserted structurally rather than remembered — a future edit that
/// sends the choice back to the session file has to delete this test to do
/// it.
///
/// Mutation: put `mark_session_dirty` back in `apply_theme_mode`, or write a
/// `theme` key on `SessionV1` again.
#[test]
fn the_theme_row_writes_settings_and_the_session_file_names_no_theme() {
    let body = method_body("Runtime", "apply_theme_mode");
    assert!(
        body.contains("settings_store.store("),
        "the theme row's choice is written to `settings.json` and nowhere else:
{body}"
    );
    assert!(
        !body.contains("mark_session_dirty"),
        "the session file is not the theme's store any more:
{body}"
    );

    // And the file it used to be stored in says nothing about themes: a key
    // that is still written is a second answer waiting to be believed.
    let written = serde_json::to_value(SessionV1::default()).expect("a session serializes");
    assert!(
        written.get("theme").is_none(),
        "a session written today carries no theme key: {written}"
    );
}

/// Every DPI Windows can report, from 100% to 300%, including the quarter
/// steps the display settings offer and the awkward ones a fractional
/// scaling setting produces.
const WINDOWS_SCALES: [f64; 8] = [1.0, 1.25, 1.4, 1.5, 1.75, 2.0, 2.5, 3.0];

/// The bug this pins: the window grew by one native frame margin — 26x71
/// physical at 192 DPI — on every single start, because it was saved as an
/// outer rect and restored as a client size, and winit adds
/// `AdjustWindowRectExForDpi` to the second. Restart is a fixed point or the
/// window walks off the screen in a fortnight.
#[test]
fn a_window_nobody_touched_is_restored_and_re_saved_byte_for_byte() {
    // Somewhere the OS would have put a window that had no saved position.
    let elsewhere = bt_platform::WindowRect {
        left: 100,
        top: 100,
        right: 900,
        bottom: 700,
    };
    for scale in WINDOWS_SCALES {
        for bounds in [
            WindowBoundsV1 {
                x: 0,
                y: 0,
                width: 960,
                height: 600,
            },
            // The odd extents and the negative origin of a window parked on a
            // secondary monitor left of the primary one.
            WindowBoundsV1 {
                x: -1128,
                y: 66,
                width: 987,
                height: 583,
            },
            WindowBoundsV1 {
                x: 1,
                y: -3,
                width: 1,
                height: 1,
            },
        ] {
            let rect = startup_window_rect(Some(placement(bounds, false)), elsewhere, scale);
            assert_eq!(
                persisted_window_bounds(rect, scale),
                bounds,
                "scale {scale} lost {bounds:?} across one restart"
            );
            // And it stays a fixed point under repetition, which is the shape
            // the bug actually had: a margin added once is invisible, added
            // fifty times it walks the window off the screen.
            let mut generation = bounds;
            for restart in 0..50 {
                let rect =
                    startup_window_rect(Some(placement(generation, false)), elsewhere, scale);
                generation = persisted_window_bounds(rect, scale);
                assert_eq!(
                    generation, bounds,
                    "scale {scale} drifted from {bounds:?} by restart {restart}"
                );
            }
        }
    }
}

/// The restored rectangle is the saved one scaled, and nothing else. Stated
/// against the exact margin that used to be added, at the DPI it was measured
/// at: `AdjustWindowRectExForDpi(WS_OVERLAPPEDWINDOW, 192)` is 26x71.
#[test]
fn restoring_adds_no_native_frame_margin() {
    let saved = WindowBoundsV1 {
        x: 74,
        y: 74,
        width: 960,
        height: 600,
    };
    let rect = startup_window_rect(
        Some(placement(saved, false)),
        bt_platform::WindowRect {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        },
        2.0,
    );
    assert_eq!(rect.right - rect.left, 1920);
    assert_eq!(rect.bottom - rect.top, 1200);
    assert_eq!((rect.left, rect.top), (148, 148));
}

/// §3.1's fallback, in its own words: a rectangle no monitor can see forfeits
/// its position, and only its position. A size is never off-screen.
#[test]
fn a_window_whose_monitor_is_gone_keeps_its_size_where_the_os_opened_it() {
    let opened_at = bt_platform::WindowRect {
        left: 40,
        top: 60,
        right: 640,
        bottom: 460,
    };
    let orphaned = RestoredPlacement {
        size: LogicalSize::new(800.0, 500.0),
        position: None,
        maximized: false,
    };
    let rect = startup_window_rect(Some(orphaned), opened_at, 2.0);
    assert_eq!((rect.left, rect.top), (40, 60));
    assert_eq!(
        (rect.right - rect.left, rect.bottom - rect.top),
        (1600, 1000)
    );
}

/// The rectangle Windows actually reported for this app's window while it was
/// minimized, measured on the machine the bug was found on. Nothing about it
/// is a place a window was ever put: the origin is far off any monitor and
/// the size is the taskbar button's, not the window's.
const ICONIC_BOUNDS: WindowBoundsV1 = WindowBoundsV1 {
    x: -16000,
    y: -16000,
    width: 157,
    height: 25,
};

/// A rectangle a user really did leave a window at.
const CHOSEN_BOUNDS: WindowBoundsV1 = WindowBoundsV1 {
    x: 240,
    y: 130,
    width: 1100,
    height: 720,
};

/// The bug this pins, end to end: minimize, quit while minimized, start
/// again. The snapshot recorded the *icon's* rectangle, and the next start
/// could not seat three panes in a 157x25 window, so it exited with
/// "place a body rectangle for terminal seat SeatId(2) from its own solve"
/// instead of opening.
///
/// Stated over the two pure halves the real path is made of — what a
/// snapshot records, and what a start makes of what it read — so the whole
/// round trip is checked without a window.
#[test]
fn quitting_while_minimized_starts_again_at_the_rectangle_the_user_chose() {
    let (bounds, maximized) = recorded_window_placement(
        WindowPosture::Minimized,
        Some(ICONIC_BOUNDS),
        CHOSEN_BOUNDS,
        false,
    );
    assert_eq!(
        bounds, CHOSEN_BOUNDS,
        "a minimized window wrote the icon's rectangle to the session file"
    );
    assert!(!maximized);
    assert_eq!(
        sane_restored_size(bounds),
        LogicalSize::new(
            f64::from(CHOSEN_BOUNDS.width),
            f64::from(CHOSEN_BOUNDS.height)
        ),
        "the rectangle the user chose was not handed back intact"
    );
}

/// `IsZoomed` answers false for an iconic window even when it will restore
/// maximized, so a snapshot that trusted it would quietly demote a maximized
/// window every time it was minimized. The posture holds both facts.
#[test]
fn minimizing_a_maximized_window_does_not_forget_that_it_was_maximized() {
    let (bounds, maximized) = recorded_window_placement(
        WindowPosture::Minimized,
        Some(ICONIC_BOUNDS),
        CHOSEN_BOUNDS,
        true,
    );
    assert_eq!(bounds, CHOSEN_BOUNDS);
    assert!(maximized, "a minimize demoted a maximized window");
}

/// fb13766's rule, restated where the new one lives so that extending it to
/// a third posture cannot quietly drop it: a maximized window keeps the
/// normal rectangle it had, and is still recorded as maximized.
#[test]
fn a_maximized_window_still_keeps_the_rectangle_it_had_while_normal() {
    let monitor_sized = WindowBoundsV1 {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    let (bounds, maximized) = recorded_window_placement(
        WindowPosture::Maximized,
        Some(monitor_sized),
        CHOSEN_BOUNDS,
        false,
    );
    assert_eq!(bounds, CHOSEN_BOUNDS);
    assert!(maximized);
}

/// The posture that does record: a normal window's rectangle is the user's,
/// and it is written exactly. The fallback to what was saved covers only the
/// case where the rectangle could not be measured at all.
#[test]
fn a_normal_window_records_the_rectangle_it_was_measured_at() {
    let measured = WindowBoundsV1 {
        x: 12,
        y: 34,
        width: 800,
        height: 600,
    };
    assert_eq!(
        recorded_window_placement(WindowPosture::Normal, Some(measured), CHOSEN_BOUNDS, true),
        (measured, false)
    );
    assert_eq!(
        recorded_window_placement(WindowPosture::Normal, None, CHOSEN_BOUNDS, true),
        (CHOSEN_BOUNDS, false),
        "an unmeasurable window discarded the rectangle it already had"
    );
}

/// The second line, for a file an older build already damaged or a hand edit
/// produced: a size below one seat's own minimum is not a size a window was
/// left at, and startup takes the product's opening size instead of failing.
#[test]
fn a_saved_size_no_window_could_have_had_falls_back_to_the_opening_size() {
    let opening = LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT);
    let floor_width = MIN_PANE_W.floor_px() as u32;
    let floor_height = MIN_PANE_H.floor_px() as u32;
    for absurd in [
        ICONIC_BOUNDS,
        // Zero area, which no window has.
        WindowBoundsV1 {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
        // One pixel short on each axis in turn — the floor is a floor, not a
        // neighbourhood.
        WindowBoundsV1 {
            x: 0,
            y: 0,
            width: floor_width - 1,
            height: 900,
        },
        WindowBoundsV1 {
            x: 0,
            y: 0,
            width: 900,
            height: floor_height - 1,
        },
    ] {
        assert_eq!(
            sane_restored_size(absurd),
            opening,
            "{absurd:?} was honoured as a window size"
        );
    }
    // And everything a window could actually have been is left alone,
    // including a window sitting exactly on the floor.
    for legitimate in [
        CHOSEN_BOUNDS,
        WindowBoundsV1 {
            x: 0,
            y: 0,
            width: floor_width,
            height: floor_height,
        },
        WindowBoundsV1 {
            x: -1128,
            y: 66,
            width: 987,
            height: 583,
        },
    ] {
        assert_eq!(
            sane_restored_size(legitimate),
            LogicalSize::new(f64::from(legitimate.width), f64::from(legitimate.height)),
            "{legitimate:?} was overridden despite being a real window size"
        );
    }
}

/// A first run has no rectangle to honour, so the product's opening size is
/// the one that is stated exactly — as an outer rect, which under the
/// self-drawn frame is what the user sees.
#[test]
fn a_first_run_opens_at_the_products_own_size() {
    let opened_at = bt_platform::WindowRect {
        left: 11,
        top: 22,
        right: 33,
        bottom: 44,
    };
    let rect = startup_window_rect(None, opened_at, 2.0);
    assert_eq!((rect.left, rect.top), (11, 22));
    assert_eq!(
        (rect.right - rect.left, rect.bottom - rect.top),
        ((INITIAL_WIDTH * 2.0) as i32, (INITIAL_HEIGHT * 2.0) as i32)
    );
}

/// A saved window with something in it, which is the only kind
/// [`choose_restored_placement`] has an opinion about.
fn saved_window(bounds: WindowBoundsV1, maximized: bool) -> SessionWindowV1 {
    SessionWindowV1 {
        placement: WindowStateV1 {
            bounds,
            dpi: 2000,
            maximized,
            monitor_id: None,
        },
        tabs: vec![TabV1 {
            root: LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                profile_id: "pwsh".to_owned(),
                cwd: String::new(),
                manual_name: None,
                card_skip: 0,
                last_command: String::new(),
            })),
            pinned: false,
            focused_leaf: "leaf-0".to_owned(),
            preview: None,
        }],
        ..SessionWindowV1::default()
    }
}

fn monitor(left: f64, top: f64, width: f64, height: f64) -> RestoreMonitor {
    RestoreMonitor {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

/// The laptop panel this bug was measured on: 2880x1800 at 200%, so 1440x900 logical, with
/// nothing taken off it for a taskbar so the arithmetic below is the placement's and not a
/// work area's.
const LAPTOP: RestoreMonitor = RestoreMonitor {
    left: 0.0,
    top: 0.0,
    right: 1440.0,
    bottom: 900.0,
};

/// The 3840x2160 external, also at 200%, parked to the left of it — where this session was
/// saved.
const EXTERNAL: RestoreMonitor = RestoreMonitor {
    left: -1920.0,
    top: -120.0,
    right: 0.0,
    bottom: 960.0,
};

/// The rectangle out of the user's own `session.json` on 2026-09-01: a window last left
/// normal on the external monitor, and maximized when the process ended.
const SAVED_ON_THE_EXTERNAL: WindowBoundsV1 = WindowBoundsV1 {
    x: -1360,
    y: 429,
    width: 926,
    height: 1080,
};

/// A monitor that is still there is still where the window goes — corner and size both
/// exactly as recorded, with nothing fitted, nudged or clamped on the way.
///
/// Red gate: fit the size against the *primary* rather than against the monitor the rectangle
/// is on, and the height comes back 900 instead of 1080.
#[test]
fn a_window_comes_back_untouched_when_the_monitor_it_was_left_on_is_still_there() {
    let placement = choose_restored_placement(
        &saved_window(SAVED_ON_THE_EXTERNAL, true),
        &[LAPTOP, EXTERNAL],
    )
    .expect("a window with a tab in it has a placement");
    assert_eq!(
        placement.position,
        Some(LogicalPosition::new(-1360.0, 429.0)),
        "a rectangle on a monitor that exists was moved"
    );
    assert_eq!(
        placement.size,
        LogicalSize::new(926.0, 1080.0),
        "a rectangle that fits its own monitor was resized"
    );
    assert!(placement.maximized);
}

/// **PIN (user report, 2026-09-01) — a restored window does not hang off the bottom of the
/// only screen there is.**
///
/// The report was "窗口出现在主屏上", and the log line under it is `BT_DPI stage=create
/// rect=49,49,1901,2209` on a machine whose only display is 2880x1800: the corner was
/// forfeited correctly, because the external monitor the rectangle names was not attached —
/// and then the *size* was honoured anyway, putting 409 physical pixels of window below the
/// bottom of the screen. The old contract said a size is never off-screen. A size bigger than
/// the screen is nothing but off-screen.
///
/// Red gate: return `sane_restored_size(bounds)` unfitted — the height comes back 1080 on a
/// 900-tall display.
#[test]
fn a_window_saved_on_a_bigger_screen_does_not_open_off_the_bottom_of_a_smaller_one() {
    let placement =
        choose_restored_placement(&saved_window(SAVED_ON_THE_EXTERNAL, true), &[LAPTOP])
            .expect("a window with a tab in it has a placement");
    assert_eq!(
        placement.position, None,
        "a rectangle no monitor can see kept its corner"
    );
    assert_eq!(
        placement.size,
        LogicalSize::new(926.0, 900.0),
        "the window opened taller than the only display there was"
    );
}

/// Fitting is a ceiling and never a floor: a small window saved on a small screen stays small
/// when it is reopened on a large one.
///
/// Red gate: fit with `max` instead of `min`, or seat every restored window at the work area,
/// and a 600x400 window comes back 1920x1080.
#[test]
fn fitting_a_restored_window_never_makes_it_bigger() {
    let small = WindowBoundsV1 {
        x: 40,
        y: 60,
        width: 600,
        height: 400,
    };
    let placement = choose_restored_placement(
        &saved_window(small, false),
        &[monitor(0.0, 0.0, 1920.0, 1080.0)],
    )
    .expect("a window with a tab in it has a placement");
    assert_eq!(placement.size, LogicalSize::new(600.0, 400.0));
    assert_eq!(placement.position, Some(LogicalPosition::new(40.0, 60.0)));
}

/// A window the user deliberately parked half off the side of a monitor comes back parked
/// half off the side of that monitor. Fitting is about rectangles that no longer *fit*; a
/// rectangle that fits is none of its business.
///
/// Red gate: seat every restored window fully inside the target work area, and this corner
/// jumps from 1300 to 1040.
#[test]
fn a_window_parked_half_off_a_monitor_is_left_where_it_was_parked() {
    let parked = WindowBoundsV1 {
        x: 1300,
        y: 100,
        width: 400,
        height: 300,
    };
    let placement = choose_restored_placement(&saved_window(parked, false), &[LAPTOP])
        .expect("a window with a tab in it has a placement");
    assert_eq!(placement.size, LogicalSize::new(400.0, 300.0));
    assert_eq!(
        placement.position,
        Some(LogicalPosition::new(1300.0, 100.0)),
        "a rectangle that still fits was pulled back onto the screen"
    );
}

/// When the size *did* have to shrink, the corner goes with it: the rectangle is no longer
/// the one anybody chose, so the top-left it was chosen with is not owed either, and leaving
/// it there would put a fitted window's title bar off the screen it was just fitted to.
///
/// Red gate: keep `corner` when the size shrinks and the window opens at x = 1200 on a
/// 1440-wide display it now spans the whole of.
#[test]
fn a_window_that_had_to_shrink_is_seated_on_the_monitor_that_shrank_it() {
    let oversized = WindowBoundsV1 {
        x: 1200,
        y: 800,
        width: 1600,
        height: 1200,
    };
    let placement = choose_restored_placement(&saved_window(oversized, false), &[LAPTOP])
        .expect("a window with a tab in it has a placement");
    assert_eq!(placement.size, LogicalSize::new(1440.0, 900.0));
    assert_eq!(placement.position, Some(LogicalPosition::new(0.0, 0.0)));
}

/// RED (review row R4-7) — **a restored window's title bar is on a monitor,
/// whatever its size.**
///
/// `shrank` was doing two jobs at once: deciding whether the rectangle was
/// still the one the reader parked, and deciding whether the corner was
/// looked at at all. So a recorded size that fits its display kept its corner
/// unexamined — and a `y` of −900, which is what a stale `session.json` from
/// a display that has gone away or one hand-edited digit produces, came back
/// as a window whose title bar is above the desktop. There is nothing to grab
/// and nothing on screen to say why.
///
/// The horizontal allowance the test above it pins is untouched, and the two
/// together are the whole rule: a window parked half off the side can always
/// be dragged back, *because its title bar is on the screen*.
///
/// Red gate: keep `corner` unchanged in the non-shrinking arm and the first
/// assertion reads −900.
#[test]
fn a_restored_window_never_comes_back_with_its_title_bar_off_the_screen() {
    // Overlapping the display by a hundred rows — so it is a window this
    // machine can see, and its title bar is above the top of the desktop.
    let above = WindowBoundsV1 {
        x: 200,
        y: -200,
        width: 400,
        height: 300,
    };
    let placement = choose_restored_placement(&saved_window(above, false), &[LAPTOP])
        .expect("a window with a tab in it has a placement");
    assert_eq!(
        placement.size,
        LogicalSize::new(400.0, 300.0),
        "nothing had to shrink: the size fits the display perfectly well"
    );
    assert_eq!(
        placement.position,
        Some(LogicalPosition::new(200.0, 0.0)),
        "the corner comes down to the work area, and stays where it was across"
    );

    // And the other end: a top below the bottom of the work area is just as
    // unreachable, and is brought back by the width of a title bar.
    // Still overlapping the display — 20 rows of it — and still a window
    // whose title bar is off the bottom.
    let below = WindowBoundsV1 {
        x: 200,
        y: 880,
        width: 400,
        height: 300,
    };
    let placement = choose_restored_placement(&saved_window(below, false), &[LAPTOP])
        .expect("a window with a tab in it has a placement");
    let bar = f64::from(bt_render::WINDOW_TITLE_BAR_LOGICAL_PX);
    assert_eq!(
        placement.position,
        Some(LogicalPosition::new(200.0, 900.0 - bar)),
        "the last row a title bar can be caught on"
    );
}

/// A window straddling a seam belongs to the screen holding most of it, and that is the
/// screen whose size it is fitted to.
///
/// Red gate: take the first monitor that overlaps at all rather than the one that overlaps
/// most, and the height is fitted to the 400-tall strip instead of left alone.
#[test]
fn a_window_across_a_seam_is_fitted_to_the_monitor_holding_most_of_it() {
    let straddling = WindowBoundsV1 {
        x: -100,
        y: 0,
        width: 900,
        height: 700,
    };
    // A short screen on the left that catches 100 columns of the window, and the real one on
    // the right that catches the other 800.
    let sliver = monitor(-1000.0, 0.0, 1000.0, 400.0);
    let placement = choose_restored_placement(&saved_window(straddling, false), &[sliver, LAPTOP])
        .expect("a window with a tab in it has a placement");
    assert_eq!(
        placement.size,
        LogicalSize::new(900.0, 700.0),
        "the window was fitted to the monitor it is barely on"
    );
    assert_eq!(placement.position, Some(LogicalPosition::new(-100.0, 0.0)));
}

/// A machine that reports no monitors at all is a machine that would not say, not a machine
/// with no screens: nothing is fitted against nothing, and the corner is forfeited exactly as
/// it is for a rectangle no monitor can see.
#[test]
fn a_machine_that_reports_no_monitors_fits_nothing() {
    let placement = choose_restored_placement(&saved_window(SAVED_ON_THE_EXTERNAL, false), &[])
        .expect("a window with a tab in it has a placement");
    assert_eq!(placement.position, None);
    assert_eq!(placement.size, LogicalSize::new(926.0, 1080.0));
}

/// **RED (M3-4) — a monitor's work area is never another monitor's.**
///
/// The first two rows are Windows, and they are here to say that nothing
/// about a Windows restore moves: a taskbar strip off the bottom and a
/// display with no strip at all both come back exactly as the platform said
/// them. The third is the desk this ticket is written for — a 1× panel
/// standing to the right of a 4K at 2×, in the physical space `bt-platform`
/// and winit share, where the panel's own coordinates are **inside** the 4K's
/// — and the answer that comes back for a point in the middle of the panel is
/// the 4K's work area. Divided by the panel's scale of 1 that is a rectangle
/// 3840 logical pixels wide on a panel 1280 wide, and every window restored
/// there is fitted to it.
///
/// MUTATION: return `asked` whenever it is `Some` and the third row comes
/// back as the 4K's rectangle; drop the `full` arm and a display whose work
/// area the platform will not say loses its rectangle altogether.
#[test]
fn a_monitors_work_area_is_never_another_monitors() {
    let rect = |left, top, right, bottom| bt_platform::WindowRect {
        left,
        top,
        right,
        bottom,
    };
    let primary = rect(0, 0, 3840, 2160);
    let with_taskbar = rect(0, 0, 3840, 2064);
    assert_eq!(
        monitor_work_area(primary, Some(with_taskbar)),
        with_taskbar,
        "a strip the system reserves on this monitor is this monitor's"
    );
    assert_eq!(
        monitor_work_area(primary, Some(primary)),
        primary,
        "a display with nothing reserved on it keeps its whole rectangle"
    );
    let panel = rect(1920, 0, 3200, 720);
    assert_eq!(
        monitor_work_area(panel, Some(rect(0, 64, 3840, 2160))),
        panel,
        "the 4K's work area is not the panel's, however far inside the 4K the \
             panel's own coordinates fall"
    );
    assert_eq!(
        monitor_work_area(panel, None),
        panel,
        "a display the platform will not describe is still standing where it stands"
    );
    assert_eq!(
        monitor_work_area(panel, Some(rect(1920, 0, 3201, 720))),
        panel,
        "and an answer that hangs off one edge is an answer about something else"
    );
}

/// A window with nothing in it is not a window, whatever its rectangle says.
#[test]
fn a_saved_window_with_no_tabs_asks_for_nothing() {
    let empty = SessionWindowV1 {
        placement: WindowStateV1 {
            bounds: SAVED_ON_THE_EXTERNAL,
            dpi: 2000,
            maximized: false,
            monitor_id: None,
        },
        ..SessionWindowV1::default()
    };
    assert_eq!(choose_restored_placement(&empty, &[LAPTOP, EXTERNAL]), None);
}

/// PIN (ticket 14) — **a share is never asked about: not on the hover, not on the press, not when
/// the modifier goes down.**
///
/// The other half of the 2026-09-21 ruling — 「悬停不碰 UNC」 — and the reason `Ctrl` on a share
/// could be allowed at all: the hand-off lane takes the round trip, and nothing else may. The real
/// producer runs here: an `OSC 8` share link fed through a real session, its target read back off
/// the real frame, and every door that asks a pane's worker — the pointer move, the press's
/// re-check, the modifier and the still-pointer frame — goes through
/// [`link_target_to_ask_about`], which answers `None`, so the worker's queue stays empty. A local
/// link in the same frame is asked about, which is what shows the seam is live.
///
/// Green on the base too (a share was refused by the same gate there); it pins that handing a
/// share over did not open a question about one.
///
/// MUTATION: let `link_target_to_ask_about` answer `Some(path)` without the `may_read_unasked`
/// gate, and the queue holds the share.
#[test]
fn a_share_is_never_asked_about() {
    let mut session = bt_term::DualPlaneSession::new(
        std::num::NonZeroU32::new(40).unwrap(),
        std::num::NonZeroU32::new(2).unwrap(),
    );
    session
        .feed(b"\x1b]8;;file://server/share/a.md\x1b\\share\x1b]8;;\x1b\\ \x1b]8;;file:///C:/work/notes.md\x1b\\local\x1b]8;;\x1b\\")
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let share = frame
        .hyperlink_at(0, 1)
        .expect("the share link is on the frame");
    let local = frame.hyperlink_at(0, 7).expect("and the local one");
    assert_eq!(share.uri, "file://server/share/a.md");

    let windows = bt_transcript::paths::PrintedPathNamespace::Windows;
    let namer = bt_transcript::paths::PathNamer::Pane(&windows);
    // Hover, press, modifier-down and the still-pointer frame: four gestures, one gate.
    for door in ["hover", "press", "modifier", "frame"] {
        if let Some(path) = link_target_to_ask_about(&share.uri, namer) {
            panic!(
                "the {door} door would ask a worker about {}",
                path.display()
            );
        }
    }
    assert!(
        session.take_decoration_worker_task().is_none(),
        "nothing about the share is queued"
    );
    // Both halves of the table answer without reading the ledger.
    let asked = std::cell::Cell::new(0);
    for control in [false, true] {
        let _ = hyperlink_activation(control, true, &share.uri, namer, &|_| {
            asked.set(asked.get() + 1);
            None
        });
    }
    assert_eq!(
        asked.get(),
        0,
        "the table never reads a verdict for a share"
    );

    // The contrast: a local name goes through the same gate and is asked about.
    let local_path = link_target_to_ask_about(&local.uri, namer)
        .expect("a local link is one this window asks a worker about");
    session.ask_about_link_target(local_path.clone());
    let mut queued = Vec::new();
    while let Some(task) = session.take_decoration_worker_task() {
        if let bt_term::SessionDecorationTask::VerifyPath(path) = task {
            queued.push(path);
        }
    }
    assert_eq!(queued, vec![local_path]);
}

/// The caret rectangle leaves the frame in the terminal seat's coordinates
/// and reaches winit and IMM32 in the window's. This is the one place the
/// seat correction runs in that direction, so it is pinned in both: a lone
/// leaf's origin is `(0, 0)` and the number is unchanged, and a seat that
/// has been moved carries the caret with it.
///
/// Red gate: return `area` unchanged and the second case fails — the
/// candidate window would open a seat's width away from the caret.
#[test]
fn the_ime_caret_leaves_the_seat_in_the_windows_coordinates() {
    let area = ImeCursorArea {
        x: 250,
        y: 100,
        width: 18,
        height: 44,
    };
    assert_eq!(
        window_ime_cursor_area(SeatViewport::whole(1920, 1200), area),
        area,
        "a lone leaf's seat is the window, so nothing moves"
    );
    let moved = window_ime_cursor_area(
        SeatViewport {
            x: 976,
            y: 0,
            width: 944,
            height: 1200,
        },
        area,
    );
    assert_eq!(
        moved,
        ImeCursorArea {
            x: 250 + 976,
            y: 100,
            width: 18,
            height: 44,
        },
        "a seat with an origin carries the caret to the window's axis"
    );
}

/// The caret a pane hands the input method, from the one cell it stands on.
///
/// Window pixels all the way: the renderer measures the cell off its own
/// padding and metrics in the **seat's** axis, [`window_ime_cursor_area`]
/// carries it to the window's, and [`ime_cursor_area_of`] rounds the line
/// box to the whole-pixel origin-and-size pair the platform takes. Nothing
/// on this path knows which display the window is on, and nothing on it
/// should — that is the contract, and §13.16 ⑥ is about what the platform
/// does with the answer afterwards.
///
/// Red gate: measure the caret's own hairline instead of the line box and
/// the height stops being the row's.
#[test]
fn a_caret_in_a_pane_reaches_winit_as_window_pixels_from_the_cell_it_stands_on() {
    // Row 29, column 7 of a seat whose cells are 9x22 device pixels behind
    // 8 pixels of padding — the grid's own arithmetic, spelled out so the
    // expectation is not the code under test written twice.
    let left = 8.0 + 7.0 * 9.0;
    let top = 8.0 + 29.0 * 22.0;
    let line = [left, top, left + 9.0, top + 22.0];
    let seat = SeatViewport {
        x: 976,
        y: 40,
        width: 944,
        height: 1160,
    };
    assert_eq!(
        window_ime_cursor_area(seat, ime_cursor_area_of(line)),
        ImeCursorArea {
            x: 976 + 71,
            y: 40 + 646,
            width: 9,
            height: 22,
        },
        "the window's axis, and the size is the row rather than the caret",
    );
}

// ── where the window is, asked once a turn (0.4.5 ticket 48) ──────────────
//
// `Runtime` cannot be built without a window, and the door itself asks the
// desktop about a real window, so these read the roads through `bt_source`:
// who asks the door, who writes the reading, and in what order a turn and a
// between-turn delivery do it.

/// The body of `AppEvent::AttentionSpoke`'s arm in `FolioApp::user_event`.
fn attention_spoke_arm() -> &'static str {
    let events =
        item_body(&ItemQuery::method("FolioApp", "user_event").of_trait("ApplicationHandler"));
    let start = events
        .find("AppEvent::AttentionSpoke => {")
        .expect("user_event answers AttentionSpoke");
    let rest = &events[start..];
    let end = rest[1..].find("AppEvent::").map_or(rest.len(), |at| at + 1);
    &rest[..end]
}

/// RED (48) — **One turn asks the desktop where the window is exactly once,
/// however many passes read the answer.**
///
/// The owner's stall reports (next89/next90) caught `sample_window_place` —
/// four to eight system calls, two of them to other processes — holding the
/// window thread for up to 3 s, and in six turns it was asked twice: once by
/// the drain and once by the strip tick, about the same instant. The door now
/// has one caller, `Runtime::observe_window_place`, which the turn calls once
/// at its head, before the drain and the strip tick that read its answer; the
/// strip tick names neither the door nor the writer.
///
/// MUTATION: restore the `sample_window_place` call in `drain_pty` (or in
/// `advance_strip_animation`) — red.
#[test]
fn one_turn_asks_the_desktop_where_the_window_is_exactly_once() {
    let asks = free_calls_of("sample_window_place").in_the_product(source());
    assert_eq!(
        reader_names(&asks),
        vec!["observe_window_place".to_owned()],
        "{}",
        asks.report(source())
    );
    assert_eq!(asks.len(), 1, "{}", asks.report(source()));
    let turn = method_body("Runtime", "turn");
    assert_eq!(
        turn.matches("self.observe_window_place()").count(),
        1,
        "the turn takes one reading"
    );
    let observed = turn.find("self.observe_window_place()").unwrap();
    let drained = turn.find("self.drain_pty()").expect("the turn drains");
    let ticked = turn
        .find("self.advance_strip_animation(now)")
        .expect("the turn ticks the strip");
    assert!(
        observed < drained && observed < ticked,
        "the reading is taken before either pass reads it"
    );
    let tick = method_body("Runtime", "advance_strip_animation");
    for asking in ["sample_window_place", "observe_window_place", "has_focus()"] {
        assert!(
            !tick.contains(asking),
            "the strip tick does not call {asking}"
        );
    }
    assert!(tick.contains("self.window.observed_place"));
}

/// RED (48) — **A delivery that arrives between turns is decided on a fresh
/// reading of its own: one reading per turn, plus one per between-turn
/// delivery.**
///
/// The brief's first design parked `AttentionSpoke` messages for the next
/// turn. On Windows there may be no next turn for seconds: inside the OS's
/// modal move/size loop winit sends no `AboutToWait` (the record is
/// `hang_watch`'s `a_thread_that_answers_is_alive_even_when_its_loop_has_stopped_turning`),
/// so a notification that arrived during a drag would wait for the hand to let
/// go. Coordinator ruling 2026-09-24: the arm keeps a reading of its own, taken
/// through the one writer, and decides the delivery on it. So the writer has
/// these callers — the turn's head, the window's birth and this arm, and since
/// ticket 62 the re-placing of a taskbar flash the lane's answer contradicts —
/// and in the arm the reading comes before the delivery that reads it.
///
/// MUTATION: drop `runtime.observe_window_place()` from the `AttentionSpoke`
/// arm (the delivery would decide on the last turn's reading) — red.
#[test]
fn a_delivery_between_turns_is_decided_on_a_fresh_reading_of_its_own() {
    let writers = calls_of("Runtime", "observe_window_place").in_the_product(source());
    assert_eq!(
        reader_names(&writers),
        vec![
            "dress_new_window".to_owned(),
            "replace_contradicted_flash".to_owned(),
            "turn".to_owned(),
            "user_event".to_owned()
        ],
        "{}",
        writers.report(source())
    );
    // Four since ticket 62: re-placing a flash the taskbar lane's answer contradicts decides
    // again on a reading of its own, through the same writer.
    assert_eq!(writers.len(), 4, "{}", writers.report(source()));
    let arm = attention_spoke_arm();
    let observed = arm
        .find("runtime.observe_window_place();")
        .expect("the arm takes a reading of its own");
    let read = arm
        .find("let place = runtime.window.observed_place;")
        .expect("and decides on that reading");
    let delivered = arm.find("deliver_attention(").expect("the arm delivers");
    assert!(observed < read && read < delivered);
    assert!(
        !arm.contains("sample_window_place"),
        "through the one writer"
    );
}

/// RED (49) — **A new window carries its title before its first frame.**
///
/// The throttle holds a title only against an earlier write, and a window being
/// dressed has had none, so the one thing that could leave it untitled on the
/// taskbar is a birth that only *says* what it wants and waits for a turn.
/// `dress_new_window` wants and writes in the same call, before it returns to
/// the door that shows the window. (The slot half — a first offer is written at
/// once — is the last assertion.)
///
/// MUTATION: make `dress_new_window` only offer (drop its `flush_title` call) —
/// red.
#[test]
fn a_new_window_carries_its_title_before_its_first_frame() {
    let dress = method_body("Runtime", "dress_new_window");
    let wanted = dress
        .find("self.want_title();")
        .expect("a window being dressed says which title it wants");
    let written = dress
        .find("self.flush_title(")
        .expect("and writes it in the same call, with no turn run");
    assert!(wanted < written, "wanted first, then written");
    let mut slot = TitleSlot::default();
    slot.want("Folio".to_owned());
    assert_eq!(
        slot.take_due(TITLE_FRAME, Instant::now()),
        Some("Folio".to_owned()),
        "a slot that has written nothing writes its first title at once"
    );
}

/// **Q183's one-sided delay**: on the way open the words wait 60ms for the
/// panel to be wide enough to hold them; on the way shut they leave at once.
///
/// The mock-up puts `transition-delay: .06s` on the `.rail-open` rule alone
/// (line 903), so it applies going *into* that state and not out of it —
/// which is the right way round, because a word that lingered while the
/// panel narrowed past it would be clipped mid-letter on its way out.
///
/// Red gate: hang the delay on both directions and the closing half of this
/// fails at 30ms, with the words still at full strength inside a panel that
/// has already started to close.
#[test]
fn the_rails_labels_wait_for_the_panel_on_the_way_out_and_leave_first_on_the_way_back() {
    let start = Instant::now();
    let mut text = RevealTween::over(RAIL_TEXT_FADE);

    // ── opening: nothing for 60ms, then a 100ms fade ──
    text.retarget_after(1.0, start, Motion::Full, RAIL_TEXT_FADE_OPEN_DELAY);
    for waiting in [0, 30, 59] {
        let at = start + Duration::from_millis(waiting);
        let (opacity, moving) = text.sample(at, Motion::Full);
        assert_eq!(
            opacity, 0.0,
            "{waiting}ms in: the panel is still widening and the words have not started"
        );
        // **And the delay is a wait, not a journey** (closure review,
        // 2026-09-18). This read `moving` before that review, on the
        // reasoning that a transition which has been aimed somewhere is
        // running; it is not, it is standing at its own start, and a window
        // that carried it rebuilt its interface for sixty milliseconds on
        // every frame anybody else composed. The loop is still woken for the
        // instant the fade begins, which is the other question.
        assert!(!moving, "{waiting}ms in: nothing is moving yet");
        assert!(
            text.owes_a_wake(at, Motion::Full),
            "{waiting}ms in: but the loop must be woken for the fade's first frame"
        );
    }
    let (halfway, moving) = text.sample(start + Duration::from_millis(110), Motion::Full);
    assert!(
        halfway > 0.0 && halfway < 1.0,
        "50ms into its own 100ms the fade is halfway up: {halfway}"
    );
    assert!(moving);
    assert_eq!(
        text.sample(start + Duration::from_millis(160), Motion::Full),
        (1.0, false),
        "60 + 100: arrived, and asking for no more frames"
    );

    // ── closing: no delay at all ──
    let shut = start + Duration::from_millis(200);
    text.retarget(0.0, shut, Motion::Full);
    let (leaving, moving) = text.sample(shut + Duration::from_millis(30), Motion::Full);
    assert!(
        leaving < 1.0,
        "30ms after the pointer left, the words are already going: {leaving}"
    );
    assert!(moving);
    assert_eq!(
        text.sample(shut + RAIL_TEXT_FADE, Motion::Full),
        (0.0, false),
        "and they are gone in their own 100ms, with no delay spent"
    );
}

/// The other half of the same rule, and the reason it is stated as a posture rather than as a
/// size: 314x50 is a rectangle a window can genuinely have, and a window that has it gets its
/// shell told about it like any other.
///
/// Red gate: refuse small rectangles instead of iconic ones — by pixel count, by the
/// `-32000` corner, by anything that is not `IsIconic` — and the request list comes back
/// empty.
#[test]
fn a_genuinely_tiny_window_is_still_a_window() {
    let start = Instant::now();
    let mut harness = ResizeGateHarness::new(50, 50);
    harness.window_resized(PhysicalSize::new(314, 50), false, start);
    harness.tick(start + WINDOW_RESIZE_QUIET);
    assert_eq!(
        harness.requests,
        vec![grid_of(5, 50)],
        "a window the user really did make this small was not passed on"
    );
}

/// PIN — M175. The restore prompt names a files-only tab by the folder it
/// was looking at, not by nothing at all.
///
/// The red gate: the fallback built `Seed::Files { root: String::new() }`
/// literally, one function away from the walker that could have answered.
/// The row still drew — this is why no test caught it — it just drew a place
/// with no name, so the prompt offered to bring back a tab it could not
/// describe.
#[test]
fn a_files_only_tab_is_named_on_the_restore_prompt_by_the_folder_it_held() {
    // The naming rule is about the tab, and since multiwindow slice D the
    // function takes one — a preview pane's file is content and does not
    // live in the tree, so a walker handed only the tree could not see it.
    let tab = |root: LayoutNodeV1| TabV1 {
        root,
        pinned: false,
        focused_leaf: "leaf-0".to_owned(),
        preview: None,
    };
    let files = |root: &str| {
        LayoutNodeV1::Leaf(LeafNodeV1::Files(bt_persist::FilesLeafV1 {
            view: bt_persist::FilesViewV1::Files,
            root: root.to_owned(),
            open: Vec::new(),
            sel: None,
            width: 240,
            remotes_open: false,
        }))
    };
    assert_eq!(
        restore_row_seed(&tab(files(r"C:\Users\dev\project"))),
        seed::Seed::Files {
            root: r"C:\Users\dev\project".to_owned()
        }
    );

    // A tab with a shell in it is still named by the shell, whichever side
    // of the split the folder is on.
    let mixed = LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
        dir: bt_persist::SplitDirV1::Row,
        ratio: 250_000,
        children: [
            Box::new(files(r"C:\repo")),
            Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                profile_id: "pwsh".to_owned(),
                cwd: r"C:\repo\crates".to_owned(),
                manual_name: None,
                card_skip: 0,
                last_command: String::new(),
            }))),
        ],
    });
    assert_eq!(
        restore_row_seed(&tab(mixed)),
        seed::Seed::Term {
            profile_id: "pwsh".to_owned(),
            cwd: r"C:\repo\crates".to_owned(),
            manual_name: None,
        },
        "a tab's identity is its terminal wherever the column sits"
    );

    // And two columns are named by the first one drawn, which is the same
    // rule `first_term_leaf` follows for shells.
    let two = LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
        dir: bt_persist::SplitDirV1::Row,
        ratio: 500_000,
        children: [Box::new(files(r"C:\first")), Box::new(files(r"C:\second"))],
    });
    assert_eq!(
        restore_row_seed(&tab(two)),
        seed::Seed::Files {
            root: r"C:\first".to_owned()
        }
    );

    // PIN (multiwindow slice D) — **and a tab that is one preview pane is
    // named by the file it was on**, which is the fourth door onto §7.1.6h's
    // third shape. Before this, a saved file tab drew a row about a nameless
    // place: the walker was handed the *tree*, and which file a pane was
    // showing is content, so the answer it could give was `Files { root: ""
    // }` — the very row M175 was written to abolish, one leaf kind over.
    assert_eq!(
        restore_row_seed(&TabV1 {
            root: LayoutNodeV1::Leaf(LeafNodeV1::Preview(bt_persist::PreviewLeafV1 {
                pinned: false
            })),
            pinned: false,
            focused_leaf: "leaf-0".to_owned(),
            preview: Some(bt_persist::TabPreviewV1 {
                panes: vec![bt_persist::PreviewPaneV1 {
                    leaf: "leaf-0".to_owned(),
                    cur: Some(r"C:\repo\README.md".to_owned()),
                    cur_source: bt_persist::PreviewSourceV1::File,
                    graph: None,
                }],
                pool: Vec::new(),
            }),
        }),
        seed::Seed::Preview {
            path: r"C:\repo\README.md".to_owned(),
            source: bt_persist::PreviewSourceV1::File,
        }
    );
}

/// RED — **off Windows a breadcrumb starts at `~` or at a name, and never
/// at `/`** (owner report and ruling 2026-09-12, §13.32 ③).
///
/// What the owner saw on the built Mac was `/ › Users › alice ›
/// .zcompdump`: four crumbs, the first of them a folder called `/` that no
/// Mac shows anybody, and two more that every path on that machine repeats.
/// The ruling is Finder's path bar with the volume dropped, and it is two
/// shapes rather than one, so both are here.
///
/// **The platform and the home directory are arguments**, which is what
/// makes this runnable at all: it asserts what a Mac draws and it is being
/// run on a Windows workstation. Unix paths are used throughout because
/// `Path::components` reads `/` as a separator on either host, so the walk
/// under test is the walk that machine performs.
///
/// MUTATIONS:
/// ① return early for every platform — the first case keeps its `/` crumb
///    and goes red;
/// ② insert `~` without draining the run it stands for — `~ › Users ›
///    alice › .zcompdump`, and the first case goes red on its names;
/// ③ point `~` at the path instead of at the home directory — the click
///    target assertion goes red, and a press on `~` would stand the files
///    column on the file the reader is already reading;
/// ④ drop the rooted guard — the empty-`HOME` case grows a `~` in front of
///    a path that is not under any home at all.
#[test]
fn a_mac_breadcrumb_starts_at_the_home_crumb_or_at_a_name() {
    use bt_platform::HostPlatform::{MacOs, Windows};

    let home = Path::new("/Users/alice");
    let names = |segments: &[(String, PathBuf)]| -> Vec<String> {
        segments.iter().map(|(name, _)| name.clone()).collect()
    };

    // ① Under home: one `~`, then what is left of the path.
    let under = crumb_segments_on(Path::new("/Users/alice/.zcompdump"), MacOs, Some(home));
    assert_eq!(
        names(&under),
        vec![seats::PREVIEW_CRUMB_HOME, ".zcompdump"],
        "a path under the reader's home reads from `~`"
    );
    assert_eq!(
        under[0].1, home,
        "`~` points at the home directory, so a press on it goes there"
    );
    let deeper = crumb_segments_on(
        Path::new("/Users/alice/folio-port/repo/README.md"),
        MacOs,
        Some(home),
    );
    assert_eq!(
        names(&deeper),
        vec![seats::PREVIEW_CRUMB_HOME, "folio-port", "repo", "README.md"]
    );
    assert_eq!(
        deeper[1].1,
        Path::new("/Users/alice/folio-port"),
        "the crumbs after `~` still name the places they lead"
    );
    assert_eq!(
        names(&crumb_segments_on(home, MacOs, Some(home))),
        vec![seats::PREVIEW_CRUMB_HOME],
        "the home directory itself is the one crumb `~`"
    );

    // ② Outside home: the first component, with no root crumb in front.
    assert_eq!(
        names(&crumb_segments_on(
            Path::new("/Applications/Utilities/Terminal.app"),
            MacOs,
            Some(home),
        )),
        vec!["Applications", "Utilities", "Terminal.app"],
        "a path outside home starts at its own first component"
    );
    // A machine that never said where home is reads the same way.
    assert_eq!(
        names(&crumb_segments_on(Path::new("/etc/hosts"), MacOs, None)),
        vec!["etc", "hosts"]
    );
    // And an empty `HOME` is not a prefix of everything.
    assert_eq!(
        names(&crumb_segments_on(
            Path::new("/etc/hosts"),
            MacOs,
            Some(Path::new("")),
        )),
        vec!["etc", "hosts"]
    );
    // A relative path has no root to drop and no home to be under.
    assert_eq!(
        names(&crumb_segments_on(
            Path::new("../sibling/notes.md"),
            MacOs,
            Some(home),
        )),
        vec!["..", "sibling", "notes.md"]
    );

    // ③ Windows is the row it was, home or no home.
    assert_eq!(
        names(&crumb_segments_on(
            Path::new("/Users/alice/.zcompdump"),
            Windows,
            Some(home),
        )),
        vec![
            std::path::MAIN_SEPARATOR_STR.to_owned(),
            "Users".to_owned(),
            "alice".to_owned(),
            ".zcompdump".to_owned(),
        ],
        "the Windows shape keeps its root crumb and knows no `~`"
    );
}

/// RED GATE (§13.40) — **a foot spells a root the way the rail above it
/// spells one.** The reading sweep opened a folder under the owner's home
/// on the Mac and read two spellings of one place in one window: the
/// breadcrumbs said `~ › folio-port › repo` and the files column's foot,
/// two hundred physical pixels below them, said
/// `/Users/<owner>/folio-port/repo`. The rule is not copied here — the foot
/// asks [`home_crumb_for`], which is the rail's own question.
///
/// The separator is never invented: it is the byte the filesystem put
/// between the home run and what stands below it, which is why a Windows
/// workstation can assert what a Mac's foot says.
///
/// MUTATIONS:
/// ① give the Windows arm the substitution too — the last case grows a `~`
///    in front of a drive path and goes red;
/// ② join with `MAIN_SEPARATOR_STR` instead of slicing the original — every
///    Mac case reads `~\folio-port` when this test is run on Windows;
/// ③ drop the `below == 0` guard — the home directory itself reads `~e`,
///    the tail of its own last component;
/// ④ drop the rooted guard — the empty-`HOME` case swallows the whole path.
#[test]
fn a_files_foot_prints_the_root_its_breadcrumbs_would_print() {
    use bt_platform::HostPlatform::{MacOs, Windows};

    let home = Path::new("/Users/alice");
    let foot =
        |path: &str, platform, home| home_shortened_path_on(path, platform, home).into_owned();
    // The rail and the foot, side by side, on the run that found this.
    let deep = "/Users/alice/folio-port/repo";
    assert_eq!(
        crumb_segments_on(Path::new(deep), MacOs, Some(home))
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("/"),
        foot(deep, MacOs, Some(home)),
        "the two surfaces of one window say one thing"
    );
    assert_eq!(foot(deep, MacOs, Some(home)), "~/folio-port/repo");
    assert_eq!(
        foot("/Users/alice", MacOs, Some(home)),
        seats::PREVIEW_CRUMB_HOME,
        "the home directory itself is `~` and nothing after it"
    );
    assert_eq!(
        foot("/Users/alice/中文 folder", MacOs, Some(home)),
        "~/中文 folder",
        "the cut falls on the separator, not inside a character"
    );

    // Outside home, and a machine that never said where home is: the path
    // the filesystem handed over, unaltered.
    assert_eq!(
        foot("/Applications/Utilities", MacOs, Some(home)),
        "/Applications/Utilities"
    );
    assert_eq!(foot("/etc/hosts", MacOs, None), "/etc/hosts");
    assert_eq!(
        foot("/etc/hosts", MacOs, Some(Path::new(""))),
        "/etc/hosts",
        "an empty HOME is not a prefix of everything"
    );
    assert_eq!(
        foot("/Users/alice/notes.md", MacOs, Some(Path::new("/Users/a"))),
        "/Users/alice/notes.md",
        "a home is a run of whole components, not a string prefix — `/Users/a` \
             is a prefix of this path's characters and of none of its folders"
    );

    // Windows is the string it was, home or no home.
    assert_eq!(
        foot(r"D:\work\repo", Windows, Some(Path::new(r"C:\Users\alice"))),
        r"D:\work\repo"
    );
    assert_eq!(
        foot(
            r"C:\Users\alice\notes",
            Windows,
            Some(Path::new(r"C:\Users\alice"))
        ),
        r"C:\Users\alice\notes",
        "Explorer does not say `~`, so neither does this foot"
    );
    assert!(
        matches!(
            home_shortened_path_on(
                r"C:\Users\alice",
                Windows,
                Some(Path::new(r"C:\Users\alice"))
            ),
            std::borrow::Cow::Borrowed(_)
        ),
        "the Windows arm allocates nothing: the string that went in comes out"
    );
}

/// **A3/A6 — one safe accessor, and the two tables stay the same shape as
/// the tree.**
///
/// The lesson `UI-UX.md` §10 principle 20 draws out of the mock-up's three
/// families of bomb is not "add a null check"; it is "give the tagged union
/// *one* helper that safely takes an identity off any leaf, and make every
/// path go through it". These are that helper's pins.
///
/// The keyboard assertion is the sharpest of them. `sessions` is documented
/// as never empty and as always containing `focused_leaf`, and every
/// `self.session` in this file rides that invariant through two `Deref`s. A
/// files column that took the keyboard would break it silently — the panic
/// would land in whatever unrelated path next dereferenced the tab.
#[test]
fn a_files_column_has_a_home_beside_the_shells_and_never_takes_the_keyboard() {
    let tab = tab_with_a_files_column(1, r"C:\Users\dev\project");
    let [column] = tab.seats.files()[..] else {
        panic!("the tab holds one files column");
    };

    assert!(
        tab.files_match_files_seats(),
        "A3: the files table and the Files leaves are the same set"
    );
    assert!(
        tab.sessions_match_terminals(),
        "and the sessions table is still the terminals'"
    );
    assert!(
        !tab.sessions.contains_key(&column),
        "a files column has no shell, and must not be filed as though it had"
    );
    assert!(
        !tab.files.contains_key(&tab.focused_leaf),
        "I106: the keyboard is on a terminal, never on the column"
    );
    assert!(
        tab.sessions.contains_key(&tab.focused_leaf),
        "which is the invariant every `Deref` in this file rides on"
    );

    // The safe accessor answers for both kinds without the caller having to
    // know which it is holding.
    assert_eq!(tab.files_state(column).root, r"C:\Users\dev\project");
    assert_eq!(
        tab.files_head_name(column).as_deref(),
        Some("project"),
        "B14: the head takes the last segment"
    );
    assert_eq!(
        tab.files_head_name(tab.focused_leaf),
        None,
        "and a terminal seat has no root to give it"
    );
    assert_eq!(
        tab.files_state(tab.focused_leaf),
        seats::FilesLeafState::default(),
        "an absent entry is an unrooted column, not a panic"
    );
    assert_eq!(
        tab.files_names().len(),
        1,
        "one name per rooted column, and none for the shells"
    );
}

/// **Item 6 — a tab's shells and its Terminal leaves are the same set.**
///
/// Both directions fail, and they fail differently: a key with no leaf is a
/// shell still draining its pipe into a screen nothing draws, and a leaf with
/// no key is I106's black rectangle. Order is not part of the question —
/// `sessions.keys()` is ascending because it is a `BTreeMap`, while
/// `Seats::terminals` walks the tree in-order and a merge can seat an
/// arriving pane to the left of one already there.
///
/// Red gate: compare the two slices directly and the merge pins below go red
/// on a tree shape rather than on the invariant; compare only the lengths and
/// a session migrated under the wrong key passes.
#[test]
fn a_tabs_shells_and_its_terminal_leaves_are_one_set() {
    let id = |n| bt_layout::SeatId(n);
    assert!(sessions_match_terminals(&[], &[]));
    assert!(sessions_match_terminals(&[id(1), id(2)], &[id(1), id(2)]));
    assert!(
        sessions_match_terminals(&[id(1), id(4)], &[id(4), id(1)]),
        "the same set in a different order is the same set"
    );
    assert!(
        !sessions_match_terminals(&[id(1), id(2)], &[id(1)]),
        "a shell with no leaf is a process nobody can see or reach"
    );
    assert!(
        !sessions_match_terminals(&[id(1)], &[id(1), id(2)]),
        "a leaf with no shell is I106's black rectangle"
    );
    assert!(
        !sessions_match_terminals(&[id(1), id(2)], &[id(1), id(3)]),
        "same count, different seats"
    );
}

/// **The reported bug, over a real multi-pane tab: what the present painted
/// is square with what its shells said — every pane of it.**
///
/// The window's own present writes each pane's frame into the leaf that drew
/// it and squares that leaf's ledger in the same breath; this walks the same
/// leaves through the same rule. The first half is the requirement — a tab
/// whose every pane reached the glass owes nothing when it is left. The
/// second half is the one that failed on the user's machine: paint only the
/// pane holding the keyboard and the tab still lights up, for a sibling the
/// user had been reading the whole time.
///
/// Red gate: drop `mark_leaf_painted` from `Runtime::redraw`'s unfocused loop
/// and the second assertion becomes the first.
#[test]
fn every_pane_the_present_painted_is_square_with_its_shell() {
    let claim =
        |tab: &TabState| fleet_claim(tab.sessions.values().map(|leaf| leaf.session_facts(false)));
    let mut tab = cross_tab(1, &["LEFT", "RIGHT"]);
    for (_, leaf) in tab.leaves_mut() {
        leaf.output_revision = output_revision(leaf.output_revision, true, false);
    }
    assert_eq!(
        claim(&tab),
        StatusClaim::Unread,
        "both shells have spoken and nothing has been painted"
    );

    // Only the pane holding the keyboard is painted — the old shape of the
    // rule, and the sibling is still owed.
    let focused = tab.focused_leaf;
    mark_leaf_painted(tab.shell_mut());
    assert_eq!(
        claim(&tab),
        StatusClaim::Unread,
        "a pane on screen but not holding the keyboard is still being read"
    );

    // The present paints all of them, which is what a present does.
    for (seat, leaf) in tab.leaves_mut() {
        if *seat != focused {
            mark_leaf_painted(leaf);
        }
    }
    assert_eq!(
        claim(&tab),
        StatusClaim::Silent,
        "a tab every pane of which reached the glass owes nothing when you leave it"
    );
}
