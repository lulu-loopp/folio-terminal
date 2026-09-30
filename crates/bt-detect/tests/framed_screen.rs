//! **Math on a screen a multiplexer has framed** (ticket 69a, T-PANE-COLUMNS; the design note
//! `docs/plans/design/pane-columns-2026-09-29.md`).

use std::sync::Arc;

use bt_detect::{
    BlockKind, DelimiterKind, DetectionContext, DetectionOptions, DetectionRevision,
    GridGeneration, GridPoint, InlineMathSite, LayoutKey, LiveDetectionInput, LiveDetectionSource,
    LiveDetectionTask, MathMode, MathSpan, SUBPIXELS_PER_PX, ScreenId, advance_detection_context,
    live_detection_isolation_gap, live_detection_ownership_ledger, resolve_live_detection_task,
    resolve_live_detection_tasks,
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

fn candidate(screen: &Screen, inputs: &Arc<[LiveDetectionInput]>, row: u32) -> LiveDetectionTask {
    LiveDetectionTask {
        candidate_row: row,
        screen: ScreenId::Alternate,
        grid_generation: GridGeneration(1),
        detection_revision: DetectionRevision(1),
        layout: layout(screen.width),
        cell_width_subpixels: 9 * SUBPIXELS_PER_PX,
        cell_height_subpixels: 18 * SUBPIXELS_PER_PX,
        ascii_baseline_subpixels: 14 * SUBPIXELS_PER_PX,
        options: screen.options,
        initial_context: screen.initial_context.clone(),
        inputs: Arc::clone(inputs),
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
    let inputs: Arc<[LiveDetectionInput]> = Arc::from(screen.inputs());
    let mut out = String::new();
    writeln!(out, "== {}", screen.name).unwrap();
    let mut tasks = (0..screen.grid.len() as u32)
        .map(|row| candidate(screen, &inputs, row))
        .collect::<Vec<_>>();
    resolve_live_detection_tasks(&mut tasks);
    for task in &tasks {
        // The batch and the single resolution agree, or the baseline would depend on which ran.
        let mut single = candidate(screen, &inputs, task.candidate_row);
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
    let ledger =
        live_detection_ownership_ledger(&inputs, screen.initial_context.clone(), screen.options);
    writeln!(out, "ledger={ledger:?}").unwrap();
    let gap = live_detection_isolation_gap(&inputs, screen.initial_context.clone(), screen.options);
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
