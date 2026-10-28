//! **What a window tells UI Automation about its terminals** — the application's half
//! (0.4.9 plan row 88, T-UIA-SKELETON, build 88a).
//!
//! `bt_platform::uia` answers every question a client asks from one value, the window's
//! [`bt_platform::uia::WindowText`]; this module builds that value from what the window already holds — each
//! terminal pane of the active tab, its last presented frame and the metrics it was drawn at, its
//! body's rectangle, its name, and whether it holds the keyboard — and decides when it is built:
//! never before a client has asked (the door's `wanted`), and after that only on a turn
//! where a frame reached the glass or what the frames do not say (the panes, their places, their
//! names, the keyboard) moved. A turn that changes none of it builds nothing.

use bt_platform::uia::{CellIn, CellPoint, ClientRect, PaneText, RowPlace, RowText};
use bt_render::{CellMetrics, SeatViewport};
use bt_viewport::ViewportFrame;
use bt_viewport::horizontal::{WordClass, cluster_word_class};

/// **One window's report**: the door its subclass answers through (`crate::window_seam::UiaDoor`;
/// the product's is `bt_platform::uia::UiaHook`), and what decides whether a turn publishes.
pub(crate) struct UiaReport<U> {
    /// `None` where the window's handle could not be reached or its subclass would not install:
    /// the window then answers UI Automation as it did before.
    pub(crate) hook: Option<U>,
    /// A frame of the active tab reached the glass since the last publication.
    pub(crate) presented: bool,
    /// What the last publication was built from besides the frames.
    published: Option<Vec<PaneKey>>,
}

/// What a pane's publication depends on besides its frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PaneKey {
    pub(crate) seat: u64,
    pub(crate) body: [u32; 4],
    pub(crate) holds_keyboard: bool,
    pub(crate) name: String,
}

impl<U> UiaReport<U> {
    pub(crate) fn new(hook: Option<U>) -> Self {
        Self {
            hook,
            presented: false,
            published: None,
        }
    }

    /// **Whether this turn publishes**: a client has asked (`wanted`, the hook's), and a frame
    /// was presented or the panes' keys moved since the last publication. Spends the presented
    /// bit and records the keys when it says yes.
    pub(crate) fn due(&mut self, wanted: bool, keys: &[PaneKey]) -> bool {
        if !wanted {
            return false;
        }
        if !self.presented && self.published.as_deref() == Some(keys) {
            return false;
        }
        self.presented = false;
        self.published = Some(keys.to_vec());
        true
    }
}

/// **One pane's publication, from its presented frame.** `body` is the pane's body in the
/// window's physical pixels; every row is placed as the glass draws it (`frame_cell_bounds_px`'s
/// arithmetic: the padding, then the column times the cell width, and the row map's top).
pub(crate) fn pane_text(
    key: &PaneKey,
    body: SeatViewport,
    frame: &ViewportFrame,
    metrics: CellMetrics,
) -> PaneText {
    let columns = frame.columns.get() as usize;
    let padding = f64::from(metrics.padding_px);
    let left = f64::from(body.x) + padding;
    let top = f64::from(body.y) + padding;
    let subpixels = bt_viewport::SUBPIXELS_PER_PX as f64;
    let caret = frame.cursor_in_view.then_some(CellPoint {
        row: frame.cursor.row,
        column: frame.cursor.column,
    });
    // The class of the cell before, carried across a soft wrap so a word broken by the pane's
    // edge stays one word.
    let mut before = WordClass::Space;
    let drawn = frame.drawable_rows().min(frame.row_map.len());
    let mut rows = Vec::with_capacity(drawn);
    for (index, mapped) in frame.row_map.iter().take(drawn).enumerate() {
        let cells = frame
            .cells
            .get(index * columns..(index + 1) * columns)
            .unwrap_or(&[]);
        let mut word_starts = Vec::with_capacity(cells.len());
        for cell in cells {
            if cell.wide_spacer {
                word_starts.push(false);
                continue;
            }
            let class = cluster_word_class(cell.text.as_str());
            word_starts.push(class != WordClass::Space && class != before);
            before = class;
        }
        if !mapped.continues {
            before = WordClass::Space;
        }
        let row = u32::try_from(index).unwrap_or(u32::MAX);
        let keep = caret
            .filter(|caret| caret.row == row)
            .map_or(0, |caret| caret.column);
        rows.push(RowText::from_cells(
            cells
                .iter()
                .zip(word_starts)
                .map(|(cell, word_start)| CellIn {
                    text: cell.text.as_str(),
                    spacer: cell.wide_spacer,
                    word_start,
                }),
            RowPlace {
                left,
                cell_width: f64::from(metrics.cell_width_px),
                top: top + mapped.top_subpixels as f64 / subpixels,
                height: mapped.height_subpixels as f64 / subpixels,
            },
            mapped.continues,
            keep,
        ));
    }
    let point = |row: u32, column: u32| CellPoint { row, column };
    let selection = frame
        .selection_spans
        .iter()
        .map(|span| {
            (
                point(span.row, span.start_column),
                point(span.row, span.end_column),
            )
        })
        .reduce(|(start, end), (next_start, next_end)| {
            let order = |point: &CellPoint| (point.row, point.column);
            (
                std::cmp::min_by_key(start, next_start, order),
                std::cmp::max_by_key(end, next_end, order),
            )
        });
    PaneText::new(
        key.seat,
        key.name.clone(),
        client_rect(body),
        key.holds_keyboard,
        rows,
        caret,
        selection,
    )
}

/// A pane body as the publication's rectangle.
pub(crate) fn client_rect(body: SeatViewport) -> ClientRect {
    ClientRect {
        left: f64::from(body.x),
        top: f64::from(body.y),
        width: f64::from(body.width),
        height: f64::from(body.height),
    }
}

#[cfg(test)]
#[path = "uia_report_tests.rs"]
mod tests;
