# Linux trash design review

Coordinator review of `docs/plans/design/linux-trash.md`, 2026-10-03.

The App-owned transaction lane is accepted. `HandoffLane`'s window-specific
abandonment and result type do not carry the scheme settings duty.

The following conditions apply before implementation:

- Use the existing handoff admission capacity where its semantics fit. Do not
  introduce a separate quota without a concrete need.
- Keep settings, catalogue and message changes on their current owner. File-row
  completion follows the addressed leaf after a window transfer.
- Delay the final quit transition while accepted trash transactions remain.
  Continue the event loop and settle their results before the final settings and
  session save. No window thread joins or receives from the worker.
- Preserve the scheme watcher ordering described in the design. A local success
  changes the matching stored selection before the deferred rescan.
- The worker uses the shared child supervisor for cancellation and reaping.
  Report command failure through the existing deletion error surface.

The last-window condition preserves the old synchronous operation's completion
ordering while moving the process work off the window thread. It does not add a
prompt or change non-Linux deletion.

Acceptance remains the design's controlled transaction tests and execution-lane
checks. Runtime validation has not yet been run.
