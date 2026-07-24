//! MiTeX still converts `\hbar` to the pre-0.15 Typst symbol path `planck.reduce`, which the
//! codex 0.3 symbol set no longer defines (its `planck` has no variants) — every formula using
//! `\hbar` failed to compile and stayed source on screen (Codex CLI Schrödinger example,
//! 2026-07-24). The template shadows `planck` with a symbol that restores the `reduce` variant.

use std::num::NonZeroU32;

use bt_math::{MathEngine, MathMode, MathRenderKey};

#[test]
fn hbar_formulas_render() {
    let engine = MathEngine::new();
    let key = MathRenderKey {
        dpi_milli: NonZeroU32::new(2000).unwrap(),
        font_milli_pt: NonZeroU32::new(24_000).unwrap(),
        foreground_rgb: [255, 255, 255],
        mode: MathMode::Display,
    };
    for source in [
        r"i\hbar\frac{\partial}{\partial t}\Psi=\hat{H}\Psi",
        r"\hbar",
        r"E=\hbar\omega",
    ] {
        let raster = engine
            .render(source, key)
            .unwrap_or_else(|error| panic!("{source} must render: {error:?}"));
        assert!(raster.width_px > 0 && raster.height_px > 0, "{source}");
    }
}
