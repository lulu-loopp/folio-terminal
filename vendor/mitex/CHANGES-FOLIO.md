# Folio's changes to `mitex` 0.2.4

`vendor/mitex/` is the crates.io archive of `mitex` 0.2.4
(<https://github.com/mitex-rs/mitex>, Apache License, Version 2.0) with changes
by the Folio contributors. Section 4(b) of that licence asks every modified file
to carry a prominent notice saying so; every file listed below does, in its first
few lines, and `scripts/check-vendor-notices.ps1` keeps "differs from upstream"
and "carries the notice" the same set of files.

Upstream ships no `NOTICE` file with this crate — the published archive carries
no licence file at all, only the `license = "Apache-2.0"` line in its manifest —
so there is no attribution notice to propagate under section 4(d). The Apache-2.0
text is reproduced in `THIRD-PARTY-NOTICES.md`.

## Why this crate is vendored at all

`Converter::convert` in `src/converter.rs` walks the syntax tree by recursing on
children, so its stack depth *is* the tree's depth. In Folio that tree comes from
text a program printed into a terminal, and a stack overflow is not a panic: no
`catch_unwind` contains one, and the process dies with every shell in every pane
in it. See `vendor/mitex-parser/CHANGES-FOLIO.md` for the other half of the same
defect.

## `src/converter.rs` — a depth counter, and a refusal with a name

`Converter` carries a `depth`, and `convert` is now a wrapper that counts one
level and refuses past `mitex_parser::MAX_TREE_DEPTH` *before* descending; the
body it used to be is `convert_element`, so every recursive `self.convert(…)`
call site in the file is unchanged and now passes through the counter.

This is a belt rather than a buckle: the parser already refuses to build a tree
deeper than that limit, so on a tree from `parse_bounded` the counter cannot fire.
It is kept because it costs one addition per node and answers for a tree that
arrived some other way.

`ConvertError` gains a `NestingTooDeep` variant, and `convert_inner_bounded` and
`BoundedConvertError` carry that refusal out under a name a caller can act on —
Folio shows the reader the source text, which is what it does for any formula it
cannot draw. `convert_inner` is unchanged in behaviour and still returns
`Result<String, String>`.

## `src/lib.rs` — the bounded entry points

`convert_math_bounded`, `convert_text_bounded` and `convert_math_no_macro_bounded`
are `convert_math`, `convert_text` and `convert_math_no_macro` over
`mitex_parser::parse_bounded`. The originals keep their signatures and upstream's
tests keep calling them.

## `src/converter.rs` — a math-content-only mode

Three sites in this converter copy source text into the output without mapping it
token by token, and each lands where Typst reads code:

| site | what it writes | why it is code |
| --- | --- | --- |
| `ItemTypstCode` | the body of `\iftypst … \fi`, verbatim | it *is* the Typst source |
| `convert_command_includegraphics` | `#image("<path>")` | the path goes inside a string literal, so a `"` in it closes the string |
| `convert_command_label` and `convert_env`'s label | `<name>` in markup | markup after the closing `>` is markup, where `#` is code |

Everywhere else a source character reaches the output only through the token map
in `convert_element`, which escapes `#`, `"`, `^`, `_`, `*`, `@`, `;`, `,`, `/`,
`(`, `)`, `[` and `]`, drops `$`, `{`, `}` and comments, and otherwise emits a
`Word` — whose lexer class excludes every one of those characters. So refusing
those three is what makes "the converted source is math content" a statement
rather than a hope.

`Converter` therefore carries a `math_content_only` flag, set only by
`convert_inner_bounded`, and those three sites answer `ConvertError::RawTypstCode`
when it is set. Upstream's entry points are unchanged and upstream's sixty-two
conversion tests still pass unmodified, `\iftypst` snapshots included.

`ConvertError` and `BoundedConvertError` gain a `RawTypstCode` variant, carried
out under a name for the same reason the depth refusal is: the reader is owed the
source text, not a diagnostic about their own formula.

## The manifest

The `divan` benchmark target is dropped (`[[bench]]` removed, `autobenches =
false`, the `divan` dev-dependency removed), so a benchmark harness is not added
to this workspace's lock file for a target no gate runs. `benches/` stays on
disk, unmodified, because the notices gate compares file sets.
