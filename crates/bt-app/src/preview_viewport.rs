//! Estimated Markdown geometry, occurrence remapping and viewport realization.
use crate::*;
use std::ops::Range;

/// A replacement's coordinates in the document immediately before the edit.
/// No text snapshot: retaining one would make the next edit copy the whole body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Edit {
    pub at: usize,
    pub removed: usize,
    pub inserted: usize,
}

impl Edit {
    fn map(self, at: usize, downstream: bool) -> usize {
        if at < self.at || (at == self.at && !downstream) {
            at
        } else if at >= self.at + self.removed {
            at - self.removed + self.inserted
        } else {
            self.at + if downstream { self.inserted } else { 0 }
        }
    }
}

fn map_byte(mut at: usize, edits: &[Edit], downstream: bool) -> usize {
    for edit in edits {
        at = edit.map(at, downstream);
    }
    at
}

/// Compact local boxes and a Fenwick tree of (preceding margin + height).
/// A height correction and a top lookup touch O(log blocks) numbers. No suffix
/// of absolute tops is rewritten on a scroll. f64 sums avoid large-page drift.
#[derive(Clone, Debug, Default)]
pub(super) struct Layout {
    // Keep the document enum compact; the index belongs to Markdown alone.
    inner: Box<LayoutIndex>,
}

#[derive(Clone, Debug, Default)]
struct LayoutIndex {
    local: Vec<MarkdownBlockLayout>,
    gaps: Vec<f32>,
    sums: Vec<f64>,
}

impl Layout {
    fn new(local: Vec<MarkdownBlockLayout>, gaps: Vec<f32>) -> Self {
        let mut sums = vec![0.0; local.len() + 1];
        for (index, (box_, gap)) in local.iter().zip(&gaps).enumerate() {
            let at = index + 1;
            sums[at] += f64::from(box_.height) + f64::from(*gap);
            let next = at + (at & at.wrapping_neg());
            if next < sums.len() {
                sums[next] += sums[at];
            }
        }
        Self {
            inner: Box::new(LayoutIndex { local, gaps, sums }),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.inner.local.len()
    }

    fn prefix(&self, mut end: usize) -> f64 {
        let mut sum = 0.0;
        while end > 0 {
            sum += self.inner.sums[end];
            end &= end - 1;
        }
        sum
    }

    pub(super) fn get(&self, index: usize) -> Option<MarkdownBlockLayout> {
        let mut placed = self.inner.local.get(index)?.clone();
        placed.top = (self.prefix(index) + f64::from(self.inner.gaps[index])) as f32;
        Some(placed)
    }

    pub(super) fn last(&self) -> Option<MarkdownBlockLayout> {
        self.get(self.len().checked_sub(1)?)
    }

    pub(super) fn extent(&self) -> f32 {
        self.prefix(self.len()) as f32
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = MarkdownBlockLayout> + '_ {
        (0..self.len()).map(|index| self.get(index).unwrap())
    }

    fn set(&mut self, index: usize, box_: MarkdownBlockLayout) {
        let delta = f64::from(box_.height) - f64::from(self.inner.local[index].height);
        self.inner.local[index] = box_;
        let mut at = index + 1;
        while at < self.inner.sums.len() {
            #[cfg(test)]
            preview_typing::count("height tree updates", 1);
            self.inner.sums[at] += delta;
            at += at & at.wrapping_neg();
        }
    }

    /// First block whose bottom is after y, using tree descent rather than a
    /// scan (or binary searching prefix queries).
    fn at_y(&self, y: f32) -> usize {
        let mut at = 0;
        let mut sum = 0.0;
        let mut bit = self.inner.sums.len().next_power_of_two() / 2;
        while bit != 0 {
            let next = at + bit;
            if next < self.inner.sums.len() && sum + self.inner.sums[next] <= f64::from(y) {
                at = next;
                sum += self.inner.sums[next];
            }
            bit >>= 1;
        }
        at.min(self.len())
    }

    pub(super) fn visible(&self, from: f32, to: f32) -> Range<usize> {
        if to <= from {
            return 0..0;
        }
        let start = self.at_y(from);
        let mut end = self.at_y(to).saturating_add(1).min(self.len());
        if end > start && self.get(end - 1).is_some_and(|b| b.top >= to) {
            end -= 1;
        }
        end = end.max(start);
        start..end
    }
}

impl From<Vec<MarkdownBlockLayout>> for Layout {
    fn from(local: Vec<MarkdownBlockLayout>) -> Self {
        let mut bottom = 0.0;
        let gaps = local
            .iter()
            .map(|box_| {
                let gap = (box_.top - bottom).max(0.0);
                bottom = box_.top + box_.height;
                gap
            })
            .collect();
        Self::new(local, gaps)
    }
}

impl FromIterator<MarkdownBlockLayout> for Layout {
    fn from_iter<T: IntoIterator<Item = MarkdownBlockLayout>>(iter: T) -> Self {
        Vec::from_iter(iter).into()
    }
}

#[cfg(test)]
impl<const N: usize> From<[MarkdownBlockLayout; N]> for Layout {
    fn from(local: [MarkdownBlockLayout; N]) -> Self {
        Vec::from(local).into()
    }
}

impl PartialEq for Layout {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

/// Indexing is used by eager layout oracles only; production uses get/visible.
#[cfg(test)]
impl std::ops::Index<usize> for Layout {
    type Output = MarkdownBlockLayout;
    fn index(&self, index: usize) -> &Self::Output {
        &self.inner.local[index]
    }
}

#[derive(Clone, Debug)]
pub(super) struct Record {
    id: u64,
    pub(super) exact: bool,
    intrinsic: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct State {
    pub(super) records: Vec<Record>,
    pub(super) unknown: std::collections::BTreeSet<usize>,
    next_id: u64,
    /// New index -> old index, derived from edit coordinates, never text hashes.
    pub(super) remap: Vec<Option<usize>>,
    pub(super) height: f32,
    pub(super) images: Vec<usize>,
}

#[derive(Clone, Copy)]
pub(super) struct View {
    pub scroll: f32,
    pub height: f32,
    pub padding: f32,
    pub end: bool,
}

impl View {
    fn band(self, layout: &Layout) -> Range<usize> {
        // Half a viewport on each side. No document-sized shaping budget.
        let margin = self.height * 0.5;
        layout.visible(
            self.scroll - self.padding - margin,
            self.scroll - self.padding + self.height + margin,
        )
    }

    fn clamp(&mut self, layout: &Layout) {
        let max = (layout.extent() + self.padding * 2.0 - self.height).max(0.0);
        self.scroll = if self.end {
            max
        } else {
            self.scroll.clamp(0.0, max)
        };
    }
}

/// Reuse is verified against the parsed block too: source equality alone is
/// insufficient when emphasis crosses a parser-generated image boundary.
pub(super) struct Reconcile<'a> {
    pub old: &'a PreviewDocument,
    pub blocks: &'a [preview::MarkdownBlock],
    pub ranges: &'a [Range<usize>],
    pub content: &'a str,
    pub source: &'a SourceBlocks,
    pub frame: preview_wrap::Frame,
    pub edits: Option<&'a [Edit]>,
    pub art_changed: bool,
}

impl State {
    /// The viewport is this tall now, and nothing in it is pending.
    pub(super) const fn fit_height(&mut self, height: f32) {
        self.height = height;
    }

    pub(super) fn reconcile(input: Reconcile<'_>) -> (Self, Layout, Vec<MarkdownBlockIntrinsic>) {
        let Reconcile {
            old,
            blocks,
            ranges,
            content,
            source,
            frame,
            edits,
            art_changed,
        } = input;
        let metrics = frame.metrics();
        let old = match old {
            PreviewDocument::Markdown {
                blocks,
                ranges,
                source,
                layout,
                intrinsic,
                wrap,
                ..
            } if edits.is_some() => Some((blocks, ranges, source, layout, intrinsic, wrap)),
            _ => None,
        };
        let mut state = Self {
            next_id: old.map_or(0, |o| o.5.viewport.next_id),
            ..Self::default()
        };
        let mut candidates = std::collections::HashMap::new();
        let mut ends = std::collections::HashMap::new();
        let mut starts = std::collections::HashMap::new();
        let mut used = vec![false; old.map_or(0, |o| o.0.len())];
        if let Some((_, old_ranges, _, _, _, wrap)) = old {
            for (index, range) in old_ranges.iter().enumerate() {
                if index < wrap.viewport.records.len() {
                    let start = map_byte(range.start, edits.unwrap(), true);
                    let end = map_byte(range.end, edits.unwrap(), false);
                    if end > start {
                        candidates.insert((start, end), index);
                        ends.insert(end, (start, index));
                        starts.insert(start, (end, index));
                    }
                }
            }
        }
        let mut local = Vec::with_capacity(blocks.len());
        let mut intrinsic = Vec::with_capacity(blocks.len());
        let mut gaps = Vec::with_capacity(blocks.len());
        let mut previous_bottom: f32 = 0.0;
        for (index, block) in blocks.iter().enumerate() {
            if matches!(block, preview::MarkdownBlock::Image(_)) {
                state.images.push(index);
            }
            let range = &ranges[index];
            let mapped = candidates
                .remove(&(range.start, range.end))
                .filter(|&i| !used[i])
                .or_else(|| {
                    let &(start, index) = ends.get(&range.end)?;
                    (!used[index] && range.start <= start && start < range.end).then_some(index)
                })
                .or_else(|| {
                    let &(end, index) = starts.get(&range.start)?;
                    (!used[index] && range.start < end && end <= range.end).then_some(index)
                });
            if let Some(index) = mapped {
                used[index] = true;
                ends.remove(&range.end);
            }
            let reuse = mapped.and_then(|i| {
                let old = old?;
                (old.0[i] == *block).then_some((i, old))
            });
            let old_index = mapped;
            let (record, box_, width) = if let Some((i, old)) = reuse {
                let old_record = &old.5.viewport.records[i];
                let same_source = match (old.2.get(i), source.get(index)) {
                    (None, None) => true,
                    (Some(MarkdownCaretBlock::Prose(a)), Some(MarkdownCaretBlock::Prose(b))) => {
                        a.text == b.text && a.heading == b.heading
                    }
                    (Some(MarkdownCaretBlock::Mono(a)), Some(MarkdownCaretBlock::Mono(b))) => {
                        a.text == b.text
                    }
                    _ => false,
                };
                let same_frame = old.5.frame == Some(frame);
                let same_intrinsic =
                    old.5.frame.is_some_and(|f| f.intrinsic_eq(frame)) && !art_changed;
                (
                    Record {
                        id: old_record.id,
                        exact: old_record.exact && same_source && same_frame && !art_changed,
                        intrinsic: old_record.intrinsic && same_intrinsic,
                    },
                    old.3.inner.local[i].clone(),
                    if same_intrinsic {
                        old.4[i].clone()
                    } else {
                        MarkdownBlockIntrinsic::default()
                    },
                )
            } else {
                state.next_id += 1;
                let id = mapped
                    .and_then(|i| old.map(|o| o.5.viewport.records[i].id))
                    .unwrap_or(state.next_id);
                (
                    Record {
                        id,
                        exact: false,
                        intrinsic: false,
                    },
                    estimate(
                        block,
                        content.get(range.clone()).unwrap_or_default(),
                        metrics,
                    ),
                    MarkdownBlockIntrinsic::default(),
                )
            };
            let (top, bottom) = preview::markdown_block_margins(
                block,
                index.checked_sub(1).map(|i| &blocks[i]),
                metrics,
            );
            gaps.push(previous_bottom.max(top));
            previous_bottom = bottom;
            if !record.exact {
                state.unknown.insert(index);
            }
            state.records.push(record);
            state.remap.push(old_index);
            local.push(box_);
            intrinsic.push(width);
        }
        (state, Layout::new(local, gaps), intrinsic)
    }

    pub(super) fn pending(&self, layout: &Layout, view: View) -> bool {
        self.unknown.range(view.band(layout)).next().is_some()
    }

    pub(super) fn picture_reach(&self, layout: &Layout, scroll: f32, height: f32) -> PictureReach {
        let above = self
            .images
            .partition_point(|&i| layout.get(i).is_some_and(|b| b.top + b.height <= scroll));
        let after = self.images.partition_point(|&i| {
            layout
                .get(i)
                .is_some_and(|b| b.top < scroll + height.max(0.0))
        });
        PictureReach {
            first: above.saturating_sub(MARKDOWN_PICTURE_MARGIN),
            last: after
                .saturating_sub(1)
                .max(above)
                .saturating_add(MARKDOWN_PICTURE_MARGIN),
        }
    }

    pub(super) fn offsets(&self, old: &[f32]) -> Vec<f32> {
        self.remap
            .iter()
            .map(|i| i.and_then(|i| old.get(i).copied()).unwrap_or(0.0))
            .collect()
    }

    /// Resolve an old occurrence through replacements. A deleted occurrence
    /// falls forward to the block containing/following its transformed byte,
    /// or backward to the final block at EOF. Duplicate content plays no role.
    pub(super) fn remap_anchor(
        &self,
        anchor: &mut Anchor,
        old_ranges: &[Range<usize>],
        ranges: &[Range<usize>],
        edits: &[Edit],
    ) {
        if self.records.is_empty() {
            anchor.index = 0;
            anchor.file_byte = None;
            anchor.position = Position::Pixel(0.0);
            return;
        }
        let byte = anchor.file_byte.map(|byte| map_byte(byte, edits, true));
        if let Some(index) = self.records.iter().position(|r| r.id == anchor.id)
            && byte.is_none_or(|byte| ranges[index].start <= byte && byte <= ranges[index].end)
        {
            anchor.index = index;
            return;
        }
        let old = old_ranges.get(anchor.index);
        let deleted =
            old.is_none_or(|r| map_byte(r.start, edits, true) >= map_byte(r.end, edits, false));
        let byte = byte.unwrap_or_else(|| map_byte(old.map_or(0, |r| r.start), edits, true));
        let index = ranges
            .partition_point(|r| r.end <= byte)
            .min(ranges.len().saturating_sub(1));
        if let Some(record) = self.records.get(index) {
            anchor.id = record.id;
            anchor.index = index;
            if deleted {
                anchor.position = Position::Pixel(0.0);
                anchor.file_byte = None;
            }
        }
    }

    pub(super) fn anchor(&self, layout: &Layout, view: View) -> Option<Anchor> {
        let index = layout
            .at_y(view.scroll - view.padding)
            .min(layout.len().checked_sub(1)?);
        let placed = layout.get(index)?;
        let local = (view.scroll - view.padding - placed.top).clamp(0.0, placed.height);
        Some(Anchor {
            id: self.records.get(index)?.id,
            index,
            position: Position::Pixel(local),
            file_byte: None,
            screen_y: view.padding + placed.top + local - view.scroll,
        })
    }
}

fn estimate(
    block: &preview::MarkdownBlock,
    source: &str,
    m: seats::PreviewMarkdownMetrics,
) -> MarkdownBlockLayout {
    let height = match block {
        preview::MarkdownBlock::Table { rows, .. } => {
            rows.len() as f32 * (m.line_height + m.table_border + 2.0 * m.table_padding_y)
                + m.table_border
        }
        preview::MarkdownBlock::Code { text, .. } => {
            text.lines().count().max(1) as f32 * m.code_line_height
                + 2.0 * (m.code_padding_y + m.code_border)
        }
        preview::MarkdownBlock::Heading { level, .. } => {
            source.lines().count().max(1) as f32 * m.heading_line_height(*level)
                + m.heading_rule_extent(*level)
        }
        preview::MarkdownBlock::Rule => m.rule_thickness,
        _ => source.lines().count().max(1) as f32 * m.line_height,
    };
    MarkdownBlockLayout::solid(height.max(1.0))
}

#[derive(Clone, Debug)]
enum Position {
    Pixel(f32),
    Text {
        paragraph: usize,
        byte: usize,
        downstream: bool,
        within: f32,
    },
    Mono {
        byte: usize,
        within: f32,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Anchor {
    id: u64,
    index: usize,
    position: Position,
    screen_y: f32,
    file_byte: Option<usize>,
}

pub(super) type RowsMeasure<'a> =
    dyn FnMut(&bt_render::PreviewParagraph) -> Vec<bt_render::PreviewTextRow> + 'a;

pub(super) trait Measure {
    fn width(&mut self, runs: &[bt_render::PreviewRun], font: f32, line: f32) -> f32;
    fn wrap(&mut self, runs: &[bt_render::PreviewRun], width: f32, font: f32, line: f32) -> f32;
    fn rows(&mut self, paragraph: &bt_render::PreviewParagraph) -> Vec<bt_render::PreviewTextRow>;
}

impl Anchor {
    pub(super) fn capture_text(
        &mut self,
        paragraphs: &[bt_render::PreviewParagraph],
        rows: &mut RowsMeasure<'_>,
    ) {
        let Position::Pixel(local) = self.position else {
            return;
        };
        for (paragraph, p) in paragraphs.iter().enumerate() {
            let measured = rows(p);
            for (r, row) in measured.iter().enumerate() {
                if local < row.top + row.height
                    || (paragraph + 1 == paragraphs.len() && r + 1 == measured.len())
                {
                    // The first seam belongs to THIS row at a soft-wrap boundary.
                    let byte = row.seams.iter().map(|s| s.offset).min().unwrap_or(0);
                    self.position = Position::Text {
                        paragraph,
                        byte,
                        downstream: true,
                        within: (local - row.top).max(0.0) / row.height.max(1.0),
                    };
                    return;
                }
            }
        }
    }

    fn local(&self, paragraphs: &[bt_render::PreviewParagraph], rows: &mut RowsMeasure<'_>) -> f32 {
        match self.position {
            Position::Pixel(y) => y,
            Position::Mono { .. } => 0.0,
            Position::Text {
                paragraph,
                byte,
                downstream,
                within,
            } => {
                let Some(p) = paragraphs.get(paragraph) else {
                    return 0.0;
                };
                let measured = rows(p);
                let mut candidates = measured.iter().filter(|r| {
                    let start = r.seams.iter().map(|s| s.offset).min().unwrap_or(0);
                    let end = r.seams.iter().map(|s| s.offset).max().unwrap_or(0);
                    start <= byte && byte <= end
                });
                let row = if downstream {
                    candidates.next_back()
                } else {
                    candidates.next()
                };
                row.or_else(|| measured.last())
                    .map_or(p.rect[1], |r| r.top + within * r.height)
            }
        }
    }

    fn restore(&self, layout: &Layout, local: f32, view: &mut View) {
        if let Some(placed) = layout.get(self.index) {
            view.scroll = view.padding + placed.top + local - self.screen_y;
        }
        view.clamp(layout);
    }

    pub(super) fn remap_text(
        &mut self,
        source: &SourceBlocks,
        blocks: &[preview::MarkdownBlock],
        ranges: &[Range<usize>],
        maps: &[preview_provenance::BlockOrigins],
        edits: &[Edit],
    ) {
        let Some(byte) = self.file_byte.map(|at| map_byte(at, edits, true)) else {
            return;
        };
        self.file_byte = Some(byte);
        let within = match self.position {
            Position::Text { within, .. } | Position::Mono { within, .. } => within,
            Position::Pixel(_) => 0.0,
        };
        match source.get(self.index) {
            Some(MarkdownCaretBlock::Prose(prose)) => {
                let local = byte.saturating_sub(prose.range.start).min(prose.text.len());
                let paragraph = prose
                    .lines
                    .partition_point(|r| r.start <= local)
                    .saturating_sub(1);
                let at = prose
                    .lines
                    .get(paragraph)
                    .map_or(0, |r| local.saturating_sub(r.start).min(r.len()));
                self.position = Position::Text {
                    paragraph,
                    byte: at,
                    downstream: true,
                    within,
                };
            }
            Some(MarkdownCaretBlock::Mono(mono)) => {
                self.position = Position::Mono {
                    byte: byte.saturating_sub(mono.range.start).min(mono.text.len()),
                    within,
                };
            }
            None => {
                let i = self.index;
                if let Some(place) = preview_provenance::place_of(
                    byte,
                    &blocks[i..i + 1],
                    &ranges[i..i + 1],
                    &maps[i..i + 1],
                ) {
                    self.position = Position::Text {
                        paragraph: place.piece,
                        byte: place.offset,
                        downstream: true,
                        within,
                    };
                } else {
                    self.position = Position::Pixel(0.0);
                }
            }
        }
    }
}

pub(super) struct Realize<'a> {
    pub blocks: &'a [preview::MarkdownBlock],
    pub source: &'a SourceBlocks,
    pub art: PageArt<'a>,
    pub layout: &'a mut Layout,
    pub intrinsic: &'a mut [MarkdownBlockIntrinsic],
    pub state: &'a mut State,
    pub pass: &'a mut preview_wrap::Pass,
    pub bytes: MarkdownSourceBytes<'a>,
    pub intrinsic_pass: IntrinsicPass<'a>,
    pub cache: &'a mut MarkdownIntrinsicCache,
}

impl Realize<'_> {
    /// Make every block in the visible band exact, and say **how many blocks
    /// this call measured** — the number `BT_PREVIEW_TRACE`'s `reflow` line
    /// reports (2026-09-23), so a caret flipping one block into source can be
    /// told apart from a pass that re-measured the page.
    pub(super) fn ensure(
        &mut self,
        view: &mut View,
        anchor: Option<Anchor>,
        measure: &mut dyn Measure,
    ) -> usize {
        let mut realized = 0;
        self.cache
            .ensure_environment(self.pass.frame().intrinsic_environment());
        let anchor = if view.end {
            None
        } else {
            anchor
                .filter(|a| {
                    self.state
                        .records
                        .get(a.index)
                        .is_some_and(|record| record.id == a.id)
                })
                .or_else(|| self.state.anchor(self.layout, *view))
        };
        // Existing callers capture text against the OLD frame before reconcile.
        // An anchor into previously unseen estimates has no text geometry yet.
        // Each iteration makes at least one new block exact, or finishes. Height
        // corrections are batched before restoring the anchor. Newly exposed
        // blocks are admitted until the complete visible band is exact.
        let mut first = true;
        let mut anchor_local = None;
        loop {
            let pending = anchor
                .as_ref()
                .filter(|a| !self.state.records[a.index].exact)
                .map(|a| a.index)
                .or_else(|| {
                    if view.end {
                        self.state
                            .unknown
                            .range(view.band(self.layout))
                            .next_back()
                            .copied()
                    } else {
                        self.state
                            .unknown
                            .range(view.band(self.layout))
                            .next()
                            .copied()
                    }
                });
            if pending.is_none() && !first {
                break;
            }
            first = false;
            if let Some(index) = pending {
                if !self.state.records[index].intrinsic {
                    #[cfg(test)]
                    let _timer = preview_typing::Timer::new("intrinsic");
                    self.intrinsic[index] = measure_markdown_intrinsics(
                        &self.blocks[index..index + 1],
                        MarkdownSourceBytes {
                            content: self.bytes.content,
                            ranges: &self.bytes.ranges[index..index + 1],
                        },
                        self.intrinsic_pass,
                        self.cache,
                        &mut |runs, font, line| measure.width(runs, font, line),
                    )
                    .pop()
                    .unwrap_or_default();
                    #[cfg(test)]
                    if matches!(
                        self.blocks[index],
                        preview::MarkdownBlock::Table { .. } | preview::MarkdownBlock::Code { .. }
                    ) {
                        preview_typing::count("intrinsic blocks", 1);
                    }
                }
                let placed = self.pass.block(
                    &self.blocks[index],
                    &self.intrinsic[index],
                    self.source.get(index),
                    self.art,
                    &mut |runs, width, font, line| measure.wrap(runs, width, font, line),
                );
                self.layout.set(index, placed);
                self.state.records[index].exact = true;
                self.state.unknown.remove(&index);
                self.state.records[index].intrinsic = true;
                realized += 1;
                #[cfg(test)]
                preview_typing::count("realized blocks", 1);
            }
            if let Some(anchor) = &anchor {
                let local = *anchor_local.get_or_insert_with(|| match anchor.position {
                    Position::Pixel(y) => y,
                    Position::Mono { byte, within } => self
                        .source
                        .get(anchor.index)
                        .and_then(MarkdownCaretBlock::mono)
                        .map_or(0.0, |source| {
                            let wrap = source.wrap(self.pass.frame().width());
                            let rows = preview_live::BlockRows {
                                text: &source.text,
                                start: 0,
                                wrap: &wrap,
                            };
                            let (mut row, _) = rows.row_of(byte).unwrap_or((0, 0));
                            if row + 1 < wrap.rows() && rows.offset_at(row + 1, 0) == byte {
                                row += 1;
                            }
                            (row as f32 + within) * source.line_height
                        }),
                    Position::Text {
                        paragraph, within, ..
                    } => {
                        let placed = self.layout.get(anchor.index).unwrap();
                        let paragraphs = self.pass.paragraphs(
                            &self.blocks[anchor.index],
                            self.source.get(anchor.index),
                            &placed,
                            self.art,
                        );
                        if paragraphs.is_empty() {
                            // A raw fence/table returning to its rendered face
                            // still names a source line/cell through provenance.
                            fixed_text_y(
                                &self.blocks[anchor.index],
                                &placed,
                                self.pass.frame().metrics(),
                                paragraph,
                                within,
                            )
                        } else {
                            anchor.local(&paragraphs, &mut |p| measure.rows(p))
                        }
                    }
                });
                anchor.restore(self.layout, local, view);
            } else {
                view.clamp(self.layout);
            }
        }
        self.state.height = view.height;
        realized
    }
}

fn fixed_text_y(
    block: &preview::MarkdownBlock,
    placed: &MarkdownBlockLayout,
    metrics: seats::PreviewMarkdownMetrics,
    piece: usize,
    within: f32,
) -> f32 {
    match block {
        preview::MarkdownBlock::Code { text, .. } => {
            metrics.code_border
                + metrics.code_padding_y
                + (piece.min(text.lines().count().saturating_sub(1)) as f32 + within)
                    * metrics.code_line_height
        }
        preview::MarkdownBlock::Table { rows, .. } => {
            let mut cells = 0;
            let row = rows
                .iter()
                .position(|row| {
                    cells += row.len();
                    piece < cells
                })
                .unwrap_or(rows.len().saturating_sub(1));
            placed.rows.iter().take(row).sum::<f32>()
                + metrics.table_border
                + metrics.table_padding_y
                + within * metrics.line_height
        }
        _ => 0.0,
    }
}

fn fixed_text_position(
    block: &preview::MarkdownBlock,
    placed: &MarkdownBlockLayout,
    metrics: seats::PreviewMarkdownMetrics,
    local: f32,
) -> Option<Position> {
    let (paragraph, within) = match block {
        preview::MarkdownBlock::Code { text, .. } => {
            let row =
                (local - metrics.code_border - metrics.code_padding_y) / metrics.code_line_height;
            if row < 0.0 {
                return None;
            }
            let index = (row.floor() as usize).min(text.lines().count().saturating_sub(1));
            (index, row - index as f32)
        }
        preview::MarkdownBlock::Table { rows, .. } => {
            let local = local - metrics.table_border - metrics.table_padding_y;
            if local < 0.0 {
                return None;
            }
            let mut top = 0.0;
            let mut piece = 0;
            for (index, height) in placed.rows.iter().enumerate() {
                if top + height > local || index + 1 == placed.rows.len() {
                    break;
                }
                top += height;
                piece += rows.get(index).map_or(0, Vec::len);
            }
            (piece, (local - top) / metrics.line_height)
        }
        _ => return None,
    };
    Some(Position::Text {
        paragraph,
        byte: 0,
        downstream: true,
        within,
    })
}

pub(super) struct RuntimeMeasure<'a, G: Glass> {
    pub(super) gpu: &'a mut G::Device,
    pub(super) renderer: &'a mut G,
}

impl<G: Glass> Measure for RuntimeMeasure<'_, G> {
    fn width(&mut self, runs: &[bt_render::PreviewRun], font: f32, line: f32) -> f32 {
        self.renderer
            .measure_preview_paragraph_width(self.gpu, runs, font, line)
    }
    fn wrap(&mut self, runs: &[bt_render::PreviewRun], width: f32, font: f32, line: f32) -> f32 {
        self.renderer
            .measure_preview_paragraph(self.gpu, runs, width, font, line)
    }
    fn rows(&mut self, paragraph: &bt_render::PreviewParagraph) -> Vec<bt_render::PreviewTextRow> {
        self.renderer.measure_preview_rows(self.gpu, paragraph)
    }
}

/// **What one geometry rebuild cost** — [`Runtime::rebuild_markdown_geometry`]'s
/// account of itself, for `BT_PREVIEW_TRACE`'s `reflow` line (2026-09-23).
///
/// The two durations are zero when the trace is closed: nothing is timed then.
/// `realized` is counted either way, because it is a count and costs nothing.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct GeometryCost {
    /// Blocks [`Realize::ensure`] measured.
    pub realized: usize,
    /// [`State::reconcile`]: the old document's records matched to the new
    /// blocks, and a fresh height tree.
    pub reconcile: std::time::Duration,
    /// The anchor remapped and the visible band made exact.
    pub realize: std::time::Duration,
}

pub(super) struct Build<'a> {
    pub blocks: &'a [preview::MarkdownBlock],
    pub bytes: MarkdownSourceBytes<'a>,
    pub source: &'a SourceBlocks,
    pub maps: &'a [preview_provenance::BlockOrigins],
    pub art: PageArt<'a>,
    pub edits: Option<&'a [Edit]>,
    pub art_changed: bool,
}

/// Capture from the geometry/frame that was actually displayed, before any
/// estimated box or measurement environment is replaced.
pub(super) fn capture(
    old: &PreviewDocument,
    view: View,
    measure: &mut dyn Measure,
) -> Option<Anchor> {
    let PreviewDocument::Markdown {
        blocks,
        ranges,
        maps,
        source,
        layout,
        wrap,
        math,
        pictures,
        ..
    } = old
    else {
        return None;
    };
    let mut anchor = wrap.viewport.anchor(layout, view)?;
    if wrap.viewport.records[anchor.index].exact {
        let paragraphs = preview_wrap::anchor_paragraphs(
            &blocks[anchor.index],
            source.get(anchor.index),
            &layout.get(anchor.index)?,
            wrap.frame?,
            PageArt {
                math,
                pictures,
                theme: bt_render::current_theme(),
            },
        );
        anchor.capture_text(&paragraphs, &mut |p| measure.rows(p));
        if source.get(anchor.index).is_none()
            && let Position::Pixel(local) = anchor.position
            && let Some(position) = fixed_text_position(
                &blocks[anchor.index],
                &layout.get(anchor.index)?,
                wrap.frame?.metrics(),
                local,
            )
        {
            anchor.position = position;
        }
        if let Some(MarkdownCaretBlock::Mono(mono)) = source.get(anchor.index)
            && let Position::Pixel(local) = anchor.position
        {
            let wrap = mono.wrap(wrap.frame?.width());
            let rows = preview_live::BlockRows {
                text: &mono.text,
                start: 0,
                wrap: &wrap,
            };
            let row = (local / mono.line_height).floor() as usize;
            anchor.position = Position::Mono {
                byte: rows.offset_at(row, 0),
                within: local / mono.line_height - row as f32,
            };
        }
        anchor.file_byte = match anchor.position {
            Position::Text {
                paragraph, byte, ..
            } => match source.get(anchor.index) {
                Some(MarkdownCaretBlock::Prose(prose)) => Some(prose.line_start(paragraph) + byte),
                _ => preview_provenance::file_offset_of(
                    &preview_select::Place::new(anchor.index, paragraph, byte),
                    blocks,
                    ranges,
                    maps,
                ),
            },
            Position::Mono { byte, .. } => source
                .get(anchor.index)
                .and_then(MarkdownCaretBlock::mono)
                .map(|s| s.range.start + byte),
            Position::Pixel(_) => None,
        };
    }
    Some(anchor)
}

/// One fence's syntax highlighting, from its info string alone (#49).
///
/// **The info string and nothing else.** A ` ```rust ` fence is highlighted as
/// Rust; a fence with no info string is not highlighted at all, and neither is
/// one naming a language whose grammar is not in the box. There is deliberately
/// no sniffing of the fence's contents: an author who wrote three backticks and
/// nothing after them wrote a block of *preformatted text*, and guessing a
/// language for it would colour a directory listing as though it were code.
///
/// The lines are expanded first because that is how the fence draws them — the
/// spans that come back are measured in the columns those expanded lines
/// occupy, which is what the paint below indexes by.
pub(crate) fn markdown_fence_highlight(lang: Option<&str>, text: &str) -> highlight::Highlighting {
    let Some(grammar) = highlight::syntax_for_fence(lang) else {
        return highlight::Highlighting::default();
    };
    let lines: Vec<String> = text.lines().map(preview::expand_tabs).collect();
    highlight::Highlighting::of(&lines, grammar)
}

/// How wide a code fence insists on being: its longest line, plus its border
/// and padding on both sides.
///
/// **The measurer is injected**, which is what makes the ruling ("a fence does
/// not reflow, so the page scrolls to it") assertable without a GPU: the
/// arithmetic around the measurement is the part that can be wrong, and it is
/// all here. Tabs are expanded first, for the same reason the fence's own lines
/// are drawn expanded — a tab is a stop, and a stop the width calculation did
/// not take is a last column that cannot be reached.
pub(crate) fn markdown_fence_width(
    text: &str,
    metrics: seats::PreviewMarkdownMetrics,
    mut measure: impl FnMut(&[bt_render::PreviewRun]) -> f32,
) -> f32 {
    let widest = text
        .lines()
        .map(|line| {
            let runs = [bt_render::PreviewRun {
                text: preview::expand_tabs(line),
                color: [0, 0, 0],
                mono: true,
                bold: false,
                italic: false,
                font_scale: 1.0,
                inline_box_px: None,
            }];
            measure(&runs)
        })
        .fold(0.0_f32, f32::max);
    if widest <= 0.0 {
        return 0.0;
    }
    widest + (metrics.code_border + metrics.code_padding_x) * 2.0
}

/// Everything about a document that a pane's width cannot change, **measured
/// once per block content and remembered** (§7.1.3q, ticket T4).
///
/// A free function with the measurer and the cache both injected, for
/// [`lay_markdown_out`]'s reason twice over: the expensive thing is the
/// measurer, and "an edit to one paragraph re-measured nothing else" is a
/// sentence about how often it was asked — which a test can only say if it owns
/// it.
///
/// **Two kinds are remembered and the rest cost nothing.** A table's columns are
/// a shaping call per cell and a fence's highlighting is a walk of a grammar;
/// every other block's intrinsic is [`MarkdownBlockIntrinsic::default`], which
/// is four zeroes and is cheaper to build than to look up. Before this, one
/// keystroke anywhere in a document re-walked syntect over every fence in it —
/// `highlight.rs` said so in as many words — because the whole vector hung off
/// the document's revision.
pub(crate) fn measure_markdown_intrinsics(
    blocks: &[preview::MarkdownBlock],
    source: MarkdownSourceBytes<'_>,
    pass: IntrinsicPass<'_>,
    cache: &mut MarkdownIntrinsicCache,
    measure: &mut WidthMeasure<'_>,
) -> Vec<MarkdownBlockIntrinsic> {
    let IntrinsicPass {
        metrics,
        math,
        palette,
        scale_ppm,
        math_generation,
    } = pass;
    cache.begin_pass();
    let intrinsics = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            // Which of the two halves of the key this kind needs — and `None`
            // for every kind that is not measured at all, which is what keeps a
            // paragraph out of the map entirely.
            let generation = match block {
                preview::MarkdownBlock::Table { .. } => Some(Some(math_generation)),
                preview::MarkdownBlock::Code { .. } => Some(None),
                _ => None,
            };
            let key = generation.and_then(|generation| source.key(index, scale_ppm, generation));
            if let Some(key) = &key
                && let Some(intrinsic) = cache.get(key)
            {
                return intrinsic;
            }
            let intrinsic = match block {
                preview::MarkdownBlock::Table { rows, .. } => {
                    let count = rows.iter().map(Vec::len).max().unwrap_or(0);
                    if count == 0 {
                        return MarkdownBlockIntrinsic::default();
                    }
                    let columns = markdown_table_columns(rows, metrics, |cell, heading| {
                        let runs = markdown_runs(cell, palette, heading, math, metrics.font_size);
                        measure(&runs, metrics.font_size, metrics.line_height)
                    });
                    // Plus the hairline that closes the last row and the one down
                    // the far edge: `border-collapse` draws one between
                    // neighbours and one at each end.
                    let width = columns.iter().sum::<f32>() + metrics.table_border;
                    MarkdownBlockIntrinsic {
                        columns,
                        width,
                        rows: rows.len(),
                        ..MarkdownBlockIntrinsic::default()
                    }
                }
                preview::MarkdownBlock::Code { lang, text } => MarkdownBlockIntrinsic {
                    // The fence's own 85% and its own 1.45, not the body's: a
                    // longest line measured at the prose size would reserve a
                    // scroll extent the fence never uses.
                    width: markdown_fence_width(text, metrics, |runs| {
                        measure(runs, metrics.code_font, metrics.code_line_height)
                    }),
                    rows: text.lines().count().max(1),
                    highlight: markdown_fence_highlight(lang.as_deref(), text),
                    ..MarkdownBlockIntrinsic::default()
                },
                _ => MarkdownBlockIntrinsic::default(),
            };
            if let Some(key) = key {
                cache.insert(key, intrinsic.clone());
            }
            intrinsic
        })
        .collect();
    cache.end_pass();
    intrinsics
}

#[cfg(test)]
#[path = "preview_viewport_tests.rs"]
pub(crate) mod tests;
