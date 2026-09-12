# X-1 — Metal alpha and composition, against the locked wgpu

*2026-09-12. Ticket X-1 of `docs/plans/port/macos-plan-2026-09-12.md`. Branch
`probe/macos-x1`. Run on the Mac mini in `~/folio-port/wt/x1` at `origin/main`
`d3d9453` under the rules in `docs/plans/port/mac-mini-venue.md`. The probe crate
is committed beside this file under `docs/plans/port/probe-x1/`.*

## What was built

A scratch binary crate with its own `[workspace]` table, so nothing here enters
Folio's own dependency graph. It opens an undecorated winit window on the
display whose backing scale is 2 — this machine has two, and the main one is
scale 1 — and puts two subviews into winit's content view: a `WKWebView` loading
a local page, and above it a plain `NSView` that wgpu is handed as its surface
target. Later subviews are in front, so the web view's layer is composited
*under* the Metal layer and a transparent pixel in the frame is a pixel of the
page. That is the macOS shape of the Windows arrangement, and it needs no visual
tree of our own.

The frame is opaque dark blue except for three rectangles, each an alpha-0
interior inside an edge one physical pixel wide at alpha 0.5, drawn from
`@builtin(position)` so the edge is one *device* pixel at scale 2. **A**'s edge
is straight-alpha red, `rgb(255,0,0)` at `a=0.5`, which is what `PostMultiplied`
says to write; **B**'s is the same colour premultiplied by hand, `rgb(128,0,0)`;
**C**'s interior is alpha 0 but coloured blue. Two opaque calibration patches
read back exactly, so the sRGB encoding round-trips and every number below is
arithmetic rather than guesswork.

## What wgpu reported

```
surface alpha_modes reported: [Opaque, PostMultiplied]
  PreMultiplied offered: false
surface formats: [Bgra8UnormSrgb, Bgra8Unorm, Rgba16Float, Rgb10a2Unorm]
adapter: Apple M4 (Metal, IntegratedGpu)
```

The plan's reading of the cached source is confirmed on the machine.
`wgpu-hal-30.0.0/src/metal/adapter.rs:468` lists
`composite_alpha_modes: vec![Opaque, PostMultiplied]` and nothing else;
`surface.rs:231` acts on exactly those two —
`Opaque => render_layer.setOpaque(true)`,
`PostMultiplied => render_layer.setOpaque(false)`, `_ => ()`. So
`required_alpha_mode(CompositionVisual)`'s `PreMultiplied` can never be
satisfied and `choose_alpha_mode` would refuse the surface. The layer wgpu
attaches is a `RawWindowMetalLayer` sublayer from `raw-window-metal` 1.1.0,
reported `opaque false, contentsScale 2`, tracking the host view.

## What the window server actually did

`screencapture` is unavailable here (see below), so the composited pixels were
read back from inside the probe with `CGWindowListCreateImage` over its own
window. The images came back 1800×1200 and then 1360×1500 — the drawable size,
ratio 1.000 — so the readback is pixel-for-pixel with the frame. The page's
`#00ff00` band arrives through the display path as `(33,252,36)`: that is the
backdrop every number below is composited over.

| Sample | initial | resized | rebuilt | rebuilt, layer owned |
|---|---|---|---|---|
| opaque frame | 14,25,55 | 14,25,55 | 14,25,55 | 14,25,55 |
| A interior, `a=0` | 33,252,36 | 33,252,36 | 33,252,36 | 33,252,36 |
| A edge, straight | **255,126,18** | 255,126,18 | **255,63,9** | 255,126,18 |
| pixel beside A's edge | 33,252,36 | 33,252,36 | 33,252,36 | 33,252,36 |
| B edge, premultiplied | **144,126,18** | 144,126,18 | **200,63,9** | 144,126,18 |
| C interior, `a=0`, blue | 33,252,255 | 33,252,255 | 33,252,255 | 33,252,255 |

In words: a dark blue field with three openings cut in it, the page's green
showing through the first two and a cyan-shifted green through the third, each
outlined by a one-pixel hairline — the pixel beside the edge is already pure
page. Three readings, each exact to the byte.

**CoreAnimation composites this layer premultiplied, whatever the mode is
called.** A's measured `(255,126,18)` is `min(255, 255 + 0.5·33)`, `0.5·252`,
`0.5·36` — premultiplied arithmetic. Straight-alpha arithmetic gives
`(144,126,18)`, which is what B, the hand-premultiplied edge, measured. C
settles it: an alpha-0 pixel carrying blue is not invisible, it adds its blue
unattenuated. `PostMultiplied` names a non-opaque layer here, not a compositing
rule. **And the blend is on encoded bytes, not linear light:** every prediction
above is sRGB-byte arithmetic and lands exactly, where linear light would put
A's red channel near 188.

**Dropping a `wgpu::Surface` does not take its layer away.** After the
device-loss stand-in the host view had *two* `RawWindowMetalLayer` sublayers and
the frame was composited twice: B's `(200,63,9)` is `128 + 0.5·144.5`,
`0.25·252`, `0.25·36` — two stacked half-alpha edges, to the byte. Removing the
stale layers before rebuilding restored the initial numbers exactly.

## The four checks

- **Hole — PASS**, in all four captures, at scale 2, with full alpha in the
  composited window.
- **Edge alpha — FAIL as specified, PASS with an explicit premultiply.** A frame
  written the way the offered mode is named composites too bright by the
  backdrop's own contribution; premultiplied, it is correct to the byte.
- **Resize — PASS.** 1800×1200 → 1360×1500: the sublayer follows the view,
  `contentsScale` stays 2, every sample unchanged.
- **Reconstruction — FAIL as performed, PASS when the layer is owned.** The
  naive drop-and-recreate doubles the composite; one `setSublayers(nil)` before
  the rebuild makes it identical to the first frame.

**X-1 therefore fails on §3's wording**, and into exactly the two items §3
named: *alpha representation* and *surface ownership* are now explicit design
work before M1-4 is scoped. Nothing here says the model cannot be built. It was
built, and the web pane needs no composition visual and no second swapchain —
two sibling views in one window is the whole arrangement.

## What it forces

1. `required_alpha_mode` cannot stay a two-arm function, and
   `SurfaceAlphaReport::is_premultiplied` — today the one place where
   "premultiplied means the ground may be translucent" is written down — has to
   answer for a mode whose name says the opposite. Recommend the report carry
   the *representation* separately from the wgpu mode, and that the renderer
   premultiply on the sRGB bytes it writes rather than on linear values.
2. Surface ownership on macOS is a layer's lifetime, not a swapchain's.
   `FrameTarget::Surrendered` exists on Windows because a visual holds one
   swapchain at a time; on Metal nothing enforces that, so the platform side
   must hold and remove the layer itself. That belongs with M1-1 and M1-4.

## Cost and housekeeping

**Versions, as resolved:** wgpu, wgpu-core, wgpu-hal, wgpu-types 30.0.0 (`std`,
`metal`, `wgsl`; the repository asks for `dx12`, the one deliberate difference);
naga 30.0.1; winit 0.30.13 (`rwh_06`); raw-window-handle 0.6.2;
raw-window-metal 1.1.0; objc2 0.6.4 with objc2-foundation, objc2-app-kit,
objc2-quartz-core and objc2-web-kit 0.3.2; pollster 0.4.0; png 0.18.1. winit
brings its *own* objc2 0.5.2 and objc2-app-kit 0.2.2; both generations ran in
one process, the probe reaching winit's view by raw pointer.

**Build:** cold, into a throwaway target directory since deleted, 37.9 s wall,
190.6 s user, peak resident 1.70 GB, 1.1 GB of products; warm rebuild of the
probe alone, 1.3 s. The repository's `.cargo/config.toml` was in force —
`cargo build -v` shows `crt-static` on the rustc line — and the binary linked
and ran, the half X-6 could not establish from Windows.

**Processes started, and how each ended:** a throwaway bundle testing whether
`open` reaches the logged-in session (pid 43557, wrote its log and exited;
bundle deleted); three runs of the probe (pids 44523 and 45054 each finished
their timeline and exited; pid 44694 died in an intermediate version that
removed sublayers while iterating the live array — the probe's own defect, fixed
by one `setSublayers(nil)`, no system report written); and the cargo launchers
under `nohup`, each exited. Every pid came from the probe's own first log line;
nothing was matched by name or title, and nothing outside `~/folio-port` was
written.

**Screen Recording / TCC — the owner has something to grant.** `screencapture
-x` fails with `could not create image from display`, both from an ssh session
and from a helper launched into the logged-in session with `open`; the
user-level TCC store has no Screen Recording entry, and no prompt appeared that
an agent could see, let alone click. The fallback used is
`CGWindowListCreateImage` over the probe's *own* window, which macOS allows
without a grant and which is the better instrument anyway — it returns the
drawable's pixels, not a scaled screen region. A later ticket needing
whole-screen capture will need that grant.
