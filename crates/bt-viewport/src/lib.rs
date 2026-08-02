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
    CapturedCell, CapturedRow, CellFlags, CellHyperlink, FrozenLine, GraphemeOffset,
    HyperlinkRange, SourceGeneration, StagedRow, StagingId, TranscriptId, detect_http_urls,
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
/// Phase-A frame contract: one complete bottom row is carried beyond the PTY grid.
pub const FRAME_OVERSCAN_ROWS: u32 = 1;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HyperlinkHit {
    pub uri: String,
    /// OSC 8 grouping id. Present: the link is every frame cell carrying the same (id, uri) —
    /// soft-wrapped segments separated by layout indent stay one link. Absent (implicitly
    /// detected bare URL): the link is the contiguous same-uri cell run.
    pub id: Option<String>,
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
    pub kind: RgbaArtifactKind,
    /// Symmetric presentation breathing outside the alpha-tight texture. This is lifecycle-scale
    /// geometry, not part of the shared RGBA artifact.
    pub vertical_padding_subpixels: i64,
    /// Presentation scale for a same-source stale raster. Fresh artifacts use 1000.
    pub render_scale_milli: u32,
    pub source: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RgbaArtifactKind {
    Math,
    InlineImage { animated: bool },
    LocalImagePath { animated: bool },
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
    /// The artifact is the exact occurrence's previous-layout raster while a replacement relayout
    /// is pending. Same-DPI width changes can therefore be stale even when render scale is 1000.
    pub transition_stale: bool,
    /// Frozen transcript rows (opener/body) already committed to scrollback while the closer is
    /// still in the live grid. Empty for an ordinary all-live block. Ordered top to bottom and
    /// immediately preceding the live band; projection renders the whole occurrence as one block
    /// bridging the history and live domains.
    pub frozen_prefix: Vec<TranscriptId>,
    /// Proven source rows between `frozen_prefix` and the live band which are still owned by the
    /// transcript staging plane. Exact staging ids keep the bridge causal while rows finalize.
    pub staging_prefix: Vec<StagingId>,
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
    /// Number of transcript-prefix visual rows this block owns above its live band, including rows
    /// which are still in staging. A boundary-split block bridges both domains, so its top sits
    /// above the live band start; zero for an ordinary block.
    pub frozen_prefix_rows: u32,
    /// Projection-layer clip provenance: logical band rows above / below the exactly-matched grid
    /// slice. These are the counts the band-height budget consumes; carrying them on the placement
    /// lets frame-level auditing distinguish a genuine edge clip from a reprojection transient
    /// without re-deriving the projection. Zero for a wholly-matched block.
    pub clipped_top_rows: u32,
    pub clipped_bottom_rows: u32,
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
///
/// `grid_rows` is the PTY grid height. `rows` is the rectangular presentation height and includes
/// any bottom overscan rows. Every row-indexed payload (`cells`, `cell_anchors`, `row_map`,
/// `selection_spans`, and `cursor`) uses presentation-row indices. The projection constructs those
/// payloads from one ordered presentation-row list; consumers must not rebuild a second row list
/// from the PTY grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewportFrame {
    pub columns: NonZeroU32,
    pub grid_rows: NonZeroU32,
    /// Number of rectangular presentation rows, including bottom overscan.
    pub rows: NonZeroU32,
    /// Exact intra-row presentation offset. The first row's signed top is its negation; a nonzero
    /// value exposes the bottom overscan suffix while preserving one rectangular row payload.
    pub presentation_offset_subpixels: i64,
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
    /// Presentation rows that may contribute pixels or input hits for this frame. An aligned frame
    /// keeps the bottom overscan suffix hidden; an exact offset makes the complete presentation
    /// list drawable while the pane clip limits actual coverage.
    pub fn drawable_rows(&self) -> usize {
        if self.presentation_offset_subpixels == 0 {
            self.grid_rows.get() as usize
        } else {
            self.rows.get() as usize
        }
    }

    pub fn drawable_interval_overlaps(&self, top_subpixels: i64, height_subpixels: i64) -> bool {
        let bottom_subpixels = top_subpixels.saturating_add(height_subpixels);
        bottom_subpixels > top_subpixels
            && self.row_map.iter().take(self.drawable_rows()).any(|row| {
                let row_bottom = row.top_subpixels.saturating_add(row.height_subpixels);
                top_subpixels < row_bottom && row.top_subpixels < bottom_subpixels
            })
    }

    pub fn validate_shape(&self) -> Result<(), FrameShapeError> {
        if self.layout_key.width_cells != self.columns {
            return Err(FrameShapeError::LayoutWidth {
                frame: self.columns.get(),
                layout: self.layout_key.width_cells.get(),
            });
        }
        if self.rows < self.grid_rows {
            return Err(FrameShapeError::GridRowsBeyondPresentation {
                grid_rows: self.grid_rows.get(),
                presentation_rows: self.rows.get(),
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
            // A boundary-split block bridges the frozen history rows above its live band, so its
            // top deliberately sits above the live band start rather than aligning to it. The other
            // band invariants (order, bottom, non-overlap of live rows outside the band) still hold.
            if block.frozen_prefix_rows == 0
                && !matches!(block.artifact.kind, RgbaArtifactKind::LocalImagePath { .. })
                && let Some(band_start) = self
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
        if y_subpixels < 0 {
            return None;
        }
        self.row_map
            .iter()
            // The aligned contract keeps overscan non-hit-testable. With an exact offset the same
            // frame-owned row map exposes any overscan pixels that entered the pane.
            .take(self.drawable_rows())
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

    /// Return the complete contiguous link span under one grid cell. Explicit OSC 8 and inferred
    /// HTTP(S) links share this path, so hover and click resolve through the same frame anchors as
    /// selection.
    pub fn hyperlink_at(&self, row: u32, column: u32) -> Option<HyperlinkHit> {
        if row >= self.rows.get() || column >= self.columns.get() {
            return None;
        }
        let index = row as usize * self.columns.get() as usize + column as usize;
        let link = self.cells.get(index)?.hyperlink.as_ref()?;
        if link.id.is_some() {
            // OSC 8 with a grouping id (the vendor synthesizes one per emission when the
            // application omits it): the link is every cell carrying the same (id, uri), even
            // across layout-indent gaps between soft-wrapped segments.
            let first = self
                .cells
                .iter()
                .position(|cell| cell_in_link_group(cell, link))?;
            let last = self
                .cells
                .iter()
                .rposition(|cell| cell_in_link_group(cell, link))?;
            return Some(HyperlinkHit {
                uri: link.uri.clone(),
                id: link.id.clone(),
                start: self.cell_anchors.get(first)?.start.clone(),
                end: self.cell_anchors.get(last)?.end.clone(),
            });
        }
        let same_uri = |cell: &bt_transcript::CapturedCell| {
            cell.hyperlink
                .as_ref()
                .is_some_and(|other| other.uri == link.uri)
        };
        let mut first = index;
        while first > 0 && same_uri(&self.cells[first - 1]) {
            first -= 1;
        }
        let mut last = index + 1;
        while last < self.cells.len() && same_uri(&self.cells[last]) {
            last += 1;
        }
        Some(HyperlinkHit {
            uri: link.uri.clone(),
            id: None,
            start: self.cell_anchors.get(first)?.start.clone(),
            end: self.cell_anchors.get(last - 1)?.end.clone(),
        })
    }

    /// Add the ordinary terminal underline flag to the active link in this frame only. Source
    /// cells and transcript styles remain unchanged.
    pub fn underline_hyperlink(&mut self, hyperlink: &HyperlinkHit) -> bool {
        if hyperlink.id.is_some() {
            let group = bt_transcript::CellHyperlink {
                id: hyperlink.id.clone(),
                uri: hyperlink.uri.clone(),
            };
            let Some(first) = self
                .cells
                .iter()
                .position(|cell| cell_in_link_group(cell, &group))
            else {
                return false;
            };
            let Some(last) = self
                .cells
                .iter()
                .rposition(|cell| cell_in_link_group(cell, &group))
            else {
                return false;
            };
            // The hit must describe this frame's group, not a stale frame's.
            if self.cell_anchors[first].start != hyperlink.start
                || self.cell_anchors[last].end != hyperlink.end
            {
                return false;
            }
            for index in first..=last {
                if cell_in_link_group(&self.cells[index], &group) {
                    let flags = &mut self.cells[index].style.flags;
                    flags.remove(CellFlags::DOTTED_UNDERLINE);
                    flags.insert(CellFlags::UNDERLINE);
                }
            }
            return true;
        }
        let same_uri = |cell: &bt_transcript::CapturedCell| {
            cell.hyperlink
                .as_ref()
                .is_some_and(|other| other.uri == hyperlink.uri)
        };
        let Some(index) = self.cells.iter().enumerate().find_map(|(index, cell)| {
            (same_uri(cell) && self.cell_anchors[index].start == hyperlink.start).then_some(index)
        }) else {
            return false;
        };
        let mut first = index;
        while first > 0 && same_uri(&self.cells[first - 1]) {
            first -= 1;
        }
        let mut last = index + 1;
        while last < self.cells.len() && same_uri(&self.cells[last]) {
            last += 1;
        }
        if self.cell_anchors[last - 1].end != hyperlink.end {
            return false;
        }
        for cell in &mut self.cells[first..last] {
            cell.style.flags.remove(CellFlags::DOTTED_UNDERLINE);
            cell.style.flags.insert(CellFlags::UNDERLINE);
        }
        true
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
    LayoutWidth {
        frame: u32,
        layout: u32,
    },
    GridRowsBeyondPresentation {
        grid_rows: u32,
        presentation_rows: u32,
    },
    CellCount {
        expected: usize,
        actual: usize,
    },
    AnchorCount {
        expected: usize,
        actual: usize,
    },
    RowMapCount {
        expected: usize,
        actual: usize,
    },
    SelectionSpanRowOutOfBounds {
        row: u32,
        rows: usize,
    },
    SelectionSpanInvalidInterval {
        row: u32,
        top: i64,
        height: i64,
    },
    MathBlockBandOrder {
        start: u32,
        end: u32,
    },
    MathBlockBandTop {
        expected: i64,
        actual: i64,
    },
    MathBlockBeyondBand {
        band_bottom: i64,
        block_bottom: i64,
    },
    MathBlockOverlapsOutsideRow {
        row: u32,
    },
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
            Self::GridRowsBeyondPresentation {
                grid_rows,
                presentation_rows,
            } => write!(
                formatter,
                "PTY grid has {grid_rows} rows but the frame carries only {presentation_rows} presentation rows"
            ),
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

/// The single projection-time source of truth for one rectangular presentation row. Flattened
/// cells/anchors and `FrameVisualRow` geometry are all derived from this same ordered list.
#[derive(Clone, Debug)]
struct PresentedRow {
    visual: VisualRow,
    height_subpixels: i64,
    live_grid_row: Option<u32>,
}

struct FlattenedPresentedRows {
    cells: Vec<CapturedCell>,
    cell_anchors: Vec<CellAnchor>,
    row_map: Vec<FrameVisualRow>,
}

fn presented_height(rows: &[PresentedRow]) -> i64 {
    rows.iter()
        .map(|row| row.height_subpixels)
        .fold(0_i64, i64::saturating_add)
}

/// Consume the projection's ordered row list into the three frame payloads in one pass.
///
/// `VisualRow` already owns the final cells and anchors. Moving those elements into the frame
/// avoids cloning every `CapturedCell` (including its strings) and every pair of content anchors
/// merely to discard the per-row vectors immediately afterwards.
fn flatten_presented_rows(
    rows: Vec<PresentedRow>,
    columns: usize,
    first_top_subpixels: i64,
) -> Result<FlattenedPresentedRows, FrameProjectionError> {
    let row_count = rows.len();
    let cell_count = row_count.saturating_mul(columns);
    let mut cells = Vec::with_capacity(cell_count);
    let mut cell_anchors = Vec::with_capacity(cell_count);
    let mut row_map = Vec::with_capacity(row_count);
    let mut next_top = first_top_subpixels;

    for (row_index, row) in rows.into_iter().enumerate() {
        validate_visual_row(&row.visual, columns, "presentation", row_index)?;
        row_map.push(FrameVisualRow {
            top_subpixels: next_top,
            height_subpixels: row.height_subpixels,
            live_grid_row: row.live_grid_row,
        });
        next_top = next_top.saturating_add(row.height_subpixels);
        cells.extend(row.visual.cells);
        cell_anchors.extend(row.visual.anchors);
    }

    Ok(FlattenedPresentedRows {
        cells,
        cell_anchors,
        row_map,
    })
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
    PresentationRowCountOverflow {
        grid_rows: u32,
        overscan_rows: u32,
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
            Self::PresentationRowCountOverflow {
                grid_rows,
                overscan_rows,
            } => write!(
                formatter,
                "grid height {grid_rows} plus {overscan_rows} overscan rows exceeds the frame row limit"
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
    inline_path_artifacts: HashMap<TranscriptId, Vec<ProjectedMathArtifact>>,
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
    scroll_offset_subpixels: i64,
    pending_scroll_offset_subpixels: Option<i64>,
    /// A review offset preserved across an application transcript rewrite. Codex-style TUIs
    /// reflow by clearing scrollback (ED3) and reprinting equivalent content; the anchored row
    /// the user was reading dies with the clear, but their review displacement is still
    /// meaningful, so it is re-established by row count as history refills instead of snapping
    /// the view to the bottom. Any explicit scroll action supersedes it.
    displaced_review_subpixels: Option<i64>,
    /// Whether a resize transaction is currently open, pushed in by the session each frame. It is
    /// the deterministic signal that a vanished review anchor is a resize-driven reflow (Codex
    /// clears scrollback then reprints) rather than a user-initiated clear, which gates the
    /// presentation-layer frame hold below.
    resize_reflow_active: bool,
    /// True while the preserved `displaced_review_subpixels` was created by a resize reflow and has not
    /// yet been re-anchored: during this window history is transiently empty, so the projection can
    /// only fall to the live bottom. Presentation reads this to hold the last frame instead of
    /// flashing to the bottom. It clears deterministically — when the displacement re-anchors, or
    /// when any explicit scroll/input supersedes the displacement — never on a timer.
    review_hold: bool,
    /// Session-owned exact-source preservation is waiting off-band for a post-zoom primary reprint
    /// to restore the source it can prove. The off-band raster cannot be painted at an unproven
    /// position, so presentation holds its previous complete frame until the session re-anchors or
    /// deterministically retires that record.
    exact_source_reprint_hold: bool,
    live_overflow_offset_subpixels: i64,
    last_live_overflow_subpixels: i64,
    unread_rows: usize,
    last_total_rows: usize,
    last_total_height_subpixels: i64,
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
            inline_path_artifacts: HashMap::new(),
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
            scroll_offset_subpixels: 0,
            pending_scroll_offset_subpixels: None,
            displaced_review_subpixels: None,
            resize_reflow_active: false,
            review_hold: false,
            exact_source_reprint_hold: false,
            live_overflow_offset_subpixels: 0,
            last_live_overflow_subpixels: 0,
            unread_rows: 0,
            last_total_rows: 0,
            last_total_height_subpixels: 0,
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
        usize::try_from(
            self.pending_scroll_offset_subpixels
                .unwrap_or(self.scroll_offset_subpixels)
                .max(0)
                .div_euclid(self.cell_height_subpixels.get()),
        )
        .unwrap_or(usize::MAX)
    }

    pub fn scroll_offset_subpixels(&self) -> i64 {
        self.pending_scroll_offset_subpixels
            .unwrap_or(self.scroll_offset_subpixels)
    }

    pub fn unread_rows(&self) -> usize {
        self.unread_rows
    }

    /// Diagnostic snapshot of the scroll-extent bookkeeping: the last projected total row count,
    /// the history offset, the live-overflow allowance, and how much of it is consumed.
    pub fn debug_scroll_extent(&self) -> (usize, usize, usize, usize, usize) {
        (
            self.last_total_rows,
            self.scroll_offset_rows(),
            usize::try_from(
                self.last_live_overflow_subpixels
                    .max(0)
                    .div_euclid(self.cell_height_subpixels.get()),
            )
            .unwrap_or(usize::MAX),
            usize::try_from(
                self.live_overflow_offset_subpixels
                    .max(0)
                    .div_euclid(self.cell_height_subpixels.get()),
            )
            .unwrap_or(usize::MAX),
            self.unread_rows,
        )
    }

    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset_subpixels() != 0
    }

    /// Tell the projection whether a resize transaction is currently open. The session pushes this
    /// each frame; it gates the frame hold so a user-initiated clear (not a resize) never holds.
    pub fn set_resize_reflow_active(&mut self, active: bool) {
        self.resize_reflow_active = active;
    }

    /// True while a resize-driven transcript rewrite has displaced the review anchor and history
    /// has not yet refilled enough to re-anchor it. Presentation holds the last frame during this
    /// window rather than flashing the view to the live bottom. Cleared deterministically once the
    /// displacement re-anchors or an explicit scroll/input supersedes it.
    pub fn review_hold(&self) -> bool {
        self.review_hold
    }

    /// Push the session's exact-source decoration hold into projection state. This is deliberately
    /// a fact supplied by the session rather than inferred from cells: only the decoration owner
    /// knows that an unmatched off-band record is a stale-pending DPI transition.
    pub fn set_exact_source_reprint_hold(&mut self, active: bool) {
        self.exact_source_reprint_hold = active;
    }

    /// Whether presentation must keep the last complete frame. The two reasons are independent:
    /// review displacement holds a vanished scroll anchor, while exact-source reprint hold covers a
    /// decoration that cannot legally paint until its proven source reappears.
    pub fn presentation_hold(&self) -> bool {
        self.review_hold || self.exact_source_reprint_hold
    }

    /// Diagnostic split for the app's env-gated publication trace. Product gating should continue
    /// to use `presentation_hold`, which composes this with resize-review displacement.
    pub fn exact_source_reprint_hold(&self) -> bool {
        self.exact_source_reprint_hold
    }

    pub fn cell_height_subpixels(&self) -> NonZeroI64 {
        self.cell_height_subpixels
    }

    /// Track the authoritative cell height. A zoom / DPI change remeasures the font, which changes
    /// the pixel height of every row while leaving row and scroll-offset semantics untouched. The
    /// projection caches subpixel geometry (`live_row_prefix`, math band tops) keyed on this height,
    /// so it must be re-derived at the new value; forcing a full reproject rebuilds them (the live
    /// prefix in `sync_live_math_artifacts`, the history layout in `project`). No scroll state is
    /// disturbed — a reviewer keeps the same anchored row, now measured at the new cell height.
    pub fn set_cell_height_subpixels(&mut self, cell_height_subpixels: NonZeroI64) {
        if self.cell_height_subpixels != cell_height_subpixels {
            self.cell_height_subpixels = cell_height_subpixels;
            self.projection_dirty = true;
        }
    }

    /// Positive rows move into history; negative rows move toward the live bottom.
    pub fn scroll_by_rows(&mut self, rows: i32) {
        self.scroll_by_subpixels(i64::from(rows).saturating_mul(self.cell_height_subpixels.get()));
    }

    /// Positive subpixels move into history; negative subpixels move toward the live bottom.
    pub fn scroll_by_subpixels(&mut self, subpixels: i64) {
        // An explicit scroll is the user taking over: any preserved review displacement from an
        // application transcript rewrite is superseded.
        self.displaced_review_subpixels = None;
        self.review_hold = false;
        let pane_height =
            i64::from(self.live_rows.get()).saturating_mul(self.cell_height_subpixels.get());
        let max = self
            .last_total_height_subpixels
            .saturating_sub(pane_height)
            .max(0);
        let offset = self
            .pending_scroll_offset_subpixels
            .unwrap_or(self.scroll_offset_subpixels)
            .saturating_add(subpixels)
            .clamp(0, max);
        self.pending_scroll_offset_subpixels = Some(offset);
        self.live_overflow_offset_subpixels = offset.min(self.last_live_overflow_subpixels);
        if offset == 0 {
            self.scroll_state = ViewportScrollState::Bottom;
            self.scroll_offset_subpixels = 0;
            self.unread_rows = 0;
        }
        self.view_generation.0 += 1;
    }

    pub fn scroll_to_top(&mut self) {
        self.displaced_review_subpixels = None;
        let pane_height =
            i64::from(self.live_rows.get()).saturating_mul(self.cell_height_subpixels.get());
        let offset = self
            .last_total_height_subpixels
            .saturating_sub(pane_height)
            .max(0);
        self.live_overflow_offset_subpixels = self.last_live_overflow_subpixels;
        self.pending_scroll_offset_subpixels = Some(offset);
        if offset == 0 {
            self.scroll_state = ViewportScrollState::Bottom;
        }
        self.view_generation.0 += 1;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.displaced_review_subpixels = None;
        if self.is_scrolled()
            || !matches!(self.scroll_state, ViewportScrollState::Bottom)
            || self.unread_rows != 0
        {
            self.scroll_state = ViewportScrollState::Bottom;
            self.scroll_offset_subpixels = 0;
            self.pending_scroll_offset_subpixels = None;
            self.live_overflow_offset_subpixels = 0;
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
        let presentation_row_count = self
            .live_rows
            .get()
            .checked_add(FRAME_OVERSCAN_ROWS)
            .and_then(NonZeroU32::new)
            .ok_or(FrameProjectionError::PresentationRowCountOverflow {
                grid_rows: self.live_rows.get(),
                overscan_rows: FRAME_OVERSCAN_ROWS,
            })?;
        let presentation_rows = presentation_row_count.get() as usize;
        let mut presented = Vec::with_capacity(presentation_rows);
        for (row_index, row) in rows.into_iter().enumerate() {
            if row.cells.len() != expected_columns {
                return Err(FrameProjectionError::ColumnCount {
                    row: row_index,
                    expected: expected_columns,
                    actual: row.cells.len(),
                });
            }
            let visual =
                captured_visual_row(row, expected_columns, |column, bias| ContentAnchor::Live {
                    screen: ScreenId::Primary,
                    point: GridPoint {
                        row: row_index as u32,
                        column: column as u32,
                    },
                    bias,
                    generation: self.grid_generation,
                });
            presented.push(PresentedRow {
                visual,
                height_subpixels: self.cell_height_subpixels.get(),
                live_grid_row: Some(row_index as u32),
            });
        }
        let last_grid_row = self.live_rows.get().saturating_sub(1);
        presented.push(PresentedRow {
            visual: blank_visual_row(expected_columns, |column, bias| ContentAnchor::Live {
                screen: ScreenId::Primary,
                point: GridPoint {
                    row: last_grid_row,
                    column: column as u32,
                },
                bias,
                generation: self.grid_generation,
            }),
            height_subpixels: self.cell_height_subpixels.get(),
            live_grid_row: None,
        });
        let FlattenedPresentedRows {
            cells,
            cell_anchors,
            row_map,
        } = flatten_presented_rows(presented, expected_columns, 0)?;
        let frame = ViewportFrame {
            columns,
            grid_rows: self.live_rows,
            rows: presentation_row_count,
            presentation_offset_subpixels: 0,
            cells,
            cursor,
            cell_anchors,
            row_map,
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
        let presentation_row_count = self
            .live_rows
            .get()
            .checked_add(FRAME_OVERSCAN_ROWS)
            .and_then(NonZeroU32::new)
            .ok_or(FrameProjectionError::PresentationRowCountOverflow {
                grid_rows: self.live_rows.get(),
                overscan_rows: FRAME_OVERSCAN_ROWS,
            })?;
        let presentation_rows = presentation_row_count.get() as usize;
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
        self.last_live_overflow_subpixels = live_extra_height;
        self.live_overflow_offset_subpixels = self
            .live_overflow_offset_subpixels
            .min(self.last_live_overflow_subpixels);
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
        let history_height = if primary { self.heights.total() } else { 0 };
        let staging_height = i64::try_from(staging_rows)
            .unwrap_or(i64::MAX)
            .saturating_mul(self.cell_height_subpixels.get());
        let total_height = history_height
            .saturating_add(staging_height)
            .saturating_add(live_height);
        self.last_total_height_subpixels = total_height;
        let pane_height = rectangular_live_height;
        let bottom_top_subpixels = total_height.saturating_sub(pane_height).max(0);
        if !primary {
            self.scroll_state = ViewportScrollState::Bottom;
            if let Some(requested) = self.pending_scroll_offset_subpixels.take() {
                let offset = requested.clamp(0, bottom_top_subpixels);
                if offset != 0
                    && let Some(anchor) = self.scroll_anchor_at_absolute_subpixel(
                        document,
                        staged_rows,
                        history_rows,
                        bottom_top_subpixels.saturating_sub(offset),
                        screen,
                    )
                {
                    self.scroll_state = ViewportScrollState::Anchored(anchor);
                }
            }
            if self.live_overflow_offset_subpixels == 0 {
                self.unread_rows = 0;
            }
        } else if let Some(requested_offset) = self.pending_scroll_offset_subpixels.take() {
            let offset = requested_offset.clamp(0, bottom_top_subpixels);
            if offset == 0 {
                self.scroll_state = ViewportScrollState::Bottom;
            } else {
                self.scroll_state = self
                    .scroll_anchor_at_absolute_subpixel(
                        document,
                        staged_rows,
                        history_rows,
                        bottom_top_subpixels.saturating_sub(offset),
                        screen,
                    )
                    .map_or(ViewportScrollState::Bottom, ViewportScrollState::Anchored);
            }
        } else if let Some(displaced) = self.displaced_review_subpixels {
            // Re-establish a review displacement preserved across an application transcript
            // rewrite: as the reprint refills history the offset deepens frame by frame, and the
            // preservation completes once the full displacement (or the new maximum) is reachable.
            let offset = displaced.min(bottom_top_subpixels);
            if offset != 0
                && let Some(anchor) = self.scroll_anchor_at_absolute_subpixel(
                    document,
                    staged_rows,
                    history_rows,
                    bottom_top_subpixels.saturating_sub(offset),
                    screen,
                )
            {
                self.scroll_state = ViewportScrollState::Anchored(anchor);
                if offset == displaced {
                    self.displaced_review_subpixels = None;
                }
            }
        }

        let mut window_top_subpixels = bottom_top_subpixels;
        if let ViewportScrollState::Anchored(mut anchor) = self.scroll_state.clone() {
            anchor.source = document.resolve_anchor(&anchor.source);
            if let Some(anchor_y) = self.absolute_subpixel_for_anchor(
                document,
                staged_rows,
                history_height,
                &anchor.source,
                screen,
            ) {
                window_top_subpixels = anchor_y
                    .saturating_add(anchor.local_offset)
                    .clamp(0, bottom_top_subpixels);
                if window_top_subpixels < bottom_top_subpixels {
                    self.scroll_state = ViewportScrollState::Anchored(anchor);
                } else {
                    self.scroll_state = ViewportScrollState::Bottom;
                }
            } else {
                // The anchored content vanished under the reader — a Codex-style reflow clears
                // scrollback before reprinting equivalent content. Preserve the displacement so
                // the refilled history restores the reading position instead of jumping to the
                // bottom; a review already being restored keeps its original target.
                if self.scroll_offset_subpixels != 0 {
                    self.displaced_review_subpixels = Some(
                        self.displaced_review_subpixels
                            .map_or(self.scroll_offset_subpixels, |kept| {
                                kept.max(self.scroll_offset_subpixels)
                            }),
                    );
                }
                self.scroll_state = ViewportScrollState::Bottom;
            }
        }
        self.scroll_offset_subpixels = bottom_top_subpixels.saturating_sub(window_top_subpixels);
        self.live_overflow_offset_subpixels = self
            .scroll_offset_subpixels
            .min(self.last_live_overflow_subpixels);
        if matches!(self.scroll_state, ViewportScrollState::Bottom) {
            window_top_subpixels = bottom_top_subpixels;
            self.scroll_offset_subpixels = 0;
            self.live_overflow_offset_subpixels = 0;
            self.unread_rows = 0;
        }
        // A resize reflow that cleared the history under an anchored reviewer leaves the view at the
        // live bottom while the reprint refills. Signal presentation to hold the last frame across
        // that transient window instead of flashing to the bottom. This is intrinsically bounded by
        // the displacement state: it turns off the frame the displacement re-anchors (`else if`
        // branch above clears `displaced_review_subpixels`) or an explicit scroll/input supersedes it,
        // and it never engages for a user-initiated clear because no resize transaction is open.
        self.review_hold =
            primary && self.resize_reflow_active && self.displaced_review_subpixels.is_some();
        let bottom_identity = matches!(self.scroll_state, ViewportScrollState::Bottom);
        let live_plane_top_subpixels = history_height.saturating_add(staging_height);
        let (mut window_start, mut first_row_top_subpixels) = if bottom_identity {
            // Replays never scroll. Preserve their Phase-A row-model identity exactly: fractional
            // history artifact height must not make `total_height - pane_height` select the
            // preceding history row and leak a partial-first-row offset into a Bottom frame.
            (
                total_rows.saturating_sub(expected_rows),
                live_plane_top_subpixels,
            )
        } else {
            self.absolute_row_at_subpixel(
                document,
                staged_rows,
                history_rows,
                history_height,
                window_top_subpixels,
                screen,
            )
            .unwrap_or((0, 0))
        };
        if !bottom_identity && window_top_subpixels >= live_plane_top_subpixels {
            // A free-height live artifact can push more than one source row above the pane while
            // its raster still intersects the visible region. Keep the complete live row list so
            // band placement, cell suppression and the fixed last row share one geometry. Exact
            // local review changes only this list's pixel offset until history/staging is reached.
            window_start = history_rows.saturating_add(staging_rows);
            first_row_top_subpixels = live_plane_top_subpixels;
        }
        let presentation_offset_subpixels = if bottom_identity {
            0
        } else {
            window_top_subpixels.saturating_sub(first_row_top_subpixels)
        };
        let live_base = history_rows + staging_rows;
        let visible_window_end = (window_start + expected_rows).min(total_rows);
        let window_end = (window_start + presentation_rows).min(total_rows);
        // The live plane is always rectangular, but blank rows at its tail are presentation
        // capacity rather than unread content. If an anchored frame displaces only those rows,
        // reporting `N lines below` would claim hidden content even though every meaningful row
        // already fits in the viewport.
        let first_live_row_below = visible_window_end
            .saturating_sub(live_base)
            .min(live_rows.len());
        let blank_live_rows_below = live_rows[first_live_row_below..]
            .iter()
            .rev()
            .take_while(|row| captured_row_is_blank(row))
            .count();
        let content_rows_below = self
            .scroll_offset_rows()
            .saturating_sub(blank_live_rows_below);
        debug_assert!(primary || live_height_delta >= 0);
        let frame_top_subpixels = if bottom_identity {
            live_extra_height.saturating_neg()
        } else {
            presentation_offset_subpixels.saturating_neg()
        };
        let rows_above = usize::try_from(
            self.last_live_overflow_subpixels
                .saturating_sub(self.live_overflow_offset_subpixels)
                .saturating_add(self.cell_height_subpixels.get() - 1)
                .div_euclid(self.cell_height_subpixels.get()),
        )
        .unwrap_or(usize::MAX);
        let window_row_top_subpixels = first_row_top_subpixels;
        let mut presented = Vec::with_capacity(presentation_rows);
        let mut math_blocks = Vec::new();

        // A boundary-split formula owns an exact transcript prefix above the live band that holds
        // its closer. Some prefix rows may still be in staging. Resolve each block once: finalized
        // ids must be the contiguous history tail, staging ids must be the complete staging plane,
        // and the live portion must begin at grid row zero. Anything else is not a clean boundary
        // split and is left to render as source.
        let mut bridge_geometry: HashMap<LiveMathOccurrenceId, (usize, u32)> = HashMap::new();
        let mut frozen_prefix_geometry: HashMap<LiveMathOccurrenceId, usize> = HashMap::new();
        if primary {
            let ordered = self.ordered_ids.len();
            for live_math in &self.live_math_artifacts {
                if (live_math.frozen_prefix.is_empty() && live_math.staging_prefix.is_empty())
                    || live_math.screen != screen
                    || live_math.generation != self.grid_generation
                    || live_math.band_start_row != 0
                {
                    continue;
                }
                let abs_top = if live_math.frozen_prefix.is_empty() {
                    history_rows
                } else {
                    let prefix = live_math.frozen_prefix.len();
                    if prefix > ordered
                        || self.ordered_ids[ordered - prefix..] != live_math.frozen_prefix[..]
                    {
                        continue;
                    }
                    // A finalized prefix must be plain source. A rendered history artifact means
                    // frozen and live detection paired differently and cannot share one band.
                    if live_math
                        .frozen_prefix
                        .iter()
                        .any(|id| self.math_artifacts.contains_key(id))
                    {
                        continue;
                    }
                    let first_index = ordered - prefix;
                    let abs_top = usize::try_from(self.visual_row_heights.prefix_sum(first_index))
                        .unwrap_or(0);
                    frozen_prefix_geometry.insert(live_math.occurrence_id, abs_top);
                    abs_top
                };
                // Staging may hold an unrelated in-progress logical line. It is not part of this
                // occurrence and must neither be swallowed nor prevent the exact frozen prefix
                // above it from being occluded. Only make one geometrically continuous bridge when
                // the complete staging plane is itself the occurrence's proven staging prefix.
                if staged_rows
                    .iter()
                    .map(|row| row.id)
                    .ne(live_math.staging_prefix.iter().copied())
                {
                    continue;
                }
                let prefix_rows = u32::try_from(live_base.saturating_sub(abs_top)).unwrap_or(0);
                if abs_top >= live_base || prefix_rows == 0 {
                    continue;
                }
                bridge_geometry.insert(live_math.occurrence_id, (abs_top, prefix_rows));
            }
        }
        let mut bridge_prefix_blank: Vec<(usize, usize)> = Vec::new();

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
                    let (laid_out, laid_out_heights) = if let Some(artifact) =
                        self.math_artifacts.get(id)
                    {
                        let max_offset =
                            entry.line.grapheme_boundaries.len().saturating_sub(1) as u32;
                        let local_start = window_start.saturating_sub(row_base);
                        let row_heights =
                            distributed_row_heights(artifact.height_subpixels, line_rows);
                        let visible_top =
                            frame_top_subpixels.saturating_add(presented_height(&presented));
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
                            frozen_prefix_rows: 0,
                            clipped_top_rows: 0,
                            clipped_bottom_rows: 0,
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
                        let mut rows = layout_frozen_line(&entry.line, column_count);
                        let source_rows = rows.len();
                        let mut heights = vec![self.cell_height_subpixels.get(); source_rows];
                        let path_artifacts = self
                            .inline_path_artifacts
                            .get(id)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]);
                        for artifact in path_artifacts {
                            let artifact_rows = usize::try_from(
                                artifact
                                    .height_subpixels
                                    .max(1)
                                    .saturating_add(self.cell_height_subpixels.get() - 1)
                                    .div_euclid(self.cell_height_subpixels.get()),
                            )
                            .unwrap_or(usize::MAX);
                            let max_offset =
                                entry.line.grapheme_boundaries.len().saturating_sub(1) as u32;
                            rows.extend((0..artifact_rows).map(|_| {
                                blank_visual_row(column_count, |_, bias| ContentAnchor::History {
                                    id: *id,
                                    offset: GraphemeOffset(max_offset),
                                    bias,
                                    generation: entry.line.source_generation,
                                })
                            }));
                            heights.extend(distributed_row_heights(
                                artifact.height_subpixels,
                                artifact_rows,
                            ));
                        }
                        let local_start = window_start.saturating_sub(row_base);
                        let line_visible_top =
                            frame_top_subpixels.saturating_add(presented_height(&presented));
                        let line_top = line_visible_top.saturating_sub(
                            heights
                                .iter()
                                .take(local_start.min(heights.len()))
                                .copied()
                                .sum::<i64>(),
                        );
                        let mut image_top = line_top.saturating_add(
                            i64::try_from(source_rows)
                                .unwrap_or(i64::MAX)
                                .saturating_mul(self.cell_height_subpixels.get()),
                        );
                        for artifact in path_artifacts {
                            math_blocks.push(MathBlockPlacement {
                                start: *id,
                                anchor: MathBlockAnchor::History {
                                    start: *id,
                                    end: artifact.end,
                                },
                                source: artifact.source.clone(),
                                artifact: artifact.clone(),
                                top_subpixels: image_top,
                                left_subpixels: 0,
                                content_offset_subpixels: 0,
                                clip_height_subpixels: artifact.height_subpixels,
                                display: MathBlockDisplay::Rendered,
                                horizontal_overflow: HorizontalOverflowOwner::Block,
                                horizontal_scroll_px: 0,
                                vertical_scroll_px: 0,
                                toolbar_visible: false,
                                occluded_source_rows: 0,
                                occluded_visible_rows: Vec::new(),
                                live_occurrence_id: None,
                                frozen_prefix_rows: 0,
                                clipped_top_rows: 0,
                                clipped_bottom_rows: 0,
                            });
                            image_top = image_top.saturating_add(artifact.height_subpixels);
                        }
                        (rows, heights)
                    };
                    for (local_row, row) in laid_out.iter().enumerate() {
                        validate_visual_row(row, column_count, "history", row_base + local_row)?;
                    }
                    let local_start = window_start.saturating_sub(row_base);
                    let local_end = window_end.saturating_sub(row_base).min(laid_out.len());
                    let visible_rows = local_end.saturating_sub(local_start);
                    presented.extend(
                        laid_out
                            .into_iter()
                            .zip(laid_out_heights)
                            .skip(local_start)
                            .take(visible_rows)
                            .map(|(visual, height_subpixels)| PresentedRow {
                                visual,
                                height_subpixels,
                                live_grid_row: None,
                            }),
                    );
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
                presented.push(PresentedRow {
                    visual: row,
                    height_subpixels: self.cell_height_subpixels.get(),
                    live_grid_row: None,
                });
            }
        }

        if window_end > live_base && window_start < live_base + expected_rows {
            let first = window_start.saturating_sub(live_base);
            let last = window_end.saturating_sub(live_base).min(live_rows.len());
            let visible_live_start = presented.len();
            presented.extend(
                live_rows
                    .into_iter()
                    .skip(first)
                    .take(last.saturating_sub(first))
                    .enumerate()
                    .map(|(offset, row)| {
                        let live_row = first + offset;
                        PresentedRow {
                            visual: captured_visual_row(row, column_count, |column, bias| {
                                ContentAnchor::Live {
                                    screen,
                                    point: GridPoint {
                                        row: live_row as u32,
                                        column: column as u32,
                                    },
                                    bias,
                                    generation: self.grid_generation,
                                }
                            }),
                            height_subpixels: self.live_row_prefix[live_row + 1]
                                .saturating_sub(self.live_row_prefix[live_row]),
                            live_grid_row: Some(live_row as u32),
                        }
                    }),
            );

            let mut path_image_offsets = HashMap::<u32, i64>::new();
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

                let cell_height = self.cell_height_subpixels.get();
                let artifact = live_math.artifact.clone();
                // A fresh live artifact renders at the readable scale; a live record whose layout
                // changed (a zoom/DPI change) holds its previous raster as a stale artifact scaled
                // by the DPI delta until the fresh relayout lands, so a scaled raster here is the
                // pinned old-layout preview, not an error.
                debug_assert!(artifact.render_scale_milli >= 1);
                let (top_subpixels, clip_height_subpixels, content_offset_subpixels, frozen_rows) =
                    if matches!(artifact.kind, RgbaArtifactKind::LocalImagePath { .. }) {
                        let row_top = frame_top_subpixels.saturating_add(
                            history_height
                                .saturating_add(staging_height)
                                .saturating_add(self.live_row_prefix[block_last])
                                .saturating_sub(window_row_top_subpixels),
                        );
                        let offset = path_image_offsets
                            .entry(live_math.band_end_row)
                            .or_default();
                        let top = row_top.saturating_add(cell_height).saturating_add(*offset);
                        *offset = offset.saturating_add(artifact.height_subpixels);
                        (top, artifact.height_subpixels, 0, 0)
                    } else if let Some(&(abs_top, frozen_rows)) =
                        bridge_geometry.get(&live_math.occurrence_id)
                    {
                        // Boundary-split block: its owned band starts in the frozen history rows
                        // above and runs down through the live closer. The whole occurrence renders
                        // as one image spanning both domains, centered in the combined row height.
                        // Positions are read from the frame's own accumulated row heights (never a
                        // uniform estimate) so a stretched history block sitting in the window above
                        // does not skew the bridge's top or clip height. Frozen-prefix rows above
                        // the window are uniform cell height, so the upward extrapolation is exact.
                        let row_top = |absolute: usize| -> i64 {
                            if absolute >= window_start {
                                let index = absolute - window_start;
                                frame_top_subpixels.saturating_add(
                                    presented
                                        .iter()
                                        .take(index)
                                        .map(|row| row.height_subpixels)
                                        .sum::<i64>(),
                                )
                            } else {
                                let rows_above =
                                    i64::try_from(window_start - absolute).unwrap_or(i64::MAX);
                                frame_top_subpixels
                                    .saturating_sub(rows_above.saturating_mul(cell_height))
                            }
                        };
                        let top = row_top(abs_top);
                        let band_end_absolute = live_base.saturating_add(block_last);
                        let bottom = row_top(band_end_absolute.saturating_add(1));
                        let combined_height = bottom.saturating_sub(top).max(0);
                        let content_offset = centered_content_offset(
                            combined_height,
                            artifact.height_subpixels,
                            artifact.vertical_padding_subpixels,
                        );
                        bridge_prefix_blank.push((abs_top, live_base));
                        (top, combined_height, content_offset, frozen_rows)
                    } else {
                        if let Some(&abs_top) = frozen_prefix_geometry.get(&live_math.occurrence_id)
                        {
                            // Exact frozen ownership is independent from bridge geometry. An
                            // unrelated staged line may sit between history and the live band; keep
                            // that line visible while still swallowing the occurrence's proven
                            // frozen source prefix.
                            bridge_prefix_blank.push((abs_top, history_rows));
                        }
                        let band_height = self.live_row_prefix[block_last + 1]
                            .saturating_sub(self.live_row_prefix[block_first]);
                        let total_rows = live_math
                            .clipped_top_rows
                            .saturating_add(
                                live_math
                                    .band_end_row
                                    .saturating_sub(live_math.band_start_row)
                                    .saturating_add(1),
                            )
                            .saturating_add(live_math.clipped_bottom_rows);
                        let source_band_height = i64::from(total_rows).saturating_mul(cell_height);
                        let presentation_height = if screen == ScreenId::Alternate {
                            artifact.height_subpixels.max(source_band_height)
                        } else {
                            band_height
                        };
                        let content_offset = centered_content_offset(
                            presentation_height,
                            artifact.height_subpixels,
                            artifact.vertical_padding_subpixels,
                        );
                        let top = frame_top_subpixels.saturating_add(
                            history_height
                                .saturating_add(staging_height)
                                .saturating_add(self.live_row_prefix[block_first])
                                .saturating_sub(window_row_top_subpixels),
                        );
                        (top, band_height, content_offset, 0)
                    };
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
                    clip_height_subpixels,
                    display: MathBlockDisplay::Rendered,
                    horizontal_overflow: HorizontalOverflowOwner::Block,
                    horizontal_scroll_px: 0,
                    vertical_scroll_px: 0,
                    toolbar_visible: false,
                    occluded_source_rows: live_math.occluded_source_rows,
                    occluded_visible_rows: live_math.occluded_visible_rows.clone(),
                    live_occurrence_id: Some(live_math.occurrence_id),
                    frozen_prefix_rows: frozen_rows,
                    clipped_top_rows: live_math.clipped_top_rows,
                    clipped_bottom_rows: live_math.clipped_bottom_rows,
                });

                if matches!(
                    live_math.artifact.kind,
                    RgbaArtifactKind::LocalImagePath { .. }
                ) {
                    continue;
                }

                let visible_first = block_first.max(first);
                let visible_last = block_last.min(last.saturating_sub(1));
                for live_row in visible_first..=visible_last {
                    let row = &mut presented[visible_live_start + live_row - first];
                    for cell in &mut row.visual.cells {
                        suppress_math_source_cell(cell);
                    }
                }
                for (live_row, clear_ranges) in &live_math.occluded_visible_rows {
                    let live_row = *live_row as usize;
                    if live_row < first || live_row >= last {
                        continue;
                    }
                    let row = &mut presented[visible_live_start + live_row - first];
                    // Only cells proven to show this occurrence's source are cleared; an
                    // application overlay sharing the row (Jump chip) keeps its text and
                    // highlight style untouched on both sides.
                    for (start, end) in clear_ranges {
                        let start = (*start as usize).min(row.visual.cells.len());
                        let end = (*end as usize).min(row.visual.cells.len());
                        for cell in &mut row.visual.cells[start..end] {
                            suppress_math_source_cell(cell);
                        }
                    }
                }
            }

            // Suppress every proven prefix row of an emitted boundary-split block, whether already
            // finalized in history or still in staging. Only rows inside the visible window change.
            for (abs_top, abs_end) in bridge_prefix_blank.drain(..) {
                for absolute in abs_top.max(window_start)..abs_end {
                    let Some(index) = absolute.checked_sub(window_start) else {
                        continue;
                    };
                    let Some(row) = presented.get_mut(index) else {
                        break;
                    };
                    for cell in &mut row.visual.cells {
                        suppress_math_source_cell(cell);
                    }
                }
            }
        }

        let last_grid_row = self.live_rows.get().saturating_sub(1);
        while presented.len() < presentation_rows {
            presented.push(PresentedRow {
                visual: blank_visual_row(column_count, |column, bias| ContentAnchor::Live {
                    screen,
                    point: GridPoint {
                        row: last_grid_row,
                        column: column as u32,
                    },
                    bias,
                    generation: self.grid_generation,
                }),
                height_subpixels: self.cell_height_subpixels.get(),
                live_grid_row: None,
            });
        }
        presented.truncate(presentation_rows);
        let FlattenedPresentedRows {
            cells,
            cell_anchors,
            row_map,
        } = flatten_presented_rows(presented, column_count, frame_top_subpixels)?;
        let mut local_path_offsets = HashMap::<u32, i64>::new();
        for placement in &mut math_blocks {
            if matches!(
                placement.artifact.kind,
                RgbaArtifactKind::LocalImagePath { .. }
            ) && let MathBlockAnchor::Live { band_end_row, .. } = placement.anchor
                && let Some(mapped) = row_map
                    .iter()
                    .find(|mapped| mapped.live_grid_row == Some(band_end_row))
            {
                let offset = local_path_offsets.entry(band_end_row).or_default();
                placement.top_subpixels = mapped
                    .top_subpixels
                    .saturating_add(self.cell_height_subpixels.get())
                    .saturating_add(*offset);
                *offset = offset.saturating_add(placement.artifact.height_subpixels);
                continue;
            }
            if let MathBlockAnchor::Live { band_start_row, .. } = placement.anchor
                && let Some((band_index, mapped)) = row_map
                    .iter()
                    .enumerate()
                    .find(|(_, mapped)| mapped.live_grid_row == Some(band_start_row))
            {
                if placement.frozen_prefix_rows == 0 {
                    placement.top_subpixels = mapped.top_subpixels;
                } else {
                    // A boundary-split block starts in the frozen scrollback rows above its live
                    // band. Snap its top to the first frozen-prefix row: visible frozen rows use
                    // their exact row-map top, and any that sit above the window are uniform cell
                    // height. Its clip height already reaches the live band bottom, so the whole
                    // occurrence renders as one block bridging both domains.
                    let frozen = placement.frozen_prefix_rows as usize;
                    placement.top_subpixels = if band_index >= frozen {
                        row_map[band_index - frozen].top_subpixels
                    } else {
                        let above = i64::try_from(frozen - band_index).unwrap_or(i64::MAX);
                        row_map
                            .first()
                            .map_or(frame_top_subpixels, |first| first.top_subpixels)
                            .saturating_sub(above.saturating_mul(self.cell_height_subpixels.get()))
                    };
                }
            }
        }
        let selection_spans = self
            .selection
            .as_ref()
            .map(|selection| {
                selection_spans(
                    &cell_anchors,
                    column_count,
                    if presentation_offset_subpixels == 0 {
                        expected_rows
                    } else {
                        presentation_rows
                    },
                    selection,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let projected_cursor_row = row_map
            .iter()
            .position(|row| row.live_grid_row == Some(cursor.row))
            .filter(|row| {
                let mapped = row_map[*row];
                mapped.top_subpixels < pane_height
                    && mapped.top_subpixels.saturating_add(mapped.height_subpixels) > 0
            })
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
        debug_assert_eq!(
            cells.len(),
            cell_anchors.len(),
            "one presentation-row list must flatten to equal cell and anchor rectangles"
        );
        let frame = ViewportFrame {
            columns,
            grid_rows: self.live_rows,
            rows: presentation_row_count,
            presentation_offset_subpixels,
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
            } else if !primary && self.scroll_offset_subpixels != 0 {
                Some(format!("{} rows below", self.scroll_offset_rows()))
            } else if content_rows_below != 0 {
                Some(format!("{content_rows_below} lines below"))
            } else if self.live_overflow_offset_subpixels != 0 {
                Some(format!("{} rows below", self.scroll_offset_rows()))
            } else {
                None
            },
            viewport_origin: match (&self.scroll_state, primary) {
                (ViewportScrollState::Bottom, _) if self.scroll_offset_subpixels == 0 => {
                    FrameViewportOrigin::Bottom
                }
                (_, false) => FrameViewportOrigin::LiveOverflow {
                    rows_below: self.scroll_offset_rows(),
                },
                (ViewportScrollState::Bottom, true) => FrameViewportOrigin::Bottom,
                (ViewportScrollState::Anchored(anchor), true) => {
                    FrameViewportOrigin::Anchored(anchor.clone())
                }
            },
            scroll_offset_rows: self.scroll_offset_rows(),
            layout_key: self.layout_key,
            view_generation: self.view_generation,
        };
        frame
            .validate_shape()
            .map_err(FrameProjectionError::FrameShape)?;
        Ok(frame)
    }

    fn history_row_heights(&self, document: &HistoryDocument, index: usize) -> Option<Vec<i64>> {
        let id = *self.ordered_ids.get(index)?;
        if let Some(artifact) = self.math_artifacts.get(&id) {
            return Some(distributed_row_heights(
                artifact.height_subpixels,
                self.visual_rows[index],
            ));
        }
        let entry = document.entries().get(&id)?;
        let source_rows =
            layout_frozen_line(&entry.line, self.layout_key.width_cells.get() as usize).len();
        let mut heights = vec![self.cell_height_subpixels.get(); source_rows];
        for artifact in self.inline_path_artifacts.get(&id).into_iter().flatten() {
            let rows = usize::try_from(
                artifact
                    .height_subpixels
                    .max(1)
                    .saturating_add(self.cell_height_subpixels.get() - 1)
                    .div_euclid(self.cell_height_subpixels.get()),
            )
            .unwrap_or(usize::MAX);
            heights.extend(distributed_row_heights(artifact.height_subpixels, rows));
        }
        Some(heights)
    }

    fn absolute_row_at_subpixel(
        &self,
        document: &HistoryDocument,
        staged_rows: &[StagedRow],
        history_rows: usize,
        history_height: i64,
        absolute_y: i64,
        screen: ScreenId,
    ) -> Option<(usize, i64)> {
        if screen == ScreenId::Primary && absolute_y < history_height {
            let index = self.heights.index_at_offset(absolute_y)?;
            let line_top = self.heights.prefix_sum(index);
            let local_y = absolute_y.saturating_sub(line_top);
            let heights = self.history_row_heights(document, index)?;
            let mut local_top = 0_i64;
            let local_row = heights.iter().position(|height| {
                let contains = local_y < local_top.saturating_add(*height);
                if !contains {
                    local_top = local_top.saturating_add(*height);
                }
                contains
            })?;
            let absolute_row = usize::try_from(self.visual_row_heights.prefix_sum(index))
                .ok()?
                .saturating_add(local_row);
            return Some((absolute_row, line_top.saturating_add(local_top)));
        }

        let staging_height = if screen == ScreenId::Primary {
            i64::try_from(staged_rows.len())
                .unwrap_or(i64::MAX)
                .saturating_mul(self.cell_height_subpixels.get())
        } else {
            0
        };
        let staging_top = history_height;
        if screen == ScreenId::Primary && absolute_y < staging_top.saturating_add(staging_height) {
            let local = absolute_y.saturating_sub(staging_top);
            let row = usize::try_from(local.div_euclid(self.cell_height_subpixels.get())).ok()?;
            let row_top = staging_top.saturating_add(
                i64::try_from(row)
                    .unwrap_or(i64::MAX)
                    .saturating_mul(self.cell_height_subpixels.get()),
            );
            return Some((history_rows.saturating_add(row), row_top));
        }

        let live_top = staging_top.saturating_add(staging_height);
        let local = absolute_y.saturating_sub(live_top);
        if local < 0 || local >= self.live_row_prefix.last().copied().unwrap_or(0) {
            return None;
        }
        let live_row = self
            .live_row_prefix
            .partition_point(|prefix| *prefix <= local)
            .saturating_sub(1)
            .min(self.live_rows.get().saturating_sub(1) as usize);
        let row = if screen == ScreenId::Primary {
            history_rows
                .saturating_add(staged_rows.len())
                .saturating_add(live_row)
        } else {
            live_row
        };
        Some((row, live_top.saturating_add(self.live_row_prefix[live_row])))
    }

    fn absolute_subpixel_for_anchor(
        &self,
        document: &HistoryDocument,
        staged_rows: &[StagedRow],
        history_height: i64,
        anchor: &ContentAnchor,
        screen: ScreenId,
    ) -> Option<i64> {
        match anchor {
            ContentAnchor::History { .. } if screen == ScreenId::Primary => {
                self.anchor_y(document, anchor).ok()
            }
            ContentAnchor::Staging { id, generation, .. }
                if screen == ScreenId::Primary && *generation == self.source_generation =>
            {
                let row = staged_rows.iter().position(|staged| staged.id == *id)?;
                Some(
                    history_height.saturating_add(
                        i64::try_from(row)
                            .unwrap_or(i64::MAX)
                            .saturating_mul(self.cell_height_subpixels.get()),
                    ),
                )
            }
            ContentAnchor::Live {
                screen: anchor_screen,
                point,
                generation,
                ..
            } if *anchor_screen == screen
                && *generation == self.grid_generation
                && point.row < self.live_rows.get() =>
            {
                let staging_height = if screen == ScreenId::Primary {
                    i64::try_from(staged_rows.len())
                        .unwrap_or(i64::MAX)
                        .saturating_mul(self.cell_height_subpixels.get())
                } else {
                    0
                };
                Some(
                    history_height
                        .saturating_add(staging_height)
                        .saturating_add(self.live_row_prefix[point.row as usize]),
                )
            }
            _ => None,
        }
    }

    fn scroll_anchor_at_absolute_subpixel(
        &self,
        document: &HistoryDocument,
        staged_rows: &[StagedRow],
        history_rows: usize,
        absolute_y: i64,
        screen: ScreenId,
    ) -> Option<ScrollAnchor> {
        let history_height = if screen == ScreenId::Primary {
            self.heights.total()
        } else {
            0
        };
        let (absolute_row, row_top) = self.absolute_row_at_subpixel(
            document,
            staged_rows,
            history_rows,
            history_height,
            absolute_y,
            screen,
        )?;
        let intra_row = absolute_y.saturating_sub(row_top);
        if screen == ScreenId::Primary && absolute_y < history_height {
            let index = self.heights.index_at_offset(absolute_y)?;
            let id = *self.ordered_ids.get(index)?;
            let entry = document.entries().get(&id)?;
            let line_top = self.heights.prefix_sum(index);
            if self.math_artifacts.contains_key(&id) {
                return Some(ScrollAnchor {
                    source: ContentAnchor::History {
                        id,
                        offset: GraphemeOffset(0),
                        bias: Bias::Before,
                        generation: entry.line.source_generation,
                    },
                    local_offset: absolute_y.saturating_sub(line_top),
                });
            }
            let source_rows =
                layout_frozen_line(&entry.line, self.layout_key.width_cells.get() as usize);
            let local_row =
                absolute_row.saturating_sub(self.visual_row_heights.prefix_sum(index) as usize);
            if let Some(row) = source_rows.get(local_row) {
                return row.anchors.first().map(|anchor| ScrollAnchor {
                    source: anchor.start.clone(),
                    local_offset: intra_row,
                });
            }
            let source_end = source_rows.last()?.anchors.last()?.end.clone();
            let source_y = self.anchor_y(document, &source_end).ok()?;
            return Some(ScrollAnchor {
                source: source_end,
                local_offset: absolute_y.saturating_sub(source_y),
            });
        }

        let staging_base = if screen == ScreenId::Primary {
            history_rows
        } else {
            0
        };
        let staging_row = absolute_row.saturating_sub(staging_base);
        if screen == ScreenId::Primary
            && let Some(staged) = staged_rows.get(staging_row)
        {
            return Some(ScrollAnchor {
                source: ContentAnchor::Staging {
                    id: staged.id,
                    offset: GraphemeOffset(0),
                    bias: Bias::Before,
                    generation: self.source_generation,
                },
                local_offset: intra_row,
            });
        }
        let live_row = if screen == ScreenId::Primary {
            staging_row.saturating_sub(staged_rows.len())
        } else {
            absolute_row
        };
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
            local_offset: intra_row,
        })
    }

    pub fn set_selection(&mut self, selection: Option<ViewSelection>) {
        self.selection = selection;
    }
    pub fn set_scroll_anchor(&mut self, anchor: Option<ScrollAnchor>) {
        self.pending_scroll_offset_subpixels = None;
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

    /// Synchronize images appended below path-bearing transcript lines. Unlike display math and
    /// OSC image placeholders, these artifacts add height without suppressing or replacing source.
    pub fn sync_inline_path_artifacts(
        &mut self,
        artifacts: impl IntoIterator<Item = (TranscriptId, ProjectedMathArtifact)>,
    ) {
        let mut next = HashMap::<TranscriptId, Vec<ProjectedMathArtifact>>::new();
        for (id, artifact) in artifacts {
            next.entry(id).or_default().push(artifact);
        }
        let changed = self
            .inline_path_artifacts
            .keys()
            .chain(next.keys())
            .copied()
            .filter(|id| self.inline_path_artifacts.get(id) != next.get(id))
            .collect::<HashSet<_>>();
        if !changed.is_empty() {
            self.cache
                .retain(|key, _| !changed.contains(&key.span.start));
            self.projection_dirty = true;
        }
        self.inline_path_artifacts = next;
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
            let per_block_limit = i64::from(
                self.live_rows
                    .get()
                    .saturating_sub(LIVE_MIN_VISIBLE_TEXT_ROWS),
            )
            .saturating_mul(self.cell_height_subpixels.get());
            // Free-height overflow is locally reviewable and bottom anchoring preserves the
            // terminal's last rows. Applying this limit cumulatively made otherwise valid older
            // formulas fall back to source merely because several blocks shared one live grid.
            // Keep the safety floor as an individual-block bound, while allowing every bounded,
            // proven occurrence to participate in the prefix map.
            candidates
                .into_iter()
                .filter(|artifact| {
                if matches!(
                    artifact.artifact.kind,
                    RgbaArtifactKind::LocalImagePath { .. }
                ) {
                    return true;
                }
                // A boundary-split block occupies only its live band on the grid; the bulk of its
                // height is carried by the frozen scrollback rows it already owns above. It is
                // measured by its live-band height (never the full image).
                let box_height = if artifact.frozen_prefix.is_empty() {
                    artifact.artifact.height_subpixels.max(1)
                } else {
                    i64::from(
                        artifact
                            .band_end_row
                            .saturating_sub(artifact.band_start_row)
                            .saturating_add(1),
                    )
                    .saturating_mul(self.cell_height_subpixels.get())
                    .max(1)
                };
                // A scaled stale raster (render_scale_milli != readable) is a proven block whose
                // layout changed under a zoom; it stays pinned (scaled to approximate the new size)
                // rather than flashing to source while its fresh relayout is off-thread. Its box
                // height is already the scaled height, so the visible-text floor stays honest.
                let accepted = box_height <= per_block_limit;
                if !accepted && std::env::var_os("BT_PERF_TRACE").is_some() {
                    eprintln!(
                        "BT_PERF_TRACE live_math_event=source-fallback row={} box_subpixels={} per_block_limit_subpixels={} min_text_rows={} reason=block-exceeds-visible-text-floor",
                        artifact.start.row,
                        box_height,
                        per_block_limit,
                        LIVE_MIN_VISIBLE_TEXT_ROWS,
                    );
                }
                accepted
            })
            .collect()
        };
        let mut per_row_height =
            vec![self.cell_height_subpixels.get(); self.live_rows.get() as usize];
        for artifact in &accepted {
            if matches!(
                artifact.artifact.kind,
                RgbaArtifactKind::LocalImagePath { .. }
            ) {
                if let Some(height) = per_row_height.get_mut(artifact.band_end_row as usize) {
                    *height = height.saturating_add(artifact.artifact.height_subpixels);
                }
                continue;
            }
            // A boundary-split block never expands its live rows: its rendered image spans the
            // frozen scrollback rows above plus its live band, and projection sizes that bridged
            // span directly. Its live band keeps natural row heights here.
            if !artifact.frozen_prefix.is_empty() {
                continue;
            }
            let visible_rows = artifact
                .band_end_row
                .saturating_sub(artifact.band_start_row)
                .saturating_add(1);
            // The primary band's height budget normally spans every clipped row (the reveal extent
            // above and below the grid). That is correct only while the clipped rows are genuinely
            // off a grid edge. A clipped count reported while the visible band sits wholly inside
            // the grid — neither reaching live row zero nor the last live row, and without cross-
            // boundary occlusion — is a transient reprojection artifact: the block is entirely on
            // screen and the detector momentarily under-counted its own rows during a reprint or
            // reflow. Spreading the artifact height across those phantom rows collapses the band
            // below the artifact, and the raster then clips short of its own descent (the first-
            // render missing integral limit / half-cut Maxwell block). In that lone case primary
            // sizes the band from the visible rows alone, flooring it to the artifact height exactly
            // as the alternate screen already does. Whenever a genuine edge clip (M1.9v top reveal,
            // bottom-edge run-off) or a fresh artifact's occlusion is present the reduced band is
            // legitimate and the HEAD sizing is kept to the subpixel; boundary-split bridges never
            // reach this loop. A transition-stale primary raster is the exception: its occluded rows
            // are the still-exact remainder of the old layout, so the preview must retain them until
            // the replacement artifact arrives.
            let last_live_row = self.live_rows.get().saturating_sub(1);
            let full_clipped_rows = artifact
                .clipped_top_rows
                .saturating_add(visible_rows)
                .saturating_add(artifact.clipped_bottom_rows);
            let genuine_bottom_clip =
                artifact.clipped_bottom_rows > 0 && artifact.band_end_row == last_live_row;
            let occluded =
                artifact.occluded_source_rows > 0 || !artifact.occluded_visible_rows.is_empty();
            let stale_reflow_preview = screen == ScreenId::Primary
                && artifact.transition_stale
                && occluded
                && !genuine_bottom_clip;
            let (rows, top_pad_rows) = if screen == ScreenId::Alternate {
                (full_clipped_rows, artifact.clipped_top_rows)
            } else {
                // The bottom edge is a real horizon: nothing exists below the last live row, so a
                // block ending there with clipped rows is genuinely running off the bottom and its
                // reduced band is correct. The top edge is NOT the mirror of that horizon. A band
                // pinned to live row zero does not prove the occurrence extends above it: a primary
                // block reaching this loop owns every source row inside the live grid, because
                // genuine upward extension into scrollback is projected as a boundary-split bridge
                // (skipped above) and a top hidden behind fixed chrome surfaces as occlusion (kept
                // below). So a reported clipped-top on such a block is never a genuine top reveal —
                // it is a reprojection transient during a reprint/reflow/zoom whose stale identity
                // out-counts the reflowed occurrence's rows. Spreading the artifact across those
                // phantom top rows and taking the middle slice is exactly what clipped the integral
                // limit and cut the pmatrix in half. Only a genuine bottom run-off or occlusion
                // keeps the reduced band, except for an exact transition-stale raster whose hidden
                // old-layout rows remain part of the deterministic preview. Otherwise primary
                // floors to the full artifact so the whole reflowed occurrence previews (its own
                // rows, full raster) instead of a half-band fragment. `band_start_row == 0` is
                // deliberately absent here — it was the buggy top mirror this closes.
                if genuine_bottom_clip || (occluded && !stale_reflow_preview) {
                    (full_clipped_rows, artifact.clipped_top_rows)
                } else {
                    (visible_rows, 0)
                }
            };
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
                        heights.get(top_pad_rows.saturating_add(offset) as usize)
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
            self.live_overflow_offset_subpixels = 0;
            self.last_live_overflow_subpixels = 0;
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
                        let source_visual_lines = frozen_visual_line_count(
                            &entry.line.text,
                            self.layout_key.width_cells.get() as usize,
                        ) as u32;
                        let (image_visual_lines, image_height) = self
                            .inline_path_artifacts
                            .get(id)
                            .into_iter()
                            .flatten()
                            .fold((0_u32, 0_i64), |(rows, height), artifact| {
                                let artifact_rows = u32::try_from(
                                    artifact
                                        .height_subpixels
                                        .max(1)
                                        .saturating_add(self.cell_height_subpixels.get() - 1)
                                        .div_euclid(self.cell_height_subpixels.get()),
                                )
                                .unwrap_or(u32::MAX);
                                (
                                    rows.saturating_add(artifact_rows),
                                    height.saturating_add(artifact.height_subpixels),
                                )
                            });
                        let visual_lines = source_visual_lines.saturating_add(image_visual_lines);
                        MeasuredLayout {
                            visual_lines,
                            height: i64::from(source_visual_lines)
                                .saturating_mul(self.cell_height_subpixels.get())
                                .saturating_add(image_height),
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

/// Remove every terminal-rendered aspect of one source-proven formula cell while leaving its
/// semantic `CellAnchor` (stored beside the cell in `VisualRow`) untouched. Clearing only `text`
/// is insufficient: an empty UNDERLINE/INVERSE/non-default-background cell still paints pixels
/// underneath the transparent formula raster. Callers choose the exact owned cells, so application
/// overlays outside those ranges keep their glyphs and highlight styling byte-for-byte.
fn suppress_math_source_cell(cell: &mut CapturedCell) {
    *cell = CapturedCell::default();
}

fn captured_row_is_blank(row: &CapturedRow) -> bool {
    row.cells
        .iter()
        .filter(|cell| !cell.wide_spacer)
        .all(|cell| cell.text.chars().all(char::is_whitespace))
}

fn captured_visual_row(
    row: CapturedRow,
    columns: usize,
    anchor: impl Fn(usize, Bias) -> ContentAnchor,
) -> VisualRow {
    let mut cells = row.cells;
    let mut anchors = Vec::with_capacity(columns);
    let mut lead = 0usize;
    for (column, cell) in cells.iter().enumerate() {
        if !cell.wide_spacer {
            lead = column;
        }
        anchors.push(CellAnchor {
            start: anchor(lead, Bias::Before),
            end: anchor(lead, Bias::After),
        });
    }
    apply_implicit_hyperlinks(&mut cells);
    VisualRow { cells, anchors }
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
    let mut cells = staged.row.cells.clone();
    apply_implicit_hyperlinks(&mut cells);
    VisualRow { cells, anchors }
}

fn apply_implicit_hyperlinks(cells: &mut [CapturedCell]) {
    // At this point every existing hyperlink came from OSC 8 in the captured terminal row.
    // Projection owns the affordance: the source grid/transcript remains byte-for-byte unchanged,
    // while inferred URLs added below deliberately retain their unmarked resting presentation.
    for cell in cells.iter_mut().filter(|cell| cell.hyperlink.is_some()) {
        cell.style.flags.insert(CellFlags::DOTTED_UNDERLINE);
    }

    let mut text = String::new();
    let mut byte_ranges = Vec::with_capacity(cells.len());
    for cell in cells.iter() {
        let start = text.len();
        if !cell.wide_spacer {
            text.push_str(&cell.text);
        }
        byte_ranges.push(start..text.len());
    }
    for range in detect_http_urls(&text) {
        let affected = byte_ranges
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.start < range.byte_end && range.byte_start < cell.end)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if affected.is_empty()
            || affected
                .iter()
                .any(|index| cells[*index].hyperlink.is_some())
        {
            continue;
        }
        let link = CellHyperlink::implicit(text[range.byte_start..range.byte_end].to_owned());
        for index in affected {
            cells[index].hyperlink = Some(link.clone());
            if index + 1 < cells.len() && cells[index + 1].wide_spacer {
                cells[index + 1].hyperlink = Some(link.clone());
            }
        }
    }
}

/// True when `cell` belongs to the same OSC 8 link group: exact (id, uri) match, read explicitly
/// because `CellHyperlink`'s own equality deliberately covers the uri alone.
fn cell_in_link_group(cell: &CapturedCell, link: &CellHyperlink) -> bool {
    cell.hyperlink
        .as_ref()
        .is_some_and(|other| other.id == link.id && other.uri == link.uri)
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
    let implicit_links = detect_http_urls(&line.text)
        .into_iter()
        .filter(|range| {
            !line.styles.iter().any(|span| {
                span.hyperlink.is_some()
                    && (span.byte_start as usize) < range.byte_end
                    && range.byte_start < span.byte_end as usize
            })
        })
        .collect::<Vec<_>>();
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
            if cell.hyperlink.is_some() {
                cell.style.flags.insert(CellFlags::DOTTED_UNDERLINE);
            }
        }
        if cell.hyperlink.is_none()
            && let Some(range) = implicit_link_at(&implicit_links, byte_start as usize)
        {
            cell.hyperlink = Some(CellHyperlink::implicit(
                line.text[range.byte_start..range.byte_end].to_owned(),
            ));
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

fn implicit_link_at(ranges: &[HyperlinkRange], byte: usize) -> Option<HyperlinkRange> {
    ranges
        .iter()
        .copied()
        .find(|range| range.byte_start <= byte && byte < range.byte_end)
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
    if anchors.len() < expected {
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
    for (row, row_anchors) in anchors[..expected].chunks(columns).enumerate() {
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
        let extra = 32 * SUBPIXELS_PER_PX;
        let mut projection = ViewportProjection::new(
            key(4),
            DetectionRevision(1),
            nz32(3),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        projection.sync_live_math_artifacts(
            ScreenId::Alternate,
            [ProjectedLiveMathArtifact {
                occurrence_id: LiveMathOccurrenceId(1),
                screen: ScreenId::Alternate,
                start: GridPoint { row: 1, column: 0 },
                end: GridPoint { row: 1, column: 3 },
                band_start_row: 1,
                band_end_row: 1,
                clipped_top_rows: 0,
                clipped_bottom_rows: 0,
                occluded_source_rows: 0,
                occluded_visible_rows: Vec::new(),
                transition_stale: false,
                frozen_prefix: Vec::new(),
                staging_prefix: Vec::new(),
                generation: GridGeneration(1),
                artifact: ProjectedMathArtifact {
                    key: "offscreen-prefix".to_owned(),
                    end: TranscriptId(0),
                    rgba: Arc::from(vec![255; 4]),
                    width_px: 1,
                    height_px: 1,
                    height_subpixels: cell + extra,
                    baseline_subpixels: 0,
                    mode: MathMode::Display,
                    kind: RgbaArtifactKind::Math,
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
                vec![CapturedRow::plain("    ", false); 3],
                GridCursor {
                    row: 2,
                    column: 0,
                    visible: true,
                },
                ScreenId::Alternate,
            )
            .unwrap();
        let live_origin = frame
            .row_map
            .iter()
            .find(|row| row.live_grid_row == Some(0))
            .unwrap()
            .top_subpixels;
        let live_row_two = frame
            .row_map
            .iter()
            .find(|row| row.live_grid_row == Some(2))
            .unwrap()
            .top_subpixels;
        assert_eq!(live_row_two.saturating_sub(live_origin), 2 * cell + extra);
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
        assert_eq!(frame.grid_rows.get(), 2);
        assert_eq!(frame.rows.get(), 3);
        assert_eq!(frame.cells.len(), 6);
        assert_eq!(frame.cells[0].text, "a");
        assert_eq!(frame.cells[3].text, "d");
        assert!(
            frame.cells[4..]
                .iter()
                .all(|cell| *cell == CapturedCell::default()),
            "phase-A bottom overscan is a presentation-blank row at live bottom"
        );
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
    fn phase_a_presentation_rectangle_carries_one_hidden_overscan_row_and_zero_offset() {
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
        let cell = cell_height().get();

        assert_eq!(frame.grid_rows.get(), 2);
        assert_eq!(
            frame.rows.get(),
            frame.grid_rows.get() + FRAME_OVERSCAN_ROWS
        );
        assert_eq!(frame.presentation_offset_subpixels, 0);
        assert_eq!(frame.cells.len(), frame.rows.get() as usize * 2);
        assert_eq!(frame.cell_anchors.len(), frame.cells.len());
        assert_eq!(frame.row_map.len(), frame.rows.get() as usize);
        assert_eq!(frame.drawable_rows(), frame.grid_rows.get() as usize);
        assert_eq!(frame.row_map[0].top_subpixels, 0);
        assert_eq!(frame.row_map[2].top_subpixels, 2 * cell);
        assert_eq!(frame.row_map[2].live_grid_row, None);
        assert_eq!(frame.visual_row_at(2 * cell), None);
        assert!(!frame.drawable_interval_overlaps(2 * cell, cell));
        assert!(
            frame.anchor_at(2, 0, Bias::Before).unwrap().is_some(),
            "overscan cells and anchors are part of the same presentation rectangle"
        );
    }

    #[test]
    fn presentation_contract_can_construct_a_negative_top_partial_first_row() {
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
        let offset = cell_height().get() / 2;
        frame.presentation_offset_subpixels = offset;
        for row in &mut frame.row_map {
            row.top_subpixels = row.top_subpixels.saturating_sub(offset);
        }

        frame.validate_shape().unwrap();
        assert_eq!(frame.row_map[0].top_subpixels, -offset);
        assert_eq!(frame.drawable_rows(), frame.rows.get() as usize);
        assert_eq!(frame.visual_row_at(0), Some(0));
        assert_eq!(
            frame.visual_row_at(2 * cell_height().get() - offset),
            Some(2),
            "the bottom overscan row becomes addressable only once an exact offset exposes it"
        );
    }

    #[test]
    fn primary_and_alternate_publish_the_same_phase_a_row_contract() {
        for screen in [ScreenId::Primary, ScreenId::Alternate] {
            let mut projection = ViewportProjection::new(
                key(2),
                DetectionRevision(1),
                nz32(2),
                cell_height(),
                SourceGeneration(1),
                GridGeneration(1),
            );
            let frame = projection
                .continuous_frame(
                    &HistoryDocument::default(),
                    &[],
                    vec![
                        CapturedRow::plain("ab", false),
                        CapturedRow::plain("cd", false),
                    ],
                    GridCursor {
                        row: 0,
                        column: 0,
                        visible: true,
                    },
                    screen,
                )
                .unwrap();
            assert_eq!(frame.grid_rows.get(), 2);
            assert_eq!(frame.rows.get(), 3);
            assert_eq!(frame.presentation_offset_subpixels, 0);
            assert_eq!(frame.row_map[2].live_grid_row, None);
        }
    }

    #[test]
    fn implicit_links_fill_only_cells_not_owned_by_osc_8() {
        let mut cells =
            CapturedRow::plain("https://shown.example https://plain.example).", false).cells;
        for cell in &mut cells[..21] {
            cell.hyperlink = Some(CellHyperlink::implicit("file:///real-target"));
        }
        apply_implicit_hyperlinks(&mut cells);

        assert!(cells[..21].iter().all(
            |cell| cell.hyperlink.as_ref().map(|link| link.uri.as_str())
                == Some("file:///real-target")
                && cell.style.flags.contains(CellFlags::DOTTED_UNDERLINE)
        ));
        let implicit = "https://plain.example";
        assert!(cells[22..43].iter().all(|cell| {
            cell.hyperlink.as_ref().map(|link| link.uri.as_str()) == Some(implicit)
                && !cell.style.flags.contains(CellFlags::DOTTED_UNDERLINE)
        }));
        assert_eq!(cells[43].hyperlink, None, "trailing ')' is not linked");
        assert_eq!(cells[44].hyperlink, None, "trailing '.' is not linked");
    }

    #[test]
    fn frozen_implicit_link_survives_reflow_without_mutating_source() {
        let text = "https://example.test/path".to_owned();
        let line = FrozenLine {
            id: TranscriptId(7),
            source_generation: SourceGeneration(3),
            grapheme_boundaries: (0..=text.len() as u32).collect(),
            text: text.clone(),
            styles: Vec::new(),
            fragments: Vec::new(),
            shell_marks: Vec::new(),
            wrap_split: false,
        };
        let rows = layout_frozen_line(&line, 8);
        let linked_text = rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .filter(|cell| {
                cell.hyperlink.as_ref().map(|link| link.uri.as_str()) == Some(text.as_str())
            })
            .map(|cell| cell.text.as_str())
            .collect::<String>();

        assert_eq!(linked_text, text);
        assert_eq!(line.text, text, "recognition is projection-only");
    }

    #[test]
    fn frozen_osc_8_link_is_dotted_while_bare_url_has_no_resting_marker() {
        let explicit = "trusted";
        let implicit = "https://plain.example";
        let text = format!("{explicit} {implicit}");
        let line = FrozenLine {
            id: TranscriptId(7),
            source_generation: SourceGeneration(3),
            grapheme_boundaries: (0..=text.len() as u32).collect(),
            text: text.clone(),
            styles: vec![bt_transcript::StyleSpan {
                byte_start: 0,
                byte_end: explicit.len() as u32,
                style: bt_transcript::CellStyle::default(),
                hyperlink: Some(CellHyperlink::implicit("file:///actual-target")),
            }],
            fragments: Vec::new(),
            shell_marks: Vec::new(),
            wrap_split: false,
        };

        let cells = layout_frozen_line(&line, 80)
            .into_iter()
            .flat_map(|row| row.cells)
            .collect::<Vec<_>>();
        assert!(cells[..explicit.len()].iter().all(|cell| {
            cell.hyperlink.as_ref().map(|link| link.uri.as_str()) == Some("file:///actual-target")
                && cell.style.flags.contains(CellFlags::DOTTED_UNDERLINE)
        }));
        assert!(
            cells[explicit.len() + 1..explicit.len() + 1 + implicit.len()]
                .iter()
                .all(|cell| {
                    cell.hyperlink.as_ref().map(|link| link.uri.as_str()) == Some(implicit)
                        && !cell.style.flags.contains(CellFlags::DOTTED_UNDERLINE)
                })
        );
        assert!(
            line.styles[0].style.flags.is_empty(),
            "projection must not mutate frozen transcript style"
        );
    }

    #[test]
    fn hyperlink_hit_exposes_real_target_and_underlines_the_complete_span() {
        let mut cells = CapturedRow::plain("trusted label", false).cells;
        for cell in &mut cells {
            cell.hyperlink = Some(CellHyperlink::implicit("https://actual.example/login"));
        }
        let projection = ViewportProjection::new(
            key(13),
            DetectionRevision(1),
            nz32(1),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let mut frame = projection
            .live_frame(
                nz32(13),
                vec![CapturedRow {
                    cells,
                    continues: false,
                    shell_mark: None,
                }],
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: false,
                },
            )
            .unwrap();

        let hit = frame.hyperlink_at(0, 4).unwrap();
        assert_eq!(hit.uri, "https://actual.example/login");
        assert!(
            frame
                .cells
                .iter()
                .take(frame.drawable_rows() * frame.columns.get() as usize)
                .all(|cell| cell.style.flags.contains(CellFlags::DOTTED_UNDERLINE))
        );
        assert!(frame.underline_hyperlink(&hit));
        assert!(
            frame
                .cells
                .iter()
                .take(frame.drawable_rows() * frame.columns.get() as usize)
                .all(|cell| cell.style.flags.contains(CellFlags::UNDERLINE)
                    && !cell.style.flags.contains(CellFlags::DOTTED_UNDERLINE))
        );
    }

    #[test]
    fn id_grouped_wrapped_link_underlines_both_segments_across_the_indent_gap() {
        // The Claude Code layout: an OSC 8 link wraps, and the continuation row is indented, so
        // the two segments are separated by non-link cells. The shared emission id (vendor
        // synthesizes one when the app omits it) makes them one link: hovering either segment
        // must upgrade both to solid, and the gap cells must stay untouched.
        let link = CellHyperlink {
            id: Some("42_alacritty".to_owned()),
            uri: "file:///C:/pictures/a.png".to_owned(),
        };
        let mut first_row = CapturedRow::plain("path-head", true).cells;
        for cell in &mut first_row[2..] {
            cell.hyperlink = Some(link.clone());
        }
        let mut second_row = CapturedRow::plain("  tail   ", false).cells;
        for cell in &mut second_row[2..6] {
            cell.hyperlink = Some(link.clone());
        }
        let projection = ViewportProjection::new(
            key(9),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let mut frame = projection
            .live_frame(
                nz32(9),
                vec![
                    CapturedRow {
                        cells: first_row,
                        continues: true,
                        shell_mark: None,
                    },
                    CapturedRow {
                        cells: second_row,
                        continues: false,
                        shell_mark: None,
                    },
                ],
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: false,
                },
            )
            .unwrap();

        // Hitting the second segment reports the whole link, first segment start to last end.
        let hit = frame.hyperlink_at(1, 3).unwrap();
        assert_eq!(hit.uri, "file:///C:/pictures/a.png");
        assert_eq!(hit.id.as_deref(), Some("42_alacritty"));
        assert_eq!(hit, frame.hyperlink_at(0, 5).unwrap());
        assert!(frame.underline_hyperlink(&hit));
        let columns = frame.columns.get() as usize;
        let solid = |frame: &ViewportFrame, row: usize, column: usize| {
            let flags = frame.cells[row * columns + column].style.flags;
            flags.contains(CellFlags::UNDERLINE) && !flags.contains(CellFlags::DOTTED_UNDERLINE)
        };
        for column in 2..9 {
            assert!(solid(&frame, 0, column), "first segment column {column}");
        }
        for column in 2..6 {
            assert!(solid(&frame, 1, column), "second segment column {column}");
        }
        // The indent gap and the untouched tail carry no underline at all.
        for column in [0, 1] {
            assert!(
                !frame.cells[columns + column]
                    .style
                    .flags
                    .intersects(CellFlags::UNDERLINE | CellFlags::DOTTED_UNDERLINE),
                "gap column {column}"
            );
        }
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
                    transition_stale: false,
                    frozen_prefix: Vec::new(),
                    staging_prefix: Vec::new(),
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
                        kind: RgbaArtifactKind::Math,
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

    /// Project a single live math block through `sync_live_math_artifacts` + `continuous_frame` and
    /// return `(clip_height, artifact_height, frame)`. The artifact is `art_cells` cell-heights tall
    /// so the row distribution is exact.
    #[allow(clippy::too_many_arguments)]
    fn project_single_live_block(
        screen: ScreenId,
        live_rows: u32,
        band_start_row: u32,
        band_end_row: u32,
        clipped_top_rows: u32,
        clipped_bottom_rows: u32,
        occluded_source_rows: u32,
        art_cells: u32,
    ) -> (i64, i64, ViewportFrame) {
        project_single_live_block_with_staleness(
            screen,
            live_rows,
            band_start_row,
            band_end_row,
            clipped_top_rows,
            clipped_bottom_rows,
            occluded_source_rows,
            art_cells,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn project_single_live_block_with_staleness(
        screen: ScreenId,
        live_rows: u32,
        band_start_row: u32,
        band_end_row: u32,
        clipped_top_rows: u32,
        clipped_bottom_rows: u32,
        occluded_source_rows: u32,
        art_cells: u32,
        transition_stale: bool,
    ) -> (i64, i64, ViewportFrame) {
        let art_h = i64::from(art_cells) * cell_height().get();
        let height_px = (art_cells * 18) as usize;
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(live_rows),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        projection.sync_live_math_artifacts(
            screen,
            [ProjectedLiveMathArtifact {
                occurrence_id: LiveMathOccurrenceId(1),
                screen,
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
                clipped_bottom_rows,
                occluded_source_rows,
                occluded_visible_rows: Vec::new(),
                transition_stale,
                frozen_prefix: Vec::new(),
                staging_prefix: Vec::new(),
                generation: GridGeneration(1),
                artifact: ProjectedMathArtifact {
                    key: "display-x".to_owned(),
                    end: TranscriptId(0),
                    rgba: Arc::from(vec![255; height_px * 4]),
                    width_px: 1,
                    height_px: height_px as u32,
                    height_subpixels: art_h,
                    baseline_subpixels: 0,
                    mode: MathMode::Display,
                    kind: RgbaArtifactKind::Math,
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
                vec![CapturedRow::plain("        ", false); live_rows as usize],
                GridCursor {
                    row: live_rows - 1,
                    column: 0,
                    visible: true,
                },
                screen,
            )
            .unwrap();
        frame.validate_shape().unwrap();
        assert_eq!(frame.math_blocks.len(), 1, "block must be emitted");
        let block = &frame.math_blocks[0];
        assert_eq!(block.display, MathBlockDisplay::Rendered);
        (block.clip_height_subpixels, art_h, frame.clone())
    }

    /// The first-render / transition-frame defect: a display block sitting wholly inside the live
    /// grid is transiently under-counted (its closer rows momentarily fail to match during a
    /// reprint), so the detector reports clipped-bottom rows while the band ends mid-grid. HEAD
    /// spreads the full artifact across the phantom rows and clips the raster short of its own
    /// descent (the missing integral lower limit / half-cut Maxwell block). With no genuine edge
    /// clip and no occlusion, primary now floors the band to the artifact height so the whole block
    /// shows, exactly as the alternate screen already does.
    #[test]
    fn primary_collapsed_band_floors_to_artifact_height() {
        // Artifact is three cells tall; the band collapsed to a single mid-grid row (band 4..=4)
        // with two spurious clipped-bottom rows. last live row is 11, so the band touches no edge.
        let (clip, art_h, _frame) =
            project_single_live_block(ScreenId::Primary, 12, 4, 4, 0, 2, 0, 3);
        assert_eq!(
            clip, art_h,
            "a wholly-visible collapsed band must floor its clip to the artifact height"
        );
    }

    /// The top-edge mirror gap (closes 976a6aa's `genuine_top_clip = clipped_top>0 && band_start==0`).
    /// On primary a non-bridge block reporting a clipped-top while its band sits pinned to live row
    /// zero is NOT a genuine reveal: genuine upward extension into scrollback is a boundary-split
    /// bridge (which never reaches this loop) and a top behind fixed chrome is occlusion (a separate
    /// suppressor). So band-touching-row-zero is a reprojection/reflow/zoom transient whose stale
    /// identity out-counts the reflowed occurrence — the fake top clip that halved the pmatrix and
    /// dropped the integral's lower limit in the zoom-stale window. Primary now floors it to the full
    /// artifact so the whole block previews; only the alternate screen (below) and occlusion keep the
    /// reduced band. HEAD asserted the reduced two-cell clip here; that assertion encoded the bug.
    #[test]
    fn primary_phantom_top_clip_floors_to_artifact_height() {
        // band 0..=1 with one clipped-top row and no bottom-edge/occlusion evidence: floors to art.
        let (clip, art_h, _frame) =
            project_single_live_block(ScreenId::Primary, 12, 0, 1, 1, 0, 0, 3);
        assert_eq!(
            clip, art_h,
            "a non-bridge primary block pinned to row zero has no genuine top clip and must floor"
        );
    }

    /// The top-clip fix is primary-only: the alternate screen is on its own expand-only + upward-
    /// reveal-fold path (line ~2053) and the floor change never touches it. On the exact phantom-top
    /// input the primary case floors above, alt yields its pre-existing geometry — byte-identical
    /// before and after the fix (corroborated by the alt captures replaying byte-identical). This
    /// pins that value so a future primary-side change cannot silently leak into the alt path.
    #[test]
    fn alternate_top_clip_input_is_unchanged_by_primary_floor() {
        // band 0..=1 with one clipped-top row on alt: the reveal-fold folds the clipped-top slice
        // into the first band row, so the band measures the full artifact height (three cells) — this
        // is HEAD's alt output on this input and the primary-only floor leaves it exactly as-is.
        let (clip, art_h, _frame) =
            project_single_live_block(ScreenId::Alternate, 12, 0, 1, 1, 0, 0, 3);
        assert_eq!(clip, art_h);
        assert_eq!(clip, 3 * cell_height().get());
    }

    /// Row-span consistent (no clipped rows) — the ordinary wholly-visible block: the band already
    /// spans exactly the artifact and neither branch reduces it. The fix leaves this untouched (the
    /// 6b906db scaled-preview happy path lands here once its relayout matches), so a matched block
    /// keeps its full clip both before and after.
    #[test]
    fn primary_matched_rowspan_keeps_full_artifact() {
        // band 3..=5 (three rows) with no clipped rows: band == art, no reduction on either side.
        let (clip, art_h, _frame) =
            project_single_live_block(ScreenId::Primary, 12, 3, 5, 0, 0, 0, 3);
        assert_eq!(clip, art_h);
    }

    /// A Codex Markdown heading can style a `# $$` opener and the formula's first body row with
    /// SGR underline. Hiding only the cell text leaves that textless underline as a long rule behind
    /// the transparent math raster. The whole owned band and only the proven ranges of an occluded
    /// source row must become presentation-blank; an application overlay between those ranges keeps
    /// both its glyphs and highlight style.
    #[test]
    fn live_math_occlusion_clears_cell_presentation_without_erasing_overlay() {
        let width = 12usize;
        let rows = 12u32;
        let mut projection = ViewportProjection::new(
            key(width as u32),
            DetectionRevision(1),
            nz32(rows),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        projection.sync_live_math_artifacts(
            ScreenId::Primary,
            [ProjectedLiveMathArtifact {
                occurrence_id: LiveMathOccurrenceId(1),
                screen: ScreenId::Primary,
                start: GridPoint { row: 1, column: 0 },
                end: GridPoint { row: 3, column: 4 },
                band_start_row: 1,
                band_end_row: 3,
                clipped_top_rows: 0,
                clipped_bottom_rows: 0,
                occluded_source_rows: 1,
                occluded_visible_rows: vec![(4, vec![(0, 4), (7, width as u32)])],
                transition_stale: false,
                frozen_prefix: Vec::new(),
                staging_prefix: Vec::new(),
                generation: GridGeneration(1),
                artifact: ProjectedMathArtifact {
                    key: "underlined-heading-formula".to_owned(),
                    end: TranscriptId(0),
                    rgba: Arc::from(vec![255; 54 * 4]),
                    width_px: 1,
                    height_px: 54,
                    height_subpixels: 3 * cell_height().get(),
                    baseline_subpixels: 0,
                    mode: MathMode::Display,
                    kind: RgbaArtifactKind::Math,
                    vertical_padding_subpixels: 0,
                    render_scale_milli: 1000,
                    source: "\\operatorname{Var}(X)".to_owned(),
                },
            }],
        );

        let mut live = vec![CapturedRow::plain(&" ".repeat(width), false); rows as usize];
        for row in &mut live[1..=4] {
            for cell in &mut row.cells {
                cell.style.flags.insert(CellFlags::UNDERLINE);
                cell.style.background = bt_transcript::TerminalColor::Rgb(30, 31, 32);
                cell.hyperlink = Some(CellHyperlink::implicit("https://source.invalid"));
            }
        }
        let overlay_cells = &mut live[4].cells[4..7];
        for (cell, text) in overlay_cells.iter_mut().zip(["J", "M", "P"]) {
            cell.text = text.to_owned();
            cell.style.flags = CellFlags::BOLD;
            cell.style.background = bt_transcript::TerminalColor::Rgb(41, 41, 41);
            cell.hyperlink = None;
        }
        let expected_overlay = overlay_cells.to_vec();

        let frame = projection
            .continuous_frame(
                &HistoryDocument::default(),
                &[],
                live,
                GridCursor {
                    row: rows - 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        let row_cells = |live_row: u32| {
            let frame_row = frame
                .row_map
                .iter()
                .position(|row| row.live_grid_row == Some(live_row))
                .unwrap();
            &frame.cells[frame_row * width..(frame_row + 1) * width]
        };
        for live_row in 1..=3 {
            assert!(
                row_cells(live_row)
                    .iter()
                    .all(|cell| *cell == CapturedCell::default()),
                "rendered band row {live_row} retained terminal presentation"
            );
        }
        assert!(
            row_cells(4)[..4]
                .iter()
                .chain(&row_cells(4)[7..])
                .all(|cell| *cell == CapturedCell::default()),
            "source-proven occlusion ranges retained terminal presentation"
        );
        assert_eq!(
            &row_cells(4)[4..7],
            expected_overlay.as_slice(),
            "the Jump-chip overlay between proven source ranges must stay byte-for-byte intact"
        );
    }

    /// Legal clip 2 — a real bottom-edge run-off: the band ends at the last live row and the block
    /// continues below the grid. The reduced clip is correct and stays exactly as HEAD (two cells),
    /// never floored to the full artifact.
    #[test]
    fn primary_genuine_bottom_edge_clip_is_unchanged() {
        // band 10..=11 (11 is the last live row) with one clipped-bottom row: 2 cells.
        let (clip, _art_h, _frame) =
            project_single_live_block(ScreenId::Primary, 12, 10, 11, 0, 1, 0, 3);
        assert_eq!(clip, 2 * cell_height().get());
    }

    /// Legal clip 3 — cross-boundary occlusion: even with a collapsed band that would otherwise
    /// floor, the presence of occluded source rows keeps HEAD sizing to the subpixel (the reduced
    /// band is deliberate). Same inputs as the collapse test but with an occluded row.
    #[test]
    fn primary_occluded_collapsed_band_stays_head() {
        let (clip, _art_h, _frame) =
            project_single_live_block(ScreenId::Primary, 12, 4, 4, 0, 2, 1, 3);
        assert_eq!(
            clip,
            cell_height().get(),
            "occlusion is a legal clip context and must suppress the floor"
        );
    }

    /// A primary zoom/reflow preview can retain a complete aligned raster after the application has
    /// repainted only one of its six source rows at a new width. The other five rows are inside the
    /// grid but outside the repaint's mutable region, so ordinary occlusion geometry would assign
    /// the band only one sixth of the stale artifact and leave the reported bottom glyph fragment.
    /// A stale preview instead keeps its complete free height, ending at the same band bottom so
    /// following fixed rows remain unmoved. Alternate keeps its established fixed-grid clipping.
    #[test]
    fn primary_stale_mid_grid_occlusion_keeps_the_complete_artifact_clip() {
        let (clip, art_h, frame) = project_single_live_block_with_staleness(
            ScreenId::Primary,
            32,
            11,
            11,
            0,
            5,
            5,
            6,
            true,
        );
        assert_eq!(
            clip, art_h,
            "primary stale reflow preview must not collapse to one source row"
        );
        assert_eq!(
            frame.math_blocks[0].top_subpixels + clip,
            12 * cell_height().get()
        );
    }

    /// Legal clipping stays exact: a fresh application overlay owns the reduced region, and a
    /// stale occurrence genuinely running beyond the terminal bottom has no revealable rows.
    #[test]
    fn fresh_occlusion_and_genuine_stale_bottom_clip_are_unchanged() {
        for screen in [ScreenId::Primary, ScreenId::Alternate] {
            let (fresh_clip, _, _) = project_single_live_block(screen, 32, 11, 11, 0, 5, 5, 6);
            assert_eq!(
                fresh_clip,
                cell_height().get(),
                "{screen:?} fresh occlusion"
            );

            let (edge_clip, _, _) =
                project_single_live_block_with_staleness(screen, 32, 31, 31, 0, 5, 5, 6, true);
            assert_eq!(
                edge_clip,
                cell_height().get(),
                "{screen:?} true terminal-bottom clip"
            );
        }

        let (alternate_stale, _, _) = project_single_live_block_with_staleness(
            ScreenId::Alternate,
            32,
            11,
            11,
            0,
            5,
            5,
            6,
            true,
        );
        assert_eq!(
            alternate_stale,
            cell_height().get(),
            "alternate stale occlusion geometry remains byte-identical"
        );
    }

    /// The floor is primary-only: the identical collapsed inputs leave the alternate screen's
    /// expand-only geometry untouched (its clip stays the single collapsed cell HEAD produced),
    /// while primary floors to the full artifact. This pins that the alternate path is unchanged.
    #[test]
    fn alternate_collapsed_band_is_unchanged_relative_to_primary() {
        let (primary_clip, art_h, _p) =
            project_single_live_block(ScreenId::Primary, 12, 4, 4, 0, 2, 0, 3);
        let (alt_clip, _art_h, _a) =
            project_single_live_block(ScreenId::Alternate, 12, 4, 4, 0, 2, 0, 3);
        assert_eq!(primary_clip, art_h, "primary floors");
        assert_eq!(
            alt_clip,
            cell_height().get(),
            "alternate geometry is untouched by the primary floor"
        );
    }

    /// A `$$…$$` block whose opener finalized while its next exact source row is still staging and
    /// its closer stayed in the live grid renders as ONE block bridging all three planes.
    #[test]
    fn boundary_split_block_renders_as_one_bridge_across_frozen_and_live() {
        let width = 32;
        let mut store = TranscriptStore::new(NonZeroUsize::new(64).unwrap());
        let opener = store
            .capture(CapturedRow::plain("$$", false))
            .finalized
            .remove(0);
        let mut document = HistoryDocument::default();
        document.finalize_transaction(opener);
        let frozen_prefix = document.entries().keys().copied().collect::<Vec<_>>();
        assert_eq!(frozen_prefix.len(), 1);
        let staging_id = StagingId(77);
        let staged_body = format!("{:<width$}", "A=", width = width as usize);
        let staged = [StagedRow {
            id: staging_id,
            row: CapturedRow::plain(&staged_body, true),
        }];

        let mut projection = ViewportProjection::new(
            key(width),
            DetectionRevision(1),
            nz32(12),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        projection.relayout(key(width), &document);
        projection.sync_live_math_artifacts(
            ScreenId::Primary,
            [ProjectedLiveMathArtifact {
                occurrence_id: LiveMathOccurrenceId(9),
                screen: ScreenId::Primary,
                start: GridPoint { row: 0, column: 0 },
                end: GridPoint { row: 0, column: 2 },
                band_start_row: 0,
                band_end_row: 0,
                clipped_top_rows: 0,
                clipped_bottom_rows: 0,
                occluded_source_rows: 0,
                occluded_visible_rows: Vec::new(),
                transition_stale: false,
                frozen_prefix: frozen_prefix.clone(),
                staging_prefix: vec![staging_id],
                generation: GridGeneration(1),
                artifact: ProjectedMathArtifact {
                    key: "sum".to_owned(),
                    end: TranscriptId(0),
                    rgba: Arc::from(vec![255; 40 * 4]),
                    width_px: 1,
                    height_px: 40,
                    height_subpixels: 40 * SUBPIXELS_PER_PX,
                    baseline_subpixels: 0,
                    mode: MathMode::Display,
                    kind: RgbaArtifactKind::Math,
                    vertical_padding_subpixels: 0,
                    render_scale_milli: 1000,
                    source: r"\sum_{k=1}^{n}k=\frac{n(n+1)}{2}".to_owned(),
                },
            }],
        );
        projection.project(&document);
        projection.scroll_to_top();

        let closer = format!("{:<width$}", "$$", width = width as usize);
        let blank = " ".repeat(width as usize);
        let mut live_rows = vec![CapturedRow::plain(&closer, false)];
        live_rows.extend(vec![CapturedRow::plain(&blank, false); 11]);
        let frame = projection
            .continuous_frame(
                &document,
                &staged,
                live_rows,
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        frame.validate_shape().unwrap();

        let bridge = frame
            .math_blocks
            .iter()
            .find(|block| block.display == MathBlockDisplay::Rendered)
            .expect("boundary-split block renders");
        assert_eq!(bridge.frozen_prefix_rows, 2);
        assert_eq!(bridge.live_occurrence_id, Some(LiveMathOccurrenceId(9)));
        assert!(
            matches!(
                bridge.anchor,
                MathBlockAnchor::Live {
                    band_start_row: 0,
                    ..
                }
            ),
            "bridge anchors on the live closer at grid row 0"
        );

        // The block's top rises above the live closer row into the frozen scrollback rows, and its
        // bottom reaches the closer row's bottom: one image spans both domains.
        let closer_row = frame
            .row_map
            .iter()
            .find(|row| row.live_grid_row == Some(0))
            .unwrap();
        let closer_bottom = closer_row
            .top_subpixels
            .saturating_add(closer_row.height_subpixels);
        assert!(
            bridge.top_subpixels < closer_row.top_subpixels,
            "bridge top {} must rise above the closer row top {}",
            bridge.top_subpixels,
            closer_row.top_subpixels
        );
        assert_eq!(
            bridge
                .top_subpixels
                .saturating_add(bridge.clip_height_subpixels),
            closer_bottom,
            "bridge bottom must reach the live closer row bottom"
        );

        // No prefix source survives: the finalized opener, staging body and live closer are all
        // suppressed by the single rendered block.
        let column_count = width as usize;
        for (row, cells) in frame.cells.chunks(column_count).enumerate() {
            let text = cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>();
            assert!(
                !text.contains("$$") && !text.contains("A="),
                "row {row} still shows split-block source: {text:?}"
            );
        }

        let wrong_staged = [StagedRow {
            id: StagingId(78),
            row: CapturedRow::plain(&staged_body, true),
        }];
        projection.scroll_to_top();
        let unproven = projection
            .continuous_frame(
                &document,
                &wrong_staged,
                vec![CapturedRow::plain(&closer, false)]
                    .into_iter()
                    .chain(vec![CapturedRow::plain(&blank, false); 11])
                    .collect(),
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .unwrap();
        assert!(
            unproven.cells.chunks(column_count).any(|cells| cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("A=")),
            "a mismatched staging row must remain ordinary text rather than being guessed into the bridge"
        );
        assert!(
            !unproven.cells.chunks(column_count).any(|cells| cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("$$")),
            "an unrelated staged row must not stop the exact frozen source prefix from being swallowed"
        );
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
            transition_stale: false,
            frozen_prefix: Vec::new(),
            staging_prefix: Vec::new(),
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
                kind: RgbaArtifactKind::Math,
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
                transition_stale: false,
                frozen_prefix: Vec::new(),
                staging_prefix: Vec::new(),
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
                    kind: RgbaArtifactKind::Math,
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
        let last = bottom
            .row_map
            .iter()
            .find(|row| row.live_grid_row == Some(bottom.grid_rows.get() - 1))
            .expect("the fixed bottom live row remains presented");
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
        projection.scroll_offset_subpixels = cell_height().get();
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
                expected: 6,
                actual: 5,
            })
        );

        frame.cells.push(CapturedCell::default());
        frame.cell_anchors.pop();
        assert_eq!(
            frame.line_selection(0),
            Err(FrameShapeError::AnchorCount {
                expected: 6,
                actual: 5,
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

        frame.selection_spans[0].row = 3;
        assert_eq!(
            frame.validate_shape(),
            Err(FrameShapeError::SelectionSpanRowOutOfBounds { row: 3, rows: 3 })
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
            kind: RgbaArtifactKind::Math,
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

    fn assert_inline_path_virtual_rows_are_reviewable(
        offsets: std::ops::RangeInclusive<i32>,
        host_index: usize,
    ) {
        let mut store = TranscriptStore::new(NonZeroUsize::new(64).unwrap());
        let mut document = HistoryDocument::default();
        let mut ids = Vec::new();
        for index in 0..50 {
            let finalized = store
                .capture(CapturedRow::plain(&format!("line-{index:02}"), false))
                .finalized
                .remove(0);
            ids.push(finalized.line.id);
            document.finalize_transaction(finalized);
        }

        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(12),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.sync_inline_path_artifacts([13_usize, 21, 29].map(|index| {
            (
                ids[index],
                ProjectedMathArtifact {
                    key: format!("path-image-{index}"),
                    end: ids[index],
                    rgba: Arc::from(vec![255; 4]),
                    width_px: 1,
                    height_px: 1,
                    height_subpixels: 4 * cell_height().get(),
                    baseline_subpixels: 0,
                    mode: MathMode::Inline,
                    kind: RgbaArtifactKind::LocalImagePath { animated: false },
                    vertical_padding_subpixels: 0,
                    render_scale_milli: 1000,
                    source: format!("image-{index}.png"),
                },
            )
        }));
        projection.project(&document);

        let live = || vec![CapturedRow::plain("        ", false); 12];
        let cursor = GridCursor {
            row: 11,
            column: 0,
            visible: true,
        };
        projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();

        let interval_end = *offsets.end();
        for offset in offsets {
            projection.scroll_to_bottom();
            projection.scroll_by_rows(offset);
            let frame = projection
                .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
                .unwrap();
            assert_eq!(
                frame.scroll_offset_rows, offset as usize,
                "path-image virtual row at review offset {offset} must retain an anchored viewport"
            );
            let FrameViewportOrigin::Anchored(ScrollAnchor {
                source:
                    ContentAnchor::History {
                        id,
                        offset: source_offset,
                        bias,
                        ..
                    },
                local_offset,
            }) = &frame.viewport_origin
            else {
                panic!(
                    "path-image virtual row at review offset {offset} must use its host history line"
                );
            };
            assert_eq!(*id, ids[host_index]);
            assert_eq!(*source_offset, GraphemeOffset(7));
            assert_eq!(*bias, Bias::After);
            assert_eq!(
                *local_offset,
                i64::from(interval_end - offset + 1).saturating_mul(cell_height().get()),
                "local_offset must encode the virtual row after the host line's end anchor"
            );
        }
    }

    #[test]
    fn first_inline_path_virtual_row_interval_is_reviewable() {
        assert_inline_path_virtual_rows_are_reviewable(21..=24, 29);
    }

    #[test]
    fn second_inline_path_virtual_row_interval_is_reviewable() {
        assert_inline_path_virtual_rows_are_reviewable(33..=36, 21);
    }

    #[test]
    fn third_inline_path_virtual_row_interval_is_reviewable() {
        assert_inline_path_virtual_rows_are_reviewable(45..=48, 13);
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

    // Isolated state-machine coverage for the presentation frame hold. `set_resize_reflow_active`
    // is the session's per-frame signal that a resize transaction is open; the projection only
    // holds a vanished review anchor while that signal is set, and releases the frame the
    // displacement re-anchors.
    fn history_of(store: &mut TranscriptStore, count: usize) -> HistoryDocument {
        let mut document = HistoryDocument::default();
        for index in 0..count {
            document.finalize_transaction(
                store
                    .capture(CapturedRow::plain(&format!("line-{index:03}"), false))
                    .finalized
                    .remove(0),
            );
        }
        document
    }

    fn six_blank_live() -> Vec<CapturedRow> {
        vec![CapturedRow::plain("         ", false); 6]
    }

    fn reviewing_projection(store: &mut TranscriptStore) -> (ViewportProjection, HistoryDocument) {
        let document = history_of(store, 40);
        let mut projection = ViewportProjection::new(
            key(9),
            DetectionRevision(1),
            nz32(6),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        let cursor = GridCursor {
            row: 0,
            column: 0,
            visible: true,
        };
        projection.project(&document);
        projection
            .continuous_frame(&document, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        projection.scroll_by_rows(20);
        projection.project(&document);
        let reviewing = projection
            .continuous_frame(&document, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_eq!(reviewing.scroll_offset_rows, 20);
        assert!(!projection.review_hold());
        (projection, document)
    }

    #[test]
    fn review_hold_engages_under_a_resize_signal_and_releases_when_the_reprint_re_anchors() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(256).unwrap());
        let (mut projection, _document) = reviewing_projection(&mut store);
        let cursor = GridCursor {
            row: 0,
            column: 0,
            visible: true,
        };

        // A resize transaction is open; the reflow clears history under the anchored reader.
        projection.set_resize_reflow_active(true);
        let cleared = HistoryDocument::default();
        projection.project(&cleared);
        let empty = projection
            .continuous_frame(&cleared, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_eq!(
            empty.scroll_offset_rows, 0,
            "with history empty the frame can only sit at the bottom"
        );
        assert!(
            projection.review_hold(),
            "the resize signal holds the last frame across the empty window"
        );

        // The reprint refills equivalent history; the displacement re-anchors and the hold clears.
        let reprinted = history_of(&mut store, 40);
        projection.project(&reprinted);
        let restored = projection
            .continuous_frame(&reprinted, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_eq!(restored.scroll_offset_rows, 20);
        assert!(
            !projection.review_hold(),
            "re-anchoring the full displacement ends the hold"
        );
    }

    #[test]
    fn review_hold_stays_off_for_a_vanished_anchor_with_no_resize_signal() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(256).unwrap());
        let (mut projection, _document) = reviewing_projection(&mut store);
        let cursor = GridCursor {
            row: 0,
            column: 0,
            visible: true,
        };

        // No resize signal: this is a user clear, so the vanished anchor snaps to the empty bottom
        // and is never held, even though the numeric displacement is still preserved (a66eb84).
        assert!(!projection.review_hold());
        let cleared = HistoryDocument::default();
        projection.project(&cleared);
        let empty = projection
            .continuous_frame(&cleared, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_eq!(empty.scroll_offset_rows, 0);
        assert!(
            !projection.review_hold(),
            "a vanished anchor with no open resize transaction never holds"
        );
    }

    #[test]
    fn exact_source_reprint_hold_participates_in_the_combined_presentation_hold() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(256).unwrap());
        let (mut projection, _document) = reviewing_projection(&mut store);

        projection.scroll_to_bottom();
        assert!(!projection.review_hold());
        assert!(!projection.exact_source_reprint_hold());
        assert!(!projection.presentation_hold());

        projection.set_exact_source_reprint_hold(true);
        assert!(projection.exact_source_reprint_hold());
        assert!(
            projection.presentation_hold(),
            "an unmatched exact-source decoration holds even at bottom follow"
        );
        assert!(
            !projection.review_hold(),
            "the decoration reason remains independent of review displacement"
        );

        projection.set_exact_source_reprint_hold(false);
        assert!(!projection.exact_source_reprint_hold());
        assert!(
            !projection.presentation_hold(),
            "the session's deterministic release removes the combined hold"
        );
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

    fn history_row_top(frame: &ViewportFrame, id: TranscriptId) -> i64 {
        let columns = frame.columns.get() as usize;
        frame
            .cell_anchors
            .chunks(columns)
            .zip(&frame.row_map)
            .find_map(|(anchors, mapped)| {
                anchors
                    .iter()
                    .any(|anchor| {
                        matches!(
                            anchor.start,
                            ContentAnchor::History {
                                id: anchor_id,
                                ..
                            } if anchor_id == id
                        )
                    })
                    .then_some(mapped.top_subpixels)
            })
            .expect("north-star content anchor remains in the presentation rows")
    }

    fn assert_constant_subpixel_steps_move_content_exactly(mixed_artifact: bool) {
        let mut store = TranscriptStore::new(NonZeroUsize::new(32).unwrap());
        let mut document = HistoryDocument::default();
        let mut ids = Vec::new();
        for index in 0..12 {
            let finalized = store
                .capture(CapturedRow::plain(&format!("line-{index:02}"), false))
                .finalized
                .remove(0);
            ids.push(finalized.line.id);
            document.finalize_transaction(finalized);
        }
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(4),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        let target = if mixed_artifact {
            let artifact_height = cell_height().get() * 5 / 2;
            projection.sync_math_artifacts([(
                ids[3],
                ProjectedMathArtifact {
                    key: "north-star-math".to_owned(),
                    end: ids[3],
                    rgba: Arc::from(vec![255; 4]),
                    width_px: 1,
                    height_px: 1,
                    height_subpixels: artifact_height,
                    baseline_subpixels: 0,
                    mode: MathMode::Display,
                    kind: RgbaArtifactKind::Math,
                    vertical_padding_subpixels: 0,
                    render_scale_milli: 1000,
                    source: "x^2".to_owned(),
                },
            )]);
            ids[5]
        } else {
            ids[7]
        };
        projection.project(&document);
        let live = || vec![CapturedRow::plain("        ", false); 4];
        let cursor = GridCursor {
            row: 3,
            column: 0,
            visible: true,
        };
        projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        projection.scroll_by_rows(if mixed_artifact { 8 } else { 6 });
        let mut frame = projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        let step = cell_height().get() / 4;
        let mut previous = history_row_top(&frame, target);
        for sample in 1..=3 {
            projection.scroll_by_subpixels(step);
            frame = projection
                .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
                .unwrap();
            let actual = history_row_top(&frame, target);
            assert_eq!(
                actual - previous,
                step,
                "north-star sample {sample}: every content anchor must move by the exact input step"
            );
            previous = actual;
        }
    }

    #[test]
    fn north_star_plain_text_content_moves_by_every_exact_subpixel_step() {
        assert_constant_subpixel_steps_move_content_exactly(false);
    }

    #[test]
    fn north_star_mixed_artifact_content_moves_by_every_exact_subpixel_step() {
        assert_constant_subpixel_steps_move_content_exactly(true);
    }

    fn last_live_bottom(frame: &ViewportFrame) -> i64 {
        let last = frame.grid_rows.get() - 1;
        let row = frame
            .row_map
            .iter()
            .find(|row| row.live_grid_row == Some(last))
            .expect("the last live row remains in the presentation list");
        row.top_subpixels.saturating_add(row.height_subpixels)
    }

    fn live_math_for_bottom_follow(occurrence_id: u64, cell: i64) -> ProjectedLiveMathArtifact {
        ProjectedLiveMathArtifact {
            occurrence_id: LiveMathOccurrenceId(occurrence_id),
            screen: ScreenId::Primary,
            start: GridPoint { row: 3, column: 0 },
            end: GridPoint { row: 4, column: 7 },
            band_start_row: 3,
            band_end_row: 4,
            clipped_top_rows: 0,
            clipped_bottom_rows: 0,
            occluded_source_rows: 0,
            occluded_visible_rows: Vec::new(),
            transition_stale: false,
            frozen_prefix: Vec::new(),
            staging_prefix: Vec::new(),
            generation: GridGeneration(1),
            artifact: ProjectedMathArtifact {
                key: format!("bottom-follow-{occurrence_id}"),
                end: TranscriptId(0),
                rgba: Arc::from(vec![255; 4]),
                width_px: 1,
                height_px: 1,
                height_subpixels: cell.saturating_mul(3),
                baseline_subpixels: 0,
                mode: MathMode::Display,
                kind: RgbaArtifactKind::Math,
                vertical_padding_subpixels: 0,
                render_scale_milli: 1000,
                source: "bottom-follow".to_owned(),
            },
        }
    }

    #[test]
    fn bottom_follow_keeps_the_last_pixel_flush_through_async_artifact_and_zoom() {
        let document = HistoryDocument::default();
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(12),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let live = || vec![CapturedRow::plain("        ", false); 12];
        let cursor = GridCursor {
            row: 11,
            column: 0,
            visible: true,
        };
        let plain = projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_eq!(last_live_bottom(&plain), 12 * cell_height().get());

        projection.sync_live_math_artifacts(
            ScreenId::Primary,
            [live_math_for_bottom_follow(1, cell_height().get())],
        );
        let decorated = projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_eq!(last_live_bottom(&decorated), 12 * cell_height().get());
        assert_eq!(projection.scroll_offset_subpixels(), 0);

        let zoomed_cell = NonZeroI64::new(cell_height().get() * 2).unwrap();
        projection.set_cell_height_subpixels(zoomed_cell);
        projection.sync_live_math_artifacts(
            ScreenId::Primary,
            [live_math_for_bottom_follow(2, zoomed_cell.get())],
        );
        let zoomed = projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_eq!(last_live_bottom(&zoomed), 12 * zoomed_cell.get());
        assert_eq!(projection.scroll_offset_subpixels(), 0);
    }

    #[test]
    fn bottom_replay_geometry_ignores_fractional_history_artifact_height() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(32).unwrap());
        let mut document = HistoryDocument::default();
        let mut ids = Vec::new();
        for index in 0..8 {
            let finalized = store
                .capture(CapturedRow::plain(&format!("line-{index:02}"), false))
                .finalized
                .remove(0);
            ids.push(finalized.line.id);
            document.finalize_transaction(finalized);
        }
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(4),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        let live = || vec![CapturedRow::plain("        ", false); 4];
        let cursor = GridCursor {
            row: 3,
            column: 0,
            visible: true,
        };
        let artifact = |height_subpixels| ProjectedMathArtifact {
            key: format!("bottom-history-{height_subpixels}"),
            end: ids[2],
            rgba: Arc::from(vec![255; 4]),
            width_px: 1,
            height_px: 1,
            height_subpixels,
            baseline_subpixels: 0,
            mode: MathMode::Display,
            kind: RgbaArtifactKind::Math,
            vertical_padding_subpixels: 0,
            render_scale_milli: 1000,
            source: "fractional-history-height".to_owned(),
        };
        let assert_row_identity = |frame: &ViewportFrame| {
            assert_eq!(frame.viewport_origin, FrameViewportOrigin::Bottom);
            assert_eq!(frame.presentation_offset_subpixels, 0);
            assert_eq!(frame.drawable_rows(), frame.grid_rows.get() as usize);
            assert_eq!(frame.row_map[0].top_subpixels, 0);
            assert_eq!(frame.row_map[0].live_grid_row, Some(0));
        };

        projection.project(&document);
        let plain = projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_row_identity(&plain);

        for height in [cell_height().get() * 5 / 2, cell_height().get() * 7 / 3] {
            projection.sync_math_artifacts([(ids[2], artifact(height))]);
            projection.project(&document);
            let ready = projection
                .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
                .unwrap();
            assert_row_identity(&ready);
            assert_eq!(projection.scroll_offset_subpixels(), 0);
        }
    }

    #[test]
    fn review_anchor_keeps_exact_y_through_append_and_artifact_height_change() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(64).unwrap());
        let mut document = HistoryDocument::default();
        let mut ids = Vec::new();
        for index in 0..16 {
            let finalized = store
                .capture(CapturedRow::plain(&format!("line-{index:02}"), false))
                .finalized
                .remove(0);
            ids.push(finalized.line.id);
            document.finalize_transaction(finalized);
        }
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(4),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.project(&document);
        let live = || vec![CapturedRow::plain("        ", false); 4];
        let cursor = GridCursor {
            row: 3,
            column: 0,
            visible: true,
        };
        projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        projection.scroll_by_subpixels(5 * cell_height().get() + cell_height().get() / 3);
        let anchored = projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        let target = ids[12];
        let target_y = history_row_top(&anchored, target);
        let exact_local = projection.scroll_anchor().unwrap().local_offset;
        assert_ne!(exact_local, 0);

        document.finalize_transaction(
            store
                .capture(CapturedRow::plain("appended", false))
                .finalized
                .remove(0),
        );
        projection.project(&document);
        let appended = projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_eq!(history_row_top(&appended, target), target_y);
        assert_eq!(
            projection.scroll_anchor().unwrap().local_offset,
            exact_local
        );

        projection.sync_math_artifacts([(
            ids[2],
            ProjectedMathArtifact {
                key: "async-history-height".to_owned(),
                end: ids[2],
                rgba: Arc::from(vec![255; 4]),
                width_px: 1,
                height_px: 1,
                height_subpixels: cell_height().get() * 5 / 2,
                baseline_subpixels: 0,
                mode: MathMode::Display,
                kind: RgbaArtifactKind::Math,
                vertical_padding_subpixels: 0,
                render_scale_milli: 1000,
                source: "height-change".to_owned(),
            },
        )]);
        projection.project(&document);
        let resized_artifact = projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        assert_eq!(history_row_top(&resized_artifact, target), target_y);
        assert_eq!(
            projection.scroll_anchor().unwrap().local_offset,
            exact_local
        );
    }

    #[test]
    fn resize_reflow_hold_restores_the_exact_subpixel_displacement() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(256).unwrap());
        let document = history_of(&mut store, 40);
        let mut projection = ViewportProjection::new(
            key(9),
            DetectionRevision(1),
            nz32(6),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        let cursor = GridCursor {
            row: 0,
            column: 0,
            visible: true,
        };
        projection.project(&document);
        projection
            .continuous_frame(&document, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        let exact = 20 * cell_height().get() + cell_height().get() / 3;
        projection.scroll_by_subpixels(exact);
        let before = projection
            .continuous_frame(&document, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        let local_before = projection.scroll_anchor().unwrap().local_offset;
        assert_ne!(before.presentation_offset_subpixels, 0);

        projection.set_resize_reflow_active(true);
        let cleared = HistoryDocument::default();
        projection.project(&cleared);
        projection
            .continuous_frame(&cleared, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        assert!(projection.review_hold());

        let reprinted = history_of(&mut store, 40);
        projection.project(&reprinted);
        let restored = projection
            .continuous_frame(&reprinted, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        assert!(!projection.review_hold());
        assert_eq!(projection.scroll_offset_subpixels(), exact);
        assert_eq!(
            projection.scroll_anchor().unwrap().local_offset,
            local_before
        );
        assert_eq!(
            restored.presentation_offset_subpixels,
            before.presentation_offset_subpixels
        );
    }

    #[test]
    fn alternate_sticky_review_uses_exact_pixel_capacity_and_exits_only_at_zero() {
        let cell = cell_height().get();
        let mut projection = ViewportProjection::new(
            key(8),
            DetectionRevision(1),
            nz32(6),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let mut artifact = live_math_for_bottom_follow(9, cell);
        artifact.screen = ScreenId::Alternate;
        artifact.start.row = 2;
        artifact.end.row = 2;
        artifact.band_start_row = 2;
        artifact.band_end_row = 2;
        artifact.artifact.height_subpixels = cell + cell / 3;
        projection.sync_live_math_artifacts(ScreenId::Alternate, [artifact]);
        let live = || vec![CapturedRow::plain("        ", false); 6];
        let cursor = GridCursor {
            row: 5,
            column: 0,
            visible: true,
        };
        projection
            .continuous_frame(
                &HistoryDocument::default(),
                &[],
                live(),
                cursor,
                ScreenId::Alternate,
            )
            .unwrap();

        projection.scroll_by_subpixels(i64::MAX);
        assert_eq!(projection.scroll_offset_subpixels(), cell / 3);
        projection.scroll_by_subpixels(-(cell / 3 - 1));
        assert_eq!(projection.scroll_offset_subpixels(), 1);
        assert!(projection.is_scrolled());
        projection.scroll_by_subpixels(-1);
        assert_eq!(projection.scroll_offset_subpixels(), 0);
        assert!(!projection.is_scrolled());
    }

    #[test]
    fn partial_first_and_overscan_rows_share_exact_selection_hit_and_cursor_geometry() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(16).unwrap());
        let mut document = HistoryDocument::default();
        let mut ids = Vec::new();
        for text in ["h0", "h1", "h2", "h3"] {
            let finalized = store
                .capture(CapturedRow::plain(text, false))
                .finalized
                .remove(0);
            ids.push(finalized.line.id);
            document.finalize_transaction(finalized);
        }
        let mut projection = ViewportProjection::new(
            key(2),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.project(&document);
        let live = || {
            vec![
                CapturedRow::plain("l0", false),
                CapturedRow::plain("l1", false),
            ]
        };
        let cursor = GridCursor {
            row: 0,
            column: 1,
            visible: true,
        };
        projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        projection.scroll_by_subpixels(cell_height().get() + cell_height().get() / 2);
        projection.set_selection(Some(ViewSelection {
            start: ContentAnchor::History {
                id: ids[2],
                offset: GraphemeOffset(0),
                bias: Bias::Before,
                generation: store.source_generation(),
            },
            end: ContentAnchor::Live {
                screen: ScreenId::Primary,
                point: GridPoint { row: 1, column: 1 },
                bias: Bias::After,
                generation: GridGeneration(1),
            },
        }));
        let frame = projection
            .continuous_frame(&document, &[], live(), cursor, ScreenId::Primary)
            .unwrap();
        let cell = cell_height().get();
        assert_eq!(frame.presentation_offset_subpixels, cell / 2);
        assert_eq!(frame.row_map[0].top_subpixels, -(cell / 2));
        assert_eq!(frame.visual_row_at(-1), None);
        assert_eq!(frame.visual_row_at(0), Some(0));
        assert_eq!(frame.visual_row_at(cell / 2 - 1), Some(0));
        assert_eq!(frame.visual_row_at(cell / 2), Some(1));
        assert_eq!(frame.visual_row_at(2 * cell - 1), Some(2));
        assert!(
            frame.selection_spans.iter().any(|span| span.row == 0)
                && frame.selection_spans.iter().any(|span| span.row == 2)
        );
        assert_eq!(frame.cursor.row, 2);
        assert!(frame.cursor.visible);
    }

    #[test]
    fn subpixel_motion_does_not_invalidate_layout_or_row_materialization_cache() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(32).unwrap());
        let document = history_of(&mut store, 20);
        let mut projection = ViewportProjection::new(
            key(9),
            DetectionRevision(1),
            nz32(6),
            cell_height(),
            store.source_generation(),
            GridGeneration(1),
        );
        projection.project(&document);
        let misses = projection.cache_misses();
        let cached = projection.cache_len();
        let cursor = GridCursor {
            row: 0,
            column: 0,
            visible: true,
        };
        projection
            .continuous_frame(&document, &[], six_blank_live(), cursor, ScreenId::Primary)
            .unwrap();
        for _ in 0..32 {
            projection.scroll_by_subpixels(cell_height().get() / 7);
            projection
                .continuous_frame(&document, &[], six_blank_live(), cursor, ScreenId::Primary)
                .unwrap();
        }
        assert_eq!(projection.cache_misses(), misses);
        assert_eq!(projection.cache_len(), cached);
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
                    cells: frame.cells[..frame.columns.get() as usize].to_vec(),
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
