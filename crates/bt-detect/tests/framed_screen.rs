//! **Math on a screen a multiplexer has framed** (ticket 69a, T-PANE-COLUMNS; the design note
//! `docs/plans/design/pane-columns-2026-09-29.md`).
#![allow(clippy::disallowed_methods)]

use std::sync::Arc;

use bt_detect::{
    BlockKind, DelimiterKind, DetectionContext, DetectionOptions, DetectionRevision,
    GridGeneration, GridPoint, InlineMathSite, LayoutKey, LiveCapture, LiveDetectionInput,
    LiveDetectionSource, LiveDetectionTask, MathMode, MathSpan, PaneRect, SUBPIXELS_PER_PX,
    ScreenId, advance_detection_context, live_detection_isolation_gap,
    live_detection_ownership_ledger, resolve_live_detection_task, resolve_live_detection_tasks,
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

/// `(byte, cell)` boundaries for one captured row, in the shape the terminal's capture hands the
/// detector: one boundary at every cluster edge, cells from `bt_unicode` cluster widths.
fn boundaries(text: &str) -> Vec<(u32, u32)> {
    let mut out = vec![(0, 0)];
    let (mut byte, mut cell) = (0u32, 0u32);
    for cluster in bt_unicode::graphemes(text) {
        byte += cluster.len() as u32;
        cell += bt_unicode::cluster_width(cluster) as u32;
        out.push((byte, cell));
    }
    out
}

/// One screen of the corpus: a frozen history tail, the grid rows, and the checkpoint and options
/// they are scanned under. `continues[i]` is the soft-wrap flag of grid row `i`.
struct Screen {
    name: &'static str,
    width: u32,
    history: Vec<String>,
    grid: Vec<String>,
    continues: Vec<bool>,
    site: InlineMathSite,
    initial_context: DetectionContext,
    options: DetectionOptions,
}

impl Screen {
    fn alt(name: &'static str, width: u32, grid: Vec<String>) -> Self {
        let continues = vec![false; grid.len()];
        Self {
            name,
            width,
            history: Vec::new(),
            grid,
            continues,
            site: InlineMathSite::AltScreenContent,
            initial_context: DetectionContext::default(),
            options: DetectionOptions::default(),
        }
    }

    fn inputs(&self) -> Vec<LiveDetectionInput> {
        let history = self
            .history
            .iter()
            .enumerate()
            .map(|(index, text)| LiveDetectionInput {
                source: LiveDetectionSource::History {
                    id: TranscriptId(100 + index as u64),
                },
                text: text.clone(),
                continues: false,
                captured_columns: self.width,
                cell_boundaries: boundaries(text),
                site: self.site,
            });
        let grid = self.grid.iter().enumerate().map(|(row, text)| {
            // A captured row that does not continue has its trailing blanks trimmed.
            let continues = self.continues[row];
            let text = if continues {
                text.clone()
            } else {
                text.trim_end_matches([' ', '\t']).to_owned()
            };
            LiveDetectionInput {
                source: LiveDetectionSource::Grid {
                    row: row as u32,
                    revision: 1,
                },
                cell_boundaries: boundaries(&text),
                text,
                continues,
                captured_columns: self.width,
                site: self.site,
            }
        });
        history.chain(grid).collect()
    }
}

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| (*row).to_owned()).collect()
}

fn padded(rows: Vec<String>, height: usize) -> Vec<String> {
    let mut rows = rows;
    rows.resize(height, String::new());
    rows
}

/// A box-drawing rule row: `left`, then `─` for each cell width, `mid` between cells, `right`.
fn table_rule(left: char, mid: char, right: char, cells: &[usize]) -> String {
    let mut row = String::from(left);
    for (index, width) in cells.iter().enumerate() {
        row.extend(std::iter::repeat_n('\u{2500}', *width));
        row.push(if index + 1 == cells.len() { right } else { mid });
    }
    row
}

fn layout(width: u32) -> LayoutKey {
    LayoutKey {
        width_cells: std::num::NonZeroU32::new(width).unwrap(),
        dpi_milli: std::num::NonZeroU32::new(1000).unwrap(),
        font_size_subpixels: 16 * SUBPIXELS_PER_PX,
        font_rev: 1,
        theme_rev: 1,
        lang_rev: 0,
        profile_rev: 0,
        line_wrapping: true,
    }
}

fn candidate(screen: &Screen, capture: &LiveCapture, row: u32) -> LiveDetectionTask {
    LiveDetectionTask {
        candidate_row: row,
        screen: ScreenId::Alternate,
        grid_generation: GridGeneration(1),
        detection_revision: DetectionRevision(1),
        layout: layout(screen.width),
        cell_width_subpixels: 9 * SUBPIXELS_PER_PX,
        cell_height_subpixels: 18 * SUBPIXELS_PER_PX,
        ascii_baseline_subpixels: 14 * SUBPIXELS_PER_PX,
        capture: capture.clone(),
        pane: capture.screen_rect(),
        start: GridPoint { row, column: 0 },
        end: GridPoint { row, column: 0 },
        band_start_row: row,
        band_end_row: row,
        span: MathSpan {
            byte_start: 0,
            byte_end: 0,
            original_source: String::new(),
            render_source: String::new(),
            delimiter_kind: DelimiterKind::Dollars,
            mode: MathMode::Display,
            kind: BlockKind::Math,
            cell_segments: Vec::new(),
            inline_runs: Vec::new(),
            inline_joined_head: None,
        },
        detection_complete: false,
        resolved: false,
        refused_table_rows: Vec::new(),
    }
}

/// The corpus the plain-screen baseline was captured over: ordinary screens of every shape the live
/// detector meets (display, inline, environments, a frozen bridge, a clipped tail, fences, a pipe
/// table, wide characters, soft wraps, an ambiguous prefix), and the screens the note says are
/// **never** split — a full-height padded table, box art, a side-by-side diff, a `vim` split, a
/// `tmux` status row with a dollar, a plain horizontal split, a rule that moved, a formula written
/// across a rule. Every one of them must detect exactly what it detected before panes existed.
fn plain_screens() -> Vec<Screen> {
    let mut screens = vec![
        Screen::alt("pane-alternate", 100, padded(lines(&PANE), 40)),
        Screen {
            history: lines(&["$ cat math.md", "some prose before"]),
            site: InlineMathSite::CommandOutput,
            ..Screen::alt("pane-primary-with-history", 80, padded(lines(&PANE), 24))
        },
        Screen {
            history: lines(&["intro", "$$", "\\frac{a}{b}"]),
            site: InlineMathSite::CommandOutput,
            ..Screen::alt(
                "frozen-bridge",
                80,
                lines(&["+ c", "$$", "after $x$ here", ""]),
            )
        },
        Screen::alt(
            "clipped-tail",
            80,
            lines(&["\\frac{a}{b}", "+ c", "$$", "text between", "$$ y^2 $$"]),
        ),
        Screen::alt(
            "fenced",
            80,
            lines(&["```", "$$x$$", "```", "$$y$$", "$z$ inline"]),
        ),
    ];
    let mut open_fence = DetectionContext::default();
    advance_detection_context(&mut open_fence, TranscriptId(1), "```");
    screens.push(Screen {
        initial_context: open_fence,
        ..Screen::alt(
            "fence-open-before-row-0",
            80,
            lines(&["$$x$$", "```", "$$y$$"]),
        )
    });
    screens.push(Screen::alt(
        "pipe-table",
        80,
        lines(&["| a | $x$ |", "|---|---|", "| b | $y^2$ |", "", "$$ z $$"]),
    ));
    screens.push(Screen::alt(
        "wide-characters",
        40,
        lines(&["前置 $e^{i\\pi}+1=0$ 之后", "", "$$", "\\alpha", "$$"]),
    ));
    screens.push(Screen {
        continues: vec![true, true, false, false, false, false],
        ..Screen::alt(
            "soft-wrap",
            20,
            lines(&[
                "prefix text $a+b$ an",
                "d more text that wra",
                "ps here",
                "$$",
                "\\beta",
                "$$",
            ]),
        )
    });
    screens.push(Screen {
        initial_context: DetectionContext::ambiguous(),
        site: InlineMathSite::CommandOutput,
        ..Screen::alt("ambiguous-prefix", 80, lines(&["$$", "x", "$$", "$y$ z"]))
    });
    screens.push(Screen::alt(
        "environments",
        80,
        lines(&[
            "\\begin{aligned}",
            "a &= b \\\\",
            "c &= d",
            "\\end{aligned}",
            "",
            "\\[x^2\\]",
            "\\[",
            "y",
            "\\]",
        ]),
    ));

    // The screens that are never split.
    let mut table = vec!["\u{2502} name  \u{2502} $x^2$    \u{2502}".to_owned(); 40];
    table[0] = table_rule('\u{250c}', '\u{252c}', '\u{2510}', &[7, 10]);
    table[2] = table_rule('\u{251c}', '\u{253c}', '\u{2524}', &[7, 10]);
    table[39] = table_rule('\u{2514}', '\u{2534}', '\u{2518}', &[7, 10]);
    screens.push(Screen::alt("full-height-unicode-table", 20, table));

    let mut codex_table = vec!["\u{2502} a  \u{2502} b  \u{2502}".to_owned(); 40];
    codex_table[0] = table_rule('\u{250c}', '\u{252c}', '\u{2510}', &[4, 4]);
    codex_table[20] = table_rule('\u{251c}', '\u{253c}', '\u{2524}', &[4, 4]);
    codex_table[39] = table_rule('\u{2514}', '\u{2534}', '\u{2518}', &[4, 4]);
    screens.push(Screen::alt("padded-table-40x11", 11, codex_table));

    let mut art = vec!["\u{2502}                              \u{2502}".to_owned(); 20];
    art[0] = table_rule('\u{256d}', '\u{2500}', '\u{256e}', &[30]);
    art[5] = "\u{2502}  $$ x^2 $$ in a box          \u{2502}".to_owned();
    art[10] = "\u{2502}  Welcome to the banner       \u{2502}".to_owned();
    art[19] = table_rule('\u{2570}', '\u{2500}', '\u{256f}', &[30]);
    screens.push(Screen::alt("box-art", 32, art));

    let diff = (0..40)
        .map(|row| {
            let code = match row % 3 {
                0 => "$$",
                1 => "x^2",
                _ => "$$",
            };
            format!(
                "{:>4} \u{2502}{code:<30}\u{2502}{:>4} \u{2502}{code}",
                row + 1,
                row + 1
            )
        })
        .collect();
    screens.push(Screen::alt("side-by-side-diff", 80, diff));

    let mut vim = (0..38)
        .map(|row| {
            let right = PANE.get(row).copied().unwrap_or("~");
            format!("{:<39}\u{2502}{right}", format!("left buffer line {row}"))
        })
        .collect::<Vec<_>>();
    vim.push("notes.md [+]                            math.md".to_owned());
    vim.push(":vsplit math.md".to_owned());
    screens.push(Screen::alt("vim-vsplit", 80, vim));

    let mut tmux_dollar = (0..39)
        .map(|row| {
            let right = PANE.get(row).copied().unwrap_or("");
            format!("{:<50}\u{2502}{right}", format!("left pane line {row}"))
        })
        .collect::<Vec<_>>();
    tmux_dollar.push("[0] 0:bash* $HOME 12:00".to_owned());
    screens.push(Screen::alt("tmux-status-with-dollar", 100, tmux_dollar));

    let mut horizontal = vec!["plain text".to_owned(); 40];
    horizontal[5] = "```".to_owned();
    horizontal[19] = "\u{2500}".repeat(80);
    horizontal[25] = "$$ x^2 $$".to_owned();
    screens.push(Screen::alt("plain-horizontal-split", 80, horizontal));

    let moved = (0..40)
        .map(|row| {
            if row < 36 {
                format!("{:<25}\u{2502}text {row}", format!("side {row}"))
            } else {
                format!("{:<30}\u{2502}text {row}", format!("side {row}"))
            }
        })
        .collect();
    screens.push(Screen::alt("rule-moved-mid-capture", 80, moved));

    let mut across = vec!["log       \u{2502}text".to_owned(); 40];
    across[20] = "$x        \u{2502}+y$".to_owned();
    screens.push(Screen::alt("formula-across-a-rule", 80, across));

    let mut status_opener = vec!["        \u{2502}text".to_owned(); 40];
    status_opener[0] = "status!!!$$".to_owned();
    status_opener[1] = "        \u{2502}x^2".to_owned();
    status_opener[2] = "        \u{2502}$$".to_owned();
    screens.push(Screen::alt("status-slice-opener", 20, status_opener));

    for (name, edge) in [("dollar-free-edge-top", 0), ("dollar-free-edge-bottom", 39)] {
        let mut screen = vec!["log  \u{2502}text".to_owned(); 40];
        screen[edge] = "\\[x^2\\]".to_owned();
        screens.push(Screen::alt(name, 40, screen));
    }
    let mut environment_edge = vec!["log  \u{2502}text".to_owned(); 40];
    environment_edge[39] = "\\begin{pmatrix}".to_owned();
    screens.push(Screen::alt("environment-at-edge", 40, environment_edge));

    let mut blanks = vec!["log  \u{2502}text".to_owned(); 40];
    blanks[20] = "$$x   +y$$".to_owned();
    screens.push(Screen::alt("formula-own-blanks", 40, blanks));

    let mut merged = vec!["\u{2502} a  \u{2502} text    \u{2502}".to_owned(); 40];
    merged[20] = "\u{2502}$x   +y$      \u{2502}".to_owned();
    screens.push(Screen::alt("table-merged-formula", 16, merged));

    let mut tui_table = (0..35)
        .map(|row| format!("output line {row}"))
        .collect::<Vec<_>>();
    tui_table.extend((0..5).map(|row| format!("name {row}    \u{2502}value {row} $x_{row}$")));
    screens.push(Screen::alt("table-inside-a-tui", 40, tui_table));

    let mut unframed = vec!["log  \u{2502}text".to_owned(); 37];
    unframed.push("$$".to_owned());
    unframed.push("x   +   y".to_owned());
    unframed.push("$$".to_owned());
    screens.push(Screen::alt("display-across-unframed-rows", 40, unframed));
    screens
}

/// Everything the live detector says about one screen, written out field by field.
fn dump(screen: &Screen) -> String {
    use std::fmt::Write as _;
    let capture = LiveCapture::new(
        screen.inputs(),
        screen.initial_context.clone(),
        screen.options,
    );
    assert!(
        !capture.frame().is_framed(),
        "{} was cut into {:?}",
        screen.name,
        capture
            .frame()
            .panes()
            .iter()
            .map(|pane| pane.rect)
            .collect::<Vec<_>>()
    );
    let mut out = String::new();
    writeln!(out, "== {}", screen.name).unwrap();
    let mut tasks = (0..screen.grid.len() as u32)
        .map(|row| candidate(screen, &capture, row))
        .collect::<Vec<_>>();
    resolve_live_detection_tasks(&mut tasks);
    for task in &tasks {
        // The batch and the single resolution agree, or the baseline would depend on which ran.
        let mut single = candidate(screen, &capture, task.candidate_row);
        resolve_live_detection_task(&mut single);
        assert_eq!(
            (single.resolved, &single.span, single.start, single.end),
            (task.resolved, &task.span, task.start, task.end),
            "{}: row {}",
            screen.name,
            task.candidate_row
        );
        if !task.resolved && task.refused_table_rows.is_empty() {
            continue;
        }
        writeln!(
            out,
            "task row={} resolved={} start={:?} end={:?} band={}..={} refused={:?}",
            task.candidate_row,
            task.resolved,
            task.start,
            task.end,
            task.band_start_row,
            task.band_end_row,
            task.refused_table_rows
        )
        .unwrap();
        if task.resolved {
            writeln!(out, "  span={:?}", task.span).unwrap();
        }
    }
    // An unframed screen has one pane, so one ledger: today's, printed as today's was.
    let ledgers = live_detection_ownership_ledger(&capture);
    if ledgers.len() == 1 {
        writeln!(out, "ledger={:?}", ledgers.values().next().unwrap()).unwrap();
    } else {
        writeln!(out, "ledgers={ledgers:?}").unwrap();
    }
    let gap = live_detection_isolation_gap(&capture);
    writeln!(out, "isolation_gap={gap}").unwrap();
    out
}

fn plain_screen_dump() -> String {
    plain_screens().iter().map(dump).collect()
}

/// RED (69a) — **a screen no frame cuts detects exactly what it detected before panes existed.**
///
/// The fixture was captured from main `7b2e0841` (the note's merge) before any line of 69a was
/// written: this test ran there against an empty fixture and its failure message is the file. It
/// holds every resolved task byte for byte (start, end, band, refused tables and the whole
/// occurrence), the ownership ledger and the isolation gap, for ordinary screens and for every
/// screen the note says is never split.
///
/// MUTATION: slice a whole-screen pane's rows, or scan an unframed screen from a neutral
/// checkpoint instead of the capture's own.
#[test]
fn a_plain_screen_detects_exactly_what_it_always_did() {
    let expected = include_str!("fixtures/plain_screens.baseline")
        .replace("\r\n", "\n")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    let actual = plain_screen_dump();
    if expected != actual {
        let first = expected
            .lines()
            .zip(actual.lines())
            .position(|(left, right)| left != right);
        panic!(
            "the plain-screen baseline moved; first differing line {first:?}\n--- actual ---\n{actual}--- end ---"
        );
    }
}

// ---- The framed screens (note §7.1) ----------------------------------------------------------

/// **The grid literal**: a capture of these rows exactly as a terminal `width` cells wide captured
/// them on its alternate screen — boundaries from `bt_unicode` cluster widths, trailing blanks
/// trimmed, from a neutral checkpoint. A fixture, not a product path.
fn framed_capture(width: u32, rows: &[String]) -> LiveCapture {
    framed_capture_from(width, rows, DetectionContext::default())
}

fn framed_capture_from(width: u32, rows: &[String], context: DetectionContext) -> LiveCapture {
    let screen = Screen {
        initial_context: context,
        ..Screen::alt("framed", width, rows.to_vec())
    };
    LiveCapture::new(screen.inputs(), screen.initial_context, screen.options)
}

fn rect(top: u32, bottom: u32, left: u32, right: u32) -> PaneRect {
    PaneRect {
        top,
        bottom,
        left,
        right,
    }
}

fn panes(capture: &LiveCapture) -> Vec<PaneRect> {
    capture
        .frame()
        .panes()
        .iter()
        .map(|pane| pane.rect)
        .collect()
}

/// One proven block: `(pane, mode, render source, start, end)`.
type Proven = (PaneRect, MathMode, String, GridPoint, GridPoint);

/// Every block the product proves on this capture: one candidate per grid row, resolved in one batch
/// exactly as the session resolves them.
fn proven(capture: &LiveCapture) -> Vec<Proven> {
    let rows = capture
        .inputs()
        .iter()
        .filter_map(|input| match input.source {
            LiveDetectionSource::Grid { row, .. } => Some(row),
            LiveDetectionSource::History { .. } => None,
        })
        .collect::<Vec<_>>();
    let screen = Screen::alt("proven", capture.screen_rect().right, Vec::new());
    let mut tasks = rows
        .into_iter()
        .map(|row| candidate(&screen, capture, row))
        .collect::<Vec<_>>();
    resolve_live_detection_tasks(&mut tasks);
    tasks
        .into_iter()
        .filter(|task| task.resolved)
        .map(|task| {
            (
                task.pane,
                task.span.mode,
                task.span.render_source,
                task.start,
                task.end,
            )
        })
        .collect()
}

fn count(blocks: &[Proven], pane: PaneRect, mode: MathMode) -> usize {
    blocks
        .iter()
        .filter(|block| block.0 == pane && block.1 == mode)
        .count()
}

/// **A screen read whole**: one pane, and that pane is the capture itself — its own inputs, history
/// and all, from its own checkpoint — so its scan is today's by construction.
fn assert_read_whole(capture: &LiveCapture) {
    let frame = capture.frame();
    assert!(
        !frame.is_framed(),
        "the screen was cut: {:?}",
        panes(capture)
    );
    assert_eq!(panes(capture), vec![capture.screen_rect()]);
    let pane = &frame.panes()[0];
    assert!(Arc::ptr_eq(pane.inputs(), capture.inputs()));
    assert_eq!(pane.initial_context(), capture.initial_context());
}

fn owned(rows: impl IntoIterator<Item = String>) -> Vec<String> {
    rows.into_iter().collect()
}

fn herdr(left: &str) -> Vec<String> {
    owned(PANE.iter().map(|text| format!("{left}\u{2502}{text}")))
}

/// RED (69a) — **the herdr sidebar no longer hides the pane.**
///
/// `herdr` 0.8.2 repaints the host screen with twenty-five blank cells and a `│` in front of every
/// pane row. Read as one line that is indented code opening on `│`, so nothing typeset. The rule is
/// a frame (V1 on every row, the left side a blank gutter), and the pane right of it proves the
/// `PANE`'s three formulas in its own columns.
///
/// MUTATION: return the unframed frame from `ScreenFrame::measure` unconditionally.
#[test]
fn the_herdr_sidebar_no_longer_hides_the_pane() {
    let capture = framed_capture(100, &herdr(&" ".repeat(25)));
    let right = rect(0, 13, 26, 100);
    assert_eq!(panes(&capture), vec![rect(0, 13, 0, 25), right]);
    let blocks = proven(&capture);
    assert_eq!(count(&blocks, right, MathMode::Inline), 1);
    assert_eq!(count(&blocks, right, MathMode::Display), 2);
    assert_eq!(blocks.len(), 3);
    let inline = blocks
        .iter()
        .find(|block| block.1 == MathMode::Inline)
        .unwrap();
    assert_eq!(inline.3, GridPoint { row: 0, column: 34 });
    assert_eq!(
        blocks
            .iter()
            .filter(|block| block.1 == MathMode::Display)
            .map(|block| (block.2.as_str(), block.3, block.4))
            .collect::<Vec<_>>(),
        vec![
            (
                "\\frac{1}{2}",
                GridPoint { row: 2, column: 26 },
                GridPoint { row: 4, column: 28 }
            ),
            (
                "\\begin{pmatrix}\na & b \\\\\nc & d \\\\\ne & f\n\\end{pmatrix}",
                GridPoint { row: 6, column: 26 },
                GridPoint {
                    row: 12,
                    column: 28
                }
            ),
        ]
    );
}

/// RED (69a) — **the herdr compact sidebar no longer hides the pane**: the same picture three
/// columns wide, the pane from column 4.
///
/// MUTATION: start the pane right of a rule one column late (`left = cut + 2` in `Measure::split`).
/// (Counting the rule's own column into it, `left = cut`, cuts the same strip forever: the stack
/// overflows, which is red too.)
#[test]
fn the_herdr_compact_sidebar_no_longer_hides_the_pane() {
    let capture = framed_capture(100, &herdr("   "));
    let right = rect(0, 13, 4, 100);
    assert_eq!(panes(&capture), vec![rect(0, 13, 0, 3), right]);
    let blocks = proven(&capture);
    assert_eq!(count(&blocks, right, MathMode::Inline), 1);
    assert_eq!(count(&blocks, right, MathMode::Display), 2);
    let inline = blocks
        .iter()
        .find(|block| block.1 == MathMode::Inline)
        .unwrap();
    assert_eq!(inline.3, GridPoint { row: 0, column: 12 });
}

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

/// RED (69a) — **a tmux split typesets the right pane and nothing in the left.** The left pane's
/// `$5 and $10` stays prose, scanned in its own columns by every gate that always read it.
///
/// MUTATION: remove V2a (clipped) from `Measure::v2`, leaving only the gutter.
#[test]
fn a_tmux_split_typesets_the_right_pane_and_nothing_in_the_left() {
    let screen = owned(
        PANE.iter()
            .zip(PROSE)
            .map(|(math, prose)| format!("{prose:<50}\u{2502}{math}")),
    );
    let capture = framed_capture(100, &screen);
    let (left, right) = (rect(0, 13, 0, 50), rect(0, 13, 51, 100));
    assert_eq!(panes(&capture), vec![left, right]);
    let blocks = proven(&capture);
    assert_eq!(count(&blocks, left, MathMode::Inline), 0);
    assert_eq!(count(&blocks, left, MathMode::Display), 0);
    assert_eq!(count(&blocks, right, MathMode::Inline), 1);
    assert_eq!(count(&blocks, right, MathMode::Display), 2);
}

/// RED (69a) — **prose left of the rule still splits the screen**: a log whose lines stop short of
/// the rule is padded, and the pane right of it is clipped, which is enough (V2a is either side).
///
/// MUTATION: require both sides clipped in `Measure::v2`.
#[test]
fn prose_left_of_the_rule_still_splits_the_screen() {
    let screen = owned(PANE.iter().enumerate().map(|(index, math)| {
        let prose = format!("line {index} of an ordinary log");
        format!("{prose:<32}\u{2502}{math}")
    }));
    let capture = framed_capture(80, &screen);
    let (left, right) = (rect(0, 13, 0, 32), rect(0, 13, 33, 80));
    assert_eq!(panes(&capture), vec![left, right]);
    let blocks = proven(&capture);
    assert!(blocks.iter().all(|block| block.0 == right));
    assert_eq!(blocks.len(), 3);
}

fn log_rows(height: usize) -> Vec<String> {
    vec!["log  \u{2502}$x^2$".to_owned(); height]
}

/// RED (69a) — **a fence the screen proves suppresses every pane** (R9). A fence open before the
/// first grid row (the checkpoint), and one opened on an excluded status row, are the screen's: the
/// frame stands and no pane proves anything under it. Without the fence the right pane proves forty
/// formulas, so the zero is the fence's doing.
///
/// MUTATION: return an empty `ScreenFenceState` from `screen_fence_pass`.
#[test]
fn a_fence_the_screen_proves_suppresses_every_region() {
    let open = || {
        let mut context = DetectionContext::default();
        advance_detection_context(&mut context, TranscriptId(1), "```");
        context
    };
    let unfenced = framed_capture(40, &log_rows(40));
    let right = rect(0, 40, 6, 40);
    assert_eq!(panes(&unfenced), vec![rect(0, 40, 0, 5), right]);
    assert_eq!(count(&proven(&unfenced), right, MathMode::Inline), 40);

    let fenced = framed_capture_from(40, &log_rows(40), open());
    assert_eq!(panes(&fenced), panes(&unfenced), "the frame stands");
    assert!(fenced.frame().screen_fence_state().covers(0));
    assert!(fenced.frame().screen_fence_state().covers(39));
    assert!(proven(&fenced).is_empty());

    // The ledger says the same: a display block the screen's fence covers is not the product's.
    let displays = vec!["log  \u{2502}$$x^2$$".to_owned(); 40];
    let owned = |capture: &LiveCapture| {
        live_detection_ownership_ledger(capture)
            .values()
            .map(|ledger| ledger.owned_block_sources.len())
            .sum::<usize>()
    };
    assert_eq!(owned(&framed_capture(40, &displays)), 40);
    assert_eq!(owned(&framed_capture_from(40, &displays, open())), 0);

    let mut status_opened = vec!["```".to_owned()];
    status_opened.extend(log_rows(40));
    let status = framed_capture(40, &status_opened);
    assert_eq!(status.frame().status_row(), Some(0));
    assert_eq!(panes(&status), vec![rect(1, 41, 0, 5), rect(1, 41, 6, 40)]);
    assert!(proven(&status).is_empty());
}

/// RED (69a) — **a fence one pane prints leaves the other pane alone** (R9): a fence opened inside the
/// left pane is that pane's own, and the right pane proves every one of its rows.
///
/// MUTATION: suppress every pane on the rows any pane's fence covers.
#[test]
fn a_fence_one_pane_prints_leaves_the_other_pane_alone() {
    let mut screen = log_rows(40);
    screen[5] = "```  \u{2502}$x^2$".to_owned();
    screen[9] = "```  \u{2502}$x^2$".to_owned();
    let capture = framed_capture(40, &screen);
    let (left, right) = (rect(0, 40, 0, 5), rect(0, 40, 6, 40));
    assert_eq!(panes(&capture), vec![left, right]);
    let blocks = proven(&capture);
    assert_eq!(count(&blocks, right, MathMode::Inline), 40);
    assert!(blocks.iter().all(|block| block.0 == right));
}

fn clipped_log(height: usize) -> Vec<String> {
    vec!["log  \u{2502}text".to_owned(); height]
}

/// RED (69a) — **a dollar-free formula at an edge keeps the screen whole** (owner's ruling
/// 2026-09-17, kept 2026-09-29): an edge row carrying `\[x^2\]` or `\begin{pmatrix}` is never set
/// aside, so the rule fails V1 on it and nothing is cut; the formula is kept. A delimiter-free
/// status row in the same place is set aside, which is what makes the refusal the delimiter's.
///
/// MUTATION: test only for `$` in `line_carries_math_delimiter`.
#[test]
fn a_dollar_free_formula_at_an_edge_keeps_the_screen_whole() {
    for edge in [0usize, 39] {
        let mut screen = clipped_log(40);
        screen[edge] = "\\[x^2\\]".to_owned();
        let capture = framed_capture(40, &screen);
        assert_read_whole(&capture);
        let blocks = proven(&capture);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].2, "x^2");
    }
    let mut screen = clipped_log(40);
    screen[39] = "\\begin{pmatrix}".to_owned();
    assert_read_whole(&framed_capture(40, &screen));

    let mut screen = clipped_log(40);
    screen[39] = "bash 12:00".to_owned();
    let capture = framed_capture(40, &screen);
    assert_eq!(capture.frame().status_row(), Some(39));
    assert_eq!(panes(&capture), vec![rect(0, 39, 0, 5), rect(0, 39, 6, 40)]);
}

/// RED (69a) — **a cut never runs through a formula's own blanks**: `$$x   +y$$` on row 20 breaks
/// the stroke at column five, so V1 refuses the column; the block is kept.
///
/// MUTATION: let V1 skip a blank cell.
#[test]
fn a_cut_never_runs_through_a_formulas_own_blanks() {
    let mut screen = clipped_log(40);
    screen[20] = "$$x   +y$$".to_owned();
    let capture = framed_capture(40, &screen);
    assert_read_whole(&capture);
    let blocks = proven(&capture);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].3, GridPoint { row: 20, column: 0 });
}

/// RED (69a) — **a table is not cut through the cell a formula merged**: the inner rule is missing on
/// row 20, and the outer rules are padded, so nothing is cut and the formula keeps its unsplit
/// anchor and cells.
///
/// MUTATION: let V1 skip a blank cell.
#[test]
fn a_table_is_not_cut_through_the_cell_a_formula_merged() {
    let mut screen = vec!["\u{2502} a  \u{2502} text    \u{2502}".to_owned(); 40];
    screen[20] = "\u{2502}$x   +y$      \u{2502}".to_owned();
    let capture = framed_capture(16, &screen);
    assert_read_whole(&capture);
    let blocks = proven(&capture);
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        (blocks[0].3, blocks[0].4),
        (
            GridPoint { row: 20, column: 1 },
            GridPoint { row: 20, column: 9 }
        )
    );
}

/// RED (69a) — **a table drawn inside a TUI never splits the screen**: five rows of it in forty, and
/// V1 asks for the stroke on every row of the rectangle.
///
/// MUTATION: accept a share of the rows in V1 (the retired nine-tenths rule at a lower share).
#[test]
fn a_table_drawn_inside_a_tui_never_splits_the_screen() {
    let mut screen = (0..35)
        .map(|row| format!("output line {row}"))
        .collect::<Vec<_>>();
    screen.extend((0..5).map(|row| format!("name {row}    \u{2502}value {row}")));
    assert_read_whole(&framed_capture(40, &screen));
}

fn full_height_table() -> Vec<String> {
    let mut table = vec!["\u{2502} name  \u{2502} $x^2$    \u{2502}".to_owned(); 40];
    table[0] = table_rule('\u{250c}', '\u{252c}', '\u{2510}', &[7, 10]);
    table[2] = table_rule('\u{251c}', '\u{253c}', '\u{2524}', &[7, 10]);
    table[39] = table_rule('\u{2514}', '\u{2534}', '\u{2518}', &[7, 10]);
    table
}

/// RED (69a) — **a full-height Unicode table is not a pane frame** (replaces the branch's
/// `a_full_screen_table_splits_into_its_cells_without_losing_their_math`). Every rule is padded on
/// both sides on every row, so none is a frame, and no horizontal rule has a proven anchor. The one
/// pane is the capture itself, so the blocks, anchors and cells are the unsplit scan's exactly (the
/// same screen is in the plain-screen baseline).
///
/// **The residual cost, pinned beside it.** The same table on a screen wider than itself has a blank
/// column right of its outer rule, which V2b reads as a gutter: the outer rule is cut and the
/// table's own horizontal rules, anchored on it, cut the strip into bands. Every formula the whole
/// screen proves is still proven, at the same cells — a cut along a table's own rules takes
/// nothing apart — but the screen is framed. The note's §0.3 invariant is about a table as wide as
/// its screen; this case is reported to the review (T-PANE-COLUMNS report, finding F-1).
///
/// MUTATION: read a side as clipped when any row touches the rule (drop the majority in
/// `Measure::v2`).
#[test]
fn a_full_height_unicode_table_is_not_a_pane_frame() {
    let capture = framed_capture(20, &full_height_table());
    assert_read_whole(&capture);
    let whole = proven(&capture);
    assert_eq!(whole.len(), 37);

    let wider = framed_capture(40, &full_height_table());
    assert!(wider.frame().is_framed());
    let anchors = |blocks: &[Proven]| {
        blocks
            .iter()
            .map(|block| (block.2.clone(), block.3, block.4))
            .collect::<Vec<_>>()
    };
    assert_eq!(anchors(&proven(&wider)), anchors(&whole));
}

fn aligned_two_by_two(right: impl Fn(usize) -> String) -> Vec<String> {
    (0..40)
        .map(|row| {
            if row == 19 {
                format!("{}\u{253c}{}", "\u{2500}".repeat(10), "\u{2500}".repeat(10))
            } else {
                format!("{:<10}\u{2502}{}", format!("left {row}"), right(row))
            }
        })
        .collect()
}

/// RED (69a) — **a fence in the top-right pane does not suppress the bottom-right pane** (Codex
/// finding 1). An aligned 2×2: the rule at column 10 on forty rows (the `┼` carries it), and row 19
/// a horizontal cut anchored by that `┼` on the proven rule. The top-right pane opens a fence and
/// never closes it; a horizontal cut restarts the scan, so the bottom-right block is proven.
///
/// MUTATION: return no horizontal cuts from `Measure::horizontal_cuts` (panes become column spans).
#[test]
fn a_fence_in_the_top_right_does_not_suppress_the_bottom_right() {
    let capture = framed_capture(
        21,
        &aligned_two_by_two(|row| match row {
            2 => "```".to_owned(),
            22 | 24 => "$$".to_owned(),
            23 => "x^2".to_owned(),
            _ => format!("out {row}"),
        }),
    );
    let bottom_right = rect(20, 40, 11, 21);
    assert_eq!(
        panes(&capture),
        vec![
            rect(0, 19, 0, 10),
            rect(0, 19, 11, 21),
            rect(20, 40, 0, 10),
            bottom_right
        ]
    );
    let blocks = proven(&capture);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].0, bottom_right);
    assert_eq!(
        blocks[0].3,
        GridPoint {
            row: 22,
            column: 11
        }
    );
}

/// RED (69a) — **a non-aligned nested split recovers the top-right pane**: the rule runs on rows 0–18
/// only, row 19 is `──────────┴──────────`, rows 20–39 are one full-width pane. Row 19 is cut first,
/// anchored by the `┴` on the rule proven in the band above; the top band is then cut at column 10.
///
/// MUTATION: anchor H2 only on junctions outside the rectangle.
#[test]
fn a_non_aligned_nested_split_recovers_the_top_right_pane() {
    let screen = (0..40)
        .map(|row| match row {
            0..19 => {
                let right = match row {
                    5 | 7 => "$$".to_owned(),
                    6 => "x^2".to_owned(),
                    _ => format!("out {row}"),
                };
                format!("{:<10}\u{2502}{right}", format!("left {row}"))
            }
            19 => format!("{}\u{2534}{}", "\u{2500}".repeat(10), "\u{2500}".repeat(10)),
            _ => format!("full width row {row}"),
        })
        .collect::<Vec<_>>();
    let capture = framed_capture(21, &screen);
    let top_right = rect(0, 19, 11, 21);
    assert_eq!(
        panes(&capture),
        vec![rect(0, 19, 0, 10), top_right, rect(20, 40, 0, 21)]
    );
    let blocks = proven(&capture);
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        (blocks[0].0, blocks[0].3, blocks[0].4),
        (
            top_right,
            GridPoint { row: 5, column: 11 },
            GridPoint { row: 7, column: 13 }
        )
    );
}

/// RED (69a) — **a padded full-height table is never cut in either direction** (Codex's check of
/// (b), blocker 1): the 40×11 table — no vertical cut (every rule padded, no outer gutter) and no
/// horizontal cut (its `├┼┤` row has no anchor on a proven frame rule).
///
/// MUTATION: let H2 anchor on any junction whose column passes V1, without V2.
#[test]
fn a_padded_full_height_table_is_never_cut_in_either_direction() {
    let mut table = vec!["\u{2502} a  \u{2502} b  \u{2502}".to_owned(); 40];
    table[0] = table_rule('\u{250c}', '\u{252c}', '\u{2510}', &[4, 4]);
    table[20] = table_rule('\u{251c}', '\u{253c}', '\u{2524}', &[4, 4]);
    table[39] = table_rule('\u{2514}', '\u{2534}', '\u{2518}', &[4, 4]);
    assert_read_whole(&framed_capture(11, &table));
}

/// RED (69a) — **full-height box art is not a pane frame**: a boxed two-column diagram drawn from
/// column 0, `│` on every row, one line of text in each box standing off the rules. Its top and
/// bottom rows are strokes touching every rule, and a stroke is frame, not text: counted as text
/// they would be two of the three rows "with text" beside the divider, touching it, and the divider
/// would read as clipped and cut the diagram in two.
///
/// MUTATION: count a stroke cell as text in `Measure::side_text`.
#[test]
fn full_height_box_art_is_not_a_pane_frame() {
    let mut art = vec!["\u{2502}              \u{2502}               \u{2502}".to_owned(); 20];
    art[0] = table_rule('\u{250c}', '\u{252c}', '\u{2510}', &[14, 15]);
    art[10] = "\u{2502}  a request   \u{2502}  its answer   \u{2502}".to_owned();
    art[19] = table_rule('\u{2514}', '\u{2534}', '\u{2518}', &[14, 15]);
    assert_read_whole(&framed_capture(32, &art));
}

/// RED (69a) — **a blank gutter beside a pane that indents is a frame** (V2b). The pane's lines all
/// start two cells past the rule, so neither side is clipped; the left side is blank on every row
/// and three columns wide, which no text can cross, so the rule is a frame (herdr's sidebar, an
/// empty pane).
///
/// MUTATION: drop V2b (`gutter = false` in `Measure::v2`).
#[test]
fn a_blank_gutter_beside_an_indented_pane_is_a_frame() {
    let screen = (0..20)
        .map(|row| format!("   \u{2502}  output {row}"))
        .collect::<Vec<_>>();
    let capture = framed_capture(40, &screen);
    assert_eq!(panes(&capture), vec![rect(0, 20, 0, 3), rect(0, 20, 4, 40)]);
}

/// RED (69a) — **a side-by-side diff is not a pane frame** (V2c): delta's rows are clipped at the
/// code columns, but the leaf left of the first rule holds only line numbers, and a numbered gutter
/// refuses the whole frame. The diffed `$$` lines stay source.
///
/// MUTATION: drop the V2c check in `ScreenFrame::measure`.
#[test]
fn a_side_by_side_diff_is_not_a_pane_frame() {
    let screen = (0..40)
        .map(|row| {
            let code = match row % 3 {
                0 => "$$",
                1 => "x^2",
                _ => "$$",
            };
            format!(
                "{:>4} \u{2502}{code:<30}\u{2502}{:>4} \u{2502}{code}",
                row + 1,
                row + 1
            )
        })
        .collect::<Vec<_>>();
    let capture = framed_capture(80, &screen);
    assert_read_whole(&capture);
    assert!(proven(&capture).is_empty());
}

/// RED (69a) — **a status slice cannot manufacture a clean display opener** (Codex finding 3): row 0
/// `status!!!$$` carries a dollar, so it is never set aside; the rule at column 8 then fails V1 on
/// it, nothing is cut, and the `$$ / x^2 / $$` a slice would have read is never proven.
///
/// MUTATION: drop the delimiter test from the status-row choice in `ScreenFrame::measure`.
#[test]
fn a_status_slice_cannot_manufacture_a_clean_display_opener() {
    let mut screen = vec!["        \u{2502}text".to_owned(); 40];
    screen[0] = "status!!!$$".to_owned();
    screen[1] = "        \u{2502}x^2".to_owned();
    screen[2] = "        \u{2502}$$".to_owned();
    let capture = framed_capture(20, &screen);
    assert_read_whole(&capture);
    assert_eq!(capture.frame().status_row(), None);
    assert!(proven(&capture).is_empty());
}

/// RED (69a) — **a vim `:vsplit` with two status rows keeps today's behaviour** (accepted conservative
/// failure, §5): only one edge row may be set aside, and the window status and the command line are
/// two.
///
/// MUTATION: allow two excluded rows.
#[test]
fn a_vim_vsplit_with_two_status_rows_keeps_todays_behaviour() {
    let mut screen = (0..38)
        .map(|row| {
            let right = PANE.get(row).copied().unwrap_or("~");
            format!("{:<39}\u{2502}{right}", format!("left buffer line {row}"))
        })
        .collect::<Vec<_>>();
    screen.push("notes.md [+]                            math.md".to_owned());
    screen.push(":vsplit math.md".to_owned());
    assert_read_whole(&framed_capture(80, &screen));
}

/// RED (69a) — **a tmux status row with a dollar keeps today's behaviour** (accepted conservative
/// failure, §5, the 2026-09-17 ruling's price): `$HOME` in the status row refuses its exclusion, so
/// the screen is read whole and the right pane's two display blocks are not proven — today's cost,
/// pinned.
///
/// MUTATION: drop the delimiter test from the status-row choice.
#[test]
fn a_tmux_status_row_with_a_dollar_keeps_todays_behaviour() {
    let mut screen = (0..39)
        .map(|row| {
            let right = PANE.get(row).copied().unwrap_or("");
            format!("{:<50}\u{2502}{right}", format!("left pane line {row}"))
        })
        .collect::<Vec<_>>();
    screen.push("[0] 0:bash* $HOME 12:00".to_owned());
    let capture = framed_capture(100, &screen);
    assert_read_whole(&capture);
    assert!(
        proven(&capture)
            .iter()
            .all(|block| block.1 != MathMode::Display)
    );
}

/// RED (69a) — **a plain horizontal split keeps today's fence and formula cost** (accepted, §5): a
/// full-width `─` row with no junction is not anchored and never cuts, so a fence above it still
/// reaches the block below.
///
/// MUTATION: let H1 alone make a horizontal cut.
#[test]
fn a_plain_horizontal_split_keeps_todays_fence_and_formula_cost() {
    let mut screen = vec!["plain text".to_owned(); 40];
    screen[5] = "```".to_owned();
    screen[19] = "\u{2500}".repeat(80);
    screen[25] = "$$ x^2 $$".to_owned();
    let capture = framed_capture(80, &screen);
    assert_read_whole(&capture);
    assert!(proven(&capture).is_empty());
}

/// RED (69a) — **a rule column that changes mid-capture is not sliced at the stale column** (R12):
/// at column 25 on rows 0–35 and 30 on rows 36–39, neither column is a cut. With only row 39 moved,
/// and delimiter-free, that one row is set aside and never sliced.
///
/// MUTATION: exclude more than one row, or let V1 accept the column on a share of the rows.
#[test]
fn a_rule_column_that_changes_mid_capture_is_not_sliced_at_the_stale_column() {
    let row_at = |row: usize, column: usize| {
        format!(
            "{:<column$}\u{2502}text {row}",
            format!("side {row}"),
            column = column
        )
    };
    let moved = (0..40)
        .map(|row| row_at(row, if row < 36 { 25 } else { 30 }))
        .collect::<Vec<_>>();
    assert_read_whole(&framed_capture(80, &moved));

    let last_moved = (0..40)
        .map(|row| row_at(row, if row < 39 { 25 } else { 30 }))
        .collect::<Vec<_>>();
    let capture = framed_capture(80, &last_moved);
    assert_eq!(capture.frame().status_row(), Some(39));
    assert_eq!(
        panes(&capture),
        vec![rect(0, 39, 0, 25), rect(0, 39, 26, 80)]
    );
    let stale = LiveDetectionSource::Grid {
        row: 39,
        revision: 1,
    };
    assert!(
        capture
            .frame()
            .panes()
            .iter()
            .all(|pane| pane.inputs().iter().all(|input| input.source != stale))
    );
}

/// RED (69a) — **an exempt edge's wide cluster, and a pane's wide characters, map through the
/// captured boundaries** (R7). The status row puts `能` across the rule column: it is set aside and
/// belongs to no pane. In the right pane, `能` before an inline formula takes two cells, and the
/// run's anchor is the captured column 4, not the character count's 3.
///
/// MUTATION: re-base a pane's boundaries to the pane's own column zero in `pane_input`.
#[test]
fn an_exempt_edge_wide_cluster_maps_through_the_captured_boundaries() {
    let mut screen = vec!["a\u{2502}text".to_owned(); 20];
    screen[5] = "a\u{2502}能$x^2$".to_owned();
    screen[19] = "能 status".to_owned();
    let capture = framed_capture(20, &screen);
    assert_eq!(capture.frame().status_row(), Some(19));
    let right = rect(0, 19, 2, 20);
    assert_eq!(panes(&capture), vec![rect(0, 19, 0, 1), right]);
    let blocks = proven(&capture);
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        (blocks[0].0, blocks[0].3, blocks[0].4),
        (
            right,
            GridPoint { row: 5, column: 4 },
            GridPoint { row: 5, column: 9 }
        )
    );
}

/// RED (69a) — **a formula written across an intact rule refuses the cut** (R5, on rule rows too):
/// `$x        │+y$` is proven by the unsplit scan, so the cut at column 10 would take it apart and is
/// refused. The branch's "deliberate loss" is withdrawn.
///
/// MUTATION: skip `vertical_cut_crosses_a_proof` in `Measure::vertical_cuts`.
#[test]
fn a_formula_written_across_an_intact_rule_refuses_the_cut() {
    let mut screen = vec!["log       \u{2502}text".to_owned(); 40];
    screen[20] = "$x        \u{2502}+y$".to_owned();
    let capture = framed_capture(80, &screen);
    assert_read_whole(&capture);
    let blocks = proven(&capture);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].3, GridPoint { row: 20, column: 0 });

    // The same screen without the formula is cut there, so the refusal is R5's.
    let clean = framed_capture(80, &vec!["log       \u{2502}text".to_owned(); 40]);
    assert_eq!(panes(&clean), vec![rect(0, 40, 0, 10), rect(0, 40, 11, 80)]);
}

/// RED (69a) — **a display block across unframed rows keeps the screen whole** (§6.3): thirty-seven
/// rows of `log  │text` and three rows `$$`, `x   +   y`, `$$`. The branch cut at column five and
/// typeset `x   +`; V1 refuses the column, and the block is proven whole.
///
/// MUTATION: accept a share of the rows in V1.
#[test]
fn a_display_block_across_unframed_rows_keeps_the_screen_whole() {
    let mut screen = clipped_log(37);
    screen.extend(["$$", "x   +   y", "$$"].map(str::to_owned));
    let capture = framed_capture(40, &screen);
    assert_read_whole(&capture);
    let blocks = proven(&capture);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].2, "x   +   y");
}

/// RED (69a) — **a rule in the last column leaves no empty pane** (R6 of revision (a), Codex B-7): the
/// zero-width strip right of it is dropped. Alone it cuts nothing, and beside a real split it adds no
/// pane.
///
/// MUTATION: keep zero-width strips in `Measure::split`.
#[test]
fn a_rule_in_the_last_column_leaves_no_empty_pane() {
    let alone = (0..20)
        .map(|row| format!("line {row:05}\u{2502}"))
        .collect::<Vec<_>>();
    assert_read_whole(&framed_capture(11, &alone));

    let beside = (0..20)
        .map(|row| format!("line {row:05}\u{2502}right {row:03}\u{2502}"))
        .collect::<Vec<_>>();
    let capture = framed_capture(21, &beside);
    assert_eq!(
        panes(&capture),
        vec![rect(0, 20, 0, 10), rect(0, 20, 11, 20)]
    );
}

/// RED (69a) — **a cluster straddling the rule belongs to neither side** (R7): the only row a wide
/// cluster can cross a rule column on is the one excluded edge row, and that row is in no pane — its
/// text is in neither pane's inputs.
///
/// MUTATION: slice the status row into the panes.
#[test]
fn a_cluster_straddling_the_rule_belongs_to_neither_side() {
    let mut screen = vec!["abcd\u{2502}text".to_owned(); 10];
    screen[9] = "abc能def".to_owned();
    let capture = framed_capture(20, &screen);
    assert_eq!(capture.frame().status_row(), Some(9));
    assert_eq!(panes(&capture), vec![rect(0, 9, 0, 4), rect(0, 9, 5, 20)]);
    for pane in capture.frame().panes() {
        assert!(
            pane.inputs().iter().all(|input| !input.text.contains('能')),
            "{:?} took the straddling cluster",
            pane.rect
        );
        // Every pane row's boundaries are screen columns inside the pane.
        for input in pane.inputs().iter() {
            assert!(
                input
                    .cell_boundaries
                    .iter()
                    .all(|(_, cell)| (pane.rect.left..=pane.rect.right).contains(cell))
            );
        }
    }
}

/// RED (69a) — **two tasks sharing one capture share one frame** (B-6; the construction counter is
/// the unit test `a_capture_measures_its_frame_once` in `bt_detect::frame`). Resolution on a worker
/// and arming on the window thread read the same `Arc<ScreenFrame>`.
///
/// MUTATION: measure a fresh frame per `LiveCapture::frame` call.
#[test]
fn a_capture_shares_its_frame_between_the_tasks_that_hold_it() {
    let capture = framed_capture(100, &herdr("   "));
    let screen = Screen::alt("shared", 100, Vec::new());
    let first = candidate(&screen, &capture, 4);
    let second = candidate(&screen, &capture, 12);
    assert!(Arc::ptr_eq(first.capture.frame(), second.capture.frame()));
}

/// RED (69a) — **a formula in each pane on one row does not refuse the cut** (R5, asked of the
/// block's cells). The unsplit scan groups every `$…$` run of a line into one inline occurrence,
/// so a row holding `$a_n$` left of the rule and `$b_n$` right of it proves one occurrence whose
/// runs stand on both sides. No cell of either formula is on the rule, so the cut takes nothing
/// apart: the screen is split, and the left pane proves its formula on every row. (The right pane's
/// formula on the same row is ticket 69b's: in 69a a candidate row is filled from the first pane
/// whose block closes on it — `two_panes_closing_on_the_same_row_both_typeset`.)
///
/// MUTATION: refuse a cut through the rectangle from a block's first cell to its last (one box over
/// the whole inline group) in `Measure::vertical_cut_crosses_a_proof`.
#[test]
fn a_formula_in_each_pane_on_one_row_does_not_refuse_the_cut() {
    let screen = (0..20)
        .map(|row| {
            format!(
                "{:<30}\u{2502}right $b_{row}$ too",
                format!("left $a_{row}$ here")
            )
        })
        .collect::<Vec<_>>();
    let capture = framed_capture(60, &screen);
    let (left, right) = (rect(0, 20, 0, 30), rect(0, 20, 31, 60));
    assert_eq!(panes(&capture), vec![left, right]);
    let blocks = proven(&capture);
    assert_eq!(count(&blocks, left, MathMode::Inline), 20);
    assert_eq!(blocks.len(), 20);
}

/// RED (69a) — **a framed pane is scanned from a neutral checkpoint** (R8). The capture's checkpoint
/// carries a `$$` opened in the frozen history above the grid. Unframed, the screen's first `$$` would
/// close it; framed, no scrollback line runs into a pane, so each pane reads its own `$$ … $$` as
/// the blocks its program printed.
///
/// MUTATION: scan a framed pane from the capture's checkpoint (`initial_context.clone()` for the
/// panes in `ScreenFrame::measure`).
#[test]
fn a_framed_pane_is_scanned_from_a_neutral_checkpoint() {
    let mut carried = DetectionContext::default();
    advance_detection_context(&mut carried, TranscriptId(1), "$$");
    let capture = framed_capture_from(100, &herdr("   "), carried);
    let right = rect(0, 13, 4, 100);
    assert_eq!(panes(&capture), vec![rect(0, 13, 0, 3), right]);
    let blocks = proven(&capture);
    assert_eq!(count(&blocks, right, MathMode::Display), 2);
}

/// RED (69a) — **a rule a few rows of text touch is still padded** (V2a asks for a majority). A
/// full-height table whose cells are padded except on three rows, where a long entry runs up to the
/// rule: the side is clipped on three rows of the forty it holds text on, which is a table's shape and
/// not a pane's, so nothing is cut.
///
/// MUTATION: read a side as clipped when any row touches the rule (`touching > 0` for the majority
/// in `Measure::v2`).
#[test]
fn a_rule_a_few_rows_of_text_touch_is_still_padded() {
    let mut table = vec!["\u{2502} name   \u{2502} value  \u{2502}".to_owned(); 40];
    for row in [7, 19, 31] {
        table[row] = "\u{2502} longest\u{2502} value  \u{2502}".to_owned();
    }
    assert_read_whole(&framed_capture(19, &table));
}

/// RED (69a) — **a plain rule beside a separator row anchors nothing** (H2's junction joins its
/// row). herdr draws a `─────` separator across its sidebar, ending at the cell left of its `│`
/// rule. That rule is a proven frame, but a plain `│` does not join the separator — only a `├`
/// would — so the sidebar is not cut there and stays one pane.
///
/// MUTATION: accept any vertical stroke just outside the rectangle as the row's junction.
#[test]
fn a_plain_rule_beside_a_separator_row_anchors_nothing() {
    let screen = (0..20)
        .map(|row| {
            let side = if row == 10 {
                "\u{2500}".repeat(25)
            } else {
                " ".repeat(25)
            };
            format!("{side}\u{2502}pane output {row}")
        })
        .collect::<Vec<_>>();
    let capture = framed_capture(60, &screen);
    assert_eq!(
        panes(&capture),
        vec![rect(0, 20, 0, 25), rect(0, 20, 26, 60)]
    );
}
