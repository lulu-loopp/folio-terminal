//! The minus sign in front of a fraction — the one a Mac did not draw.
//!
//! Reported 2026-09-14 against `main` 1dc5113b, the same commit on both
//! machines: `\frac{1}{\sqrt{2\pi\sigma^2}} \exp\!\left(-\frac{(x-\mu)^2}
//! {2\sigma^2}\right)` sets on Windows and on macOS, and on the Mac the **unary**
//! minus in front of the inner fraction is not on the glass. The binary minus of
//! `x-\mu`, three characters further in, is.
//!
//! **The first thing to establish is that those are one sign and not two**, and
//! that is what the pure test below is for. A LaTeX `-` does not reach the
//! typesetter as a hyphen: Typst's math shorthand table maps `-` to `−`
//! (U+2212 MINUS SIGN) in the lexer, once, with no regard for what stands either
//! side of it, so the unary and the binary minus are the same character shaped
//! from the same face at the same weight. All that separates them is style size
//! — the unary one is in display style, the one inside the numerator is a step
//! down — and the class the spacing rules give them. There is no italic run, no
//! bold-math run and no second family in it anywhere.
//!
//! That matters because it rules a whole family of explanations out before any
//! machine is asked anything. A face that lacked the glyph, or lacked it in one
//! style, would take *both* minus signs with it. So would a family that resolved
//! differently: the engine registers the embedded `NewCMMath` faces before it
//! scans the machine (`typst-as-lib`'s searcher adds embedded fonts first), and
//! `FontBook::find_best_variant` keeps the first of equally-scoring faces, so
//! `New Computer Modern Math` on any machine is the face this crate carries with
//! it, even on a machine that has its own copy installed.
//!
//! What is left after that is a machine question, and [`minus_probe`] is how the
//! machine is asked it. It is `#[ignore]`d because it is a measurement rather
//! than a gate: run it on both machines and diff the transcript.

use bt_math::{
    MathEngine, MathMode, MathRaster, MathRenderKey, TypesetRun, device_px_per_pt, key_for_em_px,
};

/// The family every mathematical character is set in. Named here as a literal
/// because that is the point of the assertion: the test must fail if the crate
/// changes its mind about which family answers, not follow it.
const MATH_FAMILY: &str = "New Computer Modern Math";

/// U+2212 MINUS SIGN — what a LaTeX `-` becomes, unary or binary.
const MINUS: char = '\u{2212}';

/// U+002D HYPHEN-MINUS — the character the source file holds, and the one no
/// mathematics may be set in.
const HYPHEN_MINUS: char = '\u{002D}';

/// The formula the report was filed against, with the `$$` delimiters stripped
/// the way the preview's block scan strips them.
const REPORTED: &str =
    r"\frac{1}{\sqrt{2\pi\sigma^2}} \exp\!\left(-\frac{(x-\mu)^2}{2\sigma^2}\right)";

/// The key the Mac's own trace reported for this page — `math answered set=1
/// mode=Display em_milli=26000` — asked for the way the markdown preview asks
/// for it, in device pixels.
fn mac_key() -> MathRenderKey {
    key_for_em_px(26.0, [255, 255, 255], MathMode::Display).expect("26 device px is positive")
}

/// Every run of the page that is a lone minus sign.
fn minus_runs(runs: &[TypesetRun]) -> Vec<&TypesetRun> {
    runs.iter()
        .filter(|run| run.text.chars().eq([MINUS]))
        .collect()
}

/// How many pixels of the raster carry ink at all.
fn ink_pixels(raster: &MathRaster) -> usize {
    raster
        .rgba
        .iter()
        .skip(3)
        .step_by(4)
        .filter(|alpha| **alpha > 16)
        .count()
}

/// The `[start, end)` runs of columns carrying any ink — the glyph groups, in
/// reading order. The same measure `acceptance_display_integral.rs` reads a
/// typeset formula with.
fn ink_groups(raster: &MathRaster) -> Vec<(usize, usize)> {
    let width = raster.width_px as usize;
    let inked = |x: usize| {
        (0..raster.height_px as usize).any(|y| raster.rgba[(y * width + x) * 4 + 3] > 16)
    };
    let mut groups = Vec::new();
    let mut start = None;
    for x in 0..width {
        match (inked(x), start) {
            (true, None) => start = Some(x),
            (false, Some(from)) => {
                groups.push((from, x));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        groups.push((from, width));
    }
    groups
}

/// RED GATE — **a unary minus is U+2212, set upright in the math family, and
/// there are two of them in the reported formula.**
///
/// MUTATIONS:
/// ① let the source's `-` through as U+002D — the hyphen assertion goes red,
///    and so does the code-point one, because a hyphen is a different glyph of a
///    different width;
/// ② drop the unary minus (what the Mac shows) — the count goes to one;
/// ③ set the unary one from a second family, or in an italic or bold face —
///    the face assertions name the family and weight that answered.
///
/// This runs on every machine, and that is deliberate: it is the assertion the
/// Mac has to be put in front of. It reads the *page*, not the picture, because
/// a character `typst-layout` could not shape is not drawn `.notdef`, it is
/// dropped — `math::text::layout_glyph` pushes a fragment only when
/// `GlyphFragment::new` returns `Some` — and a dropped character takes no room
/// and leaves no ink to find in a raster.
#[test]
fn the_two_minus_signs_of_the_reported_formula_are_one_sign_set_twice() {
    let engine = MathEngine::new();
    let runs = engine
        .typeset_runs(REPORTED, mac_key())
        .expect("the reported formula must typeset");

    for run in &runs {
        println!(
            "MINUS-RUN text={:?} codepoints={:04X?} family={:?} italic={} weight={} \
             size_pt={:.3} glyphs={:?} advances_milli_em={:?}",
            run.text,
            run.text.chars().map(u32::from).collect::<Vec<u32>>(),
            run.family,
            run.italic,
            run.weight,
            run.size_pt,
            run.glyph_ids,
            run.advances_milli_em,
        );
        assert!(
            !run.text.contains(HYPHEN_MINUS),
            "no run of a formula may hold a hyphen where a minus belongs: {run:?}"
        );
        assert!(
            !run.has_notdef(),
            "the page drew a .notdef, so this machine cannot set this formula: {run:?}"
        );
    }

    let minuses = minus_runs(&runs);
    assert_eq!(
        minuses.len(),
        2,
        "the formula holds a unary minus and a binary one, and both must reach the \
         page — {} did. The runs that did: {:?}",
        minuses.len(),
        runs.iter().map(|run| &run.text).collect::<Vec<_>>()
    );

    // Told apart by their style size rather than by their order on the page:
    // the unary one stands in display style, the one inside the numerator a
    // step down. Which of the two the frame happens to hold first is the
    // typesetter's business, not this assertion's.
    let mut by_size = minuses.clone();
    by_size.sort_by(|left, right| {
        right
            .size_pt
            .partial_cmp(&left.size_pt)
            .expect("a page's text sizes are finite")
    });
    let (unary, binary) = (by_size[0], by_size[1]);
    for minus in &minuses {
        assert_eq!(
            minus.family, MATH_FAMILY,
            "a minus sign is set in the math family: {minus:?}"
        );
        assert!(!minus.italic, "a minus sign is never slanted: {minus:?}");
        assert!(
            minus.weight < 600,
            "a minus sign is not bold-math: {minus:?}"
        );
        assert_eq!(
            minus.glyph_ids.len(),
            1,
            "one character, one glyph: {minus:?}"
        );
        assert!(
            minus.advances_milli_em.iter().all(|advance| *advance > 0),
            "a minus sign that takes no room is a minus sign nobody can see: {minus:?}"
        );
    }
    assert_eq!(
        unary.glyph_ids, binary.glyph_ids,
        "the unary minus and the binary one are the same glyph of the same face — \
         unary {unary:?}, binary {binary:?}"
    );
    assert_eq!(
        (unary.family.as_str(), unary.italic, unary.weight),
        (binary.family.as_str(), binary.italic, binary.weight),
        "…and they are drawn from the same face — unary {unary:?}, binary {binary:?}"
    );
    // Style size is the *only* difference between the two, and it is a real
    // one: a page that set them at the same size would be a page that had
    // stopped putting the numerator in a smaller style.
    assert!(
        unary.size_pt > binary.size_pt,
        "the unary minus is set in display style and the numerator's a step down, \
         so their sizes differ: {} against {}",
        unary.size_pt,
        binary.size_pt
    );
}

/// RED GATE — **the minus is ink, not only a fragment.** The same formula with
/// and without it: the one that has it is wider by about a minus advance and
/// carries one more group of inked columns.
///
/// A frame assertion cannot see a glyph that was laid out and then not drawn.
/// This can, and it is written as a comparison rather than as a committed
/// picture so that it says the same thing at every size and after a typst
/// upgrade.
///
/// MUTATION: remove the `-` from the source and the group count and the width
/// both fall to the bare fraction's, which is the state the Mac is in.
#[test]
fn the_unary_minus_puts_ink_on_the_page() {
    let engine = MathEngine::new();
    let key = mac_key();
    let with = engine
        .render(r"\left(-\frac{(x-\mu)^2}{2\sigma^2}\right)", key)
        .expect("the parenthesised fraction must set");
    let without = engine
        .render(r"\left(\frac{(x-\mu)^2}{2\sigma^2}\right)", key)
        .expect("the parenthesised fraction must set without its minus");

    println!(
        "MINUS-INK with={}x{} ink={} groups={:?}",
        with.width_px,
        with.height_px,
        ink_pixels(&with),
        ink_groups(&with),
    );
    println!(
        "MINUS-INK without={}x{} ink={} groups={:?}",
        without.width_px,
        without.height_px,
        ink_pixels(&without),
        ink_groups(&without),
    );

    // Everything but the minus is the same formula, so the difference in inked
    // pixels *is* the minus. A sign that was laid out and then not drawn — the
    // reported symptom — leaves this at zero or below.
    assert!(
        ink_pixels(&with) > ink_pixels(&without),
        "the minus has to put ink on the page: {} inked pixels with it against {} \
         without",
        ink_pixels(&with),
        ink_pixels(&without)
    );
    // And it has to take room. The glyph advances 0.778 em; half an em is a
    // floor no rounding gets under and no spacing rule has to be re-taught.
    let em_px =
        f64::from(key.font_milli_pt.get()) / 1000.0 * f64::from(device_px_per_pt(key.dpi_milli));
    let grew = f64::from(with.width_px) - f64::from(without.width_px);
    assert!(
        grew >= em_px / 2.0,
        "the formula that carries a minus is a minus wider than the one that does \
         not: {} px against {} px, at an em of {em_px:.1} px",
        with.width_px,
        without.width_px
    );
}

/// MACHINE PROBE — not a gate. Run it on the machine that shows the defect and
/// on one that does not, and diff the two transcripts.
///
/// ```text
/// RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin \
///   cargo test -p bt-math --test unary_minus minus_probe -- --ignored --nocapture
/// ```
///
/// It prints, for a minus in each of the positions the report distinguishes —
/// alone, binary, first inside a `\left(…\right)`, and already written as
/// U+2212 in the source — the characters that reached the page, the family,
/// style and weight that answered for each, and the glyph ids and advances that
/// came back. A machine that draws no minus says so here in one of exactly two
/// ways: a glyph id of `0` (the face claimed the character and has no outline
/// for it) or a missing run (no family could shape it and the typesetter dropped
/// it). Anything else, and the two machines' engines agree and the defect is not
/// in this crate.
#[test]
#[ignore = "machine probe: run on the machine that shows the defect and diff against one that does not"]
fn minus_probe() {
    let engine = MathEngine::new();
    let key = mac_key();
    println!(
        "MINUS-PROBE key dpi_milli={} font_milli_pt={} mode=Display",
        key.dpi_milli.get(),
        key.font_milli_pt.get()
    );
    for source in [
        r"-x",
        r"x-y",
        r"\left(-x\right)",
        "\u{2212}x",
        r"-\frac{a}{b}",
        r"\left(-\frac{a}{b}\right)",
        r"\exp\!\left(-\frac{a}{b}\right)",
        REPORTED,
    ] {
        match engine.typeset_runs(source, key) {
            Ok(runs) => {
                println!("MINUS-PROBE source={source:?} runs={}", runs.len());
                for (index, run) in runs.iter().enumerate() {
                    println!(
                        "  [{index}] text={:?} codepoints={:04X?} family={:?} italic={} \
                         weight={} size_pt={:.3} glyphs={:?} advances_milli_em={:?}",
                        run.text,
                        run.text.chars().map(u32::from).collect::<Vec<u32>>(),
                        run.family,
                        run.italic,
                        run.weight,
                        run.size_pt,
                        run.glyph_ids,
                        run.advances_milli_em,
                    );
                }
                let minuses = minus_runs(&runs);
                println!(
                    "MINUS-PROBE source={source:?} minus_runs={} notdef={}",
                    minuses.len(),
                    runs.iter().filter(|run| run.has_notdef()).count()
                );
            }
            Err(error) => println!("MINUS-PROBE source={source:?} refused: {error}"),
        }
        match engine.render(source, key) {
            Ok(raster) => println!(
                "MINUS-PROBE source={source:?} raster={}x{} ink_groups={:?}",
                raster.width_px,
                raster.height_px,
                ink_groups(&raster)
            ),
            Err(error) => println!("MINUS-PROBE source={source:?} raster refused: {error}"),
        }
    }
}
