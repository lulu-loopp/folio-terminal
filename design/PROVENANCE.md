# Where the files in `design/` came from

28 tracked files. Each is one of three things — **own** (drawn or written here),
**upstream** (someone else's, under someone else's licence), or **generated** (a
picture of something else in this directory). This file says which, for every
one of them, so that "we listed the directory" is never mistaken for "we checked
the provenance".

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
anyone else's software. All 21 carry no `tEXt` / `iTXt` / `zTXt` chunk and no
producer string: they are clean rasters.

| Files | Generated from | For |
|---|---|---|
| `assets/file-icons-r2/options.png` | `assets/file-icons-r2/options.html`, rendered in a browser | The r2 icon comparison sheet |
| `assets/file-icons-r2/detail-v1.png` … `detail-v4.png` | the same sheet, cropped | The four stamping options, examined at 15×15 px (`docs/DESIGN.md` §on icon rasterisation) |
| `assets/file-icons-r3/*.png` (11) | `ui-mockup.html`, screenshotted with the temporary `?icons=a\|b\|c` reader | The r3 round, three variants held side by side in column, window, git-page and preview-head contexts |
| `assets/wordmark-r2/options.png` | `assets/wordmark-r2/options.html`, rendered in a browser | The wordmark comparison sheet |
| `assets/wordmark-r2/detail-a.png` … `detail-d.png` | the same sheet, cropped (all four 906×192) | The four wordmark candidates |

`assets/file-icons-r2/options.html` and `assets/wordmark-r2/options.html` are
**own**: hand-authored comparison sheets with the design commentary in them.
`assets/wordmark-r2/DECISION.md` is **own**: the written ruling that closed the
wordmark round.

## Nothing here is undetermined

Every tracked file was accounted for by reading it, by inspecting PNG chunks and
PDF metadata where they exist, and by `git log --follow`. If a file is added to
this directory, it belongs in this table before it belongs in a commit.
