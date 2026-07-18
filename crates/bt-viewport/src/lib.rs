//! Per-viewport projection, layout cache and scroll anchoring.

mod height_tree;

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    num::{NonZeroI64, NonZeroU32},
};

use bt_doc::{
    AnchorError, Bias, ContentAnchor, DetectionRevision, GridGeneration, GridPoint,
    HistoryDocument, LayoutKey, ScreenId, ViewGeneration, compare_anchors,
};
use bt_transcript::{
    CapturedCell, CapturedRow, CellFlags, FrozenLine, GraphemeOffset, SourceGeneration, StagedRow,
    TranscriptId,
};
use bt_unicode::{cluster_width, graphemes};

pub use bt_doc::SUBPIXELS_PER_PX;
pub use height_tree::HeightTree;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TranscriptSpan {
    pub start: TranscriptId,
    pub end: TranscriptId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LayoutCacheKey {
    pub span: TranscriptSpan,
    pub source_gen: SourceGeneration,
    pub detection_rev: DetectionRevision,
    pub layout: LayoutKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeasuredLayout {
    pub height: i64,
    pub visual_lines: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScrollAnchor {
    pub source: ContentAnchor,
    pub local_offset: i64,
}

/// A viewport follows the live bottom until an explicit user scroll installs a semantic anchor.
/// Resize and reflow may reproject an anchor, but they never create one or replace its source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViewportScrollState {
    Bottom,
    Anchored(ScrollAnchor),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewSelection {
    pub start: ContentAnchor,
    pub end: ContentAnchor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridCursor {
    pub row: u32,
    pub column: u32,
    pub visible: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellAnchor {
    pub start: ContentAnchor,
    pub end: ContentAnchor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionSpan {
    pub row: u32,
    pub start_column: u32,
    pub end_column: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameViewportOrigin {
    Bottom,
    Anchored(ScrollAnchor),
}

/// Stable render input owned by the viewport layer. Renderers never inspect the upstream grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewportFrame {
    pub columns: NonZeroU32,
    pub rows: NonZeroU32,
    pub cells: Vec<CapturedCell>,
    pub cursor: GridCursor,
    pub cell_anchors: Vec<CellAnchor>,
    pub selection_spans: Vec<SelectionSpan>,
    pub status_text: Option<String>,
    pub viewport_origin: FrameViewportOrigin,
    pub scroll_offset_rows: usize,
    pub layout_key: LayoutKey,
    pub view_generation: ViewGeneration,
}

impl ViewportFrame {
    pub fn validate_shape(&self) -> Result<(), FrameShapeError> {
        if self.layout_key.width_cells != self.columns {
            return Err(FrameShapeError::LayoutWidth {
                frame: self.columns.get(),
                layout: self.layout_key.width_cells.get(),
            });
        }
        let expected = (self.columns.get() as usize)
            .checked_mul(self.rows.get() as usize)
            .ok_or(FrameShapeError::CellCount {
                expected: usize::MAX,
                actual: self.cells.len(),
            })?;
        if self.cells.len() != expected {
            return Err(FrameShapeError::CellCount {
                expected,
                actual: self.cells.len(),
            });
        }
        if self.cell_anchors.len() != expected {
            return Err(FrameShapeError::AnchorCount {
                expected,
                actual: self.cell_anchors.len(),
            });
        }
        Ok(())
    }

    pub fn anchor_at(
        &self,
        row: u32,
        column: u32,
        bias: Bias,
    ) -> Result<Option<ContentAnchor>, FrameShapeError> {
        self.validate_shape()?;
        if row >= self.rows.get() || column >= self.columns.get() {
            return Ok(None);
        }
        let index = row as usize * self.columns.get() as usize + column as usize;
        let Some(anchors) = self.cell_anchors.get(index) else {
            return Ok(None);
        };
        Ok(Some(match bias {
            Bias::Before => anchors.start.clone(),
            Bias::After => anchors.end.clone(),
        }))
    }

    /// Expand a cell hit to a word using the terminal selection delimiter policy. Whitespace and
    /// shell punctuation delimit words; every other grapheme (including emoji clusters) stays in
    /// the same run. Wide spacers share their lead cell's anchors and can never split a cluster.
    pub fn word_selection(
        &self,
        row: u32,
        column: u32,
    ) -> Result<Option<ViewSelection>, FrameShapeError> {
        self.validate_shape()?;
        let columns = self.columns.get() as usize;
        let row = row as usize;
        let mut column = column as usize;
        if row >= self.rows.get() as usize || column >= columns {
            return Ok(None);
        }
        let cells = &self.cells[row * columns..(row + 1) * columns];
        while column > 0 && cells[column].wide_spacer {
            column -= 1;
        }
        let class = word_class(&cells[column]);
        let mut first = column;
        while first > 0 && word_class(&cells[first - 1]) == class {
            first -= 1;
        }
        let mut last = column + 1;
        while last < columns && word_class(&cells[last]) == class {
            last += 1;
        }
        let Some(start) = self.anchor_at(row as u32, first as u32, Bias::Before)? else {
            return Ok(None);
        };
        let Some(end) = self.anchor_at(row as u32, (last - 1) as u32, Bias::After)? else {
            return Ok(None);
        };
        Ok(Some(ViewSelection { start, end }))
    }

    pub fn line_selection(&self, row: u32) -> Result<Option<ViewSelection>, FrameShapeError> {
        self.validate_shape()?;
        let Some(last) = self.columns.get().checked_sub(1) else {
            return Ok(None);
        };
        let Some(start) = self.anchor_at(row, 0, Bias::Before)? else {
            return Ok(None);
        };
        let Some(end) = self.anchor_at(row, last, Bias::After)? else {
            return Ok(None);
        };
        Ok(Some(ViewSelection { start, end }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameShapeError {
    LayoutWidth { frame: u32, layout: u32 },
    CellCount { expected: usize, actual: usize },
    AnchorCount { expected: usize, actual: usize },
}

impl fmt::Display for FrameShapeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LayoutWidth { frame, layout } => {
                write!(
                    formatter,
                    "frame width is {frame} cells, layout width is {layout}"
                )
            }
            Self::CellCount { expected, actual } => {
                write!(formatter, "frame requires {expected} cells, got {actual}")
            }
            Self::AnchorCount { expected, actual } => {
                write!(formatter, "frame requires {expected} anchors, got {actual}")
            }
        }
    }
}

impl Error for FrameShapeError {}

#[derive(Clone, Debug)]
struct VisualRow {
    cells: Vec<CapturedCell>,
    anchors: Vec<CellAnchor>,
}

fn validate_visual_row(
    row: &VisualRow,
    expected: usize,
    plane: &'static str,
    row_index: usize,
) -> Result<(), FrameProjectionError> {
    if row.cells.len() != expected || row.anchors.len() != expected {
        return Err(FrameProjectionError::PlaneShape {
            plane,
            row: row_index,
            expected,
            actual_cells: row.cells.len(),
            actual_anchors: row.anchors.len(),
        });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameProjectionError {
    RowCount {
        expected: usize,
        actual: usize,
    },
    ColumnCount {
        row: usize,
        expected: usize,
        actual: usize,
    },
    PlaneShape {
        plane: &'static str,
        row: usize,
        expected: usize,
        actual_cells: usize,
        actual_anchors: usize,
    },
    FrameShape(FrameShapeError),
}

impl fmt::Display for FrameProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RowCount { expected, actual } => {
                write!(formatter, "expected {expected} live rows, got {actual}")
            }
            Self::ColumnCount {
                row,
                expected,
                actual,
            } => write!(
                formatter,
                "expected {expected} cells in live row {row}, got {actual}"
            ),
            Self::PlaneShape {
                plane,
                row,
                expected,
                actual_cells,
                actual_anchors,
            } => write!(
                formatter,
                "expected {expected} cells and anchors in {plane} row {row}, got {actual_cells} cells and {actual_anchors} anchors"
            ),
            Self::FrameShape(error) => error.fmt(formatter),
        }
    }
}

impl Error for FrameProjectionError {}

#[derive(Clone, Debug)]
pub struct ViewportProjection {
    layout_key: LayoutKey,
    detection_rev: DetectionRevision,
    cache: HashMap<LayoutCacheKey, MeasuredLayout>,
    artifact_heights: HashMap<TranscriptId, i64>,
    ordered_ids: Vec<TranscriptId>,
    visual_rows: Vec<usize>,
    visual_row_heights: HeightTree,
    heights: HeightTree,
    scroll_state: ViewportScrollState,
    selection: Option<ViewSelection>,
    view_generation: ViewGeneration,
    live_rows: NonZeroU32,
    cell_height_subpixels: NonZeroI64,
    source_generation: SourceGeneration,
    grid_generation: GridGeneration,
    cache_misses: u64,
    scroll_offset_rows: usize,
    pending_scroll_offset_rows: Option<usize>,
    unread_rows: usize,
    last_total_rows: usize,
    /// Resize/reflow changes visual row counts without appending terminal content.
    suppress_next_growth_compensation: bool,
    projection_dirty: bool,
}

impl ViewportProjection {
    pub fn new(
        layout_key: LayoutKey,
        detection_rev: DetectionRevision,
        live_rows: NonZeroU32,
        cell_height_subpixels: NonZeroI64,
        source_generation: SourceGeneration,
        grid_generation: GridGeneration,
    ) -> Self {
        Self {
            layout_key,
            detection_rev,
            cache: HashMap::new(),
            artifact_heights: HashMap::new(),
            ordered_ids: Vec::new(),
            visual_rows: Vec::new(),
            visual_row_heights: HeightTree::default(),
            heights: HeightTree::default(),
            scroll_state: ViewportScrollState::Bottom,
            selection: None,
            view_generation: ViewGeneration(1),
            live_rows,
            cell_height_subpixels,
            source_generation,
            grid_generation,
            cache_misses: 0,
            scroll_offset_rows: 0,
            pending_scroll_offset_rows: None,
            unread_rows: 0,
            last_total_rows: 0,
            suppress_next_growth_compensation: false,
            projection_dirty: true,
        }
    }

    pub fn heights(&self) -> &HeightTree {
        &self.heights
    }
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
    pub fn cache_misses(&self) -> u64 {
        self.cache_misses
    }
    pub fn scroll_anchor(&self) -> Option<&ScrollAnchor> {
        match &self.scroll_state {
            ViewportScrollState::Bottom => None,
            ViewportScrollState::Anchored(anchor) => Some(anchor),
        }
    }
    pub fn scroll_state(&self) -> &ViewportScrollState {
        &self.scroll_state
    }
    pub fn selection(&self) -> Option<&ViewSelection> {
        self.selection.as_ref()
    }
    pub fn view_generation(&self) -> ViewGeneration {
        self.view_generation
    }
    pub fn layout_key(&self) -> LayoutKey {
        self.layout_key
    }
    pub fn detection_revision(&self) -> DetectionRevision {
        self.detection_rev
    }

    pub fn scroll_offset_rows(&self) -> usize {
        self.pending_scroll_offset_rows
            .unwrap_or(self.scroll_offset_rows)
    }

    pub fn unread_rows(&self) -> usize {
        self.unread_rows
    }

    pub fn is_scrolled(&self) -> bool {
        self.pending_scroll_offset_rows
            .unwrap_or(self.scroll_offset_rows)
            != 0
    }

    /// Positive rows move into history; negative rows move toward the live bottom.
    pub fn scroll_by_rows(&mut self, rows: i32) {
        let max = self
            .last_total_rows
            .saturating_sub(self.live_rows.get() as usize);
        let current = self
            .pending_scroll_offset_rows
            .unwrap_or(self.scroll_offset_rows);
        let next = if rows >= 0 {
            current.saturating_add(rows as usize).min(max)
        } else {
            current.saturating_sub(rows.unsigned_abs() as usize)
        };
        self.pending_scroll_offset_rows = Some(next);
        if next == 0 {
            self.scroll_state = ViewportScrollState::Bottom;
            self.scroll_offset_rows = 0;
            self.unread_rows = 0;
        }
        self.view_generation.0 += 1;
    }

    pub fn scroll_to_top(&mut self) {
        let offset = self
            .last_total_rows
            .saturating_sub(self.live_rows.get() as usize);
        self.pending_scroll_offset_rows = Some(offset);
        if offset == 0 {
            self.scroll_state = ViewportScrollState::Bottom;
        }
        self.view_generation.0 += 1;
    }

    pub fn scroll_to_bottom(&mut self) {
        if self.is_scrolled()
            || !matches!(self.scroll_state, ViewportScrollState::Bottom)
            || self.unread_rows != 0
        {
            self.scroll_state = ViewportScrollState::Bottom;
            self.scroll_offset_rows = 0;
            self.pending_scroll_offset_rows = None;
            self.unread_rows = 0;
            self.view_generation.0 += 1;
        }
    }

    /// Compose the live terminal grid into a viewport-owned frame consumed by bt-render.
    pub fn live_frame(
        &self,
        columns: NonZeroU32,
        rows: Vec<CapturedRow>,
        cursor: GridCursor,
    ) -> Result<ViewportFrame, FrameProjectionError> {
        let expected_rows = self.live_rows.get() as usize;
        if rows.len() != expected_rows {
            return Err(FrameProjectionError::RowCount {
                expected: expected_rows,
                actual: rows.len(),
            });
        }
        let expected_columns = columns.get() as usize;
        let mut cells = Vec::with_capacity(expected_rows.saturating_mul(expected_columns));
        let mut cell_anchors = Vec::with_capacity(expected_rows.saturating_mul(expected_columns));
        for (row_index, row) in rows.into_iter().enumerate() {
            if row.cells.len() != expected_columns {
                return Err(FrameProjectionError::ColumnCount {
                    row: row_index,
                    expected: expected_columns,
                    actual: row.cells.len(),
                });
            }
            let visual =
                captured_visual_row(&row, expected_columns, |column, bias| ContentAnchor::Live {
                    screen: ScreenId::Primary,
                    point: GridPoint {
                        row: row_index as u32,
                        column: column as u32,
                    },
                    bias,
                    generation: self.grid_generation,
                });
            cells.extend(visual.cells);
            cell_anchors.extend(visual.anchors);
        }
        let frame = ViewportFrame {
            columns,
            rows: self.live_rows,
            cells,
            cursor,
            cell_anchors,
            selection_spans: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: self.layout_key,
            view_generation: self.view_generation,
        };
        frame
            .validate_shape()
            .map_err(FrameProjectionError::FrameShape)?;
        Ok(frame)
    }

    /// Window the continuous primary `history + staging + live` row space. Only frozen logical
    /// lines intersecting the viewport are materialized; the height/index cache supplies offsets.
    pub fn continuous_frame(
        &mut self,
        document: &HistoryDocument,
        staged_rows: &[StagedRow],
        live_rows: Vec<CapturedRow>,
        cursor: GridCursor,
        screen: ScreenId,
    ) -> Result<ViewportFrame, FrameProjectionError> {
        let expected_rows = self.live_rows.get() as usize;
        if live_rows.len() != expected_rows {
            return Err(FrameProjectionError::RowCount {
                expected: expected_rows,
                actual: live_rows.len(),
            });
        }
        let columns = self.layout_key.width_cells;
        let column_count = columns.get() as usize;
        for (row_index, row) in live_rows.iter().enumerate() {
            if row.cells.len() != column_count {
                return Err(FrameProjectionError::ColumnCount {
                    row: row_index,
                    expected: column_count,
                    actual: row.cells.len(),
                });
            }
        }
        for (row_index, staged) in staged_rows.iter().enumerate() {
            if staged.row.cells.len() != column_count {
                return Err(FrameProjectionError::PlaneShape {
                    plane: "staging",
                    row: row_index,
                    expected: column_count,
                    actual_cells: staged.row.cells.len(),
                    actual_anchors: staged.row.cells.len(),
                });
            }
        }

        let primary = screen == ScreenId::Primary;
        let history_rows = if primary {
            usize::try_from(self.visual_row_heights.total())
                .expect("visual row height totals are non-negative")
        } else {
            0
        };
        let staging_rows = if primary { staged_rows.len() } else { 0 };
        let total_rows = history_rows + staging_rows + expected_rows;
        let was_scrolled = self.is_scrolled();
        if !self.suppress_next_growth_compensation
            && was_scrolled
            && self.last_total_rows != 0
            && total_rows > self.last_total_rows
        {
            let added = total_rows - self.last_total_rows;
            self.unread_rows = self.unread_rows.saturating_add(added);
        }
        self.suppress_next_growth_compensation = false;
        self.last_total_rows = total_rows;
        let max_offset = total_rows.saturating_sub(expected_rows);
        let bottom_start = max_offset;
        if !primary {
            self.scroll_state = ViewportScrollState::Bottom;
            self.pending_scroll_offset_rows = None;
            self.scroll_offset_rows = 0;
            self.unread_rows = 0;
        } else if let Some(requested_offset) = self.pending_scroll_offset_rows.take() {
            let offset = requested_offset.min(max_offset);
            if offset == 0 {
                self.scroll_state = ViewportScrollState::Bottom;
            } else {
                let target_row = bottom_start.saturating_sub(offset);
                self.scroll_state = self
                    .anchor_at_absolute_row(document, staged_rows, history_rows, target_row, screen)
                    .map_or(ViewportScrollState::Bottom, |source| {
                        ViewportScrollState::Anchored(ScrollAnchor {
                            source,
                            local_offset: 0,
                        })
                    });
            }
        }

        let mut window_start = bottom_start;
        if let ViewportScrollState::Anchored(mut anchor) = self.scroll_state.clone() {
            anchor.source = document.resolve_anchor(&anchor.source);
            if let Some(anchor_row) = self.absolute_row_for_anchor(
                document,
                staged_rows,
                history_rows,
                &anchor.source,
                screen,
            ) {
                let local_rows = anchor
                    .local_offset
                    .max(0)
                    .div_euclid(self.cell_height_subpixels.get())
                    as usize;
                window_start = anchor_row.saturating_add(local_rows).min(bottom_start);
                if window_start < bottom_start {
                    self.scroll_state = ViewportScrollState::Anchored(anchor);
                } else {
                    self.scroll_state = ViewportScrollState::Bottom;
                }
            } else {
                self.scroll_state = ViewportScrollState::Bottom;
            }
        }
        self.scroll_offset_rows = bottom_start.saturating_sub(window_start);
        if matches!(self.scroll_state, ViewportScrollState::Bottom) {
            window_start = bottom_start;
            self.scroll_offset_rows = 0;
            self.unread_rows = 0;
        }
        let live_base = history_rows + staging_rows;
        let window_end = (window_start + expected_rows).min(total_rows);
        // The live plane is always rectangular, but blank rows at its tail are presentation
        // capacity rather than unread content. If an anchored frame displaces only those rows,
        // reporting `N lines below` would claim hidden content even though every meaningful row
        // already fits in the viewport.
        let first_live_row_below = window_end.saturating_sub(live_base).min(live_rows.len());
        let blank_live_rows_below = live_rows[first_live_row_below..]
            .iter()
            .rev()
            .take_while(|row| captured_row_is_blank(row))
            .count();
        let content_rows_below = self
            .scroll_offset_rows
            .saturating_sub(blank_live_rows_below);
        let mut visible = Vec::with_capacity(expected_rows);

        if primary && window_start < history_rows {
            let first_index = self
                .visual_row_heights
                .index_at_offset(window_start as i64)
                .unwrap_or(0);
            let mut row_base = self.visual_row_heights.prefix_sum(first_index) as usize;
            for (index, id) in self.ordered_ids.iter().enumerate().skip(first_index) {
                let line_rows = self.visual_rows[index];
                let line_end = row_base + line_rows;
                if line_end > window_start
                    && row_base < window_end
                    && let Some(entry) = document.entries().get(id)
                {
                    let laid_out = layout_frozen_line(&entry.line, column_count);
                    for (local_row, row) in laid_out.iter().enumerate() {
                        validate_visual_row(row, column_count, "history", row_base + local_row)?;
                    }
                    let local_start = window_start.saturating_sub(row_base);
                    let local_end = window_end.saturating_sub(row_base).min(laid_out.len());
                    visible.extend(laid_out[local_start..local_end].iter().cloned());
                }
                row_base = line_end;
                if row_base >= window_end {
                    break;
                }
            }
        }

        let staging_base = history_rows;
        if primary && window_end > staging_base && window_start < staging_base + staging_rows {
            let first = window_start.saturating_sub(staging_base);
            let last = window_end
                .saturating_sub(staging_base)
                .min(staged_rows.len());
            for (offset, staged) in staged_rows[first..last].iter().enumerate() {
                let row = captured_staged_visual_row(staged, column_count, self.source_generation);
                validate_visual_row(&row, column_count, "staging", first + offset)?;
                visible.push(row);
            }
        }

        if window_end > live_base && window_start < live_base + expected_rows {
            let first = window_start.saturating_sub(live_base);
            let last = window_end.saturating_sub(live_base).min(live_rows.len());
            visible.extend(
                live_rows[first..last]
                    .iter()
                    .enumerate()
                    .map(|(offset, row)| {
                        let live_row = first + offset;
                        captured_visual_row(row, column_count, |column, bias| ContentAnchor::Live {
                            screen,
                            point: GridPoint {
                                row: live_row as u32,
                                column: column as u32,
                            },
                            bias,
                            generation: self.grid_generation,
                        })
                    }),
            );
        }

        while visible.len() < expected_rows {
            let row = visible.len() as u32;
            visible.push(blank_visual_row(column_count, |column, bias| {
                ContentAnchor::Live {
                    screen,
                    point: GridPoint {
                        row,
                        column: column as u32,
                    },
                    bias,
                    generation: self.grid_generation,
                }
            }));
        }
        visible.truncate(expected_rows);
        for (row_index, row) in visible.iter().enumerate() {
            validate_visual_row(row, column_count, "visible", row_index)?;
        }

        let cells = visible
            .iter()
            .flat_map(|row| row.cells.iter().cloned())
            .collect();
        let cell_anchors = visible
            .into_iter()
            .flat_map(|row| row.anchors)
            .collect::<Vec<_>>();
        let selection_spans = self
            .selection
            .as_ref()
            .map(|selection| selection_spans(&cell_anchors, column_count, expected_rows, selection))
            .transpose()?
            .unwrap_or_default();
        let projected_cursor_row = live_base
            .saturating_add(cursor.row as usize)
            .checked_sub(window_start)
            .filter(|row| *row < expected_rows)
            .and_then(|row| u32::try_from(row).ok());
        let frame = ViewportFrame {
            columns,
            rows: self.live_rows,
            cells,
            cursor: GridCursor {
                row: projected_cursor_row.unwrap_or(cursor.row),
                visible: cursor.visible && projected_cursor_row.is_some(),
                ..cursor
            },
            cell_anchors,
            selection_spans,
            status_text: (content_rows_below != 0)
                .then(|| format!("{content_rows_below} lines below")),
            viewport_origin: match &self.scroll_state {
                ViewportScrollState::Bottom => FrameViewportOrigin::Bottom,
                ViewportScrollState::Anchored(anchor) => {
                    FrameViewportOrigin::Anchored(anchor.clone())
                }
            },
            scroll_offset_rows: self.scroll_offset_rows,
            layout_key: self.layout_key,
            view_generation: self.view_generation,
        };
        frame
            .validate_shape()
            .map_err(FrameProjectionError::FrameShape)?;
        Ok(frame)
    }

    fn anchor_at_absolute_row(
        &self,
        document: &HistoryDocument,
        staged_rows: &[StagedRow],
        history_rows: usize,
        absolute_row: usize,
        screen: ScreenId,
    ) -> Option<ContentAnchor> {
        if absolute_row < history_rows {
            let index = self
                .visual_row_heights
                .index_at_offset(absolute_row as i64)?;
            let row_base = self.visual_row_heights.prefix_sum(index) as usize;
            let id = self.ordered_ids.get(index)?;
            let entry = document.entries().get(id)?;
            return layout_frozen_line(&entry.line, self.layout_key.width_cells.get() as usize)
                .get(absolute_row.saturating_sub(row_base))?
                .anchors
                .first()
                .map(|anchor| anchor.start.clone());
        }

        let staging_row = absolute_row.saturating_sub(history_rows);
        if let Some(staged) = staged_rows.get(staging_row) {
            return Some(ContentAnchor::Staging {
                id: staged.id,
                offset: GraphemeOffset(0),
                bias: Bias::Before,
                generation: self.source_generation,
            });
        }

        let live_row = staging_row.saturating_sub(staged_rows.len());
        (live_row < self.live_rows.get() as usize).then_some(ContentAnchor::Live {
            screen,
            point: GridPoint {
                row: live_row as u32,
                column: 0,
            },
            bias: Bias::Before,
            generation: self.grid_generation,
        })
    }

    fn absolute_row_for_anchor(
        &self,
        document: &HistoryDocument,
        staged_rows: &[StagedRow],
        history_rows: usize,
        anchor: &ContentAnchor,
        screen: ScreenId,
    ) -> Option<usize> {
        match anchor {
            ContentAnchor::History {
                id,
                offset,
                generation,
                ..
            } => {
                let index = self
                    .ordered_ids
                    .iter()
                    .position(|candidate| candidate == id)?;
                let entry = document.entries().get(id)?;
                if *generation != entry.line.source_generation {
                    return None;
                }
                let rows =
                    layout_frozen_line(&entry.line, self.layout_key.width_cells.get() as usize);
                let local_row = rows
                    .iter()
                    .enumerate()
                    .filter_map(|(row, visual)| {
                        let ContentAnchor::History {
                            offset: row_offset, ..
                        } = &visual.anchors.first()?.start
                        else {
                            return None;
                        };
                        (row_offset.0 <= offset.0).then_some(row)
                    })
                    .next_back()?;
                Some(self.visual_row_heights.prefix_sum(index) as usize + local_row)
            }
            ContentAnchor::Staging { id, generation, .. } => {
                if *generation != self.source_generation {
                    return None;
                }
                staged_rows
                    .iter()
                    .position(|staged| staged.id == *id)
                    .map(|row| history_rows + row)
            }
            ContentAnchor::Live {
                screen: anchor_screen,
                point,
                generation,
                ..
            } => {
                if *anchor_screen != screen
                    || *generation != self.grid_generation
                    || point.row >= self.live_rows.get()
                {
                    return None;
                }
                Some(history_rows + staged_rows.len() + point.row as usize)
            }
        }
    }

    pub fn set_selection(&mut self, selection: Option<ViewSelection>) {
        self.selection = selection;
    }
    pub fn set_scroll_anchor(&mut self, anchor: Option<ScrollAnchor>) {
        self.pending_scroll_offset_rows = None;
        self.scroll_state =
            anchor.map_or(ViewportScrollState::Bottom, ViewportScrollState::Anchored);
    }
    pub fn set_artifact_height(&mut self, id: TranscriptId, height_subpixels: i64) {
        if self.artifact_heights.insert(id, height_subpixels) != Some(height_subpixels) {
            self.cache.retain(|key, _| key.span.start != id);
            self.projection_dirty = true;
        }
    }
    pub fn sync_artifact_heights(
        &mut self,
        heights: impl IntoIterator<Item = (TranscriptId, i64)>,
    ) {
        let next = heights.into_iter().collect::<HashMap<_, _>>();
        let changed = self
            .artifact_heights
            .keys()
            .chain(next.keys())
            .copied()
            .filter(|id| self.artifact_heights.get(id) != next.get(id))
            .collect::<HashSet<_>>();
        if !changed.is_empty() {
            self.cache
                .retain(|key, _| !changed.contains(&key.span.start));
            self.projection_dirty = true;
        }
        self.artifact_heights = next;
    }
    pub fn set_live_state(
        &mut self,
        live_rows: NonZeroU32,
        source_generation: SourceGeneration,
        grid_generation: GridGeneration,
    ) {
        if self.live_rows != live_rows {
            self.suppress_next_growth_compensation = true;
        }
        self.live_rows = live_rows;
        self.source_generation = source_generation;
        self.grid_generation = grid_generation;
    }

    pub fn project(&mut self, document: &HistoryDocument) {
        let next_ids = document.entries().keys().copied().collect::<Vec<_>>();
        let append_only = !self.projection_dirty
            && self.ordered_ids.len() <= next_ids.len()
            && self.ordered_ids == next_ids[..self.ordered_ids.len()];
        let start = if append_only {
            self.ordered_ids.len()
        } else {
            0
        };
        if !append_only {
            self.ordered_ids.clear();
            self.visual_rows.clear();
            self.visual_row_heights.rebuild([]);
            self.heights.rebuild([]);
        }
        for id in next_ids.iter().skip(start) {
            let entry = &document.entries()[id];
            self.ordered_ids.push(*id);
            let cache_key = LayoutCacheKey {
                span: TranscriptSpan {
                    start: *id,
                    end: *id,
                },
                source_gen: entry.line.source_generation,
                detection_rev: self.detection_rev,
                layout: self.layout_key,
            };
            let measured = if let Some(measured) = self.cache.get(&cache_key).copied() {
                measured
            } else {
                self.cache_misses += 1;
                let measured = {
                    if let Some(height) = self.artifact_heights.get(id).copied() {
                        MeasuredLayout {
                            visual_lines: 1,
                            height,
                        }
                    } else {
                        let visual_lines = frozen_visual_line_count(
                            &entry.line.text,
                            self.layout_key.width_cells.get() as usize,
                        ) as u32;
                        MeasuredLayout {
                            visual_lines,
                            height: visual_lines as i64 * self.cell_height_subpixels.get(),
                        }
                    }
                };
                self.cache.insert(cache_key, measured);
                measured
            };
            self.visual_rows.push(measured.visual_lines as usize);
            self.visual_row_heights
                .push(i64::from(measured.visual_lines));
            self.heights.push(measured.height);
        }
        if start != next_ids.len() || !append_only {
            self.view_generation.0 += 1;
        }
        self.projection_dirty = false;
    }

    /// Project a semantic anchor through this viewport's independent width/height tree.
    pub fn anchor_y(
        &self,
        document: &HistoryDocument,
        anchor: &ContentAnchor,
    ) -> Result<i64, AnchorError> {
        match anchor {
            ContentAnchor::History {
                id,
                offset,
                generation,
                ..
            } => {
                let index = self
                    .ordered_ids
                    .iter()
                    .position(|candidate| candidate == id)
                    .ok_or(AnchorError::UnknownAnchor)?;
                let entry = document
                    .entries()
                    .get(id)
                    .ok_or(AnchorError::UnknownAnchor)?;
                if *generation != entry.line.source_generation {
                    return Err(AnchorError::StaleGeneration);
                }
                let max_offset = entry.line.grapheme_boundaries.len().saturating_sub(1) as u32;
                let local_y = if self.artifact_heights.contains_key(id) {
                    0
                } else {
                    let row = offset.0.min(max_offset) / self.layout_key.width_cells.get();
                    row as i64 * self.cell_height_subpixels.get()
                };
                Ok(self.heights.prefix_sum(index) + local_y)
            }
            ContentAnchor::Staging { generation, .. } => {
                if *generation != self.source_generation {
                    return Err(AnchorError::StaleGeneration);
                }
                Ok(self.heights.total())
            }
            ContentAnchor::Live {
                screen: ScreenId::Primary,
                point,
                generation,
                ..
            } => {
                if *generation != self.grid_generation {
                    return Err(AnchorError::StaleGeneration);
                }
                if point.row >= self.live_rows.get() {
                    return Err(AnchorError::LiveOutOfBounds);
                }
                Ok(self.heights.total() + point.row as i64 * self.cell_height_subpixels.get())
            }
            ContentAnchor::Live {
                screen: ScreenId::Alternate,
                ..
            } => Err(AnchorError::IsolatedScreen),
        }
    }

    pub fn scroll_y(&self, document: &HistoryDocument) -> Result<Option<i64>, AnchorError> {
        self.scroll_anchor()
            .map(|anchor| {
                self.anchor_y(document, &anchor.source)
                    .map(|y| y + anchor.local_offset)
            })
            .transpose()
    }

    pub fn selection_y(
        &self,
        document: &HistoryDocument,
    ) -> Result<Option<(i64, i64)>, AnchorError> {
        self.selection
            .as_ref()
            .map(|selection| {
                Ok((
                    self.anchor_y(document, &selection.start)?,
                    self.anchor_y(document, &selection.end)?,
                ))
            })
            .transpose()
    }

    /// Layout changes preserve the semantic scroll anchor; only its pixel projection changes.
    pub fn relayout(&mut self, layout_key: LayoutKey, document: &HistoryDocument) {
        if self.layout_key != layout_key {
            self.suppress_next_growth_compensation = true;
            self.layout_key = layout_key;
            self.projection_dirty = true;
            self.project(document);
        }
    }

    /// Consume a detector-owned revision after intent has been rebuilt in the document.
    pub fn apply_detection_revision(
        &mut self,
        revision: DetectionRevision,
        document: &HistoryDocument,
    ) {
        if self.detection_rev != revision {
            self.detection_rev = revision;
            self.projection_dirty = true;
            self.project(document);
        }
    }
}

fn endpoint(anchor: &ContentAnchor, bias: Bias) -> ContentAnchor {
    let mut anchor = anchor.clone();
    match &mut anchor {
        ContentAnchor::History { bias: side, .. }
        | ContentAnchor::Staging { bias: side, .. }
        | ContentAnchor::Live { bias: side, .. } => *side = bias,
    }
    anchor
}

fn blank_visual_row(columns: usize, anchor: impl Fn(usize, Bias) -> ContentAnchor) -> VisualRow {
    VisualRow {
        cells: vec![CapturedCell::default(); columns],
        anchors: (0..columns)
            .map(|column| CellAnchor {
                start: anchor(column, Bias::Before),
                end: anchor(column, Bias::After),
            })
            .collect(),
    }
}

fn captured_row_is_blank(row: &CapturedRow) -> bool {
    row.cells
        .iter()
        .filter(|cell| !cell.wide_spacer)
        .all(|cell| cell.text.chars().all(char::is_whitespace))
}

fn captured_visual_row(
    row: &CapturedRow,
    columns: usize,
    anchor: impl Fn(usize, Bias) -> ContentAnchor,
) -> VisualRow {
    let mut anchors = Vec::with_capacity(columns);
    let mut lead = 0usize;
    for (column, cell) in row.cells.iter().enumerate() {
        if !cell.wide_spacer {
            lead = column;
        }
        anchors.push(CellAnchor {
            start: anchor(lead, Bias::Before),
            end: anchor(lead, Bias::After),
        });
    }
    VisualRow {
        cells: row.cells.clone(),
        anchors,
    }
}

fn captured_staged_visual_row(
    staged: &StagedRow,
    columns: usize,
    generation: SourceGeneration,
) -> VisualRow {
    let mut anchors = Vec::with_capacity(columns);
    let mut grapheme_offset = 0u32;
    let mut lead_offset = 0u32;
    for cell in &staged.row.cells {
        if !cell.wide_spacer {
            lead_offset = grapheme_offset;
            grapheme_offset += u32::from(!cell.text.is_empty());
        }
        let anchor = |bias| ContentAnchor::Staging {
            id: staged.id,
            offset: GraphemeOffset(lead_offset),
            bias,
            generation,
        };
        anchors.push(CellAnchor {
            start: anchor(Bias::Before),
            end: anchor(Bias::After),
        });
    }
    VisualRow {
        cells: staged.row.cells.clone(),
        anchors,
    }
}

fn frozen_visual_line_count(text: &str, columns: usize) -> usize {
    let mut rows = 1usize;
    let mut used = 0usize;
    for cluster in graphemes(text) {
        let width = cluster_width(cluster);
        if width == 0 {
            continue;
        }
        if used != 0 && used + width > columns {
            rows += 1;
            used = 0;
        }
        used += width.min(columns);
    }
    rows
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum WordClass {
    Space,
    Delimiter,
    Word,
}

fn word_class(cell: &CapturedCell) -> WordClass {
    if cell.wide_spacer {
        return WordClass::Word;
    }
    if cell.text.is_empty() || cell.text.chars().all(char::is_whitespace) {
        return WordClass::Space;
    }
    // Stable xterm-style shell delimiters. Configuration belongs to the later settings slice.
    const DELIMITERS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";
    if cell
        .text
        .chars()
        .all(|character| DELIMITERS.contains(character))
    {
        WordClass::Delimiter
    } else {
        WordClass::Word
    }
}

fn layout_frozen_line(line: &FrozenLine, columns: usize) -> Vec<VisualRow> {
    let mut rows = vec![VisualRow {
        cells: Vec::with_capacity(columns),
        anchors: Vec::with_capacity(columns),
    }];
    for (grapheme_index, cluster) in graphemes(&line.text).enumerate() {
        let width = cluster_width(cluster).min(columns);
        if width == 0 {
            if let Some(cell) = rows.last_mut().and_then(|row| row.cells.last_mut()) {
                cell.text.push_str(cluster);
            }
            continue;
        }
        if rows
            .last()
            .is_some_and(|row| !row.cells.is_empty() && row.cells.len() + width > columns)
        {
            pad_frozen_row(rows.last_mut().unwrap(), line, columns, grapheme_index);
            rows.push(VisualRow {
                cells: Vec::with_capacity(columns),
                anchors: Vec::with_capacity(columns),
            });
        }
        let byte_start = line.grapheme_boundaries[grapheme_index];
        let span = line
            .styles
            .iter()
            .find(|span| span.byte_start <= byte_start && byte_start < span.byte_end);
        let mut cell = CapturedCell::plain(cluster);
        if let Some(span) = span {
            cell.style = span.style.clone();
            cell.hyperlink.clone_from(&span.hyperlink);
        }
        if width == 2 {
            cell.style.flags.insert(CellFlags::WIDE_CHAR);
        }
        let start = ContentAnchor::History {
            id: line.id,
            offset: GraphemeOffset(grapheme_index as u32),
            bias: Bias::Before,
            generation: line.source_generation,
        };
        let end = endpoint(&start, Bias::After);
        let row = rows.last_mut().unwrap();
        row.cells.push(cell);
        row.anchors.push(CellAnchor {
            start: start.clone(),
            end: end.clone(),
        });
        if width == 2 {
            let spacer = CapturedCell {
                wide_spacer: true,
                ..CapturedCell::default()
            };
            row.cells.push(spacer);
            row.anchors.push(CellAnchor { start, end });
        }
    }
    let end_offset = line.grapheme_boundaries.len().saturating_sub(1);
    pad_frozen_row(rows.last_mut().unwrap(), line, columns, end_offset);
    rows
}

fn pad_frozen_row(row: &mut VisualRow, line: &FrozenLine, columns: usize, offset: usize) {
    let start = ContentAnchor::History {
        id: line.id,
        offset: GraphemeOffset(offset as u32),
        bias: Bias::Before,
        generation: line.source_generation,
    };
    let end = endpoint(&start, Bias::After);
    while row.cells.len() < columns {
        row.cells.push(CapturedCell::default());
        row.anchors.push(CellAnchor {
            start: start.clone(),
            end: end.clone(),
        });
    }
}

fn selection_spans(
    anchors: &[CellAnchor],
    columns: usize,
    rows: usize,
    selection: &ViewSelection,
) -> Result<Vec<SelectionSpan>, FrameProjectionError> {
    let expected = columns.saturating_mul(rows);
    if anchors.len() != expected {
        return Err(FrameProjectionError::FrameShape(
            FrameShapeError::AnchorCount {
                expected,
                actual: anchors.len(),
            },
        ));
    }
    let (start, end) = match compare_visible_anchors(&selection.start, &selection.end) {
        Ok(std::cmp::Ordering::Greater) => (&selection.end, &selection.start),
        Ok(_) => (&selection.start, &selection.end),
        Err(_) => return Ok(Vec::new()),
    };
    let selected = |cell: &CellAnchor| {
        compare_visible_anchors(&cell.start, end)
            .is_ok_and(|order| order == std::cmp::Ordering::Less)
            && compare_visible_anchors(&cell.end, start)
                .is_ok_and(|order| order == std::cmp::Ordering::Greater)
    };
    let mut spans = Vec::new();
    for (row, row_anchors) in anchors.chunks(columns).enumerate() {
        let mut column = 0usize;
        while column < columns {
            if !selected(&row_anchors[column]) {
                column += 1;
                continue;
            }
            let start_column = column;
            while column < columns && selected(&row_anchors[column]) {
                column += 1;
            }
            spans.push(SelectionSpan {
                row: row as u32,
                start_column: start_column as u32,
                end_column: column as u32,
            });
        }
    }
    Ok(spans)
}

fn compare_visible_anchors(
    left: &ContentAnchor,
    right: &ContentAnchor,
) -> Result<std::cmp::Ordering, AnchorError> {
    match (left, right) {
        (
            ContentAnchor::Live {
                screen: ScreenId::Alternate,
                point: left_point,
                bias: left_bias,
                ..
            },
            ContentAnchor::Live {
                screen: ScreenId::Alternate,
                point: right_point,
                bias: right_bias,
                ..
            },
        ) => Ok((left_point, left_bias).cmp(&(right_point, right_bias))),
        _ => compare_anchors(left, right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_doc::{Bias, GridGeneration, GridPoint, ScreenId};
    use bt_transcript::{CapturedRow, GraphemeOffset, StagingId, TranscriptStore};
    use std::{num::NonZeroU32, num::NonZeroUsize};

    fn nz32(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    fn cell_height() -> NonZeroI64 {
        NonZeroI64::new(18 * SUBPIXELS_PER_PX).unwrap()
    }

    fn history() -> HistoryDocument {
        let mut store = TranscriptStore::new(NonZeroUsize::new(8).unwrap());
        let line = store
            .capture(CapturedRow::plain("abcdefgh", false))
            .finalized
            .remove(0);
        let mut document = HistoryDocument::default();
        document.finalize_transaction(line);
        document
    }

    fn key(width_cells: u32) -> LayoutKey {
        LayoutKey {
            width_cells: nz32(width_cells),
            dpi_milli: nz32(1000),
            font_rev: 1,
            theme_rev: 1,
        }
    }

    #[test]
    fn live_frame_flattens_only_well_formed_viewport_rows() {
        let projection = ViewportProjection::new(
            key(2),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let frame = projection
            .live_frame(
                nz32(2),
                vec![
                    CapturedRow::plain("ab", false),
                    CapturedRow::plain("cd", false),
                ],
                GridCursor {
                    row: 1,
                    column: 1,
                    visible: true,
                },
            )
            .unwrap();
        assert_eq!(frame.cells.len(), 4);
        assert_eq!(frame.cells[0].text, "a");
        assert_eq!(frame.cells[3].text, "d");
        assert_eq!(frame.layout_key, key(2));
        assert_eq!(frame.cursor.column, 1);

        assert_eq!(
            projection.live_frame(
                nz32(2),
                vec![CapturedRow::plain("ab", false)],
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
            ),
            Err(FrameProjectionError::RowCount {
                expected: 2,
                actual: 1,
            })
        );
    }

    #[test]
    fn continuous_frame_rejects_a_wrong_width_staging_plane_before_flattening() {
        let mut projection = ViewportProjection::new(
            key(4),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let staged = [StagedRow {
            id: bt_transcript::StagingId(1),
            row: CapturedRow::plain("xx", false),
        }];
        projection.scroll_offset_rows = 1;
        let error = projection
            .continuous_frame(
                &HistoryDocument::default(),
                &staged,
                vec![
                    CapturedRow::plain("live", false),
                    CapturedRow::plain("grid", false),
                ],
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            FrameProjectionError::PlaneShape {
                plane: "staging",
                expected: 4,
                actual_cells: 2,
                ..
            }
        ));
    }

    #[test]
    fn frame_consumers_recover_from_non_rectangular_cells_and_anchors() {
        let projection = ViewportProjection::new(
            key(2),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let mut frame = projection
            .live_frame(
                nz32(2),
                vec![
                    CapturedRow::plain("ab", false),
                    CapturedRow::plain("cd", false),
                ],
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
            )
            .unwrap();
        frame.cells.pop();
        assert_eq!(
            frame.word_selection(0, 0),
            Err(FrameShapeError::CellCount {
                expected: 4,
                actual: 3,
            })
        );

        frame.cells.push(CapturedCell::default());
        frame.cell_anchors.pop();
        assert_eq!(
            frame.line_selection(0),
            Err(FrameShapeError::AnchorCount {
                expected: 4,
                actual: 3,
            })
        );
    }

    #[test]
    fn g2_two_widths_have_independent_height_selection_and_scroll_anchor() {
        let document = history();
        let anchor = ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 3, column: 0 },
            bias: Bias::Before,
            generation: GridGeneration(1),
        };
        let mut narrow = ViewportProjection::new(
            key(4),
            DetectionRevision(1),
            nz32(24),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let mut wide = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(24),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        narrow.set_scroll_anchor(Some(ScrollAnchor {
            source: anchor.clone(),
            local_offset: 7,
        }));
        wide.set_scroll_anchor(Some(ScrollAnchor {
            source: anchor.clone(),
            local_offset: 19,
        }));
        narrow.set_selection(Some(ViewSelection {
            start: anchor.clone(),
            end: anchor.clone(),
        }));
        narrow.project(&document);
        wide.project(&document);

        assert_eq!(narrow.heights().total(), 36 * SUBPIXELS_PER_PX);
        assert_eq!(wide.heights().total(), 18 * SUBPIXELS_PER_PX);
        assert!(narrow.selection().is_some());
        assert!(wide.selection().is_none());
        assert_eq!(narrow.scroll_anchor().unwrap().local_offset, 7);
        assert_eq!(wide.scroll_anchor().unwrap().local_offset, 19);

        let initial_misses = narrow.cache_misses();
        narrow.project(&document);
        assert_eq!(narrow.cache_misses(), initial_misses);
        narrow.relayout(key(8), &document);
        let misses_after_new_width = narrow.cache_misses();
        narrow.relayout(key(4), &document);
        assert_eq!(narrow.cache_misses(), misses_after_new_width);
    }

    #[test]
    fn live_anchor_outside_grid_is_rejected() {
        let document = history();
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        projection.project(&document);
        let anchor = ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 2, column: 0 },
            bias: Bias::Before,
            generation: GridGeneration(1),
        };
        assert_eq!(
            projection.anchor_y(&document, &anchor),
            Err(AnchorError::LiveOutOfBounds)
        );
    }

    #[test]
    fn every_layout_key_field_has_an_independent_cache_identity() {
        let document = history();
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        projection.project(&document);
        let base = key(8);
        for changed in [
            LayoutKey {
                width_cells: nz32(9),
                ..key(8)
            },
            LayoutKey {
                dpi_milli: nz32(1250),
                ..key(8)
            },
            LayoutKey {
                font_rev: 2,
                ..key(8)
            },
            LayoutKey {
                theme_rev: 2,
                ..key(8)
            },
        ] {
            projection.relayout(base, &document);
            let misses = projection.cache_misses();
            projection.relayout(changed, &document);
            assert_eq!(projection.cache_misses(), misses + 1);
        }
    }

    #[test]
    fn stale_anchor_generations_are_rejected() {
        let document = history();
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(3),
            GridGeneration(4),
        );
        projection.project(&document);
        let stale_live = ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 1, column: 0 },
            bias: Bias::Before,
            generation: GridGeneration(3),
        };
        assert_eq!(
            projection.anchor_y(&document, &stale_live),
            Err(AnchorError::StaleGeneration)
        );

        let entry = document.entries().first_key_value().unwrap().1;
        let stale_history = ContentAnchor::History {
            id: entry.line.id,
            offset: GraphemeOffset(0),
            bias: Bias::Before,
            generation: SourceGeneration(entry.line.source_generation.0 + 1),
        };
        assert_eq!(
            projection.anchor_y(&document, &stale_history),
            Err(AnchorError::StaleGeneration)
        );

        let stale_staging = ContentAnchor::Staging {
            id: StagingId(9),
            offset: GraphemeOffset(0),
            bias: Bias::Before,
            generation: SourceGeneration(2),
        };
        assert_eq!(
            projection.anchor_y(&document, &stale_staging),
            Err(AnchorError::StaleGeneration)
        );
    }

    #[test]
    fn continuous_scroll_clamps_and_new_rows_preserve_the_frozen_window() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(16).unwrap());
        let mut document = HistoryDocument::default();
        for text in ["one", "two"] {
            document.finalize_transaction(
                store
                    .capture(CapturedRow::plain(text, false))
                    .finalized
                    .remove(0),
            );
        }
        let mut projection = ViewportProjection::new(
            key(4),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.project(&document);
        let live = || {
            vec![
                CapturedRow::plain("aaaa", false),
                CapturedRow::plain("bbbb", false),
            ]
        };
        projection
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        projection.scroll_by_rows(99);
        assert_eq!(projection.scroll_offset_rows(), 2);
        projection.scroll_by_rows(-1);
        assert_eq!(projection.scroll_offset_rows(), 1);
        let frame = projection
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        assert_eq!(frame.status_text.as_deref(), Some("1 lines below"));

        document.finalize_transaction(
            store
                .capture(CapturedRow::plain("tri", false))
                .finalized
                .remove(0),
        );
        projection.project(&document);
        let frame = projection
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        assert_eq!(projection.scroll_offset_rows(), 2);
        assert_eq!(projection.unread_rows(), 1);
        assert_eq!(frame.status_text.as_deref(), Some("2 lines below"));
        assert!(!frame.cursor.visible);
    }

    #[test]
    fn cursor_is_projected_into_every_continuous_window_that_contains_its_live_row() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(16).unwrap());
        let mut document = HistoryDocument::default();
        for row in [
            "history-0 ",
            "history-1 ",
            "history-2 ",
            "history-3 ",
            "history-4 ",
        ] {
            let finalized = store.capture(CapturedRow::plain(row, false)).finalized;
            document.finalize_transaction(finalized.into_iter().next().unwrap());
        }
        let live = || {
            vec![
                CapturedRow::plain("prompt>   ", false),
                CapturedRow::plain("          ", false),
                CapturedRow::plain("          ", false),
                CapturedRow::plain("          ", false),
                CapturedRow::plain("          ", false),
            ]
        };

        for cursor_row in 0..5 {
            for offset in 0..=5 {
                let mut projection = ViewportProjection::new(
                    key(10),
                    DetectionRevision(1),
                    nz32(5),
                    cell_height(),
                    store.source_generation(),
                    GridGeneration(1),
                );
                projection.project(&document);
                let cursor = GridCursor {
                    row: cursor_row,
                    column: 8,
                    visible: true,
                };
                projection
                    .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
                    .unwrap();
                projection.scroll_by_rows(offset as i32);
                let frame = projection
                    .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
                    .unwrap();
                let projected_row = cursor_row + offset;

                assert_eq!(frame.scroll_offset_rows, offset as usize);
                assert_eq!(frame.cursor.column, cursor.column);
                assert_eq!(frame.cursor.visible, projected_row < 5);
                if projected_row < 5 {
                    assert_eq!(frame.cursor.row, projected_row);
                }
            }
        }
    }

    #[test]
    fn blank_live_capacity_is_never_reported_as_lines_below() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(16).unwrap());
        let mut document = HistoryDocument::default();
        for text in ["frozen-a", "frozen-b"] {
            document.finalize_transaction(
                store
                    .capture(CapturedRow::plain(text, false))
                    .finalized
                    .remove(0),
            );
        }
        let mut projection = ViewportProjection::new(
            key(10),
            DetectionRevision(1),
            nz32(4),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.project(&document);
        let live = || {
            vec![
                CapturedRow::plain("live      ", false),
                CapturedRow::plain("tail      ", false),
                CapturedRow::plain("          ", false),
                CapturedRow::plain("          ", false),
            ]
        };
        projection
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 1,
                    column: 4,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        projection.scroll_by_rows(2);
        let frame = projection
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 1,
                    column: 4,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();

        assert_eq!(frame.scroll_offset_rows, 2);
        assert!(matches!(
            frame.viewport_origin,
            FrameViewportOrigin::Anchored(_)
        ));
        assert_eq!(frame.status_text, None);
    }

    #[test]
    fn internal_blank_live_row_is_reported_but_tail_capacity_is_not() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(16).unwrap());
        let mut document = HistoryDocument::default();
        for text in ["frozen-a", "frozen-b", "frozen-c", "frozen-d"] {
            document.finalize_transaction(
                store
                    .capture(CapturedRow::plain(text, false))
                    .finalized
                    .remove(0),
            );
        }
        let mut projection = ViewportProjection::new(
            key(10),
            DetectionRevision(1),
            nz32(5),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.project(&document);
        let live = || {
            vec![
                CapturedRow::plain("live-a    ", false),
                CapturedRow::plain("          ", false),
                CapturedRow::plain("live-b    ", false),
                CapturedRow::plain("          ", false),
                CapturedRow::plain("          ", false),
            ]
        };
        projection
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 2,
                    column: 6,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        projection.scroll_by_rows(4);
        let frame = projection
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 2,
                    column: 6,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();

        assert_eq!(frame.scroll_offset_rows, 4);
        assert!(matches!(
            frame.viewport_origin,
            FrameViewportOrigin::Anchored(_)
        ));
        assert_eq!(frame.status_text.as_deref(), Some("2 lines below"));
    }

    #[test]
    fn resize_reflow_does_not_masquerade_as_unread_content() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(16).unwrap());
        let mut document = HistoryDocument::default();
        document.finalize_transaction(
            store
                .capture(CapturedRow::plain("abcdefgh", false))
                .finalized
                .remove(0),
        );
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.project(&document);
        let wide_live = || {
            vec![
                CapturedRow::plain("live    ", false),
                CapturedRow::plain("tail    ", false),
            ]
        };
        projection
            .continuous_frame(
                &document,
                &[],
                wide_live(),
                GridCursor {
                    row: 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        projection.scroll_by_rows(1);
        assert_eq!(projection.scroll_offset_rows(), 1);

        projection.relayout(key(2), &document);
        let narrow_live = vec![
            CapturedRow::plain("li", false),
            CapturedRow::plain("ta", false),
        ];
        projection
            .continuous_frame(
                &document,
                &[],
                narrow_live,
                GridCursor {
                    row: 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        assert_eq!(projection.scroll_offset_rows(), 1);
        assert_eq!(projection.unread_rows(), 0);
    }

    #[test]
    fn anchored_reflow_pins_the_same_logical_offset_to_the_top_visual_row() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(16).unwrap());
        let mut document = HistoryDocument::default();
        document.finalize_transaction(
            store
                .capture(CapturedRow::plain("abcdefghijkl", false))
                .finalized
                .remove(0),
        );
        let mut projection = ViewportProjection::new(
            key(6),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.project(&document);
        let wide_live = vec![
            CapturedRow::plain("live  ", false),
            CapturedRow::plain("tail  ", false),
        ];
        projection
            .continuous_frame(
                &document,
                &[],
                wide_live,
                GridCursor {
                    row: 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        projection.scroll_by_rows(1);
        let anchored = projection
            .continuous_frame(
                &document,
                &[],
                vec![
                    CapturedRow::plain("live  ", false),
                    CapturedRow::plain("tail  ", false),
                ],
                GridCursor {
                    row: 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        let anchor_before = projection.scroll_anchor().unwrap().source.clone();
        assert!(matches!(
            anchor_before,
            ContentAnchor::History {
                offset: GraphemeOffset(6),
                ..
            }
        ));
        assert!(anchored.cell_anchors[..6].iter().any(|cell| {
            matches!(
                cell.start,
                ContentAnchor::History {
                    offset: GraphemeOffset(6),
                    ..
                }
            )
        }));

        projection.relayout(key(4), &document);
        let narrow = projection
            .continuous_frame(
                &document,
                &[],
                vec![
                    CapturedRow::plain("live", false),
                    CapturedRow::plain("tail", false),
                ],
                GridCursor {
                    row: 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        assert_eq!(projection.scroll_anchor().unwrap().source, anchor_before);
        assert!(narrow.cell_anchors[..4].iter().any(|cell| {
            matches!(
                cell.start,
                ContentAnchor::History {
                    offset: GraphemeOffset(6),
                    ..
                }
            )
        }));
    }

    #[test]
    fn staging_scroll_anchor_uses_the_g3_mapping_when_the_row_freezes() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(16).unwrap());
        store.capture(CapturedRow::plain("head", true));
        let staged = store.staged_rows().cloned().collect::<Vec<_>>();
        let mut document = HistoryDocument::default();
        let mut projection = ViewportProjection::new(
            key(4),
            DetectionRevision(1),
            nz32(1),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.project(&document);
        let live = || vec![CapturedRow::plain("tail", false)];
        projection
            .continuous_frame(
                &document,
                &staged,
                live(),
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        projection.scroll_by_rows(1);
        projection
            .continuous_frame(
                &document,
                &staged,
                live(),
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        assert!(matches!(
            projection.scroll_anchor().map(|anchor| &anchor.source),
            Some(ContentAnchor::Staging { .. })
        ));

        let finalized = store.finalize_all_candidates().remove(0);
        let frozen_id = finalized.line.id;
        document.finalize_transaction(finalized);
        projection.project(&document);
        let frame = projection
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        assert!(matches!(
            projection.scroll_anchor().map(|anchor| &anchor.source),
            Some(ContentAnchor::History { id, .. }) if *id == frozen_id
        ));
        assert!(matches!(
            &frame.viewport_origin,
            FrameViewportOrigin::Anchored(_)
        ));
    }

    #[test]
    fn word_and_line_selection_keep_wide_clusters_indivisible() {
        let mut wide = CapturedCell::plain("中");
        wide.style.flags.insert(CellFlags::WIDE_CHAR);
        let spacer = CapturedCell {
            wide_spacer: true,
            ..CapturedCell::default()
        };
        let row = CapturedRow {
            cells: vec![
                CapturedCell::plain("!"),
                wide,
                spacer,
                CapturedCell::plain("?"),
            ],
            continues: false,
            shell_mark: None,
        };
        let document = HistoryDocument::default();
        let mut projection = ViewportProjection::new(
            key(4),
            DetectionRevision(1),
            nz32(1),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let frame = projection
            .continuous_frame(
                &document,
                &[],
                vec![row],
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        assert_eq!(frame.word_selection(0, 1), frame.word_selection(0, 2));
        let selection = frame.word_selection(0, 2).unwrap().unwrap();
        projection.set_selection(Some(selection));
        let selected = projection
            .continuous_frame(
                &document,
                &[],
                vec![CapturedRow {
                    cells: frame.cells.clone(),
                    continues: false,
                    shell_mark: None,
                }],
                frame.cursor,
                ScreenId::Primary,
            )
            .unwrap();
        assert_eq!(
            selected.selection_spans,
            vec![SelectionSpan {
                row: 0,
                start_column: 1,
                end_column: 3,
            }]
        );
        let line = frame.line_selection(0).unwrap().unwrap();
        assert_eq!(
            line.start,
            frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap()
        );
        assert_eq!(
            line.end,
            frame.anchor_at(0, 3, Bias::After).unwrap().unwrap()
        );
    }

    #[test]
    fn frozen_wrap_count_respects_cluster_boundaries_instead_of_dividing_total_width() {
        assert_eq!(frozen_visual_line_count("中中中", 3), 3);
        assert_eq!(frozen_visual_line_count("a中", 3), 1);
    }
}
