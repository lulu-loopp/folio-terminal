# What Folio does

The long version of the front page. [`README.md`](../README.md) is the short
one; [`install.md`](install.md) is how to get Folio onto a machine and what the
first run looks like.

The keys named below are the Windows ones. [Shortcuts](shortcuts.md) has both
columns: on a Mac an application verb wears **Command** where Windows wears
**Ctrl**, which is what leaves **Control** to the terminal on both.

## LaTeX rendering in the terminal

The LaTeX a command prints is typeset where it was printed.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/terminal-math-dark.png">
  <img src="screenshots/terminal-math-light.png" width="100%"
       alt="A single terminal pane with the output of one command typeset where it
       was printed: paragraphs of prose carrying short inline formulas, and three
       display formulas set on lines of their own — the Gaussian normalisation
       integral, the Fourier transform pair, and the series for the exponential.">
</picture>

- `$…$` and `$$…$$` in command output are set in the line the command printed
  them on.
- The preview pane takes those, plus `\(…\)`, `\[…\]` and the bare `amsmath`
  environments.
- Unsupported LaTeX remains visible as source text.
- Inline `$…$` is told from a shell variable by the shell integration —
  PowerShell on Windows, zsh or bash on a Mac.
  Without it inline formulas stay as source, and `$$…$$` blocks still typeset.

## Made for agents

The agent that is waiting for you is marked on its tab, so there is nothing to go and check.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/settings-agents-dark.png">
  <img src="screenshots/settings-agents-light.png" width="100%"
       alt="The Agent page in Settings: a row each for Claude Code, Codex and
       GitHub Copilot CLI, each with a sentence saying which file its switch
       writes a notification hook into and all three switched off, and a fourth
       row for notifications at the end of a turn.">
</picture>

- A waiting agent lights a dot on its tab. If another program has the focus,
  Windows flashes the taskbar and macOS bounces the Dock icon until you come
  back; if the window is minimised or on another desktop, the machine's own
  notification is raised. On a Mac the first of those is where macOS asks
  whether Folio may send notifications — answer it once, and a refusal is
  reported on the Agent page rather than swallowed.
- One request interrupts at most once, and the dot clears when you answer in that
  pane or the program withdraws the request. `Ctrl+Shift+A` jumps to the longest
  wait.
- Claude Code, Codex and GitHub Copilot CLI each have a switch on the Agent page
  in Settings that writes one notification hook into that tool's own configuration
  file and takes it back out again. Nothing is installed by default.
- Seven profiles start an agent — Claude Code, Codex, Copilot CLI, Kimi Code, pi,
  Hermes, OpenCode — found on the `PATH`; on Windows, one installed inside WSL is
  run from the WSL profile. Any program that writes `OSC 1337;RequestAttention=yes`
  raises the mark, with nothing installed at all.

## Preview beside the prompt: files, PDF, video, web

What is in a file is readable beside the prompt, without opening another
application.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/preview-pdf-dark.png">
  <img src="screenshots/preview-pdf-light.png" width="100%"
       alt="The pointer rests on a file name in the files column and a card has
       come up under it, showing the first page of a PDF above its page count and
       size. The wheel winds the card through the pages.">
</picture>

- A card comes up under the pointer when it rests on a name in the files column:
  the PDF page by page, the video playing, the first lines of the text, the image
  itself.
- The preview pane opens the file beside the prompt — markdown typeset, PDF page
  by page, video playing, a web page with an address field and Back.
- A path the terminal printed opens in the preview pane on a click, and goes to
  the machine's own application on `Ctrl`+click — that one is `Ctrl` on both
  platforms, and not Command on a Mac. Paths nobody marked up are found too,
  once the file is confirmed to exist.
- A web address follows the same rule: a click opens it in the preview pane,
  `Ctrl`+click hands it to the browser.
- A window holds as many pages as it has preview panes. A second page opens on a
  pane of its own instead of navigating the first, so a page can be locked and
  another opened beside it, and a page dropped on a pane opens on the pane you
  dropped it on.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="../assets/readme/surfaces-dark.png">
  <img src="../assets/readme/surfaces-light.png" width="100%"
       alt="Three more surfaces: a window laid out as cards, a markdown document
       typeset in a preview pane, and a web page in a preview pane with a
       breadcrumb address field above it.">
</picture>

## Markdown you can edit where you read it

A `.md` file is typed into on the page you were reading, in the typeface you
were reading it in.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/preview-markdown-dark.png">
  <img src="screenshots/preview-markdown-light.png" width="100%"
       alt="A markdown document typeset in a preview pane beside a terminal: a
       heading, a table, a code block and a display formula, all readable at
       once and all in the reading typeface.">
</picture>

- Click into a paragraph, heading, list or quote and it shows its own
  Markdown — the `#` of a heading, the `**` around a bold phrase, the `- ` in
  front of an item, the `> ` down the side of a quote — in the reading typeface
  and at the reading size, while every other block on the page stays as it
  reads. Leave the block and it is back to its rendered form.
- A code block, a table or a formula turns to monospace source instead, because
  there the way the characters line up is part of what they say.
- The caret moves between blocks the way it moves between lines, so there is
  nothing to enter and nothing to leave: arrows, `Home`, `End`, `Enter` and
  `Backspace` do what they do anywhere else. What you select is what you copy,
  and a copy out of a document you are editing brings the marks with it, so what
  you paste back is what was there.
- `Ctrl+S` writes the file, `Ctrl+Z` takes back the last change and `Ctrl+Y`
  puts it again. A run of typing comes back in one press rather than a letter at
  a time, and the unsaved dot goes out when you undo back to your last save.
- A save changes the part you edited and leaves the rest of the file identical,
  down to the byte. A file that names its own encoding in its first bytes —
  what Windows PowerShell writes is the common case — is written back in it.
  Line endings, trailing spaces and a missing last line break survive as they
  always did.
- `Esc` leaves the page, and so does clicking the empty space beside the text or
  clicking away from the pane altogether. The page goes back to reading as a
  page, and the caret stays where you left it.
- A file over 8 MB, and one Folio could not read as text all the way through,
  opens and reads and copies as before but is not edited; the foot of the pane
  says why.
- The files column's right-click menu makes a file or a folder in place:
  `New file…` and `New folder…` put the name field in the tree where the new row
  will appear, `Enter` creates it and `Esc` cancels. A name the folder will not
  take turns red in the field rather than being explained somewhere else.
  `Delete` sends a file, or a whole folder, to the Recycle Bin on Windows and to
  the Trash on a Mac, and asks nothing first, because that is where it goes.
- Right-clicking the empty space in a files column opens the menu of the folder
  the column is standing in, so a folder with nothing in it can still be given
  its first file.
- `Ctrl+Shift+P` finds a file under the folder the column is standing in, and
  `Enter` opens it in the preview pane, ready to be typed into.

## Panes, tabs and windows that move

The layout changes while the sessions inside it keep running, and all of them can
be seen at once.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/tab-into-pane-dark.gif">
  <img src="screenshots/tab-into-pane-light.gif" width="100%"
       alt="A tab is pressed and dragged down out of the tab strip; over the
       right half of the window a landing preview appears, and on release the
       tab's shell becomes the right-hand pane, still running. Then the new
       pane's own header is dragged to the bottom edge, and the side-by-side
       layout becomes two full-width bands.">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/cards-dark.png">
  <img src="screenshots/cards-light.png" width="100%"
       alt="The tab strip has become a column of cards. The single card stands for
       a tab of eight panes and draws all eight in miniature; Alt — Option on a
       Mac — and the wheel scroll the picture inside a card a row at a time.">
</picture>

- `Alt+Shift+-` splits a pane across, `Alt+Shift+=` splits it down. A tab or a
  single pane can be dragged out into a window of its own, and the panes it did
  not touch keep their widths.
- A pane dropped on the join between two tabs becomes a tab *between* them: a
  gap opens where it will land. Dropped on a tab itself it joins that tab's
  layout. The horizontal strip, the vertical rail and the card column all read
  the join the same way.
- `Ctrl+Shift+Z` turns the tab strip into a column of cards, one per tab, each
  drawing that tab's own panes in the layout they have.
- `Ctrl+Shift+G` turns the files column into a Git panel: branch, working tree,
  staged and unstaged files, the commit graph, and a selected file's diff in the
  preview.
- `Ctrl+Shift+↑` and `Ctrl+Shift+↓` step between commands in the scrollback, and
  a command that failed is marked as having failed.
- The folder button over the files column lists the folders your shells are
  standing in, then the last five folders you pointed a column at, each marked
  `recent`.

## A terminal on a hotkey

One key brings a terminal down over whatever is on the screen, and the same key
takes it away again.

**The chord is each platform's own**, and it is claimed from the system rather
than read by a focused window, so it answers from inside any application:
``Win+` `` on Windows and ``⌃` `` on a Mac. It is an ordinary row on the
Shortcuts page — record a different chord and it takes effect at once, and
`Restore all defaults` brings this one back. Neither platform is asked for a
permission for it.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/quake-dark.png">
  <img src="screenshots/quake-light.png" width="100%"
       alt="A terminal window hanging from the top of a screen, a little below the
       edge and centred, over a File Explorer window showing a small project's
       files. The terminal has one tab, a gear and a close button, and its shell
       has printed four commits and a directory listing above an empty prompt.">
</picture>

- ``Win+` `` — ``⌃` `` on a Mac — drops a terminal across the top of whichever
  screen the pointer is on, over whatever was standing there. Pressing it again
  puts the window away and hands the keyboard back to the program it came down
  over.
- It is the same Folio — tabs, panes, the files column, the preview, every
  shortcut — and it keeps its shells and its scrollback between summons.
- A rectangle you move or resize by hand is remembered for the display it is on,
  so the summon comes down where you last put it on that screen.
- It has no icon of its own, and closing the last window you can see ends the
  run.
- **Settings > Summoned terminal** holds the key, which profile a new tab opens
  on, the height, width and the gap below the top of the screen, whether the
  window hides when the keyboard leaves it, and a command to run on the first
  summon of each run.
- At the next launch a pinned tab's last command can come back typed at its
  prompt — typed, and not run.

## Search everything

One box answers five questions at once, and `Enter` goes straight there.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/palette-dark.png">
  <img src="screenshots/palette-light.png" width="100%"
       alt="A box floating over the top of the window, a query typed into its
       field and its results under five headings that do not mix: an action, a
       pane, a command the window has run, a file, and a setting. The first row
       is highlighted, and the letters that matched are marked in each row.">
</picture>

- `Ctrl+Shift+P` raises a box over the top of the window with five sections that
  never mix: what Folio can do, the panes and tabs this window has open, the
  commands it has run, the files under the folder its column is standing in, and
  the settings.
- Typing narrows all five together and the arrow keys walk them. `Enter` on a
  pane raises it, on a file opens it in the preview pane, on a setting opens
  Settings at that row, and on an action does it.
- A command still running is pointed at with a ring around the pane it is running
  in, rather than a scroll to a line that has gone past.
- File search covers the folder the files column is showing.

## Windows integration

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/main-window-dark.png">
  <img src="screenshots/main-window-light.png" width="100%"
       alt="The window in its default font and default scheme: a files column on
       the left, two terminal panes side by side, and a markdown document open in
       a preview pane on the right.">
</picture>

- **Settings > General > Explorer context menu** puts Folio in the folder
  right-click menu, and On is everything your Windows can do. Windows 11 files
  "Open Folio here" under "Show more options"; on Windows 10 it stands in the
  only menu there is. On a Windows 11 with `folio.msix` beside `folio.exe`, On
  also puts "Open in Folio" on the page Windows 11 opens first. Off takes back
  whichever is registered, and the line under the row says which of them On
  reaches on your machine. A running File Explorer reads its list of first-page
  entries when it starts, so if "Open in Folio" is not there yet, sign out and
  back in. [`PRIVACY.md`](PRIVACY.md) lists what is written. If Folio is already
  running, the entry opens the folder as a new tab in the window you used last
  and brings that window forward — it asks for a terminal in a folder, not for
  another Folio, so it does that whatever **Settings > General > Opening Folio
  again** says. Starting Folio any other way opens a window, unless that row says
  otherwise.
- Windows PowerShell 5.1 ships PSReadLine 2.0.0, which misplaces the input line
  after the window is resized. Folio carries a patched 2.4.6 and installs it into
  your module path on request. On a machine whose execution policy is still the
  stock `Restricted`, the switch says so and hands you the `Set-ExecutionPolicy`
  command that lets the module load.

## macOS integration

- **Finder's right-click menu** carries **Open in Folio** under **Services**.
  Folio registers it the first time it runs, so it is there without a sign-out
  and with nothing to enable. A folder opens as a tab standing in it; a file, as
  a tab standing in the folder it is in. Either arrives in the window you used
  last rather than starting a second Folio.
- **The menu bar is the shortcut table.** Every item takes its key from the same
  row of [Shortcuts](shortcuts.md) the keyboard does, so a verb has one
  name and one key wherever you meet it.
- **The Dock icon is the attention channel.** A waiting agent bounces it until
  you come back; a command that reports how far along it is puts that on the
  icon as a badge.
- **Paths are written from your home directory.** A file under it reads
  `~ › …` in the files column, and the `~` is a step you can click like any
  other.
- **Three Settings rows are not on a Mac at all** — the Explorer context menu,
  the PowerShell integration and the PSReadLine repair. None of the three has a
  macOS counterpart, and a greyed row explaining a mechanism the machine does
  not have only teaches a Windows word. One row is a Mac's alone: **Option key
  sends Alt**, off by default, so that Option keeps typing the character it is
  printed with.
- **Not on macOS:** the sparse-package route to the first page of a right-click
  menu, and the Store video extensions. macOS is **arm64 only** in this preview.

## Visual Studio Code

**On Windows**, `folio-here.cmd` ships in the archive, beside `folio.exe`, and
is one line:

```bat
@"%~dp0folio.exe" --from-here --cwd "%CD%"
```

Point VS Code's external terminal at it — Settings, or `settings.json`:

```json
"terminal.external.windowsExec": "C:\\Tools\\folio\\folio-here.cmd"
```

**Terminal > Open in External Terminal** (`Ctrl+Shift+C`) then opens Folio on the
folder the editor is standing in. The `.cmd` exists because that setting runs a
command with no arguments, and `--cwd` is how Folio is told where to start;
`--from-here` says that this is a terminal in a folder rather than another
Folio, so it arrives as a tab in the window you used last whatever
**Settings > General > Opening Folio again** says.

On macOS that setting names an application rather than a command, so there is no
`folio-here` for it to run. Finder's **Open in Folio** above is the way to put a
folder in front of a shell.

## English and Chinese

Every string a reader can see is written in both, and the interface is one of
them at a time.

- **Settings > General > Language** offers English, 中文, or the system setting,
  and a change reaches every pane, menu and dialog the moment it is made —
  nothing has to be restarted for it.
- The front page is written twice as well: [`README.md`](../README.md) and
  [`README.zh-CN.md`](../README.zh-CN.md), and this document has
  [a Chinese half](features.zh-CN.md).
- The shortcut table, the settings descriptions and the messages a pane prints
  are all in the same two languages, so switching does not leave a row of one
  standing in the other.
