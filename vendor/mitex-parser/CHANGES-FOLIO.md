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
the name that was written. So the bound has to be *enforced*, inside the
implementation, at the point where a level would be created — which is what this
copy exists to make possible.

## `src/lib.rs` — the crate's lint level

The crate root allows `clippy::doc_lazy_continuation` and
`clippy::unnecessary_map_or`, which a clippy newer than this crate raises against
files nothing here has touched. An allow in one file is what keeps the rest
byte-identical to the published archive, which is the thing the notices gate
reads to tell a change from a reformatting.

## The manifest

The `divan` benchmark target is dropped (`[[bench]]` removed, `autobenches =
false`, the `divan` dev-dependency removed), so a benchmark harness is not added
to this workspace's lock file for a target no gate runs. `benches/simple.rs`
stays on disk, unmodified, because the notices gate compares file sets.
