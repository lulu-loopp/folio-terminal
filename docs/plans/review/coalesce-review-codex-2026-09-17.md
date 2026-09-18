# Coalesced PTY drain review

Commit: 8ec9835a, the single commit after c425e400.
Verdict: **MERGE WITH MUST-FIXES** — three findings meet the supplied release bar.
Scope: static lifecycle audit and permitted tests; no live Folio launch or manual process termination.

## Release blockers

1. **BLOCKS [bar 2/3, P1]: Short echoes and ordinary ConPTY output can acquire 3 ms latency.**
   `crates/bt-pty/src/lib.rs:1066-1073` treats any repeated running maximum as capped, including one byte.
   Read lengths [1, 1] flag the second echo; repeated 8192-byte ordinary ConPTY reads also defer when no larger read has occurred and the ring drains.
   Even with a real 1024-byte cap learned, `try_pop_slice` (1173) overwrites earlier flags: a one-byte uncapped echo followed by a capped read in the same slice reports capped.
   `main.rs:84344-84364` has neither a ConPTY exemption nor a pending-keyboard bypass; the short-read unit test supplies its uncapped flag manually and misses these paths.
   Preserve short/interactive-arrival evidence and keep ordinary ConPTY publication immediate; add detector-to-policy and mixed-chunk coverage. Actual ConPTY false-positive frequency was not measured.

2. **BLOCKS [bar 4, P1]: A DEC 2026 commit wholly inside one slice misses the immediate-publication bypass.**
   `crates/bt-app/src/main.rs:37149-37150` detects only open-before/closed-after.
   A capped 1024-byte slice containing BSU + 1008 printable bytes + ESU commits the grid, but both deadline snapshots are absent, so its completed frame waits 3 ms.
   Close/reopen on a sibling pane also escapes these snapshots: the window's `sync_open` consults only the focused pane (84347-84350).
   Carry an actual parser commit event/counter across feed, and fold open-block state over visible panes. Atomic buffering itself remains intact; the regression is delayed committed publication.

3. **BLOCKS [bar 2, P2]: An intervening publication cancels the owed cursor reveal.**
   `main.rs:84363-84368` skips cursor reset when deferring; `publish_frame_inner` settles the debt at 66520 without resetting the cursor.
   With the caret dark, a decoration-result Expose (83644-83647) before expiry publishes the new frame and clears `until`; `finish_pty_coalesce_if_due` (84382-84391) then returns before its reset.
   The caret stays dark until its previous 550 ms blink deadline instead of revealing on output. Preserve that reset when any publication settles an output burst, or reset on arrival as before.

## Recorded for later

- **Recorded for later [P2]:** The display bound is not implemented: `main.rs:84358` always passes `next_display_deadline: None`. A 3 ms interval shorter than a refresh period can still cross the next refresh; document only the timer bound unless a real display deadline is supplied.
- **Recorded for later [P2]:** The two session tests manually mark tiny pieces capped and assert formula presence, not specifically torn source cells. They exercise the policy/session combination, not the detector or runtime wiring, and do not cover the three blockers. Scheduling assertions use injected instants; no elapsed-wall-clock assertion was added.

## Lifecycle and transport audit

- No additional lost-wake stall found: every non-leaving window runs `turn`, and its deadlines reach the application minimum (main.rs:114542-114571), including unfocused/hidden windows and ordinary modal state.
- Retirement and empty-window branches intentionally stop presenting. Closed windows are reaped; hidden tabs are fed and recomposed on activation; PTY exit/pane close retain structural publication paths.
- Occluded frames remain in the pending slot (103470-103487); `Occluded(false)` republishes (115561), and quick-terminal show explicitly publishes (39591-39619). No dependency on another PTY byte found.
- Resize mutation, presentation holds and quiescence release retain their own paths/deadlines. The 150 ms sync timeout walks all panes before coalesce release; simultaneous deadlines do not force buffered sync bytes out early.
- Shell-marker pauses resume synchronously inside `feed_at` (bt-term/src/session.rs:3134-3150), before consuming the slice flag. Dump `write_chunk` still records each original read before ring slicing; its sidecar test passes.
- Full 16 KiB ConPTY reads split into two 8 KiB slices; the first is uncapped, so the OR publishes immediately when both drain together. A remaining backlog also publishes immediately: sustained backlog keeps per-turn publication, not a mandatory 333 Hz cap. No throughput/FPS benchmark was run.
- Repeated 4 KiB Linux reads can coalesce; larger maxima restart corroboration. Existing flags remain attached to queued chunks. A steadily growing maximum reduces coalescing effectiveness without extending an armed deadline.
- The source-text invariant test checks that arm/release/fold/settle sites exist, not the lifecycle behavior or cursor side effects. macOS visibility paths were inspected, not exercised live.

## Validation

- `cargo test -p bt-pty -j 4`: passed, 50 unit + 5 integration tests; 19 ignored.
- A second identical permitted command passed three temporary deterministic counterexamples: detector/policy echo deferral, real `DualPlaneSession` same-slice DEC commit, and canceled cursor reveal.
- Those counterexamples used verbatim extracted production detector/policy/blink implementations; they assert the problematic behavior and are not end-to-end window tests. Scratch source was deleted.
- `cargo test -p bt-app --bin folio coalesce -j 4`: passed, 19 tests, including both T=0/T=3 ms session tests; build took 7m 16s.
- `cargo test -p bt-app --bin folio pty_drain -j 4`: passed, 20 tests, including the arm/settle/release/fold source-text invariant.
- Product code is unchanged; only this report is committed on `docs/coalesce-review`.
