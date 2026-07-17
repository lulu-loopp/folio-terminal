//! Cell-geometry rendering for Unicode box drawing and block elements.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PixelRect {
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) right: f32,
    pub(crate) bottom: f32,
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
    let x_start = (cx_start + cx_end - 1) / 2;
    let y_start = (cy_start + cy_end - 1) / 2;
    let x_end = if shape.right != Stroke::None {
        width - 1
    } else {
        0
    };
    let y_end = if shape.down != Stroke::None {
        height - 1
    } else {
        0
    };
    let steps = width.max(height).max(2);
    let mut rects = Vec::with_capacity(steps as usize + 1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let eased = t * t * (3.0 - 2.0 * t);
        let x = (x_start as f32 + (x_end - x_start) as f32 * eased).round() as i32;
        let y = (y_end as f32 + (y_start - y_end) as f32 * eased).round() as i32;
        let half = thickness / 2;
        push_rect(
            &mut rects,
            (x - half).clamp(0, width),
            (y - half).clamp(0, height),
            (x - half + thickness).clamp(0, width),
            (y - half + thickness).clamp(0, height),
        );
    }
    rects
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
                bottom: 10.0
            }]
        );
        assert_eq!(
            local('│', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 4.0,
                top: 0.0,
                right: 5.0,
                bottom: 20.0
            }]
        );
        assert_eq!(
            local('█', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 20.0
            }]
        );
        assert_eq!(
            local('▀', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0
            }]
        );
        assert_eq!(
            local('▄', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 0.0,
                top: 10.0,
                right: 10.0,
                bottom: 20.0
            }]
        );
        assert_eq!(
            local('▌', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 0.0,
                top: 0.0,
                right: 5.0,
                bottom: 20.0
            }]
        );
        assert_eq!(
            local('▐', 10.0, 20.0, 1.0),
            vec![PixelRect {
                left: 5.0,
                top: 0.0,
                right: 10.0,
                bottom: 20.0
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
    }

    #[test]
    fn line_width_scales_in_whole_physical_pixels() {
        assert_eq!(
            local('─', 10.0, 20.0, 2.0),
            vec![PixelRect {
                left: 0.0,
                top: 9.0,
                right: 10.0,
                bottom: 11.0
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
