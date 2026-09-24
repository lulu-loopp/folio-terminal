//! **One fixed page of glyphs, drawn through the real pipeline, and the
//! arithmetic that reads it back** — M2-5's instrument (`docs/DESIGN.md`
//! §13.22).
//!
//! The port's glyph question is not *which rasterizer*: Folio rasterizes
//! through Swash via glyphon on Windows and on macOS alike, so the letters
//! themselves are the same code on both machines
//! (`docs/plans/port/macos-plan-2026-09-12.md` §R4). What changes on a Mac is
//! the **presentation path** — a `CAMetalLayer` on a view Folio owns, a
//! `PostMultiplied` surface carrying premultiplied pixels (§13.14, X-1). So the
//! only honest way to answer "does the text come out right over there" is to
//! draw one frame down both paths and read both back.
//!
//! That is what this module is for, and its whole shape follows from one rule:
//! **the two paths must be handed the same frame by construction, not by two
//! call sites that look alike.** [`GlyphFixture::present`] is the single
//! builder. An offscreen window reads back through
//! [`crate::WindowRenderer::read_back`]; a real window on a Mac is captured
//! from outside the process, because a swapchain cannot be read. Both arrive
//! here as `[b, g, r, a]` and are measured by the same functions.
//!
//! # What is in the page, and why each row is there
//!
//! [`GLYPH_BANDS`] is the list, one grid row each, and it is deliberately not
//! "some text":
//!
//! * **latin** — the ordinary case, and the one every stem measurement is taken
//!   on.
//! * **cjk** — two-cell clusters out of the fallback chain, which is a
//!   *different face* on each platform and the one row where a missing fallback
//!   shows up as nothing at all rather than as a wrong shape.
//! * **box** — box drawing, which this renderer does **not** rasterize: it is
//!   cell geometry (`crate::procedural`), so this row measures the rectangle
//!   pipeline and is the control for the glyph rows beside it.
//! * **braille** — the other half of that pair: braille *is* a glyph, so a
//!   platform difference that appeared here and not in `box` is the rasterizer
//!   and one that appeared in both is the surface.
//! * **bold**, **italic** — the two synthesised-or-real face variants, where a
//!   platform that lacks a cut of the family synthesises one.
//! * **link** — the prompt's own underline and the dotted underline a
//!   hyperlinked run wears: ink that is not a glyph at all, drawn one physical
//!   pixel high, which is where a scale-2 surface rounds differently from a
//!   scale-1 one if anything is going to.
//! * **stems** — one narrow upright stem in every column, so that a column's
//!   ink can be measured against its own cell origin. [`grid_phases`] reads
//!   them, and what it establishes is that the grid places every cell on a
//!   *whole* pixel: `CellMetrics::measure` ceils both the cell width and the
//!   padding, so the terminal never asks the rasterizer for a fractional phase
//!   at all.
//!
//! Under the grid stands the **prose lane** — four rows of the same stems whose
//! rectangles differ only by [`FRACTIONAL_ORIGINS`] — because that is the one
//! place in this window where an x really is fractional, and the ticket's
//! subpixel question has to be asked where the question exists.
//!
//! # The ground, and the premultiplied contract
//!
//! Every cell carries an **explicit black background and white ink**, so
//! nothing here depends on the theme a process happens to be holding
//! (`crate::theme` is process-global state). At
//! [`GlyphFixture::ground_alpha`] below 1 the cell grounds are premultiplied by
//! that alpha exactly as a translucent window's are, and the frame that comes
//! back is the one a compositor reads: X-1 measured that CoreAnimation blends
//! these bytes premultiplied whatever the wgpu mode is called, so
//! [`premultiplied_violations`] is that contract written as a count of pixels
//! rather than as a sentence.
//!
//! It is **two** counts, and the pair is §13.14 ⑥'s open question rather than a
//! belt and braces. A `Bgra8UnormSrgb` surface stores its three colour channels
//! encoded and its alpha channel linear, so an antialiased edge over a
//! translucent ground stores a colour byte *above* its alpha byte while the
//! blend that wrote it was perfectly premultiplied in linear light.
//! [`linear_violation`] is the contract this renderer's own pipeline owes;
//! [`encoded_violation`] is what a compositor reading the bytes as they stand
//! would see.

use std::num::NonZeroU32;

use bt_transcript::{CapturedCell, CellFlags, CellHyperlink, TerminalColor};
use bt_viewport::horizontal::HorizontalProjection;
use bt_viewport::{FrameViewportOrigin, ViewportFrame};

use crate::{
    CellMetrics, FrameSource, FrameTrigger, GpuContext, PresentOutcome, PreviewBody,
    PreviewParagraph, PreviewQuad, PreviewRun, RenderError, SeatFrame, SeatViewport,
    WindowRenderer,
};

/// One class of ink and the grid row it is drawn on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlyphBand {
    /// The name a report prints.
    pub name: &'static str,
    /// Which row of [`GlyphFixture`]'s grid it occupies.
    pub row: u32,
    /// What the row is written in.
    pub text: &'static str,
    /// Whether the row's ink comes out of the rasterizer at all. `false` for
    /// the two rows this renderer draws as cell geometry.
    pub rasterized: bool,
}

/// The page, row by row. The order is the order they are drawn in.
pub const GLYPH_BANDS: [GlyphBand; 8] = [
    GlyphBand {
        name: "latin",
        row: 0,
        text: "The quick brown fox 0123456789",
        rasterized: true,
    },
    GlyphBand {
        name: "cjk",
        row: 1,
        text: "网页预览需要运行时",
        rasterized: true,
    },
    GlyphBand {
        name: "box",
        row: 2,
        text: "┌─┬─┐├─┼─┤└─┴─┘│││",
        rasterized: false,
    },
    GlyphBand {
        name: "braille",
        row: 3,
        text: "⠁⠃⠇⠏⠟⠿⡿⣿⢸⡇⠶⠿",
        rasterized: true,
    },
    GlyphBand {
        name: "bold",
        row: 4,
        text: "Handgloves 0123 BOLD",
        rasterized: true,
    },
    GlyphBand {
        name: "italic",
        row: 5,
        text: "Handgloves 0123 ital",
        rasterized: true,
    },
    GlyphBand {
        name: "link",
        row: 6,
        text: "https://example.com/a",
        rasterized: true,
    },
    GlyphBand {
        name: "stems",
        row: 7,
        text: "",
        rasterized: true,
    },
];

/// What the `stems` row and the four prose rows under it are filled with: a
/// narrow upright stem, which is the character whose ink centroid moves least
/// for reasons other than where it was placed.
const STEM_CHARACTER: char = 'l';

/// The name of the grid band that row fills, said once so that the fixture and
/// the measurement cannot drift apart on a string literal.
const STEM_BAND: &str = "stems";

/// **The four fractional origins the prose lane is drawn at**, below the grid.
///
/// The grid cannot answer the subpixel question: `CellMetrics::measure` takes
/// `primary_advance_px.ceil()` for the cell width and `(8 × scale).ceil()` for
/// the padding, so `padding + column × cell_width_px` is a **whole number in
/// every column** and a terminal never asks the rasterizer for a fractional
/// phase at all. The prose lane does — a paragraph's `rect` is wherever the
/// document solver put it — so the question is asked where it exists, with the
/// same character, the same face and the same size, one row each.
pub const FRACTIONAL_ORIGINS: [f32; 4] = [0.0, 0.25, 0.5, 0.75];

/// How many stems each of those rows carries.
const FRACTIONAL_STEMS: usize = 20;

/// How many columns the fixture's grid is.
pub const FIXTURE_COLUMNS: u32 = 34;

/// The ink every row is written in, and the ground every cell declares.
const INK: TerminalColor = TerminalColor::Rgb(255, 255, 255);
const GROUND: TerminalColor = TerminalColor::Rgb(0, 0, 0);

/// **The one frame M2-5 measures**, and the only place it is built.
#[derive(Clone, Copy, Debug)]
pub struct GlyphFixture {
    /// The surface, in physical pixels.
    pub width: u32,
    /// The surface, in physical pixels.
    pub height: u32,
    /// What the window's ground is worth. `1.0` is an opaque window; anything
    /// less is the translucent contract X-1 measured, and is what
    /// [`premultiplied_violations`] is asked about.
    pub ground_alpha: f32,
}

impl GlyphFixture {
    /// The page at the size every gate in this workspace draws it.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ground_alpha: 1.0,
        }
    }

    /// The same page over a translucent ground.
    #[must_use]
    pub const fn over_a_ground_worth(mut self, alpha: f32) -> Self {
        self.ground_alpha = alpha;
        self
    }

    /// Build the frame. Public because the Mac's window test presents it into a
    /// swapchain and the offscreen gate presents it into a texture, and the two
    /// have to be the same frame.
    #[must_use]
    pub fn frame(&self, metrics: CellMetrics) -> ViewportFrame {
        let columns = FIXTURE_COLUMNS;
        let rows = GLYPH_BANDS.len() as u32;
        let mut cells = Vec::with_capacity((columns * rows) as usize);
        for band in GLYPH_BANDS {
            let mut written = 0u32;
            if band.name == STEM_BAND {
                for _ in 0..columns {
                    cells.push(styled(
                        &STEM_CHARACTER.to_string(),
                        CellFlags::empty(),
                        None,
                    ));
                }
                continue;
            }
            for cluster in bt_unicode::graphemes(band.text) {
                let wide = bt_unicode::cluster_width(cluster) == 2;
                if written + if wide { 2 } else { 1 } > columns {
                    break;
                }
                let mut flags = CellFlags::empty();
                match band.name {
                    "bold" => flags.insert(CellFlags::BOLD),
                    "italic" => flags.insert(CellFlags::ITALIC),
                    "link" => flags.insert(CellFlags::UNDERLINE | CellFlags::DOTTED_UNDERLINE),
                    _ => {}
                }
                if wide {
                    flags.insert(CellFlags::WIDE_CHAR);
                }
                let link =
                    (band.name == "link").then(|| CellHyperlink::implicit("https://example.com/a"));
                cells.push(styled(cluster, flags, link));
                written += 1;
                if wide {
                    let mut spacer = styled("", CellFlags::empty(), None);
                    spacer.wide_spacer = true;
                    cells.push(spacer);
                    written += 1;
                }
            }
            for _ in written..columns {
                cells.push(styled(" ", CellFlags::empty(), None));
            }
        }
        let height_subpixels =
            (metrics.cell_height_px * bt_viewport::SUBPIXELS_PER_PX as f32).round() as i64;
        ViewportFrame {
            columns: NonZeroU32::new(columns).expect("the fixture has columns"),
            horizontal: HorizontalProjection::unscrolled(columns),
            grid_rows: NonZeroU32::new(rows).expect("the fixture has rows"),
            rows: NonZeroU32::new(rows).expect("the fixture has rows"),
            presentation_offset_subpixels: 0,
            cell_anchors: (0..cells.len())
                .map(|index| {
                    let anchor = bt_doc::ContentAnchor::Live {
                        screen: bt_doc::ScreenId::Primary,
                        point: bt_doc::GridPoint {
                            row: index as u32 / columns,
                            column: index as u32 % columns,
                        },
                        bias: bt_doc::Bias::Before,
                        generation: bt_doc::GridGeneration(1),
                    };
                    bt_viewport::CellAnchor {
                        start: anchor.clone(),
                        end: anchor,
                    }
                })
                .collect(),
            cells,
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                // **Off.** A caret is the one mark on this page that would make
                // two runs of the same fixture differ, because it blinks.
                visible: false,
            },
            row_map: (0..rows)
                .map(|row| bt_viewport::FrameVisualRow {
                    top_subpixels: i64::from(row) * height_subpixels,
                    height_subpixels,
                    live_grid_row: Some(row),
                    continues: false,
                    source_ends: None,
                })
                .collect(),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc::LayoutKey {
                width_cells: NonZeroU32::new(columns).expect("the fixture has columns"),
                dpi_milli: metrics.dpi_milli(),
                font_size_subpixels: 16 * 1024,
                font_rev: 1,
                theme_rev: 1,
                lang_rev: 0,
                profile_rev: 0,
                line_wrapping: false,
            },
            view_generation: bt_doc::ViewGeneration(1),
        }
    }

    /// **Draw it.** The window may be an offscreen texture or a real
    /// swapchain; nothing below this line knows which.
    ///
    /// # Why this call is a critical section
    ///
    /// The ground is set process-wide for the duration and put back afterwards,
    /// because that is where this renderer keeps it
    /// ([`crate::set_window_ground`]) and because a fixture that left it moved
    /// would be changing what the *next* thing drawn looks like. That makes two
    /// concurrent callers a race with a very quiet failure: the frame path reads
    /// the ground's alpha more than once — the clear has its own read — so a
    /// draw that straddles somebody else's set and restore comes back as a
    /// **mixed** frame, cell grounds at one alpha and the clear at another, and
    /// every number measured off it is about no window at all.
    ///
    /// Measured, and it is the reason this lock is here rather than a rule in a
    /// test's comment: run under libtest's default parallelism the fixture drew
    /// exactly that frame on the Mac — 212 887 pixels of cell ground at
    /// `round(0.3 × 255)` and 458 717 pixels of clear at 255 — while the same
    /// cases at `--test-threads=1` on Windows had never shown it.
    pub fn present(
        &self,
        gpu: &mut GpuContext,
        window: &mut WindowRenderer,
    ) -> Result<PresentOutcome, RenderError> {
        static ONE_GROUND_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _held = ONE_GROUND_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let restore = crate::window_ground();
        let _ = crate::set_window_ground(crate::WindowGround {
            alpha: self.ground_alpha,
            ..crate::WindowGround::opaque()
        });
        let metrics = window.base_metrics();
        let frame = self.frame(metrics);
        let seat = SeatViewport::whole(self.width, self.height);
        let _ = window.set_seat_viewport(seat);
        let _ = window.set_preview_bodies(vec![self.prose(metrics)]);
        let outcome = window.present_frame(
            gpu,
            &[SeatFrame {
                seat,
                clip: seat,
                frame: &frame,
                metrics,
                focused: true,
            }],
            FrameTrigger {
                occurred_at: std::time::Instant::now(),
                source: FrameSource::Expose,
            },
        );
        let _ = crate::set_window_ground(restore);
        outcome
    }

    /// The rectangle a band's ink may land in — `[left, top, right, bottom]` in
    /// physical pixels, half-open at the right and the bottom.
    ///
    /// The grid's own arithmetic, said once: a cell is at
    /// `padding + column × cell_width_px, padding + row × cell_height_px`.
    #[must_use]
    pub fn band_rect(&self, metrics: CellMetrics, band: GlyphBand) -> [u32; 4] {
        let left = metrics.padding_px;
        let right = left + FIXTURE_COLUMNS as f32 * metrics.cell_width_px;
        let top = metrics.padding_px + band.row as f32 * metrics.cell_height_px;
        let bottom = top + metrics.cell_height_px;
        [
            left.floor().max(0.0) as u32,
            top.floor().max(0.0) as u32,
            (right.ceil() as u32).min(self.width),
            (bottom.ceil() as u32).min(self.height),
        ]
    }
}

/// **The prose lane's four rows, at four fractional origins** — see
/// [`FRACTIONAL_ORIGINS`] for why the question cannot be asked of the grid.
///
/// They stand under the grid on the same row pitch, over a black quad of their
/// own so that white ink is measured against the same ground the cells above it
/// declare rather than against whatever the theme's paper happens to be.
impl GlyphFixture {
    #[must_use]
    pub fn prose(&self, metrics: CellMetrics) -> PreviewBody {
        let text: String = std::iter::repeat_n(STEM_CHARACTER, FRACTIONAL_STEMS).collect();
        let top = self.prose_top(metrics);
        let bottom = top + FRACTIONAL_ORIGINS.len() as f32 * metrics.cell_height_px;
        PreviewBody {
            clip: [0.0, top, self.width as f32, bottom],
            quads: vec![PreviewQuad {
                rect: [0.0, top, self.width as f32, bottom],
                color: [0, 0, 0],
            }],
            paragraphs: FRACTIONAL_ORIGINS
                .iter()
                .enumerate()
                .map(|(index, origin)| PreviewParagraph {
                    runs: vec![PreviewRun {
                        text: text.clone(),
                        color: [255, 255, 255],
                        mono: true,
                        bold: false,
                        italic: false,
                        font_scale: 1.0,
                        inline_box_px: None,
                    }],
                    rect: self.prose_rect(metrics, index, *origin),
                    font_size_px: metrics.font_size_px,
                    line_height_px: metrics.cell_height_px,
                    wrap: false,
                    letter_spacing_em: 0.0,
                    align_right: false,
                    align_center: false,
                    cell_advance: None,
                })
                .collect(),
            blocks: Vec::new(),
            rasters: Vec::new(),
        }
    }

    /// The top of the prose lane: straight under the last grid band.
    #[must_use]
    pub fn prose_top(&self, metrics: CellMetrics) -> f32 {
        metrics.padding_px + GLYPH_BANDS.len() as f32 * metrics.cell_height_px
    }

    /// One prose row's rectangle, `origin` pixels to the right of the grid's
    /// own left edge.
    #[must_use]
    pub fn prose_rect(&self, metrics: CellMetrics, index: usize, origin: f32) -> [f32; 4] {
        let top = self.prose_top(metrics) + index as f32 * metrics.cell_height_px;
        [
            metrics.padding_px + origin,
            top,
            self.width as f32 - metrics.padding_px,
            top + metrics.cell_height_px,
        ]
    }
}

/// **Where the prose lane's ink actually landed**, one absolute x per row.
///
/// The four rows are one string, one face and one size, differing only by
/// [`FRACTIONAL_ORIGINS`]. So the differences between these numbers are the
/// whole answer: a rasterizer positioning at sub-pixel precision reproduces the
/// offsets, and one snapping to whole pixels answers in steps of one.
#[must_use]
pub fn fractional_phases(
    pixels: &[[u8; 4]],
    width: u32,
    fixture: GlyphFixture,
    metrics: CellMetrics,
) -> Vec<f32> {
    (0..FRACTIONAL_ORIGINS.len())
        .map(|index| {
            let rect = fixture.prose_rect(metrics, index, 0.0);
            let top = rect[1].floor().max(0.0) as u32;
            let bottom = (rect[3].ceil() as u32).min(fixture.height);
            let left = rect[0].floor().max(0.0) as u32;
            let right = (rect[2].ceil() as u32).min(width);
            let mut weight = 0.0f64;
            let mut moment = 0.0f64;
            for y in top..bottom {
                for x in left..right {
                    let Some([b, g, r, _]) = pixels.get((y * width + x) as usize) else {
                        continue;
                    };
                    let ink = f64::from(linear_from_srgb_byte(*b.max(g).max(r)));
                    if ink <= 0.0005 {
                        continue;
                    }
                    weight += ink;
                    moment += ink * (f64::from(x) + 0.5);
                }
            }
            if weight > 0.0 {
                (moment / weight) as f32
            } else {
                f32::NAN
            }
        })
        .collect()
}

fn styled(text: &str, flags: CellFlags, link: Option<CellHyperlink>) -> CapturedCell {
    let mut cell = CapturedCell::plain(text);
    cell.style.flags = flags;
    cell.style.foreground = INK;
    cell.style.background = GROUND;
    cell.hyperlink = link;
    cell
}

/// What one band's pixels say. Every number here is comparable between two
/// machines that draw the page in two different faces, which is the whole point
/// of measuring shapes of distributions rather than bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BandStats {
    /// Pixels in the band that carry any ink at all.
    pub ink_pixels: u64,
    /// Pixels the band holds altogether.
    pub total_pixels: u64,
    /// The strongest coverage anywhere in the band, `0.0..=1.0`. A stem's
    /// interior, on any face at scale 2.
    pub peak_coverage: f32,
    /// The mean coverage over the inked pixels.
    pub mean_coverage: f32,
    /// The share of inked pixels that are neither nearly clear nor nearly
    /// solid — the antialiased skirt. Zero would mean a bilevel rasterizer.
    pub antialiased_share: f32,
    /// Pixels whose stored colour **byte** exceeds their own alpha byte.
    ///
    /// Not a defect on its own, and the number M2-5 exists to have: an
    /// `*UnormSrgb` surface encodes its three colour channels and leaves alpha
    /// linear, so an antialiased edge over a translucent ground stores a colour
    /// byte above its alpha byte *by construction*. It matters because a
    /// compositor that reads those bytes as premultiplied without decoding them
    /// — which is what X-1 measured CoreAnimation doing — then draws that edge
    /// brighter than the arithmetic says.
    pub premultiplied_violations_encoded: u64,
    /// Pixels whose colour exceeds their own alpha **once the colour is decoded
    /// to linear light**, which is the space the GPU ran the blend in and the
    /// one thing this renderer's own pipeline may never produce.
    pub premultiplied_violations_linear: u64,
}

/// Read one band out of a readback.
///
/// `pixels` is `[b, g, r, a]` in rows of `width`, which is what
/// [`crate::WindowRenderer::read_back`] answers and what a window capture is
/// converted to.
///
/// **Coverage is read off the encoded byte, not off linear light**, and that is
/// deliberate: the surface is an `*UnormSrgb` format, the blend the GPU ran is
/// on the linear values behind it, and what a reader's eye meets is the byte.
/// The number this returns is therefore the gamma-carrying one — the one that
/// differs if a platform ever encodes the same coverage differently — and the
/// caller that wants linear light can undo it.
#[must_use]
pub fn measure_band(pixels: &[[u8; 4]], width: u32, rect: [u32; 4]) -> BandStats {
    let mut stats = BandStats::default();
    let mut coverage_sum = 0.0f64;
    let mut skirt = 0u64;
    for y in rect[1]..rect[3] {
        for x in rect[0]..rect[2] {
            let Some(pixel) = pixels.get((y * width + x) as usize) else {
                continue;
            };
            stats.total_pixels += 1;
            let [b, g, r, _] = *pixel;
            if encoded_violation(pixel) {
                stats.premultiplied_violations_encoded += 1;
            }
            if linear_violation(pixel) {
                stats.premultiplied_violations_linear += 1;
            }
            // The ground is black and the ink is white, so a pixel's coverage
            // is how far its brightest channel has risen off the ground. The
            // alpha is the ground's own and carries no glyph.
            let coverage = f32::from(b.max(g).max(r)) / 255.0;
            if coverage <= 0.004 {
                continue;
            }
            stats.ink_pixels += 1;
            coverage_sum += f64::from(coverage);
            stats.peak_coverage = stats.peak_coverage.max(coverage);
            if (0.05..0.95).contains(&coverage) {
                skirt += 1;
            }
        }
    }
    if stats.ink_pixels > 0 {
        stats.mean_coverage = (coverage_sum / stats.ink_pixels as f64) as f32;
        stats.antialiased_share = skirt as f32 / stats.ink_pixels as f32;
    }
    stats
}

/// **One byte of an `*UnormSrgb` surface, back in linear light.**
///
/// The standard sRGB transfer function, written out rather than approximated by
/// a power, because the whole point of the number it feeds is an exact
/// comparison against an alpha channel that was never encoded at all.
#[must_use]
pub fn linear_from_srgb_byte(byte: u8) -> f32 {
    let value = f32::from(byte) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Whether this pixel's stored colour byte is above its stored alpha byte. See
/// [`BandStats::premultiplied_violations_encoded`].
#[must_use]
pub fn encoded_violation(pixel: &[u8; 4]) -> bool {
    let [b, g, r, a] = *pixel;
    u32::from(b.max(g).max(r)) > u32::from(a) + 1
}

/// Whether this pixel's colour is above its alpha **in linear light**, which is
/// the space the blend that wrote it ran in. One 255th of slack, because the
/// surface stores eight bits and the blend did not.
#[must_use]
pub fn linear_violation(pixel: &[u8; 4]) -> bool {
    let [b, g, r, a] = *pixel;
    linear_from_srgb_byte(b.max(g).max(r)) > f32::from(a) / 255.0 + 1.0 / 255.0
}

/// How many pixels of a whole readback break each of the two readings above.
#[must_use]
pub fn premultiplied_violations(pixels: &[[u8; 4]]) -> (u64, u64) {
    (
        pixels
            .iter()
            .filter(|pixel| encoded_violation(pixel))
            .count() as u64,
        pixels
            .iter()
            .filter(|pixel| linear_violation(pixel))
            .count() as u64,
    )
}

/// A stable fingerprint of a readback, for "are these two frames the same
/// frame".
///
/// FNV-1a over the bytes, because the question is equality and a hash that is
/// written down here answers it in one number a log can carry across a
/// machine boundary. A difference is then chased with
/// [`first_difference`].
#[must_use]
pub fn digest(pixels: &[[u8; 4]]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for pixel in pixels {
        for byte in pixel {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// The first pixel two readbacks disagree about, as `(index, left, right)`.
#[must_use]
pub fn first_difference(left: &[[u8; 4]], right: &[[u8; 4]]) -> Option<(usize, [u8; 4], [u8; 4])> {
    left.iter()
        .zip(right)
        .enumerate()
        .find(|(_, (a, b))| a != b)
        .map(|(index, (a, b))| (index, *a, *b))
}

/// **Where each column's ink actually sits inside its own cell**, in pixels
/// from the cell's left edge.
///
/// The `stems` band is the same character in every column, so every entry here
/// is the same raster measured against its own cell origin. The *spread* is the
/// answer: the grid's origins are whole numbers by construction (see
/// [`FRACTIONAL_ORIGINS`]), so a spread of zero is this renderer working and
/// anything else means a cell moved off its column.
#[must_use]
pub fn grid_phases(pixels: &[[u8; 4]], width: u32, metrics: CellMetrics) -> Vec<f32> {
    let band = *GLYPH_BANDS
        .iter()
        .find(|band| band.name == STEM_BAND)
        .expect("the page has a stem band");
    let top = (metrics.padding_px + band.row as f32 * metrics.cell_height_px).floor() as u32;
    let bottom = (top as f32 + metrics.cell_height_px).ceil() as u32;
    let mut phases = Vec::new();
    for column in 0..FIXTURE_COLUMNS {
        let origin = metrics.padding_px + column as f32 * metrics.cell_width_px;
        let left = origin.floor() as u32;
        let right = (origin + metrics.cell_width_px).ceil() as u32;
        let mut weight = 0.0f64;
        let mut moment = 0.0f64;
        for y in top..bottom {
            for x in left..right.min(width) {
                let Some([b, g, r, _]) = pixels.get((y * width + x) as usize) else {
                    continue;
                };
                let ink = f64::from(linear_from_srgb_byte(*b.max(g).max(r)));
                if ink <= 0.0005 {
                    continue;
                }
                weight += ink;
                // The centre of the pixel, which is where its ink is.
                moment += ink * (f64::from(x) + 0.5 - f64::from(origin));
            }
        }
        if weight > 0.0 {
            phases.push((moment / weight) as f32);
        }
    }
    phases
}

/// One line per band, in the shape a report and a log both read.
#[must_use]
pub fn report(
    pixels: &[[u8; 4]],
    width: u32,
    fixture: GlyphFixture,
    metrics: CellMetrics,
) -> String {
    let mut out = String::new();
    for band in GLYPH_BANDS {
        let rect = fixture.band_rect(metrics, band);
        let stats = measure_band(pixels, width, rect);
        out.push_str(&format!(
            "BT_GLYPH band={} rasterized={} ink={} peak={:.4} mean={:.4} aa={:.4} \
             encoded_over_alpha={} linear_over_alpha={}\n",
            band.name,
            band.rasterized,
            stats.ink_pixels,
            stats.peak_coverage,
            stats.mean_coverage,
            stats.antialiased_share,
            stats.premultiplied_violations_encoded,
            stats.premultiplied_violations_linear,
        ));
    }
    let phases = grid_phases(pixels, width, metrics);
    let low = phases.iter().copied().fold(f32::INFINITY, f32::min);
    let high = phases.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    out.push_str(&format!(
        "BT_GLYPH grid columns={} cell_w={:.3} padding={:.3} phase_min={low:.4} \
         phase_max={high:.4} spread={:.4}\n",
        phases.len(),
        metrics.cell_width_px,
        metrics.padding_px,
        high - low,
    ));
    let fractional = fractional_phases(pixels, width, fixture, metrics);
    for (index, origin) in FRACTIONAL_ORIGINS.iter().enumerate() {
        let rect = fixture.prose_rect(metrics, index, *origin);
        let stats = measure_band(
            pixels,
            width,
            [
                rect[0].floor() as u32,
                rect[1].floor() as u32,
                (rect[2].ceil() as u32).min(width),
                (rect[3].ceil() as u32).min(fixture.height),
            ],
        );
        out.push_str(&format!(
            "BT_GLYPH prose origin={origin:.2} left={:.2} ink={} centre={:.4} moved={:+.4}
",
            rect[0],
            stats.ink_pixels,
            fractional[index],
            fractional[index] - fractional[0],
        ));
    }
    let (encoded, linear) = premultiplied_violations(pixels);
    out.push_str(&format!(
        "BT_GLYPH frame digest={:016x} alpha={:.2} encoded_over_alpha={encoded} \
         linear_over_alpha={linear}\n",
        digest(pixels),
        fixture.ground_alpha,
    ));
    out
}
