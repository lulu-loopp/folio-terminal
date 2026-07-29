use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    error::Error,
    fmt::{self, Write as _},
    fs::OpenOptions,
    hash::{DefaultHasher, Hash, Hasher},
    io::Write as _,
    num::{NonZeroI64, NonZeroU32, NonZeroUsize},
    ops::Bound,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bt_detect::{
    DecorationRecord, DelimiterKind, DetectionContext, DetectionInput, DetectionOptions,
    DetectionTask, LiveDetectionInput, LiveDetectionSource, LiveDetectionTask,
    MAX_MATH_SOURCE_BYTES, MathCellSegment, MathSourceLine, MathSpan, PlaceholderArtifact,
    StaleArtifact, advance_detection_context, detect_math_blocks_with_options,
    frozen_resync_scan_with_options, resolve_detection_task, resolve_live_detection_task,
    resolve_live_detection_tasks,
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
    cell_capture::{CapturedRowFingerprint, captured_row_is_blank},
    inline_image::{
        DecodedInlineImage, InlineImageDecodeError, InlineImageSource, InlineImageTask,
        decode_inline_image, detect_local_image_path_candidates,
    },
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
const INLINE_IMAGE_WORKER_QUEUE_CAP: usize = 4;
const LOCAL_IMAGE_PATH_WORKER_QUEUE_CAP: usize = 64;
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
    /// Detect drive-rooted image paths printed as terminal text. Product code opts in; deterministic
    /// replay and generic session construction stay closed unless explicitly enabled.
    pub detect_image_paths: bool,
}

impl Default for MathLayoutOptions {
    fn default() -> Self {
        Self {
            line_wrapping: true,
            block_max_height_px: None,
            vertical_padding_cell_milli: DEFAULT_MATH_VERTICAL_PADDING_CELL_MILLI,
            restore_stripped_environment_newlines: true,
            reject_claude_code_jump_chip_overlay: true,
            detect_image_paths: false,
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

#[derive(Clone, Debug)]
pub enum SessionDecorationTask {
    Math(Box<SessionMathTask>),
    InlineImage(InlineImageTask),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineImageRecordView {
    pub occurrence_id: u64,
    pub content_key: Option<String>,
    pub animated: bool,
    pub native_width_px: Option<u32>,
    pub native_height_px: Option<u32>,
    pub display_rows: Option<u32>,
    pub failed: bool,
    pub local_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
enum InlineImageRecordKind {
    Osc1337,
    LocalPath {
        path: PathBuf,
        source_text: String,
        start_anchor: AnchorId,
    },
}

#[derive(Clone, Debug)]
struct InlineImageRecord {
    occurrence_id: u64,
    end_anchor: AnchorId,
    kind: InlineImageRecordKind,
    artifact: Option<DecodedInlineImage>,
    failed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InlineImageGeometry {
    display_scale_milli: u32,
    display_height_subpixels: i64,
    display_rows: u32,
}

#[derive(Clone, Debug)]
struct DetectedLiveImagePath {
    path: PathBuf,
    source_text: String,
    start: GridPoint,
    end: GridPoint,
    stable: bool,
}

#[derive(Clone, Debug)]
struct LiveImagePathSegment {
    row: u32,
    byte_start: usize,
    byte_end: usize,
    boundaries: Vec<(u32, u32)>,
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
    /// Exact captured source rows which have left the live grid but have not finalized into
    /// transcript ids yet. Ordered by staging lineage immediately after `frozen_prefix`.
    staging_prefix: Vec<StagingId>,
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

/// A formula still shown on screen — a resident live decoration, a stale artifact awaiting relayout,
/// or an off-band hold — whose exact source the current detection scan no longer Owns (batch ③,
/// review §4). Pure diagnostic: reporting a `HeldUnbacked` never changes what is displayed. A
/// transient one is legitimate (a reprint/resize momentarily hides the source, or a block streams in
/// before it closes); one that persists to a quiescent final state is the "detector died, hold
/// survives" strand the third-round audit named — a hold masking dead detection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeldUnbackedRecord {
    /// The block opener's reconstructed source row (frozen-prefix head, else the live band start),
    /// so a known-legitimate long-lived form can be exempted by an exact source-line annotation
    /// rather than a blanket waiver.
    pub source_line: MathSourceLine,
    /// The exact hold key: the source the presentation layer preserves this record on.
    pub original_source: String,
    pub screen: ScreenId,
    pub band_start_row: u32,
    pub band_end_row: u32,
    /// The record is mid-relayout (its raster demoted to a stale artifact) rather than holding a live
    /// artifact — a common legitimate transient shape while a fresh relayout is in flight.
    pub stale: bool,
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
    occurrence_id: LiveMathOccurrenceId,
    span: MathSpan,
    artifact: PlaceholderArtifact,
    layout: LayoutKey,
    detection_revision: DetectionRevision,
    candidate_staging: StagingId,
    candidate_start: Option<TranscriptId>,
    expected_frozen_lines: u64,
    /// Proven source rows captured from the top of this still-live occurrence, in source order.
    /// These staging ids are populated before the terminal grid shifts; as they finalize, their
    /// transcript ids become the live record's frozen prefix so projection can bridge and suppress
    /// the complete source immediately instead of leaking it above the retained raster.
    prefix_staging: Vec<StagingId>,
    finalized_prefix_staging: usize,
    frozen_prefix: Vec<TranscriptId>,
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
    /// Frozen candidates whose in-flight scan was dropped (`block_is_current == false`) while the
    /// record was still the exact attempt we scheduled (`Frozen + Pending`, versions unchanged).
    /// The scheduler keeps no in-flight/retry record once `take` hands a task out, and
    /// `schedule_scan` no-ops on any non-`None` decoration, so such a drop would strand the block
    /// at source forever (only a width resize's layout bump rescues it). Re-arming is DEFERRED to a
    /// quiescent checkpoint (`rearm_stranded_pending`) rather than done at the drop site, so the
    /// per-frame visible scheduler never chases a still-reflowing/reprinting source into a
    /// reschedule storm. The value is the stranded attempt's `VersionStamp`, so a re-arm re-opens
    /// only that exact attempt and never cancels a newer scan the record moved on to. Empty at rest.
    stranded_pending: BTreeMap<TranscriptId, VersionStamp>,
    primary_parked: bool,
    cell_height_subpixels: NonZeroI64,
    cell_width_subpixels: NonZeroI64,
    ascii_baseline_subpixels: Option<NonZeroI64>,
    math_layout_options: MathLayoutOptions,
    live_screen: ScreenId,
    cursor_logical_line_memory: Option<CursorLogicalLineMemory>,
    /// Published logical lines currently covered by the positional edit gate. At most the cursor
    /// line plus CUP's preceding-nonblank extension, hence bounded by the live grid.
    active_edit_taints: Vec<LiveEditTaint>,
    /// Submitted edit-line instances still resident in the live grid. Overlapping entries replace
    /// each other, so this is bounded by the physical row count.
    committed_live_edit_taints: Vec<LiveEditTaint>,
    /// Removed tainted rows awaiting logical-line finalization. Transcript staging quota bounds it.
    edit_tainted_staging: BTreeMap<StagingId, EditTaintedRow>,
    /// Durable edit-line identities. Pruned with transcript eviction/clear, so frozen quota bounds it.
    edit_tainted_transcript: BTreeSet<TranscriptId>,
    alternate_detection_context: DetectionContext,
    live_rows: Vec<LiveRowStability>,
    live_tasks: VecDeque<LiveDetectionTask>,
    inline_image_tasks: VecDeque<InlineImageTask>,
    local_image_path_tasks: VecDeque<InlineImageTask>,
    inline_images: BTreeMap<u64, InlineImageRecord>,
    next_inline_image_occurrence_id: u64,
    live_decorations: BTreeMap<u32, LiveDecorationRecord>,
    next_live_occurrence_id: u64,
    offscreen_decorations: VecDeque<LiveDecorationRecord>,
    alternate_repaint_snapshot: Option<AlternateRepaintSnapshot>,
    alternate_repaint_in_progress: bool,
    /// True while a primary-screen in-stream transcript reprint is in flight (a clear+home /
    /// erase-storm / synchronized-update repaint boundary was seen and, for a DEC 2026 update, has
    /// not yet committed). It engages the same off-band preservation the resize path uses so a
    /// reflowing reprint re-anchors proven formulas by exact source equality instead of flashing
    /// them to source. See `primary_repaint_active`.
    primary_repaint_in_progress: bool,
    /// Records held resident-and-suppressed for the span of a primary in-stream reprint window (the
    /// same off-band snapshot alternate takes on a repaint boundary). While it is `Some`,
    /// `observe_live_damage` does not invalidate a decorated row: the proven raster keeps rendering
    /// over the rows Codex is rewriting (suppression) instead of the record being drained off-band
    /// and its source flashing through. `finish_primary_repaint` reprojects the snapshot onto the
    /// reflowed grid by proven-row fingerprint (`segmented_row_mapping` + `project_live_record`),
    /// which tracks a progressive/partial reprint the exact-source restore path cannot. Primary
    /// only; alternate keeps its own `alternate_repaint_snapshot`.
    primary_repaint_snapshot: Option<AlternateRepaintSnapshot>,
    /// Set while a primary reprint window is open once a row under it actually changed. A
    /// same-content repaint leaves it false, so the window closes without paying for the segmented
    /// reprojection — nothing moved, every resident record is already correctly placed.
    primary_repaint_dirty: bool,
    /// Occurrences whose stale-pending DPI raster is currently unmatched off-band after a proven
    /// primary reprint boundary. Presentation holds the last complete frame while any such exact
    /// source witness remains unmatched. Occurrence identity makes retirement final: a later,
    /// unrelated DPI record cannot inherit an old hold.
    primary_reprint_hold_occurrences: BTreeMap<LiveMathOccurrenceId, PrimaryReprintHistoryFloor>,
    /// Transcript tail before the currently open primary reprint. A frozen decoration may replace
    /// an unmatched live occurrence only when its durable id is newer than this watermark; this
    /// prevents an older equal-source formula elsewhere in history from spuriously releasing hold.
    primary_reprint_history_floor: Option<PrimaryReprintHistoryFloor>,
    alternate_content_end_row: Option<u32>,
    /// User presentation choices are content state, not decoration-instance state. Entries are
    /// created only by an explicit toggle and live for the session, so alternate-screen repaint,
    /// redetection, grid-generation changes, and layout changes cannot reset the choice.
    math_source_preferences: HashMap<MathSourcePreferenceKey, bool>,
    pending_live_handoffs: Vec<PendingLiveArtifactHandoff>,
    frozen_detection_context: DetectionContext,
    frozen_detection_contexts: BTreeMap<TranscriptId, DetectionContext>,
    /// Certified repair frontier (review §B): the last frozen id through which the resync scanner
    /// has proven the parser phase is CLEANLY neutral — every structural delimiter up to it is owned
    /// by a valid block and nothing is left open. A frozen candidate's scan window is anchored at the
    /// first id past this frontier (a proven `Known` neutral start), so the shared phantom witness can
    /// abandon a lost-opener `$$` and re-synchronise the blocks below without the poisoned dumb-parity
    /// `required_start` (which the frontier demotes to a cheap fallback hint). `None` means nothing is
    /// certified yet (the anchor is the earliest resident line). Advances monotonically within a source
    /// revision; reset only on a staging clear.
    frozen_certified_through: Option<TranscriptId>,
    frozen_detection_count: u64,
    live_detection_count: u64,
    live_invalidation_count: u64,
    math_failure_validate_count: u64,
    math_failure_convert_count: u64,
    math_failure_compile_count: u64,
    /// `BT_DECOR_TRACE=<path>` real-machine decoration-state trace target, resolved once at
    /// construction. `None` (variable unset) makes `trace_decorations` a single `Option` check and
    /// return — zero hot-path cost. See `trace_decorations`.
    decor_trace: Option<PathBuf>,
    decor_trace_frame: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PrimaryReprintHistoryFloor(Option<TranscriptId>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CursorLogicalLineMemory {
    screen: ScreenId,
    start: u32,
    end: u32,
    explicitly_positioned: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CursorLineSuppression {
    cursor_line: (u32, u32),
    preceding_nonblank_line: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EditTaintedRow {
    text: String,
    continues: bool,
}

impl EditTaintedRow {
    fn from_captured(row: &CapturedRow) -> Self {
        Self {
            text: captured_row_text(row),
            continues: row.continues,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LiveEditTaint {
    screen: ScreenId,
    start: u32,
    rows: Vec<EditTaintedRow>,
}

impl LiveEditTaint {
    fn end(&self) -> u32 {
        self.start
            .saturating_add(u32::try_from(self.rows.len().saturating_sub(1)).unwrap_or(u32::MAX))
    }

    fn intersects(&self, start: u32, end: u32) -> bool {
        ranges_intersect(start, end, self.start, self.end())
    }

    fn row_matches(&self, row: u32, content: &EditTaintedRow) -> bool {
        row.checked_sub(self.start)
            .and_then(|offset| self.rows.get(offset as usize))
            == Some(content)
    }
}

impl CursorLineSuppression {
    fn contains(self, row: u32) -> bool {
        self.intersects(row, row)
    }

    fn intersects(self, start: u32, end: u32) -> bool {
        ranges_intersect(start, end, self.cursor_line.0, self.cursor_line.1)
            || self
                .preceding_nonblank_line
                .is_some_and(|line| ranges_intersect(start, end, line.0, line.1))
    }
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
            stranded_pending: BTreeMap::new(),
            primary_parked: false,
            cell_height_subpixels,
            cell_width_subpixels: NonZeroI64::new(9 * SUBPIXELS_PER_PX).unwrap(),
            ascii_baseline_subpixels: None,
            math_layout_options: MathLayoutOptions::default(),
            live_screen: ScreenId::Primary,
            cursor_logical_line_memory: None,
            active_edit_taints: Vec::new(),
            committed_live_edit_taints: Vec::new(),
            edit_tainted_staging: BTreeMap::new(),
            edit_tainted_transcript: BTreeSet::new(),
            alternate_detection_context: DetectionContext::default(),
            live_rows: vec![LiveRowStability::default(); rows.get() as usize],
            live_tasks: VecDeque::new(),
            inline_image_tasks: VecDeque::new(),
            local_image_path_tasks: VecDeque::new(),
            inline_images: BTreeMap::new(),
            next_inline_image_occurrence_id: 1,
            live_decorations: BTreeMap::new(),
            next_live_occurrence_id: 1,
            offscreen_decorations: VecDeque::new(),
            alternate_repaint_snapshot: None,
            alternate_repaint_in_progress: false,
            primary_repaint_in_progress: false,
            primary_repaint_snapshot: None,
            primary_repaint_dirty: false,
            primary_reprint_hold_occurrences: BTreeMap::new(),
            primary_reprint_history_floor: None,
            alternate_content_end_row: None,
            math_source_preferences: HashMap::new(),
            pending_live_handoffs: Vec::new(),
            frozen_detection_context: DetectionContext::default(),
            frozen_detection_contexts: BTreeMap::new(),
            frozen_certified_through: None,
            frozen_detection_count: 0,
            live_detection_count: 0,
            live_invalidation_count: 0,
            math_failure_validate_count: 0,
            math_failure_convert_count: 0,
            math_failure_compile_count: 0,
            decor_trace: std::env::var_os("BT_DECOR_TRACE").map(PathBuf::from),
            decor_trace_frame: 0,
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
        if self.math_layout_options.detect_image_paths && !options.detect_image_paths {
            let retired = self
                .inline_images
                .values()
                .filter_map(|record| {
                    matches!(record.kind, InlineImageRecordKind::LocalPath { .. })
                        .then_some(record.occurrence_id)
                })
                .collect::<BTreeSet<_>>();
            self.retire_inline_images(&retired);
        }
        self.math_layout_options = options;
    }

    fn detection_options(&self) -> DetectionOptions {
        DetectionOptions {
            restore_stripped_environment_newlines: self
                .math_layout_options
                .restore_stripped_environment_newlines,
            restore_stripped_inline_environment_newlines: self.live_screen == ScreenId::Primary
                && self
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

    pub fn inline_image_records(&self) -> Vec<InlineImageRecordView> {
        self.inline_images
            .values()
            .map(|record| {
                let display_rows = record
                    .artifact
                    .as_ref()
                    .and_then(|artifact| self.inline_image_geometry(record, artifact))
                    .map(|geometry| geometry.display_rows);
                InlineImageRecordView {
                    occurrence_id: record.occurrence_id,
                    content_key: record
                        .artifact
                        .as_ref()
                        .map(|artifact| artifact.key.clone()),
                    animated: record
                        .artifact
                        .as_ref()
                        .is_some_and(|artifact| artifact.animated),
                    native_width_px: record.artifact.as_ref().map(|artifact| artifact.width_px),
                    native_height_px: record.artifact.as_ref().map(|artifact| artifact.height_px),
                    display_rows,
                    failed: record.failed,
                    local_path: match &record.kind {
                        InlineImageRecordKind::Osc1337 => None,
                        InlineImageRecordKind::LocalPath { path, .. } => Some(path.clone()),
                    },
                }
            })
            .collect()
    }

    /// Resolve a click capability only from a local-path record whose worker validation and decode
    /// succeeded. Path-looking terminal text that is pending or failed is intentionally inert.
    pub fn decoded_local_image_path_at(&self, anchor: &ContentAnchor) -> Option<PathBuf> {
        self.inline_images.values().find_map(|record| {
            record.artifact.as_ref()?;
            let InlineImageRecordKind::LocalPath {
                path, start_anchor, ..
            } = &record.kind
            else {
                return None;
            };
            let start = self.document.anchor(*start_anchor).ok()?;
            let end = self.document.anchor(record.end_anchor).ok()?;
            content_anchor_between(anchor, start, end).then(|| path.clone())
        })
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
        let cursor_memory_reprint_boundary = contains_clear_home_snapshot_boundary(bytes);
        if cursor_memory_reprint_boundary {
            self.cursor_logical_line_memory = None;
            self.clear_live_edit_taints();
        }
        let primary_reprint_boundary = self.live_screen == ScreenId::Primary
            && !self.terminal.modes().alternate_screen
            && cursor_memory_reprint_boundary;
        if primary_reprint_boundary && self.primary_reprint_history_floor.is_none() {
            self.primary_reprint_history_floor = Some(PrimaryReprintHistoryFloor(
                self.document.entries().keys().next_back().copied(),
            ));
        }
        if self.alternate_repaint_snapshot.is_none() {
            self.alternate_repaint_snapshot = self.begin_alternate_repaint(bytes);
        }
        self.alternate_repaint_in_progress = self.alternate_repaint_snapshot.is_some();
        // Primary in-stream reprint preservation: a Codex transcript reflow/reprint would otherwise
        // drop proven live formulas to source between the reprint and re-detection. Engage the same
        // suppress-and-remap alternate uses on a repaint boundary: while the window is open the
        // proven raster keeps rendering over the rows being rewritten (see `observe_live_damage`),
        // and `finish_primary_repaint` at the window's close reprojects each record onto the
        // reflowed grid by proven-row fingerprint. A DEC 2026 synchronized-update reprint withholds
        // its damage until the commit, so the flag stays engaged (it re-arms below on every feed)
        // until the update closes; the snapshot, taken once when the window opens, spans it.
        if self.live_screen == ScreenId::Primary
            && (self.primary_repaint_in_progress || primary_reprint_boundary)
        {
            self.primary_repaint_in_progress = true;
            // The reprint window and the resize transaction (`primary_resize_preservation_active`,
            // 002acc7) coexist: both drain/hold the same records off-band, and the reprint's
            // segmented reprojection re-anchors resize-reflowed blocks the resize path's exact-source
            // restore could not (measured: it keeps resize-repro's rapid-drag frames rendered). They
            // do not contend for ownership — a record lives in exactly one of `live_decorations` or
            // `offscreen_decorations`, and `finish_primary_repaint` merges the off-band queue that
            // resize drained rather than rebuilding over it.
            if self.primary_repaint_snapshot.is_none() {
                self.primary_repaint_snapshot = self.snapshot_primary_repaint();
            }
        }
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
        let mut cursor_stream_progressed = false;
        let result = (|| {
            for chunk in bytes.chunks(PARSE_QUANTUM) {
                let events = self.terminal.feed(chunk);
                cursor_stream_progressed |= self.terminal.cursor_stream_line_progressed();
                let damage = self.terminal.take_damage();
                self.apply_events(events, observed_at)?;
                self.observe_live_damage(damage, observed_at);
                self.sync_staging_tail();
            }
            Ok(())
        })();
        if result.is_err() {
            self.cursor_logical_line_memory = None;
            self.clear_live_edit_taints();
            self.alternate_repaint_snapshot = None;
            self.alternate_repaint_in_progress = false;
            self.primary_repaint_in_progress = false;
            self.primary_repaint_snapshot = None;
            self.primary_repaint_dirty = false;
            self.primary_reprint_history_floor = None;
            self.invalidate_all_live_decorations();
        } else if self.synchronized_update_deadline().is_none() {
            if let Some(snapshot) = self.alternate_repaint_snapshot.take() {
                self.finish_alternate_repaint(snapshot);
            }
            if let Some(snapshot) = self.primary_repaint_snapshot.take() {
                self.finish_primary_repaint(snapshot, false);
            }
        }
        self.alternate_repaint_in_progress = self.alternate_repaint_snapshot.is_some();
        if result.is_ok() {
            // Re-seat already-known path occurrences immediately after an atomic repaint. New
            // candidates and retirement still wait for the ordinary stability gate below.
            self.reconcile_live_image_paths(false, &vec![false; self.live_rows.len()]);
            self.restore_offscreen_decorations();
            self.reconcile_primary_reprint_presentation_hold(primary_reprint_boundary);
            // The reprint has landed and its records are re-anchored: end preservation unless a
            // synchronized update is still buffering the repaint (its damage arrives at the commit).
            if self.synchronized_update_deadline().is_none() {
                self.primary_repaint_in_progress = false;
                self.primary_reprint_history_floor = None;
            }
            // An open synchronized repaint still publishes the pre-transaction grid. Its boundary
            // invalidated the old cursor line above, so do not immediately memorize that stale
            // cursor again; ESU or the parser timeout records the committed cursor instead.
            if !(cursor_memory_reprint_boundary && self.synchronized_update_deadline().is_some()) {
                self.remember_visible_cursor_logical_line();
                self.update_edit_taints_after_feed(cursor_stream_progressed);
            }
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
            self.cursor_logical_line_memory = None;
            self.clear_live_edit_taints();
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
        let primary_resize = alternate_resize
            .is_none()
            .then(|| self.snapshot_primary_resize_transition())
            .flatten();
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
        // Keep the established whole-source restore for ordinary records. Transition-family
        // records stay deferred for the scoped geometric projector below; exact restore can
        // otherwise seat the narrower environment first and suppress the outer owner's proof.
        if primary_resize.is_some() {
            self.restore_offscreen_decorations_except_resize_transition();
        } else {
            self.restore_offscreen_decorations();
        }
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
        if let Some(snapshot) = primary_resize {
            // A terminal resize is itself a deterministic primary-grid repaint. Exact whole-source
            // matching is insufficient when reflow changes row boundaries or continuation
            // indentation, even though several byte/cell-exact rows still prove the occurrence's
            // new placement. Reuse the primary repaint projector before publishing the resize
            // frame; its unique row proof preserves only unambiguous records, and unresolved ones
            // remain off-band for the existing exact-source fallback.
            self.finish_primary_repaint(snapshot, true);
            self.restore_offscreen_decorations();
        }
        self.remember_visible_cursor_logical_line();
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
        let primary_reprint_boundary = self.primary_repaint_in_progress;
        let events = self.terminal.finish_synchronized_update();
        let damage = self.terminal.take_damage();
        if let Err(error) = self.apply_events(events, observed_at) {
            self.alternate_repaint_snapshot = None;
            self.alternate_repaint_in_progress = false;
            self.primary_repaint_in_progress = false;
            self.primary_repaint_snapshot = None;
            self.primary_repaint_dirty = false;
            self.primary_reprint_history_floor = None;
            self.invalidate_all_live_decorations();
            return Err(error);
        }
        self.observe_live_damage(damage, observed_at);
        self.sync_staging_tail();
        if let Some(snapshot) = self.alternate_repaint_snapshot.take() {
            self.finish_alternate_repaint(snapshot);
        }
        if let Some(snapshot) = self.primary_repaint_snapshot.take() {
            self.finish_primary_repaint(snapshot, false);
        }
        self.alternate_repaint_in_progress = false;
        self.restore_offscreen_decorations();
        self.reconcile_primary_reprint_presentation_hold(primary_reprint_boundary);
        // The synchronized-update reprint has committed and re-anchored: end primary preservation.
        self.primary_repaint_in_progress = false;
        self.primary_reprint_history_floor = None;
        self.remember_visible_cursor_logical_line();
        Ok(true)
    }

    pub fn mark_pty_resize_requested_at(
        &mut self,
        columns: NonZeroU32,
        rows: NonZeroU32,
        observed_at: Instant,
    ) -> bool {
        let reconciled = self.resize_epoch.is_active();
        let primary_reconcile = reconciled
            .then(|| self.snapshot_primary_resize_transition())
            .flatten();
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
            if let Some(snapshot) = primary_reconcile {
                // Reconciliation is a second deterministic reflow boundary after `resize_at`.
                // Preserve the just-projected occurrence by the same unique row proof; draining it
                // and attempting only a whole-source match loses blocks whose continuation
                // indentation changed at this edge.
                self.retain_live_decorations_offscreen();
                self.restore_offscreen_decorations_except_resize_transition();
                self.finish_primary_repaint(snapshot, true);
                self.restore_offscreen_decorations();
            } else if self.primary_resize_preservation_active() {
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
        // The transaction is over: fresh detection is authoritative again, so drop ordinary
        // primary holds whose exact source never reappeared on the reflowed grid. A stale-pending
        // DPI-transition record is different: its old-DPI raster is the in-flight relayout witness,
        // and Codex's byte-identical clean reprint can arrive just after this quiescence edge. Keep
        // that bounded, off-band witness until exact-source re-anchoring installs it or a hard
        // lifecycle boundary retires it; it is never painted while unmatched. Clearing it here made
        // quiescence itself masquerade as the fresh-raster completion event and exposed source
        // through the remaining worker interval (the zoom-end flash). Width-only resize rasters
        // retain the established quiescence release: extending those holds creates extra transition
        // occurrences in ordinary resize/reflow recordings without helping a DPI zoom.
        // (Alternate keeps its whole queue across repaints, so it is left untouched.)
        if self.live_screen == ScreenId::Primary {
            self.offscreen_decorations
                .retain(stale_pending_dpi_transition);
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

    /// Env-gated (`BT_DECOR_TRACE=<path>`) real-machine decoration-state trace. When the variable is
    /// unset this is a single `Option` check and return — zero hot-path cost, nothing is touched.
    /// When set, each invocation appends one snapshot: every frozen math record (any non-`None`
    /// decoration, or a source line that is still a display-math candidate) with its lifecycle
    /// state, failure reason, and source excerpt, followed by every live decoration. A user
    /// reproducing a stuck-source block on their machine then reads the offending record's exact
    /// state straight from the file — the `Pending` liveness hole, a `Failed` render, a `None`
    /// candidate that never re-scheduled — instead of relying on a replay that does not reproduce
    /// the timing race. No lock is taken and the frozen/live maps are only read.
    pub fn trace_decorations(&mut self) {
        let Some(path) = self.decor_trace.clone() else {
            return;
        };
        self.decor_trace_frame += 1;
        let epoch_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_micros());
        let mut out = String::new();
        let _ = write!(
            out,
            "DECOR_TRACE frame={} epoch_us={epoch_us} screen={:?} resize={}",
            self.decor_trace_frame,
            self.live_screen,
            if self.resize_epoch.is_active() {
                "active"
            } else {
                "idle"
            },
        );
        out.push('\n');
        for (id, record) in &self.decorations {
            let source = self
                .document
                .entries()
                .get(id)
                .map(|entry| entry.line.text.as_str())
                .unwrap_or("");
            if record.decoration == DecorationLifecycle::None && !may_contain_display_math(source) {
                continue;
            }
            let _ = write!(
                out,
                "  FROZEN id={} state={} reason={} src=\"{}\"",
                id.0,
                decoration_state_label(record.decoration),
                record.failure_reason.as_deref().unwrap_or("-"),
                decor_trace_excerpt(source),
            );
            out.push('\n');
        }
        for (row, record) in &self.live_decorations {
            let state = if record.failure_reason.is_some() {
                "failed"
            } else if record.artifact.is_some() {
                "rendered"
            } else if record.stale_artifact.is_some() {
                "stale"
            } else {
                "pending"
            };
            let _ = write!(
                out,
                "  LIVE row={row} band={}-{} state={state} reason={} src=\"{}\"",
                record.band_start_row,
                record.band_end_row,
                record.failure_reason.as_deref().unwrap_or("-"),
                decor_trace_excerpt(&record.span.original_source),
            );
            out.push('\n');
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(out.as_bytes());
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
        self.schedule_live_artifacts(&stable)
    }

    /// Schedule new live-grid decoration records from rows which have already passed the ordinary
    /// damage-derived stability window. The only additional gate is the terminal's published-grid
    /// cursor fact: while DECTCEM exposes the cursor, the complete WRAPLINE-linked logical line
    /// containing it is ineligible for a new image or math band. When that logical line is blank and
    /// CUP/HVP most recently placed the cursor there, its nearest preceding nonblank logical line
    /// joins the gate; natural LF/CR stream progression never enables that extension. While DECTCEM
    /// is temporarily off, the last visible logical line and its placement kind remain the gate
    /// until a deterministic grid boundary clears them.
    ///
    /// Existing records are deliberately outside this policy. Image occurrences are matched and
    /// re-anchored before the creation gate below; live math preservation/projection likewise never
    /// calls this method. Full-screen TUIs retain their exemption because entering/repainting them
    /// clears or switches the grid before they keep DECTCEM off, leaving no remembered input line.
    fn schedule_live_artifacts(&mut self, stable: &[bool]) -> usize {
        let image_tasks_before = self.local_image_path_tasks.len();
        self.reconcile_live_image_paths(true, stable);

        let inputs = self.live_detection_context();
        let initial_context = self.live_initial_detection_context(&inputs);
        let candidates = live_candidate_rows(&inputs, initial_context.clone(), stable);
        let context_signature = live_detection_context_signature(&inputs);
        let cursor_suppression = self.cursor_line_suppression();
        let mut new_tasks = Vec::new();
        for candidate_row in candidates {
            if cursor_suppression.is_some_and(|suppression| suppression.contains(candidate_row))
                || self.live_edit_taint_intersects(candidate_row, candidate_row)
            {
                continue;
            }
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
        new_tasks.retain(|task| {
            let suppressed = task.resolved
                && (cursor_suppression.is_some_and(|suppression| {
                    suppression.intersects(task.start.row, task.end.row)
                }) || self.live_edit_taint_intersects(task.start.row, task.end.row));
            if suppressed && let Some(state) = self.live_rows.get_mut(task.candidate_row as usize) {
                // Cursor movement does not damage the source row. Leave the signature open so the
                // next published frame can schedule this already-settled candidate immediately.
                state.candidate_signature = None;
            }
            !suppressed
        });
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
        scheduled.saturating_add(
            self.local_image_path_tasks
                .len()
                .saturating_sub(image_tasks_before),
        )
    }

    /// Inclusive physical-row bounds for one row's logical line. `CapturedRow::continues` is the
    /// vendor WRAPLINE fact for "this row continues into the next"; walking it in both directions
    /// makes a cursor on either half of a soft-wrapped input suppress the whole line.
    fn logical_line_containing(&self, row: u32) -> Option<(u32, u32)> {
        let row_count = u32::try_from(self.live_rows.len()).unwrap_or(u32::MAX);
        if row >= row_count {
            return None;
        }

        let mut start = row;
        while start > 0
            && self
                .terminal
                .visible_row(start - 1)
                .is_some_and(|row| row.continues)
        {
            start -= 1;
        }

        let mut end = row;
        while end.saturating_add(1) < row_count
            && self
                .terminal
                .visible_row(end)
                .is_some_and(|row| row.continues)
        {
            end += 1;
        }
        Some((start, end))
    }

    fn visible_cursor_logical_line(&self) -> Option<(u32, u32)> {
        let cursor = self.terminal.cursor();
        cursor
            .visible
            .then(|| self.logical_line_containing(cursor.row))
            .flatten()
    }

    fn visible_cursor_logical_line_memory(&self) -> Option<CursorLogicalLineMemory> {
        let (start, end) = self.visible_cursor_logical_line()?;
        Some(CursorLogicalLineMemory {
            screen: self.live_screen,
            start,
            end,
            explicitly_positioned: self.terminal.cursor_row_was_explicitly_positioned(),
        })
    }

    fn remember_visible_cursor_logical_line(&mut self) {
        let Some(memory) = self.visible_cursor_logical_line_memory() else {
            return;
        };
        self.cursor_logical_line_memory = Some(memory);
    }

    fn effective_cursor_logical_line_memory(&self) -> Option<CursorLogicalLineMemory> {
        self.visible_cursor_logical_line_memory().or_else(|| {
            self.cursor_logical_line_memory
                .filter(|memory| memory.screen == self.live_screen)
        })
    }

    #[cfg(test)]
    fn cursor_suppressed_logical_line(&self) -> Option<(u32, u32)> {
        self.effective_cursor_logical_line_memory()
            .map(|memory| (memory.start, memory.end))
    }

    fn logical_line_is_blank(&self, line: (u32, u32)) -> bool {
        (line.0..=line.1).all(|row| {
            self.terminal
                .visible_row(row)
                .is_some_and(|captured| captured_row_is_blank(&captured))
        })
    }

    fn preceding_nonblank_logical_line(&self, start: u32) -> Option<(u32, u32)> {
        let mut row = start.checked_sub(1)?;
        loop {
            let line = self.logical_line_containing(row)?;
            if !self.logical_line_is_blank(line) {
                return Some(line);
            }
            row = line.0.checked_sub(1)?;
        }
    }

    fn cursor_line_suppression(&self) -> Option<CursorLineSuppression> {
        let memory = self.effective_cursor_logical_line_memory()?;
        let cursor_line = (memory.start, memory.end);
        let preceding_nonblank_line = (memory.explicitly_positioned
            && self.logical_line_is_blank(cursor_line))
        .then(|| self.preceding_nonblank_logical_line(cursor_line.0))
        .flatten();
        Some(CursorLineSuppression {
            cursor_line,
            preceding_nonblank_line,
        })
    }

    fn capture_live_edit_taint(&self, range: (u32, u32)) -> Option<LiveEditTaint> {
        let rows = (range.0..=range.1)
            .map(|row| {
                self.terminal
                    .visible_row(row)
                    .map(|captured| EditTaintedRow::from_captured(&captured))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(LiveEditTaint {
            screen: self.live_screen,
            start: range.0,
            rows,
        })
    }

    fn current_active_edit_taints(&self) -> Vec<LiveEditTaint> {
        let Some(suppression) = self.cursor_line_suppression() else {
            return Vec::new();
        };
        std::iter::once(suppression.cursor_line)
            .chain(suppression.preceding_nonblank_line)
            .filter_map(|range| self.capture_live_edit_taint(range))
            .collect()
    }

    fn commit_live_edit_taint(&mut self, taint: LiveEditTaint) {
        if taint.rows.is_empty() {
            return;
        }
        self.committed_live_edit_taints.retain(|resident| {
            resident.screen != taint.screen || !resident.intersects(taint.start, taint.end())
        });
        self.committed_live_edit_taints.push(taint);
        debug_assert!(
            self.committed_live_edit_taints.len() <= self.live_rows.len(),
            "overlap replacement bounds committed edit taints by live rows"
        );
    }

    /// Promote the final contents of every published edit line when terminal execution advances by
    /// LF/VT/FF, then remember the newly published cursor gate. No timer or keyboard inference is
    /// involved: both facts come from the terminal parser and its final grid.
    fn update_edit_taints_after_feed(&mut self, cursor_stream_progressed: bool) {
        if cursor_stream_progressed {
            for active in std::mem::take(&mut self.active_edit_taints) {
                if active.screen != self.live_screen {
                    continue;
                }
                // PSReadLine may repaint the accepted command in the same quantum as Enter. Read
                // the final cells at the same row identity and retain the taint only on content
                // equality. If output replaced that row before LF, it is a new instance and must
                // not inherit the positional edit gate.
                let unchanged = self
                    .capture_live_edit_taint((active.start, active.end()))
                    .as_ref()
                    == Some(&active);
                if unchanged {
                    self.commit_live_edit_taint(active);
                }
            }
        }
        self.active_edit_taints = self.current_active_edit_taints();
    }

    fn clear_live_edit_taints(&mut self) {
        self.active_edit_taints.clear();
        self.committed_live_edit_taints.clear();
    }

    fn clear_all_edit_taints(&mut self) {
        self.clear_live_edit_taints();
        self.edit_tainted_staging.clear();
        self.edit_tainted_transcript.clear();
    }

    fn reconcile_committed_live_edit_taints(&mut self) {
        let terminal = &self.terminal;
        let screen = self.live_screen;
        self.committed_live_edit_taints.retain(|taint| {
            taint.screen == screen
                && (taint.start..=taint.end())
                    .zip(&taint.rows)
                    .all(|(row, expected)| {
                        terminal.visible_row(row).is_some_and(|actual| {
                            EditTaintedRow::from_captured(&actual) == *expected
                        })
                    })
        });
    }

    fn live_edit_taint_intersects(&self, start: u32, end: u32) -> bool {
        self.active_edit_taints
            .iter()
            .chain(&self.committed_live_edit_taints)
            .any(|taint| taint.screen == self.live_screen && taint.intersects(start, end))
    }

    fn new_live_decoration_is_edit_suppressed(&self, task: &LiveDetectionTask) -> bool {
        let replaces_existing = self.live_decorations.values().any(|record| {
            record.screen == task.screen
                && record.start == task.start
                && record.end == task.end
                && record.span.original_source == task.span.original_source
        });
        !replaces_existing
            && (self
                .cursor_line_suppression()
                .is_some_and(|suppression| suppression.intersects(task.start.row, task.end.row))
                || self.live_edit_taint_intersects(task.start.row, task.end.row))
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

    /// Diagnostic red gate: how many on-screen display blocks are provable from the live grid in
    /// isolation (a clean grid-only re-scan, what a zoom achieves) yet absent from the full
    /// history+grid detection. Nonzero means a poisoned frozen prefix is silently stranding
    /// complete blocks at source — the exact live-norender desync, which the flash oracle cannot
    /// see. Primary only; the alternate screen carries no frozen prefix.
    pub fn live_detection_isolation_gap(&self) -> usize {
        if self.live_screen != ScreenId::Primary {
            return 0;
        }
        let inputs = self.live_detection_context();
        let initial_context = self.live_initial_detection_context(&inputs);
        bt_detect::live_detection_isolation_gap(&inputs, initial_context, self.detection_options())
    }

    /// Batch ⑥ token-ownership ledger for the current live region: every structural `$$`/`\[`/`\]`/
    /// environment delimiter accounted as owned by a detected block, one of the enumerated legitimate
    /// rejections, or an orphan. Feeds the split source-integrity / detector-containment red gate.
    /// Read-only instrumentation over the exact detection the session already runs; it never mutates
    /// detection or presentation.
    pub fn live_detection_ownership_ledger(&self) -> bt_detect::OwnershipLedger {
        let inputs = self.live_detection_context();
        let initial_context = self.live_initial_detection_context(&inputs);
        bt_detect::live_detection_ownership_ledger(
            &inputs,
            initial_context,
            self.detection_options(),
        )
    }

    /// Batch ③ (review §4): every formula still *painted* — a resident live decoration holding a live
    /// or a stale artifact — whose exact source the current detection scan no longer Owns. Consumes
    /// the batch-⑥ ownership ledger's Owned display set: a held record is *backed* when its
    /// `original_source` (the very key holds re-anchor on, `restore_offscreen_decorations`) is among
    /// the currently-detected blocks, and `HeldUnbacked` otherwise — a hold showing a block the
    /// detector no longer accounts (the "红门绿、真机红" masking the audit named).
    ///
    /// Scope is the *resident* decorations, the exact set `decorate_math_frame` paints. The off-band
    /// queue (`offscreen_decorations`) is a preservation buffer that is never painted — at quiescence
    /// it holds records scrolled off the alternate viewport, which are not on screen and must not be
    /// mistaken for a masking hold. A record with neither artifact shows source, not a raster, and
    /// likewise cannot mask detection.
    ///
    /// Read-only over the detection the session already runs; it mutates no decoration, so display and
    /// preservation are byte-identical with or without this call. A record on a screen other than the
    /// live one is not part of the current detection window and is not judged.
    pub fn held_unbacked_records(&self) -> Vec<HeldUnbackedRecord> {
        let owned = self.live_detection_ownership_ledger();
        self.live_decorations
            .values()
            .filter(|record| record.screen == self.live_screen)
            .filter(|record| record.artifact.is_some() || record.stale_artifact.is_some())
            .filter(|record| !owned.owns_source(&record.span.original_source))
            .map(|record| HeldUnbackedRecord {
                source_line: record
                    .frozen_prefix
                    .first()
                    .map(|id| MathSourceLine::Transcript(*id))
                    .unwrap_or(MathSourceLine::LiveGrid(record.start.row)),
                original_source: record.span.original_source.clone(),
                screen: record.screen,
                band_start_row: record.band_start_row,
                band_end_row: record.band_end_row,
                stale: record.artifact.is_none() && record.stale_artifact.is_some(),
            })
            .collect()
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

    /// Snapshot the primary live decorations (resident and off-band) and the grid inputs when an
    /// in-stream reprint window opens, mirroring `snapshot_alternate_repaint`. Suppression keeps the
    /// resident records rendering through the window; `finish_primary_repaint` reprojects this
    /// snapshot onto the reflowed grid at the window's close. No live decoration (resident or
    /// off-band) → nothing a reprint could flash, so no snapshot and the window stays inert.
    fn snapshot_primary_repaint(&self) -> Option<AlternateRepaintSnapshot> {
        (self.live_screen == ScreenId::Primary
            && !self.terminal.modes().alternate_screen
            && (!self.live_decorations.is_empty() || !self.offscreen_decorations.is_empty()))
        .then(|| AlternateRepaintSnapshot {
            inputs: self.live_detection_context(),
            decorations: self.live_decorations.values().cloned().collect(),
            dormant_decorations: self.offscreen_decorations.iter().cloned().collect(),
            invalidation_count: self.live_invalidation_count,
            snapshot_boundary: true,
        })
    }

    /// Resize adds geometric preservation only for the quantified transition-side family: two
    /// distinct source owners with the same rendered formula (the resident outer block and the
    /// narrower environment which can replace it). A dormant historical pair cannot switch the
    /// visible side and must not activate this path. Keeping the snapshot itself scoped leaves
    /// ordinary resize records on the established exact-source path, including its ordering and
    /// `HELD_UNBACKED` audit behavior.
    fn snapshot_primary_resize_transition(&self) -> Option<AlternateRepaintSnapshot> {
        let mut snapshot = self.snapshot_primary_repaint()?;
        let occurrences = resize_transition_side_occurrences(
            snapshot
                .decorations
                .iter()
                .chain(snapshot.dormant_decorations.iter()),
        );
        let resident_outer_owner = snapshot.decorations.iter().any(|record| {
            occurrences.contains(&record.identity.occurrence_id)
                && record.span.original_source.trim_start().starts_with("$$")
        });
        if !resident_outer_owner {
            return None;
        }
        snapshot
            .decorations
            .retain(|record| occurrences.contains(&record.identity.occurrence_id));
        snapshot
            .dormant_decorations
            .retain(|record| occurrences.contains(&record.identity.occurrence_id));
        (!snapshot.decorations.is_empty() || !snapshot.dormant_decorations.is_empty())
            .then_some(snapshot)
    }

    /// Close a primary in-stream reprint window by reprojecting every proven record the snapshot
    /// held (resident and off-band) onto the reflowed grid, using the proven-row fingerprint
    /// segmented mapping alternate uses (`segmented_row_mapping` + `project_live_record`) rather
    /// than only exact source equality.
    ///
    /// A Codex reprint rewrites its transcript by wrapping-and-cursor-addressing individual rows, so
    /// a proven block's rows come back reflowed: the same body, differently wrapped or shifted.
    /// Suppression kept every proven record resident (rendering) through the window; here each is
    /// re-seated onto the reflowed grid before the frame is published, so its source never shows.
    ///
    /// The mapping set is the segmented row mapping plus a forced identity mapping: unlike an
    /// alternate clear+home that rewrites the whole screen, a primary reprint rewrites only a few
    /// lines, so most records are unchanged and must map straight through even when no proven row
    /// moved by a common delta (the segmented mapping is then empty). A record maps under exactly
    /// one of {identity, a segmented delta}: an unchanged record matches at its own rows (identity),
    /// a shifted one at its rows plus the delta, so the placement is unambiguous. A record that maps
    /// under neither falls off-band, where `restore_offscreen_decorations` re-anchors it by exact
    /// source equality if its proven text reappears verbatim; failing that it retires to
    /// re-detection — the justified fallback when the reprint genuinely rewrote the source (not a
    /// flash), never a wrongly-placed raster.
    ///
    /// Isolated from `finish_alternate_repaint`: the forced identity mapping, and the absence of
    /// alternate's content-end-row / borrowed-band handling and bounded re-detection, are primary
    /// specific. Alternate's path is untouched.
    fn finish_primary_repaint(
        &mut self,
        snapshot: AlternateRepaintSnapshot,
        resize_transition_side_only: bool,
    ) {
        let dirty = resize_transition_side_only || std::mem::take(&mut self.primary_repaint_dirty);
        if self.live_screen != ScreenId::Primary || self.terminal.modes().alternate_screen {
            return;
        }
        if !dirty && self.offscreen_decorations.is_empty() {
            // The reprint changed no row under any resident record and nothing is waiting off-band:
            // every record is already correctly placed, so skip the segmented reprojection entirely.
            return;
        }
        let current_inputs = self.live_detection_context();
        let current_initial_context = self.live_initial_detection_context(&current_inputs);
        let row_mappings = primary_repaint_row_mappings(&snapshot.inputs, &current_inputs);
        let context_signature = live_detection_context_signature(&current_inputs);
        let stable = vec![true; self.live_rows.len()];
        let candidate_rows =
            live_candidate_rows(&current_inputs, current_initial_context.clone(), &stable);

        let mut preserved = BTreeMap::new();
        let mut occupied = BTreeSet::new();
        let mut relayout_tasks = Vec::new();
        let mut unresolved = Vec::new();

        // A record still resident on a *different* grid generation than the snapshot holds for the
        // same occurrence was re-seated during the window by an exact-source proof against the
        // settled grid — the batched top-anchored scroll in `preserve_live_after_top_scroll`. That
        // verdict is stronger than anything this fingerprint reprojection can derive from a snapshot
        // taken before the scroll: it already knows where every proven row went, and it also knows
        // which occurrences left the grid entirely. Seat those residents first so they own their
        // rows, then let the snapshot only fill in occurrences the reprint tore down without proof;
        // a resurrected pre-scroll copy loses the row it no longer owns and falls off-band instead
        // of painting over its successor. A record the window never touched still carries the
        // snapshot generation, so it goes through reprojection exactly as before and the in-stream
        // reprint remap is unchanged.
        let snapshot_generations = snapshot
            .decorations
            .iter()
            .chain(snapshot.dormant_decorations.iter())
            .map(|record| (record.identity.occurrence_id, record.generation))
            .collect::<BTreeMap<_, _>>();
        // The snapshot carries its own clone of every record that was already off-band when the
        // window opened, and every record the window drained off-band was resident at that moment
        // and so is cloned too. Retire those originals now: without this the reprojection pushes a
        // second copy of each next to the one still queued, and each further window doubles the
        // queue again — MAX_OFFSCREEN_RECORDS worth of duplicate exact-source scans per feed.
        // Records first seen after the snapshot are not in it and stay queued.
        self.offscreen_decorations
            .retain(|record| !snapshot_generations.contains_key(&record.identity.occurrence_id));
        let mut reproven = BTreeSet::new();
        for (_, record) in std::mem::take(&mut self.live_decorations) {
            if snapshot_generations
                .get(&record.identity.occurrence_id)
                .is_some_and(|generation| *generation == record.generation)
            {
                continue;
            }
            reproven.insert(record.identity.occurrence_id);
            if let Some(record) =
                insert_nonoverlapping_live_record(&mut preserved, &mut occupied, record)
            {
                unresolved.push(record);
            }
        }
        for record in snapshot
            .decorations
            .into_iter()
            .chain(snapshot.dormant_decorations)
        {
            if reproven.contains(&record.identity.occurrence_id) {
                // Its re-proven successor is already seated above; this pre-scroll copy is stale.
                continue;
            }
            let projected = (!row_mappings.is_empty())
                .then(|| {
                    project_live_record_uniquely(
                        &record,
                        &row_mappings,
                        self.grid_generation,
                        self.detection_revision,
                        self.layout_key,
                        current_initial_context.clone(),
                        Arc::clone(&current_inputs),
                    )
                })
                .flatten();
            let mut record = match projected {
                Some(RecordProjection::Visible(record)) => record,
                Some(RecordProjection::Dormant(record)) => {
                    self.retain_offscreen_record(record);
                    continue;
                }
                None => {
                    unresolved.push(record);
                    continue;
                }
            };
            // A reflow (resize/zoom) makes the reprojected raster stale for the new layout: hold it
            // as a stale artifact and queue a fresh relayout, exactly as
            // `restore_offscreen_decorations` does, so no old-DPI raster is shown.
            if record.rendered_layout != self.layout_key
                && let Some(artifact) = record.artifact.take()
            {
                record.stale_artifact = Some(StaleArtifact {
                    artifact,
                    rendered_layout: record.rendered_layout,
                });
            }
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
            if let Some(record) =
                insert_nonoverlapping_live_record(&mut preserved, &mut occupied, record)
            {
                unresolved.push(record);
            }
        }

        // Suppression skipped invalidation for the window, so the count is unchanged from the
        // snapshot; re-add only records that reprojection could neither place nor keep off-band.
        self.live_invalidation_count = snapshot.invalidation_count;
        for record in unresolved {
            if record.artifact.is_some() || record.stale_artifact.is_some() {
                self.retain_offscreen_record(record);
            } else {
                self.live_invalidation_count = self.live_invalidation_count.saturating_add(1);
            }
        }
        self.live_decorations = preserved;
        // Reseat the row fingerprints/candidate signatures under each preserved band so the next
        // damage compares against the reflowed grid, exactly as `finish_alternate_repaint` does.
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
        for task in relayout_tasks {
            self.enqueue_live_task(task);
        }
    }

    /// The current live screen owns the off-band preservation queue. Alternate retains renderable
    /// decorations across every repaint; primary retains them only for the span of a resize
    /// transaction so a reflow does not flash proven formulas back to source (see
    /// `primary_resize_preservation_active`). Any other primary state drops as before.
    fn offscreen_preservation_active(&self) -> bool {
        self.live_screen == ScreenId::Alternate
            || self.primary_resize_preservation_active()
            || self.primary_repaint_active()
    }

    /// Primary preserves live formulas across an in-stream transcript reprint the same way it does
    /// across a resize: by draining renderable decorations into the off-band queue and re-anchoring
    /// them by exact source equality (`restore_offscreen_decorations`) once the reprint lands,
    /// instead of dropping them to source and re-detecting (the in-stream reprint flash). Codex
    /// reflows and reprints its whole transcript mid-stream, so a proven block whose rows are
    /// rewritten with new wrapping would otherwise revert to source until re-detection caught up.
    ///
    /// This is a distinct trigger from `primary_resize_preservation_active`, not a superset of it:
    /// a resize reflows the grid at `resize_at` time without necessarily carrying a reprint
    /// boundary in the same feed, so resize preservation must engage on the resize operation while
    /// reprint preservation engages on the clear+home / erase-storm / synchronized-update boundary.
    fn primary_repaint_active(&self) -> bool {
        self.live_screen == ScreenId::Primary && self.primary_repaint_in_progress
    }

    /// Primary preserves live formulas across a window resize by holding the same off-band queue
    /// alternate uses. Codex reflows and then reprints its whole transcript, so proven blocks would
    /// otherwise revert to source between the reflow and re-detection. The queue is re-anchored by
    /// exact source equality every frame.
    ///
    /// The window spans the resize transaction *and its aftermath*: a resize demotes every proven
    /// raster to a stale artifact and queues a fresh relayout for the reflowed grid (bt-math is
    /// async, so the fresh raster lands one or more stability intervals later). Ending preservation
    /// the instant the epoch quiesces left those re-anchored stale records unprotected, so the first
    /// post-resize repaint — whose rebuilt-grid fingerprints no longer match — dropped them to
    /// source until re-detection caught up (the "resize-completes" flash). A stale artifact retires
    /// only when its fresh render replaces it in place (`apply_live_worker_completion` installs the
    /// new record with `stale_artifact: None`), a render failure clears it, or its source stops
    /// matching the grid — never on a clock. So preservation stays engaged while any live decoration
    /// is still awaiting that fresh relayout, exactly the alternate-screen stale-artifact semantics.
    fn primary_resize_preservation_active(&self) -> bool {
        self.live_screen == ScreenId::Primary
            && (self.resize_epoch.is_active() || self.has_pending_resize_relayout())
    }

    /// A live decoration demoted to a stale artifact (its fresh raster taken, a relayout queued) is
    /// mid-flight after a layout change: it renders the old raster until the fresh one lands. While
    /// any such record remains — resident in `live_decorations` or drained off-band into
    /// `offscreen_decorations` — primary keeps preserving so a repaint cannot drop it to source
    /// before the replacement arrives. The off-band queue must be included: a repaint invalidates
    /// the resident record into the queue first, and were the queue not counted, preservation would
    /// collapse on the very next invalidation in the same feed and wipe the record it just drained.
    /// Records holding a live artifact retire on invalidation as before, so steady-state primary
    /// output (no pending relayout) is unaffected.
    fn has_pending_resize_relayout(&self) -> bool {
        self.live_decorations
            .values()
            .chain(self.offscreen_decorations.iter())
            .any(|record| record.artifact.is_none() && record.stale_artifact.is_some())
    }

    fn primary_reprint_presentation_hold(&self) -> bool {
        self.live_screen == ScreenId::Primary
            && !self.terminal.modes().alternate_screen
            && self.offscreen_decorations.iter().any(|record| {
                self.primary_reprint_hold_occurrences
                    .contains_key(&record.identity.occurrence_id)
                    && stale_pending_dpi_transition(record)
            })
    }

    fn reconcile_primary_reprint_presentation_hold(&mut self, boundary_observed: bool) {
        let mut pending = self
            .offscreen_decorations
            .iter()
            .filter(|record| stale_pending_dpi_transition(record))
            .map(|record| record.identity.occurrence_id)
            .collect::<BTreeSet<_>>();
        if boundary_observed && let Some(floor) = self.primary_reprint_history_floor {
            for occurrence in &pending {
                self.primary_reprint_hold_occurrences
                    .entry(*occurrence)
                    .or_insert(floor);
            }
        }
        self.retire_offscreen_records_replaced_by_frozen();
        pending = self
            .offscreen_decorations
            .iter()
            .filter(|record| stale_pending_dpi_transition(record))
            .map(|record| record.identity.occurrence_id)
            .collect();
        self.primary_reprint_hold_occurrences
            .retain(|occurrence, _| pending.contains(occurrence));
    }

    /// A clean reprint can move a proven live formula wholly into immutable history after zoom-in.
    /// In that case exact-live re-anchoring can never succeed. A completed frozen detection for the
    /// same byte-exact source is the durable successor, but only when its transcript id was allocated
    /// after this occurrence's reprint watermark: an older equal formula is not ownership evidence.
    fn retire_offscreen_records_replaced_by_frozen(&mut self) {
        let retired = self
            .offscreen_decorations
            .iter()
            .filter_map(|record| {
                let occurrence = record.identity.occurrence_id;
                let floor = self.primary_reprint_hold_occurrences.get(&occurrence)?;
                self.decorations
                    .iter()
                    .any(|(id, frozen)| {
                        floor.0.is_none_or(|floor| *id > floor)
                            && frozen.source == SourceLifecycle::Frozen
                            && frozen.block_end.is_some()
                            && !matches!(
                                frozen.decoration,
                                DecorationLifecycle::None | DecorationLifecycle::Pending
                            )
                            && frozen.span.as_ref().is_some_and(|span| {
                                span.mode == record.span.mode
                                    && span.original_source == record.span.original_source
                            })
                    })
                    .then_some(occurrence)
            })
            .collect::<BTreeSet<_>>();
        if retired.is_empty() {
            return;
        }
        self.offscreen_decorations
            .retain(|record| !retired.contains(&record.identity.occurrence_id));
        self.primary_reprint_hold_occurrences
            .retain(|occurrence, _| !retired.contains(occurrence));
    }

    /// Explicit keyboard/paste/IME takeover releases a presentation hold without guessing when the
    /// producer might finish repainting. The off-band record remains available for a later exact
    /// re-anchor, but it no longer prevents the user's requested frame from being published.
    pub fn release_presentation_hold_for_user_input(&mut self) -> bool {
        let released = self.primary_reprint_presentation_hold();
        self.primary_reprint_hold_occurrences.clear();
        released
    }

    fn retain_offscreen_record(&mut self, record: LiveDecorationRecord) {
        // A stale-pending record (raster demoted to stale, fresh relayout queued) is always retained
        // off-band: it is mid-flight after a layout change and must survive until its replacement
        // lands, even in the brief window after a resize epoch closes but before preservation would
        // otherwise re-engage. Records carrying a live artifact still obey the global window.
        let stale_pending = record.artifact.is_none() && record.stale_artifact.is_some();
        if !self.offscreen_preservation_active() && !stale_pending {
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
            // The re-anchor proved this occurrence's *complete* source inside the live grid, so no
            // part of it is frozen any more: a prefix carried over from the anchor it lost would
            // name history lines this placement does not span.
            record.frozen_prefix.clear();
            record.staging_prefix.clear();
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

    fn restore_offscreen_decorations_except_resize_transition(&mut self) {
        let occurrences = resize_transition_side_occurrences(self.offscreen_decorations.iter());
        let mut deferred = VecDeque::new();
        let mut ordinary = VecDeque::new();
        while let Some(record) = self.offscreen_decorations.pop_front() {
            if occurrences.contains(&record.identity.occurrence_id) {
                deferred.push_back(record);
            } else {
                ordinary.push_back(record);
            }
        }
        self.offscreen_decorations = ordinary;
        self.restore_offscreen_decorations();
        deferred.append(&mut self.offscreen_decorations);
        self.offscreen_decorations = deferred;
    }

    fn observe_live_damage(&mut self, damage: TerminalDamage, observed_at: Instant) {
        let screen = if self.terminal.modes().alternate_screen {
            ScreenId::Alternate
        } else {
            ScreenId::Primary
        };
        if screen != self.live_screen {
            self.cursor_logical_line_memory = None;
            self.clear_live_edit_taints();
            self.invalidate_all_live_decorations();
            // A screen switch is a hard boundary: never carry the previous screen's off-band
            // preservation queue across it, even if a resize transaction on the old screen kept
            // `invalidate_all_live_decorations` from clearing it.
            self.offscreen_decorations.clear();
            self.primary_repaint_in_progress = false;
            self.primary_repaint_snapshot = None;
            self.primary_repaint_dirty = false;
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
            // Suppression: inside a repaint window the proven raster keeps rendering over the rows
            // being rewritten instead of the record being torn down (and its source flashing
            // through). Alternate suppresses across a boundary repaint; primary suppresses across an
            // in-stream reprint window while its snapshot is held. `finish_*_repaint` reprojects
            // every held record onto the reflowed grid at the window's close, and the frame is only
            // published after the feed completes, so no intermediate stale position is ever shown.
            if screen == ScreenId::Alternate && self.alternate_repaint_in_progress {
                continue;
            }
            if screen == ScreenId::Primary && self.primary_repaint_snapshot.is_some() {
                // A row genuinely changed under the reprint window (an unchanged row `continue`d
                // above at the fingerprint check): mark the window dirty so its close reprojects.
                // A same-content repaint changes no row, so the reprojection — and its full-grid
                // segmented mapping — is skipped entirely.
                self.primary_repaint_dirty = true;
                continue;
            }
            self.invalidate_live_row(row);
        }
        // A committed exemption is tied to the exact logical-line instance. Repainting equal text
        // (including PSReadLine's Enter redraw) preserves it; any content/wrap change retires the
        // whole line before the next stability pass can create a decoration.
        self.reconcile_committed_live_edit_taints();
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
            // The record is already out of `live_decorations` here, so a preservation check that
            // scans it (`has_pending_resize_relayout`) can no longer see it. A stale-pending record
            // — mid-relayout after a resize — must be preserved on its own account regardless, or
            // the last such record on the grid would be dropped to source the instant the epoch
            // closes (the resize-completes flash). `retain_offscreen_record` mirrors this.
            let stale_pending = record.artifact.is_none() && record.stale_artifact.is_some();
            if (self.offscreen_preservation_active() || stale_pending)
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
            while let Some(task) = self.take_decoration_worker_task() {
                match task {
                    SessionDecorationTask::Math(task) => match *task {
                        SessionMathTask::Frozen(task) => {
                            self.complete_worker_task(task);
                        }
                        SessionMathTask::Live(mut task) => {
                            if resolve_live_detection_task(&mut task) {
                                let artifact = live_placeholder(&task);
                                size_resolved_live_task_band(&mut task);
                                self.apply_live_worker_completion(task, Some(artifact), None);
                            }
                        }
                    },
                    SessionDecorationTask::InlineImage(task) => {
                        let result = decode_inline_image(task.clone());
                        self.complete_inline_image_result(task, result);
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

    pub fn take_decoration_worker_task(&mut self) -> Option<SessionDecorationTask> {
        self.take_math_worker_task()
            .map(|task| SessionDecorationTask::Math(Box::new(task)))
            .or_else(|| {
                self.inline_image_tasks
                    .pop_front()
                    .map(SessionDecorationTask::InlineImage)
            })
            .or_else(|| {
                self.local_image_path_tasks
                    .pop_front()
                    .map(SessionDecorationTask::InlineImage)
            })
    }

    pub fn complete_inline_image_result(
        &mut self,
        task: InlineImageTask,
        result: Result<DecodedInlineImage, InlineImageDecodeError>,
    ) -> bool {
        let Some(record) = self.inline_images.get_mut(&task.occurrence_id) else {
            return false;
        };
        match result {
            Ok(artifact) if artifact.occurrence_id == task.occurrence_id => {
                record.artifact = Some(artifact);
                record.failed = false;
            }
            Ok(_) => return false,
            Err(_) => {
                record.artifact = None;
                record.failed = true;
            }
        }
        self.bump_view_generation();
        true
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
        let candidate_id = task.candidate_id;
        let versions = task.versions;
        let accepted = self.apply_worker_completion(task, Some(placeholder), None);
        if !accepted {
            self.stale_results += 1;
            self.account_stranded_pending(candidate_id, versions);
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
            self.account_stranded_pending(task.candidate_id, task.versions);
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
        if task.resolved && artifact.is_some() {
            size_resolved_live_task_band(&mut task);
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
        if self.new_live_decoration_is_edit_suppressed(&task) {
            if let Some(state) = self.live_rows.get_mut(task.candidate_row as usize) {
                // The task was valid, but presentation policy rejected creating its record at this
                // cursor state. Keep the stable row eligible for the first frame after the cursor
                // leaves its WRAPLINE-linked logical line.
                state.candidate_signature = None;
            }
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
                staging_prefix: Vec::new(),
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
        let rendered = artifact.is_some();
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
        // A resolved multi-line block owns its interior rows as body. A structural delimiter inside
        // it (e.g. the `\begin{aligned}` of a `$$…\begin{aligned}…\end{aligned}…$$` block) is never a
        // sub-block; suppress any stale standalone render left on one — which the certified-frontier
        // recovery can now produce when the enclosing `$$` opener was a phantom until its forward
        // block landed — so the block's artifact does not double-render over an inner environment.
        if applied && rendered {
            self.suppress_block_interior(task.transcript_id, task.block_end);
            // The frozen pipeline has now paired this block on durable transcript ids. Any live
            // record still bridging over those lines is a superseded duplicate of it.
            self.retire_stale_bridge_prefixes(false);
        }
        if applied {
            self.retire_offscreen_records_replaced_by_frozen();
        }
        applied
    }

    /// Suppress the frozen decorations of structural-delimiter rows strictly inside a resolved block
    /// `(start, end)`. They are block body, never independent blocks, so a residual standalone render
    /// on one must not be painted over the enclosing block's artifact.
    fn suppress_block_interior(&mut self, start: TranscriptId, end: TranscriptId) {
        if start >= end {
            return;
        }
        let interior: Vec<TranscriptId> = self
            .document
            .entries()
            .range((Bound::Excluded(start), Bound::Excluded(end)))
            .filter(|(_, entry)| may_contain_display_math(&entry.line.text))
            .map(|(id, _)| *id)
            .collect();
        for id in interior {
            if let Some(record) = self.decorations.get_mut(&id) {
                record.suppress();
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

    /// Record a dropped worker completion whose candidate is still the exact attempt we scheduled
    /// (`Frozen + Pending` at the same versions) as stranded. This is the only path that leaves a
    /// candidate frozen at `Pending`: `apply_worker_completion` returns `false` at the
    /// `block_is_current` check without touching the record, when a multi-line block's scan is in
    /// flight and one of its *non-candidate* input lines changed or was evicted (a scrollback quota
    /// eviction while scrolling, or a transient ED3/reprint rewrite). The candidate's own line is
    /// unchanged, so the version gate never self-heals it. We do not re-arm here — re-arming at the
    /// drop site is what turned the original fix into a regression: the per-frame visible scheduler
    /// re-issues any `None` frozen candidate, so an immediate `Pending -> None` here would, whenever
    /// the source is still in motion (resize reflow / reprint), be re-scheduled next frame, dropped
    /// again mid-motion, and re-armed again — a reschedule storm that re-runs block detection over
    /// the whole range every frame and tears neighbouring bands. Instead we only remember the id and
    /// flip it to `None` once the source is quiescent (`rearm_stranded_pending`).
    fn account_stranded_pending(&mut self, candidate_id: TranscriptId, versions: VersionStamp) {
        if let Some(record) = self.decorations.get(&candidate_id)
            && record.source == SourceLifecycle::Frozen
            && record.decoration == DecorationLifecycle::Pending
            && record.versions == versions
        {
            self.stranded_pending.insert(candidate_id, versions);
        }
    }

    /// Re-arm stranded frozen candidates for re-scheduling, but only once the source is quiescent so
    /// the re-issue cannot chase a still-changing block. `schedule_visible_artifacts` (our only
    /// caller) already guarantees the resize epoch is idle; here we additionally require no active
    /// reprint window, no buffering synchronized update, and no active staging tail — the same
    /// conditions under which a fresh scan sees a settled source. Each stranded id is flipped
    /// `Pending -> None` exactly once (the set is consumed); the surrounding scheduler then issues a
    /// single fresh scan against current source. If that scan is itself dropped because new output
    /// has since changed the block, `account_stranded_pending` re-records it and it waits for the
    /// next quiescent frame — never a per-frame loop, because a drop requires a resolved block whose
    /// source moved after the scan was scheduled, which cannot happen while output is quiet. A block
    /// whose opener truly evicted re-issues as a lone closer, fails detection, and settles `Failed`
    /// (terminal) — the stuck-`Pending` liveness hole is closed without ever re-rendering wrong math.
    fn rearm_stranded_pending(&mut self) {
        if self.stranded_pending.is_empty()
            || self.primary_repaint_in_progress
            || self.active_staging_tail.is_some()
            || self.synchronized_update_deadline().is_some()
        {
            return;
        }
        for (id, versions) in std::mem::take(&mut self.stranded_pending) {
            if let Some(record) = self.decorations.get_mut(&id)
                && record.source == SourceLifecycle::Frozen
                && record.decoration == DecorationLifecycle::Pending
                && record.versions == versions
            {
                record.decoration = DecorationLifecycle::None;
            }
        }
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
        // Cursor-only movement does not damage a row and therefore has no stability deadline of its
        // own. Revisit already-settled live candidates at every actual frame boundary so leaving a
        // logical input line unlocks it deterministically, without an input-event guess or timer.
        let stable = self
            .live_rows
            .iter()
            .map(|row| row.settled_revision == Some(row.revision))
            .collect::<Vec<_>>();
        let live_scheduled = self.schedule_live_artifacts(&stable);
        // Deferred re-arm of blocks stranded at `Pending` by a dropped completion. The resize epoch
        // is idle here (guard above); `rearm_stranded_pending` adds the remaining quiescence
        // requirements before flipping any candidate back to `None` for the scan loop below.
        self.rearm_stranded_pending();
        let columns = frame.columns.get() as usize;
        if columns == 0 {
            return live_scheduled;
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
            return live_scheduled;
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
        live_scheduled.saturating_add(self.frozen_detection_count.saturating_sub(before) as usize)
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
        // A zoom / DPI change remeasures the cell height on the session; propagate it before any
        // geometry is derived this pass so the projection's subpixel caches (live_row_prefix, math
        // band tops) are rebuilt at the new height rather than the height captured at construction.
        // Row and scroll-offset semantics are unaffected — only the pixel scale of each row changes.
        projection.set_cell_height_subpixels(self.cell_height_subpixels);
        projection.set_live_state(
            self.terminal.dimensions().1,
            self.transcript.source_generation(),
            self.grid_generation,
        );
        // A vanished review anchor is a resize-driven reflow (Codex clears scrollback then reprints)
        // only while a resize transaction is open; a user clear is not. The projection gates its
        // frame hold on this so a genuine clear still snaps to the (empty) bottom.
        projection.set_resize_reflow_active(self.resize_epoch.is_active());
        projection.set_exact_source_reprint_hold(self.primary_reprint_presentation_hold());
        projection.set_selection(self.view_selection());
        let mut frozen_artifacts = self
            .decorations
            .iter()
            .filter_map(|(id, record)| {
                (!record.show_source
                    && record
                        .span
                        .as_ref()
                        .is_some_and(|span| span.mode == MathMode::Display))
                .then(|| projected_frozen_artifact(record, self.math_vertical_padding_subpixels()))
                .flatten()
                .map(|artifact| (*id, artifact))
            })
            .collect::<Vec<_>>();
        frozen_artifacts.extend(self.inline_images.values().filter_map(|record| {
            if !matches!(record.kind, InlineImageRecordKind::Osc1337) {
                return None;
            }
            let ContentAnchor::History { id, .. } = self.document.anchor(record.end_anchor).ok()?
            else {
                return None;
            };
            let artifact = record.artifact.as_ref()?;
            self.projected_inline_image(record, artifact, *id)
                .map(|artifact| (*id, artifact))
        }));
        projection.sync_math_artifacts(frozen_artifacts);
        projection.sync_inline_path_artifacts(self.inline_images.values().filter_map(|record| {
            if !matches!(record.kind, InlineImageRecordKind::LocalPath { .. }) {
                return None;
            }
            let ContentAnchor::History { id, .. } = self.document.anchor(record.end_anchor).ok()?
            else {
                return None;
            };
            let artifact = record.artifact.as_ref()?;
            self.projected_inline_image(record, artifact, *id)
                .map(|artifact| (*id, artifact))
        }));
        self.sync_live_projection_artifacts(projection);
        projection.apply_detection_revision(self.detection_revision, &self.document);
        projection.project(&self.document);
    }

    fn sync_live_projection_artifacts(&self, projection: &mut ViewportProjection) {
        let mut live_artifacts = self
            .live_decorations
            .values()
            .filter_map(|record| {
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
                        transition_stale: record.artifact.is_none()
                            && record.stale_artifact.is_some(),
                        frozen_prefix: record.frozen_prefix.clone(),
                        staging_prefix: record.staging_prefix.clone(),
                        generation: record.generation,
                        artifact,
                    })
            })
            .collect::<Vec<_>>();
        live_artifacts.extend(self.inline_images.values().filter_map(|record| {
            let ContentAnchor::Live {
                screen,
                point,
                generation,
                ..
            } = self.document.anchor(record.end_anchor).ok()?
            else {
                return None;
            };
            if *screen != self.live_screen || *generation != self.grid_generation {
                return None;
            }
            let decoded = record.artifact.as_ref()?;
            let artifact = self.projected_inline_image(record, decoded, TranscriptId(0))?;
            let start = match &record.kind {
                InlineImageRecordKind::LocalPath { start_anchor, .. } => {
                    match self.document.anchor(*start_anchor).ok()? {
                        ContentAnchor::Live {
                            screen: start_screen,
                            point,
                            generation: start_generation,
                            ..
                        } if start_screen == screen && start_generation == generation => *point,
                        _ => return None,
                    }
                }
                InlineImageRecordKind::Osc1337 => *point,
            };
            Some(ProjectedLiveMathArtifact {
                occurrence_id: LiveMathOccurrenceId(record.occurrence_id),
                screen: *screen,
                start,
                end: *point,
                band_start_row: start.row,
                band_end_row: point.row,
                clipped_top_rows: 0,
                clipped_bottom_rows: 0,
                occluded_source_rows: 0,
                occluded_visible_rows: Vec::new(),
                transition_stale: false,
                frozen_prefix: Vec::new(),
                staging_prefix: Vec::new(),
                generation: *generation,
                artifact,
            })
        }));
        projection.sync_live_math_artifacts(self.live_screen, live_artifacts);
    }

    fn projected_inline_image(
        &self,
        record: &InlineImageRecord,
        decoded: &DecodedInlineImage,
        end: TranscriptId,
    ) -> Option<ProjectedMathArtifact> {
        let geometry = self.inline_image_geometry(record, decoded)?;
        Some(ProjectedMathArtifact {
            key: decoded.key.clone(),
            end,
            rgba: Arc::clone(&decoded.rgba),
            width_px: decoded.width_px,
            height_px: decoded.height_px,
            height_subpixels: geometry.display_height_subpixels,
            baseline_subpixels: 0,
            mode: MathMode::Display,
            kind: match record.kind {
                InlineImageRecordKind::Osc1337 => bt_viewport::RgbaArtifactKind::InlineImage {
                    animated: decoded.animated,
                },
                InlineImageRecordKind::LocalPath { .. } => {
                    bt_viewport::RgbaArtifactKind::LocalImagePath {
                        animated: decoded.animated,
                    }
                }
            },
            vertical_padding_subpixels: 0,
            render_scale_milli: geometry.display_scale_milli,
            source: match &record.kind {
                InlineImageRecordKind::Osc1337 => "[image]".to_owned(),
                InlineImageRecordKind::LocalPath { path, .. } => {
                    path.as_os_str().to_string_lossy().into_owned()
                }
            },
        })
    }

    fn inline_image_geometry(
        &self,
        record: &InlineImageRecord,
        decoded: &DecodedInlineImage,
    ) -> Option<InlineImageGeometry> {
        let display_anchor = match record.kind {
            InlineImageRecordKind::Osc1337 => record.end_anchor,
            InlineImageRecordKind::LocalPath { start_anchor, .. } => start_anchor,
        };
        let column = match self.document.anchor(display_anchor).ok()? {
            ContentAnchor::Live { point, .. } => point.column,
            ContentAnchor::History { offset, .. } | ContentAnchor::Staging { offset, .. } => {
                offset.0
            }
        };
        let available_columns = self
            .layout_key
            .width_cells
            .get()
            .saturating_sub(column)
            .max(1);
        let available_width_px = i64::from(available_columns)
            .saturating_mul(self.cell_width_subpixels.get())
            .div_euclid(SUBPIXELS_PER_PX)
            .max(1) as u64;
        let viewport_height_subpixels = i64::from(self.terminal.dimensions().1.get())
            .saturating_mul(self.cell_height_subpixels.get());
        let two_thirds_height_px = viewport_height_subpixels
            .saturating_mul(2)
            .div_euclid(3)
            .div_euclid(SUBPIXELS_PER_PX)
            .max(1) as u64;
        let text_floor_rows = self
            .terminal
            .dimensions()
            .1
            .get()
            .saturating_sub(LIVE_MIN_VISIBLE_TEXT_ROWS)
            .max(1);
        let text_floor_height_px = i64::from(text_floor_rows)
            .saturating_mul(self.cell_height_subpixels.get())
            .div_euclid(SUBPIXELS_PER_PX)
            .max(1) as u64;
        let max_height_px = two_thirds_height_px.min(text_floor_height_px);
        let width_scale = available_width_px
            .saturating_mul(1000)
            .checked_div(u64::from(decoded.width_px))?;
        let height_scale = max_height_px
            .saturating_mul(1000)
            .checked_div(u64::from(decoded.height_px))?;
        let display_scale_milli = u64::from(self.layout_key.dpi_milli.get())
            .min(width_scale)
            .min(height_scale)
            .clamp(1, u64::from(u32::MAX)) as u32;
        let display_height_subpixels = i64::from(decoded.height_px)
            .saturating_mul(i64::from(display_scale_milli))
            .saturating_mul(SUBPIXELS_PER_PX)
            .saturating_add(999)
            .div_euclid(1000)
            .max(1);
        let display_rows = u32::try_from(
            display_height_subpixels
                .saturating_add(self.cell_height_subpixels.get() - 1)
                .div_euclid(self.cell_height_subpixels.get()),
        )
        .unwrap_or(u32::MAX)
        .max(1);
        Some(InlineImageGeometry {
            display_scale_milli,
            display_height_subpixels,
            display_rows,
        })
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
            if matches!(
                placement.artifact.kind,
                bt_viewport::RgbaArtifactKind::InlineImage { .. }
                    | bt_viewport::RgbaArtifactKind::LocalImagePath { .. }
            ) {
                let column = match &placement.anchor {
                    MathBlockAnchor::Live { start, .. } => Some(start.column),
                    MathBlockAnchor::History { start, .. } => {
                        self.inline_images.values().find_map(|record| {
                            let display_anchor = match &record.kind {
                                InlineImageRecordKind::Osc1337 => record.end_anchor,
                                InlineImageRecordKind::LocalPath {
                                    path, start_anchor, ..
                                } if path.as_os_str().to_string_lossy()
                                    == placement.source.as_str() =>
                                {
                                    *start_anchor
                                }
                                InlineImageRecordKind::LocalPath { .. } => return None,
                            };
                            let ContentAnchor::History { id, offset, .. } =
                                self.document.anchor(display_anchor).ok()?
                            else {
                                return None;
                            };
                            (*id == *start).then_some(offset.0)
                        })
                    }
                }
                .unwrap_or(0);
                placement.left_subpixels =
                    i64::from(column).saturating_mul(self.cell_width_subpixels.get());
                placement.toolbar_visible = false;
                placement.horizontal_overflow = HorizontalOverflowOwner::Block;
                placement.horizontal_scroll_px = 0;
                placement.vertical_scroll_px = 0;
                placement.clip_height_subpixels = placement.artifact.height_subpixels;
                continue;
            }
            if placement.display == MathBlockDisplay::Rendered
                && placement.artifact.mode == MathMode::Display
                && placement.artifact.kind == bt_viewport::RgbaArtifactKind::Math
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
                clipped_top_rows: 0,
                clipped_bottom_rows: 0,
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
                clipped_top_rows: 0,
                clipped_bottom_rows: 0,
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
                clipped_top_rows: 0,
                clipped_bottom_rows: 0,
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
                clipped_top_rows: 0,
                clipped_bottom_rows: 0,
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
                self.edit_tainted_staging.remove(&staged.id);
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
        // A parse quantum is fed to the vendor terminal whole, so by the time these removal facts
        // are applied the grid already holds the *end* of the quantum. A top-anchored region that
        // committed several rows in one write therefore produces several removal events against one
        // settled grid: proving each record after each single-row event would compare a one-row
        // shift against a grid that already moved further and fail every record. Accumulate the
        // rows a preserved top scroll removed and prove the survivors once, against that settled
        // grid, with the exact total delta.
        let mut pending_top_scroll = 0_usize;
        for event in events {
            self.trace_adapter_event(&event, observed_at);
            match &event {
                AdapterEvent::GridScrolled | AdapterEvent::ScreenCleared => {
                    self.clear_live_edit_taints();
                }
                AdapterEvent::ClearHistory | AdapterEvent::Reset | AdapterEvent::Deccolm => {
                    self.clear_all_edit_taints();
                }
                _ => {}
            }
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
            let preserve_full_screen_scroll = matches!(
                full_screen_output_scroll,
                Some((RemovalScreen::Primary | RemovalScreen::Alternate, count))
                    if count != 0
            );
            let directive = classify(event);
            let batches_top_scroll = preserve_full_screen_scroll
                && !self.resize_epoch.is_active()
                && matches!(directive, LifecycleDirective::RowsRemoved { .. });
            if !batches_top_scroll {
                self.flush_top_scroll_batch(&mut pending_top_scroll);
            }
            match directive {
                LifecycleDirective::RowsRemoved { rows } => {
                    self.cursor_logical_line_memory = None;
                    if self.resize_epoch.is_active() {
                        self.rebase_vendor_owned_rows(rows);
                    } else {
                        let removed = self.apply_removed_rows(
                            rows,
                            preserve_full_screen_scroll,
                            pending_top_scroll,
                        )?;
                        if preserve_full_screen_scroll {
                            pending_top_scroll = pending_top_scroll.saturating_add(removed);
                        }
                    }
                }
                LifecycleDirective::GridCoordinatesInvalidated => {
                    self.cursor_logical_line_memory = None;
                }
                LifecycleDirective::ClearHistoryAndStaging => {
                    self.terminal.clear_resize_transaction_history();
                    let removed = self.transcript.clear_history();
                    self.delete_history(&removed, true);
                }
                LifecycleDirective::InvalidateStaging => {
                    self.cursor_logical_line_memory = None;
                    self.clear_live_edit_taints();
                    self.edit_tainted_staging.clear();
                    self.terminal.clear_resize_transaction_history();
                    self.transcript.invalidate_staging();
                    self.staging_sources.clear();
                    self.active_staging_tail = None;
                    let retired = self
                        .inline_images
                        .values()
                        .filter_map(|record| {
                            matches!(
                                self.document.anchor(record.end_anchor).ok(),
                                Some(ContentAnchor::Live { .. } | ContentAnchor::Staging { .. })
                            )
                            .then_some(record.occurrence_id)
                        })
                        .collect::<BTreeSet<_>>();
                    self.retire_inline_images(&retired);
                    self.document
                        .delete_transaction(&[], true, self.grid_generation);
                }
                LifecycleDirective::ParkPrimary => {
                    self.cursor_logical_line_memory = None;
                    self.clear_live_edit_taints();
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
                    self.cursor_logical_line_memory = None;
                    self.clear_live_edit_taints();
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
                    let retired = self
                        .inline_images
                        .values()
                        .filter_map(|record| {
                            matches!(
                                self.document.anchor(record.end_anchor).ok(),
                                Some(ContentAnchor::Live {
                                    screen: ScreenId::Alternate,
                                    ..
                                })
                            )
                            .then_some(record.occurrence_id)
                        })
                        .collect::<BTreeSet<_>>();
                    self.retire_inline_images(&retired);
                    self.bump_view_generation();
                }
                LifecycleDirective::InlineImage {
                    screen,
                    row,
                    column,
                    encoded,
                } => self.register_inline_image(screen, row, column, encoded),
            }
        }
        self.flush_top_scroll_batch(&mut pending_top_scroll);
        Ok(())
    }

    fn register_inline_image(
        &mut self,
        screen: RemovalScreen,
        row: u32,
        column: u32,
        encoded: Vec<u8>,
    ) {
        let screen = match screen {
            RemovalScreen::Primary => ScreenId::Primary,
            RemovalScreen::Alternate => ScreenId::Alternate,
        };
        let occurrence_id = self.next_inline_image_occurrence_id;
        self.next_inline_image_occurrence_id =
            self.next_inline_image_occurrence_id.saturating_add(1);
        let anchor = self.document.register_anchor(ContentAnchor::Live {
            screen,
            point: GridPoint { row, column },
            bias: Bias::Before,
            generation: self.grid_generation,
        });
        self.inline_images.insert(
            occurrence_id,
            InlineImageRecord {
                occurrence_id,
                end_anchor: anchor,
                kind: InlineImageRecordKind::Osc1337,
                artifact: None,
                failed: false,
            },
        );
        if self.inline_image_tasks.len() == INLINE_IMAGE_WORKER_QUEUE_CAP {
            self.inline_image_tasks.pop_front();
        }
        self.inline_image_tasks.push_back(InlineImageTask {
            occurrence_id,
            source: InlineImageSource::Osc1337(encoded),
        });
    }

    fn reconcile_live_image_paths(&mut self, create_and_retire: bool, stable: &[bool]) {
        if !self.math_layout_options.detect_image_paths {
            return;
        }
        let candidates = self.detected_live_image_paths(stable);
        let screen = self.live_screen;
        let generation = self.grid_generation;
        let mut existing = self
            .inline_images
            .iter()
            .filter_map(|(occurrence, record)| {
                let InlineImageRecordKind::LocalPath {
                    path,
                    source_text,
                    start_anchor,
                } = &record.kind
                else {
                    return None;
                };
                let ContentAnchor::Live {
                    screen: anchor_screen,
                    point: start,
                    generation: anchor_generation,
                    ..
                } = self.document.anchor(*start_anchor).ok()?
                else {
                    return None;
                };
                (*anchor_screen == screen && *anchor_generation == generation).then_some((
                    *occurrence,
                    path.clone(),
                    source_text.clone(),
                    *start,
                ))
            })
            .collect::<Vec<_>>();
        existing.sort_by_key(|(_, _, _, point)| (point.row, point.column));
        let mut matched = BTreeSet::new();

        for candidate in candidates {
            let matched_occurrence =
                existing
                    .iter()
                    .find_map(|(occurrence, path, source_text, _)| {
                        (!matched.contains(occurrence)
                            && *path == candidate.path
                            && *source_text == candidate.source_text)
                            .then_some(*occurrence)
                    });
            if let Some(occurrence) = matched_occurrence {
                matched.insert(occurrence);
                let Some(record) = self.inline_images.get(&occurrence) else {
                    continue;
                };
                let InlineImageRecordKind::LocalPath { start_anchor, .. } = &record.kind else {
                    continue;
                };
                let start_anchor = *start_anchor;
                let end_anchor = record.end_anchor;
                let _ = self.document.replace_anchor(
                    start_anchor,
                    ContentAnchor::Live {
                        screen,
                        point: candidate.start,
                        bias: Bias::Before,
                        generation,
                    },
                );
                let _ = self.document.replace_anchor(
                    end_anchor,
                    ContentAnchor::Live {
                        screen,
                        point: candidate.end,
                        bias: Bias::After,
                        generation,
                    },
                );
            } else if create_and_retire
                && candidate.stable
                && !self.cursor_line_suppression().is_some_and(|suppression| {
                    suppression.intersects(candidate.start.row, candidate.end.row)
                })
                && !self.live_edit_taint_intersects(candidate.start.row, candidate.end.row)
            {
                let occurrence = self.register_local_image_path(
                    candidate.path,
                    candidate.source_text,
                    ContentAnchor::Live {
                        screen,
                        point: candidate.start,
                        bias: Bias::Before,
                        generation,
                    },
                    ContentAnchor::Live {
                        screen,
                        point: candidate.end,
                        bias: Bias::After,
                        generation,
                    },
                );
                matched.insert(occurrence);
            }
        }

        if !create_and_retire {
            return;
        }
        let retired = existing
            .into_iter()
            .filter_map(|(occurrence, _, _, start)| {
                (!matched.contains(&occurrence)
                    && stable.get(start.row as usize).copied().unwrap_or(false))
                .then_some(occurrence)
            })
            .collect::<BTreeSet<_>>();
        self.retire_inline_images(&retired);
    }

    fn detected_live_image_paths(&self, stable: &[bool]) -> Vec<DetectedLiveImagePath> {
        let mut detected = Vec::new();
        let mut logical_text = String::new();
        let mut segments = Vec::<LiveImagePathSegment>::new();
        for row in 0..self.live_rows.len() as u32 {
            let Some(captured) = self.terminal.visible_row(row) else {
                continue;
            };
            let (text, boundaries) = captured_row_text_and_boundaries(&captured);
            let byte_start = logical_text.len();
            logical_text.push_str(&text);
            segments.push(LiveImagePathSegment {
                row,
                byte_start,
                byte_end: logical_text.len(),
                boundaries,
            });
            if captured.continues {
                continue;
            }
            let line_stable = segments
                .iter()
                .all(|segment| stable.get(segment.row as usize).copied().unwrap_or(false));
            for candidate in detect_local_image_path_candidates(&logical_text) {
                let Some(start) = live_path_point(&segments, candidate.byte_start, false) else {
                    continue;
                };
                let Some(end) = live_path_point(&segments, candidate.byte_end, true) else {
                    continue;
                };
                detected.push(DetectedLiveImagePath {
                    path: PathBuf::from(candidate.path),
                    source_text: logical_text.clone(),
                    start,
                    end,
                    stable: line_stable,
                });
            }
            logical_text.clear();
            segments.clear();
        }
        detected
    }

    fn detect_frozen_image_paths(&mut self, id: TranscriptId) {
        if !self.math_layout_options.detect_image_paths
            || self.edit_tainted_transcript.contains(&id)
        {
            return;
        }
        let Some(line) = self
            .document
            .entries()
            .get(&id)
            .map(|entry| entry.line.clone())
        else {
            return;
        };
        for candidate in detect_local_image_path_candidates(&line.text) {
            let path = PathBuf::from(candidate.path);
            let already_registered = self.inline_images.values().any(|record| {
                let InlineImageRecordKind::LocalPath {
                    path: existing_path,
                    source_text,
                    ..
                } = &record.kind
                else {
                    return false;
                };
                existing_path == &path
                    && source_text == &line.text
                    && matches!(
                        self.document.anchor(record.end_anchor).ok(),
                        Some(ContentAnchor::History {
                            id: anchor_id, ..
                        }) if *anchor_id == id
                    )
            });
            if already_registered {
                continue;
            }
            let Some(start_offset) =
                grapheme_offset_at_byte(&line.grapheme_boundaries, candidate.byte_start)
            else {
                continue;
            };
            let Some(end_offset) =
                grapheme_offset_at_byte(&line.grapheme_boundaries, candidate.byte_end)
            else {
                continue;
            };
            self.register_local_image_path(
                path,
                line.text.clone(),
                ContentAnchor::History {
                    id,
                    offset: start_offset,
                    bias: Bias::Before,
                    generation: line.source_generation,
                },
                ContentAnchor::History {
                    id,
                    offset: end_offset,
                    bias: Bias::After,
                    generation: line.source_generation,
                },
            );
        }
    }

    fn register_local_image_path(
        &mut self,
        path: PathBuf,
        source_text: String,
        start: ContentAnchor,
        end: ContentAnchor,
    ) -> u64 {
        let occurrence_id = self.next_inline_image_occurrence_id;
        self.next_inline_image_occurrence_id =
            self.next_inline_image_occurrence_id.saturating_add(1);
        let start_anchor = self.document.register_anchor(start);
        let end_anchor = self.document.register_anchor(end);
        self.inline_images.insert(
            occurrence_id,
            InlineImageRecord {
                occurrence_id,
                end_anchor,
                kind: InlineImageRecordKind::LocalPath {
                    path: path.clone(),
                    source_text,
                    start_anchor,
                },
                artifact: None,
                failed: false,
            },
        );
        if self.local_image_path_tasks.len() == LOCAL_IMAGE_PATH_WORKER_QUEUE_CAP
            && let Some(dropped) = self.local_image_path_tasks.pop_front()
            && let Some(record) = self.inline_images.get_mut(&dropped.occurrence_id)
        {
            record.failed = true;
        }
        self.local_image_path_tasks.push_back(InlineImageTask {
            occurrence_id,
            source: InlineImageSource::LocalPath(path),
        });
        occurrence_id
    }

    /// Prove and re-seat the live decorations a batched top-anchored scroll displaced, then reset
    /// the batch. The grid is settled at this point, so the accumulated row count is the exact
    /// delta between where each proven record was and where its source now sits.
    fn flush_top_scroll_batch(&mut self, pending: &mut usize) {
        let removed = std::mem::take(pending);
        if removed != 0 {
            self.preserve_live_after_top_scroll(removed);
        }
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

    fn removed_row_is_edit_tainted(&self, live_row: u32, row: &CapturedRow) -> bool {
        let content = EditTaintedRow::from_captured(row);
        self.active_edit_taints
            .iter()
            .any(|taint| taint.screen == self.live_screen && taint.intersects(live_row, live_row))
            || self.committed_live_edit_taints.iter().any(|taint| {
                taint.screen == self.live_screen && taint.row_matches(live_row, &content)
            })
    }

    fn rebase_live_edit_taints_after_top_removal(&mut self, removed_rows: usize) {
        let Ok(removed_rows) = u32::try_from(removed_rows) else {
            self.clear_live_edit_taints();
            return;
        };
        let rebase = |taints: &mut Vec<LiveEditTaint>| {
            taints.retain_mut(|taint| {
                if taint.end() < removed_rows {
                    return false;
                }
                if taint.start < removed_rows {
                    let removed_from_taint = (removed_rows - taint.start) as usize;
                    taint.rows.drain(..removed_from_taint.min(taint.rows.len()));
                    taint.start = 0;
                } else {
                    taint.start -= removed_rows;
                }
                !taint.rows.is_empty()
            });
        };
        rebase(&mut self.active_edit_taints);
        rebase(&mut self.committed_live_edit_taints);
    }

    /// Returns the number of live rows this event removed, so a preserved top scroll can batch its
    /// decoration proof across every removal event in the same parse quantum.
    fn apply_removed_rows(
        &mut self,
        rows: Vec<RowDirective>,
        preserve_full_screen_scroll: bool,
        batched_top_scroll_rows: usize,
    ) -> Result<usize, SessionError> {
        let mut removals = Vec::new();
        let mut captured = Vec::<CaptureResult>::new();
        let mut captured_staging = BTreeMap::new();
        for row in rows {
            match row {
                RowDirective::Capture { live_row, row } => {
                    let grapheme_offsets = captured_grapheme_offsets(&row);
                    let captured_row = row.clone();
                    let edit_tainted = self.removed_row_is_edit_tainted(live_row, &row);
                    let result = self.transcript.capture(row);
                    if edit_tainted {
                        self.edit_tainted_staging.insert(
                            result.staging_id,
                            EditTaintedRow::from_captured(&captured_row),
                        );
                    }
                    let generation = self.transcript.source_generation();
                    removals.push(LiveRowRemoval {
                        row: live_row,
                        staging: Some((result.staging_id, generation)),
                        grapheme_offsets,
                    });
                    captured_staging.insert(live_row, (result.staging_id, captured_row));
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
            return Ok(0);
        }

        if preserve_full_screen_scroll {
            self.remember_live_artifact_handoffs(&captured_staging, batched_top_scroll_rows);
        }
        if !preserve_full_screen_scroll {
            self.invalidate_all_live_decorations();
        }
        self.live_tasks.clear();
        self.grid_generation.0 += 1;
        if preserve_full_screen_scroll {
            self.rebase_live_edit_taints_after_top_removal(removals.len());
        } else {
            self.clear_live_edit_taints();
        }
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
        Ok(removals.len())
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

    /// `batched_top_scroll_rows` is how many rows earlier removal events in this same parse quantum
    /// already took off the top without yet re-seating the records (see `flush_top_scroll_batch`).
    /// Record placements are still expressed against the pre-batch grid, so the row a proven source
    /// line occupies now is its logical target minus that pending shift.
    fn remember_live_artifact_handoffs(
        &mut self,
        captured_rows: &BTreeMap<u32, (StagingId, CapturedRow)>,
        batched_top_scroll_rows: usize,
    ) {
        if self.live_screen != ScreenId::Primary {
            return;
        }
        let pending_shift = i64::try_from(batched_top_scroll_rows).unwrap_or(i64::MAX);
        let mut staging_prefix_updates = Vec::new();
        for record in self.live_decorations.values() {
            let captured_source = record
                .identity
                .source_rows
                .iter()
                .enumerate()
                .filter_map(|(index, proven)| {
                    let target = record
                        .placement
                        .logical_band_start
                        .checked_add(i64::from(proven.band_offset))?
                        .checked_sub(pending_shift)?;
                    let row = u32::try_from(target).ok()?;
                    let (staging, captured) = captured_rows.get(&row)?;
                    let (text, cell_boundaries) = captured_row_text_and_boundaries(captured);
                    let captured = LiveDetectionInput {
                        source: LiveDetectionSource::Grid { row, revision: 0 },
                        text,
                        continues: captured.continues,
                        cell_boundaries,
                    };
                    proven
                        .exactly_matches(&captured)
                        .then_some((index, *staging))
                })
                .collect::<Vec<_>>();
            let Some(&(first_index, candidate_staging)) = captured_source.first() else {
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
            if let Some(pending) = self
                .pending_live_handoffs
                .iter_mut()
                .find(|pending| pending.occurrence_id == record.identity.occurrence_id)
            {
                let next = pending.prefix_staging.len();
                if first_index != next
                    || captured_source
                        .iter()
                        .enumerate()
                        .any(|(offset, (index, _))| *index != next.saturating_add(offset))
                {
                    continue;
                }
                pending
                    .prefix_staging
                    .extend(captured_source.iter().map(|(_, staging)| *staging));
                staging_prefix_updates.push((
                    pending.occurrence_id,
                    pending.prefix_staging[pending.finalized_prefix_staging..].to_vec(),
                ));
                continue;
            }
            // A record first detected as a frozen/live bridge already has a history prefix whose
            // staging lineage predates this handoff. Do not reconstruct that lineage from text.
            // This path only grows a prefix from row zero of an all-live, already-proven occurrence.
            if !record.frozen_prefix.is_empty() || first_index != 0 {
                continue;
            }
            if captured_source
                .iter()
                .enumerate()
                .any(|(index, (source_index, _))| *source_index != index)
            {
                continue;
            }
            let pending = PendingLiveArtifactHandoff {
                occurrence_id: record.identity.occurrence_id,
                span: record.span.clone(),
                artifact: artifact.clone(),
                layout: record.layout,
                detection_revision: record.detection_revision,
                candidate_staging,
                candidate_start: None,
                expected_frozen_lines: u64::try_from(record.identity.source_rows.len())
                    .unwrap_or(u64::MAX),
                prefix_staging: captured_source
                    .iter()
                    .map(|(_, staging)| *staging)
                    .collect(),
                finalized_prefix_staging: 0,
                frozen_prefix: Vec::new(),
            };
            staging_prefix_updates.push((pending.occurrence_id, pending.prefix_staging.clone()));
            self.pending_live_handoffs.push(pending);
        }
        for (occurrence_id, staging_prefix) in staging_prefix_updates {
            if let Some(record) = self
                .live_decorations
                .values_mut()
                .find(|record| record.identity.occurrence_id == occurrence_id)
            {
                record.staging_prefix = staging_prefix;
            }
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
        let edit_tainted = finalized
            .mappings
            .iter()
            .any(|mapping| self.edit_tainted_staging.contains_key(&mapping.staging_id));
        for mapping in &finalized.mappings {
            self.edit_tainted_staging.remove(&mapping.staging_id);
        }
        if edit_tainted {
            self.edit_tainted_transcript.insert(id);
        }
        let mut live_prefix_updates = Vec::new();
        for pending in &mut self.pending_live_handoffs {
            if pending.candidate_start.is_none()
                && finalized
                    .mappings
                    .iter()
                    .any(|mapping| mapping.staging_id == pending.candidate_staging)
            {
                pending.candidate_start = Some(id);
            }
            let before = pending.finalized_prefix_staging;
            while pending
                .prefix_staging
                .get(pending.finalized_prefix_staging)
                .is_some_and(|staging| {
                    finalized
                        .mappings
                        .iter()
                        .any(|mapping| mapping.staging_id == *staging)
                })
            {
                pending.finalized_prefix_staging =
                    pending.finalized_prefix_staging.saturating_add(1);
            }
            if pending.finalized_prefix_staging != before {
                if pending.frozen_prefix.last() != Some(&id) {
                    pending.frozen_prefix.push(id);
                }
                live_prefix_updates.push((
                    pending.occurrence_id,
                    pending.frozen_prefix.clone(),
                    pending.prefix_staging[pending.finalized_prefix_staging..].to_vec(),
                ));
            }
        }
        self.document.finalize_transaction(finalized);
        for (occurrence_id, frozen_prefix, staging_prefix) in live_prefix_updates {
            if let Some(record) = self
                .live_decorations
                .values_mut()
                .find(|record| record.identity.occurrence_id == occurrence_id)
            {
                record.frozen_prefix = frozen_prefix;
                record.staging_prefix = staging_prefix;
            }
        }
        self.retire_stale_bridge_prefixes(false);
        self.detect_frozen_image_paths(id);
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
        // The exact live occurrence has now transferred its existing raster and ownership to the
        // complete frozen outer block. Close the same interior-record race as an ordinary frozen
        // worker completion does in `apply_worker_completion`: `\begin{env}` / `\end{env}` rows may
        // already have queued scans from the earlier streaming prefix, but they are body of this
        // Dollars block and must become Suppressed before those completions can land independently.
        //
        // This is not reconcile re-installation. The outer record above is the one deterministic
        // live→frozen handoff of an exact source-equivalent raster; suppression only records that its
        // strict interior has no separate presentation owner. No raster is rebuilt, no band is
        // recomputed, and a true bare environment (with no enclosing handed-off block) never reaches
        // this call.
        self.suppress_block_interior(block.start, block.end);
        if closing_id != block.start
            && let Some(candidate) = self.decorations.get_mut(&closing_id)
        {
            candidate.decoration = DecorationLifecycle::None;
        }
        self.scheduler.remove_sources(&BTreeSet::from([closing_id]));
        self.retire_offscreen_records_replaced_by_frozen();
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
        // Cheap prefilter: a certified-clean boundary is always dumb-parity-neutral too (the two agree
        // wherever there is no phantom to abandon), so only pay for the resync certification scan at a
        // dumb-neutral line. This keeps normal frozen ingestion O(1) per line — the resync only runs at
        // block boundaries over the small uncertified segment — while a phantom-poisoned stretch still
        // certifies once its forward block lands.
        if self.frozen_detection_context.is_neutral() {
            self.advance_frozen_frontier(id);
        }
        self.decorations
            .insert(id, DecorationRecord::frozen(versions));
        // Ordinary frozen lines take only the allocation-free delimiter prefilter. A candidate
        // snapshots immutable source here; the worker owns fence/pairing/escape/size detection.
        if may_contain_math
            && !self.edit_tainted_transcript.contains(&id)
            && !self.primary_parked
            && self.resize_epoch.decorations_allowed()
        {
            self.schedule_scan(id);
        }
    }

    fn delete_history(&mut self, removed: &[TranscriptId], clear_staging: bool) {
        let removed_set = removed.iter().copied().collect::<BTreeSet<_>>();
        let retired_images = self
            .inline_images
            .values()
            .filter_map(|record| {
                let retired = match self.document.anchor(record.end_anchor).ok() {
                    Some(ContentAnchor::History { id, .. }) => removed_set.contains(id),
                    Some(ContentAnchor::Staging { .. }) => clear_staging,
                    _ => false,
                };
                retired.then_some(record.occurrence_id)
            })
            .collect::<BTreeSet<_>>();
        self.retire_inline_images(&retired_images);
        self.document
            .delete_transaction(removed, clear_staging, self.grid_generation);
        if clear_staging {
            self.staging_sources.clear();
            self.active_staging_tail = None;
            self.edit_tainted_staging.clear();
            self.pending_live_handoffs.clear();
            self.frozen_detection_context = DetectionContext::default();
            self.frozen_detection_contexts.clear();
            self.frozen_certified_through = None;
        }
        self.scheduler.remove_sources(&removed_set);
        for id in removed {
            self.edit_tainted_transcript.remove(id);
            self.decorations.remove(id);
            self.frozen_detection_contexts.remove(id);
        }
        // A live record's frozen prefix names transcript lines that must still exist for the bridge
        // to span them. Scrollback eviction and Codex's `ESC [ 3 J` delete those lines outright, and
        // a prefix that survives its own lines is neither a bridge (the viewport rejects it — the
        // ids are no longer the history tail) nor an honest live-only band: it suppresses the
        // free-height budget the clipped-top rows need and the raster clips short of its own ink.
        // A prefix is all-or-nothing, so losing any line retires the whole prefix; the rows above
        // the grid remain counted as clipped, which is what they now are.
        for pending in &mut self.pending_live_handoffs {
            if pending
                .frozen_prefix
                .iter()
                .any(|id| removed_set.contains(id))
            {
                pending.frozen_prefix.clear();
            }
        }
        self.retire_stale_bridge_prefixes(clear_staging);
        // A removed line may sit at or before the certified frontier; re-certify from scratch rather
        // than trust a frontier whose proving segment no longer exists.
        if self
            .frozen_certified_through
            .is_some_and(|through| removed_set.iter().any(|id| *id <= through))
        {
            self.frozen_certified_through = None;
        }
    }

    fn retire_inline_images(&mut self, occurrences: &BTreeSet<u64>) {
        if occurrences.is_empty() {
            return;
        }
        self.inline_images
            .retain(|occurrence, _| !occurrences.contains(occurrence));
        self.inline_image_tasks
            .retain(|task| !occurrences.contains(&task.occurrence_id));
        self.local_image_path_tasks
            .retain(|task| !occurrences.contains(&task.occurrence_id));
    }

    /// A live record's bridge prefix is a claim on transcript lines that sit immediately above live
    /// row zero. That claim holds only while those exact ids are the contiguous *tail* of history:
    /// the lines are deleted (eviction, Codex's `ESC [ 3 J`), or other lines are frozen after them
    /// (a reprint re-emits the same text under fresh ids, or a neighbouring block's rows land in
    /// between), and the occurrence no longer adjoins the grid.
    ///
    /// A stale claim is not inert. Presentation reads a non-empty prefix as "this band is a bridge,
    /// sized across both planes", skips the free-height budget, and then the viewport rejects the
    /// bridge because the ids are not the tail — so the raster clips short of its own ink inside a
    /// band that is not a bridge either, and the block's real frozen source shows above it. It also
    /// blocks `remember_live_artifact_handoffs` from rebuilding a fresh lineage, so the prefix can
    /// never recover on its own. Retiring it restores both.
    ///
    /// Called wherever history changes: a line finalizes into the document, or lines are deleted.
    fn retire_stale_bridge_prefixes(&mut self, clear_staging: bool) {
        // A prefix line that has become a *rendered* history block means the frozen pipeline has
        // finished pairing this occurrence on its own. The two cannot share one band, and the frozen
        // artifact is the one anchored to durable transcript ids, so the live record is a superseded
        // duplicate: keeping it paints a one-row clip of the same raster over the history rendering.
        // This is a handoff completing, not an invalidation, so it is not counted as one.
        let superseded = self
            .live_decorations
            .iter()
            .filter(|(_, record)| {
                record.frozen_prefix.iter().any(|id| {
                    self.decorations.get(id).is_some_and(|frozen| {
                        !frozen.show_source
                            && frozen.artifact.is_some()
                            && frozen
                                .span
                                .as_ref()
                                .is_some_and(|span| span.mode == MathMode::Display)
                    })
                })
            })
            .map(|(start, _)| *start)
            .collect::<Vec<_>>();
        for start in superseded {
            self.live_decorations.remove(&start);
        }
        let document = &self.document;
        let snapshots = self
            .primary_repaint_snapshot
            .iter_mut()
            .chain(self.alternate_repaint_snapshot.iter_mut())
            .flat_map(|snapshot| {
                snapshot
                    .decorations
                    .iter_mut()
                    .chain(snapshot.dormant_decorations.iter_mut())
            });
        for record in self
            .live_decorations
            .values_mut()
            .chain(self.offscreen_decorations.iter_mut())
            .chain(snapshots)
        {
            if clear_staging {
                record.staging_prefix.clear();
            }
            if record.frozen_prefix.is_empty() {
                continue;
            }
            let is_history_tail = document
                .entries()
                .keys()
                .rev()
                .take(record.frozen_prefix.len())
                .eq(record.frozen_prefix.iter().rev());
            if !is_history_tail {
                record.frozen_prefix.clear();
                record.staging_prefix.clear();
            }
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
            .filter_map(|(id, entry)| {
                (may_contain_display_math(&entry.line.text)
                    && !self.edit_tainted_transcript.contains(id))
                .then_some(*id)
            })
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

    /// First resident id strictly past the certified repair frontier — the anchor for a resync
    /// window, and a proven `Known` neutral start. `None` when there are no resident lines.
    fn frozen_certified_anchor(&self) -> Option<TranscriptId> {
        match self.frozen_certified_through {
            None => self.document.entries().keys().next().copied(),
            Some(through) => self
                .document
                .entries()
                .range((Bound::Excluded(through), Bound::Unbounded))
                .next()
                .map(|(id, _)| *id),
        }
    }

    /// Advance the certified repair frontier (review §B) after `newest` freezes. The frontier passes
    /// a line only when the resync scan of the whole uncertified segment `[anchor..=newest]` ends
    /// CLEANLY neutral: nothing left open AND every structural delimiter owned by a resolved block.
    /// A lone orphan (an `\end{env}` whose opener was eaten, prose carrying a literal `$$`) or a
    /// still-provisional phantom leaves an uncovered delimiter, so the frontier waits behind it —
    /// which is correct: certifying past such a point could be revised by later evidence, and the
    /// scan from a proven-neutral anchor stays bounded by the source-byte cap regardless. The frontier
    /// is monotonic; it only ever moves forward within a source revision.
    fn advance_frozen_frontier(&mut self, newest: TranscriptId) {
        let Some(anchor) = self.frozen_certified_anchor() else {
            return;
        };
        if anchor > newest || !self.frozen_anchor_is_neutral(anchor) {
            return;
        }
        let options = self.detection_options();
        let mut segment: Vec<(TranscriptId, String)> = Vec::new();
        let mut bytes = 0usize;
        for (id, entry) in self.document.entries().range(anchor..=newest) {
            bytes = bytes
                .saturating_add(entry.line.text.len())
                .saturating_add(1);
            if bytes > MAX_MATH_SOURCE_BYTES {
                // The uncertified segment outran the proof-epoch budget; leave the frontier where it
                // is (schedule_scan uses a bounded fallback window for candidates inside it).
                return;
            }
            segment.push((*id, entry.line.text.clone()));
        }
        if segment.is_empty() {
            return;
        }
        let (blocks, final_neutral) = frozen_resync_scan_with_options(
            segment.iter().map(|(id, text)| (*id, text.as_str())),
            DetectionContext::default(),
            options,
        );
        if !final_neutral {
            return;
        }
        let covered = |id: TranscriptId| {
            blocks
                .iter()
                .any(|block| block.start <= id && id <= block.end)
        };
        let clean = segment
            .iter()
            .all(|(id, text)| !may_contain_display_math(text) || covered(*id));
        if clean {
            self.frozen_certified_through = Some(newest);
        }
    }

    /// The certified anchor is only a sound `Known` neutral start when its own recorded parser
    /// snapshot is neutral. At a clean frontier point the dumb parity and the resync agree, so this
    /// holds; it fails safe for a mid-block first line left by a partial history eviction (no proven
    /// neutral prefix), in which case the caller falls back to the `required_start` hint.
    fn frozen_anchor_is_neutral(&self, anchor: TranscriptId) -> bool {
        self.frozen_detection_contexts
            .get(&anchor)
            .is_some_and(DetectionContext::is_neutral)
    }

    /// Gather the resync window `[anchor..=candidate_id]` as worker inputs, or `None` if it does not
    /// span exactly that range or exceeds the source-byte cap (the proof-epoch budget).
    fn frozen_window_inputs(
        &self,
        anchor: TranscriptId,
        candidate_id: TranscriptId,
    ) -> Option<Vec<DetectionInput>> {
        let mut inputs = Vec::new();
        let mut source_bytes = 0usize;
        for (id, entry) in self.document.entries().range(anchor..=candidate_id) {
            source_bytes = source_bytes
                .saturating_add(entry.line.text.len())
                .saturating_add(1);
            if source_bytes > MAX_MATH_SOURCE_BYTES.saturating_add(1) {
                return None;
            }
            inputs.push(DetectionInput {
                id: *id,
                text: entry.line.text.clone(),
                cell_boundaries: frozen_cell_boundaries(&entry.line),
            });
        }
        (inputs.first().is_some_and(|input| input.id == anchor)
            && inputs.last().is_some_and(|input| input.id == candidate_id))
        .then_some(inputs)
    }

    fn schedule_scan(&mut self, candidate_id: TranscriptId) {
        if self.edit_tainted_transcript.contains(&candidate_id) {
            return;
        }
        let detection_options = self.detection_options();
        let Some(candidate_context) = self.frozen_detection_contexts.get(&candidate_id) else {
            return;
        };
        // Certified-frontier window (review §B): anchor the scan at the proven-neutral frontier and
        // let the worker's resync abandon a lost-opener `$$` phantom, instead of trusting the
        // poisoned dumb-parity `required_start`. Used only when the window fits the source-byte cap
        // from a proven-`Known` neutral anchor; otherwise fall back to the `required_start` hint.
        if let Some(anchor) = self.frozen_certified_anchor()
            && anchor <= candidate_id
            && self.frozen_anchor_is_neutral(anchor)
            && let Some(inputs) = self.frozen_window_inputs(anchor, candidate_id)
        {
            let Some(mut task) = self.decorations.get_mut(&candidate_id).and_then(|record| {
                record.schedule_scan(
                    candidate_id,
                    DetectionContext::default(),
                    Arc::from(inputs),
                    detection_options,
                )
            }) else {
                return;
            };
            task.cell_width_subpixels = self.cell_width_subpixels.get();
            task.cell_height_subpixels = self.cell_height_subpixels.get();
            task.ascii_baseline_subpixels =
                self.ascii_baseline_subpixels.map_or(0, NonZeroI64::get);
            self.frozen_detection_count = self.frozen_detection_count.saturating_add(1);
            self.enqueue_task(task);
            return;
        }
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
        if self
            .edit_tainted_staging
            .get(&id)
            .is_some_and(|expected| *expected != EditTaintedRow::from_captured(&row))
        {
            self.edit_tainted_staging.remove(&id);
        }
        if !self.transcript.rewrite_staged(id, row) {
            self.active_staging_tail = None;
            self.edit_tainted_staging.remove(&id);
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

fn stale_pending_dpi_transition(record: &LiveDecorationRecord) -> bool {
    record.artifact.is_none()
        && record
            .stale_artifact
            .as_ref()
            .is_some_and(|stale| stale.rendered_layout.dpi_milli != record.layout.dpi_milli)
}

fn resize_transition_side_occurrences<'a>(
    records: impl Iterator<Item = &'a LiveDecorationRecord>,
) -> BTreeSet<LiveMathOccurrenceId> {
    let records = records.collect::<Vec<_>>();
    records
        .iter()
        .filter(|record| {
            records.iter().any(|other| {
                record.identity.occurrence_id != other.identity.occurrence_id
                    && record.span.original_source != other.span.original_source
                    && record.span.original_source.trim_start().starts_with("$$")
                        != other.span.original_source.trim_start().starts_with("$$")
                    && record
                        .span
                        .render_source
                        .trim()
                        .lines()
                        .map(str::trim)
                        .eq(other.span.render_source.trim().lines().map(str::trim))
                    && record.span.mode == other.span.mode
            })
        })
        .map(|record| record.identity.occurrence_id)
        .collect()
}

fn is_outer_environment_record(record: &LiveDecorationRecord) -> bool {
    record.span.original_source.trim_start().starts_with("$$")
        && record.span.original_source.contains(r"\begin{")
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
        kind: bt_viewport::RgbaArtifactKind::Math,
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

fn ranges_intersect(left_start: u32, left_end: u32, right_start: u32, right_end: u32) -> bool {
    left_start <= right_end && right_start <= left_end
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

fn size_resolved_live_task_band(task: &mut LiveDetectionTask) {
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
    } else {
        // Presentation ownership is the exact source band on both screens. Primary used to borrow
        // up to two adjacent whitespace-only rows to spread a tall raster over more terminal rows.
        // Those rows are not detector-proven source: in a TUI they are often the intentional blank
        // separator between formula blocks or a textless, styled input-box/chrome row. Once
        // `suppress_math_source_cell` began resetting complete cells, borrowing erased that styling;
        // even before then it collapsed separators and let the raster's clip extend into chrome.
        // Free height already expands an exact source row in the projection-local prefix map, so no
        // neighbouring terminal row is needed for geometry.
        task.band_start_row = task.start.row;
        task.band_end_row = task.end.row;
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

/// Stable lifecycle label shared by the runtime `BT_DECOR_TRACE` snapshot and the oracle's
/// `BT_PROBE_FROZEN` dump so both name a record's state identically.
pub fn decoration_state_label(decoration: DecorationLifecycle) -> &'static str {
    match decoration {
        DecorationLifecycle::None => "none",
        DecorationLifecycle::Pending => "pending",
        DecorationLifecycle::Ready => "ready",
        DecorationLifecycle::Failed => "failed",
        DecorationLifecycle::Suppressed => "suppressed",
    }
}

/// One-line, bounded source excerpt for a decoration trace: trimmed, control characters neutralized,
/// and capped so a runaway line cannot bloat the trace file.
fn decor_trace_excerpt(source: &str) -> String {
    const MAX: usize = 96;
    let mut out = String::with_capacity(MAX.min(source.len()));
    for ch in source.trim().chars() {
        if out.chars().count() >= MAX {
            out.push('…');
            break;
        }
        out.push(if ch.is_control() || ch == '"' {
            ' '
        } else {
            ch
        });
    }
    out
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

/// The mapping set for a primary in-stream reprint: the proven segmented row mapping plus a forced
/// identity mapping over the whole grid. A primary reprint rewrites only part of the screen, so a
/// record whose rows did not move must map straight through even when the segmented mapping found no
/// moving anchor and is empty. A record maps Visible under at most one of {identity, a segmented
/// delta} — an unchanged record at its own rows, a shifted one at its rows plus the delta — so
/// `project_live_record_uniquely` still yields a single placement. Deduplicated so identity is not
/// tried twice when the segmented mapping already is identity.
fn primary_repaint_row_mappings(
    before: &[LiveDetectionInput],
    after: &[LiveDetectionInput],
) -> Vec<SegmentedRowMapping> {
    let mut mappings = segmented_row_mapping(before, after);
    let Some(last_row) = after_grid_row_count(after).checked_sub(1) else {
        return mappings;
    };
    let identity = SegmentedRowMapping {
        content_delta: 0,
        content_start_row: 0,
        content_end_row: last_row,
        fixed_start_row: None,
    };
    if !mappings.contains(&identity) {
        mappings.push(identity);
    }
    mappings
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
            Some(RecordProjection::Visible(record)) => {
                let support = projected_exact_source_row_support(&record);
                debug_assert!(support != 0);
                visible.push((support, record));
            }
            Some(RecordProjection::Dormant(record)) => dormant.push(record),
            None => {}
        }
    }
    if visible.len() == 1 {
        return visible
            .pop()
            .map(|(_, record)| RecordProjection::Visible(record));
    }
    if visible.len() > 1 {
        if !is_outer_environment_record(record) {
            return None;
        }
        // A top-edge transition can make both the transaction's proven moving delta and primary's
        // forced identity mapping technically Visible. The real `scroll-strand.vt` shape is the
        // minimal counterexample: delta -2 retains five byte-exact rows of an outer
        // `$$ A= \begin{pmatrix} ... $$` block, while identity retains only its closing `$$`
        // because that row now contains the following block's opening `$$`. Treating every
        // Visible candidate as an equal ambiguity drops the outer owner and lets the inner
        // environment take over.
        //
        // This does not loosen projection or source equality: every candidate here has already
        // passed `project_live_record`, including its exact row/cell proof and boundary-only
        // mismatch rule. Select only a unique strongest proof by the number of the occurrence's
        // source rows that remain byte/boundary-exact at that placement. A tie stays ambiguous and
        // returns `None`, preserving the no-wrong-raster guard.
        let strongest = visible.iter().map(|(support, _)| *support).max()?;
        let winners = visible
            .iter()
            .filter(|(support, _)| *support == strongest)
            .count();
        if winners != 1 {
            return None;
        }
        return visible
            .into_iter()
            .find(|(support, _)| *support == strongest)
            .map(|(_, record)| RecordProjection::Visible(record));
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

fn projected_exact_source_row_support(record: &LiveDecorationRecord) -> usize {
    record
        .identity
        .source_rows
        .iter()
        .filter(|proven| {
            let Some(target) = record
                .placement
                .logical_band_start
                .checked_add(i64::from(proven.band_offset))
            else {
                return false;
            };
            let Ok(target) = u32::try_from(target) else {
                return false;
            };
            live_grid_input(&record.inputs, target)
                .is_some_and(|input| proven.exactly_matches(input))
        })
        .count()
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
    // A bridge prefix is only meaningful directly above live row zero — that is the geometry the
    // viewport composes, and its precondition. A projection that seats this band lower has proven
    // the rows it owns inside the grid at a place no history line adjoins, so the prefix claim is
    // void. Leaving it attached is not neutral: the presentation reads a non-empty prefix as "this
    // band is a bridge, sized elsewhere" and skips the free-height budget, so the raster clips short
    // of its own ink inside a band that is not a bridge either.
    if record.band_start_row != 0 {
        record.frozen_prefix.clear();
        record.staging_prefix.clear();
    }
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

fn live_path_point(
    segments: &[LiveImagePathSegment],
    byte: usize,
    prefer_previous_boundary: bool,
) -> Option<GridPoint> {
    let segment = if prefer_previous_boundary {
        segments
            .iter()
            .find(|segment| segment.byte_start <= byte && byte <= segment.byte_end)
    } else {
        segments
            .iter()
            .rev()
            .find(|segment| segment.byte_start <= byte && byte < segment.byte_end)
            .or_else(|| segments.last().filter(|segment| byte == segment.byte_end))
    }?;
    let local = u32::try_from(byte.saturating_sub(segment.byte_start)).ok()?;
    let column = segment
        .boundaries
        .iter()
        .rev()
        .find(|(boundary, _)| *boundary <= local)
        .map(|(_, column)| *column)?;
    Some(GridPoint {
        row: segment.row,
        column,
    })
}

fn grapheme_offset_at_byte(boundaries: &[u32], byte: usize) -> Option<GraphemeOffset> {
    let byte = u32::try_from(byte).ok()?;
    boundaries
        .binary_search(&byte)
        .ok()
        .and_then(|index| u32::try_from(index).ok())
        .map(GraphemeOffset)
}

fn content_anchor_between(
    candidate: &ContentAnchor,
    start: &ContentAnchor,
    end: &ContentAnchor,
) -> bool {
    match (candidate, start, end) {
        (
            ContentAnchor::History {
                id,
                offset,
                generation,
                ..
            },
            ContentAnchor::History {
                id: start_id,
                offset: start_offset,
                generation: start_generation,
                ..
            },
            ContentAnchor::History {
                id: end_id,
                offset: end_offset,
                generation: end_generation,
                ..
            },
        ) => {
            id == start_id
                && id == end_id
                && generation == start_generation
                && generation == end_generation
                && start_offset <= offset
                && offset < end_offset
        }
        (
            ContentAnchor::Staging {
                id,
                offset,
                generation,
                ..
            },
            ContentAnchor::Staging {
                id: start_id,
                offset: start_offset,
                generation: start_generation,
                ..
            },
            ContentAnchor::Staging {
                id: end_id,
                offset: end_offset,
                generation: end_generation,
                ..
            },
        ) => {
            id == start_id
                && id == end_id
                && generation == start_generation
                && generation == end_generation
                && start_offset <= offset
                && offset < end_offset
        }
        (
            ContentAnchor::Live {
                screen,
                point,
                generation,
                ..
            },
            ContentAnchor::Live {
                screen: start_screen,
                point: start_point,
                generation: start_generation,
                ..
            },
            ContentAnchor::Live {
                screen: end_screen,
                point: end_point,
                generation: end_generation,
                ..
            },
        ) => {
            screen == start_screen
                && screen == end_screen
                && generation == start_generation
                && generation == end_generation
                && (point.row, point.column) >= (start_point.row, start_point.column)
                && (point.row, point.column) < (end_point.row, end_point.column)
        }
        _ => false,
    }
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

    fn hide_cursor(session: &mut DualPlaneSession, observed_at: Instant) {
        // These legacy fixtures exercise detector/lifecycle behavior unrelated to prompt editing.
        // They predate the cursor-line gate and use a hidden cursor solely to state that no input
        // line exists. Install that precondition directly; dedicated tests below prove each real
        // clear, scroll, and screen-switch event which retires the production memory.
        session.cursor_logical_line_memory = None;
        session.feed_at(b"\x1b[?25l", observed_at).unwrap();
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
                restore_stripped_inline_environment_newlines: false,
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

    /// Drive the exact app zoom sequence (`reconcile_authoritative_dpi`): remeasure the cell metrics,
    /// resize the grid, then set the new-DPI `LayoutKey`. A zoom that shrinks the font also grows the
    /// grid (a real ConPTY resize), so this reproduces the LayoutKey change + resize_at + reprint
    /// storm the real machine sees, without any zoom recording (zoom is app state, absent from PTY
    /// bytes).
    fn apply_zoom(
        session: &mut DualPlaneSession,
        columns: NonZeroU32,
        rows: NonZeroU32,
        cell_px: i64,
        dpi_milli: NonZeroU32,
        at: Instant,
    ) {
        session.set_cell_height_subpixels(NonZeroI64::new(cell_px * SUBPIXELS_PER_PX).unwrap());
        session.set_cell_width_subpixels(
            NonZeroI64::new((cell_px / 2).max(1) * SUBPIXELS_PER_PX).unwrap(),
        );
        session.set_ascii_baseline_subpixels(
            NonZeroI64::new((cell_px - 3).max(1) * SUBPIXELS_PER_PX).unwrap(),
        );
        session.resize_at(columns, rows, at).unwrap();
        session.mark_pty_resize_requested_at(columns, rows, at);
        let theme_rev = session.layout_key().theme_rev;
        session.set_layout_key(LayoutKey {
            width_cells: columns,
            dpi_milli,
            font_rev: 1,
            theme_rev,
        });
    }

    fn zoom_frame_is_all_rendered(
        session: &mut DualPlaneSession,
        projection: &mut ViewportProjection,
        expected_blocks: usize,
    ) -> Vec<u32> {
        session.refresh_projection(projection);
        let frame = session.viewport_frame(projection).unwrap();
        assert_eq!(
            crate::observe_formula_frame(&frame).state,
            crate::FormulaFrameState::Rendered,
            "a zoom must never expose a proven live formula's source"
        );
        assert_eq!(frame.math_blocks.len(), expected_blocks);
        assert!(
            frame
                .math_blocks
                .iter()
                .all(|block| block.display == MathBlockDisplay::Rendered)
        );
        // Atomicity (the stray-raster fragment): every row a rendered block owns is cleared of its
        // source text, so a scaled stale raster never coexists with exposed `$$` source.
        assert!(
            !frame.cells.iter().any(|cell| cell.text == "$"),
            "no source delimiter may remain visible while the block renders"
        );
        frame
            .math_blocks
            .iter()
            .map(|block| block.artifact.render_scale_milli)
            .collect()
    }

    #[test]
    fn zoom_out_holds_live_formula_as_scaled_stale_instead_of_flashing_to_source() {
        // Real-machine regression (2026-07-24): shrinking the font (zoom out) dropped proven Codex
        // formulas to their `$$` source and left them there. Root cause: a DPI change demotes the
        // live raster to a stale artifact scaled by the DPI ratio, and the live viewport hard-rejected
        // any non-readable-scale raster on primary -> source fallback for the whole async relayout
        // window. The scaled stale raster must instead stay pinned until the fresh relayout lands.
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(24));
        session
            .feed_at(
                b"intro line here\r\n$$x$$\r\nmid line\r\n$$y$$\r\nbarrier",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            2
        );
        let mut projection = session.new_projection(session.layout_key());
        zoom_frame_is_all_rendered(&mut session, &mut projection, 2);

        // Zoom out: smaller cells, more cols/rows, DPI 1000 -> 800.
        let t = start + Duration::from_millis(210);
        apply_zoom(&mut session, nz(52), nz(32), 14, nz(800), t);
        // The window (epoch open) shows the held stale raster scaled to 80%, not source.
        let scales = zoom_frame_is_all_rendered(&mut session, &mut projection, 2);
        assert!(
            scales.iter().all(|scale| *scale == 800),
            "the held raster is the old layout's pixels scaled by the DPI ratio: {scales:?}"
        );

        // Codex reprints its transcript after the resize: still held, still no source.
        session
            .feed_at(b"\x1b[H\x1b[2J\x1b[3J", t + Duration::from_millis(10))
            .unwrap();
        session
            .feed_at(
                b"intro line here\r\n$$x$$\r\nmid line\r\n$$y$$\r\nbarrier",
                t + Duration::from_millis(20),
            )
            .unwrap();
        zoom_frame_is_all_rendered(&mut session, &mut projection, 2);

        // Output quiesces before the async relayout lands: preservation must survive the epoch close.
        let finish_at = t + Duration::from_millis(300);
        assert!(session.finish_resize_if_quiescent(finish_at).unwrap());
        assert!(
            session.has_pending_resize_relayout(),
            "the fresh relayout has not landed, so the stale raster must remain pinned"
        );
        zoom_frame_is_all_rendered(&mut session, &mut projection, 2);

        // A re-detection pass scheduled during the hold must not tear the held record down.
        session.advance_live_stability(finish_at + LIVE_MATH_STABLE_INTERVAL);
        zoom_frame_is_all_rendered(&mut session, &mut projection, 2);

        // The fresh relayout lands at the new DPI's native scale and replaces the stale raster.
        complete_detected_live_tasks(&mut session, synthetic_raster(52, 40));
        let scales = zoom_frame_is_all_rendered(&mut session, &mut projection, 2);
        assert!(
            scales.iter().all(|scale| *scale == 1000),
            "the fresh raster renders at native readable scale: {scales:?}"
        );
    }

    #[test]
    fn zoom_quiescence_does_not_release_stale_formula_before_a_late_clean_reprint() {
        // Real-machine zoom-end regression (2026-07-28): the resize-side clear can arrive while the
        // resize epoch is active, but Codex's clean transcript reprint can land only after the
        // quiescence edge. The old-DPI raster is already demoted to stale and its fresh bt-math
        // relayout is still off-thread. Releasing the resize preservation queue at quiescence drops
        // the only exact-source witness; the late, byte-identical reprint then exposes source until
        // the delayed relayout completes.
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(24));
        let transcript = b"intro line here\r\n$$x$$\r\nmid line\r\n$$y$$\r\nbarrier";
        session.feed_at(transcript, start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            2
        );
        let mut projection = session.new_projection(session.layout_key());
        zoom_frame_is_all_rendered(&mut session, &mut projection, 2);

        let zoom_at = start + Duration::from_millis(210);
        apply_zoom(&mut session, nz(52), nz(32), 14, nz(800), zoom_at);
        zoom_frame_is_all_rendered(&mut session, &mut projection, 2);
        let mut inflight = Vec::new();
        while let Some(task) = session.take_live_worker_task() {
            inflight.push(task);
        }
        assert_eq!(
            inflight.len(),
            2,
            "both fresh-DPI relayouts are now off-thread"
        );

        // The clear half lands during the resize transaction. Its records cannot re-anchor yet
        // because the replacement source is deliberately late, so they wait off-band while their
        // fresh worker completions remain queued.
        session
            .feed_at(b"\x1b[H\x1b[2J\x1b[3J", zoom_at + Duration::from_millis(10))
            .unwrap();
        assert!(
            session.has_pending_resize_relayout(),
            "the fresh-DPI worker result is still delayed"
        );
        assert!(
            !session.offscreen_decorations.is_empty(),
            "the stale raster must wait off-band for the clean reprint"
        );

        // Resize silence wins the race. This edge must not discard a stale-pending exact-source
        // witness merely because the corresponding reprint has not arrived yet.
        let finish_at = zoom_at + Duration::from_millis(300);
        assert!(session.finish_resize_if_quiescent(finish_at).unwrap());
        assert!(
            session.has_pending_resize_relayout(),
            "quiescence is not the fresh-raster completion event"
        );

        // Codex now emits the exact same transcript. Before the fix this frame is Source: the
        // quiescence edge cleared the off-band record, so only a new detection/render could recover.
        session
            .feed_at(transcript, finish_at + Duration::from_millis(10))
            .unwrap();
        let scales = zoom_frame_is_all_rendered(&mut session, &mut projection, 2);
        assert!(
            scales.iter().all(|scale| *scale == 800),
            "the old raster remains scaled stale until the delayed fresh completion: {scales:?}"
        );
        assert_eq!(
            inflight.len(),
            2,
            "both fresh completions remain deliberately delayed across the asserted gap"
        );
    }

    #[test]
    fn late_post_quiescence_zoom_reprint_holds_its_incomplete_clear_frame() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(24));
        session
            .feed_at(b"intro\r\n$$x$$\r\nbarrier", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        zoom_frame_is_all_rendered(&mut session, &mut projection, 1);

        let zoom_at = start + Duration::from_millis(210);
        apply_zoom(&mut session, nz(52), nz(32), 14, nz(800), zoom_at);
        let delayed_relayout = session.take_live_worker_task().unwrap();
        assert!(
            session
                .finish_resize_if_quiescent(zoom_at + Duration::from_millis(300))
                .unwrap()
        );

        // The late clean reprint begins with its clear and only a prefix of the formula row. The
        // exact-source record is now off-band, so this is the diagnosed presentation gap: the grid
        // contains an incomplete source row and cannot paint the held raster yet.
        session
            .feed_at(
                b"\x1b[2J\x1b[H\x1b[3Jintro\r\n$",
                zoom_at + Duration::from_millis(310),
            )
            .unwrap();
        session.refresh_projection(&mut projection);
        let gap = session.viewport_frame(&mut projection).unwrap();
        assert!(gap.math_blocks.is_empty());
        assert!(
            projection.presentation_hold(),
            "the incomplete post-quiescence reprint frame must be held at presentation"
        );
        assert!(
            !projection.review_hold(),
            "this bottom-follow hold is decoration-owned, not review displacement"
        );

        // The next chunk completes the exact source. Re-anchoring is deterministic and does not
        // wait for the deliberately delayed fresh-DPI worker completion.
        session
            .feed_at(b"$x$$\r\nbarrier", zoom_at + Duration::from_millis(324))
            .unwrap();
        zoom_frame_is_all_rendered(&mut session, &mut projection, 1);
        assert!(!projection.presentation_hold());
        drop(delayed_relayout);
    }

    #[test]
    fn frozen_reprint_owner_retires_a_zoom_hold_after_the_formula_leaves_live_grid() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session.feed_at(b"$$x$$\r\nbarrier", start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        zoom_frame_is_all_rendered(&mut session, &mut projection, 1);

        // Zoom-in reduces the live grid. Keep the fresh-DPI live result deliberately in flight so
        // the proven old raster is the exact stale-pending occurrence that presentation holds.
        let zoom_at = start + Duration::from_millis(210);
        apply_zoom(&mut session, nz(40), nz(6), 18, nz(1250), zoom_at);
        let delayed_live_relayout = session.take_live_worker_task().unwrap();
        assert!(
            session
                .finish_resize_if_quiescent(zoom_at + Duration::from_millis(300))
                .unwrap()
        );

        // The delayed clean reprint rewrites the whole transcript, then enough ordinary rows to
        // freeze the formula out of the six-row live grid. Its exact live match can never return.
        // Until frozen detection claims the newly finalized source, the incomplete ownership
        // transfer is still a real presentation gap and must remain held.
        session
            .feed_at(
                b"\x1b[2J\x1b[H\x1b[3J$$x$$\r\none\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven\r\neight",
                zoom_at + Duration::from_millis(310),
            )
            .unwrap();
        let _ = present(&mut session, &mut projection);
        assert!(projection.presentation_hold());
        assert!(
            session
                .document()
                .entries()
                .values()
                .any(|entry| entry.line.text == "$$x$$"),
            "the reprinted exact source must now be frozen history"
        );
        assert!(
            (0..session.terminal.dimensions().1.get())
                .filter_map(|row| session.terminal.visible_row(row))
                .all(|row| captured_row_text_and_boundaries(&row).0 != "$$x$$"),
            "the formula must no longer have any exact live-grid anchor"
        );

        let mut frozen_task = session
            .take_worker_task()
            .expect("frozen detection must claim the reprinted formula");
        assert!(resolve_detection_task(&mut frozen_task));
        assert!(session.complete_worker_result(frozen_task, Ok(synthetic_raster(40, 40))));
        let _ = present(&mut session, &mut projection);
        assert!(
            !projection.presentation_hold(),
            "the new frozen exact-source owner deterministically replaces the off-band occurrence"
        );

        for index in 0..4 {
            session
                .feed_at(
                    format!("tail-{index}\r\n").as_bytes(),
                    zoom_at + Duration::from_millis(320 + index),
                )
                .unwrap();
            let _ = present(&mut session, &mut projection);
            assert!(
                !projection.presentation_hold(),
                "ordinary streaming output must remain publishable after frozen ownership transfers"
            );
        }
        drop(delayed_live_relayout);
    }

    #[test]
    fn late_zoom_reprint_hold_yields_to_user_input_and_a_screen_lifecycle_boundary() {
        for release_to_alternate in [false, true] {
            let start = Instant::now();
            let mut session = DualPlaneSession::new(nz(40), nz(24));
            session
                .feed_at(b"intro\r\n$$x$$\r\nbarrier", start)
                .unwrap();
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
            assert_eq!(
                complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
                1
            );
            let mut projection = session.new_projection(session.layout_key());
            zoom_frame_is_all_rendered(&mut session, &mut projection, 1);

            let zoom_at = start + Duration::from_millis(210);
            apply_zoom(&mut session, nz(52), nz(32), 14, nz(800), zoom_at);
            let delayed_relayout = session.take_live_worker_task().unwrap();
            assert!(
                session
                    .finish_resize_if_quiescent(zoom_at + Duration::from_millis(300))
                    .unwrap()
            );
            session
                .feed_at(
                    b"\x1b[2J\x1b[H\x1b[3Jintro\r\n$",
                    zoom_at + Duration::from_millis(310),
                )
                .unwrap();
            let _ = present(&mut session, &mut projection);
            assert!(projection.presentation_hold());

            if release_to_alternate {
                session
                    .feed_at(b"\x1b[?1049h", zoom_at + Duration::from_millis(311))
                    .unwrap();
                let _ = present(&mut session, &mut projection);
                assert!(
                    !projection.presentation_hold(),
                    "alternate-screen entry is a hard presentation boundary"
                );
            } else {
                assert!(session.release_presentation_hold_for_user_input());
                let _ = present(&mut session, &mut projection);
                assert!(
                    !projection.presentation_hold(),
                    "explicit user takeover releases without a timer"
                );
                assert!(
                    !session.release_presentation_hold_for_user_input(),
                    "release is idempotent once the occurrence set is cleared"
                );
            }
            drop(delayed_relayout);
        }
    }

    #[test]
    fn ordinary_cls_and_alternate_repaint_never_enter_the_dpi_exact_source_hold() {
        let start = Instant::now();

        let mut primary = DualPlaneSession::new(nz(40), nz(12));
        primary
            .feed_at(b"intro\r\n$$x$$\r\nbarrier", start)
            .unwrap();
        primary.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut primary, synthetic_raster(40, 40)),
            1
        );
        let mut primary_projection = primary.new_projection(primary.layout_key());
        primary
            .feed_at(b"\x1b[2J\x1b[H\x1b[3J", start + Duration::from_millis(10))
            .unwrap();
        let cleared = present(&mut primary, &mut primary_projection);
        assert!(cleared.math_blocks.is_empty());
        assert!(
            !primary_projection.presentation_hold(),
            "a real cls has no stale-pending DPI occurrence and must publish"
        );

        let mut alternate = DualPlaneSession::new(nz(40), nz(12));
        alternate
            .feed_at(b"\x1b[?1049hintro\r\n$$x$$\r\nbarrier", start)
            .unwrap();
        alternate.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut alternate, synthetic_raster(40, 40)),
            1
        );
        let mut alternate_projection = alternate.new_projection(alternate.layout_key());
        alternate.set_layout_key(LayoutKey {
            dpi_milli: nz(800),
            ..alternate.layout_key()
        });
        alternate
            .feed_at(
                b"\x1b[2J\x1b[H\x1b[3Jintro\r\n$",
                start + Duration::from_millis(10),
            )
            .unwrap();
        let _ = present(&mut alternate, &mut alternate_projection);
        assert!(
            !alternate_projection.presentation_hold(),
            "alternate repaint semantics remain outside the primary-only hold"
        );
    }

    #[test]
    fn zoom_in_holds_live_formula_across_the_relayout_window() {
        // The mirror direction: growing the font (zoom in) scales the held raster UP by the DPI ratio.
        // In a normally sized window the visible-text floor has room for the enlarged boxes, so the
        // block stays rendered the whole window rather than flashing to source.
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(52), nz(30));
        session.set_cell_height_subpixels(NonZeroI64::new(14 * SUBPIXELS_PER_PX).unwrap());
        session.set_cell_width_subpixels(NonZeroI64::new(7 * SUBPIXELS_PER_PX).unwrap());
        session.set_ascii_baseline_subpixels(NonZeroI64::new(11 * SUBPIXELS_PER_PX).unwrap());
        session.set_layout_key(LayoutKey {
            width_cells: nz(52),
            dpi_milli: nz(800),
            font_rev: 1,
            theme_rev: session.layout_key().theme_rev,
        });
        session
            .feed_at(
                b"intro line here\r\n$$x$$\r\nmid line\r\n$$y$$\r\nbarrier",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(52, 40)),
            2
        );
        let mut projection = session.new_projection(session.layout_key());
        zoom_frame_is_all_rendered(&mut session, &mut projection, 2);

        // Zoom in: bigger cells, fewer cols/rows, DPI 800 -> 1000 (scale 1250 on the held raster).
        let t = start + Duration::from_millis(210);
        apply_zoom(&mut session, nz(40), nz(24), 18, nz(1000), t);
        let scales = zoom_frame_is_all_rendered(&mut session, &mut projection, 2);
        assert!(
            scales.iter().all(|scale| *scale == 1250),
            "the held raster scales up by the DPI ratio during zoom in: {scales:?}"
        );

        session
            .feed_at(b"\x1b[H\x1b[2J\x1b[3J", t + Duration::from_millis(10))
            .unwrap();
        session
            .feed_at(
                b"intro line here\r\n$$x$$\r\nmid line\r\n$$y$$\r\nbarrier",
                t + Duration::from_millis(20),
            )
            .unwrap();
        zoom_frame_is_all_rendered(&mut session, &mut projection, 2);

        let finish_at = t + Duration::from_millis(300);
        assert!(session.finish_resize_if_quiescent(finish_at).unwrap());
        zoom_frame_is_all_rendered(&mut session, &mut projection, 2);

        session.advance_live_stability(finish_at + LIVE_MATH_STABLE_INTERVAL);
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40));
        let scales = zoom_frame_is_all_rendered(&mut session, &mut projection, 2);
        assert!(scales.iter().all(|scale| *scale == 1000), "{scales:?}");
    }

    /// Resolve a frame the way the app does — `refresh_projection` then `viewport_frame`. The scroll
    /// state and `review_hold` are committed inside the frame builder, so both must run before those
    /// are read.
    fn present(
        session: &mut DualPlaneSession,
        projection: &mut ViewportProjection,
    ) -> ViewportFrame {
        session.refresh_projection(projection);
        session.viewport_frame(projection).unwrap()
    }

    #[test]
    fn zoom_reprojects_the_formula_band_at_the_new_cell_height_bottom_follow() {
        // Bottom-follow regression (2026-07-24): after a zoom the projection kept its construction
        // cell height, so the math band was placed at the old row pitch while text rendered at the
        // new one — the block appeared to jump. The projection must track the session cell height so
        // the band lands at the new pitch, and the view stays anchored at the bottom the whole time.
        // The invariant: a zoom applied in place must produce the exact geometry of a projection
        // rebuilt fresh at the new cell height (before the fix the in-place band kept the old pitch).
        for zoom_out in [true, false] {
            let start = Instant::now();
            let (start_px, start_rows, start_dpi) = if zoom_out {
                (18_i64, nz(24), nz(1000))
            } else {
                (14, nz(32), nz(800))
            };
            let mut session = DualPlaneSession::with_cell_height(
                nz(40),
                start_rows,
                NonZeroI64::new(start_px * SUBPIXELS_PER_PX).unwrap(),
            );
            session.set_layout_key(LayoutKey {
                width_cells: nz(40),
                dpi_milli: start_dpi,
                font_rev: 1,
                theme_rev: session.layout_key().theme_rev,
            });
            session
                .feed_at(
                    b"intro line here\r\n$$x$$\r\nmid line\r\n$$y$$\r\nbarrier",
                    start,
                )
                .unwrap();
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
            assert_eq!(
                complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
                2
            );
            let mut projection = session.new_projection(session.layout_key());
            let before = present(&mut session, &mut projection);
            assert_eq!(
                before.scroll_offset_rows, 0,
                "the view begins at the bottom"
            );
            let before_tops: Vec<i64> =
                before.math_blocks.iter().map(|b| b.top_subpixels).collect();

            let t = start + Duration::from_millis(210);
            let (end_px, end_rows, end_dpi) = if zoom_out {
                (14_i64, nz(32), nz(800))
            } else {
                (18, nz(24), nz(1000))
            };
            apply_zoom(&mut session, nz(40), end_rows, end_px, end_dpi, t);

            let after = present(&mut session, &mut projection);
            assert!(
                after
                    .math_blocks
                    .iter()
                    .all(|b| b.display == MathBlockDisplay::Rendered),
                "the block stays rendered across the zoom",
            );
            assert_eq!(
                projection.cell_height_subpixels().get(),
                end_px * SUBPIXELS_PER_PX,
                "the projection tracks the session's new cell height",
            );
            assert_eq!(
                after.scroll_offset_rows, 0,
                "bottom-follow is preserved — no jump to an offset",
            );
            let after_tops: Vec<i64> = after.math_blocks.iter().map(|b| b.top_subpixels).collect();
            assert_ne!(
                after_tops, before_tops,
                "the band geometry must move to the new cell pitch, not stay stale",
            );

            // A projection rebuilt fresh at the new height is the reference geometry. The in-place
            // zoom must match it exactly — proof that the new height fully propagated with no stale
            // subpixel residue in live_row_prefix or the band tops.
            let mut fresh = session.new_projection(session.layout_key());
            let reference = present(&mut session, &mut fresh);
            let reference_tops: Vec<i64> = reference
                .math_blocks
                .iter()
                .map(|b| b.top_subpixels)
                .collect();
            assert_eq!(
                after_tops, reference_tops,
                "in-place zoom geometry equals a fresh rebuild at the new cell height",
            );
        }
    }

    #[test]
    fn zoom_preserves_the_review_offset_and_holds_across_the_reprint() {
        // Review regression (2026-07-24): the row-level review displacement already survived a
        // resize reflow (33fb866), but a zoom additionally remeasures the cell height. The offset
        // must be preserved and restored across the transcript rewrite exactly as a resize, and the
        // projection must track the new cell height throughout. Both directions.
        for zoom_out in [true, false] {
            let start = Instant::now();
            let (start_px, start_rows, start_dpi, end_px, end_rows, end_dpi) = if zoom_out {
                (18_i64, nz(10), nz(1000), 14_i64, nz(13), nz(800))
            } else {
                (14, nz(13), nz(800), 18, nz(10), nz(1000))
            };
            let mut session = DualPlaneSession::with_cell_height(
                nz(40),
                start_rows,
                NonZeroI64::new(start_px * SUBPIXELS_PER_PX).unwrap(),
            );
            session.set_layout_key(LayoutKey {
                width_cells: nz(40),
                dpi_milli: start_dpi,
                font_rev: 1,
                theme_rev: session.layout_key().theme_rev,
            });
            let mut lines = Vec::new();
            for i in 0..60 {
                lines.extend_from_slice(format!("line-{i:03}\r\n").as_bytes());
            }
            session.feed_at(&lines, start).unwrap();
            let mut projection = session.new_projection(session.layout_key());

            // Enter review 20 rows up.
            let _ = present(&mut session, &mut projection);
            projection.scroll_by_rows(20);
            let frame = present(&mut session, &mut projection);
            assert_eq!(frame.scroll_offset_rows, 20, "reviewing 20 rows up");

            // Zoom remeasures the cell height and resizes the grid; the anchored content is still
            // present, so the review holds its offset and the projection adopts the new height.
            let t = start + Duration::from_millis(210);
            apply_zoom(&mut session, nz(40), end_rows, end_px, end_dpi, t);
            let frame = present(&mut session, &mut projection);
            assert_eq!(
                projection.cell_height_subpixels().get(),
                end_px * SUBPIXELS_PER_PX,
                "the projection tracks the new cell height",
            );
            assert_eq!(
                frame.scroll_offset_rows, 20,
                "the review offset survives the zoom-driven metric change",
            );
            assert!(!projection.review_hold());

            // Codex clears scrollback and reprints: the anchor vanishes, presentation must hold.
            session
                .feed_at(b"\x1b[H\x1b[2J\x1b[3J", t + Duration::from_millis(10))
                .unwrap();
            let _ = present(&mut session, &mut projection);
            assert!(
                projection.review_hold(),
                "the cleared anchor engages the frame hold instead of flashing to bottom",
            );
            session
                .feed_at(&lines, t + Duration::from_millis(20))
                .unwrap();
            let _ = present(&mut session, &mut projection);
            assert!(
                projection.review_hold(),
                "still held while the reprint stages"
            );

            // Quiescence closes the transaction: the offset re-anchors and the hold releases.
            assert!(
                session
                    .finish_resize_if_quiescent(t + Duration::from_millis(300))
                    .unwrap()
            );
            let frame = present(&mut session, &mut projection);
            assert!(!projection.review_hold(), "the hold releases at re-anchor");
            assert_eq!(
                frame.scroll_offset_rows, 20,
                "the review returns to its original position after the zoom reprint",
            );
            assert_eq!(
                projection.cell_height_subpixels().get(),
                end_px * SUBPIXELS_PER_PX,
            );
        }
    }

    #[test]
    fn zoom_review_takeover_releases_and_real_cls_never_holds() {
        // The zoom hold must yield to the user and must not fire for a genuine clear.
        let start = Instant::now();
        let mut lines = Vec::new();
        for i in 0..60 {
            lines.extend_from_slice(format!("line-{i:03}\r\n").as_bytes());
        }

        // Takeover: while held across a zoom reprint, an explicit scroll supersedes the hold.
        let mut session = DualPlaneSession::new(nz(40), nz(10));
        session.feed_at(&lines, start).unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let _ = present(&mut session, &mut projection);
        projection.scroll_by_rows(20);
        let _ = present(&mut session, &mut projection);
        let t = start + Duration::from_millis(210);
        apply_zoom(&mut session, nz(40), nz(13), 14, nz(800), t);
        session
            .feed_at(b"\x1b[H\x1b[2J\x1b[3J", t + Duration::from_millis(10))
            .unwrap();
        let _ = present(&mut session, &mut projection);
        assert!(projection.review_hold(), "held before takeover");
        projection.scroll_by_rows(-5); // user scrolls: supersedes the preserved displacement
        let _ = present(&mut session, &mut projection);
        assert!(
            !projection.review_hold(),
            "an explicit scroll releases the hold immediately",
        );

        // Real cls (no resize transaction) must never hold — it snaps to the empty bottom.
        let mut session = DualPlaneSession::new(nz(40), nz(10));
        session.feed_at(&lines, start).unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let _ = present(&mut session, &mut projection);
        projection.scroll_by_rows(20);
        let _ = present(&mut session, &mut projection);
        session
            .feed_at(b"\x1b[H\x1b[2J\x1b[3J", start + Duration::from_millis(10))
            .unwrap();
        let frame = present(&mut session, &mut projection);
        assert!(
            !projection.review_hold(),
            "a user clear opens no resize epoch, so the hold never engages",
        );
        assert_eq!(frame.scroll_offset_rows, 0, "a real cls snaps to bottom");
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
        hide_cursor(&mut session, start);

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
        hide_cursor(&mut session, start);
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
            hide_cursor(&mut session, start);
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
            hide_cursor(&mut session, start);
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
        hide_cursor(&mut session, start);
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
            hide_cursor(&mut long, start);
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
            hide_cursor(&mut tombstoned, start);
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
        hide_cursor(&mut session, start + Duration::from_millis(10));
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
            hide_cursor(&mut session, start);
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
        hide_cursor(&mut session, start + Duration::from_millis(220));
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
    fn primary_live_band_owns_exact_source_and_preserves_adjacent_blank_rows() {
        let start = Instant::now();
        let mut primary = DualPlaneSession::new(nz(40), nz(12));
        primary.feed_at(b"$$x$$\r\n\r\nbarrier", start).unwrap();
        primary.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut primary, synthetic_raster(40, 40)),
            1
        );
        let record = primary.live_decorations.get(&0).unwrap();
        assert_eq!((record.band_start_row, record.band_end_row), (0, 0));
        let mut projection = primary.new_projection(primary.layout_key());
        let exact = primary.viewport_frame(&mut projection).unwrap();
        assert_eq!(exact.math_blocks.len(), 1);
        assert_eq!(
            exact.math_blocks[0].clip_height_subpixels,
            math_presentation_height_subpixels(
                40 * SUBPIXELS_PER_PX,
                default_math_padding_subpixels(),
            )
        );
        assert_eq!(exact.math_blocks[0].artifact.render_scale_milli, 1000);
        let separator = exact
            .row_map
            .iter()
            .find(|row| row.live_grid_row == Some(1))
            .unwrap();
        assert_eq!(
            separator.height_subpixels,
            SPIKE_CELL_HEIGHT_SUBPIXELS.get(),
            "the blank separator keeps its own terminal row instead of being absorbed by math"
        );

        primary
            .feed_at(
                b"\x1b[2;1Hseparator-row-now-used",
                start + Duration::from_millis(210),
            )
            .unwrap();
        let preserved = primary.viewport_frame(&mut projection).unwrap();
        assert_eq!(preserved.math_blocks.len(), 1);
        let separator_frame_row = preserved
            .row_map
            .iter()
            .position(|row| row.live_grid_row == Some(1))
            .unwrap();
        let columns = preserved.columns.get() as usize;
        let separator_text = preserved.cells
            [separator_frame_row * columns..(separator_frame_row + 1) * columns]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>();
        assert!(
            separator_text.contains("separator-row-now-used"),
            "a neighbouring non-source row remains native terminal content"
        );

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
        assert!(inflight.complete_live_worker_result(task, Ok(synthetic_raster(40, 40))));
        assert_eq!(
            inflight
                .live_decorations
                .get(&0)
                .map(|record| { (record.band_start_row, record.band_end_row) }),
            Some((0, 0))
        );

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
            0,
            "every blank separator remains outside the detector-proven source band"
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
        assert_eq!((record.band_start_row, record.band_end_row), (1, 1));
        upward
            .feed_at(
                b"\x1b[1;1Hupper-row-now-used",
                start + Duration::from_millis(210),
            )
            .unwrap();
        assert_eq!(upward.live_decorations.len(), 1);
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
        hide_cursor(&mut session, start + Duration::from_millis(10));
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
    fn live_visible_text_floor_is_per_block_and_keeps_every_bounded_occurrence() {
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
        assert_eq!(
            selected,
            vec![vec!["old".to_owned(), "middle".to_owned(), "new".to_owned()]; 3]
        );
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

    /// `resize-endflash.vt` frames 2836/3411: a display block whose opener has already crossed the
    /// top edge has no complete source left in the live grid. A same-DPI width resize nevertheless
    /// leaves the remaining proven rows byte/cell-exact, first across local reflow and then across
    /// ConPTY reconciliation. Both deterministic boundaries must keep the clipped owner; falling
    /// back to whole-source matching at either one exposes the aligned body for one frame.
    #[test]
    fn primary_resize_reconcile_keeps_partially_clipped_outer_owner() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(60), nz(20));
        session
            .feed_at(
                b"$$\r\n\\begin{aligned}\r\nx&=1\\\\\r\ny&=2\\\\\r\nz&=3\\\\\r\nw&=4\r\n\\end{aligned}\r\n$$\r\n\r\n\\begin{aligned}\r\nx&=1\\\\\r\ny&=2\\\\\r\nz&=3\\\\\r\nw&=4\r\n\\end{aligned}\r\nbelow",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(60, 64)),
            2
        );
        let source = session
            .live_decorations
            .values()
            .find(|record| record.span.original_source.trim_start().starts_with("$$"))
            .map(|record| record.span.original_source.clone())
            .unwrap();
        assert!(
            resize_transition_side_occurrences(session.live_decorations.values())
                .iter()
                .any(|occurrence| {
                    session.live_decorations.values().any(|record| {
                        record.identity.occurrence_id == *occurrence
                            && record.span.original_source == source
                    })
                }),
            "the fixture must contain a render-equivalent outer/narrow owner pair"
        );

        // Move only the opener above the live grid. The remaining seven rows are the exact proof;
        // a complete-grid source search is deliberately impossible from this point onward.
        session
            .feed_at(
                b"\x1b[?2026h\x1b[1;20r\x1b[1S\x1b[r\x1b[?2026l",
                start + Duration::from_millis(200),
            )
            .unwrap();
        let clipped = session
            .live_decorations
            .values()
            .find(|record| record.span.original_source == source)
            .expect("the top-scroll projection must retain the outer owner");
        assert_eq!(clipped.clipped_top_rows, 1);

        let resized_at = start + Duration::from_millis(210);
        session.resize_at(nz(61), nz(20), resized_at).unwrap();
        session.mark_pty_resize_requested_at(nz(61), nz(20), resized_at);

        let retained = session
            .live_decorations
            .values()
            .find(|record| record.span.original_source == source)
            .expect("resize and reconcile must preserve the uniquely projected clipped owner");
        assert!(retained.stale_artifact.is_some());
        assert!(
            projected_live_artifact(retained, session.layout_key, 0).is_some(),
            "the retained stale raster must remain paintable while relayout is pending"
        );
    }

    #[test]
    fn primary_resize_keeps_formula_rendered_through_the_post_quiescence_repaint() {
        // Regression for the residual resize-completes flash. 002acc7 preserves formulas *during*
        // the drag, but its preservation ended the instant the epoch quiesced. A resize demotes the
        // proven raster to a stale artifact and queues an async relayout (bt-math is off-thread), so
        // for one or more stability intervals after quiescence the block renders from its stale
        // raster. Codex repaints its whole transcript once the resize settles, and that repaint —
        // whose rebuilt-grid fingerprints no longer match — used to drop the still-stale record to
        // source until re-detection caught up: the whole formula flashed back to `$$x$$`. The stale
        // artifact must instead survive every repaint until its fresh relayout replaces it in place.
        let raster40 = synthetic_raster(40, 40);
        let raster41 = synthetic_raster(41, 40);
        let mut oracle = crate::FormulaFlashOracle::default();

        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session
            .feed_at(b"intro\r\n$$x$$\r\nbarrier", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, raster40.clone()),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let before = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            oracle.observe(&before).state,
            crate::FormulaFrameState::Rendered
        );

        // The window resize reflows the grid and opens the transaction; Codex clears and reprints
        // its transcript, both staged inside the transaction (holding the epoch open).
        let t = start + Duration::from_millis(210);
        session.resize_at(nz(41), nz(12), t).unwrap();
        session.mark_pty_resize_requested_at(nz(41), nz(12), t);
        session
            .feed_at(b"\x1b[2J\x1b[3J\x1b[H", t + Duration::from_millis(10))
            .unwrap();
        session
            .feed_at(b"intro\r\n$$x$$\r\nbarrier", t + Duration::from_millis(20))
            .unwrap();
        session.refresh_projection(&mut projection);
        let staged = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            oracle.observe(&staged).state,
            crate::FormulaFrameState::Rendered,
            "the block stays rendered (as a stale artifact) while the transaction is open"
        );

        // Output goes quiet: the transaction quiesces. The relayout queued by the reflow has NOT
        // completed yet, so the block is still rendering from its stale raster here.
        let finish_at = t + Duration::from_millis(280);
        assert!(session.finish_resize_if_quiescent(finish_at).unwrap());
        assert!(!session.resize_epoch.is_active());
        assert!(
            session.has_pending_resize_relayout(),
            "the resize relayout has not landed, so preservation must remain engaged past the epoch"
        );
        session.refresh_projection(&mut projection);
        let quiesced = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            oracle.observe(&quiesced).state,
            crate::FormulaFrameState::Rendered
        );

        // The post-resize repaint — the exact frame that used to flash. It damages the formula rows
        // while the epoch is already closed and the fresh raster is still pending.
        session
            .feed_at(
                b"\x1b[H\x1b[2Jintro\r\n$$x$$\r\nbarrier",
                finish_at + Duration::from_millis(5),
            )
            .unwrap();
        session.refresh_projection(&mut projection);
        let after_repaint = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            oracle.observe(&after_repaint).state,
            crate::FormulaFrameState::Rendered,
            "the post-quiescence repaint must not drop the stale formula to source"
        );

        // The reflowed grid settles; fresh detection reruns and its relayout lands, replacing the
        // stale raster in place. Tolerate the odd rejected stale-generation task from the reflow.
        session.advance_live_stability(
            finish_at + Duration::from_millis(5) + LIVE_MATH_STABLE_INTERVAL,
        );
        while let Some(mut task) = session.take_live_worker_task() {
            if resolve_live_detection_task(&mut task) {
                session.complete_live_worker_result(task, Ok(raster41.clone()));
            } else {
                session.complete_live_worker_result(task, Err(MathRenderError::NotDetected));
            }
        }
        session.refresh_projection(&mut projection);
        let settled = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            oracle.observe(&settled).state,
            crate::FormulaFrameState::Rendered
        );
        assert!(
            session
                .live_decorations
                .values()
                .all(|record| record.artifact.is_some() && record.stale_artifact.is_none()),
            "the fresh relayout must replace the stale raster in place, retiring the stale artifact"
        );
        assert!(
            !session.has_pending_resize_relayout(),
            "with the fresh raster installed, the aftermath preservation window closes"
        );

        // Nothing in the whole sequence exposed the formula's source.
        assert!(
            !oracle.flash_detected(),
            "no frame flashed the formula back to source: {:?}",
            oracle.flashed_sources()
        );
    }

    /// Batch ③ backed case: a live formula that is both detected and displayed is never reported
    /// `HeldUnbacked` — the ledger Owns its exact source, so the hold is honestly backed. This also
    /// pins that the report is pure observation: producing it leaves the frame byte-identical.
    #[test]
    fn a_rendered_live_formula_is_backed_not_held_unbacked() {
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
        assert_eq!(before.math_blocks[0].display, MathBlockDisplay::Rendered);

        // The block is detected AND painted: the hold is backed, so nothing is HeldUnbacked.
        assert!(
            session.held_unbacked_records().is_empty(),
            "a detected, displayed formula is backed"
        );

        // Observation only: the report does not perturb display.
        session.refresh_projection(&mut projection);
        let after = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            crate::observe_formula_frame(&after).state,
            crate::FormulaFrameState::Rendered
        );
        assert_eq!(before.math_blocks.len(), after.math_blocks.len());
    }

    /// Batch ③ unbacked case (the audit's masking mechanism): a resize opens the preservation window,
    /// then the reflow reprints the transcript with a stray unbalanced `$$` opener above the block —
    /// the exact odd-parity poison the three audits name. The block's source `$$x$$` is still literally
    /// on the grid, so the hold re-anchors and keeps rendering (display is UNCHANGED, the hold is
    /// honest about the pixels), but the detector's global toggle is now off-phase and no longer PAIRS
    /// it into a block. That divergence — a hold showing a formula the settled detector no longer
    /// accounts — is reported exactly as `HeldUnbacked`, the observable the flash oracle cannot see.
    #[test]
    fn a_hold_over_a_parity_poisoned_block_is_reported_held_unbacked_without_changing_display() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        // A multi-line block whose opener and closer sit on separate rows — the shape a stray `$$`
        // can role-shift (a single-line `$$x$$` always wins self-contained detection and cannot be
        // desynced, so the masking mechanism needs the delimiters split across rows).
        session
            .feed_at(b"$$\r\ny=1\r\n$$\r\nbarrier", start)
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        assert!(session.held_unbacked_records().is_empty());
        let held_source = "$$\ny=1\n$$".to_owned();

        // A resize opens the preservation window; the reflow clears scrollback and reprints the same
        // block but injects a stray unbalanced `$$` opener above it — the exact odd-parity poison the
        // three audits name. Now the stray `$$` opens, the block's real opener closes it into an empty
        // (rejected) body, and the block's real closer is left dangling: the block no longer pairs.
        let t = start + Duration::from_millis(210);
        session.resize_at(nz(41), nz(12), t).unwrap();
        session.mark_pty_resize_requested_at(nz(41), nz(12), t);
        session
            .feed_at(b"\x1b[2J\x1b[3J\x1b[H", t + Duration::from_millis(10))
            .unwrap();
        session
            .feed_at(
                b"$$\r\n$$\r\ny=1\r\n$$\r\nbarrier",
                t + Duration::from_millis(20),
            )
            .unwrap();

        // Display behaviour is unchanged: the block's source `$$\ny=1\n$$` is still literally on the
        // grid, so the hold re-anchors and keeps rendering its raster.
        let mut projection = session.new_projection(session.layout_key());
        session.refresh_projection(&mut projection);
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert!(
            frame
                .math_blocks
                .iter()
                .any(|block| block.display == MathBlockDisplay::Rendered),
            "the hold must keep rendering the block — display is untouched by this batch"
        );

        // ...but the settled detector no longer Owns that block: reported as exactly one HeldUnbacked
        // — a hold masking dead detection, the observable the flash oracle cannot see.
        assert!(
            !session
                .live_detection_ownership_ledger()
                .owns_source(&held_source),
            "the poison genuinely desynced the detector off the block the hold is showing"
        );
        let unbacked = session.held_unbacked_records();
        assert_eq!(
            unbacked.len(),
            1,
            "the masked-dead-detection strand must surface exactly once"
        );
        assert_eq!(unbacked[0].original_source, held_source);
    }

    #[test]
    fn primary_in_stream_reprint_reanchors_proven_formula_instead_of_flashing() {
        // Regression for the primary in-stream reprint flash. Codex reflows and reprints its whole
        // transcript mid-stream (a clear+home boundary). A proven live formula whose row is rewritten
        // at a new position by that reprint must re-anchor to its new row by exact source equality,
        // not drop to source until re-detection catches up. Before the primary_repaint preservation,
        // the reprint's `invalidate_live_row` dropped the record on the spot and the frame flashed
        // `$$x$$` back to bare source.
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session.feed_at(b"$$x$$\r\nbarrier", start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let mut oracle = crate::FormulaFlashOracle::default();
        let before = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            oracle.observe(&before).state,
            crate::FormulaFrameState::Rendered
        );
        let anchored_row = session
            .live_decorations
            .values()
            .next()
            .expect("one live decoration")
            .start
            .row;

        // A clear+home reprint (the transcript repaint boundary) rewrites the screen with the block
        // shifted down one row. The block's exact source reappears complete in the same feed.
        session
            .feed_at(
                b"\x1b[2J\x1b[Htop\r\n$$x$$\r\nbarrier",
                start + Duration::from_millis(50),
            )
            .unwrap();
        session.refresh_projection(&mut projection);
        let after = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            after.math_blocks.len(),
            1,
            "the proven block must survive the reprint, not drop to source"
        );
        assert_eq!(after.math_blocks[0].display, MathBlockDisplay::Rendered);
        assert_eq!(
            oracle.observe(&after).state,
            crate::FormulaFrameState::Rendered,
            "the reprint frame must not expose the formula's source"
        );
        let reanchored_row = session
            .live_decorations
            .values()
            .next()
            .expect("still one live decoration")
            .start
            .row;
        assert_eq!(
            reanchored_row,
            anchored_row + 1,
            "the record re-anchored to the block's new row on the reprinted grid"
        );
        assert!(
            !oracle.flash_detected(),
            "no frame flashed the formula to source: {:?}",
            oracle.flashed_sources()
        );
    }

    #[test]
    fn primary_reprint_that_removes_the_formula_source_releases_it_to_redetection() {
        // The other direction of the same guard: preservation is exact-source, never a blanket hold.
        // A reprint whose new grid no longer contains the block's source must NOT keep rendering the
        // stale raster — the record falls to re-detection, which is correct fallback, not a flash.
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session.feed_at(b"$$x$$\r\nbarrier", start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        assert_eq!(
            session.viewport_frame(&mut projection).unwrap().math_blocks[0].display,
            MathBlockDisplay::Rendered
        );

        // The reprint replaces the formula with prose: the exact source `$$x$$` is gone from the grid.
        session
            .feed_at(
                b"\x1b[2J\x1b[Hno more math here\r\nbarrier",
                start + Duration::from_millis(50),
            )
            .unwrap();
        session.refresh_projection(&mut projection);
        let after = session.viewport_frame(&mut projection).unwrap();
        assert!(
            after.math_blocks.is_empty(),
            "with its source gone, the block must not be held rendered off a stale raster"
        );
        assert!(
            !session
                .offscreen_decorations
                .iter()
                .any(|record| record.artifact.is_some()
                    && crate::observe_formula_frame(&after)
                        .rendered_sources
                        .contains(&record.span.original_source)),
            "a queued record whose source vanished never re-enters the rendered set"
        );
    }

    #[test]
    fn primary_progressive_synchronized_reprint_holds_formula_across_feeds_without_flashing() {
        // The remaining in-stream reprint class: Codex reprints inside a DEC 2026 synchronized
        // update that is *split across several pty chunks* — `?2026h` in one feed, the reflowed
        // lines in the next, the `?2026l` commit in a later feed. The reprint window must open on
        // the first chunk and span every chunk to the commit (the snapshot is taken once and held),
        // suppressing invalidation so the proven raster keeps rendering, then reproject the block
        // onto its shifted row at the commit. No observed frame across the whole split update may
        // expose the formula's source.
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session.feed_at(b"$$x$$\r\nbarrier", start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        let mut oracle = crate::FormulaFlashOracle::default();
        let before = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            oracle.observe(&before).state,
            crate::FormulaFrameState::Rendered
        );
        let anchored_row = session
            .live_decorations
            .values()
            .next()
            .expect("one live decoration")
            .start
            .row;

        // Chunk 1: open the synchronized update. The window opens and takes its snapshot; the update
        // buffers, so nothing is committed yet.
        session
            .feed_at(b"\x1b[?2026h", start + Duration::from_millis(50))
            .unwrap();
        assert!(
            session.primary_repaint_snapshot.is_some(),
            "the reprint window opened and holds a snapshot across the split update"
        );
        assert!(session.synchronized_update_deadline().is_some());
        session.refresh_projection(&mut projection);
        let mid = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            oracle.observe(&mid).state,
            crate::FormulaFrameState::Rendered,
            "mid-update the proven raster keeps rendering"
        );

        // Chunk 2: the reflowed transcript, block shifted down one row — still buffered.
        session
            .feed_at(
                b"\x1b[2J\x1b[Htop\r\n$$x$$\r\nbarrier",
                start + Duration::from_millis(51),
            )
            .unwrap();
        assert!(
            session.primary_repaint_snapshot.is_some(),
            "the window is still open across the buffered chunk"
        );
        session.refresh_projection(&mut projection);
        let buffered = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            oracle.observe(&buffered).state,
            crate::FormulaFrameState::Rendered
        );

        // Chunk 3: commit. The buffered reprint lands atomically; the window closes and reprojects
        // the block onto its new row.
        session
            .feed_at(b"\x1b[?2026l", start + Duration::from_millis(52))
            .unwrap();
        assert!(
            session.primary_repaint_snapshot.is_none(),
            "the window closed at the commit"
        );
        session.refresh_projection(&mut projection);
        let after = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            after.math_blocks.len(),
            1,
            "the proven block survives the split reprint, not dropped to source"
        );
        assert_eq!(after.math_blocks[0].display, MathBlockDisplay::Rendered);
        assert_eq!(
            oracle.observe(&after).state,
            crate::FormulaFrameState::Rendered
        );
        let reanchored_row = session
            .live_decorations
            .values()
            .next()
            .expect("still one live decoration")
            .start
            .row;
        assert_eq!(
            reanchored_row,
            anchored_row + 1,
            "the record reprojected to the block's shifted row on the committed grid"
        );
        assert!(
            !oracle.flash_detected(),
            "no frame across the split synchronized reprint flashed the formula: {:?}",
            oracle.flashed_sources()
        );
    }

    #[test]
    fn primary_progressive_reprint_that_rewrites_the_source_releases_it_across_feeds() {
        // The release direction of the split-update guard: suppression is not a blanket hold. A
        // split synchronized reprint whose committed grid no longer contains the block's source must
        // release the record to re-detection, never keep rendering a stale raster at a wrong place.
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session.feed_at(b"$$x$$\r\nbarrier", start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            1
        );
        let mut projection = session.new_projection(session.layout_key());
        assert_eq!(
            session.viewport_frame(&mut projection).unwrap().math_blocks[0].display,
            MathBlockDisplay::Rendered
        );

        // Split synchronized reprint: open, write prose (no `$$x$$`), commit.
        session
            .feed_at(b"\x1b[?2026h", start + Duration::from_millis(50))
            .unwrap();
        session
            .feed_at(
                b"\x1b[2J\x1b[Hno more math here\r\nbarrier",
                start + Duration::from_millis(51),
            )
            .unwrap();
        session
            .feed_at(b"\x1b[?2026l", start + Duration::from_millis(52))
            .unwrap();
        session.refresh_projection(&mut projection);
        let after = session.viewport_frame(&mut projection).unwrap();
        assert!(
            after.math_blocks.is_empty(),
            "with its source rewritten away, the block must not be held rendered off a stale raster"
        );
        assert!(
            !session
                .offscreen_decorations
                .iter()
                .any(|record| record.artifact.is_some()
                    && crate::observe_formula_frame(&after)
                        .rendered_sources
                        .contains(&record.span.original_source)),
            "a held record whose source vanished never re-enters the rendered set"
        );
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
        hide_cursor(&mut session, start);
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
        hide_cursor(&mut tiny, start);
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
        hide_cursor(&mut session, start);
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
    fn primary_live_scroll_bridges_every_frozen_source_prefix_row_before_full_handoff() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(16));
        session
            .feed_at(
                b"$$\r\nA=\r\n\\begin{pmatrix}\r\na & b\\\\\r\nc & d\r\n\\end{pmatrix}\r\n$$\r\ntail-1\r\ntail-2\r\ntail-3\r\ntail-4\r\ntail-5\r\ntail-6\r\ntail-7\r\ntail-8\r\ntail-9",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 54)),
            1
        );

        // Two full-screen scrolls commit the opener and the `A=` body row to history while the
        // remaining five source rows stay live. The raster is still the one proven for all seven
        // rows, so the frozen prefix and live band must be projected as one bridge immediately.
        session
            .feed_at(b"\r\nmore", start + Duration::from_millis(210))
            .unwrap();
        session
            .feed_at(b"\r\nfinal", start + Duration::from_millis(220))
            .unwrap();
        let record = session
            .live_decorations
            .values()
            .find(|record| record.span.render_source.contains(r"\begin{pmatrix}"))
            .expect("the partially frozen occurrence keeps its live raster");
        assert_eq!(
            record.frozen_prefix.len(),
            2,
            "both committed source rows belong to the retained occurrence"
        );
        assert_eq!((record.band_start_row, record.band_end_row), (0, 4));

        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        session.refresh_projection(&mut projection);
        let frame = session.viewport_frame(&mut projection).unwrap();
        let block = frame
            .math_blocks
            .iter()
            .find(|block| block.source.contains(r"\begin{pmatrix}"))
            .unwrap_or_else(|| {
                panic!(
                    "the split occurrence renders as one block; blocks={:?} prefix={:?} staging={} frozen={} record=({:?} {:?} {:?} {:?} {} {}) rows={:?}",
                    frame.math_blocks,
                    record.frozen_prefix,
                    session.transcript().staging_len(),
                    session.transcript().frozen().len(),
                    record.screen,
                    record.generation,
                    record.start,
                    record.end,
                    record.artifact.is_some(),
                    record.show_source,
                    frame
                        .row_map
                        .iter()
                        .map(|row| row.live_grid_row)
                        .collect::<Vec<_>>(),
                )
            });
        assert_eq!(
            block.frozen_prefix_rows, 2,
            "projection must bridge the complete frozen prefix into the live band"
        );

        let rows = frame
            .cells
            .chunks(frame.columns.get() as usize)
            .take(2)
            .map(|cells| {
                cells
                    .iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(
            rows.iter().all(|row| row.trim().is_empty()),
            "the bridged raster must suppress both frozen source rows, got {rows:?}"
        );
    }

    /// `scroll-strand.vt` frame 2510: the outer Dollars block begins at live row zero, and one
    /// synchronized Codex frame scrolls the top-anchored region by two rows. The opener and `A=`
    /// line freeze together while the byte-identical pmatrix body remains live. This is the
    /// intersection of the batched-scroll and frozen-prefix paths: the already-proven outer
    /// occurrence must keep ownership as one clipped bridge, rather than disappear and let the
    /// inner environment be re-detected as a narrower replacement.
    #[test]
    fn synchronized_two_row_scroll_keeps_row_zero_outer_block_as_one_bridge() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(16));
        session
            .feed_at(
                b"$$\r\nA=\r\n\\begin{pmatrix}\r\na & b\\\\\r\nc & d\r\n\\end{pmatrix}\r\n$$\r\n\r\n$$\r\ny=1\r\n$$\r\nprompt",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 54)),
            2
        );
        let outer_source = session
            .live_decorations
            .values()
            .find(|record| record.span.delimiter_kind == DelimiterKind::Dollars)
            .map(|record| record.span.original_source.clone())
            .expect("the complete outer Dollars block renders before the scroll");

        // Byte-for-byte shape from the recording's critical transaction: synchronized-update
        // open, top-anchored scroll region, two-line SU, restore margins, commit.
        session
            .feed_at(
                b"\x1b[?2026h\x1b[1;12r\x1b[2S\x1b[r\x1b[?2026l",
                start + Duration::from_millis(210),
            )
            .unwrap();

        let outer = session
            .live_decorations
            .values()
            .find(|record| record.span.original_source == outer_source)
            .expect("the partially frozen outer occurrence must keep its raster");
        assert!(outer.artifact.is_some());
        assert_eq!(
            outer.clipped_top_rows, 2,
            "the opener and A= row are clipped above the live grid"
        );
        assert_eq!((outer.band_start_row, outer.band_end_row), (0, 4));
        assert!(
            session.live_decorations.values().all(|record| {
                !matches!(record.span.delimiter_kind, DelimiterKind::Environment(_))
                    || !record.span.original_source.contains(r"\begin{pmatrix}")
            }),
            "the inner pmatrix must not take ownership from the retained outer block"
        );
    }

    /// A bridge prefix names transcript lines. Codex clears its own scrollback (`ESC [ 3 J`) all the
    /// time, and a prefix outliving its lines is neither a bridge — the viewport rejects ids that
    /// are no longer the history tail — nor an honest live-only band, because the stale claim also
    /// suppresses the free-height budget the clipped rows need and the raster clips short of its own
    /// ink. Losing any prefix line must retire the whole prefix.
    #[test]
    fn clearing_history_retires_a_live_bridge_prefix_that_no_longer_has_lines() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session
            .feed_at(
                b"lead-0\r\nlead-1\r\n$$\r\nf(x)=\\frac{1}{\\sigma\\sqrt{2\\pi}}\r\n\\exp\\left(-\\frac{(x-\\mu)^2}{2\\sigma^2}\\right)\r\n$$",
                start,
            )
            .unwrap();
        hide_cursor(&mut session, start);
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 54)),
            1
        );
        let mut bytes = b"\x1b[?2026h\x1b[1;10r\x1b[10;1H".to_vec();
        for row in 0..3 {
            bytes.extend_from_slice(b"\r\n");
            bytes.extend_from_slice(format!("new-{row}").as_bytes());
        }
        bytes.extend_from_slice(b"\x1b[r\x1b[?2026l");
        session
            .feed_at(&bytes, start + Duration::from_millis(210))
            .unwrap();
        assert_eq!(
            session
                .live_decorations
                .values()
                .map(|record| record.frozen_prefix.len())
                .collect::<Vec<_>>(),
            vec![1],
            "the scrolled-away opener must be the bridge prefix"
        );

        session
            .feed_at(b"\x1b[3J", start + Duration::from_millis(240))
            .unwrap();
        assert!(
            session
                .live_decorations
                .values()
                .chain(session.offscreen_decorations.iter())
                .all(|record| record.frozen_prefix.is_empty() && record.staging_prefix.is_empty()),
            "clearing history must retire every prefix claim, got {:?}",
            session
                .live_decorations
                .values()
                .chain(session.offscreen_decorations.iter())
                .map(|record| (record.frozen_prefix.clone(), record.staging_prefix.clone()))
                .collect::<Vec<_>>()
        );
    }

    /// The plain-shift half of the same fact, and the broadest form of it: *any* write that scrolls
    /// more than one row in a single parse quantum — three newlines in one `write`, no DECSTBM, no
    /// synchronized update — must shift a proven band by three, not invalidate it. Proving each
    /// removal event separately against the already-settled grid compared a one-row shift against a
    /// grid that had moved three and dropped every record; with no repaint window open there is no
    /// fingerprint remap to rescue them afterwards, so the formula simply reverts to source.
    #[test]
    fn a_multi_row_synchronized_commit_shifts_a_live_formula_instead_of_dropping_it() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(16));
        session
            .feed_at(
                b"lead-0\r\nlead-1\r\nlead-2\r\nlead-3\r\nlead-4\r\n$$\r\nf(x)=\\frac{1}{\\sigma\\sqrt{2\\pi}}\r\n\\exp\\left(-\\frac{(x-\\mu)^2}{2\\sigma^2}\\right)\r\n$$",
                start,
            )
            .unwrap();
        hide_cursor(&mut session, start);
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 54)),
            1
        );
        let band = |session: &DualPlaneSession| {
            session
                .live_decorations
                .values()
                .find(|record| record.span.original_source.contains(r"\sigma\sqrt"))
                .map(|record| (record.artifact.is_some(), record.band_start_row))
        };
        assert_eq!(band(&session), Some((true, 5)));

        let mut bytes = b"\x1b[16;1H".to_vec();
        for row in 0..3 {
            bytes.extend_from_slice(b"\r\n");
            bytes.extend_from_slice(format!("new-{row}").as_bytes());
        }
        session
            .feed_at(&bytes, start + Duration::from_millis(210))
            .unwrap();
        assert_eq!(
            band(&session),
            Some((true, 2)),
            "a three-row scroll in one write must shift the proven band, not invalidate it"
        );
    }

    /// Codex's inline TUI commits transcript rows through a top-anchored DECSTBM region whose
    /// bottom margin is the composer's top, and it emits the whole frame — set region, several
    /// row commits, restore region — inside one DEC 2026 synchronized update, which reaches the
    /// session as one parse quantum. The vendor terminal has already applied every row of that
    /// quantum by the time its removal facts are handled, so proving each removal against the
    /// settled grid one row at a time compares a one-row shift against a grid that already moved
    /// three, and every proven record fails and drops to source. That is the reported breakage:
    /// half-rendered/half-source wedged together while Codex streams, healed only by a zoom.
    ///
    /// The block here is wholly inside the scroll region (the real geometry: Codex never writes
    /// transcript below its own margin), and the scrolls carry its opener across live row zero so
    /// the surviving record must be a frozen/live bridge, not merely a shifted band.
    #[test]
    fn top_anchored_synchronized_scroll_keeps_a_multiline_formula_bridge_rendered() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(12));
        session
            .feed_at(
                b"lead-0\r\nlead-1\r\n$$\r\nf(x)=\\frac{1}{\\sigma\\sqrt{2\\pi}}\r\n\\exp\\left(-\\frac{(x-\\mu)^2}{2\\sigma^2}\\right)\r\n$$",
                start,
            )
            .unwrap();
        hide_cursor(&mut session, start);
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 54)),
            1
        );
        let gaussian = |session: &DualPlaneSession| {
            session
                .live_decorations
                .values()
                .find(|record| record.span.original_source.contains(r"\sigma\sqrt"))
                .map(|record| {
                    (
                        record.artifact.is_some(),
                        record.band_start_row,
                        record.band_end_row,
                        record.frozen_prefix.len() + record.staging_prefix.len(),
                    )
                })
        };
        assert_eq!(
            gaussian(&session),
            Some((true, 2, 6, 0)),
            "the gaussian raster must be resident before scrolling"
        );

        // Three rows committed inside one synchronized update, twice over. The block starts at grid
        // row 2, so the first frame already carries its opener into history and the second leaves
        // only the closer live — the deepest bridge the occurrence can hold before it retires.
        for index in 0..2_u64 {
            let at = start + Duration::from_millis(210 + index * 10);
            let mut frame = b"\x1b[1;10r\x1b[10;1H".to_vec();
            for row in 0..3_u64 {
                frame.extend_from_slice(b"\r\n");
                frame.extend_from_slice(format!("new-{index}-{row}").as_bytes());
            }
            frame.extend_from_slice(b"\x1b[r");
            let mut bytes = b"\x1b[?2026h".to_vec();
            bytes.extend_from_slice(&frame);
            bytes.extend_from_slice(b"\x1b[?2026l");
            session.feed_at(&bytes, at).unwrap();
            let Some((rendered, band_start, band_end, prefix_rows)) = gaussian(&session) else {
                panic!("synchronized frame {index} dropped the gaussian record entirely");
            };
            assert!(rendered, "synchronized frame {index} dropped the raster");
            // The band shrinks from the top by the full three rows the frame committed, and every
            // source row that left the grid joins the bridge prefix. The prefix counts logical
            // lines, so the soft-wrapped `\exp…` body row contributes one id for its two grid rows.
            let expected = [(0_u32, 3_u32, 1_usize), (0, 0, 3)][index as usize];
            assert_eq!(
                (band_start, band_end, prefix_rows),
                expected,
                "synchronized frame {index} must keep one bridged occurrence, not a re-anchored fragment"
            );
        }
    }

    #[test]
    fn multiline_handoff_suppresses_inner_environment_before_queued_workers_can_land() {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(40), nz(8));
        session
            .feed_at(
                b"$$\r\nA=\r\n\\begin{pmatrix}\r\na & b\\\\\r\nc & d\r\n\\end{pmatrix}\r\n$$\r\ntail",
                start,
            )
            .unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 54)),
            1
        );
        let live_raster = session
            .live_decorations
            .values()
            .find(|record| record.span.delimiter_kind == DelimiterKind::Dollars)
            .and_then(|record| record.artifact.as_ref())
            .map(|artifact| Arc::clone(&artifact.rgba))
            .expect("the complete outer $$ block renders live");

        // Freeze the seven source rows one by one. The `\begin` and `\end` candidates acquire
        // queued frozen scans before the closing `$$` matures the exact live-raster handoff.
        for index in 0..7 {
            session
                .feed_at(
                    format!("\r\nscroll-{index}").as_bytes(),
                    start + Duration::from_millis(210 + index * 10),
                )
                .unwrap();
        }
        assert!(
            session.live_decorations.is_empty(),
            "the fully frozen occurrence must leave the live map"
        );
        let outer_id = {
            let (outer_id, outer) = session
                .decorations
                .iter()
                .find(|(_, record)| {
                    record
                        .span
                        .as_ref()
                        .is_some_and(|span| span.delimiter_kind == DelimiterKind::Dollars)
                        && record.artifact.is_some()
                })
                .expect("the outer owner receives the handed-off raster");
            assert_eq!(outer.decoration, DecorationLifecycle::Ready);
            assert!(Arc::ptr_eq(
                &outer.artifact.as_ref().unwrap().rgba,
                &live_raster
            ));
            *outer_id
        };

        let inner_ids = session
            .document
            .entries()
            .iter()
            .filter(|(_, entry)| {
                entry.line.text.contains(r"\begin{pmatrix}")
                    || entry.line.text.contains(r"\end{pmatrix}")
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        assert_eq!(inner_ids.len(), 2);
        for id in &inner_ids {
            assert_eq!(
                session.decorations[id].decoration,
                DecorationLifecycle::Suppressed,
                "the handoff owner must suppress inner structural rows before their queued workers land"
            );
        }

        // Late completions are stale against Suppressed records and cannot replace or overlap the
        // exact outer owner. This is ownership transfer of the existing raster, not a re-render.
        while let Some(mut task) = session.take_worker_task() {
            let resolved = resolve_detection_task(&mut task);
            let result = resolved.then(|| synthetic_raster(32, 36));
            let _ =
                session.complete_worker_result(task, result.ok_or(MathRenderError::NotDetected));
        }
        let outer = &session.decorations[&outer_id];
        assert_eq!(outer.decoration, DecorationLifecycle::Ready);
        assert!(Arc::ptr_eq(
            &outer.artifact.as_ref().unwrap().rgba,
            &live_raster
        ));
        for id in inner_ids {
            assert_eq!(
                session.decorations[&id].decoration,
                DecorationLifecycle::Suppressed
            );
        }
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

    /// Strand a multi-line block's frozen closer at `Pending` by dropping its in-flight scan: the
    /// closer's scan spans the whole block, but the opener (a non-candidate input line) is evicted
    /// from history while the scan is in flight, so the delivered result fails `block_is_current`
    /// and is dropped. Returns the session (with the closer recorded as stranded) and the closer id.
    fn strand_frozen_closer_pending() -> (DualPlaneSession, TranscriptId) {
        let mut session =
            DualPlaneSession::with_frozen_quota(nz(40), nz(2), NonZeroUsize::new(8).unwrap());
        session
            .feed(b"\\begin{align}\r\nx &= y\r\n\\end{align}\r\nafter\r\nmore\r\n")
            .unwrap();

        let opener = *session
            .document()
            .entries()
            .iter()
            .find(|(_, entry)| entry.line.text.contains("\\begin{"))
            .map(|(id, _)| id)
            .expect("frozen opener");
        let closer = *session
            .document()
            .entries()
            .iter()
            .find(|(_, entry)| entry.line.text.contains("\\end{"))
            .map(|(id, _)| id)
            .expect("frozen closer");

        // Hold the closer's multi-line scan (the task whose inputs span the whole block).
        let mut held = None;
        while let Some(task) = session.take_worker_task() {
            if task.candidate_id == closer && task.inputs.iter().any(|input| input.id == opener) {
                held = Some(task);
                break;
            }
        }
        let task = held.expect("closer scan spanning the block");
        assert_eq!(
            session.decoration(closer).map(|record| record.decoration),
            Some(DecorationLifecycle::Pending),
            "closer is the in-flight scan candidate",
        );

        // Evict the opener from history while the scan is in flight, keeping the closer resident.
        let mut guard = 0;
        while session.document().entries().contains_key(&opener) {
            session.feed(b"filler\r\n").unwrap();
            guard += 1;
            assert!(guard < 256, "opener never evicted");
            assert!(
                session.document().entries().contains_key(&closer),
                "closer evicted before opener; tighten the corpus",
            );
        }

        // Deliver the now-stale result. `block_is_current` fails on the missing opener, so the
        // completion is correctly not accepted (the stale raster must never land).
        assert!(
            !session.complete_worker_task(task),
            "a result whose block lost a source line must not be accepted",
        );
        (session, closer)
    }

    #[test]
    fn dropped_multiline_completion_defers_rearm_then_recovers_at_quiescence() {
        // The liveness hole: a dropped completion left the closer frozen at `Pending` forever
        // (`schedule_scan` no-ops on any non-`None` decoration), showing raw source until a width
        // resize's layout bump — the reported "scrolling never rescues it, resizing does". The
        // correct fix records the strand and flips it to `None` only at a quiescent frame, so the
        // block re-enters scheduling from current source. It must NOT re-arm at the drop site (that
        // is what let the reopen fix storm a still-moving source).
        let (mut session, closer) = strand_frozen_closer_pending();

        // Deferred, not reopened at the drop site.
        assert_eq!(
            session.decoration(closer).map(|record| record.decoration),
            Some(DecorationLifecycle::Pending),
            "a dropped completion must be remembered, not re-armed at the drop site",
        );
        assert!(
            session.stranded_pending.contains_key(&closer),
            "the stranded closer must be recorded for a deferred re-arm",
        );

        // A quiescent re-arm reopens it for re-scheduling exactly once. RED before the fix: the
        // closer stays `Pending` and is never re-issued.
        session.rearm_stranded_pending();
        assert_eq!(
            session.decoration(closer).map(|record| record.decoration),
            Some(DecorationLifecycle::None),
            "at quiescence a stranded closer must reopen for re-scheduling, not freeze at source",
        );
        assert!(
            session.stranded_pending.is_empty(),
            "the re-arm consumes the strand; a fresh drop would re-record it",
        );
    }

    #[test]
    fn stranded_rearm_is_suppressed_while_the_source_is_in_motion() {
        // The churn guard. Re-arming a stranded `Pending` candidate while the source is still
        // reflowing/reprinting lets the per-frame visible scheduler re-issue it, drop it again
        // mid-motion, and re-arm again — a reschedule storm that re-runs block detection over the
        // whole range every frame and tears neighbouring bands (the reopen regression). The re-arm
        // must be gated on quiescence.
        let (mut session, closer) = strand_frozen_closer_pending();
        assert!(session.stranded_pending.contains_key(&closer));

        // A primary reprint window is open: re-arm is a no-op, so no fresh scan is issued into the
        // moving source and the strand is retained for later.
        session.primary_repaint_in_progress = true;
        session.rearm_stranded_pending();
        assert_eq!(
            session.decoration(closer).map(|record| record.decoration),
            Some(DecorationLifecycle::Pending),
            "re-arm must not fire while a reprint window is open",
        );
        assert!(
            session.stranded_pending.contains_key(&closer),
            "the strand must be retained until the source is quiescent",
        );

        // Once the window closes, the same re-arm proceeds.
        session.primary_repaint_in_progress = false;
        session.rearm_stranded_pending();
        assert_eq!(
            session.decoration(closer).map(|record| record.decoration),
            Some(DecorationLifecycle::None),
            "re-arm proceeds once the reprint window closes",
        );
    }

    fn decoded_test_image(
        occurrence_id: u64,
        width_px: u32,
        height_px: u32,
        animated: bool,
    ) -> DecodedInlineImage {
        DecodedInlineImage {
            occurrence_id,
            key: format!("image:test-{occurrence_id}"),
            rgba: Arc::from(vec![0x7f; width_px as usize * height_px as usize * 4]),
            width_px,
            height_px,
            animated,
        }
    }

    #[test]
    fn inline_image_geometry_uses_dpi_width_fit_and_two_thirds_height_cap() {
        let cell = NonZeroI64::new(10 * SUBPIXELS_PER_PX).unwrap();
        let mut session = DualPlaneSession::with_cell_height(nz(20), nz(12), cell);
        session.set_cell_width_subpixels(cell);
        session.feed(b"\x1b]1337;File=inline=1:AAAA\x07").unwrap();
        let SessionDecorationTask::InlineImage(task) =
            session.take_decoration_worker_task().unwrap()
        else {
            panic!("OSC 1337 must enqueue an image worker task");
        };
        assert!(session.complete_inline_image_result(
            task.clone(),
            Ok(decoded_test_image(task.occurrence_id, 200, 300, false)),
        ));

        let record = &session.inline_image_records()[0];
        assert_eq!(record.display_rows, Some(4));
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let placement = frame
            .math_blocks
            .iter()
            .find(|placement| {
                matches!(
                    placement.artifact.kind,
                    bt_viewport::RgbaArtifactKind::InlineImage { .. }
                )
            })
            .expect("decoded image projects through the shared RGBA placement");
        assert_eq!(placement.artifact.render_scale_milli, 133);
        assert_eq!(placement.artifact.height_subpixels, 40_858);
        assert_eq!(placement.clip_height_subpixels, 40_858);
    }

    #[test]
    fn inline_image_width_fit_and_zoom_scale_reuse_the_content_texture() {
        let cell = NonZeroI64::new(10 * SUBPIXELS_PER_PX).unwrap();
        let mut session = DualPlaneSession::with_cell_height(nz(20), nz(20), cell);
        session.set_cell_width_subpixels(cell);
        session.feed(b"\x1b]1337;File=inline=1:AAAA\x07").unwrap();
        let SessionDecorationTask::InlineImage(task) =
            session.take_decoration_worker_task().unwrap()
        else {
            panic!("OSC 1337 must enqueue an image worker task");
        };
        assert!(session.complete_inline_image_result(
            task.clone(),
            Ok(decoded_test_image(task.occurrence_id, 400, 10, false)),
        ));
        let mut projection = session.new_projection(session.layout_key());
        let first = session.viewport_frame(&mut projection).unwrap();
        let first = first
            .math_blocks
            .iter()
            .find(|placement| placement.artifact.key.starts_with("image:"))
            .unwrap();
        assert_eq!(first.artifact.render_scale_milli, 500);
        let key = first.artifact.key.clone();
        let decoded = session
            .inline_images
            .get(&task.occurrence_id)
            .and_then(|record| record.artifact.as_ref())
            .unwrap();
        assert_eq!(decoded.key, key);

        let mut zoom = DualPlaneSession::with_cell_height(nz(20), nz(20), cell);
        zoom.set_cell_width_subpixels(cell);
        zoom.feed(b"\x1b]1337;File=inline=1:BBBB\x07").unwrap();
        let SessionDecorationTask::InlineImage(zoom_task) =
            zoom.take_decoration_worker_task().unwrap()
        else {
            panic!("second OSC 1337 must enqueue an image worker task");
        };
        assert!(zoom.complete_inline_image_result(
            zoom_task.clone(),
            Ok(decoded_test_image(zoom_task.occurrence_id, 10, 10, false,)),
        ));
        let mut zoom_projection = zoom.new_projection(zoom.layout_key());
        let before_zoom = zoom.viewport_frame(&mut zoom_projection).unwrap();
        let before_zoom = before_zoom
            .math_blocks
            .iter()
            .find(|placement| placement.artifact.key.starts_with("image:"))
            .unwrap();
        assert_eq!(before_zoom.artifact.render_scale_milli, 1000);
        let zoom_key = before_zoom.artifact.key.clone();
        zoom.set_layout_key(LayoutKey {
            dpi_milli: nz(2000),
            ..zoom.layout_key()
        });
        let zoomed = zoom.viewport_frame(&mut zoom_projection).unwrap();
        let zoomed = zoomed
            .math_blocks
            .iter()
            .find(|placement| placement.artifact.key == zoom_key)
            .unwrap();
        assert_eq!(zoomed.artifact.render_scale_milli, 2000);
        assert_eq!(
            zoomed.artifact.key, zoom_key,
            "zoom scales the held texture without re-decoding or rekeying pixels"
        );
    }

    #[test]
    fn malformed_inline_image_completion_leaves_a_text_placeholder() {
        let mut session = DualPlaneSession::new(nz(30), nz(4));
        session
            .feed(b"\x1b]1337;File=inline=1:not_base64!\x07")
            .unwrap();
        let SessionDecorationTask::InlineImage(task) =
            session.take_decoration_worker_task().unwrap()
        else {
            panic!("OSC 1337 must enqueue an image worker task");
        };
        let result = decode_inline_image(task.clone());
        assert_eq!(result, Err(InlineImageDecodeError::InvalidBase64));
        assert!(session.complete_inline_image_result(task, result));
        assert!(session.inline_image_records()[0].failed);
        assert!(
            session
                .terminal()
                .visible_text()
                .iter()
                .any(|row| row.contains("[image]"))
        );
        let mut projection = session.new_projection(session.layout_key());
        assert!(
            session
                .viewport_frame(&mut projection)
                .unwrap()
                .math_blocks
                .iter()
                .all(|placement| !matches!(
                    placement.artifact.kind,
                    bt_viewport::RgbaArtifactKind::InlineImage { .. }
                ))
        );
    }

    #[test]
    fn inline_image_anchor_migrates_to_history_without_decode_or_texture_identity_churn() {
        let mut session = DualPlaneSession::with_cell_height(
            nz(20),
            nz(4),
            NonZeroI64::new(10 * SUBPIXELS_PER_PX).unwrap(),
        );
        session
            .feed(b"\x1b]1337;File=inline=1:AAAA\x07\r\n")
            .unwrap();
        let SessionDecorationTask::InlineImage(task) =
            session.take_decoration_worker_task().unwrap()
        else {
            panic!("OSC 1337 must enqueue an image worker task");
        };
        let key = format!("image:test-{}", task.occurrence_id);
        assert!(session.complete_inline_image_result(
            task.clone(),
            Ok(decoded_test_image(task.occurrence_id, 10, 10, true)),
        ));
        session.feed(b"one\r\ntwo\r\nthree\r\nfour\r\n").unwrap();

        let record = session.inline_images.get(&task.occurrence_id).unwrap();
        assert!(
            matches!(
                session.document.anchor(record.end_anchor).unwrap(),
                ContentAnchor::History { .. }
            ),
            "the normal document capture transaction must migrate the image anchor"
        );
        assert_eq!(
            record
                .artifact
                .as_ref()
                .map(|artifact| artifact.key.as_str()),
            Some(key.as_str())
        );
        assert!(session.take_decoration_worker_task().is_none());

        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap();
        projection.scroll_to_top();
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert!(
            frame.math_blocks.iter().any(|placement| {
                placement.artifact.key == key
                    && matches!(
                        placement.artifact.kind,
                        bt_viewport::RgbaArtifactKind::InlineImage { animated: true }
                    )
            }),
            "history frame did not project image; blocks={:?} origin={:?}",
            frame
                .math_blocks
                .iter()
                .map(|placement| (&placement.artifact.key, placement.artifact.kind))
                .collect::<Vec<_>>(),
            frame.viewport_origin,
        );
    }

    fn temporary_path_image() -> (PathBuf, PathBuf) {
        use base64::Engine as _;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "betterterminal-session-path-image-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("one pixel.png");
        let png = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap();
        std::fs::write(&path, png).unwrap();
        (directory, path)
    }

    fn enable_path_detection(session: &mut DualPlaneSession) {
        session.set_math_layout_options(MathLayoutOptions {
            detect_image_paths: true,
            ..MathLayoutOptions::default()
        });
    }

    fn feed_ascii_one_byte_at_a_time(
        session: &mut DualPlaneSession,
        text: &str,
        started: Instant,
    ) -> Instant {
        let mut observed_at = started;
        for byte in text.bytes() {
            session.feed_at(&[byte], observed_at).unwrap();
            observed_at += Duration::from_millis(1);
        }
        observed_at
    }

    fn feed_psreadline_paste_bursts_until_cursor_hidden(
        session: &mut DualPlaneSession,
        text: &str,
        started: Instant,
    ) -> Instant {
        session.feed_at(b"PS> ", started).unwrap();
        let mut observed_at = started;
        let mut typed = String::new();
        let character_count = text.chars().count();
        for (index, character) in text.chars().enumerate() {
            observed_at += Duration::from_millis(1);
            typed.push(character);
            let cursor_column = "PS> ".chars().count() + typed.chars().count() + 1;
            let burst = format!("\x1b[?25l\x1b[1;1H\x1b[2KPS> {typed}\x1b[1;{cursor_column}H");
            session.feed_at(burst.as_bytes(), observed_at).unwrap();
            if index + 1 != character_count {
                // Match the real cursor-accept.vt chunking: every one of its twelve PSReadLine
                // bursts ends a PTY chunk while DECTCEM is off, then re-enables it in a later chunk.
                session.feed_at(b"\x1b[?25h", observed_at).unwrap();
            }
        }
        observed_at
    }

    #[test]
    fn psreadline_paste_hidden_chunk_keeps_input_line_suppressed_at_stability() {
        let (directory, png_path) = temporary_path_image();
        let path = directory.join("cursor-paste.jpg");
        std::fs::rename(&png_path, &path).unwrap();
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(240), nz(8));
        enable_path_detection(&mut session);
        let command = format!("echo \"[Image: source: {}]\"", path.display());

        let hidden_at =
            feed_psreadline_paste_bursts_until_cursor_hidden(&mut session, &command, started);
        assert!(!session.terminal.cursor().visible);
        assert_eq!(
            session.advance_live_stability(hidden_at + LIVE_MATH_STABLE_INTERVAL),
            0,
            "the hidden half of a PSReadLine repaint burst must retain the input-line gate"
        );
        assert!(
            session.inline_images.is_empty(),
            "a chunk boundary inside the cursor-hidden window must not create the path occurrence"
        );
        assert!(session.take_decoration_worker_task().is_none());

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn psreadline_hidden_chunk_suppresses_in_flight_worker_completion() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));
        session.feed_at(b"$$x$$\r\nprompt", started).unwrap();
        session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL);
        let mut task = session.take_live_worker_task().unwrap();
        assert!(resolve_live_detection_task(&mut task));

        let cursor_at_source = started + LIVE_MATH_STABLE_INTERVAL;
        session.feed_at(b"\x1b[1;1H", cursor_at_source).unwrap();
        session
            .feed_at(
                b"\x1b[?25l\x1b[1;1H",
                cursor_at_source + Duration::from_millis(1),
            )
            .unwrap();
        assert!(!session.terminal.cursor().visible);
        assert!(session.complete_live_worker_result(task, Ok(synthetic_raster(32, 18))));
        assert!(
            session.live_decorations.is_empty(),
            "completion inside the hidden half-burst must retain the source-line gate"
        );
        assert_eq!(session.live_rows[0].candidate_signature, None);
    }

    #[test]
    fn hidden_cursor_memory_is_invalidated_by_grid_scroll() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));
        session.feed_at(b"prompt", started).unwrap();
        session
            .feed_at(b"\x1b[?25l", started + Duration::from_millis(1))
            .unwrap();
        assert_eq!(session.cursor_suppressed_logical_line(), Some((0, 0)));

        session
            .feed_at(b"\x1b[S", started + Duration::from_millis(2))
            .unwrap();
        assert_eq!(
            session.cursor_suppressed_logical_line(),
            None,
            "an explicit grid scroll changes row identity and must retire the remembered row"
        );
    }

    #[test]
    fn hidden_cursor_memory_is_invalidated_by_full_clear() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));
        session.feed_at(b"prompt", started).unwrap();
        session
            .feed_at(b"\x1b[?25l", started + Duration::from_millis(1))
            .unwrap();
        assert_eq!(session.cursor_suppressed_logical_line(), Some((0, 0)));

        session
            .feed_at(b"\x1b[2J", started + Duration::from_millis(2))
            .unwrap();
        assert_eq!(
            session.cursor_suppressed_logical_line(),
            None,
            "ED 2 is a semantic clear even when CUP arrives in another PTY chunk"
        );
    }

    #[test]
    fn primary_hidden_cursor_memory_does_not_cross_to_alternate_screen() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));
        session.feed_at(b"prompt", started).unwrap();
        session
            .feed_at(b"\x1b[?25l", started + Duration::from_millis(1))
            .unwrap();
        assert_eq!(session.cursor_suppressed_logical_line(), Some((0, 0)));

        session
            .feed_at(b"\x1b[?1049h", started + Duration::from_millis(2))
            .unwrap();
        assert_eq!(session.live_screen, ScreenId::Alternate);
        assert_eq!(
            session.cursor_suppressed_logical_line(),
            None,
            "the primary screen's remembered row must not suppress alternate content"
        );
    }

    #[test]
    fn newly_visible_cursor_on_another_line_replaces_hidden_memory() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));
        session.feed_at(b"prompt", started).unwrap();
        session
            .feed_at(b"\x1b[?25l", started + Duration::from_millis(1))
            .unwrap();
        assert_eq!(session.cursor_suppressed_logical_line(), Some((0, 0)));

        session
            .feed_at(b"\x1b[3;1H\x1b[?25h", started + Duration::from_millis(2))
            .unwrap();
        session
            .feed_at(b"\x1b[?25l", started + Duration::from_millis(3))
            .unwrap();
        assert_eq!(
            session.cursor_suppressed_logical_line(),
            Some((2, 2)),
            "a newly published visible line replaces, rather than extends, the old memory"
        );
    }

    #[test]
    fn psreadline_multiline_paste_suppresses_source_above_explicit_empty_cursor_line_until_enter() {
        let (directory, png_path) = temporary_path_image();
        let path = directory.join("multiline-paste.jpg");
        std::fs::rename(&png_path, &path).unwrap();
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(240), nz(8));
        enable_path_detection(&mut session);
        let command = format!("echo \"[Image: source: {}]\"", path.display());
        let mut typed = String::new();
        let mut observed_at = started;

        for character in command.chars() {
            observed_at += Duration::from_millis(1);
            typed.push(character);
            let cursor_column = typed.chars().count() + 1;
            let burst = format!("\x1b[?25l\x1b[2;1H\x1b[2K{typed}\x1b[2;{cursor_column}H\x1b[?25h");
            for byte in burst.bytes() {
                session.feed_at(&[byte], observed_at).unwrap();
            }
        }
        // A pasted trailing newline makes PSReadLine publish a separate empty continuation row.
        // The source row is not WRAPLINE-linked to it: the final cursor placement is an absolute
        // CUP, exactly as recorded in paste-accept.vt.
        for byte in b"\x1b[?25l\x1b[3;1H\x1b[?25h" {
            session.feed_at(&[*byte], observed_at).unwrap();
        }
        assert_eq!(session.visible_cursor_logical_line(), Some((2, 2)));
        assert_eq!(
            session.advance_live_stability(observed_at + LIVE_MATH_STABLE_INTERVAL),
            0,
            "an explicit empty continuation line must extend the new-band gate to its source line"
        );
        assert!(session.inline_images.is_empty());
        assert!(session.take_decoration_worker_task().is_none());

        let entered_at = observed_at + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1);
        session.feed_at(b"\r\nPS> ", entered_at).unwrap();
        assert_eq!(
            session.advance_live_stability(entered_at + LIVE_MATH_STABLE_INTERVAL),
            0,
            "submitted multiline edit content remains permanently exempt"
        );
        assert!(session.inline_images.is_empty());
        assert!(session.take_decoration_worker_task().is_none());

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn lf_stream_tail_on_empty_line_does_not_suppress_preceding_formula() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));

        session.feed_at(b"$$streamed$$\n", started).unwrap();
        assert_eq!(session.visible_cursor_logical_line(), Some((1, 1)));
        assert_eq!(
            session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL),
            1,
            "a natural LF stream tail must not borrow the preceding output line into the gate"
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
            1
        );
        assert_eq!(session.live_decorations.len(), 1);
    }

    #[test]
    fn alternate_nonblank_input_line_does_not_suppress_output_above_it() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));

        session
            .feed_at(b"\x1b[?1049h$$output$$\x1b[8;1H> ", started)
            .unwrap();
        assert_eq!(session.visible_cursor_logical_line(), Some((7, 7)));
        assert_eq!(
            session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL),
            1,
            "a nonblank input row must keep the explicit-empty-line extension disabled"
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
            1
        );
        assert_eq!(session.live_decorations.len(), 1);
    }

    #[test]
    fn editor_cup_to_empty_line_suppresses_new_band_above_but_keeps_existing_band() {
        let started = Instant::now();
        let mut gated = DualPlaneSession::new(nz(10), nz(8));
        gated.feed_at(b"$$editing$$\x1b[4;1H", started).unwrap();
        assert!(
            gated.terminal.visible_row(0).unwrap().continues,
            "the source fixture must span one WRAPLINE-linked logical line"
        );
        assert_eq!(gated.visible_cursor_logical_line(), Some((3, 3)));
        assert_eq!(
            gated.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL),
            0,
            "CUP on an empty row must skip blank rows and gate the nearest nonblank WRAPLINE whole"
        );
        assert!(gated.take_live_worker_task().is_none());

        let mut existing = DualPlaneSession::new(nz(80), nz(8));
        existing.feed_at(b"$$editing$$\n", started).unwrap();
        existing.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut existing, synthetic_raster(32, 18)),
            1
        );
        let occurrence = existing
            .live_decorations
            .values()
            .next()
            .unwrap()
            .identity
            .occurrence_id;

        existing
            .feed_at(
                b"\x1b[2;1H",
                started + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(existing.live_decorations.len(), 1);
        assert_eq!(
            existing
                .live_decorations
                .values()
                .next()
                .unwrap()
                .identity
                .occurrence_id,
            occurrence,
            "cursor suppression gates creation only; it must not retire an existing band"
        );
    }

    #[test]
    fn explicit_empty_line_extension_survives_a_hidden_cursor_burst() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));
        session
            .feed_at(b"$$editing$$\x1b[2;1H\x1b[?25h", started)
            .unwrap();
        session
            .feed_at(b"\x1b[?25l", started + Duration::from_millis(1))
            .unwrap();

        assert!(!session.terminal.cursor().visible);
        assert_eq!(session.cursor_suppressed_logical_line(), Some((1, 1)));
        assert_eq!(
            session.advance_live_stability(
                started + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1)
            ),
            0,
            "the sticky line memory must carry the explicit CUP/HVP placement kind"
        );
        assert!(session.take_live_worker_task().is_none());
    }

    #[test]
    fn visible_cursor_input_line_suppresses_new_path_and_math_before_and_after_enter() {
        let (directory, path) = temporary_path_image();
        let started = Instant::now();

        let mut image = DualPlaneSession::new(nz(160), nz(8));
        enable_path_detection(&mut image);
        let image_line = format!("[Image: source: \"{}\"]", path.display());
        let image_typed_at = feed_ascii_one_byte_at_a_time(&mut image, &image_line, started);
        assert_eq!(image.visible_cursor_logical_line(), Some((0, 0)));
        assert_eq!(
            image.advance_live_stability(image_typed_at + LIVE_MATH_STABLE_INTERVAL),
            0
        );
        assert!(image.inline_images.is_empty());
        assert!(image.take_decoration_worker_task().is_none());

        let image_entered_at = image_typed_at + LIVE_MATH_STABLE_INTERVAL;
        image.feed_at(b"\r\nPS> ", image_entered_at).unwrap();
        assert_eq!(image.visible_cursor_logical_line(), Some((1, 1)));
        assert_eq!(
            image.advance_live_stability(image_entered_at + LIVE_MATH_STABLE_INTERVAL),
            0
        );
        assert!(image.inline_images.is_empty());
        assert!(image.take_decoration_worker_task().is_none());

        let mut math = DualPlaneSession::new(nz(80), nz(8));
        let math_typed_at = feed_ascii_one_byte_at_a_time(&mut math, "$$x+1$$", started);
        assert_eq!(math.visible_cursor_logical_line(), Some((0, 0)));
        assert_eq!(
            math.advance_live_stability(math_typed_at + LIVE_MATH_STABLE_INTERVAL),
            0
        );
        assert!(math.live_decorations.is_empty());
        assert!(math.take_live_worker_task().is_none());

        let math_entered_at = math_typed_at + LIVE_MATH_STABLE_INTERVAL;
        math.feed_at(b"\r\nPS> ", math_entered_at).unwrap();
        assert_eq!(
            math.advance_live_stability(math_entered_at + LIVE_MATH_STABLE_INTERVAL),
            0
        );
        assert!(math.take_live_worker_task().is_none());
        assert!(math.live_decorations.is_empty());

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn submitted_edit_line_stays_plain_while_distinct_image_and_math_output_render() {
        let (directory, path) = temporary_path_image();
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(240), nz(8));
        enable_path_detection(&mut session);
        let command = format!(
            "echo '[Image: source: \"{}\"] $$input_formula$$'",
            path.display()
        );
        let typed_at = feed_ascii_one_byte_at_a_time(&mut session, &command, started);
        assert_eq!(
            session.advance_live_stability(typed_at + LIVE_MATH_STABLE_INTERVAL),
            0,
            "the published edit line must first pass through cursor suppression"
        );

        let entered_at = typed_at + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1);
        let output = format!(
            "\r\n[Image: source: \"{}\"]\r\n$$output_formula$$\r\nPS> ",
            path.display()
        );
        session.feed_at(output.as_bytes(), entered_at).unwrap();
        assert_eq!(
            session.advance_live_stability(entered_at + LIVE_MATH_STABLE_INTERVAL),
            2,
            "only the distinct image output and formula output may create bands"
        );
        assert_eq!(
            session.inline_images.len(),
            1,
            "the command echo must not create a duplicate image occurrence"
        );
        let image_record = session.inline_images.values().next().unwrap();
        let InlineImageRecordKind::LocalPath { start_anchor, .. } = image_record.kind else {
            panic!("expected one local image output record");
        };
        assert!(matches!(
            session.document.anchor(start_anchor).unwrap(),
            ContentAnchor::Live {
                point: GridPoint { row: 1, .. },
                ..
            }
        ));
        let math_task = session.take_live_worker_task().unwrap();
        assert_eq!(math_task.span.original_source, "$$output_formula$$");
        assert!(session.take_live_worker_task().is_none());

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn submitted_edit_taint_migrates_to_frozen_id_without_tainting_equal_output_instance() {
        let (directory, path) = temporary_path_image();
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(240), nz(3));
        enable_path_detection(&mut session);
        let line = format!("echo '[Image: source: \"{}\"] $$same$$'", path.display());
        let typed_at = feed_ascii_one_byte_at_a_time(&mut session, &line, started);
        session.advance_live_stability(typed_at + LIVE_MATH_STABLE_INTERVAL);

        let entered_at = typed_at + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1);
        session
            .feed_at(
                format!("\r\n{line}\r\nfiller-1\r\nfiller-2\r\nfiller-3\r\n").as_bytes(),
                entered_at,
            )
            .unwrap();
        let equal_ids = session
            .document
            .entries()
            .iter()
            .filter_map(|(id, entry)| (entry.line.text == line).then_some(*id))
            .collect::<Vec<_>>();
        assert_eq!(
            equal_ids.len(),
            2,
            "the command echo and a byte-equal true output line must both freeze"
        );

        let frozen_image_ids = session
            .inline_images
            .values()
            .filter_map(|record| {
                let InlineImageRecordKind::LocalPath { start_anchor, .. } = record.kind else {
                    return None;
                };
                match session.document.anchor(start_anchor).ok()? {
                    ContentAnchor::History { id, .. } => Some(*id),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            frozen_image_ids,
            vec![equal_ids[1]],
            "taint belongs to the first line instance, never to its text globally"
        );
        let math_candidates = std::iter::from_fn(|| session.take_worker_task())
            .map(|task| task.candidate_id)
            .collect::<Vec<_>>();
        assert_eq!(math_candidates, vec![equal_ids[1]]);

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn rewriting_a_submitted_edit_row_releases_its_old_taint() {
        let (directory, path) = temporary_path_image();
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(240), nz(8));
        enable_path_detection(&mut session);
        let old = format!("echo '[Image: source: \"{}\"]'", path.display());
        let typed_at = feed_ascii_one_byte_at_a_time(&mut session, &old, started);
        let entered_at = typed_at + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1);
        session.feed_at(b"\r\nPS> ", entered_at).unwrap();

        let replacement = format!("[Image: source: \"{}\"]", path.display());
        session
            .feed_at(
                format!("\x1b[1;1H\x1b[2K{replacement}\x1b[3;1Houtput").as_bytes(),
                entered_at + Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(
            session.advance_live_stability(
                entered_at + Duration::from_millis(1) + LIVE_MATH_STABLE_INTERVAL
            ),
            1,
            "different content in the same grid row is a new line instance"
        );
        assert_eq!(session.inline_images.len(), 1);

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn soft_wrapped_path_is_suppressed_when_cursor_is_on_its_lower_half() {
        let (directory, path) = temporary_path_image();
        let mut session = DualPlaneSession::new(nz(20), nz(12));
        enable_path_detection(&mut session);
        let started = Instant::now();
        let line = format!("[Image: source: \"{}\"]", path.display());
        let typed_at = feed_ascii_one_byte_at_a_time(&mut session, &line, started);
        let cursor = session.terminal.cursor();
        let logical_line = session.visible_cursor_logical_line().unwrap();
        assert_eq!(logical_line.0, 0);
        assert_eq!(logical_line.1, cursor.row);
        assert!(cursor.row > 0, "fixture must soft-wrap onto a lower row");
        let stable = vec![true; session.live_rows.len()];
        let candidate = session.detected_live_image_paths(&stable).remove(0);
        assert!(candidate.start.row < cursor.row);
        assert!(candidate.end.row <= cursor.row);

        session.advance_live_stability(typed_at + LIVE_MATH_STABLE_INTERVAL);
        assert!(session.inline_images.is_empty());
        assert!(session.take_decoration_worker_task().is_none());

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn alternate_bottom_input_path_is_suppressed_while_output_path_renders() {
        let (directory, path) = temporary_path_image();
        let mut session = DualPlaneSession::new(nz(160), nz(8));
        enable_path_detection(&mut session);
        let started = Instant::now();
        let line = format!("[Image: source: \"{}\"]", path.display());
        session
            .feed_at(
                format!("\x1b[?1049h{line}\x1b[8;1H{line}").as_bytes(),
                started,
            )
            .unwrap();
        assert_eq!(session.visible_cursor_logical_line(), Some((7, 7)));
        assert_eq!(
            session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL),
            1
        );
        assert_eq!(session.inline_images.len(), 1);
        let SessionDecorationTask::InlineImage(task) =
            session.take_decoration_worker_task().unwrap()
        else {
            panic!("the upper output path must enqueue");
        };
        let record = session.inline_images.get(&task.occurrence_id).unwrap();
        let InlineImageRecordKind::LocalPath { start_anchor, .. } = record.kind else {
            panic!("expected a local path record");
        };
        assert!(matches!(
            session.document.anchor(start_anchor).unwrap(),
            ContentAnchor::Live {
                point: GridPoint { row: 0, .. },
                ..
            }
        ));
        let mut decoder = crate::inline_image::InlineImageDecoder::default();
        let result = decoder.decode(task.clone());
        assert!(session.complete_inline_image_result(task, result));
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert!(frame.math_blocks.iter().any(|placement| matches!(
            placement.artifact.kind,
            bt_viewport::RgbaArtifactKind::LocalImagePath { .. }
        )));

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn alternate_repaint_cursor_crossing_keeps_an_existing_math_band() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));
        session
            .feed_at(b"\x1b[?1049h$$x$$\r\nprompt", started)
            .unwrap();
        session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
            1
        );
        let occurrence = session
            .live_decorations
            .values()
            .next()
            .unwrap()
            .identity
            .occurrence_id;

        session
            .feed_at(
                b"\x1b[?2026h\x1b[2J\x1b[H$$x$$\x1b[?2026l",
                started + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(session.visible_cursor_logical_line(), Some((0, 0)));
        assert_eq!(session.live_decorations.len(), 1);
        let record = session.live_decorations.values().next().unwrap();
        assert_eq!(record.identity.occurrence_id, occurrence);
        assert!(record.artifact.is_some());
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
    fn worker_completion_rechecks_cursor_line_before_creating_a_record() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));
        session.feed_at(b"$$x$$\r\nprompt", started).unwrap();
        session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL);
        let mut task = session.take_live_worker_task().unwrap();
        assert!(resolve_live_detection_task(&mut task));

        session
            .feed_at(b"\x1b[1;1H", started + LIVE_MATH_STABLE_INTERVAL)
            .unwrap();
        assert!(session.complete_live_worker_result(task, Ok(synthetic_raster(32, 18))));
        assert!(session.live_decorations.is_empty());
        assert_eq!(session.live_rows[0].candidate_signature, None);

        session
            .feed_at(
                b"\x1b[2;1H",
                started + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1),
            )
            .unwrap();
        assert!(
            session.advance_live_stability(
                started + LIVE_MATH_STABLE_INTERVAL * 2 + Duration::from_millis(1)
            ) >= 1
        );
        assert!(session.take_live_worker_task().is_some());
    }

    #[test]
    fn hidden_cursor_does_not_suppress_full_screen_tui_math() {
        let started = Instant::now();
        let mut session = DualPlaneSession::new(nz(80), nz(8));
        session.feed_at(b"prompt", started).unwrap();
        session
            .feed_at(
                b"\x1b[2J\x1b[H\x1b[?25l$$x$$",
                started + Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(session.visible_cursor_logical_line(), None);
        assert_eq!(session.cursor_suppressed_logical_line(), None);
        assert_eq!(
            session.advance_live_stability(
                started + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1)
            ),
            1
        );
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(32, 18)),
            1
        );
    }

    #[test]
    fn local_path_image_keeps_source_text_and_appends_a_worker_decoded_band() {
        let (directory, path) = temporary_path_image();
        let mut session = DualPlaneSession::new(nz(160), nz(8));
        enable_path_detection(&mut session);
        let started = Instant::now();
        let line = format!("[Image: source: \"{}\"]", path.display());
        session
            .feed_at(format!("{line}\r\nprompt").as_bytes(), started)
            .unwrap();
        session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL);
        let SessionDecorationTask::InlineImage(task) =
            session.take_decoration_worker_task().unwrap()
        else {
            panic!("stable absolute image path must enqueue the decoration worker");
        };
        let record = session.inline_images.get(&task.occurrence_id).unwrap();
        let InlineImageRecordKind::LocalPath { start_anchor, .. } = record.kind else {
            panic!("candidate must retain local-path provenance");
        };
        let start = session.document.anchor(start_anchor).unwrap().clone();
        assert_eq!(
            session.decoded_local_image_path_at(&start),
            None,
            "pending text is not an activation capability"
        );
        let mut decoder = crate::inline_image::InlineImageDecoder::default();
        let result = decoder.decode(task.clone());
        assert!(session.complete_inline_image_result(task, result));
        assert_eq!(
            session.decoded_local_image_path_at(&start),
            Some(path.clone())
        );

        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(session.terminal().visible_text()[0], line);
        let placement = frame
            .math_blocks
            .iter()
            .find(|placement| {
                matches!(
                    placement.artifact.kind,
                    bt_viewport::RgbaArtifactKind::LocalImagePath { .. }
                )
            })
            .expect("decoded local image path must project");
        let source_row = frame
            .row_map
            .iter()
            .find(|row| row.live_grid_row == Some(0))
            .unwrap();
        assert!(
            placement.top_subpixels
                >= source_row
                    .top_subpixels
                    .saturating_add(session.cell_height_subpixels.get()),
            "image must begin below the still-visible path row: image_top={} row_top={} cell={} anchor={:?}",
            placement.top_subpixels,
            source_row.top_subpixels,
            session.cell_height_subpixels.get(),
            placement.anchor,
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn nonexistent_path_fails_quietly_and_never_becomes_clickable() {
        let mut session = DualPlaneSession::new(nz(120), nz(6));
        enable_path_detection(&mut session);
        let started = Instant::now();
        let missing = std::env::temp_dir().join(format!(
            "betterterminal-missing-{}-{}.png",
            std::process::id(),
            started.elapsed().as_nanos()
        ));
        let line = format!("[Image: source: \"{}\"]", missing.display());
        session
            .feed_at(format!("{line}\r\nprompt").as_bytes(), started)
            .unwrap();
        session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL);
        let SessionDecorationTask::InlineImage(task) =
            session.take_decoration_worker_task().unwrap()
        else {
            panic!("path candidate must be validated by the worker");
        };
        let mut decoder = crate::inline_image::InlineImageDecoder::default();
        let result = decoder.decode(task.clone());
        assert!(result.is_err());
        assert!(session.complete_inline_image_result(task, result));
        let record = session.inline_images.values().next().unwrap();
        let InlineImageRecordKind::LocalPath { start_anchor, .. } = record.kind else {
            panic!("expected local path record");
        };
        assert!(
            session
                .decoded_local_image_path_at(session.document.anchor(start_anchor).unwrap())
                .is_none()
        );
        assert!(record.failed);
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(session.terminal().visible_text()[0], line);
        assert!(frame.math_blocks.iter().all(|placement| !matches!(
            placement.artifact.kind,
            bt_viewport::RgbaArtifactKind::LocalImagePath { .. }
        )));
    }

    #[test]
    fn path_detection_disabled_is_a_strict_noop() {
        let mut session = DualPlaneSession::new(nz(100), nz(5));
        let started = Instant::now();
        session
            .feed_at(
                br#"[Image: source: "C:\machine-dependent\recording.png"]"#,
                started,
            )
            .unwrap();
        session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL);
        assert!(session.inline_images.is_empty());
        assert!(session.local_image_path_tasks.is_empty());
        assert!(session.take_decoration_worker_task().is_none());
    }

    #[test]
    fn alternate_repaint_reuses_the_ready_path_occurrence_without_flash_or_decode() {
        let (directory, path) = temporary_path_image();
        let mut session = DualPlaneSession::new(nz(160), nz(8));
        enable_path_detection(&mut session);
        let started = Instant::now();
        let line = format!("[Image: source: \"{}\"]", path.display());
        let enter = format!("\u{1b}[?1049h{line}\r\nprompt");
        session.feed_at(enter.as_bytes(), started).unwrap();
        session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL);
        let SessionDecorationTask::InlineImage(task) =
            session.take_decoration_worker_task().unwrap()
        else {
            panic!("alternate path must enqueue");
        };
        let mut decoder = crate::inline_image::InlineImageDecoder::default();
        let result = decoder.decode(task.clone());
        assert!(session.complete_inline_image_result(task, result));
        let key = session.inline_image_records()[0]
            .content_key
            .clone()
            .unwrap();

        let repaint = format!("\u{1b}[?2026h\u{1b}[2J\u{1b}[H{line}\u{1b}[?2026l");
        session
            .feed_at(
                repaint.as_bytes(),
                started + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(session.inline_image_records().len(), 1);
        assert!(session.take_decoration_worker_task().is_none());
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert!(frame.math_blocks.iter().any(|placement| {
            placement.artifact.key == key
                && matches!(
                    placement.artifact.kind,
                    bt_viewport::RgbaArtifactKind::LocalImagePath { .. }
                )
        }));

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn repeated_same_path_creates_distinct_bands_backed_by_one_cached_file_read() {
        let (directory, path) = temporary_path_image();
        let mut session = DualPlaneSession::new(nz(160), nz(8));
        enable_path_detection(&mut session);
        let started = Instant::now();
        let line = format!("[Image: source: \"{}\"]", path.display());
        session
            .feed_at(format!("{line}\r\n{line}\r\nprompt").as_bytes(), started)
            .unwrap();
        session.advance_live_stability(started + LIVE_MATH_STABLE_INTERVAL);
        let mut decoder = crate::inline_image::InlineImageDecoder::default();
        let mut tasks = Vec::new();
        while let Some(SessionDecorationTask::InlineImage(task)) =
            session.take_decoration_worker_task()
        {
            tasks.push(task);
        }
        assert_eq!(tasks.len(), 2);
        let first = decoder.decode(tasks[0].clone()).unwrap();
        std::fs::remove_file(&path).unwrap();
        let second = decoder.decode(tasks[1].clone()).unwrap();
        assert_eq!(first.key, second.key);
        assert_ne!(first.occurrence_id, second.occurrence_id);
        assert!(session.complete_inline_image_result(tasks[0].clone(), Ok(first)));
        assert!(session.complete_inline_image_result(tasks[1].clone(), Ok(second)));
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            frame
                .math_blocks
                .iter()
                .filter(|placement| matches!(
                    placement.artifact.kind,
                    bt_viewport::RgbaArtifactKind::LocalImagePath { .. }
                ))
                .count(),
            2
        );
        std::fs::remove_dir(&directory).unwrap();
    }
}
