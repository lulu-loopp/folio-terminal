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

pub use bt_doc::{InlineRunPlacement, SUBPIXELS_PER_PX};
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

/// Which line of which plane one search hit sits on (§7.1.5d, S3).
///
/// Three arms because the three planes address their text three different ways, and a hit has to
/// be able to name a place in any of them: frozen history and the staged rows that have scrolled
/// out but not yet been finalized both count **graphemes inside one line**, while the live grid
/// counts **columns inside one row** — which is exactly what their [`ContentAnchor`]s carry. R7 in
/// the search-block inventory is the reason all three are here rather than only the first: "the
/// word is on my screen and search cannot find it" is the failure this vocabulary exists to
/// prevent, and two thirds of what is on the screen at any moment is not in the transcript yet.
///
/// The alternate screen is deliberately absent. §3.2 keeps its anchors in an isolated namespace
/// with no ordering against the primary document, so a hit there could not be placed in the same
/// list as the rest; the capsule does not open over it at all (D-5).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SearchLine {
    History(TranscriptId),
    Staging(StagingId),
    /// One row of the primary live grid, by its grid row index.
    Live {
        row: u32,
    },
}

/// One hit: a line, and a half-open offset range in **that line's own unit** — graphemes for the
/// two transcript planes, columns for the live grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchHit {
    pub line: SearchLine,
    pub start: u32,
    pub end: u32,
}

/// Every hit the capsule is showing, in the shape a frame can be painted from.
///
/// # Why this is grouped and sorted rather than a flat list
///
/// The projection has to answer, for each of a few thousand cells, "is this cell inside a hit" —
/// once per frame, while the transcript underneath may hold a hundred thousand of them. A flat
/// scan would make that product; grouping by line and sorting both levels makes it two binary
/// searches per cell, which is a cost in the *frame's* size and not in the transcript's. The
/// grouping is paid once, where the matches are found, and travels here by [`Arc`] so that a frame
/// which changed nothing about the search re-uses it untouched.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchHighlights {
    /// `(line, ranges)` with lines ascending and ranges ascending inside a line.
    lines: Vec<(SearchLine, Vec<(u32, u32)>)>,
    /// Which hit wears the current ground, if the capsule has one.
    current: Option<SearchHit>,
}

impl SearchHighlights {
    /// Group and sort a hit list. Ranges that arrive out of order are sorted; the engine produces
    /// them left to right per line already, so this is a guarantee rather than work.
    #[must_use]
    pub fn new(hits: impl IntoIterator<Item = SearchHit>, current: Option<SearchHit>) -> Self {
        let mut lines: Vec<(SearchLine, Vec<(u32, u32)>)> = Vec::new();
        for hit in hits {
            match lines.last_mut() {
                Some((line, ranges)) if *line == hit.line => ranges.push((hit.start, hit.end)),
                _ => lines.push((hit.line, vec![(hit.start, hit.end)])),
            }
        }
        lines.sort_by_key(|(line, _)| *line);
        for (_, ranges) in &mut lines {
            ranges.sort_unstable();
        }
        Self { lines, current }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Whether any hit covers this offset on this line.
    fn covers(&self, line: SearchLine, offset: u32) -> bool {
        let Ok(index) = self.lines.binary_search_by_key(&line, |(line, _)| *line) else {
            return false;
        };
        let ranges = &self.lines[index].1;
        // The last range that starts at or before `offset`; a hit covers `offset` only if that one
        // does, because the ranges do not overlap (the engine's own non-overlapping semantics).
        let at = ranges.partition_point(|(start, _)| *start <= offset);
        at > 0 && offset < ranges[at - 1].1
    }

    /// Whether the **current** hit covers this offset on this line.
    fn current_covers(&self, line: SearchLine, offset: u32) -> bool {
        self.current
            .is_some_and(|hit| hit.line == line && hit.start <= offset && offset < hit.end)
    }
}

/// Where a cell anchor stands, in the vocabulary a hit is written in.
///
/// The generation is deliberately dropped, exactly as [`compare_visible_anchors`] drops it for the
/// selection: a hit set is rebuilt on every transcript change, so an anchor whose line has been
/// rewritten under it is at most one frame old, and comparing generations here would blank the
/// highlight for that frame rather than repaint it.
///
/// **Public because S4 needs the same translation for a different reason.** When the search is open
/// the command rail merges the command ledger into the hit set, and a merge needs both sides in one
/// line space: a command mark carries a [`ContentAnchor`] and a hit carries a [`SearchLine`], and
/// this is the function that already turns the first into the second. A second copy of it beside
/// the rail would be a second opinion about where a line is, and the two would drift the day the
/// live grid grows a plane.
#[must_use]
pub fn search_address(anchor: &ContentAnchor) -> Option<(SearchLine, u32)> {
    match anchor {
        ContentAnchor::History { id, offset, .. } => Some((SearchLine::History(*id), offset.0)),
        ContentAnchor::Staging { id, offset, .. } => Some((SearchLine::Staging(*id), offset.0)),
        ContentAnchor::Live {
            screen: ScreenId::Primary,
            point,
            ..
        } => Some((SearchLine::Live { row: point.row }, point.column)),
        ContentAnchor::Live {
            screen: ScreenId::Alternate,
            ..
        } => None,
    }
}

/// Geometry and input identity for one row in the last presented frame. Pixel consumers use the
/// prefix position here instead of independently multiplying the frame row by cell height.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameVisualRow {
    pub top_subpixels: i64,
    pub height_subpixels: i64,
    pub live_grid_row: Option<u32>,
    /// True when this presented row soft-wraps into the next one — the same fact
    /// `CapturedRow::continues` carries on the live grid and `layout_frozen_line`
    /// knows by construction on the frozen plane, brought through to the frame
    /// because it is the only thing that can tell a wrapped link from two links.
    ///
    /// **Carried rather than inferred.** Two cells on adjacent rows look exactly
    /// alike whether one line wrapped or two lines were printed; every rule that
    /// tried to guess (the earlier row reaching the last column, the cells
    /// between being blank) is a guess that reads a list of identical links as
    /// one link. This is the answer the terminal already had.
    pub continues: bool,
}

/// Byte budget of the GPU texture cache every projected artifact competes in.
///
/// It lives with the artifact type rather than inside one renderer because it is a property of the
/// artifacts: a raster larger than this can never become a texture no matter who is asked to upload
/// it, so producers and auditors need the same number the uploader uses.
pub const MATH_TEXTURE_CACHE_BUDGET_BYTES: usize = 64 * 1024 * 1024;

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
    /// For an inline composite: which of the line's `$…$` runs this image contains and where they
    /// sit inside it, in the same raster-pixel space as `width_px` — so a hit test scales them by
    /// `render_scale_milli` exactly as it scales the block. Empty for display math.
    pub inline_runs: Vec<InlineRunPlacement>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RgbaArtifactKind {
    Math,
    /// A rendered GFM pipe table. It carries no pixels of its own: the block's picture is drawn
    /// with the window's own text and fills, at terminal metrics, by whoever owns the paint —
    /// which is why the `rgba` slot of such an artifact is empty and no texture is ever uploaded
    /// for it. Everything else on the placement (the owned rows, the clip, the height cap, the
    /// interior scroll, the source text) means exactly what it means for a formula.
    Table,
    InlineImage {
        animated: bool,
    },
    LocalImagePath {
        animated: bool,
    },
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

/// What a math interaction is aimed at.
///
/// `run` names one inline `$…$` run of the anchored line, and is what makes a line carrying two
/// formulas answer two different questions. A placement's own anchor leaves it `None` — the
/// placement is the whole line's composite — while a hit test fills it in from the pointer, so a
/// copy resolves to the formula under the cursor rather than to every formula on the row joined by
/// a separator. Display math has no runs and is always `None`.
///
/// Every record lookup keyed on an anchor deliberately ignores it: the run selects *within* a
/// record, it never selects a record.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MathBlockAnchor {
    History {
        start: TranscriptId,
        end: TranscriptId,
        run: Option<u32>,
    },
    Live {
        screen: ScreenId,
        start: GridPoint,
        end: GridPoint,
        band_start_row: u32,
        band_end_row: u32,
        generation: GridGeneration,
        run: Option<u32>,
    },
}

impl MathBlockAnchor {
    /// The same anchor pointed at one inline run of its line.
    pub fn with_run(&self, run: Option<u32>) -> Self {
        let mut anchor = self.clone();
        match &mut anchor {
            Self::History { run: slot, .. } | Self::Live { run: slot, .. } => *slot = run,
        }
        anchor
    }

    pub fn run(&self) -> Option<u32> {
        match self {
            Self::History { run, .. } | Self::Live { run, .. } => *run,
        }
    }
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
    /// Every search hit on screen **except** the current one — the `mark.srch` ground (mock 1530).
    ///
    /// A second list beside the selection's rather than a flag on the first, because they are two
    /// different marks that can cover the same cell at the same time: a reader can select text
    /// that a search has also found, and one list with a colour on it could only say one of the
    /// two. They are painted in the order they are declared here.
    pub search_spans: Vec<SelectionSpan>,
    /// The current hit — `mark.srch.cur` (mock 1532), which wears the solid accent and takes the
    /// terminal's own background as its ink.
    ///
    /// A list and not one span, because a hit lives in a logical line and a logical line wraps: the
    /// current match can straddle two presentation rows, and the second half of it is as current as
    /// the first.
    pub current_search_spans: Vec<SelectionSpan>,
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
        for span in self
            .selection_spans
            .iter()
            .chain(&self.search_spans)
            .chain(&self.current_search_spans)
        {
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

    /// The visual row a gesture at `y_subpixels` *means*, with points above the
    /// first drawable row and below the last one pulled to those rows.
    ///
    /// [`Self::visual_row_at`] answers "which row is under this point", and its
    /// `None` is the right answer for a hover: nothing is being pointed at. A
    /// drag that owns the pointer asks a different question — "which row does
    /// this gesture mean" — and past the bottom of the frame the answer is the
    /// last row, which is what dragging past the end of a pane has always
    /// selected. `None` here means the frame draws no rows at all, so there is
    /// no row for any answer to name.
    pub fn clamped_visual_row_at(&self, y_subpixels: i64) -> Option<u32> {
        let last = u32::try_from(self.drawable_rows().checked_sub(1)?).ok()?;
        if let Some(row) = self.visual_row_at(y_subpixels) {
            return Some(row);
        }
        if y_subpixels < self.row_map.first()?.top_subpixels {
            return Some(0);
        }
        Some(last)
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
        // Explicit OSC 8 and inferred bare URLs read the same way: the link is the segment of
        // same-target cells the pointer is standing on. See [`Self::link_span`].
        let (first, last) = self.link_span(index)?;
        Some(HyperlinkHit {
            uri: link.uri.clone(),
            // The **segment's** id, read off its first cell rather than off whichever cell the
            // pointer happens to be on. A link the application broke across a line arrives as two
            // emissions with two synthesized ids, and one link must be one hit however it is
            // pointed at — the app compares whole hits to decide that the pointer is still on the
            // same thing.
            id: self.cells.get(first)?.hyperlink.as_ref()?.id.clone(),
            start: self.cell_anchors.get(first)?.start.clone(),
            end: self.cell_anchors.get(last)?.end.clone(),
        })
    }

    /// The cells one link occupies on screen around a hit cell, as a pair of flat cell indices.
    ///
    /// This is [`Self::link_group_run`] — the geometrically continuous run — plus the one thing a
    /// run cannot see: **a line break the application made itself**. See
    /// [`Self::rejoined_across_break`].
    fn link_span(&self, index: usize) -> Option<(usize, usize)> {
        let link = self.cells.get(index)?.hyperlink.as_ref()?;
        let run = self.link_group_run(index, link)?;
        Some(self.rejoined_across_break(run, &link.uri).unwrap_or(run))
    }

    /// One id-group's **geometrically continuous run** around a hit cell, as a pair of flat
    /// cell indices.
    ///
    /// # Why a run and not the whole group
    ///
    /// OSC 8's convention is that cells sharing an id are one link, and the convention was
    /// written for the case it is named for: a link that soft-wraps arrives as two runs with
    /// layout indent between them, and only the id can say they are one thing. Claude Code
    /// stamps the same id on **every occurrence of the same URL in its output**, which is
    /// within the letter of the spec and turns hovering one file path into every copy of that
    /// path on the screen lighting at once (user report 2026-08-18).
    ///
    /// So the grouping is narrowed, deliberately and only here: what lights is the run the
    /// pointer is actually on. The id still governs everything else — it is what joins the two
    /// halves of a wrapped link across the indent, it is carried on the hit, and activation
    /// reads the hit's `uri` exactly as before. What changes is which cells wear the solid
    /// underline, which is a statement about *this* occurrence and was never a statement the
    /// other occurrences had a claim on.
    ///
    /// # What "continuous" means
    ///
    /// Two group cells with no group cell between them are the same run when either they are
    /// adjacent in the same row, or the earlier one's row soft-wraps into the later one's and
    /// nothing but blank cells stands between them. `row_map[..].continues` is the terminal's
    /// own answer to the first half; the blank test is the second, and it is what keeps a
    /// wrapped line that names the same URL twice from being read as one link.
    fn link_group_run(
        &self,
        index: usize,
        link: &bt_transcript::CellHyperlink,
    ) -> Option<(usize, usize)> {
        if !cell_in_link_group(self.cells.get(index)?, link) {
            return None;
        }
        let columns = self.columns.get() as usize;
        let blank = |cell: &bt_transcript::CapturedCell| {
            cell.text.is_empty() || cell.text.chars().all(char::is_whitespace)
        };
        // Whether the gap between two group cells is one the link may cross.
        let joined = |earlier: usize, later: usize| {
            let (row, next_row) = (earlier / columns, later / columns);
            if row == next_row {
                return later == earlier + 1;
            }
            next_row == row + 1
                && self.row_map.get(row).is_some_and(|mapped| mapped.continues)
                && self.cells[earlier + 1..later].iter().all(blank)
        };
        let mut first = index;
        while let Some(previous) = self.cells[..first]
            .iter()
            .rposition(|cell| cell_in_link_group(cell, link))
        {
            if !joined(previous, first) {
                break;
            }
            first = previous;
        }
        let mut last = index;
        while let Some(next) = self.cells[last + 1..]
            .iter()
            .position(|cell| cell_in_link_group(cell, link))
            .map(|offset| last + 1 + offset)
        {
            if !joined(last, next) {
                break;
            }
            last = next;
        }
        Some((first, last))
    }

    /// Grow one run into the whole link when the break beside it was made by the **application**
    /// rather than by the terminal, as a pair of flat cell indices — `None` when the run stands
    /// alone.
    ///
    /// # The case
    ///
    /// A full-screen application lays its own text out and wraps it itself, so a link too long for
    /// the pane arrives as two OSC 8 emissions with a real newline between them and no `continues`
    /// on the seam (user report 2026-08-20: Claude Code's footer, `…/en/arti` over `cles/15363606`,
    /// where hovering lit one line and left the other resting). Nothing [`Self::link_group_run`]
    /// reads can join those: the terminal never wrapped that row, and the two halves need not even
    /// share an id — the vendor synthesizes one per emission, so a re-opened link gets a fresh one.
    ///
    /// # The evidence, and why it is not the id
    ///
    /// OSC 8's `id=` exists to say "these two runs are one link", but this product already ruled it
    /// unusable as that (2026-08-18, [`Self::link_group_run`]): the applications in front of us
    /// stamp one id on **every** occurrence of a URL, so an id says "same target", not "same
    /// occurrence", and trusting it lights a whole file listing at once.
    ///
    /// What is trustworthy is the **label**: when the two fragments' printed text, concatenated,
    /// spells the link's own target exactly, they can only be one broken link. Two unrelated
    /// mentions of one URL each spell it in full, so their concatenation spells it twice and never
    /// matches; a label that is not the target (`[img] photo.png`) offers no evidence and is left
    /// as the two segments it looks like. The geometry still has to hold as well — consecutive
    /// rows, nothing but blank cells across the seam — so an occurrence with any ink after it on
    /// its row is not a fragment of anything.
    fn rejoined_across_break(&self, run: (usize, usize), uri: &str) -> Option<(usize, usize)> {
        let mut runs = vec![run];
        let mut hit = 0usize;
        let mut spelled = self.run_label(run, uri).len();
        while spelled < uri.len()
            && let Some(previous) = self.run_across_break(runs[0].0, uri, false)
        {
            spelled += self.run_label(previous, uri).len();
            runs.insert(0, previous);
            hit += 1;
        }
        while spelled < uri.len()
            && let Some(next) = self.run_across_break(runs[runs.len() - 1].1, uri, true)
        {
            spelled += self.run_label(next, uri).len();
            runs.push(next);
        }
        if runs.len() == 1 {
            return None;
        }
        // The widest window around the hovered run whose fragments spell the target.
        for first in 0..=hit {
            for last in (hit..runs.len()).rev() {
                let label = runs[first..=last]
                    .iter()
                    .map(|run| self.run_label(*run, uri))
                    .collect::<String>();
                if label == uri && (first, last) != (hit, hit) {
                    return Some((runs[first].0, runs[last].1));
                }
            }
        }
        None
    }

    /// The printed text of one run — its link cells only, so the layout indent a soft wrap leaves
    /// inside a run is not mistaken for part of the label.
    fn run_label(&self, (first, last): (usize, usize), uri: &str) -> String {
        self.cells[first..=last]
            .iter()
            .filter(|cell| !cell.wide_spacer && cell_targets(cell, uri))
            .map(|cell| cell.text.as_str())
            .collect()
    }

    /// The same-target run sitting immediately across one line break from `edge`: on the next row
    /// (or the previous one), with nothing but blank cells between the two.
    fn run_across_break(&self, edge: usize, uri: &str, forward: bool) -> Option<(usize, usize)> {
        let columns = self.columns.get() as usize;
        let blank = |cell: &bt_transcript::CapturedCell| {
            cell.text.is_empty() || cell.text.chars().all(char::is_whitespace)
        };
        let (earlier, later) = if forward {
            let next = self.cells.get(edge + 1..)?;
            let offset = next.iter().position(|cell| cell_targets(cell, uri))?;
            (edge, edge + 1 + offset)
        } else {
            let previous = self
                .cells
                .get(..edge)?
                .iter()
                .rposition(|cell| cell_targets(cell, uri))?;
            (previous, edge)
        };
        if later / columns != earlier / columns + 1
            || !self.cells[earlier + 1..later].iter().all(blank)
        {
            return None;
        }
        let seed = if forward { later } else { earlier };
        self.link_group_run(seed, self.cells[seed].hyperlink.as_ref()?)
    }

    /// Add the ordinary terminal underline flag to the active link in this frame only. Source
    /// cells and transcript styles remain unchanged.
    pub fn underline_hyperlink(&mut self, hyperlink: &HyperlinkHit) -> bool {
        // **The hit names which segment**, through the anchor it was taken at: a target with three
        // segments on screen has three hits, and the one being hovered is the one whose first cell
        // carries this anchor.
        let Some(hit) = self.cells.iter().enumerate().find_map(|(index, cell)| {
            (cell_targets(cell, &hyperlink.uri)
                && self.cell_anchors[index].start == hyperlink.start)
                .then_some(index)
        }) else {
            return false;
        };
        let Some((first, last)) = self.link_span(hit) else {
            return false;
        };
        // The hit must describe this frame's segment, not a stale frame's.
        if self.cell_anchors[first].start != hyperlink.start
            || self.cell_anchors[last].end != hyperlink.end
        {
            return false;
        }
        for index in first..=last {
            if cell_targets(&self.cells[index], &hyperlink.uri) {
                let flags = &mut self.cells[index].style.flags;
                flags.remove(CellFlags::DOTTED_UNDERLINE);
                flags.insert(CellFlags::UNDERLINE);
            }
        }
        true
    }

    /// Paint the reference affordance over an explicit set of this frame's cells: dotted at rest,
    /// solid under the pointer. Returns whether any cell was marked.
    ///
    /// This is the hyperlink vocabulary applied to a second content type, and deliberately the
    /// *same* vocabulary rather than a parallel one (user ruling 2026-08-04): §4's verb gradient is
    /// already unified across content types, so at rest a peekable reference and a declared link
    /// must be indistinguishable, and what differs is what the hover reveals. Reuse is literal —
    /// the flags are `DOTTED_UNDERLINE`/`UNDERLINE`, and the renderer's run merging then joins a
    /// reference's dots to an adjacent link's without knowing which is which.
    ///
    /// The argument is cell indices and nothing else, because the affordance no longer has an
    /// opinion about *which* cells (user ruling 2026-08-04, frame-derived rework). Its caller found
    /// them by reading this frame's own text, so there is nothing left for the painter to verify:
    /// there is no anchor to be stale, no remembered span to disagree with, and no witness to
    /// compare — a cell is marked because this frame draws a reference on it. Source cells and
    /// transcript styles are untouched; this frame is the only thing that changes.
    pub fn underline_cells(&mut self, cells: &[u32], hover: bool) -> bool {
        let mut marked = false;
        for index in cells.iter().map(|index| *index as usize) {
            let Some(cell) = self.cells.get_mut(index) else {
                continue;
            };
            marked = true;
            if hover {
                cell.style.flags.remove(CellFlags::DOTTED_UNDERLINE);
                cell.style.flags.insert(CellFlags::UNDERLINE);
            } else if !cell.style.flags.contains(CellFlags::UNDERLINE) {
                // A cell already wearing a solid underline keeps it: that is either the application's
                // own SGR 4 or a hover already resolved, and the resting affordance may not overrule
                // either into a weaker mark.
                cell.style.flags.insert(CellFlags::DOTTED_UNDERLINE);
            }
        }
        marked
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
    /// Whether this row soft-wraps into the next. See [`FrameVisualRow::continues`].
    continues: bool,
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
            continues: row.visual.continues,
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
    /// What the search capsule has found in **this** pane (§7.1.5d, S3).
    ///
    /// Beside [`Self::selection`] and set the same way — the projection is told, and every frame it
    /// builds afterwards carries the spans. It is not beside it in *who writes it*: the selection
    /// is pushed in by the session every frame (`sync_projection_state`), because a selection is a
    /// property of the shell, while a search is a property of the window and there is only one of
    /// them for the whole window (mock 8515's singleton). So the window writes this once when the
    /// hit set changes, and it survives every frame in between untouched.
    ///
    /// Shared by [`Arc`] because the same list is handed to a projection on every keystroke and
    /// nothing ever mutates it in place.
    search: Option<Arc<SearchHighlights>>,
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
    /// How much of `last_live_overflow_subpixels` the reader has currently spent.
    live_overflow_offset_subpixels: i64,
    /// Live-plane height that the resting Bottom frame still cuts off above the pane, i.e. the band
    /// inflation minus the bottom relief a blank live tail already yielded (see `continuous_frame`).
    /// It is simultaneously the resting top cut, the local-review capacity, and the count behind the
    /// "N rows above" indicator — one number, so a review always ends exactly where rest begins.
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
            search: None,
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

    /// One screenful, in subpixels — the page every scroll extent is measured
    /// against.
    pub fn viewport_height_subpixels(&self) -> i64 {
        i64::from(self.live_rows.get()).saturating_mul(self.cell_height_subpixels.get())
    }

    /// **The furthest into history this view may be scrolled**, in subpixels:
    /// everything the last projection measured, less the one screenful that is
    /// showing. Zero when the whole of the document fits, which is the same
    /// thing as "there is no scrollback to look at".
    ///
    /// Public because a scroll bar is a *picture* of this number and of
    /// [`Self::scroll_offset_subpixels`], and a picture derived from anything
    /// else would be a second opinion about how far the view can go. It is the
    /// clamp [`Self::scroll_by_subpixels`] and [`Self::scroll_to_top`] were
    /// already computing inline, named once so the three cannot disagree.
    pub fn scroll_extent_subpixels(&self) -> i64 {
        self.last_total_height_subpixels
            .saturating_sub(self.viewport_height_subpixels())
            .max(0)
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
        let max = self.scroll_extent_subpixels();
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
        let offset = self.scroll_extent_subpixels();
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
        let implicit = implicit_hyperlinks(&rows.iter().collect::<Vec<_>>());
        for (row_index, row) in rows.into_iter().enumerate() {
            if row.cells.len() != expected_columns {
                return Err(FrameProjectionError::ColumnCount {
                    row: row_index,
                    expected: expected_columns,
                    actual: row.cells.len(),
                });
            }
            let visual = captured_visual_row(
                row,
                expected_columns,
                &implicit[row_index],
                |column, bias| ContentAnchor::Live {
                    screen: ScreenId::Primary,
                    point: GridPoint {
                        row: row_index as u32,
                        column: column as u32,
                    },
                    bias,
                    generation: self.grid_generation,
                },
            );
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
            // The bare live frame carries no marks of any kind — neither a selection nor a search
            // — because it is the grid alone, with no history behind it to have found anything in.
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
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
        // Bare-URL recognition reads whole logical lines (see [`implicit_hyperlinks`]), and one
        // logical line can begin in staging and finish on the live grid, so the two planes are
        // read as the single sequence they are presented as. Over the **complete** sequence, not
        // the window about to be cut from it: a URL whose tail falls below the viewport must not
        // become a link to the shorter address its visible half spells.
        let implicit_live_base = if primary { staged_rows.len() } else { 0 };
        let implicit = implicit_hyperlinks(
            &staged_rows
                .iter()
                .map(|staged| &staged.row)
                .take(implicit_live_base)
                .chain(live_rows.iter())
                .collect::<Vec<_>>(),
        );
        let live_height = self.live_row_prefix.last().copied().unwrap_or_else(|| {
            i64::from(self.live_rows.get()).saturating_mul(self.cell_height_subpixels.get())
        });
        let rectangular_live_height =
            i64::from(self.live_rows.get()).saturating_mul(self.cell_height_subpixels.get());
        let live_height_delta = live_height.saturating_sub(rectangular_live_height);
        let live_extra_height = live_height_delta.max(0);
        // Bottom relief (user ruling 2026-08-02): a band-inflated live plane pushes content above
        // the pane while the rows under the cursor may hold nothing. At Bottom that blank tail
        // yields — up to the band overflow — so the resting window ends at the last meaningful grid
        // row instead of wasting pane on emptiness while a decoration is cut at the top.
        //
        // A row is meaningful when it carries ink, when the cursor sits on it, or when a live
        // decoration owns it (`live_row_prefix` grew that row, so a blank source row inside a band
        // still shows pixels). Because every row below the last meaningful one is exactly one cell
        // tall, capping the relief at `blank_tail_rows * cell_height` keeps the last meaningful row
        // — and therefore the cursor and every decoration — completely inside the pane.
        //
        // Content-derived and deterministic: zero relief without band overflow (byte-identical
        // classic behavior), zero on full panes (a TUI writes its lower rows), uniform across
        // Primary and Alternate.
        let last_decorated_live_row = self
            .live_math_artifacts
            .iter()
            .filter(|artifact| {
                artifact.screen == screen && artifact.generation == self.grid_generation
            })
            .map(|artifact| artifact.band_end_row as usize)
            .max()
            .unwrap_or(0)
            .min(expected_rows.saturating_sub(1));
        let last_meaningful_row = live_rows
            .iter()
            .enumerate()
            .rev()
            .find(|(_, row)| !live_row_is_blank(row))
            .map_or(0, |(index, _)| index)
            .max(cursor.row as usize)
            .max(last_decorated_live_row);
        let blank_tail_rows = expected_rows.saturating_sub(last_meaningful_row + 1);
        let bottom_relief_subpixels = i64::try_from(blank_tail_rows)
            .unwrap_or(i64::MAX)
            .saturating_mul(self.cell_height_subpixels.get())
            .min(live_extra_height)
            .max(0);
        // The relieved pixels are spent at rest, so they are no longer overflow: what remains is
        // both the height still cut off above the pane and the exact local-review capacity. One
        // number now drives the resting cut (`frame_top_subpixels`), the review indicator, and —
        // via `bottom_top_subpixels` below — the scroll ceiling, so a review that returns to offset
        // zero lands back on the identical resting geometry.
        self.last_live_overflow_subpixels =
            live_extra_height.saturating_sub(bottom_relief_subpixels);
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
        let bottom_top_subpixels = total_height
            .saturating_sub(pane_height)
            .saturating_sub(bottom_relief_subpixels)
            .max(0);
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
            // The resting live plane is cut at the top by the overflow the blank tail did NOT
            // relieve. The relieved pixels move the whole plane down instead, spending blank tail
            // rows off the pane bottom. With zero relief this is `-live_extra_height` exactly, the
            // classic flush-bottom identity. It also agrees to the subpixel with the scrolled path:
            // at offset zero that path computes `window_top - live_plane_top`, which is precisely
            // `live_extra_height - relief`, so leaving and returning to rest never jumps.
            self.last_live_overflow_subpixels.saturating_neg()
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
                                run: None,
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
                                    run: None,
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
                let row = captured_staged_visual_row(
                    staged,
                    column_count,
                    &implicit[first + offset],
                    self.source_generation,
                );
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
                            visual: captured_visual_row(
                                row,
                                column_count,
                                &implicit[implicit_live_base + live_row],
                                |column, bias| ContentAnchor::Live {
                                    screen,
                                    point: GridPoint {
                                        row: live_row as u32,
                                        column: column as u32,
                                    },
                                    bias,
                                    generation: self.grid_generation,
                                },
                            ),
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
                        run: None,
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
        let (search_spans, current_search_spans) = self.search.as_deref().map_or_else(
            <(Vec<SelectionSpan>, Vec<SelectionSpan>)>::default,
            |highlights| {
                search_spans(
                    &cell_anchors,
                    &row_map,
                    column_count,
                    if presentation_offset_subpixels == 0 {
                        expected_rows
                    } else {
                        presentation_rows
                    },
                    highlights,
                )
            },
        );
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
            search_spans,
            current_search_spans,
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
    /// Hand this pane the hits its capsule found, or `None` when it has no capsule.
    ///
    /// The one door — a pane that stops being the searched one is told `None` through it, which is
    /// what makes "close the search and no highlight is left anywhere" a single call rather than a
    /// sweep over the frames that happen to be on screen.
    pub fn set_search_highlights(&mut self, search: Option<Arc<SearchHighlights>>) {
        self.search = search;
    }
    #[must_use]
    pub fn search_highlights(&self) -> Option<&Arc<SearchHighlights>> {
        self.search.as_ref()
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
        // Padding below the last real row, which continues into nothing.
        continues: false,
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
    implicit: &[ImplicitCellLink],
    anchor: impl Fn(usize, Bias) -> ContentAnchor,
) -> VisualRow {
    let continues = row.continues;
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
    mark_osc_8_dotted(&mut cells);
    apply_implicit_hyperlinks(&mut cells, implicit);
    VisualRow {
        cells,
        anchors,
        continues,
    }
}

fn captured_staged_visual_row(
    staged: &StagedRow,
    columns: usize,
    implicit: &[ImplicitCellLink],
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
    mark_osc_8_dotted(&mut cells);
    apply_implicit_hyperlinks(&mut cells, implicit);
    VisualRow {
        cells,
        anchors,
        continues: staged.row.continues,
    }
}

/// Give every OSC 8 cell of one row its resting dotted underline.
///
/// Projection owns the affordance: the source grid/transcript remains byte-for-byte unchanged, and
/// the inferred URLs [`implicit_hyperlinks`] finds deliberately keep their unmarked resting
/// presentation, so this must run before they are laid on and see only what OSC 8 declared.
fn mark_osc_8_dotted(cells: &mut [CapturedCell]) {
    for cell in cells.iter_mut().filter(|cell| cell.hyperlink.is_some()) {
        cell.style.flags.insert(CellFlags::DOTTED_UNDERLINE);
    }
}

/// One inferred bare URL's claim on one cell of one row.
struct ImplicitCellLink {
    column: usize,
    link: CellHyperlink,
}

/// Find the bare URLs in a run of captured rows, **one logical line at a time**, and report them
/// as per-row cell claims parallel to `rows`.
///
/// # Why the logical line and not the row
///
/// A URL wider than the pane is one URL that the terminal wrapped, not two things. Reading a
/// single visual row cannot see that: the row holding `…> https://support.cla` parses as a
/// complete, valid URL at a host that someone else may own, and the row holding
/// `ude.com/en/articles/15363606` has no scheme and parses as nothing at all. That is what shipped
/// on the live grid until 2026-08-20, and it was not a drawing fault — the first row became a
/// **link to the wrong site** and the second row was not a link at all, so a click went somewhere
/// the reader never typed. The frozen plane never had the bug, because
/// [`layout_frozen_line`] has always read `FrozenLine::text` whole; this is the live plane being
/// given the same answer.
///
/// `CapturedRow::continues` is the terminal's own record of which rows are one line, the same fact
/// the frozen plane knows by construction, so the grouping is read and not guessed. Detection runs
/// over the **complete** row sequence rather than the visible window, because a URL clipped by the
/// top or bottom of the viewport would otherwise be truncated into that same wrong target.
fn implicit_hyperlinks(rows: &[&CapturedRow]) -> Vec<Vec<ImplicitCellLink>> {
    let mut claims: Vec<Vec<ImplicitCellLink>> = rows.iter().map(|_| Vec::new()).collect();
    let mut start = 0usize;
    while start < rows.len() {
        let mut end = start;
        while end + 1 < rows.len() && rows[end].continues {
            end += 1;
        }
        implicit_hyperlinks_in_line(&rows[start..=end], start, &mut claims);
        start = end + 1;
    }
    claims
}

/// One logical line's worth of [`implicit_hyperlinks`]. `base` is the index `line[0]` has in the
/// full sequence the claims are indexed by.
fn implicit_hyperlinks_in_line(
    line: &[&CapturedRow],
    base: usize,
    claims: &mut [Vec<ImplicitCellLink>],
) {
    let mut text = String::new();
    // (row, column, byte range) for every cell of the line, in reading order.
    let mut spots = Vec::new();
    for (offset, row) in line.iter().enumerate() {
        for (column, cell) in row.cells.iter().enumerate() {
            let start = text.len();
            if !cell.wide_spacer {
                text.push_str(&cell.text);
            }
            spots.push((base + offset, column, start..text.len()));
        }
    }
    let cell_at = |row: usize, column: usize| &line[row - base].cells[column];
    for range in detect_http_urls(&text) {
        let affected = spots
            .iter()
            .enumerate()
            .filter(|(_, (_, _, bytes))| {
                bytes.start < range.byte_end && range.byte_start < bytes.end
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if affected.is_empty()
            || affected.iter().any(|index| {
                cell_at(spots[*index].0, spots[*index].1)
                    .hyperlink
                    .is_some()
            })
        {
            continue;
        }
        let link = CellHyperlink::implicit(text[range.byte_start..range.byte_end].to_owned());
        for index in affected {
            let (row, column, _) = spots[index];
            claims[row].push(ImplicitCellLink {
                column,
                link: link.clone(),
            });
            // A wide glyph's spacer column belongs to the same on-screen cell as the glyph and
            // must carry its link. It is only ever the column after its own lead, never the first
            // column of the next row.
            let spacer = column + 1;
            if spacer < line[row - base].cells.len() && line[row - base].cells[spacer].wide_spacer {
                claims[row].push(ImplicitCellLink {
                    column: spacer,
                    link: link.clone(),
                });
            }
        }
    }
}

/// Lay one row's share of [`implicit_hyperlinks`] onto its cells.
fn apply_implicit_hyperlinks(cells: &mut [CapturedCell], implicit: &[ImplicitCellLink]) {
    for claim in implicit {
        if let Some(cell) = cells.get_mut(claim.column) {
            cell.hyperlink = Some(claim.link.clone());
        }
    }
}

/// A live row with no visible ink: every cell's text is empty or whitespace. Wide-char spacers
/// carry empty text and never appear without their inked lead cell, so they need no special case.
fn live_row_is_blank(row: &CapturedRow) -> bool {
    row.cells
        .iter()
        .all(|cell| cell.text.chars().all(char::is_whitespace))
}

/// True when `cell` belongs to the same OSC 8 link group: exact (id, uri) match, read explicitly
/// because `CellHyperlink`'s own equality deliberately covers the uri alone.
fn cell_in_link_group(cell: &CapturedCell, link: &CellHyperlink) -> bool {
    cell.hyperlink
        .as_ref()
        .is_some_and(|other| other.id == link.id && other.uri == link.uri)
}

/// Whether `cell` carries a link to `uri`, whatever emission it came from. This is the coarser of
/// the two memberships: it holds across the fresh id a re-opened OSC 8 sequence is given, which is
/// what a link broken by the application's own line break arrives as.
fn cell_targets(cell: &CapturedCell, uri: &str) -> bool {
    cell.hyperlink.as_ref().is_some_and(|link| link.uri == uri)
}

/// The screen column at which the grapheme `offset` of a frozen logical line sits, *within the
/// physical row that carries it*.
///
/// A `History` anchor's offset counts graphemes along the whole logical line, so on a soft-wrapped
/// line it runs past the pane and is not a column at all. This replays the same greedy wrap
/// `layout_frozen_line` performs — one cluster at a time, `cluster_width` per cluster, a new row
/// whenever the next cluster would overflow — and reports the column the cluster is placed at. It
/// is the projection's own arithmetic, so a wide character costs the two cells it actually
/// occupies and a zero-width cluster reports the column of the cell it joined.
///
/// An offset at or past the end of the line reports the column just past the last cell, which is
/// exactly where `pad_frozen_row` closes the final row.
pub fn frozen_line_screen_column(line: &FrozenLine, offset: u32, columns: usize) -> u32 {
    let mut used = 0usize;
    let mut last_cell_column = 0usize;
    for (index, cluster) in graphemes(&line.text).enumerate() {
        let at_offset = index as u32 == offset;
        let width = cluster_width(cluster).min(columns);
        if width == 0 {
            if at_offset {
                return last_cell_column as u32;
            }
            continue;
        }
        if used != 0 && used + width > columns {
            used = 0;
        }
        if at_offset {
            return used as u32;
        }
        last_cell_column = used;
        used += width;
    }
    used as u32
}

/// The screen column at which the grapheme `offset` of one staged physical row sits.
///
/// A `Staging` anchor's offset counts the row's inked cells, so wide-character spacers make it
/// differ from the column even though a staged row never wraps. This walks the row's cells exactly
/// as `captured_staged_visual_row` assigns their anchors, so the two always agree.
pub fn staged_row_screen_column(staged: &StagedRow, offset: u32) -> u32 {
    let mut grapheme_offset = 0u32;
    for (column, cell) in staged.row.cells.iter().enumerate() {
        if cell.wide_spacer {
            continue;
        }
        if grapheme_offset == offset {
            return column as u32;
        }
        grapheme_offset += u32::from(!cell.text.is_empty());
    }
    staged.row.cells.len() as u32
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
    // Every row this produces but the last is a soft wrap **by construction**:
    // one frozen logical line is being cut into as many rows as it takes, and the
    // cut is the wrap. The last is fixed up once the count is known.
    let mut rows = vec![VisualRow {
        cells: Vec::with_capacity(columns),
        anchors: Vec::with_capacity(columns),
        continues: true,
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
                continues: true,
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
    let last = rows.last_mut().unwrap();
    pad_frozen_row(last, line, columns, end_offset);
    // The line ends here, so this row wraps into nothing — whatever is drawn
    // under it is the next logical line.
    last.continues = false;
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

/// The two span lists a hit set makes over one frame's cells: the plain hits, and the current one.
///
/// # Why this is a second function and not [`selection_spans`] called twice
///
/// The selection is **one** interval and is decided by comparing each cell against its two ends,
/// which is what `compare_visible_anchors` is for. A hit set is thousands of intervals, and the
/// same road would be thousands of comparisons per cell. So the question is turned around: each
/// cell says *where it is* ([`search_address`]) and the hit set answers whether anything is there,
/// in two binary searches. The result is identical in shape — runs of adjacent covered cells,
/// coalesced per row — and its cost is a property of the frame rather than of the transcript.
///
/// **A live hit is admitted only on the presentation row that is showing that grid row.** The
/// blank overscan row at the bottom of a live projection carries the *last* grid row's coordinates
/// with `live_grid_row: None` (it is a placeholder, not a row of the grid), so without this a hit
/// on the last line of the screen would paint a ghost of itself in the empty strip beneath it.
fn search_spans(
    anchors: &[CellAnchor],
    row_map: &[FrameVisualRow],
    columns: usize,
    rows: usize,
    highlights: &SearchHighlights,
) -> (Vec<SelectionSpan>, Vec<SelectionSpan>) {
    let mut plain = Vec::new();
    let mut current = Vec::new();
    if highlights.is_empty() || columns == 0 {
        return (plain, current);
    }
    let expected = columns.saturating_mul(rows);
    let Some(anchors) = anchors.get(..expected) else {
        return (plain, current);
    };
    // Which class a cell belongs to, so a run breaks where the ink changes rather than where the
    // coverage does: the current hit standing next to an ordinary one is two spans, not one.
    let class = |row: usize, anchor: &CellAnchor| -> Option<bool> {
        let (line, offset) = search_address(&anchor.start)?;
        if let SearchLine::Live { row: grid_row } = line
            && row_map.get(row).and_then(|mapped| mapped.live_grid_row) != Some(grid_row)
        {
            return None;
        }
        if highlights.current_covers(line, offset) {
            return Some(true);
        }
        highlights.covers(line, offset).then_some(false)
    };
    for (row, row_anchors) in anchors.chunks(columns).enumerate() {
        let mut column = 0usize;
        while column < columns {
            let Some(is_current) = class(row, &row_anchors[column]) else {
                column += 1;
                continue;
            };
            let start_column = column;
            while column < columns && class(row, &row_anchors[column]) == Some(is_current) {
                column += 1;
            }
            let span = SelectionSpan {
                row: row as u32,
                start_column: start_column as u32,
                end_column: column as u32,
            };
            if is_current { &mut current } else { &mut plain }.push(span);
        }
    }
    (plain, current)
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
            lang_rev: 0,
            profile_rev: 0,
        }
    }

    /// PIN (wrapped-reference geometry, 2026-08-04): the column a frozen grapheme offset resolves
    /// to is the column `layout_frozen_line` actually places it at — inside its own physical row,
    /// with wide characters costing two cells. Read as a column, the raw offset runs past the pane
    /// on every soft-wrapped line.
    #[test]
    fn a_frozen_offset_resolves_to_the_column_its_own_physical_row_shows_it_at() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(8).unwrap());
        let mut capture = |text: &str, continues: bool| {
            store
                .capture(CapturedRow::plain(text, continues))
                .finalized
                .pop()
        };
        capture("0123456789", true);
        let line = capture("abcXdef", false).unwrap().line;

        // One logical line of 17 graphemes laid out ten columns wide: offsets 0..=9 sit on the
        // first row at their own index, and offsets 10.. restart at column zero on the second.
        assert_eq!(frozen_line_screen_column(&line, 3, 10), 3);
        assert_eq!(frozen_line_screen_column(&line, 10, 10), 0);
        assert_eq!(frozen_line_screen_column(&line, 13, 10), 3);
        // Past the end is the column just after the last cell, where the row's padding begins.
        assert_eq!(frozen_line_screen_column(&line, 17, 10), 7);
        // Laid out wide enough not to wrap, the same offsets are their own columns again.
        assert_eq!(frozen_line_screen_column(&line, 13, 40), 13);

        let mut store = TranscriptStore::new(NonZeroUsize::new(8).unwrap());
        let wide = store
            .capture(CapturedRow::plain("中中 x.png", false))
            .finalized
            .pop()
            .unwrap()
            .line;
        // Two wide characters occupy four cells, so the fourth grapheme stands at column five.
        assert_eq!(frozen_line_screen_column(&wide, 3, 40), 5);
    }

    /// PIN (wrapped-reference geometry, 2026-08-04): a staged row's grapheme offset resolves
    /// through its own cells, so a wide-character spacer moves the column exactly as
    /// `captured_staged_visual_row` moves the anchor.
    #[test]
    fn a_staged_offset_resolves_through_the_rows_own_cells() {
        let mut cells = Vec::new();
        for _ in 0..2 {
            cells.push(CapturedCell::plain("中"));
            cells.push(CapturedCell {
                wide_spacer: true,
                ..CapturedCell::default()
            });
        }
        cells.extend(" x.png".chars().map(|c| CapturedCell::plain(c.to_string())));
        let staged = StagedRow {
            id: StagingId(1),
            row: CapturedRow {
                cells,
                continues: false,
                shell_mark: None,
            },
        };
        assert_eq!(staged_row_screen_column(&staged, 0), 0);
        assert_eq!(staged_row_screen_column(&staged, 1), 2);
        assert_eq!(staged_row_screen_column(&staged, 3), 5);
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
                    inline_runs: Vec::new(),
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

    /// PIN (user repro 2026-08-04): the resting affordance may not be painted from an anchor span
    /// the frame's own cells disagree with.
    ///
    /// An anchor pair says "the reference is between here and here". The frame decides which cells
    /// that names, and after a reflow the two can disagree badly: replaying `image-accept` at a
    /// pane-widening resize handed a 44-character path's span the rest of its row — 48 blank cells
    /// — plus the first three cells of the line below, and the paint dutifully dotted all of them.
    /// The cells' own text is the only thing that can settle the disagreement, so the paint asks
    /// it: mark the run only when it still spells the reference.
    ///
    /// RED CHECK: deleting the `spells_the_reference` gate marks the over-wide span here, and reds
    /// the second assertion below. The truthful span in the same frame stays green either way,
    /// which is what makes this a pin about the disagreement rather than about the affordance.
    /// PIN (frame-derived affordance ruling, 2026-08-04): the painter marks the cells it is given,
    /// all of them and only them, and it never weakens a mark that is already there.
    ///
    /// This is all that is left of the frame half of the old content witness, and deliberately so.
    /// The witness existed because a caller handed over an anchor span — a claim about *where* — that
    /// the frame could disagree with. The caller now hands over cells it found by reading this
    /// frame's own text, so there is no claim left to check: what is asserted here is that painting
    /// is exact and non-destructive, which is the whole of the painter's contract.
    #[test]
    fn the_painter_marks_the_cells_it_is_given_and_never_weakens_a_solid() {
        let projection = ViewportProjection::new(
            key(10),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let build = || {
            let mut frame = projection
                .live_frame(
                    nz32(10),
                    vec![
                        CapturedRow::plain("x/a.png   ", false),
                        CapturedRow::plain("before    ", false),
                    ],
                    GridCursor {
                        row: 1,
                        column: 0,
                        visible: true,
                    },
                )
                .unwrap();
            // The application's own SGR 4 on the reference's first cell.
            frame.cells[0].style.flags.insert(CellFlags::UNDERLINE);
            frame
        };
        let marked = |frame: &ViewportFrame, flag: CellFlags| {
            frame
                .cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| cell.style.flags.contains(flag))
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        };

        let mut frame = build();
        assert!(frame.underline_cells(&[0, 1, 2, 3, 4, 5, 6], false));
        assert_eq!(
            marked(&frame, CellFlags::DOTTED_UNDERLINE),
            (1..7).collect::<Vec<_>>(),
            "every given cell but the one already wearing the application's solid underline",
        );
        assert_eq!(
            marked(&frame, CellFlags::UNDERLINE),
            vec![0],
            "and that one keeps the stronger mark it came with",
        );

        // The hover upgrade is the same cells, solid, with no dots left behind.
        assert!(frame.underline_cells(&[0, 1, 2, 3, 4, 5, 6], true));
        assert_eq!(
            marked(&frame, CellFlags::UNDERLINE),
            (0..7).collect::<Vec<_>>(),
        );
        assert_eq!(
            marked(&frame, CellFlags::DOTTED_UNDERLINE),
            Vec::<usize>::new(),
        );

        // A cell index this frame does not have marks nothing and says so.
        let mut frame = build();
        assert!(!frame.underline_cells(&[frame.cells.len() as u32], false));
        assert_eq!(
            marked(&frame, CellFlags::DOTTED_UNDERLINE),
            Vec::<usize>::new(),
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
        let mut row = CapturedRow::plain("https://shown.example https://plain.example).", false);
        for cell in &mut row.cells[..21] {
            cell.hyperlink = Some(CellHyperlink::implicit("file:///real-target"));
        }
        let implicit = implicit_hyperlinks(&[&row]);
        let mut cells = row.cells;
        mark_osc_8_dotted(&mut cells);
        apply_implicit_hyperlinks(&mut cells, &implicit[0]);

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

    /// PIN (user report 2026-08-18) — **two separate runs sharing one OSC 8 id are two
    /// links to hover, and only the one under the pointer lights.**
    ///
    /// Claude Code stamps the same emission id on every occurrence of the same URL it
    /// prints, which is within the letter of OSC 8 and means a screen listing one file
    /// eight times carries eight cells-worth of one "link". Hovering any of them used to
    /// light all eight: the grouping rule walked the whole frame for `(id, uri)` and took
    /// the first cell to the last, indent gaps, other rows and unrelated text included.
    ///
    /// The narrowing is deliberate and it is only about *lighting*. The id still joins a
    /// wrapped link across its indent — the pin above — and the hit still carries it, so
    /// activation is unchanged; what the pointer is over is one occurrence, and one
    /// occurrence is what may claim the solid underline.
    ///
    /// MUTATIONS:
    /// (1) go back to `position`/`rposition` over the whole frame and the first block goes
    ///     red with the second row lit;
    /// (2) drop the `continues` test from `joined` and these two rows merge into one run,
    ///     because nothing but blanks separates them;
    /// (3) drop the blank test and a wrapped line naming one URL twice merges likewise.
    #[test]
    fn two_runs_sharing_an_id_light_one_at_a_time() {
        let link = CellHyperlink {
            id: Some("7_alacritty".to_owned()),
            uri: "file:///C:/notes.md".to_owned(),
        };
        let row_with_link = || {
            let mut cells = CapturedRow::plain("  notes.md  ", false).cells;
            for cell in &mut cells[2..11] {
                cell.hyperlink = Some(link.clone());
            }
            CapturedRow {
                cells,
                // Two printed lines, not one wrapped one — which is the whole
                // difference between this fixture and the pin above it.
                continues: false,
                shell_mark: None,
            }
        };
        let projection = ViewportProjection::new(
            key(12),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let mut frame = projection
            .live_frame(
                nz32(12),
                vec![row_with_link(), row_with_link()],
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: false,
                },
            )
            .unwrap();

        let upper = frame.hyperlink_at(0, 4).unwrap();
        let lower = frame.hyperlink_at(1, 4).unwrap();
        assert_ne!(upper, lower, "two occurrences are two things to hover");
        assert_eq!(upper.uri, lower.uri);
        assert_eq!(
            upper.id, lower.id,
            "and the id is still on both, because it is what activation and the \
             wrap-joining read"
        );

        assert!(frame.underline_hyperlink(&upper));
        let columns = frame.columns.get() as usize;
        let solid = |frame: &ViewportFrame, row: usize, column: usize| {
            let flags = frame.cells[row * columns + column].style.flags;
            flags.contains(CellFlags::UNDERLINE) && !flags.contains(CellFlags::DOTTED_UNDERLINE)
        };
        for column in 2..11 {
            assert!(solid(&frame, 0, column), "the hovered run, column {column}");
            assert!(
                !solid(&frame, 1, column),
                "the other occurrence keeps its resting dots, column {column}"
            );
        }

        // And the other way round, on a frame that has not been touched.
        let mut frame = projection
            .live_frame(
                nz32(12),
                vec![row_with_link(), row_with_link()],
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: false,
                },
            )
            .unwrap();
        assert!(frame.underline_hyperlink(&lower));
        for column in 2..11 {
            assert!(solid(&frame, 1, column), "the hovered run, column {column}");
            assert!(!solid(&frame, 0, column), "and only it, column {column}");
        }
    }

    /// PIN — **a wrapped link whose continuation names the same URL again is still two
    /// links**, and the seam is what tells them apart.
    ///
    /// The case `continues` alone cannot answer: row 0 really does soft-wrap into row 1,
    /// so the wrap test says yes, and what says no is that the cells between the two runs
    /// are not blank — there is text there, and a link that stops for text and starts again
    /// has stopped.
    #[test]
    fn a_wrapped_line_naming_one_url_twice_keeps_its_two_runs_apart() {
        let link = CellHyperlink {
            id: Some("9_alacritty".to_owned()),
            uri: "file:///C:/a.png".to_owned(),
        };
        let mut first_row = CapturedRow::plain("a.png and", true).cells;
        for cell in &mut first_row[..5] {
            cell.hyperlink = Some(link.clone());
        }
        let mut second_row = CapturedRow::plain("so a.png ", false).cells;
        for cell in &mut second_row[3..8] {
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
        assert_ne!(
            frame.hyperlink_at(0, 1).unwrap(),
            frame.hyperlink_at(1, 4).unwrap(),
            "the word between them ends the first run"
        );
        let hit = frame.hyperlink_at(0, 1).unwrap();
        assert!(frame.underline_hyperlink(&hit));
        let columns = frame.columns.get() as usize;
        let solid = |frame: &ViewportFrame, row: usize, column: usize| {
            let flags = frame.cells[row * columns + column].style.flags;
            flags.contains(CellFlags::UNDERLINE) && !flags.contains(CellFlags::DOTTED_UNDERLINE)
        };
        for column in 0..5 {
            assert!(solid(&frame, 0, column), "the hovered run, column {column}");
        }
        for column in 3..8 {
            assert!(
                !solid(&frame, 1, column),
                "the second mention is its own link, column {column}"
            );
        }
    }

    /// A live frame from one grid, with the rows given verbatim.
    fn live_frame_of(rows: Vec<CapturedRow>) -> ViewportFrame {
        let columns = rows[0].cells.len();
        ViewportProjection::new(
            key(columns as u32),
            DetectionRevision(1),
            nz32(rows.len() as u32),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        )
        .live_frame(
            nz32(columns as u32),
            rows,
            GridCursor {
                row: 0,
                column: 0,
                visible: false,
            },
        )
        .unwrap()
    }

    fn solid_at(frame: &ViewportFrame, row: usize, column: usize) -> bool {
        let flags = frame.cells[row * frame.columns.get() as usize + column]
            .style
            .flags;
        flags.contains(CellFlags::UNDERLINE) && !flags.contains(CellFlags::DOTTED_UNDERLINE)
    }

    /// PIN (user report 2026-08-20) — **a bare URL wider than the pane is one link, and its
    /// target is the whole address.**
    ///
    /// The live grid used to recognise bare URLs one *visual row* at a time, so the address the
    /// user had just typed at the prompt came apart along the wrap: the first row parsed as the
    /// complete and perfectly valid `https://support.claude`, and the second row, having no
    /// scheme on it, parsed as nothing and carried no link at all. That is not a drawing fault.
    /// The first row became a **link to a different site** — one someone else may own — and
    /// clicking it went there. The frozen plane never had it, because `layout_frozen_line` reads
    /// the logical line whole; this holds the live plane to the same answer.
    ///
    /// MUTATIONS: run `implicit_hyperlinks` per visual row instead of per logical line and the
    /// `uri` assertion goes red with the truncated address; drop the `continues` grouping and it
    /// goes red the same way.
    #[test]
    fn a_wrapped_bare_url_is_one_link_to_the_complete_address() {
        const URI: &str = "https://support.claude.com/en/a";
        // 24 columns: the prompt's tail, then the address breaking mid-host.
        let head = "> https://support.claude";
        let tail = ".com/en/a";
        let mut frame = live_frame_of(vec![
            CapturedRow::plain(head, true),
            CapturedRow::plain(&format!("{tail:<24}"), false),
        ]);

        let hit = frame.hyperlink_at(0, 5).expect("the address is a link");
        assert_eq!(
            hit.uri, URI,
            "the link's target is the address, not the part of it that fitted on one row"
        );
        assert_eq!(
            frame.hyperlink_at(1, 3).expect("so is its continuation"),
            hit,
            "one address wrapped is one link, hit from either row"
        );
        assert!(
            (0..frame.drawable_rows() * 24).all(|index| !frame.cells[index]
                .style
                .flags
                .contains(CellFlags::DOTTED_UNDERLINE)),
            "an inferred URL keeps its unmarked resting presentation"
        );

        assert!(frame.underline_hyperlink(&hit));
        for column in 2..24 {
            assert!(solid_at(&frame, 0, column), "first row, column {column}");
        }
        for column in 0..tail.len() {
            assert!(solid_at(&frame, 1, column), "second row, column {column}");
        }
        for column in [0, 1] {
            assert!(
                !frame.cells[column]
                    .style
                    .flags
                    .intersects(CellFlags::UNDERLINE | CellFlags::DOTTED_UNDERLINE),
                "the prompt before it is not part of the link, column {column}"
            );
        }
    }

    /// PIN (user report 2026-08-20) — **a link the application broke itself is one link when the
    /// two halves spell its target.**
    ///
    /// Claude Code lays its own footer out and wraps the address at its own computed width, so
    /// the two halves reach the terminal as two OSC 8 emissions with a real newline between
    /// them: `continues` is false, and — because the vendor mints an id per emission — the two
    /// halves need not even carry the same id. Hovering lit one line and left the other resting,
    /// which is the picture telling the reader there are two links here where there is one.
    ///
    /// What licenses the join is the label: the halves, concatenated, spell the target exactly.
    /// MUTATIONS: require `continues` on the seam, or require the two halves to share an id, and
    /// this goes red; compare the label loosely (prefix instead of equality) and the red line
    /// below goes red.
    #[test]
    fn an_application_wrapped_link_joins_when_its_halves_spell_its_target() {
        const URI: &str = "https://support.claude.com/en/articles/15363606";
        let head = "more: https://support.claude.com/en/arti";
        let tail = "cles/15363606";
        let emission = |id: &str| CellHyperlink {
            id: Some(id.to_owned()),
            uri: URI.to_owned(),
        };
        let mut first = CapturedRow::plain(head, false);
        for cell in &mut first.cells[6..] {
            cell.hyperlink = Some(emission("17_alacritty"));
        }
        let mut second = CapturedRow::plain(&format!("{tail:<40}"), false);
        for cell in &mut second.cells[..tail.len()] {
            cell.hyperlink = Some(emission("18_alacritty"));
        }
        let mut frame = live_frame_of(vec![first, second]);

        let hit = frame.hyperlink_at(0, 10).unwrap();
        assert_eq!(hit.uri, URI);
        assert_eq!(
            frame.hyperlink_at(1, 3).unwrap(),
            hit,
            "the two halves are one link to hover, from either half"
        );

        assert!(frame.underline_hyperlink(&hit));
        for column in 6..head.len() {
            assert!(solid_at(&frame, 0, column), "first half, column {column}");
        }
        for column in 0..tail.len() {
            assert!(solid_at(&frame, 1, column), "second half, column {column}");
        }
        for column in 0..6 {
            assert!(
                !frame.cells[column]
                    .style
                    .flags
                    .contains(CellFlags::UNDERLINE),
                "the words before it are not the link, column {column}"
            );
        }
    }

    /// RED LINE for the pin above — **two mentions of one address on neighbouring lines are two
    /// links**, however the seam looks.
    ///
    /// This is the shape the rejoining rule must never swallow: both rows are full to their last
    /// column, the seam is a real newline, nothing but the row edge stands between the two, and
    /// the vendor has stamped both with the one id it reuses for this URL. Geometry says join.
    /// What says no is the label — each row already spells the whole address, so the two of them
    /// spell it twice, and twice is not once.
    #[test]
    fn two_mentions_of_one_address_on_neighbouring_lines_stay_two_links() {
        const URI: &str = "https://example.test/a-fairly-long-path";
        let osc_8 = CellHyperlink {
            id: Some("4_alacritty".to_owned()),
            uri: URI.to_owned(),
        };
        for explicit in [true, false] {
            let row = || {
                let mut row = CapturedRow::plain(URI, false);
                if explicit {
                    for cell in &mut row.cells {
                        cell.hyperlink = Some(osc_8.clone());
                    }
                }
                row
            };
            let mut frame = live_frame_of(vec![row(), row()]);
            let upper = frame.hyperlink_at(0, 9).unwrap();
            let lower = frame.hyperlink_at(1, 9).unwrap();
            assert_eq!(upper.uri, URI);
            assert_ne!(
                upper, lower,
                "two mentions are two things to hover (explicit OSC 8: {explicit})"
            );
            assert!(frame.underline_hyperlink(&upper));
            for column in 0..URI.len() {
                assert!(
                    solid_at(&frame, 0, column),
                    "the hovered mention, column {column} (explicit OSC 8: {explicit})"
                );
                assert!(
                    !solid_at(&frame, 1, column),
                    "and only it, column {column} (explicit OSC 8: {explicit})"
                );
            }
        }
    }

    /// A wrapped bare URL with full-width text beside it: the spacer column that stands under a
    /// wide glyph contributes no bytes to the line being read and takes no link, and the address
    /// found across the wrap is still the whole address.
    #[test]
    fn a_wrapped_bare_url_reads_past_wide_glyph_spacers() {
        const URI: &str = "https://a.test/x";
        let wide = |text: &str| {
            let mut lead = CapturedCell::plain(text);
            lead.style.flags.insert(CellFlags::WIDE_CHAR);
            let spacer = CapturedCell {
                wide_spacer: true,
                ..CapturedCell::default()
            };
            [lead, spacer]
        };
        let mut first = Vec::new();
        first.extend(wide("见"));
        first.extend(CapturedRow::plain(" https://a.te", true).cells);
        let mut second = CapturedRow::plain("st/x ", false).cells;
        second.extend(wide("界"));
        second.extend(CapturedRow::plain(&" ".repeat(8), false).cells);
        assert_eq!((first.len(), second.len()), (15, 15));
        let mut frame = live_frame_of(vec![
            CapturedRow {
                cells: first,
                continues: true,
                shell_mark: None,
            },
            CapturedRow {
                cells: second,
                continues: false,
                shell_mark: None,
            },
        ]);

        let hit = frame.hyperlink_at(0, 5).unwrap();
        assert_eq!(hit.uri, URI, "the wide glyph's spacer spells nothing");
        assert_eq!(frame.hyperlink_at(1, 1).unwrap(), hit);
        assert!(frame.underline_hyperlink(&hit));
        for column in 3..15 {
            assert!(solid_at(&frame, 0, column), "first row, column {column}");
        }
        for column in 0..4 {
            assert!(solid_at(&frame, 1, column), "second row, column {column}");
        }
        for column in [0, 1, 2] {
            assert!(
                frame.cells[column].hyperlink.is_none(),
                "the full-width word before the address is not in it, column {column}"
            );
        }
        for column in [4, 5, 6] {
            assert_eq!(
                frame.cells[frame.columns.get() as usize + column].hyperlink,
                None,
                "nor the one after it, column {column}"
            );
        }
    }

    /// A logical line can have its head in staging and its tail still on the live grid
    /// (`FreezeCandidate::live_tail`), so the two planes are read as the one sequence the frame
    /// presents them as. Reading them apart truncates the address at the seam — the same wrong
    /// target as reading a single row apart.
    #[test]
    fn a_bare_url_wrapping_from_staging_into_the_live_grid_is_one_link() {
        const URI: &str = "https://support.claude.com/en/a";
        let width = 32;
        let staged = [StagedRow {
            id: StagingId(77),
            row: CapturedRow::plain("the docs: https://support.claude", true),
        }];
        let mut live_rows = vec![CapturedRow::plain(
            &format!("{:<width$}", ".com/en/a", width = width as usize),
            false,
        )];
        live_rows.extend(vec![
            CapturedRow::plain(&" ".repeat(width as usize), false);
            11
        ]);
        let document = HistoryDocument::default();
        let mut projection = ViewportProjection::new(
            key(width),
            DetectionRevision(1),
            nz32(12),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        projection.relayout(key(width), &document);
        projection.project(&document);
        let cursor = GridCursor {
            row: 0,
            column: 0,
            visible: false,
        };
        // The staged row sits one row above the window's resting bottom; the first frame is what
        // teaches the projection how tall the whole thing is.
        projection
            .continuous_frame(
                &document,
                &staged,
                live_rows.clone(),
                cursor,
                ScreenId::Primary,
            )
            .unwrap();
        projection.scroll_by_rows(1);
        let frame = projection
            .continuous_frame(&document, &staged, live_rows, cursor, ScreenId::Primary)
            .unwrap();
        let live_row = frame
            .row_map
            .iter()
            .position(|row| row.live_grid_row == Some(0))
            .expect("the live grid's first row is on screen") as u32;
        assert!(live_row >= 1, "the staged row is above it");

        let head = frame.hyperlink_at(live_row - 1, 12).expect("staged half");
        assert_eq!(head.uri, URI, "the seam is not the end of the address");
        assert_eq!(
            frame.hyperlink_at(live_row, 3).expect("live half"),
            head,
            "one address across the seam is one link"
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
                    transition_stale: false,
                    frozen_prefix: Vec::new(),
                    staging_prefix: Vec::new(),
                    generation: GridGeneration(1),
                    artifact: ProjectedMathArtifact {
                        inline_runs: Vec::new(),
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
                    inline_runs: Vec::new(),
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
                    inline_runs: Vec::new(),
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
            cell.text = text.into();
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
                    inline_runs: Vec::new(),
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
                inline_runs: Vec::new(),
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
                    inline_runs: Vec::new(),
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

    /// One projection over one eight-character logical line, two columns wide — so the line wraps
    /// across four presentation rows and a hit can be made to straddle a wrap.
    fn wrapped_projection() -> (ViewportProjection, HistoryDocument) {
        let mut projection = ViewportProjection::new(
            key(2),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        let document = history();
        // The frozen plane has to be laid out before a frame can window it, and the *review*
        // offset is only knowable once one frame has measured the whole thing — so the warm-up
        // frame here is not ceremony, it is what tells  how tall the content is.
        projection.project(&document);
        let _ = wrapped_frame(&mut projection, &document);
        projection.scroll_to_top();
        (projection, document)
    }

    fn wrapped_frame(
        projection: &mut ViewportProjection,
        document: &HistoryDocument,
    ) -> ViewportFrame {
        projection
            .continuous_frame(
                document,
                &[],
                vec![
                    CapturedRow::plain("xy", false),
                    CapturedRow::plain("zw", false),
                ],
                GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .expect("a rectangular frame")
    }

    /// PIN (§7.1.5d) — **the two hit lists are two lists**, and the current one is not in the
    /// other.
    ///
    /// They are painted in different inks — the ordinary hit keeps the text's own colour over a
    /// 30% ground, the current one takes the terminal's background as its ink over a solid accent
    /// — so a cell in both would be painted twice and read as neither.
    ///
    /// MUTATIONS:
    /// (1) put the current hit in `search_spans` as well and its ground is drawn over by the plain
    ///     one, which is the current match becoming invisible;
    /// (2) coalesce runs by coverage instead of by class and a current hit adjacent to an ordinary
    ///     one becomes a single span in one of the two inks.
    #[test]
    fn the_current_hit_is_its_own_span_list_and_never_in_the_other() {
        let (mut projection, document) = wrapped_projection();
        let line = *document.entries().keys().next().expect("one frozen line");
        projection.set_search_highlights(Some(Arc::new(SearchHighlights::new(
            [
                SearchHit {
                    line: SearchLine::History(line),
                    start: 0,
                    end: 2,
                },
                SearchHit {
                    line: SearchLine::History(line),
                    start: 2,
                    end: 4,
                },
            ],
            Some(SearchHit {
                line: SearchLine::History(line),
                start: 2,
                end: 4,
            }),
        ))));
        let frame = wrapped_frame(&mut projection, &document);
        assert_eq!(
            frame.search_spans,
            vec![SelectionSpan {
                row: 0,
                start_column: 0,
                end_column: 2,
            }],
        );
        assert_eq!(
            frame.current_search_spans,
            vec![SelectionSpan {
                row: 1,
                start_column: 0,
                end_column: 2,
            }],
            "the second hit is the current one and it is only in the current list"
        );
        frame.validate_shape().expect("both lists name real rows");
    }

    /// A hit that straddles a wrap is **two spans on two rows**, not one span that stops at the
    /// edge — which is what makes `current_search_spans` a list rather than a single span.
    #[test]
    fn a_hit_across_a_wrap_lights_both_of_the_rows_it_lies_on() {
        let (mut projection, document) = wrapped_projection();
        let line = *document.entries().keys().next().expect("one frozen line");
        projection.set_search_highlights(Some(Arc::new(SearchHighlights::new(
            [SearchHit {
                line: SearchLine::History(line),
                start: 1,
                end: 3,
            }],
            None,
        ))));
        let frame = wrapped_frame(&mut projection, &document);
        assert_eq!(
            frame.search_spans,
            vec![
                SelectionSpan {
                    row: 0,
                    start_column: 1,
                    end_column: 2,
                },
                SelectionSpan {
                    row: 1,
                    start_column: 0,
                    end_column: 1,
                },
            ],
            "one hit, two rows, and each row lights only the part of it that is on that row"
        );
    }

    /// PIN — **a hit on the last live row does not ghost into the blank overscan row beneath it.**
    ///
    /// The overscan row carries the last grid row's own coordinates with `live_grid_row: None`; it
    /// is a placeholder, not a row of the grid. Without the guard in `search_spans` a match on the
    /// bottom line of the screen would paint a second copy of itself in the empty strip below.
    #[test]
    fn a_live_hit_is_drawn_on_its_grid_row_and_not_on_the_blank_row_under_it() {
        let mut projection = ViewportProjection::new(
            key(4),
            DetectionRevision(1),
            nz32(2),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        projection.set_search_highlights(Some(Arc::new(SearchHighlights::new(
            [SearchHit {
                line: SearchLine::Live { row: 1 },
                start: 0,
                end: 4,
            }],
            None,
        ))));
        let frame = projection
            .continuous_frame(
                &HistoryDocument::default(),
                &[],
                vec![
                    CapturedRow::plain("aaaa", false),
                    CapturedRow::plain("bbbb", false),
                ],
                GridCursor {
                    row: 1,
                    column: 0,
                    visible: true,
                },
                ScreenId::Primary,
            )
            .expect("a rectangular frame");
        assert_eq!(
            frame.search_spans,
            vec![SelectionSpan {
                row: 1,
                start_column: 0,
                end_column: 4,
            }],
            "exactly one row, and it is the one whose `live_grid_row` says so"
        );
    }

    /// Handing the projection `None` takes every highlight off it — the one door
    /// [`ViewportProjection::set_search_highlights`] exists to be.
    #[test]
    fn closing_the_search_leaves_no_span_behind() {
        let (mut projection, document) = wrapped_projection();
        let line = *document.entries().keys().next().expect("one frozen line");
        projection.set_search_highlights(Some(Arc::new(SearchHighlights::new(
            [SearchHit {
                line: SearchLine::History(line),
                start: 0,
                end: 2,
            }],
            None,
        ))));
        assert!(
            !wrapped_frame(&mut projection, &document)
                .search_spans
                .is_empty()
        );
        projection.set_search_highlights(None);
        let frame = wrapped_frame(&mut projection, &document);
        assert!(frame.search_spans.is_empty());
        assert!(frame.current_search_spans.is_empty());
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

    /// PIN — **a jump to a command mark is a position, not an act** (S1-UI,
    /// 2026-08-16).
    ///
    /// The command marks rail is this API's first consumer, and the whole reason
    /// it goes through an anchor rather than through `scroll_by_rows` is asserted
    /// here: the viewport is told *which line* it is looking at and how far above
    /// it to sit, and it goes on looking at that line while the shell prints under
    /// it. Computed as a row number, the same jump would slide by one line for
    /// every line of output that arrived after it.
    ///
    /// The offset is negative because `scroll_y = anchor_y(source) + local_offset`
    /// — lifting the viewport's top *above* the line is what puts the line eight
    /// pixels down the pane, which is the mock-up's own `line.offsetTop - 8`.
    ///
    /// MUTATIONS:
    /// ① drop the sign from `local_offset` — the row lands eight pixels off the
    ///    top of the pane instead of eight pixels into it, and the first
    ///    assertion goes red;
    /// ② store a row number and re-derive `scroll_y` from it — the second block
    ///    goes red the moment a line is appended, which is the whole claim.
    #[test]
    fn a_jump_to_a_commands_own_line_keeps_naming_that_line_while_the_shell_prints() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(64).unwrap());
        let mut document = HistoryDocument::default();
        let append = |store: &mut TranscriptStore, document: &mut HistoryDocument, text: &str| {
            let line = store
                .capture(CapturedRow::plain(text, false))
                .finalized
                .remove(0);
            let id = line.line.id;
            document.finalize_transaction(line);
            id
        };
        append(&mut store, &mut document, "first");
        let command = append(&mut store, &mut document, "cargo test");
        append(&mut store, &mut document, "output");

        let mut projection = ViewportProjection::new(
            key(40),
            DetectionRevision(1),
            nz32(24),
            cell_height(),
            SourceGeneration(1),
            GridGeneration(1),
        );
        projection.project(&document);
        // What `jump_to_command_mark` builds: the mark's own `start` anchor, and
        // eight logical pixels of lift.
        let eight_px = -8 * SUBPIXELS_PER_PX;
        projection.set_scroll_anchor(Some(ScrollAnchor {
            source: ContentAnchor::History {
                id: command,
                offset: GraphemeOffset(0),
                bias: Bias::Before,
                generation: SourceGeneration(1),
            },
            local_offset: eight_px,
        }));
        let landed = projection
            .scroll_y(&document)
            .expect("the anchor resolves")
            .expect("the viewport is anchored");
        // One line of eighteen pixels above it, less the eight of lift.
        assert_eq!(landed, 18 * SUBPIXELS_PER_PX + eight_px);

        // Now the shell goes on printing. The viewport is re-projected against a
        // taller document and still names the same line: the pixels below it grew
        // and the pixels above it did not.
        for text in ["running 1 test", "test result: ok", "done"] {
            append(&mut store, &mut document, text);
        }
        projection.project(&document);
        assert_eq!(
            projection
                .scroll_y(&document)
                .expect("the anchor still resolves")
                .expect("and the viewport is still anchored"),
            landed,
            "an anchored jump does not drift under output"
        );
        assert!(
            matches!(projection.scroll_anchor(), Some(anchor)
            if anchor.source == ContentAnchor::History {
                id: command,
                offset: GraphemeOffset(0),
                bias: Bias::Before,
                generation: SourceGeneration(1),
            }),
            "and it is still the command's own line that is being named"
        );
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
            inline_runs: Vec::new(),
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
                lang_rev: 0,
                profile_rev: 0,
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
                    inline_runs: Vec::new(),
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
                    inline_runs: Vec::new(),
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
                inline_runs: Vec::new(),
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
            inline_runs: Vec::new(),
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
                inline_runs: Vec::new(),
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
