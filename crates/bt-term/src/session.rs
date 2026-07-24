use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    error::Error,
    fmt,
    hash::{DefaultHasher, Hash, Hasher},
    num::{NonZeroI64, NonZeroU32, NonZeroUsize},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_detect::{
    DecorationRecord, DelimiterKind, DetectionContext, DetectionInput, DetectionOptions,
    DetectionTask, LiveDetectionInput, LiveDetectionSource, LiveDetectionTask,
    MAX_MATH_SOURCE_BYTES, MathCellSegment, MathSourceLine, MathSpan, PlaceholderArtifact,
    StaleArtifact, advance_detection_context, detect_math_blocks_with_options,
    resolve_detection_task, resolve_live_detection_task, resolve_live_detection_tasks,
};
use bt_doc::{
    AnchorError, AnchorId, Bias, ContentAnchor, DecorationIntent, DecorationLifecycle,
    DetectionRevision, GridGeneration, GridPoint, HistoryDocument, InvalidSourceTransition,
    LayoutKey, LiveRowRemoval, SUBPIXELS_PER_PX, ScreenId, SourceLifecycle, VersionStamp,
    ViewGeneration, compare_anchors,
};
use bt_math::{MathEngine, MathFailureStage, MathMode, MathRaster, MathRenderError, MathRenderKey};
use bt_transcript::{
    CaptureResult, CapturedRow, CellFlags, DEFAULT_STAGING_QUOTA, FinalizedLine, FrozenLine,
    GraphemeOffset, SPIKE_DEFAULT_FROZEN_QUOTA, SourceGeneration, StagedRow, StagingId,
    TranscriptId, TranscriptStore,
};
use bt_viewport::{
    FrameProjectionError, FrameViewportOrigin, GridCursor, HorizontalOverflowOwner,
    LiveMathOccurrenceId, MathBlockAnchor, MathBlockDisplay, MathBlockPlacement,
    MathFailurePlacement, ProjectedLiveMathArtifact, ProjectedMathArtifact, ViewSelection,
    ViewportFrame, ViewportProjection,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    adapter::{
        AdapterEvent, RemovalCause, RemovalScope, RemovalScreen, TerminalAdapter, TerminalDamage,
        TerminalModes,
    },
    cell_capture::CapturedRowFingerprint,
    lifecycle::{LifecycleDirective, RowDirective, classify, plan_resize},
    scheduling::{EnqueueOutcome, PARSE_QUANTUM, ResizeEpoch, WORKER_QUEUE_CAP, WorkerScheduler},
};

pub const SPIKE_CELL_HEIGHT_SUBPIXELS: NonZeroI64 = NonZeroI64::new(18 * SUBPIXELS_PER_PX).unwrap();
pub const LIVE_MATH_STABLE_INTERVAL: Duration = Duration::from_millis(200);
/// Primary live detection carries 1,024 frozen logical lines before the live grid. That is more
/// than forty conventional 24-row terminal screens while bounding each shared worker snapshot.
/// It is context, not an inference: an opener older than this tail is unknowable at this layer.
const LIVE_FENCE_HISTORY_CONTEXT_LINES: usize = 1_024;
const MAX_OFFSCREEN_RECORDS: usize = 128;
/// Two trailing blank rows add at most 36 px at the baseline metrics: enough for common display
/// math while preventing a single formula from consuming an arbitrarily large blank separator.
const LIVE_MATH_MAX_BORROWED_BLANK_ROWS: u32 = 2;
/// M1.9m presentation padding is expressed as thousandths of the measured cell height so one
/// option follows both DPI and font metrics. The default is one quarter cell on each side.
pub const DEFAULT_MATH_VERTICAL_PADDING_CELL_MILLI: u32 = 250;
/// Alpha-tight display rasters begin half a terminal cell inside the pane. This follows the
/// measured cell advance across fonts and DPI instead of baking a logical-pixel value into layout.
const DISPLAY_MATH_LEFT_INSET_DENOMINATOR: i64 = 2;
pub const LIVE_MATH_READABLE_SCALE_MILLI: u32 = bt_viewport::LIVE_MATH_READABLE_SCALE_MILLI;
pub const LIVE_MIN_VISIBLE_TEXT_ROWS: u32 = bt_viewport::LIVE_MIN_VISIBLE_TEXT_ROWS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MathLayoutOptions {
    pub line_wrapping: bool,
    pub block_max_height_px: Option<NonZeroU32>,
    /// Symmetric display-math padding per side, in thousandths of the measured cell height.
    /// Zero is valid and intentionally exposes the alpha-tight raster without breathing room.
    pub vertical_padding_cell_milli: u32,
    /// Work around Claude Code stripping one slash from environment row separators. Disable this
    /// after Claude Code emits LaTeX `\\\\` row separators faithfully.
    pub restore_stripped_environment_newlines: bool,
    /// Reject Claude Code's exact Jump-to-bottom overlay when it is written into a math row.
    pub reject_claude_code_jump_chip_overlay: bool,
}

impl Default for MathLayoutOptions {
    fn default() -> Self {
        Self {
            line_wrapping: true,
            block_max_height_px: None,
            vertical_padding_cell_milli: DEFAULT_MATH_VERTICAL_PADDING_CELL_MILLI,
            restore_stripped_environment_newlines: true,
            reject_claude_code_jump_chip_overlay: true,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct MathSourcePreferenceKey {
    original_source: String,
    mode: MathMode,
}

impl MathSourcePreferenceKey {
    fn from_span(span: &MathSpan) -> Self {
        Self {
            original_source: span.original_source.clone(),
            mode: span.mode,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SessionMathTask {
    Frozen(DetectionTask),
    Live(LiveDetectionTask),
}

#[derive(Clone, Debug, Default)]
struct LiveRowStability {
    revision: u64,
    last_damage_at: Option<Instant>,
    settled_revision: Option<u64>,
    candidate_signature: Option<u64>,
    content_fingerprint: Option<CapturedRowFingerprint>,
}

#[derive(Clone, Debug)]
struct ProvenLiveRow {
    band_offset: u32,
    text: String,
    continues: bool,
    cell_boundaries: Vec<(u32, u32)>,
}

impl ProvenLiveRow {
    fn exactly_matches(&self, input: &LiveDetectionInput) -> bool {
        self.text == input.text
            && self.continues == input.continues
            && self.cell_boundaries == input.cell_boundaries
    }

    /// Column ranges of `input`'s row that still display this occurrence's proven source and may
    /// therefore be cleared. `None` = the row does not carry this row's source. Identification
    /// requires the row to begin with this row's proven source (optionally interrupted by Claude
    /// Code's Jump chip); the cleared set is then exactly the cells whose content equals the
    /// proven source cell at the same column, so an overlay splitting the row (chip text, its
    /// highlight style, the trailing arrow) keeps every one of its own cells untouched while the
    /// leaked source on either side of it is removed.
    fn source_clear_ranges(&self, input: &LiveDetectionInput) -> Option<Vec<(u32, u32)>> {
        if self.text.is_empty() || self.continues != input.continues {
            return None;
        }
        let identified = (input.text.starts_with(&self.text)
            && input.cell_boundaries.starts_with(&self.cell_boundaries))
            || self.chip_split_matches(input);
        if !identified {
            return None;
        }

        let proven_cells = boundary_cells(&self.text, &self.cell_boundaries);
        let input_cells = boundary_cells(&input.text, &input.cell_boundaries);
        let mut ranges = Vec::<(u32, u32)>::new();
        for proven_cell in &proven_cells {
            let matches = input_cells.iter().any(|input_cell| {
                input_cell.columns == proven_cell.columns && input_cell.text == proven_cell.text
            });
            if !matches {
                continue;
            }
            let (start, end) = proven_cell.columns;
            match ranges.last_mut() {
                Some((_, previous_end)) if *previous_end == start => *previous_end = end,
                _ => ranges.push((start, end)),
            }
        }
        if ranges.is_empty() {
            None
        } else {
            Some(ranges)
        }
    }

    /// The Jump chip overwrote this row mid-source: everything visible before the exact chip
    /// signature must be this row's proven source prefix, byte- and boundary-identical.
    fn chip_split_matches(&self, input: &LiveDetectionInput) -> bool {
        let Some((before_chip, _)) = input.text.split_once("Jump to bottom (ctrl+End)") else {
            return false;
        };
        let visible_source = before_chip.trim_end();
        if visible_source.is_empty() || !self.text.starts_with(visible_source) {
            return false;
        }
        let prefix_end = u32::try_from(visible_source.len()).unwrap_or(u32::MAX);
        let proven_boundaries = self
            .cell_boundaries
            .iter()
            .take_while(|(byte, _)| *byte <= prefix_end);
        let visible_boundaries = input
            .cell_boundaries
            .iter()
            .take_while(|(byte, _)| *byte <= prefix_end);
        proven_boundaries.eq(visible_boundaries)
    }
}

#[derive(Eq, PartialEq)]
struct BoundaryCell<'a> {
    columns: (u32, u32),
    text: &'a str,
}

/// Split boundary-mapped row text back into its per-cell pieces. Consecutive `(byte, column)`
/// boundary pairs delimit one cell's bytes and its column span (wide glyphs span two columns).
fn boundary_cells<'a>(text: &'a str, boundaries: &[(u32, u32)]) -> Vec<BoundaryCell<'a>> {
    boundaries
        .windows(2)
        .filter_map(|window| {
            let [(byte_start, column_start), (byte_end, column_end)] = window else {
                return None;
            };
            let piece = text.get(*byte_start as usize..*byte_end as usize)?;
            Some(BoundaryCell {
                columns: (*column_start, *column_end),
                text: piece,
            })
        })
        .collect()
}

/// Immutable identity for one detector-proven occurrence. Grid rows deliberately do not live in
/// this object: two identical formulas remain distinct occurrences, while repaint placement can
/// move or become partially occluded without rewriting the proof which produced the artifact.
#[derive(Clone, Debug)]
struct ProvenLiveOccurrence {
    occurrence_id: LiveMathOccurrenceId,
    created_generation: GridGeneration,
    created_start: GridPoint,
    band_rows: u32,
    source_start_offset: u32,
    source_end_offset: u32,
    source_rows: Vec<ProvenLiveRow>,
    span: MathSpan,
}

/// Mutable projection of a proven occurrence into the current alternate-screen transaction.
/// `logical_band_start` is signed and never clamped to a terminal edge. The visible fields on the
/// record below are only the exact intersection which projection is allowed to suppress.
#[derive(Clone, Debug)]
struct LiveOccurrencePlacement {
    logical_band_start: i64,
    occluded_source_rows: u32,
    /// `(terminal_row, column_ranges)` pairs outside the projected band whose current cells still
    /// show this occurrence's proven source. Viewport may clear only those exact cells — never
    /// chrome, and never the cells of an application overlay (Jump chip) sharing the row.
    occluded_visible_rows: Vec<(u32, Vec<(u32, u32)>)>,
}

#[derive(Clone, Debug)]
struct LiveDecorationRecord {
    identity: ProvenLiveOccurrence,
    placement: LiveOccurrencePlacement,
    screen: ScreenId,
    generation: GridGeneration,
    start: GridPoint,
    end: GridPoint,
    band_start_row: u32,
    band_end_row: u32,
    /// Frozen transcript rows (opener and body) that already committed to scrollback while this
    /// occurrence's closer is still in the live grid. Empty for an ordinary all-live block. Ordered
    /// top to bottom, immediately preceding the live band; the presentation layer bridges the two
    /// domains into one rendered block so a boundary-split formula does not stall as source.
    frozen_prefix: Vec<TranscriptId>,
    /// Rows from the proven owned band that currently sit above alternate-screen row zero.
    /// Projection keeps their geometry but clips their pixels and terminal cells.
    clipped_top_rows: u32,
    /// Rows from the proven owned band that currently sit below the alternate-screen bottom.
    /// Like top clipping, these retain identity and geometry without entering the visible grid.
    clipped_bottom_rows: u32,
    detection_revision: DetectionRevision,
    layout: LayoutKey,
    rendered_layout: LayoutKey,
    initial_context: DetectionContext,
    inputs: Arc<[LiveDetectionInput]>,
    span: MathSpan,
    artifact: Option<PlaceholderArtifact>,
    stale_artifact: Option<StaleArtifact>,
    show_source: bool,
    hovered: bool,
    horizontal_scroll_px: u32,
    vertical_scroll_px: u32,
    failure_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SegmentedRowMapping {
    content_delta: i64,
    content_start_row: u32,
    content_end_row: u32,
    fixed_start_row: Option<u32>,
}

#[derive(Clone, Debug)]
struct AlternateRepaintSnapshot {
    inputs: Arc<[LiveDetectionInput]>,
    decorations: Vec<LiveDecorationRecord>,
    dormant_decorations: Vec<LiveDecorationRecord>,
    invalidation_count: u64,
    snapshot_boundary: bool,
}

#[derive(Clone, Debug)]
struct PendingLiveArtifactHandoff {
    span: MathSpan,
    artifact: PlaceholderArtifact,
    layout: LayoutKey,
    detection_revision: DetectionRevision,
    candidate_staging: StagingId,
    candidate_start: Option<TranscriptId>,
    expected_frozen_lines: u64,
}

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
    cell_width_subpixels: NonZeroI64,
    ascii_baseline_subpixels: Option<NonZeroI64>,
    math_layout_options: MathLayoutOptions,
    live_screen: ScreenId,
    alternate_detection_context: DetectionContext,
    live_rows: Vec<LiveRowStability>,
    live_tasks: VecDeque<LiveDetectionTask>,
    live_decorations: BTreeMap<u32, LiveDecorationRecord>,
    next_live_occurrence_id: u64,
    offscreen_decorations: VecDeque<LiveDecorationRecord>,
    alternate_repaint_snapshot: Option<AlternateRepaintSnapshot>,
    alternate_repaint_in_progress: bool,
    alternate_content_end_row: Option<u32>,
    /// User presentation choices are content state, not decoration-instance state. Entries are
    /// created only by an explicit toggle and live for the session, so alternate-screen repaint,
    /// redetection, grid-generation changes, and layout changes cannot reset the choice.
    math_source_preferences: HashMap<MathSourcePreferenceKey, bool>,
    pending_live_handoffs: Vec<PendingLiveArtifactHandoff>,
    frozen_detection_context: DetectionContext,
    frozen_detection_contexts: BTreeMap<TranscriptId, DetectionContext>,
    frozen_detection_count: u64,
    live_detection_count: u64,
    live_invalidation_count: u64,
    math_failure_validate_count: u64,
    math_failure_convert_count: u64,
    math_failure_compile_count: u64,
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
            cell_width_subpixels: NonZeroI64::new(9 * SUBPIXELS_PER_PX).unwrap(),
            ascii_baseline_subpixels: None,
            math_layout_options: MathLayoutOptions::default(),
            live_screen: ScreenId::Primary,
            alternate_detection_context: DetectionContext::default(),
            live_rows: vec![LiveRowStability::default(); rows.get() as usize],
            live_tasks: VecDeque::new(),
            live_decorations: BTreeMap::new(),
            next_live_occurrence_id: 1,
            offscreen_decorations: VecDeque::new(),
            alternate_repaint_snapshot: None,
            alternate_repaint_in_progress: false,
            alternate_content_end_row: None,
            math_source_preferences: HashMap::new(),
            pending_live_handoffs: Vec::new(),
            frozen_detection_context: DetectionContext::default(),
            frozen_detection_contexts: BTreeMap::new(),
            frozen_detection_count: 0,
            live_detection_count: 0,
            live_invalidation_count: 0,
            math_failure_validate_count: 0,
            math_failure_convert_count: 0,
            math_failure_compile_count: 0,
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

    pub fn set_cell_width_subpixels(&mut self, cell_width_subpixels: NonZeroI64) {
        self.cell_width_subpixels = cell_width_subpixels;
    }

    fn display_math_left_inset_subpixels(&self) -> i64 {
        self.cell_width_subpixels
            .get()
            .div_euclid(DISPLAY_MATH_LEFT_INSET_DENOMINATOR)
            .max(1)
    }

    pub fn set_ascii_baseline_subpixels(&mut self, ascii_baseline_subpixels: NonZeroI64) {
        self.ascii_baseline_subpixels = Some(ascii_baseline_subpixels);
    }

    pub fn set_math_layout_options(&mut self, options: MathLayoutOptions) {
        self.math_layout_options = options;
    }

    fn detection_options(&self) -> DetectionOptions {
        DetectionOptions {
            restore_stripped_environment_newlines: self
                .math_layout_options
                .restore_stripped_environment_newlines,
            reject_claude_code_jump_chip_overlay: self
                .math_layout_options
                .reject_claude_code_jump_chip_overlay,
        }
    }

    fn math_vertical_padding_subpixels(&self) -> i64 {
        self.cell_height_subpixels
            .get()
            .saturating_mul(i64::from(
                self.math_layout_options.vertical_padding_cell_milli,
            ))
            .div_euclid(1000)
    }

    pub fn live_detection_count(&self) -> u64 {
        self.live_detection_count
    }

    pub fn frozen_detection_count(&self) -> u64 {
        self.frozen_detection_count
    }

    pub fn live_invalidation_count(&self) -> u64 {
        self.live_invalidation_count
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
        if self.alternate_repaint_snapshot.is_none() {
            self.alternate_repaint_snapshot = self.begin_alternate_repaint(bytes);
        }
        self.alternate_repaint_in_progress = self.alternate_repaint_snapshot.is_some();
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
        if self.terminal.modes().alternate_screen && contains_clear_home_snapshot_boundary(bytes) {
            self.alternate_detection_context = DetectionContext::default();
        }
        let result = (|| {
            for chunk in bytes.chunks(PARSE_QUANTUM) {
                let events = self.terminal.feed(chunk);
                let damage = self.terminal.take_damage();
                self.apply_events(events, observed_at)?;
                self.observe_live_damage(damage, observed_at);
                self.sync_staging_tail();
            }
            Ok(())
        })();
        if result.is_err() {
            self.alternate_repaint_snapshot = None;
            self.alternate_repaint_in_progress = false;
            self.invalidate_all_live_decorations();
        } else if self.synchronized_update_deadline().is_none()
            && let Some(snapshot) = self.alternate_repaint_snapshot.take()
        {
            self.finish_alternate_repaint(snapshot);
        }
        self.alternate_repaint_in_progress = self.alternate_repaint_snapshot.is_some();
        if result.is_ok() {
            self.restore_offscreen_decorations();
        }
        result
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
        let alternate_resize = self.snapshot_alternate_repaint(false);
        self.alternate_repaint_in_progress = alternate_resize.is_some();
        let events = self.terminal.resize(columns, rows);
        let _ = self.terminal.take_damage();
        if alternate_resize.is_none() {
            // On primary this preserves proven formulas off-band for the resize transaction rather
            // than wiping them (see `invalidate_all_live_decorations`); `restore_offscreen_decorations`
            // below re-anchors each by exact source equality onto the reflowed grid.
            self.invalidate_all_live_decorations();
        }
        self.pending_live_handoffs.clear();
        // Seed the damage clock instead of leaving it None: stability is judged by
        // `last_damage_at.is_some_and(elapsed >= INTERVAL)`, so a None row is never stable and
        // never re-detected. A resize rebuilds every row, so leaving them None meant formulas
        // stayed source forever after any window resize - until unrelated output happened to
        // touch that row (user report 2026-07-19). Seeding with the resize instant makes each row
        // settle one stable interval later, which is also the honest reading: the grid just
        // changed, so it is not yet quiet.
        self.live_rows = vec![
            LiveRowStability {
                last_damage_at: Some(observed_at),
                ..LiveRowStability::default()
            };
            rows.get() as usize
        ];
        let apply_result = self.apply_events(events, observed_at);
        self.alternate_repaint_in_progress = false;
        apply_result?;
        if let Some(snapshot) = alternate_resize {
            self.finish_alternate_repaint(snapshot);
        }
        self.restore_offscreen_decorations();
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
        self.alternate_repaint_in_progress = self.alternate_repaint_snapshot.is_some();
        let events = self.terminal.finish_synchronized_update();
        let damage = self.terminal.take_damage();
        if let Err(error) = self.apply_events(events, observed_at) {
            self.alternate_repaint_snapshot = None;
            self.alternate_repaint_in_progress = false;
            self.invalidate_all_live_decorations();
            return Err(error);
        }
        self.observe_live_damage(damage, observed_at);
        self.sync_staging_tail();
        if let Some(snapshot) = self.alternate_repaint_snapshot.take() {
            self.finish_alternate_repaint(snapshot);
        }
        self.alternate_repaint_in_progress = false;
        self.restore_offscreen_decorations();
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
            // The vendor reconcile can shift rows and always bumps the grid generation, which
            // strands the formulas `restore_offscreen_decorations` re-anchored inside `resize_at`
            // one generation behind the frame the app is about to publish. Re-anchor them against
            // the now-settled grid so proven blocks render immediately instead of flashing to
            // source for the post-reconcile frame. Primary-only; alternate keeps its own path.
            if self.primary_resize_preservation_active() {
                self.retain_live_decorations_offscreen();
                self.restore_offscreen_decorations();
            }
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
        // The transaction is over: fresh detection is authoritative again, so drop any primary
        // formulas still held off-band whose exact source never reappeared on the reflowed grid.
        // (Alternate keeps its queue across repaints, so it is left untouched.)
        if self.live_screen == ScreenId::Primary {
            self.offscreen_decorations.clear();
        }
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

    pub fn live_stability_deadline(&self) -> Option<Instant> {
        // A resize epoch owns live rows until output has also gone quiet. Suppress the independent
        // stability timer for that whole interval: retaining a pre-epoch deadline would feed an
        // already-past WaitUntil back to winit on every turn while advance_live_stability refuses
        // to settle it.
        if self.resize_epoch.is_active() {
            return None;
        }
        self.live_rows
            .iter()
            .filter(|row| row.settled_revision != Some(row.revision))
            .filter_map(|row| row.last_damage_at.map(|at| at + LIVE_MATH_STABLE_INTERVAL))
            .min()
    }

    /// Advance only when the event loop's existing `WaitUntil` reaches a damage-derived deadline.
    /// Stability gates candidate rows only. Fence and delimiter context is always scanned from the
    /// top of the context available now: the complete alternate screen, or a bounded primary
    /// transcript tail followed by the complete live grid.
    pub fn advance_live_stability(&mut self, now: Instant) -> usize {
        if self.resize_epoch.is_active() {
            return 0;
        }
        let stable = self
            .live_rows
            .iter()
            .map(|row| {
                row.last_damage_at.is_some_and(|at| {
                    now.saturating_duration_since(at) >= LIVE_MATH_STABLE_INTERVAL
                })
            })
            .collect::<Vec<_>>();
        for (row, is_stable) in self.live_rows.iter_mut().zip(&stable) {
            if *is_stable {
                row.settled_revision = Some(row.revision);
            }
        }

        let inputs = self.live_detection_context();
        let initial_context = self.live_initial_detection_context(&inputs);
        let candidates = live_candidate_rows(&inputs, initial_context.clone(), &stable);
        let context_signature = live_detection_context_signature(&inputs);
        let mut new_tasks = Vec::new();
        for candidate_row in candidates {
            let signature = live_detection_signature(context_signature, candidate_row);
            let state = &mut self.live_rows[candidate_row as usize];
            if state.candidate_signature == Some(signature) {
                continue;
            }
            state.candidate_signature = Some(signature);
            let task = LiveDetectionTask {
                candidate_row,
                screen: self.live_screen,
                grid_generation: self.grid_generation,
                detection_revision: self.detection_revision,
                layout: self.layout_key,
                cell_width_subpixels: self.cell_width_subpixels.get(),
                cell_height_subpixels: self.cell_height_subpixels.get(),
                ascii_baseline_subpixels: self.ascii_baseline_subpixels.map_or(0, NonZeroI64::get),
                options: self.detection_options(),
                initial_context: initial_context.clone(),
                inputs: Arc::clone(&inputs),
                start: GridPoint {
                    row: candidate_row,
                    column: 0,
                },
                end: GridPoint {
                    row: candidate_row,
                    column: 0,
                },
                band_start_row: candidate_row,
                band_end_row: candidate_row,
                span: MathSpan {
                    byte_start: 0,
                    byte_end: 0,
                    original_source: String::new(),
                    render_source: String::new(),
                    delimiter_kind: DelimiterKind::Dollars,
                    mode: MathMode::Display,
                    cell_segments: Vec::new(),
                    inline_runs: Vec::new(),
                },
                detection_complete: false,
                resolved: false,
            };
            new_tasks.push(task);
        }
        resolve_live_detection_tasks(&mut new_tasks);
        let scheduled = new_tasks.len();
        for task in new_tasks {
            self.enqueue_live_task(task);
        }
        self.live_detection_count = self.live_detection_count.saturating_add(scheduled as u64);
        if scheduled != 0 && std::env::var_os("BT_PERF_TRACE").is_some() {
            eprintln!(
                "BT_PERF_TRACE live_math_detect={} live_math_invalidations={}",
                self.live_detection_count, self.live_invalidation_count
            );
        }
        scheduled
    }

    fn live_detection_context(&self) -> Arc<[LiveDetectionInput]> {
        let mut inputs = Vec::new();
        if self.live_screen == ScreenId::Primary {
            let history_tail = self
                .document
                .entries()
                .iter()
                .rev()
                .take(LIVE_FENCE_HISTORY_CONTEXT_LINES)
                .collect::<Vec<_>>();
            inputs.extend(
                history_tail
                    .into_iter()
                    .rev()
                    .map(|(id, entry)| LiveDetectionInput {
                        source: LiveDetectionSource::History { id: *id },
                        text: entry.line.text.clone(),
                        continues: false,
                        cell_boundaries: frozen_cell_boundaries(&entry.line),
                    }),
            );
        }
        let grid_inputs = (0..self.live_rows.len()).filter_map(|row| {
            self.terminal.visible_row(row as u32).map(|captured| {
                let (text, cell_boundaries) = captured_row_text_and_boundaries(&captured);
                LiveDetectionInput {
                    source: LiveDetectionSource::Grid {
                        row: row as u32,
                        revision: self.live_rows[row].revision,
                    },
                    text,
                    continues: captured.continues,
                    cell_boundaries,
                }
            })
        });
        inputs.extend(grid_inputs);
        Arc::from(inputs)
    }

    fn live_initial_detection_context(&self, inputs: &[LiveDetectionInput]) -> DetectionContext {
        match self.live_screen {
            ScreenId::Primary => inputs
                .first()
                .and_then(|input| match input.source {
                    LiveDetectionSource::History { id } => {
                        self.frozen_detection_contexts.get(&id).cloned()
                    }
                    LiveDetectionSource::Grid { .. } => Some(self.frozen_detection_context.clone()),
                })
                .unwrap_or_else(DetectionContext::ambiguous),
            ScreenId::Alternate => self.alternate_detection_context.clone(),
        }
    }

    fn begin_alternate_repaint(&self, bytes: &[u8]) -> Option<AlternateRepaintSnapshot> {
        let snapshot_boundary = contains_clear_home_snapshot_boundary(bytes);
        snapshot_boundary
            .then(|| self.snapshot_alternate_repaint(snapshot_boundary))
            .flatten()
    }

    fn snapshot_alternate_repaint(
        &self,
        snapshot_boundary: bool,
    ) -> Option<AlternateRepaintSnapshot> {
        (self.live_screen == ScreenId::Alternate
            && self.terminal.modes().alternate_screen
            && (!self.live_decorations.is_empty() || !self.offscreen_decorations.is_empty()))
        .then(|| {
            let inputs = self.live_detection_context();
            AlternateRepaintSnapshot {
                inputs,
                decorations: self.live_decorations.values().cloned().collect(),
                dormant_decorations: self.offscreen_decorations.iter().cloned().collect(),
                invalidation_count: self.live_invalidation_count,
                snapshot_boundary,
            }
        })
    }

    fn finish_alternate_repaint(&mut self, snapshot: AlternateRepaintSnapshot) {
        if self.live_screen != ScreenId::Alternate || !self.terminal.modes().alternate_screen {
            return;
        }

        let current_inputs = self.live_detection_context();
        let current_initial_context = self.live_initial_detection_context(&current_inputs);
        let mut row_mappings = segmented_row_mapping(&snapshot.inputs, &current_inputs);
        if let Some(content_end_row) = self.alternate_content_end_row
            && fixed_boundary_remains_proven(&snapshot.inputs, &current_inputs, content_end_row)
        {
            for mapping in &mut row_mappings {
                if mapping.fixed_start_row.is_none() {
                    mapping.content_end_row = mapping.content_end_row.min(content_end_row);
                    mapping.fixed_start_row = content_end_row.checked_add(1);
                }
            }
        }
        if let Some(content_end_row) = row_mappings
            .iter()
            .filter_map(|mapping| mapping.fixed_start_row.map(|_| mapping.content_end_row))
            .min()
        {
            self.alternate_content_end_row = Some(content_end_row);
        }
        let mut preserved = BTreeMap::new();
        let mut occupied = BTreeSet::new();
        let mut unresolved = Vec::new();
        self.offscreen_decorations.clear();

        for record in snapshot
            .decorations
            .into_iter()
            .chain(snapshot.dormant_decorations)
        {
            if row_mappings.is_empty() {
                unresolved.push(record);
                continue;
            }
            let Some(projected) = project_live_record_uniquely(
                &record,
                &row_mappings,
                self.grid_generation,
                self.detection_revision,
                self.layout_key,
                current_initial_context.clone(),
                Arc::clone(&current_inputs),
            ) else {
                unresolved.push(record);
                continue;
            };
            let record = match projected {
                RecordProjection::Visible(record) => record,
                RecordProjection::Dormant(record) => {
                    self.retain_offscreen_record(record);
                    continue;
                }
            };
            if let Some(record) =
                insert_nonoverlapping_live_record(&mut preserved, &mut occupied, record)
            {
                unresolved.push(record);
            }
        }

        if snapshot.snapshot_boundary && !unresolved.is_empty() {
            let detected = self.bounded_alternate_repaint_detection(
                Arc::clone(&current_inputs),
                current_initial_context.clone(),
            );
            let mut still_unresolved = Vec::new();
            for record in unresolved {
                let matches = detected
                    .iter()
                    .filter(|task| task.span.render_equivalent(&record.span))
                    .collect::<Vec<_>>();
                let [task] = matches.as_slice() else {
                    still_unresolved.push(record);
                    continue;
                };
                let delta = i64::from(task.start.row)
                    .saturating_sub(record.placement.logical_band_start)
                    .saturating_sub(i64::from(record.identity.source_start_offset));
                let Some(mut record) = shift_live_record(
                    &record,
                    delta,
                    self.grid_generation,
                    self.detection_revision,
                    self.layout_key,
                    current_initial_context.clone(),
                    Arc::clone(&current_inputs),
                ) else {
                    still_unresolved.push(record);
                    continue;
                };
                if record.end.row != task.end.row
                    || !alternate_borrowed_band_is_clear(&record, &current_inputs, &occupied)
                {
                    still_unresolved.push(record);
                    continue;
                }
                record.start = task.start;
                record.end = task.end;
                record.span = task.span.clone();
                if let Some(record) =
                    insert_nonoverlapping_live_record(&mut preserved, &mut occupied, record)
                {
                    still_unresolved.push(record);
                }
            }
            unresolved = still_unresolved;
        }

        self.live_invalidation_count = snapshot.invalidation_count;
        for record in unresolved {
            if record.artifact.is_some() || record.stale_artifact.is_some() {
                self.retain_offscreen_record(record);
            } else {
                self.live_invalidation_count = self.live_invalidation_count.saturating_add(1);
            }
        }
        self.live_decorations = preserved;
        let context_signature = live_detection_context_signature(&current_inputs);
        let stable = vec![true; self.live_rows.len()];
        let candidate_rows = live_candidate_rows(&current_inputs, current_initial_context, &stable);
        for record in self.live_decorations.values() {
            for row in record.band_start_row..=record.band_end_row {
                if let Some(state) = self.live_rows.get_mut(row as usize) {
                    state.content_fingerprint = self.terminal.visible_row_fingerprint(row);
                }
            }
            for candidate_row in candidate_rows
                .iter()
                .copied()
                .filter(|row| (record.start.row..=record.end.row).contains(row))
            {
                if let Some(state) = self.live_rows.get_mut(candidate_row as usize) {
                    state.candidate_signature =
                        Some(live_detection_signature(context_signature, candidate_row));
                }
            }
        }
    }

    fn bounded_alternate_repaint_detection(
        &self,
        inputs: Arc<[LiveDetectionInput]>,
        initial_context: DetectionContext,
    ) -> Vec<LiveDetectionTask> {
        if !inputs
            .iter()
            .any(|input| may_contain_display_math(input.text.trim()))
        {
            return Vec::new();
        }
        let stable = vec![true; self.live_rows.len()];
        let candidates = live_candidate_rows(&inputs, initial_context.clone(), &stable);
        let mut tasks = candidates
            .into_iter()
            .map(|candidate_row| LiveDetectionTask {
                candidate_row,
                screen: self.live_screen,
                grid_generation: self.grid_generation,
                detection_revision: self.detection_revision,
                layout: self.layout_key,
                cell_width_subpixels: self.cell_width_subpixels.get(),
                cell_height_subpixels: self.cell_height_subpixels.get(),
                ascii_baseline_subpixels: self.ascii_baseline_subpixels.map_or(0, NonZeroI64::get),
                options: self.detection_options(),
                initial_context: initial_context.clone(),
                inputs: Arc::clone(&inputs),
                start: GridPoint {
                    row: candidate_row,
                    column: 0,
                },
                end: GridPoint {
                    row: candidate_row,
                    column: 0,
                },
                band_start_row: candidate_row,
                band_end_row: candidate_row,
                span: empty_live_math_span(),
                detection_complete: false,
                resolved: false,
            })
            .collect::<Vec<_>>();
        resolve_live_detection_tasks(&mut tasks);
        tasks.retain(|task| task.resolved);
        tasks
    }

    /// The current live screen owns the off-band preservation queue. Alternate retains renderable
    /// decorations across every repaint; primary retains them only for the span of a resize
    /// transaction so a reflow does not flash proven formulas back to source (see
    /// `primary_resize_preservation_active`). Any other primary state drops as before.
    fn offscreen_preservation_active(&self) -> bool {
        self.live_screen == ScreenId::Alternate || self.primary_resize_preservation_active()
    }

    /// Primary preserves live formulas across a window resize by holding the same off-band queue
    /// alternate uses, but only while a resize transaction is open. Codex reflows and then reprints
    /// its whole transcript, so proven blocks would otherwise revert to source between the reflow
    /// and re-detection. The queue is re-anchored by exact source equality every frame and cleared
    /// once the transaction quiesces, at which point fresh detection is authoritative again.
    fn primary_resize_preservation_active(&self) -> bool {
        self.live_screen == ScreenId::Primary && self.resize_epoch.is_active()
    }

    fn retain_offscreen_record(&mut self, record: LiveDecorationRecord) {
        if !self.offscreen_preservation_active() {
            return;
        }
        if self.offscreen_decorations.len() == MAX_OFFSCREEN_RECORDS {
            self.offscreen_decorations.pop_front();
        }
        self.offscreen_decorations.push_back(record);
    }

    /// Drain every live decoration into the off-band queue so a resize reflow can re-anchor the
    /// renderable ones by exact source equality. Records with no raster (pending or failed) carry
    /// nothing to preserve and count as ordinary invalidations.
    fn retain_live_decorations_offscreen(&mut self) {
        let mut dropped = 0_u64;
        for (_, record) in std::mem::take(&mut self.live_decorations) {
            if record.artifact.is_some() || record.stale_artifact.is_some() {
                self.retain_offscreen_record(record);
            } else {
                dropped = dropped.saturating_add(1);
            }
        }
        self.live_invalidation_count = self.live_invalidation_count.saturating_add(dropped);
    }

    fn restore_offscreen_decorations(&mut self) {
        if self.offscreen_decorations.is_empty() {
            return;
        }
        let inputs = self.live_detection_context();
        let initial_context = self.live_initial_detection_context(&inputs);
        let prefixes = live_grid_parser_prefixes(&inputs, initial_context.clone());
        let mut occupied = self
            .live_decorations
            .values()
            .flat_map(|record| record.band_start_row..=record.band_end_row)
            .collect::<BTreeSet<_>>();
        let mut remaining = VecDeque::new();
        let mut relayout_tasks = Vec::new();
        while let Some(mut record) = self.offscreen_decorations.pop_front() {
            let Some((start, end, segments)) =
                exact_live_source_match(&record.span.original_source, &inputs, &occupied)
            else {
                remaining.push_back(record);
                continue;
            };
            if prefixes
                .get(&start.row)
                .is_some_and(DetectionContext::is_commonmark_code)
            {
                remaining.push_back(record);
                continue;
            }
            let Some(logical_band_start) =
                i64::from(start.row).checked_sub(i64::from(record.identity.source_start_offset))
            else {
                remaining.push_back(record);
                continue;
            };
            let Some(logical_band_end) = logical_band_start
                .checked_add(i64::from(record.identity.band_rows.saturating_sub(1)))
            else {
                remaining.push_back(record);
                continue;
            };
            let Ok(band_start_row) = u32::try_from(logical_band_start) else {
                remaining.push_back(record);
                continue;
            };
            let Ok(band_end_row) = u32::try_from(logical_band_end) else {
                remaining.push_back(record);
                continue;
            };
            record.start = start;
            record.end = end;
            record.band_start_row = band_start_row;
            record.band_end_row = band_end_row;
            record.clipped_top_rows = 0;
            record.clipped_bottom_rows = 0;
            record.placement.logical_band_start = logical_band_start;
            record.placement.occluded_source_rows = 0;
            record.placement.occluded_visible_rows.clear();
            record.generation = self.grid_generation;
            record.detection_revision = self.detection_revision;
            if record.rendered_layout != self.layout_key
                && let Some(artifact) = record.artifact.take()
            {
                record.stale_artifact = Some(StaleArtifact {
                    artifact,
                    rendered_layout: record.rendered_layout,
                });
            }
            record.layout = self.layout_key;
            record.initial_context = initial_context.clone();
            record.inputs = Arc::clone(&inputs);
            record.span = record.identity.span.clone();
            record.span.cell_segments = segments;
            if record.artifact.is_none() && record.stale_artifact.is_some() {
                relayout_tasks.push(LiveDetectionTask {
                    candidate_row: record.end.row,
                    screen: record.screen,
                    grid_generation: record.generation,
                    detection_revision: record.detection_revision,
                    layout: record.layout,
                    cell_width_subpixels: self.cell_width_subpixels.get(),
                    cell_height_subpixels: self.cell_height_subpixels.get(),
                    ascii_baseline_subpixels: self
                        .ascii_baseline_subpixels
                        .map_or(0, NonZeroI64::get),
                    options: self.detection_options(),
                    initial_context: record.initial_context.clone(),
                    inputs: Arc::clone(&record.inputs),
                    start: record.start,
                    end: record.end,
                    band_start_row: record.band_start_row,
                    band_end_row: record.band_end_row,
                    span: record.span.clone(),
                    detection_complete: true,
                    resolved: true,
                });
            }
            occupied.extend(record.band_start_row..=record.band_end_row);
            self.live_decorations.insert(record.start.row, record);
        }
        self.offscreen_decorations = remaining;
        for task in relayout_tasks {
            self.enqueue_live_task(task);
        }
    }

    fn observe_live_damage(&mut self, damage: TerminalDamage, observed_at: Instant) {
        let screen = if self.terminal.modes().alternate_screen {
            ScreenId::Alternate
        } else {
            ScreenId::Primary
        };
        if screen != self.live_screen {
            self.invalidate_all_live_decorations();
            // A screen switch is a hard boundary: never carry the previous screen's off-band
            // preservation queue across it, even if a resize transaction on the old screen kept
            // `invalidate_all_live_decorations` from clearing it.
            self.offscreen_decorations.clear();
            self.alternate_content_end_row = None;
            self.pending_live_handoffs.clear();
            self.live_screen = screen;
            self.live_tasks.clear();
            for row in &mut self.live_rows {
                *row = LiveRowStability::default();
            }
        }
        let damaged = match damage {
            TerminalDamage::Full => (0..self.live_rows.len() as u32).collect::<Vec<_>>(),
            TerminalDamage::Rows(rows) => rows,
        };
        for row in damaged {
            let needs_fingerprint = self
                .live_decorations
                .values()
                .any(|record| record.band_start_row <= row && row <= record.band_end_row);
            let content_fingerprint = needs_fingerprint
                .then(|| self.terminal.visible_row_fingerprint(row))
                .flatten();
            let Some(state) = self.live_rows.get_mut(row as usize) else {
                continue;
            };
            if needs_fingerprint && state.content_fingerprint == content_fingerprint {
                continue;
            }
            state.content_fingerprint = content_fingerprint;
            state.revision = state.revision.wrapping_add(1);
            state.last_damage_at = Some(observed_at);
            state.settled_revision = None;
            state.candidate_signature = None;
            if !(screen == ScreenId::Alternate && self.alternate_repaint_in_progress) {
                self.invalidate_live_row(row);
            }
        }
    }

    fn invalidate_live_row(&mut self, row: u32) {
        let removed = self
            .live_decorations
            .iter()
            .filter(|(_, record)| record.band_start_row <= row && row <= record.band_end_row)
            .map(|(start, _)| *start)
            .collect::<Vec<_>>();
        let mut invalidated = 0_u64;
        for start in removed {
            let Some(record) = self.live_decorations.remove(&start) else {
                continue;
            };
            if self.offscreen_preservation_active()
                && (record.artifact.is_some() || record.stale_artifact.is_some())
            {
                self.retain_offscreen_record(record);
            } else {
                invalidated = invalidated.saturating_add(1);
            }
        }
        self.live_invalidation_count = self.live_invalidation_count.saturating_add(invalidated);
        if invalidated != 0 && std::env::var_os("BT_PERF_TRACE").is_some() {
            eprintln!(
                "BT_PERF_TRACE live_math_event=invalidate live_math_detect={} live_math_invalidations={}",
                self.live_detection_count, self.live_invalidation_count
            );
        }
    }

    fn invalidate_all_live_decorations(&mut self) {
        if self.primary_resize_preservation_active() {
            // During a primary resize transaction a wipe (reflow, reflow-capture into history, or a
            // synchronized-update commit) must not flash proven formulas back to source. Move the
            // renderable ones off-band so `restore_offscreen_decorations` can re-anchor them by
            // exact source equality; the off-band queue itself is left intact.
            self.retain_live_decorations_offscreen();
            return;
        }
        let removed = self.live_decorations.len();
        self.live_invalidation_count = self.live_invalidation_count.saturating_add(removed as u64);
        self.live_decorations.clear();
        if !(self.live_screen == ScreenId::Alternate && self.alternate_repaint_in_progress) {
            self.offscreen_decorations.clear();
        }
        if removed != 0 && std::env::var_os("BT_PERF_TRACE").is_some() {
            eprintln!(
                "BT_PERF_TRACE live_math_event=invalidate-all live_math_detect={} live_math_invalidations={}",
                self.live_detection_count, self.live_invalidation_count
            );
        }
    }

    fn enqueue_live_task(&mut self, task: LiveDetectionTask) {
        if let Some(index) = self
            .live_tasks
            .iter()
            .position(|queued| queued.candidate_row == task.candidate_row)
        {
            self.live_tasks.remove(index);
        }
        if self.live_tasks.len() == WORKER_QUEUE_CAP {
            self.live_tasks.pop_front();
        }
        self.live_tasks.push_back(task);
    }

    pub fn run_workers(&mut self) {
        loop {
            while let Some(task) = self.take_math_worker_task() {
                match task {
                    SessionMathTask::Frozen(task) => {
                        self.complete_worker_task(task);
                    }
                    SessionMathTask::Live(mut task) => {
                        if resolve_live_detection_task(&mut task) {
                            let artifact = live_placeholder(&task);
                            let occupied = self.occupied_live_band_rows(task.start.row);
                            size_resolved_live_task_band(
                                &mut task,
                                math_presentation_height_subpixels(
                                    artifact.height_subpixels,
                                    self.math_vertical_padding_subpixels(),
                                ),
                                self.cell_height_subpixels.get(),
                                &occupied,
                            );
                            self.apply_live_worker_completion(task, Some(artifact), None);
                        }
                    }
                }
            }
            if !self.scheduler.has_retry() {
                break;
            }
            self.schedule_retry_artifacts();
            if self.scheduler.pending_len() == 0 {
                break;
            }
        }
    }

    pub fn take_worker_task(&mut self) -> Option<DetectionTask> {
        self.scheduler.take()
    }

    pub fn take_live_worker_task(&mut self) -> Option<LiveDetectionTask> {
        self.live_tasks.pop_front()
    }

    pub fn take_math_worker_task(&mut self) -> Option<SessionMathTask> {
        self.take_worker_task()
            .map(SessionMathTask::Frozen)
            .or_else(|| self.take_live_worker_task().map(SessionMathTask::Live))
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
        let accepted = self.apply_worker_completion(task, Some(placeholder), None);
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
        let render_error = result.as_ref().err().cloned();
        let failure_reason = render_error
            .as_ref()
            .and_then(|error| error.failure_stage().map(|_| error.to_string()));
        let render_time = result.as_ref().ok().map(|raster| raster.render_time);
        let artifact = result
            .ok()
            .map(|raster| artifact_from_raster(&task, raster));
        let accepted = self.apply_worker_completion(task.clone(), artifact, failure_reason);
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
            } else if let Some(error) = render_error.as_ref() {
                eprintln!(
                    "BT_PERF_TRACE math_render_failed source={} error={error:?}",
                    task.transcript_id.0,
                );
            }
        }
        if accepted && let Some(error) = render_error.as_ref() {
            self.record_math_failure(error);
        }
        accepted
    }

    pub fn complete_live_worker_result(
        &mut self,
        mut task: LiveDetectionTask,
        result: Result<MathRaster, MathRenderError>,
    ) -> bool {
        let render_time = result.as_ref().ok().map(|raster| raster.render_time);
        let render_error = result.as_ref().err().cloned();
        let failure_reason = render_error
            .as_ref()
            .and_then(|error| error.failure_stage().map(|_| error.to_string()));
        let artifact = result
            .ok()
            .map(|raster| artifact_from_live_raster(&task, raster));
        if task.resolved
            && let Some(artifact) = artifact.as_ref()
        {
            let occupied = self.occupied_live_band_rows(task.start.row);
            size_resolved_live_task_band(
                &mut task,
                math_presentation_height_subpixels(
                    artifact.height_subpixels,
                    self.math_vertical_padding_subpixels(),
                ),
                self.cell_height_subpixels.get(),
                &occupied,
            );
        } else {
            task.band_start_row = task.start.row;
            task.band_end_row = task.end.row;
        }
        let accepted = self.apply_live_worker_completion(task.clone(), artifact, failure_reason);
        if !accepted {
            self.stale_results = self.stale_results.saturating_add(1);
        } else if std::env::var_os("BT_PERF_TRACE").is_some()
            && let Some(elapsed) = render_time
        {
            eprintln!(
                "BT_PERF_TRACE live_math_render_us={} row={} resident_bytes={}",
                elapsed.as_micros(),
                task.start.row,
                self.math_resident_bytes(),
            );
        }
        if accepted && let Some(error) = render_error.as_ref() {
            self.record_math_failure(error);
        }
        accepted
    }

    fn record_math_failure(&mut self, error: &MathRenderError) {
        match error.failure_stage() {
            Some(MathFailureStage::Validate) => {
                self.math_failure_validate_count =
                    self.math_failure_validate_count.saturating_add(1);
            }
            Some(MathFailureStage::Convert) => {
                self.math_failure_convert_count = self.math_failure_convert_count.saturating_add(1);
            }
            Some(MathFailureStage::Compile) => {
                self.math_failure_compile_count = self.math_failure_compile_count.saturating_add(1);
            }
            None => return,
        }
        if std::env::var_os("BT_PERF_TRACE").is_some() {
            eprintln!(
                "BT_PERF_TRACE math_failures_validate={} math_failures_convert={} math_failures_compile={}",
                self.math_failure_validate_count,
                self.math_failure_convert_count,
                self.math_failure_compile_count,
            );
        }
    }

    fn occupied_live_band_rows(&self, replacing_start: u32) -> BTreeSet<u32> {
        self.live_decorations
            .values()
            .filter(|record| record.start.row != replacing_start)
            .flat_map(|record| record.band_start_row..=record.band_end_row)
            .collect()
    }

    fn apply_live_worker_completion(
        &mut self,
        task: LiveDetectionTask,
        artifact: Option<PlaceholderArtifact>,
        failure_reason: Option<String>,
    ) -> bool {
        if task.screen != self.live_screen
            || task.grid_generation != self.grid_generation
            || task.detection_revision != self.detection_revision
            || task.layout != self.layout_key
        {
            return false;
        }
        // Only the source rows and borrowed row band are byte/revision dependencies. The rest of
        // the 1,024-line detector snapshot is semantic context: rerunning detection below catches
        // fence/delimiter state changes without rejecting ordinary spinner or status-line churn.
        let current_inputs = self.live_detection_context();
        if !live_task_is_current(&task, current_inputs) {
            return false;
        }
        if !task.resolved {
            self.live_decorations.retain(|_, record| {
                !(record.start.row <= task.candidate_row && task.candidate_row <= record.end.row)
            });
            return true;
        }
        if artifact.is_none() && failure_reason.is_none() {
            self.live_decorations.remove(&task.start.row);
            return true;
        }
        let preference_key = MathSourcePreferenceKey::from_span(&task.span);
        let show_source = self
            .math_source_preferences
            .get(&preference_key)
            .copied()
            .unwrap_or(false);
        let remembered = self
            .live_decorations
            .get(&task.start.row)
            .filter(|record| MathSourcePreferenceKey::from_span(&record.span) == preference_key)
            .map(|record| {
                (
                    record.hovered,
                    record.horizontal_scroll_px,
                    record.vertical_scroll_px,
                )
            });
        self.live_decorations
            .retain(|_, record| record.end.row < task.start.row || record.start.row > task.end.row);
        let (hovered, horizontal_scroll_px, vertical_scroll_px) =
            remembered.unwrap_or((false, 0, 0));
        let occurrence_id = LiveMathOccurrenceId(self.next_live_occurrence_id);
        let Some(identity) = proven_live_occurrence(&task, occurrence_id) else {
            return false;
        };
        let frozen_prefix = frozen_prefix_ids(&task.span);
        self.next_live_occurrence_id = self.next_live_occurrence_id.saturating_add(1);
        // Install summaries only for the rows whose pixels are about to suppress source. Ordinary
        // TUI rows stay O(damaged rows), while a same-content repaint of this local dependency band
        // can retain the artifact without cloning any cell Strings.
        for row in task.band_start_row..=task.band_end_row {
            if let Some(state) = self.live_rows.get_mut(row as usize) {
                state.content_fingerprint = self.terminal.visible_row_fingerprint(row);
            }
        }
        self.live_decorations.insert(
            task.start.row,
            LiveDecorationRecord {
                identity,
                placement: LiveOccurrencePlacement {
                    logical_band_start: i64::from(task.band_start_row),
                    occluded_source_rows: 0,
                    occluded_visible_rows: Vec::new(),
                },
                screen: task.screen,
                generation: task.grid_generation,
                start: task.start,
                end: task.end,
                band_start_row: task.band_start_row,
                band_end_row: task.band_end_row,
                frozen_prefix,
                clipped_top_rows: 0,
                clipped_bottom_rows: 0,
                detection_revision: task.detection_revision,
                layout: task.layout,
                rendered_layout: task.layout,
                initial_context: task.initial_context.clone(),
                inputs: Arc::clone(&task.inputs),
                span: task.span,
                artifact,
                stale_artifact: None,
                show_source,
                hovered,
                horizontal_scroll_px,
                vertical_scroll_px,
                failure_reason,
            },
        );
        true
    }

    fn apply_worker_completion(
        &mut self,
        task: DetectionTask,
        artifact: Option<PlaceholderArtifact>,
        failure_reason: Option<String>,
    ) -> bool {
        if !self.worker_task_is_current(&task) {
            return false;
        }
        if !task.resolved {
            return self
                .decorations
                .get_mut(&task.candidate_id)
                .is_some_and(|record| record.fail(&task, None));
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
        let preference_key = MathSourcePreferenceKey::from_span(&task.span);
        let show_source = self
            .math_source_preferences
            .get(&preference_key)
            .copied()
            .unwrap_or(false);
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
        let applied = match artifact {
            Some(artifact) => {
                self.document.set_decoration(
                    task.transcript_id,
                    DecorationIntent::Math {
                        byte_start: task.span.byte_start,
                        byte_end: task.span.byte_end,
                        mode: task.span.mode,
                        detection_revision: task.versions.detection,
                    },
                );
                record.complete(&resolved_task, artifact)
            }
            None => {
                self.document
                    .set_decoration(task.transcript_id, DecorationIntent::Plain);
                record.fail(&resolved_task, failure_reason)
            }
        };
        if applied {
            record.show_source = show_source;
        }
        applied
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
        let frozen = self
            .decorations
            .values()
            .map(|record| {
                record
                    .artifact
                    .as_ref()
                    .map_or(0, |artifact| artifact.rgba.len())
                    + record
                        .stale_artifact
                        .as_ref()
                        .map_or(0, |stale| stale.artifact.rgba.len())
            })
            .sum::<usize>();
        frozen
            + self
                .live_decorations
                .values()
                .map(|record| {
                    record
                        .artifact
                        .as_ref()
                        .map_or(0, |artifact| artifact.rgba.len())
                        + record
                            .stale_artifact
                            .as_ref()
                            .map_or(0, |stale| stale.artifact.rgba.len())
                })
                .sum::<usize>()
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
        self.invalidate_all_live_decorations();
        self.pending_live_handoffs.clear();
        self.live_tasks.clear();
        for row in &mut self.live_rows {
            row.candidate_signature = None;
            row.settled_revision = None;
        }
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
        // Live worker completions can arrive between document relayouts. Synchronize transient
        // artifacts at the frame boundary so a ready raster cannot remain stranded in session
        // state merely because history projection did not otherwise change.
        self.sync_live_projection_artifacts(projection);
        let (_, rows) = self.terminal.dimensions();
        let visible_rows = (0..rows.get())
            .filter_map(|row| self.terminal.visible_row(row))
            .collect::<Vec<_>>();
        let cursor = self.terminal.cursor();
        let staged = self.transcript.staged_rows().cloned().collect::<Vec<_>>();
        let terminal_modes = self.terminal.modes();
        let mut frame = projection.continuous_frame(
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
        )?;
        self.decorate_math_frame(&mut frame);
        frame
            .validate_shape()
            .map_err(FrameProjectionError::FrameShape)?;
        Ok(frame)
    }

    /// Schedule frozen math candidates intersecting the published viewport. The frame is the
    /// visibility oracle: one history anchor is sampled per visual row, then scans are expanded
    /// only to a delimiter opener and an 8 KiB look-ahead. Work is therefore bounded by visible
    /// rows plus the detector's maximum admissible source, independent of transcript length.
    pub fn schedule_visible_artifacts(&mut self, frame: &ViewportFrame) -> usize {
        if self.primary_parked || !self.resize_epoch.decorations_allowed() {
            return 0;
        }
        let columns = frame.columns.get() as usize;
        if columns == 0 {
            return 0;
        }
        let visible = frame
            .cell_anchors
            .chunks(columns)
            .filter_map(|row| match &row.first()?.start {
                ContentAnchor::History { id, .. } => Some(*id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if visible.is_empty() {
            return 0;
        }

        let mut candidates = visible
            .iter()
            .filter(|id| {
                self.document
                    .entries()
                    .get(id)
                    .is_some_and(|entry| may_contain_display_math(&entry.line.text))
            })
            .copied()
            .collect::<BTreeSet<_>>();
        let last_visible = *visible.last().expect("non-empty visible history set");
        let mut lookahead_bytes = 0usize;
        for (id, entry) in self.document.entries().range(last_visible..) {
            lookahead_bytes = lookahead_bytes
                .saturating_add(entry.line.text.len())
                .saturating_add(1);
            if lookahead_bytes > MAX_MATH_SOURCE_BYTES {
                break;
            }
            let overlaps_visible = self
                .frozen_detection_contexts
                .get(id)
                .and_then(|context| context.required_start(*id))
                .is_some_and(|start| start <= last_visible);
            if overlaps_visible && may_contain_display_math(&entry.line.text) {
                candidates.insert(*id);
            }
        }

        let before = self.frozen_detection_count;
        for id in candidates {
            self.schedule_scan(id);
        }
        self.frozen_detection_count.saturating_sub(before) as usize
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

    pub fn math_source(&self, anchor: &MathBlockAnchor) -> Option<&str> {
        match anchor {
            MathBlockAnchor::History { start, end } => self
                .decorations
                .get(start)
                .filter(|record| record.block_end == Some(*end))
                .and_then(|record| record.span.as_ref())
                .map(|span| span.original_source.as_str()),
            MathBlockAnchor::Live {
                screen,
                start,
                end,
                generation,
                ..
            } => self
                .live_decorations
                .get(&start.row)
                .filter(|record| {
                    record.screen == *screen
                        && record.start == *start
                        && record.end == *end
                        && record.generation == *generation
                })
                .map(|record| record.span.original_source.as_str()),
        }
    }

    pub fn toggle_math_source(&mut self, anchor: &MathBlockAnchor) -> bool {
        let preference = match anchor {
            MathBlockAnchor::History { start, end } => {
                let Some(record) = self
                    .decorations
                    .get_mut(start)
                    .filter(|record| record.block_end == Some(*end))
                else {
                    return false;
                };
                let Some(key) = record.span.as_ref().map(MathSourcePreferenceKey::from_span) else {
                    return false;
                };
                if !record.toggle_source() {
                    return false;
                }
                (key, record.show_source)
            }
            MathBlockAnchor::Live {
                screen,
                start,
                end,
                generation,
                ..
            } => {
                let Some(record) = self.live_decorations.get_mut(&start.row).filter(|record| {
                    record.screen == *screen
                        && record.start == *start
                        && record.end == *end
                        && record.generation == *generation
                }) else {
                    return false;
                };
                record.show_source = !record.show_source;
                record.horizontal_scroll_px = 0;
                record.vertical_scroll_px = 0;
                (
                    MathSourcePreferenceKey::from_span(&record.span),
                    record.show_source,
                )
            }
        };
        self.math_source_preferences
            .insert(preference.0, preference.1);
        true
    }

    pub fn set_math_hover(&mut self, anchor: Option<&MathBlockAnchor>) -> bool {
        let mut changed = false;
        for (start, record) in &mut self.decorations {
            let hovered = anchor.is_some_and(|anchor| {
                matches!(
                    anchor,
                    MathBlockAnchor::History {
                        start: anchor_start,
                        end
                    } if anchor_start == start && record.block_end == Some(*end)
                )
            });
            changed |= record.hovered != hovered;
            record.hovered = hovered;
        }
        for record in self.live_decorations.values_mut() {
            let hovered = anchor.is_some_and(|anchor| {
                matches!(
                    anchor,
                    MathBlockAnchor::Live {
                        screen,
                        start,
                        end,
                        generation,
                        ..
                    } if *screen == record.screen
                        && *start == record.start
                        && *end == record.end
                        && *generation == record.generation
                )
            });
            changed |= record.hovered != hovered;
            record.hovered = hovered;
        }
        changed
    }

    /// Apply block-owned scrolling only when the settings assign that axis to the block. Returning
    /// false deliberately bubbles ordinary terminal scrolling through rendered formulas.
    pub fn scroll_math_block(
        &mut self,
        anchor: &MathBlockAnchor,
        horizontal_delta_px: i32,
        vertical_delta_px: i32,
    ) -> bool {
        let mut options = self.math_layout_options;
        options.block_max_height_px = options.block_max_height_px.and_then(|height| {
            NonZeroU32::new(
                height
                    .get()
                    .saturating_mul(self.layout_key.dpi_milli.get())
                    .checked_div(1000)
                    .unwrap_or(height.get())
                    .max(1),
            )
        });
        let pane_width_px = self
            .cell_width_subpixels
            .get()
            .saturating_mul(i64::from(self.layout_key.width_cells.get()))
            .div_euclid(SUBPIXELS_PER_PX)
            .max(1) as u32;
        let display_left_inset_subpixels = self.display_math_left_inset_subpixels();
        match anchor {
            MathBlockAnchor::History { start, end } => {
                let Some(record) = self
                    .decorations
                    .get_mut(start)
                    .filter(|record| record.block_end == Some(*end) && !record.show_source)
                else {
                    return false;
                };
                let Some((artifact, scale_milli)) = frozen_artifact_and_scale(record) else {
                    return false;
                };
                let artifact_size = (artifact.width_px, artifact.height_px);
                let available_width_px = math_block_available_width_px(
                    pane_width_px,
                    artifact.mode,
                    display_left_inset_subpixels,
                );
                scroll_offsets(
                    &mut record.horizontal_scroll_px,
                    &mut record.vertical_scroll_px,
                    artifact_size,
                    scale_milli,
                    available_width_px,
                    options,
                    horizontal_delta_px,
                    vertical_delta_px,
                )
            }
            MathBlockAnchor::Live {
                screen,
                start,
                end,
                generation,
                ..
            } => {
                let Some(record) = self.live_decorations.get_mut(&start.row).filter(|record| {
                    record.screen == *screen
                        && record.start == *start
                        && record.end == *end
                        && record.generation == *generation
                        && !record.show_source
                }) else {
                    return false;
                };
                let Some((artifact, scale_milli)) =
                    live_artifact_and_scale(record, self.layout_key)
                else {
                    return false;
                };
                let artifact_size = (artifact.width_px, artifact.height_px);
                let available_width_px = math_block_available_width_px(
                    pane_width_px,
                    artifact.mode,
                    display_left_inset_subpixels,
                );
                let live_options = MathLayoutOptions {
                    block_max_height_px: None,
                    ..options
                };
                scroll_offsets(
                    &mut record.horizontal_scroll_px,
                    &mut record.vertical_scroll_px,
                    artifact_size,
                    scale_milli,
                    available_width_px,
                    live_options,
                    horizontal_delta_px,
                    0,
                )
            }
        }
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
        // A vanished review anchor is a resize-driven reflow (Codex clears scrollback then reprints)
        // only while a resize transaction is open; a user clear is not. The projection gates its
        // frame hold on this so a genuine clear still snaps to the (empty) bottom.
        projection.set_resize_reflow_active(self.resize_epoch.is_active());
        projection.set_selection(self.view_selection());
        projection.sync_math_artifacts(self.decorations.iter().filter_map(|(id, record)| {
            (!record.show_source
                && record
                    .span
                    .as_ref()
                    .is_some_and(|span| span.mode == MathMode::Display))
            .then(|| projected_frozen_artifact(record, self.math_vertical_padding_subpixels()))
            .flatten()
            .map(|artifact| (*id, artifact))
        }));
        self.sync_live_projection_artifacts(projection);
        projection.apply_detection_revision(self.detection_revision, &self.document);
        projection.project(&self.document);
    }

    fn sync_live_projection_artifacts(&self, projection: &mut ViewportProjection) {
        projection.sync_live_math_artifacts(
            self.live_screen,
            self.live_decorations.values().filter_map(|record| {
                (!record.show_source && record.span.mode == MathMode::Display)
                    .then(|| {
                        projected_live_artifact(
                            record,
                            self.layout_key,
                            self.math_vertical_padding_subpixels(),
                        )
                    })
                    .flatten()
                    .map(|artifact| ProjectedLiveMathArtifact {
                        occurrence_id: record.identity.occurrence_id,
                        screen: record.screen,
                        start: record.start,
                        end: record.end,
                        band_start_row: record.band_start_row,
                        band_end_row: record.band_end_row,
                        clipped_top_rows: record.clipped_top_rows,
                        clipped_bottom_rows: record.clipped_bottom_rows,
                        occluded_source_rows: record.placement.occluded_source_rows,
                        occluded_visible_rows: record.placement.occluded_visible_rows.clone(),
                        frozen_prefix: record.frozen_prefix.clone(),
                        generation: record.generation,
                        artifact,
                    })
            }),
        );
    }

    fn decorate_math_frame(&self, frame: &mut ViewportFrame) {
        let overflow = if self.math_layout_options.line_wrapping {
            HorizontalOverflowOwner::Block
        } else {
            HorizontalOverflowOwner::Pane
        };
        let block_max = self.math_layout_options.block_max_height_px.map(|height| {
            i64::from(height.get())
                .saturating_mul(i64::from(self.layout_key.dpi_milli.get()))
                .saturating_mul(SUBPIXELS_PER_PX)
                / 1000
        });

        for placement in &mut frame.math_blocks {
            if placement.display == MathBlockDisplay::Rendered
                && placement.artifact.mode == MathMode::Display
            {
                placement.left_subpixels = self.display_math_left_inset_subpixels();
            }
            match &placement.anchor {
                MathBlockAnchor::History { start, .. } => {
                    let Some(record) = self.decorations.get(start) else {
                        continue;
                    };
                    placement.toolbar_visible = record.hovered;
                    placement.horizontal_overflow = overflow;
                    placement.horizontal_scroll_px = if self.math_layout_options.line_wrapping {
                        record.horizontal_scroll_px
                    } else {
                        0
                    };
                    placement.vertical_scroll_px = record.vertical_scroll_px;
                    placement.clip_height_subpixels = block_max
                        .map_or(placement.artifact.height_subpixels, |max| {
                            placement.artifact.height_subpixels.min(max)
                        });
                }
                MathBlockAnchor::Live {
                    screen,
                    start,
                    end,
                    band_start_row,
                    band_end_row,
                    generation,
                } => {
                    let Some(record) = self.live_decorations.get(&start.row).filter(|record| {
                        record.screen == *screen
                            && record.start == *start
                            && record.end == *end
                            && record.band_start_row == *band_start_row
                            && record.band_end_row == *band_end_row
                            && record.generation == *generation
                    }) else {
                        continue;
                    };
                    placement.toolbar_visible = record.hovered;
                    placement.horizontal_overflow = overflow;
                    placement.horizontal_scroll_px = if self.math_layout_options.line_wrapping {
                        record.horizontal_scroll_px
                    } else {
                        0
                    };
                    placement.vertical_scroll_px = record.vertical_scroll_px;
                }
            }
        }

        for (start, record) in &self.decorations {
            if !record.show_source {
                continue;
            }
            let Some(artifact) =
                projected_frozen_artifact(record, self.math_vertical_padding_subpixels())
            else {
                continue;
            };
            let Some(first_row) = frame_row_for_history(frame, *start) else {
                continue;
            };
            let end = record.block_end.unwrap_or(*start);
            let last_row = (first_row..frame.rows.get())
                .take_while(|row| frame_row_history_id(frame, *row).is_some_and(|id| id <= end))
                .last()
                .unwrap_or(first_row);
            let Some(first_mapped) = frame.row_map.get(first_row as usize) else {
                continue;
            };
            let Some(last_mapped) = frame.row_map.get(last_row as usize) else {
                continue;
            };
            frame.math_blocks.push(MathBlockPlacement {
                start: *start,
                anchor: MathBlockAnchor::History { start: *start, end },
                source: record
                    .span
                    .as_ref()
                    .map_or_else(String::new, |span| span.original_source.clone()),
                artifact,
                top_subpixels: first_mapped.top_subpixels,
                left_subpixels: 0,
                content_offset_subpixels: 0,
                clip_height_subpixels: last_mapped
                    .top_subpixels
                    .saturating_add(last_mapped.height_subpixels)
                    .saturating_sub(first_mapped.top_subpixels),
                display: MathBlockDisplay::Source,
                horizontal_overflow: overflow,
                horizontal_scroll_px: 0,
                vertical_scroll_px: 0,
                toolbar_visible: record.hovered,
                occluded_source_rows: 0,
                occluded_visible_rows: Vec::new(),
                live_occurrence_id: None,
                frozen_prefix_rows: 0,
            });
        }

        for record in self.live_decorations.values() {
            if record.screen != self.live_screen || record.generation != self.grid_generation {
                continue;
            }
            if !record.show_source {
                continue;
            }
            let Some(artifact) = projected_live_artifact(
                record,
                self.layout_key,
                self.math_vertical_padding_subpixels(),
            ) else {
                continue;
            };
            let Some((visible_row, _source_row)) = frame_row_for_live_range(
                frame,
                record.screen,
                record.band_start_row,
                record.band_end_row,
            ) else {
                continue;
            };
            let Some(first_mapped) = frame.row_map.get(visible_row as usize) else {
                continue;
            };
            let Some(last_mapped) = frame.row_map.iter().rfind(|row| {
                row.live_grid_row.is_some_and(|live| {
                    (record.band_start_row..=record.band_end_row).contains(&live)
                })
            }) else {
                continue;
            };
            let band_height = last_mapped
                .top_subpixels
                .saturating_add(last_mapped.height_subpixels)
                .saturating_sub(first_mapped.top_subpixels);
            frame.math_blocks.push(MathBlockPlacement {
                start: TranscriptId(0),
                anchor: MathBlockAnchor::Live {
                    screen: record.screen,
                    start: record.start,
                    end: record.end,
                    band_start_row: record.band_start_row,
                    band_end_row: record.band_end_row,
                    generation: record.generation,
                },
                source: record.span.original_source.clone(),
                artifact,
                top_subpixels: first_mapped.top_subpixels,
                left_subpixels: 0,
                content_offset_subpixels: 0,
                clip_height_subpixels: band_height,
                display: MathBlockDisplay::Source,
                horizontal_overflow: overflow,
                horizontal_scroll_px: if self.math_layout_options.line_wrapping {
                    record.horizontal_scroll_px
                } else {
                    0
                },
                vertical_scroll_px: record.vertical_scroll_px,
                toolbar_visible: record.hovered,
                occluded_source_rows: record.placement.occluded_source_rows,
                occluded_visible_rows: record.placement.occluded_visible_rows.clone(),
                live_occurrence_id: Some(record.identity.occurrence_id),
                frozen_prefix_rows: 0,
            });
        }

        for (start, record) in &self.decorations {
            let Some(span) = record
                .span
                .as_ref()
                .filter(|span| span.mode == MathMode::Inline)
            else {
                continue;
            };
            if record.show_source || record.failure_reason.is_some() {
                continue;
            }
            let Some(artifact) =
                projected_frozen_artifact(record, self.math_vertical_padding_subpixels())
            else {
                continue;
            };
            let Some(entry) = self.document.entries().get(start) else {
                continue;
            };
            let Some((row, left_column, cells)) =
                frozen_inline_cells(frame, *start, &entry.line, span)
            else {
                continue;
            };
            let Some((top_subpixels, row_height_subpixels)) = frame
                .row_map
                .get(row as usize)
                .map(|mapped| (mapped.top_subpixels, mapped.height_subpixels))
            else {
                continue;
            };
            for index in cells {
                if let Some(cell) = frame.cells.get_mut(index) {
                    cell.text.clear();
                    cell.wide_spacer = false;
                }
            }
            frame.math_blocks.push(MathBlockPlacement {
                start: *start,
                anchor: MathBlockAnchor::History {
                    start: *start,
                    end: *start,
                },
                source: span.original_source.clone(),
                artifact,
                top_subpixels,
                left_subpixels: i64::from(left_column)
                    .saturating_mul(self.cell_width_subpixels.get()),
                content_offset_subpixels: 0,
                clip_height_subpixels: row_height_subpixels,
                display: MathBlockDisplay::Rendered,
                horizontal_overflow: HorizontalOverflowOwner::Pane,
                horizontal_scroll_px: 0,
                vertical_scroll_px: 0,
                toolbar_visible: false,
                occluded_source_rows: 0,
                occluded_visible_rows: Vec::new(),
                live_occurrence_id: None,
                frozen_prefix_rows: 0,
            });
        }

        for record in self.live_decorations.values() {
            if record.screen != self.live_screen
                || record.generation != self.grid_generation
                || record.span.mode != MathMode::Inline
                || record.show_source
                || record.failure_reason.is_some()
            {
                continue;
            }
            let Some(artifact) = projected_live_artifact(
                record,
                self.layout_key,
                self.math_vertical_padding_subpixels(),
            ) else {
                continue;
            };
            let Some(text) =
                live_grid_input(&record.inputs, record.start.row).map(|input| input.text.as_str())
            else {
                continue;
            };
            let Some((row, left_column, cells)) =
                live_inline_cells(frame, record.start.row, text, &record.span)
            else {
                continue;
            };
            let Some((top_subpixels, row_height_subpixels)) = frame
                .row_map
                .get(row as usize)
                .map(|mapped| (mapped.top_subpixels, mapped.height_subpixels))
            else {
                continue;
            };
            for index in cells {
                if let Some(cell) = frame.cells.get_mut(index) {
                    cell.text.clear();
                    cell.wide_spacer = false;
                }
            }
            frame.math_blocks.push(MathBlockPlacement {
                start: TranscriptId(0),
                anchor: MathBlockAnchor::Live {
                    screen: record.screen,
                    start: record.start,
                    end: record.end,
                    band_start_row: record.start.row,
                    band_end_row: record.end.row,
                    generation: record.generation,
                },
                source: record.span.original_source.clone(),
                artifact,
                top_subpixels,
                left_subpixels: i64::from(left_column)
                    .saturating_mul(self.cell_width_subpixels.get()),
                content_offset_subpixels: 0,
                clip_height_subpixels: row_height_subpixels,
                display: MathBlockDisplay::Rendered,
                horizontal_overflow: HorizontalOverflowOwner::Pane,
                horizontal_scroll_px: 0,
                vertical_scroll_px: 0,
                toolbar_visible: false,
                occluded_source_rows: 0,
                occluded_visible_rows: Vec::new(),
                live_occurrence_id: Some(record.identity.occurrence_id),
                frozen_prefix_rows: 0,
            });
        }

        for (start, record) in &self.decorations {
            let Some(reason) = record.failure_reason.as_ref() else {
                continue;
            };
            let Some(first_row) = frame_row_for_history(frame, *start) else {
                continue;
            };
            let end = record.block_end.unwrap_or(*start);
            let last_row = (first_row..frame.rows.get())
                .take_while(|row| frame_row_history_id(frame, *row).is_some_and(|id| id <= end))
                .last()
                .unwrap_or(first_row);
            let (Some(first), Some(last)) = (
                frame.row_map.get(first_row as usize),
                frame.row_map.get(last_row as usize),
            ) else {
                continue;
            };
            frame.math_failures.push(MathFailurePlacement {
                anchor: MathBlockAnchor::History { start: *start, end },
                top_subpixels: first.top_subpixels,
                height_subpixels: last
                    .top_subpixels
                    .saturating_add(last.height_subpixels)
                    .saturating_sub(first.top_subpixels),
            });
            if record.hovered {
                frame.status_text = Some(format!("Formula not rendered: {reason}"));
            }
        }

        for record in self.live_decorations.values() {
            let Some(reason) = record.failure_reason.as_ref() else {
                continue;
            };
            let Some((first_row, _)) =
                frame_row_for_live_range(frame, record.screen, record.start.row, record.end.row)
            else {
                continue;
            };
            let Some(first) = frame.row_map.get(first_row as usize) else {
                continue;
            };
            let Some(last) = frame.row_map.iter().rfind(|mapped| {
                mapped
                    .live_grid_row
                    .is_some_and(|row| (record.start.row..=record.end.row).contains(&row))
            }) else {
                continue;
            };
            frame.math_failures.push(MathFailurePlacement {
                anchor: MathBlockAnchor::Live {
                    screen: record.screen,
                    start: record.start,
                    end: record.end,
                    band_start_row: record.start.row,
                    band_end_row: record.end.row,
                    generation: record.generation,
                },
                top_subpixels: first.top_subpixels,
                height_subpixels: last
                    .top_subpixels
                    .saturating_add(last.height_subpixels)
                    .saturating_sub(first.top_subpixels),
            });
            if record.hovered {
                frame.status_text = Some(format!("Formula not rendered: {reason}"));
            }
        }
        let mut rendered_rows = BTreeSet::new();
        for placement in frame.math_blocks.iter().filter(|placement| {
            placement.display == MathBlockDisplay::Rendered
                && placement.artifact.mode == MathMode::Display
        }) {
            match placement.anchor {
                MathBlockAnchor::History { start, end } => {
                    rendered_rows.extend((0..frame.rows.get()).filter(|row| {
                        frame_row_history_id(frame, *row).is_some_and(|id| start <= id && id <= end)
                    }));
                }
                MathBlockAnchor::Live {
                    band_start_row,
                    band_end_row,
                    ..
                } => {
                    rendered_rows.extend(frame.row_map.iter().enumerate().filter_map(
                        |(frame_row, mapped)| {
                            mapped
                                .live_grid_row
                                .is_some_and(|live_row| {
                                    (band_start_row..=band_end_row).contains(&live_row)
                                })
                                .then(|| u32::try_from(frame_row).ok())
                                .flatten()
                        },
                    ));
                }
            }
        }
        frame
            .selection_spans
            .retain(|selection| !rendered_rows.contains(&selection.row));
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
            let full_screen_output_scroll = match &event {
                AdapterEvent::RowsRemoved { context, rows }
                    if context.cause == RemovalCause::NormalScroll
                        && context.scope == RemovalScope::FullScreen =>
                {
                    if context.screen == RemovalScreen::Alternate {
                        for removed in rows {
                            advance_detection_context(
                                &mut self.alternate_detection_context,
                                TranscriptId(0),
                                &captured_row_text(&removed.row),
                            );
                        }
                    }
                    Some((context.screen, rows.len()))
                }
                _ => None,
            };
            match classify(event) {
                LifecycleDirective::RowsRemoved { rows } => {
                    if self.resize_epoch.is_active() {
                        self.rebase_vendor_owned_rows(rows);
                    } else {
                        let preserve_full_screen_scroll = matches!(
                            full_screen_output_scroll,
                            Some((RemovalScreen::Primary | RemovalScreen::Alternate, count))
                                if count != 0
                        );
                        self.apply_removed_rows(rows, preserve_full_screen_scroll)?;
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
                    self.invalidate_all_live_decorations();
                    self.pending_live_handoffs.clear();
                    self.live_tasks.clear();
                    self.live_screen = ScreenId::Alternate;
                    self.alternate_detection_context = DetectionContext::default();
                    for row in &mut self.live_rows {
                        *row = LiveRowStability::default();
                    }
                    self.primary_parked = true;
                    self.bump_view_generation();
                }
                LifecycleDirective::RestorePrimary => {
                    self.invalidate_all_live_decorations();
                    self.pending_live_handoffs.clear();
                    self.live_tasks.clear();
                    self.live_screen = ScreenId::Primary;
                    for row in &mut self.live_rows {
                        *row = LiveRowStability::default();
                    }
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
        self.invalidate_all_live_decorations();
        self.pending_live_handoffs.clear();
        self.live_tasks.clear();
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

    fn apply_removed_rows(
        &mut self,
        rows: Vec<RowDirective>,
        preserve_full_screen_scroll: bool,
    ) -> Result<(), SessionError> {
        let mut removals = Vec::new();
        let mut captured = Vec::<CaptureResult>::new();
        let mut captured_staging = BTreeMap::new();
        for row in rows {
            match row {
                RowDirective::Capture { live_row, row } => {
                    let grapheme_offsets = captured_grapheme_offsets(&row);
                    let result = self.transcript.capture(row);
                    captured_staging.insert(live_row, result.staging_id);
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

        self.remember_live_artifact_handoffs(&captured_staging);
        if !preserve_full_screen_scroll {
            self.invalidate_all_live_decorations();
        }
        self.live_tasks.clear();
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
        if preserve_full_screen_scroll {
            self.preserve_live_after_top_scroll(removals.len());
        }
        Ok(())
    }

    fn preserve_live_after_top_scroll(&mut self, removed_rows: usize) {
        if removed_rows == 0 {
            return;
        }
        let shift = removed_rows.min(self.live_rows.len());
        self.live_rows.drain(..shift);
        self.live_rows
            .extend(std::iter::repeat_n(LiveRowStability::default(), shift));
        let shift = u32::try_from(shift).unwrap_or(u32::MAX);
        let inputs = self.live_detection_context();
        let last_row = after_grid_row_count(&inputs).saturating_sub(1);
        let mapping = SegmentedRowMapping {
            content_delta: -i64::from(shift),
            content_start_row: 0,
            content_end_row: last_row,
            fixed_start_row: None,
        };
        let mut preserved = BTreeMap::new();
        let mut invalidated = 0usize;
        for (_, record) in std::mem::take(&mut self.live_decorations) {
            let Some(projected) = project_live_record(
                &record,
                mapping,
                self.grid_generation,
                self.detection_revision,
                self.layout_key,
                record.initial_context.clone(),
                Arc::clone(&inputs),
            ) else {
                invalidated += 1;
                continue;
            };
            match projected {
                RecordProjection::Visible(record) => {
                    preserved.insert(record.start.row, record);
                }
                RecordProjection::Dormant(record) if self.live_screen == ScreenId::Alternate => {
                    self.retain_offscreen_record(record);
                }
                RecordProjection::Dormant(_) => invalidated += 1,
            }
        }
        self.live_invalidation_count = self
            .live_invalidation_count
            .saturating_add(invalidated as u64);
        self.live_decorations = preserved;

        let context_signature = live_detection_context_signature(&inputs);
        for record in self.live_decorations.values_mut() {
            record.inputs = Arc::clone(&inputs);
            if let Some(state) = self.live_rows.get_mut(record.end.row as usize) {
                state.candidate_signature =
                    Some(live_detection_signature(context_signature, record.end.row));
            }
        }
    }

    fn remember_live_artifact_handoffs(&mut self, captured_rows: &BTreeMap<u32, StagingId>) {
        if self.live_screen != ScreenId::Primary {
            return;
        }
        for record in self.live_decorations.values() {
            let Some(candidate_staging) = captured_rows.get(&record.start.row).copied() else {
                continue;
            };
            if record.layout != self.layout_key
                || record.detection_revision != self.detection_revision
            {
                continue;
            }
            let Some(artifact) = record.artifact.as_ref() else {
                continue;
            };
            if self
                .pending_live_handoffs
                .iter()
                .any(|pending| pending.artifact.key == artifact.key)
            {
                continue;
            }
            self.pending_live_handoffs.push(PendingLiveArtifactHandoff {
                span: record.span.clone(),
                artifact: artifact.clone(),
                layout: record.layout,
                detection_revision: record.detection_revision,
                candidate_staging,
                candidate_start: None,
                expected_frozen_lines: u64::from(
                    record.end.row.saturating_sub(record.start.row) + 1,
                ),
            });
        }
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
        for pending in &mut self.pending_live_handoffs {
            if pending.candidate_start.is_none()
                && finalized
                    .mappings
                    .iter()
                    .any(|mapping| mapping.staging_id == pending.candidate_staging)
            {
                pending.candidate_start = Some(id);
            }
        }
        self.document.finalize_transaction(finalized);
        self.schedule_detection(id);
        self.try_handoff_live_artifact(id);
        self.staging_sources
            .retain(|_, source| *source != SourceLifecycle::Frozen);
        if closes_active {
            self.active_staging_tail = None;
        }
        Ok(())
    }

    fn try_handoff_live_artifact(&mut self, closing_id: TranscriptId) {
        // Ordinary finalized output has no live raster to transfer. This branch is the dominant
        // path and must stay independent of the frozen-history quota.
        if self.pending_live_handoffs.is_empty() {
            return;
        }
        let matured = self
            .pending_live_handoffs
            .iter()
            .enumerate()
            .filter_map(|(index, pending)| {
                if pending.layout != self.layout_key
                    || pending.detection_revision != self.detection_revision
                {
                    return None;
                }
                let start = pending.candidate_start?;
                let expected_end = start
                    .0
                    .checked_add(pending.expected_frozen_lines.saturating_sub(1))?;
                (expected_end <= closing_id.0).then_some((index, start, expected_end))
            })
            .next();
        let Some((pending_index, candidate_start, expected_end)) = matured else {
            return;
        };

        // The live detector already proved that this artifact begins at candidate_start and spans
        // exactly expected_frozen_lines detector inputs. Re-run the authoritative detector only
        // over that closed candidate, never over unrelated frozen history. A mismatch expires the
        // handoff; the normal frozen worker remains the source of truth.
        let block = (expected_end == closing_id.0)
            .then(|| {
                detect_math_blocks_with_options(
                    self.document
                        .entries()
                        .range(candidate_start..=closing_id)
                        .map(|(id, entry)| (*id, entry.line.text.as_str())),
                    self.detection_options(),
                )
                .into_iter()
                .find(|block| {
                    block.start == candidate_start
                        && block.end == closing_id
                        && self.pending_live_handoffs[pending_index]
                            .span
                            .render_equivalent(&block.span)
                })
            })
            .flatten();
        let mut pending = self.pending_live_handoffs.remove(pending_index);
        let Some(block) = block else {
            return;
        };
        pending.artifact.block_end = block.end;
        let Some(entry) = self.document.entries().get(&block.start) else {
            return;
        };
        let versions = VersionStamp {
            source: entry.line.source_generation,
            detection: self.detection_revision,
            layout: self.layout_key,
            view: self.view_generation,
        };
        let record = self
            .decorations
            .entry(block.start)
            .or_insert_with(|| DecorationRecord::frozen(versions));
        record.source = SourceLifecycle::Frozen;
        record.decoration = DecorationLifecycle::Ready;
        record.versions = versions;
        record.artifact = Some(pending.artifact);
        record.stale_artifact = None;
        record.block_end = Some(block.end);
        record.span = Some(block.span.clone());
        self.document.set_decoration(
            block.start,
            DecorationIntent::Math {
                byte_start: block.span.byte_start,
                byte_end: block.span.byte_end,
                mode: block.span.mode,
                detection_revision: self.detection_revision,
            },
        );
        if closing_id != block.start
            && let Some(candidate) = self.decorations.get_mut(&closing_id)
        {
            candidate.decoration = DecorationLifecycle::None;
        }
        self.scheduler.remove_sources(&BTreeSet::from([closing_id]));
    }

    fn schedule_detection(&mut self, id: TranscriptId) {
        let Some(entry) = self.document.entries().get(&id) else {
            return;
        };
        let may_contain_math = may_contain_display_math(&entry.line.text);
        let versions = VersionStamp {
            source: entry.line.source_generation,
            detection: self.detection_revision,
            layout: self.layout_key,
            view: self.view_generation,
        };
        self.frozen_detection_contexts
            .insert(id, self.frozen_detection_context.clone());
        advance_detection_context(&mut self.frozen_detection_context, id, &entry.line.text);
        self.decorations
            .insert(id, DecorationRecord::frozen(versions));
        // Ordinary frozen lines take only the allocation-free delimiter prefilter. A candidate
        // snapshots immutable source here; the worker owns fence/pairing/escape/size detection.
        if may_contain_math && !self.primary_parked && self.resize_epoch.decorations_allowed() {
            self.schedule_scan(id);
        }
    }

    fn delete_history(&mut self, removed: &[TranscriptId], clear_staging: bool) {
        self.document
            .delete_transaction(removed, clear_staging, self.grid_generation);
        if clear_staging {
            self.staging_sources.clear();
            self.active_staging_tail = None;
            self.pending_live_handoffs.clear();
            self.frozen_detection_context = DetectionContext::default();
            self.frozen_detection_contexts.clear();
        }
        let removed_set = removed.iter().copied().collect::<BTreeSet<_>>();
        self.scheduler.remove_sources(&removed_set);
        for id in removed {
            self.decorations.remove(id);
            self.frozen_detection_contexts.remove(id);
        }
    }

    fn invalidate_layout(&mut self) {
        self.pending_live_handoffs.clear();
        let detection_options = self.detection_options();
        for record in self.decorations.values_mut() {
            record.layout_changed(self.layout_key);
        }
        let mut live_relayouts = Vec::new();
        for record in self.live_decorations.values_mut() {
            if record.layout == self.layout_key {
                continue;
            }
            let rendered_layout = record.rendered_layout;
            if let Some(artifact) = record.artifact.take() {
                record.stale_artifact = Some(StaleArtifact {
                    artifact,
                    rendered_layout,
                });
            }
            record.layout = self.layout_key;
            record.generation = self.grid_generation;
            live_relayouts.push(LiveDetectionTask {
                candidate_row: record.end.row,
                screen: record.screen,
                grid_generation: record.generation,
                detection_revision: record.detection_revision,
                layout: self.layout_key,
                cell_width_subpixels: self.cell_width_subpixels.get(),
                cell_height_subpixels: self.cell_height_subpixels.get(),
                ascii_baseline_subpixels: self.ascii_baseline_subpixels.map_or(0, NonZeroI64::get),
                options: detection_options,
                initial_context: record.initial_context.clone(),
                inputs: Arc::clone(&record.inputs),
                start: record.start,
                end: record.end,
                band_start_row: record.band_start_row,
                band_end_row: record.band_end_row,
                span: record.span.clone(),
                detection_complete: true,
                resolved: true,
            });
        }
        for record in &mut self.offscreen_decorations {
            if record.layout == self.layout_key {
                continue;
            }
            let rendered_layout = record.rendered_layout;
            if let Some(artifact) = record.artifact.take() {
                record.stale_artifact = Some(StaleArtifact {
                    artifact,
                    rendered_layout,
                });
            }
            record.layout = self.layout_key;
            record.generation = self.grid_generation;
        }
        for task in live_relayouts {
            self.enqueue_live_task(task);
        }
        // Relayout tasks carry the current grid generation and a stale raster bridges the worker
        // interval. Clearing content signatures still provides a retry path if a queued task is
        // dropped: resize changed layout, so unchanged text must not suppress the fresh raster.
        for row in &mut self.live_rows {
            row.candidate_signature = None;
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
            .filter_map(|(id, entry)| may_contain_display_math(&entry.line.text).then_some(*id))
            .collect::<Vec<_>>();
        for id in candidates {
            self.schedule_scan(id);
        }
    }

    fn schedule_retry_artifacts(&mut self) {
        if self.primary_parked || !self.resize_epoch.decorations_allowed() {
            return;
        }
        for id in self.scheduler.retry_sources(WORKER_QUEUE_CAP) {
            self.schedule_scan(id);
        }
    }

    fn schedule_scan(&mut self, candidate_id: TranscriptId) {
        let detection_options = self.detection_options();
        let Some(candidate_context) = self.frozen_detection_contexts.get(&candidate_id) else {
            return;
        };
        let required_start = candidate_context.required_start(candidate_id);
        let mut initial_context = candidate_context.clone();
        let mut inputs = Vec::new();
        if let Some(start) = required_start {
            initial_context = self
                .frozen_detection_contexts
                .get(&start)
                .cloned()
                .unwrap_or_else(|| candidate_context.clone());
            let mut source_bytes = 0usize;
            for (id, entry) in self.document.entries().range(start..=candidate_id) {
                source_bytes = source_bytes
                    .saturating_add(entry.line.text.len())
                    .saturating_add(1);
                if source_bytes > MAX_MATH_SOURCE_BYTES.saturating_add(1) {
                    inputs.clear();
                    initial_context = candidate_context.clone();
                    break;
                }
                inputs.push(DetectionInput {
                    id: *id,
                    text: entry.line.text.clone(),
                    cell_boundaries: frozen_cell_boundaries(&entry.line),
                });
            }
            if inputs.first().is_none_or(|input| input.id != start)
                || inputs.last().is_none_or(|input| input.id != candidate_id)
            {
                inputs.clear();
                initial_context = candidate_context.clone();
            }
        }
        if inputs.is_empty()
            && let Some(entry) = self.document.entries().get(&candidate_id)
        {
            inputs.push(DetectionInput {
                id: candidate_id,
                text: entry.line.text.clone(),
                cell_boundaries: frozen_cell_boundaries(&entry.line),
            });
        }
        let Some(mut task) = self.decorations.get_mut(&candidate_id).and_then(|record| {
            record.schedule_scan(
                candidate_id,
                initial_context,
                Arc::from(inputs),
                detection_options,
            )
        }) else {
            return;
        };
        task.cell_width_subpixels = self.cell_width_subpixels.get();
        task.cell_height_subpixels = self.cell_height_subpixels.get();
        task.ascii_baseline_subpixels = self.ascii_baseline_subpixels.map_or(0, NonZeroI64::get);
        self.frozen_detection_count = self.frozen_detection_count.saturating_add(1);
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
    let line = task
        .inputs
        .iter()
        .find(|input| input.id == task.transcript_id)
        .map_or("", |input| input.text.as_str());
    render_task_math(
        engine,
        &task.span,
        line,
        task.cell_width_subpixels,
        task.cell_height_subpixels,
        task.ascii_baseline_subpixels,
        MathRenderKey {
            dpi_milli: task.versions.layout.dpi_milli,
            font_milli_pt: NonZeroU32::new(12_000).expect("12 pt is non-zero"),
            foreground_rgb,
            mode: task.span.mode,
        },
    )
}

pub fn render_live_detection_task(
    engine: &MathEngine,
    task: &mut LiveDetectionTask,
    foreground_rgb: [u8; 3],
) -> Result<MathRaster, MathRenderError> {
    if !resolve_live_detection_task(task) {
        return Err(MathRenderError::NotDetected);
    }
    if task.screen == ScreenId::Primary {
        extend_live_task_band(task);
    } else {
        task.band_start_row = task.start.row;
        task.band_end_row = task.end.row;
    }
    let line =
        live_grid_input(&task.inputs, task.start.row).map_or("", |input| input.text.as_str());
    render_task_math(
        engine,
        &task.span,
        line,
        task.cell_width_subpixels,
        task.cell_height_subpixels,
        task.ascii_baseline_subpixels,
        MathRenderKey {
            dpi_milli: task.layout.dpi_milli,
            font_milli_pt: NonZeroU32::new(12_000).expect("12 pt is non-zero"),
            foreground_rgb,
            mode: task.span.mode,
        },
    )
}

fn render_task_math(
    engine: &MathEngine,
    span: &MathSpan,
    line: &str,
    cell_width_subpixels: i64,
    cell_height_subpixels: i64,
    ascii_baseline_subpixels: i64,
    key: MathRenderKey,
) -> Result<MathRaster, MathRenderError> {
    if span.mode == MathMode::Display {
        return engine.render(&span.render_source, key);
    }
    if ascii_baseline_subpixels <= 0 {
        // Inline placement is baseline-anchored. Without the renderer's measured ASCII baseline,
        // retaining source is the only geometry-safe outcome.
        return Err(MathRenderError::InlineGeometry);
    }
    let Some(first) = span.inline_runs.first() else {
        return Err(MathRenderError::NotDetected);
    };
    let first_byte =
        usize::try_from(first.byte_start).map_err(|_| MathRenderError::InlineGeometry)?;
    let Some(prefix) = line.get(..first_byte) else {
        return Err(MathRenderError::InlineGeometry);
    };
    let base_column = UnicodeWidthStr::width(prefix);
    let cell_width_px = (cell_width_subpixels.max(1) as f32 / SUBPIXELS_PER_PX as f32).max(1.0);
    let terminal_baseline_subpixels =
        ascii_baseline_subpixels.clamp(1, cell_height_subpixels.max(1));
    let terminal_descent_subpixels = cell_height_subpixels
        .max(1)
        .saturating_sub(terminal_baseline_subpixels);
    let mut rendered = Vec::with_capacity(span.inline_runs.len());
    let mut baseline_px = 0_u32;
    let mut render_time = Duration::ZERO;
    for run in &span.inline_runs {
        let start = usize::try_from(run.byte_start).map_err(|_| MathRenderError::InlineGeometry)?;
        let end = usize::try_from(run.byte_end).map_err(|_| MathRenderError::InlineGeometry)?;
        let (Some(before), Some(delimited)) = (line.get(..start), line.get(start..end)) else {
            return Err(MathRenderError::InlineGeometry);
        };
        let column = UnicodeWidthStr::width(before).saturating_sub(base_column);
        let available_px =
            (UnicodeWidthStr::width(delimited) as f32 * cell_width_px).floor() as u32;
        let raster = engine.render(&run.source, key)?;
        if !baseline_box_fits(
            raster.height_px,
            raster.baseline_px,
            terminal_baseline_subpixels,
            terminal_descent_subpixels,
        ) || raster.width_px > available_px.max(1)
        {
            return Err(MathRenderError::InlineGeometry);
        }
        baseline_px = baseline_px.max(raster.baseline_px.ceil().max(0.0) as u32);
        render_time = render_time.saturating_add(raster.render_time);
        let x = (column as f32 * cell_width_px).round().max(0.0) as u32;
        rendered.push((x, raster));
    }
    let width_px = rendered
        .iter()
        .map(|(x, raster)| x.saturating_add(raster.width_px))
        .max()
        .ok_or(MathRenderError::InlineGeometry)?;
    let height_px = rendered
        .iter()
        .map(|(_, raster)| {
            baseline_px
                .saturating_sub(raster.baseline_px.ceil().max(0.0) as u32)
                .saturating_add(raster.height_px)
        })
        .max()
        .ok_or(MathRenderError::InlineGeometry)?;
    if !baseline_box_fits(
        height_px,
        baseline_px as f32,
        terminal_baseline_subpixels,
        terminal_descent_subpixels,
    ) {
        return Err(MathRenderError::InlineGeometry);
    }
    let len = (width_px as usize)
        .checked_mul(height_px as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(MathRenderError::InvalidDimensions)?;
    let mut rgba = vec![0_u8; len];
    for (x, raster) in rendered {
        let y = baseline_px.saturating_sub(raster.baseline_px.ceil().max(0.0) as u32);
        for row in 0..raster.height_px {
            let source_start = row as usize * raster.width_px as usize * 4;
            let source_end = source_start + raster.width_px as usize * 4;
            let target_start = ((y + row) as usize * width_px as usize + x as usize) * 4;
            let target_end = target_start + raster.width_px as usize * 4;
            rgba[target_start..target_end].copy_from_slice(&raster.rgba[source_start..source_end]);
        }
    }
    Ok(MathRaster {
        rgba,
        width_px,
        height_px,
        content_height_px: height_px,
        ascent_px: baseline_px as f32,
        descent_px: height_px.saturating_sub(baseline_px) as f32,
        baseline_px: baseline_px as f32,
        render_time,
    })
}

fn baseline_box_fits(
    height_px: u32,
    baseline_px: f32,
    terminal_ascent_subpixels: i64,
    terminal_descent_subpixels: i64,
) -> bool {
    let ascent_subpixels = (baseline_px.max(0.0) * SUBPIXELS_PER_PX as f32).ceil() as i64;
    let descent_subpixels =
        ((height_px as f32 - baseline_px).max(0.0) * SUBPIXELS_PER_PX as f32).ceil() as i64;
    ascent_subpixels <= terminal_ascent_subpixels && descent_subpixels <= terminal_descent_subpixels
}

fn artifact_from_raster(task: &DetectionTask, raster: MathRaster) -> PlaceholderArtifact {
    let height_subpixels = i64::from(raster.height_px).saturating_mul(SUBPIXELS_PER_PX);
    PlaceholderArtifact {
        key: shared_math_artifact_key(
            task.span.mode,
            &task.span.render_source,
            task.versions.layout,
            task.versions.detection,
        ),
        block_end: task.block_end,
        height_subpixels,
        width_px: raster.width_px,
        height_px: raster.height_px,
        baseline_subpixels: (raster.baseline_px * SUBPIXELS_PER_PX as f32).round() as i64,
        mode: task.span.mode,
        rgba: Arc::from(raster.rgba),
        render_time: raster.render_time,
    }
}

fn artifact_from_live_raster(task: &LiveDetectionTask, raster: MathRaster) -> PlaceholderArtifact {
    let height_subpixels = i64::from(raster.height_px).saturating_mul(SUBPIXELS_PER_PX);
    PlaceholderArtifact {
        key: shared_math_artifact_key(
            task.span.mode,
            &task.span.render_source,
            task.layout,
            task.detection_revision,
        ),
        block_end: TranscriptId(0),
        height_subpixels,
        width_px: raster.width_px,
        height_px: raster.height_px,
        baseline_subpixels: (raster.baseline_px * SUBPIXELS_PER_PX as f32).round() as i64,
        mode: task.span.mode,
        rgba: Arc::from(raster.rgba),
        render_time: raster.render_time,
    }
}

fn shared_math_artifact_key(
    mode: MathMode,
    source: &str,
    layout: LayoutKey,
    detection: DetectionRevision,
) -> String {
    let mut hasher = DefaultHasher::new();
    mode.hash(&mut hasher);
    source.hash(&mut hasher);
    layout.hash(&mut hasher);
    detection.hash(&mut hasher);
    format!("math:{:016x}", hasher.finish())
}

fn live_placeholder(task: &LiveDetectionTask) -> PlaceholderArtifact {
    PlaceholderArtifact {
        key: shared_math_artifact_key(
            task.span.mode,
            &task.span.render_source,
            task.layout,
            task.detection_revision,
        ),
        block_end: TranscriptId(0),
        height_subpixels: SUBPIXELS_PER_PX,
        width_px: 1,
        height_px: 1,
        baseline_subpixels: 0,
        mode: task.span.mode,
        rgba: Arc::from(vec![0; 4]),
        render_time: Duration::ZERO,
    }
}

fn layout_scale_milli(rendered: LayoutKey, current: LayoutKey) -> u32 {
    current
        .dpi_milli
        .get()
        .saturating_mul(1000)
        .checked_div(rendered.dpi_milli.get())
        .unwrap_or(1000)
        .max(1)
}

fn project_artifact(
    artifact: &PlaceholderArtifact,
    rendered_layout: LayoutKey,
    current_layout: LayoutKey,
    source: String,
    vertical_padding_subpixels: i64,
) -> ProjectedMathArtifact {
    let scale_milli = layout_scale_milli(rendered_layout, current_layout);
    project_artifact_at_scale(
        artifact,
        scale_milli,
        source,
        if artifact.mode == MathMode::Inline {
            0
        } else {
            vertical_padding_subpixels
        },
    )
}

fn project_artifact_at_scale(
    artifact: &PlaceholderArtifact,
    scale_milli: u32,
    source: String,
    vertical_padding_subpixels: i64,
) -> ProjectedMathArtifact {
    let tight_height_subpixels = artifact
        .height_subpixels
        .saturating_mul(i64::from(scale_milli))
        / 1000;
    ProjectedMathArtifact {
        key: artifact.key.clone(),
        end: artifact.block_end,
        rgba: Arc::clone(&artifact.rgba),
        width_px: artifact.width_px,
        height_px: artifact.height_px,
        height_subpixels: math_presentation_height_subpixels(
            tight_height_subpixels,
            vertical_padding_subpixels,
        ),
        baseline_subpixels: artifact
            .baseline_subpixels
            .saturating_mul(i64::from(scale_milli))
            / 1000,
        mode: artifact.mode,
        vertical_padding_subpixels,
        render_scale_milli: scale_milli,
        source,
    }
}

fn frozen_artifact_and_scale(record: &DecorationRecord) -> Option<(&PlaceholderArtifact, u32)> {
    if let Some(artifact) = record.artifact.as_ref() {
        Some((artifact, 1000))
    } else {
        record.stale_artifact.as_ref().map(|stale| {
            (
                &stale.artifact,
                layout_scale_milli(stale.rendered_layout, record.versions.layout),
            )
        })
    }
}

fn projected_frozen_artifact(
    record: &DecorationRecord,
    vertical_padding_subpixels: i64,
) -> Option<ProjectedMathArtifact> {
    let source = record.span.as_ref()?.render_source.clone();
    if let Some(artifact) = record.artifact.as_ref() {
        Some(project_artifact(
            artifact,
            record.versions.layout,
            record.versions.layout,
            source,
            vertical_padding_subpixels,
        ))
    } else {
        record.stale_artifact.as_ref().map(|stale| {
            project_artifact(
                &stale.artifact,
                stale.rendered_layout,
                record.versions.layout,
                source,
                vertical_padding_subpixels,
            )
        })
    }
}

fn live_artifact_and_scale(
    record: &LiveDecorationRecord,
    current_layout: LayoutKey,
) -> Option<(&PlaceholderArtifact, u32)> {
    if record.rendered_layout == current_layout
        && let Some(artifact) = record.artifact.as_ref()
    {
        Some((artifact, 1000))
    } else {
        record.stale_artifact.as_ref().map(|stale| {
            (
                &stale.artifact,
                layout_scale_milli(stale.rendered_layout, current_layout),
            )
        })
    }
}

fn projected_live_artifact(
    record: &LiveDecorationRecord,
    current_layout: LayoutKey,
    vertical_padding_subpixels: i64,
) -> Option<ProjectedMathArtifact> {
    let (artifact, scale_milli) = live_artifact_and_scale(record, current_layout)?;
    Some(project_artifact_at_scale(
        artifact,
        scale_milli,
        record.span.render_source.clone(),
        if artifact.mode == MathMode::Inline {
            0
        } else {
            vertical_padding_subpixels
        },
    ))
}

fn math_presentation_height_subpixels(
    tight_height_subpixels: i64,
    vertical_padding_subpixels: i64,
) -> i64 {
    tight_height_subpixels
        .saturating_add(vertical_padding_subpixels.saturating_mul(2))
        .max(1)
}

fn live_grid_input(inputs: &[LiveDetectionInput], row: u32) -> Option<&LiveDetectionInput> {
    inputs.iter().find(|input| {
        matches!(input.source, LiveDetectionSource::Grid { row: input_row, .. } if input_row == row)
    })
}

/// Frozen transcript rows this occurrence's opener/body already committed to scrollback, ordered
/// top to bottom. These are the leading `MathSourceLine::Transcript` segments of a boundary-split
/// block; an ordinary all-live occurrence yields an empty prefix. Consecutive segments on one
/// physical line share an id, so adjacent duplicates are collapsed.
fn frozen_prefix_ids(span: &MathSpan) -> Vec<TranscriptId> {
    let mut ids: Vec<TranscriptId> = Vec::new();
    for segment in &span.cell_segments {
        match segment.source_line {
            MathSourceLine::Transcript(id) => {
                if ids.last() != Some(&id) {
                    ids.push(id);
                }
            }
            MathSourceLine::LiveGrid(_) => break,
        }
    }
    ids
}

fn proven_live_occurrence(
    task: &LiveDetectionTask,
    occurrence_id: LiveMathOccurrenceId,
) -> Option<ProvenLiveOccurrence> {
    let band_rows = task
        .band_end_row
        .checked_sub(task.band_start_row)?
        .checked_add(1)?;
    let source_start_offset = task.start.row.checked_sub(task.band_start_row)?;
    let source_end_offset = task.end.row.checked_sub(task.band_start_row)?;
    let mut span = task.span.clone();
    for segment in &mut span.cell_segments {
        // Live-grid rows are stored band-relative so an identical formula stays a distinct
        // occurrence regardless of where it is later placed. Frozen-prefix rows keep their absolute
        // transcript identity; they are never part of the live band offset space.
        if let MathSourceLine::LiveGrid(row) = &mut segment.source_line {
            *row = row.checked_sub(task.band_start_row)?;
        }
    }
    let source_rows = (task.start.row..=task.end.row)
        .map(|row| {
            let input = live_grid_input(&task.inputs, row)?;
            Some(ProvenLiveRow {
                band_offset: row.checked_sub(task.band_start_row)?,
                text: input.text.clone(),
                continues: input.continues,
                cell_boundaries: input.cell_boundaries.clone(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ProvenLiveOccurrence {
        occurrence_id,
        created_generation: task.grid_generation,
        created_start: task.start,
        band_rows,
        source_start_offset,
        source_end_offset,
        source_rows,
        span,
    })
}

fn exact_live_source_match(
    source: &str,
    inputs: &[LiveDetectionInput],
    occupied: &BTreeSet<u32>,
) -> Option<(GridPoint, GridPoint, Vec<MathCellSegment>)> {
    struct RowRange<'a> {
        row: u32,
        logical_line: u32,
        start: usize,
        end: usize,
        input: &'a LiveDetectionInput,
    }

    if source.is_empty() {
        return None;
    }
    let mut text = String::new();
    let mut ranges = Vec::new();
    let mut logical_line = 0_u32;
    for input in inputs {
        let LiveDetectionSource::Grid { row, .. } = input.source else {
            continue;
        };
        let start = text.len();
        text.push_str(&input.text);
        let end = text.len();
        ranges.push(RowRange {
            row,
            logical_line,
            start,
            end,
            input,
        });
        if !input.continues {
            text.push('\n');
            logical_line = logical_line.saturating_add(1);
        }
    }

    let mut matches = text.match_indices(source);
    let (match_start, _) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let match_end = match_start.checked_add(source.len())?;
    let mut segments = Vec::new();
    for range in ranges {
        let segment_start = match_start.max(range.start);
        let segment_end = match_end.min(range.end);
        if segment_start >= segment_end {
            continue;
        }
        if occupied.contains(&range.row) {
            return None;
        }
        let byte_start = u32::try_from(segment_start - range.start).ok()?;
        let byte_end = u32::try_from(segment_end - range.start).ok()?;
        let cell_start = range
            .input
            .cell_boundaries
            .iter()
            .find_map(|(byte, cell)| (*byte == byte_start).then_some(*cell))?;
        let cell_end = range
            .input
            .cell_boundaries
            .iter()
            .find_map(|(byte, cell)| (*byte == byte_end).then_some(*cell))?;
        segments.push(MathCellSegment {
            logical_line: range.logical_line,
            source_line: MathSourceLine::LiveGrid(range.row),
            byte_start,
            byte_end,
            cell_start,
            cell_end,
        });
    }
    let first = segments.first()?;
    let last = segments.last()?;
    let MathSourceLine::LiveGrid(start_row) = first.source_line else {
        return None;
    };
    let MathSourceLine::LiveGrid(end_row) = last.source_line else {
        return None;
    };
    Some((
        GridPoint {
            row: start_row,
            column: first.cell_start,
        },
        GridPoint {
            row: end_row,
            column: last.cell_end,
        },
        segments,
    ))
}

fn extend_live_task_band(task: &mut LiveDetectionTask) {
    task.band_start_row = task.start.row;
    task.band_end_row = task.end.row;
    if task.span.mode == MathMode::Inline {
        return;
    }
    let mut borrowed = 0;
    for offset in 1..=LIVE_MATH_MAX_BORROWED_BLANK_ROWS {
        let Some(row) = task.end.row.checked_add(offset) else {
            break;
        };
        let Some(input) = live_grid_input(&task.inputs, row) else {
            break;
        };
        if !input.text.chars().all(char::is_whitespace) {
            break;
        }
        task.band_end_row = row;
        borrowed += 1;
    }
    for offset in 1..=LIVE_MATH_MAX_BORROWED_BLANK_ROWS.saturating_sub(borrowed) {
        let Some(row) = task.start.row.checked_sub(offset) else {
            break;
        };
        let Some(input) = live_grid_input(&task.inputs, row) else {
            break;
        };
        if !input.text.chars().all(char::is_whitespace) {
            break;
        }
        task.band_start_row = row;
    }
}

fn size_resolved_live_task_band(
    task: &mut LiveDetectionTask,
    artifact_height_subpixels: i64,
    cell_height_subpixels: i64,
    occupied_rows: &BTreeSet<u32>,
) {
    let is_bridge = task
        .span
        .cell_segments
        .iter()
        .any(|segment| matches!(segment.source_line, MathSourceLine::Transcript(_)));
    if is_bridge {
        // A boundary-split block gains its upward height from the frozen prefix rows it already
        // owns in scrollback. Borrowing blank live rows below the closer would place the rendered
        // formula below its source instead of over it, so the live band stays exactly the closer.
        task.band_start_row = task.start.row;
        task.band_end_row = task.end.row;
    } else if task.screen == ScreenId::Primary && task.span.mode == MathMode::Display {
        size_live_task_band_for_artifact(
            task,
            artifact_height_subpixels,
            cell_height_subpixels,
            occupied_rows,
        );
    } else {
        // Alternate-screen ownership is the exact source band. Presentation may expand its pixel
        // height, but it never borrows terminal rows or shrinks their coordinate identity.
        task.band_start_row = task.start.row;
        task.band_end_row = task.end.row;
    }
}

fn size_live_task_band_for_artifact(
    task: &mut LiveDetectionTask,
    artifact_height_subpixels: i64,
    cell_height_subpixels: i64,
    occupied_rows: &BTreeSet<u32>,
) {
    let source_rows = task.end.row.saturating_sub(task.start.row) + 1;
    let height = artifact_height_subpixels.max(1);
    let required_rows = u32::try_from(
        height
            .saturating_add(cell_height_subpixels - 1)
            .div_euclid(cell_height_subpixels),
    )
    .unwrap_or(u32::MAX);
    let mut needed = required_rows
        .saturating_sub(source_rows)
        .min(LIVE_MATH_MAX_BORROWED_BLANK_ROWS);
    task.band_start_row = task.start.row;
    task.band_end_row = task.end.row;

    // Preserve the established downward preference, then use contiguous blank rows above when the
    // lower neighbour is occupied. Upward rows are admitted only when they are blank and not in an
    // already-owned band, which is the missing non-overlap proof: unlike the old downward-only
    // argument, blankness alone is insufficient because a previous block may have borrowed it.
    for offset in 1..=LIVE_MATH_MAX_BORROWED_BLANK_ROWS {
        if needed == 0 {
            break;
        }
        let Some(row) = task.end.row.checked_add(offset) else {
            break;
        };
        let Some(input) = live_grid_input(&task.inputs, row) else {
            break;
        };
        if occupied_rows.contains(&row) || !input.text.chars().all(char::is_whitespace) {
            break;
        }
        task.band_end_row = row;
        needed -= 1;
    }
    for offset in 1..=LIVE_MATH_MAX_BORROWED_BLANK_ROWS {
        if needed == 0 {
            break;
        }
        let Some(row) = task.start.row.checked_sub(offset) else {
            break;
        };
        let Some(input) = live_grid_input(&task.inputs, row) else {
            break;
        };
        if occupied_rows.contains(&row) || !input.text.chars().all(char::is_whitespace) {
            break;
        }
        task.band_start_row = row;
        needed -= 1;
    }
}

fn live_detection_context_signature(inputs: &[LiveDetectionInput]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for input in inputs {
        let trimmed = input.text.trim();
        let structural = may_contain_display_math(trimmed)
            || trimmed.starts_with("```")
            || trimmed.starts_with("~~~");
        if structural {
            input.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn live_detection_signature(context_signature: u64, candidate_row: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    context_signature.hash(&mut hasher);
    candidate_row.hash(&mut hasher);
    hasher.finish()
}

fn may_contain_display_math(text: &str) -> bool {
    text.contains("$$")
        || text.contains(r"\[")
        || text.contains(r"\]")
        || text.contains(r"\begin{")
        || text.contains(r"\end{")
}

fn empty_live_math_span() -> MathSpan {
    MathSpan {
        byte_start: 0,
        byte_end: 0,
        original_source: String::new(),
        render_source: String::new(),
        delimiter_kind: DelimiterKind::Dollars,
        mode: MathMode::Display,
        cell_segments: Vec::new(),
        inline_runs: Vec::new(),
    }
}

fn live_grid_parser_prefixes(
    inputs: &[LiveDetectionInput],
    mut context: DetectionContext,
) -> BTreeMap<u32, DetectionContext> {
    let mut prefixes = BTreeMap::new();
    let mut logical_text = String::new();
    let mut logical_rows = Vec::new();
    let mut logical_prefix = context.clone();
    let mut logical_id = 1_u64;
    let mut active = false;

    for input in inputs {
        if !active {
            logical_prefix = context.clone();
            active = true;
        }
        logical_text.push_str(&input.text);
        if let LiveDetectionSource::Grid { row, .. } = input.source {
            logical_rows.push(row);
        }
        if input.continues {
            continue;
        }
        for row in logical_rows.drain(..) {
            prefixes.insert(row, logical_prefix.clone());
        }
        advance_detection_context(
            &mut context,
            TranscriptId(logical_id),
            logical_text.as_str(),
        );
        logical_id = logical_id.saturating_add(1);
        logical_text.clear();
        active = false;
    }
    if active {
        for row in logical_rows {
            prefixes.insert(row, logical_prefix.clone());
        }
    }
    prefixes
}

fn exact_row_content(left: &LiveDetectionInput, right: &LiveDetectionInput) -> bool {
    left.text == right.text
        && left.continues == right.continues
        && left.cell_boundaries == right.cell_boundaries
}

/// Build one transaction-level mapping before touching any decoration. Unique exact row/cell
/// anchors prove the moving delta. Exact fixed rows below those anchors prove the application's
/// chrome; repeated separator/blank rows may extend an already-proven fixed region but never act
/// as anchors themselves.
fn segmented_row_mapping(
    before: &[LiveDetectionInput],
    after: &[LiveDetectionInput],
) -> Vec<SegmentedRowMapping> {
    let before_rows = before
        .iter()
        .filter_map(|input| match input.source {
            LiveDetectionSource::Grid { row, .. } => Some((row, input)),
            LiveDetectionSource::History { .. } => None,
        })
        .collect::<Vec<_>>();
    let after_rows = after
        .iter()
        .filter_map(|input| match input.source {
            LiveDetectionSource::Grid { row, .. } => Some((row, input)),
            LiveDetectionSource::History { .. } => None,
        })
        .collect::<Vec<_>>();
    let Some(last_row) = after_grid_row_count(after).checked_sub(1) else {
        return Vec::new();
    };

    let unchanged = before_rows.len() == after_rows.len()
        && before_rows.iter().zip(&after_rows).all(
            |((before_row, before_input), (after_row, after_input))| {
                before_row == after_row && exact_row_content(before_input, after_input)
            },
        );
    if unchanged {
        return vec![SegmentedRowMapping {
            content_delta: 0,
            content_start_row: 0,
            content_end_row: last_row,
            fixed_start_row: None,
        }];
    }

    let unique_anchors = before_rows
        .iter()
        .filter_map(|(before_row, before_input)| {
            let before_matches = before_rows
                .iter()
                .filter(|(_, candidate)| exact_row_content(before_input, candidate))
                .count();
            if before_matches != 1 {
                return None;
            }
            let after_matches = after_rows
                .iter()
                .filter(|(_, candidate)| exact_row_content(before_input, candidate))
                .collect::<Vec<_>>();
            let [(after_row, _)] = after_matches.as_slice() else {
                return None;
            };
            Some((*before_row, *after_row))
        })
        .collect::<Vec<_>>();
    let mut by_delta = BTreeMap::<i64, Vec<(u32, u32)>>::new();
    for (before_row, after_row) in unique_anchors {
        by_delta
            .entry(i64::from(after_row).saturating_sub(i64::from(before_row)))
            .or_default()
            .push((before_row, after_row));
    }

    let moving = by_delta
        .iter()
        .filter(|(delta, anchors)| **delta != 0 && anchors.len() >= 2)
        .map(|(delta, anchors)| (*delta, anchors))
        .collect::<Vec<_>>();
    if moving.is_empty() {
        return if by_delta.get(&0).is_some_and(|anchors| anchors.len() >= 2) {
            vec![SegmentedRowMapping {
                content_delta: 0,
                content_start_row: 0,
                content_end_row: last_row,
                fixed_start_row: None,
            }]
        } else {
            Vec::new()
        };
    }

    moving
        .into_iter()
        .filter_map(|(content_delta, moving_anchors)| {
            let last_moving_anchor = moving_anchors
                .iter()
                .map(|(_, after_row)| *after_row)
                .max()?;
            let lower_fixed_anchors = by_delta
                .get(&0)
                .into_iter()
                .flatten()
                .filter(|(_, after_row)| *after_row > last_moving_anchor)
                .map(|(_, after_row)| *after_row)
                .collect::<Vec<_>>();
            let fixed_start_row = if lower_fixed_anchors.len() >= 2 {
                let mut start = *lower_fixed_anchors.iter().min()?;
                while let Some(previous) = start.checked_sub(1) {
                    let Some(before_input) = live_grid_input(before, previous) else {
                        break;
                    };
                    let Some(after_input) = live_grid_input(after, previous) else {
                        break;
                    };
                    if !exact_row_content(before_input, after_input) {
                        break;
                    }
                    start = previous;
                }
                Some(start)
            } else {
                None
            };
            let content_end_row = fixed_start_row
                .and_then(|row| row.checked_sub(1))
                .unwrap_or(last_row);
            moving_anchors
                .iter()
                .all(|(_, after_row)| *after_row <= content_end_row)
                .then_some(SegmentedRowMapping {
                    content_delta,
                    content_start_row: 0,
                    content_end_row,
                    fixed_start_row,
                })
        })
        .collect()
}

fn fixed_boundary_remains_proven(
    before: &[LiveDetectionInput],
    after: &[LiveDetectionInput],
    content_end_row: u32,
) -> bool {
    let Some(fixed_start_row) = content_end_row.checked_add(1) else {
        return false;
    };
    let mut anchors = 0_u32;
    for row in fixed_start_row..after_grid_row_count(after) {
        let (Some(before_input), Some(after_input)) =
            (live_grid_input(before, row), live_grid_input(after, row))
        else {
            continue;
        };
        if !exact_row_content(before_input, after_input) {
            continue;
        }
        let unique_before = before
            .iter()
            .filter(|candidate| exact_row_content(before_input, candidate))
            .count()
            == 1;
        let unique_after = after
            .iter()
            .filter(|candidate| exact_row_content(after_input, candidate))
            .count()
            == 1;
        if unique_before && unique_after {
            anchors = anchors.saturating_add(1);
        }
    }
    anchors >= 2
}

enum RecordProjection {
    Visible(LiveDecorationRecord),
    Dormant(LiveDecorationRecord),
}

#[allow(clippy::too_many_arguments)]
fn project_live_record_uniquely(
    record: &LiveDecorationRecord,
    mappings: &[SegmentedRowMapping],
    generation: GridGeneration,
    detection_revision: DetectionRevision,
    layout: LayoutKey,
    initial_context: DetectionContext,
    inputs: Arc<[LiveDetectionInput]>,
) -> Option<RecordProjection> {
    let mut visible = Vec::new();
    let mut dormant = Vec::new();
    for mapping in mappings {
        match project_live_record(
            record,
            *mapping,
            generation,
            detection_revision,
            layout,
            initial_context.clone(),
            Arc::clone(&inputs),
        ) {
            Some(RecordProjection::Visible(record)) => visible.push(record),
            Some(RecordProjection::Dormant(record)) => dormant.push(record),
            None => {}
        }
    }
    if visible.len() == 1 {
        return visible.pop().map(RecordProjection::Visible);
    }
    if visible.len() > 1 {
        return None;
    }

    if let Some(identity_mapping) = identity_row_mapping(record, mappings, &inputs)
        && !mappings.contains(&identity_mapping)
        && let Some(RecordProjection::Visible(record)) = project_live_record(
            record,
            identity_mapping,
            generation,
            detection_revision,
            layout,
            initial_context,
            inputs,
        )
    {
        return Some(RecordProjection::Visible(record));
    }
    (dormant.len() == 1)
        .then(|| dormant.pop().map(RecordProjection::Dormant))
        .flatten()
}

/// A partially visible proven occurrence may be the only rows in its local screen segment which
/// survived repaint. Derive a placement only when at least two complete proven row/cell
/// fingerprints vote for the same logical origin and no competing origin has the same support.
/// This is identity continuation inside an already-proven transaction region, not first detection.
fn identity_row_mapping(
    record: &LiveDecorationRecord,
    mappings: &[SegmentedRowMapping],
    inputs: &[LiveDetectionInput],
) -> Option<SegmentedRowMapping> {
    let content_start_row = mappings
        .iter()
        .map(|mapping| mapping.content_start_row)
        .max()?;
    let content_end_row = mappings
        .iter()
        .map(|mapping| mapping.content_end_row)
        .min()?;
    let fixed_start_row = mappings
        .iter()
        .filter_map(|mapping| mapping.fixed_start_row)
        .min();
    let mut origins = BTreeMap::<i64, u32>::new();
    for proven in &record.identity.source_rows {
        for input in inputs {
            let LiveDetectionSource::Grid { row, .. } = input.source else {
                continue;
            };
            if !(content_start_row..=content_end_row).contains(&row)
                || !proven.exactly_matches(input)
            {
                continue;
            }
            let origin = i64::from(row).saturating_sub(i64::from(proven.band_offset));
            let support = origins.entry(origin).or_default();
            *support = support.saturating_add(1);
        }
    }
    let best_support = origins.values().copied().max()?;
    if best_support < 2 {
        return None;
    }
    let best = origins
        .into_iter()
        .filter(|(_, support)| *support == best_support)
        .collect::<Vec<_>>();
    let [(logical_band_start, _)] = best.as_slice() else {
        return None;
    };
    Some(SegmentedRowMapping {
        content_delta: logical_band_start.saturating_sub(record.placement.logical_band_start),
        content_start_row,
        content_end_row,
        fixed_start_row,
    })
}

#[allow(clippy::too_many_arguments)]
fn project_live_record(
    record: &LiveDecorationRecord,
    mapping: SegmentedRowMapping,
    generation: GridGeneration,
    detection_revision: DetectionRevision,
    layout: LayoutKey,
    initial_context: DetectionContext,
    inputs: Arc<[LiveDetectionInput]>,
) -> Option<RecordProjection> {
    let row_count = after_grid_row_count(&inputs);
    let last_row = row_count.checked_sub(1)?;
    let mut record = record.clone();
    let logical_band_start = record
        .placement
        .logical_band_start
        .checked_add(mapping.content_delta)?;
    let logical_band_end =
        logical_band_start.checked_add(i64::from(record.identity.band_rows.checked_sub(1)?))?;
    record.placement.logical_band_start = logical_band_start;
    record.generation = generation;
    record.detection_revision = detection_revision;
    record.layout = layout;
    record.initial_context = initial_context;
    record.inputs = Arc::clone(&inputs);

    debug_assert!(record.identity.source_start_offset <= record.identity.source_end_offset);
    debug_assert!(record.identity.created_start.row >= record.identity.source_start_offset);
    let _occurrence_proof = (
        record.identity.created_generation,
        record.identity.created_start,
    );

    let content_start = i64::from(mapping.content_start_row);
    let content_end = i64::from(mapping.content_end_row.min(last_row));
    let terminal_end = i64::from(last_row);
    let band_intersects_content = logical_band_end >= content_start
        && logical_band_start <= content_end
        && logical_band_end >= 0
        && logical_band_start <= terminal_end;

    let mut exact_rows = Vec::<(u32, u32)>::new();
    let mut mismatched_rows = Vec::<i64>::new();
    let mut mismatched_source_rows = Vec::<(u32, Vec<(u32, u32)>)>::new();
    let mut occluded_source_rows = 0_u32;
    let mut occluded_visible_rows = Vec::<(u32, Vec<(u32, u32)>)>::new();
    for proven in &record.identity.source_rows {
        let target = logical_band_start.checked_add(i64::from(proven.band_offset))?;
        if target < 0 || target > terminal_end {
            continue;
        }
        let target_row = u32::try_from(target).ok()?;
        let input = live_grid_input(&inputs, target_row)?;
        if target < content_start || target > content_end {
            occluded_source_rows = occluded_source_rows.saturating_add(1);
            if let Some(ranges) = proven.source_clear_ranges(input) {
                occluded_visible_rows.push((target_row, ranges));
            }
            continue;
        }
        if proven.exactly_matches(input) {
            exact_rows.push((proven.band_offset, target_row));
        } else {
            mismatched_rows.push(target);
            if let Some(ranges) = proven.source_clear_ranges(input) {
                mismatched_source_rows.push((target_row, ranges));
            }
        }
    }

    if exact_rows.is_empty() {
        if band_intersects_content && !mismatched_rows.is_empty() {
            return None;
        }
        record.placement.occluded_source_rows = occluded_source_rows;
        record.placement.occluded_visible_rows = occluded_visible_rows;
        return Some(RecordProjection::Dormant(record));
    }
    exact_rows.sort_unstable();
    let first_exact_target = i64::from(exact_rows.first()?.1);
    let last_exact_target = i64::from(exact_rows.last()?.1);
    let top_mismatches = mismatched_rows
        .iter()
        .copied()
        .filter(|row| *row < first_exact_target)
        .collect::<Vec<_>>();
    let bottom_mismatches = mismatched_rows
        .iter()
        .copied()
        .filter(|row| *row > last_exact_target)
        .collect::<Vec<_>>();
    let boundary_mismatches_are_occluded = top_mismatches.len() + bottom_mismatches.len()
        == mismatched_rows.len()
        && top_mismatches
            .iter()
            .min()
            .is_none_or(|row| *row == content_start)
        && (top_mismatches.is_empty()
            || i64::try_from(top_mismatches.len())
                .is_ok_and(|count| count == first_exact_target.saturating_sub(content_start)))
        && bottom_mismatches
            .iter()
            .max()
            .is_none_or(|row| *row == content_end)
        && (bottom_mismatches.is_empty()
            || i64::try_from(bottom_mismatches.len())
                .is_ok_and(|count| count == content_end.saturating_sub(last_exact_target)));
    if !boundary_mismatches_are_occluded {
        return None;
    }
    occluded_source_rows = occluded_source_rows
        .saturating_add(u32::try_from(mismatched_rows.len()).unwrap_or(u32::MAX));
    occluded_visible_rows.extend(mismatched_source_rows);
    occluded_visible_rows.sort_unstable();
    occluded_visible_rows.dedup();

    let exact_offsets = exact_rows
        .iter()
        .map(|(offset, _)| *offset)
        .collect::<BTreeSet<_>>();
    let mut span = record.identity.span.clone();
    span.cell_segments.retain_mut(|segment| {
        let MathSourceLine::LiveGrid(offset) = &mut segment.source_line else {
            return false;
        };
        if !exact_offsets.contains(offset) {
            return false;
        }
        let Some(target) = logical_band_start.checked_add(i64::from(*offset)) else {
            return false;
        };
        let Ok(target) = u32::try_from(target) else {
            return false;
        };
        *offset = target;
        true
    });
    let first_segment = span.cell_segments.first()?;
    let last_segment = span.cell_segments.last()?;
    let MathSourceLine::LiveGrid(start_row) = first_segment.source_line else {
        return None;
    };
    let MathSourceLine::LiveGrid(end_row) = last_segment.source_line else {
        return None;
    };
    record.start = GridPoint {
        row: start_row,
        column: first_segment.cell_start,
    };
    record.end = GridPoint {
        row: end_row,
        column: last_segment.cell_end,
    };
    record.band_start_row = exact_rows.first()?.1;
    record.band_end_row = exact_rows.last()?.1;
    record.clipped_top_rows =
        u32::try_from(i64::from(record.band_start_row).saturating_sub(logical_band_start)).ok()?;
    record.clipped_bottom_rows =
        u32::try_from(logical_band_end.saturating_sub(i64::from(record.band_end_row))).ok()?;
    record.placement.occluded_source_rows = occluded_source_rows;
    record.placement.occluded_visible_rows = occluded_visible_rows;
    record.span = span;
    Some(RecordProjection::Visible(record))
}

#[allow(clippy::too_many_arguments)]
fn shift_live_record(
    record: &LiveDecorationRecord,
    delta: i64,
    generation: GridGeneration,
    detection_revision: DetectionRevision,
    layout: LayoutKey,
    initial_context: DetectionContext,
    inputs: Arc<[LiveDetectionInput]>,
) -> Option<LiveDecorationRecord> {
    let row_count = after_grid_row_count(&inputs);
    let last_row = row_count.checked_sub(1)?;
    let mapping = SegmentedRowMapping {
        content_delta: delta,
        content_start_row: 0,
        content_end_row: last_row,
        fixed_start_row: None,
    };
    match project_live_record(
        record,
        mapping,
        generation,
        detection_revision,
        layout,
        initial_context,
        inputs,
    )? {
        RecordProjection::Visible(record) => Some(record),
        RecordProjection::Dormant(_) => None,
    }
}

fn after_grid_row_count(inputs: &[LiveDetectionInput]) -> u32 {
    inputs
        .iter()
        .filter_map(|input| match input.source {
            LiveDetectionSource::Grid { row, .. } => Some(row.saturating_add(1)),
            LiveDetectionSource::History { .. } => None,
        })
        .max()
        .unwrap_or(0)
}

fn alternate_borrowed_band_is_clear(
    record: &LiveDecorationRecord,
    inputs: &[LiveDetectionInput],
    occupied: &BTreeSet<u32>,
) -> bool {
    (record.band_start_row..=record.band_end_row).all(|row| {
        !occupied.contains(&row)
            && ((record.start.row..=record.end.row).contains(&row)
                || live_grid_input(inputs, row)
                    .is_some_and(|input| input.text.chars().all(char::is_whitespace)))
    })
}

fn insert_nonoverlapping_live_record(
    records: &mut BTreeMap<u32, LiveDecorationRecord>,
    occupied: &mut BTreeSet<u32>,
    record: LiveDecorationRecord,
) -> Option<LiveDecorationRecord> {
    if records.contains_key(&record.start.row)
        || (record.band_start_row..=record.band_end_row).any(|row| occupied.contains(&row))
    {
        return Some(record);
    }
    occupied.extend(record.band_start_row..=record.band_end_row);
    records.insert(record.start.row, record);
    None
}

fn live_task_is_current(
    task: &LiveDetectionTask,
    current_inputs: Arc<[LiveDetectionInput]>,
) -> bool {
    let dependency_start = if task.resolved {
        task.band_start_row
    } else {
        task.candidate_row
    };
    let dependency_end = if task.resolved {
        task.band_end_row
    } else {
        task.candidate_row
    };
    for row in dependency_start..=dependency_end {
        let Some(snapshot) = live_grid_input(&task.inputs, row) else {
            return false;
        };
        let Some(current) = live_grid_input(&current_inputs, row) else {
            return false;
        };
        let same_source = matches!(
            (snapshot.source, current.source),
            (
                LiveDetectionSource::Grid { row: snapshot_row, .. },
                LiveDetectionSource::Grid { row: current_row, .. },
            ) if snapshot_row == current_row
        );
        if !same_source || snapshot.text != current.text {
            return false;
        }
    }

    let mut current_task = task.clone();
    current_task.inputs = current_inputs;
    current_task.start = GridPoint {
        row: task.candidate_row,
        column: 0,
    };
    current_task.end = current_task.start;
    current_task.band_start_row = task.candidate_row;
    current_task.band_end_row = task.candidate_row;
    current_task.span = MathSpan {
        byte_start: 0,
        byte_end: 0,
        original_source: String::new(),
        render_source: String::new(),
        delimiter_kind: DelimiterKind::Dollars,
        mode: MathMode::Display,
        cell_segments: Vec::new(),
        inline_runs: Vec::new(),
    };
    current_task.detection_complete = false;
    current_task.resolved = false;
    let resolves_now = resolve_live_detection_task(&mut current_task);
    if !task.resolved {
        return !resolves_now;
    }
    resolves_now
        && current_task.start == task.start
        && current_task.end == task.end
        && current_task.span == task.span
}

fn captured_row_text(row: &CapturedRow) -> String {
    captured_row_text_and_boundaries(row).0
}

fn live_candidate_rows(
    inputs: &[LiveDetectionInput],
    mut context: DetectionContext,
    stable: &[bool],
) -> Vec<u32> {
    let mut candidates = Vec::new();
    let mut hidden_code_prefix = context.is_commonmark_code();
    let mut logical_text = String::new();
    let mut logical_grid_rows = Vec::new();
    for input in inputs {
        logical_text.push_str(&input.text);
        if let LiveDetectionSource::Grid { row, .. } = input.source {
            logical_grid_rows.push(row);
        }
        if input.continues {
            continue;
        }
        if !hidden_code_prefix
            && may_contain_display_math(&logical_text)
            && let Some(row) = logical_grid_rows
                .last()
                .copied()
                .filter(|row| stable.get(*row as usize).copied().unwrap_or(false))
        {
            candidates.push(row);
        }
        advance_detection_context(&mut context, TranscriptId(0), &logical_text);
        if hidden_code_prefix && !context.is_commonmark_code() {
            hidden_code_prefix = false;
        }
        logical_text.clear();
        logical_grid_rows.clear();
    }
    candidates
}

fn captured_row_text_and_boundaries(row: &CapturedRow) -> (String, Vec<(u32, u32)>) {
    let mut text = String::new();
    let mut boundaries = vec![(0, 0)];
    for (column, cell) in row.cells.iter().enumerate() {
        if cell.wide_spacer {
            continue;
        }
        let byte_start = text.len();
        let cell_text = if cell.text.is_empty() {
            " "
        } else {
            cell.text.as_str()
        };
        text.push_str(cell_text);
        let mut cell_end = column + 1;
        while row
            .cells
            .get(cell_end)
            .is_some_and(|candidate| candidate.wide_spacer)
        {
            cell_end += 1;
        }
        boundaries.push((
            u32::try_from(byte_start).unwrap_or(u32::MAX),
            u32::try_from(column).unwrap_or(u32::MAX),
        ));
        boundaries.push((
            u32::try_from(text.len()).unwrap_or(u32::MAX),
            u32::try_from(cell_end).unwrap_or(u32::MAX),
        ));
    }
    text.truncate(text.trim_end_matches([' ', '\t']).len());
    let final_byte = u32::try_from(text.len()).unwrap_or(u32::MAX);
    boundaries.retain(|(byte, _)| *byte <= final_byte);
    boundaries.sort_unstable();
    boundaries.dedup_by_key(|(byte, _)| *byte);
    if boundaries
        .last()
        .is_none_or(|(byte, _)| *byte != final_byte)
    {
        let cell = boundaries.last().map_or(0, |(_, cell)| *cell);
        boundaries.push((final_byte, cell));
    }
    (text, boundaries)
}

fn frozen_cell_boundaries(line: &FrozenLine) -> Vec<(u32, u32)> {
    let mut boundaries = Vec::with_capacity(line.grapheme_boundaries.len());
    let mut cell = 0u32;
    boundaries.push((0, cell));
    for bytes in line.grapheme_boundaries.windows(2) {
        let byte_start = bytes[0];
        let byte_end = bytes[1];
        let wide = line.styles.iter().any(|style| {
            style.byte_start <= byte_start
                && byte_start < style.byte_end
                && style.style.flags.contains(CellFlags::WIDE_CHAR)
        });
        cell = cell.saturating_add(if wide { 2 } else { 1 });
        boundaries.push((byte_end, cell));
    }
    boundaries
}

/// A chunk is a full-screen repaint transaction when it either clears-and-homes, or opens a DEC
/// 2026 synchronized update, or homes and rewrites several lines with erase-to-EOL. Real TUIs
/// (Claude Code) repaint with a synchronized `\x1b[?2026h … \x1b[?2026l` block that homes and
/// erases each line (`\x1b[K`) rather than emitting `\x1b[2J`; keying the boundary only on `2J`
/// missed every one of those repaints, so a formula flashed back to source across them.
fn contains_clear_home_snapshot_boundary(bytes: &[u8]) -> bool {
    // Synchronized update: the parser withholds the intermediate state and commits one atomic
    // frame at ESU, which is exactly the repaint transaction boundary we must preserve across.
    if bytes.windows(8).any(|window| window == b"\x1b[?2026h") {
        return true;
    }
    if let Some(clear) = bytes.windows(4).position(|window| window == b"\x1b[2J") {
        let suffix = &bytes[clear + 4..];
        if suffix.windows(3).any(|window| window == b"\x1b[H")
            || suffix.windows(6).any(|window| window == b"\x1b[1;1H")
        {
            return true;
        }
    }
    // A home followed by repeated erase-to-EOL line rewrites is a full repaint without 2J.
    let homes_early = bytes.windows(3).take(8).any(|window| window == b"\x1b[H")
        || bytes
            .windows(6)
            .take(8)
            .any(|window| window == b"\x1b[1;1H");
    homes_early
        && bytes
            .windows(3)
            .filter(|window| *window == b"\x1b[K")
            .count()
            >= 3
}

fn frame_row_history_id(frame: &ViewportFrame, row: u32) -> Option<TranscriptId> {
    let index = row as usize * frame.columns.get() as usize;
    match &frame.cell_anchors.get(index)?.start {
        ContentAnchor::History { id, .. } => Some(*id),
        _ => None,
    }
}

fn frame_row_for_history(frame: &ViewportFrame, id: TranscriptId) -> Option<u32> {
    (0..frame.rows.get()).find(|row| frame_row_history_id(frame, *row) == Some(id))
}

fn frozen_inline_cells(
    frame: &ViewportFrame,
    id: TranscriptId,
    line: &FrozenLine,
    span: &MathSpan,
) -> Option<(u32, u32, Vec<usize>)> {
    let columns = frame.columns.get() as usize;
    let mut row = None;
    let mut left = u32::MAX;
    let mut cells = Vec::new();
    for run in &span.inline_runs {
        let start = u32::try_from(
            line.grapheme_boundaries
                .binary_search(&run.byte_start)
                .ok()?,
        )
        .ok()?;
        let end =
            u32::try_from(line.grapheme_boundaries.binary_search(&run.byte_end).ok()?).ok()?;
        let mut found = false;
        for (index, anchors) in frame.cell_anchors.iter().enumerate() {
            let ContentAnchor::History {
                id: anchor_id,
                offset,
                ..
            } = &anchors.start
            else {
                continue;
            };
            if *anchor_id != id || offset.0 < start || offset.0 >= end {
                continue;
            }
            let cell_row = u32::try_from(index / columns).ok()?;
            let cell_column = u32::try_from(index % columns).ok()?;
            if row.is_some_and(|current| current != cell_row) {
                return None;
            }
            row = Some(cell_row);
            left = left.min(cell_column);
            cells.push(index);
            found = true;
        }
        if !found {
            return None;
        }
    }
    Some((row?, left, cells))
}

fn live_inline_cells(
    frame: &ViewportFrame,
    live_row: u32,
    text: &str,
    span: &MathSpan,
) -> Option<(u32, u32, Vec<usize>)> {
    let frame_row = frame
        .row_map
        .iter()
        .position(|mapped| mapped.live_grid_row == Some(live_row))?;
    let columns = frame.columns.get() as usize;
    let mut left = usize::MAX;
    let mut cells = Vec::new();
    for run in &span.inline_runs {
        let start = usize::try_from(run.byte_start).ok()?;
        let end = usize::try_from(run.byte_end).ok()?;
        let start_column = UnicodeWidthStr::width(text.get(..start)?);
        let end_column = start_column.saturating_add(UnicodeWidthStr::width(text.get(start..end)?));
        if start_column >= end_column || end_column > columns {
            return None;
        }
        left = left.min(start_column);
        cells.extend((start_column..end_column).map(|column| frame_row * columns + column));
    }
    Some((
        u32::try_from(frame_row).ok()?,
        u32::try_from(left).ok()?,
        cells,
    ))
}

fn frame_row_for_live_range(
    frame: &ViewportFrame,
    screen: ScreenId,
    start: u32,
    end: u32,
) -> Option<(u32, u32)> {
    (0..frame.rows.get()).find_map(|frame_row| {
        let index = frame_row as usize * frame.columns.get() as usize;
        match &frame.cell_anchors.get(index)?.start {
            ContentAnchor::Live {
                screen: anchor_screen,
                point,
                ..
            } if *anchor_screen == screen && start <= point.row && point.row <= end => {
                Some((frame_row, point.row))
            }
            _ => None,
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn scroll_offsets(
    horizontal_scroll_px: &mut u32,
    vertical_scroll_px: &mut u32,
    artifact_size: (u32, u32),
    scale_milli: u32,
    pane_width_px: u32,
    options: MathLayoutOptions,
    horizontal_delta_px: i32,
    vertical_delta_px: i32,
) -> bool {
    let scaled_width = artifact_size.0.saturating_mul(scale_milli) / 1000;
    let scaled_height = artifact_size.1.saturating_mul(scale_milli) / 1000;
    let horizontal_max = scaled_width.saturating_sub(pane_width_px);
    let vertical_max = options
        .block_max_height_px
        .map(|max| scaled_height.saturating_sub(max.get()))
        .unwrap_or(0);
    let mut changed = false;
    if options.line_wrapping && horizontal_max != 0 && horizontal_delta_px != 0 {
        let next = horizontal_scroll_px
            .saturating_add_signed(horizontal_delta_px)
            .min(horizontal_max);
        changed |= next != *horizontal_scroll_px;
        *horizontal_scroll_px = next;
    }
    if vertical_max != 0 && vertical_delta_px != 0 {
        let next = vertical_scroll_px
            .saturating_add_signed(vertical_delta_px)
            .min(vertical_max);
        changed |= next != *vertical_scroll_px;
        *vertical_scroll_px = next;
    }
    changed
}

fn math_block_available_width_px(
    pane_width_px: u32,
    mode: MathMode,
    display_left_inset_subpixels: i64,
) -> u32 {
    if mode != MathMode::Display {
        return pane_width_px;
    }
    let inset_px = display_left_inset_subpixels
        .saturating_add(SUBPIXELS_PER_PX - 1)
        .div_euclid(SUBPIXELS_PER_PX)
        .max(0) as u32;
    pane_width_px.saturating_sub(inset_px).max(1)
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

    fn default_math_padding_subpixels() -> i64 {
        SPIKE_CELL_HEIGHT_SUBPIXELS
            .get()
            .saturating_mul(i64::from(DEFAULT_MATH_VERTICAL_PADDING_CELL_MILLI))
            / 1000
    }

    fn mapping_inputs(rows: &[&str]) -> Vec<LiveDetectionInput> {
        rows.iter()
            .enumerate()
            .map(|(row, text)| LiveDetectionInput {
                source: LiveDetectionSource::Grid {
                    row: row as u32,
                    revision: 1,
                },
                text: (*text).to_owned(),
                continues: false,
                cell_boundaries: std::iter::once((0, 0))
                    .chain(
                        text.char_indices()
                            .enumerate()
                            .map(|(column, (byte, character))| {
                                (
                                    u32::try_from(byte + character.len_utf8()).unwrap(),
                                    u32::try_from(column + 1).unwrap(),
                                )
                            }),
                    )
                    .collect(),
            })
            .collect()
    }

    #[test]
    fn exact_screen_mapping_separates_moving_content_from_fixed_chrome() {
        let before = mapping_inputs(&[
            "alpha",
            "beta",
            "$$",
            "begin",
            "equation-a",
            "equation-b",
            "end",
            "$$",
            "",
            "separator",
            "prompt",
            "status",
        ]);
        let after = mapping_inputs(&[
            "new-0",
            "new-1",
            "new-2",
            "alpha",
            "beta",
            "$$",
            "begin",
            "equation-a",
            "",
            "separator",
            "prompt",
            "status",
        ]);

        let mappings = segmented_row_mapping(&before, &after);
        assert!(mappings.contains(&SegmentedRowMapping {
            content_delta: 3,
            content_start_row: 0,
            content_end_row: 7,
            fixed_start_row: Some(8),
        }));
        assert!(fixed_boundary_remains_proven(&before, &after, 7));
        assert!(
            !mappings
                .iter()
                .any(|mapping| { mapping.content_delta == 3 && mapping.content_end_row == 11 }),
            "mutation guard: a whole-terminal single delta must not absorb fixed chrome"
        );
    }

    #[test]
    fn restore_stripped_environment_newlines_option_reaches_detector_tasks() {
        let mut session = DualPlaneSession::new(nz(40), nz(4));
        session.set_math_layout_options(MathLayoutOptions {
            restore_stripped_environment_newlines: false,
            reject_claude_code_jump_chip_overlay: false,
            ..MathLayoutOptions::default()
        });
        assert_eq!(
            session.detection_options(),
            DetectionOptions {
                restore_stripped_environment_newlines: false,
                reject_claude_code_jump_chip_overlay: false,
            }
        );
    }

    #[test]
    fn finalized_line_ingest_stays_linear_without_live_handoffs() {
        let mut elapsed = Vec::new();
        for count in [1_000_usize, 16_000, 32_000] {
            let mut session = DualPlaneSession::with_frozen_quota(
                nz(1),
                nz(1),
                NonZeroUsize::new(count + 1).unwrap(),
            );
            let started = Instant::now();
            for _ in 0..count {
                let captured = session.transcript.capture(CapturedRow::plain("x", false));
                session
                    .staging_sources
                    .insert(captured.staging_id, SourceLifecycle::Live);
                for finalized in captured.finalized {
                    session.ingest_finalized(finalized).unwrap();
                }
            }
            let measured = started.elapsed();
            eprintln!("FINALIZED_SCALE count={count} elapsed={measured:?}");
            elapsed.push(measured);
        }
        let ratio = elapsed[2].as_secs_f64() / elapsed[0].as_secs_f64();
        // 32k performs 32x the useful append work. A 64x ceiling allows 2x host/timer variance;
        // the prior whole-history rescan is quadratic and measured near 1,000x on this probe.
        assert!(
            ratio <= 64.0,
            "32k/1k finalized ingest ratio {ratio:.2} exceeds the 64x linearity ceiling: {elapsed:?}"
        );
    }

    #[test]
    fn wide_same_content_repaint_fingerprint_overhead_is_bounded() {
        const COLUMNS: u32 = 2_000;
        const ROWS: u32 = 40;
        const CYCLES: usize = 16;

        let mut repaint = Vec::with_capacity(COLUMNS as usize * ROWS as usize);
        for row in 1..=ROWS {
            repaint.extend_from_slice(format!("\x1b[{row};1H").as_bytes());
            if row == 1 {
                repaint.extend_from_slice(b"$$x$$\x1b[K");
            } else {
                repaint.extend(std::iter::repeat_n(b'x', COLUMNS as usize));
            }
        }

        let mut adapter = TerminalAdapter::new(nz(COLUMNS), nz(ROWS));
        adapter.feed(b"\x1b[?1049h");
        adapter.feed(&repaint);
        adapter.take_damage();

        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(COLUMNS), nz(ROWS));
        session.feed_at(b"\x1b[?1049h", start).unwrap();
        session.feed_at(&repaint, start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 18)),
            1
        );
        session
            .live_decorations
            .get_mut(&0)
            .expect("row-zero formula produced a ready live artifact")
            .band_end_row = ROWS - 1;
        let fingerprints = (0..ROWS)
            .map(|row| session.terminal.visible_row_fingerprint(row))
            .collect::<Vec<_>>();
        for (state, fingerprint) in session.live_rows.iter_mut().zip(fingerprints) {
            state.content_fingerprint = fingerprint;
        }

        // Warm both parsers and damage trackers before measuring the same byte stream.
        adapter.feed(&repaint);
        adapter.take_damage();
        session.feed(&repaint).unwrap();
        assert_eq!(session.live_decorations.len(), 1);

        let baseline_started = Instant::now();
        for _ in 0..CYCLES {
            adapter.feed(&repaint);
            adapter.take_damage();
        }
        let baseline = baseline_started.elapsed();

        let measured_started = Instant::now();
        for _ in 0..CYCLES {
            session.feed(&repaint).unwrap();
        }
        let measured = measured_started.elapsed();
        let ratio = measured.as_secs_f64() / baseline.as_secs_f64();
        eprintln!(
            "G1_WIDE_DAMAGE columns={COLUMNS} rows={ROWS} cycles={CYCLES} baseline={baseline:?} fingerprint={measured:?} ratio={ratio:.2}"
        );
        assert_eq!(
            session.live_decorations.len(),
            1,
            "same-content repaint must preserve the live artifact"
        );
        // Both sides parse and write 1.28M terminal cells. The fixed side additionally streams each
        // cell once into an allocation-free fingerprint. A 2.25x ceiling allows 12.5% host/timer
        // variance over the equal-cost parse+hash model, yet still rejects the measured 6.2x clone
        // regression.
        assert!(
            ratio <= 2.25,
            "wide full-screen content invalidation took {ratio:.2}x parser/damage baseline ({measured:?} vs {baseline:?})"
        );
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
    fn stopped_scrollback_schedules_only_visible_frozen_candidates() {
        const TOTAL_LINES: usize = 2_048;
        const VIEW_ROWS: u32 = 8;
        let mut bytes = Vec::new();
        for index in 0..TOTAL_LINES {
            bytes.extend_from_slice(
                format!("\\begin{{align}}x_{{{index}}}&=y\\end{{align}}\r\n").as_bytes(),
            );
        }
        bytes.extend_from_slice(b"tail");
        let mut session = DualPlaneSession::with_frozen_quota(
            nz(80),
            nz(VIEW_ROWS),
            NonZeroUsize::new(TOTAL_LINES + 16).unwrap(),
        );
        session.feed(&bytes).unwrap();
        while session.take_worker_task().is_some() {}
        for record in session.decorations.values_mut() {
            record.decoration = DecorationLifecycle::None;
            record.artifact = None;
            record.stale_artifact = None;
        }

        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_by_rows((TOTAL_LINES / 2) as i32);
        session.refresh_projection(&mut projection);
        let stopped = session.viewport_frame(&mut projection).unwrap();
        let visible_ids = stopped
            .cell_anchors
            .chunks(stopped.columns.get() as usize)
            .filter_map(|row| match &row.first()?.start {
                ContentAnchor::History { id, .. } => Some(*id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(visible_ids.len(), VIEW_ROWS as usize);

        let detections_before = session.frozen_detection_count();
        let scheduled = session.schedule_visible_artifacts(&stopped);
        let scheduled_delta = session
            .frozen_detection_count()
            .saturating_sub(detections_before) as usize;
        assert_eq!(scheduled, scheduled_delta);
        assert!(scheduled > 0);
        assert!(
            scheduled <= visible_ids.len() + 1,
            "visible scheduling inspected {scheduled} candidates for {} visible lines out of {TOTAL_LINES}",
            visible_ids.len()
        );
        assert!(
            session.retry_on_idle() > visible_ids.len(),
            "offscreen retries must remain deferred instead of triggering a history-wide scan"
        );

        let mut completed = 0usize;
        while let Some(mut task) = session.take_worker_task() {
            assert!(
                task.inputs.len() <= 1,
                "single-line visible formulas need no transcript-wide prefix: {:?}",
                task.inputs.iter().map(|input| input.id).collect::<Vec<_>>()
            );
            assert!(resolve_detection_task(&mut task));
            assert!(session.complete_worker_result(task, Ok(synthetic_raster(40, 18))));
            completed += 1;
        }
        assert_eq!(completed, scheduled);
        assert!(visible_ids.iter().all(|id| {
            session
                .decoration(*id)
                .is_some_and(|record| record.decoration == DecorationLifecycle::Ready)
        }));
    }

    #[test]
    fn visible_multiline_opener_schedules_its_offscreen_finalized_close_without_another_scroll() {
        let mut session = DualPlaneSession::new(nz(40), nz(2));
        session
            .feed(
                b"header\r\n\\begin{align}\r\nx &= y\r\n\\end{align}\r\none\r\ntwo\r\nthree\r\ntail",
            )
            .unwrap();
        while session.take_worker_task().is_some() {}
        for record in session.decorations.values_mut() {
            record.decoration = DecorationLifecycle::None;
            record.artifact = None;
        }

        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        session.refresh_projection(&mut projection);
        let stopped = session.viewport_frame(&mut projection).unwrap();
        assert!(stopped.cells.iter().any(|cell| cell.text == "\\"));
        assert!(session.schedule_visible_artifacts(&stopped) >= 2);

        let mut saw_necessary_context = false;
        while let Some(task) = session.take_worker_task() {
            saw_necessary_context |= task.inputs.len() == 3;
            session.complete_worker_task(task);
        }
        assert!(saw_necessary_context);
        assert!(session.decorations.values().any(|record| {
            record.decoration == DecorationLifecycle::Ready
                && record.block_end.is_some_and(|end| end.0 > 1)
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
            baseline_px: 12.0,
            render_time: std::time::Duration::from_millis(3),
        }
    }

    #[test]
    fn math_artifact_cache_identity_includes_delimiter_selected_mode() {
        let layout = LayoutKey {
            width_cells: nz(80),
            dpi_milli: nz(1000),
            font_rev: 1,
            theme_rev: 1,
        };
        assert_ne!(
            shared_math_artifact_key(MathMode::Display, "x", layout, DetectionRevision(1)),
            shared_math_artifact_key(MathMode::Inline, "x", layout, DetectionRevision(1)),
        );
    }

    #[test]
    fn inline_fit_uses_the_terminal_baseline_not_total_ink_height() {
        let height_px = 18;
        let math_baseline_px = 14.0;
        // Total ink exactly fits the 18 px row, so the old height-only check accepted it.
        assert!(!baseline_box_fits(
            height_px,
            math_baseline_px,
            10 * SUBPIXELS_PER_PX,
            8 * SUBPIXELS_PER_PX,
        ));
        assert!(baseline_box_fits(
            height_px,
            math_baseline_px,
            14 * SUBPIXELS_PER_PX,
            4 * SUBPIXELS_PER_PX,
        ));
    }

    #[test]
    fn inline_candidates_produce_no_work_while_detection_is_disabled() {
        // Inline `$...$` is off until its disambiguator is sound (see detect_inline_math).
        // Both of these lines - genuine inline math, and a shell line that the old heuristic
        // mis-rendered - must now behave identically: no task, no decoration, source intact.
        for source in [
            "中文 $E = mc^2$ and $a_1+b_1$ end",
            "PATH=$HOME/bin:$PATH",
            "WHERE a=$1 AND b=$2",
        ] {
            let mut session = DualPlaneSession::new(nz(80), nz(2));
            session
                .feed(format!("{source}\r\nnext\r\ntail").as_bytes())
                .unwrap();
            // Scheduling is a coarse "contains $" prescreen; detection is what decides. So a task
            // may be enqueued, but it must resolve to nothing.
            if let Some(mut task) = session.take_worker_task() {
                assert!(
                    !resolve_detection_task(&mut task),
                    "{source}: inline detection must not resolve a block while disabled"
                );
            }
            let mut projection = session.new_projection(session.layout_key());
            session.viewport_frame(&mut projection).unwrap();
            projection.scroll_to_top();
            session.refresh_projection(&mut projection);
            let frame = session.viewport_frame(&mut projection).unwrap();
            assert!(frame.math_blocks.is_empty(), "{source}");
            assert!(frame.math_failures.is_empty(), "{source}");
            assert!(
                frame.cells.iter().any(|cell| cell.text == "$"),
                "{source}: the literal text must survive untouched"
            );
        }
    }

    #[test]
    fn render_failures_are_marked_and_counted_by_stage() {
        let mut session = DualPlaneSession::new(nz(40), nz(2));
        session
            .feed(b"$$a$$\r\n$$b$$\r\n$$c$$\r\none\r\ntwo\r\ntail")
            .unwrap();
        let mut tasks = Vec::new();
        while let Some(task) = session.take_worker_task() {
            tasks.push(task);
        }
        assert_eq!(tasks.len(), 3);
        for task in &mut tasks {
            assert!(resolve_detection_task(task));
        }
        assert!(
            session.complete_worker_result(tasks.remove(0), Err(MathRenderError::UnsafeCommand))
        );
        assert!(session.complete_worker_result(
            tasks.remove(0),
            Err(MathRenderError::Convert("unsupported".to_owned()))
        ));
        assert!(session.complete_worker_result(
            tasks.remove(0),
            Err(MathRenderError::Compile("unknown variable".to_owned()))
        ));
        assert_eq!(session.math_failure_validate_count, 1);
        assert_eq!(session.math_failure_convert_count, 1);
        assert_eq!(session.math_failure_compile_count, 1);

        let failed = session
            .decorations
            .iter()
            .find(|(_, record)| record.failure_reason.is_some())
            .map(|(id, record)| (*id, record.block_end.unwrap()))
            .unwrap();
        let anchor = MathBlockAnchor::History {
            start: failed.0,
            end: failed.1,
        };
        assert!(session.set_math_hover(Some(&anchor)));
        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        session.refresh_projection(&mut projection);
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            frame.math_failures.len(),
            2,
            "each visible failed source line is marked"
        );
        assert!(
            frame
                .status_text
                .as_deref()
                .is_some_and(|status| status.starts_with("Formula not rendered:"))
        );
        assert!(frame.cells.iter().any(|cell| cell.text == "$"));
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
        assert_eq!(
            ready.math_blocks[0].left_subpixels,
            session.cell_width_subpixels.get() / DISPLAY_MATH_LEFT_INSET_DENOMINATOR
        );
        assert_eq!(
            projection.heights().get(0),
            Some(math_presentation_height_subpixels(
                35 * SUBPIXELS_PER_PX,
                default_math_padding_subpixels(),
            ))
        );
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
        assert_eq!(invalidated.math_blocks.len(), 1);
        assert_eq!(invalidated.math_blocks[0].artifact.render_scale_milli, 1250);
        assert!(!invalidated.cells.iter().any(|cell| cell.text == "$"));
        assert_eq!(session.pending_tasks(), 1);

        let mut failed = DualPlaneSession::new(nz(16), nz(2));
        failed.feed(b"$$bad$$\r\nnext\r\ntail").unwrap();
        let mut task = failed.take_worker_task().unwrap();
        assert!(resolve_detection_task(&mut task));
        assert!(failed.complete_worker_result(task, Err(MathRenderError::InvalidDimensions)));
        // A render that FAILED is distinguishable from one we chose not to decorate: the former
        // carries a reason and becomes visible to the user (M1.9f), the latter stays silent.
        // Without the distinction a broken formula is indistinguishable from a literal one.
        let failed_decoration = failed.decorations.values().next().unwrap();
        assert_eq!(failed_decoration.decoration, DecorationLifecycle::Failed);
        assert!(
            failed_decoration.failure_reason.is_some(),
            "a failed render must keep its reason so the surface can explain itself"
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
    fn display_math_inset_reduces_only_block_owned_horizontal_viewport() {
        assert_eq!(
            math_block_available_width_px(100, MathMode::Display, 5 * SUBPIXELS_PER_PX),
            95
        );
        assert_eq!(
            math_block_available_width_px(100, MathMode::Inline, 5 * SUBPIXELS_PER_PX),
            100
        );
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
        assert!(
            session
                .decorations
                .values()
                .all(|record| record.stale_artifact.is_some())
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
    fn stale_pixels_are_layout_only_and_detector_change_returns_to_source_immediately() {
        let mut session = DualPlaneSession::new(nz(16), nz(2));
        session.feed(b"$$x^2$$\r\nnext\r\ntail").unwrap();
        let mut task = session.take_worker_task().unwrap();
        assert!(resolve_detection_task(&mut task));
        assert!(session.complete_worker_result(task, Ok(synthetic_raster(240, 80))));
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

        session.resize(nz(20), nz(2)).unwrap();
        session.set_layout_key(LayoutKey {
            width_cells: nz(20),
            ..session.layout_key()
        });
        session.refresh_projection(&mut projection);
        let resized = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(resized.math_blocks.len(), 1);
        assert_eq!(resized.math_blocks[0].artifact.render_scale_milli, 1000);
        assert!(!resized.cells.iter().any(|cell| cell.text == "$"));

        session.set_layout_key(LayoutKey {
            dpi_milli: nz(1500),
            ..session.layout_key()
        });
        session.refresh_projection(&mut projection);
        let stale = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(stale.math_blocks.len(), 1);
        assert_eq!(stale.math_blocks[0].artifact.render_scale_milli, 1500);

        session.redetect(DetectionRevision(2));
        session.refresh_projection(&mut projection);
        let honest_source = session.viewport_frame(&mut projection).unwrap();
        assert!(honest_source.math_blocks.is_empty());
        assert!(honest_source.cells.iter().any(|cell| cell.text == "$"));
    }

    #[test]
    fn configured_block_max_owns_vertical_scroll_while_unlimited_bubbles() {
        let mut session = DualPlaneSession::new(nz(16), nz(2));
        session.feed(b"$$x^2$$\r\nnext\r\ntail").unwrap();
        let mut task = session.take_worker_task().unwrap();
        assert!(resolve_detection_task(&mut task));
        assert!(session.complete_worker_result(task, Ok(synthetic_raster(400, 100))));
        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        session.refresh_projection(&mut projection);
        let frame = session.viewport_frame(&mut projection).unwrap();
        let anchor = frame.math_blocks[0].anchor.clone();
        assert!(!session.scroll_math_block(&anchor, 0, 20));

        session.set_math_layout_options(MathLayoutOptions {
            line_wrapping: true,
            block_max_height_px: Some(nz(40)),
            ..MathLayoutOptions::default()
        });
        assert!(session.scroll_math_block(&anchor, 0, 20));
        let clamped = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(clamped.math_blocks[0].vertical_scroll_px, 20);
        assert_eq!(
            clamped.math_blocks[0].clip_height_subpixels,
            40 * SUBPIXELS_PER_PX
        );
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
    fn ed3_resets_the_compact_frozen_detection_boundary() {
        let mut session = DualPlaneSession::new(nz(16), nz(2));
        session.feed(b"```\r\ninside\r\ntail").unwrap();
        assert!(!session.frozen_detection_context.is_neutral());
        assert!(!session.frozen_detection_contexts.is_empty());

        session.feed(b"\x1b[3J").unwrap();
        assert!(session.frozen_detection_context.is_neutral());
        assert!(session.frozen_detection_contexts.is_empty());
    }

    fn complete_detected_live_tasks(session: &mut DualPlaneSession, raster: MathRaster) -> usize {
        let mut completed = 0;
        while let Some(mut task) = session.take_live_worker_task() {
            if resolve_live_detection_task(&mut task) {
                assert!(session.complete_live_worker_result(task, Ok(raster.clone())));
                completed += 1;
            } else {
                assert!(
                    session.complete_live_worker_result(task, Err(MathRenderError::NotDetected))
                );
            }
        }
        completed
    }

    #[test]
    fn live_window_keeps_fence_context_across_an_unstable_spinner_row() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(4));
        session
            .feed_at(b"\x1b[?1049h```rust\r\nspin-0\r\n$$x$$\x1b[2;1H", start)
            .unwrap();
        session
            .feed_at(b"\r\x1b[2Kspin-1", start + Duration::from_millis(100))
            .unwrap();

        assert_eq!(
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL),
            1,
            "the stable candidate still reaches the real live window builder"
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
            0,
            "the opener above an unstable row remains detector context"
        );
        let mut projection = session.new_projection(session.layout_key());
        assert!(
            session
                .viewport_frame(&mut projection)
                .unwrap()
                .math_blocks
                .is_empty()
        );
    }

    #[test]
    fn live_window_rejects_every_candidate_after_an_unclosed_visible_fence() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(5));
        session
            .feed_at(b"\x1b[?1049h```text\r\n$$x$$\r\n$$y$$", start)
            .unwrap();

        assert_eq!(
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL),
            2
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
            0
        );
        let mut projection = session.new_projection(session.layout_key());
        assert!(
            session
                .viewport_frame(&mut projection)
                .unwrap()
                .math_blocks
                .is_empty()
        );
    }

    #[test]
    fn alternate_truncated_prefix_never_pairs_a_closer_with_the_next_opener() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(5));
        session
            .feed_at(
                "\x1b[?1049h$$\r\n\\frac{a}{b}\r\n$$\r\nnarrative\r\n$$\r\n\\sigma(z)=1\r\n$$"
                    .as_bytes(),
                start,
            )
            .unwrap();
        assert!(
            !session.alternate_detection_context.is_neutral(),
            "the removed opener must remain the exact state before visible row 0"
        );
        assert!(
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL) >= 1,
            "the genuine block after the recovered boundary reaches the worker"
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 18)),
            1,
            "the genuine block after the phantom closer should still become Ready"
        );
        assert_eq!(session.live_decorations.len(), 1);
        assert!(session.live_decorations.values().all(|record| {
            record.artifact.is_some()
                && !record.span.render_source.contains("narrative")
                && record.span.render_source.contains(r"\sigma")
        }));
    }

    #[test]
    fn alternate_hidden_code_fence_still_suppresses_visible_math() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(4));
        session
            .feed_at(b"\x1b[?1049h```text\r\ncode\r\n$$\r\nx + y\r\n$$", start)
            .unwrap();
        assert!(!session.alternate_detection_context.is_neutral());
        assert_eq!(
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL),
            0,
            "no candidate inside a fence whose opener is above row 0 may be scheduled"
        );
        assert!(session.live_decorations.is_empty());
    }

    #[test]
    fn alternate_scrolled_cjk_prose_block_stays_native() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(5));
        session
            .feed_at(
                "\x1b[?1049hdiscard-0\r\ndiscard-1\r\ndiscard-2\r\n$$\r\n这是普通中文正文\r\n$$"
                    .as_bytes(),
                start,
            )
            .unwrap();
        assert!(session.alternate_detection_context.is_neutral());
        assert!(session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL) >= 1);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 18)),
            0,
            "the existing CJK prose guard must remain active after trust recovery"
        );
        assert!(session.live_decorations.is_empty());
    }

    #[test]
    fn live_scheduler_recognizes_all_supported_display_delimiters() {
        let start = Instant::now();
        for source in [
            "$$\r\nx + y\r\n$$",
            "\\[\r\nx + y\r\n\\]",
            "\\begin{align}\r\nx &= y + 1\r\n\\end{align}",
        ] {
            let mut session = DualPlaneSession::new(nz(40), nz(5));
            session
                .feed_at(format!("\x1b[?1049h{source}").as_bytes(), start)
                .unwrap();
            assert!(
                session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL) >= 1,
                "{source} was not scheduled"
            );
            assert_eq!(
                complete_detected_live_tasks(&mut session, synthetic_raster(40, 18)),
                1,
                "{source} was not detected"
            );
        }
    }

    #[test]
    fn alternate_full_scroll_then_detects_all_complete_display_delimiters_without_another_scroll() {
        let start = Instant::now();
        for source in [
            "$$\r\nx + y\r\n$$",
            "\\[\r\nx + y\r\n\\]",
            "\\begin{align}\r\nx &= y + 1\r\n\\end{align}",
        ] {
            let mut session = DualPlaneSession::new(nz(40), nz(5));
            session
                .feed_at(
                    format!("\x1b[?1049hdiscard-0\r\ndiscard-1\r\ndiscard-2\r\n{source}")
                        .as_bytes(),
                    start,
                )
                .unwrap();
            assert!(
                session.alternate_detection_context.is_neutral(),
                "ordinary removed rows leave a proven-neutral prefix: {source}"
            );
            assert!(
                session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL) >= 1,
                "the closed block was not scheduled after a full scroll: {source}"
            );
            assert_eq!(
                complete_detected_live_tasks(&mut session, synthetic_raster(40, 18)),
                1,
                "the closed block did not become Ready after a full scroll: {source}"
            );
            assert!(
                session
                    .live_decorations
                    .values()
                    .all(|record| record.artifact.is_some()),
                "a detected block was not Ready: {source}"
            );
        }
    }

    #[test]
    fn primary_live_window_starts_fence_state_in_the_transcript_tail() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(2));
        session
            .feed_at(b"```rust\r\nfrozen-inside\r\n$$x$$", start)
            .unwrap();
        assert!(
            session
                .document
                .entries()
                .values()
                .any(|entry| entry.line.text.trim() == "```rust"),
            "fixture must scroll the opener into primary transcript history"
        );

        assert_eq!(
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL),
            1
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
            0
        );
        let mut projection = session.new_projection(session.layout_key());
        assert!(
            session
                .viewport_frame(&mut projection)
                .unwrap()
                .math_blocks
                .is_empty()
        );
    }

    #[test]
    fn m1_9k_primary_live_detection_survives_long_history_and_tombstones() {
        let start = Instant::now();
        for source in ["$$x$$", "\\begin{align}x &= y\\end{align}"] {
            let mut long = DualPlaneSession::new(nz(40), nz(4));
            let prefix = (0..LIVE_FENCE_HISTORY_CONTEXT_LINES + 8)
                .map(|index| format!("history-{index}\r\n"))
                .collect::<String>();
            long.feed_at(format!("{prefix}{source}").as_bytes(), start)
                .unwrap();
            assert!(
                long.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL) >= 1,
                "long-history candidate was not scheduled: {source}"
            );
            assert_eq!(
                complete_detected_live_tasks(&mut long, synthetic_raster(32, 18)),
                1,
                "visible math must not be disabled by a >1024-line history: {source}"
            );

            let mut tombstoned =
                DualPlaneSession::with_frozen_quota(nz(40), nz(4), NonZeroUsize::new(2).unwrap());
            tombstoned
                .feed_at(
                    format!("old-0\r\nold-1\r\nold-2\r\nold-3\r\nold-4\r\nold-5\r\n{source}")
                        .as_bytes(),
                    start,
                )
                .unwrap();
            assert!(!tombstoned.transcript.tombstones().is_empty());
            assert!(
                tombstoned.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL) >= 1,
                "tombstoned candidate was not scheduled: {source}"
            );
            assert_eq!(
                complete_detected_live_tasks(&mut tombstoned, synthetic_raster(32, 18)),
                1,
                "visible math must not be disabled by a tombstone: {source}"
            );
        }
    }

    #[test]
    fn m1_9k_alternate_clear_repaint_starts_a_fresh_parser_snapshot() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(3));
        session
            .feed_at(b"\x1b[?1049h$$\r\nold-body\r\nold-tail\r\nscroll", start)
            .unwrap();
        assert!(!session.alternate_detection_context.is_neutral());

        session
            .feed_at(b"\x1b[2J\x1b[H$$x^2$$", start + Duration::from_millis(10))
            .unwrap();
        assert_eq!(
            session.advance_live_stability(start + Duration::from_millis(210)),
            1
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
            1,
            "a clear+home repaint must not inherit the removed snapshot's symmetric opener"
        );
        assert_eq!(session.live_decorations.len(), 1);
        assert_eq!(
            session
                .live_decorations
                .values()
                .next()
                .unwrap()
                .span
                .render_source,
            "x^2"
        );
    }

    #[test]
    fn m1_9k_soft_wrapping_does_not_change_live_detection() {
        let start = Instant::now();
        for columns in [40, 5] {
            let mut session = DualPlaneSession::new(nz(columns), nz(4));
            session.feed_at(b"$$x+y$$", start).unwrap();
            assert!(
                session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL) >= 1,
                "width {columns} did not schedule a delimiter candidate"
            );
            assert_eq!(
                complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
                1,
                "width {columns} changed the detection result"
            );
            assert_eq!(
                session
                    .live_decorations
                    .values()
                    .next()
                    .unwrap()
                    .span
                    .render_source,
                "x+y"
            );
        }
    }

    #[test]
    fn active_resize_epoch_never_exports_a_past_live_wake_deadline() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(4));
        session
            .feed_at(b"\x1b[?1049h$$x$$\r\nspin-0", start)
            .unwrap();
        let resized_at = start + Duration::from_millis(250);
        session.resize_at(nz(41), nz(4), resized_at).unwrap();
        session.mark_pty_resize_requested_at(
            nz(41),
            nz(4),
            resized_at + Duration::from_millis(200),
        );

        let mut immediate_timer_wakes = 0usize;
        for step in 0..20u64 {
            let observed_at = resized_at + Duration::from_millis(250 + step * 50);
            session
                .feed_at(format!("\r\x1b[2Kspin-{step}").as_bytes(), observed_at)
                .unwrap();
            assert_eq!(session.live_stability_deadline(), None);
            let wake_deadline = [
                session.resize_finish_deadline(),
                session.synchronized_update_deadline(),
                session.live_stability_deadline(),
            ]
            .into_iter()
            .flatten()
            .min();
            assert!(wake_deadline.is_none_or(|deadline| deadline > observed_at));
            immediate_timer_wakes +=
                usize::from(wake_deadline.is_some_and(|deadline| deadline <= observed_at));
        }
        assert_eq!(
            immediate_timer_wakes, 0,
            "one second of 20 Hz output adds no timer-driven wake loop"
        );
    }

    #[test]
    fn live_stable_render_damage_invalidate_and_restabilize_matrix() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session.feed_at(b"$$x^2$$\r\nbarrier", start).unwrap();
        assert_eq!(
            session.live_stability_deadline(),
            Some(start + LIVE_MATH_STABLE_INTERVAL)
        );
        assert_eq!(
            session.advance_live_stability(start + Duration::from_millis(199)),
            0
        );
        assert_eq!(
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL),
            1
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(60, 20)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let rendered = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(rendered.math_blocks.len(), 1);
        assert_eq!(
            rendered.math_blocks[0].artifact.render_scale_milli, 1000,
            "live display math must never acquire a lifecycle-specific scale"
        );
        assert_eq!(
            rendered.math_blocks[0].clip_height_subpixels,
            math_presentation_height_subpixels(
                20 * SUBPIXELS_PER_PX,
                default_math_padding_subpixels(),
            )
        );
        assert_eq!(rendered.status_text.as_deref(), Some("1 rows above"));
        assert!(!rendered.cells.iter().any(|cell| cell.text == "$"));

        session
            .feed_at(b"\x1b[1;1H\x1b[2Kplain", start + Duration::from_millis(210))
            .unwrap();
        let reverted = session.viewport_frame(&mut projection).unwrap();
        assert!(reverted.math_blocks.is_empty());
        assert!(
            reverted
                .row_map
                .iter()
                .all(|row| row.height_subpixels == SPIKE_CELL_HEIGHT_SUBPIXELS.get())
        );
        assert_eq!(reverted.status_text, None);
        assert_eq!(session.live_invalidation_count(), 1);

        session
            .feed_at(
                b"\x1b[1;1H\x1b[2K$$y^2$$",
                start + Duration::from_millis(220),
            )
            .unwrap();
        assert_eq!(
            session.advance_live_stability(start + Duration::from_millis(420)),
            1
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(60, 20)),
            1
        );
        assert_eq!(
            session
                .viewport_frame(&mut projection)
                .unwrap()
                .math_blocks
                .len(),
            1
        );
        session
            .feed_at(b"\x1b[2J", start + Duration::from_millis(430))
            .unwrap();
        let cleared = session.viewport_frame(&mut projection).unwrap();
        assert!(cleared.math_blocks.is_empty());
        assert_eq!(cleared.status_text, None);
        assert!(
            cleared
                .row_map
                .iter()
                .all(|row| row.height_subpixels == SPIKE_CELL_HEIGHT_SUBPIXELS.get())
        );
    }

    #[test]
    fn spinner_neighbor_does_not_block_static_formula_region() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session.feed_at(b"$$x$$\r\nspin-0", start).unwrap();
        session
            .feed_at(b"\r\x1b[2Kspin-1", start + Duration::from_millis(100))
            .unwrap();
        assert_eq!(
            session.advance_live_stability(start + Duration::from_millis(200)),
            1,
            "row 0 is stable even though row 1's spinner moved"
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(frame.math_blocks.len(), 1);
        assert!(
            frame
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("spin-1")
        );
    }

    #[test]
    fn completed_live_raster_ignores_spinner_churn_but_rejects_source_or_fence_changes() {
        let start = Instant::now();
        let mut spinner = DualPlaneSession::new(nz(40), nz(12));
        spinner.feed_at(b"$$x$$\r\nspin-0", start).unwrap();
        spinner.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        let mut task = spinner.take_live_worker_task().unwrap();
        assert!(resolve_live_detection_task(&mut task));
        spinner
            .feed_at(b"\r\x1b[2Kspin-1", start + Duration::from_millis(250))
            .unwrap();
        assert!(spinner.complete_live_worker_result(task, Ok(synthetic_raster(32, 18))));
        assert_eq!(spinner.stale_results(), 0);
        assert_eq!(spinner.live_detection_count(), 1);
        let mut projection = spinner.new_projection(spinner.layout_key());
        assert_eq!(
            spinner
                .viewport_frame(&mut projection)
                .unwrap()
                .math_blocks
                .len(),
            1,
            "an unrelated 100 ms spinner update does not discard or retry the raster"
        );

        let mut changed = DualPlaneSession::new(nz(40), nz(2));
        changed.feed_at(b"$$x$$\r\nused", start).unwrap();
        changed.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        let mut task = changed.take_live_worker_task().unwrap();
        assert!(resolve_live_detection_task(&mut task));
        changed
            .feed_at(b"\x1b[1;1H\x1b[2K$$y$$", start + Duration::from_millis(250))
            .unwrap();
        assert!(!changed.complete_live_worker_result(task, Ok(synthetic_raster(32, 18))));
        let mut projection = changed.new_projection(changed.layout_key());
        let source = changed.viewport_frame(&mut projection).unwrap();
        assert!(source.math_blocks.is_empty());
        assert!(source.cells.iter().any(|cell| cell.text == "$"));

        let mut fenced = DualPlaneSession::new(nz(40), nz(3));
        fenced.feed_at(b"plain\r\n$$z$$\r\nused", start).unwrap();
        fenced.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        let mut task = fenced.take_live_worker_task().unwrap();
        assert!(resolve_live_detection_task(&mut task));
        fenced
            .feed_at(
                b"\x1b[1;1H\x1b[2K```text",
                start + Duration::from_millis(250),
            )
            .unwrap();
        assert!(!fenced.complete_live_worker_result(task, Ok(synthetic_raster(32, 18))));
    }

    #[test]
    fn identical_tui_repaints_keep_the_borrowed_artifact_without_raster_retry() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session
            .feed_at(b"\x1b[?1049h$$x$$\r\n\r\nspin-0\r\ntail", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let artifact_key = session
            .live_decorations
            .get(&0)
            .and_then(|record| record.artifact.as_ref())
            .map(|artifact| artifact.key.clone())
            .unwrap();

        for repaint in 1..=5 {
            session
                .feed_at(
                    b"\x1b[H\x1b[2K$$x$$\x1b[2;1H\x1b[2K\x1b[3;1H\x1b[2Kspin-0\x1b[4;1H\x1b[2Ktail",
                    start + Duration::from_millis(210 + repaint * 10),
                )
                .unwrap();
            assert_eq!(
                session
                    .live_decorations
                    .get(&0)
                    .and_then(|record| record.artifact.as_ref())
                    .map(|artifact| artifact.key.as_str()),
                Some(artifact_key.as_str())
            );
        }

        for tick in 1..=5 {
            session
                .feed_at(
                    format!("\x1b[3;1H\x1b[2Kspin-{tick}").as_bytes(),
                    start + Duration::from_millis(300 + tick * 100),
                )
                .unwrap();
        }
        assert_eq!(session.live_detection_count(), 1);
        assert_eq!(session.live_invalidation_count(), 0);
        assert!(session.take_live_worker_task().is_none());
        let mut projection = session.new_projection(session.layout_key());
        assert_eq!(
            session
                .viewport_frame(&mut projection)
                .unwrap()
                .math_blocks
                .len(),
            1
        );
    }

    #[test]
    fn clear_home_bounded_reconcile_reuses_only_an_exact_m1_9k_occurrence() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(6));
        session.feed_at(b"\x1b[?1049h", start).unwrap();
        session.alternate_detection_context = DetectionContext::ambiguous();
        session.feed_at(b"\\[x\\]\r\ninput", start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 20)),
            1
        );
        assert_eq!(
            session.alternate_detection_context,
            DetectionContext::ambiguous()
        );
        let artifact_key = session
            .live_decorations
            .get(&0)
            .and_then(|record| record.artifact.as_ref())
            .map(|artifact| artifact.key.clone())
            .unwrap();
        let detections = session.live_detection_count();
        let before_inputs = session.live_detection_context();
        let before_prefixes = live_grid_parser_prefixes(
            &before_inputs,
            session.live_initial_detection_context(&before_inputs),
        );

        // Clear/home establishes a fresh Known prefix, while the original directional occurrence
        // was proven under an Ambiguous prefix. Exact prefix preservation therefore cannot prove
        // identity; M1.9k independently proves the same occurrence and bounded reconcile reuses
        // the old artifact and exact one-row band without scheduling or recomputing geometry.
        session
            .feed_at(
                b"\x1b[2J\x1b[H\\[x\\]\r\ninput",
                start + Duration::from_millis(210),
            )
            .unwrap();
        let after_inputs = session.live_detection_context();
        let after_prefixes = live_grid_parser_prefixes(
            &after_inputs,
            session.live_initial_detection_context(&after_inputs),
        );
        assert_ne!(before_prefixes.get(&0), after_prefixes.get(&0));
        let record = session.live_decorations.get(&0).unwrap();
        assert_eq!((record.band_start_row, record.band_end_row), (0, 0));
        assert_eq!(
            record
                .artifact
                .as_ref()
                .map(|artifact| artifact.key.as_str()),
            Some(artifact_key.as_str())
        );
        assert_eq!(session.live_detection_count(), detections);
        assert!(session.take_live_worker_task().is_none());
    }

    #[test]
    fn clear_home_reconcile_never_moves_a_formula_source_into_commonmark_code() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(6));
        session
            .feed_at(b"\x1b[?1049h$$x$$\r\ninput", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 20)),
            1
        );

        session
            .feed_at(
                b"\x1b[2J\x1b[H```text\r\n$$x$$\r\n```\r\ninput",
                start + Duration::from_millis(210),
            )
            .unwrap();
        assert!(session.live_decorations.is_empty());
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert!(frame.math_blocks.is_empty());
        assert!(frame.cells.iter().any(|cell| cell.text == "$"));
        // Mutation: raw source search or prefix-blind translation would wrongly retain the raster.
    }

    #[test]
    fn live_band_borrows_only_contiguous_blank_rows_and_invalidates_on_write() {
        let start = Instant::now();
        let mut primary = DualPlaneSession::new(nz(40), nz(12));
        primary.feed_at(b"$$x$$\r\n\r\nbarrier", start).unwrap();
        primary.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut primary, synthetic_raster(40, 40)),
            1
        );
        let record = primary.live_decorations.get(&0).unwrap();
        assert_eq!((record.band_start_row, record.band_end_row), (0, 1));
        let mut projection = primary.new_projection(primary.layout_key());
        let borrowed = primary.viewport_frame(&mut projection).unwrap();
        assert_eq!(borrowed.math_blocks.len(), 1);
        assert_eq!(
            borrowed.math_blocks[0].clip_height_subpixels,
            math_presentation_height_subpixels(
                40 * SUBPIXELS_PER_PX,
                default_math_padding_subpixels(),
            )
        );
        assert_eq!(borrowed.math_blocks[0].artifact.render_scale_milli, 1000);

        primary
            .feed_at(
                b"\x1b[2;1Hborrowed-row-now-used",
                start + Duration::from_millis(210),
            )
            .unwrap();
        let reverted = primary.viewport_frame(&mut projection).unwrap();
        assert!(reverted.math_blocks.is_empty());
        assert!(reverted.cells.iter().any(|cell| cell.text == "$"));

        let mut inflight = DualPlaneSession::new(nz(40), nz(12));
        inflight.feed_at(b"$$x$$\r\n\r\nbarrier", start).unwrap();
        inflight.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        let mut task = inflight.take_live_worker_task().unwrap();
        assert!(resolve_live_detection_task(&mut task));
        inflight
            .feed_at(
                b"\x1b[2;1Hwritten-during-raster",
                start + Duration::from_millis(210),
            )
            .unwrap();
        assert!(!inflight.complete_live_worker_result(task, Ok(synthetic_raster(40, 40))));

        let mut blocked = DualPlaneSession::new(nz(40), nz(12));
        blocked.feed_at(b"$$x$$\r\nbarrier\r\n", start).unwrap();
        blocked.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut blocked, synthetic_raster(40, 40)),
            1
        );
        assert_eq!(blocked.live_decorations.get(&0).unwrap().band_end_row, 0);
        let mut projection = blocked.new_projection(blocked.layout_key());
        let expanded = blocked.viewport_frame(&mut projection).unwrap();
        assert_eq!(expanded.math_blocks.len(), 1);
        assert_eq!(
            expanded.math_blocks[0].clip_height_subpixels,
            math_presentation_height_subpixels(
                40 * SUBPIXELS_PER_PX,
                default_math_padding_subpixels(),
            )
        );
        assert_eq!(expanded.math_blocks[0].artifact.render_scale_milli, 1000);
        assert_eq!(expanded.status_text.as_deref(), Some("2 rows above"));

        let mut capped = DualPlaneSession::new(nz(40), nz(12));
        capped
            .feed_at(b"$$x$$\r\n\r\n\r\n\r\nbarrier", start)
            .unwrap();
        capped.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut capped, synthetic_raster(40, 70)),
            1
        );
        assert_eq!(
            capped.live_decorations.get(&0).unwrap().band_end_row,
            2,
            "the third contiguous blank row remains outside the two-row borrowing cap"
        );

        let mut alternate = DualPlaneSession::new(nz(40), nz(12));
        alternate
            .feed_at(b"\x1b[?1049h$$a$$\r\n\r\nbarrier", start)
            .unwrap();
        alternate.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut alternate, synthetic_raster(40, 40)),
            1
        );
        assert_eq!(
            alternate.live_decorations.get(&0).unwrap().band_end_row,
            0,
            "alternate presentation must not borrow an adjacent terminal row"
        );
        let mut projection = alternate.new_projection(alternate.layout_key());
        assert_eq!(
            alternate
                .viewport_frame(&mut projection)
                .unwrap()
                .math_blocks
                .len(),
            1
        );

        let mut upward = DualPlaneSession::new(nz(40), nz(12));
        upward.feed_at(b"\r\n$$up$$\r\nbarrier", start).unwrap();
        upward.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut upward, synthetic_raster(40, 30)),
            1
        );
        let record = upward.live_decorations.get(&1).unwrap();
        assert_eq!((record.band_start_row, record.band_end_row), (0, 1));
        upward
            .feed_at(
                b"\x1b[1;1Hupper-row-now-used",
                start + Duration::from_millis(210),
            )
            .unwrap();
        assert!(upward.live_decorations.is_empty());
    }

    #[test]
    fn rendered_live_pixels_are_vertically_centered_in_the_owned_band() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session
            .feed_at(b"$$\r\nx + y\r\n$$\r\nbarrier", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 20)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let placement = &frame.math_blocks[0];
        let top_gap = placement.content_offset_subpixels;
        let bottom_gap = placement
            .clip_height_subpixels
            .saturating_sub(top_gap)
            .saturating_sub(
                i64::from(placement.artifact.height_px).saturating_mul(SUBPIXELS_PER_PX),
            );
        assert!(
            (top_gap - bottom_gap).abs() <= SUBPIXELS_PER_PX,
            "top={top_gap} bottom={bottom_gap}"
        );
    }

    #[test]
    fn bottom_frame_keeps_the_last_live_row_visible_and_alt_overflow_is_locally_reviewable() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session
            .feed_at(
                b"\x1b[?1049h$$x$$\r\nbarrier-1\r\nbarrier-2\r\nbottom",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let last_grid_row = session.terminal.visible_row(11).unwrap().cells;
        let mut projection = session.new_projection(session.layout_key());
        let bottom = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(session.terminal().dimensions(), (nz(40), nz(12)));
        assert_eq!(
            bottom.status_text.as_deref(),
            Some("2 rows above · Shift+wheel")
        );
        assert_eq!(
            &bottom.cells[11 * 40..12 * 40],
            last_grid_row.as_slice(),
            "free-height overflow must preserve the last grid row at rest"
        );
        assert_eq!(
            bottom.row_map[0].top_subpixels,
            SPIKE_CELL_HEIGHT_SUBPIXELS
                .get()
                .saturating_sub(math_presentation_height_subpixels(
                    40 * SUBPIXELS_PER_PX,
                    default_math_padding_subpixels(),
                ),)
        );
        assert_eq!(bottom.row_map[11].live_grid_row, Some(11));
        assert_eq!(
            bottom.row_map[11]
                .top_subpixels
                .saturating_add(bottom.row_map[11].height_subpixels),
            12 * SPIKE_CELL_HEIGHT_SUBPIXELS.get(),
            "the bottom logical row ends exactly at the fixed terminal pane bottom"
        );
        assert!(
            bottom
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("bottom")
        );

        projection.scroll_by_rows(1);
        let first_review = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            first_review.row_map[0].top_subpixels,
            bottom.row_map[0]
                .top_subpixels
                .saturating_add(SPIKE_CELL_HEIGHT_SUBPIXELS.get())
        );
        assert_eq!(
            first_review.status_text.as_deref(),
            Some("1 rows above · Shift+wheel")
        );
        assert!(matches!(
            first_review.viewport_origin,
            FrameViewportOrigin::LiveOverflow { rows_below: 1 }
        ));

        projection.scroll_by_rows(1);
        let full_review = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(full_review.row_map[0].top_subpixels, 0);
        assert_eq!(full_review.status_text.as_deref(), Some("2 rows below"));

        let mut no_overflow = DualPlaneSession::new(nz(40), nz(12));
        no_overflow.feed(b"bottom-only").unwrap();
        let last_grid_row = no_overflow.terminal.visible_row(11).unwrap().cells;
        let mut no_overflow_projection = no_overflow.new_projection(no_overflow.layout_key());
        let plain = no_overflow
            .viewport_frame(&mut no_overflow_projection)
            .unwrap();
        assert_eq!(plain.status_text, None);
        assert_eq!(
            &plain.cells[11 * 40..12 * 40],
            last_grid_row.as_slice(),
            "the no-overflow state also preserves the final terminal row byte-for-byte"
        );
        projection.scroll_to_bottom();
        assert_eq!(
            session.viewport_frame(&mut projection).unwrap().row_map[0].top_subpixels,
            bottom.row_map[0].top_subpixels
        );
    }

    #[test]
    fn primary_history_anchor_does_not_move_when_live_height_arrives() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        let initial = (0..16)
            .map(|row| format!("history-{row:02}"))
            .collect::<Vec<_>>()
            .join("\r\n");
        session.feed_at(initial.as_bytes(), start).unwrap();
        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_by_rows(1);
        let anchored_before = session.viewport_frame(&mut projection).unwrap();
        assert!(matches!(
            anchored_before.viewport_origin,
            FrameViewportOrigin::Anchored(_)
        ));
        assert_eq!(anchored_before.row_map[0].top_subpixels, 0);

        session
            .feed_at(b"\x1b[1;1H\x1b[2K$$x$$", start + Duration::from_millis(10))
            .unwrap();
        assert_eq!(
            session.advance_live_stability(start + Duration::from_millis(210)),
            1
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let anchored_after = session.viewport_frame(&mut projection).unwrap();
        assert!(matches!(
            anchored_after.viewport_origin,
            FrameViewportOrigin::Anchored(_)
        ));
        assert_eq!(
            anchored_after.row_map[0].top_subpixels, 0,
            "a projection-local live tail expansion must not move a primary document anchor"
        );
        assert_eq!(anchored_after.math_blocks.len(), 1);
        assert_eq!(
            anchored_after.math_blocks[0].artifact.render_scale_milli,
            1000
        );
    }

    #[test]
    fn live_visible_text_floor_renders_a_screenful_and_stably_prefers_newest_blocks() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(24));
        session
            .feed_at(b"$$x0$$\r\n$$x1$$\r\n$$x2$$\r\n$$x3$$\r\nbarrier", start)
            .unwrap();
        assert_eq!(
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL),
            4
        );
        let mut completed = 0;
        while let Some(mut task) = session.take_live_worker_task() {
            assert!(resolve_live_detection_task(&mut task));
            let heights = [18, 30, 24, 40];
            let height = heights[task.start.row as usize];
            assert!(session.complete_live_worker_result(task, Ok(synthetic_raster(40, height))));
            completed += 1;
        }
        assert_eq!(completed, 4);
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(frame.math_blocks.len(), 4);
        assert_eq!(
            frame
                .math_blocks
                .iter()
                .map(|block| block.source.as_str())
                .collect::<Vec<_>>(),
            ["x0", "x1", "x2", "x3"]
        );
        let max_formula_height =
            i64::from(24 - LIVE_MIN_VISIBLE_TEXT_ROWS) * SPIKE_CELL_HEIGHT_SUBPIXELS.get();
        assert!(
            frame
                .math_blocks
                .iter()
                .map(|block| block.clip_height_subpixels)
                .sum::<i64>()
                <= max_formula_height
        );

        let mut constrained = DualPlaneSession::new(nz(40), nz(12));
        constrained
            .feed_at(b"$$old$$\r\n$$middle$$\r\n$$new$$\r\nbarrier", start)
            .unwrap();
        constrained.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut constrained, synthetic_raster(40, 40)),
            3
        );
        let mut projection = constrained.new_projection(constrained.layout_key());
        let selected = (0..3)
            .map(|_| {
                constrained
                    .viewport_frame(&mut projection)
                    .unwrap()
                    .math_blocks
                    .iter()
                    .map(|block| block.source.clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(selected, vec![vec!["new".to_owned()]; 3]);
    }

    #[test]
    fn primary_resize_preserves_live_formula_as_stale_instead_of_flashing_to_source() {
        // Regression for the primary (Codex) resize flash: a proven live formula must survive a
        // window resize by re-anchoring its raster (demoted to stale) onto the reflowed grid, not
        // revert to source while re-detection catches up. Minimal real terminal output, no
        // synthetic viewport state. Before this fix `resize_at` wiped every live decoration on
        // primary, so the reflow frame flashed `$$x$$` back to source.
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session.feed_at(b"$$x$$\r\nbarrier", start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let before = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(before.math_blocks.len(), 1);
        assert_eq!(before.math_blocks[0].display, MathBlockDisplay::Rendered);
        assert_eq!(
            crate::observe_formula_frame(&before).state,
            crate::FormulaFrameState::Rendered
        );

        // A width resize reflows the grid, opens a resize transaction, and the vendor reconcile
        // (mark_pty_resize_requested_at) bumps the grid generation before the app's next frame.
        session
            .resize_at(nz(41), nz(12), start + Duration::from_millis(210))
            .unwrap();
        session.mark_pty_resize_requested_at(nz(41), nz(12), start + Duration::from_millis(210));
        session.refresh_projection(&mut projection);
        let resized = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(session.terminal().dimensions(), (nz(41), nz(12)));
        // Preserved, not flashed: the block still renders and the frame exposes no bare source.
        assert_eq!(
            resized.math_blocks.len(),
            1,
            "a proven formula must survive the reflow instead of reverting to source"
        );
        assert_eq!(resized.math_blocks[0].display, MathBlockDisplay::Rendered);
        assert_eq!(
            crate::observe_formula_frame(&resized).state,
            crate::FormulaFrameState::Rendered,
            "the reflow frame must not expose the formula's source"
        );

        // Once the transaction quiesces the relayout completes and a fresh raster replaces the
        // stale one; the block never passed through a source state.
        complete_detected_live_tasks(&mut session, synthetic_raster(41, 40));
        session.refresh_projection(&mut projection);
        let settled = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(settled.math_blocks.len(), 1);
        assert_eq!(settled.math_blocks[0].display, MathBlockDisplay::Rendered);
    }

    #[test]
    fn live_artifacts_cross_session_projection_and_frame_on_both_screens() {
        let start = Instant::now();
        let mut alternate = DualPlaneSession::new(nz(40), nz(13));
        alternate
            .feed_at(b"\x1b[?1049hbefore\r\n$$\r\nx + y\r\n$$\r\nafter", start)
            .unwrap();
        alternate.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut alternate, synthetic_raster(80, 70)),
            1
        );
        let mut projection = alternate.new_projection(alternate.layout_key());
        let frame = alternate.viewport_frame(&mut projection).unwrap();
        assert_eq!(frame.math_blocks.len(), 1);
        let placement = &frame.math_blocks[0];
        let MathBlockAnchor::Live { band_start_row, .. } = placement.anchor else {
            unreachable!();
        };
        assert_eq!(
            placement.top_subpixels,
            frame
                .row_map
                .iter()
                .find(|row| row.live_grid_row == Some(band_start_row))
                .unwrap()
                .top_subpixels
        );
        assert_eq!(
            placement.clip_height_subpixels,
            math_presentation_height_subpixels(
                70 * SUBPIXELS_PER_PX,
                default_math_padding_subpixels(),
            )
        );
        assert_eq!(
            placement.artifact.height_subpixels,
            math_presentation_height_subpixels(
                70 * SUBPIXELS_PER_PX,
                default_math_padding_subpixels(),
            )
        );
        assert_eq!(placement.artifact.render_scale_milli, 1000);
        let alternate_raster_top = placement
            .top_subpixels
            .saturating_add(placement.content_offset_subpixels);
        let alternate_raster_bottom = alternate_raster_top.saturating_add(
            i64::from(placement.artifact.height_px).saturating_mul(SUBPIXELS_PER_PX),
        );
        assert!(alternate_raster_top >= placement.top_subpixels);
        assert!(
            alternate_raster_bottom
                <= placement
                    .top_subpixels
                    .saturating_add(placement.clip_height_subpixels),
            "alternate placement/clip must contain the complete raster"
        );
        let text = frame
            .cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>();
        assert!(text.contains("before"));
        assert!(text.contains("after"));

        alternate
            .feed_at(
                b"\x1b[2;1H\x1b[2Kplain",
                start + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(10),
            )
            .unwrap();
        let rewritten = alternate.viewport_frame(&mut projection).unwrap();
        assert!(rewritten.math_blocks.is_empty());
        assert!(
            rewritten
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("plain")
        );

        let mut primary = DualPlaneSession::new(nz(40), nz(12));
        primary
            .feed_at(b"$$p^2$$\r\nprimary-neighbor", start)
            .unwrap();
        primary.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut primary, synthetic_raster(40, 18)),
            1
        );
        let mut projection = primary.new_projection(primary.layout_key());
        let frame = primary.viewport_frame(&mut projection).unwrap();
        assert_eq!(frame.math_blocks.len(), 1);
        let placement = &frame.math_blocks[0];
        assert_eq!(
            placement.clip_height_subpixels,
            math_presentation_height_subpixels(
                18 * SUBPIXELS_PER_PX,
                default_math_padding_subpixels(),
            )
        );
        let primary_raster_top = placement
            .top_subpixels
            .saturating_add(placement.content_offset_subpixels);
        let primary_raster_bottom = primary_raster_top.saturating_add(
            i64::from(placement.artifact.height_px).saturating_mul(SUBPIXELS_PER_PX),
        );
        assert!(primary_raster_top >= placement.top_subpixels);
        assert!(
            primary_raster_bottom
                <= placement
                    .top_subpixels
                    .saturating_add(placement.clip_height_subpixels),
            "primary placement/clip must contain the complete raster"
        );
        assert!(
            frame
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("primary-neighbor")
        );
    }

    #[test]
    fn display_box_is_tight_ink_plus_configurable_symmetric_padding() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session
            .feed_at(b"\x1b[?1049h$$x^2$$\r\ninput", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 20)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let padded = session.viewport_frame(&mut projection).unwrap();
        let block = &padded.math_blocks[0];
        let padding = default_math_padding_subpixels();
        let tight = 20 * SUBPIXELS_PER_PX;
        assert_eq!(block.artifact.height_subpixels, tight + 2 * padding);
        assert_eq!(block.clip_height_subpixels, tight + 2 * padding);
        assert_eq!(block.content_offset_subpixels, padding);
        assert_eq!(
            block
                .clip_height_subpixels
                .saturating_sub(block.content_offset_subpixels)
                .saturating_sub(tight),
            padding,
            "top and bottom padding must be symmetric"
        );

        session.set_math_layout_options(MathLayoutOptions {
            vertical_padding_cell_milli: 0,
            ..MathLayoutOptions::default()
        });
        let tight_only = session.viewport_frame(&mut projection).unwrap();
        let block = &tight_only.math_blocks[0];
        assert_eq!(block.artifact.height_subpixels, tight);
        assert_eq!(block.clip_height_subpixels, tight);
        assert_eq!(block.content_offset_subpixels, 0);
        // Mutation: ignoring the option or retaining the former 12.5% constant makes this red.
        assert_eq!(
            padded.math_blocks[0]
                .clip_height_subpixels
                .saturating_sub(block.clip_height_subpixels),
            2 * padding
        );
    }

    #[test]
    fn tall_display_raster_stays_inside_its_padded_block_clip() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(16));
        session
            .feed_at(
                b"\x1b[?1049h\\begin{align}\r\n\\nabla \\cdot E &= \\frac{rho}{epsilon} \\\\\r\n\\nabla \\times B &= J\r\n\\end{align}\r\ninput",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(80, 100)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert!(session.terminal_modes().alternate_screen);
        assert!(matches!(
            frame.cell_anchors[0].start,
            ContentAnchor::Live {
                screen: ScreenId::Alternate,
                ..
            }
        ));
        let block = &frame.math_blocks[0];
        let record = session.live_decorations.get(&0).unwrap();
        assert_eq!(
            (record.band_start_row, record.band_end_row),
            (0, 3),
            "alternate tall formulas expand pixels without borrowing terminal rows"
        );
        let raster_top = block
            .top_subpixels
            .saturating_add(block.content_offset_subpixels);
        let raster_bottom = raster_top.saturating_add(100 * SUBPIXELS_PER_PX);
        let clip_top = block.top_subpixels;
        let clip_bottom = clip_top.saturating_add(block.clip_height_subpixels);
        assert!(raster_top >= clip_top);
        assert!(raster_bottom <= clip_bottom);
        assert_eq!(
            raster_top.saturating_sub(clip_top),
            default_math_padding_subpixels()
        );
        // Mutation: box height = tight + one padding makes raster_bottom exceed clip_bottom.
        assert_eq!(
            clip_bottom.saturating_sub(raster_bottom),
            default_math_padding_subpixels()
        );
        assert_eq!(
            frame.status_text.as_deref(),
            Some("3 rows above · Shift+wheel")
        );
        assert!(
            frame
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("input")
        );
    }

    #[test]
    fn alternate_short_block_keeps_its_source_band_and_does_not_move_input() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(13));
        session
            .feed_at(
                b"\x1b[?1049h$$\r\n\r\n\r\nx = 1\r\n\r\n\r\n$$\x1b[13;1Hinput-line",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 20)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let block = &frame.math_blocks[0];
        let expected_box = math_presentation_height_subpixels(
            20 * SUBPIXELS_PER_PX,
            default_math_padding_subpixels(),
        );
        assert!(session.terminal_modes().alternate_screen);
        assert!(matches!(
            frame.cell_anchors[0].start,
            ContentAnchor::Live {
                screen: ScreenId::Alternate,
                ..
            }
        ));
        let MathBlockAnchor::Live {
            band_start_row,
            band_end_row,
            ..
        } = block.anchor
        else {
            unreachable!();
        };
        let band_height = frame
            .row_map
            .iter()
            .filter(|row| {
                row.live_grid_row
                    .is_some_and(|live| (band_start_row..=band_end_row).contains(&live))
            })
            .map(|row| row.height_subpixels)
            .sum::<i64>();
        let source_band_height =
            i64::from(band_end_row - band_start_row + 1) * SPIKE_CELL_HEIGHT_SUBPIXELS.get();
        assert!(expected_box < source_band_height);
        assert_eq!(band_height, source_band_height);
        assert_eq!(block.clip_height_subpixels, source_band_height);
        let tight = 20 * SUBPIXELS_PER_PX;
        let top_gap = block.content_offset_subpixels;
        let bottom_gap = block
            .clip_height_subpixels
            .saturating_sub(top_gap)
            .saturating_sub(tight);
        assert!((top_gap - bottom_gap).abs() <= 1);
        // Mutations back to M1.9m's presentation-box height or bottom-anchor slack make these red.
        assert_eq!(frame.row_map[0].top_subpixels, 0);
        assert_eq!(
            frame.status_text, None,
            "an expand-only short formula has neither top overflow nor synthetic top slack"
        );
        let last = frame.row_map.last().unwrap();
        assert_eq!(
            last.top_subpixels.saturating_add(last.height_subpixels),
            13 * SPIKE_CELL_HEIGHT_SUBPIXELS.get(),
            "the fixed-grid input row must remain at its terminal coordinate"
        );
        assert!(
            frame.cells[12 * 40..13 * 40]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("input-line")
        );
    }

    #[test]
    fn alternate_show_source_preference_survives_redetection_in_both_directions() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session
            .feed_at(b"\x1b[?1049h$$x^2$$\r\ninput", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 20)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let rendered = session.viewport_frame(&mut projection).unwrap();
        let anchor = rendered.math_blocks[0].anchor.clone();
        assert!(session.toggle_math_source(&anchor));

        session.redetect(DetectionRevision(2));
        assert_eq!(
            session.advance_live_stability(start + Duration::from_millis(400)),
            1
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 20)),
            1
        );
        let source = session.viewport_frame(&mut projection).unwrap();
        let source_block = source
            .math_blocks
            .iter()
            .find(|block| block.display == MathBlockDisplay::Source)
            .expect("content preference restores source after redetection");
        assert!(session.toggle_math_source(&source_block.anchor));

        session.redetect(DetectionRevision(3));
        assert_eq!(
            session.advance_live_stability(start + Duration::from_millis(600)),
            1
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 20)),
            1
        );
        let rendered_again = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(rendered_again.math_blocks.len(), 1);
        assert_eq!(
            rendered_again.math_blocks[0].display,
            MathBlockDisplay::Rendered
        );
        // Mutation: removing the content-preference lookup restores Rendered after revision 2.
    }

    #[test]
    fn multiline_live_band_uses_free_height_and_alt_exit_revokes_it() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(13));
        session
            .feed_at(b"\x1b[?1049h$$\r\nx + y\r\n$$", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(80, 70)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(frame.math_blocks.len(), 1);
        assert_eq!(
            frame.math_blocks[0].clip_height_subpixels,
            math_presentation_height_subpixels(
                70 * SUBPIXELS_PER_PX,
                default_math_padding_subpixels(),
            )
        );
        assert_eq!(frame.math_blocks[0].artifact.render_scale_milli, 1000);

        session
            .feed_at(b"\x1b[?1049l", start + Duration::from_millis(210))
            .unwrap();
        let restored = session.viewport_frame(&mut projection).unwrap();
        assert!(
            restored.math_blocks.is_empty(),
            "leaving alt invalidates every transient anchor"
        );
        assert_eq!(restored.status_text, None);
        assert!(
            restored
                .row_map
                .iter()
                .all(|row| row.height_subpixels == SPIKE_CELL_HEIGHT_SUBPIXELS.get())
        );

        let mut tiny = DualPlaneSession::new(nz(40), nz(1));
        tiny.feed_at(b"$$tiny$$", start).unwrap();
        tiny.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut tiny, synthetic_raster(40, 40)),
            1
        );
        let mut projection = tiny.new_projection(tiny.layout_key());
        let rendered = tiny.viewport_frame(&mut projection).unwrap();
        assert!(rendered.math_blocks.is_empty());
        assert!(rendered.cells.iter().any(|cell| cell.text == "$"));
        assert_eq!(rendered.status_text, None);
    }

    #[test]
    fn live_eye_copy_and_block_scroll_state_machine_is_per_block() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(16), nz(12));
        session.feed_at(b"$$x^2$$", start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(400, 18)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let anchor = frame.math_blocks[0].anchor.clone();
        assert_eq!(session.math_source(&anchor), Some("$$x^2$$"));
        assert!(session.set_math_hover(Some(&anchor)));
        assert!(session.toggle_math_source(&anchor));
        let source = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(source.math_blocks[0].display, MathBlockDisplay::Source);
        assert!(source.math_blocks[0].toolbar_visible);
        assert!(source.cells.iter().any(|cell| cell.text == "$"));
        assert!(session.toggle_math_source(&anchor));
        assert!(session.scroll_math_block(&anchor, 40, 0));
        let scrolled = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(scrolled.math_blocks[0].horizontal_scroll_px, 40);

        session.set_math_layout_options(MathLayoutOptions {
            line_wrapping: false,
            block_max_height_px: None,
            ..MathLayoutOptions::default()
        });
        assert!(!session.scroll_math_block(&anchor, 40, 0));
        assert_eq!(
            session.viewport_frame(&mut projection).unwrap().math_blocks[0].horizontal_overflow,
            HorizontalOverflowOwner::Pane
        );
    }

    #[test]
    fn live_show_source_placement_uses_row_map_after_an_expanded_block_above() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session
            .feed_at(
                b"$$upper$$\r\nbarrier-upper\r\n\r\n$$lower$$\r\nbarrier-lower",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 50)),
            2
        );
        let mut projection = session.new_projection(session.layout_key());
        let rendered = session.viewport_frame(&mut projection).unwrap();
        let lower_anchor = rendered
            .math_blocks
            .iter()
            .find_map(|block| match &block.anchor {
                MathBlockAnchor::Live { start, .. } if start.row == 3 => Some(block.anchor.clone()),
                _ => None,
            })
            .expect("lower block is rendered");

        projection.scroll_by_rows(i32::MAX);
        let reviewed = session.viewport_frame(&mut projection).unwrap();
        assert!(reviewed.status_text.is_some());
        assert!(session.toggle_math_source(&lower_anchor));
        let source = session.viewport_frame(&mut projection).unwrap();
        let placement = source
            .math_blocks
            .iter()
            .find(|block| block.display == MathBlockDisplay::Source && block.anchor == lower_anchor)
            .expect("lower source placement remains visible");
        let MathBlockAnchor::Live { band_start_row, .. } = placement.anchor else {
            unreachable!();
        };
        let band_top = source
            .row_map
            .iter()
            .find(|row| row.live_grid_row == Some(band_start_row))
            .unwrap()
            .top_subpixels;
        assert_eq!(placement.top_subpixels, band_top);
        assert_ne!(
            placement.top_subpixels,
            i64::from(band_start_row).saturating_mul(SPIKE_CELL_HEIGHT_SUBPIXELS.get()),
            "the expanded upper block must make row*cell_height observably wrong"
        );
    }

    #[test]
    fn primary_live_scroll_hands_off_the_exact_raster_without_another_worker_task() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(24), nz(2));
        session.feed_at(b"$$x^2$$\r\ntail", start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 18)),
            1
        );
        let live_artifact = session
            .live_decorations
            .get(&0)
            .and_then(|record| record.artifact.clone())
            .unwrap();
        session
            .feed_at(b"\r\nmore\r\nfinal", start + Duration::from_millis(210))
            .unwrap();
        assert!(session.live_decorations.is_empty());
        assert!(
            session.take_worker_task().is_none(),
            "handoff removes the redundant frozen raster task"
        );
        let frozen_artifact = session
            .decorations
            .values()
            .find_map(|record| record.artifact.as_ref())
            .expect("finalized formula reuses the live raster");
        assert_eq!(frozen_artifact.key, live_artifact.key);
        assert!(Arc::ptr_eq(&frozen_artifact.rgba, &live_artifact.rgba));
        assert_eq!(frozen_artifact.rgba.as_ref(), live_artifact.rgba.as_ref());
        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        session.refresh_projection(&mut projection);
        let frozen = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(frozen.math_blocks.len(), 1);
        assert_eq!(frozen.math_blocks[0].artifact.render_scale_milli, 1000);
        assert_eq!(
            frozen.math_blocks[0].artifact.rgba.as_ref(),
            live_artifact.rgba.as_ref(),
            "live and frozen frame artifacts are byte-identical"
        );
        assert!(Arc::ptr_eq(
            &frozen.math_blocks[0].artifact.rgba,
            &live_artifact.rgba
        ));
        let detections = session.live_detection_count;
        let frozen_detections = session.frozen_detection_count;
        let invalidations = session.live_invalidation_count;
        for _ in 0..8 {
            projection.scroll_to_bottom();
            session.refresh_projection(&mut projection);
            session.viewport_frame(&mut projection).unwrap();
            projection.scroll_to_top();
            session.refresh_projection(&mut projection);
            let reviewed = session.viewport_frame(&mut projection).unwrap();
            assert_eq!(reviewed.math_blocks.len(), 1);
            assert!(Arc::ptr_eq(
                &reviewed.math_blocks[0].artifact.rgba,
                &live_artifact.rgba
            ));
            assert!(session.take_math_worker_task().is_none());
        }
        assert_eq!(session.live_detection_count, detections);
        assert_eq!(session.frozen_detection_count, frozen_detections);
        assert_eq!(session.live_invalidation_count, invalidations);
    }

    #[test]
    fn pure_view_scroll_keeps_a_frozen_only_artifact_without_detection_or_raster_work() {
        let mut session = DualPlaneSession::new(nz(24), nz(3));
        session
            .feed(b"head\r\n$$x^2$$\r\nbody\r\nmore\r\ntail")
            .unwrap();
        let mut task = session.take_worker_task().expect("frozen formula task");
        assert!(resolve_detection_task(&mut task));
        assert!(session.complete_worker_result(task, Ok(synthetic_raster(40, 18))));
        let (id, rgba) = session
            .decorations
            .iter()
            .find_map(|(id, record)| {
                record
                    .artifact
                    .as_ref()
                    .map(|artifact| (*id, Arc::clone(&artifact.rgba)))
            })
            .expect("ready frozen-only artifact");
        let detections = session.frozen_detection_count();

        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        for _ in 0..12 {
            projection.scroll_to_top();
            session.refresh_projection(&mut projection);
            let reviewed = session.viewport_frame(&mut projection).unwrap();
            session.schedule_visible_artifacts(&reviewed);
            projection.scroll_to_bottom();
            session.refresh_projection(&mut projection);
            let live_boundary = session.viewport_frame(&mut projection).unwrap();
            session.schedule_visible_artifacts(&live_boundary);
            let current = session
                .decoration(id)
                .and_then(|record| record.artifact.as_ref())
                .expect("pure viewport motion keeps frozen pixels");
            assert!(Arc::ptr_eq(&rgba, &current.rgba));
            assert!(session.take_math_worker_task().is_none());
        }
        assert_eq!(session.frozen_detection_count(), detections);
    }

    #[test]
    fn full_screen_scroll_rebases_a_surviving_live_artifact_without_raster_work() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(24), nz(4));
        session
            .feed_at(b"head\r\n$$x^2$$\r\nbody\r\ntail", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 18)),
            1
        );
        let before = session
            .live_decorations
            .get(&1)
            .and_then(|record| record.artifact.as_ref())
            .map(|artifact| Arc::clone(&artifact.rgba))
            .unwrap();
        let detections = session.live_detection_count;
        let invalidations = session.live_invalidation_count;

        session
            .feed_at(b"\r\nnew-tail", start + Duration::from_millis(210))
            .unwrap();

        let after = session
            .live_decorations
            .get(&0)
            .and_then(|record| record.artifact.as_ref())
            .map(|artifact| Arc::clone(&artifact.rgba))
            .expect("the unchanged formula shifts with the full-screen scroll");
        assert!(Arc::ptr_eq(&before, &after));
        assert_eq!(session.live_detection_count, detections);
        assert_eq!(session.live_invalidation_count, invalidations);
        assert!(session.take_math_worker_task().is_none());
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

    #[test]
    fn occluded_clear_ranges_cover_source_on_both_sides_of_the_jump_chip() {
        fn ascii_boundaries(text: &str) -> Vec<(u32, u32)> {
            (0..=u32::try_from(text.len()).unwrap())
                .map(|index| (index, index))
                .collect()
        }
        let proven_text = r"\nabla \times \mathbf{B} = \mu_0 \mathbf{J}";
        let proven = ProvenLiveRow {
            band_offset: 0,
            text: proven_text.to_owned(),
            continues: false,
            cell_boundaries: ascii_boundaries(proven_text),
        };
        let chip = "Jump to bottom (ctrl+End)";
        let chip_start = 10_usize;
        let chip_end = chip_start + chip.len();
        let input_text = format!(
            "{}{chip}{}",
            &proven_text[..chip_start],
            &proven_text[chip_end..]
        );
        let input = LiveDetectionInput {
            source: LiveDetectionSource::Grid {
                row: 17,
                revision: 1,
            },
            text: input_text.clone(),
            continues: false,
            cell_boundaries: ascii_boundaries(&input_text),
        };
        // The chip overwrote columns 10..35 mid-source: the leaked prefix AND the leaked tail
        // after the chip are cleared, while every chip glyph keeps its text and style. Column 24
        // is a chip space which coincidentally equals the proven source's space at that column;
        // clearing a space's text is visually identical because cell styles are never touched.
        let expected_prefix = (0, u32::try_from(chip_start).unwrap());
        let coincidental_space = (24, 25);
        let expected_tail = (
            u32::try_from(chip_end).unwrap(),
            u32::try_from(proven_text.len()).unwrap(),
        );
        assert_eq!(
            proven.source_clear_ranges(&input),
            Some(vec![expected_prefix, coincidental_space, expected_tail])
        );

        // A row which does not carry this source (fixed chrome) must never produce clear ranges.
        let chrome_text = "────────────────";
        let chrome = LiveDetectionInput {
            source: LiveDetectionSource::Grid {
                row: 19,
                revision: 1,
            },
            text: chrome_text.to_owned(),
            continues: false,
            cell_boundaries: vec![(0, 0), (u32::try_from(chrome_text.len()).unwrap(), 16)],
        };
        assert_eq!(proven.source_clear_ranges(&chrome), None);
    }

    #[test]
    fn top_anchored_region_scroll_commits_rows_to_canonical_history() {
        // ratatui/Codex-style inline TUIs commit finalized lines by scrolling a DECSTBM region
        // anchored at row 1 whose bottom sits above their bottom viewport. xterm and the vendored
        // alacritty grid both rotate those rows into scrollback, so the transcript must capture
        // them; a region that never touches row 0 stays a local screen effect.
        let mut top_anchored = DualPlaneSession::new(nz(40), nz(10));
        top_anchored.feed(b"\x1b[1;6r\x1b[6;1H").unwrap();
        for index in 0..12 {
            top_anchored
                .feed(format!("committed-{index:02}\r\n").as_bytes())
                .unwrap();
        }
        let captured =
            top_anchored.transcript().staging_len() + top_anchored.transcript().frozen().len();
        assert!(
            captured >= 12,
            "12 committed rows plus the region's initial blanks must reach the transcript, got {captured}"
        );

        let mut mid_region = DualPlaneSession::new(nz(40), nz(10));
        mid_region.feed(b"\x1b[3;6r\x1b[6;1H").unwrap();
        for index in 0..12 {
            mid_region
                .feed(format!("local-{index:02}\r\n").as_bytes())
                .unwrap();
        }
        assert_eq!(
            mid_region.transcript().staging_len() + mid_region.transcript().frozen().len(),
            0,
            "a scroll region that never touches row 0 must stay out of canonical history"
        );
    }

    #[test]
    fn a_transcript_rewrite_preserves_the_review_offset_instead_of_jumping_to_bottom() {
        // Codex reflows by clearing scrollback (2J+3J) and reprinting equivalent content. The
        // anchored row the reviewer was reading dies with the clear, but their displacement is
        // still meaningful: the refilled history must restore the reading position.
        let mut session = DualPlaneSession::new(nz(40), nz(10));
        let mut lines = Vec::new();
        for index in 0..60 {
            lines.extend_from_slice(format!("line-{index:03}\r\n").as_bytes());
        }
        session.feed(&lines).unwrap();
        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_by_rows(20);
        session.refresh_projection(&mut projection);
        let reviewing = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(reviewing.scroll_offset_rows, 20);

        session.feed(b"\x1b[2J\x1b[3J\x1b[H").unwrap();
        session.refresh_projection(&mut projection);
        session.viewport_frame(&mut projection).unwrap();
        session.feed(&lines).unwrap();
        session.refresh_projection(&mut projection);
        let restored = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            restored.scroll_offset_rows, 20,
            "the reprint must restore the review displacement instead of leaving the bottom snap"
        );

        // An explicit jump supersedes the preservation.
        session.feed(b"\x1b[2J\x1b[3J\x1b[H").unwrap();
        session.refresh_projection(&mut projection);
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_bottom();
        session.feed(&lines).unwrap();
        session.refresh_projection(&mut projection);
        let bottom = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            bottom.scroll_offset_rows, 0,
            "an explicit jump to bottom must clear the preserved displacement"
        );
    }

    // Real byte sequence driving the a66eb84 residual: while reviewing, a resize opens the
    // transaction, Codex clears scrollback (2J+3J) and reprints. The scroll offset is already
    // preserved (a66eb84); these tests pin the *presentation hold* that removes the visible flash
    // to the bottom during the empty-history window, and its deterministic exits.
    fn scrolled_review_session(rows: u32) -> (DualPlaneSession, ViewportProjection, Vec<u8>) {
        let mut session = DualPlaneSession::new(nz(40), nz(rows));
        let mut lines = Vec::new();
        for index in 0..60 {
            lines.extend_from_slice(format!("line-{index:03}\r\n").as_bytes());
        }
        session.feed(&lines).unwrap();
        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_by_rows(20);
        session.refresh_projection(&mut projection);
        let reviewing = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(reviewing.scroll_offset_rows, 20);
        assert!(
            !projection.review_hold(),
            "no resize is in flight while simply reviewing"
        );
        (session, projection, lines)
    }

    // Drive the real resize→reflow→reprint transaction: open it, send the final PTY resize, let
    // Codex clear then reprint (both staged inside the vendor transaction), and quiesce it. Codex
    // output holds the transaction open until 200 ms of silence, so history refills only at harvest
    // — exactly where bt-app republishes. `finish_at` returns the instant past the quiescence
    // deadline for the final harvest+publish.
    fn run_reflow_reprint(
        session: &mut DualPlaneSession,
        projection: &mut ViewportProjection,
        reprint: &[u8],
        start: Instant,
    ) -> Instant {
        session.resize_at(nz(40), nz(12), start).unwrap();
        session.mark_pty_resize_requested_at(nz(40), nz(12), start + Duration::from_millis(10));
        session
            .feed_at(b"\x1b[2J\x1b[3J\x1b[H", start + Duration::from_millis(20))
            .unwrap();
        session.refresh_projection(projection);
        let empty_window = session.viewport_frame(projection).unwrap();
        assert_eq!(
            empty_window.scroll_offset_rows, 0,
            "history is empty during the reflow, so the frame itself can only sit at the bottom"
        );
        assert!(
            projection.review_hold(),
            "presentation must hold the last frame instead of flashing to the bottom on clear"
        );
        session
            .feed_at(reprint, start + Duration::from_millis(30))
            .unwrap();
        session.refresh_projection(projection);
        session.viewport_frame(projection).unwrap();
        assert!(
            projection.review_hold(),
            "the hold persists while the reprint is still staged inside the transaction"
        );
        start + Duration::from_millis(280)
    }

    #[test]
    fn a_resize_reflow_holds_presentation_across_the_empty_history_window() {
        let start = Instant::now();
        let (mut session, mut projection, lines) = scrolled_review_session(10);

        let finish_at = run_reflow_reprint(&mut session, &mut projection, &lines, start);

        // The transaction quiesces: the staged reprint freezes into history, the displacement
        // re-anchors, and the hold ends in the same frame — a direct hand-off, no bottom flash.
        assert!(session.finish_resize_if_quiescent(finish_at).unwrap());
        session.refresh_projection(&mut projection);
        let restored = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            restored.scroll_offset_rows, 20,
            "the reprint restores the review displacement"
        );
        assert!(
            !projection.review_hold(),
            "the hold releases once the transaction closes and the displacement re-anchors"
        );
    }

    #[test]
    fn an_explicit_takeover_during_the_hold_releases_it_and_the_reprint_no_longer_yanks_the_view() {
        let start = Instant::now();
        let (mut session, mut projection, lines) = scrolled_review_session(10);

        let finish_at = run_reflow_reprint(&mut session, &mut projection, &lines, start);

        // The user takes over mid-hold: an explicit scroll — or a keystroke, which routes through
        // scroll_to_bottom in bt-app — clears the preserved displacement immediately.
        projection.scroll_to_bottom();
        session.refresh_projection(&mut projection);
        let after = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(after.scroll_offset_rows, 0);
        assert!(
            !projection.review_hold(),
            "an explicit takeover releases the hold at once"
        );

        // The later harvest must not yank the view back up: the displacement was superseded.
        assert!(session.finish_resize_if_quiescent(finish_at).unwrap());
        session.refresh_projection(&mut projection);
        let bottom = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(bottom.scroll_offset_rows, 0);
        assert!(!projection.review_hold());
    }

    #[test]
    fn a_user_clear_without_a_resize_snaps_to_the_empty_bottom_without_holding() {
        let (mut session, mut projection, _lines) = scrolled_review_session(10);

        // No resize transaction is open: a genuine cls (2J+3J) must show the empty bottom rather
        // than hold the pre-clear frame. The offset is still preserved (a66eb84), but with nothing
        // to reprint it never re-anchors, and the user's next keystroke would clear it.
        session.feed(b"\x1b[2J\x1b[3J\x1b[H").unwrap();
        session.refresh_projection(&mut projection);
        let cleared = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(cleared.scroll_offset_rows, 0);
        assert!(
            !projection.review_hold(),
            "a user clear opens no resize transaction, so presentation shows the empty bottom"
        );
    }
}
