# PowerShell in the preview (issue #7) — what it was, and what it cost

Branch `fix/powershell-is-highlighted`. Measured in a scratch crate on syntect
5.3.0 + `regex-fancy` + `two-face` 0.4.5, release, on the Windows machine; bt-app
was not built here and CI is the gate. `two-face` ships PowerShell only in its
Oniguruma dumps and Folio loads the fancy one, so `.ps1` drew plain — over **one
pattern**.

## Rejected construct → replacement

| Before | After | Why equivalent |
|---|---|---|
| <code>\`u\\{(?:(?:10)?([0-9a-fA-F]){1,4}\|0?\g&lt;1&gt;{1,5})}</code> | <code>…\|0?[0-9a-fA-F]{1,5})}</code> | `\g<1>` is an Oniguruma **subroutine call**: it re-runs group 1's *pattern*, the class `[0-9a-fA-F]`. Writing that class out is a substitution of equals. Group 1 stays, so capture numbering is untouched, and the rule has no `captures:` map. fancy-regex's refusal: `FeatureNotYetSupported("Subroutine Call")`. |

97 patterns, one rejection, **no fidelity lost** — nothing deleted, weakened or
retuned. One non-pattern change with it: `pwsh` added to `file_extensions`,
because a fence's info string is answered by `find_syntax_by_token`, which reads
the extension list before the name. Three constructs the investigation called
Oniguruma-only are not — probed, not assumed. fancy-regex compiles possessive
quantifiers (`\s*+`, `(?:[0-9_])?+`); implements `\G` as "where this search
started", not `\A` (`\Gx` in `yxz` from byte 1 matches, `\Ax` does not); and
reads `{,6}` as `{0,6}` (`^a{,6}$` matches `aaa`, not the text `a{,6}`).

## A5

| | |
|---|---|
| Grammar source, patched / dump built from it | 22,217 B / 4,809 B |
| `two_face::extra_newlines()`, unchanged | 1.0 ms |
| **Merging PowerShell into it** (`into_builder`+`build`); the re-link alone | **305 ms** (+304); 244 ms |
| PowerShell alone: from the YAML / from the dump | 56 ms / **0.078 ms** |
| Cold set → first 40-line document: Rust / Python (baseline) / PowerShell | 21.8 / 21.4 / 58 ms |
| 2,000 lines of PowerShell (Rust, for scale) | 79 ms (69) |
| Compiling all 97 patterns; worst single (`commands`) | 62.8 ms; 9.7 |

So the ticket's "builder from the two-face set" costs 304 ms before the first
preview of **any** code file: `into_builder()` forces every grammar's contexts
out of the packed form the dump keeps them in. PowerShell gets a second
`SyntaxSet` instead, loaded lazily, with `highlight::Grammar` carrying the set a
syntax came from. H2: the cmdlet-verb alternation is 9.7 ms once, no stall.

## (a) or (b): (a), the dump, with the YAML as its source

`yaml-load` is free — `typst-library` already asks syntect for it, so `yaml-rust`
is in the resolved build and in the notices. The choice was only the **miss**
path: the vendored box is reached whenever the main dump has no answer (a `.log`,
a ` ```output ` fence), and 60 ms of YAML to say "not PowerShell" is a stall
charged to a document with nothing to do with this feature. The dump makes it
0.08 ms, so only PowerShell documents pay for PowerShell. `yaml-load` is named
in the manifest anyway (no new package), because the tests read the
`.sublime-syntax` and a feature that resolves on what another crate wanted breaks
on a day nobody touched it. `the_powershell_packdump_is_the_vendored_grammar`
rebuilds the dump from the source on every CI run, on three platforms, and
compares the **grammars** — byte equality would add a bet on two compressors on
three architectures. `regenerate_the_powershell_packdump` is the `#[ignore]`d
writer, on the allowlist and named in that failure message.

## Wrong in Observed, and unverified

Wrong: "the workspace's syntect has no `yaml-load`" — the manifest does not name
it, the resolved build has it; "the grammar's regexes do not compile" — one does
not; bat's copy is a checked-in `.sublime-syntax`, not the submodule, which is
Microsoft's `.tmLanguage` and unreadable to syntect. Right as stated: two-face
0.5.x still excludes PowerShell from fancy.

Unverified: bt-app was not compiled. The boxes, the lookup, the dump, the drift
check and A1/A3 ran verbatim in the scratch crate, and A2's expected spans came
from the product's `SCOPE_TABLE` over the same script — but the tests as written
have not been through `cargo test` or clippy, and the dump has not been read on
macOS or Linux (same pure Rust; the drift test is what would say otherwise).
Deliberate: a `pwsh` shebang on an extension-less file resolves to nothing.
