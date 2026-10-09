//! **`cmdrail`, as the application drives it.** Tests whose first assertion is about
//! `cmdrail`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    RAIL_FAILED_THEN_PROMPT, item_body, leaf_saying, rail_test_body, squeezed, squeezed_body,
};
use bt_source::ItemQuery;

/// RED (confirmation review of `6049179a`, P2) — **the in-pane surfaces yield
/// to every band painted above them, and that list is the paint order.**
///
/// The wheel's in-pane station stood above the palette, so a notch on the
/// palette's list where it overlapped a pill was swallowed by the pill. The
/// rule is not a station moved by hand: the router's in-pane step yields to
/// `OVER_IN_PANE_TOP_FIRST`, and every reader that asks the router — the
/// wheel, the press door, the tip — inherits it. This test reads
/// `OverlayStack::flattened` and requires that list to be exactly the bands it
/// paints above `in_pane`, top first, less the ones that take no pointer — so a
/// band added to the paint above the in-pane surfaces fails here until it is
/// classified, and a reorder of the paint fails here until the list follows.
///
/// Red gate: swap two entries of the list, or drop the yield from the router,
/// and the assertion naming it fails.
#[test]
fn the_in_pane_surfaces_yield_to_every_band_painted_above_them() {
    let paint = squeezed(item_body(&ItemQuery::method("OverlayStack", "flattened")));
    let array = &paint[paint.find("[preview_bars,").expect("the paint array")..];
    let array = &array[1..array.find(']').expect("its end")];
    let bands: Vec<&str> = array.split(',').filter(|band| !band.is_empty()).collect();
    let in_pane = bands
        .iter()
        .position(|band| *band == "in_pane")
        .expect("the in-pane surfaces are painted as one band");
    let above: Vec<&str> = bands[in_pane + 1..]
        .iter()
        .rev()
        .copied()
        .filter(|band| !BANDS_OVER_IN_PANE_THAT_TAKE_NO_POINTER.contains(band))
        .collect();
    let listed: Vec<&str> = OVER_IN_PANE_TOP_FIRST
        .iter()
        .map(|family| family.band())
        .collect();
    assert_eq!(
        listed, above,
        "OVER_IN_PANE_TOP_FIRST is the paint order above the in-pane surfaces, top first"
    );
    let router = squeezed_body("Runtime", "pointer_target_at");
    let step = router
        .find("forsurfaceinIN_PANE_SURFACES_TOP_FIRST")
        .expect("the in-pane step");
    assert!(
        router[step..].contains(
            "ifself.painted_over_in_pane_at(position,&OVER_IN_PANE_TOP_FIRST){break;}returnclaim;"
        ),
        "the router's in-pane claim yields to every band painted over it"
    );
    assert!(
        squeezed_body("Runtime", "notice_at")
            .contains("OVER_IN_PANE_TOP_FIRST.split(|family|*family==OverInPane::Float)"),
        "and a window's own pill to every band painted over the window"
    );
    for reader in [
        "mouse_wheel",
        "press_in_pane_surface",
        "owned_tooltip_anchor_at",
    ] {
        assert!(
            squeezed_body("Runtime", reader).contains("self.in_pane_surface_at(position)"),
            "`{reader}` takes the in-pane claim from the router, so it yields in the paint order"
        );
    }
    let surface = squeezed_body("Runtime", "in_pane_surface_at");
    assert!(
        surface.contains("self.pointer_target_at(position)?"),
        "and the claim is the router's"
    );
}

/// The scales a real display runs at, and three pane widths in logical pixels: a
/// narrow pane (the report's, squeezed beside a graph pane), a medium one and a
/// wide one. Odd numbers, so no width lands on a cell boundary by luck.
const RAIL_SCALES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

const RAIL_LOGICAL_WIDTHS: [u32; 3] = [331, 797, 1913];

/// RED (ticket 32) — **with a rail, the last text column ends left of the rail's
/// resting band, at every width and scale.**
///
/// The owner's ruling of 2026-09-23 is that decoration never covers text. The
/// grid used to reserve only its symmetric `padding_px`, while the rail stands
/// inboard of the eight-pixel scroll lane — so a line that filled the pane ran
/// under the ticks, and a failed command's rose tick sat on its last letters
/// (ticket 15's `12-crop-rail-over-text.png`). The rail here is the real
/// [`cmdrail::lay_out`] of a real ledger, and the grid is the one function every
/// seat-to-grid site asks. The reserve is the resting band's width
/// ([`cmdrail::Rail::bounds`]), not the hot crest's (owner, 2026-09-23). It is
/// also no wider than it has to be: one more column would cross the band.
///
/// MUTATION: return `metrics.grid_for_pixels(body.width, body.height)`
/// unconditionally from `cmdrail::terminal_grid_for` — red at the narrow width
/// (and at every other: the resting tick always overlapped the old last column).
#[test]
fn a_rail_never_covers_the_last_text_column() {
    let leaf = leaf_saying(RAIL_FAILED_THEN_PROMPT);
    let stack = cmdrail::commands(leaf.session.command_marks());
    assert!(
        stack
            .entries
            .iter()
            .any(|entry| entry.signal == cmdrail::Signal::Fail),
        "the fixture must hold the failed command whose rose tick the report saw"
    );
    let mut fonts = bt_render::preview_measure_font_system();
    for scale in RAIL_SCALES {
        let metrics = bt_render::CellMetrics::measure(&mut fonts, scale).unwrap();
        for logical in RAIL_LOGICAL_WIDTHS {
            let body = rail_test_body(logical, scale);
            let edges = [
                body.x as f32,
                body.y as f32,
                (body.x + body.width) as f32,
                (body.y + body.height) as f32,
            ];
            let rail = cmdrail::lay_out(edges, &stack, scale as f32, None);
            assert!(!rail.ticks.is_empty(), "a ledger with marks draws a rail");
            let grid = cmdrail::terminal_grid_for(&metrics, body, true);
            let columns = f32::from(grid.columns.get());
            let text_right = edges[0] + metrics.padding_px + columns * metrics.cell_width_px;
            assert!(
                text_right <= rail.bounds[0],
                "{logical} logical px at {scale}x: the last column ends at {text_right}, \
                 the rail's resting band starts at {}",
                rail.bounds[0]
            );
            for tick in &rail.ticks {
                assert!(
                    tick.rect[0] >= text_right,
                    "{logical} logical px at {scale}x: a tick starts at {} inside the text",
                    tick.rect[0]
                );
            }
            assert!(
                text_right + metrics.cell_width_px > rail.bounds[0],
                "{logical} logical px at {scale}x: the reserve took a column the band \
                 does not stand on"
            );
        }
    }
}

/// PIN (ticket 32) — **a pane with no rail keeps exactly the grid it always had.**
///
/// `cmd.exe` without its prompt marks, a WSL shell without the init file and a
/// program run bare never send a mark, and the ruling changes nothing for them:
/// the same columns and rows [`bt_render::CellMetrics::grid_for_pixels`] gives the
/// rectangle, at every width and scale the rail test uses.
///
/// MUTATION: reserve the band whatever `has_rail` says — red at every width.
#[test]
fn without_a_rail_the_grid_is_unchanged() {
    let mut fonts = bt_render::preview_measure_font_system();
    for scale in RAIL_SCALES {
        let metrics = bt_render::CellMetrics::measure(&mut fonts, scale).unwrap();
        for logical in RAIL_LOGICAL_WIDTHS {
            let body = rail_test_body(logical, scale);
            assert_eq!(
                cmdrail::terminal_grid_for(&metrics, body, false),
                metrics.grid_for_pixels(body.width, body.height),
                "{logical} logical px at {scale}x"
            );
        }
    }
}
