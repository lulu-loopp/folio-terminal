//! Per-viewport projection, layout cache and scroll anchoring.

mod height_tree;

use std::{
    collections::{HashMap, HashSet},
    num::{NonZeroI64, NonZeroU32},
};

use bt_doc::{
    AnchorError, ContentAnchor, DetectionRevision, GridGeneration, HistoryDocument, LayoutKey,
    ScreenId, ViewGeneration,
};
use bt_transcript::{SourceGeneration, TranscriptId};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewSelection {
    pub start: ContentAnchor,
    pub end: ContentAnchor,
}

#[derive(Clone, Debug)]
pub struct ViewportProjection {
    layout_key: LayoutKey,
    detection_rev: DetectionRevision,
    cache: HashMap<LayoutCacheKey, MeasuredLayout>,
    artifact_heights: HashMap<TranscriptId, i64>,
    ordered_ids: Vec<TranscriptId>,
    heights: HeightTree,
    scroll_anchor: Option<ScrollAnchor>,
    selection: Option<ViewSelection>,
    view_generation: ViewGeneration,
    live_rows: NonZeroU32,
    cell_height_subpixels: NonZeroI64,
    source_generation: SourceGeneration,
    grid_generation: GridGeneration,
    cache_misses: u64,
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
            heights: HeightTree::default(),
            scroll_anchor: None,
            selection: None,
            view_generation: ViewGeneration(1),
            live_rows,
            cell_height_subpixels,
            source_generation,
            grid_generation,
            cache_misses: 0,
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
        self.scroll_anchor.as_ref()
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

    pub fn set_selection(&mut self, selection: Option<ViewSelection>) {
        self.selection = selection;
    }
    pub fn set_scroll_anchor(&mut self, anchor: Option<ScrollAnchor>) {
        self.scroll_anchor = anchor;
    }
    pub fn set_artifact_height(&mut self, id: TranscriptId, height_subpixels: i64) {
        if self.artifact_heights.insert(id, height_subpixels) != Some(height_subpixels) {
            self.cache.retain(|key, _| key.span.start != id);
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
        }
        self.artifact_heights = next;
    }
    pub fn set_live_state(
        &mut self,
        live_rows: NonZeroU32,
        source_generation: SourceGeneration,
        grid_generation: GridGeneration,
    ) {
        self.live_rows = live_rows;
        self.source_generation = source_generation;
        self.grid_generation = grid_generation;
    }

    pub fn project(&mut self, document: &HistoryDocument) {
        self.ordered_ids.clear();
        let mut heights = Vec::new();
        for (id, entry) in document.entries() {
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
                        let graphemes =
                            entry.line.grapheme_boundaries.len().saturating_sub(1) as u32;
                        let visual_lines =
                            graphemes.max(1).div_ceil(self.layout_key.width_cells.get());
                        MeasuredLayout {
                            visual_lines,
                            height: visual_lines as i64 * self.cell_height_subpixels.get(),
                        }
                    }
                };
                self.cache.insert(cache_key, measured);
                measured
            };
            heights.push(measured.height);
        }
        self.heights.rebuild(heights);
        self.view_generation.0 += 1;
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
        self.scroll_anchor
            .as_ref()
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
            self.layout_key = layout_key;
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
            self.project(document);
        }
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
}
