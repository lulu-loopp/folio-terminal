//! The §M2 acceptance line's own formula, pinned where it is typeset.
//!
//! "Open a file containing `$$\int_0^1 x\,dx$$` and the integral is **typeset,
//! not printed as source**" (`docs/plans/port/macos-plan-2026-09-12.md` §M2).
//! Everything above this layer — the preview's block scan, the picture cache,
//! the pane — can be read off a photograph of a running window, and M2-7 did
//! read it off one. What a photograph cannot say is whether the two machines
//! agree, because the window on the other machine is a different window.
//!
//! This is where they can be made to answer the same question: `MathEngine`
//! holds no platform code at all (`scripts/check-portable-core.ps1` keeps it
//! that way), so the raster it returns for one `(source, key)` is a pure
//! function of the two, and a Windows workstation and a Mac running this test
//! print the same numbers or the port has a defect in it. That is the
//! cross-platform reference DESIGN §13.40 reads the Mac's photograph against,
//! and it is rendered **offscreen on both**: no window, no GPU, no swapchain.
//!
//! The assertion is the acceptance line's own word. "Typeset" is not "some ink
//! arrived": a line of *source* would be one band of glyphs all of a height,
//! while a set display integral stands taller than the letters beside it and
//! carries its two limits above and below the middle of the operator. Both of
//! those are measured here.

use std::num::NonZeroU32;

use bt_math::{MathEngine, MathMode, MathRenderKey};

/// The alpha plane's inked count per column.
fn ink_columns(rgba: &[u8], w: u32, h: u32) -> Vec<u32> {
    (0..w as usize)
        .map(|x| {
            (0..h as usize)
                .filter(|&y| rgba[(y * w as usize + x) * 4 + 3] > 16)
                .count() as u32
        })
        .collect()
}

/// The `[start, end)` runs of columns carrying any ink at all — the glyph
/// groups, in reading order.
fn groups(columns: &[u32]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (index, count) in columns.iter().enumerate() {
        match (*count > 0, start) {
            (true, None) => start = Some(index),
            (false, Some(from)) => {
                out.push((from, index));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        out.push((from, columns.len()));
    }
    out
}

/// FNV-1a over the alpha plane. A digest rather than a committed picture for
/// §13.22 ③'s reason said about a different raster: what is compared across two
/// machines has to be the thing that is the same on both, and the alpha plane is
/// — the colour is whatever foreground the caller asked for.
fn alpha_digest(rgba: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for alpha in rgba.iter().skip(3).step_by(4) {
        hash ^= u64::from(*alpha);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// RED GATE (DESIGN §13.40) — the acceptance line's formula is **set**, and it
/// is set identically on both machines.
///
/// MUTATIONS:
/// ① render the source as a plain text run instead of typesetting it — every
///    group is one line-height tall and the operator assertion goes red;
/// ② drop the limits from the operator — the tall group stops being taller than
///    the raster's own middle band and the limit assertion goes red;
/// ③ let the raster width follow the *running* platform's default font — the
///    two machines print different digests and the transcript says so.
#[test]
fn the_acceptance_lines_integral_is_set_rather_than_printed() {
    let engine = MathEngine::new();
    // The line as the acceptance corpus writes it, delimiters stripped the way
    // the preview's block scan strips them.
    let source = r"\int_0^1 x\,dx";
    // Backing scale 2 — the Mac desk M2-7 photographed, and the scale §13.22
    // measured the glyph bands at.
    let key = MathRenderKey {
        dpi_milli: NonZeroU32::new(2000).expect("2000 is not zero"),
        font_milli_pt: NonZeroU32::new(24_000).expect("24000 is not zero"),
        foreground_rgb: [255, 255, 255],
        mode: MathMode::Display,
    };
    let raster = engine
        .render(source, key)
        .unwrap_or_else(|error| panic!("the acceptance line's formula must set: {error:?}"));

    let columns = ink_columns(&raster.rgba, raster.width_px, raster.height_px);
    let groups = groups(&columns);
    let height = |group: &(usize, usize)| {
        (group.0..group.1)
            .map(|x| {
                let inked: Vec<usize> = (0..raster.height_px as usize)
                    .filter(|&y| raster.rgba[(y * raster.width_px as usize + x) * 4 + 3] > 16)
                    .collect();
                inked
                    .last()
                    .zip(inked.first())
                    .map_or(0, |(l, f)| l - f + 1)
            })
            .max()
            .unwrap_or(0)
    };

    println!(
        "M27-INTEGRAL source={source} width={} height={} content_height={} \
         ascent={:.3} descent={:.3} baseline={:.3} groups={} alpha_fnv1a={:016x}",
        raster.width_px,
        raster.height_px,
        raster.content_height_px,
        raster.ascent_px,
        raster.descent_px,
        raster.baseline_px,
        groups.len(),
        alpha_digest(&raster.rgba)
    );
    println!(
        "M27-INTEGRAL group heights: {:?}",
        groups
            .iter()
            .map(|g| (g.0, g.1, height(g)))
            .collect::<Vec<_>>()
    );

    assert!(
        groups.len() >= 3,
        "a set `\\int_0^1 x\\,dx` stands in several glyph groups, not one run: {groups:?}"
    );
    let tallest = groups.iter().map(&height).max().expect("there are groups");
    let last = height(groups.last().expect("there are groups"));
    assert!(
        tallest > last * 2,
        "the operator must stand taller than the letters beside it — tallest {tallest}px \
         against the trailing group's {last}px, which is what a line of *source* would \
         instead make equal"
    );
    // The operator's group is the first one and it is where the limits are, so
    // it reaches both above and below the raster's own middle third. A line of
    // source has no such group.
    let (from, to) = groups[0];
    let third = raster.height_px as usize / 3;
    let inked_in = |y0: usize, y1: usize| {
        (y0..y1).any(|y| {
            (from..to).any(|x| raster.rgba[(y * raster.width_px as usize + x) * 4 + 3] > 16)
        })
    };
    assert!(inked_in(0, third), "the upper limit is missing");
    assert!(
        inked_in(2 * third, raster.height_px as usize),
        "the lower limit is missing"
    );
}
