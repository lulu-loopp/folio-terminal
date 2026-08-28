# Route B slice ② — running report

Branch `worktree-agent-aed4f82cd0a9189dd`, base `bf0e468`. This file is written
as the work happens and committed at each step, because the line it belongs to
has already been cut twice by a dropped connection and a report that exists only
in a transcript is a report that does not exist.

## ⓪ What the two earlier hands left

`git status` on taking over: **clean**. Three commits stand on `bf0e468`:

| commit | what it is |
|---|---|
| `06a3560` | wip — the seat, the native bar, route A retired. 24 files, +4397 / −1763. |
| `9a9e7ba` | wip — the bar was forgotten on the tick that granted it; `has_finished_leaving` and its pin. |
| `21cdd3f` | evidence run ① — the side pane really plays, photographed against a burnt-in clock. |

Their working notes are `<scratchpad>\NOTES.md` (engine API, render API, the
app-side inventory of every call site route A owned, the measured playable
matrix, and a checklist with two boxes open: *release screenshots* and *the
three gates*). The evidence driver is `<scratchpad>\vev.ps1` — isolated
`APPDATA`/`LOCALAPPDATA` under `<scratchpad>\iso-vev`, `BT_PTY_DUMP` at
`<scratchpad>\iso-vev\pty.dump`, `SetWindowPos(..., SWP_NOACTIVATE)` placement,
pixel-ownership check before every shutter, and it never touches a `folio` it
did not start.

### The defect the second hand found and could not name

Written down here from the photographs it left in `<scratchpad>\vshots`, since
the transcript that saw it is gone.

**The steps that reproduce it.** Launch `target\release\folio.exe` cold (a
process that has not yet opened a video), open the files column, dock a preview pane,
and press ▶ on a recording for the **first time in that process**. The window
stops answering — it does not repaint, and the picture that is on the glass
stays exactly as it was — and then comes back on its own a second or two later
and plays normally.

**What the photographs show.** A burst is a series of full-window captures
several hundred milliseconds apart, so two consecutive captures of a window that
is repainting are never byte-identical. In these bursts they are:

| burst | run of byte-identical captures |
|---|---|
| `04-pane-playing-03..07.png` | 5 captures, all 292 397 bytes |
| `05-pane-bar-00..04.png` | 5 captures, all 292 397 bytes |
| `08-pane-play-00..11.png` | **12 captures, all 314 397 bytes** |
| `11-mov-03..07.png` | 5 captures, all 292 989 bytes |
| `12-mkv-03..07.png` | 5 captures, all 292 370 bytes |

and `13-after-freeze-00/01.png` differ again — the window recovered by itself.
The runs where nothing froze (`09-pane-bar-play-*`, `10-noinput-*`) are the ones
taken **after** a video had already been opened in that process.

The identical bytes are the whole window, not the video's rectangle, so this is
not "the decoder stopped": it is the window thread not repainting at all.
</content>
</invoke>
