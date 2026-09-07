> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.2.3-preview

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.2.3-preview/folio-0.2.3-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit). Unpack, run folio.exe.

**下载:**上方 zip 即为完整下载,其余为校验和、物料清单与源码。

0.2.3 is about reading what a pane is showing: a wider Markdown column, a wide
table that scrolls sideways under an ordinary wheel, links that are drawn as
links, and a terminal scroll bar that is there only when there is something to
scroll to. Settings loses a row that asked the same question twice and gains
three that can each record a chord, and every string in the window has been read
by a native writer in both languages.

## A Markdown document uses the width you gave the window

- The reading column was capped at 702 logical pixels, which on a maximised
  window left a document reading down a strip in the middle of the pane. The cap
  is now about a thousand.
- Everything else about the column is unchanged: it is still centred, a pane too
  narrow for it still gets the whole pane, and tables and code blocks are set in
  the same column as the prose.

## A wide table scrolls sideways, with the wheel most mice already have

- A table whose columns need more room than the page can give has always had a
  bar along its own foot and a `Shift`+wheel that moved it. A tilt wheel, and a
  touchpad's second finger, were reported to the window all along and dropped
  before they reached the page.
- They are not dropped any more and they need no modifier. The table under the
  pointer is the one that moves, so a page with several wide tables has several
  scrolling regions, exactly as a browser does. `Shift`+wheel still does what it
  did, everywhere it did it.

## A link whose text is set in code, or carries emphasis, is drawn as a link

- A line reading ``[`folio-0.2.3-windows-x64.zip`](https://…)`` used to print its
  own Markdown source, brackets and address and all, while a plain-text link
  beside it on the same line rendered. Anything at all inside a link's text — a
  code span, a formula, a picture — left the two brackets in different pieces of
  the line, and neither piece could see a pair.
- A link's text is now read as what CommonMark says it is: ordinary inline
  content. A code span in it stays monospace and takes the link colour, a bold
  word stays bold, a picture wrapped in a link draws its picture, and every one
  of them answers a click.

## A terminal pane's scroll bar says what the pane can do

- **A pane whose whole transcript fits wears no bar.** After one run of a script
  that prints display formulas, with the prompt back and empty space below it, a
  thumb was drawn down the right edge of a pane that could not be scrolled at
  all. A display formula makes its row taller than a row, and the blank rows
  under the prompt give that height back; those given-back pixels were being
  counted as somewhere the view could travel to.
- **A pane scrolled to the top shows its thumb at the top of the track**, and the
  thumb's length is the pane's share of the whole transcript.
- **The bar rides the pane's own edge.** Every other bar in the window puts its
  thumb against the inner edge of the surface it belongs to, and this one stood
  two logical pixels off its own. The bar along the foot moved with it. The
  reserved lane is the same eight pixels and the thumb is grabbed and dragged
  exactly where it was.

## A tick on the command strip lands on its own command after a split

- Pressing the newest tick used to drop the reader into the middle of that
  command's output, with the prompt line above the top of the pane, once the pane
  had been split or resized.
- A mark is now written down against the whole wrapped line it sits in — the one
  thing a re-wrap keeps — and put back on that line afterwards, whether the line
  is still on the screen or has scrolled out of it. Dragging an edge is a run of
  re-wraps rather than one, and a mark is carried through every step of the drag.
  A mark whose line the re-wrap genuinely lost leaves the strip instead of
  pointing somewhere the command never was.

## The two Explorer rows in Settings are one switch

- `Explorer context menu` and `First page of that menu` asked one question twice,
  and the second was meaningless without the first.
- There is one row now, and it is on or off like every other switch on the page.
  **On is everything this Windows can do**: on Windows 11 with `folio.msix`
  beside `folio.exe` it puts "Open Folio here" under `Show more options` and
  registers the package that puts "Open in Folio" on the page Windows 11 opens
  first; anywhere else it writes the menu entry alone. Off takes back whichever
  of the two is there.
- Because On means different amounts on different machines, the line under the
  row says what it does on yours — and on a Windows 10 that line names no page,
  because that Windows has one menu. Nothing about where the answer is stored
  changed: removing the package from `Settings > Apps > Installed apps` still
  moves the row.

## Four picture-in-picture slots, four rows, four chords

- They were one row saying `Not set` with nothing on it to press, standing
  between a row that has a Record button and two greyed rows that are keys this
  window leaves to readline — so there was no way to tell which of the two it
  was. It was neither: those chords were always yours to choose.
- `Summon picture in picture 1` to `4` are a row each now, with their own chord,
  their own Record button and their own `↺`. A chord another row already answers
  to is refused, with the offer to take it, and `Restore all defaults` empties
  all four again. `keybindings.json` is untouched: it named all four slots before
  this change and names them now.

## The interface reads the way a native writer would put it

- A review of every Chinese string a reader can see reworded 52 of them:
  sentences carrying a reassurance nobody asked for, lines that explained an
  internal mechanism instead of what the switch does, and words left in English
  where the rest of the Chinese says 标签页 and 配置.
- The English audit's 83 proposals are in, along with the passages it proposed in
  `README.md`, `docs/PRIVACY.md` and `SECURITY.md`. No em-dash is left inside a
  string in the window, and a mechanism the reader cannot act on gives way to the
  result they get.
- No fact changed and no default moved in either language.

## The core compiles on macOS

- Everything below the application layer — the terminal grid, the renderer, the
  transcript, the document model, the detectors, the layout, the maths — compiles
  on a Mac, and a job on every push compiles it there so that it goes on doing
  so.
- **Nothing about Folio on Windows changes and there is no Mac build to
  download.** This is groundwork, not a port. What it buys is that the next
  feature cannot quietly assume Windows without somebody being told the same day.

## Also fixed

- **A first-run row names the file it will actually write.** The card's three
  agent rows spelled `~/.claude/settings.json`, `~/.codex/config.toml` and
  `~/.copilot/hooks/folio.json` whatever the machine was set to, while Claude
  Code reads `CLAUDE_CONFIG_DIR`, codex reads `CODEX_HOME` and Copilot CLI reads
  `COPILOT_HOME` first — and so does Folio's installer. On a machine that sets
  none of the three the spelling is the one it always was.
- **The welcome card draws no line between its rows**, does not fill a row under
  the pointer, and its focus ring waits for a key that moves something rather
  than for any key at all, and is no longer clipped at its right-hand side. The
  Settings page, whose row shape the card borrows, does all three.
- **A second copy of Folio no longer takes over your "Open Folio here" menu
  entry.** A launch rewrites that entry only when nothing is at the path it names
  or when that path is this very file, so a copy run once out of a downloads
  folder leaves the menu alone. Moving `folio.exe` still works exactly as before.
- **An agent installer that refuses says why in one sentence**, after a colon
  rather than a dash, and a reason long enough to need a second line gets one
  instead of being cut.
- **The README, the changelog and the design note no longer say the welcome card
  asks six questions.** It offers as few as two: an agent that is not on the
  machine is not listed at all.

## Upgrading from 0.2.2

**Nothing to do.** Your existing settings are preserved when you upgrade. Unpack
over the old folder, or beside it, and run `folio.exe`. The archive holds the
same **nine** files 0.2.2's did, and `folio.msix` still belongs in the same
folder as `folio.exe`.

## Download and run

Take `folio-0.2.3-windows-x64.zip` from this release, unpack it wherever you keep
programs, and run `folio.exe`. There is no installer; keep the extracted files
together in one folder. `SHA256SUMS.txt` is the hash of what you downloaded, and
`folio-0.2.3.cdx.json` is the bill of materials for what is in the build.

Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, with a certificate from
Microsoft's Artifact Signing service and a Microsoft time stamp, as they were in
0.2.2.

**`folio.msix` is no longer an asset of its own.** It is in the zip, where it has
always also been, and that is the only place it works from: the package names the
folder it was extracted into, so a copy downloaded on its own points at a folder
with no `folio.exe` in it. Nothing about the Explorer menu changes for anybody
who unpacks the archive.

## Known issues

- **A new signature has no reputation yet.** SmartScreen can still raise
  **"Windows protected your PC"** on the first run. **More info** names
  **Weiyi Shi** as the publisher and `folio.exe` as the application; **Run
  anyway** is the way through, and switching SmartScreen off is not.
- **Folio cannot be a panel inside Visual Studio Code.** `folio-here.cmd` in the
  archive makes it the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **Two web previews can overlap for about a fifth of a second while panes are
  moving to new places.** Panes standing still never overlap, and a divider drag
  does not do it.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

The full list is in `CHANGELOG.md` in the repository.

---

<!-- zh: opus46 -->

**The Chinese half goes here, written separately.** It mirrors the English above,
section for section, with the same headings in Chinese and the same facts:

1. `# Folio 0.2.3-preview` — the heading, unchanged.
2. The opening paragraph: what this release is about.
3. A Markdown document uses the width you gave the window.
4. A wide table scrolls sideways, with the wheel most mice already have.
5. A link whose text is set in code, or carries emphasis, is drawn as a link.
6. A terminal pane's scroll bar says what the pane can do.
7. A tick on the command strip lands on its own command after a split.
8. The two Explorer rows in Settings are one switch.
9. Four picture-in-picture slots, four rows, four chords.
10. The interface reads the way a native writer would put it.
11. The core compiles on macOS.
12. Also fixed — the five entries above, in the same order.
13. Upgrading from 0.2.2.
14. Download and run — including that `folio.msix` is no longer an asset of its
    own and why.
15. Known issues — the six above, in the same order.

The download line at the top of the page is already bilingual and is not
repeated here.
