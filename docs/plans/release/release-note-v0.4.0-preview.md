> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.4.0-preview

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.0-preview/folio-0.4.0-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit) · [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.0-preview/Folio-0.4.0-macos-arm64.dmg) (macOS 14+, Apple silicon)

**下载:**上方 zip 与 dmg 即为完整下载,其余为校验和、物料清单与源码。[中文版发布说明](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.0-preview.zh-CN.md)

0.4 is the first release for two systems. Folio runs on macOS: the same program,
built for Apple silicon, asking for macOS 14 or newer, with the panes, the files
column, the Git page and the preview that were already there — and where the two
systems differ it follows the one it is on, so the chords are Command chords, the
menu bar is a real one, the window wears the three buttons macOS draws, a key
summons the terminal from any application, and a settings file written on either
machine is read by both. The other half is the Markdown preview under a very
large document. A file now opens complete and can be typed into at once, up to
the 8 MB editing ceiling — the `Read-only · 64 KB` head that made an editable
file look read-only is gone — and neither opening it nor typing in it waits on
the whole document any longer: a three-megabyte file that took about 3.4 seconds
to show its first block shows it in a tenth of a second, and a caret move inside
a paragraph, which used to rescan every line in the file, is no longer
measurable.

## What changed

### Added

- **Folio runs on macOS**, built for Apple silicon and asking for macOS 14 or
  newer: the same panes, files column, Git page and preview pane, several
  windows coming back where they were left, and an agent that still says when it
  has finished — with Command chords on the Shortcuts page, the window buttons
  macOS draws, a real menu bar, and one settings file both machines read.
- **On a Mac, a key summons the terminal from anywhere.** `Ctrl` and the
  backtick key brings the quick terminal down over any application and puts it
  away again; it is an ordinary row on the Shortcuts page, it needs no
  permission from macOS, and Windows keeps `Win` and the backtick unchanged.
- **On a Mac, Folio's Dock icon offers a new window and a new tab.** Press and
  hold the icon, or right-click it, and both stand above the rows macOS puts
  there — they work from another application and with every window closed.

### Changed

- **On a Mac, the Appearance page no longer offers Acrylic.** Folio does not
  blur what sits behind a window on macOS, so the row is gone rather than greyed
  at `Off`; Windows is unchanged, and a settings file shared between the two
  keeps the value either way.
- **A Markdown file opens complete, and is editable at once.** The whole file is
  read on the first open, up to the 8 MB editing ceiling, so the document
  scrolls to its end and the caret can go in immediately; the `Read-only · 64 KB`
  badge is gone, a file over the ceiling keeps a head on screen and the badge
  says its real size, and text, CSV and diffs keep their bounded first read.
- **Opening a very large Markdown document is immediate.** Only the blocks near
  the viewport are measured and the rest estimated from their line counts, each
  estimate corrected as it scrolls into view, with an anchor holding the text
  under your eye still while the heights above it settle: on a three-megabyte
  file the open went from about 3.4 seconds to a tenth of one.
- **Moving the caret and typing in a very large Markdown document no longer
  waits on the whole document.** The document is shared rather than copied on
  every keystroke, undo is derived from the edit itself, and the line index and
  widths are kept up to date incrementally; on that same file a keystroke went
  from 143 to 66 milliseconds and a caret move inside a paragraph to nothing
  measurable.
- **Badges in a Markdown preview stand in a row instead of one under another.** A
  web picture written in a line with anything else on it is now a small chip
  carrying its alt text, wrapping like a word among the words, so four badges at
  the top of a README no longer become four full-width cards; a picture alone in
  its own paragraph still gets the card, and pictures on your disk are unchanged.
- **A changed file in the Git page says what happened to it in words.** Every
  status git can report now has a phrase — `Modified`, `Added, staged`,
  `Conflict (both modified)` — with git's own two letters beside it, on a row and
  on the files under an expanded commit.
- **The card that appears when you rest on a file now fades in**, over the same
  ninety milliseconds the small labels elsewhere take, without moving or growing;
  the wait before it is unchanged, and reduced motion still gets it instantly.
- **The top bar takes the hand anywhere it is not a button.** The space between
  two tabs, the strip above them and the gaps around the settings button now pick
  the window up, and double-clicking still does what it did.
- **A quit keeps every tab a window was holding.** A page still closing down can
  no longer write the session out again on the way out with the tabs that had
  already gone missing from it.
- **The preview's bottom line appears only when it has something to say.** The
  page runs to the bottom of what is showing it; `Saved`, `Revealed` and a file
  changed on disk float over it for as long as they have something to say, a file
  you cannot edit shows a padlock at the end of the path row that says why when
  you click into the page, and what a picture is has moved up to that same row.
- **A file the preview cannot show offers to open it in the default app**, in the
  card an unknown file type has always worn — a picture with too many pixels, a
  file too large to read, a pane too small to draw one in, a recording this
  machine cannot play, in a pane or in a torn-off window alike.

### Fixed

- **On a Mac, `Edit ▸ Copy` and `Edit ▸ Paste` act on the pane you are looking
  at.** Over a terminal they did nothing at all; they now copy the terminal's
  selection or the text selected in a file being edited, and paste into whichever
  has the keyboard. `⌘C` and `⌘V` are unchanged.
- **On a Mac, tabs take the whole width of the title bar.** The strip was setting
  aside room for four window buttons on a window that carries one, hiding the tab
  names behind a wide empty band; seven tabs in a 934-point window now stand 91
  points wide with their names showing.
- **On a Mac, a pane started from the Dock or Finder can read what you type in
  your own language.** A shell started that way is handed no language setting by
  macOS, so `天下为公` came back as bytes and filenames outside ASCII as question
  marks; where the machine has no matching locale installed, Folio now says what
  it does know — that this window reads UTF-8 — and still never picks a country
  for you or overwrites a setting you already have.
- **A preview reuses the Markdown prose it has already measured** across caret
  moves and edits, and a block's scroll offset is reset when the document is
  parsed again.
- **A table on a focus card lines up again.** A card's rows are laid out column by
  column, as the pane's own grid is, so a box-drawing table holding Chinese text
  puts every border in the same place on every row.
- **A focus card keeps the same content when the window is resized**, and
  `Alt`+wheel answers at once when it is reversed at the oldest content.
- **`REMOTES` on the Git page no longer sits under a highlight nobody put
  there.** Group headers are dim at rest and brighten under the pointer, with no
  block at any time; the whole row is still what you press.
- **Every picker in Settings that offers profiles shows their marks**, in the
  marks and at the size the new-tab `⌄` menu draws, and keeps showing one once
  the picker is closed. `Default profile` borrows nobody's mark.
- **While you type pinyin, the caret stands at the end of what you have typed.**
  Composing over a Chinese character already on screen left half of it behind and
  the caret read it as a wide character it stood in the middle of.
- **A menu closes when you press the button that opened it** — the preview's
  `Open ⌄` pill, the `…` that stands in for folders a narrow row cannot show, and
  the commit graph's branch list inside a torn-off window.
- **A formula that took a moment to typeset appears when it is ready**, rather
  than waiting for a scroll, a resize or another file to be opened.
- **A letter your keyboard layout makes reaches the shell.** `ü`, `ä`, `ö`, `ß`
  and every other letter a layout composes outside ASCII sent nothing at all;
  they are sent now, as the bytes they are.
- **The command marks along a pane's right edge keep up with the shell.** A tick,
  or a red mark where a command failed, is drawn on the turn the shell reports it
  instead of waiting for the next keystroke or pointer move.
- **A chord held with the Windows key no longer types its letter into a box.**
  `Win+C` typed a `c` into the command palette, the search box, a name being
  edited, the branch prompt, the commit graph's search and a document being
  edited.

The full list is in `CHANGELOG.md` in the repository.

## Download and verify

### Windows

| asset | what it is |
| --- | --- |
| `folio-0.4.0-windows-x64.zip` | the nine files that belong together, in one folder — `sha256:66da4ce336f90b032ce6900dda17197f0b865ca7080a65c35f3995cf0997f512` |
| `SHA256SUMS.txt` | the hash of the archive and of the bill of materials, in the format `sha256sum -c` reads |
| `folio-0.4.0.cdx.json` | the CycloneDX bill of materials for what is in the build |

Unpack the zip wherever you keep programs and run `folio.exe`. There is no
installer; keep the extracted files together in one folder, and unpacking over an
older folder keeps your settings. Needs **Windows 10 1809 or newer, or Windows
11, 64-bit**. The web preview needs the **WebView2 Runtime**, which Windows 11
has and Windows 10 usually does.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, as they have been since
0.2.0, but a signature is not a reputation, so SmartScreen can still raise
**"Windows protected your PC"** on the first run: **More info** names **Weiyi
Shi** as the publisher, and **Run anyway** is the way through. Folio is not on
winget yet.

### macOS

| asset | what it is |
| --- | --- |
| `Folio-0.4.0-macos-arm64.dmg` | the application, signed and notarized — `sha256:346eb5a3fc2992513e141b0881762c2f85221b35d1be3a6223f1e7463e700dfd` |
| `SHA256SUMS-macos.txt` | the hash of the disk image, in the format `shasum -a 256 -c` reads |

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
take it from the releases page again. The web preview uses the WebKit already on
the machine, so there is nothing else to install.

**Folio has no telemetry, no analytics and no crash reporting.** Two things reach
the network: a page you open in the web preview, and the update check, which
Settings > General > **Update check** switches off.

## Known issues

- **A drawn table comes down when its header row sits just above the viewport.**
  Scroll the heading back into view and the table is drawn again.
- **A space dropped at a program's line break inside a table cell is not always
  put back** — a break beside punctuation, a number or a CJK character is
  rejoined without it.
- **The note the web preview shows after answering a page's message box is
  hidden behind the page.** The page is answered and goes on running.
- **Two web previews can overlap for about a fifth of a second while panes are
  moving to new places.** Panes standing still never overlap.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` — or
  `~/Library/Application Support/Folio/diagnostics.log` on a Mac — if you hit it.
- **Windows: Folio cannot be a panel inside Visual Studio Code**;
  `folio-here.cmd` in the archive makes it the external terminal VS Code opens
  instead.
- **Windows: `.webm` needs the VP9 or AV1 Video Extension** from the Microsoft
  Store. A stock Windows has neither, and without one there is no still and no
  playback.
- **macOS: a run that ends in a crash** leaves its report where the system puts
  every one, `~/Library/Logs/DiagnosticReports`; the next Folio names the file in
  its own log rather than copying it anywhere.
