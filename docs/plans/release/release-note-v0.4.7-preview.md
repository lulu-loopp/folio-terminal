> The body of the GitHub Release for this version, as published.

# Folio 0.4.7

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.7-preview/folio-0.4.7-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.7-preview/Folio-0.4.7-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.7-preview.zh-CN.md)

## Highlights

- Programs that ask for it — Claude Code, Codex, neovim, fish, helix — can
  tell Ctrl+Enter, Shift+Enter and Alt+Enter apart from Enter, and Shift+Tab
  and Esc from Tab and a lone escape.
- PowerShell panes have command marks, folder tracking and inline formulas
  with no setup.
- A Folio left running finds a new version within a day, and About ▸ Version
  offers it.
- Settings has an Uninstall row that removes Folio in one step.
- A command you install while Folio is open is found in a new tab, without
  restarting Folio.

## Changes

### Added

- The kitty keyboard protocol (first tier) and xterm's modifyOtherKeys, for
  the programs that ask for them, such as Claude Code, Codex and neovim.
- On Windows, Ctrl+Enter, Shift+Enter and Alt+Enter reach console programs
  such as Codex as those keys.
- A PowerShell profile that cannot be set up automatically can be set up
  through `$PROFILE` in one click from Settings, with Undo.
- Settings ▸ Uninstall; the zip's uninstall script also removes Folio in one
  step and speaks your language. Settings and data stay unless you ask.
- In an image or video preview, ‹ › and the arrow keys step to the previous
  or next file of the same kind.
- Pane menu ▸ Reset terminal modes undoes the modes a program left behind.
- In Claude Code, `[Image #N]` in the input line is a link to the pasted
  picture.
- Formulas inside tmux, screen, zellij or herdr panes are typeset per pane.

### Changed

- PowerShell profiles that run a command at startup, such as Developer
  PowerShell or a conda environment, keep that command and are integrated
  too.
- Folio's PowerShell script does nothing in another terminal that reads the
  same `$PROFILE`.
- A new pane picks up environment changes made after Folio started.
- The new-version notice, the update button and the Automatic check switch
  are on About ▸ Version.
- In PowerShell, Shift+Enter adds a line and Ctrl+Enter inserts one above, as
  in Windows Terminal; Enter runs the command.
- F1–F12 reach the program, with Shift, Alt and Ctrl encoded as in xterm.
- The README says how to uninstall each kind of copy.
- The outline folder icon has the filled folder's shape.

### Fixed

- Two Folio windows can no longer both start the same update.
- When an update fails while Folio is open, the open Folio says so.
- On macOS, opening Folio while an update is finishing no longer undoes a new
  version that works.
- On Windows, when the system will start neither the new version nor the
  previous one, Folio still opens the new version.
- An update whose new version could not record its start is finished at the
  next start or logon instead of losing what the new version changed.
- Update messages are more exact: an interrupted update, a release that needs
  a newer Folio, a file another program holds, a previous version that was or
  was not restored.
- Commands Folio runs to learn about the machine no longer leave processes
  behind.
- Restart shell, Duplicate and the splits start in the folder the pane was
  opened in.
- On Windows, Ctrl+Alt with a letter or digit reaches programs that asked for
  the new keyboard modes.
- Links: an address without `https://` is found after a colon or an `=`, a
  link a program sends ends before Chinese text, and file links decode the
  same way everywhere.
- macOS file names are no longer refused for Windows-only rules.
- Paths pasted into csh and tcsh are quoted correctly.
- Chinese font choices keep their order.
- Pointing at a pane's notice strip, or at a title-bar control that is cut off,
  no longer answers what lies under or beside it.
- On macOS, Folio no longer leaves an empty lock file behind for each data
  folder.

## Known issues

- Updating from 0.4.6 still uses 0.4.6's updater: on macOS, do not open Folio
  while the update is finishing, or 0.4.6 may come back although 0.4.7 works.
- Updating from 0.4.6: if another program keeps Folio's update record open
  for minutes right after the update, 0.4.7 runs without saving what it
  changes until the next start or logon finishes the update.
- Updating from 0.4.6: if the update fails while another 0.4.6 window is open,
  that window does not say so.
- macOS: the first self-update makes macOS show its "…software was added that
  can run in the background" notice for Folio's login item; it is the
  updater's recovery entry and is removed once the update finishes.
- Rarely, after an update, no Folio window opens if the disk failed at that
  moment and the old Folio was slow to close; starting Folio again finishes
  the update.
- If a program that turned on the new keyboard modes — Claude Code, Codex or
  neovim, for example — is killed or crashes, keys typed in that pane can
  arrive as text such as `[99;5u`; pane menu ▸ Reset terminal modes puts the
  pane right.
- On Windows, tabs restored at launch whose profile runs a startup command in
  PowerShell (such as Developer PowerShell or a conda environment) start
  without command marks and folder tracking; a tab of that profile opened once
  Folio is running has them.
- Rarely, Folio's first window waits a moment at launch while it looks up the
  programs installed on the machine.

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.7-windows-x64.zip` | the ten files that belong together, in one folder — `sha256:<ZIP_SHA256>` |
| `folio-windows-x64.zip` | the same archive under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS.txt` | the hash of the archive under each of its two names and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.7.cdx.json` | the CycloneDX bill of materials for what is in the build |

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
Get-FileHash folio-0.4.7-windows-x64.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.7-macos-arm64.dmg` | the application, signed and notarized — `sha256:<DMG_SHA256>` |
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

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.7-preview/CHANGELOG.md#047-preview--2026-10-DD) · [v0.4.6-preview…v0.4.7-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.6-preview...v0.4.7-preview)
