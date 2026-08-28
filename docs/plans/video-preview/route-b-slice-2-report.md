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

The identical bytes are the whole window, not the video's rectangle. What that
rules out and what it does not is settled in ① below, which reproduced it.

---

## ① The freeze, reproduced and named

### Reproduced

`target\release\folio.exe` rebuilt from this tree, launched by
`<scratchpad>\fz.ps1` on `<scratchpad>\vidshow` (isolated `APPDATA` /
`LOCALAPPDATA` under `<scratchpad>\iso-fz`, `BT_PTY_DUMP` named,
`SetWindowPos(..., SWP_NOACTIVATE)`, pixel ownership checked before every
shutter). Files column docked, `clock.mp4` — 120 s of `testsrc` with ffmpeg's
own second counter burnt into the picture — opened in the side pane, ▶ pressed,
and then **fourteen captures 800 ms apart with the pointer never moved again**:

| capture | at | bytes | md5 |
|---|---|---|---|
| `04-freeze-00.png` | 47 ms | 107 214 | `89AF7447` |
| `04-freeze-01.png` | 831 ms | 307 287 | `06479D93` |
| `04-freeze-02.png` | 1 613 ms | 288 796 | `F0D363C9` |
| `04-freeze-03.png` | 2 403 ms | 293 158 | `C30481C6` |
| `04-freeze-04..13.png` | 3 212 – 10 469 ms | 293 158 | `C30481C6` |

**Eight seconds of byte-identical captures**, and the run of them starts between
1 613 ms and 2 403 ms after the press.

### The decoder was running the whole time

`c-13.png` is the picture out of the last frozen capture: the burnt-in counter
reads **1**. One `hovershot` later — the pointer nudged one pixel per frame and
nothing else touched — `c-n1.png` reads **39**. Thirty-eight seconds of
recording went past behind a picture that had not moved.

So this is not a decoder that stalled, and it is not a window thread that hung:
no `hang-reports` directory was ever created under either run's isolated
`%APPDATA%\Folio`, which is the watchdog saying the pump answered every time it
was asked. The loop was turning, the engine was decoding, and **the glass was
being starved of presents**.

`container-probe` says the same from the other side: `clock.mov`, `clock.mkv`
and `clock.mp4` each deliver **150 frames in 5.0 s with a longest gap of 49 ms**,
`playing=true`, no error.

### Root cause — a tick that owes a picture is thrown away at the chrome gate

`Runtime::advance_strip_animation` (`crates/bt-app/src/main.rs`) collects this
tick's decoded pictures and then decides twice whether the frame is owed:

```rust
let frames_arrived = self.window.video.pump(now) | self.advance_animations(now);
let boxes_moved = anything_moving && self.refresh_video_layers();
let owes_frame = owes_frame || frames_arrived || boxes_moved;
if !owes_frame && !panes_owe {
    return Ok(());
}
…
if !self.refresh_chrome() && !panes_owe {   // ← the picture's debt is not asked about
    return Ok(());
}
self.publish_chrome_frame(now)
```

The second gate does not carry `owes_frame`. A video is **not in the chrome** —
it is a layer list handed to `bt_render::WindowRenderer::set_video_layers` and
drawn from renderer state at present time — so `refresh_chrome()` cannot ever
answer `true` because a new picture arrived. A tick whose only news is a decoded
frame is therefore always dropped one line before the present that would have
shown it.

**Why it looks like a first-open freeze.** While the control bar is up, its
elapsed clock and its scrubber move, so the *chrome* changes on almost every
tick and the picture reaches the glass as a passenger on the bar's own debt. The
bar is born up when a seat opens (§7.44 ②) and rests `VIDEO_BAR_IDLE_REST +
VIDEO_BAR_FADE` = **2 000 ms + 90 ms** after the last act. That is the number in
the table above: moving at 1 613 ms, frozen at 2 403 ms. A reader who presses
play and takes their hand off the mouse sees two seconds of video and then a
photograph — and the picture jumps forward the instant they move the pointer,
which is what makes it read as "the window froze and then recovered".

It is not first-open at all: it is **every** video, and every `.gif` too, since
`advance_animations` reports its debt through the same `frames_arrived`.

### The mend

**① The rule the tick decides by is now three debts, not two.**
`tick_owes_a_present(chrome_changed, panes_owe, pictures_owe)` in
`crates/bt-app/src/main.rs`, and the chrome gate is written through it. The
picture's debt is given a name of its own — `pictures_owe = frames_arrived ||
boxes_moved` — precisely so that it survives as far as that line instead of
being folded into `owes_frame` and forgotten. It is general: it covers a video
on any of the three surfaces, a `.gif` advancing by its own delays, and any
future thing the renderer draws from state the chrome knows nothing about.

Pinned by `a_video_frame_alone_is_enough_to_present`, in two halves — the rule's
truth table, and a source reading that the call site actually asks it. RED GATE
①: return `chrome_changed || panes_owe` and the first half fails. RED GATE ②:
write the gate back as `!self.refresh_chrome() && !panes_owe` and the second
fails, which is the state the binary that froze was built from.

**② And while the window thread was being looked at: `Engine::open` was
waiting on it.**

Not the freeze — it is a hundred milliseconds, not eight seconds — but it is the
same law broken in the same place. `Engine::open` is what a press of ▶ calls, on
the window's own thread, and it waited for the far side to run `MFStartup`,
create a Direct3D device, create an `IMFMediaEngine` and call `SetSource` before
it returned. Measured with `container-probe` on this machine:

| open | cost |
|---|---|
| first video of the process | **112 ms** |
| every one after | **36 – 45 ms** |

with `OPEN_BUDGET` = 5 s as the worst case behind it.

It now waits for nothing at all: `open` allocates a channel, starts the thread
and returns. `media_session` moved onto that thread with the rest of it. An
engine that fails to be built writes its `EngineError` into the shared
`EngineState`, which is the slot `VideoSeat::fault` already reads and
`Runtime::sweep_video_seats` already sweeps every tick — so the sentence a
surface prints for an unplayable file arrives by the path it already had, one
tick later than it used to.

**The deadline did not go away, it moved.** `OPEN_BUDGET` is now spent in
`Engine::state`: past five seconds with nothing built and nothing said, the
answer is `EngineError::Unresponsive`. Nobody blocks on it, and an engine that
never comes up still ends as a sentence rather than as a rectangle that stays
black for ever.

Pinned by `opening_a_video_never_blocks_the_window_thread`: eight opens in a
row, all eight inside 60 ms. Eight rather than one because a blocking open pays
its forty milliseconds every time, warm or cold. **Measured under its own
mutation** — the wait put back, bounded by `OPEN_BUDGET` exactly as before:

```
thread 'video::engine::tests::opening_a_video_never_blocks_the_window_thread' panicked at
crates\bt-platform\src\video\engine.rs:1422:9:
8 opens took 468.6124ms, which is a window thread waiting for a decoder
```

Two tests had to be told that an engine is built after the open returns rather
than before it: `a_software_adapter_still_serves_frames` asks
`adapter_in_use()` once the metadata is in, and
`every_engine_is_shut_down_before_the_process_leaves` waits for the ledger to
come up before asserting that it goes back down — with the third arm now
carrying the count out of the `catch_unwind` and asserting outside it, since a
panic beating the engine thread would have made that arm pass with nothing
standing.

---

## ② A second defect, found while photographing the first

**A floating window drew a play disc that lit under the pointer and did nothing
when it was pressed, and a control bar no hand could reach.**

Found taking the float's evidence shot: `04-float-play-*.png` and
`04b-float-play-*.png` — eight captures each, `212421` bytes and md5 `ACDDE96B`
for every one of the sixteen, across a click on the disc, a click to focus and
then a click on the disc, and a double click on the picture. The disc *lit*, so
the pointer was inside the box the mark was drawn from.

`BT_MOUSE_TRACE` says where the press went:

```
mouse_input state=Pressed  button=Left pointer=2112,649 route=none
mouse_input state=Released button=Left pointer=2112,649 route=none
chrome_mouse_input taken=0 at=release-target-is-some target=None
```

The *release* reached `chrome_mouse_input`; the **press never did**. `press_float`
is asked above the chrome router — that is what makes a floating window opaque to
the layout beneath it — and its `Body` arm went straight to the document ladder.
`press_video_at`, which states the whole order (a play mark on a picture, then
every control on a bar that is up, then a double click on the picture itself),
is reached only from `chrome_mouse_input`.

The asymmetry is why it survived a reading: the release half was never missing,
because a scrubber let go *is* answered in `chrome_mouse_input` and a release
does reach it.

The mend is one line of dispatch and not a second model — the same shape
`a_document_in_a_float_selects_the_way_it_does_in_a_pane` took one slice
earlier: `press_float`'s body branch asks `press_video_at` first, so a player
standing on a document out-ranks the document. Pinned by
`a_player_in_a_float_answers_the_hand_the_way_a_pane_does`, which asserts both
hosts reach the path and that the player is asked before the page underneath it.

---

## ③ The closed-float seat — a window that has left is not playing anything

**The seat sweep had one door it could not ask "does this surface still exist"
in the ordinary way, and a recording went on playing through the whole of its
window's departure.**

`Runtime::sweep_video_seats` is the single rule for all three surfaces: a seat
lives while the surface it is keyed by is still showing the file it was opened
on. It reads `preview_surfaces()` for "what still exists", and
`preview_surfaces()` builds its float half from `float::FloatHost::drawn()`.

`drawn()` deliberately keeps a window that is on its way out — it has to, because
the window is still being painted while it fades, and retiring its *view*
underneath it would blank the very thing the exit animation is animating. So a
closed float stayed in the alive list for the whole of its exit, and the sweep
had nothing to notice.

**A decoder is not a view.** The picture went on decoding and went on being
drawn, with the chassis already gone from over it, for as long as the departure
took — and the sound with it.

The mend is the float's own arm in that `match`, asking
`float::FloatHost::live()` — which is `drawn()` minus the dismissed — instead of
the drawn list, and standing *before* the fallthrough that reads `alive`, since
a `match` takes the first arm that fits:

```rust
PreviewSurface::Float(id) if self.window.float.live(*id).is_none() => {
    return true;
}
```

Every other way a video ends without the stop button was already covered by the
one rule and needed nothing: a pane handed another document, a card whose
pointer moved to the next row, a tab closed or torn away (the seats vanish from
`preview_seats()`), and a **window** closed outright (`WindowRuntime` drops,
`VideoSeats::Drop` runs `shutdown_all`). `sweep_video_seats` is called from
`advance_strip_animation`, which runs on the clock, so none of those doors has
to remember that a decoder exists.

### Red proof

Pinned by `a_closed_window_stops_the_recording_it_was_showing`, in two halves —
(1) the gap between the two lists is real (a dismissed window is still `drawn()`
and no longer `live()`, so the arm is necessary rather than redundant), and (2)
the sweep asks the live list and asks it first.

Both this and section 2 above were **measured under one mutation**: the float arm
deleted from `sweep_video_seats` and `press_video_at` deleted from
`press_float`.

```
running 2 tests
test files_locate_door_tests::a_closed_window_stops_the_recording_it_was_showing ... FAILED
test files_locate_door_tests::a_player_in_a_float_answers_the_hand_the_way_a_pane_does ... FAILED

---- files_locate_door_tests::a_closed_window_stops_the_recording_it_was_showing stdout ----
panicked at crates\bt-app\src\main.rs:82000:14:
the seat sweep has no arm for a closed window, so a recording goes on playing for the length of the window's exit

---- files_locate_door_tests::a_player_in_a_float_answers_the_hand_the_way_a_pane_does stdout ----
panicked at crates\bt-app\src\main.rs:81911:13:
    fn press_float( does not reach the player's press path, so a recording starts on one host and not the other

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 2804 filtered out
```

Restored, both green.

### One tidy taken on the way past

`sweep_video_seats` had picked up a doc paragraph belonging to
`sweep_preview_panes` — *"Drop the view of every surface that has stopped
existing…"*, which is about the preview pool's dirty gates and not about
decoders. Moved back to the function it describes.

### A trap in gate 3 worth writing down

`cargo fmt --all -- --check` on this tree did not report a diff — it **died**:

```
memory allocation of 53115959672 bytes failed
```

53 GB, from rustfmt 1.8.0-stable, exit 9. It is not a defect in this branch's
code and not a machine problem. `cargo fmt --all` (apply, no `--check`) runs to
completion in seconds; `--check` on the *formatted* file is clean and instant.
The blow-up is in rustfmt's own `--check` diff path, and it needs a pending
reformat inside a file the size of `main.rs` to reach it — the base commit
`bf0e468` does not hit it because `main.rs` was already canonical there.

**So: run `cargo fmt --all` before `cargo fmt --all -- --check`, always.** A hand
that reads the 53 GB line as "the build is broken" will lose an hour, which is
why it is written here.
