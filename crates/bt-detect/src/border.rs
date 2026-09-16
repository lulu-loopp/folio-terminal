//! **Recognising the frame a multiplexer draws beside a pane.**
//!
//! A terminal multiplexer does not hand the host terminal the bytes its panes printed. It repaints
//! the host screen itself, row by row, and every row it paints carries the frame: a sidebar, a
//! vertical rule, then the pane's own text. `herdr` writes twenty-five blank cells, a `│` (U+2502)
//! and only then the pane, so the line the detector reads is `<25 spaces>│$$` and not `$$`. A
//! `tmux` vertical split does the same with two panes: `<left pane>│<right pane>`, the rule in one
//! fixed column on every row.
//!
//! Read as one line of text, `<25 spaces>│$$` is four-space-indented CommonMark code, and its
//! trimmed form opens with `│` and not with `$$`. Both readings are right — and both are about a
//! line that was never printed. What is printed is two independent columns of text with a rule
//! between them, and the only honest fix is to see the rule: find the columns a screen draws one
//! in, cut the screen there, and let each **region** be scanned over its own columns, where the
//! pane's text starts at the region's first column. Every gate downstream — indented code, clean
//! `$$` delimiters, the prose checks, the inline-site rules — is untouched and sees a region-local
//! line. Nothing is loosened; the detector is simply no longer shown a line that is really two.
//!
//! **What counts as a rule**, in two tests a column must pass both of.
//!
//! *The share.* The same vertical box-drawing glyph — [`vertical_rule`] lists them — stands in the
//! column on at least [`BORDER_ROW_SHARE_PERMILLE`] of all the screen's rows and on at least
//! [`BORDER_MINIMUM_ROWS`] of them. The denominator is every row handed in, never a subset, and
//! that is what keeps a table drawn *inside* a TUI from cutting the screen: a table occupies a
//! handful of a screen's rows, so its rules reach nowhere near nine tenths of them, while a pane
//! rule runs the full height by construction.
//!
//! *The frame is unbroken* (owner's rulings 2026-09-16 and 2026-09-17). A share is not proof,
//! because the rows it leaves over are rows the cut still runs through. So on every row that does
//! not carry the vertical line, the column must be **clear**: the cell is blank, and the row's text
//! does not cross it — no writing immediately left of it *and* immediately right of it.
//!
//! *Carrying the line is not drawing the rule.* The plain rule nominates a column; any glyph with a
//! vertical stroke keeps it ([`continues_vertical`]). So the `┼` where a TUI draws its horizontal
//! separator across a pane rule — the middle row of a `tmux` 2×2 layout — leaves the vertical border
//! standing, and `├ ┤ ┬ ┴`, the corners, the arcs and the doubles do the same. A row of plain `─`
//! does not, and should not: that is a horizontal pane boundary, which this does not model.
//!
//! *One row may break it, and it must be a status line.* Only the topmost or the bottommost, never a
//! middle one, because a frame with a hole in the middle of it is not a frame and the hole is text
//! the cut would run through. And only a row carrying no `$`: the exempt row is still sliced, so
//! exempting a row with a formula on it is throwing that formula away, and a formula the screen can
//! prove outranks a split the screen only infers. A status line never carries math.
//!
//! The rulings' own counter-example is what this is for. Thirty-six rows of `log  │ text`, one bare
//! `$$x^2$$` row, three `plain` rows: nine tenths of the screen draws the rule in column five, and
//! that one row writes straight through it. The screen is one body of text with a glyph in it —
//! pipe-aligned output — and cutting it would take the formula apart and lose one that used to
//! typeset. Move the `$$x^2$$` row to the bottom and it is still refused, because it carries a
//! dollar; put a `bash 12:00` status line there instead and the screen splits.
//!
//! **A table drawn with box glyphs on every row now splits into its cells, and that is harmless.**
//! Its separator rows carry junctions, which continue the line, so each column of it becomes a
//! region. Nothing is lost by that: a cell's math is proved inside its own region exactly as it was
//! proved inside the whole screen, because a table's rules are drawn *between* its cells and a cut
//! along them runs through no text.
//!
//! **A fence the screen owns suppresses every region; a fence a pane prints is that pane's**
//! (owner's rulings 2026-09-16 and 2026-09-17). A region is a column of the screen and a code fence
//! is not: the ``` that opens one stands in whichever region it was printed in, and every other
//! region would begin from a neutral state and read the code between the fences as ordinary text.
//! Thirty-eight rows of `log │ $x^2$` between two bare fence lines are thirty-eight formulas to a
//! region that never saw the fence, and none at all to the screen — and the screen is right,
//! because those fence lines are not a pane's: the frame does not run through them. That is the
//! whole of the rule, and `bt_detect`'s `fenced_lines` asks it of a fence's opening row. A fence
//! whose opening row the frame *does* run through was printed inside a pane; suppressing the pane
//! across the rule from it would silence an independent program for rows of somebody else's output,
//! so it is left to the region that owns it, whose own scan refuses it as it always has. A fence
//! already open before the screen's first row was opened on no row of the screen at all, so it is
//! the screen's and vetoes from the top — the live proof walks the frozen tail ahead of the grid to
//! establish it.
//!
//! **Why ASCII `|` is not a rule.** It was considered and refused. A screen whose rows all carry a
//! `|` in one column is far more often a table — `mysql`, `column -t`, a markdown table long
//! enough to fill the screen — than a frame, and at this altitude the two are the same picture:
//! there is no local evidence that separates them. A missed split costs what today already costs
//! (the formula stays source); a wrong split re-cuts a table that `bt_detect::table` owns, and
//! hands every gate below a line that was whole. The multiplexers this is for — herdr, tmux,
//! zellij, wezterm — all draw their rule from the U+2500 block.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{LiveDetectionInput, LiveDetectionSource};

/// Share of a screen's rows that must carry the same vertical rule in one column before that
/// column is a border, in thousandths.
pub const BORDER_ROW_SHARE_PERMILLE: usize = 900;

/// Fewest rows a screen must hold, and fewest of them a column must carry the rule in, before any
/// column of it can be read as a border.
pub const BORDER_MINIMUM_ROWS: usize = 3;

/// The glyph, when `cluster` is one box-drawing character that stands for a vertical rule and
/// nothing else.
///
/// Only the members of U+2500–U+257F whose whole form is the vertical stroke are here, because
/// this is the glyph that *nominates* a column: a screen is framed at the column its panes are
/// drawn beside, and that column is drawn with a plain rule on the overwhelming majority of its
/// rows. A junction continues such a rule but never proposes one on its own — see
/// [`continues_vertical`], which is the question asked of every other row.
#[must_use]
pub fn vertical_rule(cluster: &str) -> Option<char> {
    let mut characters = cluster.chars();
    let glyph = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    matches!(
        glyph,
        '\u{2502}'   // │ light vertical
            | '\u{2503}' // ┃ heavy vertical
            | '\u{2506}' // ┆ light triple dash vertical
            | '\u{2507}' // ┇ heavy triple dash vertical
            | '\u{250a}' // ┊ light quadruple dash vertical
            | '\u{250b}' // ┋ heavy quadruple dash vertical
            | '\u{254e}' // ╎ light double dash vertical
            | '\u{254f}' // ╏ heavy double dash vertical
            | '\u{2551}' // ║ double vertical
    )
    .then_some(glyph)
}

/// **Does this cluster carry a vertical stroke?** — the question asked of a row that does not draw
/// the plain rule, and the one a *junction* answers yes to.
///
/// `├`, `┤`, `┼`, `┬`, `┴`, the corners, the arcs, the doubles and the half-verticals all contain a
/// vertical stroke and continue the line through their row; only the pure horizontals (`─`, `━`,
/// the dashes, `═`, `╌`), the diagonals and the left/right halves do not. So the row a TUI draws its
/// horizontal separator on — `──────────┼──────────` across a `tmux` 2×2 layout — keeps the
/// vertical border alive, while a row of plain `─` breaks it, which is what a horizontal pane
/// boundary *is*.
///
/// Stated as the short list of what has no vertical stroke rather than the long list of what does,
/// because the block has about a hundred and ten of the latter and eighteen of the former.
#[must_use]
pub fn continues_vertical(cluster: &str) -> bool {
    let mut characters = cluster.chars();
    let Some(glyph) = characters.next() else {
        return false;
    };
    if characters.next().is_some() {
        return false;
    }
    if !('\u{2500}'..='\u{257f}').contains(&glyph) {
        return false;
    }
    !matches!(
        glyph,
        '\u{2500}' | '\u{2501}'   // ─ ━ horizontal
            | '\u{2504}' | '\u{2505}' // ┄ ┅ triple dash horizontal
            | '\u{2508}' | '\u{2509}' // ┈ ┉ quadruple dash horizontal
            | '\u{254c}' | '\u{254d}' // ╌ ╍ double dash horizontal
            | '\u{2550}'              // ═ double horizontal
            | '\u{2571}' | '\u{2572}' | '\u{2573}' // ╱ ╲ ╳ diagonals
            | '\u{2574}' | '\u{2576}' // ╴ ╶ light left / right half
            | '\u{2578}' | '\u{257a}' // ╸ ╺ heavy left / right half
            | '\u{257c}' | '\u{257e}' // ╼ ╾ mixed-weight horizontals
    )
}

/// One vertical slice of a screen: the cells between two border columns, or between a border and a
/// screen edge.
///
/// Columns are terminal cells, counted the way the detector's own capture geometry counts them, so
/// a wide glyph occupies two of them and a cluster that would straddle a boundary belongs to
/// neither side.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScreenRegion {
    /// First cell column of the region — the column this region's lines begin at.
    pub column_start: u32,
    /// One past the region's last cell column, or `None` when the region runs to the screen's
    /// right edge (whose column the detector has no reason to know).
    pub column_end: Option<u32>,
}

impl Default for ScreenRegion {
    fn default() -> Self {
        Self::WHOLE
    }
}

impl ScreenRegion {
    /// The unsplit screen: every column, starting at zero. What every scan ran over before regions
    /// existed, and what a screen with no border still produces.
    pub const WHOLE: Self = Self {
        column_start: 0,
        column_end: None,
    };

    #[must_use]
    pub fn is_whole(self) -> bool {
        self == Self::WHOLE
    }

    #[must_use]
    pub fn contains(self, column: u32) -> bool {
        column >= self.column_start && self.column_end.is_none_or(|end| column < end)
    }

    /// Do these two regions share a cell column? Two regions of one split never do — that is what
    /// lets a band in one pane stand while a band in the other stands on the same rows.
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        other.column_end.is_none_or(|end| self.column_start < end)
            && self.column_end.is_none_or(|end| other.column_start < end)
    }
}

/// `(byte, cell)` boundaries for a plain string, in the shape
/// [`LiveDetectionInput::cell_boundaries`] holds: one entry per grapheme-cluster edge, monotonic in
/// both coordinates, ending at the string's length.
///
/// The live path never reaches here — it carries boundaries measured off the terminal's own cells,
/// which are authoritative. This is for callers holding text alone, and it measures with the same
/// [`bt_unicode`] cluster widths the rest of the detector measures with.
fn text_cell_boundaries(text: &str) -> Vec<(u32, u32)> {
    let mut boundaries = vec![(0, 0)];
    let mut byte: u32 = 0;
    let mut column: u32 = 0;
    for cluster in bt_unicode::graphemes(text) {
        byte = byte.saturating_add(u32::try_from(cluster.len()).unwrap_or(u32::MAX));
        column =
            column.saturating_add(u32::try_from(bt_unicode::cluster_width(cluster)).unwrap_or(0));
        boundaries.push((byte, column));
    }
    boundaries
}

/// One screen row, measured for the two questions a border column asks of it: is the rule drawn
/// here, and — where it is not — is this column clear.
///
/// `runs` are the row's non-blank cell ranges, merged, so "is column `c` covered by text" and "does
/// text cross column `c`" are both answered by looking `c`, `c - 1` and `c + 1` up in them. `rules`
/// are the columns holding a one-cell vertical rule, with the glyph drawn there.
#[derive(Default)]
struct RowGeometry {
    runs: Vec<(u32, u32)>,
    rules: Vec<(u32, char)>,
    /// Columns holding a glyph that continues a vertical stroke without being a plain rule — the
    /// junctions. A superset question from [`Self::rules`] and asked separately, because one
    /// nominates a column and the other only keeps it alive.
    joints: Vec<u32>,
    /// Does this row carry a `$` anywhere? A status line does not, and a row that does is a row
    /// whose content a cut could destroy (owner's ruling 2026-09-17).
    carries_a_dollar: bool,
}

impl RowGeometry {
    /// Runs are disjoint and ordered, so the column that could cover `column` is the last one
    /// starting at or before it.
    fn covered(&self, column: u32) -> bool {
        let index = self.runs.partition_point(|(start, _)| *start <= column);
        index
            .checked_sub(1)
            .and_then(|index| self.runs.get(index))
            .is_some_and(|(_, end)| column < *end)
    }

    /// Is this column clear of the row's text — empty itself, and not a column the row's text
    /// crosses?
    ///
    /// Crossing is asked of the two neighbouring cells because that is the whole question: a rule
    /// column with writing on both sides of it on some row is a column that row's producer wrote
    /// straight through, which is what a column of `column -t` output looks like and what a frame
    /// never does.
    fn clear_at(&self, column: u32) -> bool {
        if self.covered(column) {
            return false;
        }
        let crosses = column
            .checked_sub(1)
            .is_some_and(|left| self.covered(left) && self.covered(column.saturating_add(1)));
        !crosses
    }

    fn draws_rule(&self, column: u32, glyph: char) -> bool {
        self.rules.binary_search(&(column, glyph)).is_ok()
    }

    /// Does the vertical line run through this row at this column, whatever glyph draws it?
    fn carries_the_line(&self, column: u32) -> bool {
        self.rules
            .binary_search_by_key(&column, |(rule, _)| *rule)
            .is_ok()
            || self.joints.binary_search(&column).is_ok()
    }
}

/// Measure one row: its non-blank runs and the columns it draws a vertical rule in.
fn row_geometry(text: &str, boundaries: &[(u32, u32)]) -> RowGeometry {
    let mut geometry = RowGeometry {
        carries_a_dollar: text.contains('$'),
        ..RowGeometry::default()
    };
    for pair in boundaries.windows(2) {
        let (start_byte, start_column) = pair[0];
        let (end_byte, end_column) = pair[1];
        let Some(cluster) = text.get(start_byte as usize..end_byte as usize) else {
            continue;
        };
        if end_column <= start_column {
            continue;
        }
        if !cluster.trim_matches([' ', '\t']).is_empty() {
            match geometry.runs.last_mut() {
                Some((_, run_end)) if *run_end == start_column => *run_end = end_column,
                _ => geometry.runs.push((start_column, end_column)),
            }
        }
        if end_column == start_column.saturating_add(1) {
            match vertical_rule(cluster) {
                Some(glyph) => geometry.rules.push((start_column, glyph)),
                None if continues_vertical(cluster) => geometry.joints.push(start_column),
                None => {}
            }
        }
    }
    geometry.rules.sort_unstable();
    geometry.rules.dedup();
    geometry.joints.sort_unstable();
    geometry.joints.dedup();
    geometry
}

/// **Is this column a frame on every row it cuts?** (owner's ruling 2026-09-16.)
///
/// A share of the rows is not proof. The column has to be a column *of the screen*, which means
/// every row that does not carry the vertical line must be a row the column is clear on: empty at
/// the column and not crossed by that row's text.
///
/// **Carrying the line is not the same as drawing the rule.** The plain rule nominates the column
/// ([`vertical_rule`]); any glyph with a vertical stroke keeps it ([`continues_vertical`]), so the
/// `┼` where a TUI's horizontal separator crosses a pane rule — the middle row of a `tmux` 2×2
/// layout — leaves the vertical border standing. A row of plain `─` does not, and should not: that
/// is a horizontal pane boundary, which this does not model.
///
/// **One row may break it, and it must be a status line** (owner's rulings 2026-09-16 and
/// 2026-09-17). Only the topmost or the bottommost, never a middle row, because a frame with a hole
/// in the middle of it is not a frame and the hole is text the cut would run through. And only a row
/// carrying no `$`: the exempt row is still sliced, so exempting a row with a formula on it is
/// throwing that formula away, and a formula the screen can prove outranks a split the screen only
/// infers. A status line never carries math.
///
/// This is what refuses a screen of `log  │ text` rows with one bare `$$x^2$$` row among them: that
/// row's writing crosses the column, so the column is not a rule and the screen is one body of text
/// with a glyph in it — which is exactly what it is. The same screen with a `plain` status line at
/// the bottom instead does split; with the `$$x^2$$` row at the bottom it does not, because the
/// formula wins.
fn frame_is_unbroken(geometry: &[RowGeometry], column: u32, glyph: char) -> bool {
    let last = geometry.len().saturating_sub(1);
    let mut exempted = false;
    for (index, row) in geometry.iter().enumerate() {
        if row.draws_rule(column, glyph) || row.carries_the_line(column) || row.clear_at(column) {
            continue;
        }
        if exempted || (index != 0 && index != last) || row.carries_a_dollar {
            return false;
        }
        exempted = true;
    }
    true
}

/// **Which rows the frame runs through** — the rows carrying the vertical line in every one of the
/// screen's border columns.
///
/// A row that does not is a row no pane owns: the status line the exemption spent, or a row of a
/// screen with no frame at all. [`fenced_lines`](crate::fenced_lines) asks it of a fence's opening
/// row, because a fence opened on a row no pane owns is the screen's fence and binds every region,
/// while one opened inside a pane belongs to that pane.
fn framed_rows_of(geometry: &[RowGeometry], borders: &[u32]) -> Vec<bool> {
    geometry
        .iter()
        .map(|row| borders.iter().all(|column| row.carries_the_line(*column)))
        .collect()
}

/// The border columns of a screen whose rows are already paired with their capture geometry.
fn border_columns_of_rows(rows: &[(&str, &[(u32, u32)])]) -> Vec<u32> {
    let geometry = rows
        .iter()
        .map(|(text, boundaries)| row_geometry(text, boundaries))
        .collect::<Vec<_>>();
    border_columns_of_geometry(&geometry, rows.len())
}

/// The border columns of a screen whose rows are already measured.
fn border_columns_of_geometry(geometry: &[RowGeometry], rows: usize) -> Vec<u32> {
    if rows < BORDER_MINIMUM_ROWS {
        return Vec::new();
    }
    let mut tally = BTreeMap::<(u32, char), usize>::new();
    for row in geometry {
        for entry in &row.rules {
            *tally.entry(*entry).or_default() += 1;
        }
    }
    // Two tests, and a column must pass both. The share says the column is mostly a rule — which
    // is what stops a table's few rows from nominating one — and `frame_is_unbroken` says it is a
    // rule everywhere else too. At this share two different glyphs cannot both carry one column, so
    // the map, already ordered by column, yields each border once.
    let mut borders = tally
        .into_iter()
        .filter(|((column, glyph), count)| {
            *count >= BORDER_MINIMUM_ROWS
                && count.saturating_mul(1000) >= rows.saturating_mul(BORDER_ROW_SHARE_PERMILLE)
                && frame_is_unbroken(geometry, *column, *glyph)
        })
        .map(|((column, _), _)| column)
        .collect::<Vec<_>>();
    borders.dedup();
    borders
}

/// The regions a screen's border columns cut it into, left to right.
///
/// A border column belongs to no region: it is the rule itself. A region of no columns — two
/// adjacent rules, or a rule in column zero — is not one and is dropped.
fn regions_from_borders(borders: &[u32]) -> Vec<ScreenRegion> {
    if borders.is_empty() {
        return vec![ScreenRegion::WHOLE];
    }
    let mut regions = Vec::with_capacity(borders.len().saturating_add(1));
    let mut column_start = 0;
    for border in borders {
        if *border > column_start {
            regions.push(ScreenRegion {
                column_start,
                column_end: Some(*border),
            });
        }
        column_start = border.saturating_add(1);
    }
    regions.push(ScreenRegion {
        column_start,
        column_end: None,
    });
    regions
}

/// Cell columns at which (nearly) every row of this screen draws the same vertical rule.
///
/// Empty for a screen with no frame, which is nearly every screen. Rows are the complete live
/// screen, never a window of it: the share is measured against all of them, and a subset would let
/// a table's few rows pass as a frame.
#[must_use]
pub fn find_border_columns<'a>(rows: impl IntoIterator<Item = &'a str>) -> Vec<u32> {
    let measured = rows
        .into_iter()
        .map(|text| (text, text_cell_boundaries(text)))
        .collect::<Vec<_>>();
    let rows = measured
        .iter()
        .map(|(text, boundaries)| (*text, boundaries.as_slice()))
        .collect::<Vec<_>>();
    border_columns_of_rows(&rows)
}

/// The regions this screen's frame cuts it into, left to right; a single [`ScreenRegion::WHOLE`]
/// when it has no frame.
#[must_use]
pub fn screen_regions<'a>(rows: impl IntoIterator<Item = &'a str>) -> Vec<ScreenRegion> {
    split_screen(rows).regions
}

/// The regions this screen's frame cuts it into, and which of its rows that frame runs through.
#[must_use]
pub fn split_screen<'a>(rows: impl IntoIterator<Item = &'a str>) -> ScreenSplit {
    let measured = rows
        .into_iter()
        .map(|text| (text, text_cell_boundaries(text)))
        .collect::<Vec<_>>();
    let geometry = measured
        .iter()
        .map(|(text, boundaries)| row_geometry(text, boundaries))
        .collect::<Vec<_>>();
    let borders = border_columns_of_geometry(&geometry, measured.len());
    ScreenSplit {
        framed: if borders.is_empty() {
            vec![false; measured.len()]
        } else {
            framed_rows_of(&geometry, &borders)
        },
        regions: regions_from_borders(&borders),
    }
}

/// Byte range of `text` covering exactly the cells inside `region`.
///
/// A cluster is inside only when all of its cells are: one straddling a boundary — which a border
/// column makes impossible, since the rule itself owns that cell — belongs to neither side rather
/// than to the side it leans toward.
fn region_bytes(text: &str, boundaries: &[(u32, u32)], region: ScreenRegion) -> (usize, usize) {
    let start = boundaries
        .iter()
        .find(|(_, column)| *column >= region.column_start)
        .map_or(text.len(), |(byte, _)| *byte as usize)
        .min(text.len());
    let end = match region.column_end {
        None => text.len(),
        Some(limit) => boundaries
            .iter()
            .rev()
            .find(|(_, column)| *column <= limit)
            .map_or(0, |(byte, _)| *byte as usize),
    };
    (start, end.clamp(start, text.len()))
}

/// The part of one screen row that lies inside `region`, with the padding right of it dropped.
///
/// The whole screen is returned untouched for [`ScreenRegion::WHOLE`], so a screen with no frame
/// is scanned over exactly the bytes it was scanned over before.
#[must_use]
pub fn region_text(text: &str, region: ScreenRegion) -> &str {
    if region.is_whole() {
        return text;
    }
    let (start, end) = region_bytes(text, &text_cell_boundaries(text), region);
    text[start..end].trim_end_matches([' ', '\t'])
}

/// A screen its frame cuts into regions, and which of its rows the frame runs through.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenSplit {
    /// The regions, left to right. One [`ScreenRegion::WHOLE`] when the screen has no frame.
    pub regions: Vec<ScreenRegion>,
    /// Per row, whether the frame runs through it — see [`framed_rows_of`]. All `false` when the
    /// screen has no frame, because then there is no frame for a row to be part of.
    pub framed: Vec<bool>,
}

/// A live screen its frame cuts into regions, and the screen's own rows beside them.
#[derive(Clone, Debug)]
pub struct LiveScreenSplit {
    /// The screen's grid rows, unsliced and in scan order. The regions are cut from exactly these,
    /// so anything the screen as a whole proves about a row — that it is inside a code fence, above
    /// all — is proved on these and lines up with the regions row for row.
    pub screen: Arc<[LiveDetectionInput]>,
    /// The regions, left to right.
    pub regions: Vec<LiveScreenRegion>,
    /// Per grid row, whether the frame runs through it.
    pub framed: Vec<bool>,
}

/// One region of a live screen, with the screen's rows cut down to that region's columns.
#[derive(Clone, Debug)]
pub struct LiveScreenRegion {
    pub region: ScreenRegion,
    /// The region's own rows, in scan order. Each row's `text` is the region's columns alone and
    /// its `cell_boundaries` rebase the bytes while keeping the **screen's** cell columns, so
    /// everything proved from these rows is already in screen coordinates.
    pub inputs: Arc<[LiveDetectionInput]>,
}

/// One capture, and what cutting it came to.
type RememberedSplit = (Arc<[LiveDetectionInput]>, Option<LiveScreenSplit>);

thread_local! {
    /// The last capture this thread cut, and the answer it got.
    ///
    /// A screen's regions are a pure function of its rows, and one stable snapshot is asked for
    /// them several times before the frame it belongs to is finished: once to arm the candidate
    /// rows, once to resolve them, and once more for every task whose dependencies are re-checked.
    /// The snapshot is shared as one `Arc`, so its identity is the capture's identity — stronger
    /// than the grid generation, which a repaint can leave unchanged — and holding the `Arc` here
    /// rather than its address alone is what makes that identity sound: the allocation cannot be
    /// freed and reused underneath while the answer to it is still remembered.
    ///
    /// One entry. A second screen displaces the first, which is exactly the lifetime of a frame.
    static REMEMBERED_SPLIT: RefCell<Option<RememberedSplit>> = const { RefCell::new(None) };
}

/// [`live_screen_regions`] for a capture held as an `Arc`, answered from the last one when it is
/// the same capture.
#[must_use]
pub fn live_screen_regions_of(inputs: &Arc<[LiveDetectionInput]>) -> Option<LiveScreenSplit> {
    if let Some(remembered) = REMEMBERED_SPLIT.with_borrow(|slot| {
        slot.as_ref()
            .filter(|(capture, _)| Arc::ptr_eq(capture, inputs))
            .map(|(_, split)| split.clone())
    }) {
        return remembered;
    }
    let split = live_screen_regions(inputs);
    REMEMBERED_SPLIT.with_borrow_mut(|slot| {
        *slot = Some((Arc::clone(inputs), split.clone()));
    });
    split
}

/// Cut a live-detection window at the screen's border columns.
///
/// `None` — the overwhelmingly common answer — means the screen has no frame and the caller scans
/// `inputs` exactly as it always has.
///
/// Only the grid rows are measured and only they are carried into the regions. A frozen history
/// prefix is the host terminal's scrollback, and a rule running the full height of the screen is
/// proof that the screen is a frame: no line above it continues into a pane. Each region is
/// therefore a self-contained window, which is also why its scan begins from a neutral context.
#[must_use]
pub fn live_screen_regions(inputs: &[LiveDetectionInput]) -> Option<LiveScreenSplit> {
    let grid = inputs
        .iter()
        .filter(|input| matches!(input.source, LiveDetectionSource::Grid { .. }))
        .collect::<Vec<_>>();
    let geometry = grid
        .iter()
        .map(|input| row_geometry(&input.text, &input.cell_boundaries))
        .collect::<Vec<_>>();
    let borders = border_columns_of_geometry(&geometry, grid.len());
    if borders.is_empty() {
        return None;
    }
    Some(LiveScreenSplit {
        framed: framed_rows_of(&geometry, &borders),
        screen: grid
            .iter()
            .map(|input| region_input(input, ScreenRegion::WHOLE))
            .collect(),
        regions: regions_from_borders(&borders)
            .into_iter()
            .map(|region| LiveScreenRegion {
                region,
                inputs: grid
                    .iter()
                    .map(|input| region_input(input, region))
                    .collect(),
            })
            .collect(),
    })
}

/// Byte range of one screen row that its region reads.
///
/// The region's own right-edge padding is padding, by exactly the rule the grid row itself is read
/// by: a row that ends its logical line drops it, and a row the terminal soft-wrapped keeps it,
/// because the wrap may have fallen on a space that is a character of the line.
fn region_slice_bounds(input: &LiveDetectionInput, region: ScreenRegion) -> (usize, usize) {
    if region.is_whole() {
        return (0, input.text.len());
    }
    let (start, end) = region_bytes(&input.text, &input.cell_boundaries, region);
    let slice = &input.text[start..end];
    let kept = if input.continues {
        slice.len()
    } else {
        slice.trim_end_matches([' ', '\t']).len()
    };
    (start, start.saturating_add(kept))
}

/// **One screen row as its region reads it**, borrowed from the row itself.
///
/// This is the string a block proved in `region` has its byte offsets into, so every reader that
/// pairs an occurrence's bytes with the row they were measured on — the cells one run occupies, the
/// logical line the renderer folds it at — must take the row through here first. The whole row
/// comes back untouched for [`ScreenRegion::WHOLE`].
#[must_use]
pub fn live_region_text(input: &LiveDetectionInput, region: ScreenRegion) -> &str {
    let (start, limit) = region_slice_bounds(input, region);
    &input.text[start..limit]
}

/// **The screen cell column a region-local byte offset of this row stands at.**
///
/// Read from the row's own captured boundaries — the very cells detection anchored the occurrence
/// on — and never inferred by adding [`ScreenRegion::column_start`] to a width measured over the
/// slice. The two are not the same number: slicing drops a cluster that straddles the region's
/// first column, so the region's text can begin a column later than the region does, and the
/// inferred answer is then short by the width of the cluster that was dropped. One source of truth,
/// and it is the grid's.
///
/// `None` when the offset is not a boundary of this row's region — which is to say, not a place a
/// character of it begins or ends.
#[must_use]
pub fn live_region_cell_column(
    input: &LiveDetectionInput,
    region: ScreenRegion,
    local_byte: usize,
) -> Option<u32> {
    let (start, limit) = region_slice_bounds(input, region);
    let byte = start.checked_add(local_byte)?;
    if byte > limit {
        return None;
    }
    let byte = u32::try_from(byte).ok()?;
    input
        .cell_boundaries
        .iter()
        .find_map(|(boundary, column)| (*boundary == byte).then_some(*column))
}

/// One screen row as its region reads it, owned: bytes rebased onto the slice, the screen's own
/// cell columns kept.
fn region_input(input: &LiveDetectionInput, region: ScreenRegion) -> LiveDetectionInput {
    let (start, limit) = region_slice_bounds(input, region);
    let offset = u32::try_from(start).unwrap_or(u32::MAX);
    LiveDetectionInput {
        source: input.source,
        text: input.text[start..limit].to_owned(),
        continues: input.continues,
        // The width the region's rows were cut at, which is the width this row's own producer had
        // to work in. A row that carried no capture geometry still carries none.
        captured_columns: if input.captured_columns == 0 {
            0
        } else {
            region
                .column_end
                .unwrap_or(input.captured_columns)
                .saturating_sub(region.column_start)
        },
        cell_boundaries: input
            .cell_boundaries
            .iter()
            .filter(|(byte, _)| (start..=limit).contains(&(*byte as usize)))
            .map(|(byte, column)| (byte.saturating_sub(offset), *column))
            .collect(),
        site: input.site,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InlineMathSite;

    fn rows(lines: &[&str]) -> Vec<u32> {
        find_border_columns(lines.iter().copied())
    }

    #[test]
    fn a_rule_on_every_row_is_a_border() {
        let screen = ["a│x", "b│y", "c│z", "d│w"];
        assert_eq!(rows(&screen), vec![1]);
    }

    #[test]
    fn a_screen_shorter_than_three_rows_has_no_border() {
        assert_eq!(rows(&["a│x", "b│y"]), Vec::<u32>::new());
    }

    #[test]
    fn a_rule_on_a_few_rows_is_not_a_border() {
        let mut screen = vec!["plain text"; 35];
        screen.extend(["a│b"; 5]);
        assert_eq!(rows(&screen), Vec::<u32>::new());
    }

    #[test]
    fn nine_tenths_is_enough_and_less_is_not() {
        // The rows that are not the rule are clear of the column, so only the share is in question.
        let mut nine = vec!["a│b"; 36];
        nine.extend([""; 4]);
        assert_eq!(rows(&nine), vec![1]);
        let mut eight = vec!["a│b"; 35];
        eight.extend([""; 5]);
        assert_eq!(rows(&eight), Vec::<u32>::new());
    }

    /// Thirty-six rows draw the rule in column 5 and three more are clear of it, which is nine
    /// tenths of the screen. The row between them writes straight through column 5, so the column
    /// is not a frame: this screen is one body of text with a glyph in it, and cutting it would
    /// take the formula apart.
    #[test]
    fn a_row_whose_text_crosses_the_column_vetoes_it() {
        let mut screen = vec!["log  │ text".to_owned(); 36];
        screen.push("$$x^2$$".to_owned());
        screen.extend(std::iter::repeat_n("plain".to_owned(), 3));
        assert_eq!(
            find_border_columns(screen.iter().map(String::as_str)),
            Vec::<u32>::new()
        );
    }

    /// A pane may have a status line under it, so one broken row at an edge — one, and only at an
    /// edge — is spent rather than fatal.
    #[test]
    fn one_crossing_row_at_an_edge_is_a_status_line() {
        let mut screen = vec!["log  │ text".to_owned(); 36];
        screen.extend(std::iter::repeat_n("plain".to_owned(), 3));
        screen.push("[0] 0:bash* host 12:00".to_owned());
        assert_eq!(
            find_border_columns(screen.iter().map(String::as_str)),
            vec![5]
        );
        // At the top it is the same status line and the same answer.
        let moved = screen.pop().expect("the status line");
        screen.insert(0, moved);
        assert_eq!(
            find_border_columns(screen.iter().map(String::as_str)),
            vec![5]
        );
        // Two broken rows are not a status line, wherever they stand.
        screen.push("[0] 0:bash* host 12:01".to_owned());
        assert_eq!(
            find_border_columns(screen.iter().map(String::as_str)),
            Vec::<u32>::new()
        );
    }

    /// **The exempt row is still sliced, so a formula on it would be thrown away** (owner's ruling
    /// 2026-09-17). A status line never carries math; a row that does is a row whose content the cut
    /// would destroy, and a formula the screen can prove outranks a split the screen only infers.
    #[test]
    fn an_edge_row_carrying_a_formula_spends_no_exemption() {
        let rows = |last: &str| {
            let mut screen = vec!["log  │ text".to_owned(); 36];
            screen.extend(std::iter::repeat_n("plain".to_owned(), 3));
            screen.push(last.to_owned());
            find_border_columns(screen.iter().map(String::as_str))
        };
        assert_eq!(rows("bash 12:00"), vec![5], "a status line is spent");
        assert_eq!(
            rows("$$x^2$$"),
            Vec::<u32>::new(),
            "a formula at the edge keeps the screen whole"
        );
        assert_eq!(
            rows("total $5 and $10"),
            Vec::<u32>::new(),
            "and so does any row carrying a dollar at all"
        );
    }

    /// **A junction continues the rule** (owner's ruling 2026-09-17): the row a TUI draws its
    /// horizontal separator on keeps the vertical border alive, so a `tmux` 2x2 layout still splits
    /// down the middle. A row of plain `─` does not — that is a horizontal pane boundary, which this
    /// does not model.
    #[test]
    fn a_junction_keeps_the_vertical_border_and_a_plain_horizontal_breaks_it() {
        let mut screen = vec!["left      │$x^2$".to_owned(); 40];
        for joint in ['┼', '├', '┤', '┬', '┴', '╬'] {
            screen[19] = format!("{}{joint}{}", "─".repeat(10), "─".repeat(10));
            assert_eq!(
                find_border_columns(screen.iter().map(String::as_str)),
                vec![10],
                "{joint} carries a vertical stroke through its row"
            );
        }
        // A row of plain horizontal rule carries no vertical stroke and is not clear either.
        screen[19] = "─".repeat(21);
        assert_eq!(
            find_border_columns(screen.iter().map(String::as_str)),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn only_the_horizontals_and_diagonals_carry_no_vertical_stroke() {
        for glyph in [
            '│', '┃', '├', '┤', '┼', '┬', '┴', '┌', '┘', '║', '╬', '╭', '╵', '╷',
        ] {
            assert!(
                continues_vertical(&glyph.to_string()),
                "{glyph} is vertical"
            );
        }
        for glyph in ['─', '━', '┄', '┈', '╌', '═', '╱', '╲', '╳', '╴', '╶', '╼']
        {
            assert!(!continues_vertical(&glyph.to_string()), "{glyph} is not");
        }
        assert!(
            !continues_vertical("x"),
            "an ordinary glyph is not the frame"
        );
    }

    #[test]
    fn ascii_pipes_never_cut_a_screen() {
        let screen = vec!["| a | b |"; 40];
        assert_eq!(rows(&screen), Vec::<u32>::new());
    }

    #[test]
    fn junctions_are_not_rules() {
        assert!(vertical_rule("├").is_none());
        assert!(vertical_rule("┼").is_none());
        assert!(vertical_rule("┬").is_none());
        assert_eq!(vertical_rule("║"), Some('║'));
        assert_eq!(vertical_rule("┃"), Some('┃'));
    }

    #[test]
    fn wide_glyphs_are_counted_in_cells() {
        // `能` draws two cells, so the rule behind it stands in column 2, not column 1.
        let screen = vec!["能│x"; 4];
        assert_eq!(rows(&screen), vec![2]);
        let region = ScreenRegion {
            column_start: 3,
            column_end: None,
        };
        assert_eq!(region_text("能│x", region), "x");
    }

    #[test]
    fn a_region_is_the_text_between_the_rules() {
        let regions = screen_regions(std::iter::repeat_n("left│right", 4));
        assert_eq!(
            regions,
            vec![
                ScreenRegion {
                    column_start: 0,
                    column_end: Some(4)
                },
                ScreenRegion {
                    column_start: 5,
                    column_end: None
                },
            ]
        );
        assert_eq!(region_text("left│right", regions[0]), "left");
        assert_eq!(region_text("left│right", regions[1]), "right");
    }

    #[test]
    fn a_rule_in_column_zero_leaves_one_region() {
        let regions = screen_regions(std::iter::repeat_n("│body", 4));
        assert_eq!(
            regions,
            vec![ScreenRegion {
                column_start: 1,
                column_end: None
            }]
        );
    }

    #[test]
    fn a_region_keeps_its_own_indentation() {
        let regions = screen_regions(std::iter::repeat_n("side│    code", 4));
        assert_eq!(region_text("side│    code", regions[1]), "    code");
    }

    /// **The region's origin is the grid's answer, not arithmetic on the slice** (owner's ruling
    /// 2026-09-17).
    ///
    /// `能` is drawn in columns 1 and 2, so a region beginning at column 2 begins inside it and the
    /// cluster belongs to neither side. The region's text therefore starts at column **3**, one
    /// further right than the region does. Adding `column_start` to a width measured over that text
    /// answers 2 and claims a cell the pane across the rule owns; the captured boundaries answer 3,
    /// which is where the character actually is.
    #[test]
    fn a_straddling_wide_cluster_moves_the_regions_first_column() {
        let input = LiveDetectionInput {
            source: LiveDetectionSource::Grid {
                row: 0,
                revision: 1,
            },
            text: "a能 $x^2$".to_owned(),
            continues: false,
            captured_columns: 20,
            // As the terminal captures them: `a` in column 0, `能` across 1 and 2, then the rest.
            cell_boundaries: vec![
                (0, 0),
                (1, 1),
                (4, 3),
                (5, 4),
                (6, 5),
                (7, 6),
                (8, 7),
                (9, 8),
                (10, 9),
            ],
            site: InlineMathSite::AltScreenContent,
        };
        let region = ScreenRegion {
            column_start: 2,
            column_end: None,
        };
        assert_eq!(live_region_text(&input, region), " $x^2$");
        assert_eq!(
            live_region_cell_column(&input, region, 0),
            Some(3),
            "the region's text begins where the grid says it does"
        );
        assert_eq!(
            live_region_cell_column(&input, region, 1),
            Some(4),
            "and the formula's own delimiter with it"
        );
        assert_eq!(
            live_region_cell_column(&input, region, 6),
            Some(9),
            "through to its last cell"
        );
        // The whole screen is its own region and answers in the same coordinates.
        assert_eq!(
            live_region_cell_column(&input, ScreenRegion::WHOLE, 1),
            Some(1)
        );
    }

    #[test]
    fn a_whole_screen_region_returns_its_row_untouched() {
        assert_eq!(
            region_text("trailing   ", ScreenRegion::WHOLE),
            "trailing   "
        );
    }
}
