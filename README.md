# Folio

A terminal for Windows, written in Rust and drawn on the GPU, with a files
column, a document preview, a git panel and a way for command-line agents to say
they are waiting for you.

[中文说明](README.zh-CN.md) · [Shortcuts](docs/shortcuts.md) ·
[Security](SECURITY.md) · [Changes](CHANGELOG.md)

> **Status: preview.** Version 0.1.0 is the first public build. It is not signed —
> see [SmartScreen](#smartscreen) below.

<!-- Screenshots are taken on a real machine and committed under docs/screenshots/.
     docs/screenshots/README.md is the list of shots and how to take them. -->

![The Folio window: a files column on the left, two terminal panes side by side,
and a markdown document open in a preview pane on the
right.](docs/screenshots/main-window-light.png)

---

## What is in it

### A terminal

Panes split horizontally and vertically, tabs carry them, and a tab or a pane can
be dragged out into a window of its own or dropped into another one. Text is
shaped and drawn on the GPU, ligatures and colour emoji included, and each pane
keeps its own scrollback.

Shells come from profiles: PowerShell 7, Windows PowerShell, WSL, Git Bash and
Command Prompt ship as five entries, and you can add your own with their own
command line, environment and colours.

### Command marks

Folio knows where each prompt, each command and each command's output begin and
end in a shell that says so. `Ctrl+Shift+↑` and `Ctrl+Shift+↓` step between
commands, and a command that failed is marked as having failed.

Git Bash and WSL are set up for this automatically: the script is named for one
interactive shell on the command line and nothing on disk is touched. PowerShell
has no equivalent, so Folio offers to add one line to your `$PROFILE` the first
time it sees a PowerShell pane, and adds nothing unless you accept.

### A files column

A column beside the terminal shows the folder a pane is in, follows that pane as
it changes directory, and watches the folders it is showing so a file written by
a command appears without a refresh. Folders and files can be pinned, opened,
renamed, revealed in Explorer, or opened as a new pane rooted there.

### A preview

![A hover card over a file name in the files column, showing the first page of a
PDF above its page count and size.](docs/screenshots/preview-pdf-light.png)

A file opened from the files column, from a path printed in the terminal, or from
the command line opens beside the terminal rather than in another application:

- **Markdown**, typeset — headings, tables, code, and `$…$` mathematics set by the
  same engine that draws formulas in the terminal.
- **PDF**, page by page.
- **Images**, at their own pixels or fitted.
- **Video** — `.mp4`, `.m4v` and `.webm` show their first frame, their length and
  their size. They do not play yet.
- **Web pages**, local or remote, hosted by the WebView2 engine Windows provides,
  with an address field, a back stack and a source view for local pages.
- **Anything else**, as text.

Hovering a file name in the files column shows the same thing as a small card
before you open anything.

### Git

The files column turns into a git panel for a repository: the branch, the working
tree, staged and unstaged files, and the commit graph, with the diff of a
selected file in the preview.

### Agents

![The Agents page in Settings, with three installer rows for Claude Code, Codex
and Copilot CLI.](docs/screenshots/settings-agents-light.png)

A command-line agent that has stopped to ask you something can mark its own pane,
so a window full of panes says which one is waiting. Claude Code, Codex and
GitHub Copilot CLI each have a switch on the Agents page in Settings that installs
one notification hook into that tool's own configuration file, and takes it back
out again when you switch it off. Nothing is installed unless you press the row.
Programs that write the terminal's own attention sequences are heard without any
of that.

### Cards

`Ctrl+Shift+Z` lays the tab's panes out as cards, at the size they would be on
their own, so a window of eight panes can be read at once. The same key puts
them back.

### Settings

One dialog, in English or Chinese, for the font, the colour scheme, the cursor,
the scrollback, transparency, minimum contrast, what the preview renders, the
shell profiles, and every shortcut key. Colour schemes are files in a folder you
can add to.

---

## Download and install

1. Take `folio-0.1.0-windows-x64.zip` from the releases page.
   <!-- Link filled in when the first release is tagged. -->
2. Unpack it wherever you keep programs. There is no installer and nothing is
   written outside the folder you unpacked into until you run it.
3. Run `folio.exe`.

The archive holds `folio.exe`, the two ConPTY files it starts shells with
(`conpty.dll` and `OpenConsole.exe`), this README, both licence texts and the
third-party notices. `SHA256SUMS.txt` beside the archive is the hash of what you
downloaded.

**Windows 10 version 1809 or newer, or Windows 11, 64-bit.**

**WebView2 Runtime** is needed for the web preview and for previewing local HTML.
Windows 11 has it. On Windows 10 it is often already there; if it is not, install
the Evergreen Runtime from
<https://developer.microsoft.com/microsoft-edge/webview2/>. Without it everything
except the web preview works, and the preview says what is missing rather than
failing silently.

**Visual C++ Runtime:** to be confirmed before release.
<!-- Filled in when the statically linked C runtime is measured on a clean
machine: either "not needed, the runtime is linked in" or a link to the
Microsoft redistributable. Do not guess this one. -->

---

## SmartScreen

Folio 0.1.0 is **not code-signed**. Signing in 2026 no longer buys a first run
without a warning — Microsoft removed that behaviour from EV certificates in 2024
— and the open-source signing programme this project intends to apply to requires
a release to have happened first. So the first release is unsigned, and this is
what that looks like.

A file downloaded with a browser carries a mark that says it came from the
internet, and Windows Explorer copies that mark onto the files it unpacks. When
you run `folio.exe` you may therefore see a dialog titled **"Windows protected
your PC"**, with one button reading **"Don't run"**.

To run it anyway: click **"More info"**, check that the app is `folio.exe` — the
publisher line will read "Unknown publisher", which is what unsigned means — and
click **"Run anyway"**.

Check the archive against `SHA256SUMS.txt` first if you want to know that what
you have is what was published. Do not disable SmartScreen for this, and do not
take advice that involves stripping the mark off downloaded files: the mark is
doing its job here.

---

## First run

**The window** opens at 960 × 600, at whatever corner Windows places a new window
at. It is not centred. Move and size it once and the size and position are
remembered per window.

**The first shell.** Folio ships five profiles and looks for each one in this
order: PowerShell 7 (`pwsh`), Windows PowerShell (`winps`), WSL (`wsl`), Git Bash
(`gitbash`), Command Prompt (`cmd`). A profile whose program is not on the machine
is greyed out in the picker rather than hidden. Until you choose a default in
Settings, the first tab opens **Windows PowerShell**, which is the one profile
Windows always has.

**The PowerShell integration offer.** The first time a PowerShell pane prints
something, a strip appears across the pane:

> PowerShell integration is not installed. Folio uses it for command marks and
> status.

- **Add to `$PROFILE`** appends one line to your current-host PowerShell profile —
  `. "$env:APPDATA\Folio\shell-integration\folio.ps1"` — after copying the file as
  it stood to `<profile>.bak-<date>` beside itself. The script it points at is
  written to `%APPDATA%\Folio\shell-integration\`. To undo it, delete that line;
  nothing else in the file is touched.
- **Don't show again** switches the offer off for good.
- Closing the strip decides nothing, so the next PowerShell asks again.

**Agent hooks are not installed.** The three switches on the Agents page are not
defaults that happen to be off — each one reads the tool's own configuration file
and reports what is in it. On a new machine all three files are absent, so all
three read Off, and Folio never writes one on its own. Pressing a row writes it
immediately, keeping a dated copy of the file first; pressing it again removes
exactly what was written. `SECURITY.md` lists the three files and what is put in
them.

---

## Shortcuts

Forty rows, every one of them changeable on the Shortcuts page in Settings.
**[The full table is in `docs/shortcuts.md`](docs/shortcuts.md)**, in both
languages, written from the source rather than by hand.

The ones worth knowing first:

| Key | What it does |
| --- | --- |
| `Ctrl+Shift+N` | New tab |
| `Ctrl+Shift+W` | Close pane |
| `Alt+Shift+-` / `Alt+Shift+=` | Split horizontally / Split vertically |
| `Ctrl+Tab` | Next tab |
| `Ctrl+Shift+B` | Files column |
| `Ctrl+Shift+G` | Turn the files column to Git |
| `Ctrl+F` | Find in this pane |
| `Ctrl+Shift+Z` | Cards |
| `Ctrl+,` | Settings |

---

## Privacy

Folio sends nothing anywhere. There is no telemetry, no analytics, no crash
reporting, no update check, and no network client of its own. The only thing that
reaches the network is a page you open in the web preview, fetched by the WebView2
engine that Windows provides.

Everything Folio remembers is on your machine, in two directories.

### `%APPDATA%\Folio` — settings and session

Roaming configuration. Delete it and Folio starts as it did the first time.

| File | What it holds |
| --- | --- |
| `settings.json` | Your settings. |
| `keybindings.json` | Shortcuts you changed. Written only once you change one. |
| `profiles.json` | Shell profiles, including any command line and environment you set. |
| `schemes\` | Colour schemes you added. |
| `session.json`, `session.lock` | The windows, tabs and panes to restore. See below. |
| `pins.json` | Pinned folders, files and addresses. |
| `shell-integration\` | The scripts Folio writes for PowerShell and bash integration. |
| `diagnostics.log`, `diagnostics.prev.log` | Program output for a run started without a console. Checked once at startup: at 4 MiB the current log becomes `.prev.log`, replacing the older one. |
| `hang-reports\` | Written only when the window stops answering. Module names and offsets, not stack contents. |

```powershell
# Everything Folio remembers about your settings and session
Remove-Item -Recurse -Force "$env:APPDATA\Folio"

# Just the diagnostics
Remove-Item -Force "$env:APPDATA\Folio\diagnostics*.log", "$env:APPDATA\Folio\hang-reports" -Recurse -ErrorAction SilentlyContinue
```

### `%LOCALAPPDATA%\Folio\WebView2` — the web preview's profile

A separate directory, and not the one above. It is the WebView2 engine's own
profile for the preview: cookies, local storage, the disk cache, and the profile
directory autofill would use. It is local rather than roaming so that a cache and
a cookie jar do not travel between machines. Folio does not delete it.

Autofill and password saving are switched **off** in this profile — not left at
the engine's defaults — so a form you fill in a previewed page is not saved into
it. Cookies and cache still are, as they are in any browser.

```powershell
# Clear the preview's cookies, storage and cache. Close Folio first.
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

### What is in `session.json` and `pins.json`

Both are plain text, unencrypted, and readable by anything running as you. That is
the same footing as your shell history, and worth knowing because of what is in
them:

- **Addresses in full**, including query strings and fragments. If you preview a
  URL with a token in it, that token is in the file.
- **Working directories** of every pane.
- **File and folder paths** — the root of each files column, which entries were
  open, which one was selected, and the path of every file the preview held.
- **Names you typed for panes.**
- **Page titles** of previewed pages.
- **Timestamps** on recently closed tabs.
- **Git branch names** you filtered a commit graph by.
- **Window geometry, per-monitor DPI, and a monitor identifier.**

`profiles.json` holds any command line and environment variables you put in a
profile. Do not put a secret in one.

### Elsewhere

- `%TEMP%\bt-app-panic.log` — appended to if Folio panics.
- The three hooks installers, when you switch them on, write one file each into
  Claude Code's, Codex's and Copilot's own configuration directories. A dated copy
  of the file as it stood is kept beside it first. `SECURITY.md` has the details.
- Several `BT_*` environment variables make Folio write terminal content to a file
  you name — `BT_PTY_DUMP` writes every byte of every pane. None is set unless you
  set it. `docs/BT-ENVIRONMENT.md` lists all of them.

`SECURITY.md` describes the boundaries these sit inside, and how to report a
vulnerability.

---

## Known issues

- **Not signed.** See [SmartScreen](#smartscreen).
- **"Open Folio here" in Explorer is under "Show more options"**, not on the first
  page of the Windows 11 context menu. The top-level menu needs a packaged,
  signed application, so this waits on signing.
- **Video does not play.** `.mp4`, `.m4v` and `.webm` get a first frame, a length
  and a size. `.mov`, `.mkv` and `.avi` get none of that: the engine that would
  have to play them will not, and a still is not offered for a file that could
  never follow it.
- **A `.webm` card can be blank.** The frame comes from the codecs installed on
  your machine, and a stock Windows may have no VP9 or AV1 decoder. The length and
  size are still shown.
- **The first window is not centred**, and opens at 960 × 600.
- **Mixed-DPI and multi-monitor setups are thinly tested.** The window asks
  Windows for its own scale per monitor and re-rasterises on a move, but the
  machine this was written on has one display. A report of a window drawing half
  black was traced to something else and is not a DPI defect; the paths
  themselves have simply not been exercised on real hardware.
- **Some formulas fail to render live.** `\hbar` and its relatives fall back to
  showing the source as it was printed.
- **Cards fit fewer columns in a wider font.** The number of terminal columns a
  card can show is computed from the font you are using, and a card that reads
  well in the default font can be narrow in another.
- **Without the PowerShell integration**, command marks, command status and the
  fix for a mouse-mode program that leaves its mode behind are all inactive.

---

## Building from source

You need [rustup](https://rustup.rs/) and the MSVC toolchain (the Visual Studio
Build Tools with the C++ workload). Nothing else — `rust-toolchain.toml` pins the
compiler version, and rustup installs it on the first build.

```powershell
git clone <this repository>
cd folio
cargo build --release
```

The binary is `target\release\folio.exe`. To run it from the checkout,
`cargo run --release`.

The three gates every change has to pass, and the ones CI runs:

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

`cargo` will use every core it can find. On a machine you are also working on,
`cargo build -j 4` or a `jobs` entry in your own cargo configuration keeps it from
taking the machine over.

`vendor/alacritty_terminal` is a patched copy of the upstream VT engine, built as
part of the workspace; `vendor/alacritty_terminal/CHANGES-FOLIO.md` lists every
difference from upstream. `vendor/conpty/` carries the ConPTY files that ship in
the archive.

---

## Licence

MIT or Apache-2.0, at your option. The texts are in
[`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).

Folio is built on other people's work. Every dependency's licence, and the full
notices the ones that ask for them require, are in
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md), generated from the lock file
and checked against it.

---

## Contributing

[`CONTRIBUTING.md`](CONTRIBUTING.md) has the three gates, what the tests are for,
and where the design decisions are written down. Security reports go through the
private channel in [`SECURITY.md`](SECURITY.md), not through an issue.
