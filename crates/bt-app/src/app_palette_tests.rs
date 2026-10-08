//! **The crate root: command palette.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::method_body;

/// What a program inside a pane is told when it asks, on each canvas.
///
/// The `BT_BG` row is the one that earns this test: the override moves the
/// glass without moving the settings, and a window that answered `OSC 11;?`
/// out of the *scheme* would send a program off to dress for a canvas nobody
/// is looking at. The canvas verdict and the background must both come from
/// the painted colour; only the sixteen and the caret come from the scheme.
#[test]
fn the_colours_a_program_is_told_are_the_ones_the_glass_is_wearing() {
    use bt_render::{FOLIO_DARK, FOLIO_LIGHT};

    let light = terminal_palette(FOLIO_LIGHT, [0xff, 0xff, 0xff], [0x37, 0x35, 0x2f]);
    assert_eq!(light.canvas, TerminalCanvas::Light);
    assert_eq!(light.background, [0xff, 0xff, 0xff]);
    assert_eq!(light.foreground, [0x37, 0x35, 0x2f]);
    assert_eq!(light.ansi, FOLIO_LIGHT.ansi);
    assert_eq!(light.cursor, FOLIO_LIGHT.cursor);

    let dark = terminal_palette(FOLIO_DARK, [0x1b, 0x1b, 0x1b], [0xe1, 0xe1, 0xe1]);
    assert_eq!(dark.canvas, TerminalCanvas::Dark);
    assert_eq!(dark.background, [0x1b, 0x1b, 0x1b]);
    assert_ne!(dark.ansi, light.ansi);

    // A `BT_BG` override: the light scheme is in force by luma, and what the
    // program is told is the overridden canvas, not the scheme's own white.
    let overridden = terminal_palette(FOLIO_LIGHT, [0xf0, 0xe8, 0xd8], [0x37, 0x35, 0x2f]);
    assert_eq!(overridden.canvas, TerminalCanvas::Light);
    assert_eq!(overridden.background, [0xf0, 0xe8, 0xd8]);
}

/// RED GATE (found on the glass, 2026-08-28) — **a theme flip asks every
/// markdown page to lay out again.**
///
/// Two things on a page do not recolour for free: a formula's raster came
/// out of the engine already inked, and a `<picture>` names one file for a
/// dark page and another for a light one. Both are in [`PageArtKey`]
/// *precisely* so a theme flip is a different layout question — and until
/// this line nothing asked the question. Measured in the real window: the
/// chrome went light and the page kept the dark hero and the dark
/// screenshot, indefinitely.
///
/// It is a source gate rather than a behavioural one because the subject is
/// a `Runtime` method that needs a window, a GPU and a palette; what is
/// load-bearing is that the *call* is in the one function every theme change
/// goes through, beside the sibling cache-clear that was written for the
/// same failure one surface over.
///
/// MUTATION: delete the `refresh_preview_for_layout()` call from
/// `adopt_new_palette` and this goes red.
#[test]
fn a_theme_flip_asks_every_markdown_page_to_lay_out_again() {
    let body = method_body("Runtime", "adopt_new_palette");
    assert!(
        body.contains("self.refresh_preview_for_layout();"),
        "a page whose pictures and formulas are inked by the theme has to be \
             asked again: {body}",
    );
    // And the rail's own clear is still there beside it, because the two are
    // the same sentence about two surfaces.
    assert!(body.contains("cache.clear();"));
}

/// RED (0.4.4 ticket 09) — **every palette change tells this window's web pages their colour
/// scheme, the `Web pages` row goes through that same door, and a page is told at birth.**
///
/// `webhost::color_scheme_tests` holds the rule and the walk over a window's seats; this holds
/// the three places the window reaches them. `adopt_new_palette` is the one function every theme
/// flip, scheme swap and contrast floor goes through in every window (`adopt_application_change`
/// runs it on the others), so a page told from there cannot be left behind by any of them.
///
/// MUTATION: delete the `tell_web_pages_their_color_scheme` call from `adopt_new_palette`, or
/// open a seat without `web_color_scheme_in_force`, and this goes red.
#[test]
fn a_theme_flip_tells_every_web_page_its_colour_scheme() {
    let adopt = method_body("Runtime", "adopt_new_palette");
    assert!(
        adopt.contains("self.tell_web_pages_their_color_scheme();"),
        "a palette change reaches the pages: {adopt}"
    );
    let row = method_body("Runtime", "apply_web_color_scheme");
    assert!(
        row.contains("self.adopt_new_palette()?;"),
        "the row takes the palette's door: {row}"
    );
    let open = method_body("Runtime", "open_web_page_on");
    assert!(
        open.contains("self.web_color_scheme_in_force(),"),
        "a seat is told before its engine is asked for: {open}"
    );
    let tell = method_body("Runtime", "tell_web_pages_their_color_scheme");
    assert!(
        tell.contains("tell_all_seat_its_color_scheme(self.window.web.values_mut(), scheme)"),
        "every seat of the window, every tab: {tell}"
    );
}

/// RED (ticket 20) — **the command palette's field and rows are primary
/// text, not half a point above and below it.**
///
/// `UI-SPEC.md` T3/T4: the field used to sit at 13.5 and the rows at 12.5,
/// where every menu item, tree row, tab, button and combo is 13.
/// `palette.rs` has no test module of its own, so this lives here.
///
/// MUTATION: revert `palette::FIELD_FONT_LOGICAL_PX` or
/// `palette::ROW_FONT_LOGICAL_PX` to a literal and this goes red.
#[test]
fn ui_spec_palette_class_a_values_follow_the_rule() {
    assert_eq!(crate::palette::FIELD_FONT_LOGICAL_PX, 13.0, "UI-SPEC.md T3");
    assert_eq!(crate::palette::ROW_FONT_LOGICAL_PX, 13.0, "UI-SPEC.md T4");
}
