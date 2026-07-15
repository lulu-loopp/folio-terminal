//! Canonical frozen transcript and mutable staging primitives.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

pub const DEFAULT_STAGING_QUOTA: usize = 4096;
pub const DEFAULT_FROZEN_QUOTA: usize = 100_000;

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct TranscriptId(pub u64);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct StagingId(pub u64);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct SourceGeneration(pub u64);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct GraphemeOffset(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellStyle {
    pub flags: u16,
    pub foreground: u32,
    pub background: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StyleSpan {
    pub byte_start: u32,
    pub byte_end: u32,
    pub style: CellStyle,
    pub hyperlink: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PhysicalFragment {
    pub byte_start: u32,
    pub byte_end: u32,
    pub soft_wrapped: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
pub struct Finalized {
    pub line: FrozenLine,
    pub mappings: Vec<AnchorMapping>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureResult {
    pub staging_id: StagingId,
    pub finalized: Vec<Finalized>,
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
    pub fn new(quota: usize) -> Self {
        Self::with_quotas(quota, DEFAULT_FROZEN_QUOTA)
    }

    pub fn with_quotas(staging_quota: usize, frozen_quota: usize) -> Self {
        assert!(staging_quota > 0, "staging quota must be non-zero");
        assert!(frozen_quota > 0, "frozen quota must be non-zero");
        Self {
            staging_quota,
            frozen_quota,
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
        let staged = StagedRow { id, row };

        if self
            .staging
            .back()
            .is_some_and(|candidate| candidate.rows.last().is_some_and(|row| row.row.continues))
        {
            self.staging.back_mut().unwrap().rows.push(staged);
        } else {
            self.staging.push_back(FreezeCandidate {
                rows: vec![staged],
                live_tail: None,
            });
        }
        self.staging_rows += 1;

        let mut finalized = Vec::new();
        if !self
            .staging
            .back()
            .unwrap()
            .rows
            .last()
            .unwrap()
            .row
            .continues
        {
            let candidate = self.staging.pop_back().unwrap();
            self.staging_rows -= candidate.rows.len();
            finalized.push(self.finalize(candidate, false));
        }
        while self.staging_rows > self.staging_quota {
            let candidate = self
                .staging
                .pop_front()
                .expect("quota overflow requires staging");
            self.staging_rows -= candidate.rows.len();
            finalized.push(self.finalize(candidate, true));
        }

        CaptureResult {
            staging_id: id,
            finalized,
        }
    }

    /// A width change never joins a staged head with a live-grid tail.
    pub fn width_resize(&mut self) -> Vec<Finalized> {
        let candidates = self.staging.drain(..).collect::<Vec<_>>();
        self.staging_rows = 0;
        candidates
            .into_iter()
            .map(|c| self.finalize(c, true))
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
        // Staging IDs are not tombstones, but their anchors are invalidated by the generation bump.
        removed.shrink_to_fit();
        removed
    }

    /// RIS/DECCOLM invalidates candidates but retains already frozen history.
    pub fn invalidate_staging(&mut self) {
        self.staging.clear();
        self.staging_rows = 0;
        self.source_generation.0 += 1;
    }

    fn finalize(&mut self, candidate: FreezeCandidate, wrap_split: bool) -> Finalized {
        let id = TranscriptId(self.next_transcript);
        self.next_transcript += 1;
        let (line, mappings) = normalize(id, self.source_generation, candidate.rows, wrap_split);
        self.frozen.push_back(line.clone());
        let overflow = self.frozen.len().saturating_sub(self.frozen_quota);
        if overflow != 0 {
            let removed = self.evict_oldest(overflow);
            self.pending_evictions.extend(removed);
        }
        Finalized { line, mappings }
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
        if let Some(mark) = staged.row.shell_mark {
            shell_marks.push((fragment_start, mark));
        }

        let mut cells = staged.row.cells;
        while cells
            .last()
            .is_some_and(|c| !c.wide_spacer && c.text.chars().all(char::is_whitespace))
        {
            cells.pop();
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
            soft_wrapped: staged.row.continues,
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

    #[test]
    fn partial_wrap_waits_then_freezes_as_one_logical_line() {
        let mut store = TranscriptStore::new(8);
        let first = store.capture(CapturedRow::plain("ab  ", true));
        assert!(first.finalized.is_empty());
        let second = store.capture(CapturedRow::plain("c", false));
        assert_eq!(second.finalized[0].line.text, "abc");
        assert_eq!(second.finalized[0].line.fragments.len(), 2);
    }

    #[test]
    fn resize_and_quota_force_wrap_split() {
        let mut store = TranscriptStore::new(1);
        let first = store.capture(CapturedRow::plain("head", true));
        assert!(first.finalized.is_empty());
        let overflow = store.capture(CapturedRow::plain("tail", true));
        assert!(overflow.finalized[0].line.wrap_split);

        store.capture(CapturedRow::plain("again", true));
        assert!(store.width_resize()[0].line.wrap_split);
    }

    #[test]
    fn normalization_keeps_graphemes_links_and_drops_wide_spacers() {
        let mut store = TranscriptStore::new(8);
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
        let mut store = TranscriptStore::new(8);
        let staged = store.capture(CapturedRow::plain("old", true));
        assert!(store.rewrite_staged(staged.staging_id, CapturedRow::plain("new", true)));
        assert_eq!(
            store.staged_tail(staged.staging_id),
            Some(&CapturedRow::plain("new", true))
        );
        let finalized = store.width_resize().remove(0);
        assert_eq!(finalized.line.text, "old");
        let removed = store.evict_oldest(1);
        assert_eq!(removed, vec![finalized.line.id]);
        assert_eq!(store.tombstones(), removed);
    }

    #[test]
    fn frozen_quota_is_enforced_by_the_store() {
        let mut store = TranscriptStore::with_quotas(8, 2);
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
