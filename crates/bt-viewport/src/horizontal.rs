//! The one horizontal axis: typed columns, the per-line column index, and the window a
//! flattened logical line is read through.
//!
//! `docs/plans/horizontal-scroll/plan.md` §1a and §5. Ladder one, level one — the geometry and
//! the seek; **no consumer is switched over here**. `layout_frozen_line` still wraps a frozen
//! line at the pane's width, and the frame still carries no horizontal state. What this module
//! owns is the arithmetic every later level hangs off, so that the axis is one contract and not
//! a patch repeated at each call site.
//!
//! # The domains this axis covers
//!
//! Ruling of 2026-08-24 (plan §5.1), case A: **history and staging flatten and are horizontally
//! addressable; the live grid keeps its physical rows and is not part of this axis at all.** So
//! nothing here reads a live row, and [`FlattenedExtent`] must never be fed one — a scrollbar
//! that claimed live horizontal columns would be promising a service the live renderer does not
//! offer.
//!
//! # Half-open, everywhere
//!
//! A column interval is `[start, end)` (plan §5.0). `content_extent` is one past the last
//! presentable column, a window is `[x_origin, x_origin + viewport_columns)`, and a wide glyph
//! owns the two columns `[c, c + 2)`. The closed-interval spelling appears nowhere.

use std::collections::{BTreeMap, HashMap};

use bt_doc::{Bias, ContentAnchor};
use bt_transcript::{
    CapturedCell, CellFlags, CellHyperlink, FrozenLine, GraphemeOffset, SourceGeneration,
    StagingId, StyleSpan, TranscriptId,
};
use bt_unicode::{cluster_width, graphemes, text_width};

use crate::{CellAnchor, InferredLink, endpoint, implicit_link_at};

/// A column of the **content**: where a cell sits along a flattened logical line, counted from
/// that line's first column and independent of what any viewport is showing.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentColumn(pub u32);

/// A column of the **viewport**: where a cell is drawn, counted from the left edge of the pane.
///
/// Distinct from [`ContentColumn`] on purpose. The two were the same number for as long as a
/// pane showed a line from its first column, and every place that quietly assumed so is a place
/// a horizontal origin would break silently. [`HorizontalProjection`] is the only thing allowed
/// to convert between them.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ViewportColumn(pub u32);

/// The horizontal half of a frame: how wide the addressable content is, how wide the window is,
/// and where the window sits.
///
/// **The origin cannot be out of range**, because there is no way to write one. It is clamped by
/// [`Self::new`] against the extent and the width it is being built with, and the struct exposes
/// no setter. That is plan §5.3 clause 5 — "first the new viewport columns and the retained
/// extent, then one clamp, then publish the frame" — made a property of the type instead of a
/// rule callers have to remember at four separate call sites.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HorizontalProjection {
    content_extent: u32,
    viewport_columns: u32,
    x_origin: u32,
}

impl HorizontalProjection {
    /// Build the axis from the two facts that decide it, clamping the requested origin.
    ///
    /// `widest_content` is the widest presentable end column of the retained addressable set
    /// ([`FlattenedExtent::widest`]) — **not** a high-water mark, not a captured width, and not
    /// anything the live grid contributed. The extent is at least the width of the window, so a
    /// pane wider than everything in it still has a well-formed axis with a single valid origin.
    #[must_use]
    pub fn new(
        widest_content: ContentColumn,
        viewport_columns: u32,
        requested: ContentColumn,
    ) -> Self {
        let content_extent = widest_content.0.max(viewport_columns);
        let max_origin = content_extent.saturating_sub(viewport_columns);
        Self {
            content_extent,
            viewport_columns,
            x_origin: requested.0.min(max_origin),
        }
    }

    /// The window pinned at the left edge — the axis every pane has today.
    #[must_use]
    pub fn unscrolled(viewport_columns: u32) -> Self {
        Self::new(ContentColumn(0), viewport_columns, ContentColumn(0))
    }

    #[must_use]
    pub fn content_extent(&self) -> ContentColumn {
        ContentColumn(self.content_extent)
    }

    #[must_use]
    pub fn viewport_columns(&self) -> u32 {
        self.viewport_columns
    }

    #[must_use]
    pub fn x_origin(&self) -> ContentColumn {
        ContentColumn(self.x_origin)
    }

    /// The furthest left edge this axis admits: `max(0, content_extent - viewport_columns)`.
    #[must_use]
    pub fn max_x_origin(&self) -> ContentColumn {
        ContentColumn(self.content_extent.saturating_sub(self.viewport_columns))
    }

    #[must_use]
    pub fn is_scrolled(&self) -> bool {
        self.x_origin != 0
    }

    /// One past the last content column the window shows.
    #[must_use]
    pub fn window_end(&self) -> ContentColumn {
        ContentColumn(self.x_origin.saturating_add(self.viewport_columns))
    }

    /// Where a content column is drawn, or `None` when the window does not show it.
    #[must_use]
    pub fn to_viewport(&self, column: ContentColumn) -> Option<ViewportColumn> {
        (column.0 >= self.x_origin && column.0 < self.window_end().0)
            .then(|| ViewportColumn(column.0 - self.x_origin))
    }

    /// What a drawn column means in the content. Total: every viewport column names a content
    /// column, whether or not any content reaches that far.
    #[must_use]
    pub fn to_content(&self, column: ViewportColumn) -> ContentColumn {
        ContentColumn(self.x_origin.saturating_add(column.0))
    }

    /// The same axis with a new origin, clamped again.
    #[must_use]
    pub fn scrolled_to(&self, requested: ContentColumn) -> Self {
        Self::new(
            ContentColumn(self.content_extent),
            self.viewport_columns,
            requested,
        )
    }

    /// The same axis after the retained set or the pane width changed — the one road plan §5.3
    /// clause 5 leaves open, so that a shrinking extent clamps the origin in the same step that
    /// learns about it rather than one frame later.
    #[must_use]
    pub fn reclamped(&self, widest_content: ContentColumn, viewport_columns: u32) -> Self {
        Self::new(
            widest_content,
            viewport_columns,
            ContentColumn(self.x_origin),
        )
    }
}

/// How many columns a flattened logical line presents.
///
/// This is `presentable_end_column` of plan §5.3: what the retained payload can actually be
/// seeked, rendered and copied through. In this build a frozen line is either retained whole or
/// evicted whole — [`bt_transcript::FROZEN_BYTES_PER_LINE`] runs through `evict_oldest` and never
/// truncates a line it keeps — so the retained payload is the whole of `FrozenLine::text` and the
/// two numbers coincide. If a future ceiling ever truncates inside a line, this is the function
/// that has to start following the surviving payload rather than the original width.
#[must_use]
pub fn presentable_end_column(text: &str) -> ContentColumn {
    ContentColumn(text_width(text) as u32)
}

/// The widest presentable line in the retained addressable set, maintained by pairs.
///
/// A monotone maximum is the wrong answer and this is why the structure is a multiset: when the
/// widest line in history is evicted the axis must **shrink**, or the reader keeps a stretch of
/// scrollbar travel that reaches nothing at all (plan §5.3 clause 2). Removal is what a plain
/// `max` cannot do, and rescanning history for a new maximum on every eviction is the O(history)
/// walk clause 3 forbids.
///
/// The live grid is deliberately not representable here: there is no method that takes a row.
#[derive(Clone, Debug, Default)]
pub struct FlattenedExtent {
    widths: BTreeMap<u32, usize>,
    lines: usize,
}

impl FlattenedExtent {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Admit one addressable logical line of `columns` presentable columns.
    pub fn insert(&mut self, columns: ContentColumn) {
        *self.widths.entry(columns.0).or_insert(0) += 1;
        self.lines += 1;
    }

    /// Withdraw one line of `columns` presentable columns — an eviction, a reseal, or the old
    /// generation of a staging line that just grew.
    ///
    /// Paired with [`Self::insert`] by the caller. Withdrawing a width that was never admitted
    /// leaves the set alone rather than corrupting the counts.
    pub fn remove(&mut self, columns: ContentColumn) {
        let Some(count) = self.widths.get_mut(&columns.0) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            self.widths.remove(&columns.0);
        }
        self.lines -= 1;
    }

    /// Replace one line's contribution in a single step, for a staging line that grew in place.
    pub fn replace(&mut self, previous: ContentColumn, current: ContentColumn) {
        self.remove(previous);
        self.insert(current);
    }

    pub fn clear(&mut self) {
        self.widths.clear();
        self.lines = 0;
    }

    /// The widest retained line, or zero when nothing is addressable.
    #[must_use]
    pub fn widest(&self) -> ContentColumn {
        ContentColumn(self.widths.keys().next_back().copied().unwrap_or(0))
    }

    #[must_use]
    pub fn lines(&self) -> usize {
        self.lines
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines == 0
    }
}

/// Where one column of a flattened line lands: the cell that owns it, in every coordinate the
/// materializer needs to carry on from there.
///
/// `column` is the **start** column of the owning cell, so a seek that lands on the second half
/// of a wide glyph reports the glyph's own column and is therefore less than the target. That is
/// the honest answer: the column asked about belongs to that glyph, and the cell it names is the
/// one whose style, link and anchor the reader is entitled to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnSeek {
    pub column: ContentColumn,
    pub byte: u32,
    pub grapheme: u32,
}

/// Locate a column by walking from the front of the line.
///
/// The reference implementation, and the one the indexed path must agree with cell for cell.
#[must_use]
pub fn seek_from_start(text: &str, target: ContentColumn) -> ColumnSeek {
    resume_seek(
        text,
        ColumnSeek {
            column: ContentColumn(0),
            byte: 0,
            grapheme: 0,
        },
        target,
    )
}

/// Walk forward from a known-good position to `target`.
///
/// `from` must name a cell boundary — column, byte offset and grapheme index of a cluster that
/// starts a cell. Every checkpoint this module produces is such a position, which is what makes
/// the index a pure accelerator: resuming from a checkpoint and resuming from the origin run the
/// same loop over the same clusters and cannot disagree.
#[must_use]
pub fn resume_seek(text: &str, from: ColumnSeek, target: ContentColumn) -> ColumnSeek {
    let mut column = from.column.0;
    let mut byte = from.byte;
    let mut grapheme = from.grapheme;
    if column >= target.0 {
        return from;
    }
    for cluster in graphemes(&text[from.byte as usize..]) {
        let width = cluster_width(cluster) as u32;
        if width == 0 {
            // A zero-width cluster joins the cell in front of it and advances no column, so it
            // is never a position a seek can stop at.
            byte += cluster.len() as u32;
            grapheme += 1;
            continue;
        }
        if column.saturating_add(width) > target.0 {
            return ColumnSeek {
                column: ContentColumn(column),
                byte,
                grapheme,
            };
        }
        column += width;
        byte += cluster.len() as u32;
        grapheme += 1;
    }
    ColumnSeek {
        column: ContentColumn(column),
        byte,
        grapheme,
    }
}

/// What one column is, for the single purpose of deciding where a word ends.
///
/// The terminal selection policy, in the coordinate system a flattened line is read in. It is the
/// same rule `word_class` applies to a laid-out cell and deliberately not a second one: a double
/// click must select the same run whether the run is inside the window or crosses its edge, and two
/// implementations of "is this still the same word" would eventually disagree at exactly the seam
/// nobody looks at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WordClass {
    Space,
    Delimiter,
    Word,
}

/// Stable xterm-style shell delimiters. Configuration belongs to the later settings slice.
pub(crate) const WORD_DELIMITERS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

/// The class of the column one cluster **starts** at.
///
/// A wide glyph's second column is not this: it is its lead's spacer, and a spacer is
/// [`WordClass::Word`] wherever it stands, because a cluster can never be split by a word boundary
/// that falls inside it. [`column_classes`] is where that distinction is applied.
#[must_use]
pub fn cluster_word_class(cluster: &str) -> WordClass {
    if cluster.is_empty() || cluster.chars().all(char::is_whitespace) {
        return WordClass::Space;
    }
    if cluster
        .chars()
        .all(|character| WORD_DELIMITERS.contains(character))
    {
        WordClass::Delimiter
    } else {
        WordClass::Word
    }
}

/// Every column of a flattened line paired with its class, in column order.
///
/// A wide cluster contributes its own class at its lead column and [`WordClass::Word`] at the
/// spacer beside it, which is exactly the pair of answers the two cells `layout_frozen_line`
/// emits give.
fn column_classes(text: &str) -> impl Iterator<Item = WordClass> + '_ {
    graphemes(text).flat_map(|cluster| {
        let width = cluster_width(cluster);
        let class = cluster_word_class(cluster);
        (0..width).map(
            move |offset| {
                if offset == 0 { class } else { WordClass::Word }
            },
        )
    })
}

/// One maximal run of same-class columns, half-open like every other column interval here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnRun {
    pub start: ContentColumn,
    pub end: ContentColumn,
}

/// The run containing `target`, by walking from the front of the line.
///
/// The reference implementation, and the one [`LineColumnIndex::word_run`] must agree with run for
/// run. A target at or past the line's last column reports the empty run there: past the end a
/// flattened row is padding, and padding is nobody's word.
#[must_use]
pub fn word_run_from_start(text: &str, target: ContentColumn) -> ColumnRun {
    let mut start = 0u32;
    let mut column = 0u32;
    let mut current: Option<WordClass> = None;
    for class in column_classes(text) {
        if current != Some(class) {
            if column > target.0 {
                return ColumnRun {
                    start: ContentColumn(start),
                    end: ContentColumn(column),
                };
            }
            start = column;
            current = Some(class);
        }
        column += 1;
    }
    if target.0 < column {
        return ColumnRun {
            start: ContentColumn(start),
            end: ContentColumn(column),
        };
    }
    ColumnRun {
        start: target,
        end: target,
    }
}

/// How many columns apart the checkpoints of a fresh index stand.
///
/// **Columns, not bytes, not code units, not glyphs** (plan §5.2 clause 1). A tunable strategy
/// parameter and nothing more: it is not a format, nothing persists it, and changing it can only
/// change how long a seek takes.
pub const CHECKPOINT_STRIDE_COLUMNS: u32 = 64;

/// What one line's index may cost before it is refused.
///
/// The byte ceiling on a frozen line does **not** bound this (plan §5.2, the adopted doubt):
/// 2,048 bytes is a serialized-payload ceiling, and a run of whitespace, a zero-width mark, or a
/// wide glyph each break the "one byte per column" reasoning in a different direction. So the
/// index carries a cap of its own, and a line that would exceed it simply goes unindexed.
///
/// **The number is read off the line the plan benchmarks.** §1b's pathological case is a hundred
/// thousand graphemes; at stride 64 that is about 1,560 checkpoints, and a checkpoint is sixteen
/// bytes — call it 25 KB, against 100 KB of text the line is already costing. Sixty-four
/// kibibytes therefore indexes that line whole with room for a stride shortened two-fold, and
/// refuses somewhere past a quarter of a million columns, where the linear scan is still correct
/// and the reader has other problems. It was 4 KiB in the first draft, which refused the plan's
/// own benchmark line — the fallback answered correctly and nothing failed, which is precisely
/// why the cap has to be measured rather than guessed.
pub const LINE_INDEX_BUDGET_BYTES: usize = 64 * 1024;

/// What the whole store may hold — about sixty fully indexed pathological lines, or many
/// thousands of ordinary ones. Beyond it new lines simply go unindexed: never a wrong cell, only
/// a slower one.
pub const INDEX_STORE_BUDGET_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ColumnCheckpoint {
    column: u32,
    byte: u32,
    grapheme: u32,
    /// The first column of the word-class run this checkpoint stands in.
    ///
    /// The fourth word of a checkpoint, and it is what makes "which word is this" answerable in
    /// `O(log n + stride)` instead of `O(run)`. A run can be the whole line — a hundred thousand
    /// columns of `abcdefghij` is one word — so walking outward from the window to find a word's
    /// two ends is exactly the `O(line length)` per frame plan §1b forbids. These numbers ascend
    /// with the columns they sit in, so a binary search over them finds the run's far end without
    /// reading the columns in between.
    run_start: u32,
}

/// Sparse column checkpoints for one logical line.
///
/// Every checkpoint is a **self-synchronizing** position: it names the first cluster of a cell,
/// so it can never land inside a wide glyph's second column, inside a combining sequence, or
/// part-way through a cluster (plan §5.2 clause 2). A checkpoint's column is therefore the
/// largest cell start not past its stride multiple, and the seek scans forward from there over a
/// bounded number of columns.
///
/// Styles are looked up by absolute byte range in this build ([`StyleCursor`]), so a checkpoint
/// needs no decoder state to be resumable. That is a fact about `FrozenLine::styles` and not a
/// general licence: a format with incremental run state would have to carry that state here or
/// anchor only at fragment starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineColumnIndex {
    stride: u32,
    columns: u32,
    checkpoints: Vec<ColumnCheckpoint>,
}

impl LineColumnIndex {
    /// Build the index, or refuse when it would cost more than [`LINE_INDEX_BUDGET_BYTES`].
    ///
    /// Refusal is a first-class outcome, not an error: the caller falls back to
    /// [`seek_from_start`] and gets the same cell more slowly (plan §5.2 clause 5).
    #[must_use]
    pub fn build(text: &str, stride: u32, budget_bytes: usize) -> Option<Self> {
        let stride = stride.max(1);
        let capacity = budget_bytes / std::mem::size_of::<ColumnCheckpoint>();
        if capacity == 0 {
            return None;
        }
        let mut checkpoints = Vec::new();
        let mut column = 0u32;
        let mut byte = 0u32;
        let mut next_target = 0u32;
        let mut run_start = 0u32;
        let mut run_class: Option<WordClass> = None;
        for (grapheme, cluster) in graphemes(text).enumerate() {
            let width = cluster_width(cluster) as u32;
            if width > 0 {
                let class = cluster_word_class(cluster);
                if run_class != Some(class) {
                    run_start = column;
                    run_class = Some(class);
                }
                if column >= next_target {
                    if checkpoints.len() == capacity {
                        return None;
                    }
                    checkpoints.push(ColumnCheckpoint {
                        column,
                        byte,
                        grapheme: grapheme as u32,
                        run_start,
                    });
                    // The next multiple strictly past the column just anchored, so a line of wide
                    // glyphs cannot anchor twice inside one stride.
                    next_target = (column / stride + 1).saturating_mul(stride);
                }
                // A wide glyph's second column is its spacer, which is `Word` wherever it stands
                // — see [`column_classes`].
                if width == 2 && run_class != Some(WordClass::Word) {
                    run_start = column + 1;
                    run_class = Some(WordClass::Word);
                }
            }
            column = column.saturating_add(width);
            byte += cluster.len() as u32;
        }
        Some(Self {
            stride,
            columns: column,
            checkpoints,
        })
    }

    #[must_use]
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// How many columns the indexed line presents.
    #[must_use]
    pub fn columns(&self) -> ContentColumn {
        ContentColumn(self.columns)
    }

    #[must_use]
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }

    #[must_use]
    pub fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.checkpoints.capacity() * std::mem::size_of::<ColumnCheckpoint>()
    }

    /// The nearest checkpoint at or before `target`.
    fn anchor(&self, target: ContentColumn) -> ColumnSeek {
        let index = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.column <= target.0)
            .saturating_sub(1);
        self.checkpoints.get(index).map_or(
            ColumnSeek {
                column: ContentColumn(0),
                byte: 0,
                grapheme: 0,
            },
            |checkpoint| ColumnSeek {
                column: ContentColumn(checkpoint.column),
                byte: checkpoint.byte,
                grapheme: checkpoint.grapheme,
            },
        )
    }

    /// Locate a column through the index. Identical to [`seek_from_start`] by construction.
    #[must_use]
    pub fn seek(&self, text: &str, target: ContentColumn) -> ColumnSeek {
        resume_seek(text, self.anchor(target), target)
    }

    /// The word-class run containing `target`, through the index. Identical to
    /// [`word_run_from_start`] by construction, and — unlike it — bounded.
    ///
    /// Two searches and two short scans. The run's **start** is found by resuming from the
    /// checkpoint at or before the target: if the class has not changed between the two, the run
    /// began at or before the checkpoint and the checkpoint already recorded where. The run's
    /// **end** is found by binary-searching the checkpoints for the first one standing in a later
    /// run — everything before it is inside this one — and scanning the last stride's worth of
    /// columns for the boundary itself. Neither half reads the run's interior, which is the point:
    /// a run can be the whole line.
    #[must_use]
    pub fn word_run(&self, text: &str, target: ContentColumn) -> ColumnRun {
        let Some(index) = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.column <= target.0)
            .checked_sub(1)
        else {
            return word_run_from_start(text, target);
        };
        let Some((start, class)) = run_at_from(text, self.checkpoints[index], target.0) else {
            // Past the line's last column there is only padding, and padding is nobody's word.
            return ColumnRun {
                start: target,
                end: target,
            };
        };
        // Run starts ascend with the columns they name, so this is a partition: every checkpoint
        // before it is at or before this run's last column.
        let beyond = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.run_start <= start);
        let from = self.checkpoints[beyond.saturating_sub(1)];
        ColumnRun {
            start: ContentColumn(start),
            end: ContentColumn(run_end_from(text, from, class, start)),
        }
    }
}

/// The run start and class of `target`, resuming from a checkpoint whose own run start is known.
///
/// `None` when `target` is past the line's last column.
fn run_at_from(text: &str, from: ColumnCheckpoint, target: u32) -> Option<(u32, WordClass)> {
    let mut column = from.column;
    let mut run_start = from.run_start;
    let mut run_class: Option<WordClass> = None;
    for cluster in graphemes(text.get(from.byte as usize..)?) {
        let width = cluster_width(cluster) as u32;
        if width == 0 {
            continue;
        }
        let class = cluster_word_class(cluster);
        if run_class != Some(class) {
            // The first cluster is the checkpoint's own, and where *its* run began is the one
            // thing the walk cannot see for itself — it is why the checkpoint carries it.
            if run_class.is_some() {
                run_start = column;
            }
            run_class = Some(class);
        }
        if column == target {
            return Some((run_start, class));
        }
        if width == 2 {
            if run_class != Some(WordClass::Word) {
                run_start = column + 1;
                run_class = Some(WordClass::Word);
            }
            if column + 1 == target {
                return Some((run_start, WordClass::Word));
            }
        }
        column += width;
    }
    None
}

/// One past the last column of the run that began at `start` with class `class`, scanning forward
/// from a checkpoint known to stand at or before that run's end.
fn run_end_from(text: &str, from: ColumnCheckpoint, class: WordClass, start: u32) -> u32 {
    let mut column = from.column;
    let Some(tail) = text.get(from.byte as usize..) else {
        return column;
    };
    for cluster in graphemes(tail) {
        let width = cluster_width(cluster) as u32;
        if width == 0 {
            continue;
        }
        if column >= start && cluster_word_class(cluster) != class {
            return column;
        }
        // The spacer beside a wide glyph is `Word`, so a wide space ends a run one column in.
        if width == 2 && column + 1 >= start && class != WordClass::Word {
            return column + 1;
        }
        column += width;
    }
    column
}

/// Which logical line an index belongs to, and which version of it.
///
/// The generation is half the key rather than a field beside it, so a staging line rewritten in
/// place cannot be answered out of the index built for what it used to say: the old key is
/// simply no longer asked for, and the entry is released the next time the store is swept.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LineKey {
    History(TranscriptId, SourceGeneration),
    Staging(StagingId, SourceGeneration),
}

impl LineKey {
    fn names(&self, history: TranscriptId) -> bool {
        matches!(self, Self::History(id, _) if *id == history)
    }

    fn generation(&self) -> SourceGeneration {
        match self {
            Self::History(_, generation) | Self::Staging(_, generation) => *generation,
        }
    }
}

/// Every line index this viewport is holding, with a ceiling of its own.
///
/// It lives at the projection and **never** in `bt-transcript` (plan §5.2): the index is derived
/// from a line's content, is thrown away freely, and is not a second authority on what the line
/// says. Putting it beside `FrozenLine` would have charged it to `resident_bytes` and quietly
/// spent the reader's `Scrollback` capacity on an accelerator.
#[derive(Clone, Debug)]
pub struct HorizontalIndexStore {
    entries: HashMap<LineKey, LineColumnIndex>,
    stride: u32,
    line_budget_bytes: usize,
    store_budget_bytes: usize,
    resident_bytes: usize,
    builds: u64,
    refusals: u64,
}

impl Default for HorizontalIndexStore {
    fn default() -> Self {
        Self::new(
            CHECKPOINT_STRIDE_COLUMNS,
            LINE_INDEX_BUDGET_BYTES,
            INDEX_STORE_BUDGET_BYTES,
        )
    }
}

impl HorizontalIndexStore {
    #[must_use]
    pub fn new(stride: u32, line_budget_bytes: usize, store_budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            stride: stride.max(1),
            line_budget_bytes,
            store_budget_bytes,
            resident_bytes: 0,
            builds: 0,
            refusals: 0,
        }
    }

    /// Locate a column on one logical line, building the line's index if this is the first time
    /// the line has been asked about anywhere but its left edge.
    ///
    /// **The answer does not depend on whether an index exists.** A seek to column zero costs
    /// nothing and builds nothing; a line no wider than one stride is scanned linearly and never
    /// indexed, because one stride is already the bound the index would buy.
    pub fn seek(&mut self, key: LineKey, text: &str, target: ContentColumn) -> ColumnSeek {
        if target.0 == 0 {
            return ColumnSeek {
                column: ContentColumn(0),
                byte: 0,
                grapheme: 0,
            };
        }
        match self.index_for(key, text) {
            Some(index) => index.seek(text, target),
            None => seek_from_start(text, target),
        }
    }

    /// The word-class run one column of a logical line stands in, in that line's own columns.
    ///
    /// The other question a horizontal window cannot answer out of the cells it kept: a word cut
    /// by the window's edge goes on past it, and where it goes on to is a fact about the line.
    /// Unlike [`Self::seek`] this has no free answer at column zero — a run beginning at the left
    /// edge still has to end somewhere — so it builds the line's index on its own account.
    pub fn word_run(&mut self, key: LineKey, text: &str, target: ContentColumn) -> ColumnRun {
        match self.index_for(key, text) {
            Some(index) => index.word_run(text, target),
            None => word_run_from_start(text, target),
        }
    }

    /// How many columns one logical line presents, out of its index when it has one.
    ///
    /// [`presentable_end_column`] walks the line, which is the right answer for a line short
    /// enough not to be indexed and the wrong one to ask on every frame of a scroll across a
    /// hundred thousand columns. An index already counted them while it was being built.
    pub fn columns(&mut self, key: LineKey, text: &str) -> ContentColumn {
        match self.index_for(key, text) {
            Some(index) => index.columns(),
            None => presentable_end_column(text),
        }
    }

    /// This line's index, built and kept if this is the first ask and the budgets allow it.
    ///
    /// `None` is never an error and never changes an answer: it means the caller's linear
    /// reference implementation is the one that runs, either because it is already inside the
    /// bound an index would buy or because there was no room to buy it (plan §5.2 clause 5).
    fn index_for(&mut self, key: LineKey, text: &str) -> Option<&LineColumnIndex> {
        if !self.entries.contains_key(&key) {
            if text.len() as u32 <= self.stride {
                // Bytes bound columns from above, so a line this short cannot be wider than one
                // stride and the linear scan is already inside the bound.
                return None;
            }
            let index = LineColumnIndex::build(text, self.stride, self.line_budget_bytes);
            let Some(index) = index.filter(|index| index.columns > self.stride) else {
                self.refusals += 1;
                return None;
            };
            let cost = index.resident_bytes();
            if self.resident_bytes + cost > self.store_budget_bytes {
                self.refusals += 1;
                return None;
            }
            self.resident_bytes += cost;
            self.builds += 1;
            self.entries.insert(key, index);
        }
        self.entries.get(&key)
    }

    /// Release every index for one history line — its eviction, or its tombstone.
    pub fn release_history(&mut self, id: TranscriptId) {
        self.release(|key| key.names(id));
    }

    /// Release every index built against a superseded source generation.
    pub fn retain_generation(&mut self, current: SourceGeneration) {
        self.release(|key| key.generation() != current);
    }

    fn release(&mut self, doomed: impl Fn(&LineKey) -> bool) {
        let mut freed = 0usize;
        self.entries.retain(|key, index| {
            if doomed(key) {
                freed += index.resident_bytes();
                return false;
            }
            true
        });
        self.resident_bytes -= freed;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.resident_bytes = 0;
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn resident_bytes(&self) -> usize {
        self.resident_bytes
    }

    /// How many lines this store has indexed, and how many it declined to.
    #[must_use]
    pub fn build_counts(&self) -> (u64, u64) {
        (self.builds, self.refusals)
    }
}

/// A forward cursor over one line's style spans.
///
/// `FrozenLine::styles` is byte-ascending and disjoint, and layout asks about bytes in
/// non-decreasing order, so the per-cluster linear `find` this replaces was re-walking the whole
/// span list for every cell of every line — quadratic in a line's own styling, on every frame
/// that touched the line. The cursor answers each cell in amortized constant time, and
/// [`Self::seek`] re-seats it in logarithmic time when a window starts part-way along a line.
pub(crate) struct StyleCursor<'a> {
    spans: &'a [StyleSpan],
    index: usize,
}

impl<'a> StyleCursor<'a> {
    pub(crate) fn new(spans: &'a [StyleSpan]) -> Self {
        Self { spans, index: 0 }
    }

    /// Re-seat the cursor at an arbitrary byte — the window's first cell.
    pub(crate) fn seek(&mut self, byte: u32) {
        self.index = self.spans.partition_point(|span| span.byte_end <= byte);
    }

    /// The span covering `byte`, or `None` where the line carries no style of its own.
    ///
    /// Queries must arrive in non-decreasing byte order between [`Self::seek`] calls.
    pub(crate) fn at(&mut self, byte: u32) -> Option<&'a StyleSpan> {
        while self
            .spans
            .get(self.index)
            .is_some_and(|span| span.byte_end <= byte)
        {
            self.index += 1;
        }
        self.spans
            .get(self.index)
            .filter(|span| span.byte_start <= byte)
    }
}

/// One window's worth of cells cut out of a flattened logical line.
///
/// Exactly `viewport_columns` cells and the same number of anchors, so the frame's dense shape
/// check keeps holding: horizontal scrolling arrives as typed coordinates, never as a relaxed
/// rectangle (plan §0 fact 4).
#[derive(Clone, Debug, Default)]
pub struct FlattenedWindow {
    pub cells: Vec<CapturedCell>,
    pub anchors: Vec<CellAnchor>,
}

/// Materialize the visible columns of one flattened logical line — and only those.
///
/// # The red line (plan §5.6)
///
/// `links` is the inference for the **whole** logical line and this function never re-runs it.
/// Reading a window would resolve `…> https://support.cla` to a real host somebody else owns and
/// send a click there; that was the live plane's bug until 2026-08-20 and a horizontal window is
/// the identical cut. So a link's identity, its target and both ends of its run are decided in
/// content coordinates before this is called, and windowing only decides which cells exist.
///
/// # Straddling glyphs
///
/// A wide glyph owns `[c, c + 2)`. It is drawn only when the window holds **both** its columns;
/// when only one is inside, that column is the glyph's spacer — carrying the glyph's style,
/// hyperlink and anchor, exactly as the spacer beside a fully visible glyph does. Drawing half a
/// glyph would be drawing a different glyph, and a blank that still points at the whole cluster
/// keeps selection and hit-testing naming the thing the reader is looking at.
pub fn window_flattened_line(
    line: &FrozenLine,
    links: &[InferredLink],
    projection: &HorizontalProjection,
    from: ColumnSeek,
) -> FlattenedWindow {
    let viewport_columns = projection.viewport_columns() as usize;
    let window_start = projection.x_origin().0;
    let window_end = projection.window_end().0;
    let mut window = FlattenedWindow {
        cells: Vec::with_capacity(viewport_columns),
        anchors: Vec::with_capacity(viewport_columns),
    };
    let mut styles = StyleCursor::new(&line.styles);
    styles.seek(from.byte);
    let mut column = from.column.0;
    let mut byte = from.byte;
    let mut grapheme = from.grapheme;
    // Whether the cell a zero-width cluster would join is one the window kept. `from` may name a
    // position left of the window — every checkpoint is a legal resume point — so the mark of a
    // cluster nobody drew must not land on whatever cell happens to be last.
    let mut carrier_is_drawn = false;

    for cluster in graphemes(&line.text[from.byte as usize..]) {
        let width = cluster_width(cluster) as u32;
        if width == 0 {
            if carrier_is_drawn && let Some(cell) = window.cells.last_mut() {
                cell.text.push_str(cluster);
            }
            byte += cluster.len() as u32;
            grapheme += 1;
            continue;
        }
        if column >= window_end {
            break;
        }
        // Which of this cell's columns the window holds. A narrow cell has only the lead.
        let lead_visible = column >= window_start;
        let trail_visible = width == 2 && column + 1 >= window_start && column + 1 < window_end;
        carrier_is_drawn = lead_visible || trail_visible;
        if !carrier_is_drawn {
            column += width;
            byte += cluster.len() as u32;
            grapheme += 1;
            continue;
        }
        let start = ContentAnchor::History {
            id: line.id,
            offset: GraphemeOffset(grapheme),
            bias: Bias::Before,
            generation: line.source_generation,
        };
        let anchor = CellAnchor {
            end: endpoint(&start, Bias::After),
            start,
        };
        let mut cell = CapturedCell::plain(cluster);
        if let Some(span) = styles.at(byte) {
            cell.style = span.style.clone();
            cell.hyperlink.clone_from(&span.hyperlink);
            if cell.hyperlink.is_some() {
                cell.style.flags.insert(CellFlags::DOTTED_UNDERLINE);
            }
        }
        if cell.hyperlink.is_none()
            && let Some(inferred) = implicit_link_at(links, byte as usize)
        {
            cell.hyperlink = Some(CellHyperlink::implicit(inferred.uri.clone()));
            if inferred.resting_dotted && !cell.style.flags.contains(CellFlags::UNDERLINE) {
                cell.style.flags.insert(CellFlags::DOTTED_UNDERLINE);
            }
        }
        if width == 2 {
            cell.style.flags.insert(CellFlags::WIDE_CHAR);
            let mut spacer = CapturedCell {
                wide_spacer: true,
                style: cell.style.clone(),
                hyperlink: cell.hyperlink.clone(),
                ..CapturedCell::default()
            };
            spacer.style.flags.remove(CellFlags::WIDE_CHAR);
            if lead_visible && trail_visible {
                window.cells.push(cell);
                window.anchors.push(anchor.clone());
            }
            window.cells.push(spacer);
            window.anchors.push(anchor);
        } else {
            window.cells.push(cell);
            window.anchors.push(anchor);
        }
        column += width;
        byte += cluster.len() as u32;
        grapheme += 1;
    }

    let end_offset = line.grapheme_boundaries.len().saturating_sub(1) as u32;
    let tail = ContentAnchor::History {
        id: line.id,
        offset: GraphemeOffset(end_offset),
        bias: Bias::Before,
        generation: line.source_generation,
    };
    let tail_end = endpoint(&tail, Bias::After);
    while window.cells.len() < viewport_columns {
        window.cells.push(CapturedCell::default());
        window.anchors.push(CellAnchor {
            start: tail.clone(),
            end: tail_end.clone(),
        });
    }
    debug_assert_eq!(
        window.cells.len(),
        viewport_columns,
        "a window emits one cell per column of `[x_origin, x_origin + viewport_columns)`"
    );
    window
}

#[cfg(test)]
mod tests {
    use bt_transcript::{
        CapturedRow, HyperlinkRange, PhysicalFragment, SourceGeneration, TranscriptStore,
    };

    use super::*;

    fn line_of(text: &str) -> FrozenLine {
        FrozenLine {
            id: TranscriptId(11),
            source_generation: SourceGeneration(2),
            grapheme_boundaries: graphemes(text)
                .scan(0u32, |offset, cluster| {
                    let start = *offset;
                    *offset += cluster.len() as u32;
                    Some(start)
                })
                .chain(std::iter::once(text.len() as u32))
                .collect(),
            fragments: vec![PhysicalFragment {
                byte_start: 0,
                byte_end: text.len() as u32,
                soft_wrapped: false,
                captured_columns: text_width(text) as u32,
            }],
            text: text.to_owned(),
            styles: Vec::new(),
            shell_marks: Vec::new(),
            wrap_split: false,
        }
    }

    /// The shapes plan §5.2 clause 6 names, each one a different way for a column to stop being a
    /// byte: ASCII, wide glyphs that step two columns at a time, combining marks that step none,
    /// a whitespace run, and a mixture that puts a stride boundary inside every one of them.
    fn corpus() -> Vec<(&'static str, String)> {
        vec![
            ("ascii", "abcdefghij".repeat(30)),
            ("cjk", "漢字仮名".repeat(40)),
            ("combining", "e\u{301}o\u{308}".repeat(80)),
            ("blank run", format!("{}tail", " ".repeat(300))),
            (
                "mixed",
                (0..60)
                    .map(|index| match index % 4 {
                        0 => "漢".to_owned(),
                        1 => "e\u{301}".to_owned(),
                        2 => "  ".to_owned(),
                        _ => "xyz".to_owned(),
                    })
                    .collect(),
            ),
        ]
    }

    /// Plan §5.2's whole correctness contract in one assertion: the index may change how long a
    /// seek takes and may never change where it lands.
    #[test]
    fn an_indexed_seek_and_a_linear_seek_name_the_same_cell() {
        for (name, text) in corpus() {
            let width = text_width(&text) as u32;
            for stride in [1u32, 2, 3, 64, 97, width + 5] {
                let index = LineColumnIndex::build(&text, stride, LINE_INDEX_BUDGET_BYTES)
                    .expect("the corpus fits the per-line budget");
                assert_eq!(index.columns(), ContentColumn(width), "{name}");
                for target in 0..=width + 3 {
                    assert_eq!(
                        index.seek(&text, ContentColumn(target)),
                        seek_from_start(&text, ContentColumn(target)),
                        "{name} at stride {stride}, column {target}"
                    );
                }
            }
        }
    }

    /// Plan §5.5, the word half: **which word a column belongs to is a fact about the line**, so
    /// the indexed answer and the linear one must name the same two columns everywhere, including
    /// across the stride boundaries the index is made of.
    ///
    /// This is [`an_indexed_seek_and_a_linear_seek_name_the_same_cell`]'s contract for the second
    /// question a horizontal window cannot answer out of its own cells. It matters more than the
    /// seek's, not less: a seek that disagreed would draw the wrong cell and be seen, while a run
    /// that disagreed would quietly select half a path and put it on somebody's clipboard.
    #[test]
    fn an_indexed_run_and_a_linear_run_name_the_same_word() {
        let mut corpus = corpus();
        corpus.push((
            "prose",
            "the quick brown fox, jumps/over the lazy dog. ".repeat(20),
        ));
        corpus.push(("one long word", "abcdefghij".repeat(50)));
        corpus.push(("wide space", "漢字\u{3000}仮名 ".repeat(40)));
        for (name, text) in corpus {
            let width = text_width(&text) as u32;
            for stride in [1u32, 2, 3, 64, 97, width + 5] {
                let index = LineColumnIndex::build(&text, stride, LINE_INDEX_BUDGET_BYTES)
                    .expect("the corpus fits the per-line budget");
                for target in 0..=width + 3 {
                    assert_eq!(
                        index.word_run(&text, ContentColumn(target)),
                        word_run_from_start(&text, ContentColumn(target)),
                        "{name} at stride {stride}, column {target}"
                    );
                }
            }
        }
    }

    /// The run machinery's whole reason for existing (plan §1b against §5.5): a hundred thousand
    /// columns of one uninterrupted word is one run, and both of its ends must be findable without
    /// reading the columns in between.
    ///
    /// Red gate: drop `run_start` from the checkpoint and this still passes — with the far end
    /// found by walking every one of the hundred thousand columns, once per row, once per frame,
    /// which is exactly the `O(line length)` §1b forbids. `tests/horizontal_budget.rs` is where
    /// that shows up as a number; this is where it shows up as an answer.
    #[test]
    fn one_word_a_hundred_thousand_columns_wide_still_has_two_ends() {
        let text = "abcdefghij".repeat(10_000);
        let width = text_width(&text) as u32;
        let index =
            LineColumnIndex::build(&text, CHECKPOINT_STRIDE_COLUMNS, LINE_INDEX_BUDGET_BYTES)
                .expect("a hundred thousand columns at stride 64 fits the per-line budget");
        for target in [0u32, 1, 63, 64, 65, 50_000, width - 1] {
            assert_eq!(
                index.word_run(&text, ContentColumn(target)),
                ColumnRun {
                    start: ContentColumn(0),
                    end: ContentColumn(width),
                },
                "column {target}"
            );
        }
        assert_eq!(
            index.word_run(&text, ContentColumn(width)),
            ColumnRun {
                start: ContentColumn(width),
                end: ContentColumn(width),
            },
            "past the end there is padding, and padding is nobody's word"
        );
    }

    /// Plan §5.2 clause 2: a checkpoint is a position a decoder can be dropped at. Every one of
    /// them starts a cell — never a wide glyph's second column, never inside a cluster.
    #[test]
    fn every_checkpoint_stands_at_a_cell_boundary() {
        for (name, text) in corpus() {
            let index = LineColumnIndex::build(&text, 64, LINE_INDEX_BUDGET_BYTES).unwrap();
            let mut starts = Vec::new();
            let mut column = 0u32;
            let mut byte = 0u32;
            for (grapheme, cluster) in graphemes(&text).enumerate() {
                let width = cluster_width(cluster) as u32;
                if width > 0 {
                    starts.push((column, byte, grapheme as u32));
                }
                column += width;
                byte += cluster.len() as u32;
            }
            for checkpoint in &index.checkpoints {
                assert!(
                    starts.contains(&(checkpoint.column, checkpoint.byte, checkpoint.grapheme)),
                    "{name}: checkpoint {checkpoint:?} is not a cell start"
                );
                assert!(text.is_char_boundary(checkpoint.byte as usize), "{name}");
            }
            assert!(
                index
                    .checkpoints
                    .windows(2)
                    .all(|pair| pair[0].column < pair[1].column),
                "{name}: checkpoints must be strictly ascending"
            );
        }
    }

    /// Plan §5.2 clause 5: correctness never depends on the allocation succeeding. Every refusal
    /// road — a budget of nothing, a line already inside one stride, a store that is full — hands
    /// back the same cell the index would have.
    #[test]
    fn a_refused_index_costs_time_and_never_a_cell() {
        let text = "漢字".repeat(500);
        assert_eq!(LineColumnIndex::build(&text, 64, 0), None);
        assert_eq!(LineColumnIndex::build(&text, 64, 8), None);

        let key = LineKey::History(TranscriptId(1), SourceGeneration(1));
        for mut store in [
            HorizontalIndexStore::new(64, 0, INDEX_STORE_BUDGET_BYTES),
            HorizontalIndexStore::new(64, LINE_INDEX_BUDGET_BYTES, 0),
            HorizontalIndexStore::new(64, LINE_INDEX_BUDGET_BYTES, INDEX_STORE_BUDGET_BYTES),
        ] {
            for target in [0u32, 1, 63, 64, 65, 999, 1_000, 5_000] {
                assert_eq!(
                    store.seek(key, &text, ContentColumn(target)),
                    seek_from_start(&text, ContentColumn(target)),
                    "column {target}"
                );
            }
        }
    }

    /// Plan §5.2's build rule: nothing is indexed until a seek asks for a column other than the
    /// left edge, and a line no wider than one stride is never indexed at all.
    #[test]
    fn an_index_is_built_only_when_a_seek_leaves_the_left_edge() {
        let mut store = HorizontalIndexStore::default();
        let long = "x".repeat(4_000);
        let short = "x".repeat(CHECKPOINT_STRIDE_COLUMNS as usize);
        let long_key = LineKey::History(TranscriptId(1), SourceGeneration(1));
        let short_key = LineKey::History(TranscriptId(2), SourceGeneration(1));

        store.seek(long_key, &long, ContentColumn(0));
        assert!(store.is_empty(), "column zero asks nothing of an index");
        store.seek(short_key, &short, ContentColumn(30));
        assert!(store.is_empty(), "one stride is already the bound");

        store.seek(long_key, &long, ContentColumn(3_000));
        assert_eq!(store.len(), 1);
        assert!(store.resident_bytes() > 0);
        assert_eq!(
            store.build_counts(),
            (1, 0),
            "the short line was never a refusal — it was never a candidate"
        );
    }

    /// Plan §5.2's lifetime rule at both of its doors: a line's eviction, and a generation the
    /// content moved past.
    #[test]
    fn an_eviction_and_a_stale_generation_each_release_their_index() {
        let mut store = HorizontalIndexStore::default();
        let text = "x".repeat(4_000);
        let kept = LineKey::History(TranscriptId(1), SourceGeneration(1));
        let evicted = LineKey::History(TranscriptId(2), SourceGeneration(1));
        let stale = LineKey::Staging(StagingId(3), SourceGeneration(1));
        for key in [kept, evicted, stale] {
            store.seek(key, &text, ContentColumn(2_000));
        }
        assert_eq!(store.len(), 3);
        let full = store.resident_bytes();

        store.release_history(TranscriptId(2));
        assert_eq!(store.len(), 2);
        assert!(store.resident_bytes() < full);

        store.retain_generation(SourceGeneration(2));
        assert!(store.is_empty(), "every entry predates the new generation");
        assert_eq!(store.resident_bytes(), 0);
    }

    /// Plan §5.3 clause 2, the half a monotone maximum cannot do.
    #[test]
    fn the_extent_shrinks_when_the_widest_line_leaves() {
        let mut extent = FlattenedExtent::new();
        assert_eq!(extent.widest(), ContentColumn(0));
        for columns in [80u32, 4_000, 120, 4_000] {
            extent.insert(ContentColumn(columns));
        }
        assert_eq!(extent.widest(), ContentColumn(4_000));

        extent.remove(ContentColumn(4_000));
        assert_eq!(
            extent.widest(),
            ContentColumn(4_000),
            "two lines were that wide; one of them is still here"
        );
        extent.remove(ContentColumn(4_000));
        assert_eq!(extent.widest(), ContentColumn(120), "and now neither is");

        extent.replace(ContentColumn(120), ContentColumn(200));
        assert_eq!(extent.widest(), ContentColumn(200));
        assert_eq!(extent.lines(), 2);
        extent.clear();
        assert!(extent.is_empty());
        assert_eq!(extent.widest(), ContentColumn(0));
    }

    /// Plan §5.3 clause 5: there is no road to an out-of-range origin, because there is no setter.
    /// A pane that widens, and a history whose widest line is evicted, both clamp in the step that
    /// learns about it rather than one frame later.
    #[test]
    fn an_origin_is_clamped_by_construction_and_never_by_discipline() {
        let axis = HorizontalProjection::new(ContentColumn(4_000), 80, ContentColumn(9_999));
        assert_eq!(axis.x_origin(), ContentColumn(3_920));
        assert_eq!(axis.max_x_origin(), ContentColumn(3_920));
        assert_eq!(axis.content_extent(), ContentColumn(4_000));

        let widened = axis.reclamped(ContentColumn(4_000), 200);
        assert_eq!(widened.x_origin(), ContentColumn(3_800));

        let shrunk = axis.reclamped(ContentColumn(100), 80);
        assert_eq!(shrunk.content_extent(), ContentColumn(100));
        assert_eq!(shrunk.x_origin(), ContentColumn(20));

        let empty = axis.reclamped(ContentColumn(0), 80);
        assert_eq!(empty.content_extent(), ContentColumn(80));
        assert_eq!(empty.x_origin(), ContentColumn(0));
        assert!(!empty.is_scrolled());
    }

    /// The two column spaces are one function apart, and it is total in one direction and partial
    /// in the other: every drawn column names content, and content outside the window is drawn
    /// nowhere at all.
    #[test]
    fn the_two_column_spaces_meet_only_at_the_projection() {
        let axis = HorizontalProjection::new(ContentColumn(500), 80, ContentColumn(100));
        assert_eq!(axis.to_viewport(ContentColumn(99)), None);
        assert_eq!(axis.to_viewport(ContentColumn(180)), None);
        for viewport in 0..80 {
            let content = axis.to_content(ViewportColumn(viewport));
            assert_eq!(content, ContentColumn(100 + viewport));
            assert_eq!(axis.to_viewport(content), Some(ViewportColumn(viewport)));
        }
    }

    /// Plan §1b: a window is exactly the columns it promises, and those columns say what the whole
    /// flattened line says at the same place. The frame's dense shape check keeps holding.
    #[test]
    fn a_window_is_the_slice_of_the_line_it_claims_to_be() {
        for (name, text) in corpus() {
            let line = line_of(&text);
            let width = text_width(&text) as u32;
            let whole = HorizontalProjection::new(ContentColumn(width), width, ContentColumn(0));
            let full =
                window_flattened_line(&line, &[], &whole, seek_from_start(&text, ContentColumn(0)));
            assert_eq!(full.cells.len(), width as usize, "{name}");

            for origin in [0u32, 1, 2, 63, 64, 65, width / 2, width.saturating_sub(3)] {
                let axis =
                    HorizontalProjection::new(ContentColumn(width), 40, ContentColumn(origin));
                let start = axis.x_origin().0 as usize;
                let window = window_flattened_line(
                    &line,
                    &[],
                    &axis,
                    seek_from_start(&text, axis.x_origin()),
                );
                assert_eq!(window.cells.len(), 40, "{name} at {origin}");
                assert_eq!(window.anchors.len(), 40, "{name} at {origin}");
                for offset in 0..40usize {
                    let Some(expected) = full.cells.get(start + offset) else {
                        continue;
                    };
                    let seen = &window.cells[offset];
                    if (seen.text.as_str(), seen.wide_spacer)
                        == (expected.text.as_str(), expected.wide_spacer)
                    {
                        continue;
                    }
                    // The one licensed difference, and only at an edge: a glyph the window holds
                    // half of is its spacer instead of itself.
                    assert!(
                        (offset == 0 || offset == 39)
                            && seen.wide_spacer
                            && expected.style.flags.contains(CellFlags::WIDE_CHAR),
                        "{name} at origin {origin}, column {offset}: {:?} is not {:?}",
                        seen.text.as_str(),
                        expected.text.as_str()
                    );
                    assert_eq!(
                        window.anchors[offset],
                        full.anchors[start + offset],
                        "{name} at origin {origin}, column {offset}: and it still names the glyph"
                    );
                }
            }
        }
    }

    /// A resume point is not a promise about the window: every checkpoint is a legal one, and the
    /// nearest checkpoint at or before an origin is usually **left** of it. So the materializer
    /// decides visibility column by column rather than assuming the cell it started on is inside —
    /// otherwise a glyph well outside the window would still emit its spacer, and a combining mark
    /// belonging to a cell nobody drew would land on whichever cell happened to be last.
    #[test]
    fn a_window_resumed_from_the_line_start_is_the_same_window() {
        for (name, text) in corpus() {
            let line = line_of(&text);
            let width = text_width(&text) as u32;
            for origin in [0u32, 7, 64, 130, width / 2] {
                let axis =
                    HorizontalProjection::new(ContentColumn(width), 40, ContentColumn(origin));
                let sought = window_flattened_line(
                    &line,
                    &[],
                    &axis,
                    seek_from_start(&text, axis.x_origin()),
                );
                let from_origin = window_flattened_line(
                    &line,
                    &[],
                    &axis,
                    ColumnSeek {
                        column: ContentColumn(0),
                        byte: 0,
                        grapheme: 0,
                    },
                );
                assert_eq!(sought.cells, from_origin.cells, "{name} at {origin}");
                assert_eq!(sought.anchors, from_origin.anchors, "{name} at {origin}");
            }
        }
    }

    /// A wide glyph owns two columns and is drawn only when the window holds both. The column
    /// that is left inside carries the glyph's spacer, and the spacer still points at the glyph —
    /// so the reader who clicks a half-visible character selects the character.
    #[test]
    fn a_glyph_straddling_an_edge_is_its_own_spacer() {
        let text = "a漢b漢c";
        let line = line_of(text);
        let width = text_width(text) as u32;

        // `漢` occupies columns 1 and 2; a window starting at 2 holds only its second.
        let left = HorizontalProjection::new(ContentColumn(width), 3, ContentColumn(2));
        let cut = window_flattened_line(&line, &[], &left, seek_from_start(text, ContentColumn(2)));
        assert!(
            cut.cells[0].wide_spacer,
            "the left edge cut a glyph in half"
        );
        assert!(cut.cells[0].text.is_empty());
        assert_eq!(
            cut.anchors[0].start,
            ContentAnchor::History {
                id: line.id,
                offset: GraphemeOffset(1),
                bias: Bias::Before,
                generation: line.source_generation,
            },
            "and the blank still names the glyph it is half of"
        );
        assert_eq!(cut.cells[1].text.as_str(), "b");

        // The second `漢` occupies columns 4 and 5; a window ending at 5 holds only its first.
        let right = HorizontalProjection::new(ContentColumn(width), 5, ContentColumn(0));
        let clipped =
            window_flattened_line(&line, &[], &right, seek_from_start(text, ContentColumn(0)));
        assert_eq!(clipped.cells.len(), 5);
        assert!(
            clipped.cells[4].wide_spacer,
            "half a glyph would be a different glyph"
        );
        assert_eq!(
            clipped.anchors[4].start,
            ContentAnchor::History {
                id: line.id,
                offset: GraphemeOffset(3),
                bias: Bias::Before,
                generation: line.source_generation,
            }
        );

        // And with room for both columns it is the glyph, then its spacer.
        let whole = HorizontalProjection::new(ContentColumn(width), width, ContentColumn(0));
        let full =
            window_flattened_line(&line, &[], &whole, seek_from_start(text, ContentColumn(0)));
        assert_eq!(full.cells[4].text.as_str(), "漢");
        assert!(full.cells[4].style.flags.contains(CellFlags::WIDE_CHAR));
        assert!(full.cells[5].wide_spacer);
        assert!(!full.cells[5].style.flags.contains(CellFlags::WIDE_CHAR));
    }

    /// **The red line** (plan §5.6, DESIGN §7.1.5h ① / §7.1.5k). A window may decide which cells
    /// exist. It may never decide what a link is.
    ///
    /// The address below is the one that shipped broken until 2026-08-20: read from a cut it
    /// resolves to `support.cla`, a host somebody else may own, and the click goes somewhere the
    /// reader never typed. Inference runs once over the whole logical line, so every window into
    /// it — including the ones whose first and last cells fall inside the address — carries the
    /// complete target and nothing else.
    #[test]
    fn a_window_never_re_reads_a_link_out_of_the_cells_it_kept() {
        const URI: &str = "https://support.claude.com/en/articles/15363606";
        let text = format!("see {URI} for more");
        let line = line_of(&text);
        let width = text_width(&text) as u32;
        let start = text.find(URI).unwrap();
        let links = vec![InferredLink {
            range: HyperlinkRange {
                byte_start: start,
                byte_end: start + URI.len(),
            },
            uri: URI.to_owned(),
            resting_dotted: false,
        }];

        for origin in 0..width {
            let axis = HorizontalProjection::new(ContentColumn(width), 12, ContentColumn(origin));
            let window = window_flattened_line(
                &line,
                &links,
                &axis,
                seek_from_start(&text, axis.x_origin()),
            );
            for (column, cell) in window.cells.iter().enumerate() {
                let Some(link) = &cell.hyperlink else {
                    continue;
                };
                assert_eq!(
                    link.uri, URI,
                    "origin {origin}, column {column}: a window read its own address"
                );
            }
        }
    }

    /// Past the end of the line the window is blank, and every blank names the line's end — the
    /// same closure `pad_frozen_row` gives a short row today.
    #[test]
    fn a_window_past_the_end_is_blank_and_still_anchored() {
        let line = line_of("short");
        let axis = HorizontalProjection::new(ContentColumn(500), 20, ContentColumn(300));
        let window = window_flattened_line(
            &line,
            &[],
            &axis,
            seek_from_start(&line.text, axis.x_origin()),
        );
        assert_eq!(window.cells.len(), 20);
        assert!(window.cells.iter().all(|cell| cell.text.is_empty()));
        assert!(window.anchors.iter().all(|anchor| anchor.start
            == ContentAnchor::History {
                id: line.id,
                offset: GraphemeOffset(5),
                bias: Bias::Before,
                generation: line.source_generation,
            }));
    }

    /// Plan §5.3: the extent is the presentable width of what is retained, and this build's byte
    /// ceiling evicts whole lines rather than truncating one — so the two agree, and the eviction
    /// is where the extent has to shrink.
    #[test]
    fn the_extent_follows_what_the_store_actually_kept() {
        let nz = |value: usize| std::num::NonZeroUsize::new(value).unwrap();
        let mut store = TranscriptStore::with_quotas(nz(8), nz(2));
        let mut extent = FlattenedExtent::new();
        // The pairing plan §5.3 clause 3 asks for: what a line contributed is remembered by its
        // id, so its eviction can withdraw exactly that and the maximum can go down.
        let mut contributed: HashMap<TranscriptId, ContentColumn> = HashMap::new();
        for text in ["short", &"w".repeat(400), "also short", "last"] {
            for finalized in store.capture(CapturedRow::plain(text, false)).finalized {
                let columns = presentable_end_column(&finalized.line.text);
                contributed.insert(finalized.line.id, columns);
                extent.insert(columns);
            }
            for id in store.take_evictions() {
                extent.remove(
                    contributed
                        .remove(&id)
                        .expect("every evicted line was admitted"),
                );
            }
        }

        assert_eq!(store.frozen().len(), 2, "the quota kept the last two");
        assert!(
            !store.frozen().iter().any(|line| line.text.len() == 400),
            "and the four-hundred-column line is not one of them"
        );
        assert_eq!(extent.lines(), 2);
        assert_eq!(
            extent.widest(),
            ContentColumn(10),
            "the widest retained line is `also short` — a high-water mark would still say 400 \
             and leave the reader scrolling into nothing"
        );
        let axis = HorizontalProjection::new(extent.widest(), 80, ContentColumn(300));
        assert_eq!(
            axis.x_origin(),
            ContentColumn(0),
            "and the origin came home"
        );
    }
}
