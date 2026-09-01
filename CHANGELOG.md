# Changelog

All notable changes to Folio are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

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
