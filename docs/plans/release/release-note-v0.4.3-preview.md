> The body of the GitHub Release for this version, as published.

# Folio 0.4.3

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.3-preview/folio-0.4.3-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.3-preview/Folio-0.4.3-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：** 上方 zip 与 dmg 即为完整下载,其余为校验和、物料清单与源码。[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.3-preview.zh-CN.md)

## Highlights

- Folio no longer reads its own PowerShell module off the disk for the whole of
  a session — 0.4.2 does it too, and restarting was the only way to stop it.
- Chinese in the terminal is drawn in a family you choose, and on Windows the
  one Folio now asks for by name is NSimSun.
- One command takes everything Folio wrote outside its own folder back off the
  machine, and on Windows a double-click does the same.
- Saving a document in the preview editor keeps what the file was carrying — a
  download's mark, its permissions, its times — as well as replacing its bytes.
- A page or a PDF under a path with Chinese in it opens in the preview instead
  of being turned away.

## Changes

### Added

- **`folio --uninstall-cleanup`** undoes what Folio wrote outside its own
  folder — the `$PROFILE` line, the agent hooks, the Explorer entries, the
  PSReadLine module — without opening a window, and reports each result;
  `--purge` also deletes settings, sessions and browser data.
- **`uninstall.cmd`**, beside `folio.exe` in the Windows archive, is that same
  command by double-click.
- Settings ▸ Terminal ▸ **Chinese font**: *Automatic*, or any installed Chinese,
  Japanese or Korean family, without changing the monospace face used for ASCII.
- `folio --remove-shell-integration` takes Folio's line out of your PowerShell
  `$PROFILE`, without a window.
- `folio --remove-explorer-menu` takes Folio out of Explorer's right-click menu,
  both entries, without a window.
- A repair for the row breaks a coding agent's own redraw eats, so a matrix
  arrives as a matrix; it never changes what copying the formula gives you.
- `docs/recovery-after-deleting-folio.md`, for anyone who deleted an older Folio
  by hand.

### Changed

- A save in the preview editor keeps the file's streams, permissions,
  attributes and times; content is guaranteed and the rest is best effort, and
  the changelog states each limit — a hard-linked file's second name still reads
  the old bytes, and a crash between the two halves of the replacement can leave
  the old document beside the new one.
- Folio installs its PSReadLine module only where the place is empty or its own,
  and removes only the files it wrote. **0.4.2 and every version before it could
  write over a PSReadLine you had installed yourself**; the changelog says how to
  tell whether that happened to you, and what to do about it.
- Folio's `$PROFILE` line is one guarded form, silent when the script is
  missing, and existing installs are rewritten at start-up.
- An agent hook is owned by the `folio.exe` it names: Folio touches its own and
  dead ones, leaves another live copy's alone, and asks before taking one over.
  Updating hooks may reformat that agent's JSON settings; a dated backup is kept.
- The offer to switch the PowerShell integration on asks whether you already
  have it, not whether Folio wrote it.
- On Windows the terminal's Chinese family is NSimSun by default and never
  changes with the weight asked for; proportional text keeps its own chain and
  names it.
- The window thread never asks a filesystem about a path a program printed.
- A stall line names the call that took the time and carries the window thread's
  own CPU time beside the wall time.
- `diagnostics.log` names the GPU Folio asked for beside the one it got, and
  `BT_GPU_PREFERENCE=low` asks a two-GPU laptop for the integrated adapter for
  one run.
- Three diagnostics lines are always on — a window that has shown no new picture
  for over a second, a minute of heavy reading with no input from you, and a
  broken input-method composition — and none of them carries anything you typed.
- `BT_IME_TRACE` no longer records composed or committed text at all; older
  builds did.

### Fixed

- Folio no longer re-reads its installed PSReadLine module on every turn of the
  event loop once a PowerShell pane has reported its version or the Terminal
  settings page has been opened. **Present in 0.4.2, where restarting was the
  only relief**; the changelog has the whole account.
- A frame a program held inside a synchronized update is no longer lost when the
  window is resized, and ending such a block keeps the escape sequence it
  interrupted.
- The key that redraws your prompt is sent only to a prompt the shell opened in
  order, so output carrying the marks a shell prints is no longer typed into
  whatever was really running.
- Hovering or clicking a printed path makes no filesystem call on the window
  thread, so a path on a slow or disconnected share no longer freezes the
  window; a name that is gone says "not found"; a name the disk refused is asked
  about again when the program prints it again; and a link under a resting
  pointer answers the first click.
- The Settings page is laid out when its content changes, not when the pointer
  moves.
- A formula's two marks are placed from the frame being drawn, and stop moving
  when the block lands.
- A focused pane is no longer sent an opening focus report, which arrived at the
  prompt as `^[[I`.
- A waiting card's halo keeps its whole outset at the edge of the list, and the
  dot on it pulses.
- A zoomed pane's name no longer sits under the zoom mark, and a preview pane too
  narrow for its switcher neither draws it nor takes the keyboard.
- A bold Chinese cell keeps the chosen family; an italic request on an
  upright-only family stays italic.
- Chinese terminal text is named rather than found: on Windows the terminal asks
  for NSimSun, and proportional text no longer falls through to SimSun at a
  medium weight (#10).
- A row end inside `\text{…}`, a line end inside `\begin{equation}` and a nested
  `\begin{array}{cc}` are repaired correctly.
- A local `file:` address is read as the file it names, so a page or PDF under a
  path holding Chinese opens (#7).
- Pasting a screenshot copies one shape of the picture instead of all of them,
  and a clipboard picture's shape is checked before it is decoded.
- macOS: a second Folio started at the same instant no longer keeps a window
  whose session is discarded, and a preview goes to the last address it was
  given.
- An idle window lets the event loop sleep.
- A PowerShell script is highlighted in the preview.
- `diagnostics.log` always opens with the line naming the build that wrote it.

## Known issues

- Some Chinese input methods can lose a word mid-composition when the machine
  hitches; the cause is identified, it is **not fixed in this release**, and
  `diagnostics.log` now names the moment it happens.
- When the desktop compositor stalls, typing stalls with it for as long as the
  compositor takes. **Not fixed in this release.**
- Windows: handing a file to its default program with Ctrl+click holds the
  window for about a second while Windows starts it.
- A printed path with a space in it is a link only when the program quoted it.
- Bold Chinese in the terminal is drawn at regular weight when the chosen family
  has no bold cut — NSimSun, the Windows default, has none.
- A drawn table comes down when its header row sits just above the viewport;
  scroll the heading back into view and it is drawn again.
- The note the web preview shows after answering a page's message box is hidden
  behind the page, which is answered and goes on running.
- A window saved on a monitor that enumerates late comes back on the primary
  display.
- Windows: Folio cannot be a panel inside Visual Studio Code; `folio-here.cmd`
  in the archive makes it the external terminal VS Code opens instead.
- Windows: `.webm` needs the VP9 or AV1 Video Extension from the Microsoft
  Store; without one there is no still and no playback.
- macOS: a run that ends in a crash leaves its report where the system puts every
  one, and the next Folio names the file in its own log rather than copying it
  anywhere.

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.3-windows-x64.zip` | the ten files that belong together, in one folder — `sha256:3ed45c648304be9734ad6130a5075c3a63ede8d6dd5cbb487774fe85cb45cf19` |
| `folio-windows-x64.zip` | the same archive under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS.txt` | the hash of the archive under each of its two names and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.3.cdx.json` | the CycloneDX bill of materials for what is in the build |

Unpack the zip wherever you keep programs and run `folio.exe`. There is no
installer; keep the extracted files together in one folder, and unpacking over an
older folder keeps your settings. Needs **Windows 10 1809 or newer, or Windows
11, 64-bit**. The web preview needs the **WebView2 Runtime**, which Windows 11
has and Windows 10 usually does.

Or, with [Scoop](https://scoop.sh) — asked for as issue #9:

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
Get-FileHash folio-0.4.3-windows-x64.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.3-macos-arm64.dmg` | the application, signed and notarized — `sha256:d8554e78cda496569ef93bd9d3bf5e4ca83279ba9cf9bb929f0386fabdde2080` |
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

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.3-preview/CHANGELOG.md#043-preview--2026-09-21) · [v0.4.2-preview…v0.4.3-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.2-preview...v0.4.3-preview)
