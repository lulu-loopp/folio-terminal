// MODIFIED BY THE FOLIO CONTRIBUTORS — not the upstream
// mitex 0.2.4 crate root.
// Change: the `*_bounded` conversion entry points, which refuse a formula
// whose syntax tree is deeper than the parser and converter will descend.
// Index: vendor/mitex/CHANGES-FOLIO.md
// Notice given under section 4(b) of the Apache License, Version 2.0.

mod converter;

pub use mitex_parser::command_preludes;
use mitex_parser::parse;
use mitex_parser::parse_bounded;
use mitex_parser::parse_without_macro;
use mitex_parser::parse_without_macro_bounded;
pub use mitex_parser::spec::*;
pub use mitex_parser::{MAX_TREE_DEPTH, NestingTooDeep, tree_depth};

pub use converter::{BoundedConvertError, MAX_LAYOUT_CELLS};
use converter::LaTeXMode;
use converter::convert_inner;
use converter::convert_inner_bounded;

pub fn convert_text(input: &str, spec: Option<CommandSpec>) -> Result<String, String> {
    convert_inner(input, LaTeXMode::Text, spec, parse)
}

pub fn convert_math(input: &str, spec: Option<CommandSpec>) -> Result<String, String> {
    convert_inner(input, LaTeXMode::Math, spec, parse)
}

/// For internal testing
pub fn convert_math_no_macro(input: &str, spec: Option<CommandSpec>) -> Result<String, String> {
    convert_inner(input, LaTeXMode::Math, spec, parse_without_macro)
}

/// Folio: [`convert_math`], with a formula nested past [`MAX_TREE_DEPTH`] refused by name.
///
/// Both walks this runs are recursive descent — `mitex-parser` over the tokens, then the converter
/// over the tree it built — and a stack overflow is not something the caller can contain. So the
/// depth is enforced inside each of them, at the point where a level would be created, and the
/// answer here is what those enforcements decided.
pub fn convert_math_bounded(
    input: &str,
    spec: Option<CommandSpec>,
) -> Result<String, BoundedConvertError> {
    convert_inner_bounded(input, LaTeXMode::Math, spec, parse_bounded)
}

/// Folio: [`convert_text`], with a formula nested past [`MAX_TREE_DEPTH`] refused by name.
pub fn convert_text_bounded(
    input: &str,
    spec: Option<CommandSpec>,
) -> Result<String, BoundedConvertError> {
    convert_inner_bounded(input, LaTeXMode::Text, spec, parse_bounded)
}

/// For internal testing
pub fn convert_math_no_macro_bounded(
    input: &str,
    spec: Option<CommandSpec>,
) -> Result<String, BoundedConvertError> {
    convert_inner_bounded(input, LaTeXMode::Math, spec, parse_without_macro_bounded)
}
