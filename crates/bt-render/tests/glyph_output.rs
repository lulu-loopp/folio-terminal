//! **The glyph output, pinned at backing scale 2 on whichever machine is
//! running** — M2-5's gate (`docs/DESIGN.md` §13.22).
//!
//! Every case here draws [`bt_render::glyph_probe::GlyphFixture`] into an
//! offscreen window and reads the pixels back, which is the platform-neutral
//! half of the ticket's pair; the other half is a real Metal swapchain and
//! lives in `crates/bt-app/tests/macos_glyph_surface.rs`, because only a Mac
//! has one. The two are handed the same frame by the same builder, which is
//! what makes "the swapchain and the texture agree" a statement about the
//! **presentation path** rather than about two fixtures that look alike.
//!
//! # Why these facts and not a picture
//!
//! A committed PNG would pin the bytes of one machine's font. Folio draws in
//! `Consolas` on Windows and `Menlo` on a Mac (`DEFAULT_PRIMARY_FONT_FAMILY`,
//! §13.18 ⑤), so a byte comparison across the two would fail for the one
//! reason that is not a defect. What *is* the same on both — because the
//! rasterizer is the same Swash on both (`macos-plan-2026-09-12.md` §R4) — is
//! the **shape of the coverage**: a stem's interior is solid, its skirt is
//! antialiased rather than bilevel, a translucent frame is premultiplied to
//! the last pixel, every column of the grid draws its stem in the same place
//! inside its own cell, and the prose lane — the one place an x is really
//! fractional — moves with the fraction it was given. Those are the facts
//! below, and each of them is a number a Windows run and a Mac run can be
//! compared on.
//!
//! Run with `--nocapture` to read the `BT_GLYPH` lines these cases print;
//! that is the measurement the ticket reports, on either machine.

use bt_render::glyph_probe::{
    FIXTURE_COLUMNS, FRACTIONAL_ORIGINS, GLYPH_BANDS, GlyphFixture, digest, first_difference,
    fractional_phases, grid_phases, measure_band, premultiplied_violations, report,
};
use bt_render::{GpuContext, WindowRenderer};

/// The format every window in this product picks, named rather than asked of a
/// swapchain there is none of.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// The surface, in physical pixels, and the scale the ticket measures at.
const WIDTH: u32 = 900;
const HEIGHT: u32 = 760;
const SCALE: f64 = 2.0;

/// One drawn page, and the pixels it came out as.
struct Drawn {
    pixels: Vec<[u8; 4]>,
    metrics: bt_render::CellMetrics,
}

fn draw(fixture: GlyphFixture) -> Drawn {
    let mut gpu = pollster::block_on(GpuContext::headless(FORMAT)).expect("a device");
    let mut window =
        WindowRenderer::offscreen(&mut gpu, fixture.width, fixture.height, SCALE, FORMAT)
            .expect("an offscreen window");
    fixture
        .present(&mut gpu, &mut window)
        .expect("the fixture draws");
    let pixels = window.read_back(&gpu).expect("the frame reads back");
    Drawn {
        pixels,
        metrics: window.base_metrics(),
    }
}

/// **The page fits on the surface it is drawn on.**
///
/// The bands are addressed by arithmetic — `padding + row × cell_height_px` —
/// and a face whose cells grew would push the last of them off the bottom,
/// where every measurement below would quietly read a blank strip and pass.
/// So the geometry is asserted before anything is read out of it.
#[test]
fn the_fixture_fits_the_surface_it_is_measured_on() {
    let fixture = GlyphFixture::new(WIDTH, HEIGHT);
    let drawn = draw(fixture);
    let last = GLYPH_BANDS[GLYPH_BANDS.len() - 1];
    let rect = fixture.band_rect(drawn.metrics, last);
    assert!(
        rect[3] < HEIGHT && rect[2] < WIDTH,
        "the {} band ends at {rect:?} on a {WIDTH}x{HEIGHT} surface — the fixture no longer fits \
         the size this gate draws it at",
        last.name
    );
    assert!(
        drawn.metrics.scale_factor > 1.99,
        "this gate is the scale-2 measurement and the window measured {}",
        drawn.metrics.scale_factor
    );
    print!("{}", report(&drawn.pixels, WIDTH, fixture, drawn.metrics));
}

/// **Two runs of one fixture are the same bytes.**
///
/// The frame carries no clock, no caret and no animation, so the only way two
/// draws of it differ is that something upstream is not a function of its
/// inputs — an atlas packing that depends on what was drawn before it, a
/// blink, a theme read at draw time. This is the case that would go red first,
/// and it is the precondition for every comparison the Mac's window test makes
/// between a swapchain and a texture.
///
/// MUTATION: give the fixture's cursor `visible: true` and the two frames part
/// company on the blink.
#[test]
fn the_same_page_drawn_twice_is_the_same_page() {
    let fixture = GlyphFixture::new(WIDTH, HEIGHT);
    let first = draw(fixture);
    let second = draw(fixture);
    assert_eq!(
        first.pixels.len(),
        second.pixels.len(),
        "two draws of one fixture are two surfaces of one size"
    );
    if let Some((index, left, right)) = first_difference(&first.pixels, &second.pixels) {
        panic!(
            "two draws of the same page differ first at pixel {index} \
             ({},{}) — {left:?} against {right:?}; digests {:016x} and {:016x}",
            index as u32 % WIDTH,
            index as u32 / WIDTH,
            digest(&first.pixels),
            digest(&second.pixels),
        );
    }
}

/// **Every band has ink in it, and the CJK band is the one that says so.**
///
/// A fallback chain written for the wrong operating system does not fail — it
/// draws nothing, or it draws boxes, and both look like "the test passed" to
/// anything that only counts pixels in the Latin row. This asks each band for
/// its own ink, which makes a machine with no CJK face a red line here rather
/// than a surprise in a screenshot.
#[test]
fn every_band_of_the_page_has_ink_of_its_own() {
    let fixture = GlyphFixture::new(WIDTH, HEIGHT);
    let drawn = draw(fixture);
    for band in GLYPH_BANDS {
        let stats = measure_band(&drawn.pixels, WIDTH, fixture.band_rect(drawn.metrics, band));
        assert!(
            stats.ink_pixels > 0,
            "the {} band drew nothing at all — on a font stack that cannot answer this row, \
             this is what a missing face looks like",
            band.name
        );
        assert!(
            stats.peak_coverage > 0.9,
            "the {} band's strongest pixel is only {:.3} of the ink colour: nothing in it \
             reached full coverage, which no face does at scale 2",
            band.name,
            stats.peak_coverage,
        );
    }
}

/// **The rasterized bands are antialiased and the geometric one is not.**
///
/// The pair is the point. Box drawing is cell geometry in this renderer
/// (`crate::procedural`) and comes out of the rectangle pipeline on whole
/// pixels; the rows around it come out of Swash. So a change that moved
/// coverage in *both* is the surface, and one that moved it in the glyph rows
/// alone is the rasterizer — which is the question M2-5 exists to be able to
/// answer.
///
/// The tolerance is wide on purpose and is justified by what it has to hold
/// across: `Consolas` at 32px and `Menlo` at 32px are different outlines, so
/// the number that can be pinned is not a share but the *presence* of a skirt.
/// A tenth of the ink pixels lying strictly between nearly-clear and
/// nearly-solid is far below what any grayscale rasterizer produces and far
/// above the nothing a bilevel one would.
#[test]
fn a_glyph_band_carries_an_antialiased_skirt() {
    let fixture = GlyphFixture::new(WIDTH, HEIGHT);
    let drawn = draw(fixture);
    for band in GLYPH_BANDS.iter().filter(|band| band.rasterized) {
        let stats = measure_band(
            &drawn.pixels,
            WIDTH,
            fixture.band_rect(drawn.metrics, *band),
        );
        assert!(
            stats.antialiased_share > 0.10,
            "the {} band is {:.1}% partial coverage — a rasterizer that had stopped \
             antialiasing would read like this",
            band.name,
            stats.antialiased_share * 100.0,
        );
    }
}

/// **Every column of the grid draws the same raster in the same place inside
/// its own cell.**
///
/// This is the fact the terminal actually has, and it is not the one the
/// ticket's question assumes: `CellMetrics::measure` takes
/// `primary_advance_px.ceil()` for the cell width and `(8 x scale).ceil()` for
/// the padding, so `padding + column x cell_width_px` is a **whole number in
/// every column** and a terminal grid never asks the rasterizer for a
/// fractional phase at all. So the pin is a spread of zero: thirty-four copies
/// of one character, each measured against its own cell origin, agreeing to
/// the last fraction of a pixel.
///
/// MUTATION: drop the `.ceil()` on `cell_width_px` and the columns start
/// landing on fractions of a pixel — which is also a caret that no longer
/// stands where its column is.
#[test]
fn every_column_of_the_grid_draws_its_stem_in_the_same_place() {
    let fixture = GlyphFixture::new(WIDTH, HEIGHT);
    let drawn = draw(fixture);
    assert_eq!(
        drawn.metrics.cell_width_px.fract(),
        0.0,
        "a cell is a whole number of pixels wide, which is what makes the spread below zero"
    );
    let phases = grid_phases(&drawn.pixels, WIDTH, drawn.metrics);
    assert_eq!(
        phases.len(),
        FIXTURE_COLUMNS as usize,
        "every column of the stem band has ink in it"
    );
    let low = phases.iter().copied().fold(f32::INFINITY, f32::min);
    let high = phases.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        high - low < 0.01,
        "the same character's ink centroid moved {:.4}px between two columns that are a whole \
         number of pixels apart",
        high - low,
    );
}

/// **Where x really is fractional — the prose lane — the ink moves with it.**
///
/// Four rows of one string in one face at one size, whose rectangles differ
/// only by a quarter of a pixel each ([`FRACTIONAL_ORIGINS`]). A rasterizer
/// placing at sub-pixel precision reproduces those offsets in the ink; one
/// snapping to whole pixels answers in steps of one, and the four rows collapse
/// onto two. The tolerance is a tenth of a pixel: below the quarter being asked
/// for, and above what an eight-bit coverage centroid wobbles by.
///
/// MUTATION: round a paragraph's `rect[0]` before it reaches the shaper and the
/// second row's offset comes back as zero.
#[test]
fn the_prose_lane_draws_at_the_fraction_of_a_pixel_it_was_given() {
    let fixture = GlyphFixture::new(WIDTH, HEIGHT);
    let drawn = draw(fixture);
    let phases = fractional_phases(&drawn.pixels, WIDTH, fixture, drawn.metrics);
    assert_eq!(phases.len(), FRACTIONAL_ORIGINS.len());
    assert!(
        phases.iter().all(|phase| phase.is_finite()),
        "every prose row has ink in it: {phases:?}"
    );
    for (origin, phase) in FRACTIONAL_ORIGINS.iter().zip(&phases) {
        let moved = phase - phases[0];
        assert!(
            (moved - origin).abs() < 0.08,
            "the row asked for {origin}px moved {moved:.4}px — the shaper is not placing at \
             the fraction it was given: {phases:?}"
        );
    }
}

/// **A translucent frame is premultiplied in the space it was blended in**
/// (X-1's contract, §13.14 ① and the question §13.14 ⑥ left open).
///
/// The one thing a premultiplied surface may never hold is a colour above its
/// own alpha: the compositor multiplies nothing, so such a pixel adds light it
/// has no alpha to pay for — which is exactly the too-bright edge X-1 measured
/// when a frame was written the way `PostMultiplied` is named. The blend that
/// writes this surface runs in **linear light**, because that is what an
/// `*UnormSrgb` format means, so that is the reading this case pins. It is
/// checked over the whole surface rather than over the bands, because the clear
/// colour, the cell grounds, the glyph ink and the underlines are four
/// different writers and any one of them could be the one that forgot.
///
/// **The encoded count is printed and not asserted**, and that is the
/// measurement rather than a softened gate: alpha in this format is *not*
/// encoded, so an antialiased edge stores a colour byte above its alpha byte by
/// construction, and pinning that to zero would be pinning the absence of
/// antialiasing. What it means for a compositor that reads those bytes without
/// decoding them is §13.22's business.
///
/// MUTATION: hand the ground pipeline a straight-alpha colour and every ground
/// pixel of the frame counts in both readings.
#[test]
fn a_translucent_page_holds_no_pixel_brighter_than_its_own_alpha() {
    for alpha in [0.3_f32, 0.6] {
        let fixture = GlyphFixture::new(WIDTH, HEIGHT).over_a_ground_worth(alpha);
        let drawn = draw(fixture);
        print!("{}", report(&drawn.pixels, WIDTH, fixture, drawn.metrics));
        let (encoded, linear) = premultiplied_violations(&drawn.pixels);
        assert_eq!(
            linear, 0,
            "at ground alpha {alpha} the frame holds {linear} pixels whose colour, in the \
             linear light the blend ran in, its alpha cannot account for ({encoded} of them \
             are above it as stored bytes, which is the format and not a defect)"
        );
        // And the page really is translucent, so the count above is a fact
        // about a translucent frame and not about an opaque one. The ground is
        // most of the frame, so the alpha byte most of the frame carries is the
        // ground's own — `round(alpha × 255)`, exactly, because that is what a
        // premultiplied clear writes.
        let mut alphas = [0u32; 256];
        for pixel in &drawn.pixels {
            alphas[pixel[3] as usize] += 1;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let want = (alpha * 255.0).round() as u8;
        let mut top: Vec<(u8, u32)> = alphas
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(value, count)| (value as u8, *count))
            .collect();
        top.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        top.truncate(4);
        // The far corner, which `the_fixture_fits_the_surface_it_is_measured_on`
        // has already established is below and to the right of everything the
        // page draws: bare ground, and therefore the clear itself.
        //
        // **Within one, and the one is measured rather than allowed for.** A
        // clear colour is a float and the surface stores eight bits, so the
        // conversion is the hardware's: at a ground worth 0.30 — which is
        // 0.300000012 as an `f32` — this reads **76** through D3D12 and **77**
        // through Metal, two backends rounding the same exact half the two ways
        // it can be rounded. What the page is being asked is whether its clear
        // is the ground's own alpha rather than an opaque window's, and a unit
        // of eight-bit rounding is not an answer to that.
        let corner = drawn.pixels.last().copied().expect("a frame has pixels");
        assert!(
            i32::from(corner[3]).abs_diff(i32::from(want)) <= 1,
            "at ground alpha {alpha} the bare corner of the frame carries alpha {} and a \
             premultiplied clear writes {want}; the frame's alphas are {top:?}",
            corner[3],
        );
        let ground_pixels: u32 = alphas
            [usize::from(want.saturating_sub(1))..=usize::from(want.saturating_add(1))]
            .iter()
            .sum();
        assert!(
            ground_pixels as usize > drawn.pixels.len() / 8,
            "at ground alpha {alpha} only {ground_pixels} pixels of {} carry the clear's own \
             alpha, so this case measured a page with no ground in it: {top:?}",
            drawn.pixels.len(),
        );
    }
}
