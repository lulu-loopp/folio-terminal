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
    detect_math_blocks_with_sites, detect_math_blocks_with_sites_in_regions, find_border_columns,
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

/// **A fence a pane prints is that pane's.** The left pane cats a Markdown file with a code block
/// in it, on rows the frame runs through. Its own region refuses those rows exactly as it always
/// has; the pane across the rule is an independent program that never printed a backtick, and
/// silencing it for four rows of somebody else's output would be the veto over-reaching.
#[test]
fn a_fence_one_pane_prints_leaves_the_other_pane_alone() {
    let mut screen = vec!["log  \u{2502} $x^2$".to_owned(); 40];
    screen[5] = "```  \u{2502} $x^2$".to_owned();
    screen[9] = "```  \u{2502} $x^2$".to_owned();
    let found = regions(&screen);
    assert_eq!(found.len(), 2);
    let right = found
        .iter()
        .find(|region| region.region.column_start == 6)
        .expect("the right pane");
    assert_eq!(
        right.blocks.len(),
        40,
        "every row of the other pane detects"
    );
    let left = found
        .iter()
        .find(|region| region.region.column_start == 0)
        .expect("the left pane");
    assert!(
        left.blocks.is_empty(),
        "the pane that printed the fence still honours it"
    );
}

/// **A table drawn with box glyphs on every row splits into its cells, and that is harmless.** Its
/// separator rows carry junctions, which continue the vertical line, so each column becomes a
/// region — and a cell's math is detected inside its own region exactly as it was detected inside
/// the whole screen. Cutting a table along the lines it was drawn with takes nothing apart.
#[test]
fn a_full_screen_table_splits_into_its_cells_without_losing_their_math() {
    let mut screen = vec!["\u{2502} name  \u{2502} $x^2$    \u{2502}".to_owned(); 40];
    screen[0] = "\u{250c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{252c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}".to_owned();
    screen[2] = "\u{251c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{253c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2524}".to_owned();
    screen[39] = "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2534}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}".to_owned();
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
    let found = regions(&screen);
    assert_eq!(found.len(), 3, "the cells are the regions");
    assert_eq!(
        found
            .iter()
            .map(|region| region.blocks.len())
            .sum::<usize>(),
        plain.len(),
        "every formula the whole screen proves, the cells prove too"
    );
    assert_eq!(plain.len(), 37);
}

/// Every formula this screen proves whole, its regions prove too.
fn keeps_every_formula(screen: &[String]) -> usize {
    let lines = || {
        screen.iter().enumerate().map(|(index, text)| {
            (
                TranscriptId(index as u64 + 1),
                text.as_str(),
                InlineMathSite::AltScreenContent,
            )
        })
    };
    let whole = detect_math_blocks_with_sites(lines(), DetectionOptions::default());
    let regional = detect_math_blocks_with_sites_in_regions(lines(), DetectionOptions::default())
        .into_iter()
        .flat_map(|region| region.blocks)
        .collect::<Vec<_>>();
    assert_eq!(
        regional.len(),
        whole.len(),
        "the screen proves {} formulas and its regions prove {}",
        whole.len(),
        regional.len()
    );
    whole.len()
}

/// **No dollar does not mean no formula** (owner's ruling 2026-09-17). `\[x^2\]` is display math
/// the scanner supports and carries no `$` at all, so a test that looked for one let the edge
/// exemption be spent on it and the cut took it apart. The exemption now asks the grammar.
#[test]
fn a_dollar_free_formula_at_an_edge_keeps_the_screen_whole() {
    for edge in [0, 39] {
        let mut screen = vec!["log  \u{2502} text".to_owned(); 40];
        screen[edge] = "\\[x^2\\]".to_owned();
        assert_eq!(
            find_border_columns(screen.iter().map(String::as_str)),
            Vec::<u32>::new(),
            "the row at {edge} carries a formula and spends no exemption"
        );
        assert_eq!(keeps_every_formula(&screen), 1);
    }
    // An environment opener is a delimiter too, even standing alone on its row.
    let mut screen = vec!["log  \u{2502} text".to_owned(); 40];
    screen[39] = "\\begin{pmatrix}".to_owned();
    assert_eq!(
        find_border_columns(screen.iter().map(String::as_str)),
        Vec::<u32>::new()
    );
}

/// **A formula's own blanks are not a gap** (owner's ruling 2026-09-17). `$$x   +y$$` is one
/// formula with three spaces in the middle of it: the column lands on a space, the cell to its left
/// is a space too, and a test that asks only about the two neighbouring cells sees clearance and
/// cuts the formula in half. The row's proven spans are now asked as well.
#[test]
fn a_cut_never_runs_through_a_formulas_own_blanks() {
    let mut screen = vec!["log  \u{2502} text".to_owned(); 40];
    screen[20] = "$$x   +y$$".to_owned();
    assert_eq!(
        find_border_columns(screen.iter().map(String::as_str)),
        Vec::<u32>::new(),
        "column five stands inside the formula, blank or not"
    );
    assert_eq!(keeps_every_formula(&screen), 1);
}

/// The same defect in the shape it arrives in: a table whose inner rule is missing on the one row
/// where a formula occupies a merged cell across it. The outer rules are intact on every row and
/// still cut the screen; the inner column is not a rule on row 20 and no longer pretends to be.
#[test]
fn a_table_is_not_cut_through_the_cell_a_formula_merged() {
    let mut screen = vec!["\u{2502} a  \u{2502} text    \u{2502}".to_owned(); 40];
    screen[20] = "\u{2502}$x   +y$      \u{2502}".to_owned();
    assert_eq!(
        find_border_columns(screen.iter().map(String::as_str)),
        vec![0, 15],
        "the intact outer rules stand; the missing inner one does not"
    );
    assert_eq!(keeps_every_formula(&screen), 1);
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
