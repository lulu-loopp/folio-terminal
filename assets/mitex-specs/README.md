# MiTeX's LaTeX specification, as the product compiles it

`crates/bt-math` renders a formula by handing MiTeX's LaTeX-to-Typst output to a
Typst engine, and that output `#import`s these three files. They are compiled
into `folio.exe` with `include_str!` and resolved by name — `specs/mod.typ`,
`specs/prelude.typ`, `specs/latex/standard.typ` — so the engine never touches a
disk at run time.

**Upstream, not ours.** They are MiTeX's own `packages/mitex/specs`, copied
from <https://github.com/mitex-rs/mitex> when the M-1 formula spike chose that
converter (`docs/spikes/03-math-engine.md`), and carried across unedited when
the engine became `bt-math`. `MITEX-LICENSE` beside them is that project's
Apache-2.0 text, and `THIRD-PARTY-NOTICES.md` carries the same licence against
the `mitex` crates the binary links.

They are files rather than a crate dependency because MiTeX ships them as Typst
sources for a Typst compiler to import, and nothing on crates.io hands them to
an embedded engine. Replacing them is a job to do against the `mitex` version in
`Cargo.lock`, currently 0.2.4: the converter and the specification are one
release, and a specification from a different one quietly loses whatever macros
that release added.
