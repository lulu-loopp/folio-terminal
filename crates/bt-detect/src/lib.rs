//! Conservative block-level `$$...$$` detection and the dual lifecycle/version gate.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use bt_doc::{DecorationIntent, HistoryDocument};
pub use bt_doc::{
    DecorationLifecycle, DetectionRevision, GridGeneration, GridPoint, LayoutKey, MathMode,
    SUBPIXELS_PER_PX, ScreenId, SourceLifecycle, VersionStamp, ViewGeneration,
};
use bt_transcript::{SourceGeneration, TranscriptId};

pub const MAX_MATH_SOURCE_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DetectionOptions {
    /// Restore a row separator which Claude Code currently strips from the end of a logical line
    /// inside a LaTeX math environment. Set this to `false` once Claude Code emits the original
    /// `\\\\` faithfully; disabling it preserves renderer input byte-for-byte.
    pub restore_stripped_environment_newlines: bool,
    /// Reject a display-math candidate containing Claude Code's exact scroll-review overlay text.
    /// Disable this once Claude Code no longer writes that chip into terminal content rows.
    pub reject_claude_code_jump_chip_overlay: bool,
}

impl Default for DetectionOptions {
    fn default() -> Self {
        Self {
            restore_stripped_environment_newlines: true,
            reject_claude_code_jump_chip_overlay: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DelimiterKind {
    Dollars,
    Brackets,
    Environment(String),
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
    pub cell_segments: Vec<MathCellSegment>,
    pub inline_runs: Vec<InlineMathRun>,
}

pub type MathSpan = MathOccurrence;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineMathRun {
    pub byte_start: u32,
    pub byte_end: u32,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaceholderArtifact {
    pub key: String,
    pub block_end: TranscriptId,
    pub height_subpixels: i64,
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
    pub baseline_subpixels: i64,
    pub mode: MathMode,
    pub render_time: Duration,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectionInput {
    pub id: TranscriptId,
    pub text: String,
    /// UTF-8 byte boundary to terminal cell-column mappings from the captured logical line.
    pub cell_boundaries: Vec<(u32, u32)>,
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
            && self.inline_runs == other.inline_runs
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
    /// UTF-8 byte boundary to terminal cell-column mappings, including `(0, 0)` and the final
    /// source boundary. These come from captured terminal cells, never Unicode-width inference.
    pub cell_boundaries: Vec<(u32, u32)>,
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
                cell_segments: Vec::new(),
                inline_runs: Vec::new(),
            },
            versions: self.versions,
            cell_width_subpixels: SUBPIXELS_PER_PX,
            cell_height_subpixels: SUBPIXELS_PER_PX,
            ascii_baseline_subpixels: SUBPIXELS_PER_PX,
            options,
            initial_context,
            inputs,
            resolved: false,
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

/// Conservatively detect one or more `$...$` runs on a single logical line. A run needs an
/// explicit math signal; currency, shell variables and identifier-like code remain native text.
/// Inline `$...$` detection is DISABLED pending a sound disambiguator (M1.9g).
///
/// Independent review measured the current heuristic against 18 lines of ordinary terminal text
/// and found 6 false positives - `PATH=$HOME/bin:$PATH` rendered `HOME/bin:`, `WHERE a=$1 AND
/// b=$2` rendered `1 AND b=`, `Cost $5+$10` rendered `5+` - because any of `/ + - = >` inside the
/// candidate counted as a mathematical signal. The suite that passed had selection bias (it only
/// sampled space-separated currency and `echo`-prefixed lines), and the live oracle passed for the
/// same accidental reason: its probe began with `echo `.
///
/// A terminal that renders your literal text has failed as a terminal, and that outranks the
/// convenience of inline rendering. Display `$$...$$` detection is unaffected: its paired
/// whole-line delimiters carry orders of magnitude more signal than a lone `$`.
pub fn detect_inline_math(text: &str) -> Vec<InlineMathRun> {
    let _ = text;
    return Vec::new();
    #[allow(unreachable_code)]
    if inline_line_is_code_like(text) || text.len() > MAX_MATH_SOURCE_BYTES {
        return Vec::new();
    }
    let dollars = text
        .char_indices()
        .filter_map(|(byte, character)| (character == '$').then_some(byte))
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
        {
            index += 1;
            continue;
        }
        let Some(close_index) = (index + 1..dollars.len()).find(|candidate| {
            let close = dollars[*candidate];
            !delimiter_is_escaped(text, close)
                && text.as_bytes().get(close + 1) != Some(&b'$')
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

fn inline_group(runs: Vec<InlineMathRun>) -> Option<MathSpan> {
    let first = runs.first()?;
    let last = runs.last()?;
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
        cell_segments: Vec::new(),
        inline_runs: runs,
    })
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
    let mut word_len = 0usize;
    for character in line.chars().chain(std::iter::once(' ')) {
        if character.is_ascii_alphabetic() {
            word_len += 1;
        } else if word_len != 0 {
            words += 1;
            multi_letter_words += usize::from(word_len > 1);
            word_len = 0;
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

/// Advance the compact frozen-history parser proof by one immutable logical line. This mirrors the
/// structural state transitions in `detect_math_blocks_in_context`; it deliberately records no
/// body text, so retaining checkpoints is O(resident lines), not O(total source bytes squared).
pub fn advance_detection_context(context: &mut DetectionContext, id: TranscriptId, text: &str) {
    if context.opening.is_none() && commonmark_indented_code(text) {
        return;
    }
    if context.opening.is_none()
        && let Some(marker) = commonmark_fence_marker(text)
    {
        match context.fence {
            Some(active) if commonmark_fence_closes(text, active) => context.fence = None,
            None => context.fence = Some(marker),
            _ => {}
        }
        context.opening = None;
        return;
    }
    if context.fence.is_some() {
        return;
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
    let lines = lines.into_iter().collect::<Vec<_>>();
    let mut result = MathScanResult::default();
    let mut fence = initial_context.fence;
    let mut opening = initial_context.opening.map(|(_, delimiter)| ActiveOpening {
        start_index: None,
        delimiter,
        body_start: 0,
    });
    for (index, (_, text)) in lines.iter().enumerate() {
        if opening.is_none() && commonmark_indented_code(text) {
            continue;
        }
        if opening.is_none()
            && let Some(marker) = commonmark_fence_marker(text)
        {
            match fence {
                Some(active) if commonmark_fence_closes(text, active) => fence = None,
                None => fence = Some(marker),
                _ => {}
            }
            opening = None;
            continue;
        }
        if fence.is_some() {
            continue;
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
            if valid_display_body(body, body, options) {
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
                        body.to_owned(),
                        delimiter,
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
            if !valid_display_body(body, render, options) {
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
                    render.to_owned(),
                    delimiter,
                ),
            });
            continue;
        }
        if opening.is_none()
            && let Some(span) = inline_group(detect_inline_math(text))
        {
            let id = lines[index].0;
            result.blocks.push(DetectedMathBlock {
                start: id,
                end: id,
                span,
            });
            continue;
        }
        if let Some(active) = opening.as_ref()
            && let Some((body_end, close_end)) = closing_delimiter(text, &active.delimiter)
        {
            let active = opening.take().expect("active opening was just observed");
            let Some(start_index) = active.start_index else {
                // The opener is before this bounded window. Its exact state proves that this is a
                // closer, but the missing source means there is no occurrence to render.
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
                ),
                _ => restore_stripped_environment_newlines(
                    &body,
                    options.restore_stripped_environment_newlines,
                ),
            };
            if !valid_display_body(&body, &render, options) {
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
        if delimiter == DisplayDelimiter::Dollars
            && initial_context.prefix == PrefixKnowledge::Ambiguous
        {
            result.ambiguous.push(AmbiguousMathBlock {
                start: lines[index].0,
                end: lines[index].0,
                delimiter_kind: delimiter,
            });
            continue;
        }
        opening = Some(ActiveOpening {
            start_index: Some(index),
            delimiter,
            body_start,
        });
    }
    result
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
        cell_segments,
        inline_runs: Vec::new(),
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
fn restore_stripped_environment_newlines(source: &str, enabled: bool) -> String {
    if !enabled || !source.contains('\n') {
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

fn delimiter_start(text: &str) -> usize {
    text.len() - text.trim_start_matches(' ').len()
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
    let body_end = trimmed.len().checked_sub(closing.len())?;
    (open_end < body_end && trimmed.ends_with(&closing)).then_some((
        DelimiterKind::Environment(environment),
        start,
        start + open_end,
        start + body_end,
        start + trimmed.len(),
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
        DelimiterKind::Environment(environment) => {
            let closing = format!(r"\end{{{environment}}}");
            let start = trimmed_end.checked_sub(closing.len())?;
            (text.get(start..trimmed_end) == Some(closing.as_str())).then_some((start, trimmed_end))
        }
    }
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

/// Run the authoritative detector on a worker-owned frozen snapshot. The session thread only
/// chooses a cheap `$$` candidate and never calls this while ingesting a finalized line.
pub fn resolve_detection_task(task: &mut DetectionTask) -> bool {
    if task.resolved {
        return true;
    }
    let detected = detect_math_blocks_in_context_with_options(
        task.inputs
            .iter()
            .map(|input| (input.id, input.text.as_str())),
        task.initial_context.clone(),
        task.options,
    )
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
    let detected = detect_math_blocks_in_context_with_options(
        logical.iter().map(|line| (line.id, line.text.as_str())),
        task.initial_context.clone(),
        task.options,
    )
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
    let blocks = detect_math_blocks_in_context_with_options(
        logical.iter().map(|line| (line.id, line.text.as_str())),
        initial_context.clone(),
        options,
    )
    .into_iter()
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
        let Some(block) = row_to_logical
            .get(&task.candidate_row)
            .and_then(|id| blocks.get(id))
        else {
            continue;
        };
        let _ = apply_live_detected_block(task, block, &logical);
    }
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
    let Some(first) = occurrence.cell_segments.first() else {
        return false;
    };
    let Some(last) = occurrence.cell_segments.last() else {
        return false;
    };
    let MathSourceLine::LiveGrid(start_row) = first.source_line else {
        // A block that begins in frozen history cannot be represented by a live-grid anchor.
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
    fragments: Vec<LiveLogicalFragment>,
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
                fragments: Vec::new(),
            });
        }
        let Some(line) = logical.last_mut() else {
            continue;
        };
        let byte_start = line.text.len();
        line.text.push_str(&input.text);
        line.fragments.push(LiveLogicalFragment {
            input_index,
            byte_start,
            byte_end: line.text.len(),
        });
    }
    logical
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
            let LiveDetectionSource::Grid { row, .. } = input.source else {
                return None;
            };
            let local = u32::try_from(source_start.saturating_sub(fragment.byte_start)).ok()?;
            let cell = input
                .cell_boundaries
                .iter()
                .find_map(|(byte, cell)| (*byte == local).then_some(*cell))?;
            mapped.push(MathCellSegment {
                logical_line: source.logical_line,
                source_line: MathSourceLine::LiveGrid(row),
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
            let LiveDetectionSource::Grid { row, .. } = input.source else {
                return None;
            };
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
                source_line: MathSourceLine::LiveGrid(row),
                byte_start: local_start,
                byte_end: local_end,
                cell_start,
                cell_end,
            });
        }
    }
    (!mapped.is_empty()).then_some(mapped)
}

fn live_temporary_id(index: usize) -> Option<TranscriptId> {
    u64::try_from(index).ok()?.checked_add(1).map(TranscriptId)
}

fn delimiter_is_escaped(text: &str, byte: usize) -> bool {
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
        block_end: task.block_end,
        height_subpixels: 64 * SUBPIXELS_PER_PX,
        rgba: Arc::from(vec![0; 4]),
        width_px: 1,
        height_px: 1,
        baseline_subpixels: 0,
        mode: task.span.mode,
        render_time: Duration::ZERO,
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
                font_rev: 1,
                theme_rev: 1,
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

    #[test]
    fn inline_detection_stays_disabled_until_disambiguation_is_sound() {
        // The machinery below is retained for M1.9g, but must not decorate anything while the
        // disambiguator lets `PATH=$HOME/bin:$PATH` through. Genuine inline math is therefore
        // expected to stay source too: silence on both sides is the honest state, and this
        // assertion is what will fail (loudly, in the right direction) when inline is re-enabled.
        let text = "能量 $E = mc^2$，并且 $a_1+b_1=c_1$。";
        assert!(
            detect_math_blocks([(TranscriptId(1), text)]).is_empty(),
            "inline detection must stay off until its false-positive set is honest"
        );
        assert!(detect_inline_math(text).is_empty());
    }

    #[test]
    fn a_truncated_window_never_pairs_a_closer_with_the_next_opener() {
        // The window starts INSIDE a block, so its first `$$` is really a closing delimiter.
        // Pairing it with the next block's opener would swallow the prose between them - which is
        // exactly what rendered a Chinese paragraph as mathematics (user report 2026-07-19).
        let window = [
            (TranscriptId(1), r"\frac{a}{b}"), // tail of a block whose opener is off-screen
            (TranscriptId(2), "$$"),           // actually a CLOSER
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
            (TranscriptId(20), r"\frac{a}{b}"),
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
                    cell_boundaries: boundaries,
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
                (TranscriptId(1), r"\frac{a}{b}"),
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
    }

    #[test]
    fn existing_row_separators_and_non_environment_backslashes_are_unchanged() {
        let already_valid = [
            r"$$\begin{aligned}",
            r"x &= 0\\",
            r"y &= \nabla f",
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
            restore_stripped_environment_newlines(r"$$x \$$", true),
            r"$$x \$$"
        );
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

    #[test]
    fn inline_false_positive_set_stays_native() {
        for text in [
            "$5 和 $10",
            "价格是 $5$",
            "echo $PATH",
            "echo $1",
            "literal $PATH$ token",
            "`const x = $value`",
            "+ 文档里有 $x^2$",
            "2026-07-19 log $x^2$",
            r"escaped \$x^2$",
            "unclosed $x^2",
        ] {
            assert!(
                detect_inline_math(text).is_empty(),
                "unexpected match: {text}"
            );
        }
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
                        cell_boundaries: scalar_boundaries(text),
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
                cell_segments: Vec::new(),
                inline_runs: Vec::new(),
            },
            detection_complete: false,
            resolved: false,
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
