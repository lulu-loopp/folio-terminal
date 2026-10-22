> The body of the GitHub Release for this version, as published.

# Folio 0.4.8

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.8-preview/folio-0.4.8-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.8-preview/Folio-0.4.8-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.8-preview/docs/plans/release/release-note-v0.4.8-preview.zh-CN.md)

## Highlights

- Shutting down, restarting or signing out of Windows no longer loses an
  unsaved edit in a preview, and quitting saves your settings, shortcuts,
  profiles and pins first.
- An update rides out an antivirus or another program that holds one of
  Folio's files for a moment, and names the file if it stays held.
- A PDF in a preview pane reads cleanly on a display scaled to 125 %, 150 % or
  175 %.
- A Folio installed with Homebrew updates itself from About.
- Windows PowerShell panes keep a long command in one piece when they are
  resized, with nothing to install.

## Changes

### Added

- The file search in the palette is kept current in the background, so it is
  ready when the palette opens.
- Programs installed while Folio runs appear in + and the default profile
  without opening a menu.
- On macOS, a drive that mounts is read where a folder was waiting for it, and
  a changed system language reaches new panes.
- A Folio installed with Homebrew updates itself in place from About; Homebrew
  keeps managing and uninstalling it.
- After a graphics failure, the next start says where your unsaved edit was
  kept, with a button that opens the folder.

### Changed

- Windows PowerShell panes use Folio's own copy of PSReadLine 2.4.5; nothing is
  installed for your account, and other terminals keep the PSReadLine they have.
- The PSReadLine row in Settings and the card that offered to install it are
  gone; a copy an earlier Folio put in Documents can be removed from Settings ▸
  Terminal.
- Settings, shortcuts, profiles and pins saved in another editor take effect
  while Folio runs, and one card names what came from the file.
- Quitting first saves any change to settings, shortcuts, profiles or pins not
  yet on disk; if one cannot be saved, Folio stays open and names the file.
- The formula's right-click menu and the picture filter in the Windows file
  chooser follow Folio's interface language.
- The window appears before its shells have started, and what you type is kept
  until the shell is ready.
- A long address in a web pane folds the middle of its path and keeps the site.
- The first formula or picture after Folio starts takes a moment longer,
  because the formula engine loads when it is first needed.
- A preview on a drive that cannot replace a file in one step, such as some USB
  sticks, is saved through a copy beside the file, and the first save there
  says so.
- A copy installed with winget still updates with `winget upgrade`; the update
  card is for zip, scoop and Homebrew copies.

### Fixed

- A PDF previewed on a display scaled to 125 %, 150 % or 175 % no longer shows
  letters of uneven weight and words with gaps; there the PDF viewer's own
  controls are drawn larger, and its toolbar is shortened, or not shown, in
  narrower panes. A pane resized after a PDF is opened does not re-fit the page
  to its new width until the PDF is opened again, as in earlier versions.
- Unsaved preview edits survive Windows shutting down, restarting or signing
  out; what could not be written in time is kept in the recovered folder, which
  the next start offers.
- Save on the quit card no longer freezes the windows, and closing right after
  a save waits for the save instead of asking to discard it.
- On Windows, an update is no longer rolled back or left unfinished because a
  program held one of Folio's files for a moment: Folio waits up to ten seconds
  for each file.
- When an update could not be finished and Folio opens from the copy it keeps,
  that window says the update is not finished and names its folder, and is
  never offered an update.
- Update cards say what happened — the update stopped before the new version
  started, or did not finish while it runs — and when changes made in the new
  version were not kept.
- After Restart to update, Folio always comes back, and a downloaded update
  survives a power cut or a crash.
- An older copy of Folio that finds an update started by a newer one leaves it
  to the newer one and says so.
- When another program holds the clipboard, copying and pasting no longer
  freeze the window, and two quick pastes arrive in order.
- Markdown preview and the terminal read code fences the same way, as CommonMark
  defines them: `~~~` and indented fences are code, and a shorter fence no
  longer ends a block.
- A drag, selection or scroll thumb you hold ends when what it was about goes
  away; while you hold it, a second button, ⌄ menus, tooltips, hover previews
  and the web page under the pointer do nothing.
- A window no longer keeps a processor busy after a timer it waited for has run
  out, which under a heavy load left the terminal slow to draw.
- On Windows, Esc reaches programs every time, and typing with an input method
  shows what you compose in programs that hide the terminal's cursor.
- Right after Windows starts, Folio no longer closes when its PowerShell 7 tabs
  come from the Microsoft Store, and a shell that stops at startup keeps its
  pane.
- Folio started through the link winget installs runs its panes on the console
  host that comes with it.
- On a Mac with two copies of Folio, opening the second joins the one already
  running.
- On the Git page, each repository's actions run in order and never wait on
  another repository.
- Paths printed by Cygwin and WSL shells are links, and a path pasted into
  Cygwin is written the way Cygwin reads it.
- In a web preview, a page that answers 404 shows what the server sent, the
  address you asked for stays in the bar, and a link that asks for a new window
  opens beside the page.
- A link or page the system cannot open says why: no program for it, not found,
  or access denied.
- A change to settings, shortcuts, profiles or pins that cannot be saved is
  reported while Folio runs, and a fault while saving or starting a shell no
  longer closes Folio.
- Folio no longer starts a helper program that it could not stop again.

## Known issues

- Updating from 0.4.7 or 0.4.6 still uses that version's updater: a program
  that holds Folio's update record or one of its files for a few seconds can
  still roll the update back, and a window that opens from the kept copy shows
  no card. Updates from 0.4.8 on use 0.4.8's updater, which handles both.
- Updating from 0.4.6 still uses 0.4.6's updater: on macOS, do not open Folio
  while the update is finishing, or 0.4.6 may come back although 0.4.8 works.
- macOS: the first self-update makes macOS show its "…software was added that
  can run in the background" notice for Folio's login item; it is the
  updater's recovery entry and is removed once the update finishes.
- A copy installed with Homebrew before 0.4.8 takes this update with
  `brew upgrade`; from then on it updates itself from About.
- A PDF whose address does not end in `.pdf` is still drawn stretched on a
  scaled display.
- Windows, with PowerShell 7 installed from the Microsoft Store: if Folio is
  ended from outside or crashes at the moment it starts that PowerShell to
  learn about it, the PowerShell can stay behind, paused, until it is ended in
  Task Manager. macOS: after a force quit or a crash, a command Folio had
  started to learn about the machine finishes on its own instead of being
  stopped.

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.8-windows-x64.zip` | the ten files that belong together, in one folder — `sha256:<ZIP_SHA256>` |
| `folio-windows-x64.zip` | the same archive under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS.txt` | the hash of the archive under each of its two names and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.8.cdx.json` | the CycloneDX bill of materials for what is in the build |

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
Get-FileHash folio-0.4.8-windows-x64.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.8-macos-arm64.dmg` | the application, signed and notarized — `sha256:<DMG_SHA256>` |
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
updates it downloads. About ▸ Version ▸ **Automatic check** switches off the
daily check; pressing Check there still asks.

</details>

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.8-preview/CHANGELOG.md#048-preview--2026-10-23) · [v0.4.7-preview…v0.4.8-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.7-preview...v0.4.8-preview)
