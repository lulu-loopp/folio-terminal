//! The typesetting's own pieces — the render key, the inline fit and the composite's anchor —
//! tested where they live (moved with them from `bt-term`, CC-6b).

use std::num::NonZeroU32;

use bt_doc::{LayoutKey, MathMode, SUBPIXELS_PER_PX};
use bt_math::MathEngine;
use bt_term::DualPlaneSession;

use super::*;

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

#[test]
fn an_inline_composite_anchor_must_fit_the_terminal_baseline_split() {
    let height_px = 18;
    let math_baseline_px = 14.0;
    // Once composited, the anchor is fixed: a height-only check cannot validate it.
    // Individual runs may shift before assembly; the assembled picture may not overflow.
    assert!(!baseline_box_fits(
        height_px,
        math_baseline_px,
        10 * SUBPIXELS_PER_PX,
        8 * SUBPIXELS_PER_PX,
    ));
    assert!(baseline_box_fits(
        height_px,
        math_baseline_px,
        14 * SUBPIXELS_PER_PX,
        4 * SUBPIXELS_PER_PX,
    ));
}

/// Physical em is independent of DPI metadata; display math retains its band key.
#[test]
fn terminal_inline_keys_use_physical_em_and_display_keys_keep_twelve_points() {
    let session = DualPlaneSession::new(nz(80), nz(8));
    for dpi in [1000, 1250, 2000] {
        let layout = LayoutKey {
            dpi_milli: nz(dpi),
            font_size_subpixels: 26 * SUBPIXELS_PER_PX,
            ..session.layout_key()
        };
        assert_eq!(
            terminal_math_render_key(layout, [220; 3], MathMode::Inline).unwrap(),
            bt_math::key_for_em_px(26.0, [220; 3], MathMode::Inline).unwrap()
        );
        let display = terminal_math_render_key(layout, [220; 3], MathMode::Display).unwrap();
        assert_eq!(display.dpi_milli.get(), dpi);
        assert_eq!(display.font_milli_pt.get(), 12_000);
    }
}

/// A triple-decker denominator needs more than twice the row at the pane's own em.
#[test]
fn an_unreadable_triple_decker_inline_fraction_keeps_source() {
    let engine = MathEngine::new();
    for (em, height, baseline) in [(26, 32, 25), (20, 27, 21)] {
        let key = bt_math::key_for_em_px(em as f32, [220, 220, 220], MathMode::Inline).unwrap();
        assert!(
            render_inline_run_fitted(
                &engine,
                r"\dfrac{\dfrac{\dfrac{a}{b}}{\dfrac{c}{d}}}{\dfrac{\dfrac{e}{f}}{\dfrac{g}{h}}}",
                key,
                baseline * SUBPIXELS_PER_PX,
                (height - baseline) * SUBPIXELS_PER_PX
            )
            .unwrap()
            .is_none()
        );
    }
}

/// PIN: fitting converges against the measured 44px high-DPI row and fractional baseline.
///
/// Retuned for T-MATH-INLINE-EM: a 52px stress em replaces the implicit 12pt/2x size so tall
/// members still need shrink under the full-row rule. The measured 30480/14576 subpixel split
/// stays exact: flooring the two budgets reserves the fractional placement remainder. Assert
/// that shrinking really occurred and that every final run can be composited within the row.
#[test]
fn the_inline_fit_converges_for_tall_constructions_at_high_dpi() {
    let engine = MathEngine::new();
    let key = bt_math::key_for_em_px(52.0, [220, 220, 220], MathMode::Inline).unwrap();
    // Measured off a 192-DPI window: a 44px row whose ASCII baseline is 29.766px down it.
    let ascent_budget = 30_480;
    let descent_budget = 14_576;
    assert_eq!(
        ascent_budget + descent_budget,
        44 * SUBPIXELS_PER_PX,
        "the two halves must be the row, or the fixture is not a line box"
    );
    let mut shrunk = 0;
    for source in [
        "x",
        "y",
        "E = mc^2",
        "x^2",
        r"\rho",
        r"\alpha+\beta",
        r"\frac{a}{b}",
        r"\sum_i",
        r"\hat{m}_t",
        r"\int_0^1",
    ] {
        let natural = engine.render(source, key).unwrap();
        let fitted = render_inline_run_fitted(&engine, source, key, ascent_budget, descent_budget)
            .expect("the engine renders every one of these")
            .unwrap_or_else(|| {
                panic!("{source} found no size that sits on a 44px/29px/14px line box")
            });
        let baseline_px = (ascent_budget / SUBPIXELS_PER_PX) as u32;
        let row_height_px = baseline_px + (descent_budget / SUBPIXELS_PER_PX) as u32;
        let top = inline_run_top(&fitted, baseline_px, row_height_px);
        assert!(
            top + fitted.height_px <= row_height_px,
            "{source} overflows its composite"
        );
        assert!(
            baseline_box_fits(
                top + fitted.height_px,
                baseline_px as f32,
                ascent_budget,
                descent_budget
            ),
            "{source} cannot be assembled"
        );
        if natural.height_px > row_height_px {
            shrunk += 1;
            assert!(
                fitted.height_px < natural.height_px,
                "{source} must really shrink"
            );
        }
    }
    assert!(
        shrunk > 0,
        "the fixture must exercise convergence, not only the no-shrink path"
    );
}
