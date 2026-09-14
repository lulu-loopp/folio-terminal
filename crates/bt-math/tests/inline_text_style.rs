//! An inline formula is set in **text style**, and a display block is not.
//!
//! This is the claim the terminal's whole inline pipeline rests on and it had
//! no geometric pin. `MathMode` reaches the Typst source through
//! `sys.inputs.display`, which chooses `$ … $` (a block equation, display
//! style) or `$…$` (an inline equation, text style) — the spacing *is* the
//! syntax in Typst, and `MathRenderKey::mode` carries the choice into the cache
//! key so the two can never be served for one another. What existed to prove it
//! was one height comparison (`delimiter_mode_controls_the_eval_equation…`),
//! which a formula could pass while still carrying its limits over the
//! operator.
//!
//! The difference that matters to a terminal is **where a big operator's limits
//! go**. In display style `∑_{n=1}^{∞}` stacks them above and below the sign,
//! which is two extra line-heights of ink that no single terminal row can hold;
//! in text style they sit beside it, which is what LaTeX's own `\textstyle`
//! does and what makes `$\sum_{n=1}^{\infty} \frac{1}{n^2}$` a thing that can
//! stand on a line of prose at all. Stacked limits are narrow and tall; limits
//! beside the sign are wide and short, and that is one measurement with no room
//! for argument in it.
//!
//! Written because the two formulas of the 2026-09-14 report that stayed source
//! (`\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}` and
//! `\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}`) are exactly the two
//! carrying a big operator with **both** limits, and "the inline path sets them
//! in display style" was the standing hypothesis for it. It does not, and this
//! is where the next reader can see that for themselves rather than take it on
//! trust.

use std::num::NonZeroU32;

use bt_math::{MathEngine, MathMode, MathRaster, MathRenderKey};

fn key(mode: MathMode) -> MathRenderKey {
    MathRenderKey {
        dpi_milli: NonZeroU32::new(2000).expect("2000 is not zero"),
        font_milli_pt: NonZeroU32::new(24_000).expect("24000 is not zero"),
        foreground_rgb: [255, 255, 255],
        mode,
    }
}

fn both(engine: &MathEngine, source: &str) -> (MathRaster, MathRaster) {
    let render = |mode| {
        engine
            .render(source, key(mode))
            .unwrap_or_else(|error| panic!("{source} ({mode:?}) must render: {error:?}"))
    };
    let inline = render(MathMode::Inline);
    let display = render(MathMode::Display);
    println!(
        "TEXTSTYLE source={source} inline={}x{} baseline={:.2} display={}x{} baseline={:.2}",
        inline.width_px,
        inline.height_px,
        inline.baseline_px,
        display.width_px,
        display.height_px,
        display.baseline_px
    );
    (inline, display)
}

/// RED GATE — a big operator's limits stand beside it inline and over it in a
/// display block.
///
/// MUTATIONS:
/// ① wrap the inline source as `"$ " + source + " $"` too (one space, the whole
///    difference) — the inline raster becomes the display raster and every
///    assertion below goes red at once;
/// ② drop `mode` from `MathRenderKey` — the two renders collide in the cache and
///    the second call returns the first one's picture, which these comparisons
///    catch because they demand the two be *different*.
#[test]
fn a_big_operator_carries_its_limits_beside_it_inline_and_above_it_in_display() {
    let engine = MathEngine::new();

    // The flip itself, on the bare operator: stacked limits are narrow and tall,
    // limits beside the sign are wide and short.
    let (inline, display) = both(&engine, r"\sum_{n=1}^{\infty}");
    assert!(
        inline.height_px < display.height_px,
        "inline must not stack the limits: inline {}px tall against display {}px",
        inline.height_px,
        display.height_px
    );
    assert!(
        inline.width_px > display.width_px,
        "limits beside the sign make the inline form the wider of the two: inline {}px against \
         display {}px",
        inline.width_px,
        display.width_px
    );

    // And the same on the two formulas the user's screenshot left standing as
    // source. Height alone here: they carry `=` and a right-hand side, so most
    // of their width is shared and the operator's own limits move a smaller
    // share of it.
    for source in [
        r"\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}",
        r"\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}",
        r"\lim_{x \to 0} \frac{\sin x}{x} = 1",
    ] {
        let (inline, display) = both(&engine, source);
        assert!(
            inline.height_px <= display.height_px,
            "{source}: the inline form may never be the taller of the two — inline {}px against \
             display {}px",
            inline.height_px,
            display.height_px
        );
    }
}
