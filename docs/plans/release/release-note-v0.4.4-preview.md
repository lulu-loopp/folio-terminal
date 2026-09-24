# Folio 0.4.4

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.4-preview/folio-0.4.4-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.4-preview/Folio-0.4.4-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：** 上方 zip 与 dmg 即为完整下载,其余为校验和、物料清单与源码。[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.4-preview.zh-CN.md)

## Highlights

- Pasting several lines no longer runs them before you have looked: in
  PowerShell they wait on the input line for Enter, and in cmd a small card
  asks whether to run them line by line or join them into one.
- Ctrl+click on a network-share path, or on a `mailto:`, `vscode:` or other
  link, hands it to Windows or macOS, and handing a file over no longer holds
  the window while Windows starts the program.
- Settings export to one file and import on another machine, from
  Settings ▸ About, beside a button that opens the settings folder.
- A finger slid over a pane scrolls it, with the system's own flick.
- Web panes follow Folio's light or dark theme.

## Changes

### Multi-line paste

- A multi-line paste into PowerShell lands on the input line and waits there
  for Enter.
- In a shell that would run the lines one by one, such as cmd, a card asks
  first: **Run line by line**, **Join**, or cancel. A setting under
  Terminal turns the question off.
- The card works like any two-button dialog: Tab and Shift+Tab move between
  the buttons, Enter presses the highlighted one, Esc cancels.
- When the block is one command wrapped across lines — each line but the last
  ending in `^` in cmd, a backtick in PowerShell or `\` in a Unix shell —
  Enter joins it into one line and drops those marks.
- The card's title no longer runs under its × button, and a very long
  profile name is shortened with an ellipsis.

### Links and hand-offs

- Ctrl+click on a network-share path, or on a `mailto:`, `vscode:` or other
  link, hands it to Windows or macOS. Links inside a previewed document do
  the same.
- Ctrl+click no longer freezes the window while Windows opens the file, and
  Explorer comes to the front.
- The ↗ on a web pane showing an `http` or `https` page opens that page in
  your browser; it used to do nothing.
- A local HTML page in the preview can load its stylesheets, scripts and
  fonts from the web.
- A printed path with spaces in it is a link when the file is there, quoted
  or not.
- A relative path followed by a full-width colon, comma or stop
  (`plot.png：…`, `notes.md。18 items`) is a link again when the file is there.
- A folder printed with a slash on the end (`models/`) is a link, and
  clicking it shows the folder.

### Touch

- Sliding a finger over a pane scrolls it, with the system's own flick; a
  one-finger slide no longer selects text.
- A touch screen, or a remote-desktop tool that sends touch, reaches Folio the
  way it reaches other Windows programs: a tap is a click, and press-and-hold
  opens the menu.

### Glance card and ⌄ menus

- The glance card's bottom line is the file's folder, lit under the pointer:
  click it to find the file in the files column, Ctrl+click to show it in
  Explorer.
- A ⌄ menu you click stays open until you press Esc, click elsewhere or click
  the ⌄ again; resting on it still shows it only while the pointer is there.

### Settings

- Settings ▸ About has **Export…**, **Import…** and the settings folder; every
  link on that page is now a button, all of one size.
- A shortcut can use Ctrl with a letter; the row notes that the key no longer
  reaches the shell.
- The Cards setting named the wrong shortcut on macOS and after a rebind.

### Web panes

- Web panes follow Folio's light or dark theme, and a setting can pin either.
- A site's icon that would disappear into the bar it sits on gets a small
  round plate.

### Terminal and preview

- The command marks down a pane's right edge keep a column of their own and no
  longer cover the last column of text.
- In a Markdown preview you are editing, every block a selection touches shows
  its Markdown; a table the selection only passes through stays rendered. The
  blocks change when you let go of the mouse, not while you drag, and a
  selection across the block being edited highlights it too.
- A zoomed picture in a floating window can be dragged to pan it, and a
  double-click zooms it in or out, as in a pane.
- While a divider is dragged, the pane under the pointer keeps its rounded
  top-right corner.
- Heads, menus, the command palette, the Git page and Settings share one set
  of sizes, so their titles, rows and buttons line up across the window.

### macOS

- The Git page finds git.
- The Dark scheme row no longer names a Windows folder.
- The Option key row in Settings ▸ General explains itself in two lines.

### Stability

- On a slow disk, turning on PowerShell integration from the welcome card no
  longer reports a lock error when everything was written.

## Known issues

- Typing can stall for 1–2 seconds while the GPU is busy. A fix is scheduled.
- Opening a web page holds the window for a few seconds the first time. A fix
  is scheduled.

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.4-windows-x64.zip` | the ten files that belong together, in one folder — `sha256:4896c85c823d92ab99088820ebc16bcbb43e3c7f112be581255b8491ec9fc359` |
| `folio-windows-x64.zip` | the same archive under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS.txt` | the hash of the archive under each of its two names and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.4.cdx.json` | the CycloneDX bill of materials for what is in the build |

Unpack the zip wherever you keep programs and run `folio.exe`. There is no
installer; keep the extracted files together in one folder, and unpacking over an
older folder keeps your settings. Needs **Windows 10 1809 or newer, or Windows
11, 64-bit**. The web preview needs the **WebView2 Runtime**, which Windows 11
has and Windows 10 usually does.

Or, with [Scoop](https://scoop.sh):

```powershell
scoop bucket add folio https://github.com/lulu-loopp/scoop-folio
scoop install folio
```

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, as they have been since
0.2.0, but a signature is not a reputation, so SmartScreen can still raise
**"Windows protected your PC"** on the first run: **More info** names **Weiyi
Shi** as the publisher, and **Run anyway** is the way through.

To check the download, with the zip and `SHA256SUMS.txt` in the same folder:

```powershell
Get-FileHash folio-0.4.4-windows-x64.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.4-macos-arm64.dmg` | the application, signed and notarized — `sha256:ac51d0ec08e54855db1e0dd4eefe5d64fd9def924c3f9507c4f54f275d65716e` |
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

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.4-preview/CHANGELOG.md#044-preview--2026-09-24) · [v0.4.3-preview…v0.4.4-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.3-preview...v0.4.4-preview)
