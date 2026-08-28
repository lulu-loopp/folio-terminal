# Route B slice ② — evidence run (2026-08-28)

Branch `worktree-agent-aed4f82cd0a9189dd`, base `bf0e468`. Binary under test:
`target\release\folio.exe` (release, built 07:36).

**How every run below was made.** Isolated `APPDATA` / `LOCALAPPDATA` under
`<scratchpad>\iso-vev`, `BT_PTY_DUMP` pointed at `<scratchpad>\iso-vev\pty.dump`,
the window placed with `SetWindowPos(..., SWP_NOACTIVATE)`, and no `folio` this
run did not start was ever touched. Driver: `<scratchpad>\vev.ps1`, which wraps
`scripts\dev\ui-probe.ps1` (per-monitor-v2 DPI, physical pixels, pixel-ownership
check before every shutter). Screen 2880×1800 physical; window 2560×1560 at
(40, 40).

**The subject is a burnt-in clock, not a coloured rectangle.** The shipped
fixtures in `test-assets` are three seconds of flat colour, which cannot tell a
playing video from a still of one in a screenshot. So the evidence fixtures are
`<scratchpad>\vidshow\clock.{mp4,mov,mkv}` — 480×270, **120 s**, H.264,
`ffmpeg -f lavfi -i testsrc` — whose picture carries ffmpeg's own second counter
and a scrolling gradient. A frame that differs from the frame before it is a
frame the decoder produced. These are evidence props and are **not** added to
`test-assets`: the shipped fixtures are what the automated matrix opens.

---

## ① Side preview pane — really playing, control bar up

`09-pane-bar-play-*.png` (10 frames, 700 ms apart), pointer resting on the bar.

| frame | bar reads |
|---|---|
| `09-pane-bar-play-01.png` | `⏸  0:01  ▁▂▃───────  2:00  🔊 ▁▁▁▁  1×` |
| `09-pane-bar-play-09.png` | `⏸  0:07  ▁▂▃▄▅──────  2:00  🔊 ▁▁▁▁  1×` |

Crops: `vshots\c9-01.png`, `vshots\c9-09.png`. Six seconds of wall clock, six
seconds on the readout, the scrubber moved with it, and the glyph is **pause**
— which is the bar saying the thing behind it is running, because §7.44 ② has
every cell ask the engine on the frame it is drawn.

The bar carries all seven controls at this width: play, elapsed, scrubber,
duration, mute, volume rail, rate. Native quads and sprites — there is no page.

**And it plays with nobody touching it.** `10-noinput-*.png`: play pressed by
`burst -ClickFirst`, then twelve captures over 8.8 s with **no pointer motion
and no keys at all**. Every consecutive pair differs (105 000 – 176 000 pixels
changed per 800 ms step, over the picture's own rect). A window that only
advanced a recording while a pointer was moving would show twelve identical
frames here.

Still-frame shot before play: `03-pane-still.png` — the pane's ▶ disc over the
1/10 frame, facts line `2:00 · 480 × 270 · 963 KB`.

---

## ② – ⑥ The second evidence run — the other two surfaces, the animation, and two more containers

Binary under test: `target\release\folio.exe`, **`Folio 0.1.0 (4481005873)`** — HEAD
`4481005`, which is the freeze mend plus the three defects this run itself found
(report §⑨ ⑩ ⑪). Driver: `<scratchpad>\ev3run.ps1`, and it is one process for
the whole of a card's life on purpose — the card dies the moment the pointer
wanders, and two probe invocations cost more than the card has. Same discipline
as run ①: isolated `APPDATA`/`LOCALAPPDATA` under `<scratchpad>\iso-ev3`,
`BT_PTY_DUMP` and `BT_MOUSE_TRACE` both named, `SetWindowPos(..., SWP_NOACTIVATE)`,
pixel ownership checked before **every** shutter, and no `folio` this run did not
start was ever touched.

Shots in `<scratchpad>\ev3shots`. **Not committed**: the window prints the
isolated profile's path, which carries the account name.

### ② The glance card — really playing, its own bar up

`21-card-still.png` → `22-card-play-00..07.png` (8 frames, ~800 ms apart, pointer
nudged one pixel per frame so the bar does not rest).

| what | reading |
|---|---|
| still, before the press | burnt-in counter **12** — the frame at one tenth of 120 s, which is what `read_video_glance` samples; facts line `2:00 · 480 × 270 / 963 KB` |
| `22-card-play-07.png` | burnt-in counter **6**, bar `⏸ 0:06 ▁▂ … 2:00 🔊 ▁▁▁▁` |
| picture rect, frame to frame | **13 087 – 38 259 px changed** of 130 536, every consecutive pair |

The picture and the readout agree, which is the whole point: the counter in the
recording and the clock on the bar are two independent witnesses and they say the
same second. The card sheds from the right at this width — no rate chip — which
is §7.44 ②'s shedding order, and the ▶ disc is gone because the seat is open.

**Three defects stood between this shot and the one before it**, and each was
found by taking it: the press opened a pane instead of playing (§⑨), the still
was painted over the recording (§⑩), and the picture was drawn under the card's
own well (§⑪).

### ③ The head dragged into a float — the engine goes with it, and the position does not go back

`23-card-before-carry.png` → `24-float-born.png` → `25-float-play-00..07.png`.

| what | reading |
|---|---|
| card's bar, last capture before the drag | **0:06** |
| float's bar, first captures after it | **0:09**, and counting |
| float picture rect, frame to frame | **16 441 – 35 215 px changed** of 203 816 |

A re-open would have started at zero. This is `VideoSeats::rehome` photographed:
one key changed, and the decoder never knew. The float carries the full bar —
`⏸ 0:09 ▁▂▃ 2:00 🔊 ▁▁▁▁ 1×` — all seven controls at that width, against the
card's five.

### ④ A closed float leaves nothing on the glass

`27-float-closed.png`, taken 3 s after the float's `✕`: the window is gone and
**no rectangle of `clock.mp4` is left painted where it stood**. That is `5d6488b`
(report §⑧) photographed on this build — the tick that empties the picture list
still hands it over — together with §7.44 ⑨'s sweep, which stopped the decoder on
the press rather than at the end of the exit animation.

### ⑤ A `.gif` moves by itself, with nobody touching anything

`26-gif-00..07.png` — `gifclock.gif` hovered, then **eight captures with no
pointer motion and no keys at all** (`Burst-Still`).

| capture | 00 | 02 | 04 | 06 |
|---|---|---|---|---|
| digit on the card | **4** | **6** | **8** | **3** |

Every consecutive pair differs (2 408 – 4 492 px over the picture rect), the
digits advance and wrap, and the whole of it happened with the mouse untouched —
a build that advanced one frame per redraw caused by input would show eight
identical captures here. Crops: `c26-00..07.png`, strip `c26-strip.png`.

The card wears **no control bar and no ▶**, which is §7.44 ⑤'s ruling drawn: a
recording is something somebody is watching, an animation is a picture that
moves.

### ⑥ `.mov` and `.mkv` — the two containers route A refused

Both opened from the files column and started on the card's own disc.

| file | bar | burnt-in counter | picture rect, frame to frame |
|---|---|---|---|
| `28-mov-play-04.png` (`clock.mov`, 963 KB) | `⏸ 0:03 … 2:00` | **3** | 13 087 – 38 287 px of 130 536 |
| `29-mkv-play-04.png` (`clock.mkv`, 948 KB) | `⏸ 0:03 … 2:00` | **3** | 13 004 – 38 134 px of 130 536 |

`CanPlayType` answers **No** to both (§7.42 ⑧), and Media Foundation plays both
on this machine with nothing installed for it — which is §7.44 ⑥'s whole
argument, now photographed on a surface rather than measured in a probe.
