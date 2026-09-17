# Resize transaction review, round 2 — 2026-09-17

Target: `af2bf43c` (on `399b02f6`); product code read-only. Verdict: merge into 0.4.2. No new must-fix found; native-event coverage limits remain below.
References at target: `main.rs` = `crates/bt-app/src/main.rs`; `session.rs` = `crates/bt-term/src/session.rs`;
`lifecycle_matrix.rs` = `crates/bt-term/tests/lifecycle_matrix.rs`;
`platform.rs` = `crates/bt-platform/src/lib.rs`; dependency paths are relative to winit 0.30.13 `src/platform_impl/`.

- CLOSED, round-1 Windows P1: recovery precedes the held-hand read (`main.rs:85583`, `85591`),
  compares capture handles (`19056`), and cancels via the existing restoring door (`88994`, `89004`).
  `a_divider_drag_that_loses_its_pointer_stops_holding_the_resize` (`142129`) checks equality, ordering,
  release and quiescence, but not Runtime ratio restoration/native theft; those remain inspected.
- Windows attack: `GetCapture` is thread-scoped, not process-scoped; the wrapper preserves the exact
  HWND (`platform.rs:7241`, `140`). Another Folio window on this GUI thread returns Some(other);
  a capturing child/popup returns Some(child/popup), with no owner/root normalization. Another
  thread, even in our process, yields None here. Each differs from the original Some(window), so
  recovery cancels on the next flush. [Win32 contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getcapture).
  A context menu is not itself proof of a particular capture HWND: Folio uses `TrackPopupMenu`
  (`platform.rs:6221`); recovery observes actual capture during/after its nested loop. If capture
  is already back to the identical HWND before a sample, equality cannot remember that interruption.
- Healthy press: no normal None-to-Some race. `windows/event_loop.rs:1786` calls `capture_mouse`,
  which calls SetCapture (`980`), BEFORE sending Pressed (`1790`); Folio samples at `main.rs:93013`.
- Outside mouse-up while capture is ours still reaches us. Winit releases capture (`1802`), emits
  Released (`1806`), and its event runner dispatches it (`windows/event_loop/runner.rs:208`). Folio
  routes using the last pointer position (`main.rs:94324`) and takes the divider (`92555`) before
  the next turn's flush (`103604`): no cancellation/rollback of that normal completed drag.
  After a recovery cancellation, both later recovery and mouse-up find None (`89005`, `92555`).
- macOS None/None deliberately detects nothing (`portable_impl.rs:915`). Normal AppKit dragging
  ends in mouseUp, including ordinary outside release; winit forwards it (`macos/view.rs:588`,
  `1049`). Switching apps instead emits Focused(false) (`macos/window_delegate.rs:216`), which
  cancels (`main.rs:115224`). [AppKit drag sequence](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingMouseEvents/HandlingMouseEvents.html).
  If a release is truly swallowed WITHOUT blur, equality supplies no fallback; Esc or a structural
  reset is still needed. No native reproduction establishes that exceptional sequence here.
- CLOSED as documentation, still a platform limit: `docs/DESIGN.md:7755` explicitly permits two
  different child sizes during a paused macOS native live resize. It does not promise one per drag.
- CLOSED, harness accounting: `main.rs:141817` calls production `commit_leaf_resize` (`19120`),
  writes both grids exactly like `release_due_leaf_resize` (`19319`), counts told_the_child and
  reconciled separately, and uses timestamped reflow. Requests model the notification decision;
  pty=None means no actual PTY call is observed. Its local debt is discarded each tick (`141822`),
  so this harness cannot prove debt retention; the separate real-leaf debt fixture does (`140781`).
  Other users retain their claims: minimized silence (`141893`), genuinely tiny notification
  (`141935`), delayed/wandering widths (`141977`), held-hand coalescing (`142067`), immediate
  release (`142209`), unheld quiet boundary (`142230`), and typed-input reflow (`142264`).
  Their expected changed-grid requests cannot be hidden by the new same-grid settlement split.
- `a_repair_owed_before_a_wobble_is_still_owed_after_it_and_paid_once` (`main.rs:140781`): debt survives and yields
  exactly one repair, then None (passed). `a_pane_squeezed_narrow_and_let_go_leaves_its_shell_wide` (`141977`) passed: requests=[], settlements=1,
  false before its finish deadline, true at it, resize_transaction_open=false afterwards.
- PARTLY, entrance coverage: the five-entrance Shown/Behind sweep is still helper-only (`main.rs:140880`).
- PARTLY, viewport coverage: `session.rs:22481` now asserts offset 20 before/during/after, identical
  top text before/after, and cleared hold. It neither checks during-top text nor forces failed rematch.
  This named test lacks `resize` in its path and is excluded by the permitted bt-term unit filter.
- CLOSED, settlement cost gap: `lifecycle_matrix.rs:974` measures reconciliation plus quiescence,
  harvest and history scheduling; 3,971 resident entries, armed_before=0 in both measured fixtures.
  Prose: 2,244,719 B / 71 allocations / 1.688 ms / 0 armed; formulas: 3,261,024 B /
  8,025 allocations / 4.618 ms / 64 armed. Both pass heap limits and the 50 ms ceiling.
  These are this fixed workload's costs, not a history-independent latency bound; queue cap is 64.
- Validation: `cargo test -p bt-term --lib resize -j 4`: 28 passed, 450 filtered out.
- Validation: `cargo test -p bt-term --test lifecycle_matrix -j 4`: 43 passed; measurements exposed
  with RUST_TEST_NOCAPTURE=1. `cargo test -p bt-app --bin folio <filter> -j 4`: each of the three
  app test names above passed (1 each, 3,977 filtered each). No app/shell launched or process terminated.
