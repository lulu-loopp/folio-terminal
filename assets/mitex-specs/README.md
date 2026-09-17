# MiTeX's LaTeX specification, as the product compiles it

`crates/bt-math` renders a formula by handing MiTeX's LaTeX-to-Typst output to a
Typst engine, and that output `#import`s these three files. They are compiled
into `folio.exe` with `include_str!` and resolved by name — `specs/mod.typ`,
`specs/prelude.typ`, `specs/latex/standard.typ` — so the engine never touches a
disk at run time.

**Upstream, not ours.** They are MiTeX's own `packages/mitex/specs`, copied
from <https://github.com/mitex-rs/mitex> when the M-1 formula spike chose that
converter (`docs/spikes/03-math-engine.md`), and carried across when the engine
became `bt-math`. `MITEX-LICENSE` beside them is that project's Apache-2.0 text,
and `THIRD-PARTY-NOTICES.md` carries the same licence against the `mitex` crates
the binary links.

## The one change Folio makes, and why

`latex/standard.typ` is modified, and says so at the top of the file as section
4(b) of the Apache licence asks. Upstream's `\hspace`, `\vspace` and `\raisebox`
read their argument back out of the typeset content with `get-tex-str` and hand
the resulting string to Typst's **`eval`**. In Folio a formula is text that a
program merely printed into a terminal, so that is an arbitrary Typst expression
built by whoever printed the line. It is not theoretical: measured 2026-09-17,
`x\hspace{4pt*10}x` drew exactly as wide as `x\hspace{40pt}x`, and
`x\hspace{(2pt+2pt)*10}x` drew the same again, so digits, letters, parentheses
and operators all reached `eval` — and `range(0, 100000000)` is spelled with the
same characters. The math worker is one thread and nothing can interrupt it.

A `mitex-length` function beside `get-tex-str` now *parses* the length —
`[+-]?number` followed by `pt`, `mm`, `cm`, `in` or `em` — and fails the compile
for anything else, which is what `eval` already did with `\hspace{1ex}`. Nothing
that used to draw stops drawing; `a_length_is_parsed_and_never_evaluated` in
`bt-math` holds both halves of that.

This file is not covered by `scripts/check-vendor-notices.ps1`: that gate hashes
a vendored tree against a published `.crate` archive, and these come from a git
repository rather than from crates.io. The notice in the file and this section
are the record instead.

They are files rather than a crate dependency because MiTeX ships them as Typst
sources for a Typst compiler to import, and nothing on crates.io hands them to
an embedded engine. Replacing them is a job to do against the `mitex` version in
`Cargo.lock`, currently 0.2.4: the converter and the specification are one
release, and a specification from a different one quietly loses whatever macros
that release added.
