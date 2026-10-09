//! **`trace`, as the application drives it.** Tests whose first assertion is about
//! `trace`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// PIN — **the card over a reference stands beside the run the underline is
/// lit on, one row's worth, on the row the hand is on.**
///
/// [`bt_viewport::ViewportFrame::hyperlink_cells`] hands over a segment that
/// may cover the tail of one row and the head of the next, and its own note
/// says a consumer "gets a row's worth at a time and must union or choose".
/// This chooses, and the choice is the pointer's row.
///
/// MUTATIONS that must turn it red:
/// ① drop the `cell / columns != row` filter — a wrapped reference is
///    boxed from column 0 to the pane's last column and the card is placed
///    against a rectangle with a hole in it;
/// ② use `last` instead of `last + 1` for the right edge — the card
///    overlaps the reference's final character;
/// ③ take the row's height from the cell height instead of the frame's own
///    interval — a pane with a formula in it places every card below the
///    block against the wrong row.
#[test]
fn a_card_over_a_reference_stands_beside_the_run_the_underline_is_on() {
    const COLUMNS: u32 = 80;
    let origin = [12.0_f32, 40.0];
    let cell_width = 9.0_f32;

    // A run of six cells on row 3, columns 10..=15.
    let run: Vec<u32> = (10..=15).map(|column| 3 * COLUMNS + column).collect();
    let rect = reference_run_rect(&run, COLUMNS, 3, origin, cell_width, 60.0, 78.0)
        .expect("the run is on the row asked about");
    assert_eq!(
        rect,
        [
            origin[0] + 10.0 * cell_width,
            origin[1] + 60.0,
            origin[0] + 16.0 * cell_width,
            origin[1] + 78.0,
        ],
        "the whole run, its last cell included, and the row's own interval"
    );

    // **A wrapped reference is two boxes and this is the one under the
    // hand.** The same segment, asked about each of its rows in turn.
    let wrapped: Vec<u32> = (76..80)
        .map(|column| 3 * COLUMNS + column)
        .chain((0..4).map(|column| 4 * COLUMNS + column))
        .collect();
    let upper = reference_run_rect(&wrapped, COLUMNS, 3, origin, cell_width, 60.0, 78.0)
        .expect("the tail of row three");
    let lower = reference_run_rect(&wrapped, COLUMNS, 4, origin, cell_width, 78.0, 96.0)
        .expect("the head of row four");
    assert_eq!(
        upper,
        [
            origin[0] + 76.0 * cell_width,
            origin[1] + 60.0,
            origin[0] + 80.0 * cell_width,
            origin[1] + 78.0,
        ]
    );
    assert_eq!(
        lower,
        [
            origin[0],
            origin[1] + 78.0,
            origin[0] + 4.0 * cell_width,
            origin[1] + 96.0,
        ]
    );
    assert!(
        upper[0] > lower[2],
        "and they are not one rectangle: unioning them would box the whole pane"
    );

    // A row the run does not reach has no box, which is how a stale cell
    // index answers nothing rather than answering the wrong thing.
    assert_eq!(
        reference_run_rect(&run, COLUMNS, 9, origin, cell_width, 0.0, 18.0),
        None
    );
    assert_eq!(
        reference_run_rect(&[], COLUMNS, 3, origin, cell_width, 60.0, 78.0),
        None
    );
}
