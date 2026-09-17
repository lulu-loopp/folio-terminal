// MODIFIED BY THE FOLIO CONTRIBUTORS — this file is not part of upstream
// mitex-parser 0.2.4; it is added by Folio.
// Change: the depth bound the parser is held to, enforced as it builds.
// Index: vendor/mitex-parser/CHANGES-FOLIO.md
// Notice given under section 4(b) of the Apache License, Version 2.0.

//! The depth a parse is allowed to reach, enforced where depth is created.
//!
//! **A stack overflow is not a panic.** No `catch_unwind` contains one: the process dies. The
//! parser in `parser.rs` is recursive descent over text that, in Folio, a program merely printed
//! into a terminal, so the depth it will reach cannot be *predicted* from the token stream — it has
//! to be *refused* at the moment it would be created. Every cycle in that parser's call graph
//! passes through `Parser::content`, and every one of those cycles opens at least one syntax node
//! before it recurses, so a cap on the depth of the tree being built is also a cap on the parser's
//! own recursion.
//!
//! Depth is not created only by descending. `start_node_at` *wraps* syntax that is already built —
//! `\limits`, `'`, `\over`, `\displaystyle` all do — and that adds a level to the tree without a
//! single extra stack frame. So this tracks the height of the tree exactly rather than counting
//! open nodes, by mirroring `rowan::GreenNodeBuilder`'s own flat child stack: one height per
//! finished child, one frame per open node, and a wrap re-parents the heights it drains.
//!
//! When an operation would take the tree past [`MAX_TREE_DEPTH`], the node is **suppressed**: the
//! frame is pushed and popped as usual, so starts and finishes stay balanced and the green tree
//! stays well formed, but nothing is handed to rowan and the would-be children stay where they
//! are. The result is an invariant that can be measured on the finished tree rather than argued
//! about: `tree_depth(parse(..)) <= MAX_TREE_DEPTH`, for every input. A suppressed node also sets
//! [`BoundedBuilder::overflowed`], which is what `parse_bounded` turns into a refusal — Folio never
//! converts a truncated tree, it shows the reader the source text instead.

use rowan::{GreenNode, GreenNodeBuilder, SyntaxKind, WalkEvent};

use crate::syntax::SyntaxNode;

/// The deepest syntax tree this parser will build, counting tokens as a level of their own.
///
/// The number is Folio's, and it is chosen against the stack its math worker runs on
/// (`bt_math::MATH_WORKER_STACK_BYTES`, 16 MiB) rather than against taste about formulas: see the
/// module documentation of `bt_math` for the measurement, and `bt_math`'s
/// `the_vendored_parser_limit_and_the_worker_stack_are_chosen_together` for the assertion that
/// keeps the two numbers from drifting apart.
///
/// It is also, by coincidence rather than by derivation, the same number Typst's own parser uses
/// for its `MAX_DEPTH`.
pub const MAX_TREE_DEPTH: usize = 256;

/// A parse refused because its syntax tree would be deeper than [`MAX_TREE_DEPTH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NestingTooDeep;

impl core::fmt::Display for NestingTooDeep {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "nesting too deep")
    }
}

impl std::error::Error for NestingTooDeep {}

/// A position in the tree under construction, as `rowan` understands one, plus the number of
/// finished children standing at that moment — which is what a wrap has to drain.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Checkpoint {
    inner: rowan::Checkpoint,
    children: usize,
}

/// One open node: where its children start on the flat child stack, and whether rowan was told
/// about it at all.
#[derive(Debug, Clone, Copy)]
struct Frame {
    first_child: usize,
    live: bool,
}

/// A `GreenNodeBuilder` that refuses to build past [`MAX_TREE_DEPTH`].
///
/// The interface is the subset of `GreenNodeBuilder` that `parser.rs` uses, with the same names and
/// the same argument order, so every call site in the parser reads exactly as it did upstream.
#[derive(Debug, Default)]
pub(crate) struct BoundedBuilder {
    inner: GreenNodeBuilder<'static>,
    /// The height of each finished child, parallel to rowan's own flat child stack.
    heights: Vec<u32>,
    /// One entry per open node, suppressed ones included.
    frames: Vec<Frame>,
    /// How many of `frames` rowan knows about: the real depth of the tree under construction.
    live: usize,
    overflowed: bool,
}

impl BoundedBuilder {
    /// A fresh builder.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether anything has been refused. Sticky: once a node has been suppressed the tree is no
    /// longer the one the input describes, and the whole parse is refused.
    pub(crate) fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether the next node would be suppressed — the condition the parser stops descending and
    /// stops wrapping on.
    ///
    /// Asking is also answering: a parser that stops here has declined to build a level its input
    /// asked for, and a tree missing a level its input asked for is not one anybody may convert.
    pub(crate) fn at_limit(&mut self) -> bool {
        if self.overflowed {
            return true;
        }
        if !self.admits_node() {
            self.overflowed = true;
            return true;
        }
        false
    }

    /// A node may open when its own children can still be tokens without passing the limit.
    fn admits_node(&self) -> bool {
        self.live + 2 <= MAX_TREE_DEPTH
    }

    fn refuse(&mut self, first_child: usize) {
        self.overflowed = true;
        self.frames.push(Frame {
            first_child,
            live: false,
        });
    }

    /// The height of the tallest finished child standing at or after `from`.
    fn tallest_from(&self, from: usize) -> usize {
        self.heights[from..].iter().copied().max().unwrap_or(0) as usize
    }

    pub(crate) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            inner: self.inner.checkpoint(),
            children: self.heights.len(),
        }
    }

    pub(crate) fn token(&mut self, kind: SyntaxKind, text: &str) {
        // `live` never passes `MAX_TREE_DEPTH - 1`, so a token never passes `MAX_TREE_DEPTH`.
        self.inner.token(kind, text);
        self.heights.push(1);
    }

    pub(crate) fn start_node(&mut self, kind: SyntaxKind) {
        if !self.admits_node() {
            self.refuse(self.heights.len());
            return;
        }
        self.inner.start_node(kind);
        self.frames.push(Frame {
            first_child: self.heights.len(),
            live: true,
        });
        self.live += 1;
    }

    /// Wrap everything built since `checkpoint` in a new node.
    ///
    /// **This is where depth appears without recursion.** The drained children each gain a level,
    /// so the deepest leaf under the new node would land at `live + 1 + tallest`; that, and not the
    /// open-node count, is what has to stay inside the limit.
    pub(crate) fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        let tallest = self.tallest_from(checkpoint.children);
        if !self.admits_node() || self.live + 1 + tallest > MAX_TREE_DEPTH {
            self.refuse(checkpoint.children);
            return;
        }
        self.inner.start_node_at(checkpoint.inner, kind);
        self.frames.push(Frame {
            first_child: checkpoint.children,
            live: true,
        });
        self.live += 1;
    }

    pub(crate) fn finish_node(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        if !frame.live {
            // The children this node would have had stay children of its nearest live ancestor,
            // which is exactly what rowan already has, so there is nothing to fold.
            return;
        }
        self.inner.finish_node();
        self.live -= 1;
        let height = self.tallest_from(frame.first_child) + 1;
        self.heights.truncate(frame.first_child);
        self.heights.push(height as u32);
    }

    pub(crate) fn finish(self) -> GreenNode {
        self.inner.finish()
    }
}

/// How deep a finished tree is, counted without recursing: `1` for a lone token, and one more for
/// every level of nodes above the deepest one.
///
/// **This is the check the converter's safety rests on, and it depends on nothing.** It is measured
/// on the tree that actually exists rather than on the bookkeeping that built it, and `rowan`'s
/// preorder walk is an iterator over sibling and parent links — no stack of its own beyond the two
/// counters here.
pub fn tree_depth(node: &SyntaxNode) -> usize {
    let mut depth = 0usize;
    let mut deepest = 0usize;
    for event in node.preorder_with_tokens() {
        match event {
            WalkEvent::Enter(_) => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            WalkEvent::Leave(_) => depth -= 1,
        }
    }
    deepest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, parse_bounded};
    use mitex_spec_gen::DEFAULT_SPEC;

    /// Every shape that makes this parser descend or wrap, one source per level.
    ///
    /// The first five recurse; the five after them only *wrap*, adding a level of tree with no
    /// stack frame behind it; the last three reach the same shapes through a macro, which this
    /// parser's own lexer expands before a token ever arrives here.
    type Shape = (&'static str, fn(usize) -> String);

    fn shapes() -> Vec<Shape> {
        vec![
            ("groups", |n| {
                format!("{}x{}", "{".repeat(n), "}".repeat(n))
            }),
            ("bare scripts", |n| "^".repeat(n) + "x"),
            ("brace-free commands", |n| r"\sqrt".repeat(n) + " x"),
            ("arguments in arguments", |n| {
                format!("{}x{}", r"\frac{".repeat(n), "}{y}".repeat(n))
            }),
            ("optional argument globs", |n| r"\sqrt[2]".repeat(n) + "x"),
            ("paired delimiters", |n| {
                format!("{}x{}", r"\left(".repeat(n), r"\right)".repeat(n))
            }),
            ("environments", |n| {
                format!(
                    "{}x{}",
                    r"\begin{matrix}".repeat(n),
                    r"\end{matrix}".repeat(n)
                )
            }),
            ("greedy chains", |n| r"\displaystyle x ".repeat(n)),
            ("infix chains", |n| r"x\over ".repeat(n) + "x"),
            ("scripts after a term", |n| {
                "x".to_owned() + &"^y".repeat(n)
            }),
            ("primes", |n| "x".to_owned() + &"'".repeat(n)),
            ("left-wrapping commands", |n| {
                "x".to_owned() + &r"\limits".repeat(n)
            }),
            ("a macro for a command", |n| {
                format!(r"\newcommand{{\s}}{{\sqrt}}{} x", r"\s".repeat(n))
            }),
            ("a macro for an argument taker", |n| {
                format!(r"\newcommand{{\f}}{{\frac}}{}x", r"\f a ".repeat(n))
            }),
            ("a macro body invoked many times", |n| {
                format!(
                    r"\newcommand{{\d}}{{{}}}{} x",
                    r"\sqrt".repeat(4),
                    r"\d".repeat(n.div_ceil(4))
                )
            }),
        ]
    }

    /// **The invariant everything downstream stands on**, measured on the tree that exists.
    ///
    /// No shape of input, at any depth, produces a tree deeper than the limit — so no walk over one
    /// of these trees can recurse deeper than that either, whatever it is and whoever wrote it.
    #[test]
    fn no_input_builds_a_tree_deeper_than_the_limit() {
        for (name, build) in shapes() {
            for levels in [1, 64, MAX_TREE_DEPTH, MAX_TREE_DEPTH * 4, 4096] {
                let node = parse(&build(levels), DEFAULT_SPEC.clone());
                let depth = tree_depth(&node);
                assert!(
                    depth <= MAX_TREE_DEPTH,
                    "{name} at {levels} levels built a tree {depth} deep"
                );
            }
        }
    }

    /// And a tree that had to be truncated is refused rather than handed on.
    #[test]
    fn a_truncated_tree_is_refused_and_a_whole_one_is_not() {
        for (name, build) in shapes() {
            assert!(
                parse_bounded(&build(8), DEFAULT_SPEC.clone()).is_ok(),
                "{name}: eight levels is a formula, not an attack"
            );
            assert_eq!(
                parse_bounded(&build(4096), DEFAULT_SPEC.clone()).err(),
                Some(NestingTooDeep),
                "{name}: four thousand levels is refused"
            );
        }
    }

    /// The bound is nowhere near the formulas anybody writes.
    ///
    /// That these still parse *identically* is what the crate's own hundred snapshot tests say;
    /// this says the second thing those tests cannot, which is how much room is left over.
    #[test]
    fn an_ordinary_formula_is_nowhere_near_the_limit() {
        for source in [
            r"\frac{a}{b} + \sqrt[3]{x^2} \over 2",
            r"\left( \begin{matrix} a & b \\ c & d \end{matrix} \right)",
            r"\newcommand{\norm}[1]{\left\lVert#1\right\rVert}\norm{x}",
            r"\sum\limits_{i=1}^{n} x_i'' \displaystyle \frac12+",
            r"\begin{aligned} a &= b \\ c &= d \end{aligned}",
        ] {
            let node = parse_bounded(source, DEFAULT_SPEC.clone()).expect("a formula, not a depth");
            let depth = tree_depth(&node);
            assert!(depth < 32, "{source} reached {depth} of {MAX_TREE_DEPTH}");
        }
    }
}
