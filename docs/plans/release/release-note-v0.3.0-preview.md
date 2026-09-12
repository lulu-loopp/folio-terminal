> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.3.0-preview

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.3.0-preview/folio-0.3.0-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit). Unpack, run folio.exe.

**下载:**上方 zip 即为完整下载,其余为校验和、物料清单与源码。

0.3 is about writing on the page you were reading. A `.md` file in a preview
pane — headings, tables, code and formulas typeset beside the prompt — is now
something to type into, and nothing has to be turned back into source first:
click into a paragraph and that paragraph shows the file's own characters, marks
and all, in the typeface and at the size you were reading it in, while the rest
of the page goes on reading as a page. `Ctrl+S` writes the file, `Ctrl+Z` takes
the last change back, `Esc` gives you the page again. Around that: the files
column can make a file or a folder and send either to the Recycle Bin, starting
Folio again gives you another window of the Folio already running rather than a
second copy, and a page full of large screenshots no longer freezes the window.

## The page you are reading is the page you write on

- **A paragraph, heading, list or quote you click into keeps the reading
  typeface and shows its marks** — the `#` of a heading, the `**` around a bold
  phrase, the `- ` in front of an item — where the file spells them. Only a code
  block, a table or a formula turns to monospace source, because there the way
  the characters line up is part of what they say.
- **There is no mode to enter and none to leave.** The caret moves between
  blocks the way it moves between lines, and a block is rendered again the
  moment the caret leaves it. `Esc`, a click on the empty space beside the text
  and a click away from the pane all put the page back to reading as a page.
- **What you select is what you copy**, with the marks, so what you paste back
  is what was there.

## What a save writes

- **A save changes the part you edited and leaves the rest identical, down to
  the byte.** Some files begin with a few bytes naming their own encoding — what
  Windows PowerShell writes is the common case — and those bytes used to be read
  and then thrown away, so editing one line rewrote the whole file in another
  encoding without a word about it. The encoding now travels with the text from
  the read to the write.
- **`Ctrl+Z` and `Ctrl+Y` undo and redo.** A run of typing comes back in one
  press, the history belongs to the file rather than to the pane, and the
  unsaved dot goes out when you undo back to your last save.
- **A file larger than the pane's first look can be edited**, up to 8 MB. Past
  that, and where bytes will not read as text, a file opens and reads as before
  but is read-only, and the foot of the pane says why.

## Making a file, and starting Folio

- **`New file…` and `New folder…`** put the name field in the tree where the new
  row will appear, so the name is typed where the name is going to be. `Enter`
  creates, `Esc` cancels, and a name the folder will not take turns red in the
  field. **`Delete`** sends a file, or a whole folder, to the Recycle Bin and
  asks nothing first, because that is where it goes.
- **Right-clicking the empty space in a column opens the menu of the folder the
  column is standing in**, empty folders included — which is where you most want
  to make the first file. `Ctrl+Shift+P` finds a file under that folder, and
  `Enter` opens it in the preview pane, ready to be typed into.
- **Starting Folio again opens another window of the one that is running**: one
  program, one set of settings, one place your tabs are remembered.
  **Settings > General > Opening Folio again** turns that into a tab in the
  window you used last. Explorer's "Open in Folio" and `folio-here.cmd` — what
  VS Code's external terminal runs — always open a tab, because they ask for a
  terminal in a folder, not for another Folio.

## Read before it shipped

The work that became 0.3 — nineteen merges, about seventeen thousand lines —
was read back against what it claimed to do before this release was built, in
four passes: the editor, the picture and animation work, the handover between a
running Folio and a newly started one, and the files column's new verbs. Every
finding was then checked a second time, line by line, by a reader who had not
made it and who owned the verdict.

Thirty-six defects came out of that, and every one of them was real. The ones
that could have cost you a file or frozen the window are fixed in 0.3 — the save
that changed a file's encoding, the save that threw your undo away, the caret
that stood beside the Chinese character it was editing rather than on it.
What is left is written down and public in the repository, and it is the first
work after this release.

## What changed

### Added

- Markdown edited where it is shown: the block under the caret is its own
  source, the rest of the page stays as it reads, `Ctrl+S` saves, `Esc` leaves.
- The files column's menu makes a file or a folder in place and sends either to
  the Recycle Bin; the empty space in a column opens the folder it stands in.
- Undo and redo in a preview, on `Ctrl+Z` and `Ctrl+Y`, one history per file,
  and an unsaved dot that goes out when you undo back to your last save.

### Changed

- A block you click into keeps the reading typeface and shows its marks; only
  code blocks, tables and formulas turn monospace.
- Starting Folio again opens a window of the running Folio rather than a second
  copy, with a row in Settings to make it a tab instead, and a relative folder on
  the command line is resolved where you typed it.
- A text or Markdown file larger than the pane's first look can be edited, up to
  8 MB.
- The preview's `Open` menu opens on hover, and a window that stops answering is
  named in Folio's log by what it was doing.

### Fixed

- A save keeps the file's encoding and keeps what you can undo, and a read
  arriving while you type no longer replaces what you typed. A file that will
  not fully read as text is read-only rather than quietly rewritten.
- The caret sits on the character it edits on Chinese text; an empty new file can
  be typed into; the end of a file with no last line break is reachable.
- Letters being composed are drawn in the page they are going into, and a
  composition left in the quick search or a name field no longer floats.
- A page of large screenshots, a pane of large pictures and a page of many
  formulas settle instead of redrawing for ever, and a zoomed picture neither
  vanishes nor sticks to one axis.
- A long or large animated `.gif` plays, from its first frame, and switching
  between two shows the right one at once.
- A floating window takes the pointer and the menu over its whole face, so the
  row hidden underneath no longer lights up or answers for it.
- The name field takes a long pasted line without freezing, and refuses a name
  the folder will not take whatever its capitalisation.

The full list is in `CHANGELOG.md` in the repository.

## Known issues

- **A drawn table comes down when its header row sits just above the viewport.**
  Scroll the heading back into view and the table is drawn again.
- **A space dropped at a program's line break inside a table cell is not always
  put back** — a break beside punctuation, a number or a CJK character is
  rejoined without it.
- **The note the web preview shows after answering a page's message box is
  hidden behind the page.** The page is answered and goes on running.
- **Folio cannot be a panel inside Visual Studio Code**; `folio-here.cmd` makes
  it the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **Two web previews can overlap for about a fifth of a second while panes are
  moving to new places.** Panes standing still never overlap.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

## Download and run

Unpack the zip wherever you keep programs and run `folio.exe`. There is no
installer, unpacking over an older folder keeps your settings, and
`SHA256SUMS.txt` is the hash of what you downloaded. Needs **Windows 10 1809 or
newer, or Windows 11, 64-bit**.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, as they have been since
0.2.0, but a new signature has no reputation, so SmartScreen can still raise
**"Windows protected your PC"** on the first run: **More info** names **Weiyi
Shi** as the publisher, and **Run anyway** is the way through. Folio is not on
winget yet.

**Folio has no telemetry, no analytics and no crash reporting.** Two things reach
the network: a page you open in the web preview, and the update check, which
Settings > General > **Update check** switches off.
