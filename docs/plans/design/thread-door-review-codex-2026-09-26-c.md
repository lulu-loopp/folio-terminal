**adopt with changes**

Static confirmation of revision (d), limited to P1–P4, on `design/thread-door` at `d85ed3fc`, base `78a3699a`. The branch diff contains only design/review documents. Read the prior scoped review, the overriding revision and relevant source bodies. No build, tests, compiler probes or commit; only this review file is written.

## P1 — met

The meeting sentence is: “Every test arm whose producer is a thread started by `spawn_at_priority` belongs to **A1b**.” (d)1 explicitly removes M4g from A1a, assigns the worker-refusal arm of owner M5 to A1b, and retains its Window control in A1d. Its replacement table also allocates the worker phase and callback arms to A1b and forbids substituting the standalone producer. This resolves the scoped dependency objection.

## P2 — met

The meeting sentence is: “It does this on every normal return, **including when the detail ledger was full**: a `ROOT` node or scope is restored as `ROOT`”. Each admitted frame now carries the complete saved `Location::Resume` in its own `Cookie(u64)`, without the lossy adapter stack.

Verified: `Station` is `repr(u8)`, with 210 values (0–209); `hang_watch_detail::{CAPACITY, ROOT}` are both 256. The ledger produces slots 0–255 or ROOT, so saved node and scope are each 0–256. Bits 0–7, 8–23 and 24–39 preserve all three fields, including ROOT; `Heartbeat::resume_at` restores station, node and scope. The proposed capacity assertion prevents truncation (conservatively rejecting capacity 65,535 too).

One non-blocking anchor correction: today's `Station::from_byte` returns `Station`, falling back to `Starting`, not `Option`/`None` (`hang_watch.rs:1239,1451`). The proposed invalid-cookie counter therefore needs an explicit byte-range check or a newly specified checked decoder. Valid cookies and full-ledger restoration do not depend on this correction.

## P3 — not met

The native and presentation boundary repairs are concrete: `apply_pointer_cursor` has the early and tail calls (`runtime/mouse.rs:2403,2407`); visibility is true in `put_the_window_on_the_glass` and false in `hide_quake_window` and `let_go_of_this_window` (`runtime/windows.rs:692,2030`; `runtime/quake.rs:111`). The `owner_door` wrappers cover those statements. The declared `PresentFrame` batch and subsequent `CompositorCommit` are sibling admissions inside the retained `during(PresentSeats)`; `set_covered_size` remains between them. `SurfaceBirth` names the additional-window constructor/configure path; recovery configure is explicitly deferred to B9.

The four added application web-seat commit callers exist (`webhost.rs:2255,3634`; `web_spare.rs:88,96`), but they are not all remaining callers. Windows product code also calls `Compositor::commit` in `Compositor::new` and `Compositor::set_window_size` (`bt-platform/src/lib.rs:3900,4031`), twice in `WebHost::rehost` (`webview.rs:1920,1929`), and twice in `WebHost::compensate` (`webview.rs:1985,1994`). These six statements have no specified token source, admission boundary or refusal/compensation handling. Adding a required token to `commit` makes their treatment mandatory. Existing direct test callers also need adaptation (`webview.rs:3987`; `portable_impl.rs:1822`).

**One change:** complete the item-level table and A1d scope for every `commit` caller, including these six product statements, specifying token provision, phases, measurement, refusal/compensation and witnesses; include the test-call adaptation and replace remaining caller placeholders such as “the brief lists them by grep” with the actual inventory. The worker must not have to choose these contracts while implementing the brief.

## P4 — not met

The new rule rejects unlisted first-party edges, so it closes the earlier indirect-helper loophole. Both engine shutdown bodies do send Shutdown, poll with one 2 ms sleep call site under `SHUTDOWN_BUDGET`, then conditionally join (`video/engine.rs:643`; `macos_player.rs:445`). `PtySession::drop` finishes the input dump before shutdown (`bt-pty/src/lib.rs:2197`); shutdown closes input, detaches the writer, handles child exit/reaping, closes output, drops the master, then conditionally calls `join_within`. Those ordering corrections are sound.

However, “The table is the whole baseline” contradicts the proposed requirement to list every first-party edge. `flush_sink` calls first-party `Queue::close` (`trace_sink.rs:228`), absent from its row. Both ring `close` bodies call their first-party `state` helpers (`bt-pty/src/lib.rs:1355,1490`), also absent. Windows `DirWatch::drop` calls the first-party `close` helper three times; the row labels these as effects instead of edges and omits that helper's `CloseHandle` effect (`bt-platform/src/lib.rs:10788–10790,10973`). Further, `PtySession::shutdown` contains two `child.try_wait()` call sites, including the closure passed to `reap_within` (`bt-pty/src/lib.rs:2121,2137`), not the table's one. Under the stated closed rule, today's controls cannot all be green.

**One change:** reconcile all 12 rows with the complete per-body ordered first-party edge lists and vocabulary counts, including these helpers and the second `try_wait` site, and pin the reviewed helper bodies recursively. Keep the closed rule and mutations; repair its baseline rather than reintroducing an exemption for non-vocabulary helpers.

## Dispatch

**May A1a be dispatched now? No.** First complete P3's caller/token/refusal inventory so A1a's registry is unambiguous. P1 and P2's former dispatch blockers are resolved; correct P2's decoder wording alongside that revision. P4 must be repaired before A1e dispatch.

**May A1d's table be treated as its brief? No.** Finish P3's missing contracts and caller inventory first.
