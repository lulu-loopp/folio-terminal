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
struct Record {
    id: u64,
    exact: bool,
    intrinsic: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct State {
    records: Vec<Record>,
    unknown: std::collections::BTreeSet<usize>,
    next_id: u64,
    /// New index -> old index, derived from edit coordinates, never text hashes.
    pub(super) remap: Vec<Option<usize>>,
    height: f32,
    images: Vec<usize>,
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
    pub source: Option<&'a MarkdownCaretBlock>,
    pub frame: preview_wrap::Frame,
    pub edits: Option<&'a [Edit]>,
    pub art_changed: bool,
}

impl State {
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
                let same_source = match (
                    old.2.as_deref().filter(|s| s.index() == i),
                    source.filter(|s| s.index() == index),
                ) {
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

    fn remap_text(
        &mut self,
        source: Option<&MarkdownCaretBlock>,
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
        match source.filter(|s| s.index() == self.index) {
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
    pub source: Option<&'a MarkdownCaretBlock>,
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
    pub(super) fn ensure(
        &mut self,
        view: &mut View,
        anchor: Option<Anchor>,
        measure: &mut dyn Measure,
    ) {
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
                    self.source.filter(|s| s.index() == index),
                    self.art,
                    &mut |runs, width, font, line| measure.wrap(runs, width, font, line),
                );
                self.layout.set(index, placed);
                self.state.records[index].exact = true;
                self.state.unknown.remove(&index);
                self.state.records[index].intrinsic = true;
                #[cfg(test)]
                preview_typing::count("realized blocks", 1);
            }
            if let Some(anchor) = &anchor {
                let local = *anchor_local.get_or_insert_with(|| match anchor.position {
                    Position::Pixel(y) => y,
                    Position::Mono { byte, within } => self
                        .source
                        .filter(|s| s.index() == anchor.index)
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
                            self.source.filter(|s| s.index() == anchor.index),
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

struct RuntimeMeasure<'a> {
    gpu: &'a mut GpuContext,
    renderer: &'a mut WindowRenderer,
}

impl Measure for RuntimeMeasure<'_> {
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

pub(super) struct Build<'a> {
    pub blocks: &'a [preview::MarkdownBlock],
    pub bytes: MarkdownSourceBytes<'a>,
    pub source: Option<&'a MarkdownCaretBlock>,
    pub maps: &'a [preview_provenance::BlockOrigins],
    pub art: PageArt<'a>,
    pub edits: Option<&'a [Edit]>,
    pub art_changed: bool,
}

/// Capture from the geometry/frame that was actually displayed, before any
/// estimated box or measurement environment is replaced.
fn capture(old: &PreviewDocument, view: View, measure: &mut dyn Measure) -> Option<Anchor> {
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
            source.as_deref().filter(|s| s.index() == anchor.index),
            &layout.get(anchor.index)?,
            wrap.frame?,
            PageArt {
                math,
                pictures,
                theme: bt_render::current_theme(),
            },
        );
        anchor.capture_text(&paragraphs, &mut |p| measure.rows(p));
        if source.as_deref().is_none_or(|s| s.index() != anchor.index)
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
        if let Some(MarkdownCaretBlock::Mono(mono)) =
            source.as_deref().filter(|s| s.index() == anchor.index)
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
            } => match source.as_deref().filter(|s| s.index() == anchor.index) {
                Some(MarkdownCaretBlock::Prose(prose)) => Some(prose.line_start(paragraph) + byte),
                _ => preview_provenance::file_offset_of(
                    &preview_select::Place::new(anchor.index, paragraph, byte),
                    blocks,
                    ranges,
                    maps,
                ),
            },
            Position::Mono { byte, .. } => source
                .as_deref()
                .and_then(MarkdownCaretBlock::mono)
                .map(|s| s.range.start + byte),
            Position::Pixel(_) => None,
        };
    }
    Some(anchor)
}

impl Runtime<'_> {
    /// Moving between image bands is also viewport work. Keep the parse, height
    /// index and prose records; resolve only the indexed image occurrences in
    /// reach. Retiring/entering images become estimates until visible again.
    pub(super) fn update_markdown_picture_reach(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
        document: Option<&Path>,
        reach: PictureReach,
    ) {
        let previous = self
            .preview_pane(surface)
            .and_then(|p| p.doc_key.as_ref())
            .map_or(PictureReach::from_the_top(), |k| k.art.picture_reach);
        let doc = std::mem::take(&mut self.preview_pane_mut(surface).doc);
        let PreviewDocument::Markdown {
            blocks,
            ranges,
            maps,
            source,
            layout,
            intrinsic,
            pictures,
            math,
            mut wrap,
        } = doc
        else {
            self.preview_pane_mut(surface).doc = doc;
            return;
        };
        let state = &mut Arc::make_mut(&mut wrap).viewport;
        let selected: Vec<_> = state
            .images
            .iter()
            .skip(reach.first)
            .take(reach.last.saturating_sub(reach.first) + 1)
            .map(|&i| blocks[i].clone())
            .collect();
        for band in [previous, reach] {
            for &index in state
                .images
                .iter()
                .skip(band.first)
                .take(band.last.saturating_sub(band.first) + 1)
            {
                state.records[index].exact = false;
                state.unknown.insert(index);
            }
        }
        let width = wrap.frame.expect("Markdown frame").width();
        let pictures = self.resolve_document_pictures(
            &selected,
            document,
            width,
            PictureReach {
                first: 0,
                last: selected.len().saturating_sub(1),
            },
            &pictures,
        );
        self.preview_pane_mut(surface)
            .reflow_document(PreviewDocument::Markdown {
                blocks,
                ranges,
                maps,
                source,
                layout,
                intrinsic,
                pictures,
                math,
                wrap,
            });
        self.ensure_markdown_viewport(surface, body, scale);
    }

    /// A caret navigation target is made visible before asking its text rows.
    /// In particular Ctrl+End cannot be clamped to an unrealized paragraph's
    /// estimated height. The normal body rebuild then consumes this same parse.
    pub(super) fn prepare_markdown_caret_view(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
    ) {
        let old_key = self.preview_pane(surface).and_then(|p| p.doc_key.clone());
        let old_scroll = self.preview_pane(surface).map_or(0.0, |p| p.scroll[1]);
        self.rebuild_preview_document(surface, body, scale);
        let target = self.preview_pane(surface).and_then(|pane| {
            let PreviewDocument::Markdown {
                source,
                layout,
                wrap,
                ..
            } = &pane.doc
            else {
                return None;
            };
            let index = source.as_deref()?.index();
            let placed = layout.get(index)?;
            let padding = seats::preview_markdown_metrics(scale).padding_y;
            let top = padding + placed.top;
            (!wrap.viewport.records.get(index)?.exact
                || top + placed.height <= pane.scroll[1]
                || top >= pane.scroll[1] + body[3] - body[1])
                .then_some(top)
        });
        if let Some(top) = target {
            self.preview_pane_mut(surface).scroll[1] = top;
            self.ensure_markdown_viewport(surface, body, scale);
        }
        let changed = self.preview_pane(surface).is_some_and(|p| {
            p.doc_key != old_key || p.scroll[1] != old_scroll || p.md_prose.is_none()
        });
        if changed {
            let prose = self.preview_prose_geometry(surface, scale, None);
            self.preview_pane_mut(surface).md_prose = prose;
        }
    }

    pub(super) fn rebuild_markdown_geometry(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
        old: &PreviewDocument,
        build: Build<'_>,
    ) -> (
        Layout,
        Vec<MarkdownBlockIntrinsic>,
        Arc<preview_wrap::Document>,
    ) {
        let metrics = seats::preview_markdown_metrics(scale);
        let (left, right) = preview::markdown_measure_box(body, metrics);
        let frame =
            preview_wrap::Frame::new(right - left, scale, self.app.gpu.font_environment_epoch());
        let scroll = self.preview_pane(surface).map_or(0.0, |p| p.scroll[1]);
        let mut view = View {
            scroll,
            height: body[3] - body[1],
            padding: metrics.padding_y,
            end: false,
        };
        let mut old_view = view;
        if let PreviewDocument::Markdown { layout, wrap, .. } = old {
            old_view.padding = wrap
                .frame
                .map_or(metrics.padding_y, |f| f.metrics().padding_y);
            if wrap.viewport.height > 0.0 {
                old_view.height = wrap.viewport.height;
            }
            view.end = scroll > 0.0
                && scroll >= layout.extent() + 2.0 * old_view.padding - old_view.height - 0.5;
        }
        let mut measure = RuntimeMeasure {
            gpu: &mut self.app.gpu,
            renderer: &mut self.window.renderer,
        };
        let mut anchor = build
            .edits
            .and_then(|_| capture(old, old_view, &mut measure));
        let mut pass = self
            .window
            .markdown_wraps
            .prepare(old, build.edits.is_some(), frame);
        let (mut state, mut layout, mut intrinsic) = State::reconcile(Reconcile {
            old,
            blocks: build.blocks,
            ranges: build.bytes.ranges,
            content: build.bytes.content,
            source: build.source,
            frame,
            edits: build.edits,
            art_changed: build.art_changed,
        });
        if let Some(anchor) = &mut anchor
            && let PreviewDocument::Markdown { ranges, .. } = old
        {
            state.remap_anchor(
                anchor,
                ranges,
                build.bytes.ranges,
                build.edits.unwrap_or_default(),
            );
            anchor.remap_text(
                build.source,
                build.blocks,
                build.bytes.ranges,
                build.maps,
                build.edits.unwrap_or_default(),
            );
        }
        let palette = bt_render::chrome_palette();
        Realize {
            blocks: build.blocks,
            source: build.source,
            art: build.art,
            layout: &mut layout,
            intrinsic: &mut intrinsic,
            state: &mut state,
            pass: &mut pass,
            bytes: build.bytes,
            intrinsic_pass: IntrinsicPass {
                metrics,
                math: build.art.math,
                palette: &palette,
                scale_ppm: scale_ppm(scale),
                math_generation: self.window.preview_math.generation,
            },
            cache: &mut self.window.markdown_intrinsics,
        }
        .ensure(&mut view, anchor, &mut measure);
        let pane = self.preview_pane_mut(surface);
        pane.scroll[1] = view.scroll;
        let mut wrap = pass.document();
        Arc::make_mut(&mut wrap).viewport = state;
        (layout, intrinsic, wrap)
    }

    /// Called even on parse-key equality: scroll and viewport-height changes do
    /// not change a Markdown parse key. This path never visits the full parse.
    pub(super) fn ensure_markdown_viewport(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
    ) {
        let metrics = seats::preview_markdown_metrics(scale);
        let (mut view, pending) = {
            let Some(pane) = self.preview_pane(surface) else {
                return;
            };
            let PreviewDocument::Markdown { layout, wrap, .. } = &pane.doc else {
                return;
            };
            let view = View {
                scroll: pane.scroll[1],
                height: body[3] - body[1],
                padding: metrics.padding_y,
                end: pane.scroll[1] > 0.0
                    && pane.scroll[1]
                        >= layout.extent() + 2.0 * metrics.padding_y - (body[3] - body[1]) - 0.5,
            };
            (view, wrap.viewport.pending(layout, view))
        };
        if !pending {
            if let PreviewDocument::Markdown { wrap, .. } = &mut self.preview_pane_mut(surface).doc
            {
                Arc::make_mut(wrap).viewport.height = view.height;
            }
            return;
        }
        let old = std::mem::take(&mut self.preview_pane_mut(surface).doc);
        let PreviewDocument::Markdown {
            blocks,
            ranges,
            maps,
            source,
            mut layout,
            mut intrinsic,
            math,
            pictures,
            mut wrap,
        } = old
        else {
            unreachable!();
        };
        let content = self
            .preview_buffer_on(surface)
            .and_then(|b| b.content.clone())
            .unwrap_or_default();
        let mut pass = self.window.markdown_wraps.resume(&wrap);
        let palette = bt_render::chrome_palette();
        let state = &mut Arc::make_mut(&mut wrap).viewport;
        Realize {
            blocks: &blocks,
            source: source.as_deref(),
            art: PageArt {
                math: &math,
                pictures: &pictures,
                theme: bt_render::current_theme(),
            },
            layout: &mut layout,
            intrinsic: &mut intrinsic,
            state,
            pass: &mut pass,
            bytes: MarkdownSourceBytes {
                content: &content,
                ranges: &ranges,
            },
            intrinsic_pass: IntrinsicPass {
                metrics,
                math: &math,
                palette: &palette,
                scale_ppm: scale_ppm(scale),
                math_generation: self.window.preview_math.generation,
            },
            cache: &mut self.window.markdown_intrinsics,
        }
        .ensure(
            &mut view,
            None,
            &mut RuntimeMeasure {
                gpu: &mut self.app.gpu,
                renderer: &mut self.window.renderer,
            },
        );
        let pane = self.preview_pane_mut(surface);
        pane.scroll[1] = view.scroll;
        pane.reflow_document(PreviewDocument::Markdown {
            blocks,
            ranges,
            maps,
            source,
            layout,
            intrinsic,
            math,
            pictures,
            wrap,
        });
    }
}

#[cfg(test)]
#[path = "preview_viewport_tests.rs"]
pub(crate) mod tests;
