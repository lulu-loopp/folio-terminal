<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%"
       alt="Folio — the Windows terminal that renders math, and says which agent
       is waiting for you. Beside the name, a terminal pane has run a command that
       printed a file, and the display formula in that file — the integral of e to
       the minus x squared over the whole real line, equal to the square root of pi
       — stands typeset in the output, above the next prompt.">
</picture>

A Windows terminal that typesets the mathematics a command prints, previews the
files it names, and says which of your agents is waiting — without sending you to
another window.

[中文说明](README.zh-CN.md) · [Shortcuts](docs/shortcuts.md) ·
[Security](SECURITY.md) · [Changes](CHANGELOG.md)

> **Preview.** 0.1.0 is the first public build, and it is not code-signed — see
> [SmartScreen](#smartscreen) below.

---

## No terminal on Windows typesets LaTeX

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/terminal-math-dark.png">
  <img src="docs/screenshots/terminal-math-light.png" width="100%"
       alt="A single terminal pane with the output of one command typeset where it
       was printed: paragraphs of prose carrying short inline formulas, and three
       display formulas set on lines of their own — the Gaussian normalisation
       integral, the Fourier transform pair, and the series for the exponential.">
</picture>

Folio sets it in two places through one typesetter — LaTeX through MiTeX into
Typst. `$…$` and `$$…$$` in command output are set where the command printed
them; the preview beside it takes those plus `\(…\)`, `\[…\]` and the bare
`amsmath` environments. What cannot be set is shown as it was printed.

## Why that matters now

What talks in a terminal now is an agent, and an agent does not answer in plain
text. It answers in markdown: headings, tables, formulas, and the paths of the
files it just changed. A terminal shows characters. So the answer lands as source
for a document nobody typesets, and the files it names sit behind another window.

Everything below is that gap closed.

## Nothing here makes you switch away

- **See what a file is** without opening it. Hold the pointer over a name in the
  files column: the first page of the PDF, the first frame of the video, the
  first lines of the text, the image itself. *(peek card)*
- **Read it properly** without leaving the terminal. It opens in a pane beside
  the prompt — markdown typeset, PDF page by page, video playing, a web page with
  an address field and a back stack. *(preview pane)*
- **Follow a path the terminal printed** without retyping it anywhere. Click it
  and it opens beside the terminal; hold `Ctrl` and it goes to the machine's own
  application. Paths nobody marked up are found too, once the file is confirmed
  to exist.
- **Take in every session at once** instead of clicking through tabs.
  `Ctrl+Shift+Z` turns the tab strip into a column of cards, one per tab, each
  drawing that tab's own panes in the layout they have. *(Cards)*
- **Know which agent is waiting** without going to look. Its tab lights a dot; the
  taskbar flashes if you are in another program; a real Windows notification goes
  up only if the window is minimised or on another desktop. One request interrupts
  you at most once, and the dot clears when you answer in that pane or the program
  withdraws — not because you glanced at it. `Ctrl+Shift+A` jumps to the longest
  wait.
- **Stay in the repository** instead of opening a git client. `Ctrl+Shift+G` turns
  the files column into a git panel: branch, working tree, staged and unstaged
  files, the commit graph, a selected file's diff in the preview.
- **Move a pane** instead of closing and re-opening one. Drag a tab or a pane out
  into its own window; the ones it did not touch keep their widths. *(float)*

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/preview-pdf-dark.png">
  <img src="docs/screenshots/preview-pdf-light.png" width="100%"
       alt="The pointer rests on a file name in the files column and a card has
       come up under it, showing the first page of a PDF above its page count and
       size. The wheel winds the card through the pages.">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/cards-dark.png">
  <img src="docs/screenshots/cards-light.png" width="100%"
       alt="The tab strip has become a column of cards. The single card stands for
       a tab of eight panes and draws all eight in miniature; Alt and the wheel
       scroll the picture inside a card a row at a time.">
</picture>

Claude Code, Codex and GitHub Copilot CLI each have a switch on the Agent page in
Settings that writes one notification hook into that tool's own configuration file
and takes it back out again. Codex reports the end of a turn; the other two report
that they are waiting for you. Any program that writes
`OSC 1337;RequestAttention=yes` is heard with nothing installed at all.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/surfaces-dark.png">
  <img src="assets/readme/surfaces-light.png" width="100%"
       alt="Three more surfaces: a window laid out as cards, a markdown document
       typeset in a preview pane, and a web page in a preview pane with a
       breadcrumb address field above it.">
</picture>

## Windows, in detail

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/main-window-dark.png">
  <img src="docs/screenshots/main-window-light.png" width="100%"
       alt="The window in its default font and default scheme: a files column on
       the left, two terminal panes side by side, and a markdown document open in
       a preview pane on the right.">
</picture>

Wide characters get the oldest work in this repository: how many columns one
occupies, where the cursor may land, where a selection may cut, held byte by byte
by a recorded corpus of width cases. Three Chinese input methods were driven by
hand on real hardware — Microsoft Pinyin, WeType and Sogou. Sogou never reports
the syllables you are still typing, so it draws them in its own candidate window;
the committed text arrives correct.

Two monitors at different scales are measured on real hardware — dragged between,
maximised, sized past the screen edge both ways.

"Open Folio here" is in the Explorer context menu, under "Show more options".

Windows PowerShell 5.1 still ships PSReadLine 2.0.0, which takes its edit anchor
from a cell count made before the resize: narrow the window and the input line is
redrawn where it used to be, over text that has moved. Folio carries a patched
2.4.6 compiled into the executable and installs it into your module path if you
ask.

---

## Getting started

### Download

Take `folio-0.1.0-windows-x64.zip` from the releases page, unpack it wherever you
keep programs, and run `folio.exe`. There is no installer, and nothing is written
outside that folder until you run it. `SHA256SUMS.txt` is the hash of what you
downloaded. Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

The web preview needs the **WebView2 Runtime**. Windows 11 has it; Windows 10
usually does, and if it does not, the Evergreen Runtime is
[here](https://developer.microsoft.com/microsoft-edge/webview2/). Without it
everything except the web preview works, and the preview says what is missing.

### SmartScreen

0.1.0 is not code-signed, so the first run may raise **"Windows protected your
PC"**. Click **"More info"**, check the app named there is `folio.exe`, and click
**"Run anyway"**. Do not switch SmartScreen off for this.

### First run

The first tab opens the first shell your machine actually has. The five shipped
profiles are looked for in order — PowerShell 7, Windows PowerShell, WSL, Git
Bash, Command Prompt — and one whose program is not installed is greyed out in the
picker rather than hidden. Seven more profiles start an agent — Claude Code,
Codex, Copilot CLI, Kimi Code, pi, Hermes, OpenCode — and are found the same way,
on the Windows path; one installed inside WSL is run from the WSL profile, or from
a profile of your own whose program is `wsl.exe -e <name>`.

The first time a PowerShell pane prints something, a strip says the PowerShell
integration is not installed. **Add to `$PROFILE`** appends one line —
`. "$env:APPDATA\Folio\shell-integration\folio.ps1"` — after copying the file as
it stood to a dated backup beside itself; delete that line to undo it. **Don't
show again** ends the asking, and closing the strip decides nothing, so the next
PowerShell asks once more. The
integration is what command marks run on: `Ctrl+Shift+↑` and `Ctrl+Shift+↓` step
between commands, and a command that failed is marked as having failed. It is
also what tells inline `$…$` formulas apart from a shell variable, so without it
they stay as source while `$$…$$` blocks still typeset. Git Bash
and WSL need none of it and leave nothing on disk.

The three rows on the Agent page **install nothing by default**, and they are not
defaults that happen to be off: each reads the tool's own configuration file and
reports what is in it. On a new machine all three files are absent, so all three
read Off.

---

## What you should know

### Privacy

Folio sends nothing anywhere: no telemetry, no analytics, no crash reporting, no
update check, and no network client of its own. The only thing that reaches the
network is a page you open in the web preview.

What it remembers lives in two directories: `%APPDATA%\Folio` for settings,
profiles, schemes and the session, and `%LOCALAPPDATA%\Folio\WebView2` for the web
preview's cookies and cache. Delete the first and Folio starts as it did new.

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\Folio"
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

What is in each file, and why a full address ends up in `session.json`, is in
[`docs/PRIVACY.md`](docs/PRIVACY.md).

### Known issues

- **Not signed.** See [SmartScreen](#smartscreen) above.
- **`.mov` does not play.** It gets a first frame, a length and a size, and the
  pane says why there is no play button. `.mkv` and `.avi` get no preview at all.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **"Open Folio here" is not on the first page** of the Windows 11 context menu.
  That page needs a signed, packaged application, so it waits on signing.
- **A `.webm` card can be blank on a stock Windows**, which may have no VP9 or AV1
  decoder for the still. Length, size and playback are unaffected.
- The rest are in [`CHANGELOG.md`](CHANGELOG.md).

### Licence

MIT or Apache-2.0, at your option. Every dependency's licence and the notices they
require are in [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

The two licences grant copyright and patent permissions and nothing else. The
Folio name and the marks are not covered — [`TRADEMARK.md`](TRADEMARK.md) says
what that means for a modified distribution.

Building from source is in [`docs/BUILDING.md`](docs/BUILDING.md);
[`CONTRIBUTING.md`](CONTRIBUTING.md) is how a change gets made; security reports
go through the private channel in [`SECURITY.md`](SECURITY.md), not an issue.

---

## What's next

**Something that watches the desk.** It already sees what every tab is doing and
who is waiting; next it keeps the account — what is open, what is outstanding,
where you left off. It organises the work, it does not do the work: the code is
still written by the agent you already run.

**Talking to a pane.** Speaking to the window types into it.

**Reaching this machine from a phone.** The agents run on the desktop; the phone
shows which one is waiting and lets you answer it. Off unless you switch it on,
and over your own network first — the privacy section above stays true until you
decide otherwise.

These are directions, not dates.

## What this is not

**Not a faster terminal.** Drawing on the GPU and speaking ConPTY are the price of
entry here, not the argument for it.

**Not an AI that writes your code.** There is no model and no API key in here. It
serves the agent you already run, and it works the same if you run none.

**Not cross-platform yet.** Windows came first because ConPTY, the Explorer menu,
WebView2, per-monitor DPI and PSReadLine are each things you only get to do
properly by picking one and finishing it. The drawing, the layout and the
typesetting do not know which platform they are on; what does is one crate, and
macOS and Linux are where it goes after this.
