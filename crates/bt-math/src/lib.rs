//! Sandboxed MiTeX -> Typst -> SVG -> resvg math-block rendering.

use std::{num::NonZeroU32, sync::OnceLock, time::Duration};

use mitex_spec_gen::DEFAULT_SPEC;
use thiserror::Error;
use typst_as_lib::{TypstEngine, typst_kit_options::TypstKitFontOptions};
use typst_layout::PagedDocument;
use typst_library::{
    foundations::{Dict, IntoValue, Str, Value},
    layout::{Frame, FrameItem},
};

pub const MAX_SOURCE_BYTES: usize = 8 * 1024;
pub const VERTICAL_PADDING_LOGICAL_PX: u32 = 8;
/// CPU raster budget. Wide display math is tiled to the GPU's per-axis texture limit later, so
/// rejecting it at an arbitrary 16K width would violate UI-UX §7.5's horizontal overflow rule.
pub const MAX_RASTER_BYTES: usize = 64 * 1024 * 1024;

const TYPST_TEMPLATE: &str = r#"
#import "specs/mod.typ": mitex-scope
#set page(width: auto, height: auto, margin: 0pt, fill: none)
#set text(size: sys.inputs.font_size * 1pt, fill: rgb(sys.inputs.red, sys.inputs.green, sys.inputs.blue))
#let converted = eval("$" + sys.inputs.source + "$", scope: mitex-scope)
#math.equation(block: true, converted)
"#;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MathRenderKey {
    pub dpi_milli: NonZeroU32,
    pub font_milli_pt: NonZeroU32,
    pub foreground_rgb: [u8; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct MathRaster {
    pub rgba: Vec<u8>,
    pub width_px: u32,
    pub height_px: u32,
    pub content_height_px: u32,
    pub ascent_px: f32,
    pub descent_px: f32,
    pub render_time: Duration,
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
}

pub struct MathEngine {
    engine: TypstEngine<typst_as_lib::TypstTemplateMainFile>,
}

impl MathEngine {
    pub fn new() -> Self {
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
                    .include_system_fonts(false)
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
        let compiled = self.engine.compile_with_input::<_, PagedDocument>(inputs);
        let document = compiled
            .output
            .map_err(|error| MathRenderError::Compile(error.to_string()))?;
        let page = document.pages().first().ok_or(MathRenderError::NoPage)?;
        let svg = typst_svg::svg(page, &Default::default());
        let (ascent_pt, descent_pt) = find_math_metrics(&page.frame)
            .or_else(|| fallback_math_metrics(&page.frame))
            .ok_or(MathRenderError::InvalidDimensions)?;
        rasterize_svg(&svg, key, ascent_pt, descent_pt, started.elapsed())
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

fn find_math_metrics(frame: &Frame) -> Option<(f64, f64)> {
    let mut best = frame.has_baseline().then(|| {
        (
            frame.height().to_pt(),
            frame.ascent().to_pt(),
            frame.descent().to_pt(),
        )
    });
    for (_, item) in frame.items() {
        if let FrameItem::Group(group) = item
            && let Some((ascent, descent)) = find_math_metrics(&group.frame)
        {
            let height = ascent + descent;
            if best.is_none_or(|current| height > current.0) {
                best = Some((height, ascent, descent));
            }
        }
    }
    best.map(|(_, ascent, descent)| (ascent, descent))
}

/// MiTeX custom-macro wrappers can clear a Typst frame's explicit baseline even though the
/// rendered frame remains a valid math box. Treat Typst's documented implicit bottom baseline as
/// the adapter baseline. This closes spike 03's nine same-shape h/d holes without guessing from
/// the SVG ink bounds.
fn fallback_math_metrics(frame: &Frame) -> Option<(f64, f64)> {
    (!frame.is_empty()).then(|| (frame.baseline().to_pt(), frame.descent().to_pt()))
}

fn rasterize_svg(
    svg: &str,
    key: MathRenderKey,
    ascent_pt: f64,
    descent_pt: f64,
    elapsed: Duration,
) -> Result<MathRaster, MathRenderError> {
    static OPTIONS: OnceLock<resvg::usvg::Options<'static>> = OnceLock::new();
    let options = OPTIONS.get_or_init(resvg::usvg::Options::default);
    let tree = resvg::usvg::Tree::from_str(svg, options)
        .map_err(|error| MathRenderError::Svg(error.to_string()))?;
    let scale = key.dpi_milli.get() as f32 / 1000.0 * 96.0 / 72.0;
    let source_size = tree.size();
    let width_px = (source_size.width() * scale).ceil().max(1.0) as u32;
    let content_height_px = (source_size.height() * scale).ceil().max(1.0) as u32;
    let padding_px =
        ((VERTICAL_PADDING_LOGICAL_PX as f32) * key.dpi_milli.get() as f32 / 1000.0).ceil() as u32;
    // UI-UX §7.5b, M1.9a ruling: keep the TeX box plus equal padding at free-pixel height.
    // Do not quantize this value to terminal row height; revisit only with SourceMap work.
    let height_px = content_height_px
        .checked_add(padding_px.saturating_mul(2))
        .ok_or(MathRenderError::InvalidDimensions)?;
    let resident_bytes = (width_px as usize)
        .checked_mul(height_px as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(MathRenderError::InvalidDimensions)?;
    if width_px > 131_072 || height_px > 16_384 || resident_bytes > MAX_RASTER_BYTES {
        return Err(MathRenderError::InvalidDimensions);
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width_px, height_px)
        .ok_or(MathRenderError::InvalidDimensions)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale)
        .post_translate(0.0, padding_px as f32 / scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.take();
    unpremultiply_srgb_rgba(&mut rgba);
    Ok(MathRaster {
        rgba,
        width_px,
        height_px,
        content_height_px,
        ascent_px: (ascent_pt as f32 * scale),
        descent_px: (descent_pt as f32 * scale),
        render_time: elapsed,
    })
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
        }
    }

    #[test]
    fn renders_native_rgba_with_free_pixel_height() {
        let raster = MathEngine::new()
            .render(r"\frac{1}{2}+\sqrt{x}", key())
            .unwrap();
        assert!(raster.width_px > 1);
        assert!(raster.height_px > raster.content_height_px);
        assert_eq!(
            raster.rgba.len(),
            raster.width_px as usize * raster.height_px as usize * 4
        );
        assert!(raster.ascent_px > 0.0);
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
            let width = raster.width_px as usize;
            let height = raster.height_px as usize;
            for (x, y) in [
                (0, 0),
                (width - 1, 0),
                (0, height - 1),
                (width - 1, height - 1),
            ] {
                assert_eq!(raster.rgba[(y * width + x) * 4 + 3], 0);
            }
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
        let engine = MathEngine::new();
        for sample in samples {
            let raster = engine
                .render(&sample.latex, key())
                .unwrap_or_else(|error| panic!("{}: {error}", sample.id));
            assert!(raster.ascent_px > 0.0);
            assert!(raster.height_px > 0);
        }
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
