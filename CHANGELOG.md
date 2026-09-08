# Changelog

All notable changes to Folio are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- **A file inside the folder card previews on hover, like a file anywhere
  else.** Hovering a folder path printed in a pane opens the folder card and its
  tree; resting on a file row inside that tree showed nothing, while the same
  name in the file column, in a window you tore off, or printed in the output
  all answer a resting hand with a preview. It now opens that file's preview
  beside the folder card — on whichever side has more room — after the same
  350ms, at the same size, showing the same thing: a Markdown file rendered, an
  image drawn, a PDF as a column of pages you can scroll. Move off the row and
  it goes; rest on another file and the preview follows you to it. The folder
  card stays where it is the whole time, including while you are reading the
  card it opened. It stops at one: the preview never opens a card of its own,
  and a folder row inside the folder card still just expands.

### Changed

- **The repository tree is nine directories.** Fixtures moved under `tests/`
  (`tests/corpus/` for the recorded sessions, `tests/assets/` for the documents
  and videos the preview tests open), the two CI helpers under `scripts/ci/`,
  the design prototypes and the icon round under `docs/design/` with the shipped
  mark and its generators under `assets/app-icon/`, and the August handoff under
  `docs/handoff/`. The spike workspaces left the tree — they were research
  records in no build graph, `docs/spikes/README.md` says where to read them in
  history, and the two files the program actually needed from them are now
  `assets/mitex-specs/` and `tests/corpus/`. `docs/BUILDING.md` lists what each
  directory holds. Nothing about the program changed.
### Fixed

- **A name ending in a dot or a space can no longer smuggle a program past the
  file column.** Double-clicking a row in the file column opens the file and
  refuses to run one, and the refusal read the name exactly as it was written.
  Windows does not: it drops trailing dots and spaces before it looks anything
  up, so a file called `invoice.exe.` was read as a document by Folio and
  started as a program by Windows. Folio now reads the name the way Windows
  will, and the check and the launch use that one name. Paths written in the
  form that asks Windows to skip its own reading are not opened at all.

- **Show in Explorer only hands over a file that is really there.** The path
  went onto Explorer's command line wrapped in quotes and nothing else was
  asked about it, so a path that no longer existed opened whatever folder
  Explorer falls back to — which looks like Folio having shown you the wrong
  thing — and a name carrying a quote character would have been split into two
  arguments. Folio now resolves the path against the disk first, hands over the
  answer the operating system gives, and shows nothing at all when there is
  nothing there.

- **The card that refuses to read a file on another machine no longer offers to
  open it.** Folio does not read a network share unless you ask, because a
  disconnected share can hold the window for as long as the network takes to
  give up. The card that says so carried the same *Open in default app* button
  as every other card, and one press handed the share straight to the shell —
  the wait that had just been refused. That button is now on the cards it is
  true of: the ones that cannot read *what is in* the file. A card refusing a
  share, or reporting that the disk said no, has no button.

- **A local page in the preview stays a local page.** A `.html` file opened in
  the preview could navigate itself anywhere — a link, a redirect, or the
  page's own script would take the seat onto the network, carrying whatever the
  local document had reached. The preview now goes to the file it was opened
  with and nowhere else, and says so when a page tries.

- **A page can no longer send an address to your browser on its own.** A page
  that starts a download has that download cancelled — Folio's preview writes
  no files — and the address used to go straight to your real browser, at a
  moment the page chose. It now goes onto the card the cancelled download
  raises, and the browser opens when you press the button.

- **A link with a name in front of the host is refused before your browser
  opens.** `https://your-bank.example@somewhere-else/` shows one name and goes
  to another, and Folio's address bar has always refused it. Ctrl-clicking the
  same address printed in a pane handed it to your browser anyway. Both now
  read the one rule, and the hover line says the address is refused.

- **A title or a hover address cannot be written backwards.** A program can put
  the invisible characters that reverse text into the title it sets, so a tab
  could read `report.txt` over a pane running `report.exe`, and the address
  under the pointer — the one a Ctrl-click is about to open — could show a
  different host than it goes to. Folio now removes those characters from tab
  titles and pane heads and marks them in the hover address. Titles in Chinese,
  Arabic and Hebrew are unchanged.

- **A program Folio starts to ask a question is the one that is installed.**
  The agent version check, the PowerShell probes and the shell-integration
  probe named their programs without a path, and Windows looks in the folder
  the process is standing in before it looks anywhere else. So a `copilot.cmd`
  or a `powershell.exe` left in a folder you had opened would run — opening
  Settings ▸ Agents was enough. Folio now finds these programs itself, in the
  places programs are installed, and never in a working folder.

- **A directory a shell reports cannot turn into a network share.** A shell
  says where it is standing with a short message, and a percent-escaped
  backslash inside one was read back as a path separator, rebuilding a
  `\\server\share` address that the next new tab then asked the network
  about. Such a message now names nothing, and a shell's report is never read
  as a share.

- **An SVG can no longer make Folio open a file it names.** An SVG document can
  point an `<image>` at a path, and Folio's renderer used to follow it: a file
  anywhere on the machine, or on a network share, was read while the picture was
  being drawn. A `.svg` file only has to be printed in a pane to get there,
  because a printed image path is drawn on sight, so a file someone sent you
  could read `C:\Users\you\.ssh\id_rsa` into the picture, or reach a share
  belonging to whoever wrote it and hand over your Windows sign-in along the
  way. Folio now draws only the images an SVG carries inside itself and follows
  no path out of one. Formulas and the author's own artwork are unchanged: they
  never pointed anywhere.

- **A repository Folio only reads no longer gets to run a program of its
  choosing.** A repository keeps its settings in a file that travels with the
  folder, and two of those settings name programs for git to run: one on every
  status check, one whenever a file is diffed. Folio ran git without turning
  them off, so opening the Git page on a folder somebody sent you ran whatever
  that folder asked for, and a click on a printed folder path is enough to open
  it. Every git command Folio runs now switches those off, along with the
  per-file diff and text-conversion programs a repository can name for its own
  paths.

- **Starting Folio no longer takes the right-click menu's first page off
  another copy of Folio that is still installed.** On Windows 11 the entry on
  the first page of the menu belongs to whichever folder Folio was last
  registered from, and every launch used to claim it for the folder it was
  started from. So running a second copy once — a build you were trying out, a
  copy still sitting in `Downloads` — pointed that menu item at the second copy
  for good, and the install you actually use went on standing where you left
  it. A launch now looks at the folder the menu item names first: if `folio.exe`
  is gone from it, the item is repaired to point here, which is what happens
  when you move Folio and is the whole reason the repair exists; if the file
  there is this very one under another spelling, the item is rewritten for the
  same reason; and if another Folio is still standing there, nothing is touched
  and the Explorer row in Settings says the menu item belongs to a copy in
  another folder. Switching the row on by hand still names the Folio you
  switched it in, exactly as before. The classic entry under *Show more
  options* has behaved this way since the previous release; the two now decide
  it with one piece of code.
- **A card of a pane that repaints its screen in one piece is no longer a frame
  behind.** A full-screen program can ask the terminal to hold a whole frame
  back and put it up at once. Where the terminal was the one to put it up — the
  program's end-of-frame marker having come too late — the pane showed the new
  frame and that pane's card in focus mode kept the picture from before it,
  until the program printed something else. The card is keyed to a count of the
  moments the screen could have changed, and putting a held frame up was the one
  way to write the screen without moving that count. It moves now, the way a
  resize's reflow has always moved it, so the card shows the frame on the frame
  it lands. A held frame that turns out to carry nothing still costs nothing.
- **A path printed by Git Bash or by WSL is a link, in the spelling those
  shells print it in.** `D:\Demo\report.md` was recognised in every pane and
  `/d/Demo/report.md` was recognised in none — so in the two shells that spell
  a path their own way, the names on the screen were the ones you could not
  click, could not hover for a preview, and could not `Ctrl`-click into an
  editor. A pane now reads the spelling its own shell prints: `/d/Demo` and
  `/c/Users` in a Git Bash pane, `/mnt/d/Demo` in a WSL one, `~/notes/a.md` in
  Git Bash, and a relative name in a WSL pane measured from the folder that
  pane reported. The file still has to be on the disk before anything is drawn,
  exactly as before. Which spelling a pane reads comes from the profile it was
  started from and not from the text, so `/d/Demo` typed into a PowerShell or
  Command Prompt pane is still ordinary words.
- **A file inside a WSL distribution opens too**, which the entry above had left
  as the one thing a WSL pane still could not click. `/etc/hosts`,
  `/home/you/notes.md` and `~/notes.md` printed in a WSL pane are now links, to
  the same file Windows opens at `\\wsl.localhost\<distribution>\…` — a plain
  click puts it in the preview beside the pane, `Ctrl`-click hands it to the
  machine, and the folder it sits in points the files column at it. Which
  distribution is the one that pane's profile starts: its own `-d` argument, or
  the one `wsl.exe` starts by default. `~` is whatever the shell said its home
  was when it opened. This is one translation in one kind of pane and nothing
  wider: a share printed anywhere — `\\server\share\notes.md`, and a
  `\\wsl.localhost\…` written out as text in any pane — names nothing, exactly
  as before, and a network share still meets the card that says this window does
  not read one unasked.
- **A path right after a prompt's `user@host:` opens.** Ubuntu's own prompt
  writes the folder you are standing in behind that colon, and the colon was
  read as the kind that belongs to a scheme — so `alice@box:/mnt/d/Demo$` and
  `alice@box:~/notes` were words. A colon with a host name in front of it is a
  prompt's separator, and the path behind it is a path; the shape an `scp`
  address is written in is the same one. Nothing else moved: `http://…`,
  `scheme:/opaque` and the `:`-separated list a shell prints its `PATH` as are
  refused exactly as they were.
- **The tick on the prompt you are typing at no longer says a command is
  running.** Every prompt gets a tick the moment it is drawn, and hovering the
  newest one — the prompt with nothing typed into it yet — read `running ·
  command`. Nothing was running and nothing had been typed: the record had no
  ending because it had no beginning. It now says `at the prompt`. Reported in a
  Command Prompt pane, where the shell reports its prompts and nothing else, but
  the tick was the same in every shell. A line you started and abandoned with
  `Ctrl+C` still shows what you typed.

- **A tab is named after the folder its pane is standing in, whatever shell is
  in it.** A PowerShell tab used to be called `PowerShell 7` for the life of the
  pane — with the pane standing in `D:\Demo`, and its own pane head saying so —
  because the integration script ends every prompt by announcing the name the
  profile already goes by, and the tab believed it. A shell that merely repeats
  its launcher's name has announced nothing, so the folder names the tab. A
  shell that sets a title of its own is still shown saying it (Git Bash's
  `MINGW64:/d/Demo`, and the command a Command Prompt pane is running), and a
  tab you renamed still keeps your name.
- **A tab wears its folder from the moment it opens**, rather than the profile's
  name until its shell gets as far as its first prompt. The name now reads the
  whole ladder the rest of the window already reads — the shell's last report,
  else the folder the profile started it in — so a pane whose shell reports no
  directory at all is named after where it was put down.

## 0.2.3-preview — 2026-09-07

Most of this release is about reading what a pane is showing, and about the
panes that were left out. Command Prompt carries command marks and its working
directory in its own prompt, and the first WSL pane of a run is integrated like
every one after it. A tab's card follows its pane whatever shell is in it, and
draws everything that pane is showing. A Markdown document is set in a column
wide enough to use a maximised window, a wide table in it scrolls sideways under
the tilt wheel most mice already have, and a link whose text carries code or
emphasis is drawn as a link. In the terminal, a pane wears a scroll bar only
when it has something to scroll to, its thumb rides the pane's own edge, and a
tick on the command strip still lands on that command's prompt after the pane
has been split or dragged. The two Explorer rows in Settings became one switch
that does whatever this Windows can do, and a change it makes is announced to a
File Explorer that is already running; the four picture-in-picture rows left the
Shortcuts page until the window they summon exists; and the interface reads
better in both languages after an audit of every string a reader can see.
Underneath, the core now compiles on macOS and a job on every push keeps it that
way — groundwork, not a port.

### Added

- **A Command Prompt pane has a command rail.** `cmd.exe` has no startup file
  to hand a script to, so until now its rail was empty however many commands had
  been run: nothing to click, and `Ctrl+Shift+↑`/`↓` with nowhere to go. Its one
  way in is the `PROMPT` variable, and what fits there is what describes the one
  moment `cmd` expands it at — just before it reads a line, which is the end of
  the last command and the start of this prompt at once. So a `cmd` pane now
  reports both, and every prompt gets a tick that lands on its own prompt row.
  Whatever `PROMPT` you had set is kept and reported in front of, never replaced,
  and a `cmd` started from a `cmd` does not report twice. Two things `cmd` cannot
  say, it does not: a tick carries no exit code, because `PROMPT` has no way to
  read one, and nothing marks where a typed line ends, so an inline `$…$` in a
  `cmd` pane is still read as text. Display formulas and image previews in its
  output are exactly where they were.

- **A wide table in a Markdown preview scrolls sideways, and a tilt wheel is
  enough to do it.** A table whose columns need more room than the page can give
  has always had a bar along its own foot that a hand could drag, and a
  `Shift`+wheel that moved it. What it did not have was the wheel most mice
  already carry: a tilt wheel, and a touchpad's second finger, were reported to
  the window all along and dropped before they reached the page. They are not
  dropped any more, they need no modifier, and the table under the pointer is
  the one that moves — a page with several wide tables has several scrolling
  regions, exactly as a browser does. `Shift`+wheel still does what it did,
  everywhere it did it.

### Changed

- **A Markdown document is set in a wider column.** The reading column was
  capped at 702 logical pixels, which is Typora's own 860-pixel page carried
  across to this window's smaller body type. On a maximised window that left a
  document reading down a strip in the middle of the pane. The cap is now about
  a thousand pixels. Everything else about the column is unchanged: it is still
  centred, a pane too narrow for it still gets the whole pane, and prose still
  stops there rather than running the width of the window. Tables and code
  blocks are set in the same column as the prose, exactly as before; one wider
  than the column scrolls inside itself.

- **The core builds on macOS, and CI keeps it that way.** Everything below the
  application layer — the terminal grid, the renderer, the transcript, the
  document model, the detectors, the layout, the maths — now compiles on a Mac,
  and a job on every push compiles it there so that it goes on doing so.
  Nothing about Folio on Windows changes: this is groundwork, not a port, and
  there is no Mac build to download. What it buys is that the next feature
  cannot quietly assume Windows without somebody being told the same day.

- **The welcome card draws no line between its rows.** The Settings page — whose
  row shape this card borrows, and where every one of its rows also lives —
  draws none either. The rows keep their 42-pixel rhythm and abut; what separates
  Folio's own rows from the agents found on this machine is 16 pixels of air. A
  row under the pointer is no longer filled: it still carries its tooltip and a
  press anywhere along it still flips that row's switch, which is what the
  Settings page does too. The ring on a switch waits for a key that moves
  something rather than for any key at all, and it is no longer clipped at its
  right-hand side when it comes.

- **A terminal pane's scroll mark rides the pane's own edge.** Every other bar
  in the window — the glance card's, an open picker's, a preview pane's — puts
  its thumb against the inner edge of the surface it belongs to, and this one
  stood two logical pixels off its own. The bar along the foot moves with it,
  because it is the same instrument turned. Nothing else about the edge changed:
  the reserved lane is the same eight pixels, the command marks beside it have
  not moved, and the thumb is grabbed and dragged exactly where it was.
- **The Shortcuts page no longer lists `Summon picture in picture`.** Those four
  rows offered a key for a window Folio cannot summon yet: a chord could be
  recorded into one, pressing it did nothing, and the only place that was said
  was a line under the row that appeared once the chord was already there. A
  shortcut you can set and cannot use is not a setting, so the four rows are off
  the page and out of `docs/shortcuts.md` until the window they summon exists.
  Nobody loses a chord they had already recorded: `keybindings.json` still names
  all four slots, a chord written into one stays in the file exactly as it was,
  and `Restore all defaults` still clears it. The rows come back, with their
  names and their Record buttons, the day the window does.

- **The two Explorer rows in Settings are one switch.** `Explorer context menu`
  and `First page of that menu` asked one question twice, and the second was
  meaningless without the first. There is now one row, and it is on or off like
  every other switch on the page. **On is everything this Windows can do**: on
  Windows 11 with `folio.msix` beside `folio.exe` it puts "Open Folio here"
  under `Show more options` and registers the package that puts "Open in Folio"
  on the page Windows 11 opens first; anywhere else it writes the menu entry
  alone. Off takes back whichever of the two is there. Because On means
  different amounts on different machines, the line under the row says what it
  does on yours — and on a Windows 10 that line names no page, because that
  Windows has one menu. The row is now on the General page of every Windows
  rather than the pair being a Windows 11 shape, the card that used to say
  `folio.msix is not beside folio.exe` after a press is gone, and nothing about
  where the answer is stored changed: it is still read off the registry and off
  the deployment database, so removing the package from `Settings > Apps >
  Installed apps` still moves the row. The welcome card's Explorer switch means
  the same thing it always did, and now says so by calling the same function.

- **The Chinese interface reads more naturally.** A native-writing review of
  every Chinese string a reader can see reworded 52 of them: settings
  sentences that had been carrying a reassurance nobody asked for, lines that
  explained an internal mechanism instead of what the switch does, and words
  left in English — `tab`, `profile` — where the rest of the Chinese says
  标签页 and 配置. No fact changed, no default moved, and the English is
  untouched.

- **The English interface reads the way an English writer would put it.** The
  83 strings an audit proposed are in, along with the passages it proposed in
  `README.md`, `docs/PRIVACY.md`, `SECURITY.md` and the 0.2.2 release page. No
  em-dash is left inside a UI string, a sentence whose only job was to say what
  Folio will not do is gone, and a mechanism the reader cannot act on gives way
  to the result they get. No fact changed, no default moved, and the Chinese is
  untouched. `docs/plans/copy/en-copy-audit-2026-09-07.md` has every proposal
  and the width each one was measured against.

### Fixed

- **A card keeps up with its pane whatever shell is running in it.** The cards
  in the tab column were refreshed by the same clock that breathes a tab's mark
  while a command runs, and that clock is started by a shell reporting its own
  prompt. So a PowerShell or Git Bash card followed every row, while a Command
  Prompt card caught up at the next prompt and a WSL pane that reports nothing
  at all caught up whenever something else happened to repaint the window — most
  visibly at the end of a burst, where the last rows a pane printed could sit
  unreproduced on its card until you moved the pointer over it. A card now
  refreshes because the pane it is a picture of changed, which is the same thing
  for every shell. It costs no more than it did: the refresh rides the frame the
  window was already drawing at, a card still redraws at most ten times a
  second, and a card you cannot see — column collapsed, tab scrolled out of the
  list, mode off — costs a comparison and no frame at all.
- **The first WSL tab of a Folio window is integrated like every other one.** A
  WSL pane gets its command marks, its working directory and its clickable
  paths from a small script handed to the shell the distribution logs you into
  — and which shell that is, is a question only the distribution can answer.
  Folio used to ask it by starting a second `wsl.exe` beside the pane and
  never waiting for the reply, so the *first* WSL pane of every run went out
  before the answer existed and was started without the script: no ticks on
  the rail, no folder in the tab or the files column, no `Ctrl+Shift+↑`/`↓`,
  and a card in `Cards` that never refreshed while a command ran. The second
  WSL tab you opened worked, and every one after it — which on a machine whose
  default profile is WSL is no consolation, because the first pane is the only
  one there is. The question now travels *in* the pane's own command line and
  the distribution answers it about itself, so there is nothing left to wait
  for and every WSL pane is composed the same way. A distribution that logs you
  into zsh or fish still keeps its shell, untouched, exactly as before.

- **Folio now tells Windows when it changes the folder right-click menu.** Every
  program that registers a menu entry has to announce it, and Folio never did.
  A running File Explorer reads the association and context-menu tables once and
  goes on drawing what it read, so an entry written into a live session — the
  classic "Open Folio here", and the packaged "Open in Folio" that Windows 11
  puts on the page it opens first — could be registered correctly, verifiably
  present on the machine, and still invisible until the next sign-in. Both
  directions of both registrations now end with the announcement, on a refusal
  as well as on a success, because a refused write can still have changed half
  of what it was writing.

  The first page is the one Windows does not promise to refresh on that
  announcement: its list of entries belongs to the packaging system rather than
  to the tables the announcement is about, and a File Explorer that has been
  running since before the registration is reported to show a new entry late or
  only after it restarts. So where Folio itself registered the package during
  this run, the Explorer row and the card that follows the registration both say
  so and give the one step that always works — sign out and back in. Folio does
  not restart anybody's File Explorer.

- **A tab's card draws everything its pane is showing, not only what is still on
  screen.** A card is a picture of the pane under it, and it was reading the
  terminal's live screen alone — so a pane that had scrolled and was then made
  taller, which leaves it showing lines from its own scrollback above the ones
  still on screen, was pictured by a card holding nine rows at the top with two
  thirds of itself empty while the pane showed twenty-four. It was reported
  against Git Bash sitting beside PowerShell 7, and nothing about it belonged to
  either shell: both are in the same position after the window is resized, and
  what separated them was only that one had gone on printing until its screen
  filled again. A card now reads the same three places its pane reads — the
  lines that have been kept, the ones on their way there, and the ones on
  screen — and stops when it is full or the pane has no more to give. Turning
  the wheel over a card can now lift its window into lines that have scrolled
  off, for the same reason. A full-screen program is the one exception: nothing
  is kept behind its screen, so the card stops exactly where the pane does.

- **A tab mark hook writes the file it says it writes.** The first-run card's
  three agent rows name the file each switch will copy and then write, and they
  spelled `~/.claude/settings.json`, `~/.codex/config.toml` and
  `~/.copilot/hooks/folio.json` whatever the machine was set to. Claude Code
  reads `CLAUDE_CONFIG_DIR`, codex reads `CODEX_HOME` and Copilot CLI reads
  `COPILOT_HOME` before any of them looks beside your profile, and so does
  Folio's installer — so on a machine that sets one of those, the consent
  disclosure named a file that was never touched. Each row now names the file
  its own machine will actually write. Nothing moved on a machine that sets
  none of the three: the spelling there is the one it always was.

- **An agent installer that refuses says why in one sentence.** "Copilot CLI's
  hooks were not changed: copilot 1.0.26 or newer is needed for this" — the
  reason the installer gave now follows a colon rather than a dash, and a reason
  long enough to need a second line gets one instead of being cut.

- **The README, the changelog and the design note no longer say the welcome card
  asks six questions.** It offers as few as two: an agent that is not on the
  machine is not listed at all. They also said every row on it writes outside
  `%APPDATA%\Folio`, which was never true of the update-check row — that one
  writes Folio's own `settings.json`, inside that folder.

- **A second copy of Folio no longer takes over your "Open Folio here" menu
  entry.** The launch has always rewritten a right-click entry that names a
  different folder than the one it is running from, so that moving `folio.exe`
  and its files to another folder keeps the entry working. But "names another
  folder" was also true of an entry belonging to a Folio still sitting there and
  answering it perfectly well — a second copy run once out of a downloads folder
  quietly became the one your menu ran, and if you then deleted that copy the
  entry pointed at nothing. Now a launch rewrites the entry only when nothing is
  at the path it names, or when that path is this very file. A copy started
  beside an installation that is still there leaves the menu alone. Moving
  `folio.exe` still works exactly as before, because after a move there is
  nothing at the old path; and if you do want the copy to take the entry,
  Settings > General > Explorer context menu writes it the moment you ask.

- **A link whose text is set in code, or carries emphasis, is drawn as a link.**
  In the Markdown preview, a line reading ``[`folio-0.2.2-windows-x64.zip`](https://…)``
  printed its own Markdown source, brackets and address and all, while the
  plain-text link beside it on the same line rendered. The reader of code spans
  and formulas ran first and handed on what it had not claimed, and the reader of
  links then looked for a pair inside each leftover: anything at all in a link's
  text — a code span, a formula, a picture — left the opening bracket in one
  leftover and the closing one in another, and neither could see a pair. Brackets
  are now matched along the whole line, the way emphasis already was, and a
  link's text is read as what CommonMark says it is: ordinary inline content. A
  code span in it stays monospace and takes the link colour, the way it does on
  GitHub; a bold word stays bold; a formula stays a formula; and every one of
  them answers a click. `[**bold** text](url)` no longer shows its asterisks,
  a picture wrapped in a link — a badge — draws its picture, and a picture whose
  description contains code says what that code says. Where the specification
  puts a code span ahead of the brackets it is still ahead of them: `` [foo`](/uri)` ``
  is a literal bracket beside a code span, and not a link.
- **A terminal pane whose whole transcript fits wears no scroll bar, and a pane
  scrolled to the top shows its thumb at the top of the track.** After one run of
  a script that prints display formulas, with the prompt back and empty space
  below it, a thumb was drawn down the right edge of a pane that could not be
  scrolled at all — starting a fifth of the way down and running flush to the
  bottom. A display formula makes its row taller than a row, so the live screen
  stands taller than the pane it is drawn in, and the blank rows under the prompt
  give that height back where they can, which is why the pane looks complete
  standing still. Those given-back pixels were still being counted as somewhere
  the view could travel to. They are not, and the bar, the wheel and the question
  of whether a pane has any history to look at now all read the same number the
  frame itself is clamped by. On a pane that really does scroll, the thumb's top
  is the track's top when the view is at the top, and its length is the pane's
  share of the whole transcript.

- **A tick on the command strip lands on the command's own prompt row again,
  after a pane has been split or resized.** Pressing the newest tick used to drop
  the reader into the middle of that command's own output, with the highlight on
  the wrong row and the prompt line above the top of the pane. A command mark is
  taken on the screen cell its prompt was drawn on, and a resize that changes the
  width re-wraps every line on the screen at the new width, so the rows that no
  longer fit leave the top and that cell now holds somebody else's text. The
  resize used to re-date every such mark onto the new screen without looking at
  what was under it. It now writes down the whole wrapped line each mark sits in
  before the screen is re-wrapped — the one thing a re-wrap keeps — and puts each
  mark back on that line afterwards, whether the line is still on the screen or
  has scrolled out of it. Two runs of the same command draw the same prompt line
  twice, so which is which is settled by their order, which a re-wrap also keeps.
  Dragging an edge is a run of re-wraps rather than one, and each of them moves
  the rows the last one pushed off, so a mark is carried through every step of
  the drag and not only the first. A mark whose line the re-wrap genuinely lost
  leaves the strip instead of pointing somewhere the command never was.

## 0.2.2-preview — 2026-09-06

Fixes and polish for 0.2.1-preview, and one card: a machine that has never run
Folio is welcomed once and asked its boundary questions together, on a card that
presses the Settings rows rather than doing anything of its own. Everything else
here is something that was already meant to work — a pane dropped between two
tabs, a menu that lists only the shells this machine can start, a page that opens
on the pane it was dropped on, two pictures side by side in two preview panes,
and the formulas on the screen of a program that repaints itself, folds a line
too long for the pane, or has pushed the top of a block off the window.

### Added

- **Welcome to Folio: one card, once, on a machine that has never run Folio.**
  It asks whether to check for updates, add Folio to the folder right-click
  menu, enable the PowerShell integration, and mark the tab for each of Claude
  Code, Codex and Copilot CLI this machine has, and it asks them together,
  because they are one decision about how much of this machine Folio may touch.
  Each row is one line, and the line is what you get. The update check arrives
  on; the rest arrive off. A machine with no agent on it is asked fewer. Rest
  the pointer on a row and it says how — including which of your own files it writes, and that
  the file is copied to a dated backup first. **Done** applies the rows that are
  on. **Not now** and `Esc` close the card with the shipped values and change
  nothing. The shell behind it has been running the whole time.
  - **Every row on the card is a row in Settings**, and the card presses those
    rows rather than doing anything of its own — so nothing on it is a last
    chance, and the switch you find in Settings an hour later is the same switch
    in the same place on the same shape of row.
  - A row is only offered if it can be honoured. An agent that is not on this
    machine, or whose configuration already calls Folio, is not listed at all,
    and when none of the three is there the rule above them goes with them. On
    Windows 11 unpacked without `folio.msix`, the Explorer row still offers the
    entry it can offer, worded for it.
  - The PowerShell row records an intent rather than acting: where your
    `$PROFILE` is comes from the shell, so the line is added by the next
    PowerShell that starts, and `Settings > Terminal` says so until it does. As
    always, the file as it stood is copied to a dated backup beside it first.
  - Success is silent — you asked for these a moment ago and the Settings rows
    now show it. Anything that fails still says so, in the same words it says it
    in on the Settings page, and the card still closes.
  - **If you were already using Folio, you never see it.** The step that brings
    your `settings.json` up to date is what records that.

- **Dropping a pane *between* two tabs is now a target you can hit.** The join
  between two entries in the tab list is a band eight logical pixels either
  side, and a pointer inside it makes the pane a new tab there rather than
  handing it to the tab it happens to be over — the list opens a slot and the
  pane stands in it, which is where releasing puts it. It takes four more pixels
  to leave the band than to enter it, so the open slot and the tab highlight do
  not trade places under a hand that is holding still. The horizontal tab strip,
  the vertical rail and the card column all read the same rule.

### Changed

- **A menu that starts a shell now lists only the profiles this machine can
  start.** `Split with`, on a pane head and in the terminal menu alike, drew
  every profile it knew and greyed the ones whose program is not installed —
  on an ordinary machine five of the seven agent rows, filling half the
  submenu with lines a press could not spend. Those rows are simply not on
  those lists now, and neither are shells this machine has not got. All twelve
  profiles are still on the Profiles page in Settings, greyed there and naming
  the program each one looked for, which is where installing one and having its
  row come back can be read about. A profile you made yourself is never left off
  a menu, whatever its program resolves to.

- **`settings.json` is read and written forward.** The card's two keys are new,
  and a file written by 0.2.1 is brought up to date the first time 0.2.2 reads
  it: automatically, with nothing to do, nothing to delete and nothing to
  re-enter. That step is also what records that the card has been shown, so a
  machine that was already running Folio never meets it. `session.json`,
  `profiles.json`, `keybindings.json` and `pins.json` are read and written
  exactly as 0.2.1 left them, and no shortcut, default or file location has
  moved.

### Fixed

- **An inline `$…$` formula that the line wraps through is typeset like any
  other.** Where a line folds is decided by how wide the pane happens to be, and
  it was deciding something else as well: a formula whose closing `$` had been
  pushed onto the next row stayed as source text, while every other formula in
  the same output — including the same formula one column of pane width wider —
  was drawn. Nothing was recorded against it either, so a diagnostic trace of
  that screen showed no failure and no formula: it simply was not there. Folio
  reads a wrapped line as the one line it is when it looks for formulas, and now
  reads the same line when it draws one. The picture goes where the formula
  begins, over the formula's own characters wherever the fold has put them, and
  it may be as wide as the characters it replaces on the row it is drawn on —
  which is the same rule as before on a line that does not wrap. A formula whose
  picture cannot fit there keeps its source, as it always has.

- **A `$$` block under a formula whose top has gone off the screen is drawn
  again.** A full-screen program owns its whole window and moves its transcript
  up by redrawing it, not by scrolling, so the topmost formula's opening `$$`
  can leave the window without a single row being removed and without anything
  noticing. The first `$$` still on screen is then that formula's *closing* one,
  and reading it as an opening shifted every `$$` below it by one place: each
  pair then enclosed a heading instead of a formula and was refused as ordinary
  text, and the last block on the screen — the one in the report — was never
  even paired. It showed its own source with nothing recorded against it, not
  even a failure. Folio already knew how to read a screen that begins in the
  middle of a formula, but it could only say so when a line of scrollback stood
  in front of that screen, which in a program of this kind never happens. That
  question is now asked of any window, however it begins, so the formulas under
  a half-visible one are typeset like all the others.

- **A pane you drag over a web preview can now be dropped there.** The landing
  outline was drawn correctly over the page, but letting go did nothing: the
  press router handed every mouse button inside a page to the browser, releases
  included, and every gesture that spends a release — the drop, the divider, the
  video scrubber, the preview thumbs and pans, the terminal's own selection — is
  answered below that line. A hand that is already carrying something no longer
  counts as a hand hovering a page, so the release reaches the gesture that
  started it. While you are carrying something the page also stops lighting its
  own links under the pointer and stops replacing the drag cursor with its own.

- **A window holds as many web previews as it has preview panes.** Opening a
  second page in one tab used to navigate the first pane and leave the new one
  standing on its empty placeholder. A page now lands where every other preview
  lands — the first preview pane that is not locked, or a new one when there is
  none — so locking a page and opening another puts them side by side, each with
  its own engine, sharing the one browser profile they always shared. A page that
  cannot be reached or downloaded shows its card over its own pane rather than
  over the first page in the tab.

- **A page dropped on a pane opens on that pane, and the preview beside it keeps
  what it was showing.** Dragging an `.html` or a `.pdf` out of the file column
  onto a pane of its own split the layout where you aimed, then opened the page
  somewhere else: in the first preview pane of the tab, replacing the document
  you were reading there, while the pane the drop had just made stood on its
  empty placeholder. Two symptoms, one cause — where a page opens was decided
  twice, and the second answer overrode the pane you had aimed at. A page now
  opens where you put it, exactly as a picture or a text file already did, and
  the rule that picks a pane for you is asked only when nothing else has. This
  also covers dropping a page onto the middle of a locked preview pane, a page
  carried out of a hover card into its own window, and renaming a file into a
  page's name (`notes.md` → `notes.html`) on a pane that is locked. A page that
  turns out not to be on the disk shows the reason on the pane it was aimed at,
  too.

- **A formula on the screen of a program that repaints itself now gets
  typeset.** A full-screen redraw writes every row, so every row arrives as a
  change even when not one byte of it moved, and a display block (`$$ ... $$`)
  is only typeset once the rows it sits on have been still for a moment. A
  program that redraws more often than that — Claude Code redraws at a median of
  106 ms, and the wait is 200 ms — kept restarting the wait, so a block that
  landed on the screen while the program was busy stayed as its own source text
  for as long as it was there, sitting beside another block that had been drawn
  during a lull and was a picture. Both are pictures now: a row rewritten with
  the bytes it already had counts as unchanged, whether or not there is already
  a formula on it, and a row that really does change — a character or a colour —
  still restarts its wait exactly as before. Markdown tables and the pictures
  drawn under image paths wait on the same stillness, so they come back on those
  screens too.

- **A formula the pane was too narrow to hold on one row now gets typeset too.**
  When a line is longer than the pane, the terminal folds it onto the next row,
  and the fold can land on a space. Folio read each row on the screen with its
  right-hand blanks removed — right for a row that ends a line, wrong for one
  that carries on — so the two halves were welded together and the space between
  them was lost. `\quad g_i(x)` folded at that space read back as `\quadg_i(x)`,
  which is not a command, and the whole block stayed as its own source text at
  that one pane width while typesetting at every other. One column of window
  width was the whole difference, which is why the same answer from Claude Code
  drew some of its formulas and not others, and why making the window smaller
  could bring back a formula while taking away the one beside it. A line is now
  read as the line the program printed, wherever the pane happened to fold it —
  so a wider or narrower window changes where the fold falls and nothing else.
  Markdown tables and the pictures drawn under image paths are read the same way.

- **A picture stays in its own preview pane when a second preview pane is open.**
  Open an image, then open a page or a text file beside it, and the picture went
  somewhere else: drawn over the other pane's contents, or — where the other pane
  held a web page — hidden underneath it, leaving nothing but the size line where
  the picture should have been. The same thing happened when a preview pane was
  inserted between the picture and the terminal. Every frame moved the picture to
  whichever preview pane came first in the window, which was the picture's own
  only while a tab had a single one of them. It now travels with the pane that is
  holding it, through splits, insertions, divider drags and tab switches alike.

- **Two preview panes can show two pictures at once.** With an image open,
  dragging a video in from the file column beside it left the image pane with
  nothing but its size line, and doing it the other way round left the video
  pane blank instead; closing the pane that arrived did not bring the first one
  back. A window could hold one picture texture, so a tab elected one preview
  pane to it and starved the rest, and a video pane counts as a picture until
  you press play. Every preview pane holding a picture now draws it, so an
  image, a recording and a third picture beside them are three pictures on the
  screen.

### Known issues

- **A new signature has no reputation yet.** SmartScreen can still raise "Windows
  protected your PC" on the first run of a freshly signed build. **More info**
  names Weiyi Shi as the publisher and `folio.exe` as the application; **Run
  anyway** is the way through, and switching SmartScreen off is not.
- **Folio cannot be a panel inside Visual Studio Code.** That panel takes a
  process speaking a protocol, not a terminal; `folio-here.cmd` in the archive
  makes Folio the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made, and a second
  monitor can take a few seconds to appear after a cold start.
- **Two pages whose panes cross while the layout is rearranging can overlap for
  about 200 ms.** Under every hosted page is a plate of the window's colour, and
  the plate belonging to the page opened first sits above the page opened after
  it. Standing still that costs nothing, because two panes never overlap; while
  the panes are gliding to new places — a split, a close, a pane dropped, torn
  out or merged, all of which take 200 ms — the two rectangles can cross, and for
  that long one page is covered by the other's plate. Both are whole again as
  soon as the panes land, and a divider drag does not do it at all.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.
  The other six containers play on a stock Windows.

## 0.2.1-preview — 2026-09-06

Fixes and polish for 0.2.0-preview. The one thing that is new is the one 0.2.0
said was coming: `Open in Folio` on the page Windows 11 opens first. Nothing
here changes a setting, a file on disk or a key: a `settings.json` and a
`session.json` written by 0.2.0 are read as they stand.

### Added

- **Open in Folio, on the first page of the Windows 11 right-click menu.**
  `Settings > General > First page of that menu` puts it there. Windows 11 shows
  only entries declared by a signed package on the page that opens first, so
  switching this on registers `folio.msix` — the file that now ships beside
  `folio.exe` in the archive — for your account alone, with no elevation and
  nothing written outside that account. Right-clicking a folder, or the empty
  space inside an open one, then gives you `Open in Folio` without pressing
  `Show more options` first. It takes a second or two and the switch says so when it is
  done.
  - The old entry stays exactly where it was. `Open Folio here` under
    `Show more options` is still its own switch, still just two registry keys,
    and is what Windows 10 and any machine without the package have. Neither
    switch touches the other.
  - The row is not there at all below Windows 11, which has no such page.
  - Move `folio.exe` to another folder and the entry follows it: the next launch
    notices that the registration names the old one and re-registers it where the
    program actually is. That needs `folio.msix` to have come along; where it did
    not, the row says so.
  - Removing it is the same switch. It also comes off in
    `Settings > Apps > Installed apps`, and the row reads the machine rather than
    a remembered answer, so it agrees with whatever you did there.

### Fixed

- **A page keeps the whole pane it is in, whatever else the window has open.**
  With more than one page open in a window — a `.pdf` in one tab and a page in
  another, say — the one you were looking at could come up drawn in a narrow
  strip down the left of its pane, with the rest of the pane in the window's own
  colour, or blank from edge to edge. Under every hosted page there is a plate of
  the window's colour that keeps the desktop from showing through while the page
  is still arriving, and a page that went out of view left its plate lying where
  it last stood, over the top of any page opened after it. A plate is now down
  only while the page it belongs to is on screen.

- **A hover card over a PDF from LaTeX now says how many pages it has, and its
  pages turn.** A document written by `pdflatex` — most of them — packs its
  catalogue, its page tree and every page object into compressed streams, and
  the reader that counted pages read the file's bytes without inflating
  anything, so it found nothing to count. The card drew the first page and
  printed the size, the page count never arrived, and because the column of
  pages is as long as the count says, the wheel had nothing to turn. The count
  now falls back to the same reader that draws the page, which inflates those
  streams and answers off the document's own page list. Files whose structure
  is in the clear are still counted without being parsed.

- **The Chinese half of a hover line is the same size as the Latin half.**
  Pointing at a folder printed `file:///D:/Demo · Ctrl+点击在资源管理器中显示`
  with the Chinese set at about six tenths of the height of the address beside
  it. The fallback face was fine; the row it was laid on was not. That line is
  the one place window text is set on the terminal's own grid, and it was laid
  out one character per cell — a full-width character in a one-cell slot is
  shrunk until it fits. It now takes the two cells it owns, exactly as typed
  Chinese and Chinese scrolled into history always have. The same line carries
  the `N rows above` count and the notices a lost background thread raises, so
  those read at full size too.
  - The line's own budget is counted in cells now as well. The aside about
    `Ctrl` is printed only when the whole address is already on the line, and in
    Chinese that test was being answered by counting characters — the aside went
    out onto a grid too narrow for it and the start of the address fell off the
    left.

- **Both of a pane's clearing verbs do what they say.** `Clear screen` used to
  leave the pane blank with no prompt in it, and the next characters typed
  appeared on their own, part-way across an empty pane. It now keeps the row you
  are typing on, moves it to the top, and puts the rows that were above it into
  the scrollback, where they can still be scrolled back to, searched and copied.
  The shell is told the same thing, so it goes on drawing where the window does.
  Those rows used to be thrown away rather than kept, which is why
  `Clear scrollback…` on the same pane then appeared to do nothing at all, and
  why the marks down the right-hand edge went on pointing at lines that were no
  longer anywhere. Both verbs are on a pane's right-click menu.
- **A sentence's own punctuation is no longer part of the file it names.** A line
  ending `see docs/notes.md.` opened `notes.md.` — a name nothing on the disk
  holds and the preview could make nothing of. An ASCII stop, comma, semicolon,
  colon or quote at the end of a name is now read as the sentence's: the printed
  string is still asked about first and a file that really carries one still
  wins, but where it does not, the name without it does.

### Known issues

- **A new signature has no reputation yet.** SmartScreen can still raise "Windows
  protected your PC" on the first run of a freshly signed build. **More info**
  names Weiyi Shi as the publisher and `folio.exe` as the application; **Run
  anyway** is the way through, and switching SmartScreen off is not.
- **Folio cannot be a panel inside Visual Studio Code.** That panel takes a
  process speaking a protocol, not a terminal; `folio-here.cmd` in the archive
  makes Folio the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made, and a second
  monitor can take a few seconds to appear after a cold start.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.
  The other six containers play on a stock Windows.

## 0.2.0-preview — 2026-09-05

Two surfaces that were not there before — a terminal that comes down on a key
from anywhere, and one box that searches everything this window knows about —
and the first release that carries a signature.

### Added

- **A terminal on a hotkey.** ``Win+` `` brings a terminal down across the top of
  whichever screen the pointer is on, over whatever was standing there, and the
  same key takes it away again and hands the keyboard back to the program it came
  down over. It is the same Folio — tabs, panes, the files column, the preview,
  every shortcut — and it keeps its shells and its scrollback between summons. It
  lives and dies with the run: there is no icon of its own and no process left
  behind, so closing the last window you can see ends it. The window keeps one
  `×`, and pressing it is the summon key pressed again. The key is on the
  Shortcuts page as **Summon the terminal** and can be changed or cleared there.
- **Settings > Summoned terminal.** A page of its own for the summon key
  (recorded in place), which profile a new tab opens on, a command to run on the
  first summon of each run, the window's height and width as a share of the
  screen, the gap it hangs below the top, whether it hides when the keyboard
  leaves it — off as it ships — and what comes back. The gear on the summoned
  terminal's own title bar opens that page; every other window's gear opens
  General as before.
- **What comes back (Settings > Summoned terminal).** Three answers for what a
  new run puts back into the summoned terminal: nothing, the tabs and their
  folders, or the tabs, their folders **and** the last command a pinned tab ran
  — typed at its prompt and **not** run. Nothing on this row ever runs anything;
  the restored line stands there for you to press `Enter` on or edit. It ships
  on the third answer. Restoring a command needs shell integration, because that
  is what tells Folio which line was a command; without it the tabs and folders
  come back and nothing is typed.
- **Command on first summon (Settings > Summoned terminal).** One command, run
  once each time Folio starts, on the first summon. This is the one thing the
  summoned terminal runs on your behalf, and it runs because you wrote it into
  that row. Empty by default.
- **Search everything: `Ctrl+Shift+P`.** One box over the top of the window,
  answering five questions at once and never mixing their answers: what Folio
  can do, the panes and tabs this window already has open, the commands it has
  run, the files under the folder its column is standing in, and the settings.
  Typing narrows all five together, the arrow keys walk them, and `Enter` goes
  straight to the highlighted row — a pane is raised, a file opens in the
  preview pane, a setting opens at its own row, and a command still running is
  pointed at with a ring around the pane it is running in. `Esc`, or a click
  outside, puts the box away.
- **The folder menu remembers where you have been.** The list under a files
  column's folder button now has a third group, between the folders your shells
  are standing in and the folder above: the last five folders you pointed a
  column at, newest first, each marked `recent`. Picking one from the menu,
  choosing one through `Browse…`, dropping one on a column, walking into one,
  and starting Folio on one with `--cwd` or `Open Folio here` all count; a shell
  running `cd` does not, so the list stays the places you meant to go. A folder
  already offered above keeps its place and picks up the extra note rather than
  appearing twice. Every window reads the one list, which is kept in
  `session.json` and is still there after a restart. A folder that is no longer on
  the disk is greyed rather than removed, and picking it says so where the tree
  would be.
- **`folio-here.cmd` ships beside `folio.exe`.** One line, `folio.exe --cwd`
  with the directory it was started in, for a program that opens an external
  terminal by running a command and gives it no arguments. Visual Studio Code's
  `terminal.external.windowsExec` is the setting it was written for; Folio
  cannot be embedded in VS Code's own panel, and this is the other half of that
  answer.

### Changed

- **The release is signed.** From 0.2.0 the `folio.exe` in the published archive
  carries a signature made under **Weiyi Shi** with a certificate from
  Microsoft's Artifact Signing service, countersigned by Microsoft's time
  stamping service so that it keeps verifying long after the three-day
  certificate that made it has expired. `conpty.dll` and `OpenConsole.exe` are
  Microsoft's and keep Microsoft's own signature. SmartScreen can still stop a
  download until a new signature has a reputation, but **More info** now names
  the publisher instead of saying there is none.
- **`settings.json` and `session.json` are read and written forward.** The
  summoned terminal's rows and the folders the folder menu remembers are new
  keys; a file written by 0.1.1 is migrated the first time 0.2.0 reads it, and
  nothing has to be deleted or re-entered.

### Fixed

- **The summon key no longer types itself.** ``Win+` `` used to leave a
  `` ` `` sitting at the prompt of the terminal it had just called up, because
  the window reports every key that is physically down at the moment it takes
  the keyboard, and that report was being read as somebody typing. A key that
  was already down when a window took the keyboard is not a key that was pressed
  at it, and no chord's own characters reach a shell now.
- **The command palette's input takes a paste, and narrows while you
  compose.** With the palette open, `Ctrl+V` — and `Ctrl+Shift+V` and
  `Shift+Insert`, which mean the same thing here — puts the clipboard into the
  input; before, the key was taken and nothing happened. What goes in is the
  first line of what was copied, with control characters taken out, because the
  input holds one line. `Ctrl+A` selects what is in it and `Ctrl+Backspace` takes the word
  behind the caret. And a query typed through an input method now narrows the
  list as it is being composed rather than only once it is committed; pressing
  Escape part-way through a composition puts the list back where it was.
- **A file path an application wrapped over several indented rows is one link
  again.** When an agent prints a block of indented text holding a single path
  and breaks it at the window's width, the rows are read back as the one file
  they spell between them — as many as eight rows, where only two were ever put
  together before, and at the block's own indent, which used to be read as a
  column of separate lines. A path continued under the text of a bullet, where
  the second row starts further in than the first, is put back together for the
  same reason. The whole path underlines and opens as one. Rows that really are a
  column of separate lines are untouched: a listing whose rows are each a file of
  their own, and any row with other text in front of the path, are left exactly
  as they were.
- **A path after a full-width colon is found.** A line such as
  `早就在:dist\folio.exe` left the file unmarked, because the colon was read as
  the one that makes a drive letter or a URL scheme — and nothing before a
  colon that is spelled outside ASCII can be either. The name after it opens as
  the name it is.
- **A path holding an 8.3 short name is one path.** `~` is what Windows builds
  every short name out of, and it was not among the characters a path is spelled
  with, so `PROGRA~1\tools\a.txt` was never recognised and a short name inside a
  wrapped path broke the rejoin. Only a `~` a name opens with is still the
  home directory it has always been.

### Known issues

- **A new signature has no reputation yet.** SmartScreen can still raise "Windows
  protected your PC" on the first run of a freshly signed build. **More info**
  names Weiyi Shi as the publisher and `folio.exe` as the application; **Run
  anyway** is the way through, and switching SmartScreen off is not.
- **"Open Folio here" is not on the first page** of the Windows 11 context menu.
  That page needs a packaged application as well as a signed one, and it is
  planned for 0.2.1.
- **Folio cannot be a panel inside Visual Studio Code.** That panel takes a
  process speaking a protocol, not a terminal; `folio-here.cmd` in the archive
  makes Folio the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made, and a second
  monitor can take a few seconds to appear after a cold start.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.
  The other six containers play on a stock Windows.

## 0.1.1-preview — 2026-09-02

A fixes-and-polish release for 0.1.0-preview. Nothing below changes how anything
is driven; what changes is what the window tells you, and what it tells the
programs running inside it.

### Added

- **Update check.** Folio asks the releases page once a day whether a newer
  version exists. When there is one, the settings gear wears a dot and
  Settings > General names the version, with `Open releases page` at the foot of
  that row's picker. Nothing is downloaded and nothing is replaced. The request
  is one `GET` of a fixed address carrying `User-Agent: Folio` and nothing else;
  it is at most once every 24 hours across every window on the machine, and any
  failure is silent. Switch it off at Settings > General > **Update check**, or
  with `"update_check": false` in `settings.json`. What is asked and what is
  stored is written out in [`docs/PRIVACY.md`](docs/PRIVACY.md).
- **A finished turn says what the agent said.** The notice raised when an agent
  finishes a turn now carries that agent's own first sentence instead of the
  words "Turn finished". Claude Code's comes from the transcript its `Stop` hook
  names, Codex's from the `last-assistant-message` field of the payload it hands
  its notify command, and a program that supplied its own text through `OSC 9` or
  `OSC 777` is quoted as it always was. Nothing is read off the screen. The
  sentence is cut at 80 characters, and a turn that ended without prose — a
  table, a tool call, nothing at all — raises the notice it used to.

### Changed

- **The release archive can be signed.** `scripts/release/sign.ps1` signs
  `folio.exe` with a certificate Microsoft's Artifact Signing service issues, and
  `scripts/release/package.ps1 -Sign` puts the signed file into the archive
  before the hashes are written. It is off by default, so a build with nobody
  signed in still packages exactly as it did. Nothing published so far is signed;
  the first release that is will say so here, and the note about "Windows
  protected your PC" will go when it does. How it is set up is in
  `docs/RELEASING.md`.
- **A window you can see is not interrupted.** When the window holding a waiting
  agent is somewhere you can see it, a second monitor included, the wait is
  marked inside that window and nowhere else: no taskbar flash and no message on
  the desktop. A window covered by another one, minimised, or on another desktop
  escalates as before, and a pane you are typing in stays silent as before. The
  description under Turn finished in Settings is worded for this.

### Fixed

- **A file name an agent wrapped inside its own bullet paragraph went
  unmarked.** A name cut at the right edge of the window and continued on the
  next line was only put back together when the continuation began at the very
  first column. An agent writing a bullet paragraph aligns its continuations
  under the bullet's text instead, so a real file printed that way was left with
  no mark on either line. A continuation opening deeper than the line it
  continues is now read as one, while two lines opening at the same column are
  still two lines.
- **A picture pane went on showing the file it first read.** An image opened in a
  pane is now watched on disk like every other kind of file a tab stands on:
  writing a different file over the same name is a change, and so is deleting it
  and writing it again. Opening an image always reads the disk rather than a
  remembered decode, so closing a pane and opening the same name again shows what
  is there now. The hover card, an image inside a typeset markdown page, and an
  image torn out into a floating window are fixed by the same change.
- **A lost graphics device ended the run.** A laptop changing power source makes
  the driver reset the device, and that used to close the window and every shell
  in it. The device is now rebuilt where it stood — a new adapter, every window
  taking the new device — with the sessions, the terminal contents, the layout
  and the open documents untouched, because none of them were ever on the card.
  Three failed rebuilds in a row still end the run, as does a device Folio
  destroys itself. A rebuild writes `Folio rebuilt the GPU device after it was
  lost (#N)` to `%APPDATA%\Folio\diagnostics.log`, and the first frames after one
  send their pictures to the card again.
- **A minimised window left every shell in it two columns wide.** Windows
  describes a minimised window with the rectangle of its icon — 314x50, parked
  far off the screen — and that rectangle was being laid out like any other: the
  panes were solved for it, and a fifth of a second later every pane's ConPTY was
  told it was two or three columns wide. It stayed that way for as long as the
  window was minimised, so a shell wrote its output at that width the whole time,
  and text already wrapped at three columns cannot be unwrapped by a later
  resize. A minimised window is now refused by its posture rather than by its
  size, so a window somebody really has dragged down to 314x50 is still a window.
- **Dragging a divider told the shell every width the hand passed through.** The
  quiet mark that decides when to tell a shell its new size is reached at every
  pause in a drag, so a child process was sent the whole tour, the two-column
  floor included. A hand still on a divider or on the window frame is now told
  nothing at all: one gesture sends one size, and it is the size at the moment
  the hand lets go. What is drawn still follows the hand frame by frame, and a
  change no hand is holding — opening a preview, closing a tab — arrives exactly
  as it did. `BT_RESIZE_TRACE=1` now prints a line for every size sent to a
  ConPTY.
- **A restored window could open below the bottom of the screen.** A size
  recorded on a larger display was taken as it stood, on the reasoning that a
  size cannot be off-screen. It is now fitted to the work area of the display it
  will actually land on — the one that would hold most of it, or the primary
  display when none would. Fitting only ever makes a window smaller, and a window
  whose size was not touched keeps the corner it was parked at, half off a
  monitor included.
- **A second window's recorded place was replaced by a default one.** Only the
  first window was told where it had been restored to, so a second window that
  came back maximised had no plain rectangle of its own to record and was written
  down at 100,100,1280,800 on the next start. Every window is now recorded at the
  rectangle it opens at, before it is asked what it looks like.
- **A floating preview never said that its document had changed on disk.** The
  `Reload` / `Keep` strip belonged to panes and had no row to stand in inside a
  floating window, so a reader there was looking at stale text with nothing to
  say so. A floating preview now keeps a strip of its own under its head, the
  same height as a pane's, and the body moves down for it rather than being
  covered by it. `Reload` and `×` mean there what they mean in a pane.
- **A pane dragged into another window was carried by a blank card.** The
  stand-in card in the other window's card column drew nothing, while every card
  beside it drew its own contents. It now draws the pane in the hand, fetched
  from the window the pane came from once a turn — which is what a hand that
  crosses the border and then stops needs, a still hand sending no events at all.
  The first frame after the border can still be empty, and fills in on the next.
- **A drag held at the foot of the card column could not decide what it was
  doing.** With the pointer still and the list auto-scrolling under it, the
  landing changed between dropping into a card and becoming a card of its own,
  once for every card that went past. The drag was the one reader of that column
  that had never been told a row cropped away is not a row you can point at; it
  is now, and so is the vertical tab column. Over one auto-scroll run 407 frames
  out of 470 named a card that was not on screen; none do now.

### Known issues

- **Not signed.** The first run may raise "Windows protected your PC". Click
  "More info", check the application named there is `folio.exe`, and click "Run
  anyway".
- **"Open Folio here" is not on the first page** of the Windows 11 context menu.
  That page needs a signed, packaged application, so it waits on signing.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made, and a second
  monitor can take a few seconds to appear after a cold start.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.
  The other six containers play on a stock Windows.

## 0.1.0-preview — 2026-08-31

The first public build. Everything below is new, so it is grouped by what part of
the window it belongs to rather than by added and changed. The last two sections
are for what was wrong on the way here, and for what is still wrong.

### Terminal

- Panes split horizontally (`Alt+Shift+-`) and vertically (`Alt+Shift+=`), tabs
  carry them, and one process holds many windows.
- A tab or a single pane can be dragged out into a window of its own, dropped into
  another window, or sent to one from a menu. A dragged tab keeps a ghost under
  the pointer and wears a badge when it is over another window.
- Dropping a pane somewhere else is a move rather than a close and a re-open: the
  panes it did not touch keep their widths to the pixel, and putting a pane back
  where it came from changes nothing at all.
- A drag held near the end of a tab list scrolls that list — the tab strip, the
  vertical tab column and the card column alike — and the place the thing in hand
  will land is worked out again against the list that moved.
- Cards (`Ctrl+Shift+Z`) lays the tab's panes out at readable size, each one laid
  out by the same rules as the window it came from, so a fixed-width column takes
  its space on a card exactly as it does on the window. A card scrolls under the
  wheel with `Alt` held. A pane dropped on a card moves into that tab; dropped
  between two cards or below the last one, it becomes a tab of its own.
- Zoom one pane to fill the tab (`Ctrl+Shift+X`), and back.
- Quit (`Ctrl+Shift+Q`) asks once, with a card listing what is about to close a
  line at a time and an offer to save everything unsaved.
- Command marks: with PowerShell or bash integration installed, `Ctrl+Shift+↑` and
  `Ctrl+Shift+↓` step between commands, and a failed command is marked as failed.
- Mathematics printed into command output is typeset in place, and shown as it was
  printed when it cannot be.
- Find in a pane (`Ctrl+F`), `F3` and `Shift+F3` between matches.
- A tab has a menu of its own, every menu row prints the key it answers to, and
  the status line says what `Ctrl`-clicking a link will do.
- `Shift+Insert` pastes and `Ctrl+Insert` copies. Copy-on-select is a setting.
- Clicking a file path or a web address printed in the terminal opens it in this
  window; holding `Ctrl` hands it to the machine's own browser or application
  instead. A path nobody marked up is found too, once the file is confirmed to
  exist, and it is read up to the punctuation that ends it — a comma with a
  Chinese word welded to it, a closing backtick, an opening bracket before a
  version number.
- A host written without a scheme in front of it is a link as well, opened over
  `https`, recognised from a deliberately short list of endings so that
  `README.md` and `main.rs` stay ordinary text.
- A link an application broke across several printed rows lights up whole, and
  either half opens the same target.
- A pane answers a program's colour queries with the colours the window is
  actually wearing, and again after the scheme changes.

### Files and preview

- A files column beside the terminal follows the pane's directory, and keeps a
  watch on the root and on every unfolded folder, so a file a command has just
  written appears without a refresh. Folding a folder gives the handle back.
- Files and folders can be pinned, renamed on disk, revealed in Explorer, opened
  in their default application, or opened as a new pane rooted there.
- Hovering a file name shows a card: a PDF the wheel winds page by page, a video
  that plays where it stands, the first lines of a text file, an image, or the
  file's own format and size when nothing can read it. A file the terminal
  printed raises the same card, and pulling a card by its head six pixels turns
  it into a floating window that carries on where the card left off.
- Markdown is typeset — headings, tables, code, images, and mathematics written as
  `$…$`, `\(…\)`, `\[…\]` or as one of the bare `amsmath` environments. A
  `<picture>` takes the source that matches the theme in force. A remote image is
  never fetched; it stands as its own alt text and a link.
- Text in a typeset page can be dragged over, double-clicked by word and copied as
  it reads rather than as it was written, with tabs between a table's cells. The
  right button offers Copy and Select all.
- A file no list of extensions covers is read once and shown as text when its
  bytes say it is text — UTF-16 written by Windows PowerShell included.
- A local HTML file can be read as a page or flipped to its source, and flipping
  costs no reload. That source view is an editor: `Ctrl+S` writes the file, and
  the page reloads itself from what was written.
- Web pages open in a pane of their own, with an address field (`Ctrl+L`), the
  site's own icon, a source view for local pages, and a place in the session so
  they come back when Folio does. A link inside a typeset page reads the same
  rule the terminal reads: a plain click opens it here, `Ctrl`+click hands it to
  the browser.
- Every document a tab is holding is watched, not only the one on screen. A file
  rewritten outside Folio is read again when nothing is unsaved, offers Reload or
  Keep my edits on a strip when something is, and stays on screen with a notice
  saying so when it is deleted.
- A preview pane moved to another tab takes its document, its unsaved edits and
  its place in the page with it.
- Video is decoded by Windows itself and plays in the pane, in a floating window
  and on the hover card, from `.mp4`, `.m4v`, `.mov`, `.mkv`, `.avi`, `.wmv` and
  `.webm`. The controls — play, a scrubber, the times, mute, volume and speed —
  are drawn by the window, shed from the right when there is no room, and answer
  Space, the arrow keys and `M`. A tab playing sound out of sight wears a speaker
  that takes you to it. An animated GIF advances by its own frame delays.
- The files column turns into a git panel (`Ctrl+Shift+G`): branch, working tree,
  staged and unstaged files, and the commit graph, with a selected file's diff in
  the preview.

### Agents and notifications

- A pane can say it is waiting for you, and the window says which one. Programs
  that write the terminal's own attention sequences are heard without anything
  being installed.
- `Ctrl+Shift+A` jumps to the pane that has been waiting longest.
- Seven profiles start an agent — Claude Code, Codex, Copilot CLI, Kimi Code, pi,
  Hermes, OpenCode — looked for on the Windows path and in npm's global
  directory. The new-tab picker lists the ones this machine can start; the
  Profiles page lists all seven and says once how to run one that lives inside
  WSL.
- The Agents page in Settings installs one notification hook each into Claude
  Code's, Codex's and GitHub Copilot CLI's own configuration files, and takes it
  back out again — the file is written whole or not written, and a dated copy of
  what was there is kept first. Nothing is installed by default.
- A window that is genuinely out of reach — minimised, hidden, on another desktop
  — raises a real Windows notification carrying the program's own words. A
  taskbar that hides itself has no flash to give, so a wait there reaches the
  desktop as a notification instead. There is a separate switch for the
  notification a turn's end raises.

### Settings

- One dialog: font, colour scheme, cursor, scrollback, line wrapping, background
  opacity, minimum contrast, what the preview renders, shell profiles, and every
  shortcut key.
- English and Chinese, switched without restarting. Every row name and every
  sentence is written for the person reading it rather than for the source, and a
  Chinese sentence breaks between Chinese characters instead of carrying a whole
  run of them to the next line.
- The Shortcuts page records a chord as you press it, says when the chord is
  already taken and offers to take it off the row that has it, and writes
  `keybindings.json`.
- Profiles: the five shipped shells can be overridden field by field and restored,
  and profiles of your own carry their own command line, environment and colours.
  The Arguments field shows what a startup script looks like.
- Colour schemes are files in `%APPDATA%\Folio\schemes`.
- Light or dark is read from `settings.json` alone and resolved against the
  Windows setting before the window is made, so a fresh installation on a light
  Windows opens light and the first pane is told so.
- The PSReadLine row names the execution policy when that is what stops it, and a
  press hands back the `Set-ExecutionPolicy` command that lets the module load.
- A press anywhere outside a dropdown or a row menu closes it, and that press goes
  no further.
- Animation follows the system's "reduce motion" setting, and notices the moment
  that setting changes rather than at the next start.

### Windows

- "Open Folio here" is in the Explorer context menu, under "Show more options".
- Windows PowerShell 5.1 ships PSReadLine 2.0.0, which misplaces the input line
  after the window is resized. Folio carries a patched 2.4.6 and installs it into
  your module path on request.
- The first tab opens the first shell the machine actually has — PowerShell 7,
  Windows PowerShell, WSL, Git Bash, Command Prompt, in that order. One whose
  program is not installed is greyed out in the picker rather than hidden.
- The first time a PowerShell pane prints something, a strip offers to append one
  line to `$PROFILE`, after copying that file as it stood to a dated backup beside
  itself. It offers once per run.
- The taskbar button carries a running command's progress, and flashes for a pane
  that is waiting while another program holds the focus.
- WSL distributions are read from the registry, and the question of which login
  shell to use waits until a WSL pane is actually opened.
- Nothing is sent anywhere. What Folio remembers lives in `%APPDATA%\Folio`, and
  the web preview's cookies and cache in `%LOCALAPPDATA%\Folio\WebView2`.

### Fixed

- **A files column stayed empty in every window but the first.** Answers from the
  background workers were addressed by a number each window counted for itself, so
  the window that opened earliest took everybody's answers off the queue and threw
  away the ones that did not match its own. Every address now carries the window
  it belongs to.
- **A window that had shown a web page would not close.** It stayed on screen with
  its browser processes alive behind it, and the corpse held the keyboard, so a
  Folio started afterwards could not compose Chinese and beeped at every key.
  Every road out of a window now takes it off the screen first, waits for the
  engine, and leaves the process by the one route a host of that engine may take.
- **A crash left its window standing.** A panic now leaves by the same road as a
  close: every window of the process is hidden, then the process ends.
- **A machine with no WebView2 drew nothing where the preview should be.** The
  preview is now always there, wearing either the card that names the runtime to
  install or the card carrying the machine's own words about why the engine did
  not start.
- **A machine with no graphics card of its own stretched the window's own text.**
  Everything a window draws is now divided by one resolution instead of two.
- **Starting Folio raised a Windows Terminal window and took seconds.** Every
  child process outside a pane now starts quietly, and the first frame waits for
  no probe: opening the window went from seconds to hundredths of a second.
- **A window could lose every character it was drawing.** In a long session across
  many font sizes the shared glyph store wore out and then refused every frame
  after that. A frame that comes back without its text is now given fresh storage
  once, so the words return on the next frame, and each such repack writes a
  numbered line to `%APPDATA%\Folio\diagnostics.log`.
- **A hover card could come up with everything but its words**, and show them only
  on a second hover. A document that lands is now credited to every surface
  reading it, the card included.
- **A printed web address did nothing when clicked.** Plain click and `Ctrl`+click
  now read one table, on the terminal and inside a typeset page alike.
- **A web address broken across two lines opened someone else's site.** The first
  half was a valid address on its own, so it was the one that opened. Both halves
  are now one link.
- **A new installation opened dark on a light Windows.** The theme had two stores
  and the window read the wrong one; there is now one, and it is the one Settings
  writes.
- **The web preview filed away what you typed into a page.** General autofill and
  password saving are now switched off explicitly rather than left at the engine's
  defaults.
- **A page could be rasterised for the wrong display.** The window tells a hosted
  page which display it is on instead of leaving the engine to guess late.
- **A hovered video restarted the decoder every time.** The media session now
  lives as long as the process: a first frame costs about ten milliseconds warm,
  where it used to cost hundreds.
- **A floating preview took the keyboard the moment it appeared**, and kept it, so
  everything typed afterwards went to the preview instead of the shell. It now
  takes the keyboard only on a press inside it, and gives it back on a press in a
  pane.
- **Diagnostics could land in somebody else's pane.** The console borrowed at
  startup to answer a command line is given back before the run begins, and a
  run's own output goes to `%APPDATA%\Folio\diagnostics.log`.
- **A control nobody could see could still be clicked.** A hidden run of buttons
  on a pane head does not take a press until the pointer has revealed it.
- **An emptied environment variable was read as a filename.** `BT_PTY_DUMP=` and
  every switch like it now read set-but-empty as off.
- **A menu on its way out could still be pressed.** A leaving popup is a fading
  picture with nothing left to click, and switching tabs leaves none standing.
- **A sentence too long for its place was cut at both ends.** An error card, a
  notice strip, the Git panel's empty page and a pane's own middle line now wrap
  where they stand, and a notice strip keeps its buttons whole while its sentence
  shortens.
- **The window could stop answering with nothing to show for it.** A watchdog
  writes a report naming modules and offsets when the window thread misses a
  deadline it declared, and an indefinite park is not counted as a hang.

### Known issues

- **Not signed.** The first run may raise "Windows protected your PC". Click
  "More info", check the application named there is `folio.exe`, and click "Run
  anyway".
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **"Open Folio here" is not on the first page** of the Windows 11 context menu.
  That page needs a signed, packaged application, so it waits on signing.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.
  The other six containers play on a stock Windows.
