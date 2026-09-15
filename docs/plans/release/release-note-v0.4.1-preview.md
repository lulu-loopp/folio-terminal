> The body of the GitHub Release for this version, as published.

# Folio 0.4.1

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.1-preview/folio-0.4.1-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.1-preview/Folio-0.4.1-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：**
[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/v<version>-preview/docs/plans/release/release-note-v<version>-preview.zh-CN.md)

## Highlights

- Paste a file you copied in Explorer or Finder and it arrives as one quoted
  argument, spelled the way the shell in that pane spells a path.
- Text that arrives with no keystroke behind it — from a phone keyboard, from
  dictation, from a program that types on your behalf — now lands wherever you
  are typing.
- Settings opens the moment you click the gear, however many fonts are
  installed, and a new About page says which Folio this is and where to read the
  notes, report a defect or read the licences.
- A typeset formula now stands in a block with room around it, and turning it
  between the picture and its source grows or shrinks into place instead of
  jumping.
- On a Mac, Folio quits from an empty desk, a shortcut in the files column opens
  what it points at instead of running it, and a previewed page is only called
  guarded when it really is.

## Changes

### Added

- **Settings has an About page**, carrying the version and the build this copy
  came from, the system it was made for, and rows that open the release notes,
  the place a defect is filed, and the licences of everything Folio is made of.
- **A copied file or path pastes as one quoted argument**, spelled the way the
  shell in that pane spells one.

### Changed

- **A typeset formula sits in a block with room around it** — blank lines above
  and below, a clear column on each side, and its two marks inside it at the
  right edge.
- **Switching a formula between its typeset and source forms animates** instead
  of jumping, and still happens in a single frame if you have asked your system
  to reduce motion.
- **The release page also carries each download under a fixed name**, so a link
  to the latest build never goes stale.
- **Both checksum files can be checked in the folder you downloaded into**,
  without anything being edited first.
- **Two files moved out of the top of the repository**; a fork that names one by
  path updates the path.
- **The gates that compile now all compile the same thing**, so a script run
  after a green gate has nothing left to build.

### Fixed

- **Text sent from a phone keyboard, or by a program that types for you, now
  reaches the terminal**, and lands wherever you are typing.
- **Opening Settings no longer freezes the window** on a machine with a lot of
  fonts installed.
- **Copying a formula no longer leaves the window busy** for as long as it stays
  open.
- **Turning a formula into its source no longer hesitates** when there is a long
  history behind you.
- **A formula whose macros expand without end is refused** instead of exhausting
  memory.
- **A formula's two marks stay with the formula** when you switch tabs, close a
  pane, or point at a pane that is not the focused one.
- **Inner products and bra-kets written with angle brackets now typeset.**
- **Aiming a card's window with the wheel keeps up with the hand** over a pane
  with a long history.
- **On a Mac, Folio can be quit with no window open.**
- **On a Mac, opening a shortcut from the files column opens what it points at**,
  and refuses one that leads to a program.
- **On a Mac, a previewed page is only reported as guarded when its own rules
  are on it.**
- **On a Mac, Folio carries the two licences, the third-party notices and the
  trademark notice inside the application.**

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.1-windows-x64.zip` | the nine files that belong together, in one folder — `sha256:<hash>` |
| `folio-windows-x64.zip` | the same archive under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS.txt` | the hash of the archive under each of its two names and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.1.cdx.json` | the CycloneDX bill of materials for what is in the build |

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
Get-FileHash folio-0.4.1-windows-x64.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.1-macos-arm64.dmg` | the application, signed and notarized — `sha256:<hash>` |
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

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.1-preview/CHANGELOG.md#041-preview--2026-09-16) · [v0.4.0-preview…v0.4.1-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.0-preview...v0.4.1-preview)
