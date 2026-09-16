//! **Math on a screen a multiplexer has framed.**
//!
//! Every screen here is one a byte capture proved a real program draws. `herdr` 0.8.2 repaints the
//! host alternate screen itself and puts a 26-column sidebar in front of every pane row — twenty
//! five blank cells then `│` (U+2502), pane text from column 26 — so the line a detector reads is
//! `<25 spaces>│$$`. Its "compact" sidebar is the same picture three columns narrower: `   │$$`,
//! pane text from column 4. A `tmux` vertical split draws the same shape with text on both sides
//! of the rule. The captures those numbers come from are `herdr_client.bin` and `herdr_compact.bin`
//! (100×40, `TERM=xterm-256color`), decoded to the screen grid; the pane content in all of them is
//! the same thirteen lines, reproduced below.

use bt_detect::{
    DetectionOptions, InlineMathSite, MathMode, RegionMathBlocks, ScreenRegion,
    detect_math_blocks_with_sites, detect_math_blocks_with_sites_in_regions,
};
use bt_transcript::TranscriptId;

/// The thirteen content lines of the pane, exactly as the program printed them: one inline formula
/// and two display blocks.
const PANE: [&str; 13] = [
    "Inline: $e^{i\\pi}+1=0$ stays inline.",
    "",
    "$$",
    "\\frac{1}{2}",
    "$$",
    "",
    "$$",
    "\\begin{pmatrix}",
    "a & b \\\\",
    "c & d \\\\",
    "e & f",
    "\\end{pmatrix}",
    "$$",
];

/// The screen a frame of `left` cells, then a rule, then a pane row, spells.
fn framed(left: &str, rule: char, pane: &[&str]) -> Vec<String> {
    pane.iter()
        .map(|text| format!("{left}{rule}{text}"))
        .collect()
}

fn regions(rows: &[String]) -> Vec<RegionMathBlocks> {
    detect_math_blocks_with_sites_in_regions(
        rows.iter().enumerate().map(|(index, text)| {
            (
                TranscriptId(index as u64 + 1),
                text.as_str(),
                InlineMathSite::AltScreenContent,
            )
        }),
        DetectionOptions::default(),
    )
}

/// The one region of this screen that proved anything, and its column offset.
fn only_detecting_region(found: &[RegionMathBlocks]) -> (u32, usize, usize) {
    let detecting = found
        .iter()
        .filter(|region| !region.blocks.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(detecting.len(), 1, "exactly one region holds the math");
    let region = detecting[0];
    let inline = region
        .blocks
        .iter()
        .filter(|block| block.span.mode == MathMode::Inline)
        .count();
    let display = region
        .blocks
        .iter()
        .filter(|block| block.span.mode == MathMode::Display)
        .count();
    (region.region.column_start, inline, display)
}

#[test]
fn the_herdr_sidebar_no_longer_hides_the_pane() {
    let screen = framed(&" ".repeat(25), '│', &PANE);
    let found = regions(&screen);
    assert_eq!(
        found.iter().map(|region| region.region).collect::<Vec<_>>(),
        vec![
            ScreenRegion {
                column_start: 0,
                column_end: Some(25)
            },
            ScreenRegion {
                column_start: 26,
                column_end: None
            },
        ],
        "the sidebar is a region of its own, left of the rule"
    );
    assert_eq!(only_detecting_region(&found), (26, 1, 2));
}

#[test]
fn the_herdr_compact_sidebar_no_longer_hides_the_pane() {
    let screen = framed("   ", '│', &PANE);
    let found = regions(&screen);
    assert_eq!(only_detecting_region(&found), (4, 1, 2));
}

#[test]
fn a_plain_screen_detects_exactly_what_it_always_did() {
    let screen = PANE
        .iter()
        .map(|text| (*text).to_owned())
        .collect::<Vec<_>>();
    let found = regions(&screen);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].region, ScreenRegion::WHOLE);
    let plain = detect_math_blocks_with_sites(
        screen.iter().enumerate().map(|(index, text)| {
            (
                TranscriptId(index as u64 + 1),
                text.as_str(),
                InlineMathSite::AltScreenContent,
            )
        }),
        DetectionOptions::default(),
    );
    assert_eq!(found[0].blocks, plain);
    assert_eq!(plain.len(), 3);
}

#[test]
fn a_tmux_split_typesets_the_right_pane_and_nothing_in_the_left() {
    // A vertical split fifty columns wide: an ordinary shell session on the left, the pane with the
    // math on the right. A multiplexer pads and clips every row to the pane width, so the rule
    // stands in the same column on all of them.
    const PROSE: [&str; 13] = [
        "the left pane is an ordinary shell session",
        "$ ls -l",
        "total 24",
        "-rw-r--r--  1 user  staff   120 notes.md",
        "-rw-r--r--  1 user  staff  2048 report.pdf",
        "$ echo \"costs $5 and $10\"",
        "costs $5 and $10",
        "$ git status",
        "On branch main",
        "nothing to commit, working tree clean",
        "$ uname -a",
        "Darwin 25.0.0 arm64",
        "$",
    ];
    let screen = PANE
        .iter()
        .zip(PROSE)
        .map(|(math, prose)| {
            assert!(prose.len() <= 50, "the left pane is fifty columns wide");
            format!("{prose:<50}│{math}")
        })
        .collect::<Vec<_>>();
    let found = regions(&screen);
    assert_eq!(
        found.iter().map(|region| region.region).collect::<Vec<_>>(),
        vec![
            ScreenRegion {
                column_start: 0,
                column_end: Some(50)
            },
            ScreenRegion {
                column_start: 51,
                column_end: None
            },
        ]
    );
    assert_eq!(only_detecting_region(&found), (51, 1, 2));
}

#[test]
fn prose_left_of_the_rule_still_splits_the_screen() {
    let screen = PANE
        .iter()
        .enumerate()
        .map(|(index, math)| {
            let prose = format!("line {index} of an ordinary log");
            format!("{prose:<32}│{math}")
        })
        .collect::<Vec<_>>();
    let found = regions(&screen);
    assert_eq!(found.len(), 2);
    assert!(found[0].blocks.is_empty(), "the log proves nothing");
    assert_eq!(only_detecting_region(&found), (33, 1, 2));
}

/// **A fence the screen proves suppresses every region.** Thirty-eight rows of pipe-aligned text
/// between two fences are a code block, and the screen says so. Cut into columns, the side the
/// fences were never printed in begins from a neutral state and reads all thirty-eight as formulas.
/// The fence is therefore proven once, on the unsliced rows, and no region may detect on a row it
/// covers.
#[test]
fn a_fence_the_screen_proves_suppresses_every_region() {
    let mut screen = vec!["```".to_owned()];
    screen.extend(std::iter::repeat_n("log  \u{2502} $x^2$".to_owned(), 38));
    screen.push("```".to_owned());
    let plain = detect_math_blocks_with_sites(
        screen.iter().enumerate().map(|(index, text)| {
            (
                TranscriptId(index as u64 + 1),
                text.as_str(),
                InlineMathSite::AltScreenContent,
            )
        }),
        DetectionOptions::default(),
    );
    assert!(plain.is_empty(), "the screen proves this is code");
    let found = regions(&screen);
    assert_eq!(found.len(), 2, "the rule still cuts the screen");
    assert_eq!(
        found
            .iter()
            .map(|region| region.blocks.len())
            .sum::<usize>(),
        0,
        "and no region may detect inside the fence"
    );
}

#[test]
fn a_table_drawn_inside_a_tui_never_splits_the_screen() {
    // Five rows of a boxed table on a forty-row screen. Its rule reaches an eighth of the screen,
    // nowhere near the nine tenths a frame reaches, so the screen stays one region.
    let mut screen = (0..35)
        .map(|index| format!("output line {index}"))
        .collect::<Vec<_>>();
    screen.extend((0..5).map(|index| format!("name {index}    │ value {index}")));
    let found = regions(&screen);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].region, ScreenRegion::WHOLE);
}
