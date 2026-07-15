//! Adapter around `alacritty_terminal`; the active grid has no scrollback.
//!
//! The vendored compatibility seam reports semantic transcript events and the exact removed rows;
//! the adapter never guesses scroll intent from damage or raw control-sequence bytes.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

use alacritty_terminal::{
    Term,
    event::EventListener,
    grid::Dimensions,
    index::{Column, Line},
    term::{
        Config, ScrollOutCause, TranscriptEvent,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color, Processor},
};
use bt_detect::{
    DecorationRecord, DetectionTask, detect_block_math, redetect_document, render_placeholder,
};
use bt_doc::{
    DecorationIntent, DetectionRevision, GridGeneration, HistoryDocument, LayoutKey,
    LiveRowRemoval, SourceLifecycle, VersionStamp, ViewGeneration,
};
use bt_transcript::{
    CaptureResult, CapturedCell, CapturedRow, CellStyle, DEFAULT_FROZEN_QUOTA,
    DEFAULT_STAGING_QUOTA, Finalized, SourceGeneration, StagingId, TranscriptId, TranscriptStore,
};
use bt_viewport::{SUBPIXELS_PER_PX, ViewportProjection};

pub const SCROLLBACK_LINES: usize = 0;

#[derive(Clone, Copy)]
struct GridSize {
    columns: usize,
    rows: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

#[derive(Clone, Debug)]
pub struct RemovedLiveRow {
    pub live_row: u32,
    pub capture: Option<(CaptureResult, SourceGeneration)>,
}

#[derive(Clone, Debug)]
pub enum AdapterEvent {
    RowsRemoved(Vec<RemovedLiveRow>),
    ForcedFinalize(Finalized),
    PrimaryParked,
    PrimaryRestored,
    HistoryCleared(Vec<TranscriptId>),
    QuotaEvicted(Vec<TranscriptId>),
    CandidatesInvalidated,
}

#[derive(Clone, Default)]
struct CaptureListener {
    transcript_events: Arc<Mutex<Vec<TranscriptEvent>>>,
}

impl EventListener for CaptureListener {}

pub struct TerminalAdapter {
    term: Term<CaptureListener>,
    processor: Processor,
    transcript: TranscriptStore,
    listener: CaptureListener,
    resize_epoch: u64,
    quiescent_epoch: u64,
}

impl TerminalAdapter {
    pub fn new(columns: usize, rows: usize) -> Self {
        Self::with_quotas(columns, rows, DEFAULT_STAGING_QUOTA, DEFAULT_FROZEN_QUOTA)
    }

    pub fn with_quotas(
        columns: usize,
        rows: usize,
        staging_quota: usize,
        frozen_quota: usize,
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
            transcript_events.lock().unwrap().push(event);
        })));
        Self {
            term,
            processor: Processor::new(),
            transcript: TranscriptStore::with_quotas(staging_quota, frozen_quota),
            listener,
            resize_epoch: 0,
            quiescent_epoch: 0,
        }
    }

    pub fn alacritty_history_size(&self) -> usize {
        self.term.grid().history_size()
    }
    pub fn transcript(&self) -> &TranscriptStore {
        &self.transcript
    }
    pub fn transcript_mut(&mut self) -> &mut TranscriptStore {
        &mut self.transcript
    }
    pub fn dimensions(&self) -> (usize, usize) {
        (self.term.columns(), self.term.screen_lines())
    }
    pub fn decorations_allowed(&self) -> bool {
        self.resize_epoch == self.quiescent_epoch
    }
    pub fn mark_resize_quiescent(&mut self) {
        self.quiescent_epoch = self.resize_epoch;
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<AdapterEvent> {
        self.processor.advance(&mut self.term, bytes);
        self.drain_transcript_events()
    }

    pub fn resize(&mut self, columns: usize, rows: usize) -> Vec<AdapterEvent> {
        self.resize_epoch += 1;
        let old_columns = self.dimensions().0;
        let mut events = Vec::new();

        if columns != old_columns {
            events.extend(
                self.transcript
                    .width_resize()
                    .into_iter()
                    .map(AdapterEvent::ForcedFinalize),
            );
            let evicted = self.transcript.take_evictions();
            if !evicted.is_empty() {
                events.push(AdapterEvent::QuotaEvicted(evicted));
            }
        }
        self.term.resize(GridSize { columns, rows });
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

    pub fn visible_row(&self, row: usize) -> Option<CapturedRow> {
        (row < self.term.screen_lines()).then(|| {
            let cells = (0..self.term.columns())
                .map(|column| self.term.grid()[Line(row as i32)][Column(column)].clone())
                .collect::<Vec<_>>();
            to_captured_row(&cells)
        })
    }

    fn drain_transcript_events(&mut self) -> Vec<AdapterEvent> {
        let transcript_events =
            std::mem::take(&mut *self.listener.transcript_events.lock().unwrap());
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
                    let removed = self.transcript.clear_history();
                    events.push(AdapterEvent::HistoryCleared(removed));
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

fn snapshot(term: &Term<CaptureListener>) -> Vec<Vec<Cell>> {
    (0..term.screen_lines())
        .map(|row| {
            (0..term.columns())
                .map(|column| term.grid()[Line(row as i32)][Column(column)].clone())
                .collect()
        })
        .collect()
}

fn row_is_blank(row: &[Cell]) -> bool {
    row.iter()
        .all(|cell| cell.c == ' ' && cell.zerowidth().is_none_or(|z| z.is_empty()))
}

fn to_captured_row(row: &[Cell]) -> CapturedRow {
    let continues = row
        .last()
        .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE));
    let cells = row
        .iter()
        .map(|cell| {
            let mut text = cell.c.to_string();
            if let Some(zero_width) = cell.zerowidth() {
                text.extend(zero_width);
            }
            CapturedCell {
                text,
                style: CellStyle {
                    flags: cell.flags.bits(),
                    foreground: encode_color(cell.fg),
                    background: encode_color(cell.bg),
                },
                hyperlink: cell.hyperlink().map(|link| link.uri().to_string()),
                wide_spacer: cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
            }
        })
        .collect();
    CapturedRow {
        cells,
        continues,
        shell_mark: None,
    }
}

fn encode_color(color: Color) -> u32 {
    match color {
        Color::Named(named) => 0x0100_0000 | named as u32,
        Color::Indexed(index) => 0x0200_0000 | index as u32,
        Color::Spec(rgb) => {
            0x0300_0000 | ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32
        }
    }
}

pub const PARSE_QUANTUM: usize = 256 * 1024;
pub const WORKER_QUEUE_CAP: usize = 64;
pub const SPIKE_CELL_HEIGHT_SUBPIXELS: i64 = 18 * SUBPIXELS_PER_PX;

/// M-1 protocol harness: one serialized owner wires terminal capture through transcript,
/// document, detection and per-view projection. It is deliberately logic-only.
pub struct DualPlaneSession {
    terminal: TerminalAdapter,
    document: HistoryDocument,
    decorations: BTreeMap<TranscriptId, DecorationRecord>,
    pending: VecDeque<DetectionTask>,
    retry_on_idle: BTreeSet<TranscriptId>,
    staging_sources: BTreeMap<StagingId, SourceLifecycle>,
    active_staging_tail: Option<StagingId>,
    detection_revision: DetectionRevision,
    layout_key: LayoutKey,
    view_generation: ViewGeneration,
    grid_generation: GridGeneration,
    stale_results: usize,
    primary_parked: bool,
}

impl DualPlaneSession {
    pub fn new(columns: usize, rows: usize) -> Self {
        Self::with_frozen_quota(columns, rows, DEFAULT_FROZEN_QUOTA)
    }

    pub fn with_frozen_quota(columns: usize, rows: usize, frozen_quota: usize) -> Self {
        Self::with_quotas(columns, rows, DEFAULT_STAGING_QUOTA, frozen_quota)
    }

    pub fn with_quotas(
        columns: usize,
        rows: usize,
        staging_quota: usize,
        frozen_quota: usize,
    ) -> Self {
        Self {
            terminal: TerminalAdapter::with_quotas(columns, rows, staging_quota, frozen_quota),
            document: HistoryDocument::default(),
            decorations: BTreeMap::new(),
            pending: VecDeque::new(),
            retry_on_idle: BTreeSet::new(),
            staging_sources: BTreeMap::new(),
            active_staging_tail: None,
            detection_revision: DetectionRevision(1),
            layout_key: LayoutKey {
                width_cells: columns as u32,
                dpi_milli: 1000,
                font_rev: 1,
                theme_rev: 1,
            },
            view_generation: ViewGeneration(1),
            grid_generation: GridGeneration(1),
            stale_results: 0,
            primary_parked: false,
        }
    }

    pub fn terminal(&self) -> &TerminalAdapter {
        &self.terminal
    }
    pub fn document(&self) -> &HistoryDocument {
        &self.document
    }
    pub fn document_mut(&mut self) -> &mut HistoryDocument {
        &mut self.document
    }
    pub fn decoration(&self, id: TranscriptId) -> Option<&DecorationRecord> {
        self.decorations.get(&id)
    }
    pub fn pending_tasks(&self) -> usize {
        self.pending.len()
    }
    pub fn stale_results(&self) -> usize {
        self.stale_results
    }
    pub fn retry_on_idle(&self) -> usize {
        self.retry_on_idle.len()
    }
    pub fn grid_generation(&self) -> GridGeneration {
        self.grid_generation
    }

    /// The actor quantum is observable here; parser calls receive whole slices, never bytes.
    pub fn feed(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(PARSE_QUANTUM) {
            let events = self.terminal.feed(chunk);
            self.apply_events(events);
            self.sync_staging_tail();
        }
    }

    pub fn resize(&mut self, columns: usize, rows: usize) {
        self.grid_generation.0 += 1;
        let events = self.terminal.resize(columns, rows);
        self.apply_events(events);
        let next_layout = LayoutKey {
            width_cells: columns as u32,
            ..self.layout_key
        };
        if next_layout != self.layout_key {
            self.layout_key = next_layout;
            self.invalidate_layout();
        }
    }

    pub fn mark_resize_quiescent(&mut self) {
        self.terminal.mark_resize_quiescent();
        self.schedule_existing_artifacts();
    }

    pub fn run_workers(&mut self) {
        loop {
            while let Some(task) = self.take_worker_task() {
                self.complete_worker_task(task);
            }
            if self.retry_on_idle.is_empty() {
                break;
            }
            self.schedule_existing_artifacts();
            if self.pending.is_empty() {
                break;
            }
        }
    }

    pub fn take_worker_task(&mut self) -> Option<DetectionTask> {
        self.pending.pop_front()
    }

    pub fn complete_worker_task(&mut self, task: DetectionTask) -> bool {
        let artifact = render_placeholder(&task);
        let accepted = self
            .decorations
            .get_mut(&task.transcript_id)
            .is_some_and(|record| record.complete(&task, artifact));
        if !accepted {
            self.stale_results += 1;
        }
        accepted
    }

    pub fn redetect(&mut self, revision: DetectionRevision) {
        if revision == self.detection_revision {
            return;
        }
        self.detection_revision = revision;
        for record in self.decorations.values_mut() {
            record.detector_changed(revision);
        }
        let detected = redetect_document(&mut self.document, revision);
        for (id, span) in detected {
            if let Some(record) = self.decorations.get_mut(&id) {
                if !self.primary_parked
                    && self.terminal.decorations_allowed()
                    && let Some(task) = record.schedule(id, span)
                {
                    self.enqueue_task(task);
                }
            }
        }
    }

    pub fn project_view(&self, layout_key: LayoutKey) -> ViewportProjection {
        let mut projection = ViewportProjection::new(
            layout_key,
            self.detection_revision,
            self.terminal.dimensions().1 as u32,
            SPIKE_CELL_HEIGHT_SUBPIXELS,
        );
        self.refresh_view(&mut projection);
        projection
    }

    pub fn refresh_view(&self, projection: &mut ViewportProjection) {
        projection.set_live_rows(self.terminal.dimensions().1 as u32);
        projection.sync_artifact_heights(self.decorations.iter().filter_map(|(id, record)| {
            record
                .artifact
                .as_ref()
                .map(|artifact| (*id, artifact.height_subpixels))
        }));
        projection.apply_detection_revision(self.detection_revision, &self.document);
        projection.project(&self.document);
    }

    pub fn bump_view_generation(&mut self) {
        self.view_generation.0 += 1;
        for record in self.decorations.values_mut() {
            record.view_changed(self.view_generation);
        }
        self.schedule_existing_artifacts();
    }

    fn apply_events(&mut self, events: Vec<AdapterEvent>) {
        for event in events {
            match event {
                AdapterEvent::RowsRemoved(rows) => {
                    self.grid_generation.0 += 1;
                    let removals = rows
                        .iter()
                        .map(|row| LiveRowRemoval {
                            row: row.live_row,
                            staging: row
                                .capture
                                .as_ref()
                                .map(|(result, generation)| (result.staging_id, *generation)),
                        })
                        .collect::<Vec<_>>();
                    self.document
                        .capture_rows_transaction(&removals, self.grid_generation);
                    for row in rows {
                        let Some((result, _)) = row.capture else {
                            continue;
                        };
                        self.staging_sources
                            .insert(result.staging_id, SourceLifecycle::Live);
                        self.active_staging_tail =
                            result.finalized.is_empty().then_some(result.staging_id);
                        for finalized in result.finalized {
                            self.ingest_finalized(finalized);
                        }
                    }
                }
                AdapterEvent::ForcedFinalize(finalized) => self.ingest_finalized(finalized),
                AdapterEvent::HistoryCleared(removed) => self.delete_history(&removed, true),
                AdapterEvent::QuotaEvicted(removed) => self.delete_history(&removed, false),
                AdapterEvent::CandidatesInvalidated => {
                    self.staging_sources.clear();
                    self.active_staging_tail = None;
                    self.document
                        .delete_transaction(&[], true, self.grid_generation);
                }
                AdapterEvent::PrimaryParked => {
                    self.primary_parked = true;
                    self.bump_view_generation();
                }
                AdapterEvent::PrimaryRestored => {
                    self.primary_parked = false;
                    self.grid_generation.0 += 1;
                    self.bump_view_generation();
                }
            }
        }
    }

    fn ingest_finalized(&mut self, finalized: Finalized) {
        let closes_active = self.active_staging_tail.is_some_and(|active| {
            finalized
                .mappings
                .iter()
                .any(|mapping| mapping.staging_id == active)
        });
        for mapping in &finalized.mappings {
            if let Some(source) = self.staging_sources.get_mut(&mapping.staging_id) {
                let _ = source.transition(SourceLifecycle::Frozen);
            }
        }
        let id = finalized.line.id;
        self.document.finalize_transaction(finalized);
        self.schedule_detection(id);
        self.staging_sources
            .retain(|_, source| *source != SourceLifecycle::Frozen);
        if closes_active {
            self.active_staging_tail = None;
        }
    }

    fn schedule_detection(&mut self, id: TranscriptId) {
        let Some(entry) = self.document.entries().get(&id) else {
            return;
        };
        let source_generation = entry.line.source_generation;
        let span = detect_block_math(&entry.line.text).into_iter().next();
        let versions = VersionStamp {
            source: source_generation,
            detection: self.detection_revision,
            layout: self.layout_key,
            view: self.view_generation,
        };
        let mut record = DecorationRecord::frozen(versions);
        let mut task = None;
        if let Some(span) = span {
            self.document.set_decoration(
                id,
                DecorationIntent::Math {
                    byte_start: span.byte_start,
                    byte_end: span.byte_end,
                    detection_revision: self.detection_revision,
                },
            );
            if !self.primary_parked && self.terminal.decorations_allowed() {
                task = record.schedule(id, span);
            }
        }
        self.decorations.insert(id, record);
        if let Some(task) = task {
            self.enqueue_task(task);
        }
    }

    fn delete_history(&mut self, removed: &[TranscriptId], clear_staging: bool) {
        self.document
            .delete_transaction(removed, clear_staging, self.grid_generation);
        if clear_staging {
            self.staging_sources.clear();
            self.active_staging_tail = None;
        }
        let removed_set = removed.iter().copied().collect::<BTreeSet<_>>();
        self.pending
            .retain(|task| !removed_set.contains(&task.transcript_id));
        for id in removed {
            self.retry_on_idle.remove(id);
            self.decorations.remove(id);
        }
    }

    fn invalidate_layout(&mut self) {
        for record in self.decorations.values_mut() {
            record.layout_changed(self.layout_key);
        }
        self.schedule_existing_artifacts();
    }

    fn schedule_existing_artifacts(&mut self) {
        if self.primary_parked || !self.terminal.decorations_allowed() {
            return;
        }
        let math = self
            .document
            .entries()
            .iter()
            .filter_map(|(id, entry)| {
                matches!(entry.decoration, DecorationIntent::Math { .. })
                    .then(|| {
                        detect_block_math(&entry.line.text)
                            .into_iter()
                            .next()
                            .map(|span| (*id, span))
                    })
                    .flatten()
            })
            .collect::<Vec<_>>();
        for (id, span) in math {
            if let Some(task) = self
                .decorations
                .get_mut(&id)
                .and_then(|record| record.schedule(id, span))
            {
                self.enqueue_task(task);
            }
        }
    }

    fn enqueue_task(&mut self, task: DetectionTask) {
        if let Some(index) = self
            .pending
            .iter()
            .position(|queued| queued.transcript_id == task.transcript_id)
        {
            self.pending.remove(index);
        }
        if self.pending.len() == WORKER_QUEUE_CAP {
            if let Some(record) = self.decorations.get_mut(&task.transcript_id) {
                record.decoration = bt_doc::DecorationLifecycle::None;
            }
            self.retry_on_idle.insert(task.transcript_id);
        } else {
            self.retry_on_idle.remove(&task.transcript_id);
            self.pending.push_back(task);
        }
    }

    fn sync_staging_tail(&mut self) {
        let Some(id) = self.active_staging_tail else {
            return;
        };
        let Some(row) = self.terminal.visible_row(0) else {
            return;
        };
        if !self.terminal.transcript_mut().rewrite_staged(id, row) {
            self.active_staging_tail = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_doc::{Bias, ContentAnchor, DecorationLifecycle, GridPoint, ScreenId};
    use bt_viewport::{ScrollAnchor, ViewSelection};
    use proptest::prelude::*;

    fn live_anchor(row: u32, column: u32) -> ContentAnchor {
        ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row, column },
            bias: Bias::After,
            generation: GridGeneration(1),
        }
    }

    fn feed_math_to_history(session: &mut DualPlaneSession) -> TranscriptId {
        session.feed(b"$$x$$\r\nfiller\r\ntail");
        *session
            .document()
            .entries()
            .iter()
            .find(|(_, entry)| entry.line.text == "$$x$$")
            .map(|(id, _)| id)
            .expect("math line must scroll into history")
    }

    #[test]
    fn scroll_out_is_captured_with_zero_alacritty_history() {
        let mut terminal = TerminalAdapter::new(8, 3);
        let events = terminal.feed(b"one\r\ntwo\r\nthree\r\nfour");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AdapterEvent::RowsRemoved(rows) if rows.iter().any(|row| row.capture.is_some())))
        );
        assert_eq!(terminal.alacritty_history_size(), 0);
        assert_eq!(terminal.transcript().frozen().front().unwrap().text, "one");
    }

    #[test]
    fn alternate_screen_and_local_scroll_region_do_not_capture() {
        let mut terminal = TerminalAdapter::new(8, 4);
        terminal.feed(b"keep\r\na\r\nb\r\nc");
        let before = terminal.transcript().frozen().len();
        terminal.feed(b"\x1b[?1049h1\r\n2\r\n3\r\n4\r\n5\x1b[?1049l");
        assert_eq!(terminal.transcript().frozen().len(), before);

        terminal.feed(b"\x1b[2;3r\x1b[3;1Hlocal\nlocal\n");
        assert_eq!(terminal.transcript().frozen().len(), before);
    }

    #[test]
    fn grow_adds_live_rows_and_never_refills_history() {
        let mut terminal = TerminalAdapter::new(8, 2);
        terminal.feed(b"old\r\nnew\r\ntail");
        let frozen = terminal.transcript().frozen().len();
        let events = terminal.resize(8, 4);
        assert!(events.is_empty());
        assert_eq!(terminal.transcript().frozen().len(), frozen);
        assert_eq!(terminal.dimensions(), (8, 4));
        assert_eq!(terminal.visible_text().len(), 4);
    }

    #[test]
    fn width_resize_forces_staging_split() {
        let mut terminal = TerminalAdapter::new(4, 2);
        terminal
            .transcript_mut()
            .capture(CapturedRow::plain("head", true));
        let events = terminal.resize(6, 2);
        assert!(matches!(&events[0], AdapterEvent::ForcedFinalize(value) if value.line.wrap_split));
    }

    #[test]
    fn ed3_and_ris_follow_distinct_deletion_rules() {
        let mut terminal = TerminalAdapter::new(8, 2);
        terminal.feed(b"old\r\nnew\r\ntail");
        assert!(!terminal.transcript().frozen().is_empty());
        terminal.feed(b"\x1b[3J");
        assert!(terminal.transcript().frozen().is_empty());

        terminal.feed(b"one\r\ntwo\r\nthree");
        let frozen = terminal.transcript().frozen().len();
        terminal
            .transcript_mut()
            .capture(CapturedRow::plain("partial", true));
        terminal.feed(b"\x1bc");
        assert_eq!(terminal.transcript().frozen().len(), frozen);
        assert_eq!(terminal.transcript().staging_len(), 0);
    }

    #[test]
    fn shrink_captures_only_nonblank_rows_removed_from_the_top() {
        let mut terminal = TerminalAdapter::new(8, 4);
        terminal.feed(b"r1\r\nr2\r\nr3\r\nr4");
        let before = terminal.transcript().frozen().len();
        let events = terminal.resize(8, 2);
        let captured = events
            .iter()
            .map(|event| match event {
                AdapterEvent::RowsRemoved(rows) => {
                    rows.iter().filter(|row| row.capture.is_some()).count()
                }
                _ => 0,
            })
            .sum::<usize>();
        assert_eq!(captured, terminal.transcript().frozen().len() - before);
        assert!(terminal.visible_text().iter().all(|line| line != "r1"));
    }

    #[test]
    fn width_reflow_and_resize_jitter_do_not_mutate_frozen_source() {
        let mut terminal = TerminalAdapter::new(8, 3);
        terminal.feed(b"frozen\r\nsecond\r\nthird\r\ntail");
        let source = terminal
            .transcript()
            .frozen()
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>();
        for width in [4, 12, 5, 10, 8] {
            terminal.resize(width, 3);
        }
        assert_eq!(
            terminal
                .transcript()
                .frozen()
                .iter()
                .map(|line| line.text.clone())
                .collect::<Vec<_>>(),
            source
        );
        assert!(!terminal.decorations_allowed());
        terminal.mark_resize_quiescent();
        assert!(terminal.decorations_allowed());
    }

    #[test]
    fn deccolm_invalidates_candidates_and_unterminated_last_line_never_freezes() {
        let mut terminal = TerminalAdapter::new(8, 3);
        terminal.feed(b"last");
        assert!(terminal.transcript().frozen().is_empty());
        terminal
            .transcript_mut()
            .capture(CapturedRow::plain("partial", true));
        assert_eq!(terminal.transcript().staging_len(), 1);
        let events = terminal.feed(b"\x1b[?3h");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AdapterEvent::CandidatesInvalidated))
        );
        assert_eq!(terminal.transcript().staging_len(), 0);
    }

    #[test]
    fn regression_dl_at_row_zero_is_not_history() {
        let mut terminal = TerminalAdapter::new(8, 4);
        terminal.feed(b"a\r\nb\r\nc");
        assert!(terminal.transcript().frozen().is_empty());
        terminal.feed(b"\x1b[1;1H\x1b[1M");
        assert!(terminal.transcript().frozen().is_empty());
    }

    #[test]
    fn regression_width_and_height_shrink_preserves_removed_top_rows() {
        let mut terminal = TerminalAdapter::new(8, 4);
        terminal.feed(b"r1\r\nr2\r\nr3\r\nr4");
        terminal.resize(6, 2);
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
        for (name, payload) in [
            ("APC", b"\x1b_payload [3J\x1b\\".as_slice()),
            ("DCS", b"\x1bP0;1|[3J\x1b\\".as_slice()),
        ] {
            let mut terminal = TerminalAdapter::new(8, 2);
            terminal.feed(b"old\r\nnew\r\ntail");
            let before = terminal.transcript().frozen().len();
            assert!(before > 0);
            terminal.feed(payload);
            assert_eq!(terminal.transcript().frozen().len(), before, "{name}");
        }

        let mut terminal = TerminalAdapter::new(8, 2);
        terminal.feed(b"old\r\nnew\r\ntail");
        assert!(!terminal.transcript().frozen().is_empty());
        terminal.feed(b"\x1b_payload\x1b[3J");
        assert!(terminal.transcript().frozen().is_empty());
    }

    #[test]
    fn unterminated_apc_and_dcs_can_resynchronize_on_escape() {
        for introducer in [b"\x1b_stuck".as_slice(), b"\x1bP0;1|stuck".as_slice()] {
            let mut terminal = TerminalAdapter::new(8, 2);
            terminal.feed(introducer);
            terminal.feed(b"\x1b[2J\x1b[1;1Hhello");
            assert!(terminal.visible_text().iter().any(|line| line == "hello"));
        }
    }

    #[test]
    fn live_anchor_rebases_after_scroll_and_height_shrink() {
        let mut scroll = DualPlaneSession::new(8, 2);
        let row_one = scroll.document_mut().register_anchor(live_anchor(1, 1));
        scroll.feed(b"one\r\ntwo");
        scroll.feed(b"\r\nthree");
        assert!(matches!(
            scroll.document().anchor(row_one).unwrap(),
            ContentAnchor::Live {
                point: GridPoint { row: 0, column: 1 },
                ..
            }
        ));

        let mut shrink = DualPlaneSession::new(8, 4);
        let row_one_removed = shrink.document_mut().register_anchor(live_anchor(1, 1));
        let row_three = shrink.document_mut().register_anchor(live_anchor(3, 1));
        shrink.feed(b"r1\r\nr2\r\nr3\r\nr4");
        shrink.resize(8, 2);
        assert!(matches!(
            shrink.document().anchor(row_one_removed).unwrap(),
            ContentAnchor::History { .. }
        ));
        assert!(matches!(
            shrink.document().anchor(row_three).unwrap(),
            ContentAnchor::Live {
                point: GridPoint { row: 1, column: 1 },
                ..
            }
        ));
    }

    #[test]
    fn g1_scroll_staging_finalize_decorate_and_tail_rewrite_from_vt_bytes() {
        let mut session = DualPlaneSession::new(4, 2);
        let anchor = session.document_mut().register_anchor(live_anchor(0, 2));

        session.feed(b"$$x$$\r\nz");
        let staging_id = match session.document().anchor(anchor).unwrap() {
            ContentAnchor::Staging { id, .. } => *id,
            other => panic!("expected observable staging anchor, got {other:?}"),
        };
        let before = session
            .terminal()
            .transcript()
            .staged_tail(staging_id)
            .expect("live continuation snapshot")
            .clone();
        session.feed(b"\x1b[1;1H$ok");
        let after = session
            .terminal()
            .transcript()
            .staged_tail(staging_id)
            .expect("rewritten continuation snapshot");
        assert_ne!(after, &before);

        session.feed(b"\x1b[2;1H\r\nq");
        let history_id = match session.document().anchor(anchor).unwrap() {
            ContentAnchor::History { id, .. } => *id,
            other => panic!("expected finalized history anchor, got {other:?}"),
        };
        assert!(matches!(
            session.document().entries()[&history_id].decoration,
            DecorationIntent::Math { .. }
        ));
        session.run_workers();
        assert_eq!(
            session.decoration(history_id).unwrap().decoration,
            DecorationLifecycle::Ready
        );
        let projection = session.project_view(LayoutKey {
            width_cells: 4,
            dpi_milli: 1000,
            font_rev: 1,
            theme_rev: 1,
        });
        let index = session
            .document()
            .entries()
            .keys()
            .position(|id| *id == history_id)
            .unwrap();
        assert_eq!(projection.heights().get(index), Some(64 * SUBPIXELS_PER_PX));
    }

    #[test]
    fn g2_two_widths_project_the_same_byte_driven_anchor_independently() {
        let mut session = DualPlaneSession::new(10, 2);
        let anchor_id = session.document_mut().register_anchor(live_anchor(0, 6));
        session.feed(b"abcdefg\r\nnext\r\ntail");
        let anchor = session.document().anchor(anchor_id).unwrap().clone();
        assert!(matches!(anchor, ContentAnchor::History { .. }));

        let mut narrow = session.project_view(LayoutKey {
            width_cells: 4,
            dpi_milli: 1000,
            font_rev: 1,
            theme_rev: 1,
        });
        let mut wide = session.project_view(LayoutKey {
            width_cells: 10,
            dpi_milli: 1000,
            font_rev: 1,
            theme_rev: 1,
        });
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

        assert_eq!(
            narrow.anchor_y(session.document(), &anchor).unwrap(),
            18 * SUBPIXELS_PER_PX
        );
        assert_eq!(wide.anchor_y(session.document(), &anchor).unwrap(), 0);
        assert_eq!(
            narrow.scroll_y(session.document()).unwrap(),
            Some(18 * SUBPIXELS_PER_PX + 7)
        );
        assert_eq!(wide.scroll_y(session.document()).unwrap(), Some(19));
        assert!(narrow.selection_y(session.document()).unwrap().is_some());
        assert!(wide.selection_y(session.document()).unwrap().is_none());
    }

    #[test]
    fn projection_refresh_reuses_cache_and_invalidates_only_changed_artifact() {
        let mut session = DualPlaneSession::new(12, 2);
        let math_id = feed_math_to_history(&mut session);
        let key = LayoutKey {
            width_cells: 12,
            dpi_milli: 1000,
            font_rev: 1,
            theme_rev: 1,
        };
        let mut projection = session.project_view(key);
        let index = session
            .document()
            .entries()
            .keys()
            .position(|id| *id == math_id)
            .unwrap();
        let plain_misses = projection.cache_misses();
        assert_eq!(
            projection.heights().get(index),
            Some(SPIKE_CELL_HEIGHT_SUBPIXELS)
        );

        session.run_workers();
        session.refresh_view(&mut projection);
        assert_eq!(projection.heights().get(index), Some(64 * SUBPIXELS_PER_PX));
        assert_eq!(projection.cache_misses(), plain_misses + 1);

        session.refresh_view(&mut projection);
        assert_eq!(projection.cache_misses(), plain_misses + 1);
    }

    #[test]
    fn g3_versions_stale_workers_and_ed3_are_one_integrated_protocol() {
        let mut session = DualPlaneSession::new(12, 2);
        let anchor = session.document_mut().register_anchor(live_anchor(0, 1));
        let math_id = feed_math_to_history(&mut session);
        assert!(matches!(
            session.document().anchor(anchor).unwrap(),
            ContentAnchor::History { id, .. } if *id == math_id
        ));
        let v1 = session.decoration(math_id).unwrap().versions;
        assert_eq!(session.pending_tasks(), 1);
        let detection_v1_task = session.take_worker_task().unwrap();

        session.redetect(DetectionRevision(2));
        assert_eq!(session.pending_tasks(), 1);
        assert!(!session.complete_worker_task(detection_v1_task));
        session.run_workers();
        assert_eq!(session.stale_results(), 1);
        let v2 = session.decoration(math_id).unwrap();
        assert_eq!(v2.versions.source, v1.source);
        assert_eq!(v2.versions.detection, DetectionRevision(2));
        assert_eq!(v2.decoration, DecorationLifecycle::Ready);
        assert!(matches!(
            session.document().entries()[&math_id].decoration,
            DecorationIntent::Math {
                detection_revision: DetectionRevision(2),
                ..
            }
        ));

        session.resize(8, 2);
        let resized = session.decoration(math_id).unwrap();
        assert_eq!(resized.versions.source, v1.source);
        assert_eq!(resized.versions.detection, DetectionRevision(2));
        assert_eq!(resized.versions.layout.width_cells, 8);
        assert_eq!(resized.decoration, DecorationLifecycle::None);
        session.mark_resize_quiescent();
        assert_eq!(session.pending_tasks(), 1);
        let old_view_task = session.take_worker_task().unwrap();
        session.bump_view_generation();
        assert_eq!(session.pending_tasks(), 1);
        assert!(!session.complete_worker_task(old_view_task));
        session.run_workers();
        assert_eq!(session.stale_results(), 2);

        session.feed(b"\x1b[3J");
        assert!(session.document().entries().is_empty());
        assert!(matches!(
            session.document().anchor(anchor).unwrap(),
            ContentAnchor::Live {
                point: GridPoint { row: 0, column: 0 },
                bias: Bias::Before,
                ..
            }
        ));
    }

    #[test]
    fn g1_resize_matrix_uses_grid_contents_as_oracles() {
        let mut grow = DualPlaneSession::new(4, 2);
        grow.feed(b"old\r\ntail");
        let history_before = grow.document().entries().len();
        grow.resize(4, 4);
        grow.feed(b"\x1b[2J\x1b[1;1HA\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD");
        assert_eq!(grow.terminal().visible_text(), vec!["A", "B", "C", "D"]);
        assert_eq!(grow.document().entries().len(), history_before);

        let mut shrink = DualPlaneSession::new(8, 4);
        shrink.feed(b"r1\r\nr2\r\nr3\r\nr4");
        shrink.resize(6, 2);
        let frozen = shrink
            .document()
            .entries()
            .values()
            .map(|entry| entry.line.text.as_str())
            .collect::<Vec<_>>();
        assert!(frozen.contains(&"r1"));
        assert!(frozen.contains(&"r2"));
        assert!(
            shrink
                .terminal()
                .visible_text()
                .iter()
                .all(|line| line != "r1")
        );

        let mut reflow = DualPlaneSession::new(12, 3);
        reflow.feed(b"immutable\r\nsecond\r\nthird\r\ntail");
        let original = reflow
            .document()
            .entries()
            .values()
            .find(|entry| entry.line.text == "immutable")
            .unwrap()
            .line
            .clone();
        for width in [6, 16, 7, 12] {
            reflow.resize(width, 3);
        }
        assert_eq!(reflow.document().entries()[&original.id].line, original);

        let mut jitter = DualPlaneSession::new(12, 2);
        let math_id = feed_math_to_history(&mut jitter);
        jitter.run_workers();
        for width in [8, 14, 9, 12] {
            jitter.resize(width, 2);
        }
        assert_eq!(jitter.pending_tasks(), 0);
        assert_eq!(
            jitter.decoration(math_id).unwrap().decoration,
            DecorationLifecycle::None
        );
        jitter.mark_resize_quiescent();
        assert_eq!(jitter.pending_tasks(), 1);
    }

    #[test]
    fn g1_staging_limits_split_and_live_controls_are_byte_driven() {
        let mut split = DualPlaneSession::with_quotas(2, 2, 2, 32);
        split.feed(b"abcdefghijklmnop");
        assert!(
            split
                .document()
                .entries()
                .values()
                .any(|entry| entry.line.wrap_split)
        );
        assert!(split.terminal().transcript().staging_len() <= 2);

        let mut boundary = DualPlaneSession::new(4, 2);
        boundary.feed(b"abcdefghi");
        assert!(boundary.terminal().transcript().staging_len() > 0);
        boundary.resize(6, 2);
        assert!(
            boundary
                .document()
                .entries()
                .values()
                .any(|entry| entry.line.wrap_split)
        );

        let mut controls = DualPlaneSession::new(8, 4);
        controls.feed(b"keep\r\na\r\nb\r\nc");
        let before = controls.document().entries().len();
        controls.feed(b"\x1b[?1049h1\r\n2\r\n3\r\n4\r\n5\x1b[?1049l");
        assert_eq!(controls.document().entries().len(), before);
        controls.feed(b"\x1b[2;3r\x1b[3;1Hlocal\nlocal\n");
        assert_eq!(controls.document().entries().len(), before);

        let mut last = DualPlaneSession::new(8, 3);
        last.feed(b"unterminated");
        assert!(last.document().entries().is_empty());
        last.feed(b"\x1bc");
        assert!(last.document().entries().is_empty());

        let mut ris = DualPlaneSession::new(4, 2);
        let ris_anchor = ris.document_mut().register_anchor(live_anchor(0, 1));
        ris.feed(b"abcdefghi");
        assert!(matches!(
            ris.document().anchor(ris_anchor).unwrap(),
            ContentAnchor::Staging { .. }
        ));
        ris.feed(b"\x1bc");
        assert!(matches!(
            ris.document().anchor(ris_anchor).unwrap(),
            ContentAnchor::Live { .. }
        ));

        let mut deccolm = DualPlaneSession::new(4, 2);
        let anchor = deccolm.document_mut().register_anchor(live_anchor(0, 1));
        deccolm.feed(b"abcdefghi");
        assert!(matches!(
            deccolm.document().anchor(anchor).unwrap(),
            ContentAnchor::Staging { .. }
        ));
        deccolm.feed(b"\x1b[?3h");
        assert_eq!(deccolm.terminal().transcript().staging_len(), 0);
        assert!(matches!(
            deccolm.document().anchor(anchor).unwrap(),
            ContentAnchor::Live { .. }
        ));
    }

    #[test]
    fn g1_frozen_quota_calls_the_same_anchor_deletion_pipeline() {
        let mut session = DualPlaneSession::with_frozen_quota(8, 2, 1);
        let anchor = session.document_mut().register_anchor(live_anchor(0, 1));
        session.feed(b"one\r\ntwo\r\nthree\r\nfour");
        assert_eq!(session.document().entries().len(), 1);
        assert!(!session.document().tombstones().is_empty());
        let current = session.document().anchor(anchor).unwrap();
        if let ContentAnchor::History { id, .. } = current {
            assert!(session.document().entries().contains_key(id));
        } else {
            assert!(matches!(current, ContentAnchor::Live { .. }));
        }
    }

    #[test]
    fn worker_queue_is_bounded_and_retries_on_idle() {
        let mut bytes = Vec::new();
        for index in 0..70 {
            bytes.extend_from_slice(format!("$$x{index}$$\r\n").as_bytes());
        }
        bytes.extend_from_slice(b"tail");
        let mut session = DualPlaneSession::new(32, 2);
        session.feed(&bytes);
        assert_eq!(session.pending_tasks(), WORKER_QUEUE_CAP);
        assert!(session.retry_on_idle() > 0);
        session.run_workers();
        assert_eq!(session.pending_tasks(), 0);
        assert_eq!(session.retry_on_idle(), 0);
        assert!(session.document().entries().iter().all(|(id, _)| {
            session.decoration(*id).unwrap().decoration == DecorationLifecycle::Ready
        }));
    }

    #[test]
    fn alt_screen_parks_detection_and_invalidates_in_flight_generation() {
        let mut session = DualPlaneSession::new(12, 2);
        let math_id = feed_math_to_history(&mut session);
        let in_flight = session.take_worker_task().unwrap();
        session.feed(b"\x1b[?1049hpaint\r\npaint\r\npaint");
        assert!(!session.complete_worker_task(in_flight));
        assert_eq!(session.pending_tasks(), 0);
        session.feed(b"\x1b[?1049l");
        assert_eq!(session.pending_tasks(), 1);
        session.run_workers();
        assert_eq!(
            session.decoration(math_id).unwrap().decoration,
            DecorationLifecycle::Ready
        );
    }

    proptest! {
        #[test]
        fn parser_is_invariant_under_random_chunk_boundaries(cuts in prop::collection::vec(1usize..32, 1..32)) {
            let bytes = b"one\r\n$$x$$\r\nthree\r\nfour\x1b[3;1H!";
            let mut whole = DualPlaneSession::new(16, 3);
            whole.feed(bytes);

            let mut chunked = DualPlaneSession::new(16, 3);
            let mut offset = 0;
            for size in cuts {
                if offset == bytes.len() { break; }
                let end = (offset + size).min(bytes.len());
                chunked.feed(&bytes[offset..end]);
                offset = end;
            }
            if offset < bytes.len() {
                chunked.feed(&bytes[offset..]);
            }

            prop_assert_eq!(chunked.terminal().visible_text(), whole.terminal().visible_text());
            prop_assert_eq!(
                chunked.document().entries().values().map(|entry| entry.line.text.clone()).collect::<Vec<_>>(),
                whole.document().entries().values().map(|entry| entry.line.text.clone()).collect::<Vec<_>>()
            );
        }
    }
}
