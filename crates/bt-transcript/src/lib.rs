//! Canonical frozen transcript and mutable staging primitives.

use std::{collections::VecDeque, num::NonZeroUsize};

use bitflags::bitflags;
use unicode_segmentation::UnicodeSegmentation;

pub const DEFAULT_STAGING_QUOTA: NonZeroUsize = NonZeroUsize::new(4096).unwrap();
/// Spike-only value; M0 must replace it with a measured or configured quota.
pub const SPIKE_DEFAULT_FROZEN_QUOTA: NonZeroUsize = NonZeroUsize::new(100_000).unwrap();

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TranscriptId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StagingId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceGeneration(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GraphemeOffset(pub u32);

/// Stable transcript color vocabulary; no upstream discriminants cross this boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalColor {
    Named(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

bitflags! {
    /// Stable transcript style flags. Bit positions are owned by BetterTerminal.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct CellFlags: u16 {
        const INVERSE = 1 << 0;
        const BOLD = 1 << 1;
        const ITALIC = 1 << 2;
        const UNDERLINE = 1 << 3;
        const DIM = 1 << 4;
        const HIDDEN = 1 << 5;
        const STRIKEOUT = 1 << 6;
        const DOUBLE_UNDERLINE = 1 << 7;
        const UNDERCURL = 1 << 8;
        const DOTTED_UNDERLINE = 1 << 9;
        const DASHED_UNDERLINE = 1 << 10;
        const WIDE_CHAR = 1 << 11;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellStyle {
    pub flags: CellFlags,
    pub foreground: TerminalColor,
    pub background: TerminalColor,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            flags: CellFlags::empty(),
            // Named codes are BetterTerminal-owned; 16/17 mean default foreground/background.
            foreground: TerminalColor::Named(16),
            background: TerminalColor::Named(17),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapturedCell {
    pub text: String,
    pub style: CellStyle,
    pub hyperlink: Option<String>,
    /// A terminal wide-character spacer has no source text of its own.
    pub wide_spacer: bool,
}

impl CapturedCell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedRow {
    pub cells: Vec<CapturedCell>,
    /// True when this physical row soft-wraps into the next physical row.
    pub continues: bool,
    pub shell_mark: Option<String>,
}

impl CapturedRow {
    pub fn plain(text: &str, continues: bool) -> Self {
        Self {
            cells: text
                .chars()
                .map(|c| CapturedCell::plain(c.to_string()))
                .collect(),
            continues,
            shell_mark: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyleSpan {
    pub byte_start: u32,
    pub byte_end: u32,
    pub style: CellStyle,
    pub hyperlink: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalFragment {
    pub byte_start: u32,
    pub byte_end: u32,
    pub soft_wrapped: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenLine {
    pub id: TranscriptId,
    pub source_generation: SourceGeneration,
    pub text: String,
    /// UTF-8 byte positions for every grapheme boundary, including 0 and len.
    pub grapheme_boundaries: Vec<u32>,
    pub styles: Vec<StyleSpan>,
    pub fragments: Vec<PhysicalFragment>,
    pub shell_marks: Vec<(u32, String)>,
    pub wrap_split: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedRow {
    pub id: StagingId,
    pub row: CapturedRow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FreezeCandidate {
    rows: Vec<StagedRow>,
    /// Mutable snapshot of the still-live continuation. It is never copied into frozen source;
    /// the authoritative cells are captured when that physical row actually scrolls out.
    live_tail: Option<CapturedRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorMapping {
    pub staging_id: StagingId,
    pub transcript_id: TranscriptId,
    pub grapheme_base: GraphemeOffset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalizedLine {
    pub line: FrozenLine,
    pub mappings: Vec<AnchorMapping>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureResult {
    pub staging_id: StagingId,
    pub finalized: Vec<FinalizedLine>,
}

/// The only owner and quota authority for frozen terminal history.
#[derive(Clone, Debug)]
pub struct TranscriptStore {
    staging_quota: usize,
    frozen_quota: usize,
    next_staging: u64,
    next_transcript: u64,
    source_generation: SourceGeneration,
    staging_rows: usize,
    staging: VecDeque<FreezeCandidate>,
    frozen: VecDeque<FrozenLine>,
    tombstones: Vec<TranscriptId>,
    pending_evictions: Vec<TranscriptId>,
}

impl Default for TranscriptStore {
    fn default() -> Self {
        Self::new(DEFAULT_STAGING_QUOTA)
    }
}

impl TranscriptStore {
    pub fn new(quota: NonZeroUsize) -> Self {
        Self::with_quotas(quota, SPIKE_DEFAULT_FROZEN_QUOTA)
    }

    pub fn with_quotas(staging_quota: NonZeroUsize, frozen_quota: NonZeroUsize) -> Self {
        Self {
            staging_quota: staging_quota.get(),
            frozen_quota: frozen_quota.get(),
            next_staging: 1,
            next_transcript: 1,
            source_generation: SourceGeneration(1),
            staging_rows: 0,
            staging: VecDeque::new(),
            frozen: VecDeque::new(),
            tombstones: Vec::new(),
            pending_evictions: Vec::new(),
        }
    }

    pub fn staging_len(&self) -> usize {
        self.staging_rows
    }
    pub fn frozen(&self) -> &VecDeque<FrozenLine> {
        &self.frozen
    }
    /// Mutable scroll-out rows in capture order. Viewports may window these rows, but must never
    /// treat them as frozen source or mutate them outside the transcript owner.
    pub fn staged_rows(&self) -> impl Iterator<Item = &StagedRow> {
        self.staging
            .iter()
            .flat_map(|candidate| candidate.rows.iter())
    }
    pub fn tombstones(&self) -> &[TranscriptId] {
        &self.tombstones
    }
    pub fn source_generation(&self) -> SourceGeneration {
        self.source_generation
    }

    pub fn take_evictions(&mut self) -> Vec<TranscriptId> {
        std::mem::take(&mut self.pending_evictions)
    }

    pub fn capture(&mut self, row: CapturedRow) -> CaptureResult {
        let id = StagingId(self.next_staging);
        self.next_staging += 1;
        let completes_candidate = !row.continues;
        let staged = StagedRow { id, row };

        if let Some(candidate) = self
            .staging
            .back_mut()
            .filter(|candidate| candidate.rows.last().is_some_and(|row| row.row.continues))
        {
            candidate.rows.push(staged);
        } else {
            self.staging.push_back(FreezeCandidate {
                rows: vec![staged],
                live_tail: None,
            });
        }
        self.staging_rows += 1;

        let mut finalized = Vec::new();
        if completes_candidate && let Some(candidate) = self.staging.pop_back() {
            self.staging_rows -= candidate.rows.len();
            finalized.push(self.finalize(candidate, false));
        }
        finalized.extend(self.enforce_staging_quota());

        CaptureResult {
            staging_id: id,
            finalized,
        }
    }

    /// Freeze one harvested physical row as an independent wrap-split candidate.
    ///
    /// Resize-transaction harvest cannot prove whether a native `WRAPLINE` belongs to the next
    /// repainted row. Keeping the original row flag preserves every boundary cell, while forcing a
    /// candidate boundary prevents an observationally unrelated next row from being welded on.
    pub fn capture_wrap_split(&mut self, row: CapturedRow) -> CaptureResult {
        let id = StagingId(self.next_staging);
        self.next_staging += 1;
        let candidate = FreezeCandidate {
            rows: vec![StagedRow { id, row }],
            live_tail: None,
        };
        let finalized = vec![self.finalize(candidate, true)];
        CaptureResult {
            staging_id: id,
            finalized,
        }
    }

    /// A width change never joins a staged head with a live-grid tail.
    pub fn finalize_all_candidates(&mut self) -> Vec<FinalizedLine> {
        let candidates = self.staging.drain(..).collect::<Vec<_>>();
        self.staging_rows -= candidates
            .iter()
            .map(|candidate| candidate.rows.len())
            .sum::<usize>();
        candidates
            .into_iter()
            .map(|candidate| self.finalize(candidate, true))
            .collect()
    }

    pub fn rewrite_staged(&mut self, id: StagingId, row: CapturedRow) -> bool {
        for candidate in &mut self.staging {
            if candidate.rows.iter().any(|staged| staged.id == id) {
                candidate.live_tail = Some(row);
                return true;
            }
        }
        false
    }

    pub fn staged_tail(&self, id: StagingId) -> Option<&CapturedRow> {
        self.staging
            .iter()
            .find(|candidate| candidate.rows.iter().any(|staged| staged.id == id))
            .and_then(|candidate| candidate.live_tail.as_ref())
    }

    pub fn evict_oldest(&mut self, count: usize) -> Vec<TranscriptId> {
        let mut removed = Vec::new();
        for _ in 0..count {
            if let Some(line) = self.frozen.pop_front() {
                removed.push(line.id);
                self.tombstones.push(line.id);
            }
        }
        if !removed.is_empty() {
            self.source_generation.0 += 1;
        }
        removed
    }

    /// ED3 and quota eviction intentionally share this tombstoning pipeline.
    pub fn clear_history(&mut self) -> Vec<TranscriptId> {
        let mut removed = self
            .frozen
            .drain(..)
            .map(|line| line.id)
            .collect::<Vec<_>>();
        self.staging.clear();
        self.staging_rows = 0;
        self.tombstones.extend(removed.iter().copied());
        self.source_generation.0 += 1;
        // Staging IDs are not tombstones. The caller must explicitly relocate their anchors by
        // invoking HistoryDocument::delete_transaction with clear_staging=true; generation only
        // invalidates versioned work and is not an anchor-deletion mechanism.
        removed.shrink_to_fit();
        removed
    }

    /// RIS/DECCOLM invalidates candidates but retains already frozen history.
    pub fn invalidate_staging(&mut self) {
        self.staging.clear();
        self.staging_rows = 0;
        self.source_generation.0 += 1;
    }

    fn finalize(&mut self, candidate: FreezeCandidate, wrap_split: bool) -> FinalizedLine {
        let id = TranscriptId(self.next_transcript);
        self.next_transcript += 1;
        let (line, mappings) = normalize(id, self.source_generation, candidate.rows, wrap_split);
        self.frozen.push_back(line.clone());
        let overflow = self.frozen.len().saturating_sub(self.frozen_quota);
        if overflow != 0 {
            let removed = self.evict_oldest(overflow);
            self.pending_evictions.extend(removed);
        }
        FinalizedLine { line, mappings }
    }

    fn enforce_staging_quota(&mut self) -> Vec<FinalizedLine> {
        let mut finalized = Vec::new();
        while self.staging_rows > self.staging_quota {
            let Some(candidate) = self.staging.pop_front() else {
                break;
            };
            self.staging_rows -= candidate.rows.len();
            let wrap_split = candidate.rows.last().is_some_and(|row| row.row.continues);
            finalized.push(self.finalize(candidate, wrap_split));
        }
        finalized
    }
}

fn normalize(
    id: TranscriptId,
    generation: SourceGeneration,
    rows: Vec<StagedRow>,
    wrap_split: bool,
) -> (FrozenLine, Vec<AnchorMapping>) {
    let mut text = String::new();
    let mut styles: Vec<StyleSpan> = Vec::new();
    let mut fragments = Vec::new();
    let mut shell_marks = Vec::new();
    let mut mappings = Vec::new();

    for staged in rows {
        let fragment_start = text.len() as u32;
        let grapheme_base = text.graphemes(true).count() as u32;
        mappings.push(AnchorMapping {
            staging_id: staged.id,
            transcript_id: id,
            grapheme_base: GraphemeOffset(grapheme_base),
        });
        let CapturedRow {
            mut cells,
            continues,
            shell_mark,
        } = staged.row;
        if let Some(mark) = shell_mark {
            shell_marks.push((fragment_start, mark));
        }

        // A WRAPLINE fragment owns every cell through its wrap boundary.  In particular a space
        // in the final column is source text, not padding; trimming it turns "find path" into
        // "findpath" when logical rows are later rejoined.  Only hard line ends trim padding.
        if !continues {
            while cells
                .last()
                .is_some_and(|c| !c.wide_spacer && c.text.chars().all(char::is_whitespace))
            {
                cells.pop();
            }
        }
        for cell in cells.into_iter().filter(|c| !c.wide_spacer) {
            let start = text.len() as u32;
            text.push_str(&cell.text);
            let end = text.len() as u32;
            if let Some(previous) = styles.last_mut().filter(|s| {
                s.byte_end == start && s.style == cell.style && s.hyperlink == cell.hyperlink
            }) {
                previous.byte_end = end;
            } else if start != end {
                styles.push(StyleSpan {
                    byte_start: start,
                    byte_end: end,
                    style: cell.style,
                    hyperlink: cell.hyperlink,
                });
            }
        }
        fragments.push(PhysicalFragment {
            byte_start: fragment_start,
            byte_end: text.len() as u32,
            soft_wrapped: continues,
        });
    }

    let mut grapheme_boundaries = text
        .grapheme_indices(true)
        .map(|(i, _)| i as u32)
        .collect::<Vec<_>>();
    grapheme_boundaries.push(text.len() as u32);
    grapheme_boundaries.dedup();
    (
        FrozenLine {
            id,
            source_generation: generation,
            text,
            grapheme_boundaries,
            styles,
            fragments,
            shell_marks,
            wrap_split,
        },
        mappings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nz(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    #[test]
    fn partial_wrap_waits_then_freezes_as_one_logical_line() {
        let mut store = TranscriptStore::new(nz(8));
        let first = store.capture(CapturedRow::plain("ab  ", true));
        assert!(first.finalized.is_empty());
        let second = store.capture(CapturedRow::plain("c", false));
        assert_eq!(second.finalized[0].line.text, "ab  c");
        assert_eq!(second.finalized[0].line.fragments.len(), 2);
    }

    #[test]
    fn soft_wrap_preserves_a_boundary_space_while_hard_end_trims_padding() {
        let mut store = TranscriptStore::new(nz(8));
        store.capture(CapturedRow::plain("find ", true));
        let finalized = store.capture(CapturedRow::plain("path   ", false));
        assert_eq!(finalized.finalized[0].line.text, "find path");
    }

    #[test]
    fn harvested_wrap_split_preserves_boundary_cells_without_joining_the_next_row() {
        let mut store = TranscriptStore::new(nz(8));
        let first = store.capture_wrap_split(CapturedRow::plain("find ", true));
        let second = store.capture_wrap_split(CapturedRow::plain("path", false));

        assert_eq!(first.finalized[0].line.text, "find ");
        assert!(first.finalized[0].line.wrap_split);
        assert_eq!(second.finalized[0].line.text, "path");
        assert_eq!(store.frozen().len(), 2);
    }

    #[test]
    fn harvest_batch_boundary_prevents_wrapline_from_joining_the_next_batch() {
        let mut store = TranscriptStore::new(nz(8));
        store.capture(CapturedRow::plain("batch-one", true));
        let first = store.finalize_all_candidates();
        let second = store.capture(CapturedRow::plain("batch-two", false));

        assert_eq!(first[0].line.text, "batch-one");
        assert!(first[0].line.wrap_split);
        assert_eq!(second.finalized[0].line.text, "batch-two");
        assert_eq!(store.frozen().len(), 2);
    }

    #[test]
    fn resize_and_quota_force_wrap_split() {
        let mut store = TranscriptStore::new(nz(1));
        let first = store.capture(CapturedRow::plain("head", true));
        assert!(first.finalized.is_empty());
        let overflow = store.capture(CapturedRow::plain("tail", true));
        assert!(overflow.finalized[0].line.wrap_split);

        store.capture(CapturedRow::plain("again", true));
        assert!(store.finalize_all_candidates()[0].line.wrap_split);
    }

    #[test]
    fn normalization_keeps_graphemes_links_and_drops_wide_spacers() {
        let mut store = TranscriptStore::new(nz(8));
        let linked = CapturedCell {
            text: "e\u{301}".into(),
            hyperlink: Some("https://example.test".into()),
            ..CapturedCell::default()
        };
        let spacer = CapturedCell {
            wide_spacer: true,
            ..CapturedCell::default()
        };
        let result = store.capture(CapturedRow {
            cells: vec![linked, spacer, CapturedCell::plain(" ")],
            continues: false,
            shell_mark: Some("prompt".into()),
        });
        let line = &result.finalized[0].line;
        assert_eq!(line.text, "e\u{301}");
        assert_eq!(line.grapheme_boundaries, vec![0, 3]);
        assert_eq!(
            line.styles[0].hyperlink.as_deref(),
            Some("https://example.test")
        );
        assert_eq!(line.shell_marks[0].1, "prompt");
    }

    #[test]
    fn mutable_staging_can_be_rewritten_and_eviction_leaves_tombstone() {
        let mut store = TranscriptStore::new(nz(8));
        let staged = store.capture(CapturedRow::plain("old", true));
        assert!(store.rewrite_staged(staged.staging_id, CapturedRow::plain("new", true)));
        assert_eq!(
            store.staged_tail(staged.staging_id),
            Some(&CapturedRow::plain("new", true))
        );
        let finalized = store.finalize_all_candidates().remove(0);
        assert_eq!(finalized.line.text, "old");
        let removed = store.evict_oldest(1);
        assert_eq!(removed, vec![finalized.line.id]);
        assert_eq!(store.tombstones(), removed);
    }

    #[test]
    fn frozen_quota_is_enforced_by_the_store() {
        let mut store = TranscriptStore::with_quotas(nz(8), nz(2));
        for text in ["one", "two", "three"] {
            store.capture(CapturedRow::plain(text, false));
        }
        assert_eq!(
            store
                .frozen()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three"]
        );
        assert_eq!(store.take_evictions(), vec![TranscriptId(1)]);
        assert_eq!(store.tombstones(), &[TranscriptId(1)]);
    }
}
