//! **The crate root: terminal and PTY.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    CARDS_AT_150, CARDS_AT_200, RAIL_FAILED_THEN_PROMPT, ResizeGateHarness, TwoPaneHarness,
    a_local_file, a_local_folder, breathing, calls_of, cards_column, cross_metrics, cross_solve,
    free_fn_body, glance_fixture, grid_of, host_file_uri, host_path, host_spelling, leaf_saying,
    method_body, no_directories, on_this_host, one_turn, rail_test_body, reader_names, ring,
    ringing_tab, squeezed, squeezed_body, tab_with_a_files_column,
};
use std::time::Duration;

/// PIN (P2-9 slice 2) — **the capacity a pane is born with and the capacity a
/// fresh `settings.json` is written with are the same number**, and a file that
/// names zero lines is read as the product's answer rather than as a pane with
/// no past.
///
/// The first half is why `DEFAULT_FROZEN_LINE_QUOTA` is derived from
/// `bt_persist::DEFAULT_SCROLLBACK_LINES` instead of spelled twice: two literals
/// would agree today and drift the first time either moved, and the symptom
/// would be a pane that kept a different amount of history depending on whether
/// anyone had opened the settings dialog yet.
///
/// The second half is §5.4 逐叶降级 at a door. Zero is unreachable through the
/// picker, so a file that names it was typed into by hand; a store built from it
/// would forget every line as it printed, which is not a small setting but a
/// broken terminal.
#[test]
fn a_pane_is_born_with_the_capacity_a_fresh_settings_file_names() {
    assert_eq!(
        super::DEFAULT_FROZEN_LINE_QUOTA.get(),
        bt_persist::DEFAULT_SCROLLBACK_LINES as usize
    );
    assert_eq!(bt_persist::DEFAULT_SCROLLBACK_LINES, 100_000);
    for lines in settings::SCROLLBACK_OPTIONS {
        assert_eq!(super::scrollback_quota(lines).get(), lines as usize);
    }
    assert_eq!(
        super::scrollback_quota(0),
        super::DEFAULT_FROZEN_LINE_QUOTA,
        "a hand-edited zero falls to the product's own answer rather than              leaving a pane that forgets each line as it prints"
    );
    assert_eq!(
        super::scrollback_quota(7).get(),
        7,
        "and every other number the file can carry is honoured as written,              because `bt_persist` deliberately does not clamp this key"
    );
}

/// PIN (T2, real-machine bug): a session that stops working returns its mark
/// to full opacity — at every phase of the breath, and under both motion
/// preferences.
///
/// Red gate, reproduced on hardware: after `Start-Sleep 8` returned, the
/// tab icon sat at opacity **0.379** and stayed there. The breath is a
/// function of elapsed time, so asking it where it was at the moment work
/// stopped returns whatever the curve happened to be passing through —
/// which is any value in `.28 ..= 1.0`, and almost never `1.0`. The answer
/// cannot be interpolated; it has to be a rule, and this is that rule.
#[test]
fn a_session_that_stops_working_returns_its_mark_to_full_opacity() {
    let period = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS);
    for motion in [Motion::Full, Motion::Reduced] {
        for step in 0..=64 {
            let elapsed = period.mul_f32(step as f32 / 32.0);
            assert_eq!(
                mark_opacity(false, false, elapsed, motion),
                1.0,
                "{motion:?}: a mark that is not working is never faded, \
                     whatever phase the breath had reached"
            );
            // A ring has replaced the mark, so the mark is not faded either
            // — the ring is already saying "still going" in its own medium.
            assert_eq!(mark_opacity(true, true, elapsed, motion), 1.0);
        }
        // And while it *is* working the mark really is faded, or the pin
        // above would pass on a build that had simply deleted the breath.
        assert!(breathing(motion).opacity < 1.0, "{motion:?}: it breathes");
    }
}

/// PIN (§7.1.5b, §7.1.6b′ F3) — **the waiting halo breathes on the mark's
/// own period, out of nothing and back into nothing.**
///
/// `@keyframes fcard-wait 1.7s ease-in-out` beside `@keyframes breathe 1.7s
/// ease-in-out`: one tempo, taken from one constant, so the two things in
/// this window that breathe cannot drift apart. What differs is the shape —
/// the mark swings between two visible strengths because it is always there,
/// and the halo ramps from zero because a glow that never went out would be a
/// second border.
///
/// Red gate: give the halo a period of its own and the first assertion goes
/// red at the quarter; make it swing from a non-zero floor and the endpoints
/// do; answer anything but a flat zero under `Reduced` and the last does.
#[test]
fn the_waiting_halo_breathes_on_the_marks_own_period_and_out_of_nothing() {
    let period = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS);
    let at = |fraction: f32| wait_pulse(period.mul_f32(fraction), Motion::Full).halo;

    assert!(at(0.0).abs() < 1e-6, "out of nothing: {}", at(0.0));
    assert!(
        (at(1.0)).abs() < 1e-6,
        "and back into it, one period later: {}",
        at(1.0)
    );
    assert!(
        (at(0.5) - 1.0).abs() < 1e-6,
        "full at the one keyframe the mock-up writes: {}",
        at(0.5)
    );
    assert!(
        (at(0.25) - at(0.75)).abs() < 1e-6,
        "and symmetric about it — in and out are the same curve"
    );
    assert!(
        (at(2.5) - at(0.5)).abs() < 1e-6,
        "`infinite`: the second period is the first"
    );

    // It shares the mark's clock exactly: both are read off `breath_phase`,
    // so the peak of one is the trough of the other on the same tab.
    assert!(
        (breathe_opacity(period.mul_f32(0.5), Motion::Full) - WINDOW_TAB_BREATHE_MIN_OPACITY).abs()
            < 1e-6,
        "one clock, one period, two shapes"
    );

    for fraction in [0.0_f32, 0.25, 0.5, 0.75, 1.0, 3.7] {
        assert_eq!(
            wait_pulse(period.mul_f32(fraction), Motion::Reduced).halo,
            0.0,
            "reduced motion draws no halo at any phase — and the card's warn \
                 border, which is not this number, stays"
        );
    }
}

/// PIN (closure review of `b16f7592`, 2026-09-20) — **a window with nobody
/// waiting in it asks for no frame at all.**
///
/// The dual of the pin above, and the one the review named as missing: the
/// waiting breath never finishes on its own, so the term
/// [`TabState::mark_is_animating`] grew for it is the one term in that predicate
/// that could quietly hold a window awake forever. `strip_animation_work` folds
/// exactly this over every tab, so a tab that answers `false` here is a tab that
/// contributes no wake-up.
///
/// **The bell is the trap and is asked for here beside it.** `Bell` and
/// `Awaiting` wear the same warn ink and only the second is a place in the
/// queue (§7.1.5b), so a predicate that keyed on the dot rather than on
/// `StatusClaim::pulses` would look right on screen and wake an idle window
/// every 16ms for a picture that never changes.
///
/// Red gate: key the pulse or the frame debt on the dot's presence instead of on
/// the claim and the bell's three lines go red together.
#[test]
fn an_idle_window_asks_for_no_frame_for_a_pulse_nobody_is_owed() {
    let palette = bt_render::chrome_palette();
    let now = Instant::now();
    let mut tabs = vec![ringing_tab(1, 2), ringing_tab(2, 1)];
    let seat = tabs[0].seats.terminals()[0];

    for (index, tab) in tabs.iter().enumerate() {
        assert!(
            !tab.fleet_awaiting(),
            "tab {index} has nobody standing in the queue"
        );
        assert_eq!(
            tab.mark_state(index == 0, now, Motion::Full, &palette)
                .pulse,
            None,
            "tab {index} hands down no breath"
        );
        assert!(
            !tab.mark_is_animating(now, Motion::Full),
            "and owes no frame for one: an idle window does not wake for a pulse"
        );
    }

    // Something rang. It is the same warn dot, and it is not a queue place.
    ring(&mut tabs[0], seat);
    let mut next = attention::Places::default();
    one_turn(&mut tabs, 1, false, &mut next);
    let state = tabs[0].mark_state(false, now, Motion::Full, &palette);
    assert!(
        state.dot.is_some_and(|dot| dot.hollow),
        "the bell is on screen, hollow and warn"
    );
    assert_eq!(state.pulse, None, "and it carries no breath");
    assert!(
        !tabs[0].mark_is_animating(now, Motion::Full),
        "so the window stays asleep with a warn dot showing — §7.1.5b: bell 的橙点明确不脉动"
    );
}

/// **RED — a column scrolled on one display stands in the same place on the
/// next** (user report 2026-09-12, §7.1.6b′).
///
/// Every box the column draws is solved from the window's current scale on
/// the frame it is drawn, so a card on a 150% display is exactly three
/// quarters of the card it was on a 200% one — *except* that the solver is
/// handed a scroll offset in physical pixels, and nothing was restating it.
/// A list standing at its end then stood a third of a card past its end: the
/// top card lost its head off the clip box, and the sticky `+` came away
/// from the panel's foot and left a blank strip under the last card.
///
/// The assertion is the whole rule in one line — every card is where it was,
/// times the ratio.
#[test]
fn a_column_scrolled_at_one_scale_stands_in_the_same_place_at_another() {
    let (_, was) = CARDS_AT_200;
    let (_, now) = CARDS_AT_150;
    let ratio = now / was;

    let there = cards_column(CARDS_AT_200, 3, 0.0);
    // At its end, which is where a reader who has run the list down stands
    // and the one place the defect is impossible to miss.
    let there = cards_column(CARDS_AT_200, 3, there.max_scroll);
    let here = cards_column(
        CARDS_AT_150,
        3,
        restated_scroll(there.max_scroll, f64::from(was), f64::from(now)),
    );

    for (index, (was_card, now_card)) in there.cards.iter().zip(&here.cards).enumerate() {
        let expected = was_card.body[1] * ratio;
        assert!(
            (now_card.body[1] - expected).abs() <= 1.0,
            "card {index} stands at {} and the same place at 150% is {expected}",
            now_card.body[1]
        );
    }
    assert!(
        here.max_scroll >= restated_scroll(there.max_scroll, f64::from(was), f64::from(now)),
        "a list at its end on one display is not past its end on the next"
    );
    assert!(
        (here.new_tab[1] - there.new_tab[1] * ratio).abs() <= 1.0,
        "and the `+` is still on the panel's foot rather than floating over a blank"
    );
}

/// PIN — **`Ctrl+Shift+↑/↓` walks the commands and stops at both ends**
/// (§7.1.5c, mock-up 4690-4707).
///
/// The opposite of the strip's ring one test above, and deliberately: a strip
/// is a set of places you can be in any of, while a rail is a history with a
/// beginning and an end. Arriving at the oldest command and being thrown to
/// the newest is not what the key said, and the mock-up's own `if (at < 0)
/// return` says so too.
///
/// MUTATION: make either end wrap and the first two assertions of the second
/// block go red — which is the ruling refusing to be turned into a ring.
#[test]
fn the_command_walk_stops_at_both_ends_instead_of_wrapping() {
    // Five commands, with the viewport showing the third.
    assert_eq!(
        stepped_command_mark(5, Some(2), Step::Forward),
        Some(3),
        "next is the command after the one on screen"
    );
    assert_eq!(stepped_command_mark(5, Some(2), Step::Back), Some(1));

    // The two ends.
    assert_eq!(
        stepped_command_mark(5, Some(4), Step::Forward),
        None,
        "there is nothing after the newest command"
    );
    assert_eq!(
        stepped_command_mark(5, Some(0), Step::Back),
        None,
        "and nothing before the oldest"
    );

    // A viewport above every mark — scrolled into output older than the
    // oldest surviving command. Forwards is the first of them; backwards has
    // nowhere to go, and inventing the newest would be a wrap by another name.
    assert_eq!(stepped_command_mark(5, None, Step::Forward), Some(0));
    assert_eq!(stepped_command_mark(5, None, Step::Back), None);

    // A pane whose shell sends no OSC 133 at all answers nothing, either way
    // and from anywhere (inventory C13).
    assert_eq!(stepped_command_mark(0, None, Step::Forward), None);
    assert_eq!(stepped_command_mark(0, None, Step::Back), None);

    // A lone command: the viewport is on it, and both keys are silent.
    assert_eq!(stepped_command_mark(1, Some(0), Step::Forward), None);
    assert_eq!(stepped_command_mark(1, Some(0), Step::Back), None);
}

/// PIN — **a drag is only ever a selection**, and the plain half of the table
/// is empty for everything whose destination is outside this window
/// (§7.1.5g, user rulings 2026-08-20).
#[test]
fn hyperlink_activation_requires_a_click_without_drag() {
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            "https://example.test/path",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Browser("https://example.test/path".to_owned())
    );
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            "HTTP://localhost:3000",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Browser("HTTP://localhost:3000".to_owned())
    );
    // **A drag never activates anything**, whatever the scheme and whatever
    // the modifier — asked of every arm rather than of the web one, because
    // wanting to select a printed path is the commonest thing done to one and
    // a door that opened out of a drag would be the one nobody tested.
    for uri in [
        "https://example.test",
        host_file_uri(r"C:\notes.md").as_str(),
        host_file_uri(r"C:\page.html").as_str(),
        host_file_uri(r"C:\some\folder").as_str(),
        "mailto:person@example.test",
    ] {
        for control in [false, true] {
            assert_eq!(
                hyperlink_activation(
                    control,
                    false,
                    uri,
                    bt_transcript::paths::PathNamer::ThisWindow,
                    &|_| Some(a_local_folder())
                ),
                HyperlinkActivation::None,
                "a drag rather than a click: {uri:?}, Ctrl {control}"
            );
        }
    }
    // **A plain click starts no program.** Every row whose only destination is
    // outside this window is silent without the modifier — which is the
    // picture arm's own judgement (`local_image_activation` keeps `External`
    // for `Ctrl`) applied to the rows that leave.
    //
    // A folder is no longer one of them: since the folder ruling of
    // 2026-08-21 its plain half names a pane *inside* this window, so it
    // leaves this list the way a readable file was never in it.
    //
    // A page is no longer one of them either: since the ruling of
    // 2026-08-23 its plain half names this window's own seat, and the seat
    // draws the page rather than its source.
    //
    // Nor is a **web address**, since 2026-08-29: the seat it draws on is
    // the same one, and the discipline this list is about is untouched by
    // it — opening a pane in the window already in front of the reader is
    // what the plain half was always allowed to do, and what it must never
    // do is start a *program*. That is still `Ctrl`'s, and the row below
    // pins it.
    for uri in ["mailto:person@example.test", "ftp://files.example.test/pub"] {
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &|path| {
                    Some(if path == host_path(r"C:\some\folder").as_path() {
                        a_local_folder()
                    } else {
                        a_local_file()
                    })
                }
            ),
            HyperlinkActivation::None,
            "a plain click on a target that leaves this window: {uri:?}"
        );
    }
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            &host_file_uri(r"C:\page.html"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &|path| {
                Some(if path == host_path(r"C:\some\folder").as_path() {
                    a_local_folder()
                } else {
                    a_local_file()
                })
            }
        ),
        HyperlinkActivation::Preview(host_path(r"C:\page.html"), None),
        "and a page is a destination inside this window now, so it is not on \
             that list"
    );
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            &host_file_uri(r"C:\some\folder"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &|path| {
                Some(if path == host_path(r"C:\some\folder").as_path() {
                    a_local_folder()
                } else {
                    a_local_file()
                })
            }
        ),
        HyperlinkActivation::FilesColumn(host_path(r"C:\some\folder")),
        "and the folder that left it starts no program either — it opens a column"
    );
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            "https://example.test/path",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Page("https://example.test/path".to_owned()),
        "and neither does a web address: its plain half is a pane, not a browser"
    );
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            "https://example.test/path",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Browser("https://example.test/path".to_owned()),
        "the browser is still what `Ctrl` — and only `Ctrl` — reaches"
    );
}

/// RED (user report 2026-08-29, 实机) — **a web address printed in the
/// terminal opens on a plain click, in this window** (§7.1.5g ①).
///
/// The report: Claude Code printed
/// `https://claude.ai/code/artifact/04c0a133-…`, the application broke it
/// across two rows, both halves wore the underline — and a plain click did
/// nothing at all while `Ctrl`+click opened the browser as always. The
/// trace, one fixture, one cell, both modifiers:
///
/// ```text
/// activate_hyperlink control=0 uri="https://github.com/…/latest" arm=None
/// activate_hyperlink control=1 uri="https://github.com/…/latest" arm=Browser
/// ```
///
/// The wrap was not the fault and neither was the host: a one-line
/// `https://github.com/…` and a `http://localhost:5173/index.html` answered
/// `arm=None` in the same run. **The `http(s)` row's plain half was still
/// empty** — the one exception left in a table whose `file:` arm says, in
/// its own comment, that there is none. That arm's note names the event
/// that was supposed to fill this one: "Nothing in this window renders a
/// web page yet; W2 is where this half of the row stops being empty." W2
/// landed on 2026-08-23, the page arm was rewritten that day, and this row
/// was not.
///
/// So the row is asserted the way the ruling reads it — **平点 = the
/// destination inside this window, `Ctrl`+点 = hand it to the system** — and
/// it is asserted about *every* address alike, because "wrapped",
/// "claude.ai" and "loopback" were three spellings of one empty arm and a
/// fix that named any of them would be a fourth.
///
/// MUTATION: put `ClickIntent::Here => HyperlinkActivation::None` back in
/// the `http(s)` arm and every plain row below goes red while the `Ctrl`
/// rows stay green — which is the shape of the report exactly.
#[test]
fn a_web_address_printed_in_the_terminal_opens_in_this_window() {
    // The reported address, and its two controls: the one-line link and the
    // dev server's own. One arm, three hosts, no host named in the code.
    for uri in [
        "https://claude.ai/code/artifact/04c0a133-319b-4c8e-b988-7965fe063626",
        "https://github.com/openai/codex/releases/latest",
        "http://localhost:5173/index.html",
        "HTTPS://EXAMPLE.TEST/Path?q=1#frag",
    ] {
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::Page(uri.to_owned()),
            "a plain click on {uri:?} opens it in this window"
        );
        assert_eq!(
            hyperlink_activation(
                true,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::Browser(uri.to_owned()),
            "and Ctrl still hands {uri:?} to the system"
        );
        // The finger is the same reading as the verb, so it lights plainly
        // now — the half of 7.1.5f's complaint that was still true of this
        // row: an underline that answered a hover and not a press.
        assert!(
            terminal_link_answers_a_press(
                false,
                Some(uri),
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            "and the hand is on {uri:?} without a modifier"
        );
    }
    // **The gate the plain half is spent through is the address bar's own.**
    // `open_web_page`'s contract is that every caller passes
    // `webnav::address_bar` first, so the arm may only carry addresses that
    // door admits — otherwise the terminal and the address field would give
    // two answers about one string, which §7.1.5g ⑤ forbids in as many
    // words.
    for uri in [
        "https://claude.ai/code/artifact/04c0a133-319b-4c8e-b988-7965fe063626",
        "http://localhost:5173/index.html",
    ] {
        let HyperlinkActivation::Page(url) = hyperlink_activation(
            false,
            true,
            uri,
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories,
        ) else {
            panic!("{uri} is a page");
        };
        assert!(
            matches!(webnav::address_bar(&url), webnav::Decision::Navigate(_)),
            "the one door admits what the arm carries: {uri:?}"
        );
    }
    // A drag is still only a selection, and a scheme with no arm of its own is
    // still silent plainly — and under `Ctrl` handed to the machine since
    // ticket 14 (owner ruling 2026-09-21), where it used to be blocked.
    assert_eq!(
        hyperlink_activation(
            false,
            false,
            "https://example.test/x",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::None
    );
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            "mailto:person@example.test",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::None
    );
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            "mailto:person@example.test",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Scheme("mailto:person@example.test".to_owned())
    );
    // An address this window would refuse is refused under both modifiers,
    // and refusing it plainly is not the same as opening it: `Blocked` is
    // `Ctrl`'s word and the plain half stays silent, exactly as it does for
    // every other row whose text does not parse.
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            "http:8080/nohost",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::None
    );
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            "http:8080/nohost",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Blocked(LinkRefusal::Invalid)
    );
}

/// PIN (R1-26) — **one refusal, shared.** The address field will not load
/// `https://user:pass@host`, and the browser door must not spend it either.
///
/// The shape is the phishing one: what a reader checks before pressing is
/// the host, and userinfo puts a name they trust in front of a host they do
/// not. `webnav::address_bar` has refused it since the door was written;
/// the terminal's `Ctrl` half went straight to `ShellExecuteW` and never
/// asked.
///
/// MUTATION: read the row without the address door and the first assertion
/// hands the shell the address.
#[test]
fn the_browser_door_refuses_the_userinfo_shape_the_address_field_refuses() {
    let no_directories = |_: &Path| Some(a_local_file());
    for uri in [
        "https://example.test@evil.test/",
        "https://user:pass@evil.test/path",
        "http://bank.test@203.0.113.9/",
    ] {
        assert_eq!(
            hyperlink_activation(
                true,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories,
            ),
            HyperlinkActivation::Blocked(LinkRefusal::Door),
            "{uri} is the shape the address field already refuses"
        );
        // The plain half is the seat's, and the seat's own door says the
        // same word about it — under the cells the address is printed in,
        // which is where a reader is looking.
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories,
            ),
            HyperlinkActivation::Page(uri.to_owned()),
            "{uri} goes to the seat, whose door refuses it there"
        );
        assert_eq!(
            webnav::address_bar(uri),
            webnav::Decision::Refuse(webnav::Refusal::UserInfo),
            "{uri} is refused by the one door both halves read"
        );
    }
    // A bare `@` outside the authority is ordinary text and stays open.
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            "https://example.test/a@b",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories,
        ),
        HyperlinkActivation::Browser("https://example.test/a@b".to_owned())
    );
}

/// PIN — **the routing table, both halves, one assertion per cell** (§7.1.5g,
/// user rulings 2026-08-20).
///
/// The reported gap is the `file:` row: Claude Code prints `[file]` lines as
/// OSC 8 `file://` links, this window drew the hover underline over them and
/// then refused the click the underline had promised — 7.1.5f's own
/// complaint, one content type over. The second ruling of the same day split
/// every row: plainly, this window's own answer; under `Ctrl`, the system's.
///
/// MUTATIONS:
/// ① send `file:` back to `Blocked` and the readable-file cells go red, which
///    is the reported bug written down;
/// ② drop the `is_directory` question and a folder opens as a "no preview"
///    card instead of in Explorer;
/// ③ let the share be probed — swap the `may_read_unasked` gate for `true` — and the
///    UNC cells go red, which is the event loop being handed a cold network
///    round trip (the plain half is the card and `Ctrl`'s is the system, since
///    ticket 14 — neither asks);
/// ④ let the plain half answer `Browser`/`Reveal`/`External` and a stray
///    click on a printed line starts a program;
/// ⑤ send the folder's plain half back to `None` — the shape it had until
///    2026-08-21 — and every prompt in the window wears a dotted cwd that
///    answers nothing, which is the promise the underline is not allowed to
///    break;
/// ⑥ send the web address's plain half back to `None` — the shape it had
///    until 2026-08-29, and the last cell in this table that still had it —
///    and every printed address goes back to answering a hover and not a
///    press. ④ and ⑥ are the two directions this row can be got wrong in
///    and they are not the same mistake: ④ is a plain click starting a
///    *program*, which it must never do, and ⑥ is a plain click doing
///    nothing where this window has somewhere to go, which is the same
///    half-lie ⑤ is about;
/// ⑦ make `may_read_unasked` refuse a WSL distribution's share again — drop its
///    `wsl_share_root_length` arm — and the `wsl.localhost` cells go red: a file inside the
///    distribution meets the network card, which is the refusal 2026-09-07 lifted.
#[test]
fn a_click_routes_web_files_pages_folders_shares_and_unknown_schemes() {
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            "https://example.test/path",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Browser("https://example.test/path".to_owned()),
        "a web address under `Ctrl` is the machine's browser"
    );
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            "https://example.test/path",
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Page("https://example.test/path".to_owned()),
        "and plainly it is this window's own seat (2026-08-29): the cell that              read `None` here for as long as nothing in this window drew a page"
    );
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            &host_file_uri(r"C:\Users\me\phd-application-timeline.html"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Preview(
            host_path(r"C:\Users\me\phd-application-timeline.html"),
            None
        ),
        "a plain click on a page opens the page, on the seat that renders \
             one (user ruling 2026-08-23). It used to open nothing, because the \
             seat could only have shown the source of the thing the link was \
             pointing past"
    );
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            &host_file_uri(r"C:\Users\me\phd-application-timeline.html"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::External(host_path(r"C:\Users\me\phd-application-timeline.html")),
        "and Ctrl sends the page to whatever this machine opens pages with"
    );
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            &host_file_uri(r"C:\Users\me\notes.md"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Preview(host_path(r"C:\Users\me\notes.md"), None),
        "every other local file goes down the files column's own road"
    );
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            &host_file_uri(r"C:\Users\me\notes.md"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::External(host_path(r"C:\Users\me\notes.md")),
        "and Ctrl hands that same file to the system instead"
    );
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            &host_file_uri(r"D:\%E4%B8%AD%E6%96%87\note.md"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::Preview(host_path(r"D:\中文\note.md"), None),
        "and it is the decoded path that travels, not the URI"
    );
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            &host_file_uri(r"C:\repo\docs"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &|path| {
                Some(if path == host_path(r"C:\repo\docs").as_path() {
                    a_local_folder()
                } else {
                    a_local_file()
                })
            }
        ),
        HyperlinkActivation::Reveal(host_path(r"C:\repo\docs")),
        "a folder is Explorer's"
    );
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            &host_file_uri(r"C:\repo\docs"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &|path| {
                Some(if path == host_path(r"C:\repo\docs").as_path() {
                    a_local_folder()
                } else {
                    a_local_file()
                })
            }
        ),
        HyperlinkActivation::FilesColumn(host_path(r"C:\repo\docs")),
        "and a plain click opens no Explorer window: it points this window's own column at it"
    );
    // **A share: the card plainly, the system under `Ctrl`** (ticket 14, owner ruling
    // 2026-09-21). Neither half probes it — the ledger closure panics if asked.
    // Off Windows a URI naming another host names no path on this machine: the plain half
    // says nothing and `Ctrl` is refused, and neither asks either.
    for (control, expected) in on_this_host(
        [
            (
                false,
                HyperlinkActivation::Preview(PathBuf::from(r"\\server\share\notes.md"), None),
            ),
            (
                true,
                HyperlinkActivation::Share(PathBuf::from(r"\\server\share\notes.md")),
            ),
        ],
        [
            (false, HyperlinkActivation::None),
            (true, HyperlinkActivation::Blocked(LinkRefusal::Invalid)),
        ],
    ) {
        assert_eq!(
            hyperlink_activation(
                control,
                true,
                "file://server/share/notes.md",
                bt_transcript::paths::PathNamer::ThisWindow,
                &|_| { panic!("a share must not be probed: §7.1.3 does not read one unasked") }
            ),
            expected,
            "a share is answered without a round trip, Ctrl {control}"
        );
    }
    // **The one share that is not one** (user ruling 2026-09-07, §7.30). A WSL distribution's
    // filesystem is running on this machine and reaching it crosses no network, so the row it
    // takes is the row every other local file takes — and the URI it takes it by is the one
    // `local_path_to_file_uri` writes for exactly that path, which is what makes the printed
    // `/etc/hosts` in a WSL pane and this table one road rather than two. A distribution's
    // share is Windows path grammar, so these rows are Windows-only.
    #[cfg(windows)]
    {
        let hosts = PathBuf::from(r"\\wsl.localhost\Ubuntu\etc\hosts");
        let hosts_uri = bt_transcript::paths::local_path_to_file_uri(&hosts);
        assert_eq!(hosts_uri, "file://wsl.localhost/Ubuntu/etc/hosts");
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                &hosts_uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::Preview(hosts.clone(), None),
            "a distribution-internal path opens in this window, through the share Windows serves it at"
        );
        assert_eq!(
            hyperlink_activation(
                true,
                true,
                &hosts_uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::External(hosts),
            "and `Ctrl` hands it over exactly as it hands over a drive-rooted file"
        );
    }
    for uri in [
        "mailto:person@example.test",
        "javascript:alert(1)",
        "custom://payload",
        "vscode://file/C:/x",
    ] {
        assert_eq!(
            hyperlink_activation(
                true,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::Scheme(uri.to_owned()),
            "any other scheme is the machine's under `Ctrl` (ticket 14): {uri:?}"
        );
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::None,
            "and says nothing at all when it was not: {uri:?}"
        );
    }
}

/// RED (ticket 14) — **`Ctrl` on a share hands it to the system, and a plain click keeps the
/// card.**
///
/// Owner ruling 2026-09-21: 「UNC 与任意协议链接 Ctrl+点击交给系统、悬停不碰 UNC、普通点击不变」.
/// A share was the card §7.1.3 draws under either modifier for as long as `ShellExecuteW` ran on
/// the window thread — handing over a cold `\\server` would have stalled the window for the round
/// trip. Hand-offs run on their own lane since 2026-09-22, so `Ctrl`'s half is the system's; the
/// plain half is unchanged, and neither half asks the ledger anything (the closure panics).
///
/// Red on the base: `Ctrl` answers `Preview(\\server\share\a.md, None)`.
///
/// MUTATION: delete the `ClickIntent::System if is_a_share_on_another_machine(..)` arm of
/// `reference_activation` and the `Ctrl` row comes back `Preview`.
///
/// Windows only: a share is Windows path grammar. Off Windows a `file:` URI naming another
/// host names no path at all, which
/// `a_click_routes_web_files_pages_folders_shares_and_unknown_schemes` pins.
#[cfg(windows)]
#[test]
fn ctrl_on_a_share_hands_it_to_the_system_and_a_plain_click_keeps_the_card() {
    let share = PathBuf::from(r"\\server\share\a.md");
    let windows = bt_transcript::paths::PrintedPathNamespace::Windows;
    for namer in [
        bt_transcript::paths::PathNamer::ThisWindow,
        bt_transcript::paths::PathNamer::Pane(&windows),
    ] {
        let unasked: &dyn Fn(&Path) -> Option<bt_term::PathVerdict> =
            &|_| panic!("a share is never asked about");
        assert_eq!(
            hyperlink_activation(true, true, "file://server/share/a.md", namer, unasked),
            HyperlinkActivation::Share(share.clone()),
            "Ctrl hands the share to the system"
        );
        assert_eq!(
            hyperlink_activation(false, true, "file://server/share/a.md", namer, unasked),
            HyperlinkActivation::Preview(share.clone(), None),
            "and a plain click keeps the card it always raised"
        );
        // The line under the address says what `Ctrl` does, read off the same table.
        assert_eq!(
            ControlClickHint::of("file://server/share/a.md", namer, unasked),
            Some(ControlClickHint::DefaultApp)
        );
        // A folder on a share is the same row: nothing asks whether it is one, and the system
        // opens a folder in Explorer.
        assert_eq!(
            hyperlink_activation(true, true, "file://nas/photos/", namer, unasked),
            HyperlinkActivation::Share(PathBuf::from(r"\\nas\photos\")),
        );
    }
}

/// RED (ticket 14) — **`Ctrl` on a link of any other scheme hands the URI to the system; a plain
/// click does nothing.**
///
/// Owner ruling 2026-09-21: 「任意协议链接 Ctrl+点击交给系统」, and 2026-09-21 again: 「修饰键就是
/// 用户的同意」. No list of schemes stands in front of the arm — what the machine has no handler
/// for, the machine refuses. A single letter before a colon is a drive, not a scheme, and stays
/// refused: a path never leaves as a URI, because that would skip the program list.
///
/// Red on the base: every `Ctrl` row below answers `Blocked`.
///
/// MUTATION: send `ReferenceRow::Scheme` under `ClickIntent::System` to `Blocked` again and the
/// `Ctrl` rows go red.
#[test]
fn ctrl_on_any_scheme_hands_the_uri_to_the_system() {
    for uri in [
        "mailto:x@example.com",
        "vscode://file/C:/work/notes.md:12",
        "ssh://h",
        "obsidian://open?vault=notes&file=today",
        "ms-settings:display",
    ] {
        let namer = bt_transcript::paths::PathNamer::ThisWindow;
        assert_eq!(
            hyperlink_activation(true, true, uri, namer, &no_directories),
            HyperlinkActivation::Scheme(uri.to_owned()),
            "Ctrl hands {uri:?} to the system"
        );
        assert_eq!(
            hyperlink_activation(false, true, uri, namer, &no_directories),
            HyperlinkActivation::None,
            "a plain click on {uri:?} does nothing"
        );
        assert!(terminal_link_answers_a_press(
            true,
            Some(uri),
            namer,
            &no_directories
        ));
        assert!(!terminal_link_answers_a_press(
            false,
            Some(uri),
            namer,
            &no_directories
        ));
    }
    // A drive letter is a path, and a path never leaves as a URI.
    for not_a_scheme in [r"C:\work\run.cmd", "C:/work/run.cmd", "no scheme here"] {
        assert_eq!(
            hyperlink_activation(
                true,
                true,
                not_a_scheme,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::Blocked(LinkRefusal::Invalid),
            "{not_a_scheme:?} is not a URI"
        );
    }
}

/// PIN (ticket 14) — **a device path and a verbatim spelling stay refused under `Ctrl`**, so
/// handing shares over cannot widen past the ruling.
///
/// `file://./pipe/x` and a `\\?\UNC\…` spelled into a `file:` URI name no file on another
/// machine; `bt_platform::file_uri_to_path` names no path from either, and `Ctrl` is told so.
/// Another WSL distribution's share keeps the card under both modifiers.
///
/// MUTATION: route every `ReferenceRow::Unasked` path to `Share` under `Ctrl` (drop the
/// `is_a_share_on_another_machine` guard) and the distribution row goes red.
///
/// Windows only: device, verbatim and WSL share spellings are Windows path grammar, and a POSIX
/// path has none of them.
#[cfg(windows)]
#[test]
fn device_and_verbatim_spellings_stay_refused_under_ctrl() {
    for uri in [
        "file://./pipe/x",
        "file:////./pipe/x",
        "file://%3F/UNC/server/share/a.md",
        "file:////%3F/UNC/server/share/a.md",
    ] {
        assert_eq!(
            hyperlink_activation(
                true,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &|_| panic!("a device or verbatim spelling is never asked about")
            ),
            HyperlinkActivation::Blocked(LinkRefusal::Invalid),
            "{uri:?} stays refused under Ctrl"
        );
    }
    let windows = bt_transcript::paths::PrintedPathNamespace::Windows;
    let debian = PathBuf::from(r"\\wsl.localhost\Debian\etc\hosts");
    for control in [false, true] {
        assert_eq!(
            hyperlink_activation(
                control,
                true,
                "file://wsl.localhost/Debian/etc/hosts",
                bt_transcript::paths::PathNamer::Pane(&windows),
                &|_| panic!("a distribution this pane is not in is never asked about")
            ),
            HyperlinkActivation::Preview(debian.clone(), None),
            "another distribution's share is not another machine's: the card, Ctrl {control}"
        );
    }
}

/// RED — **a distribution's share is the pane's to name, and only its own pane's** (route D of
/// the untrusted-path audit, 2026-09-08).
///
/// RED EVIDENCE (2026-09-08). §7.30 gave a WSL pane's `/etc/hosts` a road into this window
/// through `\\wsl.localhost\<distro>\…`, and the gate that let it through read the path's
/// prefix and nothing else — so `file://wsl.localhost/Ubuntu/etc/hosts` declared over `OSC 8`
/// by any program in any pane went down the same road. What that costs is not hypothetical:
/// the arm below the gate calls `is_dir` on the window thread, and reaching into a
/// distribution that is not running starts a virtual machine while the window is holding
/// still. Before the fix, in a PowerShell pane:
///
/// ```text
/// a share a PowerShell pane names is not that pane's to name
///   left: External("\\\\wsl.localhost\\Ubuntu\\etc\\hosts")
///  right: Preview("\\\\wsl.localhost\\Ubuntu\\etc\\hosts", None)
/// ```
///
/// The `Preview` arm is the refusal card §7.1.3 already draws for a share, which is what a
/// pane that cannot name this place owes a reader: the reference is still there, the card
/// still says why nothing is shown, and nothing is opened.
///
/// MUTATION: ignore the namer in `may_read_unasked` and the first two assertions go red — one
/// pane's link is every pane's again.
///
/// Windows only: a WSL distribution's share exists only where Windows serves one.
#[cfg(windows)]
#[test]
fn a_distribution_share_is_named_only_by_the_pane_standing_in_it() {
    let hosts = PathBuf::from(r"\\wsl.localhost\Ubuntu\etc\hosts");
    let uri = bt_transcript::paths::local_path_to_file_uri(&hosts);
    let windows = bt_transcript::paths::PrintedPathNamespace::Windows;
    let ubuntu = bt_transcript::paths::PrintedPathNamespace::Wsl {
        distro: Some("Ubuntu".to_owned()),
        home: None,
    };
    let debian = bt_transcript::paths::PrintedPathNamespace::Wsl {
        distro: Some("Debian".to_owned()),
        home: None,
    };
    for (namespace, what) in [
        (&windows, "a PowerShell pane"),
        (&debian, "a pane standing in another distribution"),
    ] {
        assert_eq!(
            hyperlink_activation(
                true,
                true,
                &uri,
                bt_transcript::paths::PathNamer::Pane(namespace),
                &|_| panic!("the disk is not asked about a place this pane cannot name"),
            ),
            HyperlinkActivation::Preview(hosts.clone(), None),
            "a share {what} names is not that pane's to name",
        );
    }
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            &uri,
            bt_transcript::paths::PathNamer::Pane(&ubuntu),
            &no_directories,
        ),
        HyperlinkActivation::External(hosts),
        "and the pane standing in Ubuntu opens Ubuntu's own files exactly as before",
    );
}

/// PIN — **a printed `file:` link to a local page never opens its source**
/// (§7.1.5g, user rulings 2026-08-20 and 2026-08-23).
///
/// The reported gap: `html` was in `preview::TEXT_EXTENSIONS`, so the arm
/// that had just been opened for `file:` handed a `.html` to the preview seat
/// and the user — who had pressed a link *to a page* — got the page's source.
/// The 2026-08-20 answer was to leave the plain half empty until W2 had a
/// seat that renders one; W2 landed, and **the plain half now opens the
/// page**, which is the same sentence with the exception taken out of it
/// (§7.1.5j ⑦(e) wrote it forwards: "W2 落地时改的仍是那张表的臂"). What has
/// not changed by a letter is the half this test is named for — a press on a
/// link to a page never yields the page's *source*, and nothing that merely
/// reads like a page is treated as one.
///
/// MUTATIONS:
/// ① compare the extension by `ends_with`/`contains` instead of by
///    `Path::extension` and `report.html.txt` — a text file — is opened on
///    the engine's lane;
/// ② let the page arm run before the directory test and a folder named
///    `site.html` stops opening in Explorer;
/// ③ let the page arm see a share and a cold `\\server\…\index.html` is
///    handed to a synchronous `ShellExecuteW` instead of meeting §7.1.3's
///    network card — and, worse, does so having asked the network first.
#[test]
fn a_local_html_page_opens_as_a_page_and_nothing_that_merely_reads_like_one_does() {
    for (uri, path) in [
        (
            host_file_uri(r"C:\Users\me\timeline.html"),
            host_spelling(r"C:\Users\me\timeline.html"),
        ),
        (
            host_file_uri(r"C:\Users\me\TIMELINE.HTM"),
            host_spelling(r"C:\Users\me\TIMELINE.HTM"),
        ),
        (
            host_file_uri(r"C:\Program%20Files\report.html"),
            host_spelling(r"C:\Program Files\report.html"),
        ),
        (
            host_file_uri(r"D:\%E4%B8%AD%E6%96%87\%E9%A1%B5.html"),
            host_spelling(r"D:\中文\页.html"),
        ),
    ] {
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                &uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::Preview(PathBuf::from(&path), None),
            "a plain click opens the page in this window: {uri:?}"
        );
        // And what it opens it *as* is the engine's lane, never the document
        // pool — which is the half of this test's name that did not move.
        assert_eq!(
            preview_open_lane(Path::new(&path)),
            PreviewOpenLane::Page,
            "and the seat draws the page rather than its source: {uri:?}"
        );
        assert!(
            matches!(
                hyperlink_activation(
                    true,
                    true,
                    &uri,
                    bt_transcript::paths::PathNamer::ThisWindow,
                    &no_directories
                ),
                HyperlinkActivation::External(_)
            ),
            "and Ctrl still hands the page to this machine: {uri:?}"
        );
    }
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            &host_file_uri(r"C:\Program%20Files\report.html"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &no_directories
        ),
        HyperlinkActivation::External(host_path(r"C:\Program Files\report.html")),
        "and it is the decoded path that travels, not the URI"
    );
    // The real extension, never a substring of the name: one of these is a
    // template dialect and the other is a text file that happens to spell
    // `.html` in the middle of its name.
    for (uri, path) in [
        (
            host_file_uri(r"C:\site\index.htmlx"),
            host_spelling(r"C:\site\index.htmlx"),
        ),
        (
            host_file_uri(r"C:\site\report.html.txt"),
            host_spelling(r"C:\site\report.html.txt"),
        ),
        (
            host_file_uri(r"C:\site\html"),
            host_spelling(r"C:\site\html"),
        ),
    ] {
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                &uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::Preview(PathBuf::from(&path), None),
            "not a page, so a plain click is still the preview seat's: {uri:?}"
        );
        // Both halves of the table reach the seat now, so this is where the
        // difference between them is: a template dialect and a text file go
        // through the pool and are read, not rendered.
        assert_eq!(
            preview_open_lane(Path::new(&path)),
            PreviewOpenLane::Document,
            "and it is read as a document: {uri:?}"
        );
    }
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            &host_file_uri(r"C:\sites\archive.html"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &|path| {
                Some(if path == host_path(r"C:\sites\archive.html").as_path() {
                    a_local_folder()
                } else {
                    a_local_file()
                })
            }
        ),
        HyperlinkActivation::Reveal(host_path(r"C:\sites\archive.html")),
        "a folder is Explorer's however it is named"
    );
    // A plain click on a page on a share meets the network card it always met, and `Ctrl`
    // hands it to the system (ticket 14) — neither with a round trip.
    // Off Windows a URI naming another host names no path on this machine: the plain half
    // says nothing and `Ctrl` is refused, and neither asks either.
    for (control, expected) in on_this_host(
        [
            (
                false,
                HyperlinkActivation::Preview(PathBuf::from(r"\\server\share\index.html"), None),
            ),
            (
                true,
                HyperlinkActivation::Share(PathBuf::from(r"\\server\share\index.html")),
            ),
        ],
        [
            (false, HyperlinkActivation::None),
            (true, HyperlinkActivation::Blocked(LinkRefusal::Invalid)),
        ],
    ) {
        assert_eq!(
            hyperlink_activation(
                control,
                true,
                "file://server/share/index.html",
                bt_transcript::paths::PathNamer::ThisWindow,
                &|_| { panic!("a share must not be probed: §7.1.3 does not read one unasked") }
            ),
            expected,
            "a share is answered without a round trip, Ctrl {control}"
        );
    }
    assert_eq!(
        preview_open_lane(Path::new(r"\\server\share\index.html")),
        PreviewOpenLane::Document,
        "and a page on a share is not a page: the mint refuses to make one, \
             so the card that says so is the document lane's"
    );
}

/// PIN — **a malformed address, a target with no scheme and a `file:` URI naming nothing here
/// are never handed to the shell as a URI.**
///
/// The `file:` row left this list on 2026-08-20 and did not join the shell's
/// *as a URI*: it became a path this window decides about itself — the
/// preview seat, Explorer, or the path bridge that refuses programs — so what
/// leaves is always the decoded path and never the URI text a program
/// printed. A URI reaching the shell would be parsed a second time by a
/// second parser; a path has already been read once, here.
///
/// Ticket 14 took `mailto:` and `custom://` off this list (owner ruling 2026-09-21:
/// 「任意协议链接 Ctrl+点击交给系统」) — see `ctrl_on_any_scheme_hands_the_uri_to_the_system`.
/// What stays is what is not a well-formed request at all: an `http(s)` address
/// `webnav::address_bar` refuses, text with no scheme, and a `file:` URI this machine names no
/// path from, which would otherwise reach that second parser.
#[test]
fn only_a_well_formed_web_address_is_ever_handed_to_the_shell() {
    for uri in [
        "https://example.test/\nspoof",
        "http://",
        "https:example.test",
        "not-a-uri",
        // A `file:` URI naming nothing on this machine does not fall through
        // to the shell, which would parse it a second way: on Windows one with
        // no drive, elsewhere one naming another host.
        on_this_host("file:///etc/passwd", "file://elsewhere/etc/passwd"),
        "file:///C:/100%/x.md",
    ] {
        assert_eq!(
            hyperlink_activation(
                true,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::Blocked(LinkRefusal::Invalid),
            "{uri:?}"
        );
    }
}

/// PIN (§7.1.5j, user report 2026-08-20) — **a bare file path printed into the terminal
/// reaches the five-armed routing table as a `file:` target, and the press that reaches it is
/// this window's.**
///
/// This is the whole claim of the slice, asked at the seam where it either holds or does not:
/// nothing in [`hyperlink_activation`], [`Runtime::pressed_cell_target`] or
/// [`press_belongs_to_the_window`] was written for printed paths, and nothing in them was
/// changed for printed paths. The recognition folds a verified name into the one field an OSC 8
/// link uses, and everything downstream reads that field.
///
/// MUTATIONS: route a printed path anywhere but through the cell's own `hyperlink` and this
/// stops being a statement about the shared table; give the link the printed text instead of a
/// URI and the `file:` arm never fires.
#[test]
fn a_verified_bare_path_reaches_the_five_armed_table_as_a_file_target() {
    let directory = bt_testpath::temp_path("betterterminal-app-printed-path");
    std::fs::create_dir(&directory).unwrap();
    let readable = directory.join("notes.md");
    std::fs::write(&readable, b"# notes\n").unwrap();
    let page = directory.join("report.html");
    std::fs::write(&page, b"<p>hi</p>").unwrap();

    let printed = format!(
        "{} {} {}",
        readable.to_string_lossy(),
        page.to_string_lossy(),
        directory.to_string_lossy()
    );
    let mut session = DualPlaneSession::new(
        NonZeroU32::new(printed.chars().count() as u32 + 8).unwrap(),
        NonZeroU32::new(4).unwrap(),
    );
    session.set_math_layout_options(bt_term::MathLayoutOptions {
        detect_image_paths: true,
        ..bt_term::MathLayoutOptions::default()
    });
    session.feed(printed.as_bytes()).unwrap();
    let mut projection = session.new_projection(session.layout_key());
    // Frame one asks, the worker answers, frame two draws — the app's own rhythm, run here by
    // hand because a unit test has no worker thread.
    session.viewport_frame(&mut projection).unwrap();
    session.absorb_printed_path_probes(&mut projection);
    while let Some(task) = session.take_decoration_worker_task() {
        if let bt_term::SessionDecorationTask::VerifyPath(path) = task {
            let verdict = bt_term::verify_path(&path, &bt_platform::resolved_for_a_door);
            session.complete_path_verification(path, verdict);
        }
    }
    let frame = session.viewport_frame(&mut projection).unwrap();

    // Column by construction rather than by search: the directory's own name is a prefix of
    // both file names, so looking one up would find the wrong one.
    let readable_column = 0u32;
    let page_column = readable.to_string_lossy().chars().count() as u32 + 1;
    let directory_column = page_column + page.to_string_lossy().chars().count() as u32 + 1;
    let is_directory = |path: &Path| {
        Some(bt_term::verify_path(
            path,
            &bt_platform::resolved_for_a_door,
        ))
    };
    for (column, named, plain, control) in [
        (
            readable_column,
            readable.clone(),
            HyperlinkActivation::Preview(readable.clone(), None),
            HyperlinkActivation::External(readable.clone()),
        ),
        (
            // A printed page is a destination inside this window since the
            // ruling of 2026-08-23: plainly it is the seat that renders it,
            // `Ctrl` still hands it to the machine.
            page_column,
            page.clone(),
            HyperlinkActivation::Preview(page.clone(), None),
            HyperlinkActivation::External(page.clone()),
        ),
        // The prompt's own cwd is a directory that is really there, so it
        // earns the dotted rest — and a dotted rest is a promise. Plainly it
        // is the files column's (user ruling 2026-08-21); `Ctrl` still hands
        // it to Explorer.
        (
            directory_column,
            directory.clone(),
            HyperlinkActivation::FilesColumn(directory.clone()),
            HyperlinkActivation::Reveal(directory.clone()),
        ),
    ] {
        let hit = frame
            .hyperlink_at(0, column)
            .unwrap_or_else(|| panic!("{} is a link", named.display()));
        assert_eq!(
            hit.uri,
            bt_transcript::paths::local_path_to_file_uri(&named),
            "{} carries the target a `file:` link carries",
            named.display()
        );
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                &hit.uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &is_directory
            ),
            plain,
            "plain click on {}",
            named.display()
        );
        assert_eq!(
            hyperlink_activation(
                true,
                true,
                &hit.uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &is_directory
            ),
            control,
            "Ctrl+click on {}",
            named.display()
        );
        assert!(
            press_belongs_to_the_window(input::MouseProtocolButton::Left, PressedCellTarget::Ours),
            "and the press that spends it is this window's"
        );
    }
    assert!(
        frame
            .hyperlink_at(0, printed.chars().count() as u32 + 1)
            .is_none(),
        "the blank beyond the names carries nothing"
    );

    std::fs::remove_file(&readable).unwrap();
    std::fs::remove_file(&page).unwrap();
    std::fs::remove_dir(&directory).unwrap();
}

/// PIN (§7.1.5j, `path:line[:col]`) — **a reference that names a line reaches the five-armed
/// table as the file it names, with the line riding on the preview arm and on no other.**
///
/// The whole of what makes the other four arms free: they decode through something that cuts a
/// fragment, so `file:///…/notes.md#L13C5` is the same file to Explorer, to the registered
/// handler and to the files column that `file:///…/notes.md` always was.
#[test]
fn a_located_reference_carries_its_line_to_the_preview_arm_alone() {
    let directory = bt_testpath::temp_path("betterterminal-located");
    std::fs::create_dir(&directory).unwrap();
    let readable = directory.join("notes.md");
    std::fs::write(&readable, b"# notes\n").unwrap();

    let printed = format!("{}:13:5", readable.to_string_lossy());
    let mut session = DualPlaneSession::new(
        NonZeroU32::new(printed.chars().count() as u32 + 8).unwrap(),
        NonZeroU32::new(4).unwrap(),
    );
    session.set_math_layout_options(bt_term::MathLayoutOptions {
        detect_image_paths: true,
        ..bt_term::MathLayoutOptions::default()
    });
    session.feed(printed.as_bytes()).unwrap();
    let mut projection = session.new_projection(session.layout_key());
    session.viewport_frame(&mut projection).unwrap();
    session.absorb_printed_path_probes(&mut projection);
    while let Some(task) = session.take_decoration_worker_task() {
        if let bt_term::SessionDecorationTask::VerifyPath(path) = task {
            let verdict = bt_term::verify_path(&path, &bt_platform::resolved_for_a_door);
            session.complete_path_verification(path, verdict);
        }
    }
    let frame = session.viewport_frame(&mut projection).unwrap();

    // The span is the whole reference, so the `:13:5` is part of what a pointer must be over
    // and part of what the mark covers.
    let last = printed.chars().count() as u32 - 1;
    let hit = frame
        .hyperlink_at(0, last)
        .expect("the line number is inside the reference, not prose behind it");
    assert_eq!(
        hit,
        frame.hyperlink_at(0, 0).expect("and so is its head"),
        "one reference, one link"
    );
    assert_eq!(
        hit.uri,
        format!(
            "{}#L13C5",
            bt_transcript::paths::local_path_to_file_uri(&readable)
        ),
        "the target is the file, and the line rides in its fragment"
    );

    let is_directory = |path: &Path| {
        Some(bt_term::verify_path(
            path,
            &bt_platform::resolved_for_a_door,
        ))
    };
    assert_eq!(
        hyperlink_activation(
            false,
            true,
            &hit.uri,
            bt_transcript::paths::PathNamer::ThisWindow,
            &is_directory
        ),
        HyperlinkActivation::Preview(
            readable.clone(),
            Some(bt_transcript::paths::PrintedPathLocation {
                line: 13,
                column: Some(5)
            })
        ),
        "plainly, the preview seat — told where to look"
    );
    assert_eq!(
        hyperlink_activation(
            true,
            true,
            &hit.uri,
            bt_transcript::paths::PathNamer::ThisWindow,
            &is_directory
        ),
        HyperlinkActivation::External(readable.clone()),
        "and the system's handler takes the file it always took"
    );

    std::fs::remove_file(&readable).unwrap();
    std::fs::remove_dir(&directory).unwrap();
}

/// PIN — **one run, one activation target** (§7.1.5g): a link whose label
/// soft-wraps is one link wherever it is pressed, and both segments spend the
/// same door on the same URI.
///
/// From bytes rather than from a hand-built frame, because the wrap is the
/// thing under test and only the terminal can produce a real one: the
/// grouping id alacritty synthesizes, the `continues` bit the row carries,
/// and the run those two make between them are all facts about what the
/// emulator did with these bytes.
///
/// Ownership is decided by the hit's own anchors, which is what makes this
/// survive the horizontal-scroll slice: a `HyperlinkHit` names content, and
/// the column a cell was drawn at is not part of the answer.
///
/// MUTATIONS: narrow `link_group_run` to a single row and the two hits stop
/// being equal; key the activation off the pressed *cell* rather than the
/// hit's `uri` and the second half opens something else.
#[test]
fn a_wrapped_link_activates_the_same_target_from_either_segment() {
    let target = host_file_uri(r"C:\notes\phd%20application.md");
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(16).unwrap(), NonZeroU32::new(4).unwrap());
    session
        .feed(
            format!(
                "\x1b]8;;{target}\x1b\\\
                 phd-application-timeline-and-everything-else\x1b]8;;\x1b\\"
            )
            .as_bytes(),
        )
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();

    let head = frame
        .hyperlink_at(0, 2)
        .expect("the label's first row is linked");
    let tail = frame
        .hyperlink_at(2, 1)
        .expect("the label wrapped, and its continuation is the same link");
    assert_eq!(head, tail, "two segments of one run are one hit");
    assert_eq!(head.uri, target);

    let opened = HyperlinkActivation::Preview(host_path(r"C:\notes\phd application.md"), None);
    for hit in [&head, &tail] {
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                &hit.uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &|_| Some(a_local_file())
            ),
            opened,
            "either segment opens the one file the run names"
        );
    }
}

/// RED (F-SWEEP-2-048, owner ruling 2026-10-06) — **a declared web target whose host does not
/// parse is no declaration, and this window's own recogniser reads the text.**
///
/// The owner's line, as an agent printed it: an autolinker that trims only ASCII punctuation
/// declared `http://www.glancepc.com：` with OSC 8. From bytes, because what is under test is the
/// whole road — the vendor's cells, the capture, the recogniser — and only the terminal makes
/// it.
///
/// MUTATION: trust every declaration (drop the `web_host_parses` arm in
/// `CapturedRow::trim_program_url_hyperlink_spans`) and the hit is the declared target, colon and
/// all, with the program's id on it.
#[test]
fn a_declared_link_whose_host_does_not_parse_is_read_by_the_recogniser() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(60).unwrap(), NonZeroU32::new(4).unwrap());
    session
        .feed(
            "官网 \x1b]8;;http://www.glancepc.com：\x1b\\http://www.glancepc.com：\x1b]8;;\x1b\\见上"
                .as_bytes(),
        )
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    // `官网 ` is five cells, so the address starts at column 5.
    let hit = frame
        .hyperlink_at(0, 8)
        .expect("the visible address is a link");
    assert_eq!(
        hit.uri, "http://www.glancepc.com",
        "the recogniser's link, not the declared one"
    );
    assert_eq!(hit.id, None, "an inferred link, not a program's");
    // The full-width colon (columns 28 and 29) is prose again.
    assert_eq!(frame.hyperlink_at(0, 28), None);
}

/// RED (F-SWEEP-2-048) — **a declared web target with a host still wins**, label and all: the
/// regression half of the row above.
///
/// MUTATION: refuse every non-ASCII host in `bt_transcript::web_host_parses` and the program's
/// label `官网`, declaring an internationalised host, is no link at all.
#[test]
fn a_declared_link_whose_host_parses_is_the_programs_link() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(60).unwrap(), NonZeroU32::new(4).unwrap());
    session
        .feed("见 \x1b]8;;https://例子.测试/文档\x1b\\官网\x1b]8;;\x1b\\。".as_bytes())
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let hit = frame
        .hyperlink_at(0, 3)
        .expect("the program's label is its link");
    assert_eq!(hit.uri, "https://例子.测试/文档");
    assert!(hit.id.is_some(), "the program's own declaration");
}

/// **And P0 is not bought back.** With every pane beside the keyboard
/// silent, the gate is the one P0 installed, exactly: the focused frame's
/// own answer and nothing else.
#[test]
fn a_silent_sibling_leaves_the_unchanged_frame_gate_exactly_as_p0_left_it() {
    for focused_frame_unchanged in [false, true] {
        assert_eq!(
            pty_drain_says_nothing_new(focused_frame_unchanged, false),
            focused_frame_unchanged,
            "nothing else is speaking, so this is P0's question and no other"
        );
    }
    assert!(
        !pty_drain_says_nothing_new(true, true),
        "and a pane that has spoken is not answered by another pane's stillness"
    );
}

/// The same promise as a count, through the whole loop: once the sibling's
/// words are on the glass, turns in which nobody speaks cost no projection
/// and no present.
#[test]
fn turns_in_which_nobody_speaks_cost_nothing_in_a_split_window() {
    let mut harness = TwoPaneHarness::new(24, 6);
    harness.turn(b"prompt\r\n", b"first\r\n", pty_drain_says_nothing_new);
    harness.turn(b"", b"second\r\n", pty_drain_says_nothing_new);
    assert!(harness.sibling_shows("second"));
    let projections_before = harness.viewport_frames;
    let presents_before = harness.presents;

    for _ in 0..600 {
        harness.turn(b"", b"", pty_drain_says_nothing_new);
    }

    assert_eq!(harness.viewport_frames, projections_before);
    assert_eq!(harness.presents, presents_before);
}

/// RED (48) — **The drain never asks the desktop.**
///
/// `drain_pty` runs on every turn a shell speaks, and it used to open with the
/// place door and a `has_focus` call of its own, each under a drain station;
/// the reports show `drain_pty 0 ms (sample_window_place 1198 ms)`. It now
/// reads the turn's reading and names neither the door, the writer, the focus
/// call nor their stations.
///
/// MUTATION: put the `sample_window_place` call (or `Station::Place`) back in
/// `drain_pty` — red.
#[test]
fn the_drain_never_asks_the_desktop() {
    let drain = method_body("Runtime", "drain_pty");
    for asking in [
        "sample_window_place",
        "observe_window_place",
        "has_focus()",
        "Station::Place",
        "Station::PlaceFocus",
    ] {
        assert!(!drain.contains(asking), "the drain does not name {asking}");
    }
    assert!(
        drain.contains("let place = self.window.observed_place;"),
        "it reads the turn's reading"
    );
}

/// **PIN (user report, 2026-09-01) — a shell is never told the width of an icon.**
///
/// The report was a restored three-pane window whose middle pane held a TUI drawing itself
/// two to four columns wide inside a pane dozens of columns across. The recordings name the
/// cause without ambiguity: `BT_DPI stage=resized rect=-32000,-32000,-31686,-31950
/// inner_size=314x50`, twice in that one run, each time followed by the real
/// `inner_size=2880x1800` — a minimize and a restore. `-32000` is where Windows parks an
/// iconic window, and 314x50 is the client area of the icon. `Runtime::resize` refused a zero
/// extent and nothing else, so the whole of §4.2's chain ran on the icon's rectangle: the
/// seats were solved for a 314-pixel window, every pane's actor reflowed into the two columns
/// `CellMetrics::MIN_COLUMNS` floors at, and one quiet window later every pane's ConPTY was
/// told its shell was that wide.
///
/// What the user sees afterwards is this test's subject: a reflow is not an undo. Going back
/// up re-wraps at the real width the lines that were re-wrapped at three, and the pane comes
/// back full of two-character rows.
///
/// Red gate: drop the `minimized` term from [`resize_worth_solving`] — the shell is asked to
/// become five columns wide and the pane's own grid follows it down.
#[test]
fn minimizing_a_window_never_tells_its_shell_the_width_of_the_icon() {
    let start = Instant::now();
    // The window this session was restored into: 2880 physical pixels across three panes.
    let window = PhysicalSize::new(2880, 1800);
    // And the rectangle Win32 hands over with `SIZE_MINIMIZED`, off the same machine.
    let icon = PhysicalSize::new(314, 50);
    let mut harness = ResizeGateHarness::new(50, 50);
    harness.window_resized(window, false, start);
    harness.tick(start + WINDOW_RESIZE_QUIET);
    let settled = harness.grid;
    assert_eq!(settled, grid_of(50, 50));

    // Minimized, and left minimized for far longer than the coalescing window — the whole
    // point of the report is that the icon's rectangle had all the time it needed to be
    // believed.
    harness.window_resized(icon, true, start + Duration::from_secs(1));
    harness.tick(start + Duration::from_secs(2));
    assert_eq!(
        harness.grid, settled,
        "the pane's own actor reflowed to the icon's width"
    );

    // And back.
    harness.window_resized(window, false, start + Duration::from_secs(3));
    harness.tick(start + Duration::from_secs(4));
    assert_eq!(harness.grid, settled);
    assert_eq!(harness.conpty, settled);
    assert_eq!(
        harness.requests,
        Vec::new(),
        "a window that went away and came back the same size owes its shell nothing"
    );
}

/// **PIN (user report, 2026-09-01, third reading) — one gesture, one `ResizePseudoConsole`.**
///
/// The control group is what named this: the same squeeze-and-release over a pane running
/// `codex` leaves nothing behind, and over a pane running Claude Code it does. A final size
/// that never arrived would have broken both, so the final size arrives — and what is left to
/// account for is the *sequence*, which is where the two TUIs can differ.
///
/// A gesture is not a series of chosen sizes. It has exactly one chosen size, at the end;
/// every rectangle before that is somewhere the hand was passing through. The quiet window
/// alone cannot tell the two apart — it measures 200 ms of stillness, and a hand resting at
/// the far end of a divider's travel is still for as long as it likes — so a slow gesture
/// released a `ResizePseudoConsole` for every pause it contained, at whatever width the pane
/// happened to have, down to `CellMetrics::MIN_COLUMNS`.
///
/// This drives the shipping decision function over a drag that pauses three times on the way
/// in and once on the way out. The picture follows the hand throughout — that is the
/// 2026-08-06 ruling and this test reads it back on every step — and the child hears exactly
/// one size, the one the hand let go at.
///
/// Red gate: drop the `hand_on_the_geometry` term from `service_pending_pty_resize` and the
/// child is handed the whole tour, two columns and all.
#[test]
fn a_gesture_says_one_thing_to_the_shell_and_says_it_at_the_end() {
    let start = Instant::now();
    let mut harness = ResizeGateHarness::new(50, 50);
    harness.hand_down = true;

    // Four rests, each far longer than the quiet window, on the way in and back out. The
    // first of them is at `MIN_COLUMNS` — a divider really can be dragged that far.
    let tour = [
        (PhysicalSize::new(960, 1800), grid_of(16, 50)),
        (PhysicalSize::new(300, 1800), grid_of(5, 50)),
        (PhysicalSize::new(60, 1800), grid_of(2, 50)),
        (PhysicalSize::new(1500, 1800), grid_of(26, 50)),
    ];
    let mut at = start;
    for (physical, expected) in tour {
        harness.window_resized(physical, false, at);
        assert_eq!(
            harness.grid, expected,
            "the picture stopped following the hand at {physical:?}"
        );
        at += Duration::from_secs(1);
        harness.tick(at);
        assert_eq!(
            harness.requests,
            Vec::new(),
            "the shell was told about a width the hand was only passing through"
        );
        at += Duration::from_secs(1);
    }

    // The hand lets go where it means to — somewhere that is not where it started, because a
    // gesture that ends where it began owes the shell nothing at all and that is
    // `coalesce_pty_resize_on_grid_change`'s own rule, read back by
    // `a_pane_squeezed_narrow_and_let_go_leaves_its_shell_wide` ③.
    harness.window_resized(PhysicalSize::new(1920, 1800), false, at);
    harness.hand_down = false;
    harness.tick(at + WINDOW_RESIZE_QUIET);
    assert_eq!(
        harness.requests,
        vec![grid_of(33, 50)],
        "one gesture owes the shell exactly one size, and it is the last one"
    );
    assert_eq!(harness.conpty, grid_of(33, 50));
}

/// RED (T-RESTART-CWD, 2026-10-04) — **every verb that starts a shell in a
/// pane's place reads the one ladder**: `Restart shell`, every split (which
/// is `Duplicate pane` and `Split with`), and `Duplicate tab`.
///
/// Those verbs spawn a ConPTY and cannot run here, so this is their bodies, read
/// through `bt_source`.
///
/// Since 2026-10-05 the ladder is read with its kind (`LeafSession::seed_place_for_a_new_shell`,
/// which reads `place_for_a_new_shell`), so a pane born in a named folder hands it on as named.
///
/// MUTATION, observed red: put `leaf.session.working_directory()` back in any one
/// of the three bodies.
#[test]
fn every_verb_that_starts_a_shell_in_a_panes_place_reads_the_one_ladder() {
    for door in ["restart_shell", "split_seat", "duplicate_tab"] {
        let body = method_body("Runtime", door);
        assert!(
            body.contains("leaf.seed_place_for_a_new_shell()")
                && !body.contains("session.working_directory()"),
            "`{door}` reads where the pane stands through \
             `LeafSession::seed_place_for_a_new_shell`:\n{body}"
        );
    }
    // And the `+` and a picker row carry, whatever the pane under them was born in (coordinator's
    // ruling 2026-10-05; the review's unpinned clause). Its leaf half is pinned by
    // `a_pane_born_in_a_named_folder_starts_its_next_shells_there_whatever_the_profile_says`.
    // MUTATION, observed red: read `LeafSession::seed_place_for_a_new_shell` here instead.
    let beside = method_body("Runtime", "new_tab_with_profile");
    assert!(
        beside.contains("LeafSession::place_for_a_new_tab_beside")
            && !beside.contains("seed_place_for_a_new_shell"),
        "the `+` carries the pane's folder:\n{beside}"
    );
}

/// RED (confirmation review of `6049179a`, P1) — **a text selection drawn into
/// the strip is finished in its own pane.**
///
/// The selection's release was already answered before the cell lookup; it is
/// now one arm of the same owned release, ahead of every surface, and its
/// moves keep reading the origin pane's clamped cell.
///
/// Red gate: drop the `Local` arm of `release_owned_gesture` and the release
/// falls through to the surfaces and the cell lookup, which refuses the point,
/// leaving the route latched — the first assertion names it.
#[test]
fn a_text_selection_drawn_into_the_strip_is_finished_in_its_own_pane() {
    let release = squeezed_body("Runtime", "release_owned_gesture");
    assert!(
        release.contains(
            "ifself.live_paste_target(drag.owner).is_none(){self.window.mouse_route=None;returnOk(true);}self.finish_local_selection(*drag)?;Ok(true)}"
        ),
        "a selection's release finishes it, wherever the pointer is"
    );
    assert_eq!(
        reader_names(&calls_of("Runtime", "finish_local_selection")),
        ["release_owned_gesture"],
        "and that is the one place a selection is finished by a release"
    );
    let moved = squeezed_body("Runtime", "pointer_moved");
    assert!(
        moved.contains(
            "ifmatches!(self.window.mouse_route,Some(MouseRoute::Local(_))){returnself.extend_local_selection();}"
        ),
        "its moves go to its own pane"
    );
    assert!(
        squeezed_body("Runtime", "drag_hit_in_pane").contains("clamp_into_body(body,"),
        "clamped into that pane's body, so a point on the strip names its first row"
    );
}

/// **The ruling's own second sentence — 无会话 → 无状态点.**
///
/// "没有账本就不说话": a tab with no shell has nothing that can be failing,
/// nothing that can be waiting for an answer and nothing that can have gone
/// unread, so it wears no dot, no ring and no breath. This is not a rule the
/// slice writes — it is the aggregation §7.1.6b′ already borrows verbatim
/// from the strip, folding over an empty fleet — and that is exactly why it
/// is worth pinning: a later reader who "fixes" `fleet_claim`'s empty case
/// with a default other than `Silent` would put a dot on every folder tab in
/// the window.
///
/// Both faces are asked, because the ruling names both: the strip row and the
/// focus card are one `TabMarkState` by construction (`mark_state` is the
/// only builder), so this asserts the one thing both are drawn from.
///
/// Red gate: give the empty fold any claim but `Silent` and the dot appears;
/// let `fleet_working` answer `true` for an empty fleet and a folder tab
/// starts breathing at nothing.
#[test]
fn a_tab_with_no_shell_wears_no_dot_no_ring_and_no_breath() {
    let mut source = tab_with_a_files_column(1, "D:\\work\\folio");
    let column = source.seats.files()[0];
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        column,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("T5: a files column may become a tab of its own");

    let palette = bt_render::chrome_palette();
    for active in [false, true] {
        let mark = torn.mark_state(active, Instant::now(), Motion::Full, &palette);
        assert_eq!(mark.dot, None, "no ledger, no dot (active = {active})");
        assert!(mark.ring.is_none(), "and no ring");
        assert_eq!(
            mark.opacity, 1.0,
            "and no breath — the breath is a shell working, and there is none"
        );
    }
    assert!(
        !torn.fleet_working(),
        "an empty fleet is not a fleet that is busy"
    );
    assert!(torn.fleet_progress().is_none());
}

/// A card's subject for `path`, raised over `host` — the two hosts differ in
/// exactly the field that says where the path came from.
fn glance_subject(path: &Path, host: RowHost) -> FilePeekSubject {
    FilePeekSubject {
        path: Some(path.to_path_buf()),
        printed_in: match host {
            RowHost::Terminal(seat) => Some(seat),
            RowHost::Column(_) | RowHost::Float(_) | RowHost::Git(_) => None,
        },
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        ftype: preview::PreviewFtype::Markdown,
        refused: false,
        dirty: false,
        anchor: file_peek::PeekAnchor::row([40.0, 300.0, 240.0, 320.0]),
    }
}

/// RED (ticket 12, user ruling 2026-09-20) — **the glance card's foot is one
/// writing for a files row and a terminal reference: the same file gives the
/// same folder.**
///
/// The ruling says it in so many words: 「文件列与终端引用两处同一行不分两种写法」. The
/// two hosts differ in where the path came from — a program printed one, the
/// user's own tree listed the other — and that difference decides how the card
/// reads its facts (audit 3 C-2). It must decide nothing about the foot: the
/// foot is the path's parent, and a path has one parent.
///
/// MUTATION: have `FilePeekSubject::foot_address_on` print the file's own path
/// when `printed_in` is set — the two feet part, and the equality goes red.
#[test]
fn the_foot_is_one_writing_for_rows_and_references() {
    let (folder, file) = glance_fixture("one-writing");
    let platform = bt_platform::host_platform();
    let row = glance_subject(&file, RowHost::Column(SeatId(1))).foot_address_on(platform, None);
    let reference =
        glance_subject(&file, RowHost::Terminal(SeatId(2))).foot_address_on(platform, None);
    assert_eq!(
        row, reference,
        "one file, one foot, whichever surface raised the card"
    );
    assert_eq!(
        row,
        folder.join("notes").display().to_string(),
        "and the foot is the folder that holds the file"
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// RED (ticket 32) — **the first mark reserves the rail's room once, and the
/// alternate screen keeps it.**
///
/// A width that followed the per-frame rail would resize the shell whenever a
/// mark arrived and whenever `vim` entered or left the alternate screen, where
/// [`cmdrail::host_rect`] hides the rail — a reflow of the TUI because it
/// started. So `LeafSession::has_rail` turns on at the ledger's first mark and
/// never turns back: a leaf fed its first `OSC 133` schedules exactly one grid
/// change, and entering and leaving the alternate screen afterwards schedule
/// none. Driven through the real session, the real `hear_first_mark` the drain
/// asks and the real `schedule_leaf_grid_change` every solve goes through.
///
/// MUTATION: make `LeafSession::grid_for` pass
/// `self.has_rail && !self.session.terminal_modes().alternate_screen` — red at
/// the alternate screen's entry; or drop `leaf.hear_first_mark()` from
/// `drain_leaf_pty` — red on the pin at the end.
#[test]
fn the_first_mark_reserves_the_room_once_and_the_alternate_screen_keeps_it() {
    let mut fonts = bt_render::preview_measure_font_system();
    let metrics = bt_render::CellMetrics::measure(&mut fonts, 1.0).unwrap();
    let body = rail_test_body(797, 1.0);
    let physical = PhysicalSize::new(body.width, body.height);
    let unreserved = metrics.grid_for_pixels(body.width, body.height);
    let mut leaf = leaf_saying("");
    apply_leaf_metrics(&mut leaf, metrics);
    let mut heard = 0;
    let mut step = |leaf: &mut LeafSession, bytes: &str| -> bool {
        leaf.session
            .feed(bytes.as_bytes())
            .expect("feed the shell's bytes");
        if leaf.hear_first_mark() {
            heard += 1;
        }
        let next = leaf.grid_for(body);
        schedule_leaf_grid_change(
            leaf,
            next,
            physical,
            Instant::now(),
            LeafOnStage::Shown,
            "ticket 32",
            card_trace::Pane::untraced(),
        )
        .unwrap()
    };
    // Born into the pane at the grid it has always had.
    step(&mut leaf, "");
    assert_eq!(leaf.grid, unreserved);
    assert!(
        !step(&mut leaf, "a banner, before any prompt\r\n"),
        "output without a mark is not a rail"
    );
    assert!(
        step(&mut leaf, RAIL_FAILED_THEN_PROMPT),
        "the first mark makes room for the rail"
    );
    let reserved = leaf.grid;
    assert!(reserved.columns < unreserved.columns);
    assert!(
        !step(
            &mut leaf,
            "\u{1b}]133;C\u{7}ok\r\n\u{1b}]133;D;0\u{7}\u{1b}]133;A\u{7}PS> "
        ),
        "later marks change nothing"
    );
    assert!(
        !step(&mut leaf, "\u{1b}[?1049hthe editor's canvas"),
        "entering the alternate screen keeps the room"
    );
    assert!(
        leaf.session.terminal_modes().alternate_screen,
        "the fixture really is on the alternate screen"
    );
    assert!(
        !step(&mut leaf, "\u{1b}[?1049l"),
        "leaving it keeps the room too"
    );
    assert_eq!(leaf.grid, reserved);
    assert_eq!(heard, 1, "the first mark is heard once per pane lifetime");

    // And the drain is where it is heard, and a heard mark is carried by a solve.
    assert!(
        squeezed(free_fn_body("drain_leaf_pty")).contains("letrail_began=leaf.hear_first_mark();"),
        "every leaf's bytes pass the drain, so the drain is the fact's one owner"
    );
    assert!(
        squeezed_body("Runtime", "drain_pty")
            .contains("ifrail_began{self.resize_leaves_to_layout("),
        "a pane that earned its rail is re-solved on the same turn"
    );
}
