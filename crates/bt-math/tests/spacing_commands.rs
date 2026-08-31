//! The TeX spacing family (`\;` `\,` `\:` `\!`) is a backslash followed by ASCII punctuation —
//! precisely the shape a CommonMark backslash-unescape strips. When the backslash is lost, the
//! survivor is not a narrower space but a *visible glyph*: MiTeX turns a bare `;` into Typst's
//! `\;`, an escaped literal semicolon. That is the reported `;+;` symptom, and this test pins
//! both halves of it — the intact commands convert to real Typst spacing, and the stripped form
//! demonstrably degrades to a printed semicolon rather than silently rendering the same.
//!
//! Nothing here is a repair. Restoring `\;` from a bare `;` would be a content guess, because
//! `f(x; \theta)` is legitimate math; the terminal's job is to render faithfully what arrived.

use std::num::NonZeroU32;

use bt_math::{MathEngine, MathMode, MathRenderKey};

fn convert(source: &str) -> String {
    mitex::convert_math(source, Some(mitex_spec_gen::DEFAULT_SPEC.clone()))
        .unwrap_or_else(|error| panic!("{source} must convert: {error:?}"))
}

#[test]
fn spacing_commands_convert_to_typst_spacing() {
    for (source, expected) in [
        (r"a \; b", "thick"),
        (r"a \, b", "thin"),
        (r"a \: b", "med"),
        (r"a \! b", "negthinspace"),
        (r"a \quad b", "quad"),
    ] {
        let converted = convert(source);
        assert!(
            converted.contains(expected),
            "{source} must convert to Typst {expected}, got {converted:?}"
        );
        assert!(
            !converted.contains(r"\;"),
            "{source} must not degrade to a literal semicolon, got {converted:?}"
        );
    }
}

/// The stripped form is what the user actually saw. `\;` in Typst output is an *escaped*
/// semicolon, i.e. one that prints — so losing the backslash upstream turns spacing into
/// punctuation. This is the diagnostic that identifies the symptom, not a defect in this crate.
#[test]
fn a_stripped_backslash_degrades_to_a_printed_semicolon() {
    let converted = convert("a ; + ; b");
    assert!(
        converted.contains(r"\;"),
        "a bare semicolon must reach Typst as an escaped literal, got {converted:?}"
    );
    assert!(
        !converted.contains("thick"),
        "a bare semicolon must not silently become spacing, got {converted:?}"
    );
}

#[test]
/// `\text{死} \; + \; \text{活}` is here because a runner refused it (2026-08-31,
/// CI run `33397648409`, `docs/DESIGN.md` §7.1.3i′ ⑫) on a machine that drew
/// 中, 文 and 项目数 in the same suite. The whole formula now renders there —
/// 死 from `Dotum`, 活 from `Malgun Gothic`, measured on that runner — because
/// the CJK judgment is made against the whole font book and every family that
/// answers is named to Typst, rather than one face having to answer alone.
fn spacing_commands_render_including_alongside_cjk() {
    let engine = MathEngine::new();
    let key = MathRenderKey {
        dpi_milli: NonZeroU32::new(2000).unwrap(),
        font_milli_pt: NonZeroU32::new(24_000).unwrap(),
        foreground_rgb: [255, 255, 255],
        mode: MathMode::Display,
    };
    for source in [
        r"a \; + \; b",
        r"\text{死} \; + \; \text{活}",
        r"x \, y \: z \! w",
        r"f(x; \theta) \Rightarrow y",
    ] {
        let raster = engine
            .render(source, key)
            .unwrap_or_else(|error| panic!("{source} must render: {error:?}"));
        assert!(raster.width_px > 0 && raster.height_px > 0, "{source}");
    }
}
