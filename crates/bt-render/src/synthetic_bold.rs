//! **A bold grid cell whose face has no bold cut, drawn heavier from that face's
//! own outline** (0.4.5 ticket 38; `docs/DESIGN.md` 2026-09-24).
//!
//! The family that draws a cell never depends on the weight asked
//! ([`crate::cjk_fonts::match_grid_attrs`]), so a bold request on a family with
//! no bold cut is shaped from the family's regular face. cosmic-text's rasterizer
//! has a synthetic slant (`FAKE_ITALIC`) and no synthetic weight, and glyphon
//! rasterizes every text glyph itself, so the heavier raster is drawn through
//! glyphon's one open door: a **custom glyph**, rasterized here from the same
//! face, glyph and subpixel position the shaper chose, and thickened outward
//! by FreeType's emboldening ([`synthetic_bold_image`]; swash's own reads the
//! outline's orientation from the wrong polygon, ticket 61). The shaper and the
//! pen are untouched; only the ink is.
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
use swash::scale::outline::Outline;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Mask, Origin, Point, Transform, Vector, Verb};

/// **How far a synthetic bold moves an outline, as a fraction of the em.**
///
/// The embolden ([`embolden_outline`]) is FreeType's `FT_Outline_EmboldenXY`
/// without its halving, as swash ports it: every point moves by this distance on each axis plus its corner's
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
/// 14° skew for `FAKE_ITALIC` — with one addition: the outline is emboldened
/// ([`embolden_outline`]) before the skew, so a bold-italic cell in a
/// regular-only family is slanted *and* heavier. A colour source (a colour
/// outline, a colour bitmap) is drawn exactly as swash's `Render` draws it and
/// never emboldened; only the monochrome outline is. With `embolden` false this
/// is the regular raster, which is how the one glyph of a mixed cluster that has
/// a bold face of its own is drawn beside one that does not.
///
/// The outline arm is swash's own (`Render::render_into`, `Source::Outline`:
/// the scaled outline, then the transform, then a non-zero `Mask` at the
/// subpixel offset with the origin at the bottom left) with its emboldening
/// replaced, because swash 0.2.9 decides which side of a contour is ink from
/// the wrong polygon (ticket 61, [`outline_orientation`]).
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
    let skew = key
        .flags
        .contains(CacheKeyFlags::FAKE_ITALIC)
        .then(|| Transform::skew(Angle::from_degrees(14.0), Angle::from_degrees(0.0)));
    if let Some(image) = Render::new(&[
        Source::ColorOutline(0),
        Source::ColorBitmap(StrikeWith::BestFit),
    ])
    .format(Format::Alpha)
    .offset(offset)
    .transform(skew)
    .render(&mut scaler, key.glyph_id)
    {
        return Some(image);
    }
    if !scaler.has_outlines() {
        return None;
    }
    let mut outline = scaler.scale_outline(key.glyph_id)?;
    if embolden {
        embolden_outline(&mut outline, SYNTHETIC_BOLD_STRENGTH_EM * size);
    }
    if let Some(skew) = &skew {
        outline.transform(skew);
    }
    let mut image = glyphon::SwashImage::new();
    image.placement = Mask::new(outline.path())
        .format(Format::Alpha)
        .origin(Origin::BottomLeft)
        .offset(offset)
        .render_offset(offset)
        .inspect(|format, width, height| {
            image.data.resize(format.buffer_size(width, height), 0);
        })
        .render_into(&mut image.data[..], None);
    image.content = glyphon::SwashContent::Mask;
    image.source = Source::Outline;
    Some(image)
}

/// Which way an outline's filled contours run — FreeType's
/// `FT_Outline_Get_Orientation`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutlineOrientation {
    /// Filled contours run clockwise with y up (TrueType's convention).
    Clockwise,
    /// Filled contours run counter-clockwise with y up (PostScript's).
    CounterClockwise,
}

/// The point ranges of an outline's contours, split where swash's own
/// emboldening splits them: at every `MoveTo` and `Close`.
pub(crate) fn outline_contours(verbs: &[Verb]) -> Vec<Range<usize>> {
    let mut contours = Vec::new();
    let (mut start, mut end) = (0, 0);
    for verb in verbs {
        match verb {
            Verb::MoveTo | Verb::Close => {
                if end > start {
                    contours.push(start..end);
                }
                start = end;
                if *verb == Verb::MoveTo {
                    end += 1;
                }
            }
            Verb::LineTo => end += 1,
            Verb::QuadTo => end += 2,
            Verb::CurveTo => end += 3,
        }
    }
    if end > start {
        contours.push(start..end);
    }
    contours
}

/// **Which side of an outline is ink** — FreeType's
/// `FT_Outline_Get_Orientation`: the signed area of every contour, each closed
/// on itself, summed. `None` for an outline with no area.
///
/// swash 0.2.9 (`LayerMut::embolden` → `compute_winding`) takes the area of
/// *all* the outline's points as one polygon instead: every contour's closing
/// edge is missing and an edge from each contour's last point to the next
/// contour's first is added. For a glyph of one contour the two agree; for a
/// glyph of many — most Han characters — the stray edges can outweigh the real
/// area and flip the sign, and emboldening with the flipped sign moves every
/// point *into* the ink, so the glyph comes out thinner than its regular
/// raster (ticket 61: in NSimSun, `络 志 如 谁 排 就` and not their
/// neighbours).
pub(crate) fn outline_orientation(
    points: &[Point],
    contours: &[Range<usize>],
) -> Option<OutlineOrientation> {
    let area: f32 = contours
        .iter()
        .map(|contour| {
            let contour = &points[contour.clone()];
            let mut previous = contour[contour.len() - 1];
            let mut area = 0.0;
            for &point in contour {
                area += (point.y - previous.y) * (point.x + previous.x);
                previous = point;
            }
            area
        })
        .sum();
    if area > 0.0 {
        Some(OutlineOrientation::CounterClockwise)
    } else if area < 0.0 {
        Some(OutlineOrientation::Clockwise)
    } else {
        None
    }
}

/// **Thicken every stroke of an outline by twice `strength`**, outward from
/// the ink on each side of it.
///
/// FreeType's `FT_Outline_EmboldenXY` without its halving, contour by contour
/// — the same port swash carries, given the orientation of
/// [`outline_orientation`] instead of swash's one-polygon area. Every point
/// moves by `strength` on each axis plus its corner's bisector shift of the
/// same size, so the ink box grows to the right and to the top and the pen and
/// the baseline stay where the regular glyph has them. An outline with no area
/// has no inside to grow away from and is left as it is, as FreeType leaves it.
fn embolden_outline(outline: &mut Outline, strength: f32) {
    let contours = outline_contours(outline.verbs());
    let points = outline.points_mut();
    let Some(orientation) = outline_orientation(points, &contours) else {
        return;
    };
    for contour in contours {
        embolden_contour(&mut points[contour], orientation, strength);
    }
}

/// One closed contour of [`embolden_outline`]: FreeType's per-contour loop, as
/// swash 0.2.9 ports it (`scale/outline.rs`, `embolden`), with the same
/// strength on both axes.
fn embolden_contour(points: &mut [Point], orientation: OutlineOrientation, strength: f32) {
    let clockwise = orientation == OutlineOrientation::Clockwise;
    let last = points.len() - 1;
    let mut i = last;
    let mut j = 0;
    let mut k = usize::MAX;
    let mut in_len = 0.0;
    let mut anchor_len = 0.0;
    let mut anchor = Point::ZERO;
    let mut in_ = Point::ZERO;
    while j != i && i != k {
        let (out, out_len) = if j == k {
            (anchor, anchor_len)
        } else {
            let out = points[j] - points[i];
            let out_len = out.length();
            if out_len == 0.0 {
                j = if j < last { j + 1 } else { 0 };
                continue;
            }
            (Point::new(out.x / out_len, out.y / out_len), out_len)
        };
        if in_len == 0.0 {
            i = j;
        } else {
            if k == usize::MAX {
                k = i;
                anchor = in_;
                anchor_len = in_len;
            }
            let mut d = in_.x * out.x + in_.y * out.y;
            let shift = if d > -0.9396 {
                d += 1.0;
                let mut sx = in_.y + out.y;
                let mut sy = in_.x + out.x;
                let mut q = out.x * in_.y - out.y * in_.x;
                if clockwise {
                    sx = -sx;
                    q = -q;
                } else {
                    sy = -sy;
                }
                let l = in_len.min(out_len);
                let scale = if strength * q <= l * d {
                    strength / d
                } else {
                    l / q
                };
                Point::new(sx * scale, sy * scale)
            } else {
                Point::ZERO
            };
            while i != j {
                points[i].x += strength + shift.x;
                points[i].y += strength + shift.y;
                i = if i < last { i + 1 } else { 0 };
            }
        }
        in_ = out;
        in_len = out_len;
        j = if j < last { j + 1 } else { 0 };
    }
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
