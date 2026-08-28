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

---

## ④ The gates, run for the first time on this branch — eight reds

`cargo test --workspace` had not been run on this line before this hand took it
over (`NOTES.md`'s checklist had that box open). It found **eight** failures.
None of them was flaky and none was environmental; every one was a real
disagreement between what the slice did and what its own tests said.

| red | what it was |
|---|---|
| `the_environment_is_created_with_no_browser_arguments_at_all` (`bt-platform`) | The pin reads the whole of `webview.rs` looking for the retired autoplay switch — and the **comment explaining that the switch is retired** quoted it verbatim. Spelled in pieces now, the way every needle in that family already is. |
| `a_video_has_a_face_of_its_own` | Its "outside the class" list still held `.mkv` and `.avi`, which §7.44 ⑥ moved *into* the class. They are in the video block now, `.wmv` beside them, and the two rows left outside are `.mpg`/`.flv` — outside for the honest reason that nobody has opened one. |
| `only_a_playable_video_is_offered_a_play_button` | Pinned the retired **second column** ("a face and no player"), and had been half-renamed into a test that asserted `!path_names_a_video(capture.mov)` and `path_names_a_video(capture.mov)` two lines apart. **Retired**, with a tombstone naming its replacement, `every_name_in_the_class_plays_and_the_class_is_the_seven_that_were_opened`. |
| `the_modal_family_covers_the_float_and_the_tip_covers_them_both` | The new `video_bars` band was added to the constructed stack with marker `23` — the marker `card_hint` already carries, which is the collision the comment three lines below it warns about — and was never added to the expected order. Marker `24`, and the band is in the order and in the prose. |
| `the_still_and_the_first_played_frame_share_a_rect` | **A real defect, and the one this test was written for.** See ⑤ below. |
| `the_shell_page_is_gone` | Two false positives that the retirement itself created. See ⑥ below. |
| `a_video_is_one_seat_on_three_surfaces` | `engines_outstanding()` read `0` where it wanted `3`. Since ⑫ made `Engine::open` return without waiting, the ledger is bumped on the engine's own thread — so "three surfaces are three decoders" becomes true a moment *after* the third open returns. `bt-platform` had already grown `ledger_gate` + `engines_settling_to` for this; `bt-app` is a different test binary and needed its own pair. Both tests in here that conclude anything from the process-wide counter now take the gate. |
| `a_file_that_is_not_an_animation_says_so_rather_than_pretending` | Expected `NotAnAnimation` for `GIF89a but not really`. The bytes **announce** the container this module opens and then have nothing openable in them, which is `Undecodable`; answering "not an animation" would be telling a reader their `.gif` is not a `.gif` when what is true is that it is a broken one. The test was wrong and the product was right. |

## ⑤ Half a pixel between the still and the first played frame

`the_still_and_the_first_played_frame_share_a_rect` failed on every
odd-leftover row:

```
[0.0, 0.0, 960.0, 556.0] [160, 120]: the still is [109.5, 0.0, 850.5, 556.0]
                                    and the frame is [109.0, 0.0, 850.0, 556.0]
  left: 110.0
 right: 109.0
```

A 160×120 recording in a 960×556 body fits to 741 wide; the leftover is 219.
`bt_render::video_frame_rect` splits it by an **integer floor** — 109, and its
doc says so: *"a one-pixel letterbox is a pixel of ground on one side and
nothing on the other"*. `video_still_destination` centred on the body's
floating-point midpoint instead — 480 − 741/2 = 109.5.

The two were never one rule; they were two statements of one rule, and they
disagreed by exactly the half pixel a reader sees as a flicker when they press
play. The mend is that there is now only one statement:

```rust
fn video_still_destination(body: [f32; 4], video_px: [u32; 2]) -> [f32; 4] {
    viewport_of_rect(body)
        .and_then(|box_| bt_render::video_frame_rect(box_, video_px[0], video_px[1]))
        .unwrap_or(body)
}
```

The doc above it had already claimed *"one of them is the other's caller"*. It
is true now.

`a_videos_still_lands_where_the_playing_picture_does` went red on the change and
was right to: it asserted the still's centre is exactly the body's. It now
compares all four edges against `video_frame_rect` **to the pixel** — the
load-bearing half — and allows the centre the half pixel the floor costs, with
the reason written beside it.

## ⑥ A retirement pin that could not survive being written about

`the_shell_page_is_gone` reads every `.rs` in the crate for five needles. Two of
them now had false positives, and both were created by the retirement itself:

* **`<video`** matched `Option<video_seat::BarLayout>` — a Rust type naming the
  module that *replaced* the page.
* **`VideoShell`** and **`Folio\player`** matched the prose in `preview.rs`,
  `webhost.rs` and this test's own doc comment, which is where the record of why
  route A is gone actually lives.

A pin that forbids the explanation forbids the only account of the decision. So
the rule is now asked of the **code**: comment lines are dropped before the
search — which is what `bt-render`'s source pins already do, and for this reason
— and the element is looked for **shaped like a tag** (`<video` followed by `>`,
whitespace or `/`) rather than as four characters. What is left cannot be
written except by writing the element, and it still covers the whole class.

## The three gates

Run on `worktree-agent-aed4f82cd0a9189dd` after the eight above were mended:

```
cargo test --workspace --no-fail-fast -j 4     CARGO_EXIT=0
    4423 passed; 0 failed; 22 ignored
cargo clippy --workspace --all-targets -j 4 -- -D warnings    CARGO_EXIT=0
cargo fmt --all -- --check                                     CARGO_EXIT=0
```

`PSModulePath` was **not** cleared, per the standing ruling.

---

## ⑦ The red證 table — six gates, measured under one mutation round

Every gate §7.44 ⑩ names for `bt-app` was **run against a build that breaks the
thing it pins**, all six mutations applied together so that each red is its own
and none is a side effect of another. Restored afterwards; the tree is byte-clean
against the commit and the suite is green again.

| gate | the mutation | what it said |
|---|---|---|
| `the_shell_page_is_gone` | `const _ROUTE_A_SHELL: &str = "<video controls></video>";` put back as code | `an opening video element is back in …\crates\bt-app\src\main.rs` |
| `the_still_and_the_first_played_frame_share_a_rect` | the still fitted by `image_destination(…, ImageZoom::FIT)` — the picture channel's rule, which never enlarges | `[0.0, 0.0, 960.0, 556.0] [160, 120]: the still is [400.0, 218.0, 560.0, 338.0] and the frame is [109.0, 0.0, 850.0, 556.0]` — the `next12` defect exactly: a 160×120 clip drawn at its own size in a full-height pane |
| `a_video_is_one_seat_on_three_surfaces` | the texture key derived from the path instead of the serial | `three surfaces are three textures: {"video:…\folio-video-test.mp4"}` — left 1, right 3 |
| `a_card_torn_off_carries_its_engine_with_it` | `VideoSeats::rehome` closes and re-opens instead of moving the seat | `the first engine was shut down` — left 4, right 3 |
| `the_bar_rises_on_hover_and_rests_by_the_registers_numbers` | `VIDEO_BAR_REVEAL_INTENT` typed as 300ms, which is what a player "feels like" | `a player's bar and a ⌄'s menu are the same promise, made twice` — left `300ms`, right `250ms` |
| `a_gif_advances_by_its_own_frame_delays` | `frame_at` slices the loop into four equal parts instead of reading the file's delays | `at 100ms` — left 0, right 1 |

```
test result: FAILED. 0 passed; 6 failed; 0 ignored; 0 measured; 2799 filtered out
```

The two gates this hand added were measured the same way in ③ above, and
§7.44 ⑫'s `opening_a_video_never_blocks_the_window_thread` in ① (`8 opens took
468.6124ms, which is a window thread waiting for a decoder`).

---

## ⑧ A ninth red, found by the camera and not by the suite

**A closed window's last frame stayed on the glass.** Found while taking the
float's evidence shot: `07-card-00..03.png` — the floating window had been shut
1.2 s earlier, its head, its bar and its chassis were gone, and a rectangle of
`clock.mp4` was still painted where the window had been.

It is **not** the ⑨ decoder defect coming back. The decoder had already stopped:
the four captures are 700 ms apart and the crop over that rectangle
(`crops/c-orphan-00..03.png`) is `a551baa3` in all four — a photograph, not a
video. What was left was the renderer still drawing a layer list nobody had
taken back.

`Runtime::advance_strip_animation`:

```rust
self.sweep_video_seats();
…
let anything_moving = !self.window.video.is_empty() || !self.window.animations.is_empty();
let boxes_moved = anything_moving && self.refresh_video_layers();
```

The sweep is on the line above the gate, and its whole job is to *remove* seats.
So the tick on which the last seat goes is exactly the tick on which
`self.window.video` is empty, `anything_moving` is false, and
`refresh_video_layers` — the only thing that ever hands
`WindowRenderer::set_video_layers` the shorter list — is skipped. The renderer
goes on drawing what it was last given, for as long as nothing else happens.

What cleared it three and a half seconds later was dismissing the hover card:
`refresh_preview_for_layout` asks unconditionally.

**This is the freeze's own mistake, one line further down** — a gate that decides
there is nothing to say by asking about the thing that has just stopped
existing. The mend keeps the gate's real purpose, which is that an idle window
rebuilds nothing, and adds the clause it was missing:

```rust
let anything_moving = !self.window.video.is_empty()
    || !self.window.animations.is_empty()
    || !self.window.renderer.video_layers().is_empty();
```

The real idle case is *nothing is moving **and** the renderer is holding
nothing*, and asking costs one `is_empty()` on a slice.

Pinned by `the_tick_that_empties_the_picture_list_still_hands_it_over`, which
asserts the clause is in the condition **and** that the sweep stands above the
gate (a sweep below it would be a different defect, lasting one tick).

```
---- tests::the_tick_that_empties_the_picture_list_still_hands_it_over stdout ----
panicked at crates\bt-app\src\main.rs:120764:9:
the gate decides there is nothing to hand over without asking the renderer what it is
still holding, so the tick that removes the last seat never tells it:
let anything_moving = !self.window.video.is_empty()
            || !self.window.animations.is_empty()
```

---

## ⑨ A tenth red, found by the camera again — and it is ⑧'s defect on the third host

**A glance card drew the pane's own play disc, lit it under the pointer, and
opened a preview pane when it was pressed.**

Found on the first shutter of the evidence run this hand was sent to take: hover
`clock.mp4` in the files column, press the ▶ on the card, and eight captures
700 ms apart show no card at all — a **docked preview pane** with the still and
the disc still on it. `BT_MOUSE_TRACE`, verbatim:

```
40047.991 mouse_input state=Pressed button=Left pointer=1704,524 route=none
40048.074 open_preview_image enter path=…\vidshow\clock.mp4
40056.475 preview_landing_surface seat=SeatId(2) reused=0
40056.541 open_preview_image leave=opened surface=Seat(LeafId { tab: TabId(1), seat: SeatId(2) })
40118.381 mouse_input state=Released button=Left pointer=1704,524 route=none
```

The press was on the disc — `1704,524` is its measured centre, cut out of
`03-card-03.png` at full resolution — and the next station is a pane opening.
No `press-video`, no `play_video_on`, no seat.

### Why this is not a new defect but ② one host over

`press_file_peek` is asked **above** the chrome router, at
`crates/bt-app/src/main.rs` in the window's own press ladder ("the glance card
takes its own presses… above the float and below the menus"), for the same
reason `press_float` is: a card is opaque to the layout beneath it. And
`press_video_at` — the one statement of the whole order — is reached from
`chrome_mouse_input`. So the card's face arm, `file_peek::Press::Open`, went
straight to `press_file_peek_door`, and §7.44 ⑧'s ruling (*「转它就是播它，而播它
是本构建做的事」*) was defeated by a route: the `PreviewSurface::Peek` arm inside
`press_video_at` is **unreachable by hand**, because the only points that satisfy
it are points inside a card, and every press inside a card is claimed one level
higher.

②'s mend named a host rather than the rule behind it, which is why the third
surface still could not start a recording. The rule, written down in §7.44 ①
now: **every press road that stands above the chrome router and claims a whole
region owes `press_video_at` one question.**

### The mend, and where it stands

In the **face** arm and not at the top of the function — the same placement
`press_float` gives the player in its `Body` arm. The card's own furniture is
still the card's: the head that is a handle (§7.29) and the scroll thumb answer
first, and only on the face does a player out-rank what is under it.

```rust
file_peek::Press::Open => {
    if self.press_video_at(position)? {
        return Ok(true);
    }
    if self.press_preview_block_thumb(position)? {
        return Ok(true);
    }
    self.press_file_peek_door()
}
```

### Red proof

Pinned by `a_player_on_a_glance_card_answers_the_hand_the_way_a_pane_does`, in
two halves — ① the card's press road reaches the player's path at all, ② inside
the face arm the player stands before the door. Measured under its own mutation
(the two lines above deleted, which is the state every build before today was
in):

```
running 1 test
test files_locate_door_tests::a_player_on_a_glance_card_answers_the_hand_the_way_a_pane_does ... FAILED

thread '…' panicked at crates\bt-app\src\main.rs:82022:9:
the card's press does not reach the player's press path, so its own play disc
opens the pane it was drawn to make unnecessary

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 2806 filtered out
```

Restored, and both host pins green:

```
test files_locate_door_tests::a_player_on_a_glance_card_answers_the_hand_the_way_a_pane_does ... ok
test files_locate_door_tests::a_player_in_a_float_answers_the_hand_the_way_a_pane_does ... ok
```

Recorded in `docs/DESIGN.md` §7.44 ① (second addendum) and added to ⑩'s gate
list.

---

## ⑩ An eleventh red, on the very next shutter — the card's still stood over its own recording

With ⑨ mended and the release rebuilt, the card's ▶ starts the engine. The
**bar** proves it: `⏸ 0:01 … 2:00 🔇 ▁▁▁▁` at the first capture and `⏸ 0:06` at
the eighth, five seconds later, and every cell on that bar is read off the
engine on the frame it is drawn (§7.44 ②). The **picture** did not move at all:

| burst | picture rect, frame to frame |
|---|---|
| `22-card-play-00..07.png` (8 frames, 700 ms apart) | **0 pixels changed**, every consecutive pair, of 130 536 |

and the picture it was stuck on is the *still*: `clock.mp4`'s burnt-in counter
reads `12`, which is the frame at one tenth of 120 s — exactly what
`read_video_glance` samples. One gesture later the same engine, carried into a
float, read `9` and moved 17 000 – 36 000 pixels per 800 ms step.

### Root cause — a clause that knows about one of the two ways a picture moves

A card's still does **not** go down the picture channel that
`refit_preview_picture` refuses on. It is handed to `file_peek::layout` and drawn
in the card's own **icon** channel, which runs *after* the video lane — so a
still left in place is painted over the frames, and the one on top is the one
that does not move. `file_peek_card_layers` has the clause that withdraws it,
and its own comment states the mechanism correctly. The predicate was half:

```rust
let running = self.animation_running_on(PreviewSurface::Peek);
let picture = match layout.body_kind {
    file_peek::PeekBody::Facts { .. } => None,
    _ if running => None,
    …
```

Everywhere else in this window the pair is asked together —
`refit_preview_picture`'s refusal is `surface_is_playing_a_video(surface) ||
animation_running_on(surface)` — because *is the picture in this box moving* is
**one question with two ways of being true**. A clause that knows about one of
them is a clause that will be wrong about the other every time, and §7.44 ⑤ had
already written the consequence down in advance: *「留着的那一张会被画在动的那一张
上面,而盖在上面的那张是不动的那张」*.

### The mend

```rust
let moving = self.surface_is_playing_a_video(PreviewSurface::Peek)
    || self.animation_running_on(PreviewSurface::Peek);
```

### Red proof

Pinned by `a_card_withdraws_its_still_for_a_recording_the_way_it_does_for_an_animation`
— both halves are in the clause, and the clause is what gates the `match` (a
value computed and dropped would pass a naïve pin and change nothing on the
glass). Measured under its own mutation, the recording half dropped:

```
---- files_locate_door_tests::a_card_withdraws_its_still_for_a_recording_the_way_it_does_for_an_animation stdout ----
panicked at crates\bt-app\src\main.rs:82096:13:
the card's still is withdrawn without asking self.surface_is_playing_a_video(PreviewSurface::Peek),
so it is painted over the moving picture it is one frame of

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 2807 filtered out
```

Restored, green. Recorded in `docs/DESIGN.md` §7.44 ⑤ and added to ⑩'s gate list.

---

## ⑪ A twelfth red, uncovered by the eleventh — the card's picture was drawn under the card's own well

⑩'s mend took the still away, and the shutter after it showed what the still had
been hiding: the card's picture box is **empty ground**, the bar underneath it
counting `0:06` off an engine that is decoding. The recording is not late and it
is not missing — it is being drawn **underneath the card's own picture well**.

| burst | card picture rect, frame to frame |
|---|---|
| `22-card-play-00..07.png`, ⑩'s mend in | still 0 pixels changed — but now of an empty well, not of a still |
| `25-float-play-00..07.png`, same engine carried over | 17 505 – 35 306 px changed per 800 ms step |

### Root cause — and the float had this exact defect a slice ago

`bt_render::VideoStage::Overlay(n)` is drawn between layer `n`'s **ground** and
layer `n`'s **fills** — the renderer's z-order loop, and the variant's own doc
says so. That placement is right for a `WebHole`, because a hole belongs under
the marks that legitimately stand over a page, and **wrong for a picture**,
because a host's body well is one of those fills.

The float was photographed with this on 2026-08-28 and mended by pushing an
**empty layer** directly above each window's face and pointing
`float_video_level` at it — the comment beside that push spells the whole
argument out, ending *"a stage index has to name a layer the z-order loop
actually reaches, and what this one is for is being reached."*

The card was given `below_the_file_peek()` — the index of **its own face**,
whose fills include the rounded picture well. So the same defect, on the third
host, written into the code on the same day the second host's was mended. The
still standing over the top is what kept it invisible for two runs.

### The mend

`file_peek_layer` now takes `below` the way `float_layer` does, writes its own
index — only the group knows where inside itself the slot went — and pushes the
empty layer:

```rust
self.window.file_peek_level = Some(below + layers.len());
layers.push(marks::OverlayLayer::default());
```

with the play disc and the control bar extended in **above** it, for the float's
reason exactly: a layer paints its quads before the picture it carries.

### Red proof — the pin is the rule, not a host

`a_recording_on_an_overlay_host_is_drawn_into_a_layer_of_its_own`: every ledger
of *which layer is a recording drawn into* is written immediately before a push
of an empty layer. Both arms measured under their own mutation — each ledger
pointed back at its host's face, which is the state each photographed build was
in:

```
---- a_recording_on_an_overlay_host_is_drawn_into_a_layer_of_its_own ----
    fn file_peek_layer( names a recording's layer and pushes none, so the stage is
    the host's own face and the body well is painted over the picture

    fn float_layer( names a recording's layer and pushes none, so the stage is
    the host's own face and the body well is painted over the picture
```

Restored, green. This is the third time in this slice that a rule was mended on
one host and left standing on another — the press road (§⑨), the still clause
(§⑩) and now the stage index — so all three pins are written as *rules over the
set of hosts* rather than as statements about one.
