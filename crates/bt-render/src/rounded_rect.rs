//! Rounded rectangles for a pipeline that can only draw axis-aligned quads.
//!
//! `marks.rs` in the shell hands the active tab's silhouette to an SVG
//! rasterizer, because that shape has concave skirt corners and a browser draws
//! them with analytic coverage — nested rectangles would produce a staircase. A
//! floating window's corner is the other half of the same problem: still a
//! curve, still antialiased by the design's own renderer, but simple enough that
//! its coverage has a closed form. So it is computed here rather than routed
//! through a rasterizer and a texture upload, and the result is the same kind of
//! answer the tab gets — *exact* area coverage per pixel, never a stair.
//!
//! The output is physical-pixel-aligned rectangles: whole runs where the shape
//! covers a pixel completely, single pixels where it covers part of one. The
//! renderer turns each into one blended quad.

/// One physical-pixel-aligned rectangle and the fraction of it the shape covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CoverageRect {
    /// `[left, top, right, bottom]` in physical pixels.
    pub rect: [f32; 4],
    /// `0.0 < coverage <= 1.0`; empty pixels are never emitted.
    pub coverage: f32,
}

/// Decompose a rounded rectangle into pixel-aligned rectangles with exact
/// coverage.
///
/// The rectangle is snapped to whole pixels first: a floating window's edge is
/// chrome, and chrome that lands between two pixel columns is a hairline drawn
/// twice at half strength. The radius stays fractional — it is a DPI-scaled
/// design number, and rounding it would step the corner between DPI settings.
pub(crate) fn rounded_rect_coverage(rect: [f32; 4], radius: f32) -> Vec<CoverageRect> {
    let left = rect[0].round();
    let top = rect[1].round();
    let right = rect[2].round();
    let bottom = rect[3].round();
    if right <= left || bottom <= top {
        return Vec::new();
    }
    let radius = radius
        .max(0.0)
        .min((right - left) / 2.0)
        .min((bottom - top) / 2.0);
    if radius <= 0.0 {
        return vec![CoverageRect {
            rect: [left, top, right, bottom],
            coverage: 1.0,
        }];
    }
    // Whole pixel columns/rows a corner's round can reach into.
    let band = radius.ceil();
    let mid_top = top + band;
    let mid_bottom = bottom - band;
    // Two corners on the same side of the box can only both need per-pixel work
    // if the box is narrower than their two bands together; above that width the
    // strip between them is straight-edged and is one rectangle.
    let split = right - left > 2.0 * band;
    let mut out = Vec::new();
    if mid_bottom > mid_top {
        // The straight middle: full width, every row between the corner bands.
        out.push(CoverageRect {
            rect: [left, mid_top, right, mid_bottom],
            coverage: 1.0,
        });
        if split {
            out.push(CoverageRect {
                rect: [left + band, top, right - band, mid_top],
                coverage: 1.0,
            });
            out.push(CoverageRect {
                rect: [left + band, mid_bottom, right - band, bottom],
                coverage: 1.0,
            });
        }
    } else if split {
        out.push(CoverageRect {
            rect: [left + band, top, right - band, bottom],
            coverage: 1.0,
        });
    }
    let shape = [left, top, right, bottom];
    let mut row = top;
    while row < bottom {
        if row >= mid_top && row < mid_bottom {
            // Already covered by the straight middle.
            row = mid_bottom;
            continue;
        }
        let coverage = |x: f32, y: f32| pixel_coverage(shape, radius, x, y);
        if split {
            push_row(&mut out, row, left, left + band, coverage);
            push_row(&mut out, row, right - band, right, coverage);
        } else {
            push_row(&mut out, row, left, right, coverage);
        }
        row += 1.0;
    }
    out
}

/// The ring between a rounded rectangle and the same rectangle grown by `extent`
/// on every side, concentric — a drop shadow, with the hole a browser's own
/// `box-shadow` leaves: the spec clips an outer shadow out of the border box it
/// lifts, so a translucent border is not doubled by the shadow behind it.
pub(crate) fn rounded_rect_halo_coverage(
    rect: [f32; 4],
    radius: f32,
    extent: f32,
) -> Vec<CoverageRect> {
    let left = rect[0].round();
    let top = rect[1].round();
    let right = rect[2].round();
    let bottom = rect[3].round();
    // Whole pixels, so the ring and the box it surrounds share one grid.
    let extent = extent.round();
    if right <= left || bottom <= top || extent < 1.0 {
        return Vec::new();
    }
    let radius = radius
        .max(0.0)
        .min((right - left) / 2.0)
        .min((bottom - top) / 2.0);
    let outer = [left - extent, top - extent, right + extent, bottom + extent];
    let outer_radius = radius + extent;
    let inner = [left, top, right, bottom];
    // The band a round can reach into, out here and in there. Growing a rounded
    // rectangle by a whole number of pixels grows its band by the same number, so
    // the two shapes' straight middles start and end on the same rows and columns
    // — which is what lets the parts that cancel be skipped rather than computed.
    let band = radius.ceil();
    let mid_top = top + band;
    let mid_bottom = bottom - band;
    let mid_left = left + band;
    let mid_right = right - band;
    let coverage = |x: f32, y: f32| {
        pixel_coverage(outer, outer_radius, x, y) - pixel_coverage(inner, radius, x, y)
    };
    let mut out = Vec::new();
    let mut row = outer[1];
    while row < outer[3] {
        if row >= mid_top && row < mid_bottom {
            // Both shapes are straight-edged here: the ring is its two flanks.
            out.push(CoverageRect {
                rect: [outer[0], mid_top, left, mid_bottom],
                coverage: 1.0,
            });
            out.push(CoverageRect {
                rect: [right, mid_top, outer[2], mid_bottom],
                coverage: 1.0,
            });
            row = mid_bottom;
            continue;
        }
        if row < top || row >= bottom {
            // Past the box entirely: the ring is just the outer shape's row.
            push_row(&mut out, row, outer[0], outer[2], coverage);
        } else if mid_right > mid_left {
            // Alongside the box: the columns between the two corner bands are
            // covered by both shapes and cancel exactly.
            push_row(&mut out, row, outer[0], mid_left, coverage);
            push_row(&mut out, row, mid_right, outer[2], coverage);
        } else {
            push_row(&mut out, row, outer[0], outer[2], coverage);
        }
        row += 1.0;
    }
    out
}

/// One row, `[from, to)` pixel columns: consecutive fully covered pixels merge
/// into a single rectangle, partial ones stand alone, and empty ones are dropped.
fn push_row(
    out: &mut Vec<CoverageRect>,
    row: f32,
    from: f32,
    to: f32,
    coverage: impl Fn(f32, f32) -> f32,
) {
    let mut run_start: Option<f32> = None;
    let mut column = from;
    while column < to {
        let coverage = coverage(column, row);
        if coverage >= 1.0 {
            run_start.get_or_insert(column);
        } else {
            if let Some(start) = run_start.take() {
                out.push(CoverageRect {
                    rect: [start, row, column, row + 1.0],
                    coverage: 1.0,
                });
            }
            if coverage > 0.0 {
                out.push(CoverageRect {
                    rect: [column, row, column + 1.0, row + 1.0],
                    coverage,
                });
            }
        }
        column += 1.0;
    }
    if let Some(start) = run_start {
        out.push(CoverageRect {
            rect: [start, row, to, row + 1.0],
            coverage: 1.0,
        });
    }
}

/// The fraction of the pixel whose top-left corner is `(x, y)` that lies inside
/// the rounded rectangle.
///
/// What a corner takes away is the part of the box that lies past its centre on
/// both axes and outside its circle. Those four regions are disjoint — each lives
/// in its own corner's quadrant, and two corners on one side share a quadrant
/// boundary at worst — so the pixel's missing area is simply their sum, and the
/// sum is right even in the case a nearest-corner test gets wrong: a pixel that
/// straddles two corners' centres because the radius reaches the box's midline.
///
/// A corner whose centre the pixel does not pass contributes nothing, so away
/// from the rounds this costs four comparisons and no arithmetic.
fn pixel_coverage(shape: [f32; 4], radius: f32, x: f32, y: f32) -> f32 {
    let [left, top, right, bottom] = shape;
    // The pixel clipped to the box: a pixel outside it is covered by none of it,
    // which is what the halo asks when it subtracts one shape from another.
    let (x0, x1) = (x.max(left), (x + 1.0).min(right));
    let (y0, y1) = (y.max(top), (y + 1.0).min(bottom));
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let mut covered = f64::from(x1 - x0) * f64::from(y1 - y0);
    for (centre_x, before_x) in [(left + radius, true), (right - radius, false)] {
        let (a0, a1) = corner_span(x0, x1, centre_x, before_x);
        if a1 <= a0 {
            continue;
        }
        for (centre_y, before_y) in [(top + radius, true), (bottom - radius, false)] {
            let (b0, b1) = corner_span(y0, y1, centre_y, before_y);
            if b1 <= b0 {
                continue;
            }
            let quadrant = f64::from(a1 - a0) * f64::from(b1 - b0);
            covered -= quadrant
                - disc_area_in_box(radius.into(), a0.into(), a1.into(), b0.into(), b1.into());
        }
    }
    covered.clamp(0.0, 1.0) as f32
}

/// How far `[lo, hi]` reaches past `centre`, on the side the round is on, in
/// distance-from-centre coordinates. Zero-length when the span is entirely on
/// the straight side.
fn corner_span(lo: f32, hi: f32, centre: f32, before: bool) -> (f32, f32) {
    if before {
        ((centre - hi).max(0.0), (centre - lo).max(0.0))
    } else {
        ((lo - centre).max(0.0), (hi - centre).max(0.0))
    }
}

/// The exact area of the quarter disc of radius `r` centred at the origin that
/// falls inside `[x0, x1] × [y0, y1]`, with the box in the disc's own quadrant.
///
/// The boundary is `y = sqrt(r² - x²)`, decreasing, so the box splits into at
/// most three columns of x: one where the arc is above the box (full height),
/// one where it crosses it, and one where it is below (empty). Only the middle
/// one needs the integral, and the integral of a circle has a closed form.
fn disc_area_in_box(r: f64, x0: f64, x1: f64, y0: f64, y1: f64) -> f64 {
    if x1 <= x0 || y1 <= y0 || r <= 0.0 {
        return 0.0;
    }
    let arc_at = |y: f64| if y >= r { 0.0 } else { (r * r - y * y).sqrt() };
    // Left of `full_until` the arc clears the box's top; right of `empty_from`
    // it has dropped below its bottom.
    let full_until = arc_at(y1);
    let empty_from = arc_at(y0);
    let full = (full_until.min(x1) - x0).max(0.0) * (y1 - y0);
    let cross_from = x0.max(full_until);
    let cross_to = x1.min(empty_from);
    if cross_to <= cross_from {
        return full;
    }
    full + arc_integral(r, cross_to) - arc_integral(r, cross_from) - (cross_to - cross_from) * y0
}

/// `∫ sqrt(r² - x²) dx = (x·sqrt(r² - x²) + r²·asin(x / r)) / 2`.
fn arc_integral(r: f64, x: f64) -> f64 {
    let x = x.clamp(-r, r);
    (x * (r * r - x * x).max(0.0).sqrt() + r * r * (x / r).clamp(-1.0, 1.0).asin()) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_area(rects: &[CoverageRect]) -> f64 {
        rects
            .iter()
            .map(|entry| {
                f64::from(entry.rect[2] - entry.rect[0])
                    * f64::from(entry.rect[3] - entry.rect[1])
                    * f64::from(entry.coverage)
            })
            .sum()
    }

    /// Every emitted rectangle is pixel-aligned, inside the box, and none of them
    /// overlap — the pipeline blends, so a pixel covered twice at 9% alpha is an
    /// 18% pixel, and a seam of those is exactly the artifact this decomposition
    /// exists to avoid.
    fn assert_disjoint_and_inside(rects: &[CoverageRect], shape: [f32; 4]) {
        let mut seen: std::collections::HashMap<(i64, i64), f32> = std::collections::HashMap::new();
        for entry in rects {
            let [l, t, r, b] = entry.rect;
            assert!(
                r > l && b > t,
                "an empty rectangle is never worth an instance: {entry:?}"
            );
            assert!(
                entry.coverage > 0.0 && entry.coverage <= 1.0,
                "coverage out of range: {entry:?}"
            );
            for value in [l, t, r, b] {
                assert_eq!(value, value.round(), "unaligned edge in {entry:?}");
            }
            assert!(
                l >= shape[0] && t >= shape[1] && r <= shape[2] && b <= shape[3],
                "{entry:?} escapes {shape:?}"
            );
            let mut y = t;
            while y < b {
                let mut x = l;
                while x < r {
                    let previous = seen.insert((x as i64, y as i64), entry.coverage);
                    assert!(previous.is_none(), "pixel ({x}, {y}) covered twice");
                    x += 1.0;
                }
                y += 1.0;
            }
        }
    }

    /// PIN (float-window pass): a rounded corner carries partial coverage that
    /// climbs monotonically along the diagonal instead of stepping between 0 and
    /// 1 — the same claim `marks.rs` pins for the tab, which is what makes the
    /// two corners the same craft.
    ///
    /// Red gate: nested rectangles (the obvious alternative) put *no* pixel
    /// strictly between 0 and 1, so the first assertion alone rules them out.
    #[test]
    fn corners_carry_partial_coverage_instead_of_a_staircase() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let radius = 10.0 * scale;
            let shape = [0.0, 0.0, (240.0 * scale).round(), (160.0 * scale).round()];
            let rects = rounded_rect_coverage(shape, radius);
            assert_disjoint_and_inside(&rects, shape);
            let partial = rects
                .iter()
                .filter(|entry| entry.coverage < 1.0)
                .filter(|entry| entry.rect[2] - entry.rect[0] == 1.0)
                .count();
            assert!(
                partial >= radius as usize,
                "an antialiased quarter circle spends at least one partial pixel per row, \
                 saw {partial} at scale {scale}"
            );
            // The area of a rounded rectangle is exact: w·h minus the four
            // corners' worth of square-outside-circle.
            let expected = f64::from(shape[2] - shape[0]) * f64::from(shape[3] - shape[1])
                - (4.0 - std::f64::consts::PI) * f64::from(radius) * f64::from(radius);
            assert!(
                (total_area(&rects) - expected).abs() < 0.01,
                "coverage must integrate to the true area: {} vs {expected}",
                total_area(&rects)
            );
        }
    }

    /// The corner is a real quarter circle: dead in the outermost pixel, alive a
    /// radius in, and never decreasing as it walks the diagonal inwards.
    #[test]
    fn the_round_is_a_quarter_circle_and_not_a_chamfer() {
        let shape = [0.0, 0.0, 80.0, 60.0];
        let radius = 12.0_f32;
        assert_eq!(pixel_coverage(shape, radius, 0.0, 0.0), 0.0);
        assert_eq!(pixel_coverage(shape, radius, 12.0, 0.0), 1.0);
        assert_eq!(pixel_coverage(shape, radius, 0.0, 12.0), 1.0);
        assert_eq!(pixel_coverage(shape, radius, 40.0, 30.0), 1.0);
        // Diagonal: coverage rises from nothing to everything, monotonically.
        let mut previous = -1.0;
        for step in 0..12 {
            let coverage = pixel_coverage(shape, radius, step as f32, step as f32);
            assert!(
                coverage >= previous,
                "coverage dips at step {step}: {coverage} after {previous}"
            );
            previous = coverage;
        }
        assert!(previous > 0.9, "the corner must close by the radius");
        // The arc meets the 45° diagonal one radius/√2 out from the corner's
        // centre — that pixel is cut, and a decomposition into whole rectangles
        // could not cut it.
        let on_arc_pixel = (radius * (1.0 - 1.0 / std::f32::consts::SQRT_2)).floor();
        let on_arc = pixel_coverage(shape, radius, on_arc_pixel, on_arc_pixel);
        assert!(
            on_arc > 0.0 && on_arc < 1.0,
            "the 45° pixel is cut by the arc, saw {on_arc}"
        );
    }

    /// All four corners are rounded, not just the one the maths was written for.
    #[test]
    fn every_corner_is_rounded() {
        let shape = [10.0, 20.0, 90.0, 70.0];
        let radius = 8.0_f32;
        for (x, y) in [(10.0, 20.0), (89.0, 20.0), (10.0, 69.0), (89.0, 69.0)] {
            assert_eq!(
                pixel_coverage(shape, radius, x, y),
                0.0,
                "corner ({x}, {y}) is not cut away"
            );
        }
        for (x, y) in [(50.0, 20.0), (10.0, 45.0), (50.0, 69.0), (89.0, 45.0)] {
            assert_eq!(
                pixel_coverage(shape, radius, x, y),
                1.0,
                "the straight edge at ({x}, {y}) must be solid"
            );
        }
    }

    /// Degenerate boxes stay honest: no radius is a plain rectangle, a radius
    /// larger than the box clamps to a stadium/circle rather than inverting, and
    /// an empty box draws nothing.
    #[test]
    fn degenerate_boxes_do_not_produce_garbage() {
        assert_eq!(
            rounded_rect_coverage([4.0, 4.0, 20.0, 12.0], 0.0),
            vec![CoverageRect {
                rect: [4.0, 4.0, 20.0, 12.0],
                coverage: 1.0
            }]
        );
        assert!(rounded_rect_coverage([4.0, 4.0, 4.0, 12.0], 3.0).is_empty());
        assert!(rounded_rect_coverage([4.0, 12.0, 20.0, 4.0], 3.0).is_empty());
        let shape = [0.0, 0.0, 9.0, 7.0];
        let circle = rounded_rect_coverage(shape, 40.0);
        assert_disjoint_and_inside(&circle, shape);
        let expected = 7.0 * 7.0 * std::f64::consts::PI / 4.0 + 2.0 * 7.0;
        assert!(
            (total_area(&circle) - expected).abs() < 0.01,
            "an over-large radius must clamp to the box's own half: {} vs {expected}",
            total_area(&circle)
        );
    }
}
