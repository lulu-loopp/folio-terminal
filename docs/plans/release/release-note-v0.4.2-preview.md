> The body of the GitHub Release for this version, as published.

# Folio 0.4.2

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.2-preview/folio-0.4.2-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.2-preview/Folio-0.4.2-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.2-preview.zh-CN.md)

## Highlights

- Drop a file onto a split, or paste a screenshot you have just taken, and its
  path arrives on the command line, quoted for the shell running there; a file
  dragged out of the files column into the middle of a terminal does the same,
  and its edges still open a pane.
- A typeset formula stays typeset: it no longer flashes back to its `$$…$$`
  source while a program repaints the screen, and an older one no longer comes
  back as plain text when you resize the window. An inline formula is typeset
  even when its picture is ready after the prompt has come back — the first
  `cat` of a fresh window included — and your prompt is never typeset, whatever
  it says.
- A formula a program prints cannot harm the window it is printed in: one nested
  too deeply or asking for too large a table is refused and left as text, a
  formula cannot run a program, and one that cannot be drawn no longer takes the
  whole window with it.
- Typing and the pointer stay responsive: a picture that has not changed is not
  drawn again, a wheel notch that moves nothing no longer publishes a frame, and
  resizing no longer copies every pane's whole history twice before it draws.
- Reordering or deleting a profile no longer closes the window or starts the
  wrong shell, and an agent's configuration file Folio cannot read is left
  exactly as you wrote it instead of being written over.
- Losing the graphics device — a power cut that puts a laptop on battery, a
  driver updating itself — no longer closes Folio and every shell in it; the
  window asks for the device again and draws on the new one.
- On a Mac, a tab can be dragged to reorder it or out into its own window again,
  and Shift+wheel reads the rows a formula pushed above the pane; on both
  systems, quitting no longer waits for ever on a disk that has stopped
  answering.

## Changes

### Added

- **Dropping a file onto Folio puts its path on the command line** — on the
  split you dropped it on, spelled for the shell running there, several files on
  one line.
- **A picture on the clipboard pastes as the path of a file Folio writes for
  it**, so a screenshot can be handed straight to a command.
- **Dragging a file from the files column into the middle of a terminal pastes
  its path**; the box under the hand says so before you let go, and the edges
  still open the file as a pane.

### Changed

- **Folio's own log now names the kind of event a window was answering** when it
  stopped answering, so a report about a freeze says where the time went.

### Fixed

- **Formulas no longer flash back to their source while you scroll inside a
  full-screen program**, and a window dragged back to the size it started at no
  longer leaves its formulas untypeset.
- **Motion in a window is drawn once per display frame**, so a formula turning
  over and the two marks beside it move evenly instead of in bursts — on Windows
  most of all — and a formula's highlight leaves as soon as the pointer does.
- **An inline formula in a command's output is typeset even when its picture is
  ready after the prompt has come back**, and a wrapped one stays typeset when
  the window's width changes.
- **Your prompt is never typeset as mathematics**, whatever it says and wherever
  on the screen it is redrawn.
- **Formulas printed after a full-screen program exits are typeset again.**
- **A formula is no longer taken down by its neighbour's result**, and a
  screenful of formulas all at once no longer leaves the first few as source.
- **Dragging a window edge no longer lets a formula swallow the line under it.**
- **Two lines with the same formulas in them no longer show each other's
  picture.**
- **A formula block that had partly scrolled into the history is no longer cut
  off** by the line after it.
- **Clicking a command's mark beside the scroll bar goes to that command every
  time**, also when a formula is on screen.
- **Right-clicking a formula copies that formula**, not one from the pane you
  were typing in.
- **A formula nested past what Folio can draw, or a table of mathematics too big
  to draw, is refused and left as text** rather than ending the program.
- **A formula cannot run a program**, and one formula that cannot be drawn no
  longer takes the whole window with it.
- **The keyboard follows a dropped path**: the terminal that receives it takes
  the keyboard, a file from another program brings Folio to the front, and a
  drop under a dialog, or on nothing that is a terminal, is typed nowhere.
- **Text being composed in an input method is never committed into a different
  pane** from the one it was begun in.
- **Losing the graphics device no longer closes Folio.** The half-prepared
  picture is dropped, the device is asked for again, and the window draws
  everything it was saying on the new one.
- **A dropped file lands in the terminal you dropped it on**, even when Folio
  is busy or you were last hovering somewhere else: the position is read at the
  instant the file is released.
- **Closing a pane or a tab no longer pauses the window** while the program in
  it is being shut down.
- **On a Mac, Shift+wheel scrolls the rows a typeset formula pushed above the
  pane**, and scrolls sideways the way it has always gone on Windows.
- **Closing Folio no longer waits for ever on a disk that has stopped
  answering.**
- **Closing a pane whose reader is stuck no longer hangs the window.**
- **An agent configuration file Folio cannot read is left exactly as it is**
  instead of being written over.
- **A picture that has not changed is no longer drawn again.**
- **Reordering or deleting a profile no longer closes the window**, and no
  longer leaves a pane running a profile you did not choose.
- **On a Mac, a second Folio started from a terminal hands over** instead of
  becoming a second writer of your settings and your saved session.
- **On a Mac, a keyboard shortcut that is not Folio's own is left to whoever was
  waiting for it.**
- **On a Mac, a video left playing no longer builds up memory** for as long as
  it is open.
- **A Markdown file too big to edit now shows the file's current first screen**
  whenever the file is written.
- **A formula broken across two lines by a program that wraps its own text is
  now typeset.**
- **An inline formula in earlier output stays typeset when the window is
  resized.**
- **A typeset formula no longer flashes back to its source while a program
  repaints the screen.**
- **Maximising or resizing a window no longer pauses** when a pane has a long
  history behind it.
- **The tinted block behind a formula's source now stops where the text does.**
- **On a Mac, a tab can be dragged to reorder it or out into its own window**;
  the window moves when the empty part of the header is dragged.
- **A Markdown line beginning with an angle bracket and carrying non-ASCII text
  no longer crashes the preview.**
- **Showing a formula's source applies to that one formula on the screen**, and
  not to every copy of the same text afterwards.
- **A wheel notch at the end of a pane no longer asks for a frame that would be
  the same one.**
- **A traced run no longer pauses when whatever is reading the trace falls
  behind.**

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.2-windows-x64.zip` | the nine files that belong together, in one folder — `sha256:<hash>` |
| `folio-windows-x64.zip` | the same archive under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS.txt` | the hash of the archive under each of its two names and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.2.cdx.json` | the CycloneDX bill of materials for what is in the build |

Unpack the zip wherever you keep programs and run `folio.exe`. There is no
installer; keep the extracted files together in one folder, and unpacking over an
older folder keeps your settings. Needs **Windows 10 1809 or newer, or Windows
11, 64-bit**. The web preview needs the **WebView2 Runtime**, which Windows 11
has and Windows 10 usually does.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, as they have been since
0.2.0, but a signature is not a reputation, so SmartScreen can still raise
**"Windows protected your PC"** on the first run: **More info** names **Weiyi
Shi** as the publisher, and **Run anyway** is the way through.

To check the download, with the zip and `SHA256SUMS.txt` in the same folder:

```powershell
Get-FileHash folio-0.4.2-windows-x64.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.2-macos-arm64.dmg` | the application, signed and notarized — `sha256:<hash>` |
| `Folio-macos-arm64.dmg` | the same image under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS-macos.txt` | the hash of the image under each of its two names, in the format `shasum -c` reads |

Open the image and drag **Folio** to Applications. Needs an **Apple silicon Mac
running macOS 14 or newer**; there is no Intel build in this preview. Or, with
[Homebrew](https://brew.sh):

```sh
brew install --cask lulu-loopp/folio/folio
```

The image and the application in it are signed with a Developer ID and notarized
by Apple. The first open puts up one panel naming the developer, with **Open** in
it; if it opens without offering **Open**, right-click the application and choose
**Open** instead, and it asks once and not again. A panel saying the developer
**cannot be verified**, or that Folio is **damaged and can't be opened**, means
what you have is not what was published — check it against the checksum file and
take it from the releases page again.

To check the download, with the image and `SHA256SUMS-macos.txt` in the same
folder:

```sh
shasum -c SHA256SUMS-macos.txt
```

**Folio has no telemetry, no analytics and no crash reporting.** Two things reach
the network: a page you open in the web preview, and the update check, which
Settings ▸ General ▸ **Update check** switches off.

</details>

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.2-preview/CHANGELOG.md#042-preview--2026-09-18) · [v0.4.1-preview…v0.4.2-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.1-preview...v0.4.2-preview)
