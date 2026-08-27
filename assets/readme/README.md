# The README's own pictures

What `README.md` and `README.zh-CN.md` show that is not a screenshot. Both
languages point at the same files; each is a light/dark pair chosen by
`<picture>` from the reader's GitHub theme.

| File | What it is |
| --- | --- |
| `hero-light.svg`, `hero-dark.svg` | The title: the name, the two lines of promise, and a terminal pane where a command has printed a file and the display formula inside it stands typeset in the output. Hand-editable; no fonts are loaded, only asked for. |
| `surfaces-light.png`, `surfaces-dark.png` | Cards, the markdown preview and the web preview, framed together. These three are the surfaces neither README pictures anywhere else. |

## How the formula in the hero is drawn

The panel is labelled `TYPESET IN PLACE` and shows the one thing the README opens
on: a command printed a file, and the `$$…$$` in it came out set rather than
spelled. The equation is the Gaussian integral.

It is drawn the way the rest of this file is drawn — no bitmap, no linked font,
nothing fetched:

* **The letters, digits and symbols are `<text>`**, on a maths-serif fallback
  stack (`Latin Modern Math`, `Cambria Math`, `STIX Two Math`, Cambria, Times New
  Roman, Georgia, serif). Every character in it is either ASCII or one of three
  that are in effectively every font shipped with an operating system: `∞`,
  `−` and `π`. Nothing rare is asked for, so nothing has to be embedded.
* **The two shapes a font cannot be trusted to place are `<path>` strokes** — the
  integral sign and the radical with its overbar. A `∫` glyph is sized by
  whatever font answers and a `√` glyph's bar does not stretch over its
  radicand, so both are stroked here instead and are identical on every machine.
* **The limits, the exponent and the radicand are positioned by hand**, in a
  `<g>` with its own origin at the equation's baseline. To move the whole
  equation, move that one `translate`.

Both files carry the same geometry; only the six colour tokens differ, so
`hero-dark.svg` can be regenerated from `hero-light.svg` by substituting them.
There is no separate source for the hero — these two files are the source, which
is what "hand-editable" in the table above is protecting. `src/` holds the boards
only.

Check a change by rendering it, not by reading it: serve this directory and open
the file in a real browser, or `chrome --headless --screenshot --window-size=1200,316`.

## The hero's words, and why there is only one of them

The two lines under the name are the promise both READMEs are written to, so they
change when the positioning does and not otherwise. They currently read:

> A Windows terminal built for coding agents.
> It says which one is waiting for you.

`README.zh-CN.md` shows the same English hero rather than a Chinese one. The
title block is SVG text at fixed coordinates, and a Chinese line would be set in
whatever CJK face the reader's machine happens to resolve — PingFang SC on macOS,
Microsoft YaHei on Windows, something else again on Linux — at metrics none of
this file can predict. It would either overrun the 592-pixel title column or have
to be converted to outlines, which would end the "hand-editable" property in the
row above. The name is Latin in both languages and the Chinese one-line summary
is the first line of prose under the image, so nothing is lost by leaving the
picture in one language.

The colours are the design's own tokens — `--page`, `--ink`, `--termbg`,
`--border`, `--err`, `--cursor` from `design/ui-mockup.html`, composited the way
`crates/bt-render/src/theme.rs` composites them, so a light board and a light
window are the same paper.

## Remaking the boards

`src/surfaces-light.svg` and `src/surfaces-dark.svg` are what the PNGs are
rendered from. Each tile is a nested `<svg>` whose `viewBox` crops a committed
screenshot in `docs/screenshots/`; no pixels are redrawn, only framed and
labelled. To remake a board, edit the layout, open it in a browser through an
`<object>` sized `3200 × 3240` and save that box as the PNG.

**Both boards are `3200 × 3240`**, twice over the `1600 × 1620` they used to be,
because the screenshots behind them are now `3200 × 2000` — 1600 × 1000 logical
at 200%, as `docs/screenshots/README.md` sets out. Nothing in the SVGs changed
when the shots doubled: the layout is in the canvas's own units and the tiles
simply have twice the pixels to draw with, so raising the size the board is
rasterized at is the whole of the work. Run `oxipng -o 4 --strip safe` over the
result, as the screenshots get.

The SVGs reference the screenshots relatively, so they only render from inside
this checkout. That is why the published asset is the PNG.
