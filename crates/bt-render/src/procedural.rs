//! Cell-geometry rendering for Unicode box drawing and block elements.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PixelRect {
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) right: f32,
    pub(crate) bottom: f32,
    pub(crate) coverage: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stroke {
    None,
    Light,
    Heavy,
    Double,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LineShape {
    up: Stroke,
    right: Stroke,
    down: Stroke,
    left: Stroke,
    dash: Option<(Axis, u32)>,
    rounded: bool,
}

#[derive(Clone, Copy, Debug)]
struct LineMetrics {
    width: i32,
    height: i32,
    light: i32,
    heavy: i32,
    join_width: i32,
}

impl LineShape {
    const fn new(up: Stroke, right: Stroke, down: Stroke, left: Stroke) -> Self {
        Self {
            up,
            right,
            down,
            left,
            dash: None,
            rounded: false,
        }
    }

    const fn dashed(axis: Axis, count: u32, stroke: Stroke) -> Self {
        match axis {
            Axis::Horizontal => {
                Self::with_dash(Stroke::None, stroke, Stroke::None, stroke, axis, count)
            }
            Axis::Vertical => {
                Self::with_dash(stroke, Stroke::None, stroke, Stroke::None, axis, count)
            }
        }
    }

    const fn with_dash(
        up: Stroke,
        right: Stroke,
        down: Stroke,
        left: Stroke,
        axis: Axis,
        count: u32,
    ) -> Self {
        Self {
            up,
            right,
            down,
            left,
            dash: Some((axis, count)),
            rounded: false,
        }
    }

    const fn rounded(up: Stroke, right: Stroke, down: Stroke, left: Stroke) -> Self {
        Self {
            up,
            right,
            down,
            left,
            dash: None,
            rounded: true,
        }
    }
}

pub(crate) fn supports_text(text: &str) -> bool {
    let mut characters = text.chars();
    let Some(character) = characters.next() else {
        return false;
    };
    characters.next().is_none() && supports(character)
}

pub(crate) fn supports(character: char) -> bool {
    matches!(character, '\u{2500}'..='\u{2570}' | '\u{2574}'..='\u{259f}')
}

pub(crate) fn geometry(
    character: char,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    light_line_width_px: f32,
) -> Option<Vec<PixelRect>> {
    let width = width.round().max(1.0) as i32;
    let height = height.round().max(1.0) as i32;
    let local = if let Some(shape) = line_shape(character) {
        line_geometry(shape, width, height, light_line_width_px)
    } else if ('\u{2580}'..='\u{259f}').contains(&character) {
        block_geometry(character, width, height)
    } else {
        return None;
    };
    Some(
        local
            .into_iter()
            .map(|rect| PixelRect {
                left: rect.left + left,
                top: rect.top + top,
                right: rect.right + left,
                bottom: rect.bottom + top,
                coverage: rect.coverage,
            })
            .collect(),
    )
}

fn line_geometry(
    shape: LineShape,
    width: i32,
    height: i32,
    light_line_width_px: f32,
) -> Vec<PixelRect> {
    let light = light_line_width_px.round().max(1.0) as i32;
    let heavy = (2 * light).min(width.min(height).max(1));
    if let Some((axis, count)) = shape.dash {
        let stroke = if axis == Axis::Horizontal {
            shape.left
        } else {
            shape.up
        };
        return dashed_geometry(
            axis,
            count,
            stroke_width(stroke, light, heavy),
            width,
            height,
        );
    }
    if shape.rounded {
        return rounded_geometry(shape, width, height, light);
    }
    if shape.up == Stroke::None
        && shape.down == Stroke::None
        && shape.left == shape.right
        && shape.left != Stroke::None
    {
        return stroke_lanes(shape.left, height, light, heavy)
            .into_iter()
            .map(|(top, bottom)| PixelRect {
                left: 0.0,
                top: top as f32,
                right: width as f32,
                bottom: bottom as f32,
                coverage: 1.0,
            })
            .collect();
    }
    if shape.left == Stroke::None
        && shape.right == Stroke::None
        && shape.up == shape.down
        && shape.up != Stroke::None
    {
        return stroke_lanes(shape.up, width, light, heavy)
            .into_iter()
            .map(|(left, right)| PixelRect {
                left: left as f32,
                top: 0.0,
                right: right as f32,
                bottom: height as f32,
                coverage: 1.0,
            })
            .collect();
    }

    let join_width = [shape.up, shape.right, shape.down, shape.left]
        .into_iter()
        .map(|stroke| match stroke {
            Stroke::None => 0,
            Stroke::Light => light,
            Stroke::Heavy => heavy,
            Stroke::Double => (3 * light).min(width.min(height)),
        })
        .max()
        .unwrap_or(light);
    let metrics = LineMetrics {
        width,
        height,
        light,
        heavy,
        join_width,
    };
    let mut rects = Vec::with_capacity(8);
    add_arm(&mut rects, Axis::Vertical, true, shape.up, metrics);
    add_arm(&mut rects, Axis::Horizontal, false, shape.right, metrics);
    add_arm(&mut rects, Axis::Vertical, false, shape.down, metrics);
    add_arm(&mut rects, Axis::Horizontal, true, shape.left, metrics);
    rects
}

fn add_arm(
    rects: &mut Vec<PixelRect>,
    axis: Axis,
    negative: bool,
    stroke: Stroke,
    metrics: LineMetrics,
) {
    if stroke == Stroke::None {
        return;
    }
    let cross_extent = if axis == Axis::Horizontal {
        metrics.height
    } else {
        metrics.width
    };
    let main_extent = if axis == Axis::Horizontal {
        metrics.width
    } else {
        metrics.height
    };
    let lanes = stroke_lanes(stroke, cross_extent, metrics.light, metrics.heavy);
    let (center_start, center_end) = centered_span(main_extent, metrics.join_width);
    for (lane_start, lane_end) in lanes {
        let coordinates = match (axis, negative) {
            (Axis::Horizontal, true) => (0, lane_start, center_end, lane_end),
            (Axis::Horizontal, false) => (center_start, lane_start, metrics.width, lane_end),
            (Axis::Vertical, true) => (lane_start, 0, lane_end, center_end),
            (Axis::Vertical, false) => (lane_start, center_start, lane_end, metrics.height),
        };
        push_rect(
            rects,
            coordinates.0,
            coordinates.1,
            coordinates.2,
            coordinates.3,
        );
    }
}

fn stroke_lanes(stroke: Stroke, extent: i32, light: i32, heavy: i32) -> Vec<(i32, i32)> {
    match stroke {
        Stroke::None => Vec::new(),
        Stroke::Light => vec![centered_span(extent, light)],
        Stroke::Heavy => vec![centered_span(extent, heavy)],
        Stroke::Double => {
            let total = (3 * light).min(extent);
            let (start, end) = centered_span(extent, total);
            vec![
                (start, (start + light).min(end)),
                ((end - light).max(start), end),
            ]
        }
    }
}

fn stroke_width(stroke: Stroke, light: i32, heavy: i32) -> i32 {
    match stroke {
        Stroke::None => 0,
        Stroke::Light | Stroke::Double => light,
        Stroke::Heavy => heavy,
    }
}

fn centered_span(extent: i32, thickness: i32) -> (i32, i32) {
    let thickness = thickness.clamp(1, extent);
    let start = (extent - thickness) / 2;
    (start, start + thickness)
}

fn dashed_geometry(
    axis: Axis,
    count: u32,
    thickness: i32,
    width: i32,
    height: i32,
) -> Vec<PixelRect> {
    let extent = if axis == Axis::Horizontal {
        width
    } else {
        height
    };
    let cross_extent = if axis == Axis::Horizontal {
        height
    } else {
        width
    };
    let (cross_start, cross_end) = centered_span(cross_extent, thickness);
    let count = count.min(extent as u32).max(1);
    let gap = thickness
        .max(1)
        .min((extent - count as i32) / (count as i32 - 1).max(1));
    let fill = extent - gap * (count as i32 - 1);
    let mut rects = Vec::with_capacity(count as usize);
    for index in 0..count as i32 {
        let start = index * fill / count as i32 + index * gap;
        let end = if index + 1 == count as i32 {
            extent
        } else {
            (index + 1) * fill / count as i32 + index * gap
        };
        match axis {
            Axis::Horizontal => push_rect(&mut rects, start, cross_start, end, cross_end),
            Axis::Vertical => push_rect(&mut rects, cross_start, start, cross_end, end),
        }
    }
    rects
}

fn rounded_geometry(shape: LineShape, width: i32, height: i32, thickness: i32) -> Vec<PixelRect> {
    let (cx_start, cx_end) = centered_span(width, thickness);
    let (cy_start, cy_end) = centered_span(height, thickness);
    let center_x = (cx_start + cx_end) as f32 / 2.0;
    let center_y = (cy_start + cy_end) as f32 / 2.0;
    let half = thickness as f32 / 2.0;
    let x_direction = if shape.right != Stroke::None {
        1.0
    } else {
        -1.0
    };
    let y_direction = if shape.down != Stroke::None {
        1.0
    } else {
        -1.0
    };
    let x_clearance = if x_direction > 0.0 {
        width as f32 - center_x - half
    } else {
        center_x - half
    };
    let y_clearance = if y_direction > 0.0 {
        height as f32 - center_y - half
    } else {
        center_y - half
    };
    let radius = x_clearance.min(y_clearance).max(0.0);
    // The arc's center lies toward the inside of the box. Sampling the opposite quadrant makes
    // the curve bulge toward the cell's outside corner instead of looking bitten inward.
    let arc_center_x = center_x + x_direction * radius;
    let arc_center_y = center_y + y_direction * radius;
    let horizontal_endpoint = (arc_center_x, arc_center_y - y_direction * radius);
    let vertical_endpoint = (arc_center_x - x_direction * radius, arc_center_y);
    let mut rects = Vec::new();

    let (horizontal_left, horizontal_right) = if x_direction > 0.0 {
        (horizontal_endpoint.0 - half, width as f32)
    } else {
        (0.0, horizontal_endpoint.0 + half)
    };
    push_clipped_rect(
        &mut rects,
        horizontal_left,
        center_y - half,
        horizontal_right,
        center_y + half,
        width,
        height,
    );

    let (vertical_top, vertical_bottom) = if y_direction > 0.0 {
        (vertical_endpoint.1 - half, height as f32)
    } else {
        (0.0, vertical_endpoint.1 + half)
    };
    push_clipped_rect(
        &mut rects,
        center_x - half,
        vertical_top,
        center_x + half,
        vertical_bottom,
        width,
        height,
    );

    rasterize_quarter_arc(
        &mut rects,
        arc_center_x,
        arc_center_y,
        radius,
        half,
        x_direction,
        y_direction,
        width,
        height,
    );
    rects
}

#[allow(clippy::too_many_arguments)]
fn rasterize_quarter_arc(
    rects: &mut Vec<PixelRect>,
    arc_center_x: f32,
    arc_center_y: f32,
    radius: f32,
    half_thickness: f32,
    x_direction: f32,
    y_direction: f32,
    width: i32,
    height: i32,
) {
    let outer_radius = radius + half_thickness;
    let inner_radius = (radius - half_thickness).max(0.0);
    // Integrate each physical pixel's exact overlap with the quarter-annulus. Unlike tessellated
    // opaque quads, fractional edge coverage smooths both circular boundaries without changing
    // the sharp, integer-aligned arm rectangles.
    for y in 0..height {
        for x in 0..width {
            let (u0, u1) = directed_interval(x as f32, (x + 1) as f32, arc_center_x, x_direction);
            let (v0, v1) = directed_interval(y as f32, (y + 1) as f32, arc_center_y, y_direction);
            let u0 = u0.max(0.0);
            let v0 = v0.max(0.0);
            if u1 <= u0 || v1 <= v0 {
                continue;
            }
            let coverage = (circle_rect_area(outer_radius, u0, u1, v0, v1)
                - circle_rect_area(inner_radius, u0, u1, v0, v1))
            .clamp(0.0, 1.0);
            if coverage > 0.0 {
                rects.push(PixelRect {
                    left: x as f32,
                    top: y as f32,
                    right: (x + 1) as f32,
                    bottom: (y + 1) as f32,
                    coverage,
                });
            }
        }
    }
}

fn directed_interval(start: f32, end: f32, center: f32, direction: f32) -> (f32, f32) {
    let first = direction * (center - start);
    let second = direction * (center - end);
    (first.min(second), first.max(second))
}

fn circle_rect_area(radius: f32, u0: f32, u1: f32, v0: f32, v1: f32) -> f32 {
    if radius <= 0.0 || u1 <= u0 || v1 <= v0 || u0 >= radius || v0 >= radius {
        return 0.0;
    }

    let full_until = circle_extent(radius, u1);
    let partial_until = circle_extent(radius, u0);
    let full_end = v1.min(full_until);
    let full_area = (u1 - u0) * (full_end - v0).max(0.0);
    let partial_start = v0.max(full_until);
    let partial_end = v1.min(partial_until);
    if partial_end <= partial_start {
        return full_area;
    }

    full_area + circle_segment_primitive(radius, partial_end)
        - circle_segment_primitive(radius, partial_start)
        - u0 * (partial_end - partial_start)
}

fn circle_extent(radius: f32, coordinate: f32) -> f32 {
    (radius.mul_add(radius, -(coordinate * coordinate)))
        .max(0.0)
        .sqrt()
}

fn circle_segment_primitive(radius: f32, coordinate: f32) -> f32 {
    let coordinate = coordinate.clamp(0.0, radius);
    let extent = circle_extent(radius, coordinate);
    0.5 * (coordinate * extent + radius * radius * (coordinate / radius).asin())
}

#[allow(clippy::too_many_arguments)]
fn push_clipped_rect(
    rects: &mut Vec<PixelRect>,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    width: i32,
    height: i32,
) {
    let left = left.clamp(0.0, width as f32);
    let top = top.clamp(0.0, height as f32);
    let right = right.clamp(0.0, width as f32);
    let bottom = bottom.clamp(0.0, height as f32);
    if right <= left || bottom <= top {
        return;
    }
    rects.push(PixelRect {
        left,
        top,
        right,
        bottom,
        coverage: 1.0,
    });
}

fn block_geometry(character: char, width: i32, height: i32) -> Vec<PixelRect> {
    let mut rects = Vec::new();
    match character {
        '\u{2580}' => push_rect(&mut rects, 0, 0, width, fraction(height, 4, 8)),
        '\u{2581}'..='\u{2587}' => {
            let eighths = character as i32 - '\u{2580}' as i32;
            push_rect(
                &mut rects,
                0,
                height - fraction(height, eighths, 8),
                width,
                height,
            );
        }
        '\u{2588}' => push_rect(&mut rects, 0, 0, width, height),
        '\u{2589}'..='\u{258f}' => {
            let eighths = 16 - (character as i32 - '\u{2580}' as i32);
            push_rect(&mut rects, 0, 0, fraction(width, eighths, 8), height);
        }
        '\u{2590}' => push_rect(&mut rects, fraction(width, 4, 8), 0, width, height),
        '\u{2591}' => shade_geometry(&mut rects, width, height, 1),
        '\u{2592}' => shade_geometry(&mut rects, width, height, 2),
        '\u{2593}' => shade_geometry(&mut rects, width, height, 3),
        '\u{2594}' => push_rect(&mut rects, 0, 0, width, fraction(height, 1, 8)),
        '\u{2595}' => push_rect(&mut rects, width - fraction(width, 1, 8), 0, width, height),
        '\u{2596}' => quadrant(&mut rects, width, height, false, false),
        '\u{2597}' => quadrant(&mut rects, width, height, true, false),
        '\u{2598}' => quadrant(&mut rects, width, height, false, true),
        '\u{2599}' => {
            quadrant(&mut rects, width, height, false, true);
            push_rect(&mut rects, 0, fraction(height, 4, 8), width, height);
        }
        '\u{259a}' => {
            quadrant(&mut rects, width, height, false, true);
            quadrant(&mut rects, width, height, true, false);
        }
        '\u{259b}' => {
            push_rect(&mut rects, 0, 0, width, fraction(height, 4, 8));
            quadrant(&mut rects, width, height, false, false);
        }
        '\u{259c}' => {
            push_rect(&mut rects, 0, 0, width, fraction(height, 4, 8));
            quadrant(&mut rects, width, height, true, false);
        }
        '\u{259d}' => quadrant(&mut rects, width, height, true, true),
        '\u{259e}' => {
            quadrant(&mut rects, width, height, true, true);
            quadrant(&mut rects, width, height, false, false);
        }
        '\u{259f}' => {
            quadrant(&mut rects, width, height, true, true);
            push_rect(&mut rects, 0, fraction(height, 4, 8), width, height);
        }
        _ => {}
    }
    rects
}

fn fraction(extent: i32, numerator: i32, denominator: i32) -> i32 {
    (extent * numerator + denominator / 2) / denominator
}

fn quadrant(rects: &mut Vec<PixelRect>, width: i32, height: i32, right: bool, upper: bool) {
    let half_width = fraction(width, 4, 8);
    let half_height = fraction(height, 4, 8);
    let (left, right_edge) = if right {
        (half_width, width)
    } else {
        (0, half_width)
    };
    let (top, bottom) = if upper {
        (0, half_height)
    } else {
        (half_height, height)
    };
    push_rect(rects, left, top, right_edge, bottom);
}

fn shade_geometry(rects: &mut Vec<PixelRect>, width: i32, height: i32, level: i32) {
    for y in 0..height {
        let mut run_start = None;
        for x in 0..=width {
            let filled = x < width && (x + 2 * y).rem_euclid(4) < level;
            match (run_start, filled) {
                (None, true) => run_start = Some(x),
                (Some(start), false) => {
                    push_rect(rects, start, y, x, y + 1);
                    run_start = None;
                }
                _ => {}
            }
        }
    }
}

fn push_rect(rects: &mut Vec<PixelRect>, left: i32, top: i32, right: i32, bottom: i32) {
    if right <= left || bottom <= top {
        return;
    }
    rects.push(PixelRect {
        left: left as f32,
        top: top as f32,
        right: right as f32,
        bottom: bottom as f32,
        coverage: 1.0,
    });
}

#[rustfmt::skip]
fn line_shape(character: char) -> Option<LineShape> {
    use Stroke::{Double as D, Heavy as H, Light as L, None as N};
    let shape = match character {
        '─' => LineShape::new(N, L, N, L), '━' => LineShape::new(N, H, N, H),
        '│' => LineShape::new(L, N, L, N), '┃' => LineShape::new(H, N, H, N),
        '┄' => LineShape::dashed(Axis::Horizontal, 3, L), '┅' => LineShape::dashed(Axis::Horizontal, 3, H),
        '┆' => LineShape::dashed(Axis::Vertical, 3, L), '┇' => LineShape::dashed(Axis::Vertical, 3, H),
        '┈' => LineShape::dashed(Axis::Horizontal, 4, L), '┉' => LineShape::dashed(Axis::Horizontal, 4, H),
        '┊' => LineShape::dashed(Axis::Vertical, 4, L), '┋' => LineShape::dashed(Axis::Vertical, 4, H),
        '┌' => LineShape::new(N, L, L, N), '┍' => LineShape::new(N, H, L, N),
        '┎' => LineShape::new(N, L, H, N), '┏' => LineShape::new(N, H, H, N),
        '┐' => LineShape::new(N, N, L, L), '┑' => LineShape::new(N, N, L, H),
        '┒' => LineShape::new(N, N, H, L), '┓' => LineShape::new(N, N, H, H),
        '└' => LineShape::new(L, L, N, N), '┕' => LineShape::new(L, H, N, N),
        '┖' => LineShape::new(H, L, N, N), '┗' => LineShape::new(H, H, N, N),
        '┘' => LineShape::new(L, N, N, L), '┙' => LineShape::new(L, N, N, H),
        '┚' => LineShape::new(H, N, N, L), '┛' => LineShape::new(H, N, N, H),
        '├' => LineShape::new(L, L, L, N), '┝' => LineShape::new(L, H, L, N),
        '┞' => LineShape::new(H, L, L, N), '┟' => LineShape::new(L, L, H, N),
        '┠' => LineShape::new(H, L, H, N), '┡' => LineShape::new(H, H, L, N),
        '┢' => LineShape::new(L, H, H, N), '┣' => LineShape::new(H, H, H, N),
        '┤' => LineShape::new(L, N, L, L), '┥' => LineShape::new(L, N, L, H),
        '┦' => LineShape::new(H, N, L, L), '┧' => LineShape::new(L, N, H, L),
        '┨' => LineShape::new(H, N, H, L), '┩' => LineShape::new(H, N, L, H),
        '┪' => LineShape::new(L, N, H, H), '┫' => LineShape::new(H, N, H, H),
        '┬' => LineShape::new(N, L, L, L), '┭' => LineShape::new(N, L, L, H),
        '┮' => LineShape::new(N, H, L, L), '┯' => LineShape::new(N, H, L, H),
        '┰' => LineShape::new(N, L, H, L), '┱' => LineShape::new(N, L, H, H),
        '┲' => LineShape::new(N, H, H, L), '┳' => LineShape::new(N, H, H, H),
        '┴' => LineShape::new(L, L, N, L), '┵' => LineShape::new(L, L, N, H),
        '┶' => LineShape::new(L, H, N, L), '┷' => LineShape::new(L, H, N, H),
        '┸' => LineShape::new(H, L, N, L), '┹' => LineShape::new(H, L, N, H),
        '┺' => LineShape::new(H, H, N, L), '┻' => LineShape::new(H, H, N, H),
        '┼' => LineShape::new(L, L, L, L), '┽' => LineShape::new(L, L, L, H),
        '┾' => LineShape::new(L, H, L, L), '┿' => LineShape::new(L, H, L, H),
        '╀' => LineShape::new(H, L, L, L), '╁' => LineShape::new(L, L, H, L),
        '╂' => LineShape::new(H, L, H, L), '╃' => LineShape::new(H, L, L, H),
        '╄' => LineShape::new(H, H, L, L), '╅' => LineShape::new(L, L, H, H),
        '╆' => LineShape::new(L, H, H, L), '╇' => LineShape::new(H, H, L, H),
        '╈' => LineShape::new(L, H, H, H), '╉' => LineShape::new(H, L, H, H),
        '╊' => LineShape::new(H, H, H, L), '╋' => LineShape::new(H, H, H, H),
        '╌' => LineShape::dashed(Axis::Horizontal, 2, L), '╍' => LineShape::dashed(Axis::Horizontal, 2, H),
        '╎' => LineShape::dashed(Axis::Vertical, 2, L), '╏' => LineShape::dashed(Axis::Vertical, 2, H),
        '═' => LineShape::new(N, D, N, D), '║' => LineShape::new(D, N, D, N),
        '╒' => LineShape::new(N, D, L, N), '╓' => LineShape::new(N, L, D, N),
        '╔' => LineShape::new(N, D, D, N), '╕' => LineShape::new(N, N, L, D),
        '╖' => LineShape::new(N, N, D, L), '╗' => LineShape::new(N, N, D, D),
        '╘' => LineShape::new(L, D, N, N), '╙' => LineShape::new(D, L, N, N),
        '╚' => LineShape::new(D, D, N, N), '╛' => LineShape::new(L, N, N, D),
        '╜' => LineShape::new(D, N, N, L), '╝' => LineShape::new(D, N, N, D),
        '╞' => LineShape::new(L, D, L, N), '╟' => LineShape::new(D, L, D, N),
        '╠' => LineShape::new(D, D, D, N), '╡' => LineShape::new(L, N, L, D),
        '╢' => LineShape::new(D, N, D, L), '╣' => LineShape::new(D, N, D, D),
        '╤' => LineShape::new(N, D, L, D), '╥' => LineShape::new(N, L, D, L),
        '╦' => LineShape::new(N, D, D, D), '╧' => LineShape::new(L, D, N, D),
        '╨' => LineShape::new(D, L, N, L), '╩' => LineShape::new(D, D, N, D),
        '╪' => LineShape::new(L, D, L, D), '╫' => LineShape::new(D, L, D, L),
        '╬' => LineShape::new(D, D, D, D),
        '╭' => LineShape::rounded(N, L, L, N), '╮' => LineShape::rounded(N, N, L, L),
        '╯' => LineShape::rounded(L, N, N, L), '╰' => LineShape::rounded(L, L, N, N),
        '╴' => LineShape::new(N, N, N, L), '╵' => LineShape::new(L, N, N, N),
        '╶' => LineShape::new(N, L, N, N), '╷' => LineShape::new(N, N, L, N),
        '╸' => LineShape::new(N, N, N, H), '╹' => LineShape::new(H, N, N, N),
        '╺' => LineShape::new(N, H, N, N), '╻' => LineShape::new(N, N, H, N),
        '╼' => LineShape::new(N, H, N, L), '╽' => LineShape::new(L, N, H, N),
        '╾' => LineShape::new(N, L, N, H), '╿' => LineShape::new(H, N, L, N),
        _ => return None,
    };
    Some(shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(character: char, width: f32, height: f32, line_width: f32) -> Vec<PixelRect> {
        geometry(character, 0.0, 0.0, width, height, line_width).expect("supported geometry")
    }

    #[test]
    fn core_lines_and_blocks_use_exact_cell_geometry() {
        assert_eq!(
            local('─', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 0.0,
                top: 9.0,
                right: 10.0,
                bottom: 10.0,
                coverage: 1.0,
            }]
        );
        assert_eq!(
            local('│', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 4.0,
                top: 0.0,
                right: 5.0,
                bottom: 20.0,
                coverage: 1.0,
            }]
        );
        assert_eq!(
            local('█', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 20.0,
                coverage: 1.0,
            }]
        );
        assert_eq!(
            local('▀', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0,
                coverage: 1.0,
            }]
        );
        assert_eq!(
            local('▄', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 0.0,
                top: 10.0,
                right: 10.0,
                bottom: 20.0,
                coverage: 1.0,
            }]
        );
        assert_eq!(
            local('▌', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 0.0,
                top: 0.0,
                right: 5.0,
                bottom: 20.0,
                coverage: 1.0,
            }]
        );
        assert_eq!(
            local('▐', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 5.0,
                top: 0.0,
                right: 10.0,
                bottom: 20.0,
                coverage: 1.0,
            }]
        );
    }

    #[test]
    fn corner_arms_reach_both_cell_edges() {
        let rects = local('┌', 10.0, 20.0, 1.0);
        assert!(rects.iter().any(|rect| rect.right == 10.0));
        assert!(rects.iter().any(|rect| rect.bottom == 20.0));

        for corner in ['╭', '╮', '╯', '╰'] {
            assert!(local(corner, 10.0, 20.0, 2.0).iter().all(|rect| {
                rect.left >= 0.0 && rect.top >= 0.0 && rect.right <= 10.0 && rect.bottom <= 20.0
            }));
        }

        for (corner, horizontal_edge, vertical_edge) in [
            ('╭', 10.0, 20.0),
            ('╮', 0.0, 20.0),
            ('╯', 0.0, 0.0),
            ('╰', 10.0, 0.0),
        ] {
            let rects = local(corner, 10.0, 20.0, 2.0);
            assert!(
                rects
                    .iter()
                    .any(|rect| { rect.left == horizontal_edge || rect.right == horizontal_edge })
            );
            assert!(
                rects
                    .iter()
                    .any(|rect| rect.top == vertical_edge || rect.bottom == vertical_edge)
            );
        }

        let horizontal_lane = local('─', 10.0, 20.0, 2.0)[0];
        let vertical_lane = local('│', 10.0, 20.0, 2.0)[0];
        let upper_left = local('╭', 10.0, 20.0, 2.0);
        let right_arm = upper_left.iter().find(|rect| rect.right == 10.0).unwrap();
        let down_arm = upper_left.iter().find(|rect| rect.bottom == 20.0).unwrap();
        assert_eq!(
            (right_arm.top, right_arm.bottom),
            (horizontal_lane.top, horizontal_lane.bottom)
        );
        assert_eq!(
            (down_arm.left, down_arm.right),
            (vertical_lane.left, vertical_lane.right)
        );
    }

    #[test]
    fn rounded_corner_rasterizes_fractional_one_pixel_coverage() {
        for scale in [1.5, 2.0] {
            let rects = local('╭', 20.0 * scale, 30.0 * scale, scale);
            let edge_pixels = rects
                .iter()
                .filter(|rect| rect.coverage > 0.0 && rect.coverage < 1.0)
                .collect::<Vec<_>>();
            assert!(
                !edge_pixels.is_empty(),
                "scale {scale}: rounded edge must contain fractional coverage"
            );
            for rect in edge_pixels {
                assert_eq!(rect.right - rect.left, 1.0);
                assert_eq!(rect.bottom - rect.top, 1.0);
                assert_eq!(rect.left.fract(), 0.0);
                assert_eq!(rect.top.fract(), 0.0);
            }
            assert!(
                rects.iter().any(|rect| rect.coverage == 1.0),
                "scale {scale}: the corner arms and arc interior must remain opaque"
            );
        }
    }

    #[test]
    fn quarter_annulus_coverage_matches_its_analytic_area() {
        let mut arc = Vec::new();
        let radius = 4.0;
        let half_thickness = 0.5;
        rasterize_quarter_arc(
            &mut arc,
            9.0,
            14.0,
            radius,
            half_thickness,
            1.0,
            1.0,
            10,
            20,
        );
        let measured = arc.iter().map(|rect| rect.coverage).sum::<f32>();
        let outer = radius + half_thickness;
        let inner = radius - half_thickness;
        let expected = std::f32::consts::FRAC_PI_4 * (outer * outer - inner * inner);
        assert!(
            (measured - expected).abs() < 0.0001,
            "{measured} != {expected}"
        );
    }

    #[test]
    fn every_rounded_corner_bulges_toward_the_cell_outside_corner() {
        let cell_center = (5.0_f32, 10.0_f32);
        let radius = 4.0_f32;
        for (corner, x_direction, y_direction, outside_corner) in [
            ('╭', 1.0, 1.0, (0.0, 0.0)),
            ('╮', -1.0, 1.0, (10.0, 0.0)),
            ('╯', -1.0, -1.0, (10.0, 20.0)),
            ('╰', 1.0, -1.0, (0.0, 20.0)),
        ] {
            let arc_center = (
                cell_center.0 + x_direction * radius,
                cell_center.1 + y_direction * radius,
            );
            let mut arc = Vec::new();
            rasterize_quarter_arc(
                &mut arc,
                arc_center.0,
                arc_center.1,
                radius,
                0.5,
                x_direction,
                y_direction,
                10,
                20,
            );
            let total_coverage = arc.iter().map(|rect| rect.coverage).sum::<f32>();
            let arc_center_of_mass = arc.iter().fold((0.0, 0.0), |sum, rect| {
                (
                    sum.0 + (rect.left + 0.5) * rect.coverage,
                    sum.1 + (rect.top + 0.5) * rect.coverage,
                )
            });
            let arc_center_of_mass = (
                arc_center_of_mass.0 / total_coverage,
                arc_center_of_mass.1 / total_coverage,
            );
            let chord_midpoint = (
                arc_center.0 - x_direction * radius / 2.0,
                arc_center.1 - y_direction * radius / 2.0,
            );
            let distance_to_outside = |point: (f32, f32)| {
                ((point.0 - outside_corner.0).powi(2) + (point.1 - outside_corner.1).powi(2)).sqrt()
            };
            assert!(
                distance_to_outside(arc_center_of_mass) < distance_to_outside(chord_midpoint),
                "{corner} must bulge toward its outside corner"
            );
        }
    }

    #[test]
    fn line_width_scales_in_whole_physical_pixels() {
        assert_eq!(
            local('─', 10.0, 20.0, 2.0),
            vec![PixelRect {
                left: 0.0,
                top: 9.0,
                right: 10.0,
                bottom: 11.0,
                coverage: 1.0,
            }]
        );
    }

    #[test]
    fn adjacent_horizontal_and_vertical_cells_have_zero_gap() {
        let horizontal = (0..3)
            .flat_map(|column| geometry('─', column as f32 * 10.0, 0.0, 10.0, 20.0, 1.0).unwrap())
            .collect::<Vec<_>>();
        for boundary in [10.0, 20.0] {
            assert!(horizontal.iter().any(|rect| rect.right == boundary));
            assert!(horizontal.iter().any(|rect| rect.left == boundary));
        }

        let vertical = (0..3)
            .flat_map(|row| geometry('│', 0.0, row as f32 * 20.0, 10.0, 20.0, 1.0).unwrap())
            .collect::<Vec<_>>();
        for boundary in [20.0, 40.0] {
            let above = vertical
                .iter()
                .find(|rect| rect.bottom == boundary)
                .unwrap();
            let below = vertical.iter().find(|rect| rect.top == boundary).unwrap();
            assert_eq!((above.left, above.right), (below.left, below.right));
        }
    }

    #[test]
    fn coverage_has_only_the_documented_box_drawing_fallbacks() {
        for codepoint in 0x2500..=0x257f {
            let character = char::from_u32(codepoint).unwrap();
            let supported = !(0x2571..=0x2573).contains(&codepoint);
            assert_eq!(supports(character), supported, "U+{codepoint:04X}");
            assert_eq!(
                geometry(character, 0.0, 0.0, 10.0, 20.0, 1.0).is_some(),
                supported,
                "U+{codepoint:04X} geometry"
            );
        }
        for codepoint in 0x2580..=0x259f {
            let character = char::from_u32(codepoint).unwrap();
            assert!(supports(character), "U+{codepoint:04X}");
            assert!(
                geometry(character, 0.0, 0.0, 10.0, 20.0, 1.0).is_some(),
                "U+{codepoint:04X} geometry"
            );
        }
    }
}
