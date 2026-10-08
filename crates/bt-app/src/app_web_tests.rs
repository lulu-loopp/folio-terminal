//! **The crate root: web seats.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::squeezed_body;

/// RED — **a page a modal covers is drawn as the frame it last stood on the
/// glass with** (§7.8 ⑩, user report on `next22`, 缺陷 #203: a pane holding a
/// pdf or a web page went blank behind the settings dialog).
///
/// A page is not drawn by this window — it is a DirectComposition visual
/// under the swapchain's — so when a modal takes it off the glass there is
/// nothing of it left anywhere except the photograph `web_thumb` keeps. Four
/// statements carry that photograph to the pane, and **breaking any one of
/// them is the blank pane verbatim**:
///
/// * `sync_web_page` is the one walk that knows all five reasons a page can
///   be off the glass, so it is where the page the modal *alone* took is
///   named;
/// * `keep_what_the_modal_covers` photographs every page on the glass — on
///   the clock, whether or not a card is looking — and asks for the one
///   decode a dialog needs, because a hidden WebView never answers a
///   capture and the ask therefore has to have happened *before*;
/// * `refresh_chrome` puts the frame down the chrome pass's own textured-quad
///   channel, which is under every overlay layer and therefore under the
///   scrim;
/// * `preview_float_layer` does the same for a page a float is carrying, on
///   that window's own layer.
///
/// The value half is the arithmetic that decides which page gets one: the
/// same predicate the presence answer is made of, asked a second time with
/// no modal standing over it.
///
/// RED GATE: drop the `icons.extend(self.page_keepsake_icons())` — which is
/// the build as it shipped — and the third assertion fails; the pane then
/// draws its own ground, which is exactly what the report is a photograph
/// of.
#[test]
fn a_page_a_modal_covers_is_drawn_as_a_kept_frame() {
    // ① Only the modal mints a keepsake. A page hidden for any of the other
    // four reasons has something else to draw, or nobody looking at it.
    assert!(
        !a_page_is_off_the_glass(false, false, true, false, false),
        "a page in the front tab with no card and no source face is on the \
             glass, and that is the one kind a dialog takes away"
    );
    assert!(a_page_is_off_the_glass(true, false, true, false, false));
    for (floated, in_front, carded, sourced) in [
        (false, false, false, false),
        (false, true, true, false),
        (false, true, false, true),
    ] {
        assert!(
            a_page_is_off_the_glass(false, floated, in_front, carded, sourced),
            "this page is off the glass with no dialog open at all, so the \
                 modal is not what took it and it is owed no standing-in frame"
        );
    }

    // ② The walk that names them, and the pass that keeps them.
    let sync = squeezed_body("Runtime", "sync_web_page");
    assert!(
        sync.contains("keepsakes.push(PageKeepsake{"),
        "the one walk that knows all five reasons is where the page a modal \
             alone took is named:\n{sync}"
    );
    assert!(
        sync.contains("self.keep_what_the_modal_covers(keepsakes,now);"),
        "and it hands them on:\n{sync}"
    );
    let keeping = squeezed_body("Runtime", "keep_what_the_modal_covers");
    assert!(
        keeping.contains("self.photograph_pages(demands,now);"),
        "every page on the glass is photographed on the clock — a hidden \
             WebView never answers, so the ask has to be older than the \
             dialog:\n{keeping}"
    );
    assert!(
        keeping.contains("self.window.web_thumbs.frame_job(keepsake.leaf)"),
        "and the dialog asks for the one decode it needs:\n{keeping}"
    );
    assert!(
        keeping.contains("self.window.web_thumbs.drop_frames();"),
        "and lets the pixels go when it comes down:\n{keeping}"
    );

    // ③ The two places a kept frame reaches the glass.
    let chrome = squeezed_body("Runtime", "refresh_chrome_with_overlay");
    assert!(
        chrome.contains("icons.extend(self.page_keepsake_icons());"),
        "a docked pane draws its page's last frame in the chrome pass, \
             under every overlay and therefore under the scrim:\n{chrome}"
    );
    let float = squeezed_body("Runtime", "preview_float_layer");
    assert!(
        float.contains("self.float_page_keepsake_icon(id)"),
        "and a page a float is carrying draws it on that window's own \
             layer, for the reason its picture already does:\n{float}"
    );
}

/// RED ④ — **a retirement asks for one frame, not one per turn** (§7.10 ④‴).
///
/// The freeze beside the blank page. A page whose pane has gone owes the
/// glass one frame, because the hole it was seen through was cut while a
/// frame was composed and the retirement happens after that frame. But
/// `WebSeat::close` is idempotent and the wait for the browser process runs
/// to ten seconds, so the set of orphaned pages stays non-empty for the
/// whole wait — and asking for that frame off the *set* rather than off the
/// *event* is a window that goes round its own loop as fast as it can for
/// ten seconds: the funnel requests a redraw whether or not it found a
/// picture, the redraw is answered at the tail of the same turn, and the
/// next turn asks again.
///
/// RED GATE: ask the question of `!orphaned.is_empty()` — the shipped build
/// — and the last assertion goes red. The three above it are the rule
/// itself: a page already told to go is not retiring again.
#[test]
fn a_retirement_asks_for_one_frame_and_not_one_per_turn() {
    assert!(
        !a_retirement_happens_on_this_turn(std::iter::empty()),
        "a window with nothing orphaned owes no frame at all"
    );
    assert!(
        a_retirement_happens_on_this_turn([false].into_iter()),
        "the turn a page is first found orphaned is the turn it retires on"
    );
    assert!(
        !a_retirement_happens_on_this_turn([true].into_iter()),
        "and every turn after it is a page that has already been told, \
             waiting for a process to exit — nothing on the glass is changing"
    );
    assert!(
        a_retirement_happens_on_this_turn([true, false].into_iter()),
        "a second page going while the first is still leaving owes its own frame"
    );

    let clock = squeezed_body("Runtime", "advance_web_page");
    assert!(
        clock.contains("ifretiring{self.present_chrome_change()?;}"),
        "the frame is owed by the retirement and not by the wait:\n{clock}"
    );
    assert!(
        !clock.contains("if!orphaned.is_empty(){"),
        "asking the wait for it is ten seconds of a window pinned at a core \
             with nothing on the glass changing:\n{clock}"
    );
}
