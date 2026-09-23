# Changelog

All notable changes to Folio are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- Settings can be exported to one file and imported on another machine:
  **Settings > About** has Export…, Import… and the settings folder.
- Sliding a finger over a pane now scrolls it, with the system's own flick.
  A one-finger slide no longer selects text.
- Pasting several lines into a shell that would run them one by one now asks
  first: run them line by line, join them into one line, or cancel. A setting
  under Terminal turns the question off.
- Web panes follow Folio's light or dark theme; a setting can pin either. A
  site's icon that would disappear into the bar it sits on gets a small round
  plate.

### Changed

- A multi-line paste into PowerShell now waits on the input line for Enter.
- A shortcut can now use Ctrl with a letter; the row notes that the key no
  longer reaches the shell.
- Ctrl+click on a network-share path or on a mailto:, vscode: or other link now
  hands it to Windows or macOS. Links inside a previewed document do the same.
- The glance card's bottom line shows the file's folder; click it to find the
  file, Ctrl+click to show it in Explorer.

- In the Markdown preview, a paragraph turns into its source when the gesture
  ends, not while a selection is being drawn across it.

- The glance card's shadow and the tooltip's padding and corners now match the
  rest of the float-tag family.

- The Git page's row height, head and row type, and section labels now match
  the rest of the window.

- Every pane's head title is the same size as the float and glance heads', and
  the pane head's `⌄`, the preview switch, the float's dock and the files
  root button now stand in the same tool box every other head control uses.
  The formula band's own buttons, which already reuse
  the pane head's, grow with it.

- The command palette's field and rows are the same text size as the rest of
  the window.

- The first-run card's option rows are the same height as a Settings row.

- The section labels inside the right-click and `⌄` menus are the same size
  and spacing as Settings and the rail.

- Every Settings combo wears the same `⌄` every other drop-down opener wears,
  and the dialog title, the combo drop-down rows, the row titles and the nav
  items are on the same scale as the rest of the window.

### Fixed

- On a slow disk, turning on PowerShell integration from the welcome card
  could report a lock error although everything was written; it no longer
  does.
- The command marks down a pane's right edge no longer cover the last column of
  text.
- The Cards setting named the wrong shortcut on macOS and after a rebind.
- In the Markdown preview, a selection dragged across the paragraph or table
  you are editing highlights it too, instead of skipping it.

- On macOS the Git page found no git, and the dark scheme setting named a
  Windows folder.
- Ctrl+click no longer freezes the window while Windows opens the file;
  Explorer comes to the front.

- Touch input — a touch screen, or a remote-desktop tool that sends touch —
  now reaches Folio the way it reaches other Windows programs: a tap is a
  click, and press-and-hold opens the menu. Windows does the translating.

- A printed path with spaces in it is a link when the file is there.

- A relative path an agent prints with a full-width colon or comma right
  behind it (`experiments/plot.png：…`) is a link again when the file is in
  the pane's folder.

- A folder an agent prints with a slash on the end (`whydrift/models/`) is a
  link, like the same folder printed without one. Clicking it shows the folder.

- A printed path followed by a full-width stop and a number or a Latin word
  (`notes.md。18 items`) is a link when the file is there.

## 0.4.3-preview — 2026-09-21

### Added

- **Rendered blocks → Repair row breaks.** Folio restores the row separators a
  coding agent's own redraw of its finished answer eats, so a matrix arrives as
  a matrix rather than as one long row. The repair compensates for another
  program's markdown renderer, and this row is how you say not to — the day
  those tools stop damaging their own output, it stops being a repair. It ships
  on, it reaches the formulas already on screen the moment it is changed, and
  either way it never alters what copying a formula gives you.

- **`folio --remove-explorer-menu` takes Folio back out of Explorer's
  right-click menu, without opening a window.** Both entries go: the one on
  Windows 11's first page and the classic one under "Show more options". It is
  the piece that was missing from every way of removing Folio — there is no
  installer, so deleting the files left a menu entry pointing at a `folio.exe`
  that is no longer there, and the switch that could have taken it off went with
  the folder. A registration belonging to another copy of Folio on the same
  machine is left exactly as it is; one whose `folio.exe` has gone is cleared,
  because nobody is answering it. It prints one line saying what it removed and
  what it left, exits `0` when the machine is the way you asked for it —
  including when there was nothing to remove — and non-zero only when a removal
  was refused, with the reason on standard error. `docs/install.md` has the
  per-channel steps.

- **Terminal settings now include a separate CJK font choice.** Automatic uses
  Folio's platform defaults, and an installed Chinese, Japanese or Korean face
  can be chosen without changing the monospace font used for ASCII.

- **`folio --uninstall-cleanup` undoes what Folio wrote outside its own folder,
  in one command and without a window.** The `$PROFILE` line, the agent hooks,
  the Explorer entries and the PSReadLine module, each reported on its own line
  with what was removed and what was left. Hooks and Explorer entries belonging
  to another copy of Folio that is still on the machine are left alone. It exits
  `0` when the machine is the way you asked for it, including when there was
  nothing to remove; `1` names a refusal to fix and retry; `2` means a Folio is
  running, or, with `--purge`, that a process still holds the data. `--purge` on
  the same command also deletes settings, sessions and browser data — both
  Windows data roots and the legacy one, or all six macOS locations — while
  keeping the dated recovery copies beside your own configuration files. The
  application folder is never deleted. On macOS, Folio cannot tell whether
  another program holds that data, and the command says so as it starts rather
  than implying a check it never made. Windows also ships `uninstall.cmd`, which
  is the same command by double-click. `docs/install.md` has the per-channel
  steps.

- **`folio --remove-shell-integration` takes Folio's line out of your PowerShell
  `$PROFILE`, without opening a window.** It is the same remover the settings
  switch uses, and it matches only the exact forms Folio itself writes — never
  "a line mentioning `folio.ps1`", which may be your own. A profile that is
  read-only, hard-linked, symlinked, or in an encoding it cannot rewrite is
  refused and left byte-identical, with the reason on standard error. The
  profile is replaced through `ReplaceFileW`, so its permissions, alternate
  streams, attributes and creation time survive.

- **A page for anyone who already deleted an older Folio.**
  `docs/recovery-after-deleting-folio.md` names the exact `$PROFILE` line to
  remove from which file, and the hook entries to delete from each coding
  agent's own configuration — copy-and-paste, with no Folio needed. It exists
  because a copy that is already gone cannot repair its own marks.

### Changed

- **When Folio's window pauses, its own log now names the call it was inside.**
  The log records the GPU adapter Folio actually opened, then separates surface
  acquisition, frame composition, submission and presentation from one another;
  terminal-output turns say whether time went into the ring, the parser,
  detection or publication, and input turns name both the event and either IME
  platform call. A stall line also carries the run's age and its own number, so
  one day's recording can show whether pauses grew as the session aged.

- **The log now names the GPU Folio asked for as well as the one it got, and a
  laptop with two can be told to try the other.** Folio has always asked for the
  highest-performance adapter, which on a laptop with two graphics chips is the
  discrete one; it still does, and nothing about a normal run changes. What is
  new is that the `GPU adapter` line in `diagnostics.log` says what was asked
  for beside what the driver answered, and that setting `BT_GPU_PREFERENCE=low`
  for one run asks for the integrated adapter instead
  (`docs/BT-ENVIRONMENT.md`). It is there to find out whether a particular
  machine's long pauses belong to its discrete GPU — the same build can be run
  both ways and the two recordings compared.

- **A save in the preview editor replaces the content and keeps what the file
  was carrying.** On Windows the alternate data streams (the `Zone.Identifier` a
  download carries), the DACL, the creation time and the attribute word travel
  through `ReplaceFileW`; on Unix the ownership, the mode and every extended
  attribute — quarantine, Finder tags, `user.*` — are put on the replacement
  before the rename. **Content is guaranteed and what the file carried is best
  effort**, stated in that order: a volume that answers that it has no such
  operation, and has provably changed nothing, gets the plain writer instead,
  and a metadata step never fails a save on any platform. A symlinked or
  unopenable name keeps the plain writer, as before; **a hard-linked target keeps
  it too**, so the file's second name still reads the old bytes — the preserving
  replacement refuses such targets rather than breaking the link. The staging
  file is born private and widened afterwards to the mode the replaced file
  carried, so a private note is never world-readable, not even for the length of
  one write. **One window is open and recorded:** `ReplaceFileW` is a sequence
  and not an atomic rename, so a crash or power loss between its two halves
  leaves the old document under a `.tmp-<hex>` name that nothing in Folio looks
  for.

- **Folio installs its PSReadLine module only where the place is empty or its
  own.** Until now the install door had no occupancy check at all: it wrote its
  nine files into `<Documents>\WindowsPowerShell\Modules\PSReadLine\2.4.6`
  whatever was standing there, and two states of the settings row let that write
  through over a module the reader had installed from the PowerShell Gallery —
  the folder then read as Folio's, so switching the row **Off** deleted it and
  the gallery's `PSGetModuleInfo.xml`, `en-US\` and catalog with it. **This is
  byte-identical back to v0.3.0.** To tell whether it happened to you, read the
  version stamp on the module in that folder and look for the record the gallery
  leaves beside a module it installed:

  ```powershell
  $d = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'WindowsPowerShell\Modules\PSReadLine\2.4.6'
  (Get-Item "$d\Microsoft.PowerShell.PSReadLine.dll").VersionInfo.ProductVersion
  Test-Path "$d\PSGetModuleInfo.xml"
  ```

  A version containing `-bt.` is Folio's copy — only Folio's own builds put that
  in the string — and anything else is yours, and untouched. Folio's copy **with**
  `True` on the second line means a gallery install of yours stood there first
  and was written over: install the PSReadLine you wanted again from wherever you
  got it, at the version you had. Folio cannot put back a module it replaced, and
  it does not try to. From 0.4.3 the check lives inside the one function that
  writes: a folder is Folio's when the assembly is ours by bytes or by the
  `2.4.6-bt.` stamp, or when there is no assembly and every name in it is one
  Folio itself writes, and anything else is foreign — neither written to nor
  deleted, with both verbs on the row dark over it. The disk fact is read before
  the stored invitation, so a remembered "installed" can no longer offer the
  button over somebody else's module. Removal was narrowed with it: the nine
  names go, and the directories go only while they are empty, so the records
  beside a mixed folder an older Folio left stay where they are and the cleanup
  door names them.

- **Folio's line in your PowerShell `$PROFILE` is one guarded form, and existing
  installs are rewritten at start-up.** The bare dot-source line errored at every
  PowerShell start once `%APPDATA%\Folio` was gone — a Folio that had been
  deleted went on making an error in a file that is yours and not Folio's. The
  line is now one exact managed form that is silent when the script it names is
  missing, in Windows PowerShell 5.1 and PowerShell 7 and under strict mode, and
  legacy lines are rewritten to it when Folio starts, off the window thread,
  across every profile file Folio has written to and every one it can find. An
  account still on the legacy data root keeps a line that loads the script it
  actually has. Turning the integration off is recorded, and start-up then does
  nothing for that account.

- **An agent hook belongs to the `folio.exe` it names.** Until now Folio
  recognised its own hook entries by a marker alone, so one copy removed
  another's live hooks and an install silently replaced them. Ownership is now
  read from the executable the entry names: Folio removes or refreshes an entry
  naming this copy, removes one naming a `folio.exe` that no longer exists, and
  leaves one naming another copy that is still there — taking that one over needs
  a second, explicit press within thirty seconds, and the paths are named before
  it happens. Only the operand's own file name says whether an entry is a Folio
  at all, so a reader's own hook that happens to use the same words is a stranger
  and stays; an entry Folio cannot decode is nobody's and has no say over the
  entries beside it; and the bare `folio.exe` 0.4.2 wrote when it could not name
  its own path names no copy, so it is removable rather than permanent. A
  configuration path reached through a link is resolved once, at the top of the
  operation, and only to a regular file inside the folder that path names — so a
  machine whose `~/.claude` is a junction can install and uninstall like any
  other, while a target outside the folder, a link to nothing, a hard link and a
  read-only file are refused each with its own sentence, and the row says which
  instead of reading Off over hooks that are firing. The entries are written in
  each agent's direct-execution form, which also ends a quoting hazard on Windows
  where a path containing `$` or a backtick was expanded by a shell. Folio
  refuses to write a hook at all when it cannot say where its own executable is,
  or when it is running translocated on macOS. Updating hooks may reformat that
  agent's JSON settings; a dated backup is kept beside the file.

- **The offer to switch the PowerShell integration on asks whether you already
  have it, not whether Folio wrote it.** Two facts had been folded into one, and
  the offer appeared in front of someone whose profile already loaded
  `folio.ps1` by their own hand. A profile Folio never wrote to is no longer
  claimed by Folio's record of where it has written.

- **The window thread never asks a filesystem about a path a program printed.**
  A hover resolved a reference by calling `symlink_metadata` and `is_dir` on the
  thread that draws, three times per pointer move and uncached; a drive letter is
  lexically local whatever it stands for, so a mapped network drive whose server
  is gone — or a junction on a local disk into a dead share, which needs no
  mapped drive at all — held the event loop for the redirector's own timeout,
  re-armed on every motion over the cell. The pane's printed-path ledger is the
  single owner of "is this a real, readable, local path" now, and the hover, the
  press, the pointing finger and the glance card read it and call nothing; the
  question itself is asked on a lane of its own that nobody waits on, so a stat
  blocking inside SMB cannot starve the formulas and pictures on the decoration
  queue. An unanswered name is not a link yet, which is the rhythm a bare printed
  path has always had, and an `OSC 8` target is asked about on the pointer event
  that meets it.

- **On Windows the terminal names its own Chinese family, and the weight asked
  for never changes it.** A chosen family with no bold cut keeps its regular face
  rather than leaving for another family's bold, so one bold word in a line of
  Chinese is no longer set in a different face. Which family owns a script is
  read from the font's own `OS/2` declaration, with `cmap` block coverage where
  it declares nothing, never from one sample character. Proportional text and
  previews keep their own YaHei-first chain and now name their family too. Font
  enumeration stays off the window thread.

- **Three diagnostics lines are always on, and none of them contains typed
  text.** A shown window that has owed a picture for longer than the threshold
  and shown none writes one line. A minute in which file reading went over budget
  with no input from you writes one line naming the lanes that read and the three
  most re-read **basenames** — never a directory, never contents; the bounded
  name table is cleared every minute. A composition that starts inside a live one
  (`shape=restarted-inside-a-live-composition`) or ends without ever having
  started (`shape=ended-without-a-start`) writes one line carrying the live
  pre-edit's byte length and no text, at most 32 per window. `docs/PRIVACY.md`
  says what may appear and advises reading it before attaching a log to an issue.

- **`BT_IME_TRACE` no longer records composed or committed text.** It writes
  event kinds, byte lengths, cursor ranges, rectangles, static reasons and
  results, and a focused pane now records when an input method is active and has
  not engaged. Older builds wrote the literal characters an input method
  produced.

### Fixed

- **A matrix an agent printed is set in the rows it was written in, in three
  places it was not.** Coding agents redraw a finished answer through their own
  markdown renderer, which eats the `\\` that ends a row of a matrix, an
  `aligned` block or a `cases` block; Folio has restored those for a while.
  Three cases were wrong. A row end inside `\text{…}` gained a row break that
  broke the text open, because a backslash one brace deep belongs to whatever
  opened the brace and not to the block. A line end inside `\begin{equation}`
  gained one too, although an `equation` holds a single formula and has no rows
  to separate — so what it drew was a row nobody wrote. And a nested
  `\begin{array}{cc}` was not repaired at all, because the scan looked only at
  the environments that may open a block on their own. The rest of the redraw's
  damage is still left exactly as it arrives, and deliberately: `\,` arrives as
  a comma, `\[` as a bracket, and a line holding only `=` is deleted outright —
  none of those can be put back without typesetting an equation nobody wrote.
  What is repaired is repaired for the typesetter only; copying a formula, or
  showing its source, gives back the bytes the terminal received.

- **Folio names the family it draws Chinese with, instead of leaving the choice
  to the font library.** The terminal gets a chain of its own, beginning with
  **NSimSun** on Windows — the family the Windows terminal was measured already
  resolving to in 0.4.2, reached by falling through the library's own search
  rather than by being chosen; 0.4.3 makes it the decision, and Settings ▸
  Terminal can name another family. Proportional text — headers, tab titles, the
  files column, the preview — keeps its YaHei-first chain and now names it too,
  so a header asking for a medium weight no longer falls through the library's
  exact-weight filter to SimSun.

- **Pasting a screenshot no longer makes Folio copy the same picture three
  times before it uses one of them.** A screenshot tool puts the same picture on
  the clipboard in several shapes at once — on Windows a `PNG`, a `CF_DIBV5` and
  a `CF_DIB`; on macOS a PNG and a TIFF — and Folio was copying every one of them
  into memory, on the thread that draws the window, before handing the best one
  to the worker that writes the file. For a 4K screen that was about 66 MB copied
  to use perhaps 4 MB of it, and up to three synchronous round trips into the
  application the picture was copied from, each of which can render the picture
  on demand. Folio now walks its preference list and stops at the first shape it
  can see is whole — a few hundred bytes of header, no decoding — so one shape is
  copied and the source is asked once. What ends up in the folder is the same
  picture it always was. Sources that advertise a picture shape and then hand
  over something no decoder can read, which some browsers and remote-desktop
  clients do, still paste: Folio reads the header, sees it is not a picture and
  takes the next shape, exactly as it used to after copying all of them. A source
  that offers only the older shapes is read as before, and text or a file list on
  the clipboard still wins over a picture without any of it being read.

- **A local page or PDF whose path contains Chinese — or a space with `{`, `}`,
  `^` or `` ` `` in the name — now opens in the preview, instead of being turned
  away by Folio itself.** Double-clicking `报告.pdf` under `D:\文档\项目 (1)\`
  drew Folio's own refusal card: the window had opened the file and then refused
  it, because it was comparing two *spellings* of one path where it meant to
  compare the path. The browser engine re-spells a local address in its own
  form — every character outside a small set comes back percent-encoded — and
  the two strings no longer matched. Folio now reads a local address as the file
  it names, once, at the door, so the two spellings are one answer. The address
  row shows the ordinary path again (`D:\文档\报告.pdf`, not `file:///D:/%E6…`),
  links to files beside the page, in-page jumps and reload keep working, and a
  session saved before this change still reopens its pages. Nothing that was
  refused for a safety reason is opened now: a path that walks out of the page's
  own folder, a network share, or an address that matches only after a spelling
  is guessed at is refused exactly as before. Reported as issue #7.

- **`diagnostics.log` now always opens with the line that says which build
  wrote it.** The file a bug report arrives as is a stack of runs, and the line
  between them carries the version, the commit and the process id. It was
  written only by a run whose diagnostics went to that file, so a run started
  with a trace variable set — or one whose log would not take the program's
  output — appended its watchdog's lines to a file with no such line anywhere in
  it, and nothing in what you sent said which Folio had written it. The line is
  now written when the log is opened, before the run has said anything else, on
  every kind of run.

- **An idle Folio window now lets the event loop sleep.** Wake-up deadlines are
  retained as absolute appointments owned by the event that armed them instead
  of being renewed from each loop turn; unchanged macOS menu inputs also stop
  before entering AppKit, and memory diagnostics no longer query the platform
  on every wake.

- **A PowerShell script now opens in the preview highlighted, instead of as
  plain text.** `.ps1`, `.psm1` and `.psd1` files, and a ` ```powershell `,
  ` ```pwsh ` or ` ```ps1 ` fence inside a Markdown document, get the same
  keywords, strings, comments, numbers and function names every other language
  has had — on Windows, on macOS and on Linux, with nothing new to install.
  PowerShell was the one common language missing from the set of grammars Folio
  carries, for a reason that was never visible from the outside: the grammar
  needed one pattern rewritten before the pure-Rust regex engine Folio uses
  would take it. The rewritten line, what it was, and why it means the same
  thing are recorded in `assets/syntaxes/README.md`, beside the grammar itself.

- **Folio no longer reads its installed PSReadLine module off disk on every turn
  of the event loop.** Once a Windows PowerShell pane had reported its PSReadLine
  version — or the Terminal settings page had been opened, which asks the same
  question — the clock run's invitation check re-read the module from disk on
  every turn, for the rest of the session, to answer a question the Settings row
  asks once. The module is nine files and 437 KB; the reads come from the file
  cache and were measured at about 100 MB a second, for as long as the window was
  open, with nothing on screen or in the log to say why. **0.4.2 has this defect
  too, and restarting Folio was the only relief.** The fact is now owned once by
  the application rather than per window, and read at three edges: the moment the
  version answer lands, an install or removal of the module, and the opening of
  the Terminal page. A redraw or a hover on an already-open page is not an edge.
  The rule this establishes is written down: a clock-run entry is a deadline or
  an edge, never a poll, and a quiescent turn does no filesystem, registry,
  PATH-search or process-start work.

- **A frame a program held inside a synchronized update is no longer dropped,
  and ending one keeps the sequence it interrupted.** Folio has two readers of
  the instruction that opens a DEC 2026 block, and they disagreed on two
  spellings of it: `CSI ? 1 ; 2026 h` and `CSI ? 2026 : 0 h` opened a block in
  one and none in the other, so a window resized at that moment installed a grid
  the held bytes had never reached — off the screen and off the history alike.
  Both read every parameter now, and the first sub-parameter of each; an open
  block ends on an exact byte match, because a parser holding one searches bytes
  and reads no parameters at all. Separately, the deadline that closes a block a
  program never ends used to throw away the escape sequence the boundary parser
  was inside along with the block's own bytes, so the rest of that sequence's
  payload was printed into the terminal as text. One function owns the release
  now, and it never writes the flag that says a sequence is open.

- **The key that redraws your prompt is sent only to a prompt the shell opened
  in order.** `ESC[24;8~` is what `folio.ps1` binds `InvokePrompt` to, owed after
  a ConPTY resize; whether a prompt was open was read from an `OSC 133` region,
  which any program can open by printing one — a file through `cat`, a git author
  name, a compromised motd. The next resize then wrote those seven bytes onto the
  standard input of whatever was really running: `ssh`, `python`, an editor, an
  agent's own display. The marks themselves stay permissive, deliberately, since
  a program printing a whole cycle cannot be told from a nested shell speaking
  the protocol; the order is checked once, where bytes leave for the child.
  `shell_prompt_opened_in_order` asks for an open region, for the `B` to have
  stood in a prompt an `A` opened, and for no command this session watched start
  that it has not watched end. A nested integrated shell keeps its marks and is
  not typed at while the command it runs inside is live.

- **A printed path the disk refused is asked about again when the program prints
  it again.** A denial used to stand until the pane's next `OSC 133 D`, and a
  pane whose foreground program is one agent running for hours never ends a
  command — so a file the agent named before writing it stayed dark for good,
  including over the finished file. A denial is re-asked when a live row whose
  fingerprint changed still spells the name; a repaint is not a printing, so an
  unchanged row and a full-viewport redraw both cost nothing, no clock is
  consulted, yeses are never re-asked, and a re-ask enters the same bounded
  budgets by the same door.

- **A name that is not there says "not found"**, where a reveal used to hand the
  file manager a folder nobody had asked for — and on macOS to post nothing at
  all. The routing table reads existence itself, so no arm can forget it, and the
  press re-asks the question although the pane already holds an answer.

- **A link under a resting pointer answers the first click.** The ledger's
  question was put from pointer motion and nowhere else, so a path that arrived
  by a wheel scroll or a fresh line of output, under a pointer already standing
  on it, was never asked about: clicking it changed nothing, repeatably. One
  function takes that question now and four doors ask through it — the pointer
  move, the press going down, the hand-over modifier going down, and a frame
  redrawn under a pointer standing still — with no filesystem call added to the
  window thread to make it true.

- **The Settings page is laid out when its content changes, not when the pointer
  moves.** Every reader of the page's geometry in a turn — hit test, hover,
  drawing, scroll clamp, the long menu's scroll-to-selected, the expansion clock
  — laid the whole page out again, two or three times per pointer callback and
  about 145 times in one turn. The geometry is a function of content, size, scale
  and language, and it has one owner now that the readers share. Counted: opening
  the page and moving the pointer across it 64 times went from 257 layouts to
  one.

- **A formula's two marks are placed from the frame being drawn, and stop when
  the block lands.** They read the picture *before* the one being presented, so
  during a change between a block's typeset picture and its source they trailed
  the band by a frame and snapped into place when it landed. The band's own
  geometry had a second fault at the same moment: its height and opacity were
  interpolated while its width flipped in a single frame, so its ground and both
  marks, which sit against the block's right edge, jumped sideways on the landing
  frame of a shrink and the first frame of a grow — and the flight was settled
  before that frame was composed, so it went on easing for another 90 ms after
  the block had stopped. A window with no marks on screen now asks the picture
  nothing at all.

- **A focused pane is no longer sent a focus report it did not ask for.** Every
  time a program turned focus reporting on, Folio reset what it believed the
  program knew and sent an opening `CSI I`. ConPTY re-asserts focus reporting at
  every program teardown and when the shell reads cooked input, so the byte
  arrived at the prompt as a literal `^[[I`. A program that has just subscribed
  already assumes the pane is focused; only the contrary is owed.

- **A waiting card keeps the whole of its halo, and its dot breathes.** An outset
  decoration grows from the card as drawn and is never clamped a second time; the
  first card's top is the list's top exactly, so clamping the grown box had been
  costing the waiting halo its entire top outset — 6 device pixels at 200% — on
  every frame since the column existed, and the flight shadows were the same
  shape. A decoration now grows only into the room its layout gives it, one
  amount for all four sides, floored to whole device pixels, so a clamped card's
  ring stays concentric and no scale that was already exact changed. The status
  dot had never pulsed at all: the design page it was transcribed from names an
  animation it never defines, and the transcription inherited the name without a
  curve. It now takes the window's own 1.7-second breath, the one the halo uses;
  reduced motion answers each channel's flat value — no halo, full dot.

- **A zoomed pane's name is no longer printed under the zoom mark.** Three
  functions owned "where the name starts" and only one of them knew about the
  zoom, so a zoomed web pane set the first letter of its name in the same column
  of pixels as the accent mark. The head is laid out once now, the mark is one of
  its slots, and the name's left edge is derived from the slots actually present.
  That pin caught an older overlap with it: a preview head with no room for a
  name at all still placed the switcher and its badge under the tools.

- **A preview pane too narrow for its switcher no longer swallows the keyboard.**
  The layout folded the menu away while the window went on believing a popup was
  up, so one press on the name of a pane dragged under about 262 logical pixels
  ate every keystroke over a glass with nothing drawn on it. Each popup anchored
  inside a pane has one answer to "would this draw anything" now, read by both
  the layout and the window, so being drawn and taking the keyboard are the same
  answer. Neither that menu nor the root menu folds for want of an anchor any
  more: the switcher hangs from the name when the head wears no pill and the root
  menu from the caption when the head carries no button, so the list of what the
  preview has open stays reachable at every width, by pointer and by keyboard.

- **A clipboard picture's shape is checked before it is decoded**, on the one
  encoding that was not checking it. There is no TIFF decoder in this tree, so
  the shape comes back out of the platform — the representation's own pixel
  width, height and depth — and is judged by the same ceiling the PNG and DIB
  arms use, and that judgement is the argument the decoding call is given. It
  matters because an allocation failure inside AppKit ends the process rather
  than returning an error, so a picture whose bytes are all there and whose real
  shape is past the ceiling is now turned away instead of drawn at any cost.

- **macOS: the loser of a two-launch race no longer keeps a window whose session
  is discarded.** The data directory's claim is the single owner of "I am the
  writer" and both of its endpoints are opened off it; ungated, the loser bound
  the names first and the writer latched itself to nothing for the life of the
  process, so every later launch landed in the window whose session writes go
  nowhere. **And a preview goes to the last address it was given**, so an address
  superseded by a later navigation is not replayed a round-trip later.

- **An italic request on an upright-only CJK family stays italic.** Writing the
  matched face's style back turned an italic request into Normal, so the
  synthetic slant was never applied: italic Chinese in the terminal, and
  `*emphasis*` in a preview, drew upright.


## 0.4.2-preview — 2026-09-18

### Added

- **Dragging a file out of the files column into the middle of a terminal now
  pastes its path, spelled for the shell running there.** The pane you are over
  says which of the two things it will do before you let go: aim at a pane's
  edge and you get the same split preview you have always got, and the file
  opens beside it; aim at the middle of a terminal and the pane lights up with
  `Paste path` on it, and the path arrives on that terminal's command line —
  the one you dropped it on, not the one you had been typing in. It is the same
  quoting a file dropped in from File Explorer or the Finder gets, so
  PowerShell, `cmd`, a WSL shell and the rest each get the spelling they read,
  and Folio runs no command of its own: the path is put in front of the cursor
  for you to finish the line. A preview pane and a files column are unchanged —
  their middles still mean what they meant — and `Esc` still calls the whole
  thing off. If anything moves between the moment the pane lights up and the
  moment you let go — your hand to another pane, a pane closing, a tab closing
  under it — nothing is written at all, rather than written somewhere else. The
  terminal that receives the path also takes the keyboard, so the next thing you
  type — `Enter`, or the rest of the command — goes to the shell you dropped
  onto; a drop that writes nothing leaves the keyboard where it was.

- **Dropping a file onto Folio now puts its path on the command line.** Drag a
  file out of File Explorer or the Finder and let go of it over a split, and its
  path arrives in the terminal you dropped it on — not the one you happened to
  be typing in — spelled for the shell running there. It is the same quoting a
  file you *copied* has had since 0.4.1, so PowerShell, `cmd`, a WSL shell and
  the rest each get the spelling they read. Several files let go of together
  arrive on one line, one argument each, and a name the shell has no way to
  spell is reported instead of being mangled. Folio runs no command of its own:
  the path is put in front of the cursor for you to finish the line. The terminal
  that receives the path also takes the keyboard, and Folio comes to the front —
  you dropped the file here, so here is where you can carry on typing. A drop
  that writes nothing changes neither, and a file let go of anywhere that is not
  a terminal — over a card Folio is asking you something on, over a floating
  window, on the tabs or in the gap between panes — is not typed anywhere at
  all, rather than going to whichever pane you were last typing in.

- **A picture on the clipboard now pastes as the path of a file Folio writes
  for it.** A screenshot taken with `Win`+`Shift`+`S`, or with
  `⌘`+`Ctrl`+`Shift`+`4` on a Mac, is on the clipboard as a picture rather than
  as a file, so pasting it into a terminal used to type nothing at all — there
  was nothing there a shell could be handed. Folio now writes it out as a PNG
  and pastes that file's path, quoted for the shell in the pane exactly as the
  path of a file copied in Explorer or the Finder already was. What the
  clipboard holds still decides in one order: a copied file pastes its path,
  text pastes as text, and only a clipboard holding a picture and nothing else
  becomes a file — so copying a picture in a browser, which puts the page's own
  text on the clipboard beside it, goes on pasting the text. The files are
  written to `%TEMP%\folio\clipboard\` on Windows and to the same folder inside
  your own temporary directory on a Mac, named for the moment they were taken,
  and Folio keeps the twenty newest and removes the rest as it writes.

### Changed

- **When Folio's window stops answering, its own log now says which kind of
  event it was answering.** The line Folio writes about a window that held on
  too long used to end at `window_event`, which is every key, every pointer
  move, every redraw and every resize under one word; it now names the handler
  the event went to — `keyboard_input`, `redraw`, `resized` and ten others — so
  a report about a window that froze for a few seconds says where inside it the
  time went. And a run started with `BT_PERF_TRACE` set now writes a full hang
  report after two seconds of silence instead of five, which is where the stalls
  people actually notice live; a run started without it is unchanged.

### Fixed

- **Everything that moves in a window is now drawn once per display frame, on
  both platforms.** Turning a typeset formula over with `‹›`, and the two small
  marks that travel with the block, moved in bursts rather than smoothly: Folio
  was composing pictures as fast as the loop could turn — a dozen or two inside
  one ninety-millisecond motion, far more than a screen can show — and then one
  of them waited the better part of a tenth of a second for the display to take
  them, so the eye read a flurry of near-identical steps and then a pause.
  Windows was the worse of the two. A motion now asks for the next picture the
  display will actually show, at the refresh rate of the monitor the window is
  on, so a formula turning over and its marks travel evenly; a 144 Hz screen
  gets twice the steps a 60 Hz one does. The same rate now governs every other
  fade and slide in the window, and a window with nothing moving in it still
  falls completely silent. A pane printing hard no longer holds anything still
  either: whatever a picture was drawn for, it draws every motion at the instant
  it is drawn, so a formula turning over beside a busy shell keeps moving and
  still finishes on time.

- **A typeset formula's highlight now leaves as soon as the pointer does, like
  every other hover.** Move off a formula and its shading and its two small
  marks stayed for another half-second before they began to go — the only thing
  in the window that waited. They now start leaving on the same movement that
  takes a pane header's buttons, a tab's close, a link's underline and a tooltip
  away, and they fade out over the same ninety milliseconds they faded in on.
  Reaching for one of the two marks still cannot drop the highlight: the marks
  stand inside the formula's own shaded area, so the pointer never leaves it to
  get to them. A formula caught in the middle of turning over goes on turning
  over — looking away no longer cuts it short.

- **A formula's highlight no longer stays lit after the formula has scrolled out
  from under a resting pointer, and turning one over no longer stutters.** Two
  things shared a screen. A formula showing its `$$…$$` source instead of its
  picture had that picture drawn again on every single frame, thrown away each
  time — several hundred renderings of one formula for a window nobody had
  touched — which is the halting the change of face was reported for and which
  left every other formula waiting behind it. A formula Folio could not draw at
  all was tried again on every frame the same way, and fails the same way every
  time; it is now tried once, and again only when something it was worked out
  from changes — the text, the window's scale, the way formulas are read. And
  the pale band under a formula, with the two marks beside it, was worked out
  only when the pointer moved: if the formula itself moved away instead — a
  wheel, a full-screen program redrawing, new output, a resized window — the
  band stayed lit under a hand that was no longer on anything. Folio now asks
  what the pointer is on whenever either of the two moves, and a formula that
  comes back under the pointer before the half-second grace is up simply keeps
  its marks rather than fading them in again.

- **A formula scrolled partly off the top of a full-screen program's screen no
  longer swallows the formula below it, and what is left of it on screen is left
  as text.** When a program like Claude Code moves its output up and a formula's
  opening `$$` goes off the top of the window, the `$$` still on screen is that
  formula's closing one. Folio used to read it as the opening of something new,
  which swallowed everything down as far as the next formula's own `$$` — so the
  next formula stayed as plain text however long you looked at it, and where the
  scrolled formula's `\begin{aligned}` was still visible, that much of it was
  typeset on its own: a picture of part of a formula with the formula's last line
  sitting underneath it as text. The formula below is now typeset, and the rows
  above the stray `$$` that belong to no complete formula are left as the text
  they are. An environment that is whole on the screen is still typeset on its
  own, as it always was, and the `$$` under it stays one line of text: nothing
  on screen says what that `$$` once enclosed. A formula Folio has already
  typeset keeps its picture when its beginning scrolls off the top — that has
  not changed. It is a formula Folio meets for the first time with its
  beginning already gone that stays text: there is nothing on screen that says
  what the whole of it was, so it waits until you scroll its beginning back into
  view.
- **A file name printed right before an opening bracket is recognised as a file
  link again.** An agent that wrote `docs/report.html（commit …）` — the name, a
  bracket, no space in between — left the name dark: Folio read the bracket and
  everything behind it as part of the name and then found no such file. A
  bracket now ends a name from either half of its pair, in every script one is
  written in (`(`, `[`, `{`, `（`, `「`, `【`, `《` and the rest), exactly as the
  closing half always did. Nothing a name could carry is lost by it: a file
  whose name really holds a bracket was already unreadable without quotes,
  because the closing half ends the name too — and quoting a path still opens
  every name there is.

- **A formula no longer flashes back to its source while you scroll inside a
  full-screen program.** On macOS the system hands a terminal a program's
  output in pieces of at most 1,024 bytes, so one redraw of a full screen
  arrives as several of them a millisecond or two apart — and Folio could draw a
  frame in between, with half of the redraw on it and a formula's source only
  half written, which is not a formula, so the picture came down and the text
  showed through. Folio now waits up to three milliseconds for the rest
  whenever the system says there was more to come, and draws the whole redraw at
  once. Typing is not delayed: a keystroke's echo is the system saying there is
  nothing more, so it is drawn on the same frame as before. A program that
  pauses in the middle of writing its own redraw can still be caught half-drawn
  — nothing outside that program can know it has not finished — unless it marks
  its redraws with synchronized output, which Folio has always honoured.

- **A formula scrolling back into view inside a code block stays code.** When a
  formula came back onto the screen, Folio asked whether it still reads those
  lines as a formula — but it worked out the answer for those lines on their
  own, while reading the screen as a whole can reach a different one. On a
  screen whose first `$$` belongs to a formula that began above the top of it,
  the two disagree: reading the whole screen finds a code fence and leaves the
  `$$x^2$$` below it as code, and the shorter reading did not see the fence at
  all — so a picture appeared over a line inside a code block, went away on the
  next redraw of the same screen, and came back on the one after. The question
  is now answered once, by reading the whole screen, which is the same reading
  everything else in Folio uses.

- **Folio no longer looks for a formula it has already failed to find, over and
  over, on a screen that has not changed.** When a formula scrolls out of view,
  Folio keeps its picture aside so that it can be given straight back the moment
  the same text comes into view again. Looking for it means reading the whole
  screen and working out what is on it, and that was being done afresh on every
  read from the program — so a full-screen program that redraws its whole screen
  each time you press a key made Folio do all of it on every keystroke, for each
  formula it was holding aside, to reach the answer it had reached the moment
  before. It is now asked once and asked again the instant anything it depends
  on moves, so a formula scrolling back into view is still typeset in the very
  frame that brings it back.

- **A picture Folio has just taken down does not come back a moment later.**
  While a full-screen program or a reprinting one is redrawing, Folio holds the
  formulas already on the screen steady. If something ruled one of them out
  during that redraw — the shell saying those lines are the command line you
  type on, or Folio reading them again and deciding they are no longer a
  formula, for instance because a code fence opened above them — the picture
  went, and then the end of the redraw put it straight back, over lines it had
  just been ruled off. A formula ruled out while a redraw is in progress now
  stays out; one that is worked out again in the same redraw keeps its new
  picture.

- **After dragging a window's edge back to where it started, formulas in that
  pane are typeset again — they used to stop for good.** If a drag, a divider,
  a zoom or a move between monitors ended on the same size it began on, the
  pane it happened to was left believing the gesture had never finished. Nothing
  looked wrong at the time, and nothing ever came back: from that moment on
  that pane typeset no formula, drew no table, and showed no picture for an
  image path you printed — the ones already on the screen stayed, so the change
  was easy to miss until the next thing you ran came out as source text and
  stayed that way. Anything else Folio waits for a quiet screen to do was
  waiting on the same signal, so it stopped too. It is over when the gesture is,
  now, whether or not the size changed; the shell in that pane is still told
  only when its size actually moved, so nothing is sent to it that it does not
  need.

- **Programs that ask which terminal they are running in now get an answer.**
  Folio was silent when a program asked, and a terminal that says nothing is
  treated as one that can do nothing — so full-screen programs such as Claude
  Code never went on to ask whether Folio can update the screen in one piece,
  and drew their frames the old way instead. Folio now answers with its own name
  and version, the second question gets asked, and those programs switch to
  updating the whole screen at once.

- **A formula whose source was too long for the pane was read and typeset all
  over again when it scrolled up into the history, instead of taking the
  picture it already had with it.** A formula is proven on the rows of the
  screen it stands on; history is kept in lines, and a line too long for the
  pane takes two rows and is still one line. The handover counted rows and
  looked for the end of the block one line too far down, found nothing there,
  and let go of the picture — so the same formula was found and drawn a second
  time, for nothing. Nothing of this was ever on the screen: the last row of a
  block leaves the window in the same instant it leaves the live screen, so
  what came back was already out of sight. It is work that is no longer done,
  not a flicker that has stopped.

- **A formula typeset while a full-screen program was redrawing no longer
  disappears the moment that redraw finishes.** Folio holds its formulas steady
  across a redraw by remembering them when one starts — but a formula Folio
  worked out *during* the redraw was not in that memory, and finishing the redraw
  threw it away, so it went back to LaTeX and had to be worked out and drawn all
  over again. Replaying the owner's own recording, that happened at every one of
  its 117 redraws.

- **When a program finishes one screenful and begins the next in the same breath,
  the formulas on the finished one stay where they are.** Folio went on reading the
  new screen against the old one's layout, so with two identical formulas on show
  one picture was placed over the other's lines and the other went back to LaTeX.

- **A picture is never left standing over a formula that has just been edited.** A
  program that replaced a formula's body in the middle of redrawing could leave
  Folio showing the old picture over the new text until the redraw after it. Every
  picture is now checked against the lines underneath it the moment they change.

- **A formula edited in place is typeset again.** When a program rewrote only the
  middle of a formula and left its `$$` lines untouched, the old picture went — it
  was a picture of text that was no longer there — and nothing replaced it: Folio
  had already answered the question on that formula's first line, and nothing on
  that line had changed to make it ask again. A line that changes now reopens every
  formula it belonged to, and a formula whose lines come back unchanged is still
  never read twice.

- **A picture is never put on a line that is a formula plus something else.** When
  a formula scrolled back into view on the very last line of the content — the line
  a full-screen program often shares with its own "jump to bottom" chip — Folio drew
  the picture there even though it does not read such a line as a formula at all. A
  moment later it took the picture off again, and drew it for real only once the
  formula had scrolled onto a line of its own. Replaying the owner's own scrolling
  session, that was the last of its flicker. A formula scrolling back onto a
  bulleted line, a heading, or a line ending in a comma keeps its picture, and one
  indented as code is left as code — Folio asks the same question there that it
  asks everywhere else.

- **Resizing the window while a program is drawing no longer loses a formula.** A
  redraw that was still in progress when the window changed size went on measuring
  against the old shape of the screen, so a formula could be drawn over the wrong
  lines and another one lost — on a screen whose text had not changed at all. The
  same redraw no longer loses them for a frame when Windows hands back the size it
  settled on, either.

- **A formula edited before Folio finished reading it the first time is read
  again.** If a program replaced a formula's middle while Folio was still drawing
  that formula, the drawing was thrown away — rightly, it was of text that had gone
  — but nothing went back to look at what replaced it, and the formula stayed as
  LaTeX for as long as it was on the screen. Going back to look costs the same
  whether the screen is drawn thirty times a second or three hundred: Folio waits
  for the whole formula to stop moving, not just its first line.

- **A prompt that scrolls up the screen still is not typeset.** Folio decides
  what a command printed from the text itself as it arrives, and it now keeps
  that decision when the line scrolls off into the history rather than working
  it out again from where the line used to sit — so a prompt that spelled the
  same thing a command had printed stayed a prompt on its way past, instead of
  becoming a formula the moment it left the screen.

- **Dragging a window edge no longer lets a formula swallow the line under
  it.** A formula kept while you resize is put back where its source now sits,
  but it went on claiming as many rows as it used to occupy — so widening the
  window, which lets a long formula fit in fewer lines, left it covering the
  text underneath, and narrowing it left the formula showing its source until
  the next redraw. Both lasted as long as the drag. A formula now occupies
  exactly the lines its own source occupies at the width you are at.

- **Two lines with the same formulas in them no longer show each other's
  picture.** Where a line has more than one `$…$` in it, Folio draws the whole
  line's formulas as one picture, and it was filing that picture under the
  formulas alone — so two lines carrying the same formulas with different words
  between them were treated as the same picture, and whichever Folio drew first
  was shown for both. On the other line the second formula appeared in the wrong
  place, over the words beside it. The picture is now filed under where its
  formulas actually sit as well, and lines that really do match go on sharing
  one. The same is true of a line whose formulas are spread over more than one
  row: each row's picture is now filed under the part of the image it shows,
  rather than only under where that part begins.

- **Right-clicking a formula copies that formula, not one from the pane you
  last typed in.** Copy LaTeX asked whichever pane held the keyboard, and a
  right press does not move the keyboard — so in a split, copying from a formula
  in the pane you were only pointing at either did nothing at all or, when the
  other pane happened to hold a block in the same place, copied that one
  instead. All three of a formula's actions — copy, show source, and the source
  toggle's animation — now act on the pane the formula is in.

- **A formula nested past what Folio can draw is refused, rather than ending
  the program.** Some shapes of mathematics nest as deeply as they are long —
  a stack of superscripts, a fraction inside a fraction inside a fraction, or a
  short definition repeated — and following one down far enough used to end
  Folio outright, from text a program had only printed. Folio now stops at its
  limit as it reads, rather than trying to guess beforehand how far a formula
  would take it, and leaves anything past that limit as the text you printed,
  the way it leaves anything else it cannot draw. The limit is far beyond any
  formula written to be read: thirty fractions inside one another, a dozen roots
  inside one another, and every ordinary matrix and alignment are drawn as
  before. An earlier attempt at the same fix guessed the depth in advance, and
  guessed it wrong in both directions — it let several shapes of deep nesting
  through, and refused a long row of perfectly flat fractions.

- **A table of mathematics that would be too big to draw is refused before it is
  drawn, not after.** A row of column markers and a column of row markers is a
  handful of characters, and it asks for a grid as wide and as tall as both —
  eight thousand characters could have asked for seven million cells, which is
  minutes of work and more memory than Folio has. It now works that out from the
  formula itself and leaves anything past its limit as the text you printed. It
  works it out from what the formula becomes rather than from how it was written,
  so putting the markers in a group, or behind an abbreviation, or in no table at
  all, makes no difference. The limit is a sixty-four by sixty-four grid; an
  ordinary matrix, a long alignment, a definition with thirty cases and a
  multi-line derivation are all nowhere near it.

- **A formula cannot run a program.** Mathematics written for TeX has a corner
  of its notation that says "and here is some Typst" — Folio passed that through
  and ran it, so a line of text a program printed into a pane could ask Folio to
  loop forever, or to build something so large that it ran out of memory. The
  first would have left every later formula on screen as plain text, with nothing
  to show it was waiting; the second would have ended Folio. Folio now leaves
  such a formula as the text you printed, the way it leaves anything else it
  cannot draw, and the spacing commands that used to be worked out by running
  them are read instead. Nothing that was ever a formula changes.

- **One formula that cannot be drawn no longer takes the whole window with
  it.** A fault while typesetting was caught in one stage of the work and not in
  the others, so a fault in any of the rest ended Folio — every pane, every
  shell, over one line of mathematics. A formula that goes wrong now stays as
  the text you printed and nothing else is disturbed. Folio also refuses a
  formula nested past its stated limit when the nesting is written without
  braces, which it used to count only one way and let through the other.

- **A screenful of formulas all at once no longer leaves the first few as raw
  text.** Printing a dense page of mathematics — a report, a log, anything that
  arrives in one go — gave Folio more formulas to draw than it queues at a time,
  and the ones it could not take right away were forgotten rather than picked up
  on the next pass. They stayed as `$…$` until something else disturbed the
  line. Folio now comes back for them without letting a continually repainted
  line hold up the rest.

- **An inline formula in a command's output is typeset even when its picture is
  ready only after the prompt has come back.** Printing a file of mathematics
  hands Folio the whole file and the shell's "the command is done" mark in one
  breath, so every picture in it is finished a moment later — and a `$…$` was
  being judged, at that moment, against a command that had already ended. Some
  of them typeset and some were left as raw text, the same file coming out
  differently from one run to the next, and a line long enough to wrap tended to
  lose both of its formulas at once. Which text a command printed is now written
  down as it is printed, so the answer no longer depends on when the picture
  happens to be ready. Displayed `$$` blocks were never affected: they carry
  their own proof.

- **Your prompt is never typeset as mathematics, whatever it says, wherever it
  moves, and whatever a command does to the line afterwards.** Folio decides what a command printed by remembering it at the
  moment it arrives, rather than by working it out afterwards from where things
  sit — so a shell that redraws its prompt with the very text a command had just
  printed gets a prompt, not a formula, and so does one that clears the screen
  first, or reprints after a reset, or writes a shorter prompt over an older
  line. The same holds when the screen moves underneath: inserting, deleting,
  scrolling or shrinking rows carries each line's own history with it instead of
  handing it whatever used to stand in that place. And a command that writes
  over part of your prompt's line — with a tab, an accent, or by filling the
  screen first — does not thereby take the rest of it: Folio asks that every
  character on a line be one a command printed, so text that arrives by a route
  nobody has taught it about is left alone rather than taken for output.

- **Formulas printed after a full-screen program exits are typeset again.** When
  a command shows something full-screen on its way — a pager, an editor, a menu
  — and then carries on printing, everything it printed after that program left
  was treated as though nobody knew where it came from, so `$…$` in it stayed as
  raw text until the next command started. What a command prints on the screen
  it has just been handed back is that command's output, and it is typeset like
  the rest of it. What the full-screen program itself drew is unchanged.

- **A formula is no longer taken down by its neighbour's result.** Folio looks
  at every line that could be the start of something, and inside a block of
  mathematics its own body lines look like that too. When one of those came back
  as "nothing here", it took down whichever picture happened to be standing over
  it — so a matrix could vanish the instant the formula above it finished, and
  come back only when something else made Folio look again. An answer about one
  line is now an answer about that line.

- **A formula block that had partly scrolled into the history is no longer cut
  off at the bottom by the line after it.** Print the same file twice and the
  second printing pushed the first one's `$$` block up until its opening line
  had already settled into the history while its closing line was still on the
  live screen. A block standing across that join was drawn at the height of the
  three lines it was written on rather than at the height of the picture it had
  become, so a tall formula — an integral, a fraction, anything with something
  above and below the line — lost its bottom edge under the next line of
  output. The lines the block spans now make room for the whole of it wherever
  it stands, and a block shorter than its own lines is placed exactly as
  before.

- **Clicking a command's mark beside the scroll bar goes to that command every
  time, also when a formula is on screen.** A mark for one of the newest
  commands — one whose own line is still on the live screen — could not always
  be brought to the top of the pane, because there is nothing below it to
  scroll to, and Folio answered that by forgetting the jump altogether: the
  pane went back to following the output, and the moment there *was* room to
  stand on the command you were left at the bottom instead. It took a second
  click on the same mark to get there, so the same click did two different
  things. A jump is now kept as the place you asked for until you say
  otherwise, and a pane resting at the bottom still follows new output exactly
  as it did.

- **Losing the graphics device no longer closes Folio.** A power cut that
  switches a laptop to battery, a graphics driver that updates itself, a machine
  that changes which GPU it draws on: each of these takes the device away
  underneath whatever is on screen at that instant. Folio already knew how to
  ask the machine for another one and carry on, but a picture that was halfway
  prepared when the device went reached a call that could only end the run —
  the window closed, and every shell open in it closed with it. Nothing on
  that path can end the run any more: the half-prepared picture is dropped, the
  device is asked for again, and the window draws everything it was saying on
  the new one.

- **A dropped file now lands in the terminal you dropped it on even when Folio
  is busy, or when you were last hovering somewhere else.** Where the file was
  let go of was worked out after the fact — when Folio got round to typing the
  path — and it preferred the last place it had seen your pointer, which during
  a drag from another program is wherever your hand happened to be the previous
  time it was over the window. On a split, either reading could name the wrong
  terminal: the one you had been hovering before you went to fetch the file, or
  whichever one your hand had moved on to while Folio was catching up with a
  busy pane. The position is now read at the instant the file is released, and
  nothing later can change it.

- **Closing a pane or a tab no longer pauses the window while the program
  inside it winds down.** Shutting a shell down is several steps, and one of
  them waits for the console host to let go of everything running under it —
  which for a pane that had an agent or a Node program in it can take seconds.
  All of it used to happen between your click and the next frame, so closing a
  tab could leave the window sitting still for as long as the program took to
  go. The pane now leaves the window the moment you close it and is taken apart
  on its own; if something in there takes an unusual amount of time, Folio notes
  it in its own log instead of making you watch. Quitting still waits for those
  to finish, briefly and with a limit, so nothing is left running behind a
  window that has gone.

- **On a Mac, Shift+wheel now scrolls the rows a formula pushed out of view.**
  A typeset formula is taller than the line it was typed on, so it lifts the
  rows above it off the top of the pane; the chip under a full-screen program
  counts them and offers Shift+wheel to go back and read them. On a Mac that
  gesture did nothing at all. macOS turns Shift plus a wheel into a sideways
  scroll before Folio is shown it, so the notch arrived pointing along a line
  instead of up a document, and a pane asked to move up by nothing moved by
  nothing. Folio now reads it as the turn your hand made. Shift+wheel over a
  line longer than the pane still scrolls sideways, and on a Mac it now goes
  the way it has always gone on Windows rather than the opposite way.

- **Closing Folio no longer waits for ever on a disk that has stopped
  answering.** Folio writes your session to a file on the way out, and it waited
  for that write to finish however long it took. On a folder that lives on a
  network share or a cloud-sync drive that has gone quiet, that is for ever —
  the windows are already leaving and there is nothing left to click. Folio now
  gives the save three seconds, writes one line in its own log saying the save
  did not finish, and closes. What you find on the disk next time is the last
  save that completed, which may be the one that was still going when Folio
  left: the file is never half written, so whichever of the two it is, it is
  whole. Folio also keeps the marker that says this run did not see its save
  finish, so the next start offers to restore rather than assuming all was well.
  did not finish and that the last completed one still stands, and closes. The
  file itself is never left half written: a save replaces it in one move. A save
  that has already begun can still finish after Folio has gone, so what you open
  next time is the last save that completed.
- **Closing a pane whose reader is stuck no longer hangs the window.** When a
  pane closes, Folio waits for the thread that was reading that shell's output
  to come out of its last read. On one machine that wait held the window for
  five seconds. It is now bounded: the reader gets two seconds, and past that it
  is left to finish by itself while the window carries on. A shell that refuses
  to be reaped no longer leaves the console and the reader standing behind it
  either.

- **A Codex, Claude Code or Copilot configuration file Folio cannot read is now
  left exactly as it is.** Turning one of the agent rows on Settings ▸ Agents on
  or off reads that program's own configuration file first — `config.toml`,
  `settings.json`, `folio.json` — and a file Folio could not read was taken for
  a file that was not there: a single byte that is not UTF-8 somewhere in it, a
  permission that withholds it, or another program holding it open, and Folio
  wrote a fresh file over the one you had written, with no copy kept anywhere.
  Folio now tells "there is no file" apart from "there is a file and I could not
  read it". The second one leaves your file untouched, shows the row as
  something it will not write, and says so when the row is pressed. A file it
  can read is still copied beside itself before anything is changed, as it was.

- **A picture that has not changed no longer asks the GPU to draw it again.**
  Repeated redraws compare each pane and the window furniture around it with the last
  complete picture. Caret blinks, hover marks and moving panes still redraw;
  resizing and replacing a surface always get their own frame.

- **Reordering your profiles no longer closes the window when a pane is
  opened.** Moving a row in Settings ▸ Profiles, or deleting one, changed which
  profile every already-open pane thought it was running: a pane held the row's
  place in the list rather than the profile itself, and the list had just moved
  under it. Splitting or restarting such a pane started whichever profile had
  slid into that place, without saying so, and that wrong profile was written
  into your saved session, so it came back the same way the next morning. When
  the list had grown shorter than the place a pane was holding, opening a pane
  closed Folio outright, taking every tab in every window with it. A pane now
  names its profile by the profile, so a list that moves cannot move it; a row
  you really did delete costs that pane its shell choice and nothing else, and
  the pane says so in its first line.

- **On a Mac, a second Folio started from a terminal now hands over instead of
  becoming a second writer.** Which Folio is allowed to write your settings and
  your saved session was decided in a directory whose location came from
  `TMPDIR` — so a Folio started from the Dock and one started from an `ssh`
  session, a script or a login shell that clears the environment were each
  certain they were the only one. Both wrote, the later write erased the
  earlier, and the second window never found the first to pass your command line
  to. The location is now asked of macOS itself, which gives every process you
  run the same answer however it was started.
- **On a Mac, Folio no longer tells the system it has dealt with a keyboard
  shortcut that is not its own.** The summon key's handler answered "handled" to
  every hot key event offered to Folio, including ones registered by something
  else inside the same application and ones it could not read at all. It now
  says so only for the press it actually acted on, and leaves the rest to carry
  on to whoever was waiting for them.
- **On a Mac, a video left playing no longer builds up memory for as long as it
  is open.** The thread that plays a video ran without an autorelease pool, so
  everything the system's media framework handed it in passing was held until
  the preview was closed rather than released as it went. It now opens and
  drains one on every pass, so a video that plays for an hour costs what a video
  that plays for a minute does.

- **A Markdown file too big to edit now keeps up with the file.** Above 8 MB
  Folio shows the first screen of a document and will not let you type in it.
  That first screen used to be the one it was opened with, for as long as the
  pane stayed open: writing to the file from anywhere else changed nothing on
  screen, and `Reload from disk` changed nothing either. It now shows the file's
  current first screen whenever the file is written. Unsaved edits of your own
  are still never replaced — the notice about the file having changed stays up,
  with the same two answers on it.

- **A formula broken across two lines is now typeset.** When a program wraps its
  own text — Claude Code does, at the width of the pane — a formula that does not
  fit the rest of a line is split in two, with `$x` left at the end of one line
  and the rest of it starting the next. Folio read each line by itself, so the
  formula between them stayed as you typed it while the ones that happened to fit
  on a line were typeset around it. The two halves are now read together: the
  formula is set on the line that finishes it, and the piece left hanging above is
  taken down with it. A line only joins the one below it when the reading is
  unambiguous — one unclosed `$`, the matching one near the start of the next
  line, and nothing in between that starts a new paragraph, bullet or heading — so
  a price at the end of a sentence is still a price.
- **An inline formula in earlier output stays typeset when the window is
  resized.** Changing a window's width has every formula on screen set again at
  the new size. A `$…$` formula inside a command's output had to establish a
  second time that its line was printed by a command, and the evidence for that
  leaves when the prompt line the command began on scrolls out of the history —
  so an older formula came back as the text you typed and stayed that way for
  the rest of the session, while the `$$…$$` blocks beside it were set again as
  usual. Where a line was printed is now noted as the line arrives and kept with
  it, so a resize gives back the formulas it took away.

- **A typeset formula no longer flashes back to its source while a program
  repaints the screen.** A redraw arriving in several pieces now keeps the
  formula's picture until the whole turn has finished, without waiting for it
  to be typeset again.

- **Maximising or resizing a window no longer pauses when a pane has a long
  history behind it.** Every pane on screen was copying its whole terminal
  twice at the start of a resize — everything that had scrolled past included,
  on the window thread, before a single frame at the new size was drawn — and
  the second of those two copies was thrown away without ever being read. It is
  gone.

- **The tinted block behind a formula's source now stops where the text does.**
  Pressing the show-source mark on a display formula used to lay a shaded band
  across the whole width of the pane, however short the `$$…$$` lines standing on
  it were, with the two marks out at the far right edge of the window. The band
  now fits the longest of those lines, with the same clear column around it that
  the typeset formula's block has, and the show-source and copy marks sit at that
  edge — so both forms of a formula read as the same block.

- **On a Mac, a tab can be dragged to reorder it or out into its own window
  again; the window moves only when the empty part of the header is dragged.**

- **A Markdown line beginning with an angle bracket and carrying non-ASCII
  text no longer crashes the preview.** Such a line now stays readable, and
  the other tabs in the window stay open.

- **Showing a formula's source now applies to that one formula on the screen.**
  Turning a block over with the `‹›` mark used to be remembered against the
  formula's own text for the rest of the session, so printing the same `$$…$$`
  again showed that one as source too, and a formula you had once looked behind
  never went back to its typeset form on its own. Looking at the source is an
  action on the block in front of you: the next block arrives typeset, and the
  block you turned over keeps its face, including while it scrolls off the
  screen into the history above.

- **A wheel notch at the end of a pane no longer asks for the same frame.**
  When the view and its contents stay put, the wheel skips the terminal frame
  and leaves the next keystroke or pointer move less work to wait behind.
  A scroll that moves the view still draws it, and the scrollbar still gets
  its own frame when its thumb or fade changes.

- **A traced run no longer pauses when whatever is reading the trace falls
  behind.** This only concerns runs started with one of the `BT_…_TRACE`
  variables set — a diagnostic recording, not an ordinary launch. Those runs
  used to stop dead for seconds at a time, mid-keystroke, whenever the shell
  collecting the trace stopped reading it: the window was waiting for the
  recording to be taken, so the very thing being measured was what made it slow.
  The trace's lines now go to a thread of their own, and a run that produces them
  faster than they can be written drops some and says how many rather than
  holding the window. What Folio writes about itself no longer goes near that
  recording either: the watchdog that reports a window which has stopped
  answering writes the report and the line naming it into `diagnostics.log`
  through a handle of its own, so it stays at work whatever the shell reading the
  trace is doing, and so do the notes about a copied picture that could not be
  saved and a session that could not be written on the way out. The other
  messages Folio can print on that console — each of them about something that
  has already gone wrong — still go to it directly and can still wait for it.

## 0.4.1-preview — 2026-09-16

### Added

- **Settings has an About page, and it says which Folio this is.** The last word
  in the list on the left. It carries the version and the build it came from —
  the same line `folio --version` prints and the same one at the top of every
  diagnostic file, so a report about something going wrong can quote it — the
  system and processor this copy was made for, and three rows that open in your
  browser: the release notes, the place a defect is filed, and the licences of
  everything Folio is made of. The licences row opens the copy that came with
  this download where there is one, so what you read is what this copy was built
  from. Nothing on the page is a setting, so nothing on it can be changed by
  accident.

- A copied file or a copied path from Explorer/Finder pastes as one quoted
  argument in the shell’s own spelling.

### Changed

- **A typeset formula now sits in a block with room around it, and the block's
  two marks sit inside it.** A display formula keeps whole blank lines above and
  below it — as many as the window has room for — and a clear column on each
  side, so it no longer touches the text it stands between; hovering it lights
  that whole region, and the show-source and copy marks stand at its right edge,
  on its middle line, drawn as the same buttons a pane head wears. An inline formula no longer starts a few pixels
  right of where its source began, which closes the gap that opened before it in
  the middle of a sentence.

- **Switching a formula between its typeset and source forms now animates
  instead of jumping.** The block grows or shrinks to the height of the other
  form while the picture and the `$$…$$` text cross-fade, and its two marks ride
  along with it; if you have asked your system to reduce motion, the change still
  happens in a single frame.

- **The release page now also carries download files with a fixed name, so a
  link to the latest build never goes stale.** Beside the versioned archive and
  disk image there is now a `folio-windows-x64.zip` and a
  `Folio-macos-arm64.dmg` — the same bytes under a name that does not change
  from one release to the next, covered by the same checksum file.

- **The checksum files on the release page can be checked where you downloaded
  them.** Both `SHA256SUMS.txt` and `SHA256SUMS-macos.txt` now name each file
  plainly — the hash, two spaces, the file name — so putting them next to the
  archive or the disk image and running `sha256sum -c` or `shasum -c` answers
  `OK` without anything being edited first. The macOS file used to carry the
  folder it was built in ahead of the name, which sent that check looking for a
  directory nobody downloaded.

- **Two files moved out of the top of the repository.** `CONVENTIONS.md` is now
  `docs/CONVENTIONS.md`, beside the rest of the written record, and the
  `cargo-about` configuration and template — `about.toml` and `about.hbs` — are
  now `licenses/about.toml` and `licenses/about.hbs`, beside the licence texts
  they assemble into `THIRD-PARTY-NOTICES.md`. Nothing about any of them changed
  except where they are; a fork that names one by path updates the path.

- **The gates that compile now all compile the same thing.** The shortcut-table
  script and its generator asked cargo for a narrower set of crates than the test
  gate does, and one dependency came out with one feature more under one of them
  than the other — enough to make every crate above it, up to and including the
  test executable, a different thing to build. So each script rebuilt what the
  run before it had just finished building. They now ask for what the gate asks
  for, and the feature is named outright rather than arriving by accident, so a
  script run after a green gate has nothing left to compile.

### Fixed

- **On a Mac, an idle Folio window no longer keeps a processor core busy.** A
  window left open behind other windows — nothing typed into it, nothing
  printing, no watched file having moved — ran at a full core for as long as
  it stayed open. Every turn, Folio brings its file, folder and repository
  watches level with what the window is showing; doing so took out a small
  handle on the window's own loop, and on macOS taking one of those out is
  itself a request for another turn, so the window kept asking itself to wake
  up. The handle is now taken where it was always meant to be — once, at the
  moment a watch is actually opened — and a turn that changes nothing costs
  nothing.

- **Typing stays responsive while a pane prints a lot.** A build log or a long
  answer scrolling past no longer holds the window for seconds at a time: the
  output is taken in a short turn and the window goes back to answering the
  keyboard, the pointer and the tab strip between turns, however much is still
  coming.

- With the search box open, typing in a pane that is printing no longer pauses
  while the whole history is searched again.

- **On a Mac, a window that is covered or hidden no longer keeps drawing at full
  speed, and the pictures it prepared while hidden no longer pile up on the
  GPU.**

- **Text sent from a phone keyboard, or from another program that types for you,
  now reaches the terminal.** A sentence typed on a phone through its desktop
  companion, or pasted in by a tool that types on your behalf, arrived as
  characters with no key behind them — and Folio, which decides what a keystroke
  means by looking at which key it was, had nothing to look at and typed
  nothing. Such a character is now the key that would have produced it, so it
  lands wherever you are typing: the shell, the search box, a file being edited,
  a tab you are renaming.

- **Opening Settings no longer makes the window wait.** Clicking the gear used
  to freeze the window for several seconds on a machine with a lot of fonts
  installed, every time it was opened. Folio was asking the system for the list
  of monospaced families — the list the `Terminal font` picker offers — and
  waiting for the answer before it would draw anything. It now asks in the
  background: the page opens at once, the font row shows the family you are
  already using, and the rest of the list fills in a moment later. `Install
  fonts…` still does what it did — leave, install a family, come back, and it
  is there.

- **Copying a formula no longer leaves the window busy.** The tick that confirms
  the copy has always come down after a moment on screen, but the window went on
  asking to be woken for it for as long as it stayed open — one processor core,
  spent on a window doing nothing. The confirmation is now finished with when it
  leaves the screen.

- **Turning a formula into its source no longer makes the window hesitate.**
  Pressing the `‹›` mark beside a typeset block — or pressing it again to put the
  picture back — used to hitch for a moment before the block changed. Changing a
  formula makes the lines under it move, and the window was measuring the width
  of every line in the whole scrollback again to find out where they landed;
  with a long history behind you, that is the pause. It now measures the lines
  that actually changed, and the rest of your scrollback is left alone. The
  formula, the mark and the block's own tools are unchanged.

- A formula whose macros expand into more and more text is now refused instead
  of exhausting memory.

- **A formula's two marks stay with the formula.** Switching tabs or closing a
  pane used to leave the marks from the block you had been pointing at standing
  over whatever came next, until you moved the mouse. And in a window split into
  panes of different sizes, the marks in an unfocused pane were placed — and
  could be pressed — as though that pane were the size of the focused one.

- Inner products and bra-kets written with `\langle … \rangle` now typeset.

- **Aiming a card's window with the wheel keeps up with the hand.** On a tall
  card over a pane with a long history, every notch used to copy out every line
  between the bottom of the pane and the place the card was pointing at — three
  times over, for one row of movement — so the card stuttered and the notches
  piled up behind it. It now reads only as far as it has to and keeps only the
  rows the card draws. Where a notch lands, and where the card stops at the top,
  are unchanged.

- **On a Mac, Folio can be quit with no window open.** With the last window
  closed — where Folio stays in the Dock — `Quit Folio` in the menu bar was
  greyed out and `Cmd+Q` did nothing, so the only way out was the Dock icon's
  own menu. Quit now answers from an empty desk, and it is the same quit as
  always: what you were working on is written down before Folio goes.

- **On a Mac, opening a shortcut from the files column opens what it points at.**
  The files column does not run programs, and a shortcut whose name gave nothing
  away — a link called `notes` pointing at an application — used to get past that
  rule and start the application. The rule is now asked about the file the link
  leads to, which is the file that would have been opened, so a shortcut to a
  program is refused for what it is and a shortcut to a document still opens.

- **On a Mac, a previewed page is only reported as guarded when it really is.**
  When a preview was built twice in quick succession — a slow start and the
  retry behind it — the rules that keep a local page from reaching the network
  could land on the page that had just been replaced, while Folio went on saying
  the new page was covered by them. A page now only counts as guarded when its
  own rules are on it, and closing a preview while its rules were still being
  prepared no longer leaves that preview unable to prepare them again.

- **On a Mac, Folio carries its licences with it.** The application now holds
  the MIT and Apache-2.0 licence texts, the notices for every library it is
  built from, and the trademark notice, inside `Folio.app` — so they travel with
  the copy you keep rather than with a disk image you throw away. The Windows
  download has carried the same four files since the first release; the macOS
  one, until now, carried none of them.

## 0.4.0-preview — 2026-09-14

### Added

- **Folio runs on macOS.** It is the same program, built for Apple silicon and
  asking for macOS 14 or newer. A pane runs a native shell, and the files
  column, the Git page and the preview pane — a Markdown document you can edit
  where it is shown, pictures, video, typeset formulas — are the ones that were
  already there; several windows come back where they were left, and a
  background agent still says when it has finished. Where the two systems
  differ, this one follows the system it is on: the chords are Command chords
  and the Shortcuts page lists them that way, the window wears the three buttons
  macOS draws instead of its own, and the menu bar is a real one. A settings
  file written on either machine is read by both.

- **On a Mac, a key summons the terminal from anywhere.** Press `Ctrl` and the
  backtick key — the one to the left of `1` — and the quick terminal comes down
  over whatever you were doing, from any application; press it again and it
  goes away and the keyboard goes back where it came from. It is an ordinary
  row on the Shortcuts page: record a different key and it takes effect at once,
  and `Restore all defaults` brings this one back. Folio asks macOS for no
  permission to do it. Windows keeps `Win` and the backtick, unchanged.

- **On a Mac, Folio's Dock icon offers a new window and a new tab.** Press and
  hold the icon — or right-click it — and `New window` and `New tab` stand above
  the rows macOS puts there for every app. They work from another app and from
  an empty desk: with every window closed, either one opens a window.

- **A focus card can be asked to record what it does.** Set `BT_CARD_TRACE` to a
  file name and Folio appends one line per decision behind the miniature in the
  sidebar — where it clamped itself and which row that leaves on screen, what a
  wheel notch did to it, every new size the pane behind it was given, and what a
  change of display did to its height. It is off unless you set it, changes
  nothing about how the card behaves, and shares its clock with `BT_MOUSE_TRACE`
  so the two files read together. `docs/BT-ENVIRONMENT.md` says what a trace
  file can contain.

### Changed

- **Moving the caret and typing in a very large Markdown document no longer
  waits on the whole document.** Each keystroke used to copy the text, rescan
  every line for the widest one and rebuild the caret's map of lines from
  scratch; on a three-megabyte document that was a visible pause on every key.
  The document is now shared rather than copied, undo is derived from the edit
  itself, and the line index and widths are kept up to date incrementally. On
  that same document a caret move inside a paragraph went from a few
  milliseconds to nothing measurable; opening it is still slow, and that is the
  next change.

- **A typeset formula in a terminal pane now looks like the rest of the
  window.** Resting on one lays down the same rounded panel a fenced code block
  stands on, instead of the flat dark rectangle it used to get. The two buttons
  beside it have become ordinary Folio icons: nothing at rest, fading in as your
  pointer reaches the formula, lit in a soft pill under the pointer and a darker
  one while you hold the button down. The first one now shows where it takes you
  — angle brackets on a typeset formula, an eye on one showing its source — and
  the second turns into a tick for a moment once the LaTeX is on your clipboard.
  Both are a little larger than before, and right-clicking a formula still
  offers `Copy LaTeX`.

- **Opening a very large Markdown document is immediate.** The preview used to
  lay out every block of a document before it could show the first one; on a
  three-megabyte file that was a three-second freeze, and a resize or a scale
  change paid it again. It now measures only the blocks near the viewport and
  estimates the rest from their line counts, correcting each estimate as it
  scrolls into view; an anchor keeps the text under your eye where it is while
  the heights above it settle, so the page does not jump. On that same file
  the open went from about 3.4 seconds to a tenth of one, and a keystroke
  from 143 to 66 milliseconds.

- **A Markdown file opens complete, and is editable at once.** The preview
  used to read the first 64 KB of a file, show that much under a
  `Read-only · 64 KB` badge, and fetch the rest only when you entered edit,
  which made an editable file look read-only. It now reads the whole file on
  the first open, up to the 8 MB editing ceiling: the document scrolls to its
  end and the caret can go in immediately. A file over the ceiling keeps a head
  on screen and the badge says its real size. Text, CSV and diffs keep their
  bounded first read.

- **Badges in a Markdown preview now stand in a row instead of one under
  another.** Folio does not fetch pictures from the web, and it used to say so
  in a full-width card three lines tall for every one of them — so the four
  badges at the top of a README became four cards and pushed the document itself
  off the screen. A web picture written in a line with anything else on it is
  now a small rounded chip on that line, carrying its alt text (or the last part
  of its address where there is no alt), wrapping like a word among the words.
  Resting on one says why there is no picture and shows the address; pressing it
  opens the address, as before. A picture written alone in its own paragraph
  still gets the card, and pictures on your disk are unchanged.
- **On a Mac, the Appearance page no longer offers Acrylic.** Folio does not
  blur what sits behind a window on macOS, and the row said so on a line of its
  own while its picker stood greyed at `Off` — a row about something there is
  nothing to decide. It is gone instead, and the rows under it close up. Nothing
  changes on Windows, where the row still stands and still reports when a
  version of Windows has no blur to offer; and your settings file keeps the
  value either way, so a Windows machine sharing that file reads it as before.
- **A changed file in the Git page says what happened to it in words.** Resting
  on a row used to give you its path and the name of the group it stands in,
  leaving git's two letters to be read off the badges: `UU` on a row meant
  nothing unless you already knew it meant a merge conflict where both sides
  changed the file. Every status git can report now has a phrase — `Modified`,
  `Added, staged`, `Modified, staged — modified since`,
  `Conflict (both modified)` — and git's own two letters stand beside it, so a
  row and a `git status` in the pane next to it still read the same. The files
  under an expanded commit say theirs too.

- **The card that appears when you rest on a file now fades in.** It used to
  arrive solid in a single frame; it now takes the same ninety milliseconds the
  small labels elsewhere in the window take, and arrives without moving or
  growing. The wait before it appears is unchanged, it still leaves the instant
  you move away, and if you have asked your system for reduced motion it appears
  and leaves instantly as before.
- **The top bar takes the hand anywhere it is not a button.** Dragging the window
  by its top bar used to work only in the stretch between the last tab and the
  buttons in the corner; the space between two tabs, the strip above them and the
  gaps around the settings button did nothing at all. Every part of that bar that
  is not one of this window's own buttons now picks the window up, and
  double-clicking it still does what it did.
- **A quit keeps every tab a window was holding.** Quitting with a page still
  closing down could write the session out again on the way out, with the tabs
  that had already gone missing from it; the document a quit writes is the one
  the next launch reads, so it is no longer written over by the teardown.
- **The preview's bottom line appears only when it has something to say.** Every
  preview — a document, a Markdown page, a picture, a PDF, a recording, in a pane
  or in a window you have torn off — used to keep a strip along its bottom edge
  whether or not there was anything in it, and most of the time there was not.
  The page now runs all the way to the bottom of what is showing it. When there
  is news — `Saved`, `Revealed`, or a file that changed on disk under your edits
  — it floats over the bottom of the page for as long as it has something to say,
  and moves nothing while it comes and goes; `Changed on disk` still waits there
  with `Reload` and `Keep my edits` until you answer it. A file you cannot edit
  shows a small padlock at the end of the path row instead, which says why when
  you point at it — and says it out loud, for two seconds, the moment you click
  into the page or type at it, with `Open in default app` beside it. What a
  picture is — `PNG · 670 KB`, `6000 × 4000 · shown at 41%` — has moved up to
  that same row, and the video controls sit on the bottom edge of the picture
  with the recording's format and size at their right end.
- **A file the preview cannot show offers to open it in the default app, like an
  executable does.** A picture Folio declines — one with too many pixels, a file
  too large to read, a picture that would not load — used to say so in the middle
  of an empty pane and leave you there. It now wears the card an unknown file
  type has always worn: the same sentence, and under it the same
  `Open in default app` button, which hands the file to whatever the system has
  registered for it. The same goes for a picture Folio could not draw, a pane too
  small to draw one in, and a recording in a format this machine cannot play. It
  is the same card in a torn-off window as in a pane. A file the disk itself
  refused to read still says only what happened, because another program would
  be refused in the same way.

### Fixed

- **The two marks beside a typeset formula behave like the rest of the window's
  buttons.** Pointing at `‹›` or `⧉` now lights that mark the way a pane head's
  buttons light, so you can see which one a click would reach. Turning a block
  into its source makes it taller — the marks move with it straight away,
  instead of staying beside the shape the block used to have until you took the
  pointer off the formula and brought it back; the same is true when a window
  resize re-wraps the block or the display scale changes. And they arrive, move
  and leave gently rather than blinking in and out. If you have asked your
  system for less motion, they simply appear and disappear where they belong.

- **A focus card's head now rests the way a tab does.** The pane-count badge on
  a card in the sidebar sat where it belongs only while the pointer is on the
  card, one slot in from the end of the head, with an empty gap beside it where
  the pin would appear. At rest the badge now stands at the end of the head,
  against the close button's own place; bring the pointer onto the card and it
  slides aside to let the pin and the `×` in, and slides back when the pointer
  leaves — the same movement, over the same time, that a tab has always used.
  A pinned card keeps its pin on show at rest, as it did. Reduced motion settles
  it in one frame, as before.

- **Hovering a typeset formula puts that formula's two buttons beside it.** The
  `<>` and copy buttons had stopped arriving at all, and where a second formula
  had been hovered a moment earlier they stood beside *that* one — a block above
  the one the pointer was on, which was the block wearing the lit background. The
  background is painted into the terminal's own picture and the buttons are drawn
  over it, and the buttons were being placed from the picture the pane had shown
  *before* the pointer moved, with nothing coming back to look again once the new
  one had been drawn. The buttons now belong to the formula the pointer named, so
  they and the background can never be on two different formulas, and the moment
  the picture that lights a formula reaches the screen its buttons are drawn on
  it and fade in over the same ninety milliseconds as everything else this window
  raises under the hand.

- **A terminal showing formulas no longer stops when the window leaves full screen or
  changes display.** A formula that a narrower window folded onto the next row was
  placed on that row but still counted against the row its line began on, and the
  frame's own check refused it; Folio then ended without a word. The placement and
  the check now read the same row, and if a frame is ever refused again the cause is
  written to the panic log and shown before Folio stops.

- **On a Mac, the key that summons the terminal works.** Pressing `Ctrl` and the
  backtick key did nothing at all: no window came down, and nothing was written
  anywhere that said why. Folio had claimed the chord from the system correctly
  — which is why no other application saw it either — but the handler that
  answers the press asked macOS for the wrong name for the one piece of
  information that says *which* key was pressed, got nothing back, and threw the
  press away. It now reads the right one, and the key does what the Shortcuts
  page says it does. Windows was never affected.
- **On a Mac, the candidate list follows the caret onto a second display.**
  Typing Chinese in a terminal pane on a display other than the main one left
  the list of candidates floating in the middle of the window instead of
  standing under the line you were typing into; on the main display it was
  correct. macOS remembers where a window's caret is in screen coordinates and
  only asks again when it is told the answer is stale, and moving a window
  between displays never told it. Folio now says it again on every move, so the
  list stands under the caret on any display, and dragging a window costs no
  more than typing in it.
- **On a Mac, the sentence under `Option key sends Alt` is shown whole.** The
  row's description is the one deliberately longer than the rest, and on the
  General page — whose pickers take their half of the row — it ran past the
  three lines a row would draw and lost its ending to an ellipsis: the fourth
  line in Chinese, the fourth through sixth in English. A settings row now grows
  to hold every line its sentence needs, and the row, the page's scrolling and
  where you can click all follow the taller row. Every other description is
  unchanged, and none of them got longer.
- **A formula no longer loses a symbol without saying so.** A sign that the
  fonts on your computer cannot draw used to vanish from a typeset formula
  silently — not as a box or a blank, but gone, with the symbols either side of
  it closed up as though the author had never written it. Folio now asks your
  font list for every character a formula needs, not only for Chinese, Japanese
  and Korean ones, and names every installed family that can draw one the maths
  font cannot; a character nothing on the machine can draw stops the formula and
  leaves the source on the page, which is the answer that is at least true. (On
  a Mac, this is being read against a report that the minus sign in front of a
  fraction was not drawn where Windows drew it.)
  The picture of the formula was also being laid on the screen half a pixel off
  and smeared across two rows at half strength; a picture shown at its own size
  is now laid on whole pixels and copied untouched.

- **A table on a focus card lines up again, and a symbol on one is drawn whole.** A card showing a box-drawing table whose cells hold Chinese text drew its borders in a different place on every row, while the same table in the pane beside it was square. A card's rows are now laid out column by column, as the pane's own grid is, so a wide character takes exactly two columns and a border stands in the same place on every row. Laying them out that way then cut a symbol off at its column's edge — a shell printing `○` showed a clean circle in the pane and a large arc on the card — so a card now draws a symbol from the same font the pane draws it from, and sets a character that is still wider than the columns it stands in a little smaller until it fits them, instead of cutting it.
- **A formula you have selected now looks selected.** Dragging across a typeset
  formula in a terminal pane copied it correctly — the formula's own source,
  right where the picture stands — but nothing on screen said so: the selection
  coloured the text on either side and stopped at the picture. It now washes the
  picture too, in the same colour, for a formula set in a line and for one
  standing in a block of its own, on the live screen and back through the
  scrollback. What it washes is what it copies: cross part of a formula and only
  that part is washed.
- Keep rendering later formulas when an incomplete macro definition fails during conversion.
- Refuse recursive macros and excessive macro expansion before they can stall formula rendering.
- Preserve prose and later headings after display formulas, empty delimiters, and unfinished math blocks.
- Inline integrals and sums now match the terminal font size and fit within their line on shorter line spacing.
- **Two formulas on one line no longer take each other's place away when the
  window is narrow.** A line carrying `$…$` twice was typeset whole at a wide
  window and printed as source at a narrow one: the two formulas are worked out
  together, as one picture, and a picture cannot straddle the place where the
  line folds — so as soon as the fold fell between them, neither was drawn.
  Each formula now stands on the row its own `$` stands on, and how wide the
  window is decides nothing about whether either of them is set.
- **A one-column matrix keeps its rows.** `\begin{pmatrix} x \\ y \end{pmatrix}`
  came out as the single row `(x y)` beside a 2×2 on the line above that came
  out right. Both had had their row separators shortened to one backslash before
  Folio ever saw them, and Folio's repair for that read the row from the `&`
  between its cells — which a one-column row has not got. It now reads a
  one-column row too, and still leaves `\,`, `\;`, `\:`, `\!` and real commands
  such as `\frac` alone.
- **A table on a focus card lines up again.** A card showing a box-drawing table whose cells hold Chinese text drew its borders in a different place on every row, while the same table in the pane beside it was square. A card's rows are now laid out column by column, as the pane's own grid is, so a wide character takes exactly two columns and a border stands in the same place on every row.
- **On a Mac, `Edit ▸ Copy` and `Edit ▸ Paste` now act on the pane you are
  looking at.** Over a terminal they did nothing at all — the selection never
  reached the clipboard and the menu's Paste typed nothing — because macOS had
  nothing to hand those rows to. They now copy the terminal's selection, or the
  text you have selected in a file you are editing in a preview, and paste into
  whichever of the two has the keyboard. A page in a web pane and a text field
  in a dialog keep answering for themselves as before, and `⌘C` and `⌘V` are
  unchanged.

- **`REMOTES` on the Git page no longer sits under a highlight nobody put
  there.** Opening the sub-group left a filled block behind the word that stayed
  after the pointer had gone — it was the keyboard's own highlight, which a
  header should never have worn, and the pointer's could land on the wrong row
  besides, because the list is rebuilt as the repository changes while your hand
  holds still. At rest `REMOTES` is now exactly one of the section headers —
  the same size, weight and ink as `BRANCHES` and `COMMITS` above and below it —
  and under the pointer the word and its triangle brighten to the colour a row
  of the files column takes under your hand, with no block at any time. The
  whole row is still what you press — and it now brightens every time your
  pointer comes back to it. Opening or closing the sub-group used to leave the
  keyboard standing on the header, which lights it in the same way a hover does,
  so from your first press on it the word stayed bright and moving the pointer
  on and off it changed nothing. A press with the pointer now leaves the
  keyboard where it already was; the arrow keys still reach the header, and
  Enter on it still opens and closes the group.
- **A focus card shows what it showed across a resize or a display move
  again.** A resting card keeps following the newest output. Alt+wheel moves
  by whole lines and stops at the top without debt: the first downward notch
  moves immediately, even after a taller card reduces how far back it can go.
  And carrying the window to a display of another scale no longer loses the
  card's place through the size the window wears on the way: while the pane is
  briefly too small to hold what the card was showing, the card shows as much of
  it as fits and goes back to the rows you left it on.
- **On a Mac, tabs now take the whole width of the title bar.** The tab strip was setting aside room for four window buttons on a window that carries one — macOS draws minimise, zoom and close at the other end of the bar — so the tabs were squeezed to their profile marks, their names hidden, with a wide empty band before the settings gear. Seven tabs in a 934-point window now stand 91 points wide with their names showing instead of 72 without.
- **Every picker in Settings that offers profiles now shows their marks.** The
  summoned terminal's `Profile for new tabs` listed its profiles as bare words,
  where the same profiles carry their marks on the tab strip, on the pane head
  and in the new-tab `⌄` menu; and no picker showed a mark once it was closed.
  Both now do, in the marks and at the size that menu draws. `Default profile`,
  which is a deferral rather than a profile, keeps its word and borrows nobody's
  mark.

- Reuse unchanged Markdown prose measurements across caret moves and edits, and reset block scroll offsets after reparsing.

- **While you type pinyin, the caret stands at the end of what you have typed.**
  Composing over the terminal, the upright caret sometimes stood one character
  short of the end — `hai'you` drew it between `o` and `u`, `bin` between `i`
  and `n` — although the underline covered the whole composition. It happened
  where the composition was typed over a Chinese character already on screen:
  half of that character stayed behind, and the caret read it as a wide
  character it was standing in the middle of. A composition now takes the whole
  character it covers half of with it, and a caret trusts such a half only when
  the character it belongs to is still there.
- **On a Mac, a pane started from the Dock or Finder can read what you type in
  your own language.** A shell started that way is handed no language setting at
  all by macOS, and one without it treats every byte as a character: `天下为公`
  typed at the prompt came back as `天` followed by highlighted `<008b>`, and a
  filename outside ASCII listed as question marks. Folio already passed on your
  own regional setting when this machine has a matching locale installed; when it
  has none — a Chinese interface in the United States is an ordinary example — it
  now says what it does know, that this window reads UTF-8, in the same two
  variables Terminal.app uses for it. It still never picks a country for you, and
  a setting you already have — inherited, or written into a profile — is left
  alone.
- **A menu closes when you press the button that opened it.** With the `Open ⌄`
  menu up at the end of a preview's path row, pressing the pill again opened it
  afresh instead of putting it away, and so did the `…` that stands in for the
  folders a narrow row has no width to show. Every menu in this window that hangs
  from a button now closes on a press on that button and does nothing else with
  that press — including the commit graph's branch list inside a torn-off window,
  which had the same fault and was not in the report.
- **A formula that took a moment to typeset now appears when it is ready.** A
  page holding `$$\int_0^1 x\,dx$$` shows the formula as you wrote it until the
  picture is set, which is right — but if the picture arrived after the page had
  settled, nothing put it on the page, and the page went on showing the text you
  wrote until you scrolled it, resized the pane or opened something else. The
  first formula of a session is the slow one, so this was most of the time it
  could happen.
- **A letter your keyboard layout makes reaches the shell.** On a German layout
  `ü`, `ä`, `ö` and `ß` produced nothing at all — the key was pressed, and not a
  byte left this window — and the same was true of every other letter a layout
  composes outside ASCII. They are sent now, as the bytes they are. A character
  an input method is still composing is unaffected: it arrives once, when the
  method commits it, exactly as before.
- **The command marks along a pane's right edge keep up with the shell.** A tick
  that appeared, or turned red when a command failed, was drawn as soon as it
  landed only while the command was visibly still running; once the pane went
  quiet the rail waited for the next thing you did — a keystroke, a pointer
  moved — before it caught up, and it could sit a whole minute behind if you sat
  still. It is drawn on the turn the shell reports it now.
- **A chord held with the Windows key no longer types its letter into a box.**
  With the caret in the command palette, the search box, a name being edited, the
  branch prompt, the commit graph's search or a document being edited, `Win+C`
  typed a `c`. A chord is not text, and none of these take one now.

## 0.3.0-preview — 2026-09-12

### Added

- **Right-clicking the empty space in a files column opens the folder the column
  is standing in — including when that folder is empty.** The menu offers
  `New terminal here`, `New file…` and `New folder…`, then `Copy path`,
  `Insert path into terminal` and `Reveal in Explorer` — the same verbs a folder
  row offers, about the folder at the top of the column. A column standing in a
  folder with nothing in it has no rows at all, and the whole of its body is that
  folder's, so that is where the first file in it gets made. It cannot be renamed
  or deleted from there, because that folder is the column itself rather than a
  row in it.
- **A Markdown file can be edited where it is shown.** Click into the text and
  the block under the caret shows its own Markdown — a paragraph, heading, list
  or quote in the reading typeface, a code block, table or formula in the
  monospace source view — while everything else on the page stays as it reads; type, and it is
  back to its rendered form as soon as you leave it. The caret moves between
  blocks the way it moves between lines, so there is nothing to enter and
  nothing to leave — arrows, Home and End, Enter and Backspace all do what they
  do anywhere else. Escape leaves the page, and so does clicking the empty
  ground beside the text or clicking away from the pane altogether: the page
  goes back to reading as a page, and the caret stays where you left it. Ctrl+S
  saves, Ctrl+Z undoes. What you select is what you copy, and copying out of a
  document you are editing brings the marks with it, so what you paste back is
  what was there. A file
  that cannot be edited — one Folio could not fully read, one past the size
  limit, a table or a patch — still opens, reads and copies exactly as before,
  and the foot of the pane says why it is read-only.
- **The files column's menu can create a file or a folder and send either to the
  Recycle Bin.** Right-clicking a folder row now offers `New file…` and
  `New folder…` above the line, and both file rows and folder rows offer
  `Delete`. The two New rows put a field in the tree where the new row will
  appear, so the name is typed where the name is going to be: Enter creates,
  Escape cancels, and a name the folder will not take — nothing at all, a path
  separator, a name something in that folder already has whether or not it is
  spelled in the same case, a Windows device name like `NUL` — turns red in the
  box rather than being explained somewhere else.
  A new file is created empty and selected; opening it is your next click.
  `Delete` asks nothing first, because what it does is reversible: the row goes
  to the Recycle Bin and never to a permanent delete, and a folder goes whole.
  The name editor behind all of this — the one a tab, a file's name, a
  breadcrumb and an address already use — has grown the things a text field
  should have: shift-selection, word jumps with Ctrl and the arrow keys,
  Ctrl+Backspace, copy, cut, paste, and an in-progress Chinese or Japanese
  composition drawn where it is being typed instead of appearing a syllable at a
  time.

- **Undo and redo while editing a file in a preview, on Ctrl+Z and Ctrl+Y.** The
  little editor a text or Markdown file opens in had no way back: a line deleted
  by accident was gone, and the only thing that could restore it was closing the
  file without saving and losing everything else with it. Ctrl+Z now takes back
  the last change and Ctrl+Y puts it again. A run of typing comes back in one
  press rather than one letter at a time — the run ends where you moved the
  caret, pressed Enter, changed from typing to deleting, or saved — and a paste
  comes back in one press however much it brought. Both keys work only while a
  preview holds the keyboard, so Ctrl+Z still suspends a job in every terminal.
  The history belongs to the file, so the same file open in two panes has one
  history and either pane can walk it.
- **The unsaved dot goes out when you undo back to your last save.** It used to
  stay lit until the file was written, because there was no history to compare
  against; now there is, so returning to the words the file was saved with is
  being saved again as far as the dot is concerned, and one more change lights
  it back up.

### Changed

- **A paragraph, heading, list or quote you click into stays in the reading
  typeface and shows its Markdown marks.** The `#` of a heading, the `**` around
  a bold phrase, the `- ` in front of a list item and the `> ` down the side of
  a quote come back where the file spells them, in the same face and at the same
  size the page was being read in — the block you are editing looks like the
  page it is part of, and the caret walks the file's own characters through the
  marks. Only code blocks, tables and formulas switch to the monospace source
  view, because for those the way the characters line up is part of what they
  say.

- **Folio is one program now: starting it again opens another window of the one
  that is running, instead of a second copy.** One Folio, one set of settings,
  one place your tabs are remembered — whether you start it from the taskbar,
  the Start menu, a shortcut or `folio.exe`. What you get is a new window, which
  is what starting a program again gets you everywhere else. If you would rather
  it opened a tab in the window you used last, **Settings > General > Opening
  Folio again** has both answers, and `folio.exe --new-window` and
  `folio.exe --tab` ask for one or the other whatever that row says.
- **Explorer's "Open in Folio" and `folio-here.cmd` open a tab in the window you
  used last, and bring that window forward.** Those two mean "give me a terminal
  in this folder" rather than "give me another Folio", so they answer the same
  way whichever way the row above is set. That is also what VS Code's external
  terminal runs, so a terminal opened from there arrives as a tab.
- A second copy of Folio starts only when there is no Folio running to answer.
  If the one that is running has stopped responding, is in the middle of
  quitting, or already has a queue of launches waiting, the one you just started
  opens a window of its own rather than leaving you with nothing — which
  includes the case you most want it to: starting Folio again while a window is
  frozen now gets you a window.
- **`folio .` and `folio --cwd ..\somewhere` open where you meant.** A folder
  named relative to wherever you typed the command used to work when no Folio
  was running and answer "There is no ." when one was.
- **A text or Markdown file larger than the preview's first look can be
  edited.** The pane reads the first 64 KB of a file to show it to you, which is
  what keeps opening a huge file as quick as opening a small one, and until now
  that was also as much of it as there was: anything longer stayed read-only.
  Turning a Markdown file to its source, or clicking into the text of a file
  that has a caret, now reads the rest of it, once, and the file becomes
  editable — up to 8 MB, past which it stays read-only and says so.
- **When the window stops answering, Folio's own log now names what it was
  doing.** The line it writes afterwards used to say only that the window had
  been woken, which was true of every pause it ever recorded and told nobody
  anything: a file arriving, a folder being listed, a formula being drawn and a
  repository being read all looked the same from outside. Each of those now has
  its own name in that line, so a pause you report carries the answer with it.
  The report Folio writes beside it, in `hang-reports`, can also be read back to
  the exact function that was running: the build that ships now carries the
  table that makes that possible, which it did not before.

- **The preview's `Open` menu opens when the pointer rests on it, as the other
  menus do.** Resting on the `Open` control at the right of a preview's
  breadcrumb row for a quarter of a second brings up the same menu a click
  brings up, and clicking still opens it at once. Moving onto the menu keeps it
  there; leaving both the control and the menu puts it away. This is the rule the
  `⌄` in a pane's head and the one beside the tab strip's `+` have always
  followed, and it applies to a preview in a pane and to one in a window of its
  own.

### Fixed

- **The tab cards look right, and Alt+wheel scrolls them, on a second display of
  a different scale.** A window carried from a 200% display to a 150% one took
  the card column's scroll position with it in the first display's pixels, which
  is a third of the way further down the list on the second: the card at the top
  lost its head off the edge of the column, the `New tab` row came away from the
  foot of the panel and left a blank strip under the last card, and Alt+wheel
  over that blank aimed at nothing. Where the column stands is now said again in
  the new display's pixels, along with everything else a display change
  re-measures — a list at its end is at its end on both, a list halfway down is
  halfway down on both, and carrying the window back and forth leaves it where
  it was.
- **Pictures up to 64 MB open in the preview, and one with more pixels than
  Folio keeps is shown reduced instead of refused.** A phone photograph or a 4K
  screenshot is five to fifteen megabytes and a 24 megapixel frame is ninety-six
  megabytes of pixels, and both came back as `Preview failed: inline image
  exceeds its decode limit` — from the files column, from a link, from a
  `.md` page and from the card that pops up over a file name. The limit was real
  and it belonged to something else: eight megabytes is the allowance for a
  picture a program pastes straight into a terminal, where it arrives unasked and
  a screenful of them can arrive at once. A picture file you opened is now read
  up to 64 MB, and one whose picture is larger than Folio will hold is resampled
  down to fit and shown, with its real size in the line under it —
  `6000 × 4000 · shown at 5016 × 3344 · PNG · 92.0 MB · Fit`. Past either line the
  pane says which one: the file is too large, or the picture has too many pixels.
  Nothing changed for a picture written into the terminal itself, which keeps the
  allowance it had.
- **Moving the pointer over a floating window no longer highlights, or pops the
  preview card of, the row hidden underneath it.** A preview window standing
  over a files column was solid to look at and see-through to the mouse: resting
  the pointer on the window's own text lit up whichever row of the column was
  behind it, and a moment later raised that row's preview card on top of the
  window. The window now takes the pointer over its whole face, exactly as it
  takes a click — nothing behind it lights up, nothing behind it opens a card,
  and the pane it is covering no longer shows the buttons a pane shows when the
  pointer is inside it. The window's own rows go on lighting up and go on
  showing their cards.
- **Typing with an input method into a Markdown page shows the letters being
  composed.** Typing Chinese into a `.md` file put the candidate list under the
  caret and drew nothing at all between them: the reading you were part-way
  through was invisible until the moment it committed. The letters now stand at
  the caret in the typeface of the paragraph, heading, list, quote, code block
  or table they are being typed into, with a line under them and the input
  method's own caret inside them, and the rest of the line moves along in front
  of them instead of being drawn over. The candidate list follows that caret
  rather than the one in front of the letters. Nothing is written into the file
  until it commits, so Escape still leaves the document exactly as it was.
- **A composition left over from the command palette or a name box no longer
  floats over the window.** Typing Chinese into the palette and then picking a
  row left the candidate list on screen, over whatever the row had opened, with
  nothing to receive what you typed next and no way to dismiss it. A composition
  now belongs to the box it was started in: when that box goes away the letters
  in progress are thrown away with it, and they are never delivered to whatever
  has the keyboard next.
- **An animated picture of any ordinary size plays.** A `.gif` over about 8 MB
  showed nothing at all — an empty pane saying the picture was too large and the
  preview had failed — which is the size an ordinary screen recording or a
  simulation reaches in a few seconds. Folio now reads an animation from its file
  as it plays it instead of holding the whole file in memory, so the length of
  the file is no longer a limit on it, and the foot of the pane says which of the
  two sizes it means on the rare file it still declines.
- **Switching between animated pictures shows the right one at once.** Opening a
  second `.gif` on a pane or a card that was already playing one could leave the
  first file's picture on the glass, under the second file's name, for as long as
  a quarter of an hour. Each picture that moves is now its own picture, so the
  new one's first frame is the one you see.
- **An animation starts from its beginning when you open it.** A `.gif` opened
  from the files column, or handed to a pane showing something else, begins at
  its first frame rather than wherever it had got to. A picture in a tab you are
  not looking at holds still instead of playing to itself, so coming back to the
  tab finds it where you left it and moving again straight away.
- **A picture pane, a formula and a hover card keep the picture they were
  given.** Opening several large pictures at once, or a page with more formulas
  than Folio keeps in memory, no longer sends the window into a loop of decoding
  and re-drawing that nobody asked for: what is already on the glass stays there.
  A picture that was fetched to sharpen now arrives by itself, and a card
  hovering a Markdown file shows its pictures instead of their placeholders.
- **The name box takes a long pasted line without freezing the window**, and a
  name the folder will not take now says so in the box even when the difference
  is only upper and lower case, instead of leaving Enter doing nothing. Clicking
  away from a half-typed new name lands on the row you clicked, not on the one
  that moved up into its place.
- **Saving a file you are editing no longer throws away what you can undo.**
  Folio now recognises its own write instead of reading it as somebody else's, so
  Ctrl+S leaves the history, the caret and the selection where they were — and a
  file that really did change under you still says so. A read that arrives while
  you are typing no longer replaces what you typed.
- **A menu opened over a floating window is about that window, not about what it
  is covering.** Right-clicking a float used to raise the menu of the row hidden
  underneath it — `Delete` included, with nothing on screen naming the file. A
  floating tree also no longer offers verbs it cannot carry out, and a column
  standing in an empty folder can finally open the menu that makes its first
  file.
- **The caret in an edited paragraph sits on the character it edits, on Chinese
  text too.** On a line of Chinese the bar stood to the right of the character it
  was in front of, further out with every character on the line, so typing landed
  somewhere other than where the bar was. A file's text is drawn in evenly spaced
  character cells, and Folio counts everything about editing it in those cells —
  where the caret is, where a click lands, where a line folds — but the Chinese
  characters themselves were being drawn a little narrower than the two cells
  they take up, so the words crept leftwards away from where the caret was put.
  Each character now fills the cells it takes up, and clicking a Chinese
  character puts the caret on that character rather than after it.
- **A new empty file can be typed into.** A file just made with `New file…`
  opened as a blank page, and clicking anywhere in it did nothing at all: there
  was no text on the page for the click to land on, so nothing took the caret and
  nothing took what was typed. A page with nothing on it now takes the caret from
  a click anywhere in the body, and the first thing typed becomes its first
  paragraph.
- **The caret at the end of a file that does not end in a blank line stands at
  the end of the text.** In a file whose last line has no line break after it,
  pressing End on that line — or typing at the end of the document — drew the
  caret at the left margin of a line below the text that is not there, and left
  the paragraph rendered rather than showing the source being typed into.
- **A Markdown page full of large screenshots no longer freezes the window.**
  Opening this project's own README on a large screen could pin a processor core
  and leave the window standing on whatever it had drawn last — still answering
  the mouse and the keyboard as far as Windows was concerned, but painting
  nothing, for as long as the page stayed open. The page was asking for its
  pictures over and over: Folio keeps a fixed amount of memory for the pictures
  it has read from disk, nine screenshots of that size do not all fit in it, and
  every one that arrived pushed out another that the page was still showing —
  which the page read as a picture it had never asked for, so it asked again.
  A page now keeps the answer it was given: it holds the picture it is drawing,
  it asks for each one once, and running out of room to remember them changes
  nothing about what it shows. Clicking into one of its paragraphs to edit it no
  longer freezes the window either: once a screenshot had been sharpened to the
  width it is drawn at, the page could no longer tell which file it had come
  from, so the first thing the reader did to the page sent every one of those
  reads out again. A picture the page is already showing is now never read from
  disk a second time, and a picture arriving for something already on the screen
  no longer re-lays the whole document out.

- **A long animated GIF now plays in the preview instead of showing its first
  frame.** Folio used to read every frame of an animation into memory before
  drawing any of it, and a file whose frames did not all fit was drawn as its
  first frame and left there, with nothing to say why — which for a screen
  recording is most of them: a few hundred frames at any useful size is more
  pixels than a preview of one file is worth holding. Frames are now read a
  second or so ahead of the one on the screen and let go of behind it, so the
  length of the file no longer decides whether it moves, and what it costs while
  it plays is the same whether it is eight frames long or four hundred. A file
  Folio still will not play — one whose single frames are larger than it will
  hold, or that will not read past the first — says so in the foot of the pane
  instead of just sitting still.

- **A large picture zoomed past the edge of its pane no longer disappears.**
  Enlarge a big photograph or screenshot far enough and the preview went blank
  where the picture had been, while the file's name above it and the line of
  facts below it stayed exactly where they were. The pixels were never lost:
  everything drawn at one instant shares a single store of graphics memory, and
  a picture large enough to fill most of that store could be pushed out of it
  after Folio had settled on drawing it and before it was drawn — which left
  nothing to draw from and nothing to say about it. Folio now holds on to a
  picture from the moment it settles on drawing it until it is drawn, so
  nothing prepared afterwards can take it away.
- **A zoomed picture can be dragged up and down as well as sideways.** Dragging
  one that stands taller than the pane moved it left and right and then stopped
  answering vertically. Letting go of the button while the pointer was outside
  the window was not heard at all, so the picture was still being carried when
  you thought you had put it down, and the next drag started from where the last
  one had left off. Because a preview pane fills the window's height, up and
  down is the direction that runs out of window almost at once, while sideways
  travels across the pane next door and never leaves — which is why one
  direction worked and the other did not. Letting go now ends the gesture
  wherever the pointer has reached, inside the window or out of it.
- **A file that says what it is keeps saying it after you save.** Some files
  begin with a few bytes that name their own encoding — the ones Windows
  PowerShell writes are the common case here — and Folio read those bytes,
  showed the file correctly, and then threw the answer away. Editing one line of
  such a file and pressing Ctrl+S wrote the whole of it back in a different
  encoding, without the opening bytes: every character outside the line you
  touched came out different, and nothing said so. The encoding now travels with
  the text from the moment it is read to the moment it is written, so a save
  changes the part you edited and leaves the rest of the file identical, down to
  the byte. Line endings, trailing spaces and a missing last line survived
  before and still do.
- **A file Folio cannot fully read is no longer offered for editing.** Where a
  few bytes will not read as text, they are drawn as the replacement character
  so the rest of the file can still be looked at — but saving would have put
  that stand-in character on the disk in place of whatever was really there.
  Such a file is now read-only, and the foot of the pane says why.

## 0.2.5-preview — 2026-09-09

### Fixed

- **A window with a full row of tabs can still be dragged by its title bar.**
  Folio draws its own title bar, and the only stretch of it a window may be
  moved by is whatever the row of tabs leaves over. With a dozen open the row
  reached all the way to the settings gear and left nothing: the top edge could
  not be dragged anywhere along its length, and double clicking it to maximise
  had no place to happen either. The row now stops 96 pixels short of the
  buttons in the corner whatever it is carrying, so there is always a band there
  to take hold of. The tabs pay for it the way they already pay for one more
  tab, by growing narrower first and scrolling only once they are as narrow as
  they go. A window with the tabs down its side is unaffected, and so is a
  window with room to spare in the bar.
- **The four split pictures at the top of a pane's menu can be seen.** The
  little pane and the four bars around it were drawn in the same hairline the
  menu's own edge is drawn in, and a hairline is meant to be found rather than
  read: on the white card of the light theme it came out a shade off white, so
  the picture the menu opens with was there and invisible, and on the dark card
  it was only just there. Both are now drawn in the ink the words on the rows
  under them are set in, which is a colour a reader can trace, and any theme
  whose own ink sits too close to its card has the picture lifted clear of it.
  The bar under the pointer still turns and fills with the accent colour, so
  pointing at one reads as clearly as it ever did.
- **A table whose rows the printing program wrapped is drawn whole.** An agent
  printing a wide table lays it out to the width of the pane and wraps a long
  row onto a second line, usually stopping a word or two short of the last
  column. Folio drew the heading and the first row and left every row after
  them as text, which is worse than drawing no table at all. A wrapped row is
  now put back together whenever the lines spell exactly the row the heading
  calls for, whether or not the program ran the first line right to the edge,
  and while the rest of a row is still on its way nothing is drawn from the part
  that has arrived. A row is read across as many lines as it takes rather than
  two, so a row a narrow pane broke into three or four comes back as one row,
  and a break that lands right beside one of the row's own bars is read as the
  break it is instead of ending the row there. A pipe anywhere in the same
  paragraph that the table cannot account for takes the whole table down, so a
  table can no longer end halfway through the block it was printed in; a pipe
  after a blank line, and a second table under a caption line, still leave it
  standing.
- **A pane restored behind another tab comes up at the width it is going to
  have.** A window that was closed maximized is put back at its own rectangle
  first and maximized a moment later, and until now the panes of every tab were
  measured against that first rectangle. A tab you were not looking at kept
  those measurements until you clicked it, so a tab holding three panes in a
  window too narrow for three could start a shell two columns wide: its first
  prompt came out two characters to a line, and widening the pane afterwards
  could not put back together what had already scrolled past. Every tab now
  follows the window, whether or not it is the one on screen, and a pane is
  never started at the width of the little bar the layout shows in place of a
  pane it has no room for. The little picture on a tab's card follows the pane
  the same way: where a line was broken across rows at a width the pane has since
  stopped having, the card puts it back together instead of drawing the old
  break.
- **Text follows the edge of the window again while you drag it, with several
  tabs open.** A pane re-wraps its lines as the window is made narrower, and
  since every tab started following the window's size that stayed true only for
  the tab you could see: each tab behind it laid its own pane out again on every
  step of the drag, ahead of the one in front of you. With six tabs open the
  picture arrived about a third less often, and with a dozen it arrived rarely
  enough that the text looked frozen at the old width until the drag stopped. A
  tab you are not looking at now takes the new size the way its shell already
  did — once, when the drag settles — and the pane in front of you re-wraps on
  every step, as it always has.

## 0.2.4-preview — 2026-09-08

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

- **Resizing a window no longer types into a bash prompt.** A Git Bash or WSL
  pane running Folio's shell integration picked up `;8~` at its prompt every
  time the window changed size, and the next command answered `bash: syntax
  error near unexpected token ';'`. What arrived there is a key of PowerShell's:
  Folio presses it after a resize so PowerShell can put its prompt back in the
  right place, and bash, which has no such key, typed the part of it that it
  could read. It now goes only to the PowerShell panes it was written for.
  Resizing a bash, zsh or Command Prompt pane sends that pane nothing.

- **A printed table is drawn whole or not at all.** A table an agent prints
  arrives a row at a time, and a row can arrive damaged: a cell holding a bare
  `|` counts as extra columns, and a program that lays out its own output wraps
  a long row onto a second line. Either one used to stop the table where it
  stood, so the rows above were drawn as a table and everything after them
  stayed text, with no sign that a row was missing. Now a row the program itself
  wrapped is put back together and drawn as the one row it was printed as, using
  the width the terminal was when it was printed, so dragging the window does
  not split it again. And a line that starts with `|` and is not a row of the
  table takes the whole table down: every line goes back to text, including the
  rows that were already drawn. Output that merely starts with a pipe — a branch
  graph, a compiler's underline, the side of somebody else's box — no longer
  ends a table halfway and leaves half of one on the screen.

- **A saved tab whose shell is no longer on the machine opens as a tab.**
  Uninstalling Git with a Git Bash tab pinned stopped Folio before its first
  window, on every launch, until the saved session was edited by hand. Such a
  tab now opens in the folder it was standing in, running the shell the machine
  does have, with a first line naming the one it could not start. A machine with
  nothing at all to run gets a pane that says so.

- **A terminal font size Folio cannot draw no longer stops it starting.** A
  `terminal_font_size` of `0` in `settings.json`, from a hand edit or a
  half-written file, reached the text layer and ended the launch before any
  window opened. Any stored size is now brought into the range the Appearance
  page offers, and a size inside that range is used exactly as written.

- **A preferences or session file Folio cannot read is kept.** A file that would
  not parse, or that a newer Folio wrote, was replaced by defaults with one line
  on a console nobody was watching, and the first thing you did afterwards
  overwrote it. Folio now copies it beside itself as `<name>.rejected-` and the
  date and time before anything can replace it, and says on the window which
  file it was and where the copy is. The same is true of `keybindings.json`,
  `profiles.json` and `pins.json`.

- **A second Folio no longer erases the first one's preferences and tabs.** Two
  Folio windows started from two processes each held the whole of
  `settings.json` and `session.json` from the moment they opened and wrote all
  of it back, so whichever wrote last erased everything the other had done.
  Only one process now writes those two files; a second one keeps its window,
  keeps every gesture in it working, and says once that nothing in it is saved.

- **A saved session larger than Folio will open says so.** A file describing
  more windows, tabs and panes than Folio opens at once was expanded into that
  many shells before the first frame. Folio now stops at a ceiling, keeps the
  front of what you had arranged, and tells you the rest was left closed. Every
  file it reads also has a size limit, and one past it is moved aside rather
  than read whole.

- **A restored window always comes back with its title bar on a monitor.** A
  window whose recorded size still fitted its display kept its recorded corner
  whatever that corner was, so a stale or hand-edited position could bring the
  window back above the top of the desktop with nothing to take hold of. The
  corner now comes back onto the display far enough for the title bar to be
  caught, and a window parked half off the side is still left where you parked
  it.

- **A setting that could not be saved is saved when you choose it again.**
  Choosing a row wrote the file and remembered the new value whether or not the
  write worked, so choosing the same row a second time matched what was already
  remembered and tried nothing. Folio now knows a value has not reached the
  disk and writes it again at the next chance.

- **A file that keeps refusing to be written says so and stops trying.** A full
  disk or a folder gone read-only meant a save attempt every second and a half
  for as long as the window stayed open, each one leaving a half-written file
  behind, and nothing on screen about any of it. Folio now removes what it left,
  gives up after a few tries, says which file could not be saved, and starts
  again the next time you change something.

- **The marker Folio leaves while it runs no longer empties whatever stands at
  its name.** A link planted at `session.lock` was followed and emptied at every
  launch. Folio now looks at the name before opening it, leaves a link alone,
  and never truncates anything.

- **Your PowerShell profile is replaced in one step, and every write takes a
  copy.** Adding the integration line emptied the file and then wrote it, so a
  shell starting at that instant read a profile with nothing in it; and a second
  write on the same day took no copy at all, losing whatever you had typed into
  the file since the first. The file is now replaced whole, and every write
  keeps a copy of what was there.

- **A PowerShell integration file that was upgraded, deleted or truncated is
  written again.** Once the line was in your profile, the file it points at was
  never looked at again, so a Folio upgrade left your shells reading an older
  copy and a cleaner left them reading nothing, in both cases with the setting
  still saying it was installed. It is now compared against what ships and
  written again when it differs, the way the bash one already was.

- **Turning the Explorer entry off while it is being installed takes effect.**
  The switch compared your press against what the machine was before the install
  started, so an Off pressed while On was still running matched, started
  nothing, and the entry appeared anyway. A press made during that few seconds
  is now kept and acted on the moment the first job ends.

- **Folio's first-page Explorer entry reads as this copy's only when it would
  open this copy.** The entry names one fixed program inside a folder, and Folio
  compared only the folder, so an entry that would open a different Folio was
  read as belonging to this one. Both are now compared, and a launch that could
  not put itself behind the entry leaves it alone rather than re-registering it
  at every start.

- **Dismissing the update mark while a check is running is not undone by it.**
  The daily check wrote back the state it read before asking, so a mark you had
  just acknowledged came back lit and stayed that way until you acknowledged it
  after every check. The check now writes only what it learned.

- **A command line a program printed is never typed into a restored pane.** A
  program in a pane could print shell markers around any text it liked, and that
  text became the tab's last command and was put back on the prompt of the
  restored pane with the cursor after it. Only a line your own keyboard was
  present for is kept now, and only within a length anybody could have typed.
- **A local page previewed in a pane reads only the folder it was opened in.**
  A `.html` file opened from the file column could name a picture, a stylesheet,
  a script or a frame anywhere else on the disk, or on a network share, and the
  preview fetched it: only the address of the page itself was checked, and
  nothing a page loads is that address. A previewed local file now reads its own
  folder and the folders under it, and nothing else — not a folder beside it,
  not another drive, not a share, and nothing from the network. Pages opened
  from an address go on loading their own contents as before, and they now
  cannot read the disk at all.

- **A page cannot hold a preview pane shut with its own message boxes.** The
  engine's default was to open a modal window for every `alert`, `confirm` and
  `prompt` a page asked for, and a page that asked in a loop opened another the
  moment you dismissed one. The pane now answers those itself and says so for a
  moment on its bottom strip, so a page can ask as often as it likes and the
  pane stays yours.

- **Closing a preview pane lets go of everything opening it took.** Two
  subscriptions on the shared engine, the engine handle the pane cached, the
  slot a still-arriving page would have landed in, and the counter behind the
  search box were all left standing: a search in a page rebuilt after a crash or
  an engine update stopped showing how many matches it had found, and a pane
  that was torn out and closed could not be torn out again for the rest of the
  session.

- **A preview whose engine never answers offers a Retry that works.** When the
  engine was asked for and said nothing at all, the pane drew the card that says
  so and the button on it did nothing when pressed. It now asks for the engine
  again. A browser that exits under a page that is still open is no longer
  silently ignored either: the page is rebuilt rather than left as an empty
  rectangle.

- **A preview that fails to start leaves nothing running behind it.** Any step
  after the engine handed over a page kept that page and its browser alive on a
  pane that had just failed to set it up. The pane now closes what it made
  before it reports the failure, and each of its settings is applied through the
  oldest interface that carries it, so an engine that cannot take one of them no
  longer loses the other eight. An engine too old to be given the page rules at
  all will show a page from an address and refuses to open a file from your
  disk, saying which rule it could not be given.

- **The key that summons the quick terminal raises it when it is on screen and
  you are working somewhere else.** The chord read only whether the window was
  visible, so pressing it while the terminal stood behind your editor sent the
  terminal away and gave the keyboard to a third window. It now raises and
  focuses a terminal that is up but not yours, and hides one you are typing in.

- **The summon key needs Ctrl, Alt or Win.** Recording a bare letter, or a
  letter with only Shift, claimed that key from every program on the machine for
  as long as Folio ran, and brought the state back at the next launch because the
  row is saved. The recorder refuses such a chord and says why, a line in
  `keybindings.json` carrying one is refused with the same reason on its row, and
  the claim is never made.

- **A hung program in front does not stop Folio.** Summoning the terminal joins
  the input queue of whichever window has the keyboard, and joining an
  application that has stopped answering handed Folio that application's freeze.
  Folio now asks Windows whether that window is answering before joining it, and
  gives the whole handover a deadline.

- **The quick terminal answers its own key and nobody else's.** Another program
  running as you could post the same message Windows posts for the chord, and the
  terminal came down for it even when the shortcut was cleared or when Windows
  had refused Folio the key. A press is now acted on only while Folio's own claim
  on that key is live.

- **One pane's runaway hook no longer silences the others.** The endpoint an
  agent's hooks speak into counted every message against one allowance for the
  whole window, so a hook stuck in a loop in one pane spent it and the other
  panes' notifications were dropped. Each pane now has its own allowance, counted
  after the message says which pane it is for.

- **One toast per turn, whatever a program prints.** A program in a pane could
  raise a fresh Folio notification for every different sentence it wrote between
  one turn and the next. A turn ending now raises one notification, the way the
  attention dot always has, and the words in it can no longer carry the invisible
  characters that reorder or hide the rest of the line.

- **Folio's entry in the Explorer menu is repaired when it was left half
  written.** A registration interrupted part way through leaves a menu entry with
  nothing to run; Folio read that as no entry at all and left it alone at every
  later launch. It now recognises it and writes it again, and a registration that
  fails removes what it had written rather than leaving the half.

- **The Explorer menu switch says so when Windows will not answer.** A failed
  query to the Windows package database read as "nothing is registered", so
  turning the switch off reported success over an entry that was still there. The
  row now says the question could not be answered, and turning it off says so
  rather than claiming to have removed something.

- **Folio's menu entry registers correctly from a folder with `#`, `?` or `%` in
  its name.** The folder's path was handed to Windows without being encoded, so
  those three characters named a different folder and the entry pointed nowhere.

- **The watchdog reports a window that stops answering while it is idle.** A
  window parked with nothing to wait for was never asked whether it was alive, so
  a freeze that began the moment something woke it produced no report at all. It
  is now asked whenever the silence passes the threshold, whatever it was waiting
  for.

- Internal contracts at the Windows boundary: a cancelled read on the attention
  endpoint is waited for before its storage is reused or freed, two error paths
  there no longer leak a handle, and `folio attention` checks the endpoint name
  it was given, asks for the narrowest impersonation level, and writes under a
  deadline.
- **A pane starts in the environment this window is standing in.** Every pane
  rebuilt its environment from the machine's registry and wrote it over the one
  Folio itself was launched with, so the `PATH` your shell exported was thrown
  away: the program Folio found under a name and the program your pane found
  under the same name could be two different files. A pane now inherits this
  window's environment, with anything a profile row sets layered on top, and the
  registry only fills in names this window does not have.

- **Closing a pane closes what the pane started.** A build, a server or a watcher
  started inside a pane went on running after the pane was gone, with nothing
  left on screen to show it and no way to reach it but the task manager. What a
  pane starts now ends with the pane.

- **Closing a pane no longer waits on a program that will not stop.** The window
  waited without limit for a killed shell to answer. It waits a couple of seconds
  now, and what has not ended by then is ended with the pane.

- **A profile whose program is a batch file runs the file you picked.** A `.cmd`
  or `.bat` whose path holds an `&`, a `|`, a `<`, a `>` or a `^` was handed to
  the command interpreter as two commands rather than one, so the pane ran
  something else, or nothing. The whole line is now quoted the way the
  interpreter reads it.

- **A profile row that cannot name a variable changes nothing.** A row whose
  name held an equals sign used to set a different variable than the one it
  named, quietly overwriting it in every pane of that profile. Such a row, and
  one whose name holds a null character, is now left out of the pane's
  environment entirely, like the empty-name row beside it.

- **A window that could not come back after a graphics device was lost is not
  reported as recovered.** When a driver reset took the device away, Folio marked
  itself recovered before every window had a device again, so a window that could
  not be given one sat blank while everything else carried on. The recovery is
  now over when the last window has come back, and a window that could not is
  tried again.

- **A video thumbnail refuses a frame it cannot measure.** A file whose stream
  changed shape while it was being read could make the frame copy read past the
  end of the decoded frame. Both ways in now check the frame's own stride and
  length first and refuse it otherwise.

- **Closing Folio while a video is still being read waits for the reader.** The
  media platform was taken down as soon as the window loop ended, which could be
  while a slow file was still inside the decoder.

- **Five contracts with Windows that Folio was keeping only half of.** A registry
  string is given the terminator the system reads it to before it is expanded; a
  notification's connection to Windows is released before the apartment holding
  it is; the video engine's device protection is asked of the object documented
  to carry it and a machine that refuses is reported rather than run over; an
  engine that fails to open is taken off the ledger it was put on; and closing a
  video pane stops waiting for the engine's own thread after a couple of seconds.
  Nothing about any of them is visible while they work.

- **The last shell exiting closes its tab, and closing the last tab ends the
  program.** A tab holding one pane whose shell exited stayed open with a dead
  shell inside it, and a window whose only tab ended that way went on running
  with nothing left to show. Two sweeps ask each pane whether its shell has
  ended, and the first one was taking the answer away from the second; a pane now
  remembers that its shell ended and says so to everyone who asks.

- **A formula printed in the pane you are not typing in gets drawn.** A block
  between `$$` in a split pane that did not hold the keyboard stayed as its own
  source until you clicked into that pane. Every pane on screen now settles its
  rows and typesets what it printed, on the pass that was already drawing it.

- **A display, font, theme or language change reaches every pane.** All four hand
  every pane new measurements, but only the pane holding the keyboard was told
  that the pictures it had already drawn were built for the old ones, so a split
  pane kept formulas rastered for the previous cell size until something else
  happened to rebuild them.

- **A resize while a full-screen program is up leaves the command marks where
  their commands are.** Dragging the window edge while an editor or a coding
  agent is on screen re-wraps the screen behind it. The ticks on the command rail
  kept the rows they stood on before that re-wrap, so leaving the program put
  them in the middle of some other line. They now move with the lines they name.

- **Running the same command twice keeps a mark for each run.** Two prompt lines
  that read exactly alike were treated as one when the window was resized, so the
  older command's tick was moved onto the newer command's prompt.

- **A card in the focus column keeps up with a pane whose screen a timer
  released.** A program can ask for a screenful to be held back and then never
  say it is finished; Folio releases it on a timer. The cells moved with no
  output arriving, and the card went on showing the picture from before the
  release until the pane said something else.

- **A search hit on a line with wide characters is highlighted on the character
  it found.** A match on a line that has just left the screen was drawn one cell
  to the right for every CJK character in front of it, and pressing Enter put the
  view on the wrong character too.

- **A bash pane keeps the startup files, the hooks and the arguments it was
  supposed to have.** A pane whose profile asked for a plain interactive shell
  now reads `~/.bashrc`, which is where bash's own documentation puts your
  aliases and functions; only a profile that asks for a login shell reads
  `/etc/profile` and `~/.bash_profile`. Anything else you wrote in the profile's
  arguments, such as `--noediting` or `-O globstar`, reaches the shell instead of
  being dropped the moment that profile had shell integration. If your
  `PROMPT_COMMAND` is a list, which is what bash 5.1 and later allow, your hooks
  run once per prompt rather than twice, and the command mark belongs to the
  command you typed, so the output of every command is decorated again. A DEBUG
  trap you had already installed keeps running, and it no longer costs your shell
  its positional parameters.
- **zsh gets shell integration, and `sh` stops pretending to.** A zsh pane, on
  Windows or inside WSL, is served through `ZDOTDIR`, so it draws command marks,
  reports where it is standing and carries exit codes the way a bash pane does,
  and your own `.zshenv`, `.zprofile` and `.zshrc` still run. A `sh` or `dash`
  profile is told it has no integration rather than being handed bash's flag,
  which it accepted and ignored, so the profile page says what that pane can
  really do.
- **A Command Prompt in a folder with a `#` or a space in its name is not
  forgotten.** `cmd` can only spell a directory one way, and the terminal read
  that spelling as a URI, so `D:\Code\C# Projects` was recorded as `D:\Code\C`
  and a folder with a `%` in it was dropped. The directory a pane reports is now
  read as the path it is.
- **A WSL pane sitting at `/` still knows where it is.** The root was refused as
  a directory, so the pane lost it and new tabs opened from that pane started
  somewhere else.
- **A PowerShell prompt of your own is told the truth.** A prompt this terminal
  wraps, whether it is oh-my-posh, conda's or one you wrote, now sees whether
  your last command succeeded instead of always seeing success, and a prompt that
  changes directory is reported from where it left the shell rather than one
  prompt behind. The line written into your `$PROFILE` survives a script path
  holding a `$` or a backtick.
- **A colour set right after a command mark is a colour again.** An escape
  sequence that interrupted an unfinished `OSC 133` or `OSC 7` lost its first two
  bytes, so the rest of it printed as text in the middle of the output.
- **The cursor lands where a program put it inside a scrolling region.** With
  origin mode on, moving the cursor to a column or up and down a line counted
  from the top of the region twice, so full-screen programs drew rows further
  down the screen than they meant to.
- **A mouse encoding this terminal cannot write is refused rather than
  promised.** A program that asked for UTF-8 mouse reporting was told it had it
  and then sent coordinates in the ordinary encoding, which it could not frame
  past column 95.
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
  the preview could navigate itself anywhere: a link, a redirect, or the
  page's own script would take the preview onto the network, carrying whatever the
  local document had reached. The preview now goes to the file it was opened
  with and nowhere else, and says so when a page tries.

- **A page can no longer send an address to your browser on its own.** A page
  that starts a download has that download cancelled, since Folio's preview writes
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
- **A program can no longer decide how much a pane remembers.** A program can
  ask a terminal to hold its output back until the picture is complete, and
  Folio kept every byte of that request so a window resize could replay it. The
  parser underneath gives up on such a request once it has held two megabytes,
  and Folio did not hear it give up: from then on every byte the program printed
  was kept forever, and every drag of the window edge replayed all of them. A
  request carrying more numbers than the parser accepts started the same
  keeping, and nothing could end it. Folio now follows the parser it reads
  through, and stops keeping bytes at the same point the parser does.

- **A sequence that never ends no longer grows without a limit.** A program sets
  the window title, a link target or a clipboard payload with a sequence that
  runs until it says it is finished, and one that never says so was held whole,
  in three places at once, for as long as the pane lived. Folio now holds at most
  64 KB of a sequence it does not read itself and drops the rest of it, so a
  title nobody ended costs a pane the same as a title.

- **A row of links costs one link.** A program can put one address behind a
  whole line of text, and Folio copied that address once per cell, on every
  repaint. A 48 KB address across 200 columns cost nine megabytes a frame; it now
  costs the address once, shared by every cell that wears it.

- **A long session stops collecting the shells it has already forgotten.** Every
  prompt a shell drew left behind a record of where its command started and
  ended, and those records were never released, not even when the lines they
  stood on had scrolled out of the transcript and been thrown away. A shell that
  redraws its prompt on every window resize could leave thousands of them, and
  each was walked again on the next scroll. Records now leave with the lines they
  describe, and the search for the command that is running looks at the newest
  one instead of reading all of them.

- **A cell holds one character, however many marks are sent for it.** Accents and
  other marks attach to the character before them, and a program sending ten
  thousand of them for one cell made Folio re-measure and re-copy the whole
  thing on every one, on the thread that draws the window. Folio now stops at 32,
  which is past anything Unicode itself allows a real cluster to hold.

- **A clipboard request Folio does not honour costs nothing.** A program can ask
  a terminal to put text on the clipboard, and Folio does not do that. It was
  still decoding the whole request first and then throwing the result away, so a
  megabyte asked for a megabyte of work. It is now refused before it is read.

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

- **Text a program prints can no longer send Folio to a file server, a named
  pipe, or a stopped WSL distribution.** A link a program printed, a path it
  printed, or a picture written into a Markdown file could name a place that is
  not on this machine, and resting the pointer on it was enough: Folio opened
  the card, went for the file, and Windows offered your account name and
  password to whatever answered. A picture on a share was fetched even while the
  card said it would not be shown; a video on one was opened by the media
  decoder; a link written `file://./pipe/name` reached a blocking read with no
  end, which stopped every later preview in that window; a folder shortcut
  pointing at a share was followed while merely listing the folder it sits in,
  and while painting a card. Folio now asks one question of every such path
  before it touches it, in every one of those places: it reads without being
  asked only a path on a drive of this machine, or the files of the WSL
  distribution the pane itself is standing in, and it reads what a shortcut
  points at before following it. A path that fails the question gets the card
  that says so, a picture that fails it draws the same placeholder a picture
  Folio cannot read draws, and a link that fails it is plain text. Opening a
  file you name yourself is unchanged, and so is everything about ordinary
  paths on this machine.

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
- **A picture cannot ask Folio for more memory than it has, by saying it is
  bigger than it is.** Resting the pointer on an animated `.gif` read the whole
  file however large it was, and decoded its frames with no limit on the size
  the file declared itself to be. Nine bytes of picture behind a header claiming
  a 65535 by 65535 screen had Folio ask the machine for seventeen gigabytes
  before deciding not to keep the result. Folio now reads at most the eight
  megabytes it will read of any picture, and refuses a declared size larger than
  it could ever draw before a single frame is made. Site icons and page
  thumbnails are held to the same rule, so a very large picture served as a
  favicon is refused rather than decoded and thrown away.
- **Pictures Folio has decoded no longer pile up for as long as the window is
  open.** Three stores held decoded pixels and nothing ever took anything out of
  them: the pictures behind hover cards and previews, the pictures printed in a
  pane, and the frames of animated files. Every distinct file the pointer had
  rested on stayed in memory until the window closed, so a session spent
  browsing a folder of screenshots kept all of them. Each store now has a size
  it will not grow past and lets go of whatever has gone longest unlooked at.
- **A rendered Markdown page reads the pictures near what you are looking at.**
  Opening a page asked the disk for every image in the whole document at once,
  so a README with hundreds of screenshots in it read hundreds of files to show
  you the first screenful. It now reads the ones on screen and several either
  side, and reads further ones as you scroll toward them.
- **Hovering a large file named `.pdf` no longer holds up the previews behind
  it.** Counting a document's pages read the file to its end however long it
  was, on the one worker every preview waits for, so one hover over a three
  hundred megabyte file stalled every card behind it. Folio now reads at most
  the same amount it will parse, and states the file's size alone when it
  cannot count within that.
- **A folder with a hundred thousand names costs what it shows.** The file
  column shows at most two thousand entries, and it read and sorted every name
  in the folder before cutting the list down. It now keeps only the two thousand
  it will draw, as it reads, and the rows it shows are the same rows as before.

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
