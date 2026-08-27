# Screenshots

The images `README.md` and `README.zh-CN.md` point at. They are taken on a real
machine, from a real build, and committed here. Nothing regenerates them from
this repository: if one is wrong, somebody takes it again.

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

## What is committed

The size is the one `scripts/check-screenshots.ps1` enforces; the hash is what
was committed, so a file that has been re-encoded on its way through something
can be told from one that was retaken on purpose.

| File | Size | Taken | SHA-256 |
| --- | --- | --- | --- |
| `main-window-light.png` | 1600 x 1000 | 2026-08-27 | `710bb79eb5f1a1dc1c52c5f8da3fdb44582f67338904da974cdf73a922d27d31` |
| `main-window-dark.png` | 1600 x 1000 | 2026-08-27 | `25ebb1b1ee04c8a87531855e91d7f9252da04af757542cf53d69f2c0897a2861` |
| `cards-light.png` | 1600 x 1000 | 2026-08-27 | `102b8d961fa52691e4c58e1c7aad32db865a3d51429f2affaffd50d60c57d2e3` |
| `cards-dark.png` | 1600 x 1000 | 2026-08-27 | `dd7a2e5ca1812bc1d698c9243dac932c4e589598b413ce54b9bb627770ae2175` |
| `preview-markdown-light.png` | 1600 x 1000 | 2026-08-27 | `346148f9950be45287da6f9619e4403989f0bba6422a597a17fcd8d3447eeba9` |
| `preview-markdown-dark.png` | 1600 x 1000 | 2026-08-27 | `579ed8bc99e8033cd2ac9b16c8eb29a3a50538f44f309962d494b5d6db486209` |
| `preview-pdf-light.png` | 1600 x 1000 | 2026-08-27 | `38f7d681c41f0bf7a261244c5608124d2a938b289866feffe24f8b6e6b7891dd` |
| `preview-pdf-dark.png` | 1600 x 1000 | 2026-08-27 | `41dd52d59ee9ed08427387ccc9fceccbf925e0fd357b6277a4f0a583ea913076` |
| `preview-web-light.png` | 1600 x 1000 | 2026-08-27 | `6fdedcce35b82dc0d90b366f92d95e7c4575484c5d7a277df8a25f216246a0a4` |
| `preview-web-dark.png` | 1600 x 1000 | 2026-08-27 | `da3bc0079b9ff6e161e371913e0a55aba042a1a26672e082f9164e5282f0e1df` |
| `settings-agents-light.png` | 1600 x 1000 | 2026-08-27 | `1d992bb544d1fe46cb20191aacc532ee3cb201dc2c74465d56755395b7972c1e` |
| `settings-agents-dark.png` | 1600 x 1000 | 2026-08-27 | `e4d77a5e92392813a8f5187b85a147c4c3b5d7faa75a3e9481beee73e920d375` |

## What was in front of the camera

The twelve committed here were taken from `target\release\folio.exe` against a
throwaway project — `C:\Projects\aurora`, a small library with a five-commit
history, a README with a table and a display formula in it, a PDF and a page —
and through an `%APPDATA%` and `%LOCALAPPDATA%` of their own, so the machine's
real settings, session and profiles were neither read nor written. Each pane is
a shell profile whose command line runs one command and then leaves an ordinary
prompt; `-NoProfile` is deliberate, because a machine's own PowerShell profile
prints a machine's own directories.

**The 100% display was made, not found.** The machine these were taken on has no
monitor at 100% — 2880 × 1800 at 200%, 1920 × 1080 at 150%, 3840 × 2160 at 200%
— and a per-monitor-DPI-aware program draws at whatever scale the monitor it is
on is set to. There is no way to ask such a program for 100% on a 150% monitor:
the number belongs to the monitor. Making the process DPI-*unaware* does not
help either, because Windows then draws it at 1600 × 1000 and stretches the
result, and a stretched screenshot is the one thing the size rule above exists
to prevent. So the second display was set to 100% for the length of the run and
put back afterwards, and the window was parked at exactly 1600 × 1000 physical
pixels on it with `SetWindowPos` and `GetDpiForWindow` checked to read 96.

## Alt text

The README carries the alt text; it is written there, next to the image, and it
describes what is in the picture rather than naming the file again. If a shot ends
up showing something different from what the alt text says, the alt text is the
thing that is wrong.

## Replacing one

Overwrite the file, keep the name, and put the new size, date and hash in the
table above. The README links by name, and both languages link to the same
images.
