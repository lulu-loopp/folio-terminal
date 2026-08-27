# The README's own pictures

What `README.md` and `README.zh-CN.md` show that is not a screenshot. Both
languages point at the same files; each is a light/dark pair chosen by
`<picture>` from the reader's GitHub theme.

| File | What it is |
| --- | --- |
| `hero-light.svg`, `hero-dark.svg` | The title: the name, the two lines of promise, and a terminal pane where two commands are marked in the left margin — the second one as having failed. Hand-editable; no fonts are loaded, only asked for. |
| `surfaces-light.png`, `surfaces-dark.png` | Cards, the markdown preview and the web preview, framed together. These three are the surfaces neither README pictures anywhere else. |

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
`<object>` sized `1600 × 1632` — four thirds of the `1200 × 1224` canvas, so the
1600-pixel screenshots land near their own scale — and save that box as the PNG.

The SVGs reference the screenshots relatively, so they only render from inside
this checkout. That is why the published asset is the PNG.
