# Screenshots

The images `README.md` and `README.zh-CN.md` point at. They are taken on a real
machine, from a real build, and committed here. Nothing regenerates them from
this repository: if one is wrong, somebody takes it again.

`.gitignore` ignores `*.png` across the tree and makes an exception for this
folder, so a file put here is tracked. Keep them PNG: a screenshot of text is
exactly the picture JPEG is worst at.

## What every shot has in common

- **1600 × 1000 logical pixels at 200%, so 3200 × 2000 in the file.** The window
  is sized to 1600 × 1000 logical and photographed on a 3840 × 2160 display the
  machine already keeps at 200%, and the file is what came off the glass — two
  device pixels to the logical one, nothing resampled. A README is read on a
  screen that is mostly not 100%, and a picture with a pixel per pixel to give is
  the one that stays sharp there. Do not scale the file afterwards; a resampled
  screenshot of a terminal is a screenshot of the resampler.
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
- **Every file goes through `oxipng -o 4 --strip safe` before it is committed.**
  Lossless, and it takes about a fifth off. Doubling the pixels quadrupled what
  every reader has to fetch, so the cheapest bytes are the ones nobody had to
  send.

## The list

| File | Scene |
| --- | --- |
| `terminal-math-light.png` | One terminal pane and nothing else open, showing a command whose output was typeset where it was printed: paragraphs that run most of the way across the pane, three display formulas spread down it, and short inline ones inside the sentences between them. Nothing else competes for the eye — the picture says only that mathematics in command output is mathematics. |
| `terminal-math-dark.png` | The same, dark. |
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
| `terminal-math-light.png` | 3200 x 2000 | 2026-08-27 | `be5796d8f15de6100ff69233a8601f0d0e369f67af7c44cecadf05a10de40009` |
| `terminal-math-dark.png` | 3200 x 2000 | 2026-08-27 | `9086c637b0fb2247d4445ba036ee9a4fb1eacf28674e17ee7a6753be0e48bb7f` |
| `main-window-light.png` | 3200 x 2000 | 2026-08-27 | `fa04896cdfe71d4146a93cab47357d7065eabe3343733c4beb58849642490281` |
| `main-window-dark.png` | 3200 x 2000 | 2026-08-27 | `4bd254d57be67731b0490c2e5832d87fb4448362db66a273acfcf22917fad7db` |
| `cards-light.png` | 3200 x 2000 | 2026-08-27 | `90037e34d3531f81d7180f769f1ffa26f37c8dfbd85b87b2e61034f3d25e0601` |
| `cards-dark.png` | 3200 x 2000 | 2026-08-27 | `d2cc09af06856121085d45d2012ff9bc0534fcca7a2400a5d4df170bf4d79fa1` |
| `preview-markdown-light.png` | 3200 x 2000 | 2026-08-27 | `a073da69f6df9e86913d14a1f71d121c36be109bd026f5c3f09f2e0d95411110` |
| `preview-markdown-dark.png` | 3200 x 2000 | 2026-08-27 | `abd8dde82c59bbaa7554936342c39923bace31cfc8d5f943a3918bcf530dfeac` |
| `preview-pdf-light.png` | 3200 x 2000 | 2026-08-27 | `8a2222bea07019b1ef4bcffff2cc852cd822bae9b0ff72c4d7953da07167dd4e` |
| `preview-pdf-dark.png` | 3200 x 2000 | 2026-08-27 | `e1bf4ed856e86911e581c5ad5af822a67cf95a5c3a9cb4d4fb69cc8181278f51` |
| `preview-web-light.png` | 3200 x 2000 | 2026-08-27 | `ba0e133569ddc2c4989d8efc75559eaf2d1bfb5c69bfdb37ef8774dd15e6366c` |
| `preview-web-dark.png` | 3200 x 2000 | 2026-08-27 | `f9849ed8813c683d5ff94c434527b46de6878d6a2d8503b87f305263c4615810` |
| `settings-agents-light.png` | 3200 x 2000 | 2026-08-27 | `12a9d1968d7a979f6d8796cf7a8054c68f4a640c916ac6402c0c774b482768bd` |
| `settings-agents-dark.png` | 3200 x 2000 | 2026-08-27 | `ceba5a7e1f8d3b4040b5fcb4a97cc958102bd03dcfe023b705b316952c2ed01d` |

The fourteen come to 2.57 MiB after `oxipng`; `docs/plans/release/large-files.md`
carries that number beside everything else a clone has to fetch.

## What was in front of the camera

The fourteen committed here were taken from `target\release\folio.exe` against a
throwaway project — `C:\Projects\aurora`, a small library with a five-commit
history, a README with a table and a display formula in it, a PDF and a page —
and through an `%APPDATA%` and `%LOCALAPPDATA%` of their own, so the machine's
real settings, session and profiles were neither read nor written. Each pane is
a shell profile whose command line runs one command and then leaves an ordinary
prompt; `-NoProfile` is deliberate, because a machine's own PowerShell profile
prints a machine's own directories.

**`terminal-math-*` is the one whose command is not run by its profile**, and it
is worth saying why, because the next person to retake it will otherwise take a
picture with half its subject missing. An inline `$…$` is read as mathematics
only between OSC 133;C and 133;D — `bt_detect::InlineMathSite`, whose
`Ineligible` arm says as much in its own words: a primary screen with no shell
integration renders no inline mathematics, ever. A command line handed to
`-Command` runs before the interactive loop and sits inside no such region, so
that shot's command has to be submitted at a real prompt, with
`scripts\shell-integration\folio.ps1` dot-sourced by the profile the way an
installed reader has it. Typing it is what a machine with a Chinese IME loaded
will not allow — the IME owns every unmodified printable key and the line arrives
as candidate text — so it goes in through the clipboard and `Ctrl+V`, which is a
modified key the IME does not take. Its folder is `C:\Projects\notebook`, one
file and no repository, so that the other thirteen see a project that has not
grown a directory for this one's sake.

That one file is as long as it is on purpose. The first take of this pair filled
the top third of the window and the left half of the pane, with a single display
formula adrift in the white below it, and a reader's fair reading of that picture
is that the program had run out of things to draw. So the document was written to
the window: prose wrapped at 150 columns of the 177 the pane holds, enough of it
to reach about 86% of the way down, and three display formulas — the Gaussian
normalisation integral, the Fourier pair and the exponential series — spread top,
middle and bottom. The inline runs are short ones (`$\sigma$`, `$\mu$`, `$1/n!$`,
`$\pi$`), placed mid-sentence and never before a comma: an inline formula is drawn
at cell height inside the cells its *source* occupies, so a short formula written
with long macros leaves the rest of that span blank, and that blank reads as a
defect when punctuation lands on the far side of it.

**Two device pixels to the logical one, and no display is touched.** The window
is parked at exactly 3200 × 2000 physical pixels with `SetWindowPos`, on the
3840 × 2160 panel this machine already runs at 200%, and `GetDpiForWindow` is
checked to read 192 before any shutter opens. An earlier pass at 100% had to
*make* a display it could not find — no monitor here is at 100% — by setting a
second one to 100% for the length of the run and putting it back afterwards.
Photographing the scale a display already has is both truer and one moving part
fewer, and the file that comes out has a pixel per pixel to give on the screens
a README is actually read on.

## Alt text

The README carries the alt text; it is written there, next to the image, and it
describes what is in the picture rather than naming the file again. If a shot ends
up showing something different from what the alt text says, the alt text is the
thing that is wrong.

## Replacing one

Overwrite the file, keep the name, and put the new size, date and hash in the
table above. The README links by name, and both languages link to the same
images.
