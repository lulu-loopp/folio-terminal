> The body of the GitHub Release for this version, as published.

# Folio <version>

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v<version>-preview/folio-<version>-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v<version>-preview/Folio-<version>-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：**
[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/v<version>-preview/docs/plans/release/release-note-v<version>-preview.zh-CN.md)

## Highlights

<!-- Three to five. One sentence each, about what a reader can now do. No file
     names, no flags, no crate names. -->

-
-
-

## Changes

<!-- One line per item. Anything that needs a paragraph gets the paragraph in
     CHANGELOG.md, and one line here. Drop a heading that has no items. -->

### Added

-

### Changed

-

### Fixed

-

## Known issues

<!-- Only if there are any. Short bullets, one line each. -->

-

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-<version>-windows-x64.zip` | the nine files that belong together, in one folder — `sha256:<hash>` |
| `SHA256SUMS.txt` | the hash of the archive and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-<version>.cdx.json` | the CycloneDX bill of materials for what is in the build |

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
Get-FileHash folio-<version>-windows-x64.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-<version>-macos-arm64.dmg` | the application, signed and notarized — `sha256:<hash>` |
| `SHA256SUMS-macos.txt` | the hash of the disk image, in the format `shasum -c` reads |

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

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v<version>-preview/CHANGELOG.md#<version anchor>) · [<previous tag>…v<version>-preview](https://github.com/lulu-loopp/folio-terminal/compare/<previous tag>...v<version>-preview)
