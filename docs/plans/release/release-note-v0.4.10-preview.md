> The body of the GitHub Release for this version, as published.

# Folio 0.4.10

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.10-preview/folio-0.4.10-windows-x64-unsigned.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.10-preview/Folio-0.4.10-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、发布声明、物料清单与源码。[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.10-preview/docs/plans/release/release-note-v0.4.10-preview.zh-CN.md)

This release also carries the work done for 0.4.9, which was not published.

**The Windows version is not signed, and Folio 0.4.8 on Windows cannot update
itself to it.** Install it by hand and allow it when Windows warns; the first
two Known issues say how.

## Highlights

- After a restart, a pane where Claude Code or Codex was running comes back
  with that agent resuming the same conversation, in the same folder and with
  the flags it was started with.
- Closing the last window keeps Folio running with its shells and agents, in
  the Windows notification area or the macOS Dock.
- On Windows, Folio can start when you sign in, comes back with its windows
  after Windows restarts for an update, and restores your tabs after a
  shutdown.
- On Windows, pasting a file copied inside a program, such as a mail attachment
  or a file in a zip folder, saves it and pastes its path.
- From this version on, Folio checks an update with its own release key, so a
  revoked certificate or a missing signature no longer stops an update.

## Changes

### Added

- Agent panes resume after a restart, on Windows after Folio or Windows
  restarts and on macOS after Folio restarts, with the same conversation,
  folder and start flags; Settings ▸ Agents ▸ Resume agents after restart
  turns it off.
- Closing the last window keeps Folio running: on Windows in the notification
  area, whose icon shows when an agent is working, has finished or is waiting
  for you; on macOS in the Dock. Settings ▸ General ▸ Keep running when closed
  turns it off.
- On Windows, Settings ▸ General ▸ Start at sign-in starts Folio when you sign
  in; it is off until you turn it on.
- On Windows, pasting a file copied inside a program saves it to Folio's
  clipboard folder and pastes its path, as a pasted screenshot is.
- On Windows, each terminal pane is a text control that Narrator and other
  accessibility tools can find, read and follow.
- Clicking a video plays or pauses it, double-clicking zooms its pane, the
  wheel changes its volume, the progress bar shows the time under the pointer,
  `Home` and `End` go to the start and end, `,` and `.` step one frame while
  paused, and an ended video plays again from the start.
- The arrow keys move around a zoomed picture, and middle-clicking a file in
  the files column opens it in a new pane beside the current one.
- Settings ▸ Agents says why an agent's hooks are not this Folio's, and which
  agents still need hooks for resuming.
- When this Folio cannot read a file a newer Folio saved, it says so once and
  keeps a copy, and the newer Folio offers to bring that copy back.
- On Windows, the update card says *Windows may warn about this update.* when
  the new version is not signed, and About ▸ Version says so of a copy that is
  not signed.

### Changed

- Folio checks an update with its own release key: each release carries a
  signed statement that lists every file with its checksum, Folio installs
  only what it lists, and an update it does not vouch for stops with *The
  update is not vouched for.*
- Quitting while an agent is working or a command is running asks first, on
  one card that says how many of each are running; signing out, restarting or
  shutting down does not wait for it.
- While a command runs in a tab, a window wakes about half as often and does
  less work for each step of the tab's mark; what you see is unchanged.
- Folio keeps the newest three copies of each file it could not read, and
  never removes the newest copy of a file a newer Folio saved.

### Fixed

- On Windows, tabs come back after a restart or a shutdown, and a shell the
  system ended keeps its pane with a note.
- On Windows, Folio comes back with its windows after an update restart or with
  Restart apps on, and closes when an installer asks it to.
- On Windows, Folio follows a contrast theme while one is on.
- On Windows, Folio's icon shows in the snap layouts, Alt+Tab, Task View, the
  taskbar's previews and its message boxes, and resting on the maximize button
  opens the snap layouts.
- On Windows, tools that follow the text cursor, such as Magnifier and the
  emoji panel, follow it on every keyboard layout.
- On Windows, one-finger swipes scroll again after a lost touch release, scroll
  a program that reads the mouse about as far as the finger moved, and no
  longer zoom a picture or change a video's volume.
- Input-method text can be typed into Settings and profile fields, and text
  begun in one place no longer lands in another after a click or a tab switch.
- Clicking in a name, the find bar, a search, a Settings field, the palette or
  a branch prompt puts the cursor where you clicked; Shift-click and dragging
  select, and the selection is shown.
- A picture or video that another program is holding open is no longer treated
  as a file Folio cannot show, and neither is a video whose player is slow to
  start.
- A PDF from an address that does not end in `.pdf` is no longer stretched on a
  scaled display.
- On Windows 10 without a graphics driver, Folio no longer closes at its first
  start or when Settings opens.
- The previous and next buttons beside a picture light up under the pointer.
- Turning an agent's hooks on or off, and uninstalling, change only Folio's
  own entries in the agent's settings file; hooks left pointing at a removed
  copy of Folio are pointed at this one.
- Codex 0.148 and later reach Folio through a Codex hook, so Folio works beside
  another program that uses Codex's notify line.
- A tab's blue dot marks an agent turn that finished while you were elsewhere,
  not every background agent tab.
- The first-run card no longer reads the agents' files while the window
  starts, and offers only hooks Folio can add.
- A pane's bell is announced at once, even while another pane prints fast.
- On Windows, an update no longer fails or shows its "not kept" card again
  because a program held one of its files for a moment, and update entries
  left by a deleted copy are removed.
- On macOS, the hidden folder an update leaves beside the app is removed once
  the app is gone.
- Pastes, copies and saves that finish together are handled in the order you
  made them.
- On Windows, a PowerShell profile line an older Folio wrote is brought up to
  date, and nothing else in the file changes.
- On Windows, a stopped uninstall no longer leaves its remover's copy behind.
- A Markdown page with a very long paragraph opens and takes keystrokes in
  time that grows with its length, not its square.
- A pane whose scrollback is full takes in new lines as fast as before it
  filled, instead of about twenty times slower.
- A resized pane no longer keeps a second copy of everything it prints, and a
  pane resized while its program keeps writing typesets its formulas again
  within a second.
- Started through an older winget's `folio` link, About ▸ Licences opens the
  notices that came with the copy.
- A second error while Folio closes after an internal error no longer leaves a
  window behind.

## Known issues

- **Folio 0.4.8 on Windows cannot update itself to this version.** The
  certificate that signed 0.4.8 was revoked after its release, and 0.4.8 checks
  its own signature before an update, so its card says *The update is not
  verified.* and nothing is changed. Download the zip from this page and unpack
  it over your Folio folder; your settings stay. A copy installed with Scoop
  takes it with `scoop update folio`. From this version on, updates are checked
  with Folio's own release key, so later versions arrive through About again.
- **This Windows version is not signed**, which the `-unsigned` in the
  archive's name says. Before the first start Windows warns: SmartScreen shows
  *Windows protected your PC* with an unknown publisher; choose **More info** ▸
  **Run anyway**. About ▸ Version says the copy is not signed. The release
  statement on this page vouches that the files are the ones published (see
  the install notes).
- Folio 0.4.7 and 0.4.6 on Windows cannot update themselves to this version
  either: they take only a signed archive, and their card says *Download
  stopped.* Nothing is changed; install by hand as above.
- With Smart App Control on, Windows does not start this version; keep the
  version you have.
- On Windows 11, Folio cannot be put on the first page of Explorer's menu with
  this version, because Windows registers only a signed package; an entry an
  earlier version added keeps working.
- Some antivirus programs hold or remove a new program that is not signed; if
  yours does, allow Folio's folder.
- Updating from 0.4.6 still uses 0.4.6's updater: on macOS, do not open Folio
  while the update is finishing, or 0.4.6 may come back although 0.4.10 works.
- macOS: the first self-update makes macOS show its "…software was added that
  can run in the background" notice for Folio's login item; it is the
  updater's recovery entry and is removed once the update finishes.
- A copy installed with Homebrew before 0.4.8 takes this update with
  `brew upgrade`; from then on it updates itself from About.
- Windows, with PowerShell 7 installed from the Microsoft Store: if Folio is
  ended from outside or crashes at the moment it starts that PowerShell to
  learn about it, the PowerShell can stay behind, paused, until it is ended in
  Task Manager. macOS: after a force quit or a crash, a command Folio had
  started to learn about the machine finishes on its own instead of being
  stopped.

<details>
<summary>Install notes (SmartScreen, Gatekeeper, checksums, the release statement)</summary>

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.10-windows-x64-unsigned.zip` | the ten files that belong together, in one folder — `sha256:<ZIP_SHA256>` |
| `folio-windows-x64.zip` | the same archive under a name that does not change from one release to the next — the same `sha256` |
| `SHA256SUMS.txt` | the hash of the archive under each of its two names and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.10.cdx.json` | the CycloneDX bill of materials for what is in the build |
| `RELEASE-STATEMENT.txt` | the release statement: the SHA-256 and size of every other asset and of every file in the zip |
| `RELEASE-STATEMENT.txt.sig` | the statement's Ed25519 signature, made with Folio's own release key |

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

`folio.exe` and `folio.msix` in this version are **not signed**. On the first
run SmartScreen raises **"Windows protected your PC"** and names no publisher:
**More info** ▸ **Run anyway** is the way through.

What shows that the files are the ones published is the release statement. It
is signed with Folio's own release key, the key every Folio from 0.4.10 on
checks an update against, and it lists the hash of every other asset and of every
file in the zip. How to check it with stock OpenSSL, and the public key, are in
[RELEASING.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.10-preview/docs/RELEASING.md#the-release-statement).

To check the download, with the zip and `SHA256SUMS.txt` in the same folder:

```powershell
Get-FileHash folio-0.4.10-windows-x64-unsigned.zip -Algorithm SHA256
```

or, where `sha256sum` is available — Git Bash, WSL, Linux:

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.10-macos-arm64.dmg` | the application, signed and notarized — `sha256:<DMG_SHA256>` |
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

Full changelog: [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.10-preview/CHANGELOG.md#0410-preview--2026-11-25) · [v0.4.8-preview…v0.4.10-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.8-preview...v0.4.10-preview)
