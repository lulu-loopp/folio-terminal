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
a command prints it, previews files beside the prompt, and marks the tab of an
agent that is waiting for you.

[中文说明](README.zh-CN.md) · [Shortcuts](docs/shortcuts.md) ·
[Security](SECURITY.md) · [Changes](CHANGELOG.md)

> **Preview.** 0.3.0 is a preview build, signed by Weiyi Shi — on Windows with
> a certificate from Microsoft's Artifact Signing service, on macOS with a
> Developer ID certificate and notarized by Apple. See [Download](#download)
> below.

---

## Download

### Windows

Take [`folio-0.3.0-windows-x64.zip`](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.3.0-preview/folio-0.3.0-windows-x64.zip)
from the [releases page](https://github.com/lulu-loopp/folio-terminal/releases),
unpack it wherever you keep programs, and run `folio.exe`. There is no
installer; keep the extracted files together in one folder. `SHA256SUMS.txt` is
the hash of what you downloaded. Needs **Windows 10 1809 or newer, or Windows
11, 64-bit**.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, with a certificate from
Microsoft's Artifact Signing service. On first run Windows may show
**"Windows protected your PC"**.
**"More info"** names the publisher: check that it reads **Weiyi Shi** before
running it.

The archive holds nine files that belong together: `folio.exe`, the two console
libraries it needs to start a shell, `folio.msix` for the Explorer menu,
`folio-here.cmd` for VS Code, and the licences and notices.

The web preview needs the **WebView2 Runtime**. Windows 11 has it; Windows 10
usually does, and if it does not, the Evergreen Runtime is
[here](https://developer.microsoft.com/microsoft-edge/webview2/). Without it
everything except the web preview works, and the preview says what is missing.

### macOS

Take [`Folio-0.4.0-macos-arm64.dmg`](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.0-preview/Folio-0.4.0-macos-arm64.dmg)
from the same [releases page](https://github.com/lulu-loopp/folio-terminal/releases),
open it, and drag **Folio** to Applications. Needs an **Apple silicon Mac
running macOS 14 or newer**; there is no Intel build in this preview.
`SHA256SUMS.txt` is the hash of what you downloaded.

`Folio.app` is signed by **Weiyi Shi** with a **Developer ID** certificate, and
notarized by Apple; the notarization ticket is stapled to the disk image and to
the application inside it, so the check is made on your own machine and works
with the network switched off.

The first open is the ordinary one macOS gives anything downloaded from the
internet: a panel that names the developer and has **Open** in it. Check that
the panel names **Weiyi Shi** as the signer, open it, and it does not ask again.
There is no step where you have to reach for a right-click menu or allow
something afterwards.

What must **not** appear is a panel saying the developer **cannot be verified**,
or that Folio is **damaged and can't be opened**. Either means what you have is
not what was published here — an interrupted download, or a copy altered after
it was signed. Check it against `SHA256SUMS.txt` and take it from the releases
page again. A build you made yourself and signed ad-hoc for your own machine is
refused in the same words; `docs/BUILDING.md` says what to do with one.

The web preview uses the WebKit already on the machine. There is nothing to
install.

## First run

A machine that has never run Folio gets one card, once. It says **Welcome to
Folio** and asks whether to check for updates, add Folio to the folder
right-click menu, enable the PowerShell integration, and mark the tab for each
of Claude Code, Codex and Copilot CLI this machine has. Update checks arrive on;
the rest arrive off. Nothing about theme, font, size, language or layout — those
are one click away and cost nothing while they are wrong.

**The card offers only the rows the machine can honour.** The folder right-click
menu and the PowerShell integration are Windows facilities, so a Mac is asked
about the update check and the agents and nothing else.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/first-run-dark.png">
  <img src="docs/screenshots/first-run-light.png" width="100%"
       alt="The card over a window that has just started: Folio's mark beside
       Welcome to Folio, then the rows this machine was offered, one line each
       with a switch at the right of every one. Check for Folio updates is on;
       Open any folder in Folio from its right-click menu and Jump between
       commands in PowerShell are off; after a wider gap, Mark the tab when
       Claude Code is waiting, when a Codex turn ends, and when Copilot CLI is
       waiting, all three off. At the foot, a faint line reading You can change
       these options in Settings, then Not now and Done. Behind the card, one
       tab and a prompt.">
</picture>

**Rest the pointer on a row and it says how** — including which of your own
files the switch writes, and that the file is copied to a dated backup first.

**Every row on the card is also a row in Settings**, so nothing on it is a last
chance. **Done** applies the rows that are on. **Not now** and `Esc` keep the
shipped values: the update check on, the rest off. Either way the card does not
come back, and if you were already using Folio you never see it.

The first tab opens the first shell your machine actually has. On Windows the
five shipped profiles are looked for in order — PowerShell 7, Windows
PowerShell, WSL, Git Bash, Command Prompt; on a Mac it is the shell your account
already uses, then zsh, then bash, then `/bin/sh`. One whose program is not
installed does not appear in the menus that start a shell; it stays on the
Profiles page in Settings, greyed out and naming the program that was looked
for. The seven agent profiles are found the same way, on the `PATH`, and that is
the same lookup the card's agent rows use.

**On Windows**, the PowerShell integration adds one line —
`. "$env:APPDATA\Folio\shell-integration\folio.ps1"` — to the `$PROFILE` a
PowerShell names for itself, after copying the file as it stood to a dated backup
beside it; delete that line to undo it. Command marks and inline `$…$` formulas
run on that integration. Git Bash and WSL need none of it, and neither do zsh
and bash on a Mac: Folio hands those their own integration as it starts them,
out of its own directory, and writes nothing of yours.

Where that file is comes from the shell, so a row left on when the card is
answered takes effect in the next PowerShell session, and Settings > Terminal
says so until it does.

If you were not asked on the card, a strip offers the same thing the first time
a PowerShell pane prints something: **Add to `$PROFILE`** does it, **Don't show
again** ends the asking, and closing the strip decides nothing, so the next
PowerShell asks once more.

**On a Mac**, what Folio remembers lives in
`~/Library/Application Support/Folio`.

The first time a waiting agent's mark has to leave the window, macOS asks
whether Folio may send notifications. Answer it once. Say no and the Agent page
in Settings says so rather than going quiet, the dot on the tab and the Dock
icon go on working, and Folio does not ask again.

Finder's right-click menu gets **Open in Folio**, under **Services** — Folio
registers it the first time it runs, so there is nothing to switch on and no
need to sign out. On a folder it opens a tab standing in that folder; on a file,
a tab standing in the folder the file is in. Folio also has a menu bar of its
own, and every item in it carries the same key as the row it has in
[Shortcuts](docs/shortcuts.md).

The three rows on the Agent page read the tool's own configuration file and
report what is in it. On a new machine all three files are absent, so all three
read Off.

---

## Features

The keys named below are the Windows ones. [Shortcuts](docs/shortcuts.md) has
both columns: on a Mac an application verb wears **Command** where Windows wears
**Ctrl**, which is what leaves **Control** to the terminal on both.

### LaTeX rendering in the terminal

The LaTeX a command prints is typeset where it was printed.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/terminal-math-dark.png">
  <img src="docs/screenshots/terminal-math-light.png" width="100%"
       alt="A single terminal pane with the output of one command typeset where it
       was printed: paragraphs of prose carrying short inline formulas, and three
       display formulas set on lines of their own — the Gaussian normalisation
       integral, the Fourier transform pair, and the series for the exponential.">
</picture>

- `$…$` and `$$…$$` in command output are set in the line the command printed
  them on.
- The preview pane takes those, plus `\(…\)`, `\[…\]` and the bare `amsmath`
  environments.
- Unsupported LaTeX remains visible as source text.
- Inline `$…$` is told from a shell variable by the shell integration below —
  PowerShell on Windows, zsh or bash on a Mac.
  Without it inline formulas stay as source, and `$$…$$` blocks still typeset.

### Made for agents

The agent that is waiting for you is marked on its tab, so there is nothing to go and check.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/settings-agents-dark.png">
  <img src="docs/screenshots/settings-agents-light.png" width="100%"
       alt="The Agent page in Settings: a row each for Claude Code, Codex and
       GitHub Copilot CLI, each with a sentence saying which file its switch
       writes a notification hook into and all three switched off, and a fourth
       row for notifications at the end of a turn.">
</picture>

- A waiting agent lights a dot on its tab. If another program has the focus,
  Windows flashes the taskbar and macOS bounces the Dock icon until you come
  back; if the window is minimised or on another desktop, the machine's own
  notification is raised. On a Mac the first of those is where macOS asks
  whether Folio may send notifications — answer it once, and a refusal is
  reported on the Agent page rather than swallowed.
- One request interrupts at most once, and the dot clears when you answer in that
  pane or the program withdraws the request. `Ctrl+Shift+A` jumps to the longest
  wait.
- Claude Code, Codex and GitHub Copilot CLI each have a switch on the Agent page
  in Settings that writes one notification hook into that tool's own configuration
  file and takes it back out again. Nothing is installed by default.
- Seven profiles start an agent — Claude Code, Codex, Copilot CLI, Kimi Code, pi,
  Hermes, OpenCode — found on the `PATH`; on Windows, one installed inside WSL is
  run from the WSL profile. Any program that writes `OSC 1337;RequestAttention=yes`
  raises the mark, with nothing installed at all.

### Preview beside the prompt: files, PDF, video, web

What is in a file is readable beside the prompt, without opening another
application.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/preview-pdf-dark.png">
  <img src="docs/screenshots/preview-pdf-light.png" width="100%"
       alt="The pointer rests on a file name in the files column and a card has
       come up under it, showing the first page of a PDF above its page count and
       size. The wheel winds the card through the pages.">
</picture>

- A card comes up under the pointer when it rests on a name in the files column:
  the PDF page by page, the video playing, the first lines of the text, the image
  itself.
- The preview pane opens the file beside the prompt — markdown typeset, PDF page
  by page, video playing, a web page with an address field and Back.
- A path the terminal printed opens in the preview pane on a click, and goes to
  the machine's own application on `Ctrl`+click — that one is `Ctrl` on both
  platforms, and not Command on a Mac. Paths nobody marked up are found too,
  once the file is confirmed to exist.
- A web address follows the same rule: a click opens it in the preview pane,
  `Ctrl`+click hands it to the browser.
- A window holds as many pages as it has preview panes. A second page opens on a
  pane of its own instead of navigating the first, so a page can be locked and
  another opened beside it, and a page dropped on a pane opens on the pane you
  dropped it on.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/surfaces-dark.png">
  <img src="assets/readme/surfaces-light.png" width="100%"
       alt="Three more surfaces: a window laid out as cards, a markdown document
       typeset in a preview pane, and a web page in a preview pane with a
       breadcrumb address field above it.">
</picture>

### Markdown you can edit where you read it

A `.md` file is typed into on the page you were reading, in the typeface you
were reading it in.

- Click into a paragraph, heading, list or quote and it shows its own
  Markdown — the `#` of a heading, the `**` around a bold phrase, the `- ` in
  front of an item, the `> ` down the side of a quote — in the reading typeface
  and at the reading size, while every other block on the page stays as it
  reads. Leave the block and it is back to its rendered form.
- A code block, a table or a formula turns to monospace source instead, because
  there the way the characters line up is part of what they say.
- The caret moves between blocks the way it moves between lines, so there is
  nothing to enter and nothing to leave: arrows, `Home`, `End`, `Enter` and
  `Backspace` do what they do anywhere else. What you select is what you copy,
  and a copy out of a document you are editing brings the marks with it, so what
  you paste back is what was there.
- `Ctrl+S` writes the file, `Ctrl+Z` takes back the last change and `Ctrl+Y`
  puts it again. A run of typing comes back in one press rather than a letter at
  a time, and the unsaved dot goes out when you undo back to your last save.
- A save changes the part you edited and leaves the rest of the file identical,
  down to the byte. A file that names its own encoding in its first bytes —
  what Windows PowerShell writes is the common case — is written back in it.
  Line endings, trailing spaces and a missing last line break survive as they
  always did.
- `Esc` leaves the page, and so does clicking the empty space beside the text or
  clicking away from the pane altogether. The page goes back to reading as a
  page, and the caret stays where you left it.
- A file over 8 MB, and one Folio could not read as text all the way through,
  opens and reads and copies as before but is not edited; the foot of the pane
  says why.
- The files column's right-click menu makes a file or a folder in place:
  `New file…` and `New folder…` put the name field in the tree where the new row
  will appear, `Enter` creates it and `Esc` cancels. A name the folder will not
  take turns red in the field rather than being explained somewhere else.
  `Delete` sends a file, or a whole folder, to the Recycle Bin on Windows and to
  the Trash on a Mac, and asks nothing first, because that is where it goes.
- Right-clicking the empty space in a files column opens the menu of the folder
  the column is standing in, so a folder with nothing in it can still be given
  its first file.
- `Ctrl+Shift+P` finds a file under the folder the column is standing in, and
  `Enter` opens it in the preview pane, ready to be typed into.

### Panes, tabs and windows that move

The layout changes while the sessions inside it keep running, and all of them can
be seen at once.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/tab-into-pane-dark.gif">
  <img src="docs/screenshots/tab-into-pane-light.gif" width="100%"
       alt="A tab is pressed and dragged down out of the tab strip; over the
       right half of the window a landing preview appears, and on release the
       tab's shell becomes the right-hand pane, still running. Then the new
       pane's own header is dragged to the bottom edge, and the side-by-side
       layout becomes two full-width bands.">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/cards-dark.png">
  <img src="docs/screenshots/cards-light.png" width="100%"
       alt="The tab strip has become a column of cards. The single card stands for
       a tab of eight panes and draws all eight in miniature; Alt — Option on a
       Mac — and the wheel scroll the picture inside a card a row at a time.">
</picture>

- `Alt+Shift+-` splits a pane across, `Alt+Shift+=` splits it down. A tab or a
  single pane can be dragged out into a window of its own, and the panes it did
  not touch keep their widths.
- A pane dropped on the join between two tabs becomes a tab *between* them: a
  gap opens where it will land. Dropped on a tab itself it joins that tab's
  layout. The horizontal strip, the vertical rail and the card column all read
  the join the same way.
- `Ctrl+Shift+Z` turns the tab strip into a column of cards, one per tab, each
  drawing that tab's own panes in the layout they have.
- `Ctrl+Shift+G` turns the files column into a Git panel: branch, working tree,
  staged and unstaged files, the commit graph, and a selected file's diff in the
  preview.
- `Ctrl+Shift+↑` and `Ctrl+Shift+↓` step between commands in the scrollback, and
  a command that failed is marked as having failed.
- The folder button over the files column lists the folders your shells are
  standing in, then the last five folders you pointed a column at, each marked
  `recent`.

### A terminal on a hotkey

One key brings a terminal down over whatever is on the screen, and the same key
takes it away again.

**Windows only so far.** The key is registered with the system, and the macOS
way of doing that asks for the Accessibility permission and then loses the grant
every time the application is signed again — so there is no summoned terminal on
a Mac yet, and `docs/shortcuts.md` shows the row unbound in the macOS column.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/quake-dark.png">
  <img src="docs/screenshots/quake-light.png" width="100%"
       alt="A terminal window hanging from the top of a screen, a little below the
       edge and centred, over a File Explorer window showing a small project's
       files. The terminal has one tab, a gear and a close button, and its shell
       has printed four commits and a directory listing above an empty prompt.">
</picture>

- ``Win+` `` drops a terminal across the top of whichever screen the pointer is
  on, over whatever was standing there. Pressing it again puts the window away
  and hands the keyboard back to the program it came down over.
- It is the same Folio — tabs, panes, the files column, the preview, every
  shortcut — and it keeps its shells and its scrollback between summons.
- A rectangle you move or resize by hand is remembered for the display it is on,
  so the summon comes down where you last put it on that screen.
- It has no icon of its own, and closing the last window you can see ends the
  run.
- **Settings > Summoned terminal** holds the key, which profile a new tab opens
  on, the height, width and the gap below the top of the screen, whether the
  window hides when the keyboard leaves it, and a command to run on the first
  summon of each run.
- At the next launch a pinned tab's last command can come back typed at its
  prompt — typed, and not run.

### Search everything

One box answers five questions at once, and `Enter` goes straight there.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/palette-dark.png">
  <img src="docs/screenshots/palette-light.png" width="100%"
       alt="A box floating over the top of the window, a query typed into its
       field and its results under five headings that do not mix: an action, a
       pane, a command the window has run, a file, and a setting. The first row
       is highlighted, and the letters that matched are marked in each row.">
</picture>

- `Ctrl+Shift+P` raises a box over the top of the window with five sections that
  never mix: what Folio can do, the panes and tabs this window has open, the
  commands it has run, the files under the folder its column is standing in, and
  the settings.
- Typing narrows all five together and the arrow keys walk them. `Enter` on a
  pane raises it, on a file opens it in the preview pane, on a setting opens
  Settings at that row, and on an action does it.
- A command still running is pointed at with a ring around the pane it is running
  in, rather than a scroll to a line that has gone past.
- File search covers the folder the files column is showing.

### Windows integration

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/main-window-dark.png">
  <img src="docs/screenshots/main-window-light.png" width="100%"
       alt="The window in its default font and default scheme: a files column on
       the left, two terminal panes side by side, and a markdown document open in
       a preview pane on the right.">
</picture>

- **Settings > General > Explorer context menu** puts Folio in the folder
  right-click menu, and On is everything your Windows can do. Windows 11 files
  "Open Folio here" under "Show more options"; on Windows 10 it stands in the
  only menu there is. On a Windows 11 with `folio.msix` beside `folio.exe`, On
  also puts "Open in Folio" on the page Windows 11 opens first. Off takes back
  whichever is registered, and the line under the row says which of them On
  reaches on your machine. A running File Explorer reads its list of first-page
  entries when it starts, so if "Open in Folio" is not there yet, sign out and
  back in. `docs/PRIVACY.md` lists what is written. If Folio is already running,
  the entry opens the folder as a new tab in the window you used last and brings
  that window forward — it asks for a terminal in a folder, not for another
  Folio, so it does that whatever **Settings > General > Opening Folio again**
  says. Starting Folio any other way opens a window, unless that row says
  otherwise.
- Windows PowerShell 5.1 ships PSReadLine 2.0.0, which misplaces the input line
  after the window is resized. Folio carries a patched 2.4.6 and installs it into
  your module path on request. On a machine whose execution policy is still the
  stock `Restricted`, the switch says so and hands you the `Set-ExecutionPolicy`
  command that lets the module load.

### macOS integration

- **Finder's right-click menu** carries **Open in Folio** under **Services**.
  Folio registers it the first time it runs, so it is there without a sign-out
  and with nothing to enable. A folder opens as a tab standing in it; a file, as
  a tab standing in the folder it is in. Either arrives in the window you used
  last rather than starting a second Folio.
- **The menu bar is the shortcut table.** Every item takes its key from the same
  row of [Shortcuts](docs/shortcuts.md) the keyboard does, so a verb has one
  name and one key wherever you meet it.
- **The Dock icon is the attention channel.** A waiting agent bounces it until
  you come back; a command that reports how far along it is puts that on the
  icon as a badge.
- **Paths are written from your home directory.** A file under it reads
  `~ › …` in the files column, and the `~` is a step you can click like any
  other.
- **Three Settings rows are not on a Mac at all** — the Explorer context menu,
  the PowerShell integration and the PSReadLine repair. None of the three has a
  macOS counterpart, and a greyed row explaining a mechanism the machine does
  not have only teaches a Windows word. One row is a Mac's alone: **Option key
  sends Alt**, off by default, so that Option keeps typing the character it is
  printed with.
- **Not on macOS:** the summoned terminal, the sparse-package route to the
  first page of a right-click menu, and the Store video extensions. macOS is
  **arm64 only** in this preview.

### Visual Studio Code

**On Windows**, `folio-here.cmd` ships in the archive, beside `folio.exe`, and
is one line:

```bat
@"%~dp0folio.exe" --from-here --cwd "%CD%"
```

Point VS Code's external terminal at it — Settings, or `settings.json`:

```json
"terminal.external.windowsExec": "C:\\Tools\\folio\\folio-here.cmd"
```

**Terminal > Open in External Terminal** (`Ctrl+Shift+C`) then opens Folio on the
folder the editor is standing in. The `.cmd` exists because that setting runs a
command with no arguments, and `--cwd` is how Folio is told where to start;
`--from-here` says that this is a terminal in a folder rather than another
Folio, so it arrives as a tab in the window you used last whatever
**Settings > General > Opening Folio again** says.

On macOS that setting names an application rather than a command, so there is no
`folio-here` for it to run. Finder's **Open in Folio** above is the way to put a
folder in front of a shell.

---

## Privacy

Folio has no telemetry, no analytics and no crash reporting. There is no model
and no API key in it; it serves the agents you already run. Two things reach the
network: a page you open in the web preview, and the update check.

The update check is one `GET` of
`https://api.github.com/repos/lulu-loopp/folio-terminal/releases`, at most
once a day across every window, carrying a `User-Agent` of `Folio` and nothing
else: no version, no identifier, no query string. It goes out through the
machine's own HTTP stack on both platforms, so it follows the proxy settings and
the certificate store you already have. What it can do with the answer is draw a
mark on the settings gear and a line in Settings; it downloads nothing and
replaces nothing. Switch it off at Settings > General > **Update check**, or
with `"update_check": false` in `settings.json`.

What it remembers lives in two directories. On Windows: `%APPDATA%\Folio` for
settings, profiles, schemes and the session, and `%LOCALAPPDATA%\Folio\WebView2`
for the web preview's cookies and cache. On a Mac:
`~/Library/Application Support/Folio` for the first, and the place WebKit keeps
every application's website data for the second. Delete the first and Folio
starts as it did new.

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\Folio"
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

```sh
rm -rf ~/Library/Application\ Support/Folio
```

What is in each file, and why a full address ends up in `session.json`, is in
[`docs/PRIVACY.md`](docs/PRIVACY.md).

## Known issues

- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` — or
  `~/Library/Application Support/Folio/diagnostics.log` on a Mac — if you hit
  it.
- **Windows: `.webm` needs the VP9 or AV1 Video Extension** from the Microsoft
  Store. A stock Windows has neither, and without one there is no still and no
  playback.
- **macOS: there is no summoned terminal yet**, and a run that ends in a crash
  leaves its report where the system puts every one,
  `~/Library/Logs/DiagnosticReports`; the next Folio names the file in its own
  log rather than copying it anywhere.
- The rest are in [`CHANGELOG.md`](CHANGELOG.md).

## Licence

MIT or Apache-2.0, at your option. Every dependency's licence and the notices they
require are in [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

The two licences grant copyright and patent permissions and nothing else. The
Folio name and the marks are not covered — [`TRADEMARK.md`](TRADEMARK.md) says
what that means for a modified distribution.

## Building and contributing

Building from source is in [`docs/BUILDING.md`](docs/BUILDING.md);
[`CONTRIBUTING.md`](CONTRIBUTING.md) is how a change gets made; security reports
go through the private channel in [`SECURITY.md`](SECURITY.md), not an issue.

## What's next

- Linux.
- One build for both kinds of Mac.
- The terminal from a phone.

These are directions, not dates.
