//! Conservative block-level `$$...$$` detection and the dual lifecycle/version gate.

use std::{sync::Arc, time::Duration};

use bt_doc::{DecorationIntent, HistoryDocument};
pub use bt_doc::{
    DecorationLifecycle, DetectionRevision, GridGeneration, GridPoint, LayoutKey, MathMode,
    SUBPIXELS_PER_PX, ScreenId, SourceLifecycle, VersionStamp, ViewGeneration,
};
use bt_transcript::{SourceGeneration, TranscriptId};

pub const MAX_MATH_SOURCE_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathSpan {
    pub byte_start: u32,
    pub byte_end: u32,
    pub source: String,
    pub mode: MathMode,
    pub inline_runs: Vec<InlineMathRun>,
}

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
    /// Whether the first input is a known stream boundary rather than a truncated window.
    pub context_start_trusted: bool,
    pub inputs: Arc<[DetectionInput]>,
    pub resolved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectionInput {
    pub id: TranscriptId,
    pub text: String,
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
    /// Whether the detector can prove that no delimiter or fence began before `inputs[0]`.
    pub context_start_trusted: bool,
    pub inputs: Arc<[LiveDetectionInput]>,
    pub start: GridPoint,
    pub end: GridPoint,
    /// Inclusive live-grid row band reserved for presentation. Detection initializes this to the
    /// source span; the session may extend it over adjacent blank rows before rasterization.
    pub band_start_row: u32,
    pub band_end_row: u32,
    pub span: MathSpan,
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
            context_start_trusted: true,
            inputs: Arc::from([]),
            resolved: true,
        })
    }

    pub fn schedule_scan(
        &mut self,
        candidate_id: TranscriptId,
        context_start_trusted: bool,
        inputs: Arc<[DetectionInput]>,
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
                source: String::new(),
                mode: MathMode::Display,
                inline_runs: Vec::new(),
            },
            versions: self.versions,
            cell_width_subpixels: SUBPIXELS_PER_PX,
            cell_height_subpixels: SUBPIXELS_PER_PX,
            ascii_baseline_subpixels: SUBPIXELS_PER_PX,
            context_start_trusted,
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
    let trimmed = text.trim();
    let (source, rendered_source) = if trimmed.len() >= 5
        && trimmed.starts_with("$$")
        && trimmed.ends_with("$$")
        && !delimiter_is_escaped(trimmed, 0)
    {
        let close = trimmed.len() - 2;
        if close == 2 || delimiter_is_escaped(trimmed, close) {
            return Vec::new();
        }
        let source = &trimmed[2..close];
        if source.contains("$$") {
            return Vec::new();
        }
        (source, source)
    } else if trimmed.len() >= 5 && trimmed.starts_with(r"\[") && trimmed.ends_with(r"\]") {
        let source = &trimmed[2..trimmed.len() - 2];
        if source.contains(r"\[") || source.contains(r"\]") {
            return Vec::new();
        }
        (source, source)
    } else if let Some(body) = single_line_math_environment(trimmed) {
        (body, trimmed)
    } else {
        return Vec::new();
    };
    if source.is_empty()
        || rendered_source.len() > MAX_MATH_SOURCE_BYTES
        || block_body_looks_like_prose(source)
    {
        return Vec::new();
    }
    let leading = text.len() - text.trim_start().len();
    vec![MathSpan {
        byte_start: leading as u32,
        byte_end: (leading + trimmed.len()) as u32,
        source: rendered_source.to_owned(),
        mode: MathMode::Display,
        inline_runs: Vec::new(),
    }]
}

fn single_line_math_environment(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix(r"\begin{")?;
    let name_end = rest.find('}')?;
    let environment = &rest[..name_end];
    if !is_math_environment(environment) {
        return None;
    }
    let body_start = r"\begin{".len() + name_end + 1;
    let closing = format!(r"\end{{{environment}}}");
    let body_end = trimmed.len().checked_sub(closing.len())?;
    (body_start < body_end && trimmed.ends_with(&closing)).then(|| &trimmed[body_start..body_end])
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
        source: runs
            .iter()
            .map(|run| run.source.as_str())
            .collect::<Vec<_>>()
            .join("; "),
        mode: MathMode::Inline,
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum DisplayDelimiter {
    Dollars,
    Brackets,
    Environment(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DisplayDelimiterToken {
    Symmetric(DisplayDelimiter),
    Open(DisplayDelimiter),
    Close(DisplayDelimiter),
}

fn whole_line_display_delimiter(trimmed: &str) -> Option<DisplayDelimiterToken> {
    if trimmed == "$$" {
        return Some(DisplayDelimiterToken::Symmetric(DisplayDelimiter::Dollars));
    }
    if trimmed == r"\[" {
        return Some(DisplayDelimiterToken::Open(DisplayDelimiter::Brackets));
    }
    if trimmed == r"\]" {
        return Some(DisplayDelimiterToken::Close(DisplayDelimiter::Brackets));
    }
    for (prefix, open) in [(r"\begin{", true), (r"\end{", false)] {
        let Some(rest) = trimmed.strip_prefix(prefix) else {
            continue;
        };
        let Some(environment) = rest.strip_suffix('}') else {
            continue;
        };
        if !environment.contains('}') && is_math_environment(environment) {
            let delimiter = DisplayDelimiter::Environment(environment.to_owned());
            return Some(if open {
                DisplayDelimiterToken::Open(delimiter)
            } else {
                DisplayDelimiterToken::Close(delimiter)
            });
        }
    }
    None
}

pub fn detect_math_blocks<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
) -> Vec<DetectedMathBlock> {
    detect_math_blocks_in_context(lines, true)
}

/// Detect display math only when the snapshot starts at a proven parser boundary. An incomplete
/// prefix cannot establish symmetric-delimiter or code-fence parity, so the honest result is no
/// decoration rather than a guessed pairing.
pub fn detect_math_blocks_in_context<'a>(
    lines: impl IntoIterator<Item = (TranscriptId, &'a str)>,
    context_start_trusted: bool,
) -> Vec<DetectedMathBlock> {
    let lines = lines.into_iter().collect::<Vec<_>>();
    if !context_start_trusted {
        return Vec::new();
    }
    let mut blocks = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut opening: Option<(usize, DisplayDelimiter)> = None;
    for (index, (_, text)) in lines.iter().enumerate() {
        let trimmed = text.trim();
        if let Some(marker) = fence_marker(trimmed) {
            match fence {
                Some(active) if active.0 == marker.0 && marker.1 >= active.1 => fence = None,
                None => fence = Some(marker),
                _ => {}
            }
            opening = None;
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if let Some(span) = detect_block_math(text).into_iter().next() {
            let id = lines[index].0;
            blocks.push(DetectedMathBlock {
                start: id,
                end: id,
                span,
            });
            opening = None;
            continue;
        }
        if opening.is_none()
            && let Some(span) = inline_group(detect_inline_math(text))
        {
            let id = lines[index].0;
            blocks.push(DetectedMathBlock {
                start: id,
                end: id,
                span,
            });
            continue;
        }
        let Some(token) = whole_line_display_delimiter(trimmed) else {
            continue;
        };
        let matching_close = match (&opening, &token) {
            (
                Some((_, DisplayDelimiter::Dollars)),
                DisplayDelimiterToken::Symmetric(DisplayDelimiter::Dollars),
            )
            | (
                Some((_, DisplayDelimiter::Brackets)),
                DisplayDelimiterToken::Close(DisplayDelimiter::Brackets),
            ) => true,
            (
                Some((_, DisplayDelimiter::Environment(open))),
                DisplayDelimiterToken::Close(DisplayDelimiter::Environment(close)),
            ) => open == close,
            _ => false,
        };
        if matching_close {
            let Some((start_index, delimiter)) = opening.take() else {
                continue;
            };
            if index == start_index + 1 {
                continue;
            }
            let body = lines[start_index + 1..index]
                .iter()
                .map(|(_, line)| *line)
                .collect::<Vec<_>>()
                .join("\n");
            let source = if matches!(delimiter, DisplayDelimiter::Environment(_)) {
                lines[start_index..=index]
                    .iter()
                    .map(|(_, line)| *line)
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                body.clone()
            };
            if body.trim().is_empty()
                || source.len() > MAX_MATH_SOURCE_BYTES
                || block_body_looks_like_prose(&body)
            {
                continue;
            }
            blocks.push(DetectedMathBlock {
                start: lines[start_index].0,
                end: lines[index].0,
                span: MathSpan {
                    byte_start: 0,
                    byte_end: text.len() as u32,
                    source,
                    mode: MathMode::Display,
                    inline_runs: Vec::new(),
                },
            });
            continue;
        }
        if opening.is_some() {
            // Directional environments commonly nest inside an outer display block. Only the
            // delimiter which matches the active opener is structural at this level.
            continue;
        }
        opening = match token {
            DisplayDelimiterToken::Symmetric(delimiter)
            | DisplayDelimiterToken::Open(delimiter) => Some((index, delimiter)),
            DisplayDelimiterToken::Close(_) => None,
        };
    }
    blocks
}

/// Run the authoritative detector on a worker-owned frozen snapshot. The session thread only
/// chooses a cheap `$$` candidate and never calls this while ingesting a finalized line.
pub fn resolve_detection_task(task: &mut DetectionTask) -> bool {
    if task.resolved {
        return true;
    }
    let detected = detect_math_blocks_in_context(
        task.inputs
            .iter()
            .map(|input| (input.id, input.text.as_str())),
        task.context_start_trusted,
    )
    .into_iter()
    .find(|block| block.end == task.candidate_id);
    let Some(block) = detected else {
        return false;
    };
    task.transcript_id = block.start;
    task.block_end = block.end;
    task.span = block.span;
    task.resolved = true;
    true
}

/// Resolve a live-grid candidate through the exact same conservative detector as frozen history.
/// Temporary transcript IDs are a detector-local indexing device; they never escape as anchors.
pub fn resolve_live_detection_task(task: &mut LiveDetectionTask) -> bool {
    if task.resolved {
        return true;
    }
    let Some(candidate_index) = task.inputs.iter().position(|input| {
        matches!(
            input.source,
            LiveDetectionSource::Grid { row, .. } if row == task.candidate_row
        )
    }) else {
        return false;
    };
    let Some(candidate_id) = live_temporary_id(candidate_index) else {
        return false;
    };
    let detected = detect_math_blocks_in_context(
        task.inputs.iter().enumerate().filter_map(|(index, input)| {
            live_temporary_id(index).map(|id| (id, input.text.as_str()))
        }),
        task.context_start_trusted,
    )
    .into_iter()
    .find(|block| block.end == candidate_id);
    let Some(block) = detected else {
        return false;
    };
    let Some(start_index) = block
        .start
        .0
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
    else {
        return false;
    };
    let Some(end_index) = block
        .end
        .0
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
    else {
        return false;
    };
    let Some(LiveDetectionInput {
        source: LiveDetectionSource::Grid { row: start_row, .. },
        ..
    }) = task.inputs.get(start_index)
    else {
        // A block that begins in frozen history cannot be represented by a live-grid anchor.
        return false;
    };
    let Some(LiveDetectionInput {
        source: LiveDetectionSource::Grid { row: end_row, .. },
        text: end_text,
    }) = task.inputs.get(end_index)
    else {
        return false;
    };
    task.start = GridPoint {
        row: *start_row,
        column: 0,
    };
    task.end = GridPoint {
        row: *end_row,
        column: u32::try_from(end_text.len()).unwrap_or(u32::MAX),
    };
    task.band_start_row = *start_row;
    task.band_end_row = *end_row;
    task.span = block.span;
    task.resolved = true;
    true
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
    fn only_closed_block_delimiters_are_detected() {
        let spans = detect_block_math("  $$x^2$$  ");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].source, "x^2");
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
                !block.span.source.contains("内部含"),
                "prose was captured by a mis-paired delimiter: {:?}",
                block.span.source
            );
        }
        // The genuine block in this window (5..7) may still resolve; the prose one must not.
        assert!(
            blocks
                .iter()
                .all(|block| !block.span.source.contains("多行里带"))
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
        assert!(blocks[0].span.source.contains("项"));
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
            detect_math_blocks_in_context(window, false).is_empty(),
            "an unknown prefix cannot prove that the first symmetric delimiter opens"
        );
        assert_eq!(
            detect_math_blocks_in_context(
                [
                    (TranscriptId(1), "$$"),
                    (TranscriptId(2), "x = y"),
                    (TranscriptId(3), "$$"),
                ],
                true,
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
            assert!(blocks[0].span.source.contains(expected_source));
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
            context_start_trusted: true,
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
                source: String::new(),
                mode: MathMode::Display,
                inline_runs: Vec::new(),
            },
            resolved: false,
        }
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
        assert_eq!(task.span.source, "x + y");
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
            blocks[0].span.source,
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
        assert_eq!(blocks[0].span.source, "x");
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
