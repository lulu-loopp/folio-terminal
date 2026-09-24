//! **A bold grid cell whose face has no bold cut, drawn heavier from that face's
//! own outline** (0.4.5 ticket 38; `docs/DESIGN.md` 2026-09-24).
//!
//! The family that draws a cell never depends on the weight asked
//! ([`crate::cjk_fonts::match_grid_attrs`]), so a bold request on a family with
//! no bold cut is shaped from the family's regular face. cosmic-text's rasterizer
//! has a synthetic slant (`FAKE_ITALIC`) and no synthetic weight, and glyphon
//! rasterizes every text glyph itself, so the heavier raster is drawn through
//! glyphon's one open door: a **custom glyph**, rasterized here from the same
//! face, glyph and subpixel position the shaper chose, with swash's
//! `Render::embolden`. The shaper and the pen are untouched; only the ink is.
//!
//! # The three obligations the door brings
//!
//! 1. **Every prepare against the shared atlas carries [`SyntheticBoldGlyphs`]'s
//!    rasterizer**, not only the grid's. glyphon keeps a custom raster in its
//!    atlas cache after it stops being used, and when *any* prepare grows the
//!    atlas it redraws every entry through that prepare's callback and panics on
//!    a `None`.
//! 2. **An id is retired only when the atlas is replaced.** Ids are glyphon's
//!    `u16`; the table that gives them out is emptied by
//!    [`crate::GpuContext`]'s one atlas-replacing door and by nothing else, so an
//!    id never names two rasters in one atlas. When the ids run out the lane's
//!    prepare is refused as a full atlas, which is the repack episode's own road
//!    (`GpuContext::close_the_frame`).
//! 3. **The glyph census counts custom glyphs**, since a synthesized cell's text
//!    area carries no buffer glyph for it.
use super::*;

use glyphon::cosmic_text::{CacheKey, CacheKeyFlags};
use glyphon::{ContentType, CustomGlyph, RasterizeCustomGlyphRequest, RasterizedCustomGlyph};
use std::ops::Range;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Transform, Vector};

/// **How far a synthetic bold moves an outline, as a fraction of the em.**
///
/// swash's embolden is a port of FreeType's `FT_Outline_EmboldenXY` without its
/// halving: every point moves by this distance on each axis plus its corner's
/// bisector shift of the same size, so a stroke thickens by twice it and the
/// ink box grows by twice it to the right and to the top and not at all to the
/// left or the bottom — the pen and the baseline stay where the regular glyph
/// has them. Twice em/48 is em/24, FreeType's own `FT_GlyphSlot_Embolden`
/// thickening. Measured on the checked-in rectangle fonts
/// (`synthetic_bold_ink_stays_inside_its_cell_tolerance`): at 8 logical px the
/// box grows by one pixel at the top and none to the right, at 72 px × 2 by six
/// on both, and at every size from 8 to 72 logical px at scales 1 and 2 the
/// grown ink stays inside the two-cell slot a wide cell's text area clips to.
pub(crate) const SYNTHETIC_BOLD_STRENGTH_EM: f32 = 1.0 / 48.0;

/// The weight below which a face is not bold. A face at 600 or above answers a
/// bold request with its own strokes; one below it is regular or medium.
const BOLD_FACE_WEIGHT_FLOOR: u16 = 600;

/// How many ids glyphon's custom-glyph key has room for.
const SYNTHETIC_BOLD_ID_SPACE: usize = glyphon::CustomGlyphId::MAX as usize + 1;

/// **The one function that makes a synthesized raster.**
///
/// cosmic-text's `swash_image` step for step — the face's `wght` coordinate,
/// the hint flag, the subpixel offset, the three sources in the same order, the
/// 14° skew for `FAKE_ITALIC` — with one addition: `embolden`. swash applies it
/// to the outline before the skew, so a bold-italic cell in a regular-only
/// family is slanted *and* heavier, and it applies it to `Source::Outline` only,
/// so a colour glyph comes out exactly as it would have. With `embolden` false
/// this is the regular raster, which is how the one glyph of a mixed cluster
/// that has a bold face of its own is drawn beside one that does not.
pub(crate) fn synthetic_bold_image(
    context: &mut ScaleContext,
    font: &glyphon::Font,
    key: CacheKey,
    embolden: bool,
) -> Option<glyphon::SwashImage> {
    let face = font.as_swash();
    let wght = swash::Tag::from_be_bytes(*b"wght");
    let size = f32::from_bits(key.font_size_bits);
    let mut builder = context
        .builder(face)
        .size(size)
        .hint(!key.flags.contains(CacheKeyFlags::DISABLE_HINTING));
    if let Some(axis) = face.variations().find_by_tag(wght) {
        builder = builder.normalized_coords(face.variations().normalized_coords([(
            wght,
            f32::from(key.font_weight.0).clamp(axis.min_value(), axis.max_value()),
        )]));
    }
    let mut scaler = builder.build();
    let offset = if key.flags.contains(CacheKeyFlags::PIXEL_FONT) {
        Vector::new(key.x_bin.as_float().round(), key.y_bin.as_float().round())
    } else {
        Vector::new(key.x_bin.as_float(), key.y_bin.as_float())
    };
    Render::new(&[
        Source::ColorOutline(0),
        Source::ColorBitmap(StrikeWith::BestFit),
        Source::Outline,
    ])
    .format(Format::Alpha)
    .offset(offset)
    .embolden(if embolden {
        SYNTHETIC_BOLD_STRENGTH_EM * size
    } else {
        0.0
    })
    .transform(
        key.flags
            .contains(CacheKeyFlags::FAKE_ITALIC)
            .then(|| Transform::skew(Angle::from_degrees(14.0), Angle::from_degrees(0.0))),
    )
    .render(&mut scaler, key.glyph_id)
}

/// **Which glyphs of a shaped cell are drawn emboldened** — decided once, at the
/// shaping cache's miss, and stored with the shape.
///
/// `None` unless the cell asks bold and is not a colour emoji, and unless at
/// least one of its glyphs comes from a face with no bold of its own: a static
/// face below [`BOLD_FACE_WEIGHT_FLOOR`] whose `wght` axis, if it has one, does
/// not reach bold (a variable face already gets its bold through the axis). The
/// flag is the key's `bold`, not the glyph's weight: the grid's weight match
/// hands the shaper the face's own weight, so a bold `你` in NSimSun is shaped
/// at 400.
pub(crate) fn synthetic_bold_glyphs(
    bold: bool,
    color_emoji: bool,
    buffer: &Buffer,
    font_system: &FontSystem,
) -> Option<Arc<[bool]>> {
    if !bold || color_emoji {
        return None;
    }
    let db = font_system.db();
    let flags: Vec<bool> = buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter())
        .map(|glyph| {
            db.face(glyph.font_id).is_some_and(|face| {
                face.weight.0 < BOLD_FACE_WEIGHT_FLOOR
                    && !cjk_fonts::wght_axis_reaches(db, face.id, Weight::BOLD)
            })
        })
        .collect();
    flags.contains(&true).then(|| Arc::from(flags))
}

/// The ids of one atlas's synthesized rasters were all given out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyntheticBoldIdsExhausted;

/// One synthesized raster: what it is drawn from, and where its ink sits
/// relative to the pen, measured when its id was given out.
struct SyntheticBoldEntry {
    font: Arc<glyphon::Font>,
    key: CacheKey,
    embolden: bool,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

/// What a synthesized raster answers a placement with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SyntheticRaster {
    pub id: glyphon::CustomGlyphId,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

/// **The custom-glyph ids of the shared atlas, and how to draw each of them
/// again.** Owned by [`crate::GpuContext`] beside its `TextAtlas` and emptied
/// with it (see the module's obligation 2).
pub(crate) struct SyntheticBoldGlyphs {
    ids: HashMap<(CacheKey, bool), glyphon::CustomGlyphId>,
    entries: Vec<SyntheticBoldEntry>,
    ceiling: usize,
    context: ScaleContext,
}

impl SyntheticBoldGlyphs {
    pub(crate) fn new() -> Self {
        Self {
            ids: HashMap::new(),
            entries: Vec::new(),
            ceiling: SYNTHETIC_BOLD_ID_SPACE,
            context: ScaleContext::new(),
        }
    }

    /// Every id given out, retired together — called only where the atlas that
    /// could hold them is replaced.
    pub(crate) fn retire_all(&mut self) {
        self.ids.clear();
        self.entries.clear();
    }

    /// How many ids are out.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The face every id given out is drawn from.
    #[cfg(test)]
    pub(crate) fn faces(&self) -> Vec<glyphon::fontdb::ID> {
        self.entries.iter().map(|entry| entry.key.font_id).collect()
    }

    /// A smaller id space, so a test can reach the end of it (as the soaks reach
    /// a device's texture roof through `headless_under_a_texture_ceiling`).
    #[cfg(test)]
    pub(crate) fn set_ceiling(&mut self, ceiling: usize) {
        self.ceiling = ceiling.min(SYNTHETIC_BOLD_ID_SPACE);
    }

    /// The id and placement of the raster for `key`, drawn emboldened or not,
    /// giving out an id the first time it is asked for. `Ok(None)` for a glyph
    /// whose face cannot be loaded or that has no ink.
    pub(crate) fn raster(
        &mut self,
        font_system: &mut FontSystem,
        key: CacheKey,
        embolden: bool,
    ) -> Result<Option<SyntheticRaster>, SyntheticBoldIdsExhausted> {
        let id = match self.ids.get(&(key, embolden)) {
            Some(id) => *id,
            None => {
                if self.entries.len() >= self.ceiling {
                    return Err(SyntheticBoldIdsExhausted);
                }
                let Some(font) = font_system.get_font(key.font_id, key.font_weight) else {
                    return Ok(None);
                };
                let placement = synthetic_bold_image(&mut self.context, &font, key, embolden)
                    .map(|image| image.placement)
                    .unwrap_or_default();
                let id = glyphon::CustomGlyphId::try_from(self.entries.len())
                    .map_err(|_| SyntheticBoldIdsExhausted)?;
                self.entries.push(SyntheticBoldEntry {
                    font,
                    key,
                    embolden,
                    left: placement.left,
                    top: placement.top,
                    width: placement.width,
                    height: placement.height,
                });
                self.ids.insert((key, embolden), id);
                id
            }
        };
        let entry = &self.entries[usize::from(id)];
        Ok(
            (entry.width > 0 && entry.height > 0).then_some(SyntheticRaster {
                id,
                left: entry.left,
                top: entry.top,
                width: entry.width,
                height: entry.height,
            }),
        )
    }

    /// glyphon's custom-glyph callback: draw id `request.id` again, for a first
    /// upload or for an atlas that grew. Answers for every id this table gave
    /// out and still holds, which is every id the atlas it lives beside can hold.
    pub(crate) fn rasterize(
        &mut self,
        request: RasterizeCustomGlyphRequest,
    ) -> Option<RasterizedCustomGlyph> {
        let entry = self.entries.get(usize::from(request.id))?;
        let image =
            synthetic_bold_image(&mut self.context, &entry.font, entry.key, entry.embolden)?;
        if image.placement.width != u32::from(request.width)
            || image.placement.height != u32::from(request.height)
        {
            return None;
        }
        Some(RasterizedCustomGlyph {
            data: image.data,
            content_type: match image.content {
                glyphon::SwashContent::Color => ContentType::Color,
                glyphon::SwashContent::Mask | glyphon::SwashContent::SubpixelMask => {
                    ContentType::Mask
                }
            },
        })
    }
}

/// One cell's text area as the grid lays it out, before synthesis decides what
/// it carries: its regular buffer, and which of its glyphs are emboldened.
pub(crate) struct GridCellArea<'a> {
    pub area: TextArea<'a>,
    pub synthetic_bold: Option<&'a [bool]>,
}

/// One frame's synthesized glyphs for one lane, one entry per cell in the order
/// the cells were laid out.
pub(crate) struct SyntheticPlacements {
    glyphs: Vec<CustomGlyph>,
    cells: Vec<Option<Range<usize>>>,
    /// The buffer a synthesized cell's text area carries: nothing, so glyphon
    /// draws no regular glyph under the custom ones.
    empty: Buffer,
}

impl SyntheticPlacements {
    /// Custom glyphs placed for this lane — what the census and a test read.
    #[cfg(test)]
    pub(crate) fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }
}

/// **Place every synthesized glyph of one lane**, exactly where glyphon would
/// have drawn the regular one.
///
/// glyphon draws a text glyph at `physical.x + placement.left`,
/// `round(line_y × scale) + physical.y − placement.top`, where `physical` is
/// cosmic-text's `LayoutGlyph::physical` against the area's corner and the
/// raster was drawn at the subpixel offset `physical` binned. The custom glyph
/// is given that same whole-pixel corner and snapped, so glyphon's rounding of
/// `area.left + left × scale` lands on it with no half-pixel drift, and its
/// raster is drawn at the same offset (the offset is part of its key).
pub(crate) fn place_synthetic_bold<'a>(
    cells: impl IntoIterator<Item = GridCellArea<'a>>,
    table: &mut SyntheticBoldGlyphs,
    font_system: &mut FontSystem,
) -> Result<SyntheticPlacements, SyntheticBoldIdsExhausted> {
    let mut placements = SyntheticPlacements {
        glyphs: Vec::new(),
        cells: Vec::new(),
        empty: Buffer::new_empty(Metrics::new(1.0, 1.0)),
    };
    for cell in cells {
        let Some(flags) = cell.synthetic_bold else {
            placements.cells.push(None);
            continue;
        };
        let area = &cell.area;
        let start = placements.glyphs.len();
        let glyphs = area
            .buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter().map(move |glyph| (run.line_y, glyph)));
        for (index, (line_y, glyph)) in glyphs.enumerate() {
            let physical = glyph.physical((area.left, area.top), area.scale);
            let embolden = flags.get(index).copied().unwrap_or(false);
            let Some(raster) = table.raster(font_system, physical.cache_key, embolden)? else {
                continue;
            };
            let x = physical.x + raster.left;
            let y = (line_y * area.scale).round() as i32 + physical.y - raster.top;
            placements.glyphs.push(CustomGlyph {
                id: raster.id,
                left: (x as f32 - area.left) / area.scale,
                top: (y as f32 - area.top) / area.scale,
                width: raster.width as f32 / area.scale,
                height: raster.height as f32 / area.scale,
                color: glyph.color_opt,
                snap_to_physical_pixel: true,
                metadata: glyph.metadata,
            });
        }
        placements.cells.push(Some(start..placements.glyphs.len()));
    }
    Ok(placements)
}

/// **The text areas glyphon is handed**: a plain cell as it was laid out, a
/// synthesized one with an empty buffer and its custom glyphs. The same cell
/// sequence [`place_synthetic_bold`] placed, zipped one to one.
pub(crate) fn with_synthetic_bold<'a>(
    cells: impl Iterator<Item = GridCellArea<'a>> + 'a,
    placements: &'a SyntheticPlacements,
) -> impl Iterator<Item = TextArea<'a>> + 'a {
    cells
        .zip(placements.cells.iter())
        .map(move |(cell, placed)| match placed {
            None => cell.area,
            Some(range) => TextArea {
                buffer: &placements.empty,
                custom_glyphs: &placements.glyphs[range.clone()],
                ..cell.area
            },
        })
}
