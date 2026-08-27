# `licenses/`

Verbatim licence texts, and the prose that frames them. These are the **inputs**
to `THIRD-PARTY-NOTICES.md` at the root — not a second, competing copy of it.

```
./scripts/generate-notices.ps1   # preamble + bundled-assets + cargo about  ->  THIRD-PARTY-NOTICES.md
./scripts/check-notices.ps1      # regenerate into a temp file and demand the bytes match
```

Nothing in this directory should ever be edited except `notices-preamble.md` and
`bundled-assets.md`. The rest are other people's words, copied byte for byte;
the only correct change to one of them is replacing it wholesale when its
upstream changes.

## The two written here

| File | |
|---|---|
| `notices-preamble.md` | The head of `THIRD-PARTY-NOTICES.md`: what the file is and how it is made |
| `bundled-assets.md` | The section for everything inside the product that `Cargo.lock` cannot see — fonts, icons, colour schemes, vendored binaries, vendored source. Each `<!-- verbatim: path -->` marker is replaced at generation time by that file's exact bytes |

## The texts copied here

| File | Copied from | Covers |
|---|---|---|
| `typst-assets-NOTICE.txt` | `typst-assets` 0.15.1, `NOTICE` | Everything `typst-kit-embed-fonts` compiles in: Libertinus (OFL), NewComputerModern (GUST), NewCM10-Regular (GPL-3.0-or-later + FE + DE), DejaVu (Bitstream Vera / Arev), ICC profiles (CC0), WHATWG data (CC BY 4.0 / BSD-3), W3C data (W3C Document Licence), Foxit fonts (PDFium BSD-3). **Reproduced whole**: the W3C Document Licence in it requires the full text of the notice to travel with any redistribution, so a summary would breach one of the licences it summarises |
| `hayro-LICENSE_FOXIT.txt` | `hayro-interpret` 0.7.0, `assets/LICENSE_FOXIT` | The fourteen standard PDF fonts embedded by `hayro`'s `embed-fonts` |
| `hayro-assets-README.md` | `hayro-interpret` 0.7.0, `assets/README.md` | Upstream's own statement of where the Foxit fonts and the two ICC profiles came from |
| `hayro-CGATS_LICENSE.txt` | `hayro-interpret` 0.7.0, `assets/CGATS_LICENSE.txt` | CC0 1.0, for `CGATS001Compat-v2-micro.icc` |
| `Unicode-LICENSE-v3.txt` | `icu_collections` 2.2.0, `LICENSE` | The twenty-five Unicode-3.0 packages (ICU4X and friends) and `unicode-ident` |
| `option-ext-MPL-2.0.txt` | `option-ext` 0.2.0, `LICENSE.txt` | MPL-2.0. `bundled-assets.md` carries the source offer this licence requires, fixed to the crate archive by SHA-256 |
| `material-design-icons-LICENSE.txt` | <https://github.com/google/material-design-icons>, `LICENSE` | Apache-2.0, for the one copied icon path (the settings gear) |
| `microsoft-terminal-LICENSE.txt` | <https://github.com/microsoft/terminal>, `LICENSE` | MIT. Covers the four colour schemes taken from Windows Terminal's `defaults.json`, and the ConPTY sidecar. Kept byte-identical to `vendor/conpty/LICENSE-MICROSOFT-TERMINAL`, which `check-notices.ps1` asserts |
| `microsoft-terminal-NOTICE-v1.25.1912.0.md` | <https://github.com/microsoft/terminal>, `NOTICE.md` at tag `v1.25.1912.0` | The components `OpenConsole.exe` is statically linked against — `jsoncpp`, `{fmt}`, `stb`, `cmark`, `wil` and a dozen more. The `.nupkg` the binary is extracted from does not carry it, so it is fetched by tag and fixed by SHA-256 in `bundled-assets.md`. **CRLF, as upstream stores it** — the bytes are the point |
| `colour-schemes/iTerm2-Color-Schemes-LICENSE.txt` | <https://github.com/mbadolato/iTerm2-Color-Schemes>, `LICENSE` | MIT, for the four schemes taken from it |
| `colour-schemes/onehalf-LICENSE.txt` | <https://github.com/sonph/onehalf>, `LICENSE.txt` | MIT — One Half, upstream of Windows Terminal's transcription |
| `colour-schemes/solarized-LICENSE.txt` | <https://github.com/altercation/solarized>, `LICENSE` | MIT — Solarized, same |
| `colour-schemes/dracula-LICENSE.txt` | <https://github.com/dracula/dracula-theme>, `LICENSE` | MIT — Dracula |
| `colour-schemes/nord-LICENSE.txt` | <https://github.com/nordtheme/nord>, `license` | MIT — Nord |

**Gruvbox has no file here.** `morhetz/gruvbox` carries no `LICENSE` at its root;
its `package.json` declares `"license": "MIT"` and `"author": "Pavel Pertsev"`,
and that declaration is the whole of its licence statement. Copying in a generic
MIT text and attributing it to him would be inventing a document that does not
exist upstream, so `bundled-assets.md` quotes what does.

## Texts that live elsewhere, because they travel with what they cover

| File | Covers |
|---|---|
| `../LICENSE-MIT`, `../LICENSE-APACHE` | Folio's own code |
| `../vendor/alacritty_terminal/LICENSE-APACHE` | The vendored terminal core; `../vendor/alacritty_terminal/CHANGES-FOLIO.md` indexes the twenty-three files changed under §4(b) |
| `../vendor/conpty/LICENSE-MICROSOFT-TERMINAL` | The two ConPTY `.nupkg` files and the `conpty.dll` / `OpenConsole.exe` extracted from them |
| `../vendor/conpty/portable-pty/LICENSE.md` | The patched `portable-pty` 0.9.0 |
| `../assets/fonts/NotoColorEmoji-LICENSE` | The embedded colour emoji font (OFL-1.1) |
| `../assets/psreadline/2.4.6/License.txt` | PSReadLine 2.4.6, which is installed with this file beside it |

Every one of them is also reproduced in `THIRD-PARTY-NOTICES.md`, so a release
archive carrying only that one file is still complete.
