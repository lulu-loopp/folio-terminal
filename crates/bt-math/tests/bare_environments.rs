//! The markdown preview hands a bare `\begin{…}…\end{…}` straight to this engine, with no `$$`
//! wrapped around it (`bt_app::preview::display_math_block`, 2026-08-25). The claim that made that
//! safe was "MiTeX already knows these environments" — this pins it, because a parser that
//! recognises an environment the engine cannot set is a parser that turns a rendered formula into
//! literal text, which is exactly the report the slice answers.
//!
//! Written down alongside: the environments this engine **refuses**, so the next reader knows the
//! refusal is old news and not a regression. `alignat`, `multline`, `multline*`, `gather*` and
//! `eqnarray` are unknown to MiTeX's spec; `Vmatrix` converts to a `mat` delimiter Typst rejects.
//! All five stay in the parser's list — recognising an environment is a question about the
//! document's language, and what happens when the engine says no is §7.1.3i′⑩: the author's own
//! text stands, unmarked.

use std::num::NonZeroU32;

use bt_math::{MathEngine, MathMode, MathRenderKey};

#[test]
fn the_bare_environments_the_preview_recognises_are_set_by_this_engine() {
    let engine = MathEngine::new();
    let key = MathRenderKey {
        dpi_milli: NonZeroU32::new(2000).unwrap(),
        font_milli_pt: NonZeroU32::new(24_000).unwrap(),
        foreground_rgb: [255, 255, 255],
        mode: MathMode::Display,
    };
    for source in [
        r"\begin{align}a &= b \\ c &= d\end{align}",
        r"\begin{align*}a &= b \\ c &= d\end{align*}",
        r"\begin{aligned}a &= b \\ c &= d\end{aligned}",
        r"\begin{equation}a = b\end{equation}",
        r"\begin{equation*}a = b\end{equation*}",
        r"\begin{gather}a = b \\ c = d\end{gather}",
        r"\begin{gathered}a = b \\ c = d\end{gathered}",
        r"\begin{split}a &= b \\ &= c\end{split}",
        r"\begin{cases}x, & x \geq 0 \\ -x, & x < 0\end{cases}",
        r"\begin{matrix}a & b \\ c & d\end{matrix}",
        r"\begin{pmatrix}a & b \\ c & d\end{pmatrix}",
        r"\begin{bmatrix}a & b \\ c & d\end{bmatrix}",
        r"\begin{Bmatrix}a & b \\ c & d\end{Bmatrix}",
        r"\begin{vmatrix}a & b \\ c & d\end{vmatrix}",
        r"\begin{smallmatrix}a & b \\ c & d\end{smallmatrix}",
        r"\begin{array}{cc}a & b \\ c & d\end{array}",
        r"\sum_{\begin{subarray}{c}i<j\end{subarray}} x",
    ] {
        let raster = engine
            .render(source, key)
            .unwrap_or_else(|error| panic!("{source} must render: {error:?}"));
        assert!(raster.width_px > 0 && raster.height_px > 0, "{source}");
    }
}
