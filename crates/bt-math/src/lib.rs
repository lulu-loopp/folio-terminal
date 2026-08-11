//! Sandboxed MiTeX -> Typst -> SVG -> resvg math-block rendering.

use std::{num::NonZeroU32, sync::OnceLock, time::Duration};

pub use bt_doc::{InlineRunPlacement, MathMode};
use mitex_spec_gen::DEFAULT_SPEC;
use thiserror::Error;
use typst_as_lib::{TypstEngine, typst_kit_options::TypstKitFontOptions};
use typst_layout::PagedDocument;
use typst_library::{
    foundations::{Dict, IntoValue, Str, Value},
    layout::{Frame, FrameItem, Point, Transform},
};

pub const MAX_SOURCE_BYTES: usize = 8 * 1024;
pub const VERTICAL_PADDING_LOGICAL_PX: u32 = 8;
/// CPU raster budget. Wide display math is tiled to the GPU's per-axis texture limit later, so
/// rejecting it at an arbitrary 16K width would violate UI-UX §7.5's horizontal overflow rule.
pub const MAX_RASTER_BYTES: usize = 64 * 1024 * 1024;

const TYPST_TEMPLATE: &str = r#"
#import "specs/mod.typ": mitex-scope as base-mitex-scope
#set page(width: auto, height: auto, margin: (x: 0pt, y: sys.inputs.font_size * 1pt), fill: none)
#set text(size: sys.inputs.font_size * 1pt, fill: rgb(sys.inputs.red, sys.inputs.green, sys.inputs.blue))
#let mitex-scope = base-mitex-scope + (
  diff: math.partial,
  sect: math.inter,
  planck: symbol("ħ", ("reduce", "ℏ")),
)
#let source = if sys.inputs.display {
  "$ " + sys.inputs.source + " $"
} else {
  "$" + sys.inputs.source + "$"
}
#eval(source, scope: mitex-scope)
"#;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MathRenderKey {
    pub dpi_milli: NonZeroU32,
    pub font_milli_pt: NonZeroU32,
    pub foreground_rgb: [u8; 3],
    pub mode: MathMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MathRaster {
    pub rgba: Vec<u8>,
    pub width_px: u32,
    pub height_px: u32,
    pub content_height_px: u32,
    pub ascent_px: f32,
    pub descent_px: f32,
    /// Math baseline measured from the top of the alpha-tight raster.
    pub baseline_px: f32,
    pub render_time: Duration,
    /// For an inline composite: the runs this image actually contains, and where. Empty for a
    /// display block and for a single-run engine raster — the compositor fills it in.
    pub inline_runs: Vec<InlineRunPlacement>,
}

impl MathRaster {
    pub fn resident_bytes(&self) -> usize {
        self.rgba.len()
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum MathRenderError {
    #[error("worker scan found no conservative block-math match")]
    NotDetected,
    #[error("math source exceeds the 8 KiB block limit")]
    SourceTooLong,
    #[error("math source contains a disabled file or network command")]
    UnsafeCommand,
    #[error("math source nesting exceeds 256")]
    NestingTooDeep,
    #[error("MiTeX conversion failed: {0}")]
    Convert(String),
    #[error("Typst compilation failed: {0}")]
    Compile(String),
    #[error("Typst returned no page")]
    NoPage,
    #[error("SVG parsing failed: {0}")]
    Svg(String),
    #[error("raster dimensions are invalid or too large")]
    InvalidDimensions,
    #[error("inline math does not fit its terminal line box")]
    InlineGeometry,
    #[error("no installed font provides every requested CJK glyph")]
    MissingCjkGlyph,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MathFailureStage {
    Validate,
    Convert,
    Compile,
}

impl MathRenderError {
    pub fn failure_stage(&self) -> Option<MathFailureStage> {
        match self {
            Self::SourceTooLong | Self::UnsafeCommand | Self::NestingTooDeep => {
                Some(MathFailureStage::Validate)
            }
            Self::Convert(_) => Some(MathFailureStage::Convert),
            Self::Compile(_) | Self::NoPage | Self::Svg(_) | Self::InvalidDimensions => {
                Some(MathFailureStage::Compile)
            }
            Self::NotDetected | Self::InlineGeometry | Self::MissingCjkGlyph => None,
        }
    }
}

pub struct MathEngine {
    engine: TypstEngine<typst_as_lib::TypstTemplateMainFile>,
}

impl MathEngine {
    pub fn new() -> Self {
        Self::with_system_fonts(true)
    }

    fn with_system_fonts(include_system_fonts: bool) -> Self {
        let engine = TypstEngine::builder()
            .main_file(TYPST_TEMPLATE)
            .with_static_source_file_resolver([
                (
                    "specs/mod.typ",
                    include_str!("../../../spikes/03-math-engine/assets/specs/mod.typ"),
                ),
                (
                    "specs/prelude.typ",
                    include_str!("../../../spikes/03-math-engine/assets/specs/prelude.typ"),
                ),
                (
                    "specs/latex/standard.typ",
                    include_str!("../../../spikes/03-math-engine/assets/specs/latex/standard.typ"),
                ),
            ])
            .search_fonts_with(
                TypstKitFontOptions::default()
                    .include_system_fonts(include_system_fonts)
                    .include_embedded_fonts(true),
            )
            .build();
        Self { engine }
    }

    pub fn render(&self, source: &str, key: MathRenderKey) -> Result<MathRaster, MathRenderError> {
        let started = std::time::Instant::now();
        validate_source(source)?;
        let converted = mitex::convert_math(source, Some(DEFAULT_SPEC.clone()))
            .map_err(|error| MathRenderError::Convert(error.to_string()))?;
        let mut inputs = Dict::new();
        inputs.insert("source".into(), Value::Str(Str::from(converted)));
        inputs.insert(
            "font_size".into(),
            (f64::from(key.font_milli_pt.get()) / 1000.0).into_value(),
        );
        inputs.insert("red".into(), key.foreground_rgb[0].into_value());
        inputs.insert("green".into(), key.foreground_rgb[1].into_value());
        inputs.insert("blue".into(), key.foreground_rgb[2].into_value());
        inputs.insert(
            "display".into(),
            matches!(key.mode, MathMode::Display).into_value(),
        );
        let compiled = self.engine.compile_with_input::<_, PagedDocument>(inputs);
        let document = compiled
            .output
            .map_err(|error| MathRenderError::Compile(error.to_string()))?;
        let page = document.pages().first().ok_or(MathRenderError::NoPage)?;
        if frame_has_missing_cjk_glyph(&page.frame) {
            return Err(MathRenderError::MissingCjkGlyph);
        }
        let svg = typst_svg::svg(page, &Default::default());
        // The same number the template turned into `margin.y`, which is what makes the page's
        // content box — and therefore its baseline — recoverable from the page frame alone.
        let margin_pt = f64::from(key.font_milli_pt.get()) / 1000.0;
        let metrics = find_math_metrics(&page.frame)
            .or_else(|| fallback_math_metrics(&page.frame, margin_pt))
            .ok_or(MathRenderError::InvalidDimensions)?;
        rasterize_svg(&svg, key, metrics, started.elapsed())
    }
}

impl Default for MathEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_source(source: &str) -> Result<(), MathRenderError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(MathRenderError::SourceTooLong);
    }
    if [
        "\\input",
        "\\include",
        "\\includegraphics",
        "\\write",
        "\\openout",
    ]
    .iter()
    .any(|command| source.contains(command))
    {
        return Err(MathRenderError::UnsafeCommand);
    }
    let mut depth = 0_u16;
    for byte in source.bytes() {
        match byte {
            b'{' => {
                depth = depth.saturating_add(1);
                if depth > 256 {
                    return Err(MathRenderError::NestingTooDeep);
                }
            }
            b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct MathMetrics {
    baseline_from_page_top_pt: f64,
}

fn find_math_metrics(frame: &Frame) -> Option<MathMetrics> {
    fn visit(frame: &Frame, transform: Transform, best: &mut Option<(f64, MathMetrics)>) {
        if frame.has_baseline() {
            let top = Point::zero().transform(transform).y.to_pt();
            let baseline = Point::new(Default::default(), frame.baseline())
                .transform(transform)
                .y
                .to_pt();
            let bottom = Point::new(Default::default(), frame.height())
                .transform(transform)
                .y
                .to_pt();
            let height = (bottom - top).abs();
            let metrics = MathMetrics {
                baseline_from_page_top_pt: baseline,
            };
            if best.as_ref().is_none_or(|current| height > current.0) {
                *best = Some((height, metrics));
            }
        }
        for (position, item) in frame.items() {
            if let FrameItem::Group(group) = item {
                let child_transform = transform
                    .pre_concat(Transform::translate(position.x, position.y))
                    .pre_concat(group.transform);
                visit(&group.frame, child_transform, best);
            }
        }
    }

    let mut best = None;
    visit(frame, Transform::identity(), &mut best);
    best.map(|(_, metrics)| metrics)
}

/// The baseline read off the page's own geometry, for the majority of formulas that expose no
/// explicit baseline anywhere in their frame tree.
///
/// MiTeX custom-macro wrappers can clear a Typst frame's explicit baseline even though the
/// rendered frame remains a valid math box — and in practice most inline sources produce no group
/// frame at all, only bare text items sitting directly on the page. So this path, not
/// [`find_math_metrics`], is what actually measures `$x$`, `$y$` and `$\frac{a}{b}$`.
///
/// The page's *content box bottom* is the baseline, and that is structural rather than lucky:
/// Typst's default text bottom-edge is `"baseline"`, so an auto-height page ends exactly on the
/// baseline of its last line. Every main glyph of a one-line formula is laid down on that line —
/// measured directly, `x`, `y`, `E`, `=`, `m` and `c` all sit on it to the hundredth of a point,
/// while `\frac`'s denominator hangs a clear 8pt *below* it.
///
/// `margin_pt` is the vertical page margin this render asked for, and subtracting it is the whole
/// of the fix. The old form returned `frame.baseline()` — the page frame's implicit
/// bottom-of-frame, margin included — which was right only while the margin was zero and the page
/// bottom therefore coincided with the baseline. That coincidence is exactly why inline could not
/// be given the overshoot margin display already had: adding it moved the page bottom a full em
/// below the baseline while the arithmetic went on calling it one, and every inline raster
/// measured its baseline at the very bottom of its own ink, descenders and all.
fn fallback_math_metrics(frame: &Frame, margin_pt: f64) -> Option<MathMetrics> {
    (!frame.is_empty()).then(|| MathMetrics {
        baseline_from_page_top_pt: frame.height().to_pt() - margin_pt,
    })
}

fn frame_has_missing_cjk_glyph(frame: &Frame) -> bool {
    frame.items().any(|(_, item)| match item {
        FrameItem::Text(text) => {
            text.text.chars().any(is_cjk_character) && text.glyphs.iter().any(|glyph| glyph.id == 0)
        }
        FrameItem::Group(group) => frame_has_missing_cjk_glyph(&group.frame),
        _ => false,
    })
}

fn is_cjk_character(character: char) -> bool {
    matches!(
        character,
        '\u{3000}'..='\u{303f}'
            | '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{ff00}'..='\u{ffef}'
    )
}

fn rasterize_svg(
    svg: &str,
    key: MathRenderKey,
    metrics: MathMetrics,
    elapsed: Duration,
) -> Result<MathRaster, MathRenderError> {
    static OPTIONS: OnceLock<resvg::usvg::Options<'static>> = OnceLock::new();
    let options = OPTIONS.get_or_init(resvg::usvg::Options::default);
    let tree = resvg::usvg::Tree::from_str(svg, options)
        .map_err(|error| MathRenderError::Svg(error.to_string()))?;
    let scale = key.dpi_milli.get() as f32 / 1000.0 * 96.0 / 72.0;
    let source_size = tree.size();
    let width_px = (source_size.width() * scale).ceil().max(1.0) as u32;
    let source_height_px = (source_size.height() * scale).ceil().max(1.0) as u32;
    let source_resident_bytes = (width_px as usize)
        .checked_mul(source_height_px as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(MathRenderError::InvalidDimensions)?;
    if width_px > 131_072 || source_height_px > 16_384 || source_resident_bytes > MAX_RASTER_BYTES {
        return Err(MathRenderError::InvalidDimensions);
    }
    let mut source_pixmap = resvg::tiny_skia::Pixmap::new(width_px, source_height_px)
        .ok_or(MathRenderError::InvalidDimensions)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut source_pixmap.as_mut(),
    );
    let source_rgba = source_pixmap.take();
    // Typst's auto page is a layout frame, not an alpha-tight raster box. The shared artifact owns
    // ink only; transcript and live projections add their scale-appropriate symmetric breathing
    // outside these pixels. This also lets live->frozen handoff reuse the exact same RGBA bytes.
    let (mut rgba, content_height_px, content_top_px) =
        crop_vertical_alpha(&source_rgba, width_px).ok_or(MathRenderError::InvalidDimensions)?;
    let height_px = content_height_px;
    let resident_bytes = rgba.len();
    if width_px > 131_072 || height_px > 16_384 || resident_bytes > MAX_RASTER_BYTES {
        return Err(MathRenderError::InvalidDimensions);
    }
    unpremultiply_srgb_rgba(&mut rgba);
    // Typst reports the baseline in points; `scale` maps the SVG's own units to device pixels and
    // those units are CSS pixels, not points — `tree.size()` comes back as the page's point height
    // times 96/72. So a point becomes a device pixel through *both* factors, and using `scale`
    // alone undercounted the baseline by 4/3 of itself. That error is why the old inline metric
    // could only ever be described as coincidental: it placed the baseline a third of the way up
    // from where it belonged, and went unnoticed because the zero-margin page had already clipped
    // away every pixel below the baseline that could have shown the mistake.
    let device_px_per_pt = scale * 96.0 / 72.0;
    let baseline_px = (metrics.baseline_from_page_top_pt as f32 * device_px_per_pt
        - content_top_px as f32)
        .clamp(0.0, content_height_px as f32);
    Ok(MathRaster {
        rgba,
        width_px,
        height_px,
        content_height_px,
        ascent_px: baseline_px,
        descent_px: content_height_px as f32 - baseline_px,
        baseline_px,
        render_time: elapsed,
        inline_runs: Vec::new(),
    })
}

fn vertical_alpha_bounds(rgba: &[u8], width_px: u32) -> Option<(u32, u32)> {
    let row_bytes = width_px as usize * 4;
    if row_bytes == 0 || !rgba.len().is_multiple_of(row_bytes) {
        return None;
    }
    let rows = rgba.len() / row_bytes;
    let row_has_ink = |row: usize| {
        rgba[row * row_bytes..(row + 1) * row_bytes]
            .chunks_exact(4)
            .any(|pixel| pixel[3] != 0)
    };
    let first = (0..rows).find(|row| row_has_ink(*row))?;
    let last = (first..rows).rev().find(|row| row_has_ink(*row))? + 1;
    Some((first as u32, last as u32))
}

/// A standalone SVG document rasterized at its intrinsic size, in straight (unpremultiplied)
/// sRGB RGBA — the same byte contract the math rasters and decoded images share.
pub struct SvgRaster {
    pub rgba: Vec<u8>,
    pub width_px: u32,
    pub height_px: u32,
}

/// The two ways an SVG payload fails, kept apart so callers can classify honestly: bytes that do
/// not parse are simply not an SVG (an unsupported payload), while a valid document with an
/// absurd intrinsic size is a dimensions problem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SvgRasterError {
    Parse(String),
    Dimensions(String),
}

/// Rasterize a standalone SVG document at its intrinsic size (one user unit per pixel). Serves
/// the inline-image pipeline's SVG admission (M2 preview matrix §2: SVG displays as a static
/// raster); this crate owns the resvg dependency, so image decoding borrows the rasterizer
/// instead of growing its own.
pub fn rasterize_svg_document(bytes: &[u8]) -> Result<SvgRaster, SvgRasterError> {
    static OPTIONS: OnceLock<resvg::usvg::Options<'static>> = OnceLock::new();
    let options = OPTIONS.get_or_init(resvg::usvg::Options::default);
    let tree = resvg::usvg::Tree::from_data(bytes, options)
        .map_err(|error| SvgRasterError::Parse(error.to_string()))?;
    let size = tree.size();
    let width_px = size.width().ceil().max(1.0) as u32;
    let height_px = size.height().ceil().max(1.0) as u32;
    let resident_bytes = (width_px as usize)
        .checked_mul(height_px as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| SvgRasterError::Dimensions("svg raster dimensions overflow".to_owned()))?;
    if width_px > 16_384 || height_px > 16_384 || resident_bytes > MAX_RASTER_BYTES {
        return Err(SvgRasterError::Dimensions(format!(
            "svg intrinsic size {width_px}x{height_px} exceeds the raster budget"
        )));
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width_px, height_px)
        .ok_or_else(|| SvgRasterError::Dimensions("svg raster allocation failed".to_owned()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let mut rgba = pixmap.take();
    unpremultiply_srgb_rgba(&mut rgba);
    Ok(SvgRaster {
        rgba,
        width_px,
        height_px,
    })
}

fn crop_vertical_alpha(rgba: &[u8], width_px: u32) -> Option<(Vec<u8>, u32, u32)> {
    let (first, last) = vertical_alpha_bounds(rgba, width_px)?;
    let row_bytes = width_px as usize * 4;
    let start = first as usize * row_bytes;
    let end = last as usize * row_bytes;
    Some((rgba.get(start..end)?.to_vec(), last - first, first))
}

/// tiny-skia exposes premultiplied sRGB bytes. The renderer uploads to an sRGB texture and uses
/// straight-alpha blending, so undo byte-space premultiplication before the GPU decodes RGB to
/// linear light. Transparent pixels remain canonical transparent black.
fn unpremultiply_srgb_rgba(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn key() -> MathRenderKey {
        MathRenderKey {
            dpi_milli: NonZeroU32::new(1000).unwrap(),
            font_milli_pt: NonZeroU32::new(12_000).unwrap(),
            foreground_rgb: [224, 224, 224],
            mode: MathMode::Display,
        }
    }

    fn ink_row_runs(raster: &MathRaster, left: u32, right: u32) -> usize {
        let left = left.min(raster.width_px) as usize;
        let right = right.min(raster.width_px).max(left as u32) as usize;
        let minimum_ink = ((right.saturating_sub(left)) / 10).max(3);
        let row_bytes = raster.width_px as usize * 4;
        raster
            .rgba
            .chunks_exact(row_bytes)
            .map(|row| {
                row[left * 4..right * 4]
                    .chunks_exact(4)
                    .filter(|pixel| pixel[3] != 0)
                    .count()
                    >= minimum_ink
            })
            .fold((0, false), |(runs, previous), ink| {
                (runs + usize::from(ink && !previous), ink)
            })
            .0
    }

    fn alpha_signature(raster: &MathRaster) -> Vec<u32> {
        let row_bytes = raster.width_px as usize * 4;
        (0..raster.width_px as usize)
            .map(|column| {
                raster
                    .rgba
                    .chunks_exact(row_bytes)
                    .filter(|row| row[column * 4 + 3] != 0)
                    .count() as u32
            })
            .collect()
    }

    #[test]
    fn renders_native_rgba_with_free_pixel_height() {
        let raster = MathEngine::new()
            .render(r"\frac{1}{2}+\sqrt{x}", key())
            .unwrap();
        assert!(raster.width_px > 1);
        assert_eq!(raster.height_px, raster.content_height_px);
        assert_eq!(
            raster.rgba.len(),
            raster.width_px as usize * raster.height_px as usize * 4
        );
        assert!(raster.ascent_px > 0.0);
    }

    #[test]
    fn display_environments_remain_multiline_with_bounded_width() {
        let engine = MathEngine::new();
        let single = engine.render("x + y", key()).unwrap();
        let samples = [
            (
                "cases",
                r"\operatorname{sgn}(x)=\begin{cases}+1 & x>0\\0 & x=0\\-1 & x<0\end{cases}",
                3,
            ),
            (
                "pmatrix",
                r"A=\begin{pmatrix}a_{11}&a_{12}&a_{13}\\a_{21}&a_{22}&a_{23}\\a_{31}&a_{32}&a_{33}\end{pmatrix}",
                3,
            ),
            (
                "bmatrix",
                r"\begin{bmatrix}1&0\\0&1\end{bmatrix}\begin{bmatrix}x\\y\end{bmatrix}=\begin{bmatrix}x\\y\end{bmatrix}",
                2,
            ),
            (
                "aligned",
                r"\begin{aligned}a&=b+c\\d&=e+f\\g&=h+i\end{aligned}",
                3,
            ),
            (
                "align",
                r"\begin{align}(a+b)^2&=a^2+2ab+b^2\\(a-b)^2&=a^2-2ab+b^2\\(a+b)(a-b)&=a^2-b^2\end{align}",
                3,
            ),
        ];
        for (name, source, expected_rows) in samples {
            let raster = engine
                .render(source, key())
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(
                raster.height_px > single.height_px.saturating_mul(2),
                "{name} collapsed: {}x{} vs single {}x{}",
                raster.width_px,
                raster.height_px,
                single.width_px,
                single.height_px,
            );
            assert!(raster.width_px < 4096, "{name} width is abnormal");
            let margin = raster.width_px / 8;
            assert!(
                ink_row_runs(&raster, margin, raster.width_px - margin) >= expected_rows,
                "{name} did not preserve approximately {expected_rows} ink rows"
            );
        }
    }

    #[test]
    fn delimiter_mode_controls_the_eval_equation_without_a_second_wrapper() {
        let engine = MathEngine::new();
        let mut inline = key();
        inline.mode = MathMode::Inline;
        let display = engine.render(r"\sum_{i=1}^n i", key()).unwrap();
        let inline = engine.render(r"\sum_{i=1}^n i", inline).unwrap();
        assert!(display.height_px > inline.height_px);
        assert!(display.baseline_px > 0.0 && inline.baseline_px > 0.0);
    }

    #[test]
    fn cjk_text_uses_distinct_real_system_glyphs_and_mixed_math_stays_visible() {
        // System fonts remain external OS assets (for example Microsoft YaHei/DengXian on
        // Windows); BetterTerminal neither embeds nor redistributes their bytes.
        let engine = MathEngine::new();
        let middle = engine.render(r"\text{中}", key()).unwrap();
        let writing = engine.render(r"\text{文}", key()).unwrap();
        let chinese = engine.render(r"\text{中文}", key()).unwrap();
        assert!(chinese.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0));
        assert_ne!(
            (middle.width_px, middle.height_px, alpha_signature(&middle)),
            (
                writing.width_px,
                writing.height_px,
                alpha_signature(&writing)
            ),
            "two CJK characters must not collapse to one repeated .notdef box"
        );

        let latin = engine.render("x", key()).unwrap();
        let mixed = engine.render(r"x + \text{项目数}", key()).unwrap();
        assert!(mixed.width_px > latin.width_px);
        assert!(mixed.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn missing_cjk_font_returns_source_fallback_signal_instead_of_tofu() {
        assert_eq!(
            MathEngine::with_system_fonts(false).render(r"\text{中文}", key()),
            Err(MathRenderError::MissingCjkGlyph)
        );
    }

    #[test]
    fn baseline_is_measured_from_the_page_top_for_each_formula() {
        let engine = MathEngine::new();
        let samples = [
            "x",
            r"\frac{a}{b}",
            r"\sqrt{x}",
            r"x_1",
            r"x^2",
            r"\sum_{i=1}^n i",
            r"\int_0^1 x\,dx",
        ];
        let metrics = samples
            .map(|source| engine.render(source, key()).unwrap())
            .map(|raster| {
                (
                    raster.height_px,
                    raster.baseline_px,
                    raster.height_px as f32 - raster.baseline_px,
                )
            })
            .to_vec();
        let descents = metrics
            .iter()
            .map(|(_, _, descent)| (descent * 100.0).round() as i32)
            .collect::<std::collections::BTreeSet<_>>();
        eprintln!("per-formula baseline metrics: {metrics:?}");
        assert!(
            descents.len() >= 3,
            "formula descents must not repeat one page-local baseline constant: {metrics:?}"
        );
    }

    #[test]
    fn user_reported_partial_and_intersection_formulas_compile() {
        let engine = MathEngine::new();
        for (name, source) in [
            (
                "residue",
                r"f(z) = \frac{1}{2\pi i} \oint_{\gamma} \frac{f(\zeta)}{\zeta - z}\,\mathrm{d}\zeta, \quad \left| \frac{\partial^2 u}{\partial x^2} + \frac{\partial^2 u}{\partial y^2} \right| \leq \epsilon",
            ),
            (
                "maxwell",
                r"\begin{aligned} \nabla \cdot \mathbf{E} &= \frac{\rho}{\varepsilon_0} \\ \nabla \cdot \mathbf{B} &= 0 \\ \nabla \times \mathbf{E} &= -\frac{\partial \mathbf{B}}{\partial t} \\ \nabla \times \mathbf{B} &= \mu_0\mathbf{J} + \mu_0\varepsilon_0\frac{\partial \mathbf{E}}{\partial t} \end{aligned}",
            ),
            (
                "symbols",
                r"\alpha \beta \gamma \delta ; \Gamma \Delta \Theta \Lambda ; \aleph_0 \in \mathbb{R} \subseteq \mathbb{C}, \quad A \cup B, ; A \cap B, ; \varnothing",
            ),
        ] {
            let raster = engine
                .render(source, key())
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(raster.width_px > 1 && raster.height_px > 1);
        }
    }

    #[test]
    fn vertical_crop_removes_known_transparent_source_margins() {
        let width = 2_u32;
        let row_bytes = width as usize * 4;
        let mut source = vec![0_u8; row_bytes * 6];
        source[row_bytes * 2..row_bytes * 3].copy_from_slice(&[10, 20, 30, 255, 0, 0, 0, 0]);
        source[row_bytes * 3..row_bytes * 4].copy_from_slice(&[0, 0, 0, 0, 40, 50, 60, 128]);

        assert_eq!(vertical_alpha_bounds(&source, width), Some((2, 4)));
        let (cropped, height, top) = crop_vertical_alpha(&source, width).unwrap();
        assert_eq!(height, 2);
        assert_eq!(top, 2);
        assert_eq!(cropped.len(), row_bytes * 2);
        assert!(cropped.len() < source.len());
        assert_eq!(cropped, source[row_bytes * 2..row_bytes * 4]);
    }

    #[test]
    fn dark_and_light_theme_rasters_have_transparent_pages_and_theme_ink() {
        let engine = MathEngine::new();
        let mut dark_key = key();
        dark_key.foreground_rgb = [0xe8, 0xe8, 0xe8];
        let mut light_key = key();
        light_key.foreground_rgb = [0x18, 0x18, 0x18];

        let dark = engine.render(r"E = mc^2", dark_key).unwrap();
        let light = engine.render(r"E = mc^2", light_key).unwrap();
        assert_eq!(
            (dark.width_px, dark.height_px),
            (light.width_px, light.height_px)
        );
        assert_ne!(dark.rgba, light.rgba);

        for raster in [&dark, &light] {
            assert!(
                raster.rgba.chunks_exact(4).any(|pixel| pixel[3] == 0),
                "the alpha-tight page still preserves transparent background between ink"
            );
        }
        for (raster, ink) in [
            (&dark, dark_key.foreground_rgb),
            (&light, light_key.foreground_rgb),
        ] {
            assert!(
                raster
                    .rgba
                    .chunks_exact(4)
                    .any(|pixel| { pixel[3] == 255 && pixel[..3] == ink })
            );
        }
    }

    #[test]
    fn premultiplied_resvg_bytes_are_exported_as_straight_rgba() {
        let mut rgba = [64, 32, 16, 128, 9, 8, 7, 0, 4, 5, 6, 255];
        unpremultiply_srgb_rgba(&mut rgba);
        assert_eq!(rgba, [128, 64, 32, 128, 0, 0, 0, 0, 4, 5, 6, 255]);
    }

    #[test]
    fn svg_document_rasterizes_at_intrinsic_size_with_straight_alpha() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="6">
            <rect x="0" y="0" width="8" height="6" fill="#ff0000"/>
        </svg>"##;
        let raster = rasterize_svg_document(svg).unwrap();
        assert_eq!((raster.width_px, raster.height_px), (8, 6));
        assert_eq!(raster.rgba.len(), 8 * 6 * 4);
        assert_eq!(&raster.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn svg_raster_rejects_invalid_documents_and_absurd_intrinsic_sizes() {
        assert!(rasterize_svg_document(b"not an svg at all").is_err());
        let huge = br##"<svg xmlns="http://www.w3.org/2000/svg" width="99999" height="99999"/>"##;
        assert!(rasterize_svg_document(huge).is_err());
    }

    #[derive(Deserialize)]
    struct Sample {
        id: String,
        latex: String,
        expected_valid: bool,
    }

    #[test]
    fn spike_310_valid_input_gate_has_metrics_and_pixels() {
        let corpus = include_str!("../../../corpus/math-expressions.jsonl");
        let samples = corpus
            .lines()
            .map(|line| serde_json::from_str::<Sample>(line).unwrap())
            .filter(|sample| sample.expected_valid)
            .collect::<Vec<_>>();
        assert_eq!(samples.len(), 310);
        let sample_count = samples.len();
        let engine = MathEngine::new();
        let mut dimensions_fnv = 0xcbf29ce484222325_u64;
        let mut multiline_samples = 0usize;
        for sample in samples {
            let raster = engine
                .render(&sample.latex, key())
                .unwrap_or_else(|error| panic!("{}: {error}", sample.id));
            assert!(raster.ascent_px > 0.0);
            assert!(raster.height_px > 0);
            multiline_samples +=
                usize::from(sample.latex.contains(r"\begin") && sample.latex.contains(r"\\"));
            for byte in sample
                .id
                .bytes()
                .chain(raster.width_px.to_le_bytes())
                .chain(raster.height_px.to_le_bytes())
            {
                dimensions_fnv ^= u64::from(byte);
                dimensions_fnv = dimensions_fnv.wrapping_mul(0x100000001b3);
            }
        }
        eprintln!(
            "math corpus gate: {sample_count}/{sample_count} valid samples produced metrics and pixels; multiline_samples={multiline_samples}; dimensions_fnv={dimensions_fnv:016x}"
        );
    }

    #[test]
    fn rejects_product_cap_and_file_commands() {
        assert_eq!(
            MathEngine::new().render(&"x".repeat(MAX_SOURCE_BYTES + 1), key()),
            Err(MathRenderError::SourceTooLong)
        );
        assert_eq!(
            MathEngine::new().render(r"\input{secret}", key()),
            Err(MathRenderError::UnsafeCommand)
        );
    }
}

#[cfg(test)]
mod display_page_margin {
    use super::*;
    use std::num::NonZeroU32;

    /// Typst auto-sizes the page to the layout box, and glyph ink is allowed to overshoot that
    /// box (a first-row superscript, `\frac{\rho}{\varepsilon_0}` in an aligned row). With
    /// `margin: 0pt` that overshoot is rasterised off-page and silently lost, which the user saw
    /// as multi-line display blocks with their first line's top clipped on both screens. The
    /// display-only 1em vertical page margin captures the overshoot and `crop_vertical_alpha`
    /// trims the raster back to alpha-tight ink, so geometry stays tight while ink is complete.
    #[test]
    fn display_page_margin_preserves_overshooting_ink() {
        let engine = MathEngine::new();
        let key = MathRenderKey {
            dpi_milli: NonZeroU32::new(2000).unwrap(),
            font_milli_pt: NonZeroU32::new(24_000).unwrap(),
            foreground_rgb: [255, 255, 255],
            mode: MathMode::Display,
        };
        // Pinned against the vendored Typst + fonts at this exact render key. Reverting the page
        // margin to 0pt clips the aligned first row and drops these to 89/135/92.
        for (source, expected_ink_height) in [
            (
                r"\begin{aligned}(a+b)^2 &= a^2 + 2ab + b^2 \ (a-b)^2 &= a^2 - 2ab + b^2\end{aligned}",
                92,
            ),
            (
                r"\begin{aligned}\nabla \cdot \mathbf{E} &= \frac{\rho}{\varepsilon_0} \ \nabla \cdot \mathbf{B} &= 0\end{aligned}",
                177,
            ),
            (r"(a+b)^2 = a^2 + 2ab + b^2", 93),
        ] {
            let raster = engine.render(source, key).unwrap();
            assert_eq!(
                raster.content_height_px, expected_ink_height,
                "display ink must include layout-box overshoot for {source}"
            );
        }
    }

    /// PIN: an inline raster keeps every pixel it draws, above the baseline and below it.
    ///
    /// The handoff called the old inline metric "coincidental" and predicted a descender would be
    /// found clipped the day inline rendering was switched on. Both halves were true, and the
    /// measurement here is what makes them concrete: with `margin: 0pt` an inline page ran from the
    /// cap height to the baseline and *nothing else existed*, so `x` and `y` rasterised to byte-for
    /// -byte identical heights — the descender of the `y` was never drawn — and `\frac`'s
    /// denominator, which sits a clear 8pt below the baseline, was cut off at it.
    ///
    /// The invariants, stated as physics rather than as numbers, are that two glyphs sitting on one
    /// line share a baseline and differ only in how far their ink reaches from it, and that ink
    /// which belongs below the baseline is present.
    #[test]
    fn inline_rasters_keep_their_descenders_and_measure_a_true_baseline() {
        let engine = MathEngine::new();
        let key = MathRenderKey {
            dpi_milli: NonZeroU32::new(2000).unwrap(),
            font_milli_pt: NonZeroU32::new(24_000).unwrap(),
            foreground_rgb: [255, 255, 255],
            mode: MathMode::Inline,
        };
        let measure = |source: &str| {
            let raster = engine.render(source, key).unwrap();
            (
                raster.content_height_px,
                raster.baseline_px,
                raster.content_height_px as f32 - raster.baseline_px,
            )
        };

        // Two letters typeset on one line. Same baseline to the pixel; the `y` reaches further down
        // because it has a descender, and under the zero-margin page both measured 39/23.7 alike.
        let (x_height, x_baseline, x_descent) = measure("x");
        let (y_height, y_baseline, y_descent) = measure("y");
        assert_eq!(x_height, 39);
        assert_eq!(
            y_height, 55,
            "the `y`'s descender must be rasterised, not clipped"
        );
        assert!(
            (x_baseline - y_baseline).abs() < 0.01,
            "two glyphs on one line share a baseline: {x_baseline} vs {y_baseline}"
        );
        assert!(
            y_descent > x_descent + 10.0,
            "the descender must be measured *below* the baseline: {y_descent} vs {x_descent}"
        );

        // An expression with no descending part ends on its baseline, so essentially all of its ink
        // is ascent. Anti-aliasing bleeds about a pixel past it and that is the whole tolerance.
        let (_, _, flat_descent) = measure("E = mc^2");
        assert!(
            flat_descent < 2.0,
            "`E = mc^2` has nothing below the baseline, so its descent is ink bleed: {flat_descent}"
        );

        // The construction the handoff named. A third of a fraction's ink is its denominator, and
        // all of it belongs below the baseline.
        let (frac_height, frac_baseline, frac_descent) = measure(r"\frac{a}{b}");
        assert_eq!(frac_height, 91);
        assert!(
            frac_descent > 25.0 && frac_baseline > 25.0,
            "a fraction straddles its baseline: ascent {frac_baseline}, descent {frac_descent}"
        );
    }
}
