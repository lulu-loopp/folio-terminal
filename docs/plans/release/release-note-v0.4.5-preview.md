> The body of the GitHub Release for this version, as published.

# Folio 0.4.5

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.5-preview/folio-0.4.5-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.5-preview/Folio-0.4.5-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：** 上方 zip 与 dmg 即为完整下载,其余为校验和、物料清单与源码。[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.5-preview.zh-CN.md)

## Highlights

- Typing stays quick: the window no longer stops to ask Windows where it is
  or about the taskbar, to pass on a changing title, to list every font or to
  move the input method's candidate window, and once you have used web pages
  in Folio, the first one after launch can appear in a fraction of a second.
- Each terminal pane can have its own text size: Ctrl+= and Ctrl+- (⌘ on a
  Mac) or Ctrl+wheel over the pane, and Ctrl+0 to reset.
- Bold text in a terminal font with no bold style is drawn heavier in that
  same font, and bold Chinese text no longer leaves single characters at
  regular weight.
- Tooltips, cards, menus and other fading surfaces fade as one piece.
- Unsaved edits are safe: closing a window while it asks about unsaved
  changes no longer discards them, and keys, pastes and clicks no longer
  reach what is under the "reopen your other tabs" card.

## Changes

### Added

- Each terminal pane can have its own text size: Ctrl+= and Ctrl+- (⌘ on a
  Mac) or Ctrl+wheel over the pane, Ctrl+0 to reset. The pane shows the size
  while it is not 100 %; it resets when Folio restarts.

### Changed

- The first web page you open after launch can appear in a fraction of a
  second: once you have opened web pages in Folio, it gets one ready while
  idle.
- Tooltips, cards, menus and other fading surfaces fade as one piece; their
  edges and text no longer arrive before their plates.
- Bold text in a terminal font with no bold style is drawn heavier in that
  same font.
- Search, video controls, notices, toasts, tab previews, restore lists and
  close buttons use the same text sizes, spacing and corners as the rest of
  the window.
- The download notice looks like the other dialogs and floating notices.
- Right-click and drop-down menus use the same icon-to-label spacing as
  Settings.
- The first-run card uses the same edge spacing and title line height as
  other dialogs.
- Command palette rows use the same corners and icon spacing as other lists.
- Pane heads, files bars and floating windows share the standard spacing,
  captions, icons and controls.
- Settings key caps, profile badges, navigation spacing, bottom padding and
  menu-button corners match their counterparts elsewhere.
- The Git panel and graph match the rest of the window in spacing, corners,
  captions, badges and icons.
- The glance card's head, the drag tag and the peek tag match the other
  floating tags.
- A web pane on Windows uses the system's overlay scrollbars.
- When a web page opening holds Folio for half a second or more,
  `diagnostics.log` names the step that took the time.
- On upgrade, Folio updates the PSReadLine copy it installed earlier by
  itself; copies you installed are left alone.

### Fixed

- Typing with an input method no longer pauses while Folio moves the
  candidate window.
- Folio no longer pauses to ask Windows about the taskbar while you type.
- Typing no longer pauses while Folio asks Windows where its window is; it
  asks once a turn.
- A program that keeps changing its window title no longer slows typing.
- Typing in the find box stays quick in a pane with a long history; matches
  further back fill in over the next few frames.
- A launch with a chosen terminal font no longer waits for Folio to list
  every font on the machine.
- A font installed while Folio is running shows up in the font lists the next
  time Settings opens.
- A terminal font with no bold style no longer switches to another font's
  bold for bold text.
- Bold Chinese text no longer leaves single characters at regular weight.
- Keys, pastes, clicks and the wheel no longer reach what is under the
  "reopen your other tabs" card; Esc puts the card away, and the next launch
  asks again.
- The wheel no longer scrolls a pane under the other dialogs.
- Closing a window while it is still asking about unsaved changes no longer
  discards them.

## Known issues

- When the GPU or the display driver stalls, the picture can freeze for a
  moment while you type; the input itself keeps up, and the frozen picture
  catches up at once.
- In a profile that has never opened a web page, the first web page still
  takes a couple of seconds to appear; after that, and in profiles that have
  used web pages, it is warm.

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.5-windows-x64.zip` | the ten files that belong together, in one folder — `sha256:91c1a8bf5e0c09500c44aea2a78fe51258377486084b62027896ef45ceed2c98` |
| `folio-windows-x64.zip` | the same archive under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS.txt` | the hash of the archive under each of its two names and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.5.cdx.json` | the CycloneDX bill of materials for what is in the build |

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
Get-FileHash folio-0.4.5-windows-x64.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.5-macos-arm64.dmg` | the application, signed and notarized — `sha256:7052892ef8343085e4c7be8b46392089abc55956ade37458c9ad686e2ef79b53` |
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

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.5-preview/CHANGELOG.md#045-preview--2026-09-26) · [v0.4.4-preview…v0.4.5-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.4-preview...v0.4.5-preview)
