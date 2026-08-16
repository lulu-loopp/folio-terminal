use std::collections::{BTreeMap, BTreeSet};

use bt_viewport::{LiveMathOccurrenceId, MathBlockDisplay, RgbaArtifactKind, ViewportFrame};

/// Headless classification of one frame's formula presentation. This is diagnostic state only;
/// render and detection decisions remain owned by the normal session pipeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormulaFrameState {
    Empty,
    Source,
    Rendered,
    Mixed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormulaFrameObservation {
    pub state: FormulaFrameState,
    pub rendered_sources: Vec<String>,
    pub source_rows: Vec<String>,
    /// Every visible row (trimmed) joined by `\n`. Unlike `source_rows`, this keeps the delimiter-
    /// free body lines of a multi-line block, so a reverted `$$\n<body>\n$$` can be matched against
    /// the source it used to render from.
    pub source_plane: String,
    /// Sources whose proven identity is still present but whose missing rows are covered by an
    /// application-internal fixed region. This is per occurrence, never a whole-frame exemption.
    pub occluded_sources: Vec<String>,
    pub rendered_occurrences: BTreeMap<LiveMathOccurrenceId, String>,
    pub occluded_occurrences: BTreeSet<LiveMathOccurrenceId>,
}

/// Every artifact kind that occupies a presentation band: the math raster and both image bands.
/// Band geometry, ownership, clipping and source occlusion are one mechanism for all three, so
/// every diagnostic that audits a band audits all of them. Spelled out variant by variant rather
/// than defaulted to `true`, so a future artifact kind has to opt in deliberately.
pub fn is_banded_artifact(kind: &RgbaArtifactKind) -> bool {
    matches!(
        kind,
        RgbaArtifactKind::Math
            | RgbaArtifactKind::InlineImage { .. }
            | RgbaArtifactKind::LocalImagePath { .. }
    )
}

/// The banded kinds whose artifact stands in for the terminal rows underneath it. A math raster and
/// an OSC 1337 inline image are drawn over their own band rows, so those rows must be blanked and
/// must belong to that block alone. A local image path band is not one of them: its height is added
/// to its source row's height, so it paints in space nothing else occupies, below a path line that
/// deliberately stays readable, and several of them stack under one row. Auditing row suppression or
/// row exclusivity against a local-path band would assert the opposite of its layout.
pub fn band_owns_its_rows(kind: &RgbaArtifactKind) -> bool {
    match kind {
        RgbaArtifactKind::Math | RgbaArtifactKind::InlineImage { .. } => true,
        RgbaArtifactKind::LocalImagePath { .. } => false,
    }
}

/// Classify the exact `ViewportFrame` handed to the renderer. A rendered block carries its source
/// in frame metadata; an exposed source block remains visible in the terminal cell plane.
///
/// Image bands are observed alongside formulas. Their `source` is a file path (or `[image]`),
/// which carries none of the display-math delimiters `source_rows_expose` matches on, so an image
/// band contributes presence and occurrence identity here without ever being reported as an
/// exposed-source flash — the path line under an image band is deliberately still visible.
pub fn observe_formula_frame(frame: &ViewportFrame) -> FormulaFrameObservation {
    let rendered_sources = frame
        .math_blocks
        .iter()
        .filter(|block| {
            block.display == MathBlockDisplay::Rendered && is_banded_artifact(&block.artifact.kind)
        })
        .map(|block| block.source.clone())
        .collect::<Vec<_>>();
    let occluded_sources = frame
        .math_blocks
        .iter()
        .filter(|block| {
            block.display == MathBlockDisplay::Rendered
                && is_banded_artifact(&block.artifact.kind)
                && block.occluded_source_rows != 0
        })
        .map(|block| block.source.clone())
        .collect::<Vec<_>>();
    let rendered_occurrences = frame
        .math_blocks
        .iter()
        .filter_map(|block| {
            (block.display == MathBlockDisplay::Rendered
                && is_banded_artifact(&block.artifact.kind))
            .then_some(block.live_occurrence_id)
            .flatten()
            .map(|id| (id, block.source.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let occluded_occurrences = frame
        .math_blocks
        .iter()
        .filter_map(|block| {
            (block.display == MathBlockDisplay::Rendered
                && is_banded_artifact(&block.artifact.kind)
                && block.occluded_source_rows != 0)
                .then_some(block.live_occurrence_id)
                .flatten()
        })
        .collect::<BTreeSet<_>>();
    let columns = frame.columns.get() as usize;
    let all_rows = frame
        .cells
        .chunks(columns)
        .map(|row| {
            row.iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let source_rows = all_rows
        .iter()
        .filter(|row| row_may_contain_display_math(row))
        .cloned()
        .collect::<Vec<_>>();
    let source_plane = all_rows.join("\n");
    let state = match (rendered_sources.is_empty(), source_rows.is_empty()) {
        (true, true) => FormulaFrameState::Empty,
        (true, false) => FormulaFrameState::Source,
        (false, true) => FormulaFrameState::Rendered,
        (false, false) => FormulaFrameState::Mixed,
    };
    FormulaFrameObservation {
        state,
        rendered_sources,
        source_rows,
        source_plane,
        occluded_sources,
        rendered_occurrences,
        occluded_occurrences,
    }
}

/// Stateful oracle for the transient sequence static screenshot tests miss. Once an exact formula
/// source has appeared as rendered, any later frame exposing that source is a repaint flash.
#[derive(Debug, Default)]
pub struct FormulaFlashOracle {
    frames: Vec<FormulaFrameObservation>,
    active_occurrences: BTreeMap<LiveMathOccurrenceId, String>,
    anonymous_rendered_sources: BTreeSet<String>,
    flashed_sources: BTreeSet<String>,
}

impl FormulaFlashOracle {
    pub fn observe(&mut self, frame: &ViewportFrame) -> &FormulaFrameObservation {
        let observation = observe_formula_frame(frame);
        for (id, source) in std::mem::take(&mut self.active_occurrences) {
            let exposed =
                source_rows_expose(&observation.source_rows, &observation.source_plane, &source);
            if exposed && !observation.occluded_occurrences.contains(&id) {
                self.flashed_sources.insert(source.clone());
            }
            if observation.rendered_occurrences.contains_key(&id) {
                self.active_occurrences.insert(id, source);
            }
        }
        for source in &self.anonymous_rendered_sources {
            if !observation.occluded_sources.contains(source)
                && source_rows_expose(&observation.source_rows, &observation.source_plane, source)
            {
                self.flashed_sources.insert(source.clone());
            }
        }
        self.active_occurrences.extend(
            observation
                .rendered_occurrences
                .iter()
                .map(|(id, source)| (*id, source.clone())),
        );
        self.anonymous_rendered_sources.extend(
            frame
                .math_blocks
                .iter()
                .filter(|block| {
                    block.display == MathBlockDisplay::Rendered
                        && is_banded_artifact(&block.artifact.kind)
                        && block.live_occurrence_id.is_none()
                })
                .map(|block| block.source.clone()),
        );
        self.frames.push(observation);
        self.frames.last().expect("just pushed one observation")
    }

    pub fn frames(&self) -> &[FormulaFrameObservation] {
        &self.frames
    }

    pub fn flashed_sources(&self) -> &BTreeSet<String> {
        &self.flashed_sources
    }

    pub fn flash_detected(&self) -> bool {
        !self.flashed_sources.is_empty()
    }
}

fn source_rows_expose(rows: &[String], source_plane: &str, rendered_source: &str) -> bool {
    let source = rendered_source.trim();
    if source.is_empty() {
        return false;
    }
    if rows.iter().any(|row| {
        let row = row.trim();
        row == source
            || row
                .strip_prefix("$$")
                .and_then(|inner| inner.strip_suffix("$$"))
                .is_some_and(|inner| inner.trim() == source)
            || row
                .strip_prefix(r"\[")
                .and_then(|inner| inner.strip_suffix(r"\]"))
                .is_some_and(|inner| inner.trim() == source)
    }) {
        return true;
    }
    // The multi-line delimiter-on-its-own-line forms: match against the full cell plane, which
    // retains the delimiter-free body rows that `rows` (delimiter-filtered) drops.
    if let Some(environment) = rendered_environment_name(source)
        && source_plane.contains(&format!(r"\begin{{{environment}}}"))
        && source_plane.contains(&format!(r"\end{{{environment}}}"))
    {
        return true;
    }
    [
        format!("$${source}$$"),
        format!("$$\n{source}\n$$"),
        format!(r"\[{source}\]"),
        format!("\\[\n{source}\n\\]"),
    ]
    .iter()
    .any(|delimited| source_plane.contains(delimited))
}

fn rendered_environment_name(source: &str) -> Option<&str> {
    let suffix = source.split_once(r"\begin{")?.1;
    suffix.split_once('}').map(|(environment, _)| environment)
}

fn row_may_contain_display_math(row: &str) -> bool {
    row.contains("$$")
        || row.contains(r"\[")
        || row.contains(r"\]")
        || row.contains(r"\begin{")
        || row.contains(r"\end{")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{num::NonZeroU32, sync::Arc};

    use bt_doc::{GridGeneration, GridPoint, LayoutKey, MathMode, ScreenId, ViewGeneration};
    use bt_transcript::CapturedCell;
    use bt_transcript::TranscriptId;
    use bt_viewport::{
        FrameViewportOrigin, GridCursor, HorizontalOverflowOwner, LiveMathOccurrenceId,
        MathBlockAnchor, MathBlockDisplay, MathBlockPlacement, ProjectedMathArtifact,
        ViewportFrame,
    };

    fn source_frame(text: &str) -> ViewportFrame {
        let columns = NonZeroU32::new(12).unwrap();
        let rows = NonZeroU32::new(1).unwrap();
        let mut cells = text
            .chars()
            .map(|character| CapturedCell {
                text: character.into(),
                ..CapturedCell::default()
            })
            .collect::<Vec<_>>();
        cells.resize(columns.get() as usize, CapturedCell::default());
        ViewportFrame {
            columns,
            grid_rows: rows,
            rows,
            presentation_offset_subpixels: 0,
            cells,
            cursor: GridCursor {
                row: 0,
                column: 0,
                visible: false,
            },
            cell_anchors: Vec::new(),
            row_map: Vec::new(),
            selection_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: LayoutKey {
                width_cells: columns,
                dpi_milli: NonZeroU32::new(1000).unwrap(),
                font_rev: 1,
                theme_rev: 1,
            },
            view_generation: ViewGeneration(0),
        }
    }

    #[test]
    fn source_cells_are_observed_before_the_renderer_plane() {
        let frame = source_frame("$$x^2$$");
        let observation = observe_formula_frame(&frame);
        assert_eq!(observation.state, FormulaFrameState::Source);
        assert_eq!(observation.source_rows, ["$$x^2$$"]);
    }

    #[test]
    fn exact_source_matching_does_not_confuse_a_formula_with_a_superstring() {
        assert!(source_rows_expose(&["$$x$$".to_owned()], "$$x$$", "x"));
        assert!(!source_rows_expose(&["$$x+y$$".to_owned()], "$$x+y$$", "x"));
    }

    #[test]
    fn a_reverted_multiline_block_body_row_is_recognised_as_the_rendered_source() {
        // A `$$` / body / `$$` block that flipped back to source: the body row has no delimiter and
        // is dropped from `source_rows`, but the full plane must still expose the rendered source.
        let rows = vec!["$$".to_owned(), "$$".to_owned()];
        let plane = "filler\n$$\n\\nabla \\cdot \\mathbf{E}\n$$\nprompt>";
        assert!(source_rows_expose(
            &rows,
            plane,
            "\\nabla \\cdot \\mathbf{E}"
        ));
        // An unrelated formula that never rendered must not be reported.
        assert!(!source_rows_expose(&rows, plane, "\\int_0^1 x\\,dx"));
    }

    fn rendered_frame(id: LiveMathOccurrenceId, occluded_source_rows: u32) -> ViewportFrame {
        let mut frame = source_frame("");
        frame.math_blocks.push(MathBlockPlacement {
            start: TranscriptId(0),
            anchor: MathBlockAnchor::Live {
                run: None,
                screen: ScreenId::Alternate,
                start: GridPoint { row: 0, column: 0 },
                end: GridPoint { row: 0, column: 4 },
                band_start_row: 0,
                band_end_row: 0,
                generation: GridGeneration(1),
            },
            source: "x".to_owned(),
            artifact: ProjectedMathArtifact {
                inline_runs: Vec::new(),
                key: "x".to_owned(),
                end: TranscriptId(0),
                rgba: Arc::from(vec![255_u8; 4]),
                width_px: 1,
                height_px: 1,
                height_subpixels: 1,
                baseline_subpixels: 0,
                mode: MathMode::Display,
                kind: bt_viewport::RgbaArtifactKind::Math,
                vertical_padding_subpixels: 0,
                render_scale_milli: 1000,
                source: "x".to_owned(),
            },
            top_subpixels: 0,
            left_subpixels: 0,
            content_offset_subpixels: 0,
            clip_height_subpixels: 1,
            display: MathBlockDisplay::Rendered,
            horizontal_overflow: HorizontalOverflowOwner::Block,
            horizontal_scroll_px: 0,
            vertical_scroll_px: 0,
            toolbar_visible: false,
            occluded_source_rows,
            occluded_visible_rows: Vec::new(),
            live_occurrence_id: Some(id),
            frozen_prefix_rows: 0,
            clipped_top_rows: 0,
            clipped_bottom_rows: 0,
        });
        frame
    }

    #[test]
    fn restored_environment_source_is_matched_to_raw_grid_markers() {
        let rendered = "\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}";
        let plane = "$$\n\\begin{aligned}\na &= b \\\nc &= d\n\\end{aligned}\n$$";
        assert!(source_rows_expose(
            &[r"\begin{aligned}".to_owned(), r"\end{aligned}".to_owned()],
            plane,
            rendered,
        ));
        assert!(
            source_rows_expose(
                &[r"\begin{aligned}".to_owned(), r"\end{aligned}".to_owned()],
                &format!("{plane}\nJump to bottom (ctrl+End)"),
                rendered,
            ),
            "an unrelated Jump chip must not exempt the whole frame"
        );
    }

    #[test]
    fn dropping_a_live_occurrence_while_its_source_is_exposed_is_a_flash() {
        let id = LiveMathOccurrenceId(7);
        let mut oracle = FormulaFlashOracle::default();
        oracle.observe(&rendered_frame(id, 0));
        oracle.observe(&source_frame("$$x$$"));
        assert!(oracle.flash_detected());
    }

    #[test]
    fn only_the_same_occluded_occurrence_receives_the_flash_exemption() {
        let id = LiveMathOccurrenceId(7);
        let mut oracle = FormulaFlashOracle::default();
        oracle.observe(&rendered_frame(id, 0));
        let mut occluded = rendered_frame(id, 1);
        occluded.cells = source_frame("$$x$$").cells;
        oracle.observe(&occluded);
        assert!(!oracle.flash_detected());

        let mut dropped = FormulaFlashOracle::default();
        dropped.observe(&rendered_frame(id, 0));
        dropped.observe(&source_frame("$$x$$"));
        assert!(dropped.flash_detected());
    }

    #[test]
    fn an_offscreen_occurrence_does_not_poison_a_later_equal_source() {
        let mut oracle = FormulaFlashOracle::default();
        oracle.observe(&rendered_frame(LiveMathOccurrenceId(7), 0));
        oracle.observe(&source_frame(""));
        oracle.observe(&source_frame("$$x$$"));
        assert!(!oracle.flash_detected());
    }
}
