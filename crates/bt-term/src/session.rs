use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    num::{NonZeroI64, NonZeroU32, NonZeroUsize},
};

use bt_detect::{
    DecorationRecord, DetectionTask, detect_block_math, redetect_document, render_placeholder,
};
use bt_doc::{
    AnchorError, AnchorId, Bias, ContentAnchor, DecorationIntent, DetectionRevision,
    GridGeneration, GridPoint, HistoryDocument, InvalidSourceTransition, LayoutKey, LiveRowRemoval,
    SUBPIXELS_PER_PX, ScreenId, SourceLifecycle, VersionStamp, ViewGeneration,
};
use bt_transcript::{
    CaptureResult, DEFAULT_STAGING_QUOTA, FinalizeReason, FinalizedLine,
    SPIKE_DEFAULT_FROZEN_QUOTA, StagingId, TranscriptId, TranscriptStore,
};
use bt_viewport::{FrameProjectionError, GridCursor, ViewportFrame, ViewportProjection};

use crate::{
    adapter::{AdapterEvent, TerminalAdapter},
    lifecycle::{LifecycleDirective, RowDirective, classify, plan_resize},
    scheduling::{EnqueueOutcome, PARSE_QUANTUM, ResizeEpoch, WorkerScheduler},
};

pub const SPIKE_CELL_HEIGHT_SUBPIXELS: NonZeroI64 = NonZeroI64::new(18 * SUBPIXELS_PER_PX).unwrap();

#[derive(Debug)]
pub enum SessionError {
    InvalidSourceTransition(InvalidSourceTransition),
    MissingStagingSource(StagingId),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSourceTransition(error) => error.fmt(formatter),
            Self::MissingStagingSource(id) => {
                write!(
                    formatter,
                    "finalization references unknown staging source {}",
                    id.0
                )
            }
        }
    }
}

impl Error for SessionError {}

impl From<InvalidSourceTransition> for SessionError {
    fn from(error: InvalidSourceTransition) -> Self {
        Self::InvalidSourceTransition(error)
    }
}

/// Per-session actor core. It is the serialized owner required by DESIGN.md §1.3 and composes
/// terminal facts with lifecycle, transcript, detection, scheduling, and viewport policy.
pub struct DualPlaneSession {
    terminal: TerminalAdapter,
    transcript: TranscriptStore,
    document: HistoryDocument,
    decorations: BTreeMap<TranscriptId, DecorationRecord>,
    scheduler: WorkerScheduler,
    resize_epoch: ResizeEpoch,
    staging_sources: BTreeMap<StagingId, SourceLifecycle>,
    active_staging_tail: Option<StagingId>,
    detection_revision: DetectionRevision,
    layout_key: LayoutKey,
    view_generation: ViewGeneration,
    grid_generation: GridGeneration,
    stale_results: usize,
    primary_parked: bool,
    cell_height_subpixels: NonZeroI64,
}

impl DualPlaneSession {
    pub fn new(columns: NonZeroU32, rows: NonZeroU32) -> Self {
        Self::with_cell_height(columns, rows, SPIKE_CELL_HEIGHT_SUBPIXELS)
    }

    /// Compatibility constructor for logic callers that still use the spike frozen-line limit.
    /// Product callers should use `with_quotas_and_cell_height` and own both limits explicitly.
    pub fn with_cell_height(
        columns: NonZeroU32,
        rows: NonZeroU32,
        cell_height_subpixels: NonZeroI64,
    ) -> Self {
        Self::with_quotas_and_cell_height(
            columns,
            rows,
            DEFAULT_STAGING_QUOTA,
            SPIKE_DEFAULT_FROZEN_QUOTA,
            cell_height_subpixels,
        )
    }

    pub fn with_frozen_quota(
        columns: NonZeroU32,
        rows: NonZeroU32,
        frozen_quota: NonZeroUsize,
    ) -> Self {
        Self::with_quotas_and_cell_height(
            columns,
            rows,
            DEFAULT_STAGING_QUOTA,
            frozen_quota,
            SPIKE_CELL_HEIGHT_SUBPIXELS,
        )
    }

    pub fn with_quotas(
        columns: NonZeroU32,
        rows: NonZeroU32,
        staging_quota: NonZeroUsize,
        frozen_quota: NonZeroUsize,
    ) -> Self {
        Self::with_quotas_and_cell_height(
            columns,
            rows,
            staging_quota,
            frozen_quota,
            SPIKE_CELL_HEIGHT_SUBPIXELS,
        )
    }

    /// Product construction path with explicit transcript limits and measured cell height.
    pub fn with_quotas_and_cell_height(
        columns: NonZeroU32,
        rows: NonZeroU32,
        staging_quota: NonZeroUsize,
        frozen_quota: NonZeroUsize,
        cell_height_subpixels: NonZeroI64,
    ) -> Self {
        Self {
            terminal: TerminalAdapter::new(columns, rows),
            transcript: TranscriptStore::with_quotas(staging_quota, frozen_quota),
            document: HistoryDocument::default(),
            decorations: BTreeMap::new(),
            scheduler: WorkerScheduler::default(),
            resize_epoch: ResizeEpoch::default(),
            staging_sources: BTreeMap::new(),
            active_staging_tail: None,
            detection_revision: DetectionRevision(1),
            layout_key: LayoutKey {
                width_cells: columns,
                dpi_milli: NonZeroU32::new(1000).unwrap(),
                font_rev: 1,
                theme_rev: 1,
            },
            view_generation: ViewGeneration(1),
            grid_generation: GridGeneration(1),
            stale_results: 0,
            primary_parked: false,
            cell_height_subpixels,
        }
    }

    pub fn terminal(&self) -> &TerminalAdapter {
        &self.terminal
    }

    pub fn application_cursor_mode(&self) -> bool {
        self.terminal.application_cursor_mode()
    }

    pub fn bracketed_paste_mode(&self) -> bool {
        self.terminal.bracketed_paste_mode()
    }

    /// Protocol replies are returned to the owning app, which is the only PTY writer.
    pub fn take_pty_writes(&self) -> Vec<Vec<u8>> {
        self.terminal.take_pty_writes()
    }

    pub fn document(&self) -> &HistoryDocument {
        &self.document
    }

    pub fn transcript(&self) -> &TranscriptStore {
        &self.transcript
    }

    pub fn decoration(&self, id: TranscriptId) -> Option<&DecorationRecord> {
        self.decorations.get(&id)
    }

    pub fn pending_tasks(&self) -> usize {
        self.scheduler.pending_len()
    }

    pub fn stale_results(&self) -> usize {
        self.stale_results
    }

    pub fn retry_on_idle(&self) -> usize {
        self.scheduler.retry_len()
    }

    pub fn grid_generation(&self) -> GridGeneration {
        self.grid_generation
    }

    pub fn layout_key(&self) -> LayoutKey {
        self.layout_key
    }

    pub fn set_cell_height_subpixels(&mut self, cell_height_subpixels: NonZeroI64) {
        self.cell_height_subpixels = cell_height_subpixels;
    }

    pub fn register_live_anchor(
        &mut self,
        screen: ScreenId,
        point: GridPoint,
        bias: Bias,
    ) -> AnchorId {
        self.document.register_anchor(ContentAnchor::Live {
            screen,
            point,
            bias,
            generation: self.grid_generation,
        })
    }

    pub fn anchor(&self, id: AnchorId) -> Result<&ContentAnchor, AnchorError> {
        self.document.anchor(id)
    }

    /// The actor quantum is observable here; parser calls receive whole slices, never bytes.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), SessionError> {
        for chunk in bytes.chunks(PARSE_QUANTUM) {
            let events = self.terminal.feed(chunk);
            self.apply_events(events)?;
            self.sync_staging_tail();
        }
        Ok(())
    }

    pub fn resize(&mut self, columns: NonZeroU32, rows: NonZeroU32) -> Result<(), SessionError> {
        let plan = plan_resize(self.terminal.dimensions(), (columns, rows));
        if plan.begin_cooldown {
            self.resize_epoch.changed();
            self.grid_generation.0 += 1;
        }
        if plan.finalize_staging {
            let finalized = self
                .transcript
                .finalize_all_candidates(FinalizeReason::WidthResize);
            let evicted = self.transcript.take_evictions();
            for line in finalized {
                self.ingest_finalized(line)?;
            }
            self.delete_history(&evicted, false);
        }
        let events = self.terminal.resize(columns, rows);
        self.apply_events(events)?;
        // A grow or width-only resize can change the generation without removing rows.
        self.document
            .capture_rows_transaction(&[], self.grid_generation);
        let next_layout = LayoutKey {
            width_cells: columns,
            ..self.layout_key
        };
        self.set_layout_key(next_layout);
        Ok(())
    }

    pub fn set_layout_key(&mut self, layout_key: LayoutKey) {
        if layout_key != self.layout_key {
            self.layout_key = layout_key;
            self.invalidate_layout();
        }
    }

    pub fn mark_resize_quiescent(&mut self) {
        self.resize_epoch.mark_quiescent();
        self.schedule_existing_artifacts();
    }

    pub fn run_workers(&mut self) {
        loop {
            while let Some(task) = self.take_worker_task() {
                self.complete_worker_task(task);
            }
            if !self.scheduler.has_retry() {
                break;
            }
            self.schedule_existing_artifacts();
            if self.scheduler.pending_len() == 0 {
                break;
            }
        }
    }

    pub fn take_worker_task(&mut self) -> Option<DetectionTask> {
        self.scheduler.take()
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
            if let Some(record) = self.decorations.get_mut(&id)
                && !self.primary_parked
                && self.resize_epoch.decorations_allowed()
                && let Some(task) = record.schedule(id, span)
            {
                self.enqueue_task(task);
            }
        }
    }

    pub fn new_projection(&self, layout_key: LayoutKey) -> ViewportProjection {
        let mut projection = ViewportProjection::new(
            layout_key,
            self.detection_revision,
            self.terminal.dimensions().1,
            self.cell_height_subpixels,
            self.transcript.source_generation(),
            self.grid_generation,
        );
        self.refresh_projection(&mut projection);
        projection
    }

    pub fn viewport_frame(
        &self,
        projection: &ViewportProjection,
    ) -> Result<ViewportFrame, FrameProjectionError> {
        let (columns, rows) = self.terminal.dimensions();
        let visible_rows = (0..rows.get())
            .filter_map(|row| self.terminal.visible_row(row))
            .collect::<Vec<_>>();
        let cursor = self.terminal.cursor();
        projection.live_frame(
            columns,
            visible_rows,
            GridCursor {
                row: cursor.row,
                column: cursor.column,
                visible: cursor.visible,
            },
        )
    }

    pub fn refresh_projection(&self, projection: &mut ViewportProjection) {
        projection.set_live_state(
            self.terminal.dimensions().1,
            self.transcript.source_generation(),
            self.grid_generation,
        );
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

    fn apply_events(&mut self, events: Vec<AdapterEvent>) -> Result<(), SessionError> {
        for event in events {
            match classify(event) {
                LifecycleDirective::RowsRemoved(rows) => self.apply_removed_rows(rows)?,
                LifecycleDirective::ClearHistoryAndStaging => {
                    let removed = self.transcript.clear_history();
                    self.delete_history(&removed, true);
                }
                LifecycleDirective::InvalidateStaging => {
                    self.transcript.invalidate_staging();
                    self.staging_sources.clear();
                    self.active_staging_tail = None;
                    self.document
                        .delete_transaction(&[], true, self.grid_generation);
                }
                LifecycleDirective::ParkPrimary => {
                    self.primary_parked = true;
                    self.bump_view_generation();
                }
                LifecycleDirective::RestorePrimary => {
                    self.primary_parked = false;
                    self.grid_generation.0 += 1;
                    self.document
                        .capture_rows_transaction(&[], self.grid_generation);
                    self.bump_view_generation();
                }
            }
        }
        Ok(())
    }

    fn apply_removed_rows(&mut self, rows: Vec<RowDirective>) -> Result<(), SessionError> {
        let mut removals = Vec::new();
        let mut captured = Vec::<CaptureResult>::new();
        for row in rows {
            match row {
                RowDirective::Capture { live_row, row } => {
                    let result = self.transcript.capture(row);
                    let generation = self.transcript.source_generation();
                    removals.push(LiveRowRemoval {
                        row: live_row,
                        staging: Some((result.staging_id, generation)),
                    });
                    captured.push(result);
                }
                RowDirective::DiscardFromTop { live_row } => removals.push(LiveRowRemoval {
                    row: live_row,
                    staging: None,
                }),
                RowDirective::Ignore => {}
            }
        }
        if removals.is_empty() {
            return Ok(());
        }

        self.grid_generation.0 += 1;
        self.document
            .capture_rows_transaction(&removals, self.grid_generation);
        for result in captured {
            self.staging_sources
                .insert(result.staging_id, SourceLifecycle::Live);
            self.active_staging_tail = result.finalized.is_empty().then_some(result.staging_id);
            for finalized in result.finalized {
                self.ingest_finalized(finalized)?;
            }
        }
        let evicted = self.transcript.take_evictions();
        self.delete_history(&evicted, false);
        Ok(())
    }

    fn ingest_finalized(&mut self, finalized: FinalizedLine) -> Result<(), SessionError> {
        let closes_active = self.active_staging_tail.is_some_and(|active| {
            finalized
                .mappings
                .iter()
                .any(|mapping| mapping.staging_id == active)
        });
        for mapping in &finalized.mappings {
            let source = self
                .staging_sources
                .get_mut(&mapping.staging_id)
                .ok_or(SessionError::MissingStagingSource(mapping.staging_id))?;
            source.transition(SourceLifecycle::Frozen)?;
        }
        let id = finalized.line.id;
        self.document.finalize_transaction(finalized);
        self.schedule_detection(id);
        self.staging_sources
            .retain(|_, source| *source != SourceLifecycle::Frozen);
        if closes_active {
            self.active_staging_tail = None;
        }
        Ok(())
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
            if !self.primary_parked && self.resize_epoch.decorations_allowed() {
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
        self.scheduler.remove_sources(&removed_set);
        for id in removed {
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
        if self.primary_parked || !self.resize_epoch.decorations_allowed() {
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
        let transcript_id = task.transcript_id;
        if self.scheduler.enqueue(task) == EnqueueOutcome::RetryOnIdle
            && let Some(record) = self.decorations.get_mut(&transcript_id)
        {
            record.decoration = bt_doc::DecorationLifecycle::None;
        }
    }

    fn sync_staging_tail(&mut self) {
        let Some(id) = self.active_staging_tail else {
            return;
        };
        let Some(row) = self.terminal.visible_row(0) else {
            return;
        };
        if !self.transcript.rewrite_staged(id, row) {
            self.active_staging_tail = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_doc::DecorationLifecycle;
    use bt_transcript::TerminalColor;
    use proptest::prelude::*;

    fn nz(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    #[test]
    fn worker_queue_is_bounded_and_retries_on_idle() {
        let mut bytes = Vec::new();
        for index in 0..70 {
            bytes.extend_from_slice(format!("$$x{index}$$\r\n").as_bytes());
        }
        bytes.extend_from_slice(b"tail");
        let mut session = DualPlaneSession::new(nz(32), nz(2));
        session.feed(&bytes).unwrap();
        assert_eq!(session.pending_tasks(), crate::WORKER_QUEUE_CAP);
        assert!(session.retry_on_idle() > 0);
        session.run_workers();
        assert_eq!(session.pending_tasks(), 0);
        assert_eq!(session.retry_on_idle(), 0);
        assert!(session.document().entries().iter().all(|(id, _)| {
            session.decoration(*id).unwrap().decoration == DecorationLifecycle::Ready
        }));
    }

    #[test]
    fn byte_driven_terminal_state_projects_through_the_viewport_frame_boundary() {
        let mut session = DualPlaneSession::new(nz(4), nz(2));
        session.feed(b"\x1b[31mA").unwrap();
        let projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&projection).unwrap();

        assert_eq!((frame.columns.get(), frame.rows.get()), (4, 2));
        assert_eq!(frame.cells.len(), 8);
        assert_eq!(frame.cells[0].text, "A");
        assert_eq!(frame.cells[0].style.foreground, TerminalColor::Named(1));
        assert_eq!((frame.cursor.row, frame.cursor.column), (0, 1));
    }

    #[test]
    fn byte_driven_prompt_cursor_is_the_cell_after_typed_text_and_ignores_prediction() {
        let mut session = DualPlaneSession::new(nz(32), nz(2));
        session.feed(b"PS> carg").unwrap();
        let projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&projection).unwrap();
        let typed_end = frame.cells[..32]
            .iter()
            .rposition(|cell| !cell.text.chars().all(char::is_whitespace))
            .unwrap() as u32;

        assert_eq!(frame.cursor.column, typed_end + 1);
        assert_eq!(frame.cursor.column, 8);

        // PSReadLine paints inline prediction after saving the input cursor, then restores it.
        // Prediction cells remain visible but must not participate in the cursor column.
        session.feed(b"o\x1b7 --version\x1b8").unwrap();
        let projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&projection).unwrap();
        assert_eq!(frame.cursor.column, "PS> cargo".len() as u32);
        assert!(
            frame.cells[..32]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("cargo --version")
        );
    }

    proptest! {
        #[test]
        fn parser_is_invariant_under_random_chunk_boundaries(cuts in prop::collection::vec(1usize..32, 1..32)) {
            let bytes = b"one\r\n$$x$$\r\nthree\r\nfour\x1b[3;1H!";
            let mut whole = DualPlaneSession::new(nz(16), nz(3));
            whole.feed(bytes).unwrap();

            let mut chunked = DualPlaneSession::new(nz(16), nz(3));
            let mut offset = 0;
            for size in cuts {
                if offset == bytes.len() { break; }
                let end = (offset + size).min(bytes.len());
                chunked.feed(&bytes[offset..end]).unwrap();
                offset = end;
            }
            if offset < bytes.len() {
                chunked.feed(&bytes[offset..]).unwrap();
            }

            prop_assert_eq!(chunked.terminal().visible_text(), whole.terminal().visible_text());
            prop_assert_eq!(
                chunked.document().entries().values().map(|entry| entry.line.text.clone()).collect::<Vec<_>>(),
                whole.document().entries().values().map(|entry| entry.line.text.clone()).collect::<Vec<_>>()
            );
        }
    }
}
