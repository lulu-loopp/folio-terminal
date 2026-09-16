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
//! *The frame is unbroken* (owner's ruling 2026-09-16). A share is not proof, because the rows it
//! leaves over are rows the cut still runs through. So on every row where the glyph is **not** the
//! rule, the column must be **clear**: the cell is blank, and the row's text does not cross it —
//! no writing immediately left of it *and* immediately right of it. Exactly one row may break
//! that, and only the topmost or the bottommost, never a middle one: a pane may have a status line
//! under it, and nothing else looks like this.
//!
//! The ruling's own counter-example is what it is for. Thirty-six rows of `log  │ text`, one row of
//! `log  │ $$x^2$$`, three plain rows: nine tenths of the screen draws the rule in column five, and
//! the row between them writes straight through it. That screen is one body of text with a glyph in
//! it — pipe-aligned output — and cutting it would take the formula apart and lose it, which is a
//! formula that used to typeset. The same rows with the crossing one *last* are a pane with a
//! status line under it, and do split. A junction (`├`, `┼`, …) where a TUI's horizontal separator
//! meets the rule is neither the rule glyph nor blank, so it spends the one exemption or vetoes the
//! column: a box whose interior rule is interrupted has not shown that the text on either side of
//! it is two independent streams.
//!
//! **A fence the whole screen proves suppresses every region** (owner's ruling 2026-09-16). A
//! region is a column of the screen and a code fence is not: the ``` that opens one stands in
//! whichever region it was printed in, and every other region would begin from a neutral state and
//! read the code between the fences as ordinary text. Thirty-eight rows of `log │ $x^2$` between
//! two fences are thirty-eight formulas to a region that never saw the fence, and none at all to
//! the screen — and the screen is right. So the question is asked once, of the unsliced rows
//! (`bt_detect`'s `fenced_lines`), and no region may prove anything on a row its answer covers.
//!
//! **Why ASCII `|` is not a rule.** It was considered and refused. A screen whose rows all carry a
//! `|` in one column is far more often a table — `mysql`, `column -t`, a markdown table long
//! enough to fill the screen — than a frame, and at this altitude the two are the same picture:
//! there is no local evidence that separates them. A missed split costs what today already costs
//! (the formula stays source); a wrong split re-cuts a table that `bt_detect::table` owns, and
//! hands every gate below a line that was whole. The multiplexers this is for — herdr, tmux,
//! zellij, wezterm — all draw their rule from the U+2500 block.

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
/// Only the members of U+2500–U+257F whose whole form is the vertical stroke are here. A junction
/// (`├`, `┼`, `╪`, `┬`, …) is where a *horizontal* rule crosses or meets a vertical one, and it is
/// deliberately absent: the handful of rows on which a TUI's horizontal separator lands on the
/// pane rule are already absorbed by the tenth of the screen
/// [`BORDER_ROW_SHARE_PERMILLE`] leaves free, while counting `┬` — the glyph at the *top* of a
/// table's inner rule — would invite exactly the table this must never cut.
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
}

impl RowGeometry {
    fn covered(&self, column: u32) -> bool {
        self.runs
            .iter()
            .any(|(start, end)| (*start..*end).contains(&column))
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
}

/// Measure one row: its non-blank runs and the columns it draws a vertical rule in.
fn row_geometry(text: &str, boundaries: &[(u32, u32)]) -> RowGeometry {
    let mut geometry = RowGeometry::default();
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
        if end_column == start_column.saturating_add(1)
            && let Some(glyph) = vertical_rule(cluster)
        {
            geometry.rules.push((start_column, glyph));
        }
    }
    geometry.rules.sort_unstable();
    geometry.rules.dedup();
    geometry
}

/// **Is this column a frame on every row it cuts?** (owner's ruling 2026-09-16.)
///
/// A share of the rows is not proof. The column has to be a column *of the screen*, which means
/// every row where the rule is not drawn must be a row the column is clear on: empty at the column
/// and not crossed by that row's text. One row may break it — a status line — and only the topmost
/// or the bottommost, never a middle row, because a frame with a hole in the middle of it is not a
/// frame and the hole is text the cut would run through.
///
/// This is what refuses a screen of `log  │ text` rows with one `log  │ $$x^2$$` row among them:
/// that row's writing crosses the column, so the column is not a rule and the screen is one body of
/// text with a glyph in it — which is exactly what it is. The same screen with that row at the
/// bottom is a pane with a status line under it, and does split.
///
/// A junction (`├`, `┼`, …) where a TUI's horizontal separator meets the rule is not the rule
/// glyph and is not blank, so it spends the one exemption or vetoes the column. That is the
/// ruling's deliberate cost: a box whose interior rule is interrupted has not shown that the text
/// on either side of it is two independent streams.
fn frame_is_unbroken(geometry: &[RowGeometry], column: u32, glyph: char) -> bool {
    let last = geometry.len().saturating_sub(1);
    let mut exempted = false;
    for (index, row) in geometry.iter().enumerate() {
        if row.draws_rule(column, glyph) || row.clear_at(column) {
            continue;
        }
        if exempted || (index != 0 && index != last) {
            return false;
        }
        exempted = true;
    }
    true
}

/// The border columns of a screen whose rows are already paired with their capture geometry.
fn border_columns_of_rows(rows: &[(&str, &[(u32, u32)])]) -> Vec<u32> {
    if rows.len() < BORDER_MINIMUM_ROWS {
        return Vec::new();
    }
    let geometry = rows
        .iter()
        .map(|(text, boundaries)| row_geometry(text, boundaries))
        .collect::<Vec<_>>();
    let mut tally = BTreeMap::<(u32, char), usize>::new();
    for row in &geometry {
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
                && count.saturating_mul(1000)
                    >= rows.len().saturating_mul(BORDER_ROW_SHARE_PERMILLE)
                && frame_is_unbroken(&geometry, *column, *glyph)
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
    regions_from_borders(&find_border_columns(rows))
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

/// A live screen its frame cuts into regions, and the screen's own rows beside them.
#[derive(Clone, Debug)]
pub struct LiveScreenSplit {
    /// The screen's grid rows, unsliced and in scan order. The regions are cut from exactly these,
    /// so anything the screen as a whole proves about a row — that it is inside a code fence, above
    /// all — is proved on these and lines up with the regions row for row.
    pub screen: Arc<[LiveDetectionInput]>,
    /// The regions, left to right.
    pub regions: Vec<LiveScreenRegion>,
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
    let rows = grid
        .iter()
        .map(|input| (input.text.as_str(), input.cell_boundaries.as_slice()))
        .collect::<Vec<_>>();
    let borders = border_columns_of_rows(&rows);
    if borders.is_empty() {
        return None;
    }
    Some(LiveScreenSplit {
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

    /// The same rows with the crossing one last. A pane may have a status line under it, so one
    /// broken row at an edge — one, and only at an edge — is spent rather than fatal.
    #[test]
    fn one_crossing_row_at_the_bottom_is_a_status_line() {
        let mut screen = vec!["log  │ text".to_owned(); 36];
        screen.extend(std::iter::repeat_n("plain".to_owned(), 3));
        screen.push("$$x^2$$".to_owned());
        assert_eq!(
            find_border_columns(screen.iter().map(String::as_str)),
            vec![5]
        );
        // Two broken rows are not a status line, wherever they stand.
        screen.insert(0, "$$x^2$$".to_owned());
        assert_eq!(
            find_border_columns(screen.iter().map(String::as_str)),
            Vec::<u32>::new()
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

    #[test]
    fn a_whole_screen_region_returns_its_row_untouched() {
        assert_eq!(
            region_text("trailing   ", ScreenRegion::WHOLE),
            "trailing   "
        );
    }
}
