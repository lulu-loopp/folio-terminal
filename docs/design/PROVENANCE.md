# Where the design files came from

45 tracked files in `docs/design/`, plus the five in `assets/app-icon/` that the
build reads — the mark and the scripts that draw it left this directory when the
tree was rearranged, and their provenance is the same question, so it is answered
here rather than in a second document. Each file is one of three things — **own**
(drawn or written here), **upstream** (someone else's, under someone else's
licence), or **generated** (a picture of something else). This file says which,
for every one of them, so that "we listed the directory" is never mistaken for
"we checked the provenance".

Anything upstream that ends up **inside the product** is also in the repository's
`THIRD-PARTY-NOTICES.md`; this file is about the directory, not the binary.

## The four prototypes

| File | | |
|---|---|---|
| `ui-mockup.html` | **own**, with one upstream fragment — see below | The interactive master. `crates/bt-app/src/marks.rs` takes the chrome's marks from it, and every symbol in the product is drawn here except the one named below. |
| `gesture-hint-mockup.html` | **own** | Prototype for the gesture hint card. Its own header declares zero external resources; palette, type and geometry are copied from `ui-mockup.html`, timings from `crates/bt-render/src/motion.rs`. |
| `files-miller-demo.html` | **own** | A Miller-columns file browser explored and then rejected (`docs/PROBLEM-LIST.md`). Kept as the record of a road not taken. |
| `sidebar-focus-demo.html` | **own** | Prototype for focus mode, `docs/DESIGN.md` §7.1.6b. |

### The one upstream fragment: the settings gear

`ui-mockup.html` contains `<symbol id="i-gear" viewBox="0 0 24 24">`, whose path
data is Google's Material Design **`settings`** icon at 24dp, copied verbatim.
It is the only `viewBox="0 0 24 24"` in the entire repository — every other mark
is drawn here, on a 16- or 10-unit box — and the same path is compiled into the
product as `SYMBOL_BODY[0]` in `crates/bt-app/src/marks.rs`.

- Upstream: <https://github.com/google/material-design-icons>
- Licence: **Apache License, Version 2.0**
- Modified: yes — drawn at this product's sizes and inheriting `currentColor`
  instead of carrying a fill of its own. The geometry is unchanged.
- Notice: `THIRD-PARTY-NOTICES.md`, "One Material Design icon", which carries the
  Apache-2.0 text and this modification statement.

### Two prototypes load from a CDN when opened

`ui-mockup.html` and `files-miller-demo.html` `<link>` Google Fonts (Inter,
JetBrains Mono), and `ui-mockup.html` also loads KaTeX 0.16.11 from jsDelivr.
Nothing is vendored: opening these files in a browser fetches them, and opening
them offline degrades to system fonts. No third-party code is committed here on
their behalf, and the shipped product loads neither — it has no network path of
its own at all.

The `github.com` / `microsoft.com` / `jetbrains.com` URLs elsewhere in
`ui-mockup.html` are citations inside a design-rationale comment (prior art on
icon crowding), not copied code.

## The pictures

Every PNG here is **generated** — a screenshot or a crop of a page in this same
directory, taken to make a decision and kept as the evidence for it. They are
this project's own work; none is a photograph, a stock image, or a screenshot of
anyone else's software. All 22 carry no `tEXt` / `iTXt` / `zTXt` chunk and no
producer string: they are clean rasters.

| Files | Generated from | For |
|---|---|---|
| `file-icons-r2/options.png` | `file-icons-r2/options.html`, rendered in a browser | The r2 icon comparison sheet |
| `file-icons-r2/detail-v1.png` … `detail-v4.png` | the same sheet, cropped | The four stamping options, examined at 15×15 px (`docs/DESIGN.md` §on icon rasterisation) |
| `file-icons-r3/*.png` (11) | `ui-mockup.html`, screenshotted with the temporary `?icons=a\|b\|c` reader | The r3 round, three variants held side by side in column, window, git-page and preview-head contexts |
| `wordmark-r2/options.png` | `wordmark-r2/options.html`, rendered in a browser | The wordmark comparison sheet |
| `wordmark-r2/detail-a.png` … `detail-d.png` | the same sheet, cropped (all four 906×192) | The four wordmark candidates |
| `app-icon/candidates-2026-08-28.png` | `app-icon/candidates/{a..e}.svg`, rasterised by `app-icon/make-candidates-board.py` | The application-icon round: five directions at 256/48/32/16, on a light and a dark taskbar. Closed 2026-09-06 with the shipped drawing kept. |

`file-icons-r2/options.html` and `wordmark-r2/options.html` are
**own**: hand-authored comparison sheets with the design commentary in them.
`wordmark-r2/DECISION.md` is **own**: the written ruling that closed the
wordmark round.

## The application icon

The shipped mark, the second rendering of it that the macOS bundle reads, and
the two scripts that draw them live in `assets/app-icon/`, which is the
directory the build reads; the round that was run before it was
kept is `app-icon/` here.

| File | | |
|---|---|---|
| `assets/app-icon/folio.ico` | **generated** | **The icon in the binary** (user ruling, 2026-09-06 — kept over the five candidates below), written by the script beside it out of geometry it holds itself. No input file, no traced artwork. `crates/bt-app/build.rs` links it into `folio.exe`, and `crates/bt-app/src/first_run.rs` embeds the same file for the first-run card's header. |
| `assets/app-icon/folio-1024.png` | **generated** | **The icon source the macOS bundle is built from**, written by the script above out of the same geometry, at a size no `.ico` entry reaches: `python assets/app-icon/make-folio-ico.py --png`. Not an upscale of anything and not a second drawing — the generator states the mark in units of the square and this is that statement resolved at 1024. `scripts/release/macos/bundle.sh` resamples every slot of `Folio.icns` from it, 1024 being `icon_512x512@2x`, the largest slot an icon set has. |
| `assets/app-icon/make-folio-ico.py` | **own** | Draws both of the files above, and is the source of record for the mark. Standard library only. |
| `assets/app-icon/make-msix-logos.py` | **own** | Draws the three PNGs `packaging/msix/AppxManifest.xml` names, at the sizes it names, by importing the script above. It owns no geometry of its own. Standard library only. |
| `app-icon/candidates/{a..e}.svg` | **own** | **Retired 2026-09-06**, none chosen. Five directions for a replacement mark, hand-set in plain SVG. Nothing traced, no font outlines converted, no external resources — the `∫` in `b.svg` is stroked geometry and not a glyph. |
| `app-icon/candidates/{a..e}.ico` | **generated** | **Retired 2026-09-06.** Each of those five at nine sizes, written by `make-ico.py` from the SVG beside it. |
| `app-icon/make-ico.py`, `app-icon/make-candidates-board.py` | **own** | The two build scripts: one SVG to one `.ico`, and the five SVGs to the contact sheet. Both need Pillow and a Chromium-family browser at run time; neither vendors anything. |
| `assets/app-icon/README.md`, `app-icon/README.md` | **own** | The shipped mark and how it reaches the binary; and the written brief for the round, the ruling that closed it, and what each candidate cost at 16 pixels. |

### The macOS bundle's icon is the same drawing, resampled

`Folio.app/Contents/Resources/Folio.icns` is **generated**, and it is not a
second drawing: `scripts/release/macos/bundle.sh` builds it at bundle time from
`assets/app-icon/folio.ico` with `sips` and `iconutil`, both of which are in
`/usr/bin` on a stock Mac. It is not tracked, because it is a function of a file
that is.

Two consequences of the `.ico` container, recorded here rather than left to be
rediscovered in the result: `sips` addresses only that file's **largest** entry,
so the small slots of the icon set are resampled from the 256 rather than taken
from the entries hand-drawn at 16, 20, 24, 32, 40, 48 and 64; and the source has
no pixels above 256, so the three slots above it are left absent rather than
filled with an upscale, and macOS scales the 256 for every reader instead. Both
are the icon source's to fix, not the packaging's.

## The written spec

| File | | |
|---|---|---|
| `UI-SPEC.md` | **own** | The current UI's rules, read off the shipped code's constants (2026-09-22). Written here, from the code; it copies nothing. |
| `UI-DEVIATIONS.md` | **own** | Its companion: every constant that does not follow those rules yet, ranked, with the ticket groups. |

## Nothing here is undetermined

Every tracked file was accounted for by reading it, by inspecting PNG chunks and
PDF metadata where they exist, and by `git log --follow`. If a file is added to
this directory or to `assets/app-icon/`, it belongs in this table before it
belongs in a commit.
