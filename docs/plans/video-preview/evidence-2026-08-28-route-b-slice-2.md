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
