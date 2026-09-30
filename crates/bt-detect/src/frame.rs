//! **The frame of a live capture: which rectangles of the screen are panes a multiplexer draws.**
//!
//! Ticket 69a (T-PANE-COLUMNS); the specification is `docs/plans/design/pane-columns-2026-09-29.md`,
//! revision (c), and every rule below carries the note's name for it.
//!
//! A multiplexer does not pass its panes' bytes through. It repaints the host screen itself, and every
//! pane row carries the frame: `<frame cells>│<pane text>`. Read as one line, `<25 blanks>│$$` is
//! indented code whose trimmed form opens on `│`, and both refusals are right about the line they are
//! shown. What nobody ever printed is that line. So the detector is shown the line the program did
//! print: the captured grid is cut into **pane rectangles** by a guillotine tree (§2.2), and each pane
//! is scanned alone, from a neutral checkpoint, over its own rows and columns, with every gate of the
//! scanner unchanged (R8).
//!
//! **The door is [`LiveCapture`]** (§3). It holds one capture — the inputs, the parser checkpoint
//! before them and the options they are scanned under — and measures its frame once, on first ask, by
//! whichever lane asks. The frame lives exactly as long as the last holder of the capture. Nothing
//! else in the product constructs a frame, and nothing else slices a grid row into a pane.
//!
//! **What a cut is.** A column `c` of a rectangle `R` is a vertical cut when
//! - (V1) every row of `R` carries a vertical stroke at `c`, `R` has at least three rows, and the
//!   plain rule (not a junction) stands at `c` on at least three of them;
//! - (V2) the rule is a frame, not a table: the text beside it is *clipped* at it on one side (V2a),
//!   or one side is a blank gutter at least one column wide (V2b);
//! - (R5) no block the unsplit screen proves crosses it.
//!
//! Only when `R` has no vertical cut is a row `r` tried as a horizontal cut: (H1) every column of `R`
//! carries a horizontal stroke at `r`, (H2) a junction on that row — inside `R` or just outside it —
//! stands on a column that is a **proven** frame rule (V1 and V2) in the band its vertical stroke
//! runs into, and (R5). A horizontal rule never cuts on its own evidence, so a rectangle whose every
//! rule is padded is a table and is cut in neither direction. After the tree is built, a leaf that
//! holds only digits and blanks is a line-number gutter and refuses the whole frame (V2c).
//!
//! **What is never a stroke:** ASCII `|` and `-` (R4), and any glyph that is not one cell wide.
//!
//! **The status row** (§2.1). At most one edge row may be set aside, and only when setting it aside
//! makes a cut that it breaks, and only when it carries no delimiter the math grammar recognises
//! (owner's ruling 2026-09-17, kept 2026-09-29). No pane scans it; only the screen-fence pass (R9)
//! reads it, for fence state.

use std::sync::{Arc, OnceLock};

use crate::{
    DetectionContext, DetectionOptions, LiveDetectionInput, LiveDetectionSource, MathSourceLine,
    advance_detection_context, is_math_environment, live_occurrence_segments, live_scan,
};
use bt_transcript::TranscriptId;

/// One pane's rectangle, in screen coordinates: rows `top..bottom` of the live grid and cell columns
/// `left..right`. Always bounded (the note's `PaneRect { rows, columns }`, spelled as four numbers so
/// that it is `Copy` and orders as a map key).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PaneRect {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

impl PaneRect {
    #[must_use]
    pub fn rows(self) -> std::ops::Range<u32> {
        self.top..self.bottom
    }

    #[must_use]
    pub fn columns(self) -> std::ops::Range<u32> {
        self.left..self.right
    }

    #[must_use]
    pub fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }

    #[must_use]
    pub fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }

    #[must_use]
    pub fn contains_row(self, row: u32) -> bool {
        self.rows().contains(&row)
    }

    #[must_use]
    pub fn contains_column(self, column: u32) -> bool {
        self.columns().contains(&column)
    }
}

/// One pane of a frame: its rectangle, and the capture's rows as that pane reads them (R7) together
/// with the checkpoint its scan starts from (R8).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pane {
    pub rect: PaneRect,
    inputs: Arc<[LiveDetectionInput]>,
    initial_context: DetectionContext,
}

impl Pane {
    /// The pane's own lines. For the one whole-screen pane of an unframed screen this is the
    /// capture's input list itself — the same `Arc`, history tail included. For a framed pane it is
    /// one input per grid row of the rectangle, holding only the clusters wholly inside its columns,
    /// with every cell boundary still in **screen** columns.
    #[must_use]
    pub fn inputs(&self) -> &Arc<[LiveDetectionInput]> {
        &self.inputs
    }

    /// The checkpoint this pane's scan begins at: the capture's own for the whole-screen pane,
    /// neutral for a framed pane.
    #[must_use]
    pub fn initial_context(&self) -> &DetectionContext {
        &self.initial_context
    }
}

/// Which grid rows a **screen-owned** fence covers (R9): a fence open before the first grid row
/// (the checkpoint, carried through the frozen tail), or one opened on a row no pane owns. Such a
/// fence suppresses every pane's blocks on the rows it covers. Empty on an unframed screen, whose one
/// pane is scanned from the capture's own checkpoint and so honours those fences itself.
///
/// This is the value ticket 69b's screen tier compares (§8.2): a change in it clears every pane's
/// math exactly as a frame change does.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct ScreenFenceState {
    covered_rows: Vec<u32>,
}

impl ScreenFenceState {
    #[must_use]
    pub fn covers(&self, row: u32) -> bool {
        self.covered_rows.binary_search(&row).is_ok()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.covered_rows.is_empty()
    }
}

/// The frame of one capture: its panes (one whole-screen pane when the screen is unframed), the edge
/// status row it set aside, and the screen-owned fence state.
#[derive(Debug, Eq, PartialEq)]
pub struct ScreenFrame {
    panes: Vec<Pane>,
    status_row: Option<u32>,
    screen_fence: ScreenFenceState,
}

impl ScreenFrame {
    /// The panes, in reading order (top, then left).
    #[must_use]
    pub fn panes(&self) -> &[Pane] {
        &self.panes
    }

    /// The pane with exactly this rectangle.
    #[must_use]
    pub fn pane(&self, rect: PaneRect) -> Option<&Pane> {
        self.panes.iter().find(|pane| pane.rect == rect)
    }

    /// The edge row set aside as a status row (§2.1), if any.
    #[must_use]
    pub fn status_row(&self) -> Option<u32> {
        self.status_row
    }

    /// Whether a multiplexer's frame cut this screen at all. An unframed screen has exactly one pane
    /// and no status row, and its scan is byte for byte the scan that always ran.
    #[must_use]
    pub fn is_framed(&self) -> bool {
        self.panes.len() > 1 || self.status_row.is_some()
    }

    /// The screen-owned fence state (R9).
    #[must_use]
    pub fn screen_fence_state(&self) -> &ScreenFenceState {
        &self.screen_fence
    }

    /// **Whether two frames are the same layout** — the same rectangles and the same status row,
    /// compared by value and never by pointer (§8.2, "the current frame"). A frame that differs from
    /// the previous capture's is a frame change (R12).
    #[must_use]
    pub fn same_layout(&self, other: &Self) -> bool {
        self.status_row == other.status_row
            && self.panes.len() == other.panes.len()
            && self
                .panes
                .iter()
                .zip(&other.panes)
                .all(|(left, right)| left.rect == right.rect)
    }
}

#[cfg(test)]
thread_local! {
    /// How many frames this thread has measured: the test-only construction counter
    /// `a_capture_measures_its_frame_once` reads (a counter, not a timer).
    static FRAMES_MEASURED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// **The door** (§3): one live capture, the only thing every live entry point of the detector
/// accepts. It owns the derivation of its frame, measured once on first ask (by whichever lane asks —
/// the window thread's arming today) and never written again; the frame lives exactly as long as the
/// last holder of the capture.
#[derive(Clone)]
pub struct LiveCapture(Arc<CaptureInner>);

struct CaptureInner {
    inputs: Arc<[LiveDetectionInput]>,
    /// The parser checkpoint immediately before `inputs[0]`.
    initial_context: DetectionContext,
    options: DetectionOptions,
    frame: OnceLock<Arc<ScreenFrame>>,
}

impl LiveCapture {
    #[must_use]
    pub fn new(
        inputs: impl Into<Arc<[LiveDetectionInput]>>,
        initial_context: DetectionContext,
        options: DetectionOptions,
    ) -> Self {
        Self(Arc::new(CaptureInner {
            inputs: inputs.into(),
            initial_context,
            options,
            frame: OnceLock::new(),
        }))
    }

    /// The captured rows: an optional bounded tail of frozen history, then every grid row.
    #[must_use]
    pub fn inputs(&self) -> &Arc<[LiveDetectionInput]> {
        &self.0.inputs
    }

    /// The parser checkpoint immediately before `inputs()[0]`.
    #[must_use]
    pub fn initial_context(&self) -> &DetectionContext {
        &self.0.initial_context
    }

    #[must_use]
    pub fn options(&self) -> DetectionOptions {
        self.0.options
    }

    /// The frame, measured on the first ask.
    #[must_use]
    pub fn frame(&self) -> &Arc<ScreenFrame> {
        self.0.frame.get_or_init(|| {
            #[cfg(test)]
            FRAMES_MEASURED.with(|count| count.set(count.get() + 1));
            Arc::new(ScreenFrame::measure(
                &self.0.inputs,
                &self.0.initial_context,
                self.0.options,
            ))
        })
    }

    /// The whole screen as one rectangle — every grid row, every captured column — without measuring
    /// the frame. What a task carries before it is resolved.
    #[must_use]
    pub fn screen_rect(&self) -> PaneRect {
        screen_rect(&self.0.inputs)
    }

    /// **The rows a block proven in `rect` reads**: that pane's inputs, as the pane reads them (R7).
    /// The whole-screen rectangle a task carries before it is resolved names the capture itself.
    /// `None` for a rectangle that is neither.
    #[must_use]
    pub fn pane_inputs(&self, rect: PaneRect) -> Option<&Arc<[LiveDetectionInput]>> {
        self.frame()
            .pane(rect)
            .map(Pane::inputs)
            .or_else(|| (rect == self.screen_rect()).then(|| self.inputs()))
    }

    /// The same capture, not merely an equal one.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl PartialEq for LiveCapture {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
            || (self.0.options == other.0.options
                && self.0.initial_context == other.0.initial_context
                && self.0.inputs == other.0.inputs)
    }
}

impl Eq for LiveCapture {}

impl std::fmt::Debug for LiveCapture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LiveCapture")
            .field("inputs", &self.0.inputs)
            .field("initial_context", &self.0.initial_context)
            .field("options", &self.0.options)
            .finish_non_exhaustive()
    }
}

/// The grid inputs of a capture: every input whose source is a live-grid row, with its row.
fn grid_inputs(inputs: &[LiveDetectionInput]) -> impl Iterator<Item = (u32, &LiveDetectionInput)> {
    inputs.iter().filter_map(|input| match input.source {
        LiveDetectionSource::Grid { row, .. } => Some((row, input)),
        LiveDetectionSource::History { .. } => None,
    })
}

/// The capture's width in cells: the width its grid rows were captured on, or — for inputs that
/// carry no capture geometry — the widest row's last cell.
fn screen_width(inputs: &[LiveDetectionInput]) -> u32 {
    grid_inputs(inputs)
        .map(|(_, input)| {
            input
                .captured_columns
                .max(input.cell_boundaries.last().map_or(0, |(_, cell)| *cell))
        })
        .max()
        .unwrap_or(0)
}

fn screen_rect(inputs: &[LiveDetectionInput]) -> PaneRect {
    let mut rows = grid_inputs(inputs).map(|(row, _)| row);
    let top = rows.next().unwrap_or(0);
    let bottom = rows.last().unwrap_or(top).saturating_add(1);
    PaneRect {
        top,
        bottom: if grid_inputs(inputs).next().is_some() {
            bottom
        } else {
            top
        },
        left: 0,
        right: screen_width(inputs),
    }
}

/// The stroke directions a box-drawing glyph draws, from its Unicode name (U+2500–U+257F).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Strokes {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

impl Strokes {
    fn vertical(self) -> bool {
        self.up || self.down
    }

    fn horizontal(self) -> bool {
        self.left || self.right
    }
}

/// The strokes of one box-drawing glyph, or `None` when `glyph` is not in U+2500–U+257F. The table
/// follows the block's names: `LIGHT VERTICAL AND RIGHT` draws up, down and right, `ARC DOWN AND
/// LEFT` down and left, a `DIAGONAL` nothing this model reads.
fn box_strokes(glyph: char) -> Option<Strokes> {
    const fn s(up: bool, down: bool, left: bool, right: bool) -> Strokes {
        Strokes {
            up,
            down,
            left,
            right,
        }
    }
    let code = u32::from(glyph);
    Some(match code {
        0x2500 | 0x2501 | 0x2504 | 0x2505 | 0x2508 | 0x2509 | 0x254C | 0x254D | 0x2550 => {
            s(false, false, true, true)
        }
        0x2502 | 0x2503 | 0x2506 | 0x2507 | 0x250A | 0x250B | 0x254E | 0x254F | 0x2551 => {
            s(true, true, false, false)
        }
        0x250C..=0x250F | 0x2552..=0x2554 | 0x256D => s(false, true, false, true),
        0x2510..=0x2513 | 0x2555..=0x2557 | 0x256E => s(false, true, true, false),
        0x2514..=0x2517 | 0x2558..=0x255A | 0x2570 => s(true, false, false, true),
        0x2518..=0x251B | 0x255B..=0x255D | 0x256F => s(true, false, true, false),
        0x251C..=0x2523 | 0x255E..=0x2560 => s(true, true, false, true),
        0x2524..=0x252B | 0x2561..=0x2563 => s(true, true, true, false),
        0x252C..=0x2533 | 0x2564..=0x2566 => s(false, true, true, true),
        0x2534..=0x253B | 0x2567..=0x2569 => s(true, false, true, true),
        0x253C..=0x254B | 0x256A..=0x256C => s(true, true, true, true),
        0x2571..=0x2573 => s(false, false, false, false),
        0x2574 | 0x2578 => s(false, false, true, false),
        0x2575 | 0x2579 => s(true, false, false, false),
        0x2576 | 0x257A => s(false, false, false, true),
        0x2577 | 0x257B => s(false, true, false, false),
        0x257C | 0x257E => s(false, false, true, true),
        0x257D | 0x257F => s(true, true, false, false),
        _ => return None,
    })
}

/// The plain vertical rule — `│ ┃ ┆ ┇ ┊ ┋ ╎ ╏ ║`, a stroke and nothing else. A junction continues a
/// rule (V1's "every row") but never counts towards the three rows a rule must be drawn on.
fn is_plain_rule(glyph: char) -> bool {
    matches!(
        glyph,
        '\u{2502}'
            | '\u{2503}'
            | '\u{2506}'
            | '\u{2507}'
            | '\u{250A}'
            | '\u{250B}'
            | '\u{254E}'
            | '\u{254F}'
            | '\u{2551}'
    )
}

/// What one cell of the captured grid holds, as far as the frame is concerned.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Cell {
    #[default]
    Blank,
    /// Anything a program wrote that is not a one-cell box-drawing stroke: the text V2 asks about.
    Text,
    Stroke {
        strokes: Strokes,
        plain_rule: bool,
    },
}

impl Cell {
    fn strokes(self) -> Strokes {
        match self {
            Self::Stroke { strokes, .. } => strokes,
            Self::Blank | Self::Text => Strokes::default(),
        }
    }
}

/// The clusters of one captured row, as `(byte range, cell range)`, read off its captured boundaries.
fn clusters(input: &LiveDetectionInput) -> impl Iterator<Item = (usize, usize, u32, u32)> + '_ {
    input.cell_boundaries.windows(2).filter_map(|pair| {
        let (start_byte, start_cell) = pair[0];
        let (end_byte, end_cell) = pair[1];
        (end_byte > start_byte).then_some((
            start_byte as usize,
            end_byte as usize,
            start_cell,
            end_cell,
        ))
    })
}

fn row_cells(input: &LiveDetectionInput, width: u32) -> Vec<Cell> {
    let mut cells = vec![Cell::Blank; width as usize];
    for (start_byte, end_byte, start_cell, end_cell) in clusters(input) {
        let Some(cluster) = input.text.get(start_byte..end_byte) else {
            continue;
        };
        if cluster
            .chars()
            .all(|character| character == ' ' || character == '\t')
        {
            continue;
        }
        let mut characters = cluster.chars();
        let single = characters.next().filter(|_| characters.next().is_none());
        let kind = match single.and_then(|glyph| box_strokes(glyph).map(|strokes| (glyph, strokes)))
        {
            Some((glyph, strokes)) if end_cell == start_cell.saturating_add(1) => Cell::Stroke {
                strokes,
                plain_rule: is_plain_rule(glyph),
            },
            _ => Cell::Text,
        };
        for column in start_cell..end_cell.min(width) {
            cells[column as usize] = kind;
        }
    }
    cells
}

/// **Does this line carry any delimiter the math grammar recognises?** `$`, `\[ \] \( \)`, or a
/// `\begin{…}`/`\end{…}` naming a math environment — the grammar's question, not a character's,
/// because `\[x^2\]` carries no dollar and is a display formula (owner's ruling 2026-09-17). A row
/// that does may not be set aside as a status row (§2.1, §2.4).
fn line_carries_math_delimiter(text: &str) -> bool {
    if text.contains('$') {
        return true;
    }
    if !text.contains('\\') {
        return false;
    }
    if [r"\[", r"\]", r"\(", r"\)"]
        .iter()
        .any(|delimiter| text.contains(delimiter))
    {
        return true;
    }
    [r"\begin{", r"\end{"].iter().any(|prefix| {
        text.match_indices(prefix).any(|(at, _)| {
            text[at + prefix.len()..]
                .split_once('}')
                .is_some_and(|(environment, _)| is_math_environment(environment))
        })
    })
}

/// The cells of one block the unsplit screen proves (R5), one live row's run of them at a time: the
/// block's live cell segments. A cut through any of them is a cut through the block.
///
/// Segment by segment and not as one rectangle over the block (revision (d) of the note): an inline
/// occurrence groups every `$…$` run of its line, so a rectangle from its first run's first cell to
/// its last run's last cell would cover the rule between two panes that each hold a formula on the
/// same row, and refuse every such split — while no cell of either formula is on the rule. A display
/// block's segments cover each of its rows whole, so a rule anywhere inside one of its lines is still
/// a cut through it.
#[derive(Clone, Copy, Debug)]
struct ProvenCells {
    row: u32,
    left: u32,
    right: u32,
}

/// A candidate cut, for comparing two readings of the root (§2.1's status-row test).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cut {
    Vertical(u32),
    Horizontal(u32),
}

/// The measuring pass: the grid as cells, and the unsplit proof, asked for only when a cut is tried.
struct Measure<'a> {
    inputs: &'a [LiveDetectionInput],
    initial_context: &'a DetectionContext,
    options: DetectionOptions,
    /// Grid row number of `cells[0]`.
    first_row: u32,
    width: u32,
    /// One entry per grid row, in row order.
    cells: Vec<Vec<Cell>>,
    grid: Vec<&'a LiveDetectionInput>,
    unsplit: OnceLock<Vec<ProvenCells>>,
}

impl<'a> Measure<'a> {
    fn cell(&self, row: u32, column: u32) -> Cell {
        self.cells
            .get(row.saturating_sub(self.first_row) as usize)
            .and_then(|cells| cells.get(column as usize))
            .copied()
            .unwrap_or_default()
    }

    /// R5's evidence: the live cells of every block today's scan proves over the unsplit capture.
    fn unsplit(&self) -> &[ProvenCells] {
        self.unsplit.get_or_init(|| {
            let scan = live_scan(self.inputs, self.initial_context, self.options);
            scan.scan
                .blocks
                .iter()
                .filter_map(|block| {
                    live_occurrence_segments(&block.span, block.start, &scan.logical, self.inputs)
                })
                .flatten()
                .filter_map(|segment| match segment.source_line {
                    MathSourceLine::LiveGrid(row) => Some(ProvenCells {
                        row,
                        left: segment.cell_start,
                        right: segment.cell_end,
                    }),
                    MathSourceLine::Transcript(_) => None,
                })
                .collect()
        })
    }

    /// (R5) Does a block the unsplit screen proves have a cell in column `column` on a row of `rect`?
    fn vertical_cut_crosses_a_proof(&self, rect: PaneRect, column: u32) -> bool {
        self.unsplit().iter().any(|proof| {
            rect.contains_row(proof.row) && proof.left <= column && column < proof.right
        })
    }

    /// (R5) Does a block the unsplit screen proves have a cell on row `row` in the columns of `rect`?
    /// A block's rows are consecutive and every one of them carries a segment, so a block standing
    /// above and below the row stands on it too.
    fn horizontal_cut_crosses_a_proof(&self, rect: PaneRect, row: u32) -> bool {
        self.unsplit()
            .iter()
            .any(|proof| proof.row == row && proof.left < rect.right && proof.right > rect.left)
    }

    /// (V1) Every row of `rect` carries a vertical stroke at `column`, `rect` has at least three
    /// rows, and the plain rule stands there on at least three of them.
    fn v1(&self, rect: PaneRect, column: u32) -> bool {
        if rect.height() < 3 {
            return false;
        }
        let mut plain = 0usize;
        for row in rect.rows() {
            match self.cell(row, column) {
                Cell::Stroke {
                    strokes,
                    plain_rule,
                } if strokes.vertical() => plain += usize::from(plain_rule),
                _ => return false,
            }
        }
        plain >= 3
    }

    /// Rows of `rect` on which the columns `columns` hold any text, and how many of those hold text
    /// in `adjacent` (the cell next to the rule).
    fn side_text(
        &self,
        rect: PaneRect,
        columns: std::ops::Range<u32>,
        adjacent: u32,
    ) -> (usize, usize) {
        let mut with_text = 0usize;
        let mut touching = 0usize;
        for row in rect.rows() {
            if columns
                .clone()
                .any(|column| self.cell(row, column) == Cell::Text)
            {
                with_text += 1;
                touching += usize::from(self.cell(row, adjacent) == Cell::Text);
            }
        }
        (with_text, touching)
    }

    /// (V2) Is the rule at `column` a frame and not a table? `neighbours` are the other columns of
    /// `rect` that pass V1: a side runs from the rule to the next of them or the edge of `rect`.
    fn v2(&self, rect: PaneRect, column: u32, neighbours: &[u32]) -> bool {
        let left_edge = neighbours
            .iter()
            .rev()
            .find(|other| **other < column)
            .map_or(rect.left, |other| other + 1);
        let right_edge = neighbours
            .iter()
            .find(|other| **other > column)
            .copied()
            .unwrap_or(rect.right);
        let sides = [
            (left_edge..column, column.checked_sub(1)),
            (column + 1..right_edge, Some(column + 1)),
        ];
        sides.into_iter().any(|(columns, adjacent)| {
            let width = columns.end.saturating_sub(columns.start);
            let (with_text, touching) = match adjacent {
                Some(adjacent) if width > 0 => self.side_text(rect, columns, adjacent),
                _ => (0, 0),
            };
            // (V2a) clipped: most rows with text on this side have it in the cell next to the rule.
            let clipped = with_text > 0 && touching * 2 > with_text;
            // (V2b) a blank gutter, at least one column wide.
            let gutter = width > 0 && with_text == 0;
            clipped || gutter
        })
    }

    /// The columns of `rect` that pass V1.
    fn v1_columns(&self, rect: PaneRect) -> Vec<u32> {
        rect.columns()
            .filter(|column| self.v1(rect, *column))
            .collect()
    }

    /// Whether `column` is a proven frame rule (V1 and V2) over `rect`.
    fn proven_rule(&self, rect: PaneRect, column: u32) -> bool {
        let candidates = self.v1_columns(rect);
        candidates.contains(&column) && self.v2(rect, column, &candidates)
    }

    fn vertical_cuts(&self, rect: PaneRect) -> Vec<u32> {
        let candidates = self.v1_columns(rect);
        candidates
            .iter()
            .copied()
            .filter(|column| {
                self.v2(rect, *column, &candidates)
                    && !self.vertical_cut_crosses_a_proof(rect, *column)
            })
            .collect()
    }

    /// (H1) Every column of `rect` carries a horizontal stroke at `row`.
    fn h1(&self, rect: PaneRect, row: u32) -> bool {
        rect.width() > 0
            && rect
                .columns()
                .all(|column| self.cell(row, column).strokes().horizontal())
    }

    /// (H1, H2, R5) The horizontal cuts of `rect`. `outer` is the rectangle `rect` was cut from, whose
    /// columns include the cells just outside `rect` at either end.
    fn horizontal_cuts(&self, rect: PaneRect, outer: PaneRect) -> Vec<u32> {
        let full = rect
            .rows()
            .filter(|row| self.h1(rect, *row))
            .collect::<Vec<_>>();
        full.iter()
            .copied()
            .filter(|row| {
                self.anchored(rect, outer, *row, &full)
                    && !self.horizontal_cut_crosses_a_proof(rect, *row)
            })
            .collect()
    }

    /// (H2) A junction on `row` — inside `rect`, or the cell just outside it at either end — whose
    /// vertical stroke continues into an adjacent band, where its column is a proven frame rule.
    fn anchored(&self, rect: PaneRect, outer: PaneRect, row: u32, full: &[u32]) -> bool {
        let above = full
            .iter()
            .rev()
            .find(|other| **other < row)
            .map_or(rect.top, |other| other + 1);
        let below = full
            .iter()
            .find(|other| **other > row)
            .copied()
            .unwrap_or(rect.bottom);
        let outside = [
            rect.left
                .checked_sub(1)
                .filter(|column| *column >= outer.left),
            Some(rect.right).filter(|column| *column < outer.right),
        ];
        let junctions = rect.columns().chain(outside.into_iter().flatten());
        junctions.into_iter().any(|column| {
            let strokes = self.cell(row, column).strokes();
            // A junction *of this row*: a cell inside `rect` carries the row's stroke by H1; one
            // just outside it has to reach into the rectangle — `├` left of it, `┤` right of it. A
            // plain `│` beside a `─` row joins nothing.
            let joins_the_row = if column < rect.left {
                strokes.right
            } else if column >= rect.right {
                strokes.left
            } else {
                true
            };
            if !joins_the_row {
                return false;
            }
            let columns = if rect.contains_column(column) {
                (rect.left, rect.right)
            } else {
                (outer.left, outer.right)
            };
            let band = |top: u32, bottom: u32| PaneRect {
                top,
                bottom,
                left: columns.0,
                right: columns.1,
            };
            (strokes.up && above < row && self.proven_rule(band(above, row), column))
                || (strokes.down
                    && row + 1 < below
                    && self.proven_rule(band(row + 1, below), column))
        })
    }

    /// The cuts of `rect` at the top level: its vertical cuts, or — when it has none — its horizontal
    /// ones.
    fn top_level_cuts(&self, rect: PaneRect, outer: PaneRect) -> Vec<Cut> {
        let vertical = self.vertical_cuts(rect);
        if !vertical.is_empty() {
            return vertical.into_iter().map(Cut::Vertical).collect();
        }
        self.horizontal_cuts(rect, outer)
            .into_iter()
            .map(Cut::Horizontal)
            .collect()
    }

    /// §2.2's `split`: the leaves of the guillotine tree under `rect`.
    fn split(&self, rect: PaneRect, outer: PaneRect, leaves: &mut Vec<PaneRect>) {
        let vertical = self.vertical_cuts(rect);
        if !vertical.is_empty() {
            let mut left = rect.left;
            for cut in vertical.iter().copied().chain(std::iter::once(rect.right)) {
                if cut > left {
                    self.split(
                        PaneRect {
                            left,
                            right: cut,
                            ..rect
                        },
                        rect,
                        leaves,
                    );
                }
                left = cut + 1;
            }
            return;
        }
        let horizontal = self.horizontal_cuts(rect, outer);
        if !horizontal.is_empty() {
            let mut top = rect.top;
            for cut in horizontal
                .iter()
                .copied()
                .chain(std::iter::once(rect.bottom))
            {
                if cut > top {
                    self.split(
                        PaneRect {
                            top,
                            bottom: cut,
                            ..rect
                        },
                        outer,
                        leaves,
                    );
                }
                top = cut + 1;
            }
            return;
        }
        leaves.push(rect);
    }
}

impl ScreenFrame {
    /// The one whole-screen pane: the capture itself, scanned from its own checkpoint.
    fn unframed(inputs: &Arc<[LiveDetectionInput]>, initial_context: &DetectionContext) -> Self {
        Self {
            panes: vec![Pane {
                rect: screen_rect(inputs),
                inputs: Arc::clone(inputs),
                initial_context: initial_context.clone(),
            }],
            status_row: None,
            screen_fence: ScreenFenceState::default(),
        }
    }

    /// Measure the frame of one capture (§2). Pure: the same capture always measures the same frame.
    fn measure(
        inputs: &Arc<[LiveDetectionInput]>,
        initial_context: &DetectionContext,
        options: DetectionOptions,
    ) -> Self {
        // The fast exit, run on every capture: no grid row holds a box-drawing glyph.
        if !grid_inputs(inputs).any(|(_, input)| {
            input
                .text
                .chars()
                .any(|character| ('\u{2500}'..='\u{257F}').contains(&character))
        }) {
            return Self::unframed(inputs, initial_context);
        }
        let screen = screen_rect(inputs);
        let grid = grid_inputs(inputs)
            .map(|(_, input)| input)
            .collect::<Vec<_>>();
        let measure = Measure {
            inputs,
            initial_context,
            options,
            first_row: screen.top,
            width: screen.right,
            cells: grid
                .iter()
                .map(|input| row_cells(input, screen.right))
                .collect(),
            grid,
            unsplit: OnceLock::new(),
        };
        // The cheap tally: V2, R5 and the recursion run only when some column carries the plain
        // rule on at least three rows — V1's floor, which every cut needs somewhere: a vertical cut
        // over its own rectangle, a horizontal one over the band its anchor stands in.
        let any_rule = (0..screen.right).any(|column| {
            measure
                .cells
                .iter()
                .filter(|cells| {
                    matches!(
                        cells.get(column as usize),
                        Some(Cell::Stroke {
                            plain_rule: true,
                            ..
                        })
                    )
                })
                .nth(2)
                .is_some()
        });
        if !any_rule {
            return Self::unframed(inputs, initial_context);
        }
        let without = |row: u32| PaneRect {
            top: if row == screen.top {
                screen.top + 1
            } else {
                screen.top
            },
            bottom: if row == screen.top {
                screen.bottom
            } else {
                screen.bottom - 1
            },
            ..screen
        };
        let edges = if screen.height() >= 2 {
            vec![screen.bottom - 1, screen.top]
        } else {
            Vec::new()
        };

        // §2.1: at most one edge row, set aside only when doing so makes a cut it breaks, and only
        // when it carries no math delimiter. The bottom row is asked first (tmux's default place).
        let full_cuts = measure.top_level_cuts(screen, screen);
        let status_row = edges.iter().copied().find(|row| {
            let text = measure.grid[(row - screen.top) as usize].text.as_str();
            if line_carries_math_delimiter(text) {
                return false;
            }
            let rect = without(*row);
            measure
                .top_level_cuts(rect, rect)
                .iter()
                .any(|cut| !full_cuts.contains(cut))
        });
        let root = status_row.map_or(screen, without);
        let mut leaves = Vec::new();
        measure.split(root, root, &mut leaves);
        if leaves.len() <= 1 {
            return Self::unframed(inputs, initial_context);
        }
        leaves.sort_by_key(|rect| (rect.top, rect.left));
        let panes = leaves
            .into_iter()
            .map(|rect| Pane {
                rect,
                inputs: measure
                    .grid
                    .iter()
                    .filter(|input| {
                        matches!(input.source, LiveDetectionSource::Grid { row, .. } if rect.contains_row(row))
                    })
                    .map(|input| pane_input(input, rect, measure.width))
                    .collect(),
                initial_context: DetectionContext::default(),
            })
            .collect::<Vec<_>>();
        // (V2c) A leaf of digits and blanks is a line-number gutter: a side-by-side diff or a numbered
        // listing, never panes. It refuses the whole frame.
        if panes.iter().any(|pane| {
            let mut digits = false;
            let only_digits = pane.inputs.iter().all(|input| {
                input.text.chars().all(|character| {
                    digits |= character.is_ascii_digit();
                    character.is_ascii_digit() || character.is_whitespace()
                })
            });
            only_digits && digits
        }) {
            return Self::unframed(inputs, initial_context);
        }
        let screen_fence = screen_fence_pass(inputs, initial_context, &panes, &measure);
        Self {
            panes,
            status_row,
            screen_fence,
        }
    }
}

/// **The screen-fence pass** (R9): the fence state the checkpoint carries through the frozen tail,
/// advanced over the rows no pane owns (the status row, the rule rows), never looking for math. The
/// grid rows a pane owns while that state is inside a fence are the rows the fence suppresses in
/// every pane.
fn screen_fence_pass(
    inputs: &[LiveDetectionInput],
    initial_context: &DetectionContext,
    panes: &[Pane],
    measure: &Measure<'_>,
) -> ScreenFenceState {
    let mut context = initial_context.clone();
    for input in inputs {
        if let LiveDetectionSource::History { id } = input.source {
            advance_detection_context(&mut context, id, &input.text);
        }
    }
    let mut covered_rows = Vec::new();
    for input in &measure.grid {
        let LiveDetectionSource::Grid { row, .. } = input.source else {
            continue;
        };
        let owned = panes.iter().any(|pane| pane.rect.contains_row(row));
        if owned {
            if context.is_commonmark_code() {
                covered_rows.push(row);
            }
            // A row that is partly a pane's and partly a rule's is advanced by nothing: only a fence
            // marker standing alone on a row no pane owns is the screen's.
            continue;
        }
        // Only fence state is read here: the row carries no math this pass looks for (a status row
        // with a delimiter is never set aside, §2.1), so advancing the checkpoint over it moves
        // nothing but the fence.
        advance_detection_context(&mut context, TranscriptId(0), &input.text);
    }
    ScreenFenceState { covered_rows }
}

/// **R7: one grid row as a pane reads it.** The row's clusters that lie wholly inside the pane's
/// columns; a cluster straddling an edge belongs to neither side. The cell boundaries stay **screen**
/// columns, so every coordinate the pane proves is already where the grid drew it. A row that ends
/// its logical line drops its trailing blanks; one that continues keeps them (§4.6a) — and only a
/// pane that spans the full width can continue, since a soft wrap lands in column zero of the next
/// row.
fn pane_input(input: &LiveDetectionInput, rect: PaneRect, screen_width: u32) -> LiveDetectionInput {
    let continues = input.continues && rect.left == 0 && rect.right >= screen_width;
    let mut text = String::new();
    let mut boundaries = Vec::new();
    for (start_byte, end_byte, start_cell, end_cell) in clusters(input) {
        let inside = start_cell >= rect.left
            && end_cell <= rect.right
            && (end_cell > start_cell || start_cell < rect.right);
        if !inside {
            continue;
        }
        let Some(cluster) = input.text.get(start_byte..end_byte) else {
            continue;
        };
        if boundaries.is_empty() {
            boundaries.push((0, start_cell));
        }
        text.push_str(cluster);
        boundaries.push((u32::try_from(text.len()).unwrap_or(u32::MAX), end_cell));
    }
    if boundaries.is_empty() {
        boundaries.push((0, rect.left));
    }
    if !continues {
        let kept = text.trim_end_matches([' ', '\t']).len();
        text.truncate(kept);
        let kept = u32::try_from(kept).unwrap_or(u32::MAX);
        let last_cell = boundaries
            .iter()
            .rev()
            .find(|(byte, _)| *byte <= kept)
            .map_or(rect.left, |(_, cell)| *cell);
        boundaries.retain(|(byte, _)| *byte <= kept);
        if boundaries.last().is_none_or(|(byte, _)| *byte != kept) {
            boundaries.push((kept, last_cell));
        }
    }
    LiveDetectionInput {
        source: input.source,
        text,
        continues,
        captured_columns: if input.captured_columns == 0 {
            0
        } else {
            rect.width()
        },
        cell_boundaries: boundaries,
        site: input.site,
    }
}

#[cfg(test)]
mod tests {
    //! The branch's unit tests, re-stated for rectangles (note §7.1, last paragraph), and the
    //! construction counter. `nine_tenths_is_enough_and_less_is_not` is retired: V1's every-row rule
    //! replaced the share (§10).

    use super::*;
    use crate::InlineMathSite;

    /// A capture of these rows on a screen `width` cells wide: boundaries from `bt_unicode` cluster
    /// widths, trailing blanks trimmed, neutral checkpoint.
    fn capture(width: u32, rows: &[&str]) -> LiveCapture {
        let inputs = rows
            .iter()
            .enumerate()
            .map(|(row, text)| {
                let text = text.trim_end_matches(' ');
                let mut boundaries = vec![(0, 0)];
                let (mut byte, mut cell) = (0u32, 0u32);
                for cluster in bt_unicode::graphemes(text) {
                    byte += cluster.len() as u32;
                    cell += bt_unicode::cluster_width(cluster) as u32;
                    boundaries.push((byte, cell));
                }
                LiveDetectionInput {
                    source: LiveDetectionSource::Grid {
                        row: row as u32,
                        revision: 1,
                    },
                    text: text.to_owned(),
                    continues: false,
                    captured_columns: width,
                    cell_boundaries: boundaries,
                    site: InlineMathSite::AltScreenContent,
                }
            })
            .collect::<Vec<_>>();
        LiveCapture::new(
            inputs,
            DetectionContext::default(),
            DetectionOptions::default(),
        )
    }

    fn columns(capture: &LiveCapture) -> Vec<(u32, u32)> {
        capture
            .frame()
            .panes()
            .iter()
            .map(|pane| (pane.rect.left, pane.rect.right))
            .collect()
    }

    /// RED (69a) — **a capture measures its frame once**, however many tasks ask (Codex B-6): two
    /// tasks sharing one capture, the arming walk and the resolution, construct one `ScreenFrame`.
    ///
    /// MUTATION: build the frame in `LiveCapture::frame` without the `OnceLock`.
    #[test]
    fn a_capture_measures_its_frame_once() {
        let rows = ["   │$$", "   │x", "   │$$", "   │text"];
        let shared = capture(20, &rows);
        let before = FRAMES_MEASURED.with(std::cell::Cell::get);
        let (first, second) = (shared.clone(), shared.clone());
        let _ = first.frame();
        let _ = second.frame();
        let _ = shared.frame();
        assert_eq!(FRAMES_MEASURED.with(std::cell::Cell::get) - before, 1);
        // An equal capture built again is another capture, and measures its own.
        let _ = capture(20, &rows).frame();
        assert_eq!(FRAMES_MEASURED.with(std::cell::Cell::get) - before, 2);
    }

    /// RED (69a) — **one crossing row at an edge is a status line**, and it is set aside, not
    /// sliced.
    ///
    /// MUTATION: slice the status row into the panes.
    #[test]
    fn one_crossing_row_at_an_edge_is_a_status_line() {
        let mut rows = vec!["left │right"; 10];
        rows.push("[0] 0:bash* 12:00 status");
        let capture = capture(40, &rows);
        let frame = capture.frame();
        assert_eq!(frame.status_row(), Some(10));
        assert_eq!(columns(&capture), vec![(0, 5), (6, 40)]);
        assert!(
            frame
                .panes()
                .iter()
                .all(|pane| pane.rect.rows() == (0..10) && pane.inputs().len() == 10)
        );
    }

    /// RED (69a) — **a junction keeps the vertical border and a plain horizontal breaks it** (V1):
    /// `┼` carries the stroke on its row, `─` does not.
    ///
    /// MUTATION: count a horizontal-only glyph as a vertical stroke in `box_strokes`.
    #[test]
    fn a_junction_keeps_the_vertical_border_and_a_plain_horizontal_breaks_it() {
        let mut rows = vec!["left │right"; 10];
        rows[4] = "─────┼──────";
        // The rule stands on every row, the `┼` included, so the screen is cut at column 5; the
        // `┼` then anchors row 4 on that proven rule, and each strip is cut there too.
        let joined = capture(12, &rows);
        assert_eq!(
            joined
                .frame()
                .panes()
                .iter()
                .map(|pane| pane.rect)
                .collect::<Vec<_>>(),
            vec![
                PaneRect {
                    top: 0,
                    bottom: 4,
                    left: 0,
                    right: 5
                },
                PaneRect {
                    top: 0,
                    bottom: 4,
                    left: 6,
                    right: 12
                },
                PaneRect {
                    top: 5,
                    bottom: 10,
                    left: 0,
                    right: 5
                },
                PaneRect {
                    top: 5,
                    bottom: 10,
                    left: 6,
                    right: 12
                },
            ]
        );
        rows[4] = "────────────";
        let broken = capture(12, &rows);
        assert!(!broken.frame().is_framed());
    }

    /// RED (69a) — **ASCII pipes never cut a screen** (R4): `mysql` and `column -t` draw them, and
    /// `bt_detect::table` owns what they draw.
    ///
    /// MUTATION: add `|` to `box_strokes`.
    #[test]
    fn ascii_pipes_never_cut_a_screen() {
        let rows = vec!["left |right"; 10];
        assert!(!capture(12, &rows).frame().is_framed());
    }

    /// RED (69a) — **wide glyphs are counted in cells**: a CJK sidebar of eight cells puts the rule
    /// in column 8, not in character column 4.
    ///
    /// MUTATION: index cells by character in `row_cells`.
    #[test]
    fn wide_glyphs_are_counted_in_cells() {
        let rows = vec!["目录条目│text"; 6];
        assert_eq!(columns(&capture(20, &rows)), vec![(0, 8), (9, 20)]);
    }

    /// RED (69a) — **junctions are not rules**: a column carrying only junctions has no plain rule on
    /// three rows, so it nominates nothing (V1's three plain rows).
    ///
    /// MUTATION: count junctions towards V1's three plain rows.
    #[test]
    fn junctions_are_not_rules() {
        let rows = vec!["left ┼right"; 10];
        assert!(!capture(12, &rows).frame().is_framed());
    }

    /// RED (69a) — **a rule in column zero leaves one region**: it has no outer side at all, which is
    /// not a gutter, and the strip left of it has no width.
    ///
    /// MUTATION: keep zero-width strips in `Measure::split`.
    #[test]
    fn a_rule_in_column_zero_leaves_one_region() {
        let rows = vec!["│text"; 10];
        assert!(!capture(10, &rows).frame().is_framed());
    }

    /// RED (69a) — **a region keeps its own indentation**: a pane row is the pane's cells as the
    /// program wrote them, leading blanks included, so the pane's own indented-code gate still sees
    /// four spaces.
    ///
    /// MUTATION: trim leading blanks in `pane_input`.
    #[test]
    fn a_region_keeps_its_own_indentation() {
        let mut rows = vec!["ab │text"; 6];
        rows[2] = "ab │    $$x$$";
        let capture = capture(20, &rows);
        let right = &capture.frame().panes()[1];
        assert_eq!(right.rect.columns(), 4..20);
        assert_eq!(right.inputs()[2].text, "    $$x$$");
        assert_eq!(right.inputs()[2].cell_boundaries.first(), Some(&(0, 4)));
    }
}
