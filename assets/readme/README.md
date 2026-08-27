# The README's own pictures

What `README.md` and `README.zh-CN.md` show that is not a screenshot. Both
languages point at the same files; each is a light/dark pair chosen by
`<picture>` from the reader's GitHub theme.

| File | What it is |
| --- | --- |
| `hero-light.svg`, `hero-dark.svg` | The title: the name, what the program is, and a terminal pane where two commands are marked in the left margin — the second one as having failed. Hand-editable; no fonts are loaded, only asked for. |
| `surfaces-light.png`, `surfaces-dark.png` | Cards, the markdown preview and the web preview, framed together. These three are the surfaces neither README pictures anywhere else. |

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
