// MODIFIED BY THE FOLIO CONTRIBUTORS — not the upstream
// mitex-parser 0.2.4 crate root.
// Change: two clippy allows, so upstream's own files are held to upstream's
// lint standards rather than to this workspace's.
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
mod parser;
pub mod syntax;

pub use mitex_spec as spec;
pub use spec::preludes::command as command_preludes;
pub use spec::*;
use syntax::SyntaxNode;

use parser::Parser;

/// Parse the input text with the given command specification
/// and return the untyped syntax tree
///
/// The error nodes are attached to the tree
pub fn parse(input: &str, spec: CommandSpec) -> SyntaxNode {
    SyntaxNode::new_root(Parser::new_macro(input, spec).parse())
}

/// It is only for internal testing
pub fn parse_without_macro(input: &str, spec: CommandSpec) -> SyntaxNode {
    SyntaxNode::new_root(Parser::new(input, spec).parse())
}
