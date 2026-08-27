# Screenshots

The images `README.md` and `README.zh-CN.md` point at. They are taken on a real
machine, by hand, and committed here. Nothing generates them.

`.gitignore` ignores `*.png` across the tree and makes an exception for this
folder, so a file put here is tracked. Keep them PNG: a screenshot of text is
exactly the picture JPEG is worst at.

## What every shot has in common

- **1600 × 1000 logical pixels at 100% scale.** Size the window to that, take the
  shot on a 100% display, and do not scale the file afterwards — a resampled
  screenshot of a terminal is a screenshot of the resampler. On a machine that is
  only 150% or 200%, take it at that scale and say so in the file name
  (`…@200.png`); do not shrink it.
- **Light and dark, one each.** Switch the scheme in Settings between the two
  shots and change nothing else, so the pair differs only in the palette.
- **The default font, the default scheme.** A reader is looking at what they will
  get, not at a setup.
- **Nothing personal in the picture.** No real user name in a path, a prompt, a
  title bar or a files column; no addresses; no repository that is not this one or
  a throwaway. `scripts/check-machine-paths.ps1` reads text, not pixels — this one
  is on you.
- **The window is the whole picture.** No desktop, no wallpaper, no taskbar.
  Include the window's own shadow only if it is a clean edge.

## The list

| File | Scene |
| --- | --- |
| `main-window-light.png` | The three-column window: a files column on the left, two terminal panes side by side in the middle, a markdown document open in a preview pane on the right. Something real in the terminal — a build running, a directory listing — not an empty prompt. |
| `main-window-dark.png` | The same window, dark scheme, same content. |
| `cards-light.png` | Cards (`Ctrl+Shift+Z`) with six to eight panes, each showing different content, so the point of the layout is visible at a glance. |
| `cards-dark.png` | The same, dark. |
| `preview-markdown-light.png` | A markdown document in a preview pane beside a terminal: a heading, a table, a code block and a display formula, all visible at once. |
| `preview-markdown-dark.png` | The same, dark. |
| `preview-pdf-light.png` | A hover card over a file name in the files column, showing the first page of a PDF above its page count and size. A card, not an open pane — this is the one the README uses for the preview section. |
| `preview-pdf-dark.png` | The same, dark. |
| `preview-web-light.png` | A web page in a preview pane beside a terminal, with the address field and the site's icon visible. Use a page that will still exist and does not date the shot. |
| `preview-web-dark.png` | The same, dark. |
| `settings-agents-light.png` | The Agents page in Settings, showing the three installer rows for Claude Code, Codex and Copilot CLI with their sentences readable. Leave them switched off — that is what a reader's own machine looks like. |
| `settings-agents-dark.png` | The same, dark. |

## Alt text

The README carries the alt text; it is written there, next to the image, and it
describes what is in the picture rather than naming the file again. If a shot ends
up showing something different from what the alt text says, the alt text is the
thing that is wrong.

## Replacing one

Overwrite the file, keep the name. The README links by name, and both languages
link to the same images.
