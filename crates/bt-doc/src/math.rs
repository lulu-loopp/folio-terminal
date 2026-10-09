//! The math data types: what a formula's render is keyed by, what it produces, how it fails and
//! the stage a failure belongs to.
//!
//! They are renderer-neutral — standard-library types and this crate's own, no Typst, image or
//! rasterizer type — so a crate that files, carries or answers a math render names them here
//! without depending on the engine that draws one. `bt-math`, the engine, re-exports them at its
//! root, so `bt_math::MathRaster` and the other three still name these types.

use std::{num::NonZeroU32, time::Duration};

use thiserror::Error;

use crate::{InlineRunPlacement, MathMode};

/// How deep a formula may nest before it is refused: the number
/// [`MathRenderError::NestingTooDeep`]'s message names.
///
/// The math engine enforces the limit and asserts at compile time that its own constant equals
/// this one, so the message cannot name a number the engine does not apply.
pub const MAX_NESTING_DEPTH: usize = 256;

/// The most laid-out cells a formula may ask for: the number
/// [`MathRenderError::TooManyLayoutCells`]'s message names.
///
/// The math engine's converter enforces the limit, and the engine asserts at compile time that the
/// converter's constant equals this one.
pub const MAX_LAYOUT_CELLS: usize = 4096;

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
    #[error("math source nesting exceeds {MAX_NESTING_DEPTH}")]
    NestingTooDeep,
    /// The formula asked to have Typst *code* run, rather than mathematics drawn.
    ///
    /// **A formula is not a program, and this is the sentence that makes that true.** MiTeX has
    /// three ways to copy source text into its output without mapping it — `\iftypst … \fi`'s body
    /// goes through whole, `\includegraphics`'s path lands inside a Typst string literal, and
    /// `\label`'s name lands in markup after a `<` — and Typst reads code at every one of them. A
    /// line of text a program printed could therefore have carried a loop that never ends (the
    /// math worker is one thread and cannot be interrupted), or one that builds content nested
    /// past what anything downstream will survive. The formula stays as the text that was printed.
    #[error("math source carries Typst code rather than mathematics")]
    RawTypstCode,
    /// The formula asked for a laid-out rectangle bigger than a formula may be.
    ///
    /// **The third budget, beside the bytes and the depth, and not implied by either.** An
    /// environment with rows is laid out as a rectangle: every row is padded out to the widest one
    /// and every cell of the result becomes an element. So a source that writes one wide row and a
    /// column of empty ones asks for their product — `\begin{array}{l}x` with N `&` and N `\\` is
    /// 3N+29 bytes at constant nesting and (N+1)² cells, which at the 8 KiB budget is more than
    /// seven million. The cost is linear in cells and measured, so [`MAX_LAYOUT_CELLS`]
    /// bounds the time and the memory of the whole formula rather than of one environment.
    #[error("math source asks for more than {} laid-out cells", MAX_LAYOUT_CELLS)]
    TooManyLayoutCells,
    #[error("math macro definitions contain a cycle")]
    MacroCycle,
    #[error("math macro expansion exceeds the work limit")]
    MacroExpansionLimit,
    #[error("math macro definition cannot be bounded safely")]
    UnboundedMacro,
    #[error("math conversion could not complete")]
    ConversionPanic,
    /// A panic anywhere else in the render — the Typst compile, the SVG, the rasterizer.
    ///
    /// One formula's render is one unit of work over owned inputs, so a fault inside it is that
    /// formula's failure and not the program's. Without this the process panic hook reached its
    /// fatal path and took every pane down with the formula.
    #[error("math rendering could not complete")]
    Aborted,
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
    /// A character the formula asked to see drawn came back `.notdef` — the
    /// page drew a blank or a box where a symbol belongs.
    ///
    /// This is [`Self::MissingCjkGlyph`]'s rule applied to the characters that
    /// rule used to walk past. A formula whose minus sign is not drawn is not a
    /// formula with a cosmetic flaw in it; it is a *different* formula, and
    /// showing the reader the source is the only honest answer left.
    #[error("no installed font provides a glyph for {0:?}")]
    MissingGlyph(char),
    /// The host declined to run this render.
    ///
    /// A completion, not a fault of the formula: the executor a host installs to run math tasks
    /// may decline one it does not run, and answers the task with this so the task does not stay
    /// outstanding. The composition layer's executor answers with it from CC-6b (a browser build's
    /// executor declines what it does not run); on native nothing produces it. It belongs to no
    /// failure stage.
    #[error("the host declined to run this math render")]
    HostDeclined,
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
            Self::SourceTooLong
            | Self::UnsafeCommand
            | Self::NestingTooDeep
            | Self::MacroCycle
            | Self::MacroExpansionLimit
            | Self::UnboundedMacro => Some(MathFailureStage::Validate),
            Self::Convert(_)
            | Self::ConversionPanic
            | Self::RawTypstCode
            | Self::TooManyLayoutCells => Some(MathFailureStage::Convert),
            Self::Compile(_)
            | Self::NoPage
            | Self::Svg(_)
            | Self::InvalidDimensions
            | Self::Aborted => Some(MathFailureStage::Compile),
            Self::NotDetected
            | Self::InlineGeometry
            | Self::MissingCjkGlyph
            | Self::MissingGlyph(_)
            | Self::HostDeclined => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample of every variant with the message it shows and the stage it belongs to.
    ///
    /// The message is what a pane shows in place of a formula that did not render, so it is
    /// pinned byte for byte. The payloads carry CJK and mixed-script text: a message interpolates
    /// what the engine said, and a non-ASCII payload must come through unchanged.
    fn every_variant() -> Vec<(MathRenderError, &'static str, Option<MathFailureStage>)> {
        use MathFailureStage::{Compile, Convert, Validate};
        vec![
            (
                MathRenderError::NotDetected,
                "worker scan found no conservative block-math match",
                None,
            ),
            (
                MathRenderError::SourceTooLong,
                "math source exceeds the 8 KiB block limit",
                Some(Validate),
            ),
            (
                MathRenderError::UnsafeCommand,
                "math source contains a disabled file or network command",
                Some(Validate),
            ),
            (
                MathRenderError::NestingTooDeep,
                "math source nesting exceeds 256",
                Some(Validate),
            ),
            (
                MathRenderError::RawTypstCode,
                "math source carries Typst code rather than mathematics",
                Some(Convert),
            ),
            (
                MathRenderError::TooManyLayoutCells,
                "math source asks for more than 4096 laid-out cells",
                Some(Convert),
            ),
            (
                MathRenderError::MacroCycle,
                "math macro definitions contain a cycle",
                Some(Validate),
            ),
            (
                MathRenderError::MacroExpansionLimit,
                "math macro expansion exceeds the work limit",
                Some(Validate),
            ),
            (
                MathRenderError::UnboundedMacro,
                "math macro definition cannot be bounded safely",
                Some(Validate),
            ),
            (
                MathRenderError::ConversionPanic,
                "math conversion could not complete",
                Some(Convert),
            ),
            (
                MathRenderError::Aborted,
                "math rendering could not complete",
                Some(Compile),
            ),
            (
                MathRenderError::Convert("\\frac{\u{5206}\u{5B50}}{x} unknown".to_owned()),
                "MiTeX conversion failed: \\frac{\u{5206}\u{5B50}}{x} unknown",
                Some(Convert),
            ),
            (
                MathRenderError::Compile("unknown variable: \u{89D2}\u{5EA6}".to_owned()),
                "Typst compilation failed: unknown variable: \u{89D2}\u{5EA6}",
                Some(Compile),
            ),
            (
                MathRenderError::NoPage,
                "Typst returned no page",
                Some(Compile),
            ),
            (
                MathRenderError::Svg("bad path \u{8DEF}\u{5F84} d".to_owned()),
                "SVG parsing failed: bad path \u{8DEF}\u{5F84} d",
                Some(Compile),
            ),
            (
                MathRenderError::InvalidDimensions,
                "raster dimensions are invalid or too large",
                Some(Compile),
            ),
            (
                MathRenderError::InlineGeometry,
                "inline math does not fit its terminal line box",
                None,
            ),
            (
                MathRenderError::MissingCjkGlyph,
                "no installed font provides every requested CJK glyph",
                None,
            ),
            (
                MathRenderError::MissingGlyph('\u{4E2D}'),
                "no installed font provides a glyph for '\u{4E2D}'",
                None,
            ),
            (
                MathRenderError::MissingGlyph('\u{2212}'),
                "no installed font provides a glyph for '\u{2212}'",
                None,
            ),
            (
                MathRenderError::HostDeclined,
                "the host declined to run this math render",
                None,
            ),
        ]
    }

    /// The arm a variant takes, with no wildcard: a variant added to `MathRenderError` without an
    /// arm here does not compile, and one added with an arm but no sample in `every_variant` fails
    /// the coverage assertion of `every_math_error_shows_its_message_byte_for_byte`.
    fn arm(error: &MathRenderError) -> usize {
        match error {
            MathRenderError::NotDetected => 0,
            MathRenderError::SourceTooLong => 1,
            MathRenderError::UnsafeCommand => 2,
            MathRenderError::NestingTooDeep => 3,
            MathRenderError::RawTypstCode => 4,
            MathRenderError::TooManyLayoutCells => 5,
            MathRenderError::MacroCycle => 6,
            MathRenderError::MacroExpansionLimit => 7,
            MathRenderError::UnboundedMacro => 8,
            MathRenderError::ConversionPanic => 9,
            MathRenderError::Aborted => 10,
            MathRenderError::Convert(_) => 11,
            MathRenderError::Compile(_) => 12,
            MathRenderError::NoPage => 13,
            MathRenderError::Svg(_) => 14,
            MathRenderError::InvalidDimensions => 15,
            MathRenderError::InlineGeometry => 16,
            MathRenderError::MissingCjkGlyph => 17,
            MathRenderError::MissingGlyph(_) => 18,
            MathRenderError::HostDeclined => 19,
        }
    }

    /// The number of arms `arm` has.
    const ARMS: usize = 20;

    /// A stage's arm, with no wildcard: a stage added to `MathFailureStage` without an arm here
    /// does not compile.
    fn stage_arm(stage: MathFailureStage) -> usize {
        match stage {
            MathFailureStage::Validate => 0,
            MathFailureStage::Convert => 1,
            MathFailureStage::Compile => 2,
        }
    }

    /// The number of arms `stage_arm` has.
    const STAGE_ARMS: usize = 3;

    #[test]
    fn every_math_error_shows_its_message_byte_for_byte() {
        let table = every_variant();
        for (error, message, stage) in &table {
            assert_eq!(error.to_string(), *message, "{error:?}");
            assert_eq!(error.failure_stage(), *stage, "{error:?}");
        }
        let mut arms: Vec<usize> = table.iter().map(|(error, _, _)| arm(error)).collect();
        arms.sort_unstable();
        arms.dedup();
        assert_eq!(
            arms,
            (0..ARMS).collect::<Vec<_>>(),
            "a variant has no sample"
        );
        let mut stages: Vec<usize> = table
            .iter()
            .filter_map(|(error, _, _)| error.failure_stage())
            .map(stage_arm)
            .collect();
        stages.sort_unstable();
        stages.dedup();
        assert_eq!(
            stages,
            (0..STAGE_ARMS).collect::<Vec<_>>(),
            "a failure stage no error belongs to"
        );
    }
}
