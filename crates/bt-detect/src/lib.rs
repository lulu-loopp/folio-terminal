//! Conservative block-level `$$...$$` detection and the dual lifecycle/version gate.

mod ledger;
pub use ledger::{
    ContainmentVerdict, LedgerEntry, LegitimateRejection, OrphanKind, OwnershipLedger,
    SourceIntegrityAnnotation, StructuralDelimiterKind, TokenFate,
};
use ledger::{OwnershipRecorder, source_line_of, structural_kind};

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
    // A natural-language word is set off by whitespace and is an uninterrupted run of letters,
    // optionally wrapped in punctuation (`word,` / `(word)`). A whitespace token whose interior
    // mixes letters with math connectives — `ad-bc`, `mc^2`, `x_i`, `a=b` — is a math operand group,
    // not a prose word, and must not be counted: `-`, `=`, `^`, `_`, digits and the like separate
    // operands, not words. (The earlier version split on every non-letter, so `=ad-bc` read as the
    // two "words" `ad`/`bc` and a genuine `$$…=ad-bc…$$` block was rejected as prose.) Trim only the
    // outer punctuation, then require the remaining core to be a single letter run.
    for token in line.split_ascii_whitespace() {
        let core = token.trim_matches(|character: char| !character.is_ascii_alphabetic());
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
    scan_math_blocks_impl(lines, initial_context, options, None, None, None)
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
fn scan_math_blocks_impl<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    initial_context: DetectionContext,
    options: DetectionOptions,
    live_grid_boundary: Option<usize>,
    clipped_open_index: Option<u32>,
    mut recorder: Option<&mut OwnershipRecorder>,
) -> MathScanResult {
    let lines = lines.into_iter().collect::<Vec<_>>();
    let mut result = MathScanResult::default();
    let mut fence = initial_context.fence;
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
            if let Some(rec) = recorder.as_deref_mut() {
                record_code_context_delimiter(rec, index, text);
            }
            continue;
        }
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
        // Swallow-radius bound. A math *environment* body can never legally contain a `$$`/`\[`
        // display opener — those switch display mode and are a syntax error inside
        // `\begin{env}…\end{env}`. So when one appears while an environment opening is still
        // unclosed, that environment's closer was lost (mangled by a reflow, scrolled out of this
        // window, or malformed), and continuing to swallow would consume every following display
        // block as phantom environment body (the `\end{pmatrix},`-poisoning failure mode). Abandon
        // the stale environment opening here; the line is then handled by the normal
        // opening-is-none paths below (single-line complete block, or a fresh multi-line opener)
        // under every existing guard — prose body, escapes, CommonMark code, ambiguous-prefix
        // pairing. This never fires for an active `$$`/`\[` opening: inner `\begin`/`\end`
        // directional environments are not display openers, so a genuinely nested environment is
        // still swallowed as body by the catch-all further down.
        if opening
            .as_ref()
            .is_some_and(|active| matches!(active.delimiter, DisplayDelimiter::Environment(_)))
            && opening_delimiter(text).is_some_and(|(kind, _)| {
                matches!(kind, DelimiterKind::Dollars | DelimiterKind::Brackets)
            })
        {
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
            let abandon_stale_dollars = if let Some(active) = opening.as_ref() {
                active.delimiter == DisplayDelimiter::Dollars
                    && active.start_index.is_none_or(|start| start < boundary)
                    && closing_delimiter(text, &active.delimiter).is_some_and(|(body_end, _)| {
                        let frozen_body_invalid = match active.start_index {
                            Some(start) => {
                                let body =
                                    joined_range(&lines, start, index, active.body_start, body_end);
                                let render = restore_stripped_environment_newlines(
                                    &body,
                                    options.restore_stripped_environment_newlines,
                                );
                                !valid_display_body(&body, &render, options)
                            }
                            None => true,
                        };
                        // Convergence guard. Abandoning the stale opening re-reads this grid `$$`
                        // as a *fresh opener*; that is only sound when the `$$` genuinely opens the
                        // next block (a lost-opener parity phantom, where the frozen opening was
                        // spurious). If instead this `$$` is the real closer of a complete block
                        // that straddles the seam — its body merely tripping the body check (e.g. a
                        // `=ad-bc` line the prose heuristic over-reads, an over-length body, a Jump
                        // chip) — abandoning would turn its own closer into an opener and shift
                        // every following grid block by one, stranding the whole screen at source
                        // with no recovery until a reprint. So only abandon when re-pairing this
                        // `$$` forward actually yields a valid display block; otherwise keep the
                        // opening and let the normal closer path consume this `$$`, dropping only
                        // the single unrenderable block and preserving parity for everything below.
                        frozen_body_invalid
                            && grid_dollars_opens_valid_block(&lines, index, options)
                    })
            } else {
                false
            };
            if abandon_stale_dollars {
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
            let valid = valid_display_body(body, body, options);
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
            let valid = valid_display_body(body, render, options);
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
                ),
                _ => restore_stripped_environment_newlines(
                    &body,
                    options.restore_stripped_environment_newlines,
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
    result
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
        None,
        None,
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
    live_grid_boundary: Option<usize>,
    clipped_open_index: Option<u32>,
) -> Vec<DetectedMathBlock> {
    scan_math_blocks_impl(
        lines,
        initial_context,
        options,
        live_grid_boundary,
        clipped_open_index,
        None,
    )
    .blocks
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
    let clipped = clipped_open_index(&logical, boundary, &initial_context, options);
    let full = detect_live_math_blocks_in_context(
        logical.iter().map(|line| (line.id, line.text.as_str())),
        initial_context,
        options,
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
    let clipped = clipped_open_index(&logical, boundary, &initial_context, options);

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
        boundary,
        clipped,
        Some(&mut recorder),
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
fn clipped_open_index(
    logical: &[LiveLogicalLine],
    boundary: Option<usize>,
    initial_context: &DetectionContext,
    options: DetectionOptions,
) -> Option<u32> {
    let b = boundary?;
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
    let body = logical[b..first_dollars]
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    valid_display_body(&body, &body, options).then_some(first_dollars as u32)
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
    let live_grid_boundary = live_grid_boundary_index(&logical, &task.inputs);
    let clipped = clipped_open_index(
        &logical,
        live_grid_boundary,
        &task.initial_context,
        task.options,
    );
    let detected = detect_live_math_blocks_in_context(
        logical.iter().map(|line| (line.id, line.text.as_str())),
        task.initial_context.clone(),
        task.options,
        live_grid_boundary,
        clipped,
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
    let live_grid_boundary = live_grid_boundary_index(&logical, &inputs);
    let clipped = clipped_open_index(&logical, live_grid_boundary, &initial_context, options);
    let blocks = detect_live_math_blocks_in_context(
        logical.iter().map(|line| (line.id, line.text.as_str())),
        initial_context.clone(),
        options,
        live_grid_boundary,
        clipped,
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

/// Index of the first logical line that carries a live-grid fragment, when at least one frozen
/// history line precedes it — i.e. a real frozen→live seam. Returns `None` when the first logical
/// line is already grid (a pure alternate-screen context, or a primary with empty history): there
/// is no frozen prefix to be poisoned, so the boundary-resync guard must stay inert and leave a
/// legitimately truncated alternate prefix (its off-screen opener) to pair as it always has.
fn live_grid_boundary_index(
    logical: &[LiveLogicalLine],
    inputs: &[LiveDetectionInput],
) -> Option<usize> {
    logical
        .iter()
        .position(|line| {
            line.fragments.iter().any(|fragment| {
                matches!(
                    inputs[fragment.input_index].source,
                    LiveDetectionSource::Grid { .. }
                )
            })
        })
        .filter(|&boundary| boundary > 0)
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
                cell_boundaries: scalar_boundaries(text),
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
            boundary,
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
        // Genuine natural-language lines must still be caught (M1.9k prose red line).
        assert!(block_body_looks_like_prose("the quick brown fox jumps"));
        assert!(block_body_looks_like_prose("See figure below now"));
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
