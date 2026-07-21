use std::collections::BTreeSet;

use bt_viewport::{MathBlockDisplay, ViewportFrame};

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
}

/// Classify the exact `ViewportFrame` handed to the renderer. A rendered block carries its source
/// in frame metadata; an exposed source block remains visible in the terminal cell plane.
pub fn observe_formula_frame(frame: &ViewportFrame) -> FormulaFrameObservation {
    let rendered_sources = frame
        .math_blocks
        .iter()
        .filter(|block| block.display == MathBlockDisplay::Rendered)
        .map(|block| block.source.clone())
        .collect::<Vec<_>>();
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
    }
}

/// Stateful oracle for the transient sequence static screenshot tests miss. Once an exact formula
/// source has appeared as rendered, any later frame exposing that source is a repaint flash.
#[derive(Debug, Default)]
pub struct FormulaFlashOracle {
    frames: Vec<FormulaFrameObservation>,
    rendered_sources: BTreeSet<String>,
    flashed_sources: BTreeSet<String>,
}

impl FormulaFlashOracle {
    pub fn observe(&mut self, frame: &ViewportFrame) -> &FormulaFrameObservation {
        let observation = observe_formula_frame(frame);
        for source in &self.rendered_sources {
            if source_rows_expose(&observation.source_rows, &observation.source_plane, source) {
                self.flashed_sources.insert(source.clone());
            }
        }
        self.rendered_sources
            .extend(observation.rendered_sources.iter().cloned());
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
    [
        format!("$${source}$$"),
        format!("$$\n{source}\n$$"),
        format!(r"\[{source}\]"),
        format!("\\[\n{source}\n\\]"),
    ]
    .iter()
    .any(|delimited| source_plane.contains(delimited))
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
    use std::num::NonZeroU32;

    use bt_doc::{LayoutKey, ViewGeneration};
    use bt_transcript::CapturedCell;
    use bt_viewport::{FrameViewportOrigin, GridCursor, ViewportFrame};

    fn source_frame(text: &str) -> ViewportFrame {
        let columns = NonZeroU32::new(12).unwrap();
        let rows = NonZeroU32::new(1).unwrap();
        let mut cells = text
            .chars()
            .map(|character| CapturedCell {
                text: character.to_string(),
                ..CapturedCell::default()
            })
            .collect::<Vec<_>>();
        cells.resize(columns.get() as usize, CapturedCell::default());
        ViewportFrame {
            columns,
            rows,
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
}
