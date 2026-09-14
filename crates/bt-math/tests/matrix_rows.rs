//! How many rows a `pmatrix` is set in, measured off the raster.
//!
//! The user's screenshot (2026-09-14) carried three matrices in one display
//! block: `\begin{pmatrix} a & b \\ c & d \end{pmatrix}` came out 2x2, and the
//! two column vectors beside it — `\begin{pmatrix} x \\ y \end{pmatrix}` and
//! `\begin{pmatrix} ax + by \\ cx + dy \end{pmatrix}` — came out as single rows,
//! `(x y)` and `(ax + by  cx + dy)`. A one-column matrix had lost its row
//! breaks while the two-column matrix on the line above had kept them.
//!
//! The defect was upstream of this crate, in the terminal's recovery of the row
//! separator Claude Code collapses to a single backslash
//! (`bt_detect::restore_stripped_environment_newlines`): it read the boundary
//! off the ampersands around the slash, and a one-column row has none. Its red
//! test lives there. **This is the other half of the same claim, and it is the
//! half that says the repair is worth making**: handed a real `\\`, this engine
//! must actually set a one-column matrix in two rows. If it does not, no amount
//! of recovery upstream can put the screenshot right, and the next reader should
//! look at MiTeX's `is-matrix` conversion (`\\` becomes `zws ;`) rather than at
//! the terminal.
//!
//! The measurement is the ink, not a digest: the delimiter columns are struck
//! off — a `(` is one tall glyph spanning every row and would weld the bands
//! together — and what is left is counted in horizontal bands. One band is one
//! row of cells.

use std::num::NonZeroU32;

use bt_math::{MathEngine, MathMode, MathRenderKey};

/// The `[start, end)` runs of columns carrying ink — the glyph groups, in
/// reading order. The first and last of them are the matrix's own delimiters.
fn column_groups(rgba: &[u8], width: u32, height: u32) -> Vec<(usize, usize)> {
    let inked_column =
        |x: usize| (0..height as usize).any(|y| rgba[(y * width as usize + x) * 4 + 3] > 16);
    runs((0..width as usize).map(inked_column))
}

/// Horizontal ink bands within `columns` — one band per row of cells.
fn row_bands(rgba: &[u8], width: u32, height: u32, columns: (usize, usize)) -> Vec<(usize, usize)> {
    let inked_row =
        |y: usize| (columns.0..columns.1).any(|x| rgba[(y * width as usize + x) * 4 + 3] > 16);
    runs((0..height as usize).map(inked_row))
}

fn runs(flags: impl Iterator<Item = bool>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut index = 0usize;
    for inked in flags {
        match (inked, start) {
            (true, None) => start = Some(index),
            (false, Some(from)) => {
                out.push((from, index));
                start = None;
            }
            _ => {}
        }
        index += 1;
    }
    if let Some(from) = start {
        out.push((from, index));
    }
    out
}

/// The rows a matrix source is set in, counted between its delimiters.
fn rows_between_the_delimiters(engine: &MathEngine, source: &str) -> usize {
    let key = MathRenderKey {
        dpi_milli: NonZeroU32::new(2000).expect("2000 is not zero"),
        font_milli_pt: NonZeroU32::new(24_000).expect("24000 is not zero"),
        foreground_rgb: [255, 255, 255],
        mode: MathMode::Display,
    };
    let raster = engine
        .render(source, key)
        .unwrap_or_else(|error| panic!("{source} must render: {error:?}"));
    let groups = column_groups(&raster.rgba, raster.width_px, raster.height_px);
    assert!(
        groups.len() >= 3,
        "{source}: a matrix stands between two delimiters with something inside them: {groups:?}"
    );
    // Everything but the opening and closing delimiter.
    let interior = (groups[1].0, groups[groups.len() - 2].1);
    let bands = row_bands(&raster.rgba, raster.width_px, raster.height_px, interior);
    println!(
        "MATRIX-ROWS source={source} width={} height={} groups={} interior={interior:?} bands={bands:?}",
        raster.width_px,
        raster.height_px,
        groups.len()
    );
    bands.len()
}

/// RED GATE (user report 2026-09-14) — a `\\` ends a row whether or not the row
/// has an `&` in it.
///
/// MUTATIONS:
/// ① set the column vector's `\\` as a space — one band, and the first two
///    assertions go red with the very picture the user photographed;
/// ② set the 2x2's `\\` as a space — the third assertion goes red, which is the
///    regression this change must not cause upstream.
#[test]
fn a_one_column_matrix_is_set_in_as_many_rows_as_it_has_separators() {
    let engine = MathEngine::new();

    assert_eq!(
        rows_between_the_delimiters(&engine, r"\begin{pmatrix} x \\ y \end{pmatrix}"),
        2,
        "a column vector stands in two rows"
    );
    assert_eq!(
        rows_between_the_delimiters(&engine, r"\begin{pmatrix} ax + by \\ cx + dy \end{pmatrix}"),
        2,
        "a column vector whose cells are whole expressions stands in two rows"
    );
    assert_eq!(
        rows_between_the_delimiters(&engine, r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
        2,
        "the 2x2 on the line above still stands in two rows"
    );
    // The control: the same cells with no separator at all really do come out as
    // one row, so the count above is measuring the separator and not the source's
    // length.
    assert_eq!(
        rows_between_the_delimiters(&engine, r"\begin{pmatrix} x & y \end{pmatrix}"),
        1,
        "a row vector stands in one row"
    );
}
