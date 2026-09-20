# The next intermittent window stall names the call that held it

## Roles and trees

- Project owner: repository owner. Coordinator: Claude session. Implementer: Codex.
  Reviewer: CI and a different model.
- Baseline: `main` at `92dc5f31`; candidate: `fix/stall-self-report` in `D:/Developer/bt-wt/stall-report`.

## Scope and evidence authority

- A1-A8: instrumentation only; no pacing, coalescing, detection, UI, or repair
  policy changed. The private recordings were neither read nor copied here.
- `hang_watch` remains the only owner of elapsed window-thread time. Renderer
  callbacks name boundaries but carry no durations; the present trace's new
  field is derived from timestamps it already owned.

## Findings and boundaries

- A1 · static: startup and every successful device rebuild read the in-use
  adapter, then `diagnostics::note` writes `diagnostics.log` and offers the same
  bounded console sink used by stall lines.
- A2 · static: redraw separates CPU compose/command encoding,
  `Surface::get_current_texture`, `Queue::submit`, `Queue::present`, and the two
  DirectComposition calls after present.
- A3 · static: the existing `WindowEvent` kind stations remain the enclosing
  names; winit `set_ime_cursor_area` and the Windows system-caret update are
  independent child stations.
- A4 · static: drain separates ring reads, `DualPlaneSession::feed_at`, terminal
  reply dispatch, `end_feed_turn`, outcome handling, visible-artifact detection,
  trace-sink offer, and the publication decision.
- A5-A7 · synthetic: every slow line appends session age and a one-based ordinal;
  stations sort largest first. Synthetic-millisecond tests prove that redraw
  children plus parent equal the hold exactly, with no sleep.
- A8 · static: the Unreleased changelog has one Changed bullet.

## Accounting rule and budget

- Nested time is exclusive: opening a child closes the parent's interval and
  leaving it reopens the parent. A printed hold therefore charges each
  millisecond once.
- A rendered on-screen redraw adds 7 phase-transition clock reads and 4 reads
  around the two compositor calls: 11, plus 2 per visible terminal pane whose
  artifact-detection pass runs. A gate-skipped frame pays no renderer reads.
- A window event adds 0 reads unless it applies an IME cursor area; that path
  adds 4. The event-kind station existed at baseline.
- For one drain, let L be PTY-leaf visits, E nonempty visits, T tabs, and A be 1
  when active output reaches publication logic: added reads are
  `4L + 4E + 2T + 2 + 4A`, plus normal frame reads if it publishes now.
- No per-turn path adds allocation, formatting, I/O, a timer, or a wake-up; a
  quiescent window adds zero work.

## Synthetic output

- Stall: `Folio: the window thread held control for 1529 ms on turn 42 — swapchain present 1507 ms, redraw compose/encode 22 ms · faults +0, working set 412 → 412 MB · session age 8404.000s, stall #31`
- Adapter: `Folio: GPU adapter name="Synthetic RTX" vendor=0x10de device=0x1234 type=DiscreteGpu backend=Dx12 driver="Synthetic" driver_info="1.2.3"`

## Limits and validation

- Compose and encoding are non-contiguous pieces of one renderer pass and share
  one station. `DualPlaneSession::feed_at` is one public bt-term call; splitting
  its internals would cross the ticket's crate boundary. `BT_PTY_DUMP` writes on
  the PTY reader/publisher threads, not the window drain, so no false station was
  added for it.
- Passed: `cargo fmt --package bt-app --package bt-render`;
  `cargo test -p bt-app --bin folio hang_watch -j 4` (41 passed);
  `cargo clippy -p bt-app -p bt-render --all-targets --locked -j 4 -- -D warnings`.
- Unverified: readability and causal capture on the owner's real hybrid-GPU
  machine during the next day-long traced run; macOS/Linux compilation remains
  for CI.
