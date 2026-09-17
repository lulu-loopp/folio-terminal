//! Conservative block-level `$$...$$` detection and the dual lifecycle/version gate.

mod ledger;
pub mod table;
pub use ledger::{
    ContainmentVerdict, LedgerEntry, LegitimateRejection, OrphanKind, OwnershipLedger,
    SourceIntegrityAnnotation, StructuralDelimiterKind, TokenFate,
};
use ledger::{OwnershipRecorder, source_line_of, structural_kind};

use std::{collections::BTreeMap, sync::Arc, time::Duration};

pub use bt_doc::{
    BlockKind, DecorationLifecycle, DetectionRevision, GridGeneration, GridPoint, InlineMathSite,
    InlineRunPlacement, LayoutKey, MathMode, SUBPIXELS_PER_PX, ScreenId, SourceLifecycle,
    VersionStamp, ViewGeneration,
};
use bt_doc::{DecorationIntent, HistoryDocument};
use bt_transcript::{SourceGeneration, TranscriptId};

pub const MAX_MATH_SOURCE_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DetectionOptions {
    /// Restore a row separator which Claude Code currently strips from the end of a logical line
    /// inside a LaTeX math environment. Set this to `false` once Claude Code emits the original
    /// `\\\\` faithfully; disabling it preserves renderer input byte-for-byte.
    pub restore_stripped_environment_newlines: bool,
    /// Restore a stripped separator inside a one-line tabular environment. The terminal session
    /// enables this only on primary: alternate-screen replay is a byte-pinned compatibility path.
    pub restore_stripped_inline_environment_newlines: bool,
    /// Reject a display-math candidate containing Claude Code's exact scroll-review overlay text.
    /// Disable this once Claude Code no longer writes that chip into terminal content rows.
    pub reject_claude_code_jump_chip_overlay: bool,
    /// The user-facing "Inline formulas" switch: may a lone `$…$` run become mathematics at all?
    ///
    /// This gates **detection**, and that is the one way it differs from its sibling
    /// `display_formulas` — which lives in bt-term, not here, because it is presentation-only: with
    /// display bands off the scanner still pairs `$$`, workers still rasterize, and records still
    /// hold their artifacts, so flipping it back on re-arms proven formulas from memory with no
    /// re-scan. Inline has no such downstream state worth preserving. An inline run that is never
    /// detected produces no record, no task and no raster, so gating it at the scanner costs
    /// nothing that turning it back on cannot rebuild from the next scan — and it means the switch
    /// genuinely silences the disambiguator rather than merely hiding its verdict.
    pub inline_formulas: bool,
}

impl Default for DetectionOptions {
    fn default() -> Self {
        Self {
            restore_stripped_environment_newlines: true,
            restore_stripped_inline_environment_newlines: true,
            reject_claude_code_jump_chip_overlay: true,
            inline_formulas: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DelimiterKind {
    Dollars,
    Brackets,
    Environment(String),
    /// A GFM pipe table's own "delimiter": the `|---|:--:|` row. It carries no
    /// payload because, unlike the three above, the row is not a token to be
    /// paired — it is the proof, and it has already been read by the time an
    /// occurrence exists.
    Table,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MathSourceLine {
    Transcript(TranscriptId),
    LiveGrid(u32),
}

/// Exact terminal-cell coverage for one physical source-row fragment of an occurrence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathCellSegment {
    pub logical_line: u32,
    pub source_line: MathSourceLine,
    pub byte_start: u32,
    pub byte_end: u32,
    pub cell_start: u32,
    pub cell_end: u32,
}

/// One proven math occurrence. Original terminal source and renderer input are deliberately
/// separate so copy/source presentation never has to reconstruct delimiters from render input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathOccurrence {
    pub byte_start: u32,
    pub byte_end: u32,
    pub original_source: String,
    pub render_source: String,
    pub delimiter_kind: DelimiterKind,
    pub mode: MathMode,
    /// Which renderer owns this occurrence. See [`BlockKind`] for why one field
    /// on one occurrence type is the whole of the difference.
    pub kind: BlockKind,
    pub cell_segments: Vec<MathCellSegment>,
    pub inline_runs: Vec<InlineMathRun>,
    /// The half of an inline formula that was left on the logical line *above* this one, when a
    /// producer's own line wrapping split one `$…$` across two rows (see
    /// [`detect_inline_math_across_rows`]). `None` for every occurrence that stands on one line,
    /// which is all of them but the joined ones.
    ///
    /// It is carried rather than resolved here because this crate has no coordinates: the fragment
    /// is named by its byte range inside the line above and by the exact text that was there when
    /// the join was proved, and the presentation layer — which alone knows which cells that line
    /// was drawn in — clears it under the picture.
    pub inline_joined_head: Option<InlineJoinedFragment>,
}

pub type MathSpan = MathOccurrence;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineMathRun {
    pub byte_start: u32,
    pub byte_end: u32,
    pub source: String,
}

/// The opening fragment of a row-split inline formula, as it sits on the line above.
///
/// `byte_start` points at the opening `$` itself and `byte_end` just past the last non-space
/// character of the fragment, so the range is exactly the cells the picture below makes redundant.
/// `text` is that range's content at the moment the join was proved: the presentation layer
/// compares it against what the line holds now and clears nothing if the two have diverged, which
/// is the only defence a two-line claim has against a full-screen application repainting one of
/// them between detection and paint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineJoinedFragment {
    pub byte_start: u32,
    pub byte_end: u32,
    pub text: String,
}

/// One row-split inline formula, as proved on the row that closes it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinedInlineRow {
    /// The fragment on the row above, for the placer to clear.
    pub head: InlineJoinedFragment,
    /// The closing row's runs: the joined formula first — its byte range is the head of this row,
    /// its source the two fragments joined — then every further run this row carries, re-paired
    /// from after the closing `$`.
    pub runs: Vec<InlineMathRun>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaceholderArtifact {
    pub key: String,
    /// Which renderer produced these pixels, carried so the presentation layer can answer the two
    /// questions that differ — whether an over-wide block is shrunk to fit or scrolled at full
    /// size, and which of the two switches on the Rendered blocks page decides it is drawn.
    pub kind: BlockKind,
    pub block_end: TranscriptId,
    pub height_subpixels: i64,
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
    pub baseline_subpixels: i64,
    pub mode: MathMode,
    pub render_time: Duration,
    /// For an inline composite: which of the span's runs this image contains, and where inside it.
    /// A run that did not fit its own source cells is absent, and its terminal text is what the
    /// user still sees. Empty for display math and for any placeholder.
    pub inline_runs: Vec<InlineRunPlacement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaleArtifact {
    pub artifact: PlaceholderArtifact,
    pub rendered_layout: LayoutKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectionTask {
    /// The newly frozen line which caused this scan. This remains stable after worker detection
    /// resolves a multi-line block to its opening line.
    pub candidate_id: TranscriptId,
    pub transcript_id: TranscriptId,
    pub block_end: TranscriptId,
    pub span: MathSpan,
    pub versions: VersionStamp,
    pub cell_width_subpixels: i64,
    pub cell_height_subpixels: i64,
    pub ascii_baseline_subpixels: i64,
    pub options: DetectionOptions,
    /// Exact parser checkpoint immediately before `inputs[0]`.
    pub initial_context: DetectionContext,
    pub inputs: Arc<[DetectionInput]>,
    pub resolved: bool,
    /// The header row of every table this scan refused whole. A block already drawn from the rows
    /// above a refusing line is no longer a table and the session takes it down; see
    /// [`RefusedTable`].
    pub refused_table_starts: Vec<TranscriptId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectionInput {
    pub id: TranscriptId,
    pub text: String,
    /// The width of the grid this line's own physical row was captured on, or zero when the line
    /// has no single such row — a logical line the terminal soft-wrapped out of several, or a
    /// fixture with no capture geometry at all. It is the provenance
    /// `bt_detect::table`'s wrap join runs on, and it is recorded rather than recomputed because
    /// the pane is resized and a frozen line is not.
    pub captured_columns: u32,
    /// UTF-8 byte boundary to terminal cell-column mappings from the captured logical line.
    pub cell_boundaries: Vec<(u32, u32)>,
    /// Where this frozen line sat in the shell's command lifecycle, captured from bt-term's OSC 133
    /// region bookkeeping **when the task was built**. The worker cannot work this out: it holds
    /// text and nothing else, and a line's site is a fact about the session that owns it. Carrying
    /// it here is what lets scrollback keep the inline verdict the live grid reached — without it
    /// the frozen scan passes `None`, every line reads [`InlineMathSite::Ineligible`], and a
    /// formula un-typesets itself the moment it scrolls off the grid.
    pub site: InlineMathSite,
}

/// Compact parser state immediately before a frozen line. The session retains one of these per
/// resident line so a viewport-local scan can begin at a proven neutral boundary without copying
/// the complete transcript prefix.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PrefixKnowledge {
    #[default]
    Known,
    Ambiguous,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DetectionContext {
    fence: Option<(char, usize)>,
    opening: Option<(TranscriptId, DisplayDelimiter)>,
    prefix: PrefixKnowledge,
}

impl DetectionContext {
    /// Return the earliest resident line needed to resolve `candidate`. `None` means the candidate
    /// is inside a proven code fence and therefore cannot begin or close a detectable math block.
    pub fn required_start(&self, candidate: TranscriptId) -> Option<TranscriptId> {
        self.fence
            .is_none()
            .then(|| self.opening.as_ref().map_or(candidate, |opening| opening.0))
    }

    /// A neutral state is a parser boundary: neither a code fence nor a display delimiter began
    /// before the next input line.
    pub fn is_neutral(&self) -> bool {
        self.prefix == PrefixKnowledge::Known && self.fence.is_none() && self.opening.is_none()
    }

    pub fn ambiguous() -> Self {
        Self {
            prefix: PrefixKnowledge::Ambiguous,
            ..Self::default()
        }
    }

    pub fn is_commonmark_code(&self) -> bool {
        self.fence.is_some()
    }
}

impl MathOccurrence {
    /// Cell segments are coordinate-system specific (live grid versus frozen transcript). Handoff
    /// identity therefore compares the source/render semantics and lets the destination retain its
    /// freshly detected segment map.
    pub fn render_equivalent(&self, other: &Self) -> bool {
        self.byte_start == other.byte_start
            && self.byte_end == other.byte_end
            && self.original_source == other.original_source
            && self.render_source == other.render_source
            && self.delimiter_kind == other.delimiter_kind
            && self.mode == other.mode
            && self.kind == other.kind
            && self.inline_runs == other.inline_runs
            && self.inline_joined_head == other.inline_joined_head
    }
}

/// A worker-owned snapshot of one stable live-grid run. Row numbers are grid coordinates, never
/// transcript identities; the authoritative detector maps them to temporary IDs only for the
/// duration of the shared `detect_math_blocks` call.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LiveDetectionSource {
    History { id: TranscriptId },
    Grid { row: u32, revision: u64 },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LiveDetectionInput {
    pub source: LiveDetectionSource,
    pub text: String,
    /// True when this physical row soft-wraps into the next input row.
    pub continues: bool,
    /// The width of the grid this physical row was captured on; zero when it carries no capture
    /// geometry. See [`DetectionInput::captured_columns`].
    pub captured_columns: u32,
    /// UTF-8 byte boundary to terminal cell-column mappings, including `(0, 0)` and the final
    /// source boundary. These come from captured terminal cells, never Unicode-width inference.
    pub cell_boundaries: Vec<(u32, u32)>,
    /// Where this physical row sits in the shell's command lifecycle, as bt-term's OSC 133 region
    /// bookkeeping reports it. Part of the input's identity — a row whose site changed is a row
    /// whose inline verdict may change, so a cached snapshot comparing equal must mean equal here
    /// too.
    pub site: InlineMathSite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveDetectionTask {
    pub candidate_row: u32,
    pub screen: ScreenId,
    pub grid_generation: GridGeneration,
    pub detection_revision: DetectionRevision,
    pub layout: LayoutKey,
    pub cell_width_subpixels: i64,
    pub cell_height_subpixels: i64,
    pub ascii_baseline_subpixels: i64,
    pub options: DetectionOptions,
    /// Exact parser checkpoint immediately before `inputs[0]`.
    pub initial_context: DetectionContext,
    pub inputs: Arc<[LiveDetectionInput]>,
    pub start: GridPoint,
    pub end: GridPoint,
    /// Inclusive live-grid row band reserved for presentation. Detection initializes this to the
    /// source span; the session may extend it over adjacent blank rows before rasterization.
    pub band_start_row: u32,
    pub band_end_row: u32,
    pub span: MathSpan,
    /// The shared scanner has examined this snapshot. `resolved == false` then means a proven
    /// non-occurrence, so a worker never rescans the same window per candidate.
    pub detection_complete: bool,
    pub resolved: bool,
    /// The first live-grid row of every table this scan refused whole. A live block already drawn
    /// from the rows above a refusing row is no longer a table and the session takes it down; see
    /// [`RefusedTable`].
    pub refused_table_rows: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectedMathBlock {
    pub start: TranscriptId,
    pub end: TranscriptId,
    pub span: MathSpan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecorationRecord {
    pub source: SourceLifecycle,
    pub decoration: DecorationLifecycle,
    pub versions: VersionStamp,
    pub artifact: Option<PlaceholderArtifact>,
    /// Old pixels are presentation-only while the same source is being laid out for a new DPI or
    /// width. Source/detector invalidation clears this slot immediately.
    pub stale_artifact: Option<StaleArtifact>,
    pub block_end: Option<TranscriptId>,
    pub span: Option<MathSpan>,
    pub show_source: bool,
    pub hovered: bool,
    pub horizontal_scroll_px: u32,
    pub vertical_scroll_px: u32,
    pub failure_reason: Option<String>,
}

impl DecorationRecord {
    pub fn frozen(versions: VersionStamp) -> Self {
        Self {
            source: SourceLifecycle::Frozen,
            decoration: DecorationLifecycle::None,
            versions,
            artifact: None,
            stale_artifact: None,
            block_end: None,
            span: None,
            show_source: false,
            hovered: false,
            horizontal_scroll_px: 0,
            vertical_scroll_px: 0,
            failure_reason: None,
        }
    }

    pub fn schedule(
        &mut self,
        transcript_id: TranscriptId,
        block_end: TranscriptId,
        span: MathSpan,
    ) -> Option<DetectionTask> {
        if self.source != SourceLifecycle::Frozen || self.decoration != DecorationLifecycle::None {
            return None;
        }
        self.decoration = DecorationLifecycle::Pending;
        Some(DetectionTask {
            candidate_id: transcript_id,
            transcript_id,
            block_end,
            span,
            versions: self.versions,
            cell_width_subpixels: SUBPIXELS_PER_PX,
            cell_height_subpixels: SUBPIXELS_PER_PX,
            ascii_baseline_subpixels: SUBPIXELS_PER_PX,
            options: DetectionOptions::default(),
            initial_context: DetectionContext::default(),
            inputs: Arc::from([]),
            resolved: true,
            refused_table_starts: Vec::new(),
        })
    }

    pub fn schedule_scan(
        &mut self,
        candidate_id: TranscriptId,
        initial_context: DetectionContext,
        inputs: Arc<[DetectionInput]>,
        options: DetectionOptions,
    ) -> Option<DetectionTask> {
        if self.source != SourceLifecycle::Frozen || self.decoration != DecorationLifecycle::None {
            return None;
        }
        self.decoration = DecorationLifecycle::Pending;
        Some(DetectionTask {
            candidate_id,
            transcript_id: candidate_id,
            block_end: candidate_id,
            span: MathSpan {
                byte_start: 0,
                byte_end: 0,
                original_source: String::new(),
                render_source: String::new(),
                delimiter_kind: DelimiterKind::Dollars,
                mode: MathMode::Display,
                kind: BlockKind::Math,
                cell_segments: Vec::new(),
                inline_runs: Vec::new(),
                inline_joined_head: None,
            },
            versions: self.versions,
            cell_width_subpixels: SUBPIXELS_PER_PX,
            cell_height_subpixels: SUBPIXELS_PER_PX,
            ascii_baseline_subpixels: SUBPIXELS_PER_PX,
            options,
            initial_context,
            inputs,
            resolved: false,
            refused_table_starts: Vec::new(),
        })
    }

    /// Worker results are never rewritten; every relevant version must still match.
    pub fn complete(&mut self, task: &DetectionTask, artifact: PlaceholderArtifact) -> bool {
        if self.source != SourceLifecycle::Frozen
            || self.decoration != DecorationLifecycle::Pending
            || task.versions != self.versions
        {
            return false;
        }
        self.artifact = Some(artifact);
        self.stale_artifact = None;
        self.block_end = Some(task.block_end);
        self.span = Some(task.span.clone());
        self.decoration = DecorationLifecycle::Ready;
        self.failure_reason = None;
        true
    }

    pub fn source_changed(&mut self, generation: SourceGeneration) {
        self.versions.source = generation;
        self.clear_content_dependent_state();
        self.decoration = DecorationLifecycle::None;
    }

    pub fn detector_changed(&mut self, revision: DetectionRevision) {
        self.versions.detection = revision;
        self.clear_content_dependent_state();
        self.decoration = DecorationLifecycle::None;
    }

    pub fn layout_changed(&mut self, layout: LayoutKey) {
        let rendered_layout = self.versions.layout;
        self.versions.layout = layout;
        if let Some(artifact) = self.artifact.take() {
            self.stale_artifact = Some(StaleArtifact {
                artifact,
                rendered_layout,
            });
        }
        if self.decoration != DecorationLifecycle::Suppressed {
            self.decoration = DecorationLifecycle::None;
        }
    }

    pub fn view_changed(&mut self, view: ViewGeneration) {
        self.versions.view = view;
        if self.decoration == DecorationLifecycle::Pending {
            self.decoration = DecorationLifecycle::None;
        }
    }

    pub fn suppress(&mut self) {
        if self.source == SourceLifecycle::Frozen {
            self.decoration = DecorationLifecycle::Suppressed;
            self.artifact = None;
            self.stale_artifact = None;
        }
    }

    pub fn fail(&mut self, task: &DetectionTask, reason: Option<String>) -> bool {
        if self.source != SourceLifecycle::Frozen
            || self.decoration != DecorationLifecycle::Pending
            || task.versions != self.versions
        {
            return false;
        }
        self.artifact = None;
        self.stale_artifact = None;
        self.block_end = Some(task.block_end);
        self.span = Some(task.span.clone());
        self.failure_reason = reason;
        self.decoration = if self.failure_reason.is_some() {
            DecorationLifecycle::Failed
        } else {
            DecorationLifecycle::Suppressed
        };
        true
    }

    pub fn toggle_source(&mut self) -> bool {
        if self.artifact.is_none() && self.stale_artifact.is_none() {
            return false;
        }
        self.show_source = !self.show_source;
        self.horizontal_scroll_px = 0;
        self.vertical_scroll_px = 0;
        self.failure_reason = None;
        true
    }

    fn clear_content_dependent_state(&mut self) {
        self.artifact = None;
        self.stale_artifact = None;
        self.block_end = None;
        self.span = None;
        self.show_source = false;
        self.hovered = false;
        self.horizontal_scroll_px = 0;
        self.vertical_scroll_px = 0;
    }
}

/// Detection is the owner of intent rebuilding. A viewport may only consume the resulting
/// revision; it must not impersonate redetection by clearing layout entries alone.
pub fn redetect_document(
    document: &mut HistoryDocument,
    revision: DetectionRevision,
) -> Vec<DetectedMathBlock> {
    document.clear_decorations();
    let mut detected = Vec::new();
    let inputs = document
        .entries()
        .iter()
        .map(|(id, entry)| (*id, entry.line.text.as_str()));
    for block in detect_math_blocks(inputs) {
        document.set_decoration(
            block.start,
            DecorationIntent::Math {
                byte_start: block.span.byte_start,
                byte_end: block.span.byte_end,
                mode: block.span.mode,
                detection_revision: revision,
            },
        );
        detected.push(block);
    }
    detected
}

pub fn detect_block_math(text: &str) -> Vec<MathSpan> {
    detect_math_blocks([(TranscriptId(1), text)])
        .into_iter()
        .map(|block| block.span)
        .collect()
}

fn is_math_environment(environment: &str) -> bool {
    matches!(
        environment,
        "equation"
            | "equation*"
            | "align"
            | "align*"
            | "alignat"
            | "alignat*"
            | "flalign"
            | "flalign*"
            | "gather"
            | "gather*"
            | "multline"
            | "multline*"
            | "aligned"
            | "alignedat"
            | "gathered"
            | "split"
            | "cases"
            | "matrix"
            | "pmatrix"
            | "bmatrix"
            | "Bmatrix"
            | "vmatrix"
            | "Vmatrix"
            | "smallmatrix"
    )
}

/// Conservatively detect one or more `$...$` runs on a single logical line.
///
/// The disambiguator has three independent gates, and a run must pass all of them. Any one of
/// them alone was measured to be insufficient (see the corpus in this module's tests):
///
/// * **A — site.** `site` must be [`InlineMathSite::CommandOutput`]. This is the structural gate
///   and it is checked first because it is the only one that is not about the text at all.
/// * **D — escapes and code.** `\$` is a literal dollar and never a delimiter (already the
///   meaning `delimiter_is_escaped` gives it for display math), `$$` belongs to display math and
///   is skipped here, and a line that is recognisably code — a diff hunk, a dated log line, a
///   command echo, anything carrying a backtick — is exempt wholesale.
/// * **content.** The span between the delimiters must read as a *complete* mathematical
///   expression: it must carry a math signal, it must not be prose, and it must not dangle (see
///   [`inline_source_is_complete`]).
///
/// The history this replaces is worth keeping. An independent review measured the previous
/// heuristic against 18 lines of ordinary terminal text and found 6 false positives —
/// `PATH=$HOME/bin:$PATH` rendered `HOME/bin:`, `WHERE a=$1 AND b=$2` rendered `1 AND b=`,
/// `Cost $5+$10` rendered `5+` — because any of `/ + - = >` inside the candidate counted as a
/// mathematical signal. Detection was then disabled outright rather than shipped wrong. Note what
/// the three survivors have in common and what gate actually stops them today: every one is a
/// *truncated* expression, cut off mid-operator by a second `$` that was never a closing
/// delimiter. Site alone does not save them — all three occur in genuine command output (a
/// `cat`-ed profile, a query log, a price list) — which is precisely why the completeness rule
/// exists alongside gate A rather than instead of it.
///
/// A terminal that renders your literal text has failed as a terminal, and that outranks the
/// convenience of inline rendering. Display `$$...$$` detection is unaffected: its paired
/// whole-line delimiters carry orders of magnitude more signal than a lone `$`.
pub fn detect_inline_math(text: &str, site: InlineMathSite) -> Vec<InlineMathRun> {
    if !site.permits_inline() {
        return Vec::new();
    }
    if inline_line_is_code_like(text) || text.len() > MAX_MATH_SOURCE_BYTES {
        return Vec::new();
    }
    inline_runs_from(text, 0)
}

/// The pairing half of [`detect_inline_math`], restricted to the `$` at or after `from`.
///
/// The site and gate-D checks are the caller's because they are facts about the whole line, and
/// they must not be re-asked of a suffix: a row whose remainder happens to begin `- ` is not a diff
/// hunk. Escape and identifier context is still read from the *whole* `text`, so a `$` at `from` is
/// judged by the character in front of it exactly as it would be in a full-line scan.
///
/// A non-zero `from` is what a row-split formula leaves behind: its closing `$` has already been
/// spent on the fragment above, and the rest of the row has to re-pair from after it or every
/// following formula on the row inherits the wrong parity (the second formula in the user's 2026-09-15
/// report was lost exactly that way).
fn inline_runs_from(text: &str, from: usize) -> Vec<InlineMathRun> {
    let dollars = text
        .char_indices()
        .filter_map(|(byte, character)| (character == '$' && byte >= from).then_some(byte))
        .collect::<Vec<_>>();
    let mut runs = Vec::new();
    let mut index = 0usize;
    while index < dollars.len() {
        let open = dollars[index];
        if delimiter_is_escaped(text, open)
            || text.as_bytes().get(open + 1) == Some(&b'$')
            || open
                .checked_sub(1)
                .is_some_and(|before| text.as_bytes().get(before) == Some(&b'$'))
            || open
                .checked_sub(1)
                .is_some_and(|before| byte_continues_an_identifier(text, before))
        {
            index += 1;
            continue;
        }
        let Some(close_index) = (index + 1..dollars.len()).find(|candidate| {
            let close = dollars[*candidate];
            !delimiter_is_escaped(text, close)
                && text.as_bytes().get(close + 1) != Some(&b'$')
                && !byte_continues_an_identifier(text, close + 1)
                && close
                    .checked_sub(1)
                    .is_none_or(|before| text.as_bytes().get(before) != Some(&b'$'))
        }) else {
            break;
        };
        let close = dollars[close_index];
        let source = &text[open + 1..close];
        if !source.is_empty()
            && !source.starts_with(char::is_whitespace)
            && !source.ends_with(char::is_whitespace)
            && inline_source_is_math(source)
            && inline_source_is_complete(source)
            && !block_body_looks_like_prose(source)
        {
            runs.push(InlineMathRun {
                byte_start: open as u32,
                byte_end: (close + 1) as u32,
                source: source.to_owned(),
            });
        }
        index = close_index + 1;
    }
    runs
}

/// How far into the closing row the closing `$` of a row-split formula may stand, in terminal
/// cells, for [`detect_inline_math_across_rows`] to join the two halves.
///
/// **Measured off the fixtures, not chosen for roundness.** The longest inline formula the corpora
/// carry is `$A=\begin{pmatrix}a & b \\ c & d\end{pmatrix}$` at 45 cells with its delimiters; the
/// Euler identity is 36, and the quadratic formula in the 2026-09-15 report is 40, of which 37
/// cells landed on the second row. A split can leave at most the whole formula on the closing row,
/// so the bound has to clear the longest whole formula — 48 does, with three cells to spare.
///
/// It is a bound on *locality*, not a length limit on mathematics: a formula longer than this is
/// not refused, it is only refused a **join**, and stays the source text it is today. That is the
/// right direction for a rule whose whole licence is that it never guesses — the further down a row
/// a `$` sits, the more ordinary text stands between it and the fragment above, and the less it can
/// be said to be that fragment's closer.
pub const INLINE_ROW_JOIN_MAX_CLOSING_COLUMNS: usize = 48;

/// Join an inline formula the producer's own line wrapping split across two rows.
///
/// **The defect.** Claude Code wraps its answers itself, at the pane width, with hard newlines of
/// its own rather than letting the terminal fold them. A sentence carrying `$x = \frac{-b \pm
/// \sqrt{b^2 - 4ac}}{2a}$` therefore arrives as two rows, the first ending `…the quadratic formula
/// $x` and the second beginning `= \frac{…}{2a}$, and the standard…`. Every gate in
/// [`detect_inline_math`] reads one row, so neither half is a formula: the first has an opener and
/// no closer, the second a closer and no opener. The user saw the Euler identity and the normal
/// density typeset (both landed whole on a row) with the quadratic formula left as raw text between
/// them (2026-09-15).
///
/// **What is proved here, and nothing more.** The row above must end in a single unmatched `$` —
/// *the only* `$` on it, not escaped, not `$$`, not a sigil glued to the end of a word, and followed
/// by a non-space. This row must carry the matching `$` within
/// [`INLINE_ROW_JOIN_MAX_CLOSING_COLUMNS`] cells, as its first `$`, with no other `$` before it.
/// Neither fragment may contain a second `$`, because a pair that could have been read two ways was
/// never proved. Both rows must sit at the same inline-eligible site — adjacent eligible rows with
/// nothing between them are one command's output, and a prompt row between two outputs is not
/// eligible, so requiring the site is how "the same command printed both halves" gets proved
/// without this crate knowing what a command is. And this row may not *begin* something: a bullet,
/// a quote, a heading or a numbered item is a new structure whose `$` belongs to its own sentence.
/// A blank row — the plainest new block of all, a paragraph break — needs no rule: it carries no
/// closing `$`, so there is nothing for the fragment above to be joined to.
///
/// The joined source — the two fragments with the row break replaced by a single space, which is
/// where the producer's wrap ate one — then faces **every content gate a one-row run faces**, in
/// the same order: a math signal, completeness, balanced brackets, not prose, no whitespace at
/// either end. That is what keeps the rule at zero false positives rather than merely few: it adds
/// a way for two fragments to *reach* the gates and relaxes none of them. `costs $5` above `and
/// three more` has nothing to close it; above `and Pro costs $15` the dollar below is glued to a
/// number and cannot close anything either; and where one below could close, `Basic $5` above `and
/// Pro (see above)$ monthly` produces `5 and Pro (see above)`, which is prose and dies exactly
/// where a one-row `$5 and Pro (see above)$` would.
///
/// The run this returns is anchored on the **closing** row, spanning its cells from column 0 to the
/// closing `$`: the picture has to stand somewhere, a picture cannot straddle two rows of a cell
/// grid, and the larger part of the formula — and all the room to draw it — is here. The fragment
/// left above travels with it as [`JoinedInlineRow::head`] so the placer can clear it.
pub fn detect_inline_math_across_rows(
    previous: &str,
    previous_site: InlineMathSite,
    text: &str,
    site: InlineMathSite,
) -> Option<JoinedInlineRow> {
    if !site.permits_inline() || previous_site != site {
        return None;
    }
    if previous.len() > MAX_MATH_SOURCE_BYTES || text.len() > MAX_MATH_SOURCE_BYTES {
        return None;
    }
    // The two delimiter questions first: they are one `$` pass each and a bounded column walk, and
    // they refuse nearly every pair of rows a terminal ever prints. The structural reads below walk
    // the lines, so they are asked only of rows that have a delimiter pair to argue about.
    //
    // Neither fragment can hold a second `$`, which is what makes the pair unambiguous: the opener
    // is the row above's *only* dollar, and the closer is the first one on this row, so the text
    // between them contains none.
    let open = lone_trailing_opener(previous)?;
    let close = first_inline_closer(text)?;
    if commonmark_indented_code(previous) || commonmark_indented_code(text) {
        return None;
    }
    if row_opens_a_block(text) {
        return None;
    }
    if inline_line_is_code_like(previous) || inline_line_is_code_like(text) {
        return None;
    }
    let head_body = previous.get(open + 1..)?.trim_end();
    let before_closer = text.get(..close)?;
    let tail_body = before_closer.trim_start();
    if tail_body.is_empty() {
        return None;
    }
    // The producer's wrap consumed exactly one space when it broke the line, so one space is what
    // puts the formula back. Joining with nothing would weld `\frac` onto the token above it.
    let source = format!("{head_body} {tail_body}");
    if source.starts_with(char::is_whitespace)
        || source.ends_with(char::is_whitespace)
        || !inline_source_is_math(&source)
        || !inline_source_is_complete(&source)
        || block_body_looks_like_prose(&source)
    {
        return None;
    }
    let head_end = open + 1 + head_body.len();
    let mut runs = vec![InlineMathRun {
        byte_start: 0,
        byte_end: u32::try_from(close + 1).ok()?,
        source,
    }];
    // The closing `$` is spent. Everything after it on this row re-pairs from a clean state, which
    // is how the second formula in the reported sentence survives the first one's split.
    runs.extend(inline_runs_from(text, close + 1));
    Some(JoinedInlineRow {
        head: InlineJoinedFragment {
            byte_start: u32::try_from(open).ok()?,
            byte_end: u32::try_from(head_end).ok()?,
            text: previous.get(open..head_end)?.to_owned(),
        },
        runs,
    })
}

/// What one pass over a line's dollars found: none, exactly one and where, or at least two.
///
/// Both inline questions a line is ever asked — "could it carry a pair of its own?" and "could it
/// close one left open above?" — are answered from this, so a line is scanned for `$` **once**.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DollarCensus {
    None,
    Lone(usize),
    Pair,
}

/// Take that one pass. Lazy and short-circuiting: it stops at the second `$` and never looks past
/// it, so a densely dollared line costs two `memchr`-shaped steps and not a walk of the line.
fn dollar_census(text: &str) -> DollarCensus {
    let mut dollars = text
        .bytes()
        .enumerate()
        .filter_map(|(index, byte)| (byte == b'$').then_some(index));
    match (dollars.next(), dollars.next()) {
        (None, _) => DollarCensus::None,
        (Some(only), None) => DollarCensus::Lone(only),
        (Some(_), Some(_)) => DollarCensus::Pair,
    }
}

/// Could this row take part in inline detection at all — by carrying a pair of `$` of its own, or
/// by closing a formula the row above left open?
///
/// **The arming prefilter's whole inline question, and the budget it keeps.** This runs on the
/// per-line path: once for every logical line of history a frame can see
/// (`schedule_visible_artifacts`), once for every logical row of the live grid
/// (`live_candidate_rows`), and once per line again for the live context signature. It must
/// therefore stay **one pass over the line's bytes and nothing else** — the same cost as the
/// two-dollar byte scan it replaced. Two rules keep it there and must survive any edit:
///
/// * one [`dollar_census`], never a second scan bolted on beside it; and
/// * no `text_width` over the line. [`closes_a_row_split`] measures columns with a walk that stops
///   at [`INLINE_ROW_JOIN_MAX_CLOSING_COLUMNS`], so a hundred-thousand-column line costs the same
///   as a short one. A plain `bt_unicode::text_width(&text[..close])` here is O(line) on a path
///   that is walked once per line per frame, which is how a per-line prefilter becomes a per-frame
///   cost.
///
/// The row-split half is deliberately the *tight* half of the join's own test, which is what keeps
/// it from arming a screen's worth of shell text: a lone `$` that could close something stands past
/// column 0 (a row beginning with one is opening, not closing), is not escaped, is not half of a
/// `$$`, and is **not followed by an identifier character** — that last one is the whole of
/// `$PATH`, `$1`, `$5`, `$BUILD_DIR` and every other sigil, which is to say nearly every lone
/// dollar a terminal ever prints.
///
/// A row this arms still has to survive [`detect_inline_math_across_rows`] in full, including
/// everything about the row above, which this cannot see. Arming is permission to ask, never an
/// answer.
#[must_use]
pub fn may_carry_inline_math(text: &str) -> bool {
    match dollar_census(text) {
        DollarCensus::None => false,
        // A pair is a run this row could prove on its own; nothing more needs asking.
        DollarCensus::Pair => true,
        DollarCensus::Lone(close) => closes_a_row_split(text, close),
    }
}

/// Could this row carry the closing half of a formula the row above left open?
///
/// **The frozen scan window's question, and it has to be the join's own.** The window asks this to
/// decide whether to reach one line further back than the candidate, and the row above is the only
/// place the opening fragment can be. So an answer narrower than
/// [`detect_inline_math_across_rows`] would give is not a conservatism, it is a silent loss: the
/// pair the detector is perfectly willing to join never reaches it, and the formula stays raw
/// source with nothing anywhere recording why. This is therefore not a second rule about closing
/// rows — it is [`first_inline_closer`], the join's own closer test, asked without the row above.
///
/// It once read the row's dollars as a census and demanded a *lone* one, which is the question
/// [`may_carry_inline_math`] asks and not the question the join asks. A closing row may carry
/// dollars of its own: the closer is the row's **first** `$`, and everything past it re-pairs from
/// a clean state, which is how `…the quadratic formula $x` over `= \frac{…}{2a}$, and the density
/// $\varphi(x) = e^{-x^2/2}$.` keeps both of its formulas. Under the census the second one made
/// the first one unreachable — the window opened at the closing row, the fragment above was never
/// in it, and the sentence that reported the split lost the very formula it reported (release
/// review 2026-09-17, X-6).
///
/// One `$` scan and the same bounded column walk: the budget on [`may_carry_inline_math`] holds
/// here unchanged.
#[must_use]
pub fn may_close_row_split_inline_math(text: &str) -> bool {
    first_inline_closer(text).is_some()
}

/// Can the `$` at `close` be read as the closer of a formula the row above left open?
///
/// Ordered cheapest first and bounded at the end: three constant-time byte tests, then a column
/// walk that stops the moment it passes the reach. Nothing here may become O(line) — see the budget
/// on [`may_carry_inline_math`].
fn closes_a_row_split(text: &str, close: usize) -> bool {
    close > 0
        && text.as_bytes().get(close + 1) != Some(&b'$')
        && !byte_continues_an_identifier(text, close + 1)
        && !delimiter_is_escaped(text, close)
        && closer_is_within_reach(text, close)
}

/// Does the `$` at `close` stand inside [`INLINE_ROW_JOIN_MAX_CLOSING_COLUMNS`] of the row's start?
///
/// **The bound lives here.** Width is monotonic across a prefix, so once the accumulated columns
/// reach the ceiling no later cluster can bring them back under it, and the walk stops — which
/// makes the cost a function of the reach and not of the line. It is the same measurement
/// `bt_unicode::text_width` makes, taken one cluster at a time so it can be abandoned.
///
/// A consequence worth stating because a test pins it: nothing past the reach can change the
/// verdict, because nothing past the reach is ever read.
fn closer_is_within_reach(text: &str, close: usize) -> bool {
    let Some(prefix) = text.get(..close) else {
        return false;
    };
    let mut column = 0usize;
    for cluster in bt_unicode::graphemes(prefix) {
        column = column.saturating_add(bt_unicode::cluster_width(cluster));
        if column >= INLINE_ROW_JOIN_MAX_CLOSING_COLUMNS {
            return false;
        }
    }
    true
}

/// The row's single `$`, when it is an unmatched opener at the end of the row.
///
/// *Single* is the whole of the conservatism. A row with two or more `$` has a reading in which
/// they pair with each other, and a fragment that could have been read another way is not proof of
/// anything — `tiers $5-$10-$20` must never contribute half of a formula to the row below it.
fn lone_trailing_opener(text: &str) -> Option<usize> {
    let DollarCensus::Lone(open) = dollar_census(text) else {
        return None;
    };
    if delimiter_is_escaped(text, open) {
        return None;
    }
    if open
        .checked_sub(1)
        .is_some_and(|before| byte_continues_an_identifier(text, before))
    {
        return None;
    }
    let after = text.get(open + 1..)?.chars().next()?;
    (!after.is_whitespace()).then_some(open)
}

/// The row's first `$`, when it can be read as the closer of a formula opened above.
///
/// The one closer rule there is. [`detect_inline_math_across_rows`] pairs with it and
/// [`may_close_row_split_inline_math`] arms the scan window with it, so the two sides of the join
/// cannot drift apart: whatever this refuses is never joined, and whatever it accepts is always
/// read with the row above it.
fn first_inline_closer(text: &str) -> Option<usize> {
    let close = text.find('$')?;
    closes_a_row_split(text, close).then_some(close)
}

/// Does this row *begin* something, rather than continue the row above?
///
/// A wrapped sentence resumes in plain prose. A bullet, a quotation, a heading or a numbered item
/// starts a new one, and its `$` is its own — so a dangling opener above it stays dangling. The
/// markers are the ones terminal producers actually draw, including the round bullets Claude Code
/// and other TUIs use, which [`delimiter_start`] already recognises for display math.
fn row_opens_a_block(text: &str) -> bool {
    let trimmed = text.trim_start();
    if ["• ", "◦ ", "▪ ", "● ", "- ", "* ", "+ ", "> "]
        .iter()
        .any(|marker| trimmed.starts_with(marker))
    {
        return true;
    }
    if trimmed.starts_with('#') {
        return true;
    }
    let digits = trimmed.len()
        - trimmed
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .len();
    digits > 0
        && trimmed
            .get(digits..)
            .is_some_and(|rest| rest.starts_with('.') || rest.starts_with(')'))
        && trimmed
            .get(digits + 1..)
            .is_some_and(|rest| rest.starts_with(' '))
}

/// Would the byte at `index` continue a shell identifier begun by an adjacent `$`?
///
/// This is the rule that tells `$a$b` from `$a$ b`, and it earns its place because the alt-screen
/// corpus caught the former rendering `a` as mathematics — a defect that was never alt-screen's,
/// since a `cat`-ed script printed to command output had always done the same. `$a$b` is one shell
/// expansion followed by another; the middle `$` is the *sigil of the second variable*, not the
/// closing delimiter of the first. Symmetrically an opening `$` glued to the end of a word
/// (`dir$SUFFIX`) is a sigil embedded mid-token, not an opener.
///
/// Deliberately ASCII-only, and the raw byte test is what makes that exact: a UTF-8 continuation
/// byte is never ASCII, so `能量$E$的值` — the ordinary Chinese habit of writing inline maths with
/// no surrounding space — keeps both of its delimiters. A rule that asked `char::is_alphanumeric`
/// would have cost that sentence its formula to defend against a shell syntax that cannot contain
/// a Han character in the first place.
///
/// The honest cost, stated because it is real: `the $n$th term` no longer renders. Two texts that
/// differ only in whether the letter after the closing `$` is a shell variable name or an English
/// suffix cannot be separated by any amount of looking, and when a rule must fail it fails toward
/// leaving the user's text alone.
fn byte_continues_an_identifier(text: &str, index: usize) -> bool {
    text.as_bytes()
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn inline_line_is_code_like(text: &str) -> bool {
    let trimmed = text.trim_start();
    let diff = trimmed.starts_with("+ ") || trimmed.starts_with("- ");
    let dated_log = trimmed.len() >= 11
        && trimmed.as_bytes().get(4) == Some(&b'-')
        && trimmed.as_bytes().get(7) == Some(&b'-')
        && trimmed
            .as_bytes()
            .get(10)
            .is_some_and(u8::is_ascii_whitespace);
    let shell = trimmed.starts_with("$ ")
        || trimmed.starts_with("echo ")
        || trimmed.starts_with("export ")
        || trimmed.starts_with("set ");
    diff || dated_log || shell || text.contains('`')
}

fn inline_source_is_math(source: &str) -> bool {
    let mut characters = source.chars();
    let first = characters.next();
    if first.is_some_and(char::is_alphabetic) && characters.next().is_none() {
        return true;
    }
    source.contains('\\')
        || source.chars().any(|character| {
            matches!(
                character,
                '^' | '_'
                    | '='
                    | '+'
                    | '-'
                    | '*'
                    | '/'
                    | '<'
                    | '>'
                    | '{'
                    | '}'
                    | '('
                    | ')'
                    | '['
                    | ']'
            )
        })
        || source
            .chars()
            .any(|character| ('\u{0370}'..='\u{03ff}').contains(&character))
}

/// Does this span read as a *whole* mathematical expression, rather than one cut in half?
///
/// This is the gate that separates `$a+b$` from the `5+` inside `Cost $5+$10`, and it is the
/// reason the disambiguator does not have to fall back on "does the line look shell-ish". The two
/// texts are indistinguishable by *signal* — both are short, both carry an operator, neither is
/// prose — and they are told apart by grammar instead: `a+b` is a complete expression, `5+` is an
/// expression with its right operand missing.
///
/// That asymmetry is not a coincidence of these examples, it is structural. When a `$` that was
/// meant as a currency mark or a shell sigil gets paired with the *next* such `$`, the text
/// captured between them is a fragment of running text that was severed at whatever character
/// happened to precede the second sigil — and running text severed at an arbitrary point lands on
/// a dangling operator, an unbalanced bracket or a trailing space far more often than it lands on
/// something that parses. Real inline math is bounded by delimiters its author chose, so it ends
/// where the expression ends.
///
/// Three conditions, each of which a truncated fragment fails and a real expression does not:
///
/// 1. No dangling operator at the tail. Nothing may *end* on a binary operator or relation —
///    `5+`, `1 AND b=`, `HOME/bin:` all die here.
/// 2. No dangling operator at the head, with unary `+`/`-` explicitly allowed so `$-x$` survives.
/// 3. Balanced brackets. `f(x` is not an expression; a fragment cut mid-call is.
///
/// Deliberately *not* a LaTeX parse. Parsing proves the fragment is well-formed LaTeX, which `5+`
/// very nearly is and `1 AND b=` is outright — MiTeX would happily typeset both. Well-formedness
/// is the wrong question; completeness is the right one.
fn inline_source_is_complete(source: &str) -> bool {
    /// Characters that need an operand on *both* sides. A span may neither open nor close on one.
    const INFIX: [char; 11] = ['+', '*', '/', '=', '<', '>', '^', '_', ',', ';', ':'];
    /// `-` is infix too, but it is also the unary minus, so it is legal at the head and only
    /// there. This is the one place the two ends of the span are allowed to disagree.
    const LEADING_SIGN: [char; 2] = ['+', '-'];

    let first = source.chars().next();
    let last = source.chars().next_back();
    let (Some(first), Some(last)) = (first, last) else {
        return false;
    };
    if INFIX.contains(&last) || last == '-' {
        return false;
    }
    if INFIX.contains(&first) && !LEADING_SIGN.contains(&first) {
        return false;
    }
    // A sign is only a sign if something follows it to be signed.
    if LEADING_SIGN.contains(&first) && source.chars().nth(1).is_none() {
        return false;
    }
    inline_brackets_balance(source)
}

/// Do `()`, `[]` and `{}` open and close in order across the span?
///
/// A LaTeX escape suppresses the very next character, so `\{` is a literal brace and not a group
/// opener — the same reading `delimiter_is_escaped` gives a `\$`.
fn inline_brackets_balance(source: &str) -> bool {
    let mut stack = Vec::new();
    let mut escaped = false;
    for character in source.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '(' | '[' | '{' => stack.push(character),
            ')' => {
                if stack.pop() != Some('(') {
                    return false;
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return false;
                }
            }
            '}' => {
                if stack.pop() != Some('{') {
                    return false;
                }
            }
            _ => {}
        }
    }
    stack.is_empty()
}

/// The OSC 133 site of the line at `index`, defaulting the only way a site may ever be defaulted.
///
/// A scan with no `sites` slice, or an index the slice does not reach, is a scan whose caller could
/// not state where the line sits. That is not the same as stating it is command output, and the
/// disambiguator's whole premise is that the absence of shell integration must cost inline
/// rendering rather than buy it — so the unknown case resolves to
/// [`InlineMathSite::Ineligible`] here, once, instead of at every call site.
fn site_at(sites: Option<&[InlineMathSite]>, index: usize) -> InlineMathSite {
    sites
        .and_then(|sites| sites.get(index))
        .copied()
        .unwrap_or(InlineMathSite::Ineligible)
}

/// Gather one line's proven `$…$` runs into a single inline occurrence.
///
/// Each run gets its **own** cell segment. The segments are what every downstream mapper works
/// from — `live_occurrence_segments` and `frozen_occurrence_segments` both walk them, and a span
/// that arrives with none maps to none and is silently dropped before it can ever be anchored
/// (the defect this replaces: `cell_segments: Vec::new()`). Per run rather than one segment
/// spanning first-open to last-close because the runs are separately anchored, separately
/// rasterized and separately allowed to fall back to source; a segment covering the prose between
/// two formulas would claim cells no formula owns.
///
/// `line` is the logical line the runs were found on, needed only for the provisional cell
/// columns. Those columns are **dead reckoning**: both mappers overwrite them from the terminal's
/// captured byte→column table before anything reads them, because only the terminal knows how wide
/// a character was drawn. The byte offsets are the load-bearing part — they must land on real
/// grapheme boundaries, which a `$` always does.
fn inline_group(
    id: TranscriptId,
    line: &str,
    runs: Vec<InlineMathRun>,
    joined_head: Option<InlineJoinedFragment>,
) -> Option<MathSpan> {
    let first = runs.first()?;
    let last = runs.last()?;
    let cell_segments = runs
        .iter()
        .map(|run| MathCellSegment {
            logical_line: 0,
            source_line: MathSourceLine::Transcript(id),
            byte_start: run.byte_start,
            byte_end: run.byte_end,
            cell_start: provisional_cell_column(line, run.byte_start),
            cell_end: provisional_cell_column(line, run.byte_end),
        })
        .collect();
    Some(MathSpan {
        byte_start: first.byte_start,
        byte_end: last.byte_end,
        original_source: runs
            .iter()
            .map(|run| run.source.as_str())
            .collect::<Vec<_>>()
            .join("; "),
        render_source: runs
            .iter()
            .map(|run| run.source.as_str())
            .collect::<Vec<_>>()
            .join("; "),
        delimiter_kind: DelimiterKind::Dollars,
        mode: MathMode::Inline,
        kind: BlockKind::Math,
        cell_segments,
        inline_runs: runs,
        inline_joined_head: joined_head,
    })
}

/// Char-count stand-in for a terminal column, matching what [`occurrence`] records for display
/// math. It is wrong for any wide character — `能` counts one and draws two — and that is
/// tolerable for exactly one reason: no consumer reads it. Every path from a scanner occurrence to
/// a placement runs through a mapper that replaces both columns with the terminal's own
/// `cell_boundaries` entry for the same byte. Left here so the two constructors agree.
fn provisional_cell_column(line: &str, byte: u32) -> u32 {
    let byte = usize::try_from(byte).unwrap_or(usize::MAX).min(line.len());
    let Some(prefix) = line.get(..byte) else {
        return 0;
    };
    u32::try_from(prefix.chars().count()).unwrap_or(u32::MAX)
}

/// Detect conservative block-level math over already-frozen logical lines. Fences are tracked
/// across lines; a multi-line delimiter must occupy its whole logical line. This deliberately
/// rejects shell, diff and log prose containing literal `$$` rather than trying to parse it.
/// Reject a `$$ ... $$` pairing whose body contains a line of ordinary prose.
///
/// The scanner sees a window of lines, not the whole stream, and on the alternate screen there is
/// no history to consult - so when a block's OPENING delimiter has scrolled out of view, the first
/// `$$` in view is really a CLOSING one. Treating it as an opener pairs it with the NEXT block's
/// opener and swallows everything between them, which is how a paragraph of Chinese ended up
/// typeset as mathematics (user report 2026-07-19).
///
/// Context completeness is the primary proof. The body check is independent defense in depth:
/// real display math does not contain a whole natural-language line unless a LaTeX command carries
/// it. Two multi-letter Latin words count as prose, while one-letter algebra such as `x = y` stays
/// valid. Refusing the whole block is the honest response when either proof fails.
fn block_body_looks_like_prose(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && !trimmed.contains('\\')
            && (trimmed.chars().any(is_cjk_prose_char) || ascii_line_looks_like_prose(trimmed))
    })
}

fn ascii_line_looks_like_prose(line: &str) -> bool {
    let mut words = 0usize;
    let mut multi_letter_words = 0usize;
    // A natural-language word is set off by whitespace and is an uninterrupted run of letters,
    // optionally wrapped in punctuation (`word,` / `(word)`). A whitespace token that mixes letters
    // with math connectives or digits — `ad-bc`, `mc^2`, `x_i`, `a=b`, `2ab` — is a math operand
    // group, not a prose word, and must not be counted. (One earlier version split on every
    // non-letter, so `=ad-bc` read as the two "words" `ad`/`bc`; the next trimmed every non-letter
    // off the ends, so the coefficient token `2ab` read as the word `ab` and a genuine
    // `$$…+ 2ab +…$$` block was rejected as prose.) Trim only outer punctuation — digits are
    // operand material, never word wrapping — then require the whole remaining core to be letters.
    for token in line.split_ascii_whitespace() {
        let core = token.trim_matches(|character: char| character.is_ascii_punctuation());
        if !core.is_empty() && core.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            words += 1;
            multi_letter_words += usize::from(core.len() > 1);
        }
    }
    multi_letter_words >= 2 || (multi_letter_words >= 1 && words >= 3)
}

fn is_cjk_prose_char(character: char) -> bool {
    matches!(character,
        '\u{3000}'..='\u{303f}'      // CJK punctuation
        | '\u{3400}'..='\u{4dbf}'    // CJK extension A
        | '\u{4e00}'..='\u{9fff}'    // CJK unified ideographs
        | '\u{f900}'..='\u{faff}'    // compatibility ideographs
        | '\u{ff00}'..='\u{ffef}'    // fullwidth forms
    )
}

type DisplayDelimiter = DelimiterKind;

/// What one line does to the structural state a scan carries from line to line — the code fence it
/// is inside, and the display delimiter it has open.
///
/// **There is one of these, and both readers of that state call it.** The authoritative scanner
/// keeps far more per line (body text, the index a block opened at, an ownership ledger, the
/// look-ahead witnesses that rescue a lost opener), but the part that decides *whether a line is
/// read as structure at all* is this, and it used to exist twice: once in `scan_math_blocks_impl`
/// and once, written out again, in `advance_detection_context`. The copy drifted — it never learned
/// the swallow-radius bound below — and a copy that drifts is a caller being told the parser is in a
/// state it is not in, which is how a picture ends up on a line the scanner disowns.
enum StructuralStep {
    /// Four columns of indentation with nothing open: CommonMark code, and no structure at all.
    IndentedCode,
    /// A fence marker. `fence` has been updated; the caller abandons whatever it had open.
    FenceMarker,
    /// A line inside an open fence.
    InsideFence,
    /// Ordinary structure — read the line. `abandon_opening` is the swallow-radius bound: an
    /// unfinished environment has met a display opener, and must be given up first.
    Read { abandon_opening: bool },
}

fn structural_line_step(
    text: &str,
    fence: &mut Option<(char, usize)>,
    opening: Option<&DisplayDelimiter>,
) -> StructuralStep {
    if opening.is_none() && commonmark_indented_code(text) {
        return StructuralStep::IndentedCode;
    }
    if opening.is_none()
        && let Some(marker) = commonmark_fence_marker(text)
    {
        match *fence {
            Some(active) if commonmark_fence_closes(text, active) => *fence = None,
            None => *fence = Some(marker),
            _ => {}
        }
        return StructuralStep::FenceMarker;
    }
    if fence.is_some() {
        return StructuralStep::InsideFence;
    }
    // Swallow-radius bound. A math *environment* body can never legally contain a `$$`/`\[` display
    // opener — those switch display mode and are a syntax error inside `\begin{env}…\end{env}`. So
    // when one appears while an environment opening is still unclosed, that environment's closer was
    // lost (mangled by a reflow, scrolled out of this window, or malformed), and continuing to
    // swallow would consume every following display block as phantom environment body (the
    // `\end{pmatrix},`-poisoning failure mode). The environment opening is given up, and the line is
    // then read by the ordinary opening-is-none paths under every existing guard — prose body,
    // escapes, CommonMark code, ambiguous-prefix pairing. This never fires for an active `$$`/`\[`
    // opening: inner `\begin`/`\end` directional environments are not display openers, so a
    // genuinely nested environment is still swallowed as body.
    let abandon_opening = matches!(opening, Some(DisplayDelimiter::Environment(_)))
        && opening_delimiter(text).is_some_and(|(kind, _)| {
            matches!(kind, DelimiterKind::Dollars | DelimiterKind::Brackets)
        });
    StructuralStep::Read { abandon_opening }
}

/// Advance the compact frozen-history parser proof by one immutable logical line. The structural
/// half is [`structural_line_step`], shared with the authoritative scanner; what is written out here
/// is only the delimiter pairing, which deliberately records no body text, so retaining checkpoints
/// is O(resident lines), not O(total source bytes squared).
pub fn advance_detection_context(context: &mut DetectionContext, id: TranscriptId, text: &str) {
    {
        let DetectionContext { fence, opening, .. } = &mut *context;
        match structural_line_step(text, fence, opening.as_ref().map(|(_, kind)| kind)) {
            StructuralStep::IndentedCode | StructuralStep::InsideFence => return,
            StructuralStep::FenceMarker => {
                *opening = None;
                return;
            }
            StructuralStep::Read { abandon_opening } => {
                if abandon_opening {
                    *opening = None;
                }
            }
        }
    }
    if let Some((delimiter, ..)) = complete_display_on_line(text) {
        if delimiter == DisplayDelimiter::Dollars
            && context
                .opening
                .as_ref()
                .is_some_and(|(_, active)| *active == DisplayDelimiter::Dollars)
        {
            context.opening = None;
        }
        return;
    }
    if context
        .opening
        .as_ref()
        .is_some_and(|(_, delimiter)| closing_delimiter(text, delimiter).is_some())
    {
        context.opening = None;
        return;
    }
    if context.opening.is_none()
        && let Some((delimiter, _)) = opening_delimiter(text)
        && (delimiter != DisplayDelimiter::Dollars || context.prefix == PrefixKnowledge::Known)
    {
        context.opening = Some((id, delimiter));
    }
}

pub fn detect_math_blocks<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
) -> Vec<DetectedMathBlock> {
    detect_math_blocks_with_options(lines, DetectionOptions::default())
}

pub fn detect_math_blocks_with_options<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    options: DetectionOptions,
) -> Vec<DetectedMathBlock> {
    scan_math_blocks_in_context_with_options(lines, DetectionContext::default(), options).blocks
}

/// [`detect_math_blocks_with_options`] for a caller that can state each line's OSC 133 site.
///
/// Every other entry point in this module scans lines alone, and a line alone cannot say whether a
/// command printed it — so they all resolve to [`InlineMathSite::Ineligible`] and detect display
/// math only. This is the one door inline `$…$` comes through, and it is deliberately narrow:
/// naming a site is an assertion about the terminal's semantic bookkeeping, and only the terminal
/// holds that.
pub fn detect_math_blocks_with_sites<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str, InlineMathSite)>,
    options: DetectionOptions,
) -> Vec<DetectedMathBlock> {
    let (lines, sites): (Vec<_>, Vec<_>) = lines
        .into_iter()
        .map(|(id, text, site)| ((id, text), site))
        .unzip();
    scan_math_blocks_impl(
        lines,
        DetectionContext::default(),
        options,
        Some(&sites),
        None,
        None,
        None,
        None,
        false,
        None,
    )
    .blocks
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmbiguousMathBlock {
    pub start: TranscriptId,
    pub end: TranscriptId,
    pub delimiter_kind: DelimiterKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MathScanResult {
    pub blocks: Vec<DetectedMathBlock>,
    pub ambiguous: Vec<AmbiguousMathBlock>,
    /// Every table candidate this window refused whole (`bt_detect::table` rule 2). A table drawn
    /// from rows that arrived earlier stands over lines this scan can now see are not its rows, so
    /// the caller has to take it down: the refusal is the only place that fact exists.
    pub refused_tables: Vec<RefusedTable>,
}

/// A header row over a delimiter row that a later line refused, and the lines it covered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefusedTable {
    /// The header row — the line a block built from these rows would be keyed at.
    pub start: TranscriptId,
    /// The last row the candidate could have accepted.
    pub end: TranscriptId,
    /// The line that carried the refusing pipe. It stands somewhere in the paragraph that runs on
    /// past `end` and need not be `end`'s neighbour: see `bt_detect::table` rule 2.
    pub refused_at: TranscriptId,
}

#[derive(Clone, Debug)]
struct ActiveOpening {
    start_index: Option<usize>,
    delimiter: DisplayDelimiter,
    body_start: usize,
}

/// One display-math scanner for single-line and multi-line forms. The supplied checkpoint is the
/// actual CommonMark/display parser state immediately before the first input line. An ambiguous
/// prefix suppresses only symmetric multi-line pairing; self-contained and directional forms are
/// still independently provable.
pub fn scan_math_blocks_in_context<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
) -> MathScanResult {
    scan_math_blocks_in_context_with_options(lines, initial_context, DetectionOptions::default())
}

pub fn scan_math_blocks_in_context_with_options<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
    options: DetectionOptions,
) -> MathScanResult {
    scan_math_blocks_impl(
        lines,
        initial_context,
        options,
        None,
        None,
        None,
        None,
        None,
        false,
        None,
    )
}

/// `live_grid_boundary` is the logical index of the first live-grid line when this scan spans a
/// frozen-history prefix followed by the live grid (primary live detection); `None` for every
/// frozen-only or self-contained context. When set, a `$$` opening that began in the frozen prefix
/// and would close on a live-grid `$$` is only honoured if the joined body is valid display math (a
/// genuine frozen/live bridge, per `0848375`). Otherwise the "opener" is a lost-closer poison from a
/// history reflow that left odd `$$` parity: consuming this grid `$$` as its closer would shift
/// every following grid block by one and strand the whole screen at source. In that case the stale
/// frozen opening is abandoned so the grid re-pairs from a clean state — exactly what a zoom reprint
/// achieves, done deterministically. It never fires for a pure-grid context (an alternate screen,
/// or a primary with empty history — `live_grid_boundary` is then `None`) nor for a block whose
/// opener is itself in the grid.
// The single authoritative scanner threads several orthogonal, independently-optional knobs (seam
// boundary, clip evidence, ownership recorder, frozen resync, final-phase readout); public callers
// reach it through the small purpose-named wrappers above, never this raw signature.
///
/// `sites` is the per-line OSC 133 lifecycle, indexed in parallel with the collected `lines`. It is
/// the structural half of the inline disambiguator and only bt-term can compute it, so every scan
/// that does not carry one reads as [`InlineMathSite::Ineligible`] — see [`site_at`].
///
/// `captured_columns` is the per-line capture geometry, indexed the same way: the width of the grid
/// each logical line's own physical row came off, and zero for a line that has no single such row.
/// Only bt-term can know it, and it is what `table`'s wrap join runs on, so a scan that does not
/// carry one joins nothing.
#[allow(clippy::too_many_arguments)]
fn scan_math_blocks_impl<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
    options: DetectionOptions,
    sites: Option<&[InlineMathSite]>,
    captured_columns: Option<&[u32]>,
    live_grid_boundary: Option<usize>,
    clipped_open_index: Option<u32>,
    mut recorder: Option<&mut OwnershipRecorder>,
    frozen_resync: bool,
    final_neutral: Option<&mut bool>,
) -> MathScanResult {
    let lines = lines.into_iter().collect::<Vec<_>>();
    let mut result = MathScanResult::default();
    // Frozen certified-boundary resync is only sound from a proven-neutral (`Known`) prefix: it is
    // the whole-history mirror of the live seam resync, resolving a lost-opener `$$` parity phantom
    // by the same body-invalid + forward-valid witness. Under an `Ambiguous` scrollback prefix the
    // direction of a symmetric `$$` is genuinely undecidable (M1.9p), so the resync must stay off and
    // the ambiguous opener is suppressed as it always has been — never re-paired on a guess.
    let frozen_resync = frozen_resync && initial_context.prefix == PrefixKnowledge::Known;
    let initial_fence = initial_context.fence;
    let mut fence = initial_fence;
    let mut opening = initial_context.opening.map(|(_, delimiter)| ActiveOpening {
        start_index: None,
        delimiter,
        body_start: 0,
    });
    if opening.is_some()
        && let Some(rec) = recorder.as_deref_mut()
    {
        rec.seed_carried_opening();
    }
    // The line inline detection last ran on, and only if it was the line immediately above this
    // one. Taken at the top of every iteration so that any path which skips the inline branch —
    // a code fence, an indented code block, a display block's body, a line inside an unclosed
    // opening — leaves no predecessor behind for a row-split join to reach across.
    let mut inline_carry: Option<usize> = None;
    for (index, (_, text)) in lines.iter().enumerate() {
        let inline_predecessor = inline_carry.take();
        // The structural half of this line, from the one place that decides it. The swallow-radius
        // verdict is taken here with the rest and applied at its own place below, so that the clip
        // resync still gets the first word and the ownership ledger still records the abandon
        // exactly where it always did.
        let abandon_environment = match structural_line_step(
            text,
            &mut fence,
            opening.as_ref().map(|active| &active.delimiter),
        ) {
            StructuralStep::IndentedCode => continue,
            StructuralStep::FenceMarker => {
                opening = None;
                continue;
            }
            StructuralStep::InsideFence => {
                if let Some(rec) = recorder.as_deref_mut() {
                    record_code_context_delimiter(rec, index, text);
                }
                continue;
            }
            StructuralStep::Read { abandon_opening } => abandon_opening,
        };
        // Row-0 clip resync (④ / evidence-driven ②). `clipped_open_index` is the decidable evidence
        // that the live grid's row 0 is inside a display block whose opener scrolled above grid row 0
        // (Codex's in-place scroll-region compression) and that the parser reached the frozen→live
        // seam in the CLOSED phase — so this first grid `$$` is really that block's CLOSER, not a
        // fresh opener. Reading it as an opener consumes it into a spurious forward pair and shifts
        // every following grid `$$` by one, stranding the whole screen below (the round-3
        // compress-rewrite topology). Consume it here as an above-window closer: a legitimate
        // occlusion whose opener is off-screen-up and unrenderable in this window (never synthesized,
        // never forced-closed globally, no source re-anchor — the M1.9p mirror of the `0848375`
        // frozen→live bridge). The grid then re-pairs from a clean closed state and every block below
        // — including one freshly streamed with no prior hold — is detected and rendered. Any active
        // opening at this exact index is an upstream-poison phantom (the clip evidence proved the seam
        // is closed); it is abandoned into the same account. This never fires for a pure-grid or
        // frozen-only context, nor when the seam carries a genuine opener (`clipped_open_index` is
        // then `None`), nor when grid row 0 is itself a valid opener.
        if clipped_open_index == u32::try_from(index).ok() {
            if let Some(rec) = recorder.as_deref_mut() {
                rec.close_rejected(
                    index,
                    delimiter_start(text),
                    StructuralDelimiterKind::Dollars,
                    LegitimateRejection::OpenerAboveWindow,
                );
            }
            opening = None;
            continue;
        }
        // The swallow-radius bound, decided by `structural_line_step` above and applied here, where
        // it has always been applied: the line is then handled by the normal opening-is-none paths
        // below (single-line complete block, or a fresh multi-line opener) under every existing
        // guard — prose body, escapes, CommonMark code, ambiguous-prefix pairing.
        if abandon_environment {
            opening = None;
            if let Some(rec) = recorder.as_deref_mut() {
                rec.abandon_pending(LegitimateRejection::EnvironmentSwallowAbandoned);
            }
        }
        // Frozen→live `$$` boundary resync. A Dollars opening whose opener lies in the frozen
        // prefix (or before the scanned window entirely) meeting its candidate closer on a
        // live-grid line is a genuine bridge only when the joined body is valid display math;
        // otherwise the opener is lost-closer poison (odd `$$` parity from a history reflow) and
        // consuming this grid `$$` would desync every later grid block. Abandon the stale opening
        // so this line re-enters the opening paths below as a fresh grid opener. Openers already in
        // the grid (`start_index >= boundary`) are ordinary grid blocks and are never abandoned.
        if let Some(boundary) = live_grid_boundary
            && index >= boundary
        {
            let abandon_stale_dollars = opening.as_ref().is_some_and(|active| {
                active.start_index.is_none_or(|start| start < boundary)
                    && phantom_opener_witness(&lines, active, index, text, options)
            });
            if abandon_stale_dollars {
                opening = None;
                if let Some(rec) = recorder.as_deref_mut() {
                    rec.abandon_pending(LegitimateRejection::PhantomOpenerAbandoned);
                }
            }
        }
        // Frozen certified-boundary resync (the FrozenResyncWitness gate, review §A). A frozen-only
        // scan has no live seam, so the live boundary guard above never fires; but a history reflow
        // that ate one `$$` opener leaves the same lost-opener parity phantom — a stale `$$` opening
        // whose body through its candidate closer is not valid math while that `$$` genuinely opens
        // the next block. The *same* body-invalid + forward-valid witness proves the phantom without
        // guessing direction. It runs only from a `Known` prefix (see `frozen_resync` above), so the
        // carried opening is always an in-window opener (`start_index` is `Some`) and the symmetric
        // `$$` ambiguity M1.9p forbids resolving never arises. Abandoning the phantom re-reads this
        // `$$` as a fresh opener, re-synchronising every block below it — the deterministic mirror of
        // the zoom reprint that rescues these blocks by hand.
        if frozen_resync {
            let abandon_phantom = opening.as_ref().is_some_and(|active| {
                active.start_index.is_some()
                    && phantom_opener_witness(&lines, active, index, text, options)
            });
            if abandon_phantom {
                opening = None;
                if let Some(rec) = recorder.as_deref_mut() {
                    rec.abandon_pending(LegitimateRejection::PhantomOpenerAbandoned);
                }
            }
        }
        if opening
            .as_ref()
            .is_some_and(|active| active.delimiter == DisplayDelimiter::Dollars)
            && let Some((delimiter, open_start, body_start, body_end, close_end)) =
                complete_display_on_line(text)
            && delimiter == DelimiterKind::Dollars
        {
            // A self-contained dollars block cannot close a prior abandoned dollars opener: doing
            // so would swallow its own opening token into the render body.
            opening = None;
            let body = &text[body_start..body_end];
            let original = &text[open_start..close_end];
            let render = restore_stripped_environment_newlines(
                body,
                options.restore_stripped_environment_newlines,
                options.restore_stripped_inline_environment_newlines,
            );
            let valid = valid_display_body(body, &render, options);
            if let Some(rec) = recorder.as_deref_mut() {
                // The pending opener loses its closer to this self-contained block: it is genuinely
                // unpaired (an odd-parity residue), not a legitimate rejection.
                rec.orphan_pending();
                rec.self_contained(
                    index,
                    open_start,
                    body_end,
                    StructuralDelimiterKind::Dollars,
                    StructuralDelimiterKind::Dollars,
                    valid,
                );
            }
            if valid {
                let id = lines[index].0;
                result.blocks.push(DetectedMathBlock {
                    start: id,
                    end: id,
                    span: occurrence(
                        &lines,
                        OccurrenceRange {
                            start_index: index,
                            end_index: index,
                            byte_start: open_start,
                            byte_end: close_end,
                        },
                        original.to_owned(),
                        render,
                        delimiter,
                        BlockKind::Math,
                    ),
                });
            }
            continue;
        }
        if opening.is_none()
            && let Some((delimiter, open_start, body_start, body_end, close_end)) =
                complete_display_on_line(text)
        {
            let body = &text[body_start..body_end];
            let original = &text[open_start..close_end];
            let render = if matches!(delimiter, DisplayDelimiter::Environment(_)) {
                original
            } else {
                body
            };
            let render = restore_stripped_environment_newlines(
                render,
                options.restore_stripped_environment_newlines,
                options.restore_stripped_inline_environment_newlines,
            );
            let valid = valid_display_body(body, &render, options);
            if let Some(rec) = recorder.as_deref_mut() {
                rec.self_contained(
                    index,
                    open_start,
                    body_end,
                    structural_kind(&delimiter, true),
                    structural_kind(&delimiter, false),
                    valid,
                );
            }
            if !valid {
                continue;
            }
            let id = lines[index].0;
            result.blocks.push(DetectedMathBlock {
                start: id,
                end: id,
                span: occurrence(
                    &lines,
                    OccurrenceRange {
                        start_index: index,
                        end_index: index,
                        byte_start: open_start,
                        byte_end: close_end,
                    },
                    original.to_owned(),
                    render,
                    delimiter,
                    BlockKind::Math,
                ),
            });
            continue;
        }
        if opening.is_none() && options.inline_formulas {
            let site = site_at(sites, index);
            // This line reached inline detection, so the next one may try to join back to it —
            // whatever this line's own verdict turns out to be. The head of a row-split formula
            // yields no run at all (it is an opener with no closer), which is exactly why the
            // carry is set on arrival here rather than on a detection.
            inline_carry = Some(index);
            let joined = inline_predecessor.and_then(|previous| {
                detect_inline_math_across_rows(
                    lines[previous].1,
                    site_at(sites, previous),
                    text,
                    site,
                )
            });
            let (runs, head) = match joined {
                Some(join) => (join.runs, Some(join.head)),
                None => (detect_inline_math(text, site), None),
            };
            if let Some(span) = inline_group(lines[index].0, text, runs, head) {
                let id = lines[index].0;
                result.blocks.push(DetectedMathBlock {
                    start: id,
                    end: id,
                    span,
                });
                continue;
            }
        }
        if let Some(active) = opening.as_ref()
            && let Some((body_end, close_end)) = closing_delimiter(text, &active.delimiter)
        {
            let active = opening.take().expect("active opening was just observed");
            let closer_kind = structural_kind(&active.delimiter, false);
            let Some(start_index) = active.start_index else {
                // The opener is before this bounded window. Its exact state proves that this is a
                // closer, but the missing source means there is no occurrence to render.
                if let Some(rec) = recorder.as_deref_mut() {
                    rec.close_rejected(
                        index,
                        body_end,
                        closer_kind,
                        LegitimateRejection::OpenerAboveWindow,
                    );
                }
                continue;
            };
            let body = joined_range(&lines, start_index, index, active.body_start, body_end);
            let original = joined_range(
                &lines,
                start_index,
                index,
                delimiter_start(lines[start_index].1),
                close_end,
            );
            let render = match active.delimiter {
                DisplayDelimiter::Environment(_) => restore_stripped_environment_newlines(
                    &original,
                    options.restore_stripped_environment_newlines,
                    options.restore_stripped_inline_environment_newlines,
                ),
                _ => restore_stripped_environment_newlines(
                    &body,
                    options.restore_stripped_environment_newlines,
                    options.restore_stripped_inline_environment_newlines,
                ),
            };
            let valid = valid_display_body(&body, &render, options);
            if let Some(rec) = recorder.as_deref_mut() {
                if valid {
                    rec.close_owned(index, body_end, closer_kind, start_index, index);
                } else {
                    rec.close_rejected(
                        index,
                        body_end,
                        closer_kind,
                        LegitimateRejection::GuardRejectedBody,
                    );
                }
            }
            if !valid {
                continue;
            }
            result.blocks.push(DetectedMathBlock {
                start: lines[start_index].0,
                end: lines[index].0,
                span: occurrence(
                    &lines,
                    OccurrenceRange {
                        start_index,
                        end_index: index,
                        byte_start: delimiter_start(lines[start_index].1),
                        byte_end: close_end,
                    },
                    original,
                    render,
                    active.delimiter,
                    BlockKind::Math,
                ),
            });
            continue;
        }
        if opening.is_some() {
            // Directional environments commonly nest inside an outer display block. Only the
            // delimiter which matches the active opener is structural at this level.
            continue;
        }
        let Some((delimiter, body_start)) = opening_delimiter(text) else {
            continue;
        };
        let open_byte = delimiter_start(text);
        if delimiter == DisplayDelimiter::Dollars
            && initial_context.prefix == PrefixKnowledge::Ambiguous
        {
            if let Some(rec) = recorder.as_deref_mut() {
                rec.reject_single(
                    index,
                    open_byte,
                    StructuralDelimiterKind::Dollars,
                    LegitimateRejection::AmbiguousPrefixSuppressed,
                );
            }
            result.ambiguous.push(AmbiguousMathBlock {
                start: lines[index].0,
                end: lines[index].0,
                delimiter_kind: delimiter,
            });
            continue;
        }
        if let Some(rec) = recorder.as_deref_mut() {
            rec.open(index, open_byte, structural_kind(&delimiter, true));
        }
        opening = Some(ActiveOpening {
            start_index: Some(index),
            delimiter,
            body_start,
        });
    }
    // The resync-corrected parser phase after the last line. A caller certifying a repair frontier
    // (session frozen scheduling, review §B) treats `opening.is_none() && fence.is_none()` as a
    // proven-neutral boundary: the scanned segment resolved to complete blocks with nothing left
    // open, so a window anchored just past it begins from a `Known` neutral state.
    if let Some(slot) = final_neutral {
        *slot = opening.is_none() && fence.is_none();
    }
    append_table_blocks(&lines, captured_columns, initial_fence, &mut result);
    result
}

/// Find every GFM pipe table in the same window the display-math scan just read, and append it to
/// the same result.
///
/// **A second walk rather than a branch inside the first, and that is the design.** The math loop
/// above is a delimiter state machine: it carries an opening across lines, abandons poisoned
/// openers, honours a clip witness, and feeds an ownership ledger — every one of those is a fact
/// about `$$`, and a table has none of them. A table is decided by two adjacent lines and nothing
/// else, which is a different shape of question and gets its own loop. What the two share is the
/// *context* they read in, and that is shared literally: the CommonMark fence state carried in and
/// tracked here is the same rule, so a table inside a ``` fence is no more a table than a `$$`
/// inside one is mathematics.
///
/// Math wins any overlap. It cannot happen with a well-formed input — a `$$` body is not a pipe
/// row over a delimiter row — but "cannot happen" is not a reason to leave two blocks claiming one
/// line, and the display scan is the older and stricter claim.
fn append_table_blocks(
    lines: &[(TranscriptId, &str)],
    captured_columns: Option<&[u32]>,
    initial_fence: Option<(char, usize)>,
    result: &mut MathScanResult,
) {
    if lines.len() < 2 {
        return;
    }
    // The geometry travels beside the text, one entry a line, exactly as `sites` does. A scan with
    // none — a probe, a certification pass, a fixture — reads every line as having no capture
    // geometry, which is the reading under which only the text can ask for a join.
    let candidate_lines: Vec<table::TableLine> = lines
        .iter()
        .enumerate()
        .map(|(index, (_, text))| {
            table::TableLine::on_grid(
                text,
                captured_columns
                    .and_then(|columns| columns.get(index).copied())
                    .unwrap_or(0),
            )
        })
        .collect();
    let texts: Vec<&str> = lines.iter().map(|(_, text)| *text).collect();
    let mut claimed = vec![false; lines.len()];
    for block in &result.blocks {
        for (index, (id, _)) in lines.iter().enumerate() {
            if *id >= block.start && *id <= block.end {
                claimed[index] = true;
            }
        }
    }
    let mut fence = initial_fence;
    let mut index = 0;
    while index < lines.len() {
        let text = texts[index];
        if let Some(marker) = commonmark_fence_marker(text) {
            match fence {
                Some(active) if commonmark_fence_closes(text, active) => fence = None,
                None => fence = Some(marker),
                _ => {}
            }
            index += 1;
            continue;
        }
        if fence.is_some() || claimed[index] || commonmark_indented_code(text) {
            index += 1;
            continue;
        }
        let Some(candidate) = table::table_at(&candidate_lines[index..]) else {
            index += 1;
            continue;
        };
        let span = match candidate {
            table::TableCandidate::Proven(span) => span,
            // Rule 2's whole refusal. Nothing is drawn and nothing is claimed, so every line of the
            // candidate stays text; the refusal is reported so a caller holding a block built from
            // these rows before the refusing line arrived can take it down. Scanning resumes at the
            // first line past the candidate, which is free to be a header of its own — the pipe
            // that refused this candidate may be several lines further down the same paragraph.
            table::TableCandidate::Refused {
                line_count,
                refused_at,
            } => {
                let end_index = index + line_count - 1;
                if !claimed[index..=end_index].iter().any(|it| *it) {
                    result.refused_tables.push(RefusedTable {
                        start: lines[index].0,
                        end: lines[end_index].0,
                        refused_at: lines[index + refused_at].0,
                    });
                }
                index += line_count;
                continue;
            }
        };
        let end_index = index + span.line_count - 1;
        if claimed[index..=end_index].iter().any(|it| *it) {
            index += 1;
            continue;
        }
        let original = joined_range(lines, index, end_index, 0, texts[end_index].len());
        result.blocks.push(DetectedMathBlock {
            start: lines[index].0,
            end: lines[end_index].0,
            span: occurrence(
                lines,
                OccurrenceRange {
                    start_index: index,
                    end_index,
                    byte_start: 0,
                    byte_end: texts[end_index].len(),
                },
                original,
                // The render source is the rows as this scan resolved them, one row to a line:
                // see `TableSpan::resolved_source`. It is what the painter reads, and it is why a
                // row this scan rejoined out of two printed lines is still one row after a resize.
                span.resolved_source(),
                DelimiterKind::Table,
                BlockKind::Table,
            ),
        });
        index = end_index + 1;
    }
    result.blocks.sort_by_key(|block| block.start);
}

/// Record a structural display delimiter that appears inside a CommonMark fenced code context as a
/// legitimate code-context rejection. Only the leading owns-the-line form is recognised (`$$`,
/// `\[`, `\]`); inline `$…$` inside code is never structural. Best-effort: unrecorded code-context
/// delimiters are inert and never affect the containment verdict.
fn record_code_context_delimiter(recorder: &mut OwnershipRecorder, index: usize, text: &str) {
    let trimmed = text.trim_start_matches(' ');
    let (kind, byte) = if trimmed.starts_with("$$") {
        (StructuralDelimiterKind::Dollars, text.len() - trimmed.len())
    } else if trimmed.starts_with(r"\[") {
        (
            StructuralDelimiterKind::BracketOpen,
            text.len() - trimmed.len(),
        )
    } else if trimmed.trim_end().ends_with(r"\]") {
        (
            StructuralDelimiterKind::BracketClose,
            text.trim_end().len() - 2,
        )
    } else {
        return;
    };
    recorder.reject_single(
        index,
        byte,
        kind,
        LegitimateRejection::CommonMarkCodeContext,
    );
}

/// The lost-opener parity-phantom witness (review §A `FrozenResyncWitness`), shared by the live
/// frozen→live seam resync and the frozen certified-boundary resync so both convergence rules stay
/// one predicate. `active` is a stale `$$` opening about to meet its candidate closer on `text` at
/// logical `index`. Returns true iff both decidable conditions hold, neither of which guesses the
/// `$$`'s direction. First, **body-invalid**: the joined body from the stale opening through this
/// closer is *not* valid display math (an empty/blank body, prose, CJK prose, oversize, or the Jump
/// chip). A genuine block never trips this, so the pairing being consumed here is spurious. Second,
/// **forward-valid**: re-reading this `$$` as a *fresh opener* pairs forward into a valid display
/// block, i.e. the real block starts here. This is the `3875209` convergence guard — if the `$$`
/// were instead the true closer of a straddling block whose body merely tripped the body check,
/// forward re-pairing would not yield a valid block and the opening is kept, so the single
/// unrenderable block drops itself and parity below is preserved. When both hold the stale opening
/// is a phantom and abandoning it lets `index` re-pair cleanly.
fn phantom_opener_witness(
    lines: &[(TranscriptId, &str)],
    active: &ActiveOpening,
    index: usize,
    text: &str,
    options: DetectionOptions,
) -> bool {
    active.delimiter == DisplayDelimiter::Dollars
        && closing_delimiter(text, &active.delimiter).is_some_and(|(body_end, _)| {
            let frozen_body_invalid = match active.start_index {
                Some(start) => {
                    let body = joined_range(lines, start, index, active.body_start, body_end);
                    let render = restore_stripped_environment_newlines(
                        &body,
                        options.restore_stripped_environment_newlines,
                        options.restore_stripped_inline_environment_newlines,
                    );
                    !valid_display_body(&body, &render, options)
                }
                None => true,
            };
            frozen_body_invalid && grid_dollars_opens_valid_block(lines, index, options)
        })
}

/// True when the grid `$$` at `index`, re-interpreted as a fresh display opener, pairs forward into
/// a valid display block. This is the signature of a lost-opener parity phantom: the frozen opening
/// carried into the grid was spurious, and this `$$` really opens the next block, so abandoning the
/// phantom lets the grid re-pair cleanly (the live-norender resync). When it is false, the `$$` is
/// instead the real closer of a straddling block and must be consumed as a closer so the blocks
/// below it stay in parity. `index` is at or past the frozen→live boundary, so `lines[index..]` is a
/// pure grid slice and the forward scan runs with the boundary guard disabled (no recursion, no
/// further resync). Grid rows are proven present, so the forward scan begins from a `Known` prefix.
fn grid_dollars_opens_valid_block(
    lines: &[(TranscriptId, &str)],
    index: usize,
    options: DetectionOptions,
) -> bool {
    let opener_id = lines[index].0;
    scan_math_blocks_impl(
        lines[index..].iter().copied(),
        DetectionContext::default(),
        options,
        // A forward display-validity probe, not a detection pass: it asks only whether this `$$`
        // opens a block that closes. No site means no inline run can enter the answer, and no
        // geometry means no table row joins the one under it — which is exactly the verdict this
        // probe has always produced.
        None,
        None,
        None,
        None,
        None,
        false,
        None,
    )
    .blocks
    .iter()
    .any(|block| block.start == opener_id)
}

fn valid_display_body(body: &str, render_source: &str, options: DetectionOptions) -> bool {
    if options.reject_claude_code_jump_chip_overlay && body.contains("Jump to bottom (ctrl+End)") {
        return false;
    }
    !body.trim().is_empty()
        && render_source.len() <= MAX_MATH_SOURCE_BYTES
        && !block_body_looks_like_prose(body)
}

#[derive(Clone, Copy)]
struct OccurrenceRange {
    start_index: usize,
    end_index: usize,
    byte_start: usize,
    byte_end: usize,
}

fn occurrence(
    lines: &[(TranscriptId, &str)],
    range: OccurrenceRange,
    original_source: String,
    render_source: String,
    delimiter_kind: DelimiterKind,
    kind: BlockKind,
) -> MathOccurrence {
    let cell_segments = (range.start_index..=range.end_index)
        .map(|index| {
            let line = lines[index].1;
            let start = if index == range.start_index {
                range.byte_start
            } else {
                0
            };
            let end = if index == range.end_index {
                range.byte_end
            } else {
                line.len()
            };
            MathCellSegment {
                logical_line: u32::try_from(index - range.start_index).unwrap_or(u32::MAX),
                source_line: MathSourceLine::Transcript(lines[index].0),
                byte_start: u32::try_from(start).unwrap_or(u32::MAX),
                byte_end: u32::try_from(end).unwrap_or(u32::MAX),
                cell_start: u32::try_from(line[..start].chars().count()).unwrap_or(u32::MAX),
                cell_end: u32::try_from(line[..end].chars().count()).unwrap_or(u32::MAX),
            }
        })
        .collect();
    MathOccurrence {
        byte_start: u32::try_from(range.byte_start).unwrap_or(u32::MAX),
        byte_end: u32::try_from(range.byte_end).unwrap_or(u32::MAX),
        original_source,
        render_source,
        delimiter_kind,
        mode: MathMode::Display,
        kind,
        cell_segments,
        inline_runs: Vec::new(),
        inline_joined_head: None,
    }
}

fn joined_range(
    lines: &[(TranscriptId, &str)],
    start_index: usize,
    end_index: usize,
    first_start: usize,
    last_end: usize,
) -> String {
    let mut parts = (start_index..=end_index)
        .map(|index| {
            let text = lines[index].1;
            let start = if index == start_index { first_start } else { 0 };
            let end = if index == end_index {
                last_end
            } else {
                text.len()
            };
            &text[start..end]
        })
        .collect::<Vec<_>>();
    if parts.first().is_some_and(|part| part.is_empty()) {
        parts.remove(0);
    }
    if parts.last().is_some_and(|part| part.is_empty()) {
        parts.pop();
    }
    parts.join("\n")
}

#[derive(Clone, Debug)]
struct MathEnvironmentRange {
    name: String,
    content_start: usize,
    close_start: usize,
}

/// Claude Code currently turns a LaTeX environment row separator (`\\\\`) into a bare trailing
/// backslash. A bare `\\` at a logical-line boundary is not a LaTeX command, so restoring its
/// missing mate is syntax recovery rather than a probabilistic content guess.
///
/// This function only sees detector-joined logical lines. Live-grid soft wraps have already been
/// merged before `joined_range` creates these `\n` boundaries. Original terminal source remains
/// untouched; this output is renderer input only.
fn restore_stripped_environment_newlines(
    source: &str,
    enabled: bool,
    inline_enabled: bool,
) -> String {
    if !enabled {
        return source.to_owned();
    }

    let mut stack = Vec::<(String, usize)>::new();
    let mut environments = Vec::<MathEnvironmentRange>::new();
    let mut byte = 0usize;
    while byte < source.len() {
        if source.as_bytes()[byte] != b'\\' || delimiter_is_escaped(source, byte) {
            byte += source[byte..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        if let Some((environment, token_len)) = environment_token(&source[byte..], true) {
            stack.push((environment, byte + token_len));
            byte += token_len;
            continue;
        }
        if let Some((environment, token_len)) = environment_token(&source[byte..], false)
            && stack
                .last()
                .is_some_and(|(active, _)| *active == environment)
        {
            let (_, content_start) = stack.pop().expect("matching environment is active");
            environments.push(MathEnvironmentRange {
                name: environment,
                content_start,
                close_start: byte,
            });
            byte += token_len;
            continue;
        }
        byte += 1;
    }
    if environments.is_empty() {
        return source.to_owned();
    }

    let mut insertions = Vec::new();
    let mut line_start = 0usize;
    while let Some(relative_newline) = source[line_start..].find('\n') {
        let newline = line_start + relative_newline;
        let line = &source[line_start..newline];
        let trimmed_end = line.trim_end().len();
        let slash = line_start + trimmed_end;
        let has_bare_trailing_slash = trimmed_end != 0
            && line.as_bytes()[trimmed_end - 1] == b'\\'
            && (trimmed_end == 1 || line.as_bytes()[trimmed_end - 2] != b'\\');
        let active_environment = environments
            .iter()
            .filter(|environment| {
                environment.content_start <= slash && slash <= environment.close_start
            })
            .max_by_key(|environment| environment.content_start);
        if has_bare_trailing_slash
            && active_environment.is_some_and(|environment| {
                source[newline + 1..environment.close_start]
                    .chars()
                    .any(|character| !character.is_whitespace())
            })
        {
            insertions.push(slash);
        }
        line_start = newline + 1;
    }

    // Claude Code also collapses a row separator inside a one-line environment, where there is no
    // logical-line boundary for the rule above to use. Recover only inside tabular math
    // environments and only at a row-shaped boundary, of which there are two. A **table row** is
    // proven by its ampersands: the current row already has an `&`, the following cell reaches
    // another `&`, and an apparent command after the slash is at most one bare cell character —
    // this repairs `a&b\c&d` / `a & b \ c & d`. A **one-column row** has no ampersand anywhere to
    // prove anything with, and is bounded instead by the slash standing on whitespace, or by one
    // bare cell character behind it — this repairs `x \ y`, `x\y` and `ax + by \ cx + dy`, the
    // column vectors that stood as single rows beside a 2x2 the first rule had already put right
    // (user report 2026-09-14). Neither touches a real command such as `\frac`, `\cos`, `\cdot` or
    // the `\,` `\;` `\:` `\!` spacing family. As above, terminal/source bytes remain exact; the
    // added slash exists only in renderer input and the workaround is controlled by the existing
    // switch.
    for environment in environments.iter().filter(|_| inline_enabled) {
        if !matches!(
            environment.name.as_str(),
            "array"
                | "align"
                | "align*"
                | "alignat"
                | "alignat*"
                | "flalign"
                | "flalign*"
                | "matrix"
                | "pmatrix"
                | "bmatrix"
                | "Bmatrix"
                | "vmatrix"
                | "Vmatrix"
                | "smallmatrix"
                | "cases"
                | "aligned"
                | "alignedat"
                | "gathered"
                | "split"
        ) {
            continue;
        }
        let mut row_start = environment.content_start;
        let mut byte = environment.content_start;
        while byte < environment.close_start {
            if source.as_bytes()[byte] != b'\\' || delimiter_is_escaped(source, byte) {
                byte += source[byte..].chars().next().map_or(1, char::len_utf8);
                continue;
            }
            if source[byte..].starts_with(r"\\") {
                row_start = byte + 2;
                byte += 2;
                continue;
            }
            let physical_line_start = source[row_start..byte]
                .rfind('\n')
                .map_or(row_start, |newline| row_start + newline + 1);
            let before = &source[physical_line_start..byte];
            let remaining = &source[byte + 1..environment.close_start];
            let after = remaining
                .find('\n')
                .map_or(remaining, |newline| &remaining[..newline]);
            let trimmed_after = after.trim_start();
            // Which cell follows the slash, and what proves where it ends.
            //
            // A row carrying an `&` is a table row, and the next `&` bounds the cell after the
            // slash — that is the boundary the rule above reads. A row with no `&` on either side
            // of the slash is a **one-column** row, and there is no ampersand anywhere in it to
            // read: `\begin{pmatrix} x \\ y \end{pmatrix}` is a column vector, and its cell runs to
            // the end of the row. Refusing that shape is what left a collapsed column vector
            // standing as a single row — `(x y)` where `(x / y)` was written — on the very screen
            // whose `\begin{pmatrix} a & b \\ c & d \end{pmatrix}` was repaired and came out 2x2
            // (user report 2026-09-14).
            //
            // The boundary such a row has instead is what a collapsed `\\` leaves behind: the slash
            // stands on whitespace (`x \ y`), or — the spaceless form the table rule already
            // accepts — what follows it is one bare cell character (`x\y`). That is what keeps the
            // spacing family out of it: `\,`, `\;`, `\:` and `\!` carry their own character
            // immediately behind the slash and leave more than one character of "cell" behind it,
            // so none of them can be read here as a row break. A `\alpha` or a `\frac` is refused
            // by the bare-cell test below, exactly as it is in a table row.
            let one_column_row = !before.contains('&') && !trimmed_after.contains('&');
            let next_cell = match trimmed_after.find('&') {
                Some(ampersand) => trimmed_after[..ampersand].trim(),
                None if one_column_row => trimmed_after.trim(),
                None => {
                    byte += 1;
                    continue;
                }
            };
            let one_column_boundary = one_column_row
                && (after.starts_with(char::is_whitespace) || next_cell.chars().count() == 1);
            let command_len = after
                .as_bytes()
                .iter()
                .take_while(|byte| byte.is_ascii_alphabetic())
                .count();
            let single_bare_cell =
                command_len <= 1 && (command_len == 0 || next_cell.chars().count() == 1);
            if (before.contains('&') || one_column_boundary)
                && !next_cell.is_empty()
                && single_bare_cell
                && !next_cell.contains(['\\', '{', '}'])
            {
                insertions.push(byte);
                row_start = byte + 1;
            }
            byte += 1;
        }
    }
    insertions.sort_unstable();
    insertions.dedup();
    if insertions.is_empty() {
        return source.to_owned();
    }

    let mut restored = String::with_capacity(source.len() + insertions.len());
    let mut copied = 0usize;
    for insertion in insertions {
        restored.push_str(&source[copied..insertion]);
        restored.push('\\');
        copied = insertion;
    }
    restored.push_str(&source[copied..]);
    restored
}

/// Byte offset where a display delimiter may begin on its line: leading spaces, then optionally
/// rendered CommonMark block markers followed by their spacing — one list-marker glyph (how TUI
/// markdown renderers such as Claude Code and Codex emit a list item) and/or one ATX heading
/// marker (Codex's resize reflow re-renders a `$$` opener as a `# $$` heading, observed stacked
/// as `• # $$`). A display block is valid list-item content and the heading form is the reflow's
/// mangling of one, so the markers do not deny the delimiter its owns-the-line status. Crucially,
/// refusing the opener here would desynchronise the whole message's `$$` pairing and let a later
/// closer pair across prose — every other guard (escapes, prose body, CommonMark code contexts,
/// pairing) runs unchanged.
fn delimiter_start(text: &str) -> usize {
    let spaces = text.len() - text.trim_start_matches(' ').len();
    let mut offset = spaces;
    let mut skipped_list = false;
    let mut skipped_heading = false;
    loop {
        let rest = &text[offset..];
        if !skipped_list
            && let Some(marker) = ["• ", "◦ ", "▪ ", "● "]
                .iter()
                .find(|marker| rest.starts_with(**marker))
        {
            let after = &rest[marker.len()..];
            offset += marker.len() + (after.len() - after.trim_start_matches(' ').len());
            skipped_list = true;
            continue;
        }
        if !skipped_heading {
            let hashes = rest.len() - rest.trim_start_matches('#').len();
            if (1..=6).contains(&hashes)
                && let Some(after) = rest.get(hashes..)
                && after.starts_with(' ')
            {
                offset += hashes + (after.len() - after.trim_start_matches(' ').len());
                skipped_heading = true;
                continue;
            }
        }
        return offset;
    }
}

fn complete_display_on_line(text: &str) -> Option<(DelimiterKind, usize, usize, usize, usize)> {
    if commonmark_indented_code(text) {
        return None;
    }
    let start = delimiter_start(text);
    let trimmed = text[start..].trim_end_matches([' ', '\t']);
    if let Some(rest) = trimmed.strip_prefix("$$") {
        let close = rest.len().checked_sub(2)?;
        if !rest.ends_with("$$") || close == 0 || rest[..close].contains("$$") {
            return None;
        }
        let body_start = start + 2;
        let body_end = body_start + close;
        return (!delimiter_is_escaped(text, start + trimmed.len() - 2)).then_some((
            DelimiterKind::Dollars,
            start,
            body_start,
            body_end,
            start + trimmed.len(),
        ));
    }
    if let Some(rest) = trimmed.strip_prefix(r"\[") {
        let close = rest.len().checked_sub(2)?;
        if !rest.ends_with(r"\]") || close == 0 || rest[..close].contains(r"\[") {
            return None;
        }
        return Some((
            DelimiterKind::Brackets,
            start,
            start + 2,
            start + 2 + close,
            start + trimmed.len(),
        ));
    }
    let (environment, open_end) = environment_token(trimmed, true)?;
    let closing = format!(r"\end{{{environment}}}");
    // Same prose-punctuation tolerance as the multi-line closer (`closing_delimiter`): a
    // single-line `\begin{pmatrix}…\end{pmatrix},` still pairs, with the trailing punctuation held
    // out of the rendered occurrence.
    let content_end = trimmed
        .trim_end_matches(is_trailing_prose_punctuation)
        .len();
    let body_end = content_end.checked_sub(closing.len())?;
    (open_end < body_end && trimmed[..content_end].ends_with(&closing)).then_some((
        DelimiterKind::Environment(environment),
        start,
        start + open_end,
        start + body_end,
        start + content_end,
    ))
}

fn opening_delimiter(text: &str) -> Option<(DelimiterKind, usize)> {
    if commonmark_indented_code(text) {
        return None;
    }
    let start = delimiter_start(text);
    let trimmed = &text[start..];
    if trimmed.starts_with("$$") && !delimiter_is_escaped(text, start) {
        return Some((DelimiterKind::Dollars, start + 2));
    }
    if trimmed.starts_with(r"\[") {
        return Some((DelimiterKind::Brackets, start + 2));
    }
    let (environment, open_end) = environment_token(trimmed, true)?;
    Some((DelimiterKind::Environment(environment), start + open_end))
}

fn closing_delimiter(text: &str, delimiter: &DelimiterKind) -> Option<(usize, usize)> {
    let trimmed_end = text.trim_end_matches([' ', '\t']).len();
    match delimiter {
        DelimiterKind::Dollars => {
            let start = trimmed_end.checked_sub(2)?;
            (text.get(start..trimmed_end) == Some("$$") && !delimiter_is_escaped(text, start))
                .then_some((start, trimmed_end))
        }
        DelimiterKind::Brackets => {
            let start = trimmed_end.checked_sub(2)?;
            (text.get(start..trimmed_end) == Some(r"\]")).then_some((start, trimmed_end))
        }
        // A table has no closing delimiter to look for: it ends at the first line that is not one
        // of its rows, which `table::table_at` has already decided.
        DelimiterKind::Table => None,
        DelimiterKind::Environment(environment) => {
            let closing = format!(r"\end{{{environment}}}");
            // A display environment closer is routinely followed by sentence punctuation when it
            // sits in prose (`\end{pmatrix},` / `\end{aligned}.` / CJK `\end{pmatrix}。`). Tolerate
            // that trailing run so the environment still pairs, but pin the closer's end at the
            // `}` — the punctuation is prose, not formula, and stays a native cell outside the
            // rendered occurrence. Anything other than a punctuation run after `\end{env}`
            // (e.g. `\end{pmatrix} extra`) still refuses to close.
            let content_end = text[..trimmed_end]
                .trim_end_matches(is_trailing_prose_punctuation)
                .len();
            let start = content_end.checked_sub(closing.len())?;
            (text.get(start..content_end) == Some(closing.as_str())).then_some((start, content_end))
        }
    }
}

/// Sentence/clause punctuation (ASCII and CJK full-width forms) that may trail a display
/// environment closer in prose without denying it its closing role. Deliberately excludes `$`,
/// `\`, `]`, `}` and every other structural glyph so it can never absorb a real delimiter or
/// command; only prose terminators are stripped.
fn is_trailing_prose_punctuation(character: char) -> bool {
    matches!(
        character,
        ',' | '.' | ';' | ':' | '!' | '?' | '，' | '。' | '、' | '；' | '：' | '！' | '？'
    )
}

fn environment_token(text: &str, open: bool) -> Option<(String, usize)> {
    let prefix = if open { r"\begin{" } else { r"\end{" };
    let rest = text.strip_prefix(prefix)?;
    let name_end = rest.find('}')?;
    let environment = &rest[..name_end];
    is_math_environment(environment)
        .then_some((environment.to_owned(), prefix.len() + name_end + 1))
}

fn commonmark_indented_code(text: &str) -> bool {
    let mut columns = 0usize;
    for character in text.chars() {
        match character {
            ' ' => columns += 1,
            '\t' => columns += 4 - columns % 4,
            _ => break,
        }
        if columns >= 4 {
            return true;
        }
    }
    false
}

fn commonmark_fence_marker(text: &str) -> Option<(char, usize)> {
    if commonmark_indented_code(text) {
        return None;
    }
    let trimmed = text.trim_start_matches(' ');
    let marker = fence_marker(trimmed)?;
    let suffix = trimmed.get(marker.1..)?;
    (marker.0 != '`' || !suffix.contains('`')).then_some(marker)
}

fn commonmark_fence_closes(text: &str, active: (char, usize)) -> bool {
    let trimmed = text.trim_start_matches(' ');
    let count = trimmed
        .chars()
        .take_while(|character| *character == active.0)
        .count();
    count >= active.1
        && trimmed
            .get(count..)
            .is_some_and(|suffix| suffix.chars().all(char::is_whitespace))
}

pub fn detect_math_blocks_in_context<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
) -> Vec<DetectedMathBlock> {
    detect_math_blocks_in_context_with_options(lines, initial_context, DetectionOptions::default())
}

pub fn detect_math_blocks_in_context_with_options<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
    options: DetectionOptions,
) -> Vec<DetectedMathBlock> {
    scan_math_blocks_in_context_with_options(lines, initial_context, options).blocks
}

/// Live-detection variant that knows the frozen→live seam so a `$$` opening straddling it can be
/// resynchronised (see `scan_math_blocks_impl`'s `live_grid_boundary`). `live_grid_boundary` is the
/// first live-grid logical index; `None` means the window is pure frozen history.
pub fn detect_live_math_blocks_in_context<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
    options: DetectionOptions,
    sites: Option<&[InlineMathSite]>,
    live_grid_boundary: Option<usize>,
    clipped_open_index: Option<u32>,
) -> Vec<DetectedMathBlock> {
    scan_live_math_blocks_in_context(
        lines,
        initial_context,
        options,
        sites,
        None,
        live_grid_boundary,
        clipped_open_index,
    )
    .blocks
}

/// The same scan, carrying the capture geometry in and the refused table candidates out. The live
/// resolver reads it because a refusal takes down a table the grid has already drawn; a caller that
/// only wants the blocks uses the wrapper above.
pub fn scan_live_math_blocks_in_context<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
    options: DetectionOptions,
    sites: Option<&[InlineMathSite]>,
    captured_columns: Option<&[u32]>,
    live_grid_boundary: Option<usize>,
    clipped_open_index: Option<u32>,
) -> MathScanResult {
    scan_math_blocks_impl(
        lines,
        initial_context,
        options,
        sites,
        captured_columns,
        live_grid_boundary,
        clipped_open_index,
        None,
        false,
        None,
    )
}

/// Frozen-history variant that resolves a lost-opener `$$` parity phantom left by a history reflow
/// (review §A). Unlike the live seam resync it has no `live_grid_boundary`; instead, from a proven
/// `Known` prefix, it applies the shared `phantom_opener_witness` at every candidate closer so an
/// eaten `$$` opener upstream no longer desynchronises every block below it. Under an `Ambiguous`
/// prefix the resync is inert (M1.9p): the scan is byte-identical to `detect_math_blocks_in_context`.
///
/// `sites` is the per-line OSC 133 lifecycle in scan order, as the session recorded it when the
/// task was built. `None` keeps the old reading — every line [`InlineMathSite::Ineligible`], so
/// display math only.
pub fn detect_frozen_math_blocks_in_context_with_options<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
    options: DetectionOptions,
    sites: Option<&[InlineMathSite]>,
) -> Vec<DetectedMathBlock> {
    scan_frozen_math_blocks_in_context_with_options(lines, initial_context, options, sites, None)
        .blocks
}

/// The same frozen scan, carrying the capture geometry in and the refused table candidates out.
pub fn scan_frozen_math_blocks_in_context_with_options<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
    options: DetectionOptions,
    sites: Option<&[InlineMathSite]>,
    captured_columns: Option<&[u32]>,
) -> MathScanResult {
    scan_math_blocks_impl(
        lines,
        initial_context,
        options,
        sites,
        captured_columns,
        None,
        None,
        None,
        true,
        None,
    )
}

/// Frozen resync scan that also reports whether the parser phase is neutral after the last line, so
/// the session can advance its certified repair frontier (review §B). `final_neutral` is true when
/// the scanned segment resolved to complete blocks with no delimiter left open — a proven boundary
/// from which the next window may begin in a `Known` neutral state.
pub fn frozen_resync_scan_with_options<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
    options: DetectionOptions,
) -> (Vec<DetectedMathBlock>, bool) {
    let mut final_neutral = false;
    let result = scan_math_blocks_impl(
        lines,
        initial_context,
        options,
        None,
        None,
        None,
        None,
        None,
        true,
        Some(&mut final_neutral),
    );
    (result.blocks, final_neutral)
}

/// Document-level detection red gate (the tool-gap the live-norender audit filed). Counts display
/// blocks that are provable from the live grid alone — a clean-context grid-only scan, exactly what
/// a zoom reprint re-detects — yet are ABSENT from the full history+grid detection. A nonzero value
/// is a silent detection desync: a poisoned frozen prefix (odd `$$` parity from a reflow) has
/// stranded on-screen blocks at source while the flash oracle — which derives "rendered" from
/// placement history and never sees a block that was never placed — stays green. Compared by render
/// source, so a block detected in both (bridged or plain) does not count. Returns `0` for a pure
/// grid context (no frozen prefix) since the two scans then coincide.
pub fn live_detection_isolation_gap(
    inputs: &[LiveDetectionInput],
    initial_context: DetectionContext,
    options: DetectionOptions,
) -> usize {
    let logical = live_logical_lines(inputs);
    let boundary = live_grid_boundary_index(&logical, inputs);
    let clipped = clipped_open_index(
        &logical,
        live_grid_first_index(&logical, inputs),
        &initial_context,
        options,
    );
    let full = detect_live_math_blocks_in_context(
        logical.iter().map(|line| (line.id, line.text.as_str())),
        initial_context,
        options,
        Some(&live_logical_sites(&logical)),
        boundary,
        clipped,
    );
    // A block the full context renders as a `0848375` frozen→live bridge carries its frozen opener
    // (and any leading body) as a PREFIX of its render source; the grid-only re-scan, lacking those
    // frozen rows, sees the same block as a bare-environment SUFFIX. Comparing exact render sources
    // would then flag every bridged block as "provable in isolation but absent from detection" — a
    // false strand, since the block is in fact rendered. A grid-only block is genuinely stranded
    // only when NO full block ends with it. (A real poison strands the block entirely: the full scan
    // yields nothing that ends with it — live-norender's 5 blocks stay counted.)
    let full_sources = full
        .iter()
        .map(|block| block.span.render_source.trim_end())
        .collect::<Vec<_>>();

    let grid_inputs = inputs
        .iter()
        .filter(|input| matches!(input.source, LiveDetectionSource::Grid { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let grid_logical = live_logical_lines(&grid_inputs);
    // Sites are deliberately withheld from the isolation re-scan: this gate counts *display*
    // structure the grid proves in isolation but the full context lost, and a siteless scan yields
    // exactly that set. Feeding it sites could only add inline blocks, which the full scan may
    // legitimately suppress under a carried opening — a difference that is not the strand this gate
    // exists to catch.
    let isolation = detect_math_blocks_in_context_with_options(
        grid_logical
            .iter()
            .map(|line| (line.id, line.text.as_str())),
        DetectionContext::default(),
        options,
    );
    isolation
        .iter()
        .filter(|block| {
            let grid_source = block.span.render_source.trim_end();
            !full_sources.iter().any(|full| full.ends_with(grid_source))
        })
        .count()
}

/// Build the token-ownership ledger for one reconstructed live region (frozen prefix + live grid).
/// This re-runs the authoritative `scan_math_blocks_impl` with the recorder attached — the same
/// control flow the product uses with `None` — so every fate is the one the real detector assigned,
/// never a second heuristic. The result feeds the split source-integrity / detector-containment red
/// gate (batch ⑥) and, via its lineage and dependency-interval fields, the certified-checkpoint work
/// of batch 2.
pub fn live_detection_ownership_ledger(
    inputs: &[LiveDetectionInput],
    initial_context: DetectionContext,
    options: DetectionOptions,
) -> OwnershipLedger {
    let logical = live_logical_lines(inputs);
    let boundary = live_grid_boundary_index(&logical, inputs);
    let clipped = clipped_open_index(
        &logical,
        live_grid_first_index(&logical, inputs),
        &initial_context,
        options,
    );

    // Lineage: a logical line's source row is its first physical fragment's input source.
    let source_of = |index: u32| -> Option<MathSourceLine> {
        logical.get(index as usize).and_then(|line| {
            line.fragments
                .first()
                .and_then(|fragment| inputs.get(fragment.input_index))
                .map(source_line_of)
        })
    };

    let mut recorder = OwnershipRecorder::default();
    let result = scan_math_blocks_impl(
        logical.iter().map(|line| (line.id, line.text.as_str())),
        initial_context,
        options,
        Some(&live_logical_sites(&logical)),
        Some(&live_logical_captured_columns(&logical)),
        boundary,
        clipped,
        Some(&mut recorder),
        false,
        None,
    );
    let mut ledger = recorder.finish(boundary, source_of, clipped);
    // Batch ③: carry the display blocks the same scan Owned, keyed by the exact `original_source` the
    // presentation layer preserves holds on. Inline `$…$` runs never enter a hold, so they are
    // excluded here — this vector is exactly the Owned structural-display set (`ledger.detected()`).
    ledger.owned_block_sources = result
        .blocks
        .iter()
        .filter(|block| block.span.mode == MathMode::Display)
        .map(|block| block.span.original_source.clone())
        .collect();
    ledger
}

/// Detect the round-3 clipped-open topology: the live grid's row 0 is inside a display block body
/// whose opener scrolled above it, and the scanner reaches the seam in the closed phase (no opener
/// carried). Returns the logical index of the first grid `$$` — really the clipped block's closer —
/// so the ledger can mark it a `ClippedOpen` orphan. Decidable purely from the reconstructed rows
/// and the parser phase at the boundary; hold-independent, exactly what `isolation_gap` cannot see.
///
/// `first_grid` is [`live_grid_first_index`] and **not** the frozen→live seam: whether a screen
/// begins inside a block has nothing to do with whether any frozen line stands in front of it, and
/// keying the evidence to the seam left the one screen where this topology arises on its own —
/// a full-screen program's alternate screen, which owns its window and moves its content by
/// redrawing rather than scrolling, so no row is ever removed and nothing advances that screen's
/// context — unable to state it at all (§4.6b).
fn clipped_open_index(
    logical: &[LiveLogicalLine],
    first_grid: Option<usize>,
    initial_context: &DetectionContext,
    options: DetectionOptions,
) -> Option<u32> {
    let b = first_grid?;
    // Parser phase at the seam. If a Dollars opener is carried in (`opening.is_some()`) the block's
    // opener is accounted above the window (a genuine bridge / carry, not a clip); if inside a code
    // fence there is no structural math. Only a CLOSED phase at the seam can misread a clipped
    // closer as a fresh opener.
    let mut context = initial_context.clone();
    for line in &logical[..b] {
        advance_detection_context(&mut context, line.id, &line.text);
    }
    if context.opening.is_some() || context.fence.is_some() {
        return None;
    }
    // The first grid `$$` must be a lone closer-shaped delimiter, and it must be preceded by grid
    // body rows (row 0 is mid-body). A grid whose first `$$` sits at row 0, or is a self-contained
    // `$$…$$`, is an ordinary grid opener, not a clip.
    let first_dollars = (b..logical.len()).find(|&i| {
        let text = logical[i].text.as_str();
        let start = delimiter_start(text);
        text[start..].trim_end() == "$$"
    })?;
    if first_dollars == b {
        return None;
    }
    // No display opener may appear among the body rows before it (that would put the opener in the
    // grid), and those rows must form a valid display body (real math, not prose/blank) — the proof
    // that row 0 is genuinely inside a block.
    if (b..first_dollars).any(|i| opening_delimiter(logical[i].text.as_str()).is_some()) {
        return None;
    }
    // Nor may a CommonMark fence open among them. The scanner skips a fenced region whole, so a
    // `$$` reached inside one is inert text and the rows above it are code, not a block body — and
    // `valid_display_body` cannot say so, because a bare fence marker is a single token with no
    // whitespace in it and reads as math rather than prose.
    if (b..first_dollars).any(|i| commonmark_fence_marker(logical[i].text.as_str()).is_some()) {
        return None;
    }
    let body = logical[b..first_dollars]
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !valid_display_body(&body, &body, options) {
        return None;
    }
    // The convergence guard, the same one the two resyncs use: a symmetric `$$` is never re-read on
    // a guess, only when the reading in force is provably broken. Reading this one as an ordinary
    // opener is the reading in force, so if it pairs forward into a valid block there is nothing to
    // repair and the clip must stay inert — whatever the rows above happen to look like. A genuine
    // clip fails it by construction: the `$$` after a clipped closer is the *next* block's opener,
    // so pairing forward from the closer encloses the prose between them and is refused. Without
    // this, one row of ordinary text above a block — a single word, which is not enough whitespace
    // to read as prose — was enough to eat that block's opening `$$` as an above-window closer.
    let lines = logical
        .iter()
        .map(|line| (line.id, line.text.as_str()))
        .collect::<Vec<_>>();
    (!grid_dollars_opens_valid_block(&lines, first_dollars, options))
        .then_some(first_dollars as u32)
}

/// Run the authoritative detector on a worker-owned frozen snapshot. The session thread only
/// chooses a cheap `$$` candidate and never calls this while ingesting a finalized line.
pub fn resolve_detection_task(task: &mut DetectionTask) -> bool {
    if task.resolved {
        return true;
    }
    // The frozen worker scan applies the certified-boundary resync (review §A/§B): the session
    // anchors this window at a proven-neutral frontier, so an eaten `$$` opener upstream is
    // abandoned by the shared witness rather than shifting every following block's pairing by one.
    // For a clean window the resync is a no-op and the result is byte-identical to the ordinary
    // frozen scan; under an `Ambiguous` prefix (a cap-bounded fallback window) it is inert (M1.9p).
    // The window's per-line sites travel with the inputs: they were read off the session's OSC 133
    // regions when this task was built, which is the only moment they are knowable here.
    let sites = task
        .inputs
        .iter()
        .map(|input| input.site)
        .collect::<Vec<_>>();
    let columns = task
        .inputs
        .iter()
        .map(|input| input.captured_columns)
        .collect::<Vec<_>>();
    let scan = scan_frozen_math_blocks_in_context_with_options(
        task.inputs
            .iter()
            .map(|input| (input.id, input.text.as_str())),
        task.initial_context.clone(),
        task.options,
        Some(&sites),
        Some(&columns),
    );
    // A refusal is the one verdict that travels backwards: it unmakes a table the session may
    // already have drawn from rows that arrived before the refusing line existed. Every refusal
    // this window saw travels, not only the one at this candidate, because the line that refuses a
    // table need not be a candidate itself.
    task.refused_table_starts = scan
        .refused_tables
        .iter()
        .map(|refused| refused.start)
        .collect();
    let detected = scan
        .blocks
        .into_iter()
        .find(|block| block.end == task.candidate_id);
    let Some(block) = detected else {
        return false;
    };
    task.transcript_id = block.start;
    task.block_end = block.end;
    let mut occurrence = block.span;
    let Some(cell_segments) = frozen_occurrence_segments(&occurrence, &task.inputs) else {
        return false;
    };
    occurrence.cell_segments = cell_segments;
    task.span = occurrence;
    task.resolved = true;
    true
}

fn frozen_occurrence_segments(
    occurrence: &MathOccurrence,
    inputs: &[DetectionInput],
) -> Option<Vec<MathCellSegment>> {
    let mut mapped = Vec::with_capacity(occurrence.cell_segments.len());
    let mut input_index = 0usize;
    for segment in &occurrence.cell_segments {
        let MathSourceLine::Transcript(id) = segment.source_line else {
            return None;
        };
        while inputs.get(input_index).is_some_and(|input| input.id < id) {
            input_index += 1;
        }
        let input = inputs.get(input_index).filter(|input| input.id == id)?;
        let cell_start = input
            .cell_boundaries
            .iter()
            .find_map(|(byte, cell)| (*byte == segment.byte_start).then_some(*cell))?;
        let cell_end = input
            .cell_boundaries
            .iter()
            .find_map(|(byte, cell)| (*byte == segment.byte_end).then_some(*cell))?;
        mapped.push(MathCellSegment {
            cell_start,
            cell_end,
            ..segment.clone()
        });
    }
    Some(mapped)
}

/// Resolve a live-grid candidate through the exact same conservative detector as frozen history.
/// Temporary transcript IDs are a detector-local indexing device; they never escape as anchors.
pub fn resolve_live_detection_task(task: &mut LiveDetectionTask) -> bool {
    if task.resolved {
        return true;
    }
    if task.detection_complete {
        return false;
    }
    let logical = live_logical_lines(&task.inputs);
    let row_to_logical = live_grid_logical_ids(&logical, &task.inputs);
    let Some(candidate_id) = row_to_logical.get(&task.candidate_row).copied() else {
        task.detection_complete = true;
        return false;
    };
    let live_grid_boundary = live_grid_boundary_index(&logical, &task.inputs);
    let clipped = clipped_open_index(
        &logical,
        live_grid_first_index(&logical, &task.inputs),
        &task.initial_context,
        task.options,
    );
    let scan = scan_live_math_blocks_in_context(
        logical.iter().map(|line| (line.id, line.text.as_str())),
        task.initial_context.clone(),
        task.options,
        Some(&live_logical_sites(&logical)),
        Some(&live_logical_captured_columns(&logical)),
        live_grid_boundary,
        clipped,
    );
    task.refused_table_rows = refused_table_rows(&scan, &row_to_logical);
    let detected = scan
        .blocks
        .into_iter()
        .find(|block| block.end == candidate_id);
    task.detection_complete = true;
    let Some(block) = detected else {
        return false;
    };
    apply_live_detected_block(task, &block, &logical)
}

/// Resolve every candidate from one stable snapshot with one O(n) scanner pass. Non-matches are
/// marked complete as well, preserving the observable candidate queue without repeating the scan
/// once per delimiter-looking row.
pub fn resolve_live_detection_tasks(tasks: &mut [LiveDetectionTask]) {
    let Some(first) = tasks.first() else {
        return;
    };
    let inputs = Arc::clone(&first.inputs);
    let initial_context = first.initial_context.clone();
    let options = first.options;
    let logical = live_logical_lines(&inputs);
    let row_to_logical = live_grid_logical_ids(&logical, &inputs);
    let live_grid_boundary = live_grid_boundary_index(&logical, &inputs);
    let clipped = clipped_open_index(
        &logical,
        live_grid_first_index(&logical, &inputs),
        &initial_context,
        options,
    );
    let scan = scan_live_math_blocks_in_context(
        logical.iter().map(|line| (line.id, line.text.as_str())),
        initial_context.clone(),
        options,
        Some(&live_logical_sites(&logical)),
        Some(&live_logical_captured_columns(&logical)),
        live_grid_boundary,
        clipped,
    );
    let blocks = scan
        .blocks
        .iter()
        .map(|block| (block.end, block))
        .collect::<BTreeMap<_, _>>();
    for task in tasks {
        if task.resolved || task.detection_complete {
            continue;
        }
        if task.initial_context != initial_context
            || task.inputs.as_ref() != inputs.as_ref()
            || task.options != options
        {
            let _ = resolve_live_detection_task(task);
            continue;
        }
        task.detection_complete = true;
        task.refused_table_rows = refused_table_rows(&scan, &row_to_logical);
        let Some(block) = row_to_logical
            .get(&task.candidate_row)
            .and_then(|id| blocks.get(id))
        else {
            continue;
        };
        let _ = apply_live_detected_block(task, block, &logical);
    }
}

/// The first live-grid row of every table this scan refused.
///
/// A candidate whose rows are all still in the frozen prefix has no live row to name and no live
/// record to take down; the frozen plane owns that one.
fn refused_table_rows(
    scan: &MathScanResult,
    row_to_logical: &BTreeMap<u32, TranscriptId>,
) -> Vec<u32> {
    scan.refused_tables
        .iter()
        .filter_map(|refused| {
            row_to_logical
                .iter()
                .filter(|(_, id)| (refused.start..=refused.end).contains(id))
                .map(|(row, _)| *row)
                .min()
        })
        .collect()
}

fn apply_live_detected_block(
    task: &mut LiveDetectionTask,
    block: &DetectedMathBlock,
    logical: &[LiveLogicalLine],
) -> bool {
    let mut occurrence = block.span.clone();
    let Some(cell_segments) =
        live_occurrence_segments(&occurrence, block.start, logical, &task.inputs)
    else {
        return false;
    };
    occurrence.cell_segments = cell_segments;
    // The occurrence may straddle the frozen/live boundary: its opener (and body) can already be
    // committed to scrollback while the closer is still in the live grid. Detection over the
    // combined context proves the complete pairing; the live-grid rows carry the anchor, and the
    // leading frozen rows form a prefix the presentation layer bridges. A block with no live-grid
    // row at all is entirely frozen and belongs to the frozen detector, not a live anchor.
    let first_live = occurrence
        .cell_segments
        .iter()
        .position(|segment| matches!(segment.source_line, MathSourceLine::LiveGrid(_)));
    let last_live = occurrence
        .cell_segments
        .iter()
        .rposition(|segment| matches!(segment.source_line, MathSourceLine::LiveGrid(_)));
    let (Some(first_live), Some(last_live)) = (first_live, last_live) else {
        return false;
    };
    // The frozen prefix must be exactly a contiguous leading run: the live-grid rows are never
    // interrupted by a frozen row. Anything else is not a boundary split and is rejected rather
    // than anchored to a fabricated geometry.
    if occurrence.cell_segments[first_live..=last_live]
        .iter()
        .any(|segment| matches!(segment.source_line, MathSourceLine::Transcript(_)))
    {
        return false;
    }
    let first = &occurrence.cell_segments[first_live];
    let last = &occurrence.cell_segments[last_live];
    let MathSourceLine::LiveGrid(start_row) = first.source_line else {
        return false;
    };
    let MathSourceLine::LiveGrid(end_row) = last.source_line else {
        return false;
    };
    task.start = GridPoint {
        row: start_row,
        column: first.cell_start,
    };
    task.end = GridPoint {
        row: end_row,
        column: last.cell_end,
    };
    task.band_start_row = start_row;
    task.band_end_row = end_row;
    task.span = occurrence;
    task.resolved = true;
    true
}

/// Index of the first logical line that carries a live-grid fragment, whether or not anything
/// precedes it.
///
/// This is where the scanned window stops being frozen text and starts being the screen, and two
/// different questions are asked at that coordinate. [`live_grid_boundary_index`] asks the narrower
/// one; [`clipped_open_index`] asks this one, because "does this screen begin inside a block" is a
/// question a window with no frozen prefix at all can be asked just as well.
fn live_grid_first_index(
    logical: &[LiveLogicalLine],
    inputs: &[LiveDetectionInput],
) -> Option<usize> {
    logical.iter().position(|line| {
        line.fragments.iter().any(|fragment| {
            matches!(
                inputs[fragment.input_index].source,
                LiveDetectionSource::Grid { .. }
            )
        })
    })
}

/// Index of the first logical line that carries a live-grid fragment, when at least one frozen
/// history line precedes it — i.e. a real frozen→live seam. Returns `None` when the first logical
/// line is already grid (a pure alternate-screen context, or a primary with empty history): there
/// is no frozen prefix to be poisoned, so the boundary-resync guard must stay inert and leave a
/// legitimately truncated alternate prefix (its off-screen opener) to pair as it always has.
fn live_grid_boundary_index(
    logical: &[LiveLogicalLine],
    inputs: &[LiveDetectionInput],
) -> Option<usize> {
    live_grid_first_index(logical, inputs).filter(|&boundary| boundary > 0)
}

fn live_grid_logical_ids(
    logical: &[LiveLogicalLine],
    inputs: &[LiveDetectionInput],
) -> BTreeMap<u32, TranscriptId> {
    logical
        .iter()
        .flat_map(|line| {
            line.fragments.iter().filter_map(|fragment| {
                let LiveDetectionSource::Grid { row, .. } = inputs[fragment.input_index].source
                else {
                    return None;
                };
                Some((row, line.id))
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
struct LiveLogicalFragment {
    input_index: usize,
    byte_start: usize,
    byte_end: usize,
}

#[derive(Clone, Debug)]
struct LiveLogicalLine {
    id: TranscriptId,
    text: String,
    /// The grid width this line's single physical row was captured on, and zero the moment a
    /// second row joins it: a line the terminal soft-wrapped is one line the program printed, so
    /// there is no row of it for a program's own wrap to have ended.
    captured_columns: u32,
    fragments: Vec<LiveLogicalFragment>,
    /// The joined line's site. A soft-wrapped line is several physical rows scanned as one string,
    /// and the disambiguator judges that string whole — so the line carries a site only when every
    /// fragment agrees on one, and is `Ineligible` the moment two disagree. The seam is exactly
    /// where a region boundary lands (a `C` or `D` mid-wrap), and a line straddling one is a line
    /// half of which nothing has claimed to have printed; conservative there means Ineligible.
    site: InlineMathSite,
}

fn live_logical_lines(inputs: &[LiveDetectionInput]) -> Vec<LiveLogicalLine> {
    let mut logical = Vec::<LiveLogicalLine>::new();
    for (input_index, input) in inputs.iter().enumerate() {
        let joins_previous = input_index != 0 && inputs[input_index - 1].continues;
        if !joins_previous {
            let Some(id) = live_temporary_id(logical.len()) else {
                break;
            };
            logical.push(LiveLogicalLine {
                id,
                text: String::new(),
                captured_columns: input.captured_columns,
                fragments: Vec::new(),
                site: input.site,
            });
        }
        let Some(line) = logical.last_mut() else {
            continue;
        };
        let byte_start = line.text.len();
        if !line.fragments.is_empty() {
            line.captured_columns = 0;
        }
        line.text.push_str(&input.text);
        line.fragments.push(LiveLogicalFragment {
            input_index,
            byte_start,
            byte_end: line.text.len(),
        });
        // Unanimity, not membership. Seeding from the first fragment and demoting on the first
        // disagreement is what lets a wrapped line on the alternate screen stay eligible: every
        // fragment of it is `AltScreenContent`, and a rule phrased as "all fragments are
        // `CommandOutput`" would have made every wrapped line on that screen ineligible — which is
        // to say it would have silently repealed the widening for exactly the screen the widening
        // was for.
        if input.site != line.site {
            line.site = InlineMathSite::Ineligible;
        }
    }
    logical
}

/// The per-logical-line capture geometry a live scan hands to the scanner, in scanner index order.
fn live_logical_captured_columns(logical: &[LiveLogicalLine]) -> Vec<u32> {
    logical.iter().map(|line| line.captured_columns).collect()
}

/// The per-logical-line site slice a live scan hands to the scanner, in scanner index order.
fn live_logical_sites(logical: &[LiveLogicalLine]) -> Vec<InlineMathSite> {
    logical.iter().map(|line| line.site).collect()
}

fn live_occurrence_segments(
    occurrence: &MathOccurrence,
    start: TranscriptId,
    logical: &[LiveLogicalLine],
    inputs: &[LiveDetectionInput],
) -> Option<Vec<MathCellSegment>> {
    let start_index = logical.iter().position(|line| line.id == start)?;
    let mut mapped = Vec::new();
    for source in &occurrence.cell_segments {
        let logical_index = start_index.checked_add(source.logical_line as usize)?;
        let line = logical.get(logical_index)?;
        let source_start = source.byte_start as usize;
        let source_end = source.byte_end as usize;
        if source_start == source_end {
            let fragment = line.fragments.first()?;
            let input = inputs.get(fragment.input_index)?;
            let source_line = live_input_source_line(input);
            let local = u32::try_from(source_start.saturating_sub(fragment.byte_start)).ok()?;
            let cell = input
                .cell_boundaries
                .iter()
                .find_map(|(byte, cell)| (*byte == local).then_some(*cell))?;
            mapped.push(MathCellSegment {
                logical_line: source.logical_line,
                source_line,
                byte_start: local,
                byte_end: local,
                cell_start: cell,
                cell_end: cell,
            });
            continue;
        }
        for fragment in &line.fragments {
            let overlap_start = source_start.max(fragment.byte_start);
            let overlap_end = source_end.min(fragment.byte_end);
            if overlap_start >= overlap_end {
                continue;
            }
            let input = inputs.get(fragment.input_index)?;
            let source_line = live_input_source_line(input);
            let local_start = u32::try_from(overlap_start - fragment.byte_start).ok()?;
            let local_end = u32::try_from(overlap_end - fragment.byte_start).ok()?;
            let cell_start = input
                .cell_boundaries
                .iter()
                .find_map(|(byte, cell)| (*byte == local_start).then_some(*cell))?;
            let cell_end = input
                .cell_boundaries
                .iter()
                .find_map(|(byte, cell)| (*byte == local_end).then_some(*cell))?;
            mapped.push(MathCellSegment {
                logical_line: source.logical_line,
                source_line,
                byte_start: local_start,
                byte_end: local_end,
                cell_start,
                cell_end,
            });
        }
    }
    (!mapped.is_empty()).then_some(mapped)
}

/// Physical source row of a live-detection fragment. A live-detection context prepends a bounded
/// tail of already-frozen transcript lines before the live grid, so a proven occurrence may cover
/// both: the frozen rows carry their real `TranscriptId`, the live rows their grid index.
fn live_input_source_line(input: &LiveDetectionInput) -> MathSourceLine {
    match input.source {
        LiveDetectionSource::Grid { row, .. } => MathSourceLine::LiveGrid(row),
        LiveDetectionSource::History { id } => MathSourceLine::Transcript(id),
    }
}

fn live_temporary_id(index: usize) -> Option<TranscriptId> {
    u64::try_from(index).ok()?.checked_add(1).map(TranscriptId)
}

/// Is the delimiter at `byte` escaped by the backslashes in front of it?
///
/// **One definition, because two surfaces ask it.** The terminal's detector asks
/// it of a printed line and the markdown preview asks it of a document's own
/// bytes ([`crate::preview`]'s inline pass in `bt-app`), and `\$` has to mean the
/// same thing in both: a literal dollar, never a delimiter. An even run of
/// backslashes escapes itself and leaves the dollar free, which is why this
/// counts parity rather than looking at the one byte before.
#[must_use]
pub fn delimiter_is_escaped(text: &str, byte: usize) -> bool {
    text[..byte]
        .bytes()
        .rev()
        .take_while(|character| *character == b'\\')
        .count()
        % 2
        == 1
}

fn fence_marker(text: &str) -> Option<(char, usize)> {
    let marker = text.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let count = text
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (count >= 3).then_some((marker, count))
}

pub fn render_placeholder(task: &DetectionTask) -> PlaceholderArtifact {
    PlaceholderArtifact {
        key: format!(
            "math:{}:{}:{}",
            task.transcript_id.0, task.span.byte_start, task.versions.detection.0
        ),
        kind: task.span.kind,
        block_end: task.block_end,
        height_subpixels: 64 * SUBPIXELS_PER_PX,
        rgba: Arc::from(vec![0; 4]),
        width_px: 1,
        height_px: 1,
        baseline_subpixels: 0,
        mode: task.span.mode,
        render_time: Duration::ZERO,
        inline_runs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_transcript::{CapturedRow, TranscriptStore};
    use std::{num::NonZeroU32, num::NonZeroUsize};

    fn nz32(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    fn stamp() -> VersionStamp {
        VersionStamp {
            source: SourceGeneration(1),
            detection: DetectionRevision(1),
            layout: LayoutKey {
                width_cells: nz32(80),
                dpi_milli: nz32(1000),
                font_size_subpixels: 16 * 1024,
                font_rev: 1,
                theme_rev: 1,
                lang_rev: 0,
                profile_rev: 0,
                line_wrapping: true,
            },
            view: ViewGeneration(1),
        }
    }

    #[test]
    fn compact_context_proves_local_pairing_boundaries_without_guessing_through_fences() {
        let id = TranscriptId;
        let mut context = DetectionContext::default();
        advance_detection_context(&mut context, id(1), "ordinary");
        assert!(context.is_neutral());

        advance_detection_context(&mut context, id(2), r"\begin{align}");
        assert_eq!(context.required_start(id(3)), Some(id(2)));
        advance_detection_context(&mut context, id(3), "x &= y");
        assert_eq!(context.required_start(id(4)), Some(id(2)));
        advance_detection_context(&mut context, id(4), r"\end{align}");
        assert!(context.is_neutral());

        advance_detection_context(&mut context, id(5), "```");
        assert_eq!(context.required_start(id(6)), None);
        advance_detection_context(&mut context, id(6), "$$");
        assert_eq!(context.required_start(id(7)), None);
        advance_detection_context(&mut context, id(7), "```");
        assert!(context.is_neutral());
    }

    #[test]
    fn only_closed_block_delimiters_are_detected() {
        let spans = detect_block_math("  $$x^2$$  ");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].render_source, "x^2");
    }

    /// **The prefix walker and the scanner are one state machine, and this is what says so.**
    ///
    /// `advance_detection_context` exists so a caller can ask "what parser state is this line in?"
    /// without rescanning everything above it, and every answer it gives is worth exactly the
    /// scanner's agreement. It was a second copy of the scanner's structural rules and it drifted:
    /// it never learned the swallow-radius bound, so after an unfinished `\begin{align}` it went on
    /// swallowing every line as environment body and never saw the code fence below — and a caller
    /// that asked it for the state of a line inside that fence was told there was no fence.
    ///
    /// The test is the equivalence itself rather than a list of cases: for every corpus, and every
    /// point it could be split at, walking the prefix and then scanning the tail from the state the
    /// walk arrived at must find exactly the blocks a single scan of the whole thing finds below
    /// that split. One state machine, or this goes red.
    #[test]
    fn walking_a_prefix_leaves_the_scanner_where_scanning_it_would_have() {
        const CORPORA: &[&[&str]] = &[
            // The shape the review named: an environment nobody closed, a block that ends its
            // swallow, and a fence that only a reader who noticed the abandon can see.
            &[
                r"\begin{align}",
                "$$z^2$$",
                "```",
                "code",
                "$$x^2$$",
                "```",
                "tail",
            ],
            // A fence opened inside an unfinished environment, and never closed.
            &[r"\begin{align}", "$$z^2$$", "```", "code", "more code"],
            // Fence markers of both kinds, one inside the other's body.
            &["~~~", "```", "$$a$$", "~~~", "$$b$$", "```", "$$c$$"],
            // The environment does close, later: nothing may be abandoned.
            &[
                r"\begin{align}",
                "x &= y",
                r"\end{align}",
                "```",
                "$$d$$",
                "```",
                "$$e$$",
            ],
            // Indented code under an unfinished environment, then a block.
            &[r"\begin{align}", "    indented", "$$f$$", "tail"],
        ];

        for (corpus_index, corpus) in CORPORA.iter().enumerate() {
            let numbered = corpus
                .iter()
                .enumerate()
                .map(|(index, text)| (TranscriptId(index as u64 + 1), *text))
                .collect::<Vec<_>>();
            let whole = detect_math_blocks_in_context(
                numbered.iter().copied(),
                DetectionContext::default(),
            );
            for split in 0..=corpus.len() {
                let mut context = DetectionContext::default();
                for (id, text) in &numbered[..split] {
                    advance_detection_context(&mut context, *id, text);
                }
                let found =
                    detect_math_blocks_in_context(numbered[split..].iter().copied(), context)
                        .iter()
                        .map(|block| (block.start, block.end))
                        .collect::<Vec<_>>();
                let expected = whole
                    .iter()
                    .filter(|block| block.start.0 > split as u64)
                    .map(|block| (block.start, block.end))
                    .collect::<Vec<_>>();
                assert_eq!(
                    found, expected,
                    "corpus {corpus_index} split at {split}: the walked prefix does not leave the \
                     scanner where scanning it would have"
                );
            }
        }
    }

    #[test]
    fn zero_tolerance_false_positive_set_is_rejected() {
        for text in [
            "echo $$",
            "pid=$$",
            "+ $$x^2$$",
            "2026-07-18 log: $$x^2$$",
            r"\$$x^2$$",
            "prefix $$x^2$$ suffix",
            "$$broken",
        ] {
            assert!(
                detect_block_math(text).is_empty(),
                "unexpected match: {text}"
            );
        }
    }

    /// Ordinary terminal text that must never be typeset, **stated at the site where it really
    /// occurs**.
    ///
    /// The site column is the point of this table and it is chosen adversarially, not
    /// conveniently. It would be easy — and dishonest — to file every shell-looking line under
    /// `Ineligible`, where gate A rejects it before a single character is read, and then report a
    /// false-positive rate of zero that measured nothing. So each line is filed where it actually
    /// shows up: a `cat`-ed profile, a query log and a price list are all things a command
    /// *prints*, so they are `CommandOutput` and have to be stopped on content alone. Only the
    /// lines that genuinely belong to a prompt or an unmarked screen are filed as `Ineligible`.
    ///
    /// The three cases the independent review named as rendering wrongly — `PATH=$HOME/bin:$PATH`
    /// giving `HOME/bin:`, `WHERE a=$1 AND b=$2` giving `1 AND b=`, `Cost $5+$10` giving `5+` —
    /// are the first three rows, all at `CommandOutput`.
    const INLINE_FALSE_POSITIVE_CORPUS: &[(&str, InlineMathSite)] = &[
        // The named six, at the hardest site.
        ("PATH=$HOME/bin:$PATH", InlineMathSite::CommandOutput),
        ("WHERE a=$1 AND b=$2", InlineMathSite::CommandOutput),
        ("Cost $5+$10", InlineMathSite::CommandOutput),
        (r"escaped \$x^2\$ stays flat", InlineMathSite::CommandOutput),
        ("`const x = $value`", InlineMathSite::CommandOutput),
        ("literal $PATH$ token", InlineMathSite::CommandOutput),
        // Currency, the single most common lone-dollar in real output.
        ("$5 和 $10", InlineMathSite::CommandOutput),
        ("价格是 $5$", InlineMathSite::CommandOutput),
        (
            "Total $19.99 or $24.99 with tax",
            InlineMathSite::CommandOutput,
        ),
        ("refund $5 - $3 today", InlineMathSite::CommandOutput),
        ("tiers $5-$10-$20", InlineMathSite::CommandOutput),
        // Shell and script text a command printed rather than a user typed.
        ("awk '{print $1}' report.txt", InlineMathSite::CommandOutput),
        ("if [ $a -eq $b ]; then", InlineMathSite::CommandOutput),
        ("usage: run.sh $src $dst", InlineMathSite::CommandOutput),
        ("export FOO=$BAR:$BAZ", InlineMathSite::CommandOutput),
        ("$1 $2 $3", InlineMathSite::CommandOutput),
        ("sed -i 's/$old/$new/g' f", InlineMathSite::CommandOutput),
        // Structurally exempt line shapes (gate D).
        ("+ 文档里有 $x^2$", InlineMathSite::CommandOutput),
        ("2026-07-19 log $x^2$", InlineMathSite::CommandOutput),
        ("unclosed $x^2", InlineMathSite::CommandOutput),
        // Gate A's own territory: text that would otherwise pass every content test, sitting
        // where a lone `$` is not ours to interpret.
        ("$x^2$", InlineMathSite::Ineligible),
        (r"$\alpha+\beta$", InlineMathSite::Ineligible),
        ("echo $PATH", InlineMathSite::Ineligible),
        ("PS D:\\dev> echo $env:PATH", InlineMathSite::Ineligible),
        // ── The alternate screen, added when the site ruling was widened (2026-08-10). ──
        //
        // These are the rows that pay for the widening. Gate A no longer stops anything here, so
        // every one of them has to die on content alone, and they are chosen to be the two things
        // a full-screen application most plausibly puts on screen that a lone `$` could be
        // misread in: a shell script open in an editor, and a table of prices in a TUI.
        //
        // A shell script in `vim`, seen through the gutter that stops the line ever *starting*
        // with `export`/`echo` and so denies gate D its cheapest catch.
        (
            "  12 PATH=$HOME/bin:$PATH",
            InlineMathSite::AltScreenContent,
        ),
        ("  4 echo \"$1 of $2\"", InlineMathSite::AltScreenContent),
        (
            "  7 if [ $# -gt $1 ]; then",
            InlineMathSite::AltScreenContent,
        ),
        (
            "  15 rm -rf \"$BUILD_DIR\"/$TARGET",
            InlineMathSite::AltScreenContent,
        ),
        (
            "  9 for f in $src/*.sh; do",
            InlineMathSite::AltScreenContent,
        ),
        ("  3 test $a$b", InlineMathSite::AltScreenContent),
        ("  21 dst=$1; src=$2", InlineMathSite::AltScreenContent),
        // A price table drawn with box characters, both loosely and tightly spaced.
        (
            "│ Pro plan   │ $29 │ $290 │",
            InlineMathSite::AltScreenContent,
        ),
        ("│$29│$290│", InlineMathSite::AltScreenContent),
        (
            "│ Total      $19.99   $24.99 │",
            InlineMathSite::AltScreenContent,
        ),
        ("Subtotal: $12+$8 = $20", InlineMathSite::AltScreenContent),
        (
            "  Basic  $5  Pro  $15  Max  $50",
            InlineMathSite::AltScreenContent,
        ),
        // A debugger's register pane, the other place a bare `$` names a variable.
        ("(gdb) print $rsp - $rbp", InlineMathSite::AltScreenContent),
        ("$rsp-$rbp", InlineMathSite::AltScreenContent),
        // Adjacent expansions, the shape that made `byte_continues_an_identifier` necessary.
        ("  8 out=$dir$SUFFIX", InlineMathSite::AltScreenContent),
        ("prefix$x$y suffix", InlineMathSite::CommandOutput),
    ];

    /// Genuine inline mathematics, which must be typeset when a command printed it.
    const INLINE_TRUE_POSITIVE_CORPUS: &[&str] = &[
        "$x$",
        "$a+b$",
        r"$f_\theta$",
        r"$\rho$",
        "$x^2$",
        r"$\alpha+\beta$",
        "$-x$",
        "$E = mc^2$",
        r"$\frac{a}{b}$",
        "能量 $E = mc^2$，并且 $a_1+b_1=c_1$。",
        r"the loss $\mathcal{L}(\theta)$ fell",
        // No spaces around the delimiters, which is simply how the sentence is written in Chinese.
        // `byte_continues_an_identifier` must stay ASCII-only for this line to survive it.
        "能量$E$的值",
        r"梯度$\nabla f$在此处为零",
    ];

    /// **Zero false positives on the corpus.** Not "few" — the ruling that re-enabled inline
    /// rendering did so on the strength of this number, and a terminal that typesets one line of
    /// a user's literal output has failed at the job the rendering was a bonus on top of.
    #[test]
    fn the_inline_false_positive_corpus_is_rendered_natively_at_the_site_it_occurs() {
        let mut wrong = Vec::new();
        for (text, site) in INLINE_FALSE_POSITIVE_CORPUS {
            let runs = detect_inline_math(text, *site);
            if !runs.is_empty() {
                let rendered = runs
                    .iter()
                    .map(|run| run.source.as_str())
                    .collect::<Vec<_>>()
                    .join(" | ");
                wrong.push(format!("{text:?} at {site:?} would typeset {rendered:?}"));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {} corpus lines would be typeset:\n  {}",
            wrong.len(),
            INLINE_FALSE_POSITIVE_CORPUS.len(),
            wrong.join("\n  ")
        );
    }

    #[test]
    fn the_inline_true_positive_corpus_is_typeset_in_command_output() {
        let mut missed = Vec::new();
        for text in INLINE_TRUE_POSITIVE_CORPUS {
            if detect_inline_math(text, InlineMathSite::CommandOutput).is_empty() {
                missed.push(*text);
            }
        }
        assert!(
            missed.is_empty(),
            "{} of {} genuine formulas went undetected in command output: {missed:?}",
            missed.len(),
            INLINE_TRUE_POSITIVE_CORPUS.len()
        );
    }

    /// The alternate screen typesets the same genuine mathematics command output does.
    ///
    /// This is the recovered half of the widened ruling and the reason it was made: a full-screen
    /// application — Claude Code being the one in daily use here — emits no OSC 133 at all, so
    /// under the original single-site rule every inline formula it printed stayed source text
    /// while the display formulas beside it rendered. Stated against the same corpus as the
    /// command-output test so the two sites cannot quietly diverge.
    #[test]
    fn the_inline_true_positive_corpus_is_typeset_on_the_alternate_screen() {
        let mut missed = Vec::new();
        for text in INLINE_TRUE_POSITIVE_CORPUS {
            if detect_inline_math(text, InlineMathSite::AltScreenContent).is_empty() {
                missed.push(*text);
            }
        }
        assert!(
            missed.is_empty(),
            "{} of {} genuine formulas went undetected on the alternate screen: {missed:?}",
            missed.len(),
            INLINE_TRUE_POSITIVE_CORPUS.len()
        );
    }

    /// The measured, accepted cost of gate A, kept as an assertion rather than a sentence in a
    /// report so that it stays true.
    ///
    /// Every line of genuine mathematics in the corpus goes undetected at an ineligible site —
    /// that is the whole of the ruling's price, and it is a *complete* loss on that side, not a
    /// partial one. An unintegrated primary screen renders no inline formulas at all. If a future
    /// change makes even one of these detect, the structural guarantee that buys the zero above
    /// has been broken and this test is where it surfaces.
    #[test]
    fn gate_a_costs_every_genuine_formula_printed_at_an_ineligible_site() {
        for text in INLINE_TRUE_POSITIVE_CORPUS {
            assert!(
                detect_inline_math(text, InlineMathSite::Ineligible).is_empty(),
                "{text:?} must not be typeset without proof of where it was printed"
            );
        }
    }

    /// Eligibility is a property of the site and nothing else — the two eligible sites must agree
    /// on every line in both corpora, or one of them has grown a content rule of its own.
    #[test]
    fn the_two_eligible_sites_reach_identical_verdicts() {
        let corpus = INLINE_FALSE_POSITIVE_CORPUS
            .iter()
            .map(|(text, _)| *text)
            .chain(INLINE_TRUE_POSITIVE_CORPUS.iter().copied());
        for text in corpus {
            assert_eq!(
                detect_inline_math(text, InlineMathSite::CommandOutput),
                detect_inline_math(text, InlineMathSite::AltScreenContent),
                "{text:?} is read differently in command output than on the alternate screen"
            );
        }
    }

    /// A line that reaches the scanner without a stated site gets no inline rendering. The
    /// conservative direction is the only safe default for a question this crate cannot answer.
    #[test]
    fn a_scan_that_states_no_site_yields_no_inline_math() {
        let text = "能量 $E = mc^2$，并且 $a_1+b_1=c_1$。";
        assert!(
            detect_math_blocks([(TranscriptId(1), text)]).is_empty(),
            "the site-less entry point must not typeset a lone dollar run"
        );
    }

    /// The "Inline formulas" switch is load-bearing, not decorative: with everything else held
    /// equal — same text, same proven command-output site — `false` must yield nothing.
    ///
    /// Stated at the scanner rather than at the viewport because that is where this switch acts.
    /// Its display sibling hides finished rasters; this one stops the run from ever being a run.
    #[test]
    fn the_inline_formulas_switch_decides_whether_a_proven_run_is_detected_at_all() {
        let text = "能量 $E = mc^2$，并且 $a_1+b_1=c_1$。";
        let scan = |inline_formulas| {
            detect_math_blocks_with_sites(
                [(TranscriptId(1), text, InlineMathSite::CommandOutput)],
                DetectionOptions {
                    inline_formulas,
                    ..DetectionOptions::default()
                },
            )
        };
        let on = scan(true);
        assert_eq!(
            on.len(),
            1,
            "command-output inline math must be detected with the switch on"
        );
        assert_eq!(on[0].span.mode, MathMode::Inline);
        assert_eq!(on[0].span.inline_runs.len(), 2);
        assert!(
            scan(false).is_empty(),
            "the same proven run must vanish with the switch off"
        );
    }

    // ── T-MATH-INLINE-WRAP: a formula the producer's own wrapping split across two rows. ──

    /// The head row and the tail row of the sentence in the 2026-09-15 report, as Claude Code wrote
    /// them into the pane. The wrap fell between `$x` and `= \frac…`, which is where the terminal
    /// would have folded it too — and is why no amount of one-row reading finds a formula here.
    const SPLIT_HEAD: &str = "Use the quadratic formula $x";
    const SPLIT_TAIL: &str =
        r"= \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$, and the density $\varphi(x) = e^{-x^2/2}$.";

    fn join(previous: &str, text: &str) -> Option<JoinedInlineRow> {
        detect_inline_math_across_rows(
            previous,
            InlineMathSite::CommandOutput,
            text,
            InlineMathSite::CommandOutput,
        )
    }

    /// PIN: the exact defect. Two rows, one formula, typeset.
    #[test]
    fn a_formula_the_producer_wrapped_across_two_rows_is_joined_and_typeset() {
        assert!(
            detect_inline_math(SPLIT_HEAD, InlineMathSite::CommandOutput).is_empty()
                && detect_inline_math(SPLIT_TAIL, InlineMathSite::CommandOutput).is_empty(),
            "the premise of the ticket: neither half is a formula on its own"
        );
        let joined = join(SPLIT_HEAD, SPLIT_TAIL).expect("the two halves must join");
        assert_eq!(
            joined.runs[0].source, r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}",
            "the row break becomes one space and nothing else changes"
        );
        assert_eq!(
            joined.runs[0].byte_start, 0,
            "the joined run stands at the start of the closing row"
        );
        assert_eq!(
            &SPLIT_TAIL[..joined.runs[0].byte_end as usize],
            r"= \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$",
            "and owns exactly the cells of its own fragment, closing delimiter included"
        );
        assert_eq!(
            joined.head.text, "$x",
            "the fragment left above, for the placer"
        );
        assert_eq!(
            &SPLIT_HEAD[joined.head.byte_start as usize..joined.head.byte_end as usize],
            "$x"
        );
        assert_eq!(
            joined.runs.len(),
            2,
            "the second formula on the closing row must survive the first one's split: {:?}",
            joined.runs
        );
        assert_eq!(joined.runs[1].source, r"\varphi(x) = e^{-x^2/2}");
    }

    /// The same two rows through the scanner: one occurrence, keyed on the closing line.
    #[test]
    fn the_scanner_keys_a_row_split_formula_on_the_line_that_closes_it() {
        let blocks = detect_math_blocks_with_sites(
            [
                (TranscriptId(1), SPLIT_HEAD, InlineMathSite::CommandOutput),
                (TranscriptId(2), SPLIT_TAIL, InlineMathSite::CommandOutput),
            ],
            DetectionOptions::default(),
        );
        assert_eq!(blocks.len(), 1, "one occurrence, not two: {blocks:?}");
        assert_eq!(blocks[0].start, TranscriptId(2));
        assert_eq!(blocks[0].end, TranscriptId(2));
        assert_eq!(blocks[0].span.mode, MathMode::Inline);
        assert_eq!(blocks[0].span.inline_runs.len(), 2);
        assert_eq!(
            blocks[0]
                .span
                .inline_joined_head
                .as_ref()
                .map(|head| head.text.as_str()),
            Some("$x"),
            "the fragment above travels with the occurrence or the placer cannot clear it"
        );
        assert_eq!(
            blocks[0].span.cell_segments.len(),
            2,
            "one segment per run, on the closing line, as for any inline occurrence"
        );
    }

    /// A currency mark at the end of a row is not an opening delimiter, whatever follows it.
    #[test]
    fn a_trailing_currency_mark_never_joins_with_the_row_below() {
        assert!(
            join("The starter plan costs $5", "and the upgrade costs more").is_none(),
            "a row with no closing delimiter cannot close anything"
        );
        assert!(
            join("The starter plan costs $5", "and Pro costs $15 monthly").is_none(),
            "nor can a row whose only dollar is another price"
        );
        assert!(
            join("Basic $5", "and Pro (see above)$ monthly").is_none(),
            "and where the dollar below could close, `5 and Pro (see above)` is prose and dies \
             exactly where a one-row `$5 and Pro (see above)$` dies"
        );
    }

    /// A blank row is a paragraph break, and nothing joins across one.
    #[test]
    fn a_dangling_opener_never_joins_across_a_blank_row() {
        assert!(join(SPLIT_HEAD, "").is_none());
        assert!(join(SPLIT_HEAD, "   ").is_none());
    }

    /// Nor across a row that begins something of its own.
    #[test]
    fn a_dangling_opener_never_joins_into_a_bullet_a_quote_or_a_heading() {
        for opener in ["• ", "- ", "* ", "> ", "## ", "1. "] {
            let text = format!(r"{opener}= \frac{{-b}}{{2a}}$ and so on");
            assert!(
                join(SPLIT_HEAD, &text).is_none(),
                "{text:?} starts a new block and its dollar is its own"
            );
        }
    }

    /// Two rows that each leave a dollar open are two unfinished thoughts, not one formula.
    #[test]
    fn two_open_dollars_on_consecutive_rows_do_not_join() {
        assert!(join("the value $x", "and the value $y").is_none());
        assert!(join("the tensor $T", r"and also $\Phi + 1$ here").is_none());
    }

    /// A formula that is whole on its row is read exactly as it was before the join existed.
    #[test]
    fn a_complete_one_row_formula_is_untouched_by_the_join() {
        let lines = [
            (
                TranscriptId(1),
                "energy $E = mc^2$ here",
                InlineMathSite::CommandOutput,
            ),
            (
                TranscriptId(2),
                r"and $a_1+b_1=c_1$ too",
                InlineMathSite::CommandOutput,
            ),
        ];
        let blocks = detect_math_blocks_with_sites(lines, DetectionOptions::default());
        assert_eq!(blocks.len(), 2);
        for block in &blocks {
            assert_eq!(block.span.inline_runs.len(), 1);
            assert!(
                block.span.inline_joined_head.is_none(),
                "nothing was split, so nothing was joined"
            );
        }
        assert!(
            join("energy $E = mc^2$ here", r"and $a_1+b_1=c_1$ too").is_none(),
            "a row whose dollars already pair has no unmatched opener to offer"
        );
    }

    /// The locality bound, stated as a bound and not as a length limit on mathematics: the formula
    /// below is perfectly good and is refused a *join* only because its closer stands too far in.
    #[test]
    fn a_closer_beyond_the_join_reach_is_left_alone() {
        // One formula, two lengths: the only thing that differs is how far into the row its
        // closing delimiter lands.
        let tail = |pad: usize| format!(r"= \gamma_{{{}}}$ and the rest", "b".repeat(pad));
        let closing_column = |text: &str| {
            bt_unicode::text_width(text.split('$').next().expect("a prefix")).saturating_add(1)
        };
        let near = tail(20);
        let far = tail(60);
        assert!(closing_column(&near) <= INLINE_ROW_JOIN_MAX_CLOSING_COLUMNS);
        assert!(closing_column(&far) > INLINE_ROW_JOIN_MAX_CLOSING_COLUMNS);
        assert!(join("the value $x", &near).is_some());
        assert!(
            join("the value $x", &far).is_none(),
            "past {INLINE_ROW_JOIN_MAX_CLOSING_COLUMNS} cells the closer is no longer this \
             fragment's closer"
        );
    }

    /// Both halves must sit at the same inline-eligible site — which is how "one command printed
    /// both of them" is proved by a crate that has never heard of a command.
    #[test]
    fn a_row_split_join_needs_one_site_for_both_halves() {
        for (previous, text) in [
            (InlineMathSite::CommandOutput, InlineMathSite::Ineligible),
            (InlineMathSite::Ineligible, InlineMathSite::CommandOutput),
            (
                InlineMathSite::CommandOutput,
                InlineMathSite::AltScreenContent,
            ),
            (InlineMathSite::Ineligible, InlineMathSite::Ineligible),
        ] {
            assert!(
                detect_inline_math_across_rows(SPLIT_HEAD, previous, SPLIT_TAIL, text).is_none(),
                "{previous:?} above {text:?} is not one command's output"
            );
        }
        assert!(
            detect_inline_math_across_rows(
                SPLIT_HEAD,
                InlineMathSite::AltScreenContent,
                SPLIT_TAIL,
                InlineMathSite::AltScreenContent,
            )
            .is_some(),
            "the alternate screen — where Claude Code lives — joins exactly as command output does"
        );
    }

    /// **Zero false positives, in two dimensions.** The one-row corpus is the measurement the
    /// inline ruling was made on; the join adds a second row to every line of it, so it is measured
    /// again over every ordered pair the corpus can make. 1,600 pairs of real terminal text, no
    /// joins.
    #[test]
    fn no_ordered_pair_of_the_false_positive_corpus_ever_joins() {
        let mut wrong = Vec::new();
        for (above, _) in INLINE_FALSE_POSITIVE_CORPUS {
            for (below, _) in INLINE_FALSE_POSITIVE_CORPUS {
                if let Some(joined) = join(above, below) {
                    wrong.push(format!(
                        "{above:?} over {below:?} would typeset {:?}",
                        joined.runs[0].source
                    ));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} corpus pairs would be joined:\n  {}",
            wrong.len(),
            wrong.join("\n  ")
        );
    }

    /// **The prefilter reads no further than its own reach**, which is the observable half of the
    /// budget on [`may_carry_inline_math`]: it is answered by a walk that stops at
    /// [`INLINE_ROW_JOIN_MAX_CLOSING_COLUMNS`], so a hundred thousand columns of line cannot change
    /// the verdict — and cannot be walked to reach it either. A `text_width` over the prefix would
    /// give the same answers and cost the line; this is the assertion that says the answers must
    /// not depend on what is out there.
    #[test]
    fn the_arming_prefilter_never_reads_past_its_own_reach() {
        let near = "= a$ rest";
        assert!(
            may_carry_inline_math(near) && may_close_row_split_inline_math(near),
            "a lone closer inside the reach arms its row"
        );
        // The same closer, pushed past the reach — first by a little text, then by a hundred
        // thousand columns of it. Both are refused, and the second costs what the first does,
        // because the walk stops at the reach and never sees the rest.
        for padding in [60, 50_000] {
            let far = format!("{}{near}", "漢".repeat(padding));
            assert!(!may_carry_inline_math(&far), "{padding} wide characters in");
            assert!(
                !may_close_row_split_inline_math(&far),
                "{padding} wide characters in, asked the window's way"
            );
        }
    }

    /// The arming prefilter's half of the rule: tight enough that the sigils a terminal prints do
    /// not answer it, wide enough that the row which closes a split formula does.
    #[test]
    fn only_a_dollar_that_could_close_something_arms_a_row_on_its_own() {
        for text in [
            "echo $PATH",
            "cost $5 today",
            "awk '{print $1}' report.txt",
            "$ ls -la",
            r"escaped \$ here",
            "out=$dir and more",
            "no dollars at all",
            "literal $PATH$ token",
        ] {
            assert!(
                !may_close_row_split_inline_math(text),
                "{text:?} must not arm a scan by itself"
            );
        }
        for text in [
            r"= \frac{-b}{2a}$, and the rest",
            "4ac}}{2a}$ and so on",
            "a}$ text",
            // A closer with a whole formula of its own behind it. The row carries three dollars
            // and is a closing row all the same: the closer is the first of them (X-6).
            r"= \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$, and the density $\varphi(x) = e^{-x^2/2}$.",
        ] {
            assert!(
                may_close_row_split_inline_math(text),
                "{text:?} could close a formula left open above and must be asked about"
            );
        }
    }

    /// **One closer rule, asked by both sides.** The frozen scan window decides whether the row
    /// above a candidate is read at all; the join decides what happens when it is. Let the two
    /// disagree and the formula is simply lost — the worker is handed one row, the opening fragment
    /// is not in it, and nothing anywhere records that a join was refused. That is exactly what
    /// happened to the reported sentence, whose closing row the window read as a census of dollars
    /// and therefore refused for carrying a second, whole formula (release review 2026-09-17, X-6).
    ///
    /// So the window's question is [`first_inline_closer`], the join's own, and this holds the two
    /// answers together over every line of the false-positive corpus and over both halves of the
    /// report. It is a property of the code rather than of the corpus, which is the point: no new
    /// closing row can be admitted on one side alone.
    #[test]
    fn the_scan_window_and_the_join_agree_on_every_closing_row() {
        let mut disagreed = Vec::new();
        for text in INLINE_FALSE_POSITIVE_CORPUS
            .iter()
            .map(|(line, _)| *line)
            .chain([SPLIT_HEAD, SPLIT_TAIL, "and Pro costs $15", "no dollars"])
        {
            if join(SPLIT_HEAD, text).is_some() && !may_close_row_split_inline_math(text) {
                disagreed.push(format!("{text:?} joins, but its row above is never read"));
            }
        }
        assert!(
            disagreed.is_empty(),
            "{} closing rows the window refuses and the join accepts:\n  {}",
            disagreed.len(),
            disagreed.join("\n  ")
        );
        assert!(
            may_close_row_split_inline_math(SPLIT_TAIL),
            "the reported sentence's own closing row carries three dollars and has to reach the \
             row above it: the closer is the first of them, and the density behind it is a run of \
             its own"
        );
    }

    #[test]
    fn a_truncated_window_never_pairs_a_closer_with_the_next_opener() {
        // The window starts INSIDE a block, so its first `$$` is really a closing delimiter.
        // Pairing it with the next block's opener would swallow the prose between them - which is
        // exactly what rendered a Chinese paragraph as mathematics (user report 2026-07-19).
        let window = [
            (TranscriptId(1), r"rac{a}{b}"), // tail of a block whose opener is off-screen
            (TranscriptId(2), "$$"),          // actually a CLOSER
            (TranscriptId(3), "内部含转义或美元符号语义的:"),
            (TranscriptId(4), "多行里带对齐点和长表达式:"),
            (TranscriptId(5), "$$"), // the NEXT block's opener
            (TranscriptId(6), r"\sigma(z)_i = 1"),
            (TranscriptId(7), "$$"),
        ];
        let blocks = detect_math_blocks(window);
        for block in &blocks {
            assert!(
                !block.span.render_source.contains("内部含"),
                "prose was captured by a mis-paired delimiter: {:?}",
                block.span.render_source
            );
        }
        // The genuine block in this window (5..7) may still resolve; the prose one must not.
        assert!(
            blocks
                .iter()
                .all(|block| !block.span.render_source.contains("多行里带"))
        );
    }

    #[test]
    fn a_block_body_may_still_carry_cjk_through_a_command() {
        // Legitimate: CJK inside \text{...} always arrives on a line that carries a backslash,
        // so the prose guard must not reject it.
        let window = [
            (TranscriptId(1), "$$"),
            (TranscriptId(2), r"\underbrace{x + x}_{n \text{ 项}} = nx"),
            (TranscriptId(3), "$$"),
        ];
        let blocks = detect_math_blocks(window);
        assert_eq!(blocks.len(), 1, "\\text{{CJK}} must remain renderable");
        assert!(blocks[0].span.render_source.contains("项"));
    }

    #[test]
    fn truncated_context_rejects_ambiguous_pairing_before_body_heuristics_run() {
        let window = [
            (TranscriptId(20), r"rac{a}{b}"),
            (TranscriptId(21), "$$"),
            (TranscriptId(22), "ordinary English prose continues here"),
            (TranscriptId(23), "$$"),
            (TranscriptId(24), "x = y"),
            (TranscriptId(25), "$$"),
        ];
        assert!(
            detect_math_blocks_in_context(window, DetectionContext::ambiguous()).is_empty(),
            "an unknown prefix cannot prove that the first symmetric delimiter opens"
        );
        assert_eq!(
            detect_math_blocks_in_context(
                [
                    (TranscriptId(1), "$$"),
                    (TranscriptId(2), "x = y"),
                    (TranscriptId(3), "$$"),
                ],
                DetectionContext::default(),
            )
            .len(),
            1,
            "the same complete block is valid at a trusted boundary"
        );
    }

    #[test]
    fn independent_latin_prose_line_is_never_a_display_body() {
        let blocks = detect_math_blocks([
            (TranscriptId(1), "$$"),
            (TranscriptId(2), "ordinary English prose continues here"),
            (TranscriptId(3), "$$"),
        ]);
        assert!(blocks.is_empty());
        assert_eq!(
            detect_math_blocks([
                (TranscriptId(1), "$$"),
                (TranscriptId(2), "x = y"),
                (TranscriptId(3), "$$"),
            ])
            .len(),
            1,
            "one-letter algebra must not be mistaken for prose"
        );
    }

    #[test]
    fn display_delimiters_and_bare_math_environment_whitelist_are_supported() {
        let cases = [
            (vec!["$$", "x + y", "$$"], "x + y"),
            (vec![r"\[", "x + y", r"\]"], "x + y"),
            (
                vec![r"\begin{align}", r"x &= y + 1", r"\end{align}"],
                r"\begin{align}",
            ),
        ];
        for (lines, expected_source) in cases {
            let blocks = detect_math_blocks(
                lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
            );
            assert_eq!(blocks.len(), 1, "{lines:?}");
            assert!(blocks[0].span.render_source.contains(expected_source));
        }

        for lines in [
            vec![r"\begin{document}", "x + y", r"\end{document}"],
            vec![r"\begin{itemize}", r"\item x", r"\end{itemize}"],
        ] {
            assert!(
                detect_math_blocks(
                    lines
                        .iter()
                        .enumerate()
                        .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
                )
                .is_empty(),
                "non-math environment matched: {lines:?}"
            );
        }
    }

    #[test]
    fn m1_9k_inline_body_display_openers_and_closers_span_logical_lines() {
        let cases = [
            (
                vec![r"$$\oint_0^1 x", r"+ y\,dx$$"],
                "\\oint_0^1 x\n+ y\\,dx",
            ),
            (
                vec![r"\[\oint_0^1 x", r"+ y\,dx\]"],
                "\\oint_0^1 x\n+ y\\,dx",
            ),
            (
                vec![r"\begin{align}x &= y", r"z &= 1\end{align}"],
                "\\begin{align}x &= y\nz &= 1\\end{align}",
            ),
        ];
        for (lines, expected_source) in cases {
            let blocks = detect_math_blocks(
                lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
            );
            assert_eq!(
                blocks.len(),
                1,
                "inline-body delimiter form was missed: {lines:?}"
            );
            assert_eq!(blocks[0].span.render_source, expected_source, "{lines:?}");
        }
    }

    #[test]
    fn m1_9k_commonmark_indented_code_keeps_display_math_literal() {
        assert!(
            detect_math_blocks([(TranscriptId(1), "    $$x^2$$")]).is_empty(),
            "four-space CommonMark code must never become a math decoration"
        );
        assert!(
            detect_math_blocks([(TranscriptId(1), "\t$$x^2$$")]).is_empty(),
            "tab-indented CommonMark code must never become a math decoration"
        );
    }

    #[test]
    fn m1_9k_math_occurrence_separates_source_render_kind_and_exact_live_cells() {
        let mut task = live_task(&[r"$$\text{", "中}$$"], 1);
        let inputs = Arc::make_mut(&mut task.inputs);
        inputs[0].continues = true;
        inputs[1].cell_boundaries = vec![(0, 0), (3, 2), (4, 3), (5, 4), (6, 5)];
        assert!(resolve_live_detection_task(&mut task));
        assert_eq!(task.span.original_source, r"$$\text{中}$$");
        assert_eq!(task.span.render_source, r"\text{中}");
        assert_eq!(task.span.delimiter_kind, DelimiterKind::Dollars);
        assert_eq!(task.span.cell_segments.len(), 2);
        assert_eq!(task.span.cell_segments[0].logical_line, 0);
        assert_eq!(
            task.span.cell_segments[0].source_line,
            MathSourceLine::LiveGrid(0)
        );
        assert_eq!(
            task.span.cell_segments[1].source_line,
            MathSourceLine::LiveGrid(1)
        );
        assert_eq!(task.span.cell_segments[1].cell_end, 5);
    }

    #[test]
    fn m1_9k_frozen_occurrence_uses_captured_cell_boundaries() {
        let text = r"$$\text{中}$$";
        let mut byte = 0u32;
        let mut cell = 0u32;
        let mut boundaries = vec![(byte, cell)];
        for character in text.chars() {
            byte += character.len_utf8() as u32;
            cell += if character == '中' { 2 } else { 1 };
            boundaries.push((byte, cell));
        }
        let mut record = DecorationRecord::frozen(stamp());
        let mut task = record
            .schedule_scan(
                TranscriptId(1),
                DetectionContext::default(),
                Arc::from([DetectionInput {
                    id: TranscriptId(1),
                    text: text.to_owned(),
                    captured_columns: 0,
                    cell_boundaries: boundaries,
                    site: InlineMathSite::CommandOutput,
                }]),
                DetectionOptions::default(),
            )
            .unwrap();
        assert!(resolve_detection_task(&mut task));
        assert_eq!(task.span.cell_segments.len(), 1);
        assert_eq!(task.span.cell_segments[0].cell_end, 13);
    }

    #[test]
    fn m1_9k_redline_a_unknown_symmetric_prefix_is_ambiguous_not_prose_math() {
        let scan = scan_math_blocks_in_context(
            [
                (TranscriptId(1), r"rac{a}{b}"),
                (TranscriptId(2), "$$"),
                (TranscriptId(3), "retrying"),
                (TranscriptId(4), "$$"),
            ],
            DetectionContext::ambiguous(),
        );
        assert!(scan.blocks.is_empty(), "ambiguous prose must remain source");
        assert!(!scan.ambiguous.is_empty(), "the refusal must be explicit");

        for lines in [
            vec![r"\[x + y", r"+ z\]"],
            vec![r"\begin{align}x &= y", r"z &= 1\end{align}"],
        ] {
            assert_eq!(
                scan_math_blocks_in_context(
                    lines
                        .iter()
                        .enumerate()
                        .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
                    DetectionContext::ambiguous(),
                )
                .blocks
                .len(),
                1,
                "directional delimiters remain provable with an unknown prefix: {lines:?}"
            );
        }
        assert_eq!(
            scan_math_blocks_in_context(
                [(TranscriptId(1), "$$x+y$$")],
                DetectionContext::ambiguous(),
            )
            .blocks
            .len(),
            1,
            "a self-contained symmetric occurrence is independently provable"
        );
    }

    #[test]
    fn m1_9k_redline_b_commonmark_code_context_never_renders_text() {
        for lines in [
            vec!["```text", "$$x$$", "```"],
            vec!["~~~text", "$$x$$", "~~~"],
            vec!["   ```text", "$$x$$", "   ```"],
            vec!["    $$x$$"],
            vec!["\t$$x$$"],
        ] {
            assert!(
                detect_math_blocks(
                    lines
                        .iter()
                        .enumerate()
                        .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
                )
                .is_empty(),
                "CommonMark code was decorated: {lines:?}"
            );
        }
    }

    #[test]
    fn m1_9k_complete_display_shape_matrix_is_covered() {
        for environment in [
            "equation",
            "equation*",
            "align",
            "align*",
            "alignat",
            "alignat*",
            "flalign",
            "flalign*",
            "gather",
            "gather*",
            "multline",
            "multline*",
            "aligned",
            "alignedat",
            "gathered",
            "split",
            "cases",
            "matrix",
            "pmatrix",
            "bmatrix",
            "Bmatrix",
            "vmatrix",
            "Vmatrix",
            "smallmatrix",
        ] {
            let lines = [
                format!(r"\begin{{{environment}}}x &= y"),
                format!(r"z &= 1\end{{{environment}}}"),
            ];
            let blocks = detect_math_blocks(
                lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| (TranscriptId(index as u64 + 1), line.as_str())),
            );
            assert_eq!(blocks.len(), 1, "environment shape missed: {environment}");
            assert_eq!(
                blocks[0].span.delimiter_kind,
                DelimiterKind::Environment(environment.to_owned())
            );
            assert_eq!(blocks[0].span.original_source, blocks[0].span.render_source);
        }

        let outer = detect_math_blocks([
            (TranscriptId(1), r"$$\begin{aligned}"),
            (TranscriptId(2), r"x &= y"),
            (TranscriptId(3), r"\end{aligned}$$"),
        ]);
        assert_eq!(outer.len(), 1);
        assert_eq!(outer[0].span.delimiter_kind, DelimiterKind::Dollars);
        assert!(outer[0].span.render_source.contains(r"\begin{aligned}"));

        for literal in [r"$x+y$", r"\(x+y\)", r"\$$x$$", "$$$$", "$$open"] {
            assert!(
                detect_math_blocks([(TranscriptId(1), literal)]).is_empty(),
                "{literal}"
            );
        }
    }

    fn detect_lines(lines: &[&str]) -> Vec<DetectedMathBlock> {
        detect_math_blocks(
            lines
                .iter()
                .enumerate()
                .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
        )
    }

    #[test]
    fn environment_closer_with_trailing_prose_punctuation_still_pairs() {
        // The independent audit's POISONED/CLEAN pair, pinned. A bare `\begin{pmatrix}` (Codex ate
        // the enclosing `\[`) whose closer reads `\end{pmatrix},` used to never close, so the
        // scanner stayed inside the open environment and swallowed every following `$$` block as
        // phantom body. Both the matrix and the `$$` block must now resolve.
        let poisoned = detect_lines(&[
            "matrix:",
            r"\begin{pmatrix}",
            "a & b",
            r"\end{pmatrix},",
            "energy:",
            "$$",
            "E=mc^2",
            "$$",
        ]);
        assert_eq!(
            poisoned.len(),
            2,
            "trailing comma must not poison the later $$ block: {poisoned:#?}"
        );
        assert!(
            poisoned
                .iter()
                .any(|block| block.span.render_source.contains(r"\begin{pmatrix}")),
            "the pmatrix environment must resolve"
        );
        assert!(
            poisoned
                .iter()
                .any(|block| block.span.render_source.contains("E=mc^2")),
            "the following $$ block must no longer be swallowed"
        );
        // The tolerated punctuation is prose, not formula: it never enters the rendered source.
        for block in &poisoned {
            assert!(
                !block.span.render_source.ends_with(','),
                "trailing prose punctuation leaked into the render source: {:?}",
                block.span.render_source
            );
        }
    }

    #[test]
    fn environment_closer_tolerates_each_prose_terminator_variant() {
        for punctuation in [",", ".", ";", ":", "，", "。", "、", "；", "！", "？"] {
            // Multi-line closer on its own line.
            let multiline = detect_lines(&[
                r"\begin{pmatrix}",
                "a & b",
                &format!(r"\end{{pmatrix}}{punctuation}"),
            ]);
            assert_eq!(
                multiline.len(),
                1,
                "multi-line closer refused trailing {punctuation:?}"
            );
            assert_eq!(
                multiline[0].span.delimiter_kind,
                DelimiterKind::Environment("pmatrix".to_owned())
            );
            assert!(
                !multiline[0].span.render_source.contains(punctuation),
                "trailing {punctuation:?} leaked into multi-line render source"
            );
            // Environment render source is exactly its original source (no punctuation on either).
            assert_eq!(
                multiline[0].span.original_source,
                multiline[0].span.render_source
            );

            // Single-line closer on the same line as the opener.
            let single = detect_lines(&[&format!(
                r"\begin{{pmatrix}}a & b\end{{pmatrix}}{punctuation}"
            )]);
            assert_eq!(
                single.len(),
                1,
                "single-line closer refused trailing {punctuation:?}"
            );
            assert!(
                !single[0].span.render_source.contains(punctuation),
                "trailing {punctuation:?} leaked into single-line render source"
            );
        }
    }

    #[test]
    fn environment_closer_still_refuses_arbitrary_trailing_content() {
        // Only a run of prose punctuation is tolerated. Any other trailing text means the line is
        // not the closer, so the environment keeps looking (here: never closes → nothing renders).
        assert!(
            detect_lines(&[r"\begin{pmatrix}", "a & b", r"\end{pmatrix} extra prose"]).is_empty(),
            "arbitrary trailing prose must not be accepted as an environment closer"
        );
        // `$` is structural, never prose punctuation: `\end{pmatrix}$$` is not an environment close.
        assert!(
            detect_lines(&[r"\begin{pmatrix}", "a & b", r"\end{pmatrix}$$"]).is_empty(),
            "a trailing $$ must not be swallowed as tolerated punctuation"
        );
    }

    #[test]
    fn unclosed_environment_is_abandoned_at_a_display_opener_not_swallowing_later_blocks() {
        // Swallow-radius bound: an environment whose closer is lost entirely (no `\end` at all)
        // must not consume the following `$$` block as phantom body — a `$$`/`\[` opener cannot
        // legally appear inside a math environment, so the stale opening is abandoned.
        let dollars = detect_lines(&[r"\begin{pmatrix}", "a & b", "$$", "E=mc^2", "$$"]);
        assert_eq!(
            dollars.len(),
            1,
            "the $$ block must survive an unclosed environment above it: {dollars:#?}"
        );
        assert!(dollars[0].span.render_source.contains("E=mc^2"));

        let brackets = detect_lines(&[r"\begin{pmatrix}", "a & b", r"\[", "E=mc^2", r"\]"]);
        assert_eq!(
            brackets.len(),
            1,
            "the \\[ block must also survive: {brackets:#?}"
        );
        assert!(brackets[0].span.render_source.contains("E=mc^2"));
    }

    #[test]
    fn genuinely_nested_environment_inside_dollars_is_not_abandoned() {
        // The abandonment is one-directional: a display `$$`/`\[` opening still legitimately nests
        // an inner directional environment as body. It must keep rendering as one Dollars block.
        let nested = detect_lines(&["$$", r"\begin{aligned}", "x &= y", r"\end{aligned}", "$$"]);
        assert_eq!(
            nested.len(),
            1,
            "nested environment broke $$ pairing: {nested:#?}"
        );
        assert_eq!(nested[0].span.delimiter_kind, DelimiterKind::Dollars);
        assert!(nested[0].span.render_source.contains(r"\begin{aligned}"));
    }

    #[test]
    fn streamed_dollars_wrapped_environment_resolves_only_as_the_complete_outer_block() {
        let arrivals = ["$$", r"\begin{aligned}", "x &= y", r"\end{aligned}", "$$"];
        for end in 1..arrivals.len() {
            assert!(
                detect_lines(&arrivals[..end]).is_empty(),
                "an incomplete streamed prefix must not let the inner environment run ahead: {:?}",
                &arrivals[..end]
            );
        }

        let complete = detect_lines(&arrivals);
        assert_eq!(complete.len(), 1);
        assert_eq!(complete[0].span.delimiter_kind, DelimiterKind::Dollars);
        assert_eq!(
            complete[0].span.original_source,
            "$$\n\\begin{aligned}\nx &= y\n\\end{aligned}\n$$"
        );
        assert_eq!(
            complete[0].span.render_source,
            "\\begin{aligned}\nx &= y\n\\end{aligned}"
        );
    }

    #[test]
    fn streamed_bare_environment_still_resolves_independently() {
        let arrivals = [r"\begin{pmatrix}", "a & b", r"\end{pmatrix}"];
        for end in 1..arrivals.len() {
            assert!(detect_lines(&arrivals[..end]).is_empty());
        }

        let complete = detect_lines(&arrivals);
        assert_eq!(complete.len(), 1);
        assert_eq!(
            complete[0].span.delimiter_kind,
            DelimiterKind::Environment("pmatrix".to_owned())
        );
        assert_eq!(
            complete[0].span.original_source,
            "\\begin{pmatrix}\na & b\n\\end{pmatrix}"
        );
    }

    #[test]
    fn dollars_wrapped_environment_does_not_bypass_the_prose_guard() {
        assert!(
            detect_lines(&[
                "$$",
                r"\begin{aligned}",
                "this is ordinary english prose that must remain source",
                r"\end{aligned}",
                "$$",
            ])
            .is_empty(),
            "an outer $$ owner must not turn an environment-shaped prose block into math"
        );
        assert!(
            detect_lines(&[
                "$$",
                r"\begin{aligned}",
                "这是一段普通的中文散文绝不能被排成公式",
                r"\end{aligned}",
                "$$",
            ])
            .is_empty(),
            "the CJK prose guard must remain active under an outer $$ owner"
        );
    }

    #[test]
    fn prose_body_is_still_rejected_even_when_the_closer_carries_punctuation() {
        // The loosened closer must not become a prose loophole: tolerating `\end{pmatrix},` for
        // *pairing* still leaves the prose-body guard to reject an environment whose interior is
        // ordinary sentences (English or CJK). Nothing renders.
        assert!(
            detect_lines(&[
                r"\begin{pmatrix}",
                "this is ordinary english prose that should never render",
                r"\end{pmatrix},",
            ])
            .is_empty(),
            "an English-prose body must stay source despite a tolerated closer"
        );
        assert!(
            detect_lines(&[
                r"\begin{pmatrix}",
                "这是一段普通的中文散文绝不能被排成公式",
                r"\end{pmatrix}。",
            ])
            .is_empty(),
            "a CJK-prose body must stay source despite a tolerated closer"
        );
        // And the abandonment path must not render the prose it steps over either: a bare
        // unclosed environment sitting above a real $$ block leaves the intervening prose native.
        let stepped = detect_lines(&[
            r"\begin{pmatrix}",
            "meanwhile some ordinary english prose is written here",
            "$$",
            "E=mc^2",
            "$$",
        ]);
        assert_eq!(
            stepped.len(),
            1,
            "only the $$ block should render: {stepped:#?}"
        );
        assert!(
            !stepped[0].span.render_source.contains("meanwhile"),
            "stepped-over prose leaked into the render source"
        );
    }

    #[test]
    fn single_line_display_forms_work_but_parenthesis_inline_stays_disabled() {
        for source in [r"\[x + y\]", r"\begin{equation}x+y\end{equation}"] {
            assert_eq!(detect_block_math(source).len(), 1, "{source}");
        }
        assert!(detect_block_math(r"\(x + y\)").is_empty());
        assert!(
            detect_math_blocks([(TranscriptId(1), r"\(x + y\)")]).is_empty(),
            "parenthesis inline detection must remain disabled"
        );
    }

    #[test]
    fn claude_code_jump_chip_overlay_is_not_baked_into_display_math() {
        let polluted = r"$$\hat{f}(\xi) = \int_{-\infty}^{\in Jump to bottom (ctrl+End) ↓ dx$$";
        assert!(detect_block_math(polluted).is_empty());

        let options = DetectionOptions {
            reject_claude_code_jump_chip_overlay: false,
            ..DetectionOptions::default()
        };
        assert_eq!(
            detect_math_blocks_with_options([(TranscriptId(1), polluted)], options).len(),
            1,
            "the CC-specific workaround must be removable after the overlay bug is fixed"
        );

        let clean = r"$$\hat{f}(\xi) = \int_{-\infty}^{\infty} dx$$";
        assert_eq!(detect_block_math(clean).len(), 1);
    }

    #[test]
    fn stripped_environment_newlines_are_restored_only_in_renderer_input() {
        let lines = [
            r"$$\begin{aligned}",
            r"F_x &= 0 \ ",
            r"F_y &= 1\",
            r"F_z &= 2\",
            r"\end{aligned}$$",
        ];
        let detected = detect_math_blocks(
            lines
                .iter()
                .enumerate()
                .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
        );
        assert_eq!(detected.len(), 1);
        assert_eq!(
            detected[0].span.render_source,
            concat!(
                r"\begin{aligned}",
                "\n",
                r"F_x &= 0 \\ ",
                "\n",
                r"F_y &= 1\\",
                "\n",
                r"F_z &= 2\",
                "\n",
                r"\end{aligned}"
            )
        );
        assert_eq!(
            detected[0].span.original_source,
            lines.join("\n"),
            "copy/source presentation must retain the exact terminal bytes"
        );
    }

    #[test]
    fn restore_switch_off_is_byte_identical_to_the_unrepaired_baseline() {
        let lines = [
            r"$$\begin{aligned}",
            r"x &= 0\",
            r"y &= 1\",
            r"\end{aligned}$$",
        ];
        let options = DetectionOptions {
            restore_stripped_environment_newlines: false,
            ..DetectionOptions::default()
        };
        let detected = detect_math_blocks_with_options(
            lines
                .iter()
                .enumerate()
                .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
            options,
        );
        assert_eq!(detected.len(), 1);
        assert_eq!(
            detected[0].span.render_source,
            concat!(
                r"\begin{aligned}",
                "\n",
                r"x &= 0\",
                "\n",
                r"y &= 1\",
                "\n",
                r"\end{aligned}"
            )
        );

        let inline = r"$$\begin{align}a&=b\c&=d\end{align}$$";
        let detected = detect_math_blocks_with_options([(TranscriptId(1), inline)], options);
        assert_eq!(detected.len(), 1);
        assert_eq!(
            detected[0].span.render_source,
            r"\begin{align}a&=b\c&=d\end{align}"
        );
    }

    #[test]
    fn existing_row_separators_and_non_environment_backslashes_are_unchanged() {
        let already_valid = [
            r"$$\begin{aligned}",
            r"x &= 0\\",
            r"y &= 
abla f",
            r"\end{aligned}$$",
        ];
        let detected = detect_math_blocks(
            already_valid
                .iter()
                .enumerate()
                .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
        );
        assert_eq!(
            detected[0].span.render_source,
            already_valid[0][2..].to_owned()
                + "\n"
                + already_valid[1]
                + "\n"
                + already_valid[2]
                + "\n"
                + &already_valid[3][..already_valid[3].len() - 2]
        );
        assert!(!detected[0].span.render_source.contains(r"\\\"));

        let outside = [r"$$", r"foo \", r"bar", r"$$"];
        let detected = detect_math_blocks(
            outside
                .iter()
                .enumerate()
                .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
        );
        assert_eq!(detected[0].span.render_source, "foo \\\nbar");
        assert_eq!(
            restore_stripped_environment_newlines(r"$$x \$$", true, true),
            r"$$x \$$"
        );
    }

    /// The TeX spacing family (`\;` `\,` `\:` `\!`) is a backslash followed by ASCII punctuation,
    /// which is exactly the shape a CommonMark backslash-unescape eats. Extraction must never be
    /// the thing that eats it: whatever arrives on the grid reaches the renderer byte-for-byte,
    /// with the row-separator repair switched on as it is in production. This pins both
    /// directions — a present backslash is never dropped, and an absent one is never invented,
    /// because a bare `;` is legitimate math (`f(x; \theta)`) and guessing would be fabrication.
    #[test]
    fn tex_spacing_commands_survive_extraction_byte_for_byte() {
        for body in [
            r"a \; + \; b",
            r"a \, b",
            r"a \: b",
            r"a \! b",
            r"\text{死} \; + \; \text{活}",
            r"f(x; \theta) \Rightarrow y",
            // The damaged form the user saw, carrying the command that keeps it math at all:
            // extraction must leave it damaged rather than silently "repairing" it into
            // something the terminal never received.
            r"\text{死};+;\text{活}",
        ] {
            let single = format!("$${body}$$");
            let detected = detect_math_blocks([(TranscriptId(1), single.as_str())]);
            assert_eq!(detected.len(), 1, "{body:?} must detect as one block");
            assert_eq!(
                detected[0].span.render_source, body,
                "single-line extraction altered {body:?}"
            );
            // `original_source` is the untouched grid text, delimiters included.
            assert_eq!(
                detected[0].span.original_source, single,
                "single-line original_source altered {body:?}"
            );

            // Same bytes inside a tabular environment, where the row-separator repair is armed
            // and actively scanning every backslash it finds.
            let lines = [r"$$\begin{aligned}", body, r"\end{aligned}$$"];
            let detected = detect_math_blocks(
                lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
            );
            assert_eq!(detected.len(), 1, "{body:?} must detect inside aligned");
            let expected = format!("\\begin{{aligned}}\n{body}\n\\end{{aligned}}");
            assert_eq!(
                detected[0].span.render_source, expected,
                "aligned extraction altered {body:?}"
            );
        }
    }

    #[test]
    fn matrix_and_cases_use_the_same_syntax_recovery_rule() {
        for environment in ["matrix", "cases"] {
            let lines = [
                r"$$".to_owned(),
                format!(r"\begin{{{environment}}}"),
                r"a & b \".to_owned(),
                r"c & d".to_owned(),
                format!(r"\end{{{environment}}}"),
                r"$$".to_owned(),
            ];
            let detected = detect_math_blocks(
                lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| (TranscriptId(index as u64 + 1), line.as_str())),
            );
            assert_eq!(detected.len(), 1, "{environment}");
            assert!(
                detected[0].span.render_source.contains("a & b \\\\\nc & d"),
                "{}",
                detected[0].span.render_source
            );
        }
    }

    #[test]
    fn stripped_single_line_matrix_separator_is_restored_only_in_renderer_input() {
        for source in [
            r"$$A=\begin{pmatrix}a&b\c&d\end{pmatrix}$$",
            r"$$A=\begin{pmatrix}a & b \ c & d\end{pmatrix}$$",
        ] {
            let detected = detect_math_blocks([(TranscriptId(1), source)]);
            assert_eq!(detected.len(), 1, "{source}");
            assert_eq!(
                detected[0].span.original_source, source,
                "copy/source presentation must retain the exact terminal bytes"
            );
            assert!(detected[0].span.render_source.contains(r"\begin{pmatrix}a"));
            assert!(
                detected[0].span.render_source.contains(r"\\c&")
                    || detected[0].span.render_source.contains(r"\\ c &"),
                "{}",
                detected[0].span.render_source
            );
        }

        for source in [
            r"$$\begin{pmatrix}a&\cos x&b\end{pmatrix}$$",
            r"$$\begin{pmatrix}a&\frac{1}{2}&b\end{pmatrix}$$",
            r"$$\begin{pmatrix}a&\cdot&b\end{pmatrix}$$",
        ] {
            let detected = detect_math_blocks([(TranscriptId(1), source)]);
            assert_eq!(detected.len(), 1);
            assert!(
                !detected[0].span.render_source.contains(r"\\cos")
                    && !detected[0].span.render_source.contains(r"\\frac")
                    && !detected[0].span.render_source.contains(r"\\cdot"),
                "a real command was mistaken for a stripped row separator: {}",
                detected[0].span.render_source
            );
        }
    }

    /// RED GATE (user report 2026-09-14): a **one-column** matrix loses its rows.
    ///
    /// The screenshot carried three matrices on two lines of one display block. The 2x2 came out
    /// 2x2 because its row separator was collapsed at a boundary this rule could read — an `&` on
    /// either side of it. The two column vectors beside it came out as single rows, `(x y)` and
    /// `(ax + by  cx + dy)`, because a one-column row has no ampersand anywhere and the recovery
    /// asked for one before it would look: the surviving `\` stayed a control space, and a control
    /// space joins cells rather than ending a row.
    ///
    /// MUTATIONS:
    /// ① require an `&` before the slash again — both column vectors go back to one row;
    /// ② drop the "slash stands on whitespace" bound — `x \, y` gains a row separator and the
    ///    thin space becomes a row break;
    /// ③ drop the bare-cell test — `\begin{pmatrix} x \alpha \end{pmatrix}` gains one too.
    #[test]
    fn a_one_column_row_separator_is_restored_without_an_ampersand_to_read_it_by() {
        for (source, expected) in [
            (
                r"$$\begin{pmatrix} x \ y \end{pmatrix}$$",
                r"\begin{pmatrix} x \\ y \end{pmatrix}",
            ),
            (
                r"$$\begin{pmatrix} ax + by \ cx + dy \end{pmatrix}$$",
                r"\begin{pmatrix} ax + by \\ cx + dy \end{pmatrix}",
            ),
            (
                r"$$\begin{bmatrix}x\y\end{bmatrix}$$",
                r"\begin{bmatrix}x\\y\end{bmatrix}",
            ),
        ] {
            let detected = detect_math_blocks([(TranscriptId(1), source)]);
            assert_eq!(detected.len(), 1, "{source}");
            assert_eq!(
                detected[0].span.original_source, source,
                "copy/source presentation must retain the exact terminal bytes"
            );
            assert_eq!(
                detected[0].span.render_source, expected,
                "one-column row separator: {source}"
            );
        }

        // What the whitespace bound and the bare-cell test are there to protect. A spacing command
        // carries its own character right behind the slash, and a real command carries its name.
        for source in [
            r"$$\begin{pmatrix} x \, y \end{pmatrix}$$",
            r"$$\begin{pmatrix} x \; y \end{pmatrix}$$",
            r"$$\begin{pmatrix} x \alpha \end{pmatrix}$$",
            r"$$\begin{pmatrix} x \ \frac{1}{2} \end{pmatrix}$$",
        ] {
            let detected = detect_math_blocks([(TranscriptId(1), source)]);
            assert_eq!(detected.len(), 1, "{source}");
            assert!(
                !detected[0].span.render_source.contains(r"\\"),
                "a command was mistaken for a stripped row separator in {source}: {}",
                detected[0].span.render_source
            );
        }

        // The 2x2 on the line above, unchanged: the two rules answer the same source the same way.
        let detected = detect_math_blocks([(
            TranscriptId(1),
            r"$$\begin{pmatrix} a & b \ c & d \end{pmatrix}$$",
        )]);
        assert_eq!(
            detected[0].span.render_source,
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"
        );

        let restored = restore_stripped_environment_newlines(
            r"\begin{pmatrix} x \ y \end{pmatrix}",
            true,
            true,
        );
        assert_eq!(
            restore_stripped_environment_newlines(&restored, true, true),
            restored,
            "one-column recovery must be idempotent"
        );
    }

    #[test]
    fn stripped_single_line_separator_is_restored_in_all_tabular_math_environments() {
        for environment in [
            "smallmatrix",
            "vmatrix",
            "cases",
            "aligned",
            "align",
            "align*",
            "alignat",
            "alignat*",
            "flalign",
            "flalign*",
        ] {
            let source = format!(r"$$\begin{{{environment}}}a&b\c&d\end{{{environment}}}$$");
            let detected = detect_math_blocks([(TranscriptId(1), source.as_str())]);
            assert_eq!(detected.len(), 1, "{environment}");
            assert_eq!(
                detected[0].span.original_source, source,
                "{environment}: source bytes changed"
            );
            assert!(
                detected[0].span.render_source.contains(r"a&b\\c&d"),
                "{environment}: {}",
                detected[0].span.render_source
            );
        }

        let nested = r"$$\begin{aligned}A&=\begin{vmatrix}a&b\c&d\end{vmatrix}\x&=y\end{aligned}$$";
        let detected = detect_math_blocks([(TranscriptId(1), nested)]);
        assert_eq!(detected.len(), 1);
        assert_eq!(
            detected[0].span.render_source,
            r"\begin{aligned}A&=\begin{vmatrix}a&b\\c&d\end{vmatrix}\\x&=y\end{aligned}"
        );

        let restored = restore_stripped_environment_newlines(
            r"\begin{smallmatrix}a&b\c&d\end{smallmatrix}",
            true,
            true,
        );
        assert_eq!(
            restore_stripped_environment_newlines(&restored, true, true),
            restored,
            "inline recovery must be idempotent"
        );
    }

    #[test]
    fn final_line_of_the_innermost_environment_does_not_gain_a_separator() {
        let lines = [
            r"\begin{equation}",
            r"\begin{aligned}",
            r"x &= 0 \ ",
            r"y &= 1\",
            r"\end{aligned}",
            r"\end{equation}",
        ];
        let detected = detect_math_blocks(
            lines
                .iter()
                .enumerate()
                .map(|(index, line)| (TranscriptId(index as u64 + 1), *line)),
        );
        assert_eq!(detected.len(), 1);
        assert!(
            detected[0]
                .span
                .render_source
                .contains("x &= 0 \\\\ \ny &= 1\\\n")
        );
        assert!(!detected[0].span.render_source.contains("y &= 1\\\\\n"));
    }

    /// The completeness rule, isolated from the rest of the gates.
    ///
    /// Every pair here sits at `CommandOutput` with a genuine math signal present, so site and
    /// signal are both satisfied and the *only* thing separating the two columns is whether the
    /// span reads as a whole expression. This is the test that fails if someone ever "simplifies"
    /// `inline_source_is_complete` back into a bag of interesting characters.
    #[test]
    fn a_severed_expression_is_not_math_but_the_whole_one_beside_it_is() {
        for (truncated, whole) in [
            ("a $5+$10", "a $5+x$"),
            ("q $1 AND b=$2", "q $1 + b=2$"),
            ("p $HOME/bin:$PATH", "p $HOME/bin$"),
            ("f $g(x$ y", "f $g(x)$ y"),
            ("s $a_$b", "s $a_b$"),
        ] {
            assert!(
                detect_inline_math(truncated, InlineMathSite::CommandOutput).is_empty(),
                "severed expression must stay native: {truncated}"
            );
            assert!(
                !detect_inline_math(whole, InlineMathSite::CommandOutput).is_empty(),
                "complete expression must be typeset: {whole}"
            );
        }
    }

    #[test]
    fn a_fenced_code_block_is_never_inline_math() {
        assert!(
            detect_math_blocks([
                (TranscriptId(1), "```text"),
                (TranscriptId(2), "code $x^2$"),
                (TranscriptId(3), "```"),
            ])
            .is_empty()
        );
    }

    fn live_task(lines: &[&str], candidate_row: u32) -> LiveDetectionTask {
        LiveDetectionTask {
            candidate_row,
            screen: ScreenId::Alternate,
            grid_generation: GridGeneration(7),
            detection_revision: DetectionRevision(1),
            layout: stamp().layout,
            cell_width_subpixels: 9 * SUBPIXELS_PER_PX,
            cell_height_subpixels: 18 * SUBPIXELS_PER_PX,
            ascii_baseline_subpixels: 14 * SUBPIXELS_PER_PX,
            options: DetectionOptions::default(),
            initial_context: DetectionContext::default(),
            inputs: Arc::from(
                lines
                    .iter()
                    .enumerate()
                    .map(|(row, text)| LiveDetectionInput {
                        source: LiveDetectionSource::Grid {
                            row: row as u32,
                            revision: 1,
                        },
                        text: (*text).to_owned(),
                        continues: false,
                        captured_columns: 0,
                        cell_boundaries: scalar_boundaries(text),
                        site: InlineMathSite::Ineligible,
                    })
                    .collect::<Vec<_>>(),
            ),
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
                kind: BlockKind::Math,
                cell_segments: Vec::new(),
                inline_runs: Vec::new(),
                inline_joined_head: None,
            },
            detection_complete: false,
            resolved: false,
            refused_table_rows: Vec::new(),
        }
    }

    fn scalar_boundaries(text: &str) -> Vec<(u32, u32)> {
        let mut boundaries = text
            .char_indices()
            .enumerate()
            .map(|(cell, (byte, _))| (byte as u32, cell as u32))
            .collect::<Vec<_>>();
        boundaries.push((text.len() as u32, text.chars().count() as u32));
        boundaries
    }

    #[test]
    fn soft_wrap_join_does_not_create_a_restoration_boundary() {
        let mut task = live_task(
            &[
                r"$$\begin{aligned}",
                r"x &= 0 \",
                r"+ 1",
                r"y &= 2",
                r"\end{aligned}$$",
            ],
            4,
        );
        Arc::make_mut(&mut task.inputs)[1].continues = true;
        assert!(resolve_live_detection_task(&mut task));
        assert!(task.span.render_source.contains("x &= 0 \\+ 1\ny &= 2"));
        assert!(!task.span.render_source.contains("x &= 0 \\\\+ 1"));
    }

    #[test]
    fn live_path_reuses_all_nine_zero_tolerance_false_positive_disciplines() {
        let huge = "x".repeat(MAX_MATH_SOURCE_BYTES + 1);
        let cases = [
            (vec!["echo $$"], 0, "shell echo"),
            (vec!["pid=$$"], 0, "shell pid"),
            (vec!["+ $$x^2$$"], 0, "diff line"),
            (vec!["2026-07-18 log: $$x^2$$"], 0, "log prose"),
            (vec![r"\$$x^2$$"], 0, "escaped delimiter"),
            (vec!["prefix $$x^2$$ suffix"], 0, "inline prose"),
            (vec!["$$broken"], 0, "unclosed single line"),
            (vec!["```sh", "$$x$$", "```"], 1, "code fence"),
            (vec!["$$", huge.as_str(), "$$"], 2, "over-size block"),
        ];
        for (lines, candidate, name) in cases {
            let mut task = live_task(&lines, candidate);
            assert!(!resolve_live_detection_task(&mut task), "{name}");
        }
    }

    #[test]
    fn live_detector_resolves_grid_points_without_transcript_anchor_escape() {
        let mut task = live_task(&["$$", "x + y", "$$"], 2);
        assert!(resolve_live_detection_task(&mut task));
        assert_eq!(task.start, GridPoint { row: 0, column: 0 });
        assert_eq!(task.end, GridPoint { row: 2, column: 2 });
        assert_eq!(task.span.render_source, "x + y");
    }

    /// A `$$…$$` block whose opener and body already scrolled into frozen history while the closer
    /// is still in the live grid must resolve: detection over the combined context proves the whole
    /// pairing, the live-grid closer carries the anchor, and the leading frozen rows are kept as
    /// `Transcript` segments so the presentation layer can bridge both domains.
    #[test]
    fn live_block_split_across_frozen_boundary_anchors_on_the_live_closer() {
        let mut task = live_task(&["placeholder"], 0);
        task.inputs = boundary_inputs(&[
            (
                LiveDetectionSource::History {
                    id: TranscriptId(296),
                },
                "$$",
            ),
            (
                LiveDetectionSource::History {
                    id: TranscriptId(297),
                },
                r"\sum_{k=1}^{n}k=\frac{n(n+1)}{2}",
            ),
            (
                LiveDetectionSource::Grid {
                    row: 0,
                    revision: 1,
                },
                "$$",
            ),
        ]);
        // The candidate is the live-grid closer row.
        assert!(resolve_live_detection_task(&mut task));
        assert_eq!(task.start, GridPoint { row: 0, column: 0 });
        assert_eq!(task.end.row, 0);
        assert_eq!(task.band_start_row, 0);
        assert_eq!(task.band_end_row, 0);
        assert_eq!(task.span.render_source, r"\sum_{k=1}^{n}k=\frac{n(n+1)}{2}");
        let sources = task
            .span
            .cell_segments
            .iter()
            .map(|segment| segment.source_line)
            .collect::<Vec<_>>();
        assert_eq!(
            sources.first(),
            Some(&MathSourceLine::Transcript(TranscriptId(296)))
        );
        assert!(sources.contains(&MathSourceLine::Transcript(TranscriptId(297))));
        assert_eq!(sources.last(), Some(&MathSourceLine::LiveGrid(0)));
    }

    /// A `$$…$$` block that has already fully committed to frozen history (closer included) ends in
    /// history, not at the live-grid candidate row, so the live path must not anchor it: the frozen
    /// detector owns it, and there is no boundary split to bridge.
    #[test]
    fn fully_frozen_block_is_not_anchored_by_the_live_path() {
        let mut task = live_task(&["tail"], 0);
        task.inputs = boundary_inputs(&[
            (
                LiveDetectionSource::History {
                    id: TranscriptId(10),
                },
                "$$",
            ),
            (
                LiveDetectionSource::History {
                    id: TranscriptId(11),
                },
                "x + y",
            ),
            (
                LiveDetectionSource::History {
                    id: TranscriptId(12),
                },
                "$$",
            ),
            (
                LiveDetectionSource::Grid {
                    row: 0,
                    revision: 1,
                },
                "unrelated",
            ),
        ]);
        assert!(!resolve_live_detection_task(&mut task));
    }

    fn boundary_inputs(rows: &[(LiveDetectionSource, &str)]) -> Arc<[LiveDetectionInput]> {
        rows.iter()
            .map(|(source, text)| LiveDetectionInput {
                source: *source,
                text: (*text).to_owned(),
                continues: false,
                captured_columns: 0,
                cell_boundaries: scalar_boundaries(text),
                site: InlineMathSite::Ineligible,
            })
            .collect()
    }

    // ---- Batch ⑥ ownership ledger + split red gate ----

    fn hist(id: u64, text: &str) -> (LiveDetectionSource, &str) {
        (
            LiveDetectionSource::History {
                id: TranscriptId(id),
            },
            text,
        )
    }

    fn grid(row: u32, text: &str) -> (LiveDetectionSource, &str) {
        (LiveDetectionSource::Grid { row, revision: 1 }, text)
    }

    fn ledger_of(rows: &[(LiveDetectionSource, &str)], ctx: DetectionContext) -> OwnershipLedger {
        let inputs = boundary_inputs(rows);
        live_detection_ownership_ledger(&inputs, ctx, DetectionOptions::default())
    }

    /// A soft-wrapped line is scanned as one string, so its site must be the *conjunction* of its
    /// fragments' — a line half of which the shell never claimed to have printed is not output.
    ///
    /// Red gate: the all-output case is asserted alongside, so a rule that simply answered
    /// `Ineligible` for every wrapped line could not pass.
    #[test]
    fn a_wrapped_line_is_command_output_only_when_every_fragment_is() {
        let wrapped = |sites: [InlineMathSite; 3]| {
            let inputs = sites
                .iter()
                .enumerate()
                .map(|(row, site)| LiveDetectionInput {
                    source: LiveDetectionSource::Grid {
                        row: row as u32,
                        revision: 1,
                    },
                    text: "abc".to_owned(),
                    // The first two rows soft-wrap into the next, so all three join into one line.
                    continues: row < 2,
                    captured_columns: 0,
                    cell_boundaries: scalar_boundaries("abc"),
                    site: *site,
                })
                .collect::<Vec<_>>();
            let logical = live_logical_lines(&inputs);
            assert_eq!(
                logical.len(),
                1,
                "the fixture must join into one logical line"
            );
            logical[0].site
        };
        use InlineMathSite::{CommandOutput, Ineligible};
        assert_eq!(
            wrapped([CommandOutput, CommandOutput, CommandOutput]),
            CommandOutput
        );
        for seam in 0..3 {
            let mut sites = [CommandOutput; 3];
            sites[seam] = Ineligible;
            assert_eq!(
                wrapped(sites),
                Ineligible,
                "one ineligible fragment at index {seam} makes the whole joined line ineligible"
            );
        }
    }

    fn has_reason(ledger: &OwnershipLedger, reason: LegitimateRejection) -> bool {
        ledger
            .entries
            .iter()
            .any(|entry| entry.fate == TokenFate::Rejected(reason.clone()))
    }

    /// Fidelity pin: the ledger is threaded through the REAL scanner, so its owned-block count must
    /// equal the authoritative detector's block count on the same input. If the recorder ever
    /// diverged from detection, this fails — which is the guarantee that the ledger observes rather
    /// than re-derives, and that the `None` product path stays byte-identical.
    #[test]
    fn ledger_owned_blocks_match_the_authoritative_detector() {
        let rows = [
            hist(100, "$$"),
            hist(101, r"\det"),
            hist(102, r"\begin{pmatrix}"),
            hist(103, r"a & b\"),
            hist(104, "c & d"),
            hist(105, r"\end{pmatrix}"),
            hist(106, "=ad-bc"),
            grid(0, "$$"),
            grid(1, ""),
            grid(2, "$$"),
            grid(3, r"\nabla^2 u=x"),
            grid(4, "$$"),
        ];
        let inputs = boundary_inputs(&rows);
        let logical = live_logical_lines(&inputs);
        let boundary = live_grid_boundary_index(&logical, &inputs);
        let clipped = clipped_open_index(
            &logical,
            live_grid_first_index(&logical, &inputs),
            &DetectionContext::default(),
            DetectionOptions::default(),
        );
        let blocks = detect_live_math_blocks_in_context(
            logical
                .iter()
                .map(|line| (line.id, line.text.clone()))
                .collect::<Vec<_>>()
                .iter()
                .map(|(id, text)| (*id, text.as_str())),
            DetectionContext::default(),
            DetectionOptions::default(),
            Some(&live_logical_sites(&logical)),
            boundary,
            clipped,
        );
        let ledger = live_detection_ownership_ledger(
            &inputs,
            DetectionContext::default(),
            DetectionOptions::default(),
        );
        assert_eq!(
            ledger.detected(),
            blocks.len(),
            "ledger owned-block count must equal the detector's block count"
        );
        assert_eq!(
            ledger.containment(&[]).orphans,
            0,
            "healthy input has no orphans"
        );
    }

    /// Batch ③: the ledger carries the exact `original_source` of every Owned display block, so a
    /// still-displayed hold can be judged backed by the same source-equality key holds re-anchor on.
    /// A detected block's source is owned; a prose-rejected pair's source is not — a hold on the
    /// latter would be `HeldUnbacked`. `owned_block_sources` is exactly the Owned display set.
    #[test]
    fn owned_block_sources_back_detected_display_blocks_only() {
        let rows = [
            grid(0, "$$"),
            grid(1, r"\nabla^2 u = x"),
            grid(2, "$$"),
            grid(3, "$$"),
            grid(4, "the quick brown fox jumps over"),
            grid(5, "$$"),
        ];
        let ledger = ledger_of(&rows, DetectionContext::default());
        let inputs = boundary_inputs(&rows);
        let blocks = detect_live_math_blocks_in_context(
            live_logical_lines(&inputs)
                .iter()
                .map(|line| (line.id, line.text.clone()))
                .collect::<Vec<_>>()
                .iter()
                .map(|(id, text)| (*id, text.as_str())),
            DetectionContext::default(),
            DetectionOptions::default(),
            Some(&live_logical_sites(&live_logical_lines(&inputs))),
            live_grid_boundary_index(&live_logical_lines(&inputs), &inputs),
            None,
        );
        assert_eq!(ledger.detected(), 1);
        assert_eq!(blocks.len(), 1);
        assert_eq!(ledger.owned_block_sources.len(), ledger.detected());
        // The detected block's exact source is owned — a hold on it is backed.
        assert!(ledger.owns_source(&blocks[0].span.original_source));
        // The prose-rejected pair is NOT owned — a hold on it would be HeldUnbacked.
        assert!(!ledger.owns_source("$$\nthe quick brown fox jumps over\n$$"));
        assert!(!ledger.owns_source("$$never detected$$"));
    }

    /// A `$$…$$` pair whose body is natural-language prose is a legitimate `GuardRejectedBody`, not
    /// an orphan — the M1.9k prose red line, accounted parity-neutrally.
    #[test]
    fn prose_body_pair_is_a_legitimate_guard_rejection_not_an_orphan() {
        let ledger = ledger_of(
            &[
                grid(0, "$$"),
                grid(1, "the quick brown fox jumps"),
                grid(2, "$$"),
            ],
            DetectionContext::default(),
        );
        assert!(has_reason(&ledger, LegitimateRejection::GuardRejectedBody));
        assert_eq!(ledger.containment(&[]).orphans, 0);
        assert_eq!(ledger.detected(), 0);
    }

    /// A `$$` inside a fenced code block is inert text: a `CommonMarkCodeContext` rejection.
    #[test]
    fn code_fenced_dollars_is_a_legitimate_code_context_rejection() {
        let ledger = ledger_of(
            &[grid(0, "```text"), grid(1, "$$"), grid(2, "```")],
            DetectionContext::default(),
        );
        assert!(has_reason(
            &ledger,
            LegitimateRejection::CommonMarkCodeContext
        ));
        assert_eq!(ledger.containment(&[]).orphans, 0);
    }

    /// M1.9p: a symmetric multi-line `$$` opener under an `Ambiguous` scrollback prefix is refused,
    /// recorded as `AmbiguousPrefixSuppressed`, never an orphan.
    #[test]
    fn ambiguous_prefix_dollars_is_suppressed_not_orphaned() {
        let ledger = ledger_of(
            &[grid(0, "$$"), grid(1, "x = y")],
            DetectionContext::ambiguous(),
        );
        assert!(has_reason(
            &ledger,
            LegitimateRejection::AmbiguousPrefixSuppressed
        ));
        assert_eq!(ledger.containment(&[]).orphans, 0);
    }

    /// A closer whose opener is carried in from the initial context (above the window) is
    /// `OpenerAboveWindow` — owned by an existing occurrence, not this window; never an orphan.
    #[test]
    fn closer_for_an_above_window_opener_is_legitimate() {
        let mut ctx = DetectionContext::default();
        advance_detection_context(&mut ctx, TranscriptId(1), "$$");
        let ledger = ledger_of(&[grid(0, r"\gamma = 2"), grid(1, "$$")], ctx);
        assert!(has_reason(&ledger, LegitimateRejection::OpenerAboveWindow));
        assert_eq!(ledger.containment(&[]).orphans, 0);
    }

    /// A trailing `$$` opener still streaming at the end of a pure-grid region is the single
    /// legitimate `StreamingUnclosedTail`, not an orphan.
    #[test]
    fn trailing_unclosed_opener_is_a_streaming_tail() {
        let ledger = ledger_of(
            &[grid(0, "$$"), grid(1, "x = 1")],
            DetectionContext::default(),
        );
        assert!(has_reason(
            &ledger,
            LegitimateRejection::StreamingUnclosedTail
        ));
        assert_eq!(ledger.containment(&[]).orphans, 0);
    }

    /// GREEN containment: an odd-`$$` frozen-history reflow (a lost opener) is contained by the
    /// resync — the phantom frozen opener is abandoned (`PhantomOpenerAbandoned`) and the live grid
    /// re-pairs cleanly. The damage does not spill: zero orphans (the live-norender class).
    #[test]
    fn contained_phantom_abandon_leaves_no_orphan() {
        let ledger = ledger_of(
            &[
                hist(1, "$$"),
                hist(2, r"\alpha = 1"),
                hist(3, "$$"),
                hist(4, "$$"), // dangling phantom opener from a reflow
                hist(5, "some prose here now"),
                grid(0, "$$"),
                grid(1, r"\gamma = 2"),
                grid(2, "$$"),
            ],
            DetectionContext::default(),
        );
        assert!(has_reason(
            &ledger,
            LegitimateRejection::PhantomOpenerAbandoned
        ));
        let verdict = ledger.containment(&[]);
        assert_eq!(verdict.orphans, 0, "contained damage must not red the gate");
        assert!(!verdict.red);
        assert!(verdict.detected >= 2, "both clean blocks resolve");
    }

    // ---- Frozen certified-boundary resync (review §A `FrozenResyncWitness`) ----
    //
    // The scroll-strand class: a history reflow ate one `$$` opener, leaving a lost-opener parity
    // phantom in PURE frozen history (no live seam, so the live-boundary resync never fires). The
    // frozen resync applies the same body-invalid + forward-valid witness from a `Known` prefix, so
    // an eaten opener no longer shifts every following `$$` by one and strands each clean block.

    /// Core recovery: the block below an eaten `$$` opener is stranded by the naive scan and
    /// recovered by the frozen resync, while the orphan closer is never fabricated into a wrapper.
    #[test]
    fn frozen_resync_recovers_the_block_below_an_eaten_dollar_opener() {
        let lines = [
            (TranscriptId(1), r"\begin{pmatrix}"),
            (TranscriptId(2), "a & b"),
            (TranscriptId(3), r"\end{pmatrix}"),
            (TranscriptId(4), "$$"), // orphan closer — its opener `$$` was eaten by the reflow
            (TranscriptId(5), ""),
            (TranscriptId(6), "$$"), // the real opener of the block below
            (TranscriptId(7), "E = mc^2"),
            (TranscriptId(8), "$$"), // the real closer
        ];
        // Baseline reproduces the poison: the phantom pairs (4,6) over a blank body and strands the
        // clean block below.
        let baseline = detect_math_blocks(lines);
        assert!(
            !baseline
                .iter()
                .any(|block| block.span.render_source.contains("E = mc^2")),
            "baseline must reproduce the strand (clean block below the eaten opener is lost)"
        );
        // The frozen resync abandons the phantom `$$` (id 4) and re-pairs (6,8) cleanly.
        let resync = detect_frozen_math_blocks_in_context_with_options(
            lines,
            DetectionContext::default(),
            DetectionOptions::default(),
            None,
        );
        assert!(
            resync
                .iter()
                .any(|block| block.span.render_source.contains("E = mc^2")),
            "frozen resync must recover the block below the eaten opener"
        );
        // The pmatrix still renders standalone; the orphan `$$` is never made an opener (no
        // fabricated wrapper — M1.9p).
        assert!(
            resync
                .iter()
                .any(|block| matches!(block.span.delimiter_kind, DelimiterKind::Environment(_))),
            "the surviving environment renders on its own"
        );
        assert!(
            resync
                .iter()
                .all(|block| block.start != TranscriptId(4) && block.end != TranscriptId(4)),
            "the orphan closer must never anchor a block"
        );
    }

    /// M1.9p: under an `Ambiguous` scrollback prefix the symmetric `$$` direction is undecidable,
    /// so the resync stays inert — byte-identical to the ordinary ambiguous scan, never a guess.
    #[test]
    fn frozen_resync_is_inert_under_an_ambiguous_prefix() {
        let lines = [
            (TranscriptId(1), "$$"),
            (TranscriptId(2), "some prose paragraph here"),
            (TranscriptId(3), "$$"),
            (TranscriptId(4), "E = mc^2"),
            (TranscriptId(5), "$$"),
        ];
        let resynced = detect_frozen_math_blocks_in_context_with_options(
            lines,
            DetectionContext::ambiguous(),
            DetectionOptions::default(),
            None,
        );
        let baseline = detect_math_blocks_in_context(lines, DetectionContext::ambiguous());
        assert_eq!(
            resynced, baseline,
            "the resync must be byte-identical to the ambiguous baseline"
        );
    }

    /// The forward-valid arm of the witness forbids rendering prose: when the "block below" an
    /// orphan closer is prose (no valid forward block), the phantom is NOT abandoned into it.
    #[test]
    fn frozen_resync_never_renders_prose_when_it_cannot_re_pair_forward() {
        let lines = [
            (TranscriptId(1), r"\begin{pmatrix}"),
            (TranscriptId(2), "a & b"),
            (TranscriptId(3), r"\end{pmatrix}"),
            (TranscriptId(4), "$$"),
            (TranscriptId(5), "ordinary english prose sentence here"),
            (TranscriptId(6), "$$"),
        ];
        let resync = detect_frozen_math_blocks_in_context_with_options(
            lines,
            DetectionContext::default(),
            DetectionOptions::default(),
            None,
        );
        assert!(
            resync
                .iter()
                .all(|block| !block.span.render_source.contains("prose")),
            "prose between an orphan closer and the next $$ must never render"
        );
    }

    /// A lone trailing `$$` (the id=171 strand-tail form) stays source: it opens nothing and the
    /// resync never fabricates a body for it.
    #[test]
    fn frozen_resync_keeps_a_lone_trailing_dollar_as_source() {
        let lines = [
            (TranscriptId(1), "$$"),
            (TranscriptId(2), "E = mc^2"),
            (TranscriptId(3), "$$"),
            (TranscriptId(4), "$$"), // lone trailing opener — a streaming tail, never a block
        ];
        let resync = detect_frozen_math_blocks_in_context_with_options(
            lines,
            DetectionContext::default(),
            DetectionOptions::default(),
            None,
        );
        assert_eq!(resync.len(), 1, "only the genuine E=mc^2 block resolves");
        assert!(
            resync.iter().all(|block| block.end != TranscriptId(4)),
            "the trailing $$ never closes a block"
        );
    }

    /// Convergence guard (shared with the live seam, `3875209`): a genuine block whose body merely
    /// trips the prose heuristic is a real closer, not a phantom — re-reading it forward yields no
    /// valid block, so it is never abandoned and parity below is preserved.
    #[test]
    fn frozen_resync_does_not_abandon_a_real_closer_whose_body_looks_prose() {
        // `= ad - bc` is real math the whitespace-word prose guard tolerates; a following clean
        // block must still resolve, proving no spurious abandon shifted parity.
        let lines = [
            (TranscriptId(1), "$$"),
            (TranscriptId(2), "M = a - b"),
            (TranscriptId(3), "$$"),
            (TranscriptId(4), ""),
            (TranscriptId(5), "$$"),
            (TranscriptId(6), "E = mc^2"),
            (TranscriptId(7), "$$"),
        ];
        let resync = detect_frozen_math_blocks_in_context_with_options(
            lines,
            DetectionContext::default(),
            DetectionOptions::default(),
            None,
        );
        assert_eq!(
            resync.len(),
            2,
            "both genuine blocks resolve, parity intact"
        );
    }

    /// CONTAINED (batch 2, ④ + evidence-driven ②). This was the batch-2 baseline RED test
    /// (`clipped_open_grid_is_an_unannotated_orphan_that_reds_containment`): the compress-rewrite /
    /// round-3 topology — grid row 0 is inside a display block whose opener scrolled above it, and the
    /// parser reaches the seam in the closed phase, so the first grid `$$` is really that block's
    /// CLOSER. Batch 2 makes the clip-aware scanner consume it as a legitimate above-window closer
    /// (occlusion; never a synthesized opener, never a global force-closed), so the grid re-pairs from
    /// a clean closed state and every block BELOW the clip is detected and renders — the unit analogue
    /// of `\zeta` recovering in compress-rewrite.vt. The former assertions (`clipped_open`, `orphans >=
    /// 1`, `red`) encoded the pre-fix RED state this batch exists to flip; they are replaced, not
    /// weakened, by the contained-state assertions below.
    #[test]
    fn clipped_open_closer_is_contained_and_the_block_below_re_pairs() {
        let ledger = ledger_of(
            &[
                hist(1, "intro paragraph text"),
                grid(0, r"\int_{-\infty}^{\infty}"),
                grid(1, "f(t)"),
                grid(2, "$$"), // clipped block's closer — consumed as an above-window closer
                grid(3, "$$"), // a fresh block below the clip...
                grid(4, r"\gamma = 2"),
                grid(5, "$$"), // ...re-pairs cleanly and is detected
            ],
            DetectionContext::default(),
        );
        let verdict = ledger.containment(&[]);
        assert!(
            has_reason(&ledger, LegitimateRejection::OpenerAboveWindow),
            "the clipped closer is contained as an above-window closer"
        );
        assert_eq!(verdict.orphans, 0, "the clip does not spill an orphan");
        assert!(
            !verdict.red,
            "a contained clip is not a containment failure"
        );
        assert!(
            !verdict.clipped_open,
            "no uncontained clipped-open orphan remains"
        );
        assert_eq!(
            verdict.detected, 1,
            "the block below the clip re-pairs and is detected"
        );
    }

    /// The same clip, on the screen where it actually happens: a **pure grid** context, with no
    /// frozen line in front of it at all. An alternate-screen TUI owns its whole window and redraws
    /// it in place, so when its content moves up the rows that leave the top are overwritten rather
    /// than removed — no scroll event, nothing to advance the alternate context, and grid row 0
    /// simply begins inside a block whose opener is gone. That is this topology and nothing else,
    /// yet every clip fixture above carries a `hist` line, and the clip evidence was keyed to the
    /// frozen→live seam, which a grid-only window does not have. Without the clip the first grid
    /// `$$` reads as an opener and swallows the prose beneath it, and every `$$` below is shifted by
    /// one — so the genuine block at the bottom of the window is never paired at all.
    #[test]
    fn a_grid_only_clipped_open_closer_is_contained_and_the_block_below_re_pairs() {
        let ledger = ledger_of(
            &[
                grid(0, r"\nabla \times \mathbf{B} &= \mu_0\mathbf{J}"),
                grid(1, r"\end{aligned}"),
                grid(2, "$$"), // clipped block's closer — consumed as an above-window closer
                grid(3, ""),
                grid(4, "prose between two blocks"),
                grid(5, ""),
                grid(6, "$$"), // a fresh block below the clip...
                grid(7, r"\gamma = 2"),
                grid(8, "$$"), // ...re-pairs cleanly and is detected
            ],
            DetectionContext::default(),
        );
        let verdict = ledger.containment(&[]);
        assert!(
            has_reason(&ledger, LegitimateRejection::OpenerAboveWindow),
            "a grid-only window's clipped closer is contained as an above-window closer"
        );
        assert_eq!(verdict.orphans, 0, "the clip does not spill an orphan");
        assert!(!verdict.red);
        assert!(!verdict.clipped_open);
        assert_eq!(
            verdict.detected, 1,
            "the block below the clip re-pairs and is detected on a grid-only window"
        );
    }

    /// Containment isolation semantics on a GENUINE orphan (`UnbalancedResidue`). This replaces the
    /// batch-2 baseline test (`source_integrity_annotation_isolates_known_damage_but_new_orphan_still
    /// _reds`), whose clipped-open scenario no longer produces an orphan (it is contained above). The
    /// two-layer gate still needs coverage against a real orphan, so it is exercised here on a lone
    /// `$$` opener whose closer was replaced by a self-contained `$$…$$` on the next line — odd-parity
    /// residue that is neither a clip, a bridge, nor a phantom. Unannotated it reds; an annotation
    /// naming its exact row tolerates it (green); a mismatched annotation still reds.
    #[test]
    fn source_integrity_annotation_isolates_a_genuine_unbalanced_residue_orphan() {
        let ledger = ledger_of(
            &[grid(0, "$$"), grid(1, "$$x = y$$")],
            DetectionContext::default(),
        );
        let verdict = ledger.containment(&[]);
        assert!(verdict.orphans >= 1, "a lone unpaired opener is an orphan");
        assert!(verdict.red);
        let orphan_line = ledger
            .orphan_entries()
            .next()
            .and_then(|entry| entry.source_line)
            .expect("the residue orphan carries a source row");
        let annotated = ledger.containment(&[SourceIntegrityAnnotation {
            source_line: orphan_line,
            note: "upstream reflow dropped the closer".to_owned(),
        }]);
        assert_eq!(annotated.orphans, 0);
        assert_eq!(annotated.annotated_damage, 1);
        assert!(
            !annotated.red,
            "precisely isolated upstream damage is not a failure"
        );
        let mismatched = ledger.containment(&[SourceIntegrityAnnotation {
            source_line: MathSourceLine::LiveGrid(99),
            note: "unrelated".to_owned(),
        }]);
        assert!(
            mismatched.red,
            "an unannotated orphan is a containment failure"
        );
    }

    /// False-positive guard: a live grid whose row 0 is itself a valid `$$` opener is an ordinary grid
    /// block, never a clip. `clipped_open_index` requires the first grid `$$` to be PRECEDED by grid
    /// body rows (row 0 mid-body); a `$$` at row 0 fails that, so the clip branch stays inert and the
    /// block is detected normally — no spurious above-window closer.
    #[test]
    fn grid_opener_at_row_zero_is_not_a_clip() {
        let ledger = ledger_of(
            &[
                hist(1, "some neutral prose line"),
                grid(0, "$$"),
                grid(1, r"\gamma = 2"),
                grid(2, "$$"),
            ],
            DetectionContext::default(),
        );
        let verdict = ledger.containment(&[]);
        assert_eq!(verdict.detected, 1, "an ordinary grid block is detected");
        assert!(!verdict.clipped_open);
        assert_eq!(verdict.orphans, 0);
        assert!(
            !has_reason(&ledger, LegitimateRejection::OpenerAboveWindow),
            "a row-0 opener must not be read as an above-window closer"
        );
    }

    /// False-positive guard (M1.9k prose red line): grid row 0 that is natural-language PROSE is not a
    /// clipped math body, so the first grid `$$` below it is NOT consumed as an above-window closer.
    /// The clip predicate requires the pre-`$$` grid rows to be a valid display body; prose fails it,
    /// the clip stays inert, and no prose is typeset as a clipped block.
    #[test]
    fn prose_grid_row_zero_is_not_a_clipped_body() {
        let ledger = ledger_of(
            &[
                hist(1, "intro paragraph text"),
                grid(0, "the quick brown fox jumps over"),
                grid(1, "$$"),
            ],
            DetectionContext::default(),
        );
        assert!(
            !has_reason(&ledger, LegitimateRejection::OpenerAboveWindow),
            "prose row 0 must not turn the `$$` into a clip closer"
        );
        assert!(has_reason(
            &ledger,
            LegitimateRejection::StreamingUnclosedTail
        ));
        assert!(!ledger.containment(&[]).clipped_open);
        assert_eq!(ledger.containment(&[]).orphans, 0);
    }

    /// A determinant identity line `=ad-bc` is display math, not prose: the earlier prose heuristic
    /// split on every non-letter and read the hyphen-joined operands `ad`/`bc` as two words, so the
    /// whole `$$…=ad-bc…$$` block was rejected as prose and never rendered. Prose words are set off
    /// by whitespace; a hyphen-joined token is one math operand group.
    #[test]
    fn an_operator_joined_math_token_is_not_prose() {
        assert!(!block_body_looks_like_prose("=ad-bc"));
        assert!(!block_body_looks_like_prose(
            r"\det\begin{pmatrix}a&b\\c&d\end{pmatrix}=ad-bc"
        ));
        assert!(!block_body_looks_like_prose("E = mc^2"));
        // Real-machine regression (repro-unified.vt, 2026-07-28): the coefficient token `2ab` was
        // end-trimmed to the "word" `ab`, so this command-free algebra line counted one multi-letter
        // word plus the single-letter operands and the whole block was rejected as prose — the only
        // never-rendered block on the user's screen. A token containing a digit is an operand group.
        assert!(!block_body_looks_like_prose("(a+b)^2 = a^2 + 2ab + b^2"));
        // Genuine natural-language lines must still be caught (M1.9k prose red line).
        assert!(block_body_looks_like_prose("the quick brown fox jumps"));
        assert!(block_body_looks_like_prose("See figure below now"));
        // Digit-carrying words do not launder prose: enough pure words remain to convict.
        assert!(block_body_looks_like_prose("see section2 for more details"));
    }

    /// Streaming-arrival regression (`stream-mispair.vt`). A complete `$$ \det …=ad-bc $$` block
    /// straddles the frozen/live seam: its opener has frozen while its `\begin{pmatrix}…\end{pmatrix}`
    /// body and closer are still on the live grid. The boundary resync must not abandon that genuine
    /// opener at its own grid closer — doing so re-reads the closer as a fresh opener and shifts every
    /// following grid block by one, stranding the whole screen at source (the user's "环境抢跑 + 外层
    /// $$ 裸奔" / whole-screen-stuck). The heat-equation block below it must resolve. Red before the
    /// convergence guard, green after. (With the `=ad-bc` prose fix the det block itself also bridges
    /// and renders — see `an_operator_joined_math_token_is_not_prose`.)
    #[test]
    fn a_straddling_block_does_not_desync_the_blocks_below_it() {
        let id = TranscriptId;
        let lines: Vec<(TranscriptId, &str)> = vec![
            (id(100), "$$"),               // 0 frozen det opener
            (id(101), r"\det"),            // 1
            (id(102), r"\begin{pmatrix}"), // 2
            (id(103), r"a & b\"),          // 3
            (id(104), "c & d"),            // 4
            (id(105), r"\end{pmatrix}"),   // 5
            (id(106), "=ad-bc"),           // 6
            (id(107), "$$"),               // 7 live det closer
            (id(108), ""),                 // 8
            (id(109), "$$"),               // 9 heat opener
            (id(110), r"\nabla^2 u=x"),    // 10
            (id(111), "$$"),               // 11 heat closer
        ];
        let blocks = detect_live_math_blocks_in_context(
            lines.iter().copied(),
            DetectionContext::default(),
            DetectionOptions::default(),
            None,
            Some(7),
            None,
        );
        assert!(
            blocks
                .iter()
                .any(|block| block.span.render_source.contains(r"\nabla^2 u=")),
            "the heat block below the straddling det block must still resolve"
        );
        assert!(
            blocks
                .iter()
                .any(|block| block.span.render_source.contains(r"\det")),
            "the det block itself bridges and renders once =ad-bc is not read as prose"
        );
    }

    /// Frozen history holds an odd number of structural `$$` (a reflow lost one block's opener), so
    /// a flat re-scan reaches the live grid already inside an open `$$`. Without the frozen→live
    /// boundary resync the grid's first `$$` is consumed as that dangling opener's closer and every
    /// grid block shifts by one, stranding the whole screen at source (the live-norender.vt bug).
    /// The clean grid block must still resolve. Red before the resync, green after.
    #[test]
    fn odd_dollar_parity_in_frozen_history_does_not_strand_the_live_grid_block() {
        let mut task = live_task(&["a", "b", "c"], 2);
        task.inputs = boundary_inputs(&[
            // A complete, well-paired display block already in history.
            (
                LiveDetectionSource::History {
                    id: TranscriptId(200),
                },
                "$$",
            ),
            (
                LiveDetectionSource::History {
                    id: TranscriptId(201),
                },
                r"\gamma=0",
            ),
            (
                LiveDetectionSource::History {
                    id: TranscriptId(202),
                },
                "$$",
            ),
            // A block whose opener a reflow deleted: only its closer survives in history. This is
            // the odd delimiter that leaves the scanner open at the boundary.
            (
                LiveDetectionSource::History {
                    id: TranscriptId(203),
                },
                "$$",
            ),
            // The intact live-grid block; its opener is grid row 0.
            (
                LiveDetectionSource::Grid {
                    row: 0,
                    revision: 1,
                },
                "$$",
            ),
            (
                LiveDetectionSource::Grid {
                    row: 1,
                    revision: 1,
                },
                r"\alpha+\beta",
            ),
            (
                LiveDetectionSource::Grid {
                    row: 2,
                    revision: 1,
                },
                "$$",
            ),
        ]);
        assert!(resolve_live_detection_task(&mut task));
        assert_eq!(task.span.render_source, r"\alpha+\beta");
        assert_eq!(task.start, GridPoint { row: 0, column: 0 });
        assert_eq!(task.end.row, 2);
    }

    /// The abandonment relaxation must never fabricate a formula: after a poison opener is dropped,
    /// the grid block that re-pairs from clean is still held to `valid_display_body`, so a prose
    /// body is rejected rather than typeset (M1.9k/M1.9p prose red line).
    #[test]
    fn boundary_resync_never_renders_prose_after_abandoning_a_poison_opener() {
        let mut task = live_task(&["a", "b", "c"], 2);
        task.inputs = boundary_inputs(&[
            (
                LiveDetectionSource::History {
                    id: TranscriptId(203),
                },
                "$$",
            ),
            (
                LiveDetectionSource::Grid {
                    row: 0,
                    revision: 1,
                },
                "$$",
            ),
            (
                LiveDetectionSource::Grid {
                    row: 1,
                    revision: 1,
                },
                "the quick brown fox jumps",
            ),
            (
                LiveDetectionSource::Grid {
                    row: 2,
                    revision: 1,
                },
                "$$",
            ),
        ]);
        assert!(!resolve_live_detection_task(&mut task));
    }

    /// The discriminator that keeps the `0848375` bridge alive is body validity, not position: a
    /// frozen opener whose seam-spanning body is prose is not a bridge and is abandoned, so nothing
    /// renders. Paired with `live_block_split_across_frozen_boundary_anchors_on_the_live_closer`
    /// (valid math body → renders) this pins the exact keep/abandon boundary.
    #[test]
    fn a_frozen_opener_with_a_prose_seam_body_is_abandoned_not_bridged() {
        let mut task = live_task(&["a"], 0);
        task.inputs = boundary_inputs(&[
            (
                LiveDetectionSource::History {
                    id: TranscriptId(300),
                },
                "$$",
            ),
            (
                LiveDetectionSource::History {
                    id: TranscriptId(301),
                },
                "the quick brown fox jumps",
            ),
            (
                LiveDetectionSource::Grid {
                    row: 0,
                    revision: 1,
                },
                "$$",
            ),
        ]);
        assert!(!resolve_live_detection_task(&mut task));
    }

    #[test]
    fn detects_contiguous_multi_logical_line_block() {
        let lines = [
            (TranscriptId(1), "before"),
            (TranscriptId(2), "$$"),
            (TranscriptId(3), r"\begin{aligned}"),
            (TranscriptId(4), r"x &= y + 1"),
            (TranscriptId(5), r"\end{aligned}"),
            (TranscriptId(6), "$$"),
        ];
        let blocks = detect_math_blocks(lines);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start, TranscriptId(2));
        assert_eq!(blocks[0].end, TranscriptId(6));
        assert_eq!(
            blocks[0].span.render_source,
            "\\begin{aligned}\nx &= y + 1\n\\end{aligned}"
        );
    }

    #[test]
    fn single_line_block_resets_an_abandoned_multiline_opening() {
        let blocks = detect_math_blocks([
            (TranscriptId(1), "$$"),
            (TranscriptId(2), "$$x$$"),
            (TranscriptId(3), "$$"),
        ]);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            (blocks[0].start, blocks[0].end),
            (TranscriptId(2), TranscriptId(2))
        );
        assert_eq!(blocks[0].span.render_source, "x");
    }

    #[test]
    fn rejects_nested_empty_unclosed_and_adjacent_empty_blocks() {
        assert!(
            detect_block_math("$$outer $$ inner$$").is_empty(),
            "nested delimiter"
        );
        assert!(
            detect_block_math("$$$$").is_empty(),
            "empty single-line block"
        );
        assert!(
            detect_math_blocks([(TranscriptId(1), "$$"), (TranscriptId(2), "x")]).is_empty(),
            "unclosed multiline block"
        );
        assert!(
            detect_math_blocks([
                (TranscriptId(1), "$$"),
                (TranscriptId(2), "$$"),
                (TranscriptId(3), "$$"),
                (TranscriptId(4), "$$"),
            ])
            .is_empty(),
            "adjacent empty blocks"
        );
    }

    #[test]
    fn fences_and_over_8k_blocks_are_rejected() {
        let fenced = [
            (TranscriptId(1), "```sh"),
            (TranscriptId(2), "$$x$$"),
            (TranscriptId(3), "```"),
        ];
        assert!(detect_math_blocks(fenced).is_empty());
        let huge = "x".repeat(MAX_MATH_SOURCE_BYTES + 1);
        let lines = [
            (TranscriptId(1), "$$"),
            (TranscriptId(2), huge.as_str()),
            (TranscriptId(3), "$$"),
        ];
        assert!(detect_math_blocks(lines).is_empty());
    }

    #[test]
    fn stale_worker_generation_is_discarded_without_leak() {
        let span = detect_block_math("$$x$$").remove(0);
        let mut record = DecorationRecord::frozen(stamp());
        let task = record
            .schedule(TranscriptId(1), TranscriptId(1), span)
            .unwrap();
        record.source_changed(SourceGeneration(2));
        assert!(!record.complete(&task, render_placeholder(&task)));
        assert!(record.artifact.is_none());
    }

    #[test]
    fn four_versions_have_distinct_invalidation_boundaries() {
        let span = detect_block_math("$$x$$").remove(0);
        let mut record = DecorationRecord::frozen(stamp());
        let task = record
            .schedule(TranscriptId(1), TranscriptId(1), span.clone())
            .unwrap();
        assert!(record.complete(&task, render_placeholder(&task)));

        let source_before = record.versions.source;
        record.layout_changed(LayoutKey {
            width_cells: nz32(40),
            ..stamp().layout
        });
        assert_eq!(record.versions.source, source_before);
        assert_eq!(record.decoration, DecorationLifecycle::None);
        assert!(record.artifact.is_none());
        assert!(record.stale_artifact.is_some());

        let old_detection_task = record
            .schedule(TranscriptId(1), TranscriptId(1), span.clone())
            .unwrap();
        record.detector_changed(DetectionRevision(2));
        assert!(!record.complete(&old_detection_task, render_placeholder(&old_detection_task)));
        assert_eq!(record.decoration, DecorationLifecycle::None);
        assert!(record.stale_artifact.is_none());

        let view_task = record
            .schedule(TranscriptId(1), TranscriptId(1), span)
            .unwrap();
        record.view_changed(ViewGeneration(2));
        assert!(!record.complete(&view_task, render_placeholder(&view_task)));

        record.suppress();
        assert_eq!(record.source, SourceLifecycle::Frozen);
        assert_eq!(record.decoration, DecorationLifecycle::Suppressed);
    }

    #[test]
    fn redetection_revision_is_recorded_in_rebuilt_intent() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(8).unwrap());
        let finalized = store
            .capture(CapturedRow::plain("$$x$$", false))
            .finalized
            .remove(0);
        let id = finalized.line.id;
        let mut document = HistoryDocument::default();
        document.finalize_transaction(finalized);

        redetect_document(&mut document, DetectionRevision(7));
        assert!(matches!(
            document.entries()[&id].decoration,
            DecorationIntent::Math {
                detection_revision: DetectionRevision(7),
                ..
            }
        ));
        redetect_document(&mut document, DetectionRevision(8));
        assert!(matches!(
            document.entries()[&id].decoration,
            DecorationIntent::Math {
                detection_revision: DetectionRevision(8),
                ..
            }
        ));
    }
}

#[cfg(test)]
mod rendered_list_marker_delimiters {
    use super::*;

    /// Codex CLI (and other TUI markdown renderers) put a reply inside a list item, so the first
    /// display opener arrives as `• $$`. Denying that opener its owns-the-line status desyncs the
    /// `$$` pairing for the entire message: every following pair swallows a blank body and gets
    /// rejected, which the user saw as "none of these render" (2026-07-23). A rendered list
    /// marker is valid CommonMark list-item context for a display block, so it is skipped like
    /// leading indentation; all other guards run unchanged.
    #[test]
    fn a_rendered_list_marker_does_not_desync_display_pairing() {
        let lines: Vec<(TranscriptId, &str)> = vec![
            (TranscriptId(1), "• $$"),
            (TranscriptId(2), r"  x=rac{-b\pm\sqrt{b^2-4ac}}{2a}"),
            (TranscriptId(3), "  $$"),
            (TranscriptId(4), ""),
            (TranscriptId(5), "  $$"),
            (TranscriptId(6), r"  e^{i\pi}+1=0"),
            (TranscriptId(7), "  $$"),
            (TranscriptId(8), ""),
            (TranscriptId(9), "  $$"),
            (TranscriptId(10), "  A="),
            (TranscriptId(11), r"  \begin{pmatrix}"),
            (TranscriptId(12), "  a & b\\"),
            (TranscriptId(13), "  c & d"),
            (TranscriptId(14), r"  \end{pmatrix}"),
            (TranscriptId(15), "  $$"),
        ];
        let blocks = detect_math_blocks(lines);
        assert_eq!(
            blocks
                .iter()
                .map(|block| (block.start, block.end))
                .collect::<Vec<_>>(),
            [
                (TranscriptId(1), TranscriptId(3)),
                (TranscriptId(5), TranscriptId(7)),
                (TranscriptId(9), TranscriptId(15)),
            ],
            "every $$ block must pair despite the list marker on the first opener"
        );
    }

    /// Codex's resize reflow re-renders a `$$` opener as an ATX heading (`# $$`, stacked
    /// `• # $$` behind the list marker). Refusing those openers desynchronised the whole
    /// message's pairing and let a later closer pair across the injected `#` line, typesetting it
    /// (real capture resize-repro.vt, 2026-07-24). Skipping the heading marker resyncs pairing.
    #[test]
    fn a_reflowed_heading_opener_keeps_pairing_in_sync() {
        let lines: Vec<(TranscriptId, &str)> = vec![
            (TranscriptId(1), "  # $$"),
            (
                TranscriptId(2),
                r"  \oint_{\partial\Omega}\mathbf F\cdot\mathrm d\mathbf r",
            ),
            (TranscriptId(3), ""),
            (
                TranscriptId(4),
                r"  \iint_\Omega(
abla	imes\mathbf F)\cdot\mathbf n",
            ),
            (TranscriptId(5), "  $$"),
            (TranscriptId(6), ""),
            (TranscriptId(7), "• # $$"),
            (
                TranscriptId(8),
                r"  i\hbarrac{\partial}{\partial t}\Psi(\mathbf r,t)",
            ),
            (TranscriptId(9), "  $$"),
        ];
        let blocks = detect_math_blocks(lines);
        assert_eq!(
            blocks
                .iter()
                .map(|block| (block.start, block.end))
                .collect::<Vec<_>>(),
            [
                (TranscriptId(1), TranscriptId(5)),
                (TranscriptId(7), TranscriptId(9)),
            ],
        );
    }

    /// The marker skip must not weaken the prose and inline-superstring guards.
    #[test]
    fn a_list_marker_line_with_prose_dollars_stays_undetected() {
        let lines: Vec<(TranscriptId, &str)> = vec![
            (TranscriptId(1), "• $$5.00 and then $$6.00 for shipping"),
            (TranscriptId(2), "• $$"),
            (TranscriptId(3), "  the total price is five dollars"),
            (TranscriptId(4), "  $$"),
        ];
        assert!(
            detect_math_blocks(lines).is_empty(),
            "prose bodies and inline superstrings stay rejected behind a list marker"
        );
    }
}

/// The table half of the scanner: proven in the same window, under the same code-fence context,
/// and never at the cost of a formula.
#[cfg(test)]
mod pipe_tables_in_a_scan {
    use super::*;

    fn scan(lines: &[&str]) -> Vec<DetectedMathBlock> {
        let numbered: Vec<(TranscriptId, &str)> = lines
            .iter()
            .enumerate()
            .map(|(index, text)| (TranscriptId(index as u64 + 1), *text))
            .collect();
        detect_math_blocks(numbered)
    }

    #[test]
    fn a_table_in_command_output_is_one_block_over_its_own_lines() {
        let blocks = scan(&[
            "here is what came back:",
            "| name | count |",
            "| --- | ---: |",
            "| alpha | 3 |",
            "| beta | 41 |",
            "done.",
        ]);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start, TranscriptId(2));
        assert_eq!(blocks[0].end, TranscriptId(5));
        assert_eq!(blocks[0].span.kind, BlockKind::Table);
        assert_eq!(blocks[0].span.mode, MathMode::Display);
        assert_eq!(blocks[0].span.delimiter_kind, DelimiterKind::Table);
        assert_eq!(
            blocks[0].span.original_source,
            "| name | count |\n| --- | ---: |\n| alpha | 3 |\n| beta | 41 |",
            "the block's source is the terminal's own bytes, delimiter row included"
        );
        assert_eq!(
            blocks[0].span.cell_segments.len(),
            4,
            "one segment per source row, so the rows it covers are the rows it clears"
        );
    }

    #[test]
    fn a_table_inside_a_code_fence_is_not_a_table() {
        assert!(
            scan(&["```markdown", "| a | b |", "|---|---|", "| 1 | 2 |", "```",]).is_empty(),
            "a fence suppresses a table for the same reason it suppresses a formula"
        );
    }

    #[test]
    fn an_indented_code_block_is_not_a_table_either() {
        assert!(
            scan(&["    | a | b |", "    |---|---|", "    | 1 | 2 |"]).is_empty(),
            "four spaces is CommonMark's own code block"
        );
    }

    #[test]
    fn a_formula_and_a_table_in_one_window_are_two_blocks_in_document_order() {
        let blocks = scan(&[
            "| a | b |",
            "|---|---|",
            "| 1 | 2 |",
            "",
            "$$",
            "x = 1",
            "$$",
        ]);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].span.kind, BlockKind::Table);
        assert_eq!(
            (blocks[0].start, blocks[0].end),
            (TranscriptId(1), TranscriptId(3))
        );
        assert_eq!(blocks[1].span.kind, BlockKind::Math);
        assert_eq!(
            (blocks[1].start, blocks[1].end),
            (TranscriptId(5), TranscriptId(7))
        );
    }

    #[test]
    fn two_tables_separated_by_prose_are_two_blocks() {
        let blocks = scan(&[
            "| a |", "|---|", "| 1 |", "and then", "| b |", "|---|", "| 2 |",
        ]);
        assert_eq!(blocks.len(), 2);
        assert!(
            blocks
                .iter()
                .all(|block| block.span.kind == BlockKind::Table)
        );
        assert_eq!(
            (blocks[0].start, blocks[0].end),
            (TranscriptId(1), TranscriptId(3))
        );
        assert_eq!(
            (blocks[1].start, blocks[1].end),
            (TranscriptId(5), TranscriptId(7))
        );
    }

    /// The near-miss corpus, run through the real scanner rather than the recogniser alone.
    #[test]
    fn the_near_miss_corpus_produces_no_block_at_all() {
        for sample in [
            vec!["2026-08-18 INFO | starting", "2026-08-18 INFO | ready"],
            vec!["│ a │ b │", "├───┼───┤", "│ 1 │ 2 │"],
            vec!["+---+---+", "| a | b |", "+---+---+"],
            vec!["if [ -f x ] || [ -f y ]; then", "  echo both", "fi"],
            vec!["| a | b | c |", "| --- | --- |", "| 1 | 2 | 3 |"],
            vec!["| a | b |", "| --a-- | --- |", "| 1 | 2 |"],
            vec!["Total", "-----", "42"],
        ] {
            assert!(
                scan(&sample).is_empty(),
                "no block may be proven from {sample:?}"
            );
        }
    }

    /// The user's real-world sample. GFM's own answer is that this is a table; see
    /// [`crate::table`]'s module doc for the spec rule and the citation.
    #[test]
    fn the_empty_first_heading_sample_is_a_table_and_keeps_its_bytes() {
        let blocks = scan(&[
            "| | 计划发卡 β 峰 | 时间 |",
            "|---|---|---|",
            "| 甲 | 12.5 | 08:00 |",
            "| 乙 | 9.0 | 09:30 |",
        ]);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].span.kind, BlockKind::Table);
        assert_eq!(
            blocks[0].span.original_source.lines().next(),
            Some("| | 计划发卡 β 峰 | 时间 |")
        );
    }

    /// Streaming: the same window, one line longer each time. Nothing is a block until the
    /// delimiter row lands, and from then on each arriving row extends the same block.
    #[test]
    fn a_table_arriving_a_row_at_a_time_renders_as_soon_as_it_is_provable_and_then_grows() {
        let full = ["| a | b |", "|---|---|", "| 1 | 2 |", "| 3 | 4 |"];
        let mut ends = Vec::new();
        for length in 1..=full.len() {
            let blocks = scan(&full[..length]);
            ends.push(blocks.first().map(|block| block.end));
        }
        assert_eq!(
            ends,
            vec![
                None,
                Some(TranscriptId(2)),
                Some(TranscriptId(3)),
                Some(TranscriptId(4)),
            ]
        );
    }
}
