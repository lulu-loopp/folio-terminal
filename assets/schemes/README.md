# Bundled colour schemes

Ten scheme files, embedded in the executable with `include_str!` and read
through the same parser a user's own `%APPDATA%\Folio\schemes\*.json` goes
through — one code path, so a bundled scheme cannot be correct in a way a
hand-written one is not.

The format is **Windows Terminal's scheme JSON**, unchanged, so a scheme
copied from windowsterminalthemes.dev or exported from
`mbadolato/iTerm2-Color-Schemes` pastes in as it stands. Folio adds exactly one
optional key, `accent`, which is what the chrome's links, focus rings and
active divider are drawn in; a file without it falls back to the scheme's
`blue`. `cursorColor` falls back to `foreground`, and `selectionBackground`
falls back to the accent at 30 % over the background — this product's own rule
for a selection-weight mark, the one `LIGHT_SELECTION_BACKGROUND_RGB` in
`crates/bt-render/src/theme.rs` was itself struck from.

Every value below is byte-exact from the source named against it. Nothing here
was transcribed from memory.

## Folio's own two

`folio-dark.json`, `folio-light.json` — this product's defaults, and the two
inputs to the derivation pin
(`the_derivation_reproduces_the_dark_palette_byte_for_byte` and its light
twin): the ~139-token `ChromePalette` is *computed* from these two files'
colours and must come out equal to the tables in
`crates/bt-render/src/theme.rs`. Their ANSI sixteen are Windows Terminal's
Campbell (dark) and macOS Terminal.app's default palette (light), which is what
this product shipped before schemes existed. A test pins each file's bytes to
the `FOLIO_DARK` / `FOLIO_LIGHT` constants, so the copy a user can open and
edit and the copy the pin runs against cannot drift apart.

## From `microsoft/terminal`

`one-half-dark.json`, `one-half-light.json`, `solarized-dark.json`,
`solarized-light.json`

Source: <https://github.com/microsoft/terminal>,
`src/cascadia/TerminalSettingsModel/defaults.json`.
Licence: MIT — Copyright (c) Microsoft Corporation. All rights reserved.

These four are Windows Terminal's own built-ins, taken from the file that
defines them rather than from a re-export, which is also why the Solarized pair
keeps the canonical base03/base02 ordering for `black`/`brightBlack` and shares
one ANSI sixteen between its light and dark halves. Windows Terminal writes no
`selectionBackground` for the two dark ones; the fallback above supplies it.

One Half is Son A. Pham's `sonph/onehalf` (MIT), by way of Microsoft's
transcription of it; Solarized is Ethan Schoonover's (MIT).

## From `mbadolato/iTerm2-Color-Schemes`

`gruvbox-dark.json`, `gruvbox-light.json`, `dracula.json`, `nord.json`

Source: <https://github.com/mbadolato/iTerm2-Color-Schemes>,
`windowsterminal/Gruvbox Dark.json`, `windowsterminal/Gruvbox Light.json`,
`windowsterminal/Dracula.json`, `windowsterminal/Nord.json`.
Licence: MIT — Copyright (c) 2011 to Present Mark Badolato. That LICENSE adds,
verbatim: "This license covers the iTerm-Color-Schemes repository collection of
themes. The copyright/license for each individual theme belongs to the author
of that theme."

Upstream authors: Gruvbox is Pavel Pertsev's `morhetz/gruvbox` (MIT); Dracula
is Zeno Rocha's `dracula/dracula-theme` (MIT); Nord is Sven Greb's
`nordtheme/nord` (MIT).

## What is *not* a scheme

The chrome's ~139 colours are derived from the six above (background,
foreground, cursor, selection, accent, and the canvas the background lands on),
never stored, so there is nothing in these files that names a divider or a tab.
The status four, the commit graph's eight lanes and the seven syntax inks keep
their fixed hues in both canvases: each was struck against a contrast floor on
both surfaces, and a red that means "this failed" is not a colour a scheme gets
a vote on. Profile identity marks are likewise not scheme-controlled.

## Editing one

Copy any file here into `%APPDATA%\Folio\schemes\`, change its `name`, and it
appears in Settings → Appearance → Light scheme / Dark scheme the next time the
dialog opens. A file that will not parse is skipped and says so once, by name,
in a toast; it never takes the window down with it.
