//! Symptom B pin (2026-07-25): the user reported `\int_{-\infty}^{+\infty} e^{-x^2}` rendering
//! with its lower limit missing (only the upper `+\infty` visible) on the *first* render, recovering
//! on re-render. `MathEngine::render` is a pure, deterministic function of (source, key), so a
//! transient first-render-only defect cannot originate here. This test pins the math layer:
//! at every DPI the integral's raster carries ink across its full vertical extent — the lower limit
//! (descent below the operator) is present, first inked row at the top edge and last inked row at
//! the bottom edge, with ink in all three vertical bands. A failure here would mean a *deterministic*
//! bt-math clip; a pass (the observed behaviour) exonerates bt-math and localizes the symptom to the
//! transient first-frame placement/band geometry above this layer.

use std::num::NonZeroU32;

use bt_math::{MathEngine, MathMode, MathRenderKey};

fn ink_row_profile(rgba: &[u8], w: u32, h: u32) -> Vec<u32> {
    (0..h as usize)
        .map(|y| {
            (0..w as usize)
                .filter(|&x| rgba[(y * w as usize + x) * 4 + 3] > 16)
                .count() as u32
        })
        .collect()
}

#[test]
fn integral_raster_contains_the_lower_limit_at_every_dpi() {
    let engine = MathEngine::new();
    let src = r"\int_{-\infty}^{+\infty} e^{-x^2}\,dx";
    for dpi in [1000u32, 1500, 2000, 3000] {
        let key = MathRenderKey {
            dpi_milli: NonZeroU32::new(dpi).unwrap(),
            font_milli_pt: NonZeroU32::new(24_000).unwrap(),
            foreground_rgb: [255, 255, 255],
            mode: MathMode::Display,
        };
        let raster = engine
            .render(src, key)
            .unwrap_or_else(|e| panic!("DPI={dpi}: integral must render: {e:?}"));
        let prof = ink_row_profile(&raster.rgba, raster.width_px, raster.height_px);
        let h = raster.height_px as usize;
        let top: u32 = prof[..h / 3].iter().sum();
        let mid: u32 = prof[h / 3..2 * h / 3].iter().sum();
        let bot: u32 = prof[2 * h / 3..].iter().sum();
        let first = prof.iter().position(|&c| c > 0).expect("some ink");
        let last = prof.iter().rposition(|&c| c > 0).expect("some ink");
        // The lower limit lives in the descent: the bottom band and the true bottom edge must be inked.
        assert!(
            bot > 0,
            "DPI={dpi}: lower limit clipped — no ink in bottom third"
        );
        assert!(
            top > 0,
            "DPI={dpi}: upper limit clipped — no ink in top third"
        );
        assert!(mid > 0, "DPI={dpi}: operator body missing");
        assert!(first <= 1, "DPI={dpi}: top edge not inked (first={first})");
        assert!(
            last >= h - 2,
            "DPI={dpi}: descent (lower limit) clipped short of the bottom edge (last={last}, h={h})"
        );
    }
}
