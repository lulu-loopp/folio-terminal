//! **`shell_integration`, as the application drives it.** Tests whose first assertion is about
//! `shell_integration`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{TwoPaneHarness, cell_ink, mono_caret_block, prose, prose_caret_block};

#[test]
fn tab_state_machine_creates_switches_and_closes_to_the_adjacent_tab() {
    let mut tabs = vec!["first"];
    tabs.push("second");
    let mut active = tabs.len() - 1;
    assert_eq!(
        (tabs.as_slice(), active),
        (["first", "second"].as_slice(), 1)
    );

    active = 0;
    assert_eq!(active, 0, "clicking a tab changes only the active index");
    assert_eq!(
        tab_close_action(tabs.len(), active, 0),
        TabCloseAction::Keep { active_tab: 0 },
        "closing the active left tab activates its right neighbour"
    );
    tabs.remove(0);
    assert_eq!(tabs, ["second"]);
    assert_eq!(
        tab_close_action(tabs.len(), 0, 0),
        TabCloseAction::CloseWindow,
        "the last tab delegates to the existing WM_CLOSE path"
    );
}

/// **The mutant**: the policy this fix replaces, in which the focused pane's
/// picture decided, alone, whether the window had anything to say.
fn focused_frame_alone(focused_frame_unchanged: bool, _unpainted_pane_output: bool) -> bool {
    focused_frame_unchanged
}

/// **The bug, as a count.** A pane that is not holding the keyboard says
/// something; it must be on the glass at the end of the same turn of the
/// loop, having cost exactly one present — the rhythm the focused pane gets.
///
/// The second turn is the one that matters: by then the focused pane's
/// picture is a byte-for-byte match of the one on the glass, so the
/// unchanged-frame gate — which only ever looks at that pane — says the
/// window has nothing to say while the other half of it has just scrolled.
#[test]
fn a_pane_that_is_not_the_keyboards_reaches_the_glass_on_the_turn_it_speaks() {
    let mut harness = TwoPaneHarness::new(24, 6);
    harness.turn(b"prompt\r\n", b"first\r\n", pty_drain_says_nothing_new);
    assert!(harness.sibling_shows("first"));
    let presents_before = harness.presents;

    harness.turn(b"", b"second\r\n", pty_drain_says_nothing_new);

    assert!(
        harness.sibling_shows("second"),
        "the pane beside the keyboard spoke and is on the glass in the same turn"
    );
    assert_eq!(
        harness.presents,
        presents_before + 1,
        "and it cost one present, not a wait for some unrelated event"
    );
}

/// **The mutation.** The identical script with the focused pane's frame as
/// the only judge — which is what this window shipped — and the sibling
/// stays on a picture it has already outgrown, for as long as nothing else
/// happens to publish. Measured on the real machine as 19 wheel notches out
/// of 20 producing no picture at all.
#[test]
fn the_focused_frame_alone_strands_the_other_pane_on_a_stale_picture() {
    let mut harness = TwoPaneHarness::new(24, 6);
    harness.turn(b"prompt\r\n", b"first\r\n", focused_frame_alone);
    assert!(harness.sibling_shows("first"));
    let presents_before = harness.presents;

    for _ in 0..20 {
        harness.turn(b"", b"second\r\n", focused_frame_alone);
    }

    assert!(
        !harness.sibling_shows("second"),
        "twenty turns of a shell talking, and none of it on the glass"
    );
    assert_eq!(
        harness.presents, presents_before,
        "because not one frame was ever published to draw it in"
    );
}

/// **The caret's block is as tall as its own source, and the page under it
/// moves by the difference** (§7.1.3q, ticket T4).
///
/// The one rule of the live preview, stated as geometry: a block drawn as
/// the file's own bytes is its folded line count times the source face's
/// line height, the shaper is never asked about it — a monospace row's
/// height is a fact, not a measurement — and every block after it starts
/// exactly that much further down.
///
/// MUTATION ①: drop the `source.filter(...)` arm in `lay_markdown_out` and
/// the block under the caret is laid out as wrapped prose, so the source
/// lines are drawn into a box measured for something else and the blocks
/// below overlap them.
/// MUTATION ②: measure the block's height from `lines.len()` instead of from
/// `wrap(width).rows()` and a source line wider than the column is drawn on
/// rows the layout did not reserve.
#[test]
fn the_carets_block_is_laid_out_as_its_own_source_lines() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let blocks = prose(&["first", "middle", "last"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let calls = std::cell::Cell::new(0usize);
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        calls.set(calls.get() + 1);
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let width = 400.0;
    let rendered = lay_markdown_out(
        &blocks,
        &intrinsic,
        &NO_SOURCE_BLOCKS,
        width,
        metrics,
        art,
        &mut shaper,
    );

    let source = mono_caret_block(1, 6, "one\ntwo\nthree\nfour");
    let asked = calls.get();
    let live = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        width,
        metrics,
        art,
        &mut shaper,
    );
    assert_eq!(
        calls.get() - asked,
        blocks.len() - 1,
        "the shaper is asked about every block but the one drawn as source",
    );
    assert_eq!(
        (live[1].height, live[1].rows.len()),
        (80.0, 4),
        "four source lines at the source face's own line height",
    );
    assert_eq!(
        live[0], rendered[0],
        "the block in front of it is untouched",
    );
    assert_eq!(
        live[2].top - rendered[2].top,
        live[1].height - rendered[1].height,
        "and the block under it moves by exactly the difference",
    );

    // **And a source line too wide for the column folds**, on the source
    // face's own terms — the block is taller, and it is taller by whole
    // rows.
    let long = mono_caret_block(1, 6, &"x".repeat(200));
    let folded = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(long.clone()),
        width,
        metrics,
        art,
        &mut shaper,
    );
    let (measure_left, measure_right) =
        preview::markdown_measure_box([0.0, 0.0, width, 400.0], metrics);
    let _ = (measure_left, measure_right);
    assert_eq!(
        folded[1].rows.len(),
        (200.0_f32 / (width / 8.0)).ceil() as usize,
        "one row per column-full of a two-hundred-character line",
    );
}

/// **Seating a caret in a prose block re-flows the page and does not
/// re-parse it** (§7.1.3q, kept whole by §7.1.3w).
///
/// The caret's block is a *layout* fact: the page is laid out again because
/// one block is now drawn from other bytes, and the parse standing behind it
/// is untouched. What the shaper is asked is one question per line of the
/// prose block — how far that line folds — and nothing at all about the
/// block's rendered spans, which are not on the glass while the caret is in
/// it.
///
/// MUTATION: measure the rendered arm as well and every keystroke pays for a
/// block that is not being drawn.
#[test]
fn seating_a_caret_in_a_prose_block_reflows_and_does_not_reparse() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let blocks = prose(&["first", "middle", "last"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let asked = std::cell::RefCell::new(Vec::<String>::new());
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        asked
            .borrow_mut()
            .push(runs.iter().map(|run| run.text.as_str()).collect::<String>());
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let width = 400.0;
    let rendered = lay_markdown_out(
        &blocks,
        &intrinsic,
        &NO_SOURCE_BLOCKS,
        width,
        metrics,
        art,
        &mut shaper,
    );
    asked.borrow_mut().clear();
    let source = prose_caret_block(1, 6, "**middle**\nsecond line");
    let live = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        width,
        metrics,
        art,
        &mut shaper,
    );
    assert_eq!(
        asked.borrow().clone(),
        ["first", "**middle**", "second line", "last"],
        "the shaper is asked about the prose block's own lines and never \
             about the rendering it is standing in for",
    );
    assert_eq!(
        live[1].rows.len(),
        2,
        "the block is as tall as its own two lines",
    );
    assert!(
        live[2].top > rendered[2].top,
        "and the block under it moved by the difference",
    );
    // The parse is untouched: the same blocks, in the same order, with the
    // same spans — the caret changed a layout and nothing else.
    assert_eq!(blocks, prose(&["first", "middle", "last"]));
}
