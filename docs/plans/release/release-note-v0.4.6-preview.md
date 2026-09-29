> The body of the GitHub Release for this version, as published.

# Folio 0.4.6

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.6-preview/folio-0.4.6-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.6-preview/Folio-0.4.6-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.6-preview/docs/plans/release/release-note-v0.4.6-preview.zh-CN.md)

## Highlights

- Folio updates itself on Windows: when a new release is out, a card offers
  it, downloads it and restarts Folio into it, and if the new version does
  not start, the previous one comes back.
- The same self-update works on a Mac. <!-- U-32 -->
- A reboot or a logout saves your layout the way quitting does, pinned tabs
  and their panes included.
- Pasting into the find bar, a settings field or the Git panel's search and
  branch prompt puts the text there, not into the terminal behind it.
- The Chinese interface is complete, and its settings read as a settings
  page, with one word for each thing on every page.

## Changes

### Added

- Windows: Folio can update itself from the update card; Settings ▸ General
  has a Restart to update row that brings the card back.
- macOS: Folio can update itself the same way. <!-- U-32 -->
- A copy installed with scoop or winget is shown its package manager's
  command to copy instead of updating itself.
- A copy installed with Homebrew is shown its command the same way. <!-- U-32 -->
- If an update cannot finish, the installed Folio reopens with an "Update
  incomplete" card; CHANGELOG says what happens when an update is cut off.
- An update downloaded but not installed — closed with Later, or cut off by
  a power loss — is offered again at the next start, ready to restart.

### Changed

- The Chinese settings descriptions are written statements and use one word
  for each thing across every page.
- The update card, the update row, the shortcut panel's zoom actions, the
  profile editor's login switch and the shutdown screen are translated into
  Chinese.
- When the settings gear shows its update dot, clicking it opens Settings at
  the update row; the About page names the newer version.
- The right-click menu is on by default only in installs that can remove it
  again.

### Fixed

- A reboot or logout saves your layout like a quit; pinned tabs come back
  with their panes.
- A web address right after Chinese text, a colon or an `=` is a link, and
  one followed by an opening bracket ends before the bracket.
- Pasting with a text field focused — the find bar, the Git graph's search,
  the branch prompt, a settings field — puts the text into that field.
- Renaming a file to a name that differs only in capitals no longer replaces
  a different file in a folder that tells capitals apart.
- Opening Folio from Explorer's right-click menu, or running
  `folio --version` while another Folio starts, no longer leaves the new
  window unable to save its settings and tabs.
- Pasting a picture works with every kind of bitmap the clipboard can hold,
  including browser copies and long screenshots.
- A web pane survives the browser engine updating itself underneath it.
- A redraw that arrives in several pieces no longer shows a formula's source
  for a frame.
- macOS: the shipped shells start as login shells, so Homebrew's tools are
  found; a profile can turn login on or off.
- macOS: a shell that reports its folder the way fish does gives the pane
  that folder.
- Markdown preview: `file:///Users/…` links open on macOS, and links with
  `%20` for a space find the file on both platforms.
- macOS: uninstall cleanup with `--purge` removes the clipboard staging
  folder and the panic log.
- A trace file that cannot be opened no longer stalls tracing.

## Known issues

- Ctrl+Enter and Shift+Enter still reach programs as Enter; the kitty
  keyboard protocol that tells them apart comes in 0.4.7
  ([#13](https://github.com/lulu-loopp/folio-terminal/issues/13)).
- If another program keeps Folio's update record open for minutes right
  after an update, the new version runs without saving its changes, and the
  next start puts the previous version back.
- macOS: the first self-update makes macOS show its "…software was added
  that can run in the background" notice for Folio's login item; it is
  the updater's recovery entry and is removed once the update finishes. <!-- U-32 -->

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.6-windows-x64.zip` | the ten files that belong together, in one folder — `sha256:<!-- checksums -->` |
| `folio-windows-x64.zip` | the same archive under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS.txt` | the hash of the archive under each of its two names and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.6.cdx.json` | the CycloneDX bill of materials for what is in the build |

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
Get-FileHash folio-0.4.6-windows-x64.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.6-macos-arm64.dmg` | the application, signed and notarized — `sha256:<!-- checksums -->` |
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
the network: a page you open in the web preview, and the update check with the
updates it downloads, which Settings ▸ General ▸ **Update check** switches off.

</details>

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.6-preview/CHANGELOG.md#046-preview--2026-09-29) · [v0.4.5-preview…v0.4.6-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.5-preview...v0.4.6-preview)
