// MODIFIED BY THE FOLIO CONTRIBUTORS — not the upstream
// mitex-parser 0.2.4 crate root.
// Change: the `depth` module, and the `*_bounded` entry points that report a
// parse refused for depth instead of returning a truncated tree.
// Index: vendor/mitex-parser/CHANGES-FOLIO.md
// Notice given under section 4(b) of the Apache License, Version 2.0.

//! Given source strings, MiTeX Parser provides an AST (abstract syntax tree).
//!
//! ## Option: Command Specification
//! The parser retrieves a command specification which defines shape of
//! commands. With the specification, the parser can parse commands correctly.
//! Otherwise, all commands are parsed as barely names without arguments.
//!
//! ## Produce: AST
//! It returns an untyped syntax node representing the AST defined by [`rowan`].
//! You can access the AST conveniently with interfaces provided by
//! [`rowan::SyntaxNode`].
//!
//! The untyped syntax node can convert to typed ones defined in
//! [`crate::syntax`].
//!
//! The untyped syntax node can also convert to [`rowan::cursor::SyntaxNode`] to
//! modify the AST syntactically.

// Folio: upstream's code is held to upstream's lint standards, not to whatever the clippy of the
// day has added since it was published. Both of these fire only in files this workspace has not
// otherwise touched, and an allow here is what keeps those files byte-identical to the published
// crate — which is the thing `scripts/check-vendor-notices.ps1` reads to tell a change from a
// reformatting.
#![allow(clippy::doc_lazy_continuation, clippy::unnecessary_map_or)]

mod arg_match;
mod depth;
mod parser;
pub mod syntax;

pub use depth::{MAX_TREE_DEPTH, NestingTooDeep, tree_depth};
pub use mitex_spec as spec;
pub use spec::preludes::command as command_preludes;
pub use spec::*;
use syntax::SyntaxNode;

use parser::Parser;

/// Parse the input text with the given command specification
/// and return the untyped syntax tree
///
/// The error nodes are attached to the tree
///
/// Folio: the tree is never deeper than [`MAX_TREE_DEPTH`]. A source that would have gone deeper
/// comes back truncated at that depth; [`parse_bounded`] is the entry point that says so instead.
pub fn parse(input: &str, spec: CommandSpec) -> SyntaxNode {
    SyntaxNode::new_root(Parser::new_macro(input, spec).parse().0)
}

/// It is only for internal testing
pub fn parse_without_macro(input: &str, spec: CommandSpec) -> SyntaxNode {
    SyntaxNode::new_root(Parser::new(input, spec).parse().0)
}

/// Folio: [`parse`], refusing a source whose syntax tree would be deeper than [`MAX_TREE_DEPTH`].
///
/// The refusal is decided twice, on purpose. The parser sets a flag the moment it declines to build
/// a level — which is *before* it would have descended, and is the only place a stack overflow can
/// still be prevented — and the depth of the tree that came back is then measured iteratively and
/// checked against the same limit. The second check believes nothing the first one says: it reads
/// the tree that exists. Either one refusing is a refusal, because whatever follows this call —
/// `mitex`'s converter above all — recurses to the depth of this tree.
pub fn parse_bounded(input: &str, spec: CommandSpec) -> Result<SyntaxNode, NestingTooDeep> {
    bounded(Parser::new_macro(input, spec).parse())
}

/// Folio: [`parse_without_macro`], refused past [`MAX_TREE_DEPTH`]. Used by `mitex`'s own
/// no-macro conversion entry point.
pub fn parse_without_macro_bounded(
    input: &str,
    spec: CommandSpec,
) -> Result<SyntaxNode, NestingTooDeep> {
    bounded(Parser::new(input, spec).parse())
}

fn bounded(
    (green, overflowed): (rowan::GreenNode, bool),
) -> Result<SyntaxNode, NestingTooDeep> {
    if overflowed {
        return Err(NestingTooDeep);
    }
    let node = SyntaxNode::new_root(green);
    if tree_depth(&node) > MAX_TREE_DEPTH {
        return Err(NestingTooDeep);
    }
    Ok(node)
}
