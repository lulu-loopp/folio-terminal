# Changelog

All notable changes to Folio are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Changed

- **The summoned terminal lives and dies with Folio.** The notification-area
  icon is gone, along with the `Keep an icon on the taskbar` row and the
  residency it decided: closing the last window you can see ends the run, and a
  summoned terminal hidden behind its key goes with it. It loses nothing by
  going — what comes back next launch is the new **What comes back** row's
  answer. Existing `settings.json` files have `tray_icon` taken out of them
  the first time Folio reads them.
- **One `×` on the summoned terminal, and it means hide.** The window keeps the
  gear and one `×`; the minimise and maximise buttons are gone. Pressing the `×`
  is the summon key pressed again, down to the bit it sets — the window goes
  away, the keyboard goes back to whatever it came down over, and the tabs,
  shells and scrollback are all still there at the next press. Minimise used to
  put the window away without going through that door, which left the next press
  of the key doing nothing at all. End the session with `exit` in the shell, or
  by closing the tab.
- **A settings page of its own.** Everything about the summoned terminal is now
  under **Settings > Summoned terminal**: the summon key (recorded in place),
  which profile it opens on, a command to run on the first summon of each run,
  its height, width and the gap above it, whether it hides when it loses focus,
  and what comes back. The gear on the summoned terminal's own title bar opens
  that page; every other window's gear opens General as before. The four rows
  that used to stand on General have moved here.
- **The summon's rectangle is decided in one place.** Which display it comes
  down on, how big it is there, and whether you have arranged it on that display
  by hand are one question with one answer, whichever door asked for the summon.

### Added

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
- **Profile and Gap above it (Settings > Summoned terminal).** Which shell a new
  tab in the summoned terminal starts — the default profile unless you say
  otherwise — and how far below the top of the screen the window hangs, which
  was a fixed twelve pixels until now.

### Fixed

- **A file path an application wrapped over several indented rows is one link
  again.** When an agent prints a block of indented text holding a single path
  and breaks it at the window's width, the rows are read back as the one file
  they spell between them — as many as eight rows, where only two were ever put
  together before, and at the block's own indent, which used to be read as a
  column of separate lines. The whole path underlines and opens as one. Rows
  that really are a column of separate lines are untouched: a listing whose rows
  are each a file of their own, and any row with other text in front of the
  path, are left exactly as they were.

## 0.1.1-preview (unreleased)

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

## 0.1.0-preview (unreleased)

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
