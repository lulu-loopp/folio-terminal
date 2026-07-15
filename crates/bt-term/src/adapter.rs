use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::{Arc, Mutex, MutexGuard},
};

use alacritty_terminal::{
    Term,
    event::EventListener,
    grid::Dimensions,
    index::{Column, Line},
    term::{Config, ScrollOutCause, TranscriptEvent},
    vte::ansi::Processor,
};
use bt_transcript::{
    CaptureResult, CapturedRow, FinalizeReason, FinalizedLine, SourceGeneration, TranscriptId,
    TranscriptStore,
};

use crate::cell_capture::{row_is_blank, snapshot, to_captured_row};

pub const SCROLLBACK_LINES: usize = 0;

#[derive(Clone, Copy)]
struct GridSize {
    columns: NonZeroU32,
    rows: NonZeroU32,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows.get() as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows.get() as usize
    }

    fn columns(&self) -> usize {
        self.columns.get() as usize
    }
}

#[derive(Clone, Debug)]
pub struct RemovedLiveRow {
    pub live_row: u32,
    pub capture: Option<(CaptureResult, SourceGeneration)>,
}

/// DESIGN.md §3.1 semantic facts reported by the alacritty adapter.
/// Policy decisions are deliberately deferred to the lifecycle/session modules.
#[derive(Clone, Debug)]
pub enum AdapterEvent {
    RowsRemoved(Vec<RemovedLiveRow>),
    ForcedFinalize(FinalizedLine),
    PrimaryParked,
    PrimaryRestored,
    HistoryCleared(Vec<TranscriptId>),
    QuotaEvicted(Vec<TranscriptId>),
    CandidatesInvalidated,
}

#[derive(Clone, Default)]
pub(crate) struct CaptureListener {
    transcript_events: Arc<Mutex<Vec<TranscriptEvent>>>,
}

impl EventListener for CaptureListener {}

fn lock_events(listener: &CaptureListener) -> MutexGuard<'_, Vec<TranscriptEvent>> {
    listener
        .transcript_events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Vendor-facing terminal adapter. This module exposes facts, not decoration policy.
pub struct TerminalAdapter {
    term: Term<CaptureListener>,
    processor: Processor,
    transcript: TranscriptStore,
    listener: CaptureListener,
    columns: NonZeroU32,
    rows: NonZeroU32,
}

impl TerminalAdapter {
    pub fn new(columns: NonZeroU32, rows: NonZeroU32) -> Self {
        Self::with_quotas(
            columns,
            rows,
            bt_transcript::DEFAULT_STAGING_QUOTA,
            bt_transcript::SPIKE_DEFAULT_FROZEN_QUOTA,
        )
    }

    pub fn with_quotas(
        columns: NonZeroU32,
        rows: NonZeroU32,
        staging_quota: NonZeroUsize,
        frozen_quota: NonZeroUsize,
    ) -> Self {
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        let size = GridSize { columns, rows };
        let listener = CaptureListener::default();
        let transcript_events = listener.transcript_events.clone();
        let mut term = Term::new(config, &size, listener.clone());
        term.set_transcript_hook(Some(Arc::new(move |event| {
            transcript_events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(event);
        })));
        Self {
            term,
            processor: Processor::new(),
            transcript: TranscriptStore::with_quotas(staging_quota, frozen_quota),
            listener,
            columns,
            rows,
        }
    }

    pub fn alacritty_history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    pub fn transcript(&self) -> &TranscriptStore {
        &self.transcript
    }

    pub(crate) fn transcript_mut(&mut self) -> &mut TranscriptStore {
        &mut self.transcript
    }

    pub fn dimensions(&self) -> (NonZeroU32, NonZeroU32) {
        (self.columns, self.rows)
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<AdapterEvent> {
        self.processor.advance(&mut self.term, bytes);
        self.drain_transcript_events()
    }

    pub fn resize(&mut self, columns: NonZeroU32, rows: NonZeroU32) -> Vec<AdapterEvent> {
        let old_columns = self.columns;
        let mut events = Vec::new();

        if columns != old_columns {
            events.extend(
                self.transcript
                    .finalize_all_candidates(FinalizeReason::WidthResize)
                    .into_iter()
                    .map(AdapterEvent::ForcedFinalize),
            );
            let evicted = self.transcript.take_evictions();
            if !evicted.is_empty() {
                events.push(AdapterEvent::QuotaEvicted(evicted));
            }
        }
        self.term.resize(GridSize { columns, rows });
        self.columns = columns;
        self.rows = rows;
        events.extend(self.drain_transcript_events());
        events
    }

    pub fn visible_text(&self) -> Vec<String> {
        snapshot(&self.term)
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    pub fn visible_row(&self, row: u32) -> Option<CapturedRow> {
        (row < self.rows.get()).then(|| {
            let cells = (0..self.columns.get())
                .map(|column| self.term.grid()[Line(row as i32)][Column(column as usize)].clone())
                .collect::<Vec<_>>();
            to_captured_row(&cells)
        })
    }

    fn drain_transcript_events(&mut self) -> Vec<AdapterEvent> {
        let transcript_events = std::mem::take(&mut *lock_events(&self.listener));
        let mut events = Vec::new();
        for event in transcript_events {
            match event {
                TranscriptEvent::ScrollOut { cause, rows } => {
                    let mut removed = Vec::with_capacity(rows.len());
                    let mut evicted = Vec::new();
                    for row in rows {
                        let capture = if cause == ScrollOutCause::Resize && row_is_blank(&row.cells)
                        {
                            None
                        } else {
                            let result = self.transcript.capture(to_captured_row(&row.cells));
                            let generation = self.transcript.source_generation();
                            evicted.extend(self.transcript.take_evictions());
                            Some((result, generation))
                        };
                        removed.push(RemovedLiveRow {
                            live_row: row.live_row as u32,
                            capture,
                        });
                    }
                    if !removed.is_empty() {
                        events.push(AdapterEvent::RowsRemoved(removed));
                    }
                    if !evicted.is_empty() {
                        events.push(AdapterEvent::QuotaEvicted(evicted));
                    }
                }
                TranscriptEvent::ClearHistory => {
                    events.push(AdapterEvent::HistoryCleared(
                        self.transcript.clear_history(),
                    ));
                }
                TranscriptEvent::Reset | TranscriptEvent::Deccolm => {
                    self.transcript.invalidate_staging();
                    events.push(AdapterEvent::CandidatesInvalidated);
                }
                TranscriptEvent::PrimaryParked => events.push(AdapterEvent::PrimaryParked),
                TranscriptEvent::PrimaryRestored => events.push(AdapterEvent::PrimaryRestored),
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_transcript::CapturedRow;

    fn nz(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    #[test]
    fn scroll_out_is_captured_with_zero_alacritty_history() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(2));
        terminal.feed(b"one\r\ntwo\r\nthree");
        assert_eq!(terminal.alacritty_history_size(), 0);
        assert_eq!(terminal.transcript().frozen()[0].text, "one");
    }

    #[test]
    fn alternate_screen_and_local_scroll_region_do_not_capture() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"keep\r\na\r\nb\r\nc");
        let before = terminal.transcript().frozen().len();
        terminal.feed(b"\x1b[?1049h1\r\n2\r\n3\r\n4\r\n5\x1b[?1049l");
        terminal.feed(b"\x1b[2;3r\x1b[3;1Hlocal\nlocal\n");
        assert_eq!(terminal.transcript().frozen().len(), before);
    }

    #[test]
    fn deccolm_invalidates_candidates_and_unterminated_last_line_never_freezes() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.feed(b"last");
        assert!(terminal.transcript().frozen().is_empty());
        terminal
            .transcript_mut()
            .capture(CapturedRow::plain("partial", true));
        let events = terminal.feed(b"\x1b[?3h");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AdapterEvent::CandidatesInvalidated))
        );
        assert_eq!(terminal.transcript().staging_len(), 0);
    }

    #[test]
    fn dl_at_row_zero_is_not_history() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc");
        terminal.feed(b"\x1b[1;1H\x1b[1M");
        assert!(terminal.transcript().frozen().is_empty());
    }

    #[test]
    fn width_and_height_shrink_preserves_removed_top_rows() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"r1\r\nr2\r\nr3\r\nr4");
        terminal.resize(nz(6), nz(2));
        let frozen = terminal
            .transcript()
            .frozen()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert!(frozen.contains(&"r1"));
        assert!(frozen.contains(&"r2"));
    }

    #[test]
    fn ed3_is_only_emitted_by_the_vt_clear_history_action() {
        for payload in [
            b"\x1b_payload [3J\x1b\\".as_slice(),
            b"\x1bP0;1|[3J\x1b\\".as_slice(),
        ] {
            let mut terminal = TerminalAdapter::new(nz(8), nz(2));
            terminal.feed(b"old\r\nnew\r\ntail");
            let before = terminal.transcript().frozen().len();
            terminal.feed(payload);
            assert_eq!(terminal.transcript().frozen().len(), before);
        }

        let mut terminal = TerminalAdapter::new(nz(8), nz(2));
        terminal.feed(b"old\r\nnew\r\ntail");
        terminal.feed(b"\x1b_payload\x1b[3J");
        assert!(terminal.transcript().frozen().is_empty());
    }

    #[test]
    fn unterminated_apc_and_dcs_can_resynchronize_on_escape() {
        for introducer in [b"\x1b_stuck".as_slice(), b"\x1bP0;1|stuck".as_slice()] {
            let mut terminal = TerminalAdapter::new(nz(8), nz(2));
            terminal.feed(introducer);
            terminal.feed(b"\x1b[2J\x1b[1;1Hhello");
            assert!(terminal.visible_text().iter().any(|line| line == "hello"));
        }
    }
}
