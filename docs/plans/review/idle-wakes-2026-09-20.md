# An idle Folio window keeps waking — implementation report, 2026-09-20

Baseline: `484157e4` (`main`, descendant of `0b8eb7c8`); branch `fix/an-idle-window-sleeps`.
Scope and method: static source audit first; no Folio window, input, clipboard, process stop, or overlapping Cargo process.

## Today's deadline fold — independent account

`Runtime::turn` owns the per-window answer; `FolioApp::about_to_wait_inner` takes the minimum over application and window answers.
Classification below is before comparison with the supplied hypotheses: A = absolute state-owned instant/None; R = recomputed from the query's `now` with unchanged owner state.

- 01 startup poll — `first_text_presented` / `startup_poll_delay`: **R** (`now + 10 ms`).
- 02 IME cursor — `ImeCursorThrottle`: A; 03 shell caret — `CursorBlink`: A; 04 tab press — `TabPress`: A.
- 05 rename caret — `CursorBlink`: A; 06 strip animation — `Runtime::strip_animation_work`, `PaneMotion`, video bar: **R**.
- 07 web teardown — `WebSeat`: A; 08 PTY resize — `PendingPtyResize`: A; 09 resize finish — session: A.
- 10 synchronized update — session: A; 11 live stability — row damage epochs: A; 12 PTY coalesce — `Pending`: A.
- 13 attention credential — leaf ledger: A; 14 tooltip — `TooltipHost` plus shared frame clamp: **R**.
- 15 key hint — `KeyHintHost` plus shared frame clamp: **R**; 16 Cards hint — `CardHint` plus frame clamp: **R**.
- 17 toast — `ToastHost` plus frame clamp: **R**; 18 command flash — flash epoch plus frame clamp: **R**.
- 19 command rails — rail tweens plus frame clamp: **R**; 20 terminal thumbs — rest epoch plus `now + frame`: **R**.
- 21 layout peek — its clock: A; 22 file peek — `PeekClock` plus frame clamp: **R**.
- 23 file-peek close grace — `closing_at` plus frame clamp: **R**; 24 file-peek dwell — dwell clock plus frame clamp: **R**.
- 25 float — `FloatHost` plus `now + frame`: **R**; 26 revealed foot — acknowledgement epoch: A.
- 27 web zoom acknowledgement — `WebSeat`: A; 28 web dialog acknowledgement — `WebSeat`: A.
- 29 preview save notice — preview epoch: A; 30 preview refusal — refusal epoch: A.
- 31 chevrons — three `ChevronGate`s: A; 32 pane menu — hover/safety clocks: A.
- 33 terminal menu — hover/safety clocks: A; 34 tab menu — hover/safety clocks: A.
- 35 drag spring — `SpringAim::Waiting.since`: A; 36 drag auto-scroll — last tick or query `now`: **R**.
- 37 hyperlink hover — stored `show_at`: A; 38 peek hover — stored `show_at`: A.
- 39 formula tools — journey epochs plus shared frame clamp: **R**; 40 formula toggle — landing plus frame clamp: **R**.
- 41 formula-copy acknowledgement — copy epoch, spent values filtered: A; 42 preview resample — stored settle clocks: A.
- 43 session save — `SessionStore`/`Debouncer`: **R** (`Instant::now() + 1.5 s`).
- 44 schemes watch — watcher debounce epoch: A; 45 storage watch — watcher debounce epoch: A.
- 46 git watch — watcher debounce epoch: A; 47 preview watch — watcher debounce epoch: A.
- 48 files watch — watcher debounce epoch: A; 49 refused frame — `FrameClock` plus shared `max(now)`: **R**.

The common source for most R entries is `FrameClock::next_frame(last_present, now)`: once `last_present + interval` is past, a second query returns the second `now`.
The strip has two additional renewals: `strip_animation_work` uses `now + interval`, and `PaneMotion::deadline` does likewise.

## A1 evidence and hypothesis comparison

No existing headless/session harness constructs `ActiveEventLoop` or drives `about_to_wait`; existing tests drive clocks or inspect source.
Because launching Folio is forbidden, today's Windows idle turns/s and winning deadline are **unmeasured**; the candidate will carry the requested one-line `BT_PERF_TRACE` probe.
PTY silence and no-input conditions therefore cannot be established on this machine; the coordinator must measure them in the candidate GUI run.
H1 is statically confirmed as a defect class, not yet dynamically confirmed as today's idle winner; the old `strip_animation_deadline` symbol is gone but renewing strip/frame deadlines remain.
H2 is partly confirmed: `refresh_main_menu` derives an allocating plan and enters `bt_platform::menu::refresh` every turn before the platform memo; `Heartbeat::open_hold` samples process memory every wake.
H3 is confirmed as a renewing deadline and a save-liveness defect; it is a waker whenever the store is dirty, but is not proven to explain a clean idle shell.
Real Windows and macOS turn rates, the real winner, and native menu behavior remain unverified; the coordinator measures both platforms after this branch builds.

## Candidate and cost

The candidate retains startup, shared-frame, strip, drag, and save appointments as absolute owner instants; no service is gated and no running flag was added.
`BT_PERF_TRACE idle_wake owner=… in_us=…` names the winning sub-100 ms entry only when no journey or frame debt is owed.
An idle per-window fold still evaluates 49 entries; naming/minimum selection is stack-only and allocation-free. The unchanged-menu path compares shortcuts/focus/language with no allocation or AppKit call.
Footprint baselines are sampled at most every 250 ms (four platform calls/s under a spurious spin), retaining a baseline no more than half the 500 ms slow-hold threshold old; closing samples remain report-only.
Windows compiles and targeted tests pass; the local macOS cross-check stops in dependency `psm` because this Windows host has no Apple `cc`, before project code is checked.

## Eight-line summary
Baseline audited: `484157e4`; no GUI was launched.
The fold has 49 entries; 29 are already absolute and 20 are renewing at the top-level classification above.
Those renewals are replaced by owner epochs without changing journey liveness or service gates.
No headless harness can provide A1's authoritative turn rate or winning deadline.
The candidate names sub-100 ms idle winners under `BT_PERF_TRACE`.
The menu memo rejects unchanged inputs before plan allocation or AppKit entry.
Memory footprint sampling is coarse rather than per-wake.
Windows and macOS idle measurements remain for the coordinator's candidate build.

## CI follow-up

The Windows `logic` job found one production orphan in the deadline rewrite:
`PaneMotion::deadline` was called only by tests and still manufactured
`now + frame`. It is removed. The pane wake test now composes
`PaneMotion::is_animating` with one absolute animation appointment and proves
resting, in-flight, unchanged-query, landed, and reduced-motion behavior without
pinning the retired renewal shape.

Orphan audit: `PaneMotion::deadline` was the only `fn deadline` or
`*_deadline` definition whose callers were all under `tests.rs` or
`#[cfg(test)]`; no other deadline owner required removal.

Windows validation passed, serially and with `-j 4`:

- `cargo clippy -p bt-app --all-targets --locked -j 4 -- -D warnings`
- `RUSTFLAGS="-D warnings" cargo check -p bt-app --bin folio --locked -j 4`
- Narrowed `cargo test -p bt-app --bin folio --locked -j 4` filters for the
  absolute pane wake, reduced motion, the full pane flight, tab-switch cleanup,
  and the no-query-time-renewal source gate.

## CI follow-up 2

The remaining journey source pin still looked up the removed
`PaneMotion::deadline` and blessed its retired `now + frame` body. It now pins
the fact that `strip_animation_work` maps `PaneMotion::is_animating` onto the
strip's absolute tick read from the window `frame_clock`, and separately keeps
the deleted renewable helper absent.

The companion termscroll audit found its equivalent renewal still present but
unreachable: `terminal_thumb_work` already used `next_animation_deadline` while
a live fade, so the moving arm of `termscroll::fade_deadline` was exercised only
by tests. That arm and its frame argument are removed; `fade_wait_deadline`
retains only the state-owned end of `THUMB_REST`, while the runtime owns every
moving-frame appointment through the same window clock.

Windows validation passed, serially and with `-j 4`:

- Whole suite: `cargo test -p bt-app --bin folio --locked -j 4` — 4,041 passed,
  0 failed, 6 ignored (plus the intentional subprocess probe: 1 passed).
- `cargo clippy -p bt-app --all-targets --locked -j 4 -- -D warnings` — passed.
- `RUSTFLAGS="-D warnings" cargo check -p bt-app --bin folio --locked -j 4` —
  passed.
