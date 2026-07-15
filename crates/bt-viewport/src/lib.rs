//! Per-viewport projection, layout cache, fixed-point height tree and scroll anchoring.

use std::collections::{HashMap, HashSet};

use bt_doc::{
    AnchorError, ContentAnchor, DetectionRevision, HistoryDocument, LayoutKey, ScreenId,
    ViewGeneration,
};
use bt_transcript::{SourceGeneration, TranscriptId};

pub const SUBPIXELS_PER_PX: i64 = 1024;

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

#[derive(Clone, Debug, Default)]
pub struct HeightTree {
    heights: Vec<i64>,
    fenwick: Vec<i64>,
}

impl HeightTree {
    pub fn rebuild(&mut self, heights: impl IntoIterator<Item = i64>) {
        self.heights = heights.into_iter().collect();
        self.fenwick = vec![0; self.heights.len() + 1];
        for index in 0..self.heights.len() {
            let value = self.heights[index];
            self.add(index, value);
        }
    }

    fn add(&mut self, index: usize, delta: i64) {
        let mut i = index + 1;
        while i < self.fenwick.len() {
            self.fenwick[i] += delta;
            i += i & i.wrapping_neg();
        }
    }

    pub fn set(&mut self, index: usize, value: i64) {
        let delta = value - self.heights[index];
        self.heights[index] = value;
        self.add(index, delta);
    }

    pub fn prefix_sum(&self, count: usize) -> i64 {
        let mut i = count.min(self.heights.len());
        let mut sum = 0;
        while i > 0 {
            sum += self.fenwick[i];
            i &= i - 1;
        }
        sum
    }

    pub fn total(&self) -> i64 {
        self.prefix_sum(self.heights.len())
    }
    pub fn get(&self, index: usize) -> Option<i64> {
        self.heights.get(index).copied()
    }
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
    live_rows: u32,
    cell_height_subpixels: i64,
    cache_misses: u64,
}

impl ViewportProjection {
    pub fn new(
        layout_key: LayoutKey,
        detection_rev: DetectionRevision,
        live_rows: u32,
        cell_height_subpixels: i64,
    ) -> Self {
        assert!(layout_key.width_cells > 0);
        assert!(live_rows > 0);
        assert!(cell_height_subpixels > 0);
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
    pub fn set_live_rows(&mut self, live_rows: u32) {
        assert!(live_rows > 0);
        self.live_rows = live_rows;
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
                        let visual_lines = graphemes.max(1).div_ceil(self.layout_key.width_cells);
                        MeasuredLayout {
                            visual_lines,
                            height: visual_lines as i64 * self.cell_height_subpixels,
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
            ContentAnchor::History { id, offset, .. } => {
                let index = self
                    .ordered_ids
                    .iter()
                    .position(|candidate| candidate == id)
                    .ok_or(AnchorError::UnknownAnchor)?;
                let entry = document
                    .entries()
                    .get(id)
                    .ok_or(AnchorError::UnknownAnchor)?;
                let max_offset = entry.line.grapheme_boundaries.len().saturating_sub(1) as u32;
                let local_y = if self.artifact_heights.contains_key(id) {
                    0
                } else {
                    let row = offset.0.min(max_offset) / self.layout_key.width_cells;
                    row as i64 * self.cell_height_subpixels
                };
                Ok(self.heights.prefix_sum(index) + local_y)
            }
            ContentAnchor::Staging { .. } => Ok(self.heights.total()),
            ContentAnchor::Live {
                screen: ScreenId::Primary,
                point,
                ..
            } => {
                if point.row >= self.live_rows {
                    return Err(AnchorError::LiveOutOfBounds);
                }
                Ok(self.heights.total() + point.row as i64 * self.cell_height_subpixels)
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
    use bt_transcript::{CapturedRow, TranscriptStore};

    fn history() -> HistoryDocument {
        let mut store = TranscriptStore::new(8);
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
            width_cells,
            dpi_milli: 1000,
            font_rev: 1,
            theme_rev: 1,
        }
    }

    #[test]
    fn g2_two_widths_have_independent_height_selection_and_scroll_anchor() {
        let document = history();
        let anchor = ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 0, column: 0 },
            bias: Bias::Before,
            generation: GridGeneration(1),
        };
        let mut narrow =
            ViewportProjection::new(key(4), DetectionRevision(1), 24, 18 * SUBPIXELS_PER_PX);
        let mut wide =
            ViewportProjection::new(key(8), DetectionRevision(1), 24, 18 * SUBPIXELS_PER_PX);
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
    fn fixed_point_tree_updates_without_float_drift() {
        let mut tree = HeightTree::default();
        tree.rebuild([i64::MAX / 8, i64::MAX / 8, 42]);
        let before = tree.total();
        tree.set(2, 84);
        assert_eq!(tree.total(), before + 42);
    }

    #[test]
    fn live_anchor_outside_grid_is_rejected() {
        let document = history();
        let mut projection =
            ViewportProjection::new(key(8), DetectionRevision(1), 2, 18 * SUBPIXELS_PER_PX);
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
}
