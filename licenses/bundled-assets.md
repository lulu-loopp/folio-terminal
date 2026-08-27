## Bundled components that are not crates

`cargo about` reads `Cargo.lock`. Everything below is inside the binary, or inside
this repository, without appearing there as a package of its own: fonts and data
embedded by a crate rather than published as one, binaries vendored from a
release asset, a patched vendored crate, an icon path copied by hand, and a set
of colour scheme files. Each entry says what it is, where it came from, and
carries the licence text it travels under, verbatim.

Two crates that *are* in `Cargo.lock` also appear here, because their terms ask
for more than a licence text: `option-ext` (MPL-2.0) needs a source offer, and
the twenty-five Unicode-3.0 packages need their notice reproduced rather than
counted.

---

### `alacritty_terminal` 0.26.0, vendored and modified

<https://github.com/alacritty/alacritty> — Apache License, Version 2.0.

`vendor/alacritty_terminal/` is the crates.io 0.26.0 archive with changes by the
Folio contributors. Twenty-three of its 211 files differ; each of those carries a
notice at the top of the file, as section 4(b) requires, and
`vendor/alacritty_terminal/CHANGES-FOLIO.md` indexes them. Twenty of the
twenty-three differ only because this workspace's `rustfmt` settings are not
upstream's — provably so: `rustfmt --edition 2024` over the upstream file
reproduces the vendored file byte for byte.

Upstream ships **no `NOTICE` file** with this crate — the published archive
contains `LICENSE-APACHE` and nothing else of that kind — so there is no
attribution notice to propagate under section 4(d).

The Apache-2.0 text is reproduced in this document under the crate listing, and
in the repository at `vendor/alacritty_terminal/LICENSE-APACHE`.

---

### Microsoft ConPTY sidecar — `conpty.dll` and `OpenConsole.exe`

<https://github.com/microsoft/terminal> — MIT.

A release archive ships `conpty.dll` and `OpenConsole.exe` beside `folio.exe`.
They are extracted, hash-verified, from the official NuGet package
`Microsoft.Windows.Console.ConPTY.1.25.260710002-preview.nupkg`, which is
vendored at `vendor/conpty/` together with its predecessor `1.24.260512001`,
kept as a reproducible A/B archive. Neither `.nupkg` carries a `LICENSE` or
`NOTICE` entry of its own — the only metadata entry is the `.nuspec`, which
declares MIT — so the upstream text is carried beside them, and here:

<!-- verbatim: vendor/conpty/LICENSE-MICROSOFT-TERMINAL -->

---

### `portable-pty` 0.9.0, vendored and patched

<https://github.com/wezterm/wezterm> — MIT, Copyright (c) 2018 Wez Furlong.

`vendor/conpty/portable-pty/` is `portable-pty` 0.9.0 with a Windows loader
patch described in `vendor/conpty/README.md`. Its licence travels with it at
`vendor/conpty/portable-pty/LICENSE.md`:

<!-- verbatim: vendor/conpty/portable-pty/LICENSE.md -->

---

### Noto Color Emoji

<https://github.com/googlefonts/noto-emoji> — SIL Open Font License 1.1.

`assets/fonts/NotoColorEmoji_WindowsCompatible.ttf` is compiled into the
executable (`crates/bt-render/src/lib.rs`, `include_bytes!`). It is the CBDT/CBLC
bitmap build of Noto Color Emoji; the font's own `name` table records
`Copyright 2022 Google Inc.` and version
`2.051;GOOG;noto-emoji:20250818:e92753bfa55fd449e427d4d325f9c8c40408c74e`. The
file was renamed for Windows compatibility, which the licence permits: the
upstream copyright notice declares **no** Reserved Font Name.

<!-- verbatim: assets/fonts/NotoColorEmoji-LICENSE -->

---

### PSReadLine 2.4.6

<https://github.com/PowerShell/PSReadLine> — BSD 2-Clause,
Copyright (c) 2013, Jason Shirk.

Nine files, including two `.dll`s, are embedded in the executable and written to
`Documents\WindowsPowerShell\Modules\PSReadLine` when the user asks for the
PowerShell integration. `License.txt` is one of the nine — the module is
installed with its licence beside it, not without.

<!-- verbatim: assets/psreadline/2.4.6/License.txt -->

---

### One Material Design icon

<https://github.com/google/material-design-icons> — Apache License, Version 2.0.

The settings gear is the Material Design `settings` icon at 24dp, copied path
data and all. It is `SYMBOL_BODY[0]` in `crates/bt-app/src/marks.rs` (`#i-gear`)
and the sole `viewBox="0 0 24 24"` in the tree; every other mark in this product
is drawn here, on a 16- or 10-unit box. **It has been modified**: it is drawn at
this product's own sizes and inherits `currentColor` rather than carrying a fill
of its own. The path geometry is unchanged.

<!-- verbatim: licenses/material-design-icons-LICENSE.txt -->

---

### Ten colour schemes

`assets/schemes/*.json`. Two of the ten — `folio-dark.json` and
`folio-light.json` — are this product's own. The other eight are transcriptions,
byte-exact from the sources named in `assets/schemes/README.md`, of themes that
travel under MIT:

| Scheme files | Taken from | Originally by |
|---|---|---|
| `one-half-dark`, `one-half-light`, `solarized-dark`, `solarized-light` | `microsoft/terminal`, `src/cascadia/TerminalSettingsModel/defaults.json` (MIT) | One Half: Son A. Pham, `sonph/onehalf` (MIT). Solarized: Ethan Schoonover, `altercation/solarized` (MIT) |
| `gruvbox-dark`, `gruvbox-light`, `dracula`, `nord` | `mbadolato/iTerm2-Color-Schemes`, `windowsterminal/*.json` (MIT) | Gruvbox: Pavel Pertsev, `morhetz/gruvbox`. Dracula: Zeno Rocha, `dracula/dracula-theme` (MIT). Nord: Sven Greb, `nordtheme/nord` (MIT) |

`mbadolato/iTerm2-Color-Schemes`'s own licence says, verbatim: "This license
covers the iTerm-Color-Schemes repository collection of themes. The
copyright/license for each individual theme belongs to the author of that
theme." — which is why the second column is here and not only the first.

`morhetz/gruvbox` carries no `LICENSE` file; its `package.json` declares
`"license": "MIT"` and `"author": "Pavel Pertsev"`. That declaration is the whole
of its licence statement, and is reproduced here rather than invented into a
text that does not exist upstream.

#### `microsoft/terminal`

<!-- verbatim: licenses/microsoft-terminal-LICENSE.txt -->

#### `mbadolato/iTerm2-Color-Schemes`

<!-- verbatim: licenses/colour-schemes/iTerm2-Color-Schemes-LICENSE.txt -->

#### `sonph/onehalf`

<!-- verbatim: licenses/colour-schemes/onehalf-LICENSE.txt -->

#### `altercation/solarized`

<!-- verbatim: licenses/colour-schemes/solarized-LICENSE.txt -->

#### `dracula/dracula-theme`

<!-- verbatim: licenses/colour-schemes/dracula-LICENSE.txt -->

#### `nordtheme/nord`

<!-- verbatim: licenses/colour-schemes/nord-LICENSE.txt -->

---

### Assets embedded by `typst-assets` 0.15.1

The `typst-as-lib` dependency is taken with `typst-kit-fonts` and
`typst-kit-embed-fonts`, which compiles `typst-assets`' fonts and data tables
into the executable. The crate itself is Apache-2.0 and appears in the crate
listing below; the *assets* are not, and travel under six further sets of terms.

Upstream states them in one `NOTICE` file, and that file is reproduced here
**whole and unedited**. That is not thoroughness for its own sake: the W3C
Document Licence in it requires "the full text of this NOTICE in a location
viewable to users of the redistributed or derivative work", so a summary would
be a breach of one of the licences it summarises. For orientation only — the
file below is the authority:

- SIL Open Font License 1.1 — Libertinus Serif, with Reserved Font Names
  "Linux Libertine", "Biolinum", "STIX Fonts".
- GUST Font License 1.0 — the NewComputerModern fonts **other than**
  NewCM10-Regular.
- Bitstream Vera and Arev licences — DejaVu Sans Mono.
- CC0 1.0 — the bundled ICC profiles.
- CC BY 4.0 and BSD 3-Clause — data derived from the WHATWG HTML and Fetch
  specifications.
- W3C Document Licence — data derived from WAI-ARIA 1.1, Referrer Policy, and
  MathML Core.
- GPL-3.0-or-later with Font Exception and Distribution Exception —
  **NewCM10-Regular** specifically.
- PDFium BSD 3-Clause — the Foxit fonts.

<!-- verbatim: licenses/typst-assets-NOTICE.txt -->

---

### Assets embedded by `hayro-interpret` 0.7.0

The PDF preview is `hayro`, taken with `embed-fonts`, which compiles two kinds of
asset into the executable.

**Fourteen standard PDF fonts** (`Foxit*.pfb`), extracted from PDFium:

<!-- verbatim: licenses/hayro-LICENSE_FOXIT.txt -->

**Two ICC profiles.** `CGATS001Compat-v2-micro.icc` is from
<https://github.com/saucecontrol/Compact-ICC-Profiles> under CC0 1.0;
`LAB.icc` was generated with LCMS2. Upstream states both in its assets README:

<!-- verbatim: licenses/hayro-assets-README.md -->

The CC0 1.0 text that came with the CGATS profile:

<!-- verbatim: licenses/hayro-CGATS_LICENSE.txt -->

---

### `option-ext` 0.2.0 — MPL-2.0, and where to get its source

<https://github.com/soc/option-ext> — Mozilla Public License 2.0, reached
through `dirs` → `dirs-sys`.

The MPL is a file-level copyleft: linking it into this executable does not place
this executable under the MPL, but section 3.2 obliges anyone distributing the
executable to make the Source Code Form of the covered files available, and to
tell recipients how to get it. This is that notice.

The Source Code Form is the **unmodified** published crate archive, fixed here by
hash rather than by a pointer at a page that can change:

| | |
|---|---|
| Archive | `option-ext-0.2.0.crate` |
| URL | <https://static.crates.io/crates/option-ext/option-ext-0.2.0.crate> |
| SHA-256 | `04744f49eae99ab78e0d5c0b603ab218f515ea8cfe5a456d7629ad883a3b6e7d` |
| Size | 7,345 bytes |
| Also at | <https://crates.io/api/v1/crates/option-ext/0.2.0/download> |

That SHA-256 is the same value `Cargo.lock` records for the package, so the
archive a recipient downloads is verifiably the one this binary was built from.
No file of it was modified.

<!-- verbatim: licenses/option-ext-MPL-2.0.txt -->

---

### Unicode-3.0 — twenty-five packages

The ICU4X family — `icu_collator`, `icu_collections`, `icu_locale*`,
`icu_normalizer*`, `icu_properties*`, `icu_provider*`, `icu_segmenter*`, `yoke`,
`zerovec`, `zerofrom`, `zerotrie`, `tinystr`, `litemap`, `writeable`,
`potential_utf` — is licensed under the Unicode License v3, which asks that the
copyright and permission notice appear with all copies of the Data Files or
Software, or in the accompanying documentation. `unicode-ident` is
`(MIT OR Apache-2.0) AND Unicode-3.0` and is covered by the same notice.

<!-- verbatim: licenses/Unicode-LICENSE-v3.txt -->
