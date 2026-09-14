<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%"
       alt="Folio, a terminal for Windows and macOS that typesets LaTeX and
       marks the tab of an agent waiting for you. Beside the name, a terminal
       pane shows a display formula typeset in a command's output, above the
       next prompt.">
</picture>

[![License: MIT or Apache-2.0](https://img.shields.io/badge/license-MIT%20or%20Apache--2.0-green)](#licence)
[![Build](https://github.com/lulu-loopp/folio-terminal/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/lulu-loopp/folio-terminal/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/lulu-loopp/folio-terminal?include_prereleases&label=release&color=blue)](https://github.com/lulu-loopp/folio-terminal/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/lulu-loopp/folio-terminal/total?label=downloads&color=pink)](https://github.com/lulu-loopp/folio-terminal/releases)

Folio is an open-source terminal for Windows and macOS. It typesets LaTeX where
a command prints it, previews files beside the prompt, lets panes and windows
move freely, and marks the tab of an agent that is waiting for you.

[中文说明](README.zh-CN.md) · [Shortcuts](docs/shortcuts.md) ·
[Security](SECURITY.md) · [Changes](CHANGELOG.md)

## Install

**Windows** — take the zip from the
[releases page](https://github.com/lulu-loopp/folio-terminal/releases), unpack it,
and run `folio.exe`. No installer; the files in it belong together, so keep the
folder as it came. Windows 10 1809 or newer, 64-bit.

**macOS** — take the DMG from the same page and drag **Folio** to Applications.
Needs an Apple silicon Mac running macOS 14 or newer. Or, with Homebrew:

```sh
brew install --cask lulu-loopp/folio/folio
```

<!-- winget: add when live -->

[`docs/install.md`](docs/install.md) has the rest: what is in the archive, what
the first run asks, and what to do if the system puts a panel in front of you.

## What it does

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/terminal-math-dark.png">
  <img src="docs/screenshots/terminal-math-light.png" width="100%"
       alt="One terminal pane, the output of a single command typeset where it
       was printed: prose carrying inline formulas, and three display ones.">
</picture>

*The LaTeX a command prints is typeset where it was printed.*

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/preview-markdown-dark.png">
  <img src="docs/screenshots/preview-markdown-light.png" width="100%"
       alt="A markdown document typeset in a preview pane beside a terminal: a
       heading, a table, a code block and a display formula, readable at once.">
</picture>

*A file is read beside the prompt, and a markdown one typed into where you read it.*

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/settings-agents-dark.png">
  <img src="docs/screenshots/settings-agents-light.png" width="100%"
       alt="The Agent page in Settings: a row each for Claude Code, Codex and
       GitHub Copilot CLI, each saying which file its switch writes to, all
       three off, and a fourth row for the end of a turn.">
</picture>

*[The agent waiting for you](docs/features.md#made-for-agents) is marked on its tab.*

- [LaTeX in the terminal](docs/features.md#latex-rendering-in-the-terminal) — `$…$` and `$$…$$` in command output, typeset in the line they were printed on.
- [Preview beside the prompt](docs/features.md#preview-beside-the-prompt-files-pdf-video-web) — PDF page by page, video playing, a web page with an address field.
- [Markdown you edit where you read it](docs/features.md#markdown-you-can-edit-where-you-read-it) — in the reading typeface, saving every other byte of the file untouched.
- [Panes, tabs and windows that move](docs/features.md#panes-tabs-and-windows-that-move) — split, tear off, float, and the sessions inside keep running.
- [A terminal on a hotkey](docs/features.md#a-terminal-on-a-hotkey) — ``Win+` ``, or ``⌃` `` on a Mac, brings one down over whatever is on the screen.
- [Search everything](docs/features.md#search-everything) — actions, panes, commands, files and settings in one box.
- [System integration](docs/features.md#windows-integration) — the Explorer and Finder right-click menus, and VS Code's external terminal.
- [English and Chinese](docs/features.md#english-and-chinese) — every string in both, switched from one row in Settings.

## Privacy

Folio collects nothing: no telemetry, no analytics, no crash reporting. Two
things reach the network — a page you open in the web preview, and the update
check, one `GET` of `https://api.github.com/repos/lulu-loopp/folio-terminal/releases`
carrying no version and no identifier, which switches off at
**Settings > General > Update check** or with `"update_check": false`.
Settings, profiles and sessions live on your own machine and go nowhere;
[`docs/PRIVACY.md`](docs/PRIVACY.md) says what is in each file.

## Licence

MIT or Apache-2.0, at your option; [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) carries every dependency's and [`TRADEMARK.md`](TRADEMARK.md) the name and the marks.

## Building and contributing

[`docs/BUILDING.md`](docs/BUILDING.md) builds it from source and points on to [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`SECURITY.md`](SECURITY.md).
