# Folio's changes to `mitex-parser` 0.2.4

`vendor/mitex-parser/` is the crates.io archive of `mitex-parser` 0.2.4
(<https://github.com/mitex-rs/mitex>, Apache License, Version 2.0) with changes
by the Folio contributors. Section 4(b) of that licence asks every modified file
to carry a prominent notice saying so; every file listed below does, in its first
few lines, and `scripts/check-vendor-notices.ps1` is what keeps "differs from
upstream" and "carries the notice" the same set of files.

Upstream ships no `NOTICE` file with this crate — the published archive carries
no licence file at all, only the `license = "Apache-2.0"` line in its manifest —
so there is no attribution notice to propagate under section 4(d). The Apache-2.0
text is reproduced in `THIRD-PARTY-NOTICES.md`.

## Why this crate is vendored at all

`Parser` in `src/parser.rs` is recursive descent, and in Folio it runs over text
that a program merely *printed into a terminal*: `cat evil.md` is enough. A stack
overflow is not a panic — `catch_unwind` cannot contain one — so a formula that
drives this parser deep enough ends the whole process, every shell in every pane
with it.

The depth cannot be *predicted* from the token stream before the parser runs. The
2026-09-17 review disproved a prediction that tried: `\over`, `\displaystyle`,
`\limits`, `'` and `\sqrt[2]` each create a level in ways no table of arities
describes, and a macro's expansion is what the parser actually sees rather than
the name that was written. So the bound is *enforced*, inside the implementation,
at the point where a level would be created.

## `src/depth.rs` — added

The bound, and the bookkeeping that makes it exact.

`BoundedBuilder` is a `rowan::GreenNodeBuilder` with the same method names and
argument order, so every call site in `parser.rs` reads as it did. It mirrors
rowan's own flat child stack, one height per finished child, so it knows the
height of the tree at every moment — including after `start_node_at`, which
*wraps* already-built syntax and adds a level with no stack frame behind it. When
an operation would take the tree past `MAX_TREE_DEPTH` the node is **suppressed**:
the frame is pushed and popped as usual, so starts and finishes stay balanced and
the green tree stays well formed, but rowan is not told about it. The invariant is
therefore measurable rather than arguable, and `no_input_builds_a_tree_deeper_than_the_limit`
measures it for fifteen shapes of input at five depths each.

`tree_depth` is the independent half: an iterative preorder walk over a finished
tree that believes none of the bookkeeping above.

## `src/parser.rs` — one gate, in `content`

Every cycle in this file's call graph passes through `Parser::content`, and every
one of those cycles opens a syntax node before it recurses. So a cap on the tree's
depth is also a cap on the parser's recursion, and one test at the top of
`content` is the whole enforcement: at the bound the token is consumed, nothing
descends and nothing wraps, and the parse is marked as refused. The cycles are

- `content` → `item_group` → `item_list` → `content`
- `content` → `command` → `match_arguments`/`match_arguments_` → `content`
- `content` → `command` → `match_arguments_` → `item_group` → `item_list` → `content`
- `content` → `environment` → `item_list` → `content` (and → `match_arguments_` → `content`)
- `content` → `item_lr` → `item_list` → `content`
- `content` → `attach_component` → `content`

`eat_body_of_ifs`, `text`, `clause_lr` and `single_char` iterate and do not
recurse; the lexer and its macro engine expand macros by pushing tokens back into
a buffer, not by calling themselves.

Also changed there: `builder` is a `BoundedBuilder`, `list_state` holds the
builder's checkpoint type rather than rowan's, and `Parser::parse` returns whether
anything was refused alongside the tree.

## `src/lib.rs` — `parse_bounded`, and the crate's lint level

`parse` and `parse_without_macro` keep their signatures and upstream's tests keep
calling them; the bound is unconditional, so those entry points are safe too —
they simply return a tree truncated at the limit. `parse_bounded` and
`parse_without_macro_bounded` are the entry points that *say so*, refusing with
`NestingTooDeep` when either the parser declined to build a level or the finished
tree measures deeper than `MAX_TREE_DEPTH`. Folio converts neither.

The crate root also allows `clippy::doc_lazy_continuation` and
`clippy::unnecessary_map_or`, which a clippy newer than this crate raises against
files nothing here has touched. An allow in the one file already modified is what
keeps the rest byte-identical to the published archive.

## The manifest

The `divan` benchmark target is dropped (`[[bench]]` removed, `autobenches =
false`, the `divan` dev-dependency removed), so a benchmark harness is not added
to this workspace's lock file for a target no gate runs. `benches/simple.rs`
stays on disk, unmodified, because the notices gate compares file sets.
