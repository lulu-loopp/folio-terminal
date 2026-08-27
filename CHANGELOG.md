# Changelog

All notable changes to Folio are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.1.0-preview (unreleased)

The first public build. Everything below is new, so it is grouped by what part of
the window it belongs to rather than by added and changed. The last section is for
things that were wrong on the way here and are worth knowing were wrong.

### Terminal

- Panes split horizontally (`Alt+Shift+-`) and vertically (`Alt+Shift+=`), tabs
  carry them, and one process holds many windows.
- A tab or a single pane can be dragged out into a window of its own, dropped into
  another window, or sent to one from a menu. A dragged tab keeps a ghost under
  the pointer and wears a badge when it is over another window.
- Dropping a pane somewhere else is a move rather than a close and a re-open: the
  panes it did not touch keep their widths to the pixel, and putting a pane back
  where it came from changes nothing at all.
- Cards (`Ctrl+Shift+Z`) lays the tab's panes out at readable size, each one laid
  out by the same rules as the window it came from, so a fixed-width column takes
  its space on a card exactly as it does on the window. A card scrolls under the
  wheel with `Alt` held.
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
- Clicking a file path or a URL printed in the terminal opens it here; holding
  `Ctrl` hands it to the machine's own browser or application instead.

### Files and preview

- A files column beside the terminal follows the pane's directory, and keeps a
  watch on the root and on every unfolded folder, so a file a command has just
  written appears without a refresh. Folding a folder gives the handle back.
- Files and folders can be pinned, renamed on disk, revealed in Explorer, opened
  in their default application, or opened as a new pane rooted there.
- Hovering a file name shows a card: the first page of a PDF, which the wheel can
  wind through page by page; the first lines of a text file; the first frame of a
  video; an image; or the file's own format and size when nothing can read it.
- Markdown is typeset — headings, tables, code, and mathematics written as `$…$`,
  `\(…\)`, `\[…\]` or as one of the bare `amsmath` environments.
- A local HTML file can be read as a page or flipped to its source, and flipping
  costs no reload.
- Web pages open in a pane of their own, with an address field (`Ctrl+L`), the
  site's own icon, a source view for local pages, and a place in the session so
  they come back when Folio does.
- Video files — `.mp4`, `.m4v`, `.webm` — show their first frame, their length and
  their size, and play in the pane. A `.mov` gets the same first frame and says
  why it has no play button.
- The files column turns into a git panel (`Ctrl+Shift+G`): branch, working tree,
  staged and unstaged files, and the commit graph, with a selected file's diff in
  the preview.

### Agents and notifications

- A pane can say it is waiting for you, and the window says which one. Programs
  that write the terminal's own attention sequences are heard without anything
  being installed.
- `Ctrl+Shift+A` jumps to the pane that has been waiting longest.
- The Agents page in Settings installs one notification hook each into Claude
  Code's, Codex's and GitHub Copilot CLI's own configuration files, and takes it
  back out again — the file is written whole or not written, and a dated copy of
  what was there is kept first.
- A window that is genuinely out of reach — minimised, hidden, on another desktop
  — raises a real Windows notification carrying the program's own words. There is
  a separate switch for the notification a turn's end raises.

### Settings

- One dialog: font, colour scheme, cursor, scrollback, line wrapping, background
  opacity, minimum contrast, what the preview renders, shell profiles, and every
  shortcut key.
- English and Chinese, switched without restarting. Every row name and every
  sentence is written for the person reading it rather than for the source.
- The Shortcuts page records a chord as you press it, says when the chord is
  already taken and offers to take it off the row that has it, and writes
  `keybindings.json`.
- Profiles: the five shipped shells can be overridden field by field and restored,
  and profiles of your own carry their own command line, environment and colours.
- Colour schemes are files in `%APPDATA%\Folio\schemes`.
- Animation follows the system's "reduce motion" setting, and notices the moment
  that setting changes rather than at the next start.

### Fixed

- **A files column stayed empty in every window but the first.** Answers from the
  background workers were addressed by a number each window counted for itself, so
  the window that opened earliest took everybody's answers off the queue and threw
  away the ones that did not match its own. Every address now carries the window
  it belongs to.
- **The web preview filed away what you typed into a page.** General autofill and
  password saving are now switched off explicitly rather than left at the engine's
  defaults.
- **A page could be rasterised for the wrong display.** The window tells a hosted
  page which display it is on instead of leaving the engine to guess late.
- **A hovered video restarted the decoder every time.** The media session now
  lives as long as the process: a first frame costs about ten milliseconds warm,
  where it used to cost hundreds.
- **Diagnostics could land in somebody else's pane.** The console borrowed at
  startup to answer a command line is given back before the run begins, and a
  run's own output goes to `%APPDATA%\Folio\diagnostics.log`.
- **A control nobody could see could still be clicked.** A hidden run of buttons
  on a pane head does not take a press until the pointer has revealed it.
- **An emptied environment variable was read as a filename.** `BT_PTY_DUMP=` and
  every switch like it now read set-but-empty as off.
- **A menu on its way out could still be pressed.** A leaving popup is a fading
  picture with nothing left to click, and switching tabs leaves none standing.
- **The window could stop answering with nothing to show for it.** A watchdog
  writes a report naming modules and offsets when the window thread misses a
  deadline it declared, and an indefinite park is not counted as a hang.
