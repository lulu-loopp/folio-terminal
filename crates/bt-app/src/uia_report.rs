//! **What a window tells UI Automation about its terminals** — the application's half
//! (0.4.9 plan row 88, T-UIA-SKELETON, build 88a).
//!
//! `bt_platform::uia` answers every question a client asks from one value, the window's
//! [`bt_platform::uia::WindowText`]; this module builds that value from what the window already holds — each
//! terminal pane of the active tab, its last presented frame and the metrics it was drawn at, its
//! body's rectangle, its name, and whether it holds the keyboard — and decides when it is built:
//! never before a client has asked (the door's `wanted`), and after that only on a turn where what
//! the frames do not say (the panes, their places, their names, the keyboard) moved, or where a
//! presented frame says something the last publication did not — at most once every
//! [`UIA_FRAME_INTERVAL`] for those. A turn that changes none of it builds nothing.

use std::time::{Duration, Instant};

use bt_platform::uia::{CellIn, CellPoint, ClientRect, PaneId, PaneText, RowPlace, RowText};
use bt_render::{CellMetrics, SeatViewport};
use bt_viewport::ViewportFrame;
use bt_viewport::horizontal::{WordClass, cluster_word_class};

/// **How often a frame-only change is published, at most** (the review's F2): text that streams
/// is published ten times a second, never once a frame; a change of what the frames do not say
/// (the panes, their places, their names, the keyboard) is published on its own turn.
pub(crate) const UIA_FRAME_INTERVAL: Duration = Duration::from_millis(100);

/// **One window's report**: the door its subclass answers through (`crate::window_seam::UiaDoor`;
/// the product's is `bt_platform::uia::UiaHook`), and what decides whether a turn publishes.
pub(crate) struct UiaReport<U> {
    /// `None` where the window's handle could not be reached or its subclass would not install:
    /// the window then answers UI Automation as it did before.
    pub(crate) hook: Option<U>,
    /// A frame of the active tab reached the glass since the last look at the frames.
    pub(crate) presented: bool,
    /// What the last publication was built from, and when.
    published: Option<Published>,
    /// A frame-only change is held for [`UIA_FRAME_INTERVAL`] after the last publication.
    held: bool,
    /// The threads of a client's first call have been said in `diagnostics.log`.
    pub(crate) threads_said: bool,
}

/// What a publication was built from: the keys, each pane's frame at the metrics it was drawn
/// at, and the moment.
struct Published {
    keys: Vec<PaneKey>,
    frames: Vec<(ViewportFrame, CellMetrics)>,
    at: Instant,
}

/// What a pane's publication depends on besides its frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PaneKey {
    pub(crate) id: PaneId,
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
            held: false,
            threads_said: false,
        }
    }

    /// **Whether this turn looks at the frames at all**: nothing has been published yet, the
    /// keys moved, a frame reached the glass, or a held change is due. An idle turn reads only
    /// the keys.
    pub(crate) fn looks(&self, keys: &[PaneKey], now: Instant) -> bool {
        let Some(last) = &self.published else {
            return true;
        };
        self.presented || last.keys != keys || self.deadline().is_some_and(|due| now >= due)
    }

    /// **Whether this turn publishes** — `pace::LatestThrottle`'s three rules, for the frames,
    /// with the keys outside them:
    ///
    /// * the keys moved (or nothing was published): now;
    /// * every pane's frame says what the published one said (its rows' text, their places, the
    ///   caret, the selection, the metrics): nothing, and nothing is held;
    /// * a frame says something new: now if [`UIA_FRAME_INTERVAL`] has passed since the last
    ///   publication, held otherwise, with [`Self::deadline`] saying when.
    ///
    /// Spends the presented bit, and records what it says yes to.
    pub(crate) fn due(
        &mut self,
        keys: &[PaneKey],
        frames: &[(&ViewportFrame, CellMetrics)],
        now: Instant,
    ) -> bool {
        self.presented = false;
        let publish = match &self.published {
            None => true,
            Some(last) if last.keys != keys => true,
            Some(last) => {
                let same = last.frames.len() == frames.len()
                    && last.frames.iter().zip(frames).all(
                        |((old, old_metrics), (new, new_metrics))| {
                            says_the_same(old, *old_metrics, new, *new_metrics)
                        },
                    );
                if same {
                    self.held = false;
                    false
                } else if now.saturating_duration_since(last.at) >= UIA_FRAME_INTERVAL {
                    true
                } else {
                    self.held = true;
                    false
                }
            }
        };
        if publish {
            self.held = false;
            self.published = Some(Published {
                keys: keys.to_vec(),
                frames: frames
                    .iter()
                    .map(|(frame, metrics)| ((*frame).clone(), *metrics))
                    .collect(),
                at: now,
            });
        }
        publish
    }

    /// When a held frame-only change is owed; the turn's wake fold reads it.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        let last = self.published.as_ref().filter(|_| self.held)?;
        Some(last.at + UIA_FRAME_INTERVAL)
    }
}

/// The cells of a frame's first `rows` rows.
fn drawn_cells(frame: &ViewportFrame, rows: usize) -> &[bt_transcript::CapturedCell] {
    let end = (rows * frame.columns.get() as usize).min(frame.cells.len());
    &frame.cells[..end]
}

/// **Whether two frames say the same to UI Automation** — everything [`pane_text`] reads: the
/// width, the drawn rows' placement records (whole, which hold a little more than their places
/// and wraps), their cells' text and spacers, the caret, the selection, and the metrics that
/// place them. A difference it sees that the publication would not show costs one rebuild; one
/// it missed would leave a client reading old text, so it misses none.
fn says_the_same(
    old: &ViewportFrame,
    old_metrics: CellMetrics,
    new: &ViewportFrame,
    new_metrics: CellMetrics,
) -> bool {
    let drawn = |frame: &ViewportFrame| frame.drawable_rows().min(frame.row_map.len());
    let rows = drawn(old);
    if old.columns != new.columns
        || rows != drawn(new)
        || old.row_map[..rows] != new.row_map[..rows]
        || old.cursor_in_view != new.cursor_in_view
        || (old.cursor.row, old.cursor.column) != (new.cursor.row, new.cursor.column)
        || old.selection_spans != new.selection_spans
        || old_metrics.padding_px.to_bits() != new_metrics.padding_px.to_bits()
        || old_metrics.cell_width_px.to_bits() != new_metrics.cell_width_px.to_bits()
    {
        return false;
    }
    let (old_cells, new_cells) = (drawn_cells(old, rows), drawn_cells(new, rows));
    old_cells.len() == new_cells.len()
        && old_cells.iter().zip(new_cells).all(|(old, new)| {
            old.wide_spacer == new.wide_spacer && old.text.as_str() == new.text.as_str()
        })
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
    let selection = frame
        .selection_spans
        .iter()
        .map(|span| {
            (
                CellPoint {
                    row: span.row,
                    column: span.start_column,
                },
                CellPoint {
                    row: span.row,
                    column: span.end_column,
                },
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
        key.id,
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
