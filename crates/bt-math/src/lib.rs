//! Sandboxed MiTeX -> Typst -> SVG -> resvg math-block rendering.

use std::{collections::BTreeSet, num::NonZeroU32, sync::OnceLock, time::Duration};

pub use bt_doc::{InlineRunPlacement, MathMode};
use mitex_spec_gen::DEFAULT_SPEC;
use thiserror::Error;
use typst_as_lib::{TypstEngine, typst_kit_options::TypstKitFontOptions};
use typst_layout::PagedDocument;
use typst_library::{
    World,
    foundations::{Array, Dict, IntoValue, Str, Value},
    layout::{Frame, FrameItem, Point, Transform},
    text::{FontBook, FontInfo, FontStyle},
};

mod macro_budget;

/// **Test-only: make one stage of a render panic on purpose.**
///
/// The containment this exists to pin has no reachable input — no formula is known that panics
/// inside the Typst compile or the rasterizer, and a review that waits for one to be found is a
/// review that never tests the boundary. So the fault is injected at exactly the two stages that
/// used to run outside an unwind boundary, and the test asserts the same thing a real fault would:
/// this formula fails, the next one renders, and the process lives.
#[cfg(test)]
pub(crate) mod panic_injection {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum PanicStage {
        Compile,
        Rasterize,
    }

    thread_local! {
        static ARMED: Cell<Option<PanicStage>> = const { Cell::new(None) };
    }

    pub(crate) fn arm(stage: PanicStage) {
        ARMED.set(Some(stage));
    }

    /// Panic if this stage is the armed one, disarming first so the retry after it renders.
    pub(crate) fn trip(stage: PanicStage) {
        if ARMED.get() == Some(stage) {
            ARMED.set(None);
            panic!("injected {stage:?} fault");
        }
    }
}

thread_local! {
    static CONTAINED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run one formula's render inside an unwind boundary.
///
/// **A render is one unit of work over owned inputs, so a fault inside it is that formula's
/// failure and not the program's.** Only the MiTeX conversion used to stand inside a boundary; the
/// Typst compile, the SVG writer and the rasterizer did not, and the process panic hook escalates
/// anything it is not told is contained — so one formula would have taken the window down and every
/// shell in every pane with it.
///
/// `AssertUnwindSafe` is answerable here rather than convenient. The closure captures `&MathEngine`,
/// whose one field is a `TypstEngine`, and a compile builds a fresh world per call out of that
/// engine's immutable parts (`typst_as_lib::TypstEngine::do_compile`): the template, the font book,
/// the library and the file resolvers are read and never written, so an unwind cannot leave half of
/// one behind. The only mutable state a compile touches is `comemo`'s process-wide memo cache,
/// which is a cache of pure functions and gains an entry only after a call has returned. So there
/// is nothing to rebuild after an unwind, and the engine is used again — which the test asserts
/// rather than assumes.
fn contained<T>(render: impl FnOnce() -> Result<T, MathRenderError>) -> Result<T, MathRenderError> {
    struct Guard(bool);
    impl Drop for Guard {
        fn drop(&mut self) {
            CONTAINED.set(self.0);
        }
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _guard = Guard(CONTAINED.replace(true));
        render()
    }))
    .unwrap_or(Err(MathRenderError::Aborted))
}

/// The app's panic hook must log these panics, then return to the unwind boundary
/// instead of showing its fatal-error dialog or exiting the process.
///
/// True for the whole of one formula's render, not only for its MiTeX conversion: a render is one
/// unit of work over owned inputs and a world built fresh from immutable engine state, so a fault
/// anywhere inside it is that formula's failure. The hook keeps the diagnostic and lets the unwind
/// reach [`MathEngine::render`]'s boundary.
pub fn render_panic_is_contained() -> bool {
    CONTAINED.get()
}

/// Normalize named delimiters at the MiTeX/Typst boundary, leaving literals intact.
fn normalize_delimiter_symbols(source: &str) -> String {
    fn write(node: &typst_syntax::SyntaxNode, source: &str, output: &mut String) {
        if node.kind() == typst_syntax::SyntaxKind::MathFieldAccess {
            // MiTeX 0.2.4 emits Typst 0.10's angle.l/r; Typst 0.15 calls
            // them chevron.l/r. Even an isolated angle.l fails: no comma or
            // missing whitespace is involved. Normalize the delimiter family
            // to literal glyphs (escape ASCII grouping syntax). MiTeX ignores
            // a custom spec when emitting, so this belongs after conversion.
            output.push_str(match source {
                "angle.l" | "chevron.l" => "⟨",
                "angle.r" | "chevron.r" => "⟩",
                "paren.l" => r"\(",
                "paren.r" => r"\)",
                "bracket.l" => r"\[",
                "bracket.r" => r"\]",
                "brace.l" => r"\{",
                "brace.r" => r"\}",
                "floor.l" => "⌊",
                "floor.r" => "⌋",
                "ceil.l" => "⌈",
                "ceil.r" => "⌉",
                "bar.v" => "|",
                "bar.v.double" => "‖",
                _ => source,
            });
        } else if node.children().len() == 0 {
            output.push_str(source);
        } else {
            let mut offset = 0;
            for child in node.children() {
                let end = offset + child.len();
                write(child, &source[offset..end], output);
                offset = end;
            }
        }
    }

    // Use Typst's syntax tree: a string containing "angle.l" is text, while
    // a math field access is a symbol. Unknown complete accesses stay intact.
    let mut output = String::with_capacity(source.len());
    write(&typst_syntax::parse_math(source), source, &mut output);
    output
}

/// LaTeX to Typst, with the recursion this runs bounded from inside it.
///
/// **`catch_unwind` is not what makes this safe.** A stack overflow is not an unwind; the process
/// dies and takes every shell in every pane with it, over a line a program merely printed. Two
/// recursive-descent walks run in here — `mitex-parser` over the tokens, then MiTeX's converter
/// over the tree it built — and the depth each of them will reach cannot be *predicted* from the
/// source, because the parser's own arities are not the whole story: `\limits`, `'`, `\over` and
/// `\displaystyle` deepen the tree by wrapping what stands to their left, without a stack frame,
/// and a macro's expansion is what the parser actually sees. So the depth is *enforced*, in
/// `vendor/mitex-parser/src/depth.rs`, at the point where a level would be created, and
/// [`MathRenderError::NestingTooDeep`] is that enforcement answering. The formula stays as the text
/// the terminal printed, exactly like one MiTeX has no rule for.
fn convert_math(source: &str) -> Result<String, MathRenderError> {
    struct ConversionGuard(bool);
    impl Drop for ConversionGuard {
        fn drop(&mut self) {
            CONTAINED.set(self.0);
        }
    }
    let source = source.to_owned();
    // MiTeX is a pure conversion over owned input and a cloned immutable spec.
    // No MathEngine, locks, or caller state enter this closure; unwinding drops
    // all partial conversion state, so AssertUnwindSafe cannot hide a poisoned
    // shared invariant. The process hook retains the diagnostic in its log.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _guard = ConversionGuard(CONTAINED.replace(true));
        mitex::convert_math_bounded(&source, Some(DEFAULT_SPEC.clone()))
    }))
    .map_err(|_| MathRenderError::ConversionPanic)?
    .map_err(|error| match error {
        mitex::BoundedConvertError::NestingTooDeep => MathRenderError::NestingTooDeep,
        mitex::BoundedConvertError::RawTypstCode => MathRenderError::RawTypstCode,
        mitex::BoundedConvertError::TooManyCells => MathRenderError::TooManyLayoutCells,
        mitex::BoundedConvertError::Convert(message) => MathRenderError::Convert(message),
    })
    .map(|converted| normalize_delimiter_symbols(&converted))
}

pub const MAX_SOURCE_BYTES: usize = 8 * 1024;
/// The stack the math worker is given, and the number the nesting limit is chosen against.
///
/// **Rust's 2 MiB default is not a number anybody chose for this work.** One formula's render
/// descends through `mitex-parser`, then MiTeX's converter, then Typst's parser, its math layout
/// and its SVG writer, each recursing on the same nesting; and a stack overflow is not a panic, so
/// no `catch_unwind` can keep one from taking the process. The thread that does that work therefore
/// says how much stack it wants, here, where the limit that depends on it is also written.
///
/// Sixteen mebibytes against [`MAX_NESTING_DEPTH`] levels is 64 KiB of headroom per level, and the
/// five walks run one after another rather than inside one another, so each of them has the whole
/// of it. What the pair actually has to make good is narrower than any per-frame arithmetic and is
/// measured rather than argued: `every_recursive_shape_renders_at_the_limit_on_the_worker_stack`
/// builds, for each shape of nesting the grammar has, the deepest formula the limit accepts, and
/// draws it on a thread given exactly this stack.
pub const MATH_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;
pub const VERTICAL_PADDING_LOGICAL_PX: u32 = 8;
/// CPU raster budget. Wide display math is tiled to the GPU's per-axis texture limit later, so
/// rejecting it at an arbitrary 16K width would violate UI-UX §7.5's horizontal overflow rule.
pub const MAX_RASTER_BYTES: usize = 64 * 1024 * 1024;

const TYPST_TEMPLATE: &str = r#"
#import "specs/mod.typ": mitex-scope as base-mitex-scope
#set page(width: auto, height: auto, margin: (x: 0pt, y: sys.inputs.font_size * 1pt), fill: none)
#set text(size: sys.inputs.font_size * 1pt, fill: rgb(sys.inputs.red, sys.inputs.green, sys.inputs.blue))
// The math font, and behind it the installed families this particular source's
// remaining characters were found in (see `covering_families`). Typst's equation
// element show-sets the math font alone; naming it again here keeps it first —
// every letter, operator and symbol the math font carries still comes from it —
// and appends the faces that answer for the characters it does not carry. The
// list is empty for a source the math font answers for whole, which is then
// byte-for-byte the Typst default.
#show math.equation: set text(font: ("New Computer Modern Math",) + sys.inputs.fallback_fonts)
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

/// The family every mathematical character is set in, named once.
///
/// It is the face typst-assets embeds, so it is present on every machine Folio
/// runs on and it is the *first* family the book knows under this name — the
/// engine adds the embedded faces before it scans the system. Everything the
/// engine asks the machine for is measured against this: a character this family
/// carries is never a question about the reader's computer, and a character it
/// does not carry is exactly the question [`covering_families`] answers.
///
/// The template below names the same string; the test
/// `the_math_family_is_the_one_the_template_names` keeps the two from drifting.
#[cfg(test)]
const MATH_FAMILY: &str = "New Computer Modern Math";

/// The font book's own key for [`MATH_FAMILY`]. `FontBook` lowercases the names
/// it indexes by, so a lookup has to be lowercased too.
const MATH_FAMILY_KEY: &str = "new computer modern math";

/// CSS pixels one typographic point is worth. Both of this crate's two scale
/// factors are this number, which is why it is named once.
const PX_PER_PT: f32 = 96.0 / 72.0;

/// The factor `resvg` rasterizes the Typst page's own SVG by.
///
/// Typst reports the page in points and writes them into the SVG unitless, where
/// usvg reads them as CSS pixels — so this converts once, and
/// [`device_px_per_pt`] converts the second time.
fn svg_scale(dpi_milli: NonZeroU32) -> f32 {
    dpi_milli.get() as f32 / 1000.0 * PX_PER_PT
}

/// Device pixels one typographic point becomes at this DPI.
///
/// **The one definition of this scale**, because two callers need the same
/// answer: [`rasterize_svg`] places the baseline with it, and a caller sizing a
/// formula to match text it already has on screen inverts it
/// ([`key_for_em_px`]). A second copy of `96/72` squared living in a caller is
/// the shape of defect this file has already paid for once — the baseline was
/// undercounted by exactly one of these two factors for as long as only one of
/// them was written down.
#[must_use]
pub fn device_px_per_pt(dpi_milli: NonZeroU32) -> f32 {
    svg_scale(dpi_milli) * PX_PER_PT
}

/// A key that sets its mathematics at `em_device_px` **device pixels**.
///
/// Terminal inline runs and Markdown formulas know their surrounding text's physical em.
/// This inverts [`device_px_per_pt`] with a normalized DPI, so device scaling is applied once.
///
/// `None` when the requested size rounds to nothing, which is the only way the
/// point size can fail to be positive.
#[must_use]
pub fn key_for_em_px(
    em_device_px: f32,
    foreground_rgb: [u8; 3],
    mode: MathMode,
) -> Option<MathRenderKey> {
    let dpi_milli = NonZeroU32::new(1000).expect("1000 is non-zero");
    let milli_pt = (em_device_px / device_px_per_pt(dpi_milli) * 1000.0).round();
    Some(MathRenderKey {
        dpi_milli,
        font_milli_pt: NonZeroU32::new(milli_pt.max(0.0) as u32)?,
        foreground_rgb,
        mode,
    })
}

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
    /// seven million. The cost is linear in cells and measured, so [`mitex::MAX_LAYOUT_CELLS`]
    /// bounds the time and the memory of the whole formula rather than of one environment.
    #[error(
        "math source asks for more than {} laid-out cells",
        mitex::MAX_LAYOUT_CELLS
    )]
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
            | Self::MissingGlyph(_) => None,
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

    /// **The engine has no door onto this machine, and that is a property of how it is built.**
    ///
    /// The only file resolver it is given is `with_static_source_file_resolver`, over exactly the
    /// three MiTeX specification files compiled into the executable. There is no filesystem
    /// resolver and no package resolver, so a Typst `read`, `image`, `include` or `import` of
    /// anything else has nowhere to resolve and fails the compile — which is why the refusals in
    /// [`convert_math`] are about *computation* rather than about reading files. A formula still
    /// may not carry code, because a loop nobody can interrupt is its own kind of harm.
    fn with_system_fonts(include_system_fonts: bool) -> Self {
        let engine = TypstEngine::builder()
            .main_file(TYPST_TEMPLATE)
            .with_static_source_file_resolver([
                (
                    "specs/mod.typ",
                    include_str!("../../../assets/mitex-specs/mod.typ"),
                ),
                (
                    "specs/prelude.typ",
                    include_str!("../../../assets/mitex-specs/prelude.typ"),
                ),
                (
                    "specs/latex/standard.typ",
                    include_str!("../../../assets/mitex-specs/latex/standard.typ"),
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
        contained(|| self.render_inner(source, key))
    }

    fn render_inner(
        &self,
        source: &str,
        key: MathRenderKey,
    ) -> Result<MathRaster, MathRenderError> {
        let started = std::time::Instant::now();
        let document = self.typeset(source, key)?;
        let page = document.pages().first().ok_or(MathRenderError::NoPage)?;
        if let Some(character) = frame_undrawn_character(&page.frame) {
            return Err(if is_cjk_character(character) {
                MathRenderError::MissingCjkGlyph
            } else {
                MathRenderError::MissingGlyph(character)
            });
        }
        let svg = typst_svg::svg(page, &Default::default());
        // The same number the template turned into `margin.y`, which is what makes the page's
        // content box — and therefore its baseline — recoverable from the page frame alone.
        let margin_pt = f64::from(key.font_milli_pt.get()) / 1000.0;
        let metrics = find_math_metrics(&page.frame)
            .or_else(|| fallback_math_metrics(&page.frame, margin_pt))
            .ok_or(MathRenderError::InvalidDimensions)?;
        #[cfg(test)]
        panic_injection::trip(panic_injection::PanicStage::Rasterize);
        rasterize_svg(&svg, key, metrics, started.elapsed())
    }

    /// Typeset the source and stop at the laid-out page, before anything is
    /// drawn.
    ///
    /// [`Self::render`] and [`Self::typeset_runs`] are the same compilation
    /// asked two different questions — what the formula *looks* like, and what
    /// the typesetter *did* — so they share one body rather than two that can
    /// drift apart. A reference measured on one machine is only worth anything
    /// while it is measuring the same document the reader gets.
    fn typeset(&self, source: &str, key: MathRenderKey) -> Result<PagedDocument, MathRenderError> {
        validate_source(source)?;
        let converted = convert_math(source)?;
        bound_converted_nesting(&converted)?;
        let mut inputs = Dict::new();
        inputs.insert(
            "fallback_fonts".into(),
            Value::Array(self.families_for(&converted)),
        );
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
        #[cfg(test)]
        panic_injection::trip(panic_injection::PanicStage::Compile);
        self.engine
            .compile_with_input::<_, PagedDocument>(inputs)
            .output
            .map_err(|error| MathRenderError::Compile(error.to_string()))
    }

    /// What the typesetter actually drew for this formula: every shaped run on
    /// the page, in page order, with the face it was drawn from.
    ///
    /// **A raster cannot say which of two machines is wrong.** Two pictures that
    /// differ tell you they differ; this says *why* — the characters that
    /// reached the page, the family, style and weight that answered for each of
    /// them, and the glyph ids and advances that came back. It is the reference
    /// DESIGN §13.40 ⑥ compares two machines on, made specific enough to name a
    /// culprit rather than only to disagree, and it is the only way a character
    /// the typesetter dropped on the floor can be seen at all: a character no
    /// family could shape is not drawn *and not spaced*, so it leaves no mark on
    /// the picture to find (`typst-layout`'s `math::text::layout_glyph` pushes
    /// nothing when `GlyphFragment::new` comes back `None`).
    pub fn typeset_runs(
        &self,
        source: &str,
        key: MathRenderKey,
    ) -> Result<Vec<TypesetRun>, MathRenderError> {
        let document = self.typeset(source, key)?;
        let page = document.pages().first().ok_or(MathRenderError::NoPage)?;
        let mut runs = Vec::new();
        collect_typeset_runs(&page.frame, &mut runs);
        Ok(runs)
    }

    /// Every installed family that claims a character of this source the math
    /// font itself cannot draw, in the order Typst should read them. Empty when
    /// [`MATH_FAMILY`] answers for the whole source — which is the ordinary
    /// case, and is then byte-for-byte what Typst does unaided.
    ///
    /// **This is what makes [`MathRenderError::MissingCjkGlyph`] mean what it
    /// says.** The error itself is still read off the finished page — a `.notdef`
    /// where an ideograph belongs is the one honest test, because it asks the
    /// outlines rather than the `cmap`'s claim about them. What was wrong before
    /// is that the page only ever got *one* face's answer. Left to itself Typst
    /// gets exactly one attempt per ideograph: `select_fallback` returns the
    /// single best-scoring installed font covering that character — scored by
    /// how much the family's name resembles the math font's, with "shorter
    /// family name" as the last tiebreak — and when the character comes back
    /// `.notdef` the recursion asks the same question, gets the same font, finds
    /// it already used, and stops. So one face had to answer for a formula's
    /// ideographs alone, and `.notdef` meant "the face this scoring function
    /// happened to pick lacks this glyph", not "this machine lacks it".
    ///
    /// A GitHub Windows runner told the two apart on 2026-08-31: `\text{中}`,
    /// `\text{文}` and `\text{项目数}` drew, and `\text{死} \; + \; \text{活}` in
    /// the very same suite came back `MissingCjkGlyph` — 活's best-scoring face
    /// was `Gulim`, whose `cmap` claims 活 and whose outlines do not contain it,
    /// while `Malgun Gothic` sat installed on the same machine with the glyph.
    ///
    /// Handed the whole list, Typst walks it per character, and a `.notdef` that
    /// survives every claimant is a machine that genuinely cannot draw the
    /// character. Note what this does *not* do: it does not declare a character
    /// missing because no `cmap` claims it. The page is still the judge, so a
    /// character the source names and the document never typesets cannot fail a
    /// render that does not draw it.
    ///
    /// **The question this asks is "can the math font draw it", not "is it
    /// Chinese".** It was written for ideographs and read the request through a
    /// list of CJK code blocks, which made every other character the math font
    /// happens to lack somebody else's problem — and that somebody is
    /// `select_fallback`, the one-attempt, name-similarity scoring the paragraph
    /// above is the whole record of. Worse than a wrong face: when the scoring
    /// finds nothing at all, `typst-layout` does not draw a `.notdef` for the
    /// character, it drops the character — `math::text::layout_glyph` pushes a
    /// fragment only `if let Some(glyph) = GlyphFragment::new(…)`, and a
    /// character that never becomes a fragment takes no room and leaves no ink,
    /// so the formula silently loses a symbol and every check that looks at the
    /// picture says it is fine. Asking the book directly, for every character,
    /// takes that path out of the engine's way: a character some installed face
    /// can draw is named to a family that can draw it, and one no face can draw
    /// is what [`frame_undrawn_character`] now reports for any character rather
    /// than for ideographs alone.
    fn families_for(&self, source: &str) -> Array {
        self.engine
            // The only way building a world fails is injecting inputs into the
            // library, and this one is asked for the font book alone.
            .with_world(|world| {
                let book = world.book();
                let requested = characters_the_math_font_lacks(book, source);
                if requested.is_empty() {
                    Vec::new()
                } else {
                    covering_families(book, &requested)
                }
            })
            .expect("a world built with no inputs")
            .into_iter()
            .map(|family| Value::Str(Str::from(family)))
            .collect()
    }
}

/// The characters a Typst source asks to see drawn that [`MATH_FAMILY`] itself
/// does not carry.
///
/// Whitespace and control characters are not drawn by anybody, and a character
/// the math family claims needs no second opinion — naming another family for it
/// could only take it away from the face the mathematics is set in. What is left
/// is exactly the request the machine has to answer: the ideographs of a
/// `\text{…}`, an author's emoji, a symbol from a corner of Unicode this face
/// never covered.
///
/// A book with no [`MATH_FAMILY`] in it at all cannot be argued with — every
/// character is then unanswered — and that is the honest reading: on such a
/// machine the mathematics is being set in whatever the fallback finds, and
/// naming the claimants is the most that can be done for it.
fn characters_the_math_font_lacks(book: &FontBook, source: &str) -> BTreeSet<char> {
    let math_faces = book
        .select_family(MATH_FAMILY_KEY)
        .filter_map(|face| book.info(face))
        .collect::<Vec<&FontInfo>>();
    source
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .filter(|c| {
            !math_faces
                .iter()
                .any(|info| info.coverage.contains(*c as u32))
        })
        .collect()
}

/// Every installed family that claims a character of `requested`, ordered so
/// that reading them in turn draws as much of it as this machine can. Empty when
/// nothing installed claims any of it, which is a machine that will draw tofu
/// and be told so by the page.
///
/// The head of the list is a greedy set cover: take the family that claims the
/// most of what is still unanswered, strike those characters off, repeat. One
/// family usually answers for the whole request in one step; a machine whose CJK
/// support is split across a partial face and a fuller one gets both, in the
/// order that leaves the fewest characters to the second.
///
/// **The tail is the rest of the claimants, and it is not redundant.** A `cmap`
/// is a claim, not a promise, and the machine that proved it is the very runner
/// this defect came from: all four faces of its `gulim.ttc` — `Gulim`,
/// `GulimChe`, `Dotum`, `DotumChe` — list U+6D3B 活 in their coverage and have
/// no glyph for it (measured 2026-08-31: `coverage.contains` yes,
/// `glyph_index` none, while 死 中 文 目 shape from the same faces). Typst
/// walks a font list per tofu run — a character the current family cannot draw
/// is re-shaped with the *next* family — so handing it every claimant is what
/// turns a broken claim into one wasted step instead of a missing glyph. Its own
/// fallback cannot do that on its own: `select_fallback` returns the single
/// best-scoring font covering the character, so on the recursion it returns the
/// same font again, finds it already used, and gives up. A minimal cover has no
/// slack for that either, and the tail costs nothing on a machine whose first
/// family is honest: families past the first are consulted only for characters
/// that came back `.notdef`.
///
/// Ties go to the family the font book names first, which is its own order —
/// alphabetical, by lowercased family name. Folio deliberately expresses no
/// taste here. Which Han face a formula is set in is a real design question and
/// this is not the place it gets answered; what this owes the reader is that the
/// characters appear at all, drawn by whichever installed face can draw them,
/// with no font embedded or redistributed to make that true.
///
/// **The measurements above are about ideographs and the reasoning is not.**
/// Nothing in this function looks at a code block: it is handed the characters
/// [`MATH_FAMILY`] cannot draw, whatever they turn out to be, and every word of
/// the argument — a `cmap` is a claim rather than a promise, Typst gets one
/// attempt per character from `select_fallback`, a font list is walked per tofu
/// run — is true of a rare operator or an emoji exactly as it was of 活.
/// Restricting it to CJK was the shape of the defect that was under
/// investigation, not a property of the remedy.
fn covering_families(book: &FontBook, requested: &BTreeSet<char>) -> Vec<String> {
    // Every family the book knows, in the book's own order, each with the
    // characters of the request it claims. A family's faces are pooled: what
    // reaches Typst is a family *name*, and it picks the variant itself, so a
    // family claims a character when any of its faces does.
    let candidates = book
        .families()
        .map(|(family, faces)| {
            let drawn = faces
                .filter_map(|face| book.info(face))
                .flat_map(|info| {
                    requested
                        .iter()
                        .copied()
                        .filter(|c| info.coverage.contains(*c as u32))
                })
                .collect::<BTreeSet<char>>();
            (family.to_owned(), drawn)
        })
        .filter(|(_, drawn)| !drawn.is_empty())
        .collect::<Vec<_>>();

    let mut remaining = requested.clone();
    let mut families: Vec<String> = Vec::new();
    // The cover runs while it can still answer something new; a request no
    // installed family claims the rest of simply leaves the loop early, and the
    // page reports the tofu.
    while let Some((family, drawn)) = candidates
        .iter()
        .filter(|(family, _)| !families.contains(family))
        // `min_by_key` over the negated count and not `max_by_key`, because the
        // tiebreak is the point: `max_by_key` keeps the *last* of equal keys and
        // this has to keep the first, which is the book's own order.
        .min_by_key(|(_, drawn)| std::cmp::Reverse(drawn.intersection(&remaining).count()))
        .filter(|(_, drawn)| !drawn.is_disjoint(&remaining))
    {
        remaining.retain(|c| !drawn.contains(c));
        families.push(family.clone());
    }
    let tail = candidates
        .iter()
        .map(|(family, _)| family)
        .filter(|family| !families.contains(family))
        .cloned()
        .collect::<Vec<_>>();
    families.extend(tail);
    families
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
    bound_nesting(source, |byte| match byte {
        b'{' => Some(true),
        b'}' => Some(false),
        _ => None,
    })?;
    macro_budget::validate(source)
}

/// How deep a formula may nest before it is refused.
///
/// **Written once, where it is enforced, and read here.** The limit belongs to the recursive
/// implementations that obey it — `vendor/mitex-parser/src/depth.rs` refuses to build a syntax tree
/// deeper than this, and MiTeX's converter refuses to descend past it — so this crate names that
/// constant rather than keeping a second copy of the number that could drift from it.
/// `the_limit_and_the_worker_stack_are_one_pair` pins the pair.
pub const MAX_NESTING_DEPTH: usize = mitex::MAX_TREE_DEPTH;

/// Walk a source once and refuse it past [`MAX_NESTING_DEPTH`] levels of whatever `opens` calls a
/// level. `Some(true)` opens one, `Some(false)` closes one, `None` is neither.
fn bound_nesting(source: &str, opens: impl Fn(u8) -> Option<bool>) -> Result<(), MathRenderError> {
    let mut depth = 0_usize;
    for byte in source.bytes() {
        match opens(byte) {
            Some(true) => {
                depth = depth.saturating_add(1);
                if depth > MAX_NESTING_DEPTH {
                    return Err(MathRenderError::NestingTooDeep);
                }
            }
            Some(false) => depth = depth.saturating_sub(1),
            None => {}
        }
    }
    Ok(())
}

/// The same ceiling, asked of the Typst the conversion produced.
///
/// **This one is a belt, and the buckle is upstream.** What recurses on this text is Typst: its
/// parser, then its math resolution and math layout, then the SVG writer. Of those, only the parser
/// refuses on its own — `typst_syntax`'s `MAX_DEPTH` is 256 and a source past it comes back as an
/// error node reading "maximum parsing depth exceeded", which arrives here as a compile error and
/// leaves the formula as text (`typst-syntax-0.15.1/src/parser.rs:13` and `:2127`). Typst's math IR
/// resolution and its math layout carry no depth counter at all
/// (`typst-library-0.15.0/src/math/ir/resolve.rs`, `typst-layout-0.15.0/src/math/mod.rs`), and
/// neither does `typst-svg`'s frame walk (`typst-svg-0.15.0/src/lib.rs:313` and `:362`); they are
/// safe because everything that reaches them came through that parser. Which it does: what this
/// engine hands Typst is text, and the one shape that escapes the parser's cap — content
/// accumulated in a Typst loop — is one that only *code* can write, and a formula cannot carry code
/// (see [`MathRenderError::RawTypstCode`], and
/// `every_formula_that_converts_calls_only_names_mitex_itself_wrote` for the invariant).
/// Downstream of all of it, `usvg` refuses an SVG nested past 1024
/// (`usvg-0.47.0/src/parser/svgtree/parse.rs:182`).
///
/// So this is not the guard that saves the process, and neither is the brace count in
/// [`validate_source`]: the recursion that has no owner but Folio is the conversion's own, it runs
/// before this text exists, and it is refused *inside* itself (see [`convert_math`]). What this adds
/// is a cheap refusal in Folio's own words for a conversion that managed to deepen what it was
/// given — brackets being where MiTeX spells the nesting that a brace-free `\sqrt\sqrt\sqrt…` never
/// wrote.
fn bound_converted_nesting(converted: &str) -> Result<(), MathRenderError> {
    bound_nesting(converted, |byte| match byte {
        b'(' | b'[' | b'{' => Some(true),
        b')' | b']' | b'}' => Some(false),
        _ => None,
    })
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

/// The backstop behind [`MathEngine::families_for`]: the first character this
/// page drew as `.notdef`, if it drew one.
///
/// This is no longer how the *question* is decided — see that method for why a
/// shaping outcome is not a fact about the machine — and after the covering
/// families reach the template it can only fire for a font whose `cmap` claims a
/// character it cannot actually shape. That is still a machine that cannot draw
/// the formula, so it is still a refusal; it is just no longer the thing that
/// decides whether the machine has the font.
///
/// **It reads every character, not the ideographs.** Restricting it to CJK was
/// never a rule about mathematics, it was the shape of the defect that happened
/// to be under investigation, and the cost of that restriction is a class of
/// silently *wrong* formulas: a page missing an operator is not a page with a
/// blemish, it says something the author did not write, and until this looked at
/// the rest of Unicode nothing in the pipeline could tell that from a page that
/// is right. The character comes back with the answer so the refusal can name
/// it.
fn frame_undrawn_character(frame: &Frame) -> Option<char> {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Text(text) => {
                for glyph in text.glyphs.iter().filter(|glyph| glyph.id == 0) {
                    // The glyph's range indexes its own item's text. A range a
                    // shaper left inconsistent is not worth a panic on a reading
                    // surface, so an unreadable one is reported as the item's
                    // first character instead.
                    if let Some(character) = text
                        .text
                        .get(glyph.range())
                        .and_then(|covered| covered.chars().next())
                        .or_else(|| text.text.chars().next())
                    {
                        return Some(character);
                    }
                }
            }
            FrameItem::Group(group) => {
                if let Some(character) = frame_undrawn_character(&group.frame) {
                    return Some(character);
                }
            }
            _ => {}
        }
    }
    None
}

/// One shaped run as it stands on a finished page.
///
/// The unit is the typesetter's, not the reader's: mathematics is laid out one
/// fragment at a time, so a formula arrives as many short runs rather than one
/// long one, and that is the point — each run carries the face that answered for
/// *it*.
#[derive(Clone, Debug, PartialEq)]
pub struct TypesetRun {
    /// The characters of the run, as the typesetter shaped them. This is what a
    /// code-point assertion reads: Typst's math shorthands have already been
    /// applied here, so a source `-` arrives as `−` (U+2212 MINUS SIGN).
    pub text: String,
    /// The family that answered for the run.
    pub family: String,
    /// Whether the face is an italic or oblique one.
    pub italic: bool,
    /// The face's weight, 100–900.
    pub weight: u16,
    /// The size the run is set at, in typographic points.
    pub size_pt: f64,
    /// The glyph ids the shaper returned. A zero is `.notdef`.
    pub glyph_ids: Vec<u16>,
    /// Each glyph's advance, in thousandths of an em. A run that is drawn but
    /// takes no room is as invisible as one that was dropped, and this is where
    /// that shows.
    pub advances_milli_em: Vec<i32>,
}

impl TypesetRun {
    /// Whether any glyph of the run came back `.notdef`.
    #[must_use]
    pub fn has_notdef(&self) -> bool {
        self.glyph_ids.contains(&0)
    }
}

/// Every text run of a page, in page order, with the groups walked in place.
fn collect_typeset_runs(frame: &Frame, runs: &mut Vec<TypesetRun>) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Text(text) => runs.push(TypesetRun {
                text: text.text.to_string(),
                family: text.font.font().info().family.clone(),
                italic: matches!(
                    text.font.font().info().variant.style,
                    FontStyle::Italic | FontStyle::Oblique
                ),
                weight: text.font.font().info().variant.weight.to_number(),
                size_pt: text.size.to_pt(),
                glyph_ids: text.glyphs.iter().map(|glyph| glyph.id).collect(),
                advances_milli_em: text
                    .glyphs
                    .iter()
                    .map(|glyph| (glyph.x_advance.get() * 1000.0).round() as i32)
                    .collect(),
            }),
            FrameItem::Group(group) => collect_typeset_runs(&group.frame, runs),
            _ => {}
        }
    }
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
    let options = OPTIONS.get_or_init(svg_options_without_external_images);
    let tree = resvg::usvg::Tree::from_str(svg, options)
        .map_err(|error| MathRenderError::Svg(error.to_string()))?;
    let scale = svg_scale(key.dpi_milli);
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
    let device_px_per_pt = device_px_per_pt(key.dpi_milli);
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

/// The base parse options every SVG this crate reads is parsed under.
///
/// **An `<image href>` is not a door onto this machine.** usvg's stock string
/// resolver treats every href that is not a `data:` URI as a path and reads it,
/// which makes `<image href="\\attacker\share\p.png"/>` a connection to whoever
/// owns that share and `<image href="C:\secrets\x.svg"/>` a way to draw a file
/// nobody asked to see. Neither needs a click: a `.svg` path printed into a pane
/// is queued for decoding on sight, and decoding arrives here.
///
/// So `resolve_string` returns nothing, for every href, always. What survives is
/// `resolve_data`, which decodes `data:` URIs the document carries itself — and
/// the sub-SVGs it can hold are parsed under these same options, so the rule
/// holds however deep the nesting goes. Nothing legitimate is lost: the
/// typesetter's own documents come out of Typst as paths, and an author's SVG
/// that wants a bitmap in it can embed one.
fn svg_options_without_external_images() -> resvg::usvg::Options<'static> {
    let mut options = resvg::usvg::Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_href, _options| None);
    options
}

/// The parse options every standalone SVG document is read under, built once.
fn svg_document_options() -> &'static resvg::usvg::Options<'static> {
    static OPTIONS: OnceLock<resvg::usvg::Options<'static>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let mut options = svg_options_without_external_images();
        // **The machine's own fonts, or an SVG with words in it draws none of
        // them** (2026-08-28, `docs/DESIGN.md` §7.1.3k).
        //
        // `Options::default()` starts with an **empty** font database, and usvg
        // drops every `<text>` it cannot shape, silently. That was invisible
        // while the only documents reaching this function came out of Typst — a
        // typeset formula is paths, not text — and it stopped being invisible the
        // day a markdown page started drawing an author's own SVG: this
        // repository's `README.md` hero is sixteen `<text>` elements, and it came
        // back as its background and nothing else.
        //
        // Loaded once, behind this `OnceLock`, on whichever worker asks first:
        // enumerating the installed faces costs of the order of a hundred
        // milliseconds, and it is paid on the lane that exists to keep tens of
        // milliseconds of typesetting off the window's thread.
        options.fontdb_mut().load_system_fonts();
        options
    })
}

/// Rasterize a standalone SVG document at its intrinsic size (one user unit per pixel). Serves
/// the inline-image pipeline's SVG admission (M2 preview matrix §2: SVG displays as a static
/// raster); this crate owns the resvg dependency, so image decoding borrows the rasterizer
/// instead of growing its own.
pub fn rasterize_svg_document(bytes: &[u8]) -> Result<SvgRaster, SvgRasterError> {
    let options = svg_document_options();
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

    #[test]
    fn named_delimiters_render() {
        let engine = MathEngine::with_system_fonts(false);
        let key = MathRenderKey {
            dpi_milli: NonZeroU32::new(2000).unwrap(),
            font_milli_pt: NonZeroU32::new(24_000).unwrap(),
            ..key()
        };
        let mut failures = Vec::new();
        for source in [
            r"\left| \langle u, v \rangle \right|^2 \le \langle u, u \rangle \cdot \langle v, v \rangle",
            r"\langle u, v \rangle",
            r"\langle\psi|\phi\rangle",
            r"\lVert x \rVert_2",
            r"\lfloor x \rfloor",
            r"\left\langle a, b \right\rangle",
            r"\lvert x \rvert",
            r"\lceil x \rceil",
            r"\lparen x \rparen",
            r"\left| x \right|^2",
            r"|x|^2 \le 1",
            r"\langle",
            r"\rangle",
            r"\lang u, v \rang",
            r"\lbrack x \rbrack",
            r"\lbrace x \rbrace",
            r"\left\lvert x \right\rvert",
            r"\left\lVert x \right\rVert_2",
            r"\left\lfloor x \right\rfloor",
            r"\left\lceil x \right\rceil",
            r"\left\lparen x \right\rparen",
            r"\left\lbrack x \right\rbrack",
            r"\left\lbrace x \right\rbrace",
            r"\newcommand{\inner}[2]{\langle #1, #2 \rangle}\inner{u}{v}",
        ] {
            let emitted = mitex::convert_math(source, Some(DEFAULT_SPEC.clone())).unwrap();
            let corrected = convert_math(source).unwrap();
            eprintln!("LaTeX: {source}\nBefore: {emitted:?}\nAfter: {corrected:?}");
            match engine.render(source, key) {
                Ok(raster) => assert!(raster.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0)),
                Err(error) => failures.push(format!("{source}: {error}")),
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn delimiter_aliases_preserve_literals_and_geometric_angles() {
        assert_eq!(
            normalize_delimiter_symbols(
                r#"angle.l "angle.l \"angle.r\"" /* angle.r */ angle.right angle.l.unknown"#
            ),
            r#"⟨ "angle.l \"angle.r\"" /* angle.r */ angle.right angle.l.unknown"#
        );
        for (source, expected) in [
            (r"\langle u, v \rangle", r"⟨  u \, v  ⟩ "),
            (r"\langle\psi|\phi\rangle", "⟨ psi | phi.alt ⟩ "),
            (r"\left\langle a, b \right\rangle", r"lr(⟨  a \, b  ⟩ )"),
        ] {
            assert_eq!(convert_math(source).unwrap(), expected);
        }
        for source in [
            r"\text{angle.l angle.r chevron.l}",
            r"\angle + \measuredangle + \sphericalangle",
        ] {
            assert_eq!(
                convert_math(source).unwrap(),
                mitex::convert_math(source, Some(DEFAULT_SPEC.clone())).unwrap()
            );
        }
        let engine = MathEngine::with_system_fonts(false);
        for (source, equivalent) in [
            (r"\langle u, v \rangle", "⟨u, v⟩"),
            (r"\langle\psi|\phi\rangle", r"⟨\psi|\phi⟩"),
            (r"\lang u, v \rang", "⟨u, v⟩"),
            (
                r"\left\langle\frac{a}{b}\right\rangle",
                r"\left⟨\frac{a}{b}\right⟩",
            ),
            (r"\angle", "∠"),
        ] {
            let named = engine.render(source, key()).unwrap();
            let literal = engine.render(equivalent, key()).unwrap();
            assert_eq!(
                (named.width_px, named.height_px),
                (literal.width_px, literal.height_px),
                "{source}"
            );
            assert_eq!(named.rgba, literal.rgba, "{source}");
        }
    }

    #[test]
    fn a_conversion_panic_is_a_neutral_refusal_and_clears_its_guard() {
        assert_eq!(
            convert_math(r"\newcommand{\a}{#}"),
            Err(MathRenderError::ConversionPanic)
        );
        assert!(!render_panic_is_contained());
        assert_eq!(
            MathRenderError::ConversionPanic.failure_stage(),
            Some(MathFailureStage::Convert)
        );
        assert!(
            !MathRenderError::ConversionPanic
                .to_string()
                .contains("unwrap")
        );
        assert!(convert_math("x+1").is_ok());
    }

    /// A fault outside the MiTeX conversion is that formula's failure, not the program's.
    ///
    /// Only `mitex::convert_math` stood inside an unwind boundary; the Typst compile, the SVG and
    /// the rasterizer did not, and the process panic hook escalates any panic it does not recognise
    /// as contained into the fatal path — so one formula would have taken the window down and every
    /// shell in every pane with it.
    #[test]
    fn a_fault_anywhere_in_a_render_fails_that_formula_and_leaves_the_engine_usable() {
        use panic_injection::PanicStage;
        let engine = MathEngine::with_system_fonts(false);
        for stage in [PanicStage::Compile, PanicStage::Rasterize] {
            panic_injection::arm(stage);
            assert_eq!(
                engine.render("x^2", key()),
                Err(MathRenderError::Aborted),
                "{stage:?} must come back as this formula's refusal"
            );
            assert!(
                !render_panic_is_contained(),
                "{stage:?} must release the guard it took"
            );
            assert_eq!(
                MathRenderError::Aborted.failure_stage(),
                Some(MathFailureStage::Compile)
            );
        }
        assert!(
            engine.render("x^2", key()).is_ok(),
            "the engine is reusable: a render builds its world from immutable state"
        );
    }

    /// Nesting a formula can reach without a single brace.
    ///
    /// `validate_source` counts `{` only, so `\sqrt\sqrt\sqrt…` — six bytes a level, thousands of
    /// levels inside the 8 KiB budget — passes it untouched, and no count taken before the parser
    /// ran could be trusted to see it either. What refuses it is the parser declining to build the
    /// level, at the level.
    #[test]
    fn brace_free_recursion_is_bounded_by_the_nesting_it_converts_to() {
        let engine = MathEngine::with_system_fonts(false);
        let deep = r"\sqrt".repeat(300) + " x";
        assert!(
            deep.len() < MAX_SOURCE_BYTES,
            "the budget must not be what refuses it"
        );
        assert_eq!(
            engine.render(&deep, key()),
            Err(MathRenderError::NestingTooDeep)
        );
        let shallow = r"\sqrt".repeat(8) + " x";
        assert!(
            engine.render(&shallow, key()).is_ok(),
            "an ordinary nested formula is untouched"
        );
    }

    /// **Depth is enforced where it is created, not predicted before it is.**
    ///
    /// A stack overflow is not a panic: `catch_unwind` cannot contain one and the process dies,
    /// from text a program merely printed. The guard therefore cannot live after the conversion —
    /// and the version of it that lived *before* the conversion was disproved, because a count
    /// taken over the token stream cannot know what the parser will do with it. Every case here is
    /// refused by [`convert_math`], which is the parser and the converter each declining to build
    /// or descend a level at the moment that level would exist.
    #[test]
    fn deep_nesting_is_refused_by_the_walk_that_would_have_descended() {
        for (name, source) in [
            // `^` is one byte and `attach_component` recurses into `content` for its argument, so
            // the scripts chain right-associatively: eight thousand carets are eight thousand
            // levels, inside the 8 KiB the source budget allows.
            ("chained scripts", "^".repeat(8_191) + "x"),
            // A macro's body is expanded where it is called, and the macro work cap is 32 KiB, so
            // the source's own length bounds nothing: a hundred `\sqrt` in a body, invoked
            // forty-eight times, is four thousand eight hundred levels. MiTeX's macro engine sits
            // under the parser's lexer, so those levels are exactly what the guard sees.
            (
                "a macro body invoked many times",
                format!(
                    "\\newcommand{{\\deep}}{{{}}}{}x",
                    "\\sqrt".repeat(100),
                    "\\deep".repeat(48)
                ),
            ),
            ("brace-free commands", "\\sqrt".repeat(300) + " x"),
            ("groups", "{".repeat(300) + "x" + &"}".repeat(300)),
            (
                "arguments nested in arguments",
                format!("{}x{}", "\\frac{".repeat(300), "}{y}".repeat(300)),
            ),
        ] {
            assert!(
                source.len() <= MAX_SOURCE_BYTES,
                "{name}: the byte budget must not be what refuses it ({} bytes)",
                source.len()
            );
            assert_eq!(
                convert_math(&source),
                Err(MathRenderError::NestingTooDeep),
                "{name}"
            );
        }
    }

    /// The five shapes the predictive scan undercounted, from the 2026-09-17 review's F4.
    ///
    /// **Each of them is a level the grammar creates and no arity predicts.** `\over` and
    /// `\displaystyle` take what is to their *left* or the rest of the scope, and wrap or consume it;
    /// `\limits` wraps the item before it, and wraps its own wrapper next time, so a chain of them
    /// deepens the tree without the parser recursing at all; `\sqrt`'s glob `{,b}t` is not finished
    /// by closing its optional bracket; and a macro that expands to `\frac` arrives at the parser as
    /// `\frac`, with `\frac`'s arity, which the name `\f` does not have. The scan read the first
    /// four as depth 0, 1, 0 and 2 and accepted the fifth; all five converted three hundred levels
    /// deep.
    #[test]
    fn the_shapes_a_predicted_bound_undercounted_are_refused() {
        for (name, source) in [
            ("an infix operator's chain", r"x\over ".repeat(300) + "x"),
            ("a greedy command's chain", r"\displaystyle x ".repeat(300)),
            (
                "a left-wrapping command's chain",
                "x".to_owned() + &r"\limits".repeat(300),
            ),
            (
                "an optional argument before a required one",
                r"\sqrt[2]".repeat(300) + "x",
            ),
            (
                "a macro that expands to a command with arguments",
                format!(r"\newcommand{{\f}}{{\frac}}{}x", r"\f a ".repeat(300)),
            ),
        ] {
            assert!(
                source.len() <= MAX_SOURCE_BYTES,
                "{name}: the byte budget must not be what refuses it ({} bytes)",
                source.len()
            );
            assert_eq!(
                convert_math(&source),
                Err(MathRenderError::NestingTooDeep),
                "{name}"
            );
        }
    }

    /// Every shape of nesting the grammar has, as a source of a given depth.
    ///
    /// The first six make the parser *recurse*; the four after them make it *wrap* — a level of
    /// tree with no stack frame behind it, which is why a bound on recursion alone would not be a
    /// bound at all. The last four are the same shapes reached through a macro, whose expansion the
    /// parser's own lexer performs, and one that mixes them.
    type Shape = (&'static str, fn(usize) -> String);

    fn shapes() -> Vec<Shape> {
        vec![
            ("nested groups", |n| {
                format!("{}x{}", "{".repeat(n), "}".repeat(n))
            }),
            ("bare script chains", |n| "^".repeat(n) + "x"),
            ("brace-free command towers", |n| r"\sqrt".repeat(n) + " x"),
            ("argument towers", |n| {
                format!("{}x{}", r"\frac{".repeat(n), "}{y}".repeat(n))
            }),
            ("optional-argument globs", |n| r"\sqrt[2]".repeat(n) + "x"),
            ("greedy chains", |n| r"\displaystyle x ".repeat(n)),
            ("infix chains", |n| r"x\over ".repeat(n) + "x"),
            ("script chains after a term", |n| {
                "x".to_owned() + &"^y".repeat(n)
            }),
            ("prime chains", |n| "x".to_owned() + &"'".repeat(n)),
            ("left-wrapping chains", |n| {
                "x".to_owned() + &r"\limits".repeat(n)
            }),
            ("paired delimiters", |n| {
                format!("{}x{}", r"\left(".repeat(n), r"\right)".repeat(n))
            }),
            ("environments in environments", |n| {
                format!(
                    "{}x{}",
                    r"\begin{matrix}".repeat(n),
                    r"\end{matrix}".repeat(n)
                )
            }),
            ("a macro that expands to a command", |n| {
                format!(r"\newcommand{{\s}}{{\sqrt}}{} x", r"\s".repeat(n))
            }),
            ("a macro that expands to an argument taker", |n| {
                format!(r"\newcommand{{\f}}{{\frac}}{}x", r"\f a ".repeat(n))
            }),
            ("a macro body invoked many times", |n| {
                format!(
                    r"\newcommand{{\d}}{{{}}}{} x",
                    r"\sqrt".repeat(4),
                    r"\d".repeat(n.div_ceil(4))
                )
            }),
            ("mixed", |n| {
                let mut source = String::new();
                let mut close = String::new();
                for level in 0..n {
                    match level % 4 {
                        0 => {
                            source.push_str(r"\frac{");
                            close.insert_str(0, "}{y}");
                        }
                        1 => {
                            source.push_str(r"\sqrt{");
                            close.insert(0, '}');
                        }
                        2 => {
                            source.push('{');
                            close.insert(0, '}');
                        }
                        _ => {
                            source.push_str("x^{");
                            close.insert(0, '}');
                        }
                    }
                }
                source + "z" + &close
            }),
        ]
    }

    /// Whether the guard lets this source through, which is the only question the limit answers.
    fn accepted(source: &str) -> bool {
        !matches!(convert_math(source), Err(MathRenderError::NestingTooDeep))
    }

    /// The deepest formula of this shape the guard accepts, found rather than assumed.
    fn deepest_accepted(build: fn(usize) -> String) -> usize {
        let (mut accepts, mut refuses) = (1usize, 1024usize);
        assert!(accepted(&build(accepts)), "one level must be accepted");
        assert!(
            !accepted(&build(refuses)),
            "a thousand levels must be refused"
        );
        while refuses - accepts > 1 {
            let middle = accepts + (refuses - accepts) / 2;
            if accepted(&build(middle)) {
                accepts = middle;
            } else {
                refuses = middle;
            }
        }
        accepts
    }

    /// **This is the measurement the limit rests on, made for every shape rather than for one.**
    ///
    /// There is no safe way to ask how many bytes of stack a level costs: the way to find out is to
    /// overflow, and an overflow takes the process rather than the test. What the limit actually
    /// has to be true of is narrower and can be measured without risking anything — that the
    /// deepest formula it *accepts* completes on the stack the math worker runs with. So for each
    /// shape this finds that formula, asserts one level more is refused (by the guard, which
    /// answers before it descends, so no deep recursion happened to produce the refusal), and draws
    /// it on a thread given exactly [`MATH_WORKER_STACK_BYTES`].
    ///
    /// The outcome of the draw is allowed to be a refusal — a formula nested two hundred levels can
    /// exceed the raster budget, and Typst's own parser stops at a depth of its own and says so —
    /// but it is never allowed to be a panic, and the thread is never allowed not to come back.
    /// That, and not the picture, is what the limit and the stack owe each other.
    #[test]
    fn every_recursive_shape_renders_at_the_limit_on_the_worker_stack() {
        for (name, build) in shapes() {
            let deepest = deepest_accepted(build);
            assert!(
                !accepted(&build(deepest + 1)),
                "{name}: {} levels must be refused, so {deepest} really is the edge",
                deepest + 1
            );
            assert!(
                deepest >= 60,
                "{name}: the limit must admit a formula deep enough to be worth drawing, not \
                 {deepest}"
            );
            // The deepest one that also fits the source budget, so `render` can be asked at all.
            let mut drawable = deepest;
            while build(drawable).len() > MAX_SOURCE_BYTES {
                drawable -= 1;
            }
            let source = build(drawable);
            // The conversion is the walk Folio owns, so it has to *succeed* at this depth — a
            // refusal further on is somebody's graceful limit and is reported below, but it must
            // not be what saved this run.
            let converted = convert_math(&source);
            assert!(
                converted.is_ok(),
                "{name}: {drawable} levels must convert, not merely escape the depth guard: \
                 {converted:?}"
            );
            let outcome = std::thread::Builder::new()
                .stack_size(MATH_WORKER_STACK_BYTES)
                .spawn(move || {
                    MathEngine::with_system_fonts(false)
                        .render(&source, key())
                        .err()
                })
                .expect("spawn a thread with the worker's stack")
                .join()
                .unwrap_or_else(|_| {
                    panic!("{name}: the deepest accepted formula must not take the thread with it")
                });
            eprintln!("{name}: accepted to {deepest}, drawn at {drawable} -> {outcome:?}");
            assert!(
                !matches!(outcome, Some(MathRenderError::Aborted)),
                "{name}: at {drawable} levels the render panicked somewhere inside itself"
            );
        }
    }

    /// The limit and the stack are one decision, so they are written down together.
    ///
    /// Sixteen mebibytes over two hundred and fifty-six levels is 64 KiB a level, and the walks —
    /// `mitex-parser`, MiTeX's converter, Typst's parser, its math layout, its SVG writer — run one
    /// after another rather than inside one another, so each of them has the whole of it. The
    /// factor is enormous on purpose: a recursive-descent frame is of the order of a hundred bytes,
    /// and the number this test really defends is the one
    /// `every_recursive_shape_renders_at_the_limit_on_the_worker_stack` measures.
    #[test]
    fn the_limit_and_the_worker_stack_are_one_pair() {
        assert_eq!(MAX_NESTING_DEPTH, 256);
        assert_eq!(MATH_WORKER_STACK_BYTES / MAX_NESTING_DEPTH, 64 * 1024);
    }

    /// A formula is refused for its *depth* and never for its length.
    ///
    /// **The last row is the false refusal a predicted bound produced.** `\frac12+` takes its two
    /// arguments as two characters out of one word — the parser splits `12+` and keeps the `+` —
    /// so three hundred of them stand side by side and nest nothing. A scan over tokens settled
    /// that word once and read three hundred levels; the parser, asked directly, builds four.
    #[test]
    fn a_flat_formula_is_not_refused_for_its_length() {
        let engine = MathEngine::with_system_fonts(false);
        for (name, source) in [
            (
                "commands with no argument",
                r"\alpha\beta\gamma".repeat(400),
            ),
            ("one long flat row", "x + ".repeat(1_000) + "y"),
            ("a left-associating command", r"\sum\limits".repeat(400)),
            (
                "arguments taken from inside a word",
                r"\frac12+".repeat(300),
            ),
        ] {
            assert!(source.len() <= MAX_SOURCE_BYTES, "{name}");
            assert_eq!(validate_source(&source), Ok(()), "{name}");
            assert!(convert_math(&source).is_ok(), "{name}");
        }
        assert!(
            engine.render(&r"\frac12+".repeat(300), key()).is_ok(),
            "and it draws"
        );
    }

    /// The formulas a person actually writes are drawn, at the sizes they are written at.
    #[test]
    fn the_formulas_a_person_writes_are_drawn() {
        let engine = MathEngine::with_system_fonts(false);
        let continued = (0..30).fold("1".to_owned(), |inner, _| {
            format!(r"1+\frac{{1}}{{{inner}}}")
        });
        let roots = (0..12).fold("x".to_owned(), |inner, _| format!(r"\sqrt{{1+{inner}}}"));
        for (name, source) in [
            ("a thirty-level continued fraction", continued),
            ("twelve nested roots", roots),
            (
                "an aligned block",
                r"\begin{aligned} a &= b + c \\ d &= e - f \end{aligned}".to_owned(),
            ),
            (
                "cases",
                r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & x = 0 \\ -1 & x < 0 \end{cases}".to_owned(),
            ),
            (
                "a matrix",
                r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}".to_owned(),
            ),
            ("a twelve by twelve matrix", {
                let row = ["x"; 12].join(" & ");
                format!(
                    r"\begin{{pmatrix}}{}\end{{pmatrix}}",
                    vec![row; 12].join(r" \\ ")
                )
            }),
            ("a forty-row alignment", {
                let rows = (0..40)
                    .map(|n| format!("a_{{{n}}} &= b_{{{n}}} &&= c_{{{n}}}"))
                    .collect::<Vec<_>>()
                    .join(r" \\ ");
                format!(r"\begin{{aligned}}{rows}\end{{aligned}}")
            }),
            ("a thirty-case definition", {
                let rows = (0..30)
                    .map(|n| format!("{n} & x = {n}"))
                    .collect::<Vec<_>>()
                    .join(r" \\ ");
                format!(r"f(x) = \begin{{cases}}{rows}\end{{cases}}")
            }),
        ] {
            assert_eq!(validate_source(&source), Ok(()), "{name}");
            assert!(engine.render(&source, key()).is_ok(), "{name}");
        }
    }

    /// **A formula is mathematics, not a program**, and this is the road that said otherwise.
    ///
    /// `\iftypst … \fi` is collected by MiTeX's parser without its structure being read and emitted
    /// verbatim, so a line a program printed into a terminal could carry Typst code: a loop that
    /// never returns (the math worker is one thread and nothing can interrupt it, so every later
    /// formula in every pane would stay as source), a loop that builds content nested past what
    /// Typst's unguarded math layout survives, or one that simply allocates until the process dies.
    /// Every case here is refused at the conversion boundary, which is before a Typst compiler is
    /// ever handed the text — so the loop is never run in order to find out.
    #[test]
    fn a_formula_that_carries_typst_code_is_refused_before_it_is_compiled() {
        let engine = MathEngine::with_system_fonts(false);
        for (name, source) in [
            ("a bare Typst block", r"\iftypst #1 + 1 \fi"),
            ("a loop that never ends", r"x \iftypst #while true {} \fi"),
            (
                "content built past any depth",
                r"\iftypst #{ let c = $x$; for _ in range(20000) { c = $sqrt(c)$ }; c } \fi",
            ),
            (
                "an allocation that never ends",
                r"\iftypst #range(0, 100000000) \fi",
            ),
            (
                "the same, hidden in a macro body",
                r"\newcommand{\q}{\iftypst #while true {} \fi}x\q",
            ),
            (
                "the same, reached through two macros",
                r"\newcommand{\q}{\iftypst #while true {} \fi}\newcommand{\r}{\q}x\r",
            ),
            ("an else branch", r"\iftypst #1 \else #2 \fi"),
        ] {
            assert_eq!(
                convert_math(source),
                Err(MathRenderError::RawTypstCode),
                "{name}"
            );
            assert_eq!(
                engine.render(source, key()),
                Err(MathRenderError::RawTypstCode),
                "{name}: and a render answers the same"
            );
            assert_eq!(
                MathRenderError::RawTypstCode.failure_stage(),
                Some(MathFailureStage::Convert)
            );
        }
        // The rest of the `\if…` family is the same lexer feature and none of it copies source
        // text: `\iffalse` drops its body, `\iftrue` passes the body on as ordinary LaTeX, which
        // the token map escapes like anything else, and the conditionals MiTeX does not implement
        // are commands it has no rule for.
        assert_eq!(
            convert_math(r"x\iffalse #while true {} \fi"),
            Ok("x ".into())
        );
        let passed_through = convert_math(r"x\iftrue #while true {} \fi").expect("ordinary LaTeX");
        assert!(
            passed_through.contains(r"\#") && !passed_through.contains(" #"),
            "a body passed through is escaped, not copied: {passed_through:?}"
        );
        assert!(matches!(
            convert_math(r"x\ifnum 1=1 y\fi"),
            Err(MathRenderError::Convert(_))
        ));
    }

    /// The other two sites that copy source text into the output without mapping it.
    ///
    /// `\includegraphics` writes `#image("…")` with the path taken from the formula, quote marks
    /// and all, so a `"` in it closes the string and the rest is code; `\label` writes `<…>` into
    /// Typst markup, which ends at the first `>`. Both are refused by the conversion. The first is
    /// refused twice over — [`validate_source`] has always named it a file command — and this
    /// asserts the conversion refuses it on its own, because a blocklist of spellings is not a
    /// reason to believe anything.
    #[test]
    fn the_other_verbatim_roads_into_the_typst_source_are_refused() {
        assert_eq!(
            convert_math(r#"\includegraphics{a"); while true {}; #("}"#),
            Err(MathRenderError::RawTypstCode)
        );
        assert_eq!(
            validate_source(r#"\includegraphics{x}"#),
            Err(MathRenderError::UnsafeCommand)
        );
        // `\label` writes nothing in math mode, which is every formula's mode, so it is untouched;
        // the text mode it *would* write in is reached through a command whose Typst name is a
        // function call, and there it is refused.
        assert_eq!(convert_math(r"x\label{a}"), Ok("x ".into()));
        assert_eq!(
            convert_math(r"\text{\label{a>#while true {}}}"),
            Err(MathRenderError::RawTypstCode)
        );
    }

    /// **No source character can open Typst code, because the lexer's own word class excludes
    /// every character that would.**
    ///
    /// This is the reason the conversion is confined to math content rather than a hope about it.
    /// A source character reaches the converted output in exactly two ways: as its own token, which
    /// the converter maps to a fixed escape or drops, or inside a `Word`. So the question is what a
    /// `Word` may contain, and the answer is asked of the lexer rather than read off its regex.
    #[test]
    fn a_word_token_cannot_carry_a_character_that_opens_typst_code() {
        use mitex_lexer::{Lexer, Token};
        for forbidden in [
            '#', '$', '"', '\\', '{', '}', '[', ']', '(', ')', '%', '^', '_',
        ] {
            let source = format!("a{forbidden}b");
            let mut lexer = Lexer::<()>::new(&source, DEFAULT_SPEC.clone());
            let words: String = std::iter::from_fn(|| lexer.eat())
                .filter(|(token, _)| *token == Token::Word)
                .map(|(_, text)| text)
                .collect();
            assert!(
                !words.contains(forbidden),
                "{forbidden:?} reached a Word token, as {words:?}"
            );
        }
    }

    /// Every name MiTeX may call in the Typst it writes: the specification's own, and the three
    /// the converter spells out for itself.
    fn typst_vocabulary() -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for (name, item) in DEFAULT_SPEC.items() {
            let alias = match item {
                mitex::CommandSpecItem::Cmd(shape) => shape.alias.clone(),
                mitex::CommandSpecItem::Env(shape) => shape.alias.clone(),
            };
            // An alias may be a whole call — `\section` is `#heading(level: 1)` — and what is
            // being collected is the name it calls.
            let name = alias.unwrap_or_else(|| name.to_owned());
            names.insert(
                name.trim_start_matches('#')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                    .collect(),
            );
        }
        // `convert_formula`, and `convert_normal_command`'s two text-mode hacks.
        names.insert("math.equation".to_owned());
        names.insert("strong".to_owned());
        names.insert("emph".to_owned());
        names
    }

    /// Every `#` in `converted` that Typst would read as the start of code, with its name.
    fn code_introducers(converted: &str) -> Vec<String> {
        let characters: Vec<char> = converted.chars().collect();
        characters
            .iter()
            .enumerate()
            .filter(|(at, character)| {
                // `\#` is the escape the converter writes for a source `#`; Typst draws it.
                **character == '#' && (*at == 0 || characters[at - 1] != '\\')
            })
            .map(|(at, _)| {
                characters[at + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                    .collect()
            })
            .collect()
    }

    /// **The invariant: the Typst a formula converts to is math content, and every function it
    /// calls is one MiTeX itself named.**
    ///
    /// Stated exactly: in the converted source, every `#` is either escaped — `\#`, which Typst
    /// draws as a hash — or the first character of a name from
    /// [`typst_vocabulary`]. Nothing a formula contains can add a name to that set, because the
    /// three sites that copied source text through unmapped are refused
    /// (`the_other_verbatim_roads_into_the_typst_source_are_refused`,
    /// `a_formula_that_carries_typst_code_is_refused_before_it_is_compiled`) and because no `Word`
    /// can carry a `#` (`a_word_token_cannot_carry_a_character_that_opens_typst_code`).
    ///
    /// The corpus is the whole specification rather than a list somebody thought of: every command
    /// and environment MiTeX knows, invoked five ways, two of them hostile.
    #[test]
    fn every_formula_that_converts_calls_only_names_mitex_itself_wrote() {
        let vocabulary = typst_vocabulary();
        let mut converted_count = 0usize;
        let hostile = r#"{"); #while true {}; ("}"#;
        let mut check = |source: &str| {
            let Ok(converted) = convert_math(source) else {
                return;
            };
            converted_count += 1;
            for name in code_introducers(&converted) {
                assert!(
                    vocabulary.contains(&name),
                    "{source:?} converted to {converted:?}, which calls #{name}"
                );
            }
        };
        for (name, item) in DEFAULT_SPEC.items() {
            let environment = matches!(item, mitex::CommandSpecItem::Env(_));
            for argument in ["", "{a}", "{a}{b}", "[1]{a}", hostile] {
                if environment {
                    check(&format!("\\begin{{{name}}}{argument}a\\end{{{name}}}"));
                } else {
                    check(&format!("\\{name}{argument}"));
                }
            }
        }
        for source in [
            r"\text{a #b $c$ \# d}",
            r#"\text{a " b}"#,
            r"\text{<a>#while true {}</a>}",
            r"\begin{tabular}{lc}a&b\\c&d\end{tabular}",
            r"\begin{array}{c|c}a&b\end{array}",
            r"\newcommand{\q}[1]{\text{#1}}\q{x}",
            r"\color{red}\text{x}",
            r"\frac{1}{2}+\sqrt[3]{x}\substack{a\\b}",
        ] {
            check(source);
        }
        assert!(
            converted_count > 500,
            "the corpus must actually convert something: {converted_count}"
        );
    }

    /// **A length is read, not run.**
    ///
    /// `\hspace`, `\vspace` and `\raisebox` used to read their argument back out of the typeset
    /// content and hand the string to Typst's `eval` — which is an arbitrary Typst expression
    /// assembled from a line a program printed. Measured on 2026-09-17, before the fix:
    /// `x\hspace{4pt*10}x` drew 96 pixels wide, exactly as `x\hspace{40pt}x` does, and
    /// `x\hspace{(2pt+2pt)*10}x` drew the same again — so digits, letters, parentheses and
    /// operators all reached `eval`, and `range(0, 100000000)` is spelled with the same characters.
    /// `assets/mitex-specs/latex/standard.typ` parses the length instead; the arithmetic fails the
    /// compile and the formula stays as the text the terminal printed.
    #[test]
    fn a_length_is_parsed_and_never_evaluated() {
        let engine = MathEngine::with_system_fonts(false);
        let plain = engine.render("xx", key()).expect("a formula").width_px;
        let spaced = engine
            .render(r"x\hspace{40pt}x", key())
            .expect("a length still makes space")
            .width_px;
        assert!(spaced > plain, "{spaced} must be wider than {plain}");
        for unit in ["pt", "mm", "cm", "in", "em"] {
            assert!(
                engine
                    .render(&format!(r"x\hspace{{2{unit}}}x"), key())
                    .is_ok(),
                "{unit} is a length"
            );
        }
        for arithmetic in [
            r"x\hspace{4pt*10}x",
            r"x\hspace{(2pt+2pt)*10}x",
            r"x\vspace{4pt*10}x",
            r"x\raisebox{4pt*2}{y}x",
        ] {
            assert!(
                matches!(
                    engine.render(arithmetic, key()),
                    Err(MathRenderError::Compile(_))
                ),
                "{arithmetic} must not be evaluated"
            );
        }
    }

    /// One wide row and a column of empty ones: a rectangle, from almost no bytes.
    ///
    /// `rows` separators and `rows` columns, so `(rows+1)²` cells out of `3*rows+29` bytes.
    fn sparse_array(rows: usize) -> String {
        format!(
            r"\begin{{array}}{{l}}x{}{}x\end{{array}}",
            "&".repeat(rows),
            r"\\".repeat(rows)
        )
    }

    /// **The third budget: what a formula asks to have *laid out*, which its bytes and its nesting
    /// both fail to bound.**
    ///
    /// An environment with rows becomes a rectangle — MiTeX's `array` pads every row out to the
    /// widest one, and Typst's `mat` does the same — so a source that writes one wide row and a
    /// column of empty ones asks for their product. [`sparse_array`] is 3N+29 bytes at constant
    /// nesting: at the 8 KiB source budget it asks for more than seven million cells, which is
    /// minutes of the one math worker and gigabytes of a process whose allocation failure is an
    /// abort rather than an unwind. Measured on the worker's own thread, the cost is *linear* in
    /// cells — 81 in 13ms, 1089 in 55ms, 2401 in 121ms, 4225 in 254ms, about 60µs each — so a
    /// budget on cells is a budget on the time and the memory, and
    /// [`mitex::MAX_LAYOUT_CELLS`] of them is a 64x64 matrix.
    ///
    /// The refusal is at the conversion, so nothing is ever laid out to find out: 4225 cells come
    /// back refused in 77µs.
    #[test]
    fn a_formula_may_not_ask_for_more_cells_than_it_can_be_drawn_with() {
        let engine = MathEngine::with_system_fonts(false);
        let side = |cells: usize| (cells as f64).sqrt() as usize - 1;
        let under = sparse_array(side(mitex::MAX_LAYOUT_CELLS));
        let over = sparse_array(side(mitex::MAX_LAYOUT_CELLS) + 1);
        assert!(under.len() < 300 && over.len() < 300, "almost no bytes");
        assert!(convert_math(&under).is_ok(), "the budget itself is allowed");
        let started = std::time::Instant::now();
        let drawn = engine.render(&under, key()).expect("and it draws");
        eprintln!(
            "at the budget: {} cells, {}x{} pixels, {} bytes, {:?}",
            mitex::MAX_LAYOUT_CELLS,
            drawn.width_px,
            drawn.height_px,
            drawn.resident_bytes(),
            started.elapsed()
        );
        assert!(
            drawn.resident_bytes() < MAX_RASTER_BYTES / 8,
            "and the picture it draws is a small part of the raster budget: {} bytes",
            drawn.resident_bytes()
        );
        assert_eq!(
            convert_math(&over),
            Err(MathRenderError::TooManyLayoutCells),
            "one cell more is refused, at the conversion, before anything is laid out"
        );
        assert_eq!(
            engine.render(&over, key()),
            Err(MathRenderError::TooManyLayoutCells)
        );
        assert_eq!(
            MathRenderError::TooManyLayoutCells.failure_stage(),
            Some(MathFailureStage::Convert)
        );

        // The same rectangle, assembled by macros the guard sees expanded.
        let through_macros = format!(
            r"\newcommand{{\c}}{{{}}}\newcommand{{\r}}{{{}}}\begin{{array}}{{l}}x{}{}x\end{{array}}",
            "&".repeat(10),
            r"\\".repeat(10),
            r"\c".repeat(7),
            r"\r".repeat(7),
        );
        assert_eq!(
            convert_math(&through_macros),
            Err(MathRenderError::TooManyLayoutCells)
        );

        // The budget belongs to the formula, not to one environment, so several of them add up.
        let block = |n: usize| {
            let row = vec!["x"; n].join("&");
            format!(
                r"\begin{{pmatrix}}{}\end{{pmatrix}}",
                vec![row; n].join(r"\\")
            )
        };
        let refused = |source: &str| {
            matches!(
                convert_math(source),
                Err(MathRenderError::TooManyLayoutCells)
            )
        };
        assert!(convert_math(&block(30)).is_ok(), "900 cells on their own");
        assert!(
            refused(&vec![block(30); 5].join("+")),
            "but five of them are four and a half thousand"
        );
        // And a matrix inside a matrix is charged for each rectangle, out of the same budget.
        assert!(convert_math(&block(50)).is_ok(), "2500 cells on their own");
        assert!(
            refused(&format!(
                r"\begin{{pmatrix}}{}&{}\\c&d\end{{pmatrix}}",
                block(50),
                block(50)
            )),
            "two of them inside a third are five thousand"
        );
    }

    /// A length is a length, and not a distance nothing can draw.
    ///
    /// Measured 2026-09-17: an enormous one was already refused downstream, by the
    /// raster-dimension check, which runs before a pixmap is allocated —
    /// `\hspace{999999999999999999999999pt}` came back "raster dimensions are invalid or too large"
    /// in ten milliseconds and allocated nothing. The cap in `mitex-length` says the same thing at
    /// the length itself, so the answer does not rest on a float surviving a cast three stages on.
    #[test]
    fn a_length_longer_than_a_page_is_refused_at_the_length() {
        let engine = MathEngine::with_system_fonts(false);
        assert!(
            engine.render(r"x\hspace{1000pt}x", key()).is_ok(),
            "a long but drawable space still draws"
        );
        for source in [
            r"x\hspace{100000pt}x",
            r"x\hspace{999999999999999999999999pt}x",
            r"x\vspace{999999999pt}x",
            r"x\raisebox{999999999pt}{y}x",
            r"x\hspace{99999999in}x",
            r"\xrightarrow{\hspace{100000pt}}",
        ] {
            assert!(
                matches!(
                    engine.render(source, key()),
                    Err(MathRenderError::Compile(_))
                ),
                "{source}"
            );
        }
    }

    fn key() -> MathRenderKey {
        MathRenderKey {
            dpi_milli: NonZeroU32::new(1000).unwrap(),
            font_milli_pt: NonZeroU32::new(12_000).unwrap(),
            foreground_rgb: [224, 224, 224],
            mode: MathMode::Display,
        }
    }

    /// PIN — **a caller that asks in device pixels gets device pixels.**
    ///
    /// The identity half would go red the moment [`key_for_em_px`] inverted only
    /// one of this crate's two `96/72` factors — the exact mistake the baseline
    /// carried for as long as those factors were written down in one place and
    /// used in two. The proportional half is what proves the key reaches the
    /// raster at all: a key that were ignored would give two identical pictures.
    #[test]
    fn a_key_asked_for_an_em_in_device_pixels_sets_that_em() {
        for em in [12.0_f32, 20.0, 33.5] {
            let key = key_for_em_px(em, [0, 0, 0], MathMode::Inline).expect("a positive em");
            let back = key.font_milli_pt.get() as f32 / 1000.0 * device_px_per_pt(key.dpi_milli);
            assert!(
                (back - em).abs() < 0.5,
                "asked for {em} device px, the key means {back}",
            );
        }
        let engine = MathEngine::with_system_fonts(false);
        let single = engine
            .render(
                "x^2 + y^2",
                key_for_em_px(16.0, [0, 0, 0], MathMode::Inline).expect("a positive em"),
            )
            .expect("a formula this simple compiles");
        let double = engine
            .render(
                "x^2 + y^2",
                key_for_em_px(32.0, [0, 0, 0], MathMode::Inline).expect("a positive em"),
            )
            .expect("a formula this simple compiles");
        let ratio = double.width_px as f32 / single.width_px as f32;
        assert!(
            (ratio - 2.0).abs() < 0.1,
            "twice the em is twice the picture: {}px against {}px",
            double.width_px,
            single.width_px,
        );
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
        // Windows); Folio neither embeds nor redistributes their bytes.
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

    /// A font book that answers exactly the coverage written here, and nothing
    /// else about a font is needed to decide which families draw a request.
    fn book_of(faces: &[(&str, &str)]) -> FontBook {
        use typst_library::text::{Coverage, FontFlags, FontInfo, FontVariant};
        FontBook::from_infos(faces.iter().map(|(family, covers)| FontInfo {
            family: (*family).to_owned(),
            variant: FontVariant::default(),
            flags: FontFlags::empty(),
            axes: Vec::new(),
            coverage: Coverage::from_vec(covers.chars().map(|c| c as u32).collect()),
        }))
    }

    /// RED GATE (2026-08-31, `docs/DESIGN.md` §7.1.3i′ ⑫) — **a partial font in
    /// front does not decide the answer for the ones behind it.**
    ///
    /// The CI failure this pins was a judgment that split down the middle on one
    /// machine: `\text{中}`, `\text{文}` and `\text{项目数}` drew on a GitHub
    /// Windows runner while `\text{死} \; + \; \text{活}` in the same suite came
    /// back `MissingCjkGlyph`. Nothing was missing. One face — the single one
    /// Typst's name-similarity fallback reaches — had to answer for all of it,
    /// and it could not: the runner's `Gulim` shapes 死 and returns `.notdef`
    /// for 活, whose codepoint its own `cmap` claims. The old check read that
    /// one face's `.notdef` as a fact about the machine.
    ///
    /// A face that carries part of a request and a face that *claims* part of a
    /// request are the same defect from here, because both make one candidate
    /// decide the answer. The book below is the first shape, which is the one a
    /// constructed `Coverage` can state; the second is why the tail of the list
    /// exists, and it is measured on the runner rather than modelled here.
    ///
    /// MUTATIONS, both verified red 2026-08-31: stop at the first family that
    /// claims anything and 活 is left with no font at all, which is the defect
    /// itself; keep the minimal cover and drop the tail, and the one family that
    /// leads has to be right about every glyph it claimed — which is the shape
    /// the runner disproved.
    #[test]
    fn a_partial_font_read_first_does_not_answer_for_the_glyphs_it_lacks() {
        let requested = BTreeSet::from(['死', '活']);

        // The runner's shape: the face that gets tried first carries half of it.
        let split = book_of(&[("PartialFront", "死中文"), ("WholeBehind", "死活中文")]);
        let families = covering_families(&split, &requested);
        assert!(
            families.iter().any(|f| f == "WholeBehind"),
            "the family that draws 活 has to be named, whatever is in front of it: {families:?}"
        );
        assert_eq!(
            families.first().map(String::as_str),
            Some("WholeBehind"),
            "the family that answers for most of the request leads: {families:?}"
        );
        assert!(
            families.iter().any(|f| f == "PartialFront"),
            "and every other claimant is still behind it, because a cmap is a \
             claim and the leader's may be the one that breaks: {families:?}"
        );

        // No one face covers the request; two between them do, and both are named.
        let shared = book_of(&[("DeadOnly", "死"), ("LivingOnly", "活")]);
        assert_eq!(
            covering_families(&shared, &requested),
            vec!["DeadOnly".to_owned(), "LivingOnly".to_owned()],
        );

        // One face answering for everything is one family, and it is named once.
        assert_eq!(
            covering_families(&book_of(&[("Whole", "死活中文")]), &requested),
            vec!["Whole".to_owned()],
        );

        // A request this book can only half answer still names the half it can,
        // and stops rather than looping. What happens to 活 is then the page's
        // to report — see `missing_cjk_font_returns_source_fallback_signal_
        // instead_of_tofu`, which is that end of it against the real engine.
        // Alphabetical, because these two are tied on what they answer and the
        // book's own order is how ties are broken.
        assert_eq!(
            covering_families(
                &book_of(&[("DeadOnly", "死"), ("AlsoDeadOnly", "死")]),
                &requested
            ),
            vec!["AlsoDeadOnly".to_owned(), "DeadOnly".to_owned()],
        );
        assert!(covering_families(&book_of(&[]), &requested).is_empty());
    }

    /// The families this machine's own book offers for a request draw all of it
    /// — the same claim as the gate above, made against real installed fonts
    /// rather than a constructed book.
    #[test]
    fn the_families_named_for_a_request_draw_every_character_of_it() {
        let engine = MathEngine::new();
        let requested = BTreeSet::from(['死', '活', '中', '文', '项', '目', '数']);
        let names = engine
            .engine
            .with_world(|world| covering_families(world.book(), &requested))
            .unwrap();
        engine
            .engine
            .with_world(|world| {
                let book = world.book();
                for character in &requested {
                    assert!(
                        names.iter().any(|name| book
                            .select_family(&name.to_lowercase())
                            .filter_map(|face| book.info(face))
                            .any(|info| info.coverage.contains(*character as u32))),
                        "{character} is in the request and in no named family: {names:?}"
                    );
                }
            })
            .unwrap();
    }

    /// The family the cover measures against is the family the template sets
    /// the mathematics in. Two strings saying the same thing is exactly how a
    /// cover quietly starts answering a question nobody asked.
    #[test]
    fn the_math_family_is_the_one_the_template_names() {
        assert!(
            TYPST_TEMPLATE.contains(&format!("({MATH_FAMILY:?},)")),
            "the template must set the equation in {MATH_FAMILY}"
        );
    }

    /// PIN — **an ordinary formula asks the machine for nothing.**
    ///
    /// The cover exists for the characters the math font does not carry, and a
    /// formula made of mathematics has none: the list it produces is empty, the
    /// template's font list is then the math family alone, and the page is
    /// byte-for-byte what Typst draws unaided. That is what lets DESIGN §13.40
    /// ⑥'s two machines print the same digest at all, and it is what keeps this
    /// crate from having a taste in fonts it has no business having.
    ///
    /// MUTATION: let the filter keep characters the math family *does* claim and
    /// every Latin letter of every formula drags the whole book in behind it.
    #[test]
    fn a_formula_the_math_font_answers_for_names_no_other_family() {
        let engine = MathEngine::new();
        engine
            .engine
            .with_world(|world| {
                for source in [
                    r"-x",
                    r"x-y",
                    "lr((-frac((x - mu)^2, 2 sigma^2)))",
                    "exp negthinspace lr((-frac(1, sqrt(2 pi sigma^2))))",
                    "integral_0^1 x dif x",
                    "partial / (partial t) Psi = planck.reduce omega",
                ] {
                    assert!(
                        characters_the_math_font_lacks(world.book(), source).is_empty(),
                        "{source} asks for nothing {MATH_FAMILY} cannot draw, so no \
                         other family may be named for it: {:?}",
                        characters_the_math_font_lacks(world.book(), source)
                    );
                }
                // And a character it genuinely does not carry is still asked
                // about — this is the CJK case, arrived at by the general road.
                assert_eq!(
                    characters_the_math_font_lacks(world.book(), r#"text("死活")"#),
                    BTreeSet::from(['死', '活']),
                );
            })
            .expect("a world built with no inputs");
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

    /// RED GATE (2026-08-28, `docs/DESIGN.md` §7.1.3k) — **an SVG with words in
    /// it draws the words.**
    ///
    /// `usvg::Options::default()` starts with an empty font database and drops
    /// every `<text>` it cannot shape, silently. That was invisible while the
    /// only documents reaching this function were Typst's own output — a typeset
    /// formula is paths — and it stopped being invisible the day a markdown page
    /// began drawing an author's SVG: this repository's `README.md` hero is
    /// sixteen `<text>` elements and it came back as its background alone.
    ///
    /// MUTATION: drop the `load_system_fonts()` call and this raster comes back
    /// entirely transparent (verified, 2026-08-28).
    #[test]
    fn an_svg_that_says_something_draws_it() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40">
            <text x="4" y="30" font-family="Arial, sans-serif" font-size="28"
                  fill="#000000">Folio</text>
        </svg>"##;
        let raster = rasterize_svg_document(svg).unwrap();
        let inked = raster.rgba.chunks_exact(4).filter(|px| px[3] != 0).count();
        assert!(
            inked > 0,
            "a machine with fonts on it can set five letters; \
             an empty font database silently sets none",
        );
    }

    /// RED GATE — **an SVG cannot name a file on this machine and have it
    /// opened.**
    ///
    /// usvg's stock `<image href>` resolver treats every href that is not a
    /// `data:` URI as a path and reads it, so a document saying
    /// `<image href="\\attacker\share\p.png"/>` makes this process open that
    /// path — a read of a local file, or a connection to whoever owns that
    /// share. The document does not have to be clicked to get here: a printed
    /// `.svg` path is queued for decoding on sight, and decoding lands in
    /// [`rasterize_svg_document`].
    ///
    /// Three assertions, because no one of them says the whole thing. The first
    /// is what a user could see: an `<image>` naming a real `.svg` on disk drew
    /// that file's contents into the raster. The second says the raster is
    /// byte-for-byte what the same document produces when the file it names is
    /// not there, which is only true if nothing was read. The third asks the
    /// resolver itself, for the formats this build would not have drawn anyway
    /// — the danger in a `.png` href is the open, not the pixels.
    ///
    /// MUTATION: restore `ImageHrefResolver::default_string_resolver()` and the
    /// magenta is drawn, the two rasters differ, and the resolver hands back
    /// the bytes of both files.
    #[test]
    fn an_svg_cannot_make_folio_open_a_file_it_names() {
        let dir = std::env::temp_dir().join(format!("bt-math-svg-href-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Each file is nothing but the one colour, so a single pixel of it in
        // the raster is proof the file was read.
        let secret_svg = dir.join("secret.svg");
        std::fs::write(
            &secret_svg,
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <rect width="16" height="16" fill="#ff00ff"/>
            </svg>"##,
        )
        .unwrap();
        let secret_png = dir.join("secret.png");
        let mut pixmap = resvg::tiny_skia::Pixmap::new(16, 16).unwrap();
        pixmap.fill(resvg::tiny_skia::Color::from_rgba8(255, 0, 255, 255));
        std::fs::write(&secret_png, pixmap.encode_png().unwrap()).unwrap();

        let href = |path: &std::path::Path| path.to_string_lossy().replace('\\', "/");
        let document = |href: &str| {
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="16" height="16">
                    <image href="{href}" xlink:href="{href}" x="0" y="0" width="16" height="16"/>
                </svg>"##
            )
            .into_bytes()
        };

        let named = rasterize_svg_document(&document(&href(&secret_svg))).unwrap();
        let missing =
            rasterize_svg_document(&document(&href(&dir.join("no-such-file.svg")))).unwrap();

        assert!(
            !named
                .rgba
                .chunks_exact(4)
                .any(|pixel| pixel[..3] == [255, 0, 255]),
            "the file this document named is on disk, and none of it reached the raster"
        );
        assert_eq!(
            (named.width_px, named.height_px),
            (missing.width_px, missing.height_px)
        );
        assert_eq!(
            named.rgba, missing.rgba,
            "naming a file that exists draws exactly what naming one that does not draws"
        );

        let options = svg_document_options();
        for path in [&secret_svg, &secret_png] {
            assert!(
                (options.image_href_resolver.resolve_string)(&href(path), options).is_none(),
                "an href is not a door onto {}",
                path.display()
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
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
        let corpus = include_str!("../../../tests/corpus/math-expressions.jsonl");
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
