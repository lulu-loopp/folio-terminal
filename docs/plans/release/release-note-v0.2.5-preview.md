> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.2.5-preview

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.2.5-preview/folio-0.2.5-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit). Unpack, run folio.exe.

**下载:**上方 zip 即为完整下载,其余为校验和、物料清单与源码。

0.2.5 is about what a pane shows. A table an agent prints is drawn whole or not
at all, however the printing program wrapped its rows. A pane restored behind
another tab comes up at the width it is going to have rather than one no shell
can print at, and dragging the window edge re-wraps the pane in front of you as
promptly with a dozen tabs open as with one. The title bar keeps a band to drag
the window by however many tabs are in it, and the four split pictures at the
top of a pane's menu can be seen in the light theme.

## A printed table survives the way the program wrapped it

- **A row the program itself wrapped is drawn as the one row it was printed
  as.** An agent printing a wide table lays it out to the width of the pane and
  wraps a long row onto a second line, usually stopping a word or two short of
  the last column. Folio drew the heading and the first row and left every row
  after them as text. A wrapped row is now put back together whenever the lines
  spell exactly the row the heading calls for, whether or not the program ran
  the first line right to the edge.
- **A row broken into three or four lines comes back as one row**, not only a
  row broken into two, and a break that lands right beside one of the row's own
  bars is read as a break rather than as the end of the row.
- **A row still arriving is not drawn in part.** Nothing appears from the lines
  that have come until the row is complete.
- **A pipe the table cannot account for takes the whole table down**, so a table
  no longer ends halfway through the block it was printed in. A pipe after a
  blank line, and a second table under a caption line, leave the table standing.

## A pane restored behind another tab comes up at its real width

- A window that was closed maximized is put back at its own rectangle first and
  maximized a moment later, and every tab was measured against that first
  rectangle. A tab you were not looking at kept those measurements until you
  clicked it, so a tab holding three panes in a window too narrow for three
  could start a shell two columns wide: its first prompt came out two characters
  to a line, and widening the pane afterwards could not put back together what
  had already scrolled past.
- Every tab now follows the window whether or not it is the one on screen, and a
  pane is never started at the width of the little bar the layout shows in place
  of a pane it has no room for.

## Text follows the window edge while you drag it, with any number of tabs

- A pane re-wraps its lines as the window is made narrower. Once every tab
  followed the window's size, each tab behind the one on screen laid its own
  pane out again on every step of the drag: with six tabs open the picture
  arrived about a third less often, and with a dozen the text looked frozen at
  the old width until the drag stopped.
- A tab you are not looking at now takes the new size once, when the drag
  settles, the way its shell already did. The pane in front of you re-wraps on
  every step, as it always has.

## The title bar keeps a handle to drag the window by

- Folio draws its own title bar, and the only stretch of it a window may be
  moved by is whatever the row of tabs leaves over. With a dozen tabs open the
  row reached the settings gear and left nothing: the top edge could not be
  dragged anywhere along its length, and there was no place to double click it
  to maximise either.
- The row now stops 96 pixels short of the buttons in the corner whatever it is
  carrying, so there is always a band there to take hold of. The tabs pay for it
  the way they already pay for one more tab, by growing narrower first and
  scrolling only once they are as narrow as they go. A window with the tabs down
  its side is unaffected, and so is a window with room to spare in the bar.

## The split pictures in a pane's menu can be seen

- The little pane and the four bars around it were drawn in the same hairline
  the menu's own edge is drawn in. On the white card of the light theme that
  came out a shade off white, so the picture the menu opens with was there and
  invisible; on the dark card it was only just there.
- Both are now drawn in the ink the words on the rows under them are set in, and
  any theme whose ink sits too close to its card has the picture lifted clear of
  it. The bar under the pointer still turns and fills with the accent colour.

## Also fixed

- **The little picture on a tab's card shows the pane as it stands.** Where a
  line was broken across rows at a width the pane has since stopped having, the
  card drew the old break; it now puts the line back together.

## Known issues

- **A drawn table comes down when its header row sits just above the viewport.**
  Scroll the heading back into view and the table is drawn again.
- **A space dropped at a program's line break inside a table cell is not always
  put back.** A break between two ordinary words is rejoined with its space; a
  break beside punctuation, a number or a CJK character is rejoined without one,
  so `受命 −28°` comes back as `受命−28°`.
- **The note the web preview shows after answering a page's message box is
  hidden behind the page.** The page is answered and goes on running; the line
  saying so on the bottom strip cannot be read.
- **Folio cannot be a panel inside Visual Studio Code.** `folio-here.cmd` in the
  archive makes it the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **Two web previews can overlap for about a fifth of a second while panes are
  moving to new places.** Panes standing still never overlap, and a divider drag
  does not do it.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

The full list is in `CHANGELOG.md` in the repository.

## Download and run

Take `folio-0.2.5-windows-x64.zip` from this release, unpack it wherever you
keep programs, and run `folio.exe`. There is no installer; keep the extracted
files together in one folder. Unpacking over an older folder keeps your
settings. `SHA256SUMS.txt` is the hash of what you downloaded, and
`folio-0.2.5.cdx.json` is the bill of materials for what is in the build.

Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, with a certificate from
Microsoft's Artifact Signing service and a Microsoft time stamp, as they have
been since 0.2.0. A new signature has no reputation yet, so SmartScreen can
still raise **"Windows protected your PC"** on the first run. **More info** names
**Weiyi Shi** as the publisher and `folio.exe` as the application; **Run anyway**
is the way through, and switching SmartScreen off is not.

**Folio is not on winget yet.** A manifest is with the winget community
repository and is waiting for a reviewer. This zip is the way to install Folio
until it is accepted.

**Folio has no telemetry, no analytics and no crash reporting.** Two things reach
the network: a page you open in the web preview, and the update check, one `GET`
of the releases list at most once a day, which you can switch off at Settings >
General > **Update check**. `docs/PRIVACY.md` says what is written to disk.
