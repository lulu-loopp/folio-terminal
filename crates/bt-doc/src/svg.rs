//! The SVG codec's data types: what rasterizing a standalone SVG document produces and how it
//! fails.
//!
//! They are renderer-neutral — standard-library types only — so a crate that decodes an inline
//! image hands SVG bytes to whichever codec its host installed and reads the answer here, without
//! depending on the rasterizer. `bt-math`, which owns the rasterizer (`rasterize_svg_document`),
//! re-exports them at its root, so `bt_math::SvgRaster` and `bt_math::SvgRasterError` still name
//! these types.

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
