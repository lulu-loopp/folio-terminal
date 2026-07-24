//! Per-viewport projection, layout cache and scroll anchoring.

mod height_tree;

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    num::{NonZeroI64, NonZeroU32},
    sync::Arc,
};

use bt_doc::{
    AnchorError, Bias, ContentAnchor, DetectionRevision, GridGeneration, GridPoint,
    HistoryDocument, LayoutKey, MathMode, ScreenId, ViewGeneration, compare_anchors,
};
use bt_transcript::{
    CapturedCell, CapturedRow, CellFlags, FrozenLine, GraphemeOffset, SourceGeneration, StagedRow,
    TranscriptId,
};
use bt_unicode::{cluster_width, graphemes};

pub use bt_doc::SUBPIXELS_PER_PX;
pub use height_tree::HeightTree;

/// Live display math is never given a lifecycle-specific presentation scale. Projection preserves
/// at least this many ordinary text rows and gives the remaining vertical attention budget to the
/// newest (lowest) complete formula blocks. Eight rows keep a prompt plus several answer/status
/// lines readable on conventional 24-row terminals without imposing a fixed formula count.
pub const LIVE_MATH_READABLE_SCALE_MILLI: u32 = 1000;
pub const LIVE_MIN_VISIBLE_TEXT_ROWS: u32 = 8;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LiveMathOccurrenceId(pub u64);

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

/// Geometry and input identity for one row in the last presented frame. Pixel consumers use the
/// prefix position here instead of independently multiplying the frame row by cell height.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameVisualRow {
    pub top_subpixels: i64,
    pub height_subpixels: i64,
    pub live_grid_row: Option<u32>,
}

/// Immutable CPU raster produced off the presentation thread. The renderer owns the independent
/// GPU texture cache; carrying pixels here keeps viewport projection deterministic and device-free.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectedMathArtifact {
    pub key: String,
    pub end: TranscriptId,
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
    pub height_subpixels: i64,
    pub baseline_subpixels: i64,
    pub mode: MathMode,
    /// Symmetric presentation breathing outside the alpha-tight texture. This is lifecycle-scale
    /// geometry, not part of the shared RGBA artifact.
    pub vertical_padding_subpixels: i64,
    /// Presentation scale for a same-source stale raster. Fresh artifacts use 1000.
    pub render_scale_milli: u32,
    pub source: String,
}

/// A rendered artifact tied to transient grid coordinates. Unlike history artifacts, this never
/// enters the primary document order or terminal reflow; its free height participates only in the
/// projection-local live prefix map while the screen and grid generation still match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectedLiveMathArtifact {
    pub occurrence_id: LiveMathOccurrenceId,
    pub screen: ScreenId,
    pub start: GridPoint,
    pub end: GridPoint,
    pub band_start_row: u32,
    pub band_end_row: u32,
    /// Rows of this proven block that remain above live row zero. They still participate in the
    /// complete presentation geometry; only their pixels and terminal cells are clipped.
    pub clipped_top_rows: u32,
    /// Rows of this proven block that remain below the last live row. They participate in the
    /// complete presentation geometry while pixels and terminal cells are bottom-clipped.
    pub clipped_bottom_rows: u32,
    /// Proven source rows hidden by an application-internal fixed region or its overlay boundary.
    /// Terminal-edge clipping is tracked separately above.
    pub occluded_source_rows: u32,
    /// Occluded terminal rows whose cells still carry this occurrence's proven source prefix.
    pub occluded_visible_rows: Vec<(u32, Vec<(u32, u32)>)>,
    pub generation: GridGeneration,
    pub artifact: ProjectedMathArtifact,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MathBlockAnchor {
    History {
        start: TranscriptId,
        end: TranscriptId,
    },
    Live {
        screen: ScreenId,
        start: GridPoint,
        end: GridPoint,
        band_start_row: u32,
        band_end_row: u32,
        generation: GridGeneration,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MathBlockDisplay {
    Rendered,
    Source,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HorizontalOverflowOwner {
    Block,
    Pane,
}

/// A visible math block replaces its complete source span. `top_subpixels` may be negative when
/// an anchored viewport starts inside a tall block; the renderer clips it to the pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathBlockPlacement {
    pub start: TranscriptId,
    pub anchor: MathBlockAnchor,
    pub source: String,
    pub artifact: ProjectedMathArtifact,
    pub top_subpixels: i64,
    pub left_subpixels: i64,
    /// Offset of rendered pixels within the owned row band. Live artifacts use this to distribute
    /// spare vertical space evenly without moving the band's clip or cleared terminal rows.
    pub content_offset_subpixels: i64,
    /// Clip height is part of the block itself (live row band or configured blockMax). The pane
    /// clip remains an independent outer bound in the renderer.
    pub clip_height_subpixels: i64,
    pub display: MathBlockDisplay,
    pub horizontal_overflow: HorizontalOverflowOwner,
    pub horizontal_scroll_px: u32,
    pub vertical_scroll_px: u32,
    pub toolbar_visible: bool,
    /// Diagnostic provenance for a still-live identity whose source is partly covered by fixed TUI
    /// chrome. A missing placement is never treated as occlusion.
    pub occluded_source_rows: u32,
    /// Source-proven occluded rows cleared from this frame; fixed chrome is never included.
    pub occluded_visible_rows: Vec<(u32, Vec<(u32, u32)>)>,
    /// Present only for live-grid math; survives repaint placement changes and distinguishes equal
    /// source text belonging to different occurrences.
    pub live_occurrence_id: Option<LiveMathOccurrenceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathFailurePlacement {
    pub anchor: MathBlockAnchor,
    pub top_subpixels: i64,
    pub height_subpixels: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameViewportOrigin {
    Bottom,
    /// Projection-local review of live pixels displaced above the pane. This is deliberately not
    /// a primary-history anchor and is therefore also valid for the isolated alternate screen.
    LiveOverflow {
        rows_below: usize,
    },
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
    pub row_map: Vec<FrameVisualRow>,
    pub selection_spans: Vec<SelectionSpan>,
    pub math_blocks: Vec<MathBlockPlacement>,
    pub math_failures: Vec<MathFailurePlacement>,
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
        if self.row_map.len() != self.rows.get() as usize {
            return Err(FrameShapeError::RowMapCount {
                expected: self.rows.get() as usize,
                actual: self.row_map.len(),
            });
        }
        for span in &self.selection_spans {
            self.selection_span_vertical_interval(span)?;
        }
        for block in &self.math_blocks {
            let MathBlockAnchor::Live {
                band_start_row,
                band_end_row,
                ..
            } = block.anchor
            else {
                continue;
            };
            if block.display != MathBlockDisplay::Rendered {
                continue;
            }
            if band_start_row > band_end_row {
                return Err(FrameShapeError::MathBlockBandOrder {
                    start: band_start_row,
                    end: band_end_row,
                });
            }
            if let Some(band_start) = self
                .row_map
                .iter()
                .find(|row| row.live_grid_row == Some(band_start_row))
                && block.top_subpixels != band_start.top_subpixels
            {
                return Err(FrameShapeError::MathBlockBandTop {
                    expected: band_start.top_subpixels,
                    actual: block.top_subpixels,
                });
            }
            let block_bottom = block
                .top_subpixels
                .saturating_add(block.clip_height_subpixels);
            if let Some(band_end) = self
                .row_map
                .iter()
                .find(|row| row.live_grid_row == Some(band_end_row))
            {
                let band_bottom = band_end
                    .top_subpixels
                    .saturating_add(band_end.height_subpixels);
                if block_bottom > band_bottom {
                    return Err(FrameShapeError::MathBlockBeyondBand {
                        band_bottom,
                        block_bottom,
                    });
                }
            }
            for row in self.row_map.iter().filter(|row| {
                row.live_grid_row
                    .is_some_and(|live| !(band_start_row..=band_end_row).contains(&live))
            }) {
                let row_bottom = row.top_subpixels.saturating_add(row.height_subpixels);
                if block.top_subpixels < row_bottom && row.top_subpixels < block_bottom {
                    return Err(FrameShapeError::MathBlockOverlapsOutsideRow {
                        row: row.live_grid_row.unwrap_or_default(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Return the exact vertical drawing interval for a selection span. Selection renderers use
    /// this frame-owned interval instead of reconstructing a row position from nominal cell
    /// height, and `validate_shape` rejects every span for which no such interval exists.
    pub fn selection_span_vertical_interval(
        &self,
        span: &SelectionSpan,
    ) -> Result<std::ops::Range<i64>, FrameShapeError> {
        let mapped = self.row_map.get(span.row as usize).ok_or(
            FrameShapeError::SelectionSpanRowOutOfBounds {
                row: span.row,
                rows: self.row_map.len(),
            },
        )?;
        let Some(bottom) = mapped.top_subpixels.checked_add(mapped.height_subpixels) else {
            return Err(FrameShapeError::SelectionSpanInvalidInterval {
                row: span.row,
                top: mapped.top_subpixels,
                height: mapped.height_subpixels,
            });
        };
        if bottom <= mapped.top_subpixels {
            return Err(FrameShapeError::SelectionSpanInvalidInterval {
                row: span.row,
                top: mapped.top_subpixels,
                height: mapped.height_subpixels,
            });
        }
        Ok(mapped.top_subpixels..bottom)
    }

    pub fn visual_row_at(&self, y_subpixels: i64) -> Option<u32> {
        self.row_map
            .iter()
            .position(|row| {
                row.top_subpixels <= y_subpixels
                    && y_subpixels < row.top_subpixels.saturating_add(row.height_subpixels)
            })
            .and_then(|row| u32::try_from(row).ok())
    }

    pub fn live_point_at(&self, row: u32, column: u32) -> Option<GridPoint> {
        let live_row = self.row_map.get(row as usize)?.live_grid_row?;
        Some(GridPoint {
            row: live_row,
            column: column.min(self.columns.get().saturating_sub(1)),
        })
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
    RowMapCount { expected: usize, actual: usize },
    SelectionSpanRowOutOfBounds { row: u32, rows: usize },
    SelectionSpanInvalidInterval { row: u32, top: i64, height: i64 },
    MathBlockBandOrder { start: u32, end: u32 },
    MathBlockBandTop { expected: i64, actual: i64 },
    MathBlockBeyondBand { band_bottom: i64, block_bottom: i64 },
    MathBlockOverlapsOutsideRow { row: u32 },
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
            Self::RowMapCount { expected, actual } => {
                write!(
                    formatter,
                    "frame requires {expected} visual rows, got {actual}"
                )
            }
            Self::SelectionSpanRowOutOfBounds { row, rows } => write!(
                formatter,
                "selection span row {row} is outside the frame row map with {rows} rows"
            ),
            Self::SelectionSpanInvalidInterval { row, top, height } => write!(
                formatter,
                "selection span row {row} has invalid drawing interval top={top} height={height}"
            ),
            Self::MathBlockBandOrder { start, end } => {
                write!(
                    formatter,
                    "live math band starts at row {start} after row {end}"
                )
            }
            Self::MathBlockBandTop { expected, actual } => write!(
                formatter,
                "live math block top is {actual} subpixels, band starts at {expected}"
            ),
            Self::MathBlockBeyondBand {
                band_bottom,
                block_bottom,
            } => write!(
                formatter,
                "live math block ends at {block_bottom} subpixels beyond band bottom {band_bottom}"
            ),
            Self::MathBlockOverlapsOutsideRow { row } => {
                write!(formatter, "live math block overlaps outside grid row {row}")
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

/// Center one complete presentation box (tight ink plus symmetric breathing) in either a borrowed
/// fixed-height band or a free-height-expanded band. Both paths use this integer calculation, so
/// their top/bottom remainder can differ by at most one subpixel.
fn centered_content_offset(
    band_height_subpixels: i64,
    box_height_subpixels: i64,
    vertical_padding_subpixels: i64,
) -> i64 {
    band_height_subpixels
        .saturating_sub(box_height_subpixels)
        .max(0)
        .div_euclid(2)
        .saturating_add(vertical_padding_subpixels)
}

fn distributed_row_heights(total_height_subpixels: i64, rows: usize) -> Vec<i64> {
    if rows == 0 {
        return Vec::new();
    }
    let total = total_height_subpixels.max(1);
    let row_count = i64::try_from(rows).unwrap_or(i64::MAX);
    let base = total.div_euclid(row_count);
    let remainder = usize::try_from(total.rem_euclid(row_count)).unwrap_or(usize::MAX);
    (0..rows)
        .map(|row| base + if row < remainder { 1 } else { 0 })
        .collect()
}

fn continuous_row_top_subpixels(
    row: usize,
    live_base: usize,
    live_row_prefix: &[i64],
    cell_height_subpixels: i64,
) -> i64 {
    let fixed_rows = row.min(live_base);
    let fixed_height = i64::try_from(fixed_rows)
        .unwrap_or(i64::MAX)
        .saturating_mul(cell_height_subpixels);
    if row <= live_base {
        return fixed_height;
    }
    let live_row = row.saturating_sub(live_base);
    let live_height = live_row_prefix.get(live_row).copied().unwrap_or_else(|| {
        let known_rows = live_row_prefix.len().saturating_sub(1);
        live_row_prefix.last().copied().unwrap_or(0).saturating_add(
            i64::try_from(live_row.saturating_sub(known_rows))
                .unwrap_or(i64::MAX)
                .saturating_mul(cell_height_subpixels),
        )
    });
    fixed_height.saturating_add(live_height)
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
    math_artifacts: HashMap<TranscriptId, ProjectedMathArtifact>,
    live_math_artifacts: Vec<ProjectedLiveMathArtifact>,
    live_row_prefix: Vec<i64>,
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
    /// A review offset preserved across an application transcript rewrite. Codex-style TUIs
    /// reflow by clearing scrollback (ED3) and reprinting equivalent content; the anchored row
    /// the user was reading dies with the clear, but their review displacement is still
    /// meaningful, so it is re-established by row count as history refills instead of snapping
    /// the view to the bottom. Any explicit scroll action supersedes it.
    displaced_review_rows: Option<usize>,
    live_overflow_offset_rows: usize,
    last_live_overflow_rows: usize,
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
            math_artifacts: HashMap::new(),
            live_math_artifacts: Vec::new(),
            live_row_prefix: (0..=live_rows.get())
                .map(|row| i64::from(row).saturating_mul(cell_height_subpixels.get()))
                .collect(),
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
            displaced_review_rows: None,
            live_overflow_offset_rows: 0,
            last_live_overflow_rows: 0,
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
            .saturating_add(self.live_overflow_offset_rows)
    }

    pub fn unread_rows(&self) -> usize {
        self.unread_rows
    }

    /// Diagnostic snapshot of the scroll-extent bookkeeping: the last projected total row count,
    /// the history offset, the live-overflow allowance, and how much of it is consumed.
    pub fn debug_scroll_extent(&self) -> (usize, usize, usize, usize, usize) {
        (
            self.last_total_rows,
            self.pending_scroll_offset_rows
                .unwrap_or(self.scroll_offset_rows),
            self.last_live_overflow_rows,
            self.live_overflow_offset_rows,
            self.unread_rows,
        )
    }

    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset_rows() != 0
    }

    /// Positive rows move into history; negative rows move toward the live bottom.
    pub fn scroll_by_rows(&mut self, rows: i32) {
        // An explicit scroll is the user taking over: any preserved review displacement from an
        // application transcript rewrite is superseded.
        self.displaced_review_rows = None;
        let max = self
            .last_total_rows
            .saturating_sub(self.live_rows.get() as usize);
        let mut history = self
            .pending_scroll_offset_rows
            .unwrap_or(self.scroll_offset_rows);
        if rows >= 0 {
            let requested = rows as usize;
            let local_capacity = self
                .last_live_overflow_rows
                .saturating_sub(self.live_overflow_offset_rows);
            let local = requested.min(local_capacity);
            self.live_overflow_offset_rows = self.live_overflow_offset_rows.saturating_add(local);
            history = history
                .saturating_add(requested.saturating_sub(local))
                .min(max);
        } else {
            let requested = rows.unsigned_abs() as usize;
            let history_delta = requested.min(history);
            history = history.saturating_sub(history_delta);
            self.live_overflow_offset_rows = self
                .live_overflow_offset_rows
                .saturating_sub(requested.saturating_sub(history_delta));
        }
        self.pending_scroll_offset_rows = Some(history);
        if history == 0 {
            self.scroll_state = ViewportScrollState::Bottom;
            self.scroll_offset_rows = 0;
            self.unread_rows = 0;
        }
        self.view_generation.0 += 1;
    }

    pub fn scroll_to_top(&mut self) {
        self.displaced_review_rows = None;
        let offset = self
            .last_total_rows
            .saturating_sub(self.live_rows.get() as usize);
        self.live_overflow_offset_rows = self.last_live_overflow_rows;
        self.pending_scroll_offset_rows = Some(offset);
        if offset == 0 {
            self.scroll_state = ViewportScrollState::Bottom;
        }
        self.view_generation.0 += 1;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.displaced_review_rows = None;
        if self.is_scrolled()
            || !matches!(self.scroll_state, ViewportScrollState::Bottom)
            || self.unread_rows != 0
        {
            self.scroll_state = ViewportScrollState::Bottom;
            self.scroll_offset_rows = 0;
            self.pending_scroll_offset_rows = None;
            self.live_overflow_offset_rows = 0;
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
            row_map: (0..self.live_rows.get())
                .map(|row| FrameVisualRow {
                    top_subpixels: i64::from(row).saturating_mul(self.cell_height_subpixels.get()),
                    height_subpixels: self.cell_height_subpixels.get(),
                    live_grid_row: Some(row),
                })
                .collect(),
            selection_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
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
        let live_height = self.live_row_prefix.last().copied().unwrap_or_else(|| {
            i64::from(self.live_rows.get()).saturating_mul(self.cell_height_subpixels.get())
        });
        let rectangular_live_height =
            i64::from(self.live_rows.get()).saturating_mul(self.cell_height_subpixels.get());
        let live_height_delta = live_height.saturating_sub(rectangular_live_height);
        let live_extra_height = live_height_delta.max(0);
        let live_rows_above = usize::try_from(
            live_extra_height
                .saturating_add(self.cell_height_subpixels.get() - 1)
                .div_euclid(self.cell_height_subpixels.get()),
        )
        .unwrap_or(usize::MAX);
        self.last_live_overflow_rows = live_rows_above;
        self.live_overflow_offset_rows = self
            .live_overflow_offset_rows
            .min(self.last_live_overflow_rows);
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
            if self.live_overflow_offset_rows == 0 {
                self.unread_rows = 0;
            }
        } else if let Some(requested_offset) = self.pending_scroll_offset_rows.take() {
            let offset = requested_offset.min(max_offset);
            if offset == 0 {
                self.scroll_state = ViewportScrollState::Bottom;
            } else {
                let target_row = bottom_start.saturating_sub(offset);
                self.scroll_state = self
                    .scroll_anchor_at_absolute_row(
                        document,
                        staged_rows,
                        history_rows,
                        target_row,
                        screen,
                    )
                    .map_or(ViewportScrollState::Bottom, ViewportScrollState::Anchored);
            }
        } else if let Some(displaced) = self.displaced_review_rows {
            // Re-establish a review displacement preserved across an application transcript
            // rewrite: as the reprint refills history the offset deepens frame by frame, and the
            // preservation completes once the full displacement (or the new maximum) is reachable.
            let offset = displaced.min(max_offset);
            if offset != 0 {
                let target_row = bottom_start.saturating_sub(offset);
                if let Some(anchor) = self.scroll_anchor_at_absolute_row(
                    document,
                    staged_rows,
                    history_rows,
                    target_row,
                    screen,
                ) {
                    self.scroll_state = ViewportScrollState::Anchored(anchor);
                    if offset == displaced {
                        self.displaced_review_rows = None;
                    }
                }
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
                // The anchored content vanished under the reader — a Codex-style reflow clears
                // scrollback before reprinting equivalent content. Preserve the displacement so
                // the refilled history restores the reading position instead of jumping to the
                // bottom; a review already being restored keeps its original target.
                if self.scroll_offset_rows != 0 {
                    self.displaced_review_rows = Some(
                        self.displaced_review_rows
                            .map_or(self.scroll_offset_rows, |kept| {
                                kept.max(self.scroll_offset_rows)
                            }),
                    );
                }
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
        // A primary history anchor is already reviewing the continuous document above live. If a
        // live raster arrives while that anchor is installed, its new tail height must not move
        // the anchored pixels. Bottom/alternate views instead consume the projection-local live
        // overflow explicitly before entering primary history.
        let reviewed_live_height = if matches!(self.scroll_state, ViewportScrollState::Anchored(_))
        {
            live_extra_height
        } else {
            i64::try_from(self.live_overflow_offset_rows)
                .unwrap_or(i64::MAX)
                .saturating_mul(self.cell_height_subpixels.get())
                .min(live_extra_height)
        };
        debug_assert!(primary || live_height_delta >= 0);
        let frame_top_subpixels = reviewed_live_height.saturating_sub(live_extra_height);
        let rows_above = usize::try_from(
            frame_top_subpixels
                .saturating_neg()
                // Bottom-anchoring leaves positive top slack (blank), not hidden rows: its negated
                // value is below zero and must read as zero rows above, never wrap to usize::MAX.
                .max(0)
                .saturating_add(self.cell_height_subpixels.get() - 1)
                .div_euclid(self.cell_height_subpixels.get()),
        )
        .unwrap_or(usize::MAX);
        let window_top_subpixels = continuous_row_top_subpixels(
            window_start,
            live_base,
            &self.live_row_prefix,
            self.cell_height_subpixels.get(),
        );
        let mut visible = Vec::with_capacity(expected_rows);
        let mut visible_heights = Vec::with_capacity(expected_rows);
        let mut math_blocks = Vec::new();

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
                    let (laid_out, laid_out_heights) =
                        if let Some(artifact) = self.math_artifacts.get(id) {
                            let max_offset =
                                entry.line.grapheme_boundaries.len().saturating_sub(1) as u32;
                            let local_start = window_start.saturating_sub(row_base);
                            let row_heights =
                                distributed_row_heights(artifact.height_subpixels, line_rows);
                            let visible_top = frame_top_subpixels
                                .saturating_add(visible_heights.iter().copied().sum::<i64>());
                            math_blocks.push(MathBlockPlacement {
                                start: *id,
                                anchor: MathBlockAnchor::History {
                                    start: *id,
                                    end: artifact.end,
                                },
                                source: artifact.source.clone(),
                                artifact: artifact.clone(),
                                top_subpixels: visible_top.saturating_sub(
                                    row_heights[..local_start.min(row_heights.len())]
                                        .iter()
                                        .copied()
                                        .sum::<i64>(),
                                ),
                                left_subpixels: 0,
                                content_offset_subpixels: artifact.vertical_padding_subpixels,
                                clip_height_subpixels: artifact.height_subpixels,
                                display: MathBlockDisplay::Rendered,
                                horizontal_overflow: HorizontalOverflowOwner::Block,
                                horizontal_scroll_px: 0,
                                vertical_scroll_px: 0,
                                toolbar_visible: false,
                                occluded_source_rows: 0,
                                occluded_visible_rows: Vec::new(),
                                live_occurrence_id: None,
                            });
                            (
                                (0..line_rows)
                                    .map(|_| {
                                        blank_visual_row(column_count, |_, bias| {
                                            ContentAnchor::History {
                                                id: *id,
                                                offset: if bias == Bias::Before {
                                                    GraphemeOffset(0)
                                                } else {
                                                    GraphemeOffset(max_offset)
                                                },
                                                bias,
                                                generation: entry.line.source_generation,
                                            }
                                        })
                                    })
                                    .collect::<Vec<_>>(),
                                row_heights,
                            )
                        } else {
                            let rows = layout_frozen_line(&entry.line, column_count);
                            let heights = vec![self.cell_height_subpixels.get(); rows.len()];
                            (rows, heights)
                        };
                    for (local_row, row) in laid_out.iter().enumerate() {
                        validate_visual_row(row, column_count, "history", row_base + local_row)?;
                    }
                    let local_start = window_start.saturating_sub(row_base);
                    let local_end = window_end.saturating_sub(row_base).min(laid_out.len());
                    visible.extend(laid_out[local_start..local_end].iter().cloned());
                    visible_heights.extend_from_slice(&laid_out_heights[local_start..local_end]);
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
                visible_heights.push(self.cell_height_subpixels.get());
            }
        }

        if window_end > live_base && window_start < live_base + expected_rows {
            let first = window_start.saturating_sub(live_base);
            let last = window_end.saturating_sub(live_base).min(live_rows.len());
            let visible_live_start = visible.len();
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
            visible_heights.extend((first..last).map(|live_row| {
                self.live_row_prefix[live_row + 1].saturating_sub(self.live_row_prefix[live_row])
            }));

            for live_math in &self.live_math_artifacts {
                if live_math.screen != screen
                    || live_math.generation != self.grid_generation
                    || live_math.start.row > live_math.end.row
                    || live_math.end.row >= self.live_rows.get()
                    || live_math.band_start_row > live_math.start.row
                    || live_math.band_end_row < live_math.end.row
                    || live_math.band_end_row >= self.live_rows.get()
                {
                    continue;
                }
                let block_first = live_math.band_start_row as usize;
                let block_last = live_math.band_end_row as usize;
                if block_last < first || block_first >= last {
                    continue;
                }

                let band_height = self.live_row_prefix[block_last + 1]
                    .saturating_sub(self.live_row_prefix[block_first]);
                let artifact = live_math.artifact.clone();
                debug_assert_eq!(artifact.render_scale_milli, LIVE_MATH_READABLE_SCALE_MILLI);
                let total_rows = live_math
                    .clipped_top_rows
                    .saturating_add(
                        live_math
                            .band_end_row
                            .saturating_sub(live_math.band_start_row)
                            .saturating_add(1),
                    )
                    .saturating_add(live_math.clipped_bottom_rows);
                let source_band_height =
                    i64::from(total_rows).saturating_mul(self.cell_height_subpixels.get());
                let presentation_height = if screen == ScreenId::Alternate {
                    artifact.height_subpixels.max(source_band_height)
                } else {
                    band_height
                };
                let content_offset_subpixels = centered_content_offset(
                    presentation_height,
                    artifact.height_subpixels,
                    artifact.vertical_padding_subpixels,
                );
                let absolute_start = live_base.saturating_add(block_first);
                let top_subpixels = frame_top_subpixels.saturating_add(
                    continuous_row_top_subpixels(
                        absolute_start,
                        live_base,
                        &self.live_row_prefix,
                        self.cell_height_subpixels.get(),
                    )
                    .saturating_sub(window_top_subpixels),
                );
                math_blocks.push(MathBlockPlacement {
                    start: TranscriptId(0),
                    anchor: MathBlockAnchor::Live {
                        screen: live_math.screen,
                        start: live_math.start,
                        end: live_math.end,
                        band_start_row: live_math.band_start_row,
                        band_end_row: live_math.band_end_row,
                        generation: live_math.generation,
                    },
                    source: artifact.source.clone(),
                    artifact,
                    top_subpixels,
                    left_subpixels: 0,
                    content_offset_subpixels,
                    // The shared live prefix map expands this owned band before all following
                    // logical rows. It never paints into a neighbour's fixed terminal row.
                    clip_height_subpixels: band_height,
                    display: MathBlockDisplay::Rendered,
                    horizontal_overflow: HorizontalOverflowOwner::Block,
                    horizontal_scroll_px: 0,
                    vertical_scroll_px: 0,
                    toolbar_visible: false,
                    occluded_source_rows: live_math.occluded_source_rows,
                    occluded_visible_rows: live_math.occluded_visible_rows.clone(),
                    live_occurrence_id: Some(live_math.occurrence_id),
                });

                let visible_first = block_first.max(first);
                let visible_last = block_last.min(last.saturating_sub(1));
                for live_row in visible_first..=visible_last {
                    let row = &mut visible[visible_live_start + live_row - first];
                    for cell in &mut row.cells {
                        cell.text.clear();
                        cell.wide_spacer = false;
                        cell.style.flags.remove(CellFlags::WIDE_CHAR);
                    }
                }
                for (live_row, clear_ranges) in &live_math.occluded_visible_rows {
                    let live_row = *live_row as usize;
                    if live_row < first || live_row >= last {
                        continue;
                    }
                    let row = &mut visible[visible_live_start + live_row - first];
                    // Only cells proven to show this occurrence's source are cleared; an
                    // application overlay sharing the row (Jump chip) keeps its text and
                    // highlight style untouched on both sides.
                    for (start, end) in clear_ranges {
                        let start = (*start as usize).min(row.cells.len());
                        let end = (*end as usize).min(row.cells.len());
                        for cell in &mut row.cells[start..end] {
                            cell.text.clear();
                            cell.wide_spacer = false;
                            cell.style.flags.remove(CellFlags::WIDE_CHAR);
                        }
                    }
                }
            }
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
            visible_heights.push(self.cell_height_subpixels.get());
        }
        visible.truncate(expected_rows);
        visible_heights.truncate(expected_rows);
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
        let mut next_top = frame_top_subpixels;
        let row_map = (0..expected_rows)
            .map(|frame_row| {
                let absolute_row = window_start.saturating_add(frame_row);
                let live_grid_row = absolute_row
                    .checked_sub(live_base)
                    .filter(|row| *row < expected_rows)
                    .and_then(|row| u32::try_from(row).ok());
                let height_subpixels = visible_heights
                    .get(frame_row)
                    .copied()
                    .unwrap_or(self.cell_height_subpixels.get());
                let mapped = FrameVisualRow {
                    top_subpixels: next_top,
                    height_subpixels,
                    live_grid_row,
                };
                next_top = next_top.saturating_add(height_subpixels);
                mapped
            })
            .collect::<Vec<_>>();
        for placement in &mut math_blocks {
            if let MathBlockAnchor::Live { band_start_row, .. } = placement.anchor
                && let Some(mapped) = row_map
                    .iter()
                    .find(|mapped| mapped.live_grid_row == Some(band_start_row))
            {
                placement.top_subpixels = mapped.top_subpixels;
            }
        }
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
        let cursor_hidden_by_math = math_blocks.iter().any(|placement| {
            matches!(
                placement.anchor,
                MathBlockAnchor::Live {
                    screen: anchor_screen,
                    start,
                    end,
                    generation,
                    ..
                } if anchor_screen == screen
                    && generation == self.grid_generation
                    && start.row <= cursor.row
                    && cursor.row <= end.row
            )
        });
        let frame = ViewportFrame {
            columns,
            rows: self.live_rows,
            cells,
            cursor: GridCursor {
                row: projected_cursor_row.unwrap_or(cursor.row),
                visible: cursor.visible && projected_cursor_row.is_some() && !cursor_hidden_by_math,
                ..cursor
            },
            cell_anchors,
            row_map,
            selection_spans,
            math_blocks,
            math_failures: Vec::new(),
            status_text: if rows_above != 0 {
                if primary {
                    Some(format!("{rows_above} rows above"))
                } else {
                    // Plain wheel belongs to the application on the alternate screen (M1.7);
                    // these projection-local rows are only reachable through the explicit local
                    // override, so the indicator itself must teach that affordance.
                    Some(format!("{rows_above} rows above · Shift+wheel"))
                }
            } else if content_rows_below != 0 {
                Some(format!("{content_rows_below} lines below"))
            } else if self.live_overflow_offset_rows != 0 {
                Some(format!("{} rows below", self.live_overflow_offset_rows))
            } else {
                None
            },
            viewport_origin: match &self.scroll_state {
                ViewportScrollState::Bottom if self.live_overflow_offset_rows == 0 => {
                    FrameViewportOrigin::Bottom
                }
                ViewportScrollState::Bottom => FrameViewportOrigin::LiveOverflow {
                    rows_below: self.live_overflow_offset_rows,
                },
                ViewportScrollState::Anchored(anchor) => {
                    FrameViewportOrigin::Anchored(anchor.clone())
                }
            },
            scroll_offset_rows: self
                .scroll_offset_rows
                .saturating_add(self.live_overflow_offset_rows),
            layout_key: self.layout_key,
            view_generation: self.view_generation,
        };
        frame
            .validate_shape()
            .map_err(FrameProjectionError::FrameShape)?;
        Ok(frame)
    }

    fn scroll_anchor_at_absolute_row(
        &self,
        document: &HistoryDocument,
        staged_rows: &[StagedRow],
        history_rows: usize,
        absolute_row: usize,
        screen: ScreenId,
    ) -> Option<ScrollAnchor> {
        if absolute_row < history_rows {
            let index = self
                .visual_row_heights
                .index_at_offset(absolute_row as i64)?;
            let row_base = self.visual_row_heights.prefix_sum(index) as usize;
            let id = self.ordered_ids.get(index)?;
            let entry = document.entries().get(id)?;
            let local_row = absolute_row.saturating_sub(row_base);
            if self.math_artifacts.contains_key(id) {
                return Some(ScrollAnchor {
                    source: ContentAnchor::History {
                        id: *id,
                        offset: GraphemeOffset(0),
                        bias: Bias::Before,
                        generation: entry.line.source_generation,
                    },
                    local_offset: (local_row as i64)
                        .saturating_mul(self.cell_height_subpixels.get()),
                });
            }
            return layout_frozen_line(&entry.line, self.layout_key.width_cells.get() as usize)
                .get(local_row)?
                .anchors
                .first()
                .map(|anchor| ScrollAnchor {
                    source: anchor.start.clone(),
                    local_offset: 0,
                });
        }

        let staging_row = absolute_row.saturating_sub(history_rows);
        if let Some(staged) = staged_rows.get(staging_row) {
            return Some(ScrollAnchor {
                source: ContentAnchor::Staging {
                    id: staged.id,
                    offset: GraphemeOffset(0),
                    bias: Bias::Before,
                    generation: self.source_generation,
                },
                local_offset: 0,
            });
        }

        let live_row = staging_row.saturating_sub(staged_rows.len());
        (live_row < self.live_rows.get() as usize).then_some(ScrollAnchor {
            source: ContentAnchor::Live {
                screen,
                point: GridPoint {
                    row: live_row as u32,
                    column: 0,
                },
                bias: Bias::Before,
                generation: self.grid_generation,
            },
            local_offset: 0,
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
                let (index, projected_id) = self.projected_history_index(*id)?;
                let entry = document.entries().get(id)?;
                if *generation != entry.line.source_generation {
                    return None;
                }
                if self.math_artifacts.contains_key(&projected_id) {
                    return Some(self.visual_row_heights.prefix_sum(index) as usize);
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
    pub fn sync_math_artifacts(
        &mut self,
        artifacts: impl IntoIterator<Item = (TranscriptId, ProjectedMathArtifact)>,
    ) {
        let next = artifacts.into_iter().collect::<HashMap<_, _>>();
        let changed = self
            .math_artifacts
            .keys()
            .chain(next.keys())
            .copied()
            .filter(|id| self.math_artifacts.get(id) != next.get(id))
            .collect::<HashSet<_>>();
        if !changed.is_empty() {
            self.cache
                .retain(|key, _| !changed.contains(&key.span.start));
            self.projection_dirty = true;
        }
        self.artifact_heights = next
            .iter()
            .map(|(id, artifact)| (*id, artifact.height_subpixels))
            .collect();
        self.math_artifacts = next;
    }
    pub fn sync_live_math_artifacts(
        &mut self,
        screen: ScreenId,
        artifacts: impl IntoIterator<Item = ProjectedLiveMathArtifact>,
    ) {
        let candidates = artifacts
            .into_iter()
            .filter(|artifact| {
                artifact.screen == screen && artifact.generation == self.grid_generation
            })
            .collect::<Vec<_>>();
        let accepted = if screen == ScreenId::Alternate {
            // Alternate presentation is expand-only: every proven Ready block remains rendered.
            // Extra pixels overflow above the fixed N x M grid; they never consume or move the
            // application's input/status rows at the bottom.
            candidates
        } else {
            let mut remaining = i64::from(
                self.live_rows
                    .get()
                    .saturating_sub(LIVE_MIN_VISIBLE_TEXT_ROWS),
            )
            .saturating_mul(self.cell_height_subpixels.get());
            let mut accepted = Vec::new();
            // Primary keeps its existing visible-text floor and newest/lower-block preference.
            for artifact in candidates.into_iter().rev() {
                let box_height = artifact.artifact.height_subpixels.max(1);
                if artifact.artifact.render_scale_milli == LIVE_MATH_READABLE_SCALE_MILLI
                    && box_height <= remaining
                {
                    remaining = remaining.saturating_sub(box_height);
                    accepted.push(artifact);
                } else if std::env::var_os("BT_PERF_TRACE").is_some() {
                    let reason = if box_height
                        > i64::from(
                            self.live_rows
                                .get()
                                .saturating_sub(LIVE_MIN_VISIBLE_TEXT_ROWS),
                        )
                        .saturating_mul(self.cell_height_subpixels.get())
                    {
                        "block-exceeds-visible-text-floor"
                    } else {
                        "newer-blocks-reserved-visible-text-floor"
                    };
                    eprintln!(
                        "BT_PERF_TRACE live_math_event=source-fallback row={} box_subpixels={} remaining_subpixels={} min_text_rows={} reason={reason}",
                        artifact.start.row, box_height, remaining, LIVE_MIN_VISIBLE_TEXT_ROWS,
                    );
                }
            }
            accepted.reverse();
            accepted
        };
        let mut per_row_height =
            vec![self.cell_height_subpixels.get(); self.live_rows.get() as usize];
        for artifact in &accepted {
            let visible_rows = artifact
                .band_end_row
                .saturating_sub(artifact.band_start_row)
                .saturating_add(1);
            let rows = artifact
                .clipped_top_rows
                .saturating_add(visible_rows)
                .saturating_add(artifact.clipped_bottom_rows);
            let source_band_height =
                i64::from(rows).saturating_mul(self.cell_height_subpixels.get());
            let presentation_height = if screen == ScreenId::Alternate {
                artifact.artifact.height_subpixels.max(source_band_height)
            } else {
                artifact.artifact.height_subpixels
            };
            let heights = distributed_row_heights(presentation_height, rows.max(1) as usize);
            // Primary retains free height. Alternate is expand-only: a short formula keeps the
            // complete source-row band and centers inside it; a tall formula expands above it.
            for offset in 0..visible_rows {
                if let Some(height) =
                    per_row_height.get_mut(artifact.band_start_row.saturating_add(offset) as usize)
                    && let Some(distributed) =
                        heights.get(artifact.clipped_top_rows.saturating_add(offset) as usize)
                {
                    *height = *distributed;
                }
            }
            if screen == ScreenId::Alternate && artifact.clipped_top_rows > 0 {
                // Terminal-edge clipping removes logical rows, not their upward presentation
                // extent. Fold the clipped-top slice into the first visible band row so the live
                // prefix still measures the complete height that was pushed above the fixed grid.
                // Bottom anchoring consumes this added height at the pane top; local review can
                // then spend the same amount to bring the complete box back, with a non-negative
                // content offset. Clipped-bottom rows remain outside this upward reveal extent.
                let clipped_top_height = heights
                    .iter()
                    .take(artifact.clipped_top_rows as usize)
                    .copied()
                    .sum::<i64>();
                if let Some(first_visible_height) =
                    per_row_height.get_mut(artifact.band_start_row as usize)
                {
                    *first_visible_height = first_visible_height.saturating_add(clipped_top_height);
                }
            }
        }
        let mut prefix = Vec::with_capacity(per_row_height.len() + 1);
        prefix.push(0_i64);
        for height in per_row_height {
            let next = prefix.last().copied().unwrap_or(0).saturating_add(height);
            prefix.push(next);
        }
        if self.live_math_artifacts != accepted || self.live_row_prefix != prefix {
            self.view_generation.0 = self.view_generation.0.saturating_add(1);
        }
        self.live_math_artifacts = accepted;
        self.live_row_prefix = prefix;
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
        if self.grid_generation != grid_generation {
            self.live_overflow_offset_rows = 0;
            self.last_live_overflow_rows = 0;
        }
        self.live_rows = live_rows;
        self.source_generation = source_generation;
        self.grid_generation = grid_generation;
        if self.live_row_prefix.len() != live_rows.get() as usize + 1 {
            self.live_row_prefix = (0..=live_rows.get())
                .map(|row| i64::from(row).saturating_mul(self.cell_height_subpixels.get()))
                .collect();
            self.live_math_artifacts.clear();
        }
    }

    pub fn project(&mut self, document: &HistoryDocument) {
        let mut next_ids = Vec::new();
        let mut suppressed_through = None;
        for id in document.entries().keys().copied() {
            if suppressed_through.is_some_and(|end| id <= end) {
                continue;
            }
            next_ids.push(id);
            suppressed_through = self.math_artifacts.get(&id).map(|artifact| artifact.end);
        }
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
                    end: self
                        .math_artifacts
                        .get(id)
                        .map_or(*id, |artifact| artifact.end),
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
                        let visual_lines = height
                            .max(1)
                            .saturating_add(self.cell_height_subpixels.get() - 1)
                            / self.cell_height_subpixels.get();
                        MeasuredLayout {
                            visual_lines: u32::try_from(visual_lines).unwrap_or(u32::MAX),
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
                let (index, projected_id) = self
                    .projected_history_index(*id)
                    .ok_or(AnchorError::UnknownAnchor)?;
                let entry = document
                    .entries()
                    .get(id)
                    .ok_or(AnchorError::UnknownAnchor)?;
                if *generation != entry.line.source_generation {
                    return Err(AnchorError::StaleGeneration);
                }
                let max_offset = entry.line.grapheme_boundaries.len().saturating_sub(1) as u32;
                let local_y = if self.math_artifacts.contains_key(&projected_id) {
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
                Ok(self.heights.total() + self.live_row_prefix[point.row as usize])
            }
            ContentAnchor::Live {
                screen: ScreenId::Alternate,
                ..
            } => Err(AnchorError::IsolatedScreen),
        }
    }

    fn projected_history_index(&self, id: TranscriptId) -> Option<(usize, TranscriptId)> {
        self.ordered_ids
            .iter()
            .copied()
            .enumerate()
            .find(|(_, start)| {
                *start == id
                    || self
                        .math_artifacts
                        .get(start)
                        .is_some_and(|artifact| *start <= id && id <= artifact.end)
            })
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
        // The spacer column belongs to the same on-screen cell as its wide glyph: it must carry
        // the glyph's style (background, hyperlink) exactly like the live grid's spacer does, or
        // a background bar behind CJK text breaks into per-glyph stripes once the line freezes
        // into history.
        let spacer = (width == 2).then(|| {
            let mut spacer_style = cell.style.clone();
            spacer_style.flags.remove(CellFlags::WIDE_CHAR);
            CapturedCell {
                wide_spacer: true,
                style: spacer_style,
                hyperlink: cell.hyperlink.clone(),
                ..CapturedCell::default()
            }
        });
        let row = rows.last_mut().unwrap();
        row.cells.push(cell);
        row.anchors.push(CellAnchor {
            start: start.clone(),
            end: end.clone(),
        });
        if let Some(spacer) = spacer {
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
    fn borrowed_and_free_height_paths_share_subpixel_exact_vertical_centering() {
        let padding = 2 * SUBPIXELS_PER_PX + SUBPIXELS_PER_PX / 4;
        let ink = 20 * SUBPIXELS_PER_PX;
        let box_height = ink + 2 * padding;
        for band_height in [box_height, 3 * cell_height().get()] {
            let top = centered_content_offset(band_height, box_height, padding);
            let bottom = band_height.saturating_sub(top).saturating_sub(ink);
            assert!(
                top.abs_diff(bottom) <= 1,
                "band={band_height} top={top} bottom={bottom}"
            );
        }
    }

    #[test]
    fn offscreen_live_band_fallback_uses_the_free_height_prefix() {
        let cell = cell_height().get();
        let prefix = [
            0,
            cell,
            2 * cell + 32 * SUBPIXELS_PER_PX,
            3 * cell + 32 * SUBPIXELS_PER_PX,
        ];
        let live_base = 5;
        let live_row_two = continuous_row_top_subpixels(live_base + 2, live_base, &prefix, cell);
        let live_origin = continuous_row_top_subpixels(live_base, live_base, &prefix, cell);
        assert_eq!(
            live_row_two.saturating_sub(live_origin),
            2 * cell + 32 * SUBPIXELS_PER_PX
        );
        assert_ne!(
            live_row_two.saturating_sub(live_origin),
            2 * cell,
            "rows*cell_height must not survive as the offscreen placement fallback"
        );
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
    fn rendered_live_blocks_own_exactly_their_borrowed_band() {
        for (band_start_row, band_end_row) in [(2, 3), (3, 4)] {
            let mut projection = ViewportProjection::new(
                key(8),
                DetectionRevision(1),
                nz32(12),
                cell_height(),
                SourceGeneration(1),
                GridGeneration(1),
            );
            projection.sync_live_math_artifacts(
                ScreenId::Primary,
                [ProjectedLiveMathArtifact {
                    occurrence_id: LiveMathOccurrenceId(1),
                    screen: ScreenId::Primary,
                    start: GridPoint { row: 3, column: 0 },
                    end: GridPoint { row: 3, column: 4 },
                    band_start_row,
                    band_end_row,
                    clipped_top_rows: 0,
                    clipped_bottom_rows: 0,
                    occluded_source_rows: 0,
                    occluded_visible_rows: Vec::new(),
                    generation: GridGeneration(1),
                    artifact: ProjectedMathArtifact {
                        key: format!("display-x-{band_start_row}-{band_end_row}"),
                        end: TranscriptId(0),
                        rgba: Arc::from(vec![255; 50 * 4]),
                        width_px: 1,
                        height_px: 50,
                        height_subpixels: 50 * SUBPIXELS_PER_PX,
                        baseline_subpixels: 0,
                        mode: MathMode::Display,
                        vertical_padding_subpixels: 0,
                        render_scale_milli: 1000,
                        source: "x".to_owned(),
                    },
                }],
            );
            let frame = projection
                .continuous_frame(
                    &HistoryDocument::default(),
                    &[],
                    vec![CapturedRow::plain("        ", false); 12],
                    GridCursor {
                        row: 11,
                        column: 0,
                        visible: true,
                    },
                    ScreenId::Primary,
                )
                .unwrap();
            let block = &frame.math_blocks[0];
            let band_top = frame
                .row_map
                .iter()
                .find(|row| row.live_grid_row == Some(band_start_row))
                .unwrap()
                .top_subpixels;
            let band_end = frame
                .row_map
                .iter()
                .find(|row| row.live_grid_row == Some(band_end_row))
                .unwrap();
            let band_bottom = band_end
                .top_subpixels
                .saturating_add(band_end.height_subpixels);
            let block_extent = (
                block.top_subpixels,
                block
                    .top_subpixels
                    .saturating_add(block.clip_height_subpixels),
            );
            assert_eq!(block.top_subpixels, band_top);
            assert!(block_extent.1 <= band_bottom);
            for row in frame.row_map.iter().filter(|row| {
                row.live_grid_row
                    .is_some_and(|live| !(band_start_row..=band_end_row).contains(&live))
            }) {
                let row_extent = (
                    row.top_subpixels,
                    row.top_subpixels.saturating_add(row.height_subpixels),
                );
                assert!(
                    block_extent.1 <= row_extent.0 || row_extent.1 <= block_extent.0,
                    "block {block_extent:?} overlaps outside row {:?} {row_extent:?}",
                    row.live_grid_row
                );
            }
            frame.validate_shape().unwrap();
            if band_start_row < 3 {
                assert_ne!(band_start_row, 3, "upward borrowing must be covered");
            } else {
                assert_ne!(band_end_row, 3, "downward borrowing must be covered");
            }
        }
    }

    #[test]
    fn live_height_accounting_filters_the_same_screen_and_generation_as_placement() {
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(12),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(7),
        );
        let artifact = |screen, generation, key: &str| ProjectedLiveMathArtifact {
            occurrence_id: LiveMathOccurrenceId(1),
            screen,
            start: GridPoint { row: 3, column: 0 },
            end: GridPoint { row: 3, column: 4 },
            band_start_row: 2,
            band_end_row: 3,
            clipped_top_rows: 0,
            clipped_bottom_rows: 0,
            occluded_source_rows: 0,
            occluded_visible_rows: Vec::new(),
            generation,
            artifact: ProjectedMathArtifact {
                key: key.to_owned(),
                end: TranscriptId(0),
                rgba: Arc::from(vec![255; 50 * 4]),
                width_px: 1,
                height_px: 50,
                height_subpixels: 50 * SUBPIXELS_PER_PX,
                baseline_subpixels: 0,
                mode: MathMode::Display,
                vertical_padding_subpixels: 0,
                render_scale_milli: 1000,
                source: "x".to_owned(),
            },
        };
        projection.sync_live_math_artifacts(
            ScreenId::Primary,
            [
                artifact(ScreenId::Alternate, GridGeneration(7), "wrong-screen"),
                artifact(ScreenId::Primary, GridGeneration(6), "stale-generation"),
            ],
        );
        assert!(projection.live_math_artifacts.is_empty());
        assert!(
            projection
                .live_row_prefix
                .windows(2)
                .all(|rows| { rows[1].saturating_sub(rows[0]) == cell_height().get() })
        );
    }

    #[test]
    fn alternate_clipped_top_extent_is_fully_reviewable_without_negative_content_offset() {
        let cell = cell_height().get();
        let padding = cell / 4;
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(6),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let artifact = |occurrence_id,
                        band_start_row,
                        band_end_row,
                        clipped_top_rows,
                        box_cells,
                        key: &str| {
            let box_height = i64::from(box_cells).saturating_mul(cell);
            let tight_height = box_height.saturating_sub(2 * padding);
            let height_px = u32::try_from(tight_height / SUBPIXELS_PER_PX).unwrap();
            ProjectedLiveMathArtifact {
                occurrence_id: LiveMathOccurrenceId(occurrence_id),
                screen: ScreenId::Alternate,
                start: GridPoint {
                    row: band_start_row,
                    column: 0,
                },
                end: GridPoint {
                    row: band_end_row,
                    column: 4,
                },
                band_start_row,
                band_end_row,
                clipped_top_rows,
                clipped_bottom_rows: 0,
                occluded_source_rows: 0,
                occluded_visible_rows: Vec::new(),
                generation: GridGeneration(1),
                artifact: ProjectedMathArtifact {
                    key: key.to_owned(),
                    end: TranscriptId(0),
                    rgba: Arc::from(vec![255; height_px as usize * 4]),
                    width_px: 1,
                    height_px,
                    height_subpixels: box_height,
                    baseline_subpixels: 0,
                    mode: MathMode::Display,
                    vertical_padding_subpixels: padding,
                    render_scale_milli: 1000,
                    source: key.to_owned(),
                },
            }
        };
        projection.sync_live_math_artifacts(
            ScreenId::Alternate,
            [
                artifact(1, 0, 1, 1, 4, "clipped-top"),
                artifact(2, 3, 3, 0, 3, "lower-expansion"),
            ],
        );

        let live = || vec![CapturedRow::plain("        ", false); 6];
        let cursor = GridCursor {
            row: 5,
            column: 0,
            visible: true,
        };
        let bottom = projection
            .continuous_frame(
                &HistoryDocument::default(),
                &[],
                live(),
                cursor,
                ScreenId::Alternate,
            )
            .unwrap();
        let last = bottom.row_map.last().unwrap();
        assert_eq!(
            last.top_subpixels.saturating_add(last.height_subpixels),
            6 * cell,
            "the extra reveal extent must still be consumed above the fixed bottom row"
        );
        assert_eq!(projection.debug_scroll_extent().2, 4);
        assert_eq!(
            bottom.status_text.as_deref(),
            Some("4 rows above · Shift+wheel")
        );

        projection.scroll_by_rows(99);
        let top = projection
            .continuous_frame(
                &HistoryDocument::default(),
                &[],
                live(),
                cursor,
                ScreenId::Alternate,
            )
            .unwrap();
        assert_eq!(projection.scroll_offset_rows(), 4);
        let top_block = top
            .math_blocks
            .iter()
            .find(|block| block.source == "clipped-top")
            .unwrap();
        assert_eq!(top_block.top_subpixels, 0);
        assert_eq!(top_block.content_offset_subpixels, padding);
        assert_eq!(top_block.clip_height_subpixels, 4 * cell);
        let raster_top = top_block
            .top_subpixels
            .saturating_add(top_block.content_offset_subpixels);
        let raster_bottom = raster_top.saturating_add(
            i64::from(top_block.artifact.height_px).saturating_mul(SUBPIXELS_PER_PX),
        );
        assert!(raster_top >= 0);
        assert!(
            raster_bottom
                <= top_block
                    .top_subpixels
                    .saturating_add(top_block.clip_height_subpixels)
        );
        // Mutations that omit the clipped-top slice cap the allowance at three rows; retaining
        // `centered - hidden_top` makes the final content offset negative.
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
    fn frame_validation_binds_every_selection_span_to_its_row_map_interval() {
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
        frame.selection_spans.push(SelectionSpan {
            row: 1,
            start_column: 0,
            end_column: 2,
        });
        let interval = frame
            .selection_span_vertical_interval(&frame.selection_spans[0])
            .unwrap();
        assert_eq!(
            interval,
            frame.row_map[1].top_subpixels
                ..frame.row_map[1]
                    .top_subpixels
                    .saturating_add(frame.row_map[1].height_subpixels)
        );
        frame.validate_shape().unwrap();

        frame.selection_spans[0].row = 2;
        assert_eq!(
            frame.validate_shape(),
            Err(FrameShapeError::SelectionSpanRowOutOfBounds { row: 2, rows: 2 })
        );

        frame.selection_spans[0].row = 1;
        frame.row_map[1].height_subpixels = 0;
        assert_eq!(
            frame.validate_shape(),
            Err(FrameShapeError::SelectionSpanInvalidInterval {
                row: 1,
                top: cell_height().get(),
                height: 0,
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
    fn math_block_replaces_a_multi_line_span_in_two_projections_at_free_pixel_height() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(8).unwrap());
        let mut document = HistoryDocument::default();
        let mut ids = Vec::new();
        for text in ["$$", "x^2 + y^2", "$$"] {
            let finalized = store
                .capture(CapturedRow::plain(text, false))
                .finalized
                .remove(0);
            ids.push(finalized.line.id);
            document.finalize_transaction(finalized);
        }
        let artifact = ProjectedMathArtifact {
            key: "math:test".to_owned(),
            end: ids[2],
            rgba: Arc::from(vec![255; 4]),
            width_px: 1,
            height_px: 1,
            height_subpixels: 35 * SUBPIXELS_PER_PX,
            baseline_subpixels: 0,
            mode: MathMode::Display,
            vertical_padding_subpixels: 0,
            render_scale_milli: 1000,
            source: "x^2 + y^2".to_owned(),
        };
        let make_projection = |width| {
            let mut projection = ViewportProjection::new(
                key(width),
                DetectionRevision(1),
                nz32(4),
                cell_height(),
                store.source_generation(),
                GridGeneration(1),
            );
            projection.sync_math_artifacts([(ids[0], artifact.clone())]);
            projection.project(&document);
            projection
        };
        let mut narrow = make_projection(4);
        let wide = make_projection(20);
        assert_eq!(narrow.heights().get(0), Some(35 * SUBPIXELS_PER_PX));
        assert_eq!(wide.heights().get(0), Some(35 * SUBPIXELS_PER_PX));

        let middle = &document.entries()[&ids[1]].line;
        let middle_anchor = ContentAnchor::History {
            id: ids[1],
            offset: GraphemeOffset(3),
            bias: Bias::Before,
            generation: middle.source_generation,
        };
        assert_eq!(narrow.anchor_y(&document, &middle_anchor), Ok(0));
        assert_eq!(wide.anchor_y(&document, &middle_anchor), Ok(0));

        let live = || vec![CapturedRow::plain("    ", false); 4];
        narrow
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: false,
                },
                ScreenId::Primary,
            )
            .unwrap();
        narrow.scroll_to_top();
        let frame = narrow
            .continuous_frame(
                &document,
                &[],
                live(),
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: false,
                },
                ScreenId::Primary,
            )
            .unwrap();
        assert_eq!(frame.math_blocks.len(), 1);
        assert_eq!(frame.math_blocks[0].artifact.end, ids[2]);
        assert!(frame.cells.iter().all(|cell| cell.text.trim().is_empty()));
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
    #[test]
    fn frozen_wide_glyph_spacer_keeps_the_glyph_background() {
        // A background bar behind CJK text (Codex prompt echo) must stay continuous after the
        // line freezes into history: the wide glyph's spacer column carries the same style.
        let mut store = TranscriptStore::new(NonZeroUsize::new(8).unwrap());
        let mut wide = CapturedCell::plain("请");
        wide.style.flags.insert(CellFlags::WIDE_CHAR);
        wide.style.background = bt_transcript::TerminalColor::Rgb(41, 41, 41);
        let mut live_spacer = CapturedCell {
            wide_spacer: true,
            ..CapturedCell::default()
        };
        live_spacer.style.background = bt_transcript::TerminalColor::Rgb(41, 41, 41);
        let result = store.capture(CapturedRow {
            cells: vec![wide, live_spacer],
            continues: false,
            shell_mark: None,
        });
        let frozen = &result.finalized[0].line;
        let rows = layout_frozen_line(frozen, 4);
        let spacer = &rows[0].cells[1];
        assert!(spacer.wide_spacer);
        assert_eq!(
            spacer.style.background,
            bt_transcript::TerminalColor::Rgb(41, 41, 41),
            "the spacer column must keep its glyph's background"
        );
        assert!(!spacer.style.flags.contains(CellFlags::WIDE_CHAR));
    }
}
