# Resize transaction review — 2026-09-17

Target: `399b02f6` (base `1b71dbcd`); product code remains read-only.
Verdict: merge with must-fixes before 0.4.2; the reported wobble is repaired, but
the no-blur divider capture-loss path still fails the universal closure bar.
Scope: transaction closure, child notifications, PSReadLine debt, canonical output,
artifact budgets, viewport stability, and regression-test coverage.
References are at 399b02f6: `main.rs` = `crates/bt-app/src/main.rs`;
`session.rs`/`adapter.rs` = `crates/bt-term/src/`; `portable_impl.rs` = `crates/bt-platform/src/`;
`lifecycle_matrix.rs` = `crates/bt-term/tests/`; dependency paths are relative to winit 0.30.13.

- Coverage finding: the entrance sweep drives `schedule_leaf_grid_change` on a standalone
  leaf for five labelled grid sequences and Shown/Behind (`main.rs:140754`, `140793`).
  It does not dispatch native events or construct an unfocused pane in a Runtime.
  Actual unfocused routing is established by inspection (`main.rs:85373-85396`).
- Silence/accounting: `main.rs:19114-19126` gates both `pty.resize` and debt replacement
  on `next != conpty`; unchanged settlement preserves prior debt. Consumption uses
  `mem::take` (`18939`), after session quiescence (`85228-85250`): at most one chord,
  conditional on the PowerShell opt-in and an input region still being open.
- Canonical output: `adapter.rs:894-901` feeds both parsers every terminal byte and
  discards duplicate replies; `1046-1068` seeds partial/synchronized parser state.
  Reconcile preserves displayed replies and swaps parser + terminal together (`1133-1171`).
  Output arriving during the gesture does not by itself make the fork stale.
- P1 / remaining closure gap (pre-existing): divider capture loss without top-level blur
  or a delivered mouse-up leaves `divider_drag` set (`main.rs:92494`, `115159-115161`).
  `flush_pending_pty_resize` treats that as a held hand (`85547`), and service refuses both
  release and a wake (`18882-18883`); subsequent ordinary resizes cannot settle the pane.
  The native capture query exists, but its guard is attached only to the cross-window
  broker (`113380`), not divider dragging. Add divider capture-cancel recovery and a
  no-blur/lost-up regression before claiming every gesture closes. Esc/blur can recover;
  this is a source-level counterexample, not a reproduced native capture-steal sequence.
- Platform limit (pre-existing): `portable_impl.rs:360-361` always returns false for
  `in_size_move`; native macOS live-resize/fullscreen has no end-of-gesture gate here.
  A pause beyond 200 ms can release B, then continued motion releases C: two distinct
  child-size notifications. This is not duplicate same-size notification, but the
  cross-platform claim of one notification per physical drag is not established.
  Windows native edge tracking clears on `WM_EXITSIZEMOVE` (`bt-platform/src/lib.rs:5042-5048`);
  on macOS the quiet timer can still settle after the resize/fullscreen stream stops.
- Lifecycle inspection: release and finish walk every tab/leaf (`main.rs:85566`, `85221`);
  all non-retiring windows still turn while hidden/minimized (`114262-114272`). Minimize
  ignores icon geometry (`99908`) and quake hide preserves the runtime (`39497-39499`).
  Queued settlement survives tab activation/Behind, subject to the held-hand gap above.
  Tab transfer moves the whole TabState (`111546`, `111586`), including each leaf's
  pending release/session/debt, and rebinds output wake (`111566-111567`). Restart replaces
  the entire leaf (`76267-76286`); it does not reuse the old epoch (`35434-35435`).
- Two solves before release overwrite one queue; after a successful release the child
  grid advances (`main.rs:19280-19281`). A later identical solve cannot tell it again;
  a genuinely different grid owes a second notification. Split/close re-solves use the
  same leaf queue (`66275`), and structural changes clear the divider flag (`54504`).
  A clean same-grid restore queues nothing (`18798`, `141281`).
- Cost/coverage: settlement calls `schedule_existing_artifacts` (`session.rs:3623`),
  which walks all resident history entries (`11228-11242`); queue cap 64 bounds queued
  work, not that walk. The 200-frame benchmark measures only `resize` (`lifecycle_matrix.rs:774`),
  not end-of-wobble reconciliation/harvest/scanning. It cannot certify long-history
  settlement latency; add a separate settlement budget before claiming that bound.
- Review position: settlement reseats registered anchors (`session.rs:3587`, `5404`);
  projection resolves those anchors and retains anchored state (`bt-viewport/src/lib.rs:2821-2847`).
  Closing the epoch clears resize hold on refresh (`session.rs:8426`, viewport `2880`).
  Existing resize/reprint tests pin offset 20 and explicit takeover at bottom (`22421`,
  `22443`); no new wobble-specific scrolled-review test proves zero jump if rematch fails.
- Red-test limitation: the five named new tests contain neither `resize` nor `psreadline`
  in their test paths, so the permitted filtered unit-suite commands do not select them.
  They are absent on main; the new settlement API also prevents verbatim application there.
  Four temporary Rust helper tests passed: exact extracted main coalescer fails the
  release assertion (expected panic), fixed coalescer releases once, a stale held-hand
  bit blocks after an hour, and preserved debt consumes once. Geometry/profile types
  were minimal stand-ins; this is helper-level red/green, not native/session validation.
  No native application or child shell was launched; scratch tests were deleted.
- Validation: `cargo test -p bt-term --lib resize -j 4`: 28 passed, 449 filtered out.
- Validation: `cargo test -p bt-term --test lifecycle_matrix -j 4`: 42 passed.
  Nocapture measurements, 200 frames: sparse 128,089,299 B / 107,372 allocations,
  full 35,604,923 B / 35,073 allocations; shrink medians 474.9 / 144.9 microseconds,
  paired ratio 3.02. Per-frame bounds: sparse 737,280 B / 600 allocations,
  full 221,184 B / 192 allocations; median ceilings 3 / 1 ms, ratio ceiling 6. All pass.
- Dependency cross-check: locked winit 0.30.13 handles `WM_CAPTURECHANGED` only by
  zeroing its capture count (`platform_impl/windows/event_loop.rs:1927-1935`); it emits
  no release/cancel event. Its macOS `windowDidEndLiveResize` only resets resize
  increments (`platform_impl/macos/window_delegate.rs:178-181`), with no app boundary.
- Debt test gap: the new app wobble test consumes the earlier real debt before starting
  its wobble (`main.rs:140637-140652`); preservation across a still-unpaid debt is supported
  by branch inspection/helper testing, not that new integrated test.
- Public-session scratch witness passed on primary and alternate: 9 -> 8 -> 9 columns,
  output during both resize legs, and a CSI split across transaction start. Without
  settlement an hour cannot close it; unchanged settlement closes at its deadline,
  preserves visible text/cursor against an unresized control, and queues zero PTY bytes.
  Linked the existing bt-term library; no product edits. Scratch source/binary deleted.
- Validation: `cargo test -p bt-app --bin folio resize -j 4`: 30 passed, 3,946 filtered out.
- Validation: `cargo test -p bt-app --bin folio psreadline -j 4`: 26 passed, 3,950 filtered out.
