//! **What one frame asks the shared glyph atlas for, counted lane by lane.**
//!
//! §7.1.3l closed with a debt written down in the open: the frame that lost a
//! whole rendered page's words was diagnosed from a screenshot and a one-line
//! log, and the sentence "about 25 000 distinct rasters is survivable and about
//! 38 000 is not" was measured by hand on one machine and could not be measured
//! again. A ceiling nobody can measure is a ceiling nobody can move.
//!
//! So this is the instrument, and it answers exactly the three questions the
//! ceiling is made of:
//!
//! * **requested** — how many glyph *instances* a lane handed the atlas. This is
//!   the cost of the draw, not of the atlas: a page of the same character a
//!   thousand times over asks a thousand times and occupies one raster.
//! * **unique** — how many distinct *rasters* those instances resolve to, which
//!   is the number that fills the texture. The key is cosmic-text's own
//!   `CacheKey` — face, glyph id, size, subpixel bin, flags — because that is
//!   the key glyphon stores under, so counting anything else would be counting a
//!   different atlas.
//! * **ink** — the area those rasters cover, against the area the device's
//!   `max_texture_dimension_2d` allows. A *lower bound* on occupancy and
//!   deliberately named as one: glyphon packs into buckets and a bucket has
//!   waste in it, so the real texture fills before this number says it should.
//!   What it is good for is the thing a bound is good for — if the ink alone is
//!   over the roof, no packer was ever going to save the frame.
//!
//! **It rasterizes to measure.** Asking swash for a glyph's placement is asking
//! swash to draw it, which is why nothing here runs unless a caller has asked
//! for it by name ([`crate::WindowRenderer::set_glyph_census`]). A census frame
//! is a slow frame on purpose; every other frame is untouched.

use std::collections::{HashMap, HashSet};

use glyphon::cosmic_text::{CacheKey, SubpixelBin};
use glyphon::{FontSystem, SwashCache, TextArea};

use crate::TextLane;

/// One lane's demand on the atlas, for one frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LaneGlyphDemand {
    /// Glyph instances the lane handed the atlas.
    pub requested: usize,
    /// Distinct rasters among them — what actually has to fit.
    pub unique: usize,
    /// The area those distinct rasters cover, in atlas pixels.
    pub ink_px: u64,
}

/// Every lane's demand on one frame, and the roof they share.
#[derive(Clone, Debug, Default)]
pub struct GlyphCensus {
    /// Indexed by [`TextLane::index`]; a lane that prepared nothing stays empty.
    lanes: [LaneRasters; TextLane::COUNT],
    /// The device's `max_texture_dimension_2d` when the frame was counted.
    atlas_side: u32,
}

#[derive(Clone, Debug, Default)]
struct LaneRasters {
    requested: usize,
    /// Raster key to the area it occupies. A map and not a set because the two
    /// facts are asked together and a second pass would rasterize twice.
    rasters: HashMap<CacheKey, u64>,
}

impl GlyphCensus {
    /// A fresh census for a frame drawn against a device with this texture roof.
    #[must_use]
    pub fn for_device(atlas_side: u32) -> Self {
        Self {
            lanes: Default::default(),
            atlas_side,
        }
    }

    /// What this lane asked for.
    #[must_use]
    pub fn lane(&self, lane: TextLane) -> LaneGlyphDemand {
        let entry = &self.lanes[lane.index()];
        LaneGlyphDemand {
            requested: entry.requested,
            unique: entry.rasters.len(),
            ink_px: entry.rasters.values().sum(),
        }
    }

    /// Glyph instances over every lane.
    #[must_use]
    pub fn requested(&self) -> usize {
        self.lanes.iter().map(|lane| lane.requested).sum()
    }

    /// Distinct rasters over every lane — the **union**, not the sum, because
    /// one atlas holds them and a character the chrome and the page both set at
    /// the same size is one raster in it.
    #[must_use]
    pub fn unique(&self) -> usize {
        self.union().len()
    }

    /// The area those distinct rasters cover.
    #[must_use]
    pub fn ink_px(&self) -> u64 {
        self.union().values().sum()
    }

    /// How much atlas the device allows.
    #[must_use]
    pub fn atlas_px(&self) -> u64 {
        u64::from(self.atlas_side) * u64::from(self.atlas_side)
    }

    /// The device's texture roof, one side.
    #[must_use]
    pub fn atlas_side(&self) -> u32 {
        self.atlas_side
    }

    /// Ink over roof, as a fraction. A lower bound on occupancy — see the
    /// module's own note about bucket waste.
    #[must_use]
    pub fn occupancy(&self) -> f64 {
        let roof = self.atlas_px();
        if roof == 0 {
            return 0.0;
        }
        self.ink_px() as f64 / roof as f64
    }

    /// Rasters asked for by more than one lane. The saving a single shared
    /// atlas makes, stated as a number rather than as a hope.
    #[must_use]
    pub fn shared_across_lanes(&self) -> usize {
        self.union()
            .keys()
            .filter(|key| {
                self.lanes
                    .iter()
                    .filter(|lane| lane.rasters.contains_key(*key))
                    .count()
                    > 1
            })
            .count()
    }

    /// Whether this raster was asked for by this lane. The cross-lane sharing
    /// gate reads it: the same character at the same size in two lanes has to be
    /// the same `CacheKey`, or the atlas is holding two of it.
    #[must_use]
    pub fn lane_holds(&self, lane: TextLane, key: CacheKey) -> bool {
        self.lanes[lane.index()].rasters.contains_key(&key)
    }

    /// Every raster key this lane asked for.
    #[must_use]
    pub fn lane_keys(&self, lane: TextLane) -> Vec<CacheKey> {
        self.lanes[lane.index()].rasters.keys().copied().collect()
    }

    /// The same, with the subpixel bins collapsed: the face, glyph, size,
    /// weight and flags a lane asked for, stripped of *where in the pixel* each
    /// occurrence happened to land.
    ///
    /// What two lanes have in common at this resolution is what they *could*
    /// share; what they share at full resolution is what they do share. Both
    /// questions are worth asking separately, because only the second one is
    /// about the atlas and only the first one is about the fonts.
    #[must_use]
    pub fn lane_faces_and_sizes(&self, lane: TextLane) -> HashSet<CacheKey> {
        self.lanes[lane.index()]
            .rasters
            .keys()
            .map(|key| {
                let mut flattened = *key;
                flattened.x_bin = SubpixelBin::Zero;
                flattened.y_bin = SubpixelBin::Zero;
                flattened
            })
            .collect()
    }

    /// Distinct rasters with the **subpixel bins collapsed** — one bitmap per
    /// face, glyph, size, weight and flag set.
    ///
    /// The gap between this and [`Self::unique`] is the whole cost of drawing
    /// text at fractional pen positions: cosmic-text bins the fractional part of
    /// a glyph's origin into four steps on each axis, so one character at one
    /// size can be as many as sixteen bitmaps in the atlas depending on where in
    /// the pixel each of its occurrences happens to land. The ratio is a number
    /// worth reporting on its own — it is the one multiplier on this ladder that
    /// is not about how much text is on the screen.
    #[must_use]
    pub fn unique_without_subpixel(&self) -> usize {
        let mut collapsed: HashSet<CacheKey> = HashSet::new();
        for key in self.union().keys() {
            let mut flattened = *key;
            flattened.x_bin = SubpixelBin::Zero;
            flattened.y_bin = SubpixelBin::Zero;
            collapsed.insert(flattened);
        }
        collapsed.len()
    }

    fn union(&self) -> HashMap<CacheKey, u64> {
        let mut all: HashMap<CacheKey, u64> = HashMap::new();
        for lane in &self.lanes {
            for (key, area) in &lane.rasters {
                all.insert(*key, *area);
            }
        }
        all
    }

    /// One line, for a log or a report.
    #[must_use]
    pub fn line(&self) -> String {
        let mut parts = Vec::new();
        for lane in TextLane::ALL {
            let demand = self.lane(lane);
            if demand.requested == 0 {
                continue;
            }
            parts.push(format!(
                "{}={}/{}",
                lane.name(),
                demand.requested,
                demand.unique
            ));
        }
        let unique = self.unique();
        let without_bins = self.unique_without_subpixel();
        format!(
            "BT_GLYPH_CENSUS requested={} unique={} faces_sizes={} bins={:.2}x shared={} \
             ink_px={} roof_px={} occupancy={:.1}% side={} {}",
            self.requested(),
            unique,
            without_bins,
            if without_bins == 0 {
                0.0
            } else {
                unique as f64 / without_bins as f64
            },
            self.shared_across_lanes(),
            self.ink_px(),
            self.atlas_px(),
            self.occupancy() * 100.0,
            self.atlas_side,
            parts.join(" ")
        )
    }

    /// Count one lane's batch, in exactly the sequence glyphon is handed it.
    ///
    /// The visible-run skip is glyphon's own (`text_render.rs`, `is_run_visible`)
    /// and is reproduced here rather than approximated: a run outside the area's
    /// bounds is never rasterized, so counting it would report a demand the
    /// atlas was never asked for. What is deliberately *not* reproduced is the
    /// per-glyph bounds test, because glyphon applies that **after** it has
    /// rasterized and allocated — a glyph clipped out of view still costs a
    /// raster, which is the whole reason [`crate::shape_chrome_labels`] has to
    /// refuse the box before it gets here.
    pub(crate) fn record<'a>(
        &mut self,
        lane: TextLane,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        areas: impl IntoIterator<Item = TextArea<'a>>,
    ) {
        let entry = &mut self.lanes[lane.index()];
        for area in areas {
            let visible = |run: &glyphon::cosmic_text::LayoutRun| {
                let start = (area.top + (run.line_top * area.scale)) as i32;
                let end = start + (run.line_height * area.scale) as i32;
                start <= area.bounds.bottom && area.bounds.top <= end
            };
            let runs = area
                .buffer
                .layout_runs()
                .skip_while(|run| !visible(run))
                .take_while(visible);
            for run in runs {
                for glyph in run.glyphs {
                    let key = glyph.physical((area.left, area.top), area.scale).cache_key;
                    entry.requested += 1;
                    if entry.rasters.contains_key(&key) {
                        continue;
                    }
                    let area_px =
                        swash_cache
                            .get_image_uncached(font_system, key)
                            .map_or(0, |image| {
                                u64::from(image.placement.width) * u64::from(image.placement.height)
                            });
                    entry.rasters.insert(key, area_px);
                }
            }
        }
    }
}
