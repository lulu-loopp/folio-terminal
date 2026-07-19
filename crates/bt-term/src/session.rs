use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    hash::{DefaultHasher, Hash, Hasher},
    num::{NonZeroI64, NonZeroU32, NonZeroUsize},
    sync::Arc,
    time::Instant,
};

use bt_detect::{
    DecorationRecord, DetectionInput, DetectionTask, PlaceholderArtifact, resolve_detection_task,
};
use bt_doc::{
    AnchorError, AnchorId, Bias, ContentAnchor, DecorationIntent, DecorationLifecycle,
    DetectionRevision, GridGeneration, GridPoint, HistoryDocument, InvalidSourceTransition,
    LayoutKey, LiveRowRemoval, SUBPIXELS_PER_PX, ScreenId, SourceLifecycle, VersionStamp,
    ViewGeneration, compare_anchors,
};
use bt_math::{MathEngine, MathRaster, MathRenderError, MathRenderKey};
use bt_transcript::{
    CaptureResult, CapturedRow, DEFAULT_STAGING_QUOTA, FinalizedLine, GraphemeOffset,
    SPIKE_DEFAULT_FROZEN_QUOTA, SourceGeneration, StagedRow, StagingId, TranscriptId,
    TranscriptStore,
};
use bt_viewport::{
    FrameProjectionError, FrameViewportOrigin, GridCursor, ProjectedMathArtifact, ViewSelection,
    ViewportFrame, ViewportProjection,
};

use crate::{
    adapter::{AdapterEvent, TerminalAdapter, TerminalModes},
    lifecycle::{LifecycleDirective, RowDirective, classify, plan_resize},
    scheduling::{EnqueueOutcome, PARSE_QUANTUM, ResizeEpoch, WorkerScheduler},
};

pub const SPIKE_CELL_HEIGHT_SUBPIXELS: NonZeroI64 = NonZeroI64::new(18 * SUBPIXELS_PER_PX).unwrap();

#[derive(Debug)]
pub enum SessionError {
    InvalidSourceTransition(InvalidSourceTransition),
    MissingStagingSource(StagingId),
    ResizeCandidateMismatch { vendor: usize, staging: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResizeTraceRowOrigin {
    NormalScroll,
    Resize,
    DeleteLines,
    VendorHarvest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResizeTraceKind {
    TransactionBegin {
        columns: u32,
        rows: u32,
    },
    LocalResizeRequest {
        columns: u32,
        rows: u32,
    },
    PtyResizeRequest {
        columns: u32,
        rows: u32,
    },
    VendorReconcile {
        history_before: usize,
        history_after: usize,
        cursor_row: u32,
        cursor_column: u32,
        cursor_visible: bool,
    },
    VendorRestore {
        rows: usize,
    },
    PtyChunkArrived {
        bytes: usize,
    },
    AdapterRows {
        origin: ResizeTraceRowOrigin,
        widths: Vec<usize>,
    },
    VendorTail {
        rows: usize,
    },
    Harvest {
        origin: ResizeTraceRowOrigin,
        widths: Vec<usize>,
        continues: Vec<bool>,
    },
    FramePublished {
        columns: u32,
        rows: u32,
        layout_columns: u32,
        cells: usize,
        anchors: usize,
        scroll_offset_rows: usize,
        anchored: bool,
        cursor_row: u32,
        cursor_column: u32,
        cursor_visible: bool,
    },
    TransactionEnd,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResizeTraceEvent {
    pub transaction: u64,
    pub ordinal: u64,
    pub elapsed_micros: u64,
    pub kind: ResizeTraceKind,
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
            Self::ResizeCandidateMismatch { vendor, staging } => write!(
                formatter,
                "resize reverse-harvest mismatch: vendor has {vendor} rows, staging has {staging}"
            ),
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
    resize_trace_transaction: u64,
    resize_trace_started: Option<Instant>,
    resize_trace_next_ordinal: u64,
    resize_trace_post_end_frames_remaining: Option<u8>,
    resize_trace: Vec<ResizeTraceEvent>,
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
            resize_trace_transaction: 0,
            resize_trace_started: None,
            resize_trace_next_ordinal: 0,
            resize_trace_post_end_frames_remaining: None,
            resize_trace: Vec::new(),
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

    pub fn terminal_modes(&self) -> TerminalModes {
        self.terminal.modes()
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
        self.feed_at(bytes, Instant::now())
    }

    /// Deterministic replay entry point. Production callers normally use `feed`; integration tests
    /// can supply a monotonic timestamp without sleeping through the resize silence window.
    pub fn feed_at(&mut self, bytes: &[u8], observed_at: Instant) -> Result<(), SessionError> {
        // Frozen history is immutable. Staging/live selections are conservatively invalidated by
        // output because the parser may rewrite a selected row without emitting a removal fact.
        if !bytes.is_empty() && self.selection_touches_mutable_source() {
            self.document.clear_selection();
        }
        if !bytes.is_empty() {
            self.resize_epoch.observe_output(observed_at);
            if self.resize_epoch.is_active() {
                self.trace_resize_event(
                    observed_at,
                    ResizeTraceKind::PtyChunkArrived { bytes: bytes.len() },
                );
            }
        }
        for chunk in bytes.chunks(PARSE_QUANTUM) {
            let events = self.terminal.feed(chunk);
            self.apply_events(events, observed_at)?;
            self.sync_staging_tail();
        }
        Ok(())
    }

    fn selection_touches_mutable_source(&self) -> bool {
        self.view_selection().is_some_and(|selection| {
            !matches!(selection.start, ContentAnchor::History { .. })
                || !matches!(selection.end, ContentAnchor::History { .. })
        })
    }

    pub fn resize(&mut self, columns: NonZeroU32, rows: NonZeroU32) -> Result<(), SessionError> {
        self.resize_at(columns, rows, Instant::now())
    }

    /// Deterministic replay counterpart to `resize`.
    pub fn resize_at(
        &mut self,
        columns: NonZeroU32,
        rows: NonZeroU32,
        observed_at: Instant,
    ) -> Result<(), SessionError> {
        let plan = plan_resize(self.terminal.dimensions(), (columns, rows));
        if plan.begin_transaction {
            self.begin_resize_transaction(observed_at)?;
            self.resize_epoch.changed(observed_at);
            self.grid_generation.0 += 1;
            self.trace_resize_event(
                observed_at,
                ResizeTraceKind::LocalResizeRequest {
                    columns: columns.get(),
                    rows: rows.get(),
                },
            );
        }
        let events = self.terminal.resize(columns, rows);
        self.apply_events(events, observed_at)?;
        if plan.begin_transaction {
            self.trace_resize_event(
                observed_at,
                ResizeTraceKind::VendorTail {
                    rows: self.terminal.resize_transaction_history_size(),
                },
            );
        }
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

    pub fn resize_finish_deadline(&self) -> Option<Instant> {
        self.resize_epoch.quiescence_deadline()
    }

    pub fn resize_request_deadline(&self) -> Option<Instant> {
        self.resize_epoch.request_deadline()
    }

    pub fn synchronized_update_deadline(&self) -> Option<Instant> {
        self.terminal.synchronized_update_deadline()
    }

    /// Commit a DEC 2026 update when its parser-owned timeout expires without ESU.
    pub fn finish_synchronized_update(
        &mut self,
        observed_at: Instant,
    ) -> Result<bool, SessionError> {
        if self.synchronized_update_deadline().is_none() {
            return Ok(false);
        }
        let events = self.terminal.finish_synchronized_update();
        self.apply_events(events, observed_at)?;
        self.sync_staging_tail();
        Ok(true)
    }

    pub fn mark_pty_resize_requested_at(
        &mut self,
        columns: NonZeroU32,
        rows: NonZeroU32,
        observed_at: Instant,
    ) -> bool {
        let reconciled = self.resize_epoch.is_active();
        self.resize_epoch.final_request_sent(observed_at);
        self.trace_resize_event(
            observed_at,
            ResizeTraceKind::PtyResizeRequest {
                columns: columns.get(),
                rows: rows.get(),
            },
        );
        let (history_before, history_after) =
            self.terminal.reconcile_resize_transaction_to_viewport();
        if reconciled {
            self.grid_generation.0 += 1;
            self.document
                .capture_rows_transaction(&[], self.grid_generation);
        }
        let cursor = self.terminal.cursor();
        self.trace_resize_event(
            observed_at,
            ResizeTraceKind::VendorReconcile {
                history_before,
                history_after,
                cursor_row: cursor.row,
                cursor_column: cursor.column,
                cursor_visible: cursor.visible,
            },
        );
        reconciled
    }

    /// Finish only after both resize and output have been silent for their configured intervals.
    /// Returns true when this call closed a resize transaction.
    pub fn finish_resize_if_quiescent(
        &mut self,
        observed_at: Instant,
    ) -> Result<bool, SessionError> {
        if !self.resize_epoch.is_quiescent_at(observed_at) {
            return Ok(false);
        }
        self.harvest_resize_transaction(observed_at)?;
        self.trace_resize_event(observed_at, ResizeTraceKind::TransactionEnd);
        // Retain a short post-transaction window so a real wheel-up oracle is represented without
        // tracing every unrelated frame for the remainder of the session.
        self.resize_trace_post_end_frames_remaining = Some(16);
        self.resize_epoch.mark_quiescent();
        self.schedule_existing_artifacts();
        Ok(true)
    }

    pub fn resize_trace(&self) -> &[ResizeTraceEvent] {
        &self.resize_trace
    }

    pub fn resize_trace_transaction(&self) -> u64 {
        self.resize_trace_transaction
    }

    pub fn record_published_frame(&mut self, frame: &ViewportFrame, observed_at: Instant) {
        self.trace_resize_event(
            observed_at,
            ResizeTraceKind::FramePublished {
                columns: frame.columns.get(),
                rows: frame.rows.get(),
                layout_columns: frame.layout_key.width_cells.get(),
                cells: frame.cells.len(),
                anchors: frame.cell_anchors.len(),
                scroll_offset_rows: frame.scroll_offset_rows,
                anchored: matches!(&frame.viewport_origin, FrameViewportOrigin::Anchored(_)),
                cursor_row: frame.cursor.row,
                cursor_column: frame.cursor.column,
                cursor_visible: frame.cursor.visible,
            },
        );
        if let Some(remaining) = self.resize_trace_post_end_frames_remaining.as_mut() {
            *remaining = remaining.saturating_sub(1);
            if *remaining == 0 {
                self.resize_trace_started = None;
            }
        }
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
        if !self.worker_task_is_current(&task) {
            self.stale_results += 1;
            return false;
        }
        let mut task = task;
        if !resolve_detection_task(&mut task) {
            return self.complete_worker_result(task, Err(MathRenderError::NotDetected));
        }
        let placeholder = bt_detect::render_placeholder(&task);
        let accepted = self.apply_worker_completion(task, Some(placeholder));
        if !accepted {
            self.stale_results += 1;
        }
        accepted
    }

    pub fn complete_worker_result(
        &mut self,
        task: DetectionTask,
        result: Result<MathRaster, MathRenderError>,
    ) -> bool {
        let render_error = result.as_ref().err().map(ToString::to_string);
        let render_time = result.as_ref().ok().map(|raster| raster.render_time);
        let artifact = result
            .ok()
            .map(|raster| artifact_from_raster(&task, raster));
        let accepted = self.apply_worker_completion(task.clone(), artifact);
        if !accepted {
            self.stale_results += 1;
        } else if std::env::var_os("BT_PERF_TRACE").is_some() {
            if let Some(elapsed) = render_time {
                eprintln!(
                    "BT_PERF_TRACE math_render_us={} source={} resident_bytes={}",
                    elapsed.as_micros(),
                    task.transcript_id.0,
                    self.math_resident_bytes(),
                );
            } else if let Some(error) = render_error {
                eprintln!(
                    "BT_PERF_TRACE math_render_failed source={} error={error:?}",
                    task.transcript_id.0,
                );
            }
        }
        accepted
    }

    fn apply_worker_completion(
        &mut self,
        task: DetectionTask,
        artifact: Option<PlaceholderArtifact>,
    ) -> bool {
        if !self.worker_task_is_current(&task) {
            return false;
        }
        if !task.resolved {
            return self
                .decorations
                .get_mut(&task.candidate_id)
                .is_some_and(|record| record.fail(&task));
        }
        let block_is_current = task.inputs.iter().all(|input| {
            self.document
                .entries()
                .get(&input.id)
                .is_some_and(|entry| entry.line.text == input.text)
        });
        if !block_is_current {
            return false;
        }
        if task.candidate_id != task.transcript_id
            && let Some(candidate) = self.decorations.get_mut(&task.candidate_id)
        {
            candidate.decoration = DecorationLifecycle::None;
            candidate.artifact = None;
        }
        let Some(record) = self.decorations.get_mut(&task.transcript_id) else {
            return false;
        };
        if record.source != SourceLifecycle::Frozen
            || record.versions.detection != task.versions.detection
            || record.versions.layout != task.versions.layout
            || record.versions.view != task.versions.view
        {
            return false;
        }
        record.decoration = DecorationLifecycle::None;
        record.artifact = None;
        let Some(resolved_task) =
            record.schedule(task.transcript_id, task.block_end, task.span.clone())
        else {
            return false;
        };
        match artifact {
            Some(artifact) => {
                self.document.set_decoration(
                    task.transcript_id,
                    DecorationIntent::Math {
                        byte_start: task.span.byte_start,
                        byte_end: task.span.byte_end,
                        detection_revision: task.versions.detection,
                    },
                );
                record.complete(&resolved_task, artifact)
            }
            None => {
                self.document
                    .set_decoration(task.transcript_id, DecorationIntent::Plain);
                record.fail(&resolved_task)
            }
        }
    }

    fn worker_task_is_current(&self, task: &DetectionTask) -> bool {
        self.decorations
            .get(&task.candidate_id)
            .is_some_and(|record| {
                record.source == SourceLifecycle::Frozen
                    && record.decoration == bt_doc::DecorationLifecycle::Pending
                    && record.versions == task.versions
            })
    }

    pub fn math_resident_bytes(&self) -> usize {
        self.decorations
            .values()
            .filter_map(|record| record.artifact.as_ref())
            .map(|artifact| artifact.rgba.len())
            .sum()
    }

    pub fn redetect(&mut self, revision: DetectionRevision) {
        if revision == self.detection_revision {
            return;
        }
        self.detection_revision = revision;
        for record in self.decorations.values_mut() {
            record.detector_changed(revision);
        }
        self.document.clear_decorations();
        self.schedule_existing_artifacts();
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
        self.sync_projection_state(&mut projection);
        projection
    }

    pub fn viewport_frame(
        &self,
        projection: &mut ViewportProjection,
    ) -> Result<ViewportFrame, FrameProjectionError> {
        let (_, rows) = self.terminal.dimensions();
        let visible_rows = (0..rows.get())
            .filter_map(|row| self.terminal.visible_row(row))
            .collect::<Vec<_>>();
        let cursor = self.terminal.cursor();
        let staged = self.transcript.staged_rows().cloned().collect::<Vec<_>>();
        let terminal_modes = self.terminal.modes();
        projection.continuous_frame(
            &self.document,
            &staged,
            visible_rows,
            GridCursor {
                row: cursor.row,
                column: cursor.column,
                visible: cursor.visible,
            },
            if terminal_modes.alternate_screen {
                ScreenId::Alternate
            } else {
                ScreenId::Primary
            },
        )
    }

    pub fn set_view_selection(&mut self, selection: Option<ViewSelection>) {
        if let Some(selection) = selection {
            self.document
                .replace_selection(selection.start, selection.end);
        } else {
            self.document.clear_selection();
        }
    }

    pub fn view_selection(&self) -> Option<ViewSelection> {
        let selection = self.document.selection()?;
        Some(ViewSelection {
            start: self.document.anchor(selection.start).ok()?.clone(),
            end: self.document.anchor(selection.end).ok()?.clone(),
        })
    }

    /// Extract the current semantic selection. Viewport-only reflow is intentionally absent from
    /// this walk: soft physical wraps concatenate, while hard logical boundaries become CRLF.
    /// Spaces and tabs at every copied hard-line end (and the final end) are trimmed.
    pub fn selection_text(&self) -> Option<String> {
        let selection = self.view_selection()?;
        let (start, end) = ordered_selection(&selection)?;
        let screen = if self.terminal.modes().alternate_screen {
            ScreenId::Alternate
        } else {
            ScreenId::Primary
        };
        let mut rows = Vec::new();
        if screen == ScreenId::Primary {
            rows.extend(
                self.document
                    .entries()
                    .values()
                    .map(|entry| copy_row_from_history(&entry.line)),
            );
            rows.extend(
                self.transcript
                    .staged_rows()
                    .map(|row| copy_row_from_staging(row, self.transcript.source_generation())),
            );
        }
        let (_, live_rows) = self.terminal.dimensions();
        rows.extend((0..live_rows.get()).filter_map(|row| {
            self.terminal
                .visible_row(row)
                .map(|cells| copy_row_from_live(&cells, row, screen, self.grid_generation))
        }));

        let mut output = String::new();
        let mut copied_any_row = false;
        let mut hard_break_pending = false;
        for row in rows {
            let overlaps = selection_overlaps(&row.start, &row.end, start, end);
            if !overlaps {
                continue;
            }
            if copied_any_row && hard_break_pending {
                trim_copy_line_end(&mut output);
                output.push_str("\r\n");
            }
            for cell in row.cells {
                if selection_overlaps(&cell.start, &cell.end, start, end) {
                    output.push_str(&cell.text);
                }
            }
            copied_any_row = true;
            hard_break_pending = row.hard_break_after;
        }
        if !copied_any_row {
            return None;
        }
        trim_copy_line_end(&mut output);
        Some(output)
    }

    pub fn refresh_projection(&self, projection: &mut ViewportProjection) {
        projection.relayout(self.layout_key, &self.document);
        self.sync_projection_state(projection);
    }

    fn sync_projection_state(&self, projection: &mut ViewportProjection) {
        projection.set_live_state(
            self.terminal.dimensions().1,
            self.transcript.source_generation(),
            self.grid_generation,
        );
        projection.set_selection(self.view_selection());
        projection.sync_math_artifacts(self.decorations.iter().filter_map(|(id, record)| {
            record.artifact.as_ref().map(|artifact| {
                (
                    *id,
                    ProjectedMathArtifact {
                        key: artifact.key.clone(),
                        end: artifact.block_end,
                        rgba: Arc::clone(&artifact.rgba),
                        width_px: artifact.width_px,
                        height_px: artifact.height_px,
                        height_subpixels: artifact.height_subpixels,
                    },
                )
            })
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

    fn begin_resize_transaction(&mut self, observed_at: Instant) -> Result<(), SessionError> {
        if self.resize_epoch.is_active() {
            return Ok(());
        }

        // A transaction starts with a clean ownership boundary. The sole unfinished resize
        // candidate returns to native history; every other pre-existing staging source freezes.
        // Already-finalized history never enters this reverse-harvest path.
        self.document.clear_selection();
        let vendor_candidate_rows = self.terminal.resize_staging_candidate_rows();
        let staging_candidate_rows = self.transcript.unclosed_candidate_len();
        if vendor_candidate_rows != 0 && vendor_candidate_rows != staging_candidate_rows {
            return Err(SessionError::ResizeCandidateMismatch {
                vendor: vendor_candidate_rows,
                staging: staging_candidate_rows,
            });
        }
        if vendor_candidate_rows != 0 {
            for staged in self.transcript.take_unclosed_candidate() {
                self.staging_sources.remove(&staged.id);
            }
            self.active_staging_tail = None;
        }
        let finalized = self.transcript.finalize_all_candidates();
        let evicted = self.transcript.take_evictions();
        for line in finalized {
            self.ingest_finalized(line)?;
        }
        self.delete_history(&evicted, false);

        self.resize_trace.clear();
        self.resize_trace_transaction = self.resize_trace_transaction.wrapping_add(1);
        self.resize_trace_started = Some(observed_at);
        self.resize_trace_next_ordinal = 0;
        self.resize_trace_post_end_frames_remaining = None;
        let (columns, rows) = self.terminal.dimensions();
        self.trace_resize_event(
            observed_at,
            ResizeTraceKind::TransactionBegin {
                columns: columns.get(),
                rows: rows.get(),
            },
        );
        let restored = self.terminal.begin_resize_transaction();
        debug_assert_eq!(restored, vendor_candidate_rows);
        self.trace_resize_event(
            observed_at,
            ResizeTraceKind::VendorRestore { rows: restored },
        );
        Ok(())
    }

    fn trace_resize_event(&mut self, observed_at: Instant, kind: ResizeTraceKind) {
        let Some(started) = self.resize_trace_started else {
            return;
        };
        let elapsed_micros = observed_at
            .saturating_duration_since(started)
            .as_micros()
            .min(u128::from(u64::MAX)) as u64;
        self.resize_trace.push(ResizeTraceEvent {
            transaction: self.resize_trace_transaction,
            ordinal: self.resize_trace_next_ordinal,
            elapsed_micros,
            kind,
        });
        self.resize_trace_next_ordinal = self.resize_trace_next_ordinal.wrapping_add(1);
    }

    fn trace_adapter_event(&mut self, event: &AdapterEvent, observed_at: Instant) {
        if !self.resize_epoch.is_active() {
            return;
        }
        let AdapterEvent::RowsRemoved { context, rows } = event else {
            return;
        };
        let origin = match context.cause {
            crate::adapter::RemovalCause::NormalScroll => ResizeTraceRowOrigin::NormalScroll,
            crate::adapter::RemovalCause::Resize => ResizeTraceRowOrigin::Resize,
            crate::adapter::RemovalCause::DeleteLines => ResizeTraceRowOrigin::DeleteLines,
        };
        self.trace_resize_event(
            observed_at,
            ResizeTraceKind::AdapterRows {
                origin,
                widths: rows.iter().map(|row| row.row.cells.len()).collect(),
            },
        );
    }

    fn apply_events(
        &mut self,
        events: Vec<AdapterEvent>,
        observed_at: Instant,
    ) -> Result<(), SessionError> {
        for event in events {
            self.trace_adapter_event(&event, observed_at);
            match classify(event) {
                LifecycleDirective::RowsRemoved { rows } => {
                    if self.resize_epoch.is_active() {
                        self.rebase_vendor_owned_rows(rows);
                    } else {
                        self.apply_removed_rows(rows)?;
                    }
                }
                LifecycleDirective::ClearHistoryAndStaging => {
                    self.terminal.clear_resize_transaction_history();
                    let removed = self.transcript.clear_history();
                    self.delete_history(&removed, true);
                }
                LifecycleDirective::InvalidateStaging => {
                    self.terminal.clear_resize_transaction_history();
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

    fn rebase_vendor_owned_rows(&mut self, rows: Vec<RowDirective>) {
        let removals = rows
            .into_iter()
            .filter_map(|row| match row {
                RowDirective::Capture { live_row, .. }
                | RowDirective::DiscardFromTop { live_row } => Some(LiveRowRemoval {
                    row: live_row,
                    staging: None,
                    grapheme_offsets: Vec::new(),
                }),
                RowDirective::Ignore => None,
            })
            .collect::<Vec<_>>();
        if removals.is_empty() {
            return;
        }
        self.grid_generation.0 += 1;
        self.document
            .capture_rows_transaction(&removals, self.grid_generation);
    }

    fn harvest_resize_transaction(&mut self, observed_at: Instant) -> Result<(), SessionError> {
        let rows = self.terminal.finish_resize_transaction();
        self.trace_resize_event(
            observed_at,
            ResizeTraceKind::Harvest {
                origin: ResizeTraceRowOrigin::VendorHarvest,
                widths: rows.iter().map(|row| row.cells.len()).collect(),
                continues: rows.iter().map(|row| row.continues).collect(),
            },
        );
        for row in rows {
            // Rows returned by one `finish_resize_transaction` are an ordered snapshot of one
            // vendor-owned grid. Within that batch WRAPLINE is therefore a causal continuation
            // fact, so the normal capture path may reconstruct its logical line without guessing.
            // A closed batch finalizes below. The sole trailing candidate which still wraps into
            // live row zero remains staging and is the only row set eligible for reverse harvest.
            let result = self.transcript.capture(row);
            self.staging_sources
                .insert(result.staging_id, SourceLifecycle::Live);
            self.active_staging_tail = result.finalized.is_empty().then_some(result.staging_id);
            for finalized in result.finalized {
                self.ingest_finalized(finalized)?;
            }
        }
        let unclosed_candidate_rows = self.transcript.unclosed_candidate_len();
        self.terminal
            .retain_resize_staging_candidate_rows(unclosed_candidate_rows);
        if unclosed_candidate_rows == 0 {
            for finalized in self.transcript.finalize_all_candidates() {
                self.ingest_finalized(finalized)?;
            }
            self.active_staging_tail = None;
        }
        let evicted = self.transcript.take_evictions();
        self.delete_history(&evicted, false);
        Ok(())
    }

    fn apply_removed_rows(&mut self, rows: Vec<RowDirective>) -> Result<(), SessionError> {
        let mut removals = Vec::new();
        let mut captured = Vec::<CaptureResult>::new();
        for row in rows {
            match row {
                RowDirective::Capture { live_row, row } => {
                    let grapheme_offsets = captured_grapheme_offsets(&row);
                    let result = self.transcript.capture(row);
                    let generation = self.transcript.source_generation();
                    removals.push(LiveRowRemoval {
                        row: live_row,
                        staging: Some((result.staging_id, generation)),
                        grapheme_offsets,
                    });
                    captured.push(result);
                }
                RowDirective::DiscardFromTop { live_row } => {
                    removals.push(LiveRowRemoval {
                        row: live_row,
                        staging: None,
                        grapheme_offsets: Vec::new(),
                    });
                }
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
        let versions = VersionStamp {
            source: entry.line.source_generation,
            detection: self.detection_revision,
            layout: self.layout_key,
            view: self.view_generation,
        };
        self.decorations
            .insert(id, DecorationRecord::frozen(versions));
        // Ordinary frozen lines take only the allocation-free delimiter prefilter. A candidate
        // snapshots immutable source here; the worker owns fence/pairing/escape/size detection.
        if !entry.line.text.contains("$$") {
            return;
        }
        if !self.primary_parked && self.resize_epoch.decorations_allowed() {
            self.schedule_scan(id);
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
        let candidates = self
            .document
            .entries()
            .iter()
            .filter_map(|(id, entry)| entry.line.text.contains("$$").then_some(*id))
            .collect::<Vec<_>>();
        for id in candidates {
            self.schedule_scan(id);
        }
    }

    fn schedule_scan(&mut self, candidate_id: TranscriptId) {
        let inputs = self
            .document
            .entries()
            .iter()
            .map(|(id, entry)| DetectionInput {
                id: *id,
                text: entry.line.text.clone(),
            })
            .collect::<Vec<_>>();
        let Some(task) = self
            .decorations
            .get_mut(&candidate_id)
            .and_then(|record| record.schedule_scan(candidate_id, Arc::from(inputs)))
        else {
            return;
        };
        self.enqueue_task(task);
    }

    fn enqueue_task(&mut self, task: DetectionTask) {
        let transcript_id = task.candidate_id;
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

#[derive(Clone)]
struct CopyCell {
    start: ContentAnchor,
    end: ContentAnchor,
    text: String,
}

struct CopyRow {
    start: ContentAnchor,
    end: ContentAnchor,
    cells: Vec<CopyCell>,
    hard_break_after: bool,
}

fn captured_grapheme_offsets(row: &CapturedRow) -> Vec<GraphemeOffset> {
    let mut next = 0u32;
    let mut lead = 0u32;
    row.cells
        .iter()
        .map(|cell| {
            if !cell.wide_spacer {
                lead = next;
                next += u32::from(!cell.text.is_empty());
            }
            GraphemeOffset(lead)
        })
        .collect()
}

fn copy_row_from_history(line: &bt_transcript::FrozenLine) -> CopyRow {
    let anchor = |offset, bias| ContentAnchor::History {
        id: line.id,
        offset: GraphemeOffset(offset),
        bias,
        generation: line.source_generation,
    };
    let cells = line
        .grapheme_boundaries
        .windows(2)
        .enumerate()
        .map(|(index, bytes)| CopyCell {
            start: anchor(index as u32, Bias::Before),
            end: anchor(index as u32, Bias::After),
            text: line.text[bytes[0] as usize..bytes[1] as usize].to_owned(),
        })
        .collect();
    let end = line.grapheme_boundaries.len().saturating_sub(1) as u32;
    CopyRow {
        start: anchor(0, Bias::Before),
        end: anchor(end, Bias::After),
        cells,
        hard_break_after: true,
    }
}

fn copy_row_from_staging(row: &StagedRow, generation: SourceGeneration) -> CopyRow {
    let offsets = captured_grapheme_offsets(&row.row);
    let anchor = |offset: GraphemeOffset, bias| ContentAnchor::Staging {
        id: row.id,
        offset,
        bias,
        generation,
    };
    copy_row_from_cells(
        &row.row,
        |column, bias| anchor(offsets[column], bias),
        !row.row.continues,
    )
}

fn copy_row_from_live(
    row: &CapturedRow,
    row_index: u32,
    screen: ScreenId,
    generation: GridGeneration,
) -> CopyRow {
    copy_row_from_cells(
        row,
        |column, bias| ContentAnchor::Live {
            screen,
            point: GridPoint {
                row: row_index,
                column: column as u32,
            },
            bias,
            generation,
        },
        !row.continues,
    )
}

fn copy_row_from_cells(
    row: &CapturedRow,
    anchor: impl Fn(usize, Bias) -> ContentAnchor,
    hard_break_after: bool,
) -> CopyRow {
    let last = row.cells.len().saturating_sub(1);
    let mut cells = Vec::new();
    for (column, cell) in row.cells.iter().enumerate() {
        if cell.wide_spacer {
            continue;
        }
        cells.push(CopyCell {
            start: anchor(column, Bias::Before),
            end: anchor(column, Bias::After),
            text: if cell.text.is_empty() {
                " ".to_owned()
            } else {
                cell.text.clone()
            },
        });
    }
    CopyRow {
        start: anchor(0, Bias::Before),
        end: anchor(last, Bias::After),
        cells,
        hard_break_after,
    }
}

pub fn render_detection_task(
    engine: &MathEngine,
    task: &mut DetectionTask,
    foreground_rgb: [u8; 3],
) -> Result<MathRaster, MathRenderError> {
    if !resolve_detection_task(task) {
        return Err(MathRenderError::NotDetected);
    }
    engine.render(
        &task.span.source,
        MathRenderKey {
            dpi_milli: task.versions.layout.dpi_milli,
            font_milli_pt: NonZeroU32::new(12_000).expect("12 pt is non-zero"),
            foreground_rgb,
        },
    )
}

fn artifact_from_raster(task: &DetectionTask, raster: MathRaster) -> PlaceholderArtifact {
    let mut hasher = DefaultHasher::new();
    task.span.source.hash(&mut hasher);
    task.versions.layout.hash(&mut hasher);
    task.versions.detection.hash(&mut hasher);
    let height_subpixels = i64::from(raster.height_px).saturating_mul(SUBPIXELS_PER_PX);
    PlaceholderArtifact {
        key: format!("math:{:016x}", hasher.finish()),
        block_end: task.block_end,
        height_subpixels,
        width_px: raster.width_px,
        height_px: raster.height_px,
        rgba: Arc::from(raster.rgba),
        render_time: raster.render_time,
    }
}

fn ordered_selection(selection: &ViewSelection) -> Option<(&ContentAnchor, &ContentAnchor)> {
    match compare_selection_anchors(&selection.start, &selection.end)? {
        std::cmp::Ordering::Greater => Some((&selection.end, &selection.start)),
        _ => Some((&selection.start, &selection.end)),
    }
}

fn compare_selection_anchors(
    left: &ContentAnchor,
    right: &ContentAnchor,
) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (
            ContentAnchor::Live {
                screen: ScreenId::Alternate,
                point: left_point,
                bias: left_bias,
                ..
            },
            ContentAnchor::Live {
                screen: ScreenId::Alternate,
                point: right_point,
                bias: right_bias,
                ..
            },
        ) => Some((left_point, left_bias).cmp(&(right_point, right_bias))),
        _ => compare_anchors(left, right).ok(),
    }
}

fn selection_overlaps(
    item_start: &ContentAnchor,
    item_end: &ContentAnchor,
    selection_start: &ContentAnchor,
    selection_end: &ContentAnchor,
) -> bool {
    compare_selection_anchors(item_start, selection_end)
        .is_some_and(|order| order == std::cmp::Ordering::Less)
        && compare_selection_anchors(item_end, selection_start)
            .is_some_and(|order| order == std::cmp::Ordering::Greater)
}

fn trim_copy_line_end(text: &mut String) {
    text.truncate(text.trim_end_matches([' ', '\t']).len());
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

    fn synthetic_raster(width_px: u32, height_px: u32) -> MathRaster {
        MathRaster {
            rgba: vec![255; width_px as usize * height_px as usize * 4],
            width_px,
            height_px,
            content_height_px: height_px.saturating_sub(16),
            ascent_px: 12.0,
            descent_px: 4.0,
            render_time: std::time::Duration::from_millis(3),
        }
    }

    #[test]
    fn math_worker_intermediate_success_failure_and_layout_invalidation_are_projectable() {
        let mut session = DualPlaneSession::new(nz(16), nz(2));
        session.feed(b"$$x^2$$\r\nnext\r\ntail").unwrap();
        let mut task = session.take_worker_task().unwrap();

        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        session.refresh_projection(&mut projection);
        let pending = session.viewport_frame(&mut projection).unwrap();
        assert!(pending.math_blocks.is_empty());
        assert!(pending.cells.iter().any(|cell| cell.text == "$"));

        assert!(resolve_detection_task(&mut task));
        assert!(session.complete_worker_result(task, Ok(synthetic_raster(24, 35))));
        session.refresh_projection(&mut projection);
        projection.scroll_to_bottom();
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        let ready = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(ready.math_blocks.len(), 1);
        assert_eq!(projection.heights().get(0), Some(35 * SUBPIXELS_PER_PX));
        assert!(!ready.cells.iter().any(|cell| cell.text == "$"));

        session.set_layout_key(LayoutKey {
            dpi_milli: NonZeroU32::new(1250).unwrap(),
            ..session.layout_key()
        });
        session.refresh_projection(&mut projection);
        projection.scroll_to_bottom();
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        let invalidated = session.viewport_frame(&mut projection).unwrap();
        assert!(invalidated.math_blocks.is_empty());
        assert!(invalidated.cells.iter().any(|cell| cell.text == "$"));
        assert_eq!(session.pending_tasks(), 1);

        let mut failed = DualPlaneSession::new(nz(16), nz(2));
        failed.feed(b"$$bad$$\r\nnext\r\ntail").unwrap();
        let mut task = failed.take_worker_task().unwrap();
        assert!(resolve_detection_task(&mut task));
        assert!(failed.complete_worker_result(task, Err(MathRenderError::InvalidDimensions)));
        assert_eq!(
            failed.decorations.values().next().unwrap().decoration,
            DecorationLifecycle::Suppressed
        );
        let mut projection = failed.new_projection(failed.layout_key());
        failed.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        failed.refresh_projection(&mut projection);
        let fallback = failed.viewport_frame(&mut projection).unwrap();
        assert!(fallback.math_blocks.is_empty());
        assert!(fallback.cells.iter().any(|cell| cell.text == "$"));
    }

    #[test]
    fn theme_revision_rerenders_math_under_a_distinct_texture_key() {
        let mut session = DualPlaneSession::new(nz(16), nz(2));
        session.feed(b"$$x^2$$\r\nnext\r\ntail").unwrap();
        let mut first_task = session.take_worker_task().unwrap();
        assert!(resolve_detection_task(&mut first_task));
        assert!(session.complete_worker_result(first_task, Ok(synthetic_raster(24, 35))));
        let first_key = session
            .decorations
            .values()
            .find_map(|record| record.artifact.as_ref())
            .unwrap()
            .key
            .clone();

        session.set_layout_key(LayoutKey {
            theme_rev: 2,
            ..session.layout_key()
        });
        assert!(
            session
                .decorations
                .values()
                .all(|record| record.artifact.is_none())
        );
        let mut themed_task = session.take_worker_task().unwrap();
        assert_eq!(themed_task.versions.layout.theme_rev, 2);
        assert!(resolve_detection_task(&mut themed_task));
        assert!(session.complete_worker_result(themed_task, Ok(synthetic_raster(24, 35))));
        let themed_key = &session
            .decorations
            .values()
            .find_map(|record| record.artifact.as_ref())
            .unwrap()
            .key;
        assert_ne!(themed_key, &first_key);
    }

    #[test]
    fn ed3_discards_ready_math_artifacts_from_the_viewport_and_resident_budget() {
        let mut session = DualPlaneSession::new(nz(16), nz(2));
        session.feed(b"$$x^2$$\r\nnext\r\ntail").unwrap();
        let mut task = session.take_worker_task().unwrap();
        assert!(resolve_detection_task(&mut task));
        assert!(session.complete_worker_result(task, Ok(synthetic_raster(24, 35))));
        assert!(session.math_resident_bytes() > 0);

        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        session.refresh_projection(&mut projection);
        assert_eq!(
            session
                .viewport_frame(&mut projection)
                .unwrap()
                .math_blocks
                .len(),
            1
        );

        session.feed(b"\x1b[3J").unwrap();
        session.refresh_projection(&mut projection);
        let cleared = session.viewport_frame(&mut projection).unwrap();
        assert!(cleared.math_blocks.is_empty());
        assert_eq!(session.math_resident_bytes(), 0);
        assert!(session.decorations.is_empty());
    }

    #[test]
    fn byte_driven_terminal_state_projects_through_the_viewport_frame_boundary() {
        let mut session = DualPlaneSession::new(nz(4), nz(2));
        session.feed(b"\x1b[31mA").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();

        assert_eq!((frame.columns.get(), frame.rows.get()), (4, 2));
        assert_eq!(frame.cells.len(), 8);
        assert_eq!(frame.cells[0].text, "A");
        assert_eq!(frame.cells[0].style.foreground, TerminalColor::Named(1));
        assert_eq!((frame.cursor.row, frame.cursor.column), (0, 1));
    }

    #[test]
    fn synchronized_update_timeout_commits_buffered_cells_at_session_boundary() {
        let mut session = DualPlaneSession::new(nz(16), nz(2));
        session.feed(b"old\x1b[?2026h\rnew").unwrap();
        assert!(session.synchronized_update_deadline().is_some());
        assert_eq!(session.terminal().visible_text()[0], "old");

        assert!(session.finish_synchronized_update(Instant::now()).unwrap());
        assert!(session.synchronized_update_deadline().is_none());
        assert_eq!(session.terminal().visible_text()[0], "new");
        assert!(!session.finish_synchronized_update(Instant::now()).unwrap());
    }

    #[test]
    fn byte_driven_prompt_cursor_is_the_cell_after_typed_text_and_ignores_prediction() {
        let mut session = DualPlaneSession::new(nz(32), nz(2));
        session.feed(b"PS> carg").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let typed_end = frame.cells[..32]
            .iter()
            .rposition(|cell| !cell.text.chars().all(char::is_whitespace))
            .unwrap() as u32;

        assert_eq!(frame.cursor.column, typed_end + 1);
        assert_eq!(frame.cursor.column, 8);

        // PSReadLine paints inline prediction after saving the input cursor, then restores it.
        // Prediction cells remain visible but must not participate in the cursor column.
        session.feed(b"o\x1b7 --version\x1b8").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(frame.cursor.column, "PS> cargo".len() as u32);
        assert!(
            frame.cells[..32]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("cargo --version")
        );
    }

    #[test]
    fn selection_copy_rejoins_soft_wraps_and_uses_crlf_for_hard_rows() {
        let mut soft = DualPlaneSession::new(nz(4), nz(2));
        soft.feed(b"abcdef").unwrap();
        let mut projection = soft.new_projection(soft.layout_key());
        let frame = soft.viewport_frame(&mut projection).unwrap();
        soft.set_view_selection(Some(ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(1, 1, Bias::After).unwrap().unwrap(),
        }));
        assert_eq!(soft.selection_text().as_deref(), Some("abcdef"));

        let mut hard = DualPlaneSession::new(nz(4), nz(2));
        hard.feed(b"ab\r\ncd").unwrap();
        let mut projection = hard.new_projection(hard.layout_key());
        let frame = hard.viewport_frame(&mut projection).unwrap();
        hard.set_view_selection(Some(ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(1, 1, Bias::After).unwrap().unwrap(),
        }));
        assert_eq!(hard.selection_text().as_deref(), Some("ab\r\ncd"));
    }

    #[test]
    fn selecting_a_wide_spacer_copies_the_whole_cluster_and_output_clears_live_selection() {
        let mut session = DualPlaneSession::new(nz(4), nz(2));
        session.feed("中".as_bytes()).unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        session.set_view_selection(Some(ViewSelection {
            start: frame.anchor_at(0, 1, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 1, Bias::After).unwrap().unwrap(),
        }));
        assert_eq!(session.selection_text().as_deref(), Some("中"));
        session.feed(b"x").unwrap();
        assert!(session.view_selection().is_none());
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
