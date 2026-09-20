# Which GPU Folio asks for is a choice now, and the log says which one

Step one of two — the request is switchable and reportable on the owner's
hybrid-graphics laptop. **The default is unchanged.**

- `bt_render::gpu_power_request()` is the one owner of "which power preference
  Folio asks for". It reads `BT_GPU_PREFERENCE` once per process (`OnceLock`),
  `low`/`high` in either case, set-but-empty read as unset like every other
  switch, anything else left at the default and carried as `NotUnderstood` so
  it is reported rather than dropped.
- All three `request_adapter` sites read it: `bootstrap_for_surface`,
  `rebuild_after_device_loss` (A3) and `headless_on` — the headless one too,
  because a fourth literal would be a second place deciding (B2), and it changes
  nothing for tests, which either force WARP (`demand_fallback`) or take
  whatever the machine gives, exactly as they do with the name unset.
- `note_gpu_adapter` appends `asked=<preference> (<source>)` to the existing
  `GPU adapter` line, so one recording carries both the request and the answer
  (A2) and an unrecognised value is named there (A1) — once per adapter note,
  which is start-up and each device rebuild. The parse is held to a table with
  the environment passed in (A4). `docs/BT-ENVIRONMENT.md` gains the row its
  gate test requires, CHANGELOG one bullet that promises no fix.

## A6 — who else picks an adapter

`bt_platform::Compositor::mint_ground_surface` calls `D3D11CreateDevice` with
**no adapter named**, so Windows picks the default one for the ground/skirt
surface; it will not follow `BT_GPU_PREFERENCE`, so under `=low` it and the
terminal's swapchain can sit on different adapters — which DirectComposition
allows and which is worth watching in the comparison. The video engine
(`bt_platform::video::engine`) takes its own D3D11 device the same way (default
hardware adapter, WARP fallback). WebView2 is a separate process with its own
policy; `docs/plans/port/probe-x1` keeps its own literal and is in no build.

## Unverified

**Nothing was compiled**: another agent was building `bt-render` and the owner
was on the machine, so only `cargo fmt` ran — tests, clippy and macOS/Linux
compilation are for CI. Whether `LowPower` returns the integrated adapter there
(H2), and whether the stalls follow it (H1), the owner's two recordings answer.
