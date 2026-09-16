# Performance review — where the window thread's time goes (GLM 5.3, 2026-09-16)

STATUS: COMPLETE — phases 1–4 done; P-1…P-10 re-verified against the cited lines; two corrections applied during re-verification (P-3's fix now respects the same-batch ordering the inline wheel flush exists for; P-6 demoted to low after `reset_cursor_blink` was found to publish only on a real blink-state change)

Review target: main 2a261ebf (0.4.1 candidate). Read-only. Evidence: D:\Developer\trace\next67\stderr.log (+ mouse/card/thumb/ime logs, pty.bin.chunks), scratchpad\review3\perf-diagnostics.log, hang-20260916011136410.txt.

The owner's two complaints, kept in view throughout: **"typing stalls"** and **"the pointer stutters when I move the mouse."**

## What the traced run actually did (88.14 min, 30,776 presents)

Every present — retained or composed — runs `compose_frame` and prints one `frame=` line, so present-side and compose-side numbers below cover the same 30,776 GPU passes.

| quantity | value |
|---|---|
| presents by source | Expose 15,678 · PtyOutput 11,095 · Keyboard 3,989 · Resize 14 |
| presents of an **unchanged picture** (`retained=1`) | **14,064 / 30,776 = 45.7 %** (89.7 % of all Expose presents) |
| frame total | p50 8.2 ms · p90 33.6 ms |
| `rectangles_us` (rect instance build + encode) | p50 5.7 · p90 10.8 · p99 14.5 · max 163 ms — 4,490 frames > 10 ms — sum 212.6 s |
| `acquire_us` (`get_current_texture`) | p50 0.03 · p99 41.4 · max **746 ms** (17 frames > 100 ms) |
| `submit_present_us` (`queue.submit` + `queue.present`) | p50 0.8 · p90 3.1 · p99 **103 ms** · max **1,076 ms** (414 frames > 100 ms; sum 219.5 s) |
| event→present by source | Keyboard 20.9/107.8/~230 ms (max 42.4 s †) · Expose 9.4/41.3 ms (max 1.08 s) · PtyOutput 14.1/31.9 ms · Resize 11.4/61.9 ms (p50/p90) |
| Expose inter-present gap | p10 18 ms · p50 109 ms · p90 479 ms — 2,763 burst runs (< 50 ms apart), longest 53 |
| live math scans scheduled | 548 trace lines, 1,184 detections cumulative |
| DEC 2026 synchronized-update defers | **0** — the 150 ms BSU/ESU window never fired in this run |
| held-control events in this run | 17 events, 29.9 s: flush_wheel 21.7 s (n=12, p50 934 ms, max 6.05 s) · drain_pty 3.9 s · advance_web_page 1.9 s · publish_frame_inner 1.2 s |

† The 42.4 s and 9.2 s "Keyboard" presents are an attribution artifact — see P-7.

The given baseline (Keyboard 275/19.4/97.2/296.9 etc.) is a narrower aggregation of the same log; whole-run figures above are used throughout because every finding is checked against them.

Where the thread's time went in this run, coarse but decisive: **212.6 s building rectangles, 219.5 s inside submit+present, 14,064 of the presents carrying pixels identical to the frame before them.** The stalls the owner feels are not in the terminal pipeline (digest 100–400 µs, row compose 350–550 µs — as given); they are in how often the GPU is asked to present, what blocks while it is asked, and a handful of synchronous side-trips.

---

## Findings, ranked by contribution to the felt stalls

### P-1 · Nearly half of all presents re-present an unchanged picture, each as a full GPU pass — critical

- **Where**: `crates/bt-app/src/main.rs:64644` (`publish_frame` grants `skip_unchanged` **only** to `FrameSource::PtyOutput`); `crates/bt-app/src/main.rs:64677-64687` (`publish_chrome_frame` → `chrome_present_pending` + `request_redraw`); `crates/bt-app/src/main.rs:100048-100164` (`present_retained_picture` — "No projection, no capture" yet it still calls `present_seats_and_commit` → `renderer.present_frame` → `compose_frame`, so every retained present pays the full rectangle/encode/submit/present pass; its own frame line at p50 8.2 ms proves it); `crates/bt-app/src/main.rs:85839-85848` and `:85861` (`repaint_pane_change` / `represent_on_screen_frame` publish unconditionally).
- **Per-event work that is too expensive**: a whole-window GPU compose + swapchain present + DirectComposition commit (`crates/bt-render/src/lib.rs:8278-8320`, `:9631-9640`) to put on the glass a picture byte-identical to the one already there. Redundant recomputation by construction: the content digest (`content_fnv`) of 7,929 of 15,678 Expose-composed frames equals the previous Expose frame's, and on top of those, the retained path presents 14,064 times without even asking whether anything moved.
- **Trigger**: any chrome-visible tick — hover marks, tween phases, pane notices, wheel overlay thumb, cursor-blink chrome — plus every `repaint_pane_change` from hover crossings whose pane content did not change. With no PTY output, this is a ~9 Hz present heartbeat (Expose gap p50 109 ms) that never goes quiet.
- **Measured cost**: 45.7 % of all presents; ≥ 14,064 × p50 8.2 ms ≈ **115 s of GPU compose for zero pixel change** in 88 min, plus the same again in DWM/compositor work, plus the queue pressure that becomes P-2.
- **Smallest correct fix**: the revision facts already exist — `picture_on_glass()` carries `content_revision`/`presented_revision` (`main.rs:64651-64664`). Extend that gate one step: before `present_seats_and_commit` in the retained path (and before `request_redraw` in `publish_chrome_frame`), require that either the picture revision, a chrome-quad signature, or a blink/tween phase actually moved; a no-op present asks for nothing. Keep composing when anything moved — the retained path exists for real chrome deltas.
- **Expected effect**: removes ~45 % of all GPU passes. Expose p50 9.4 ms → ~0 for idle windows; Expose p90 41.3 ms and the whole tail shrink; Keyboard/PtyOutput p90s improve indirectly through P-2 (less queue pressure). Idle CPU/GPU both drop.

### P-2 · `queue.present`/`get_current_texture` block the window thread for up to a second — critical

- **Where**: `crates/bt-render/src/lib.rs:9631-9640` — `submit_present_us` brackets `gpu.queue.submit` + `gpu.queue.present`; acquire is the same frame's `get_current_texture`.
- **Per-event work that is too expensive**: synchronous swapchain operations on the window thread. Presenting faster than DWM consumes fills the swapchain's frame queue; the next acquire then blocks (max 746 ms) and the next present blocks (max 1,076 ms). While blocked, no input is read, no turn runs — this is precisely "the pointer stutters when I move the mouse."
- **Trigger**: present bursts. The four worst submits in the run (1,076 ms, 916 ms, 991 ms, 604 ms) all sit inside wheel-driven Expose/retained storms (log lines 64752/64761: `wheel=322`; 13207; 37109), where wheel publishes and retained re-presents interleave at < 50 ms gaps.
- **Measured cost**: 414 frames with submit > 100 ms, 2,899 > 10 ms; acquire > 100 ms 17 times; together this is the 100-ms-to-1-s band of every latency tail. Keyboard's submit→present share is p90 48.6 ms / p99 102.5 ms — half the real Keyboard p90 is waiting on the GPU queue, not on Folio's own work.
- **Smallest correct fix**: never queue a second present while one is unconsumed — in `redraw` (`main.rs:100187+`), if `pending_frames` was already holding an unconsumed frame (`slot_overwrites` > 0 this turn) skip the GPU pass and let the next redraw carry the newest picture; that is coalescing at the one place all presents already pass through. Second lever: request a mailbox/immediate-fifo present mode where the platform allows, so present never waits on a stale queue slot.
- **Expected effect**: eliminates the 0.1–1.1 s stalls outright; Keyboard p90 107.8 → ~50 ms (its submit share), p99 ~230 ms → well under 150 ms; Expose max 1.08 s → tens of ms; PtyOutput p90 31.9 → ~25 ms.

### P-3 · The wheel burst is flushed synchronously in front of every non-wheel event — high

- **Where**: `crates/bt-app/src/main.rs:111329-111334` (`window_event`: `if !matches!(event, MouseWheel) { runtime.flush_wheel()? }`), `crates/bt-app/src/main.rs:95170-95177` (`flush_wheel` at the head of every `turn()`), routing ladder `main.rs:95179+`, terminal route `scroll_view_exact_in` `main.rs:95839-95862`.
- **Per-event work that is too expensive**: one full routing ladder (a dozen overlay hit-tests), `settle_math_toggle`, thumb wake, then `repaint_pane_change` → `publish_frame_inner(trigger, false)` — **never skippable** (`main.rs:85839-85848`) — then a GPU present, executed inline *before the event that triggered the flush is even looked at*. A keystroke or pointer move that arrives during a wheel gesture pays the whole flush first. (The Wheel station's ledger span also swallows DPI settling — that attribution split is the known in-progress ticket and is not re-reported here.)
- **Trigger**: any wheel gesture (bursts of notches coalesce correctly — 376 events → 373 routings — but each routing is a full publish+present), then *any* other event.
- **Measured cost**: in the traced run flush_wheel is 21.7 s of the 29.9 s total held time (12 events, p50 934 ms, max 6.05 s); owner history: 970.9 s over 1,101 events, p50 671 ms, p90 1.63 s. Per flush the composed frame is p50 8–16 ms — the seconds-scale entries are the P-2/P-1 storms landing inside the Wheel span plus DPI settling billed there.
- **Smallest correct fix**: the inline position is load-bearing (`main.rs:111324-111328`: "whatever arrives next answers the window the wheel has already moved" — a same-batch click must land on the scrolled state), so it stays. What does not have to stay is the flush's price: grant `skip_unchanged` to the wheel publish (`repaint_pane_change`'s `publish_frame_inner(trigger, false)` at `main.rs:85844`), because a notch that lands on a clamp edge — or a burst whose whole travel is clamped — changes no cell, and the content digest already knows that. Second, `repaint_pane_change` for an unfocused pane falls through to `represent_on_screen_frame` even when the scroll moved nothing; the same digest gate should retire that re-present.
- **Expected effect**: keystrokes and pointer moves during scroll stop queueing behind no-op full publishes; clamp-edge wheeling costs nothing; the flush entries that remain in the ledger are real content, which is what the in-progress station-split ticket needs to see. Typing-stalls-while-scrolling shrinks to the real compose cost (~10 ms per batch), not a redundant GPU pass on top of it.

### P-4 · Every present re-projects and re-arms every unfocused pane, and each one re-clones up to 1,024 scrollback lines — high

- **Where**: `crates/bt-app/src/main.rs:100257-100287` (`redraw`: per unfocused pane `refresh_projection` + `viewport_frame` + `absorb_printed_path_probes` + `schedule_visible_artifacts`); the same `schedule_visible_artifacts` from `publish_frame_inner` (`main.rs:64817`); `crates/bt-term/src/session.rs:7634-7646` (calls `schedule_live_artifacts` unconditionally — only `primary_parked`/resize-epoch gate it); `session.rs:3639-3651` — `live_detection_context()` **before** any signature check; `session.rs:5743-5802` — the clone: up to `LIVE_FENCE_HISTORY_CONTEXT_LINES = 1024` history lines (`entry.line.text.clone()` + `frozen_cell_boundaries` per line, primary screen) plus every visible row, re-allocated per call; `crates/bt-detect/src/lib.rs:2763-2814` — when any candidate is new, `scan_live_math_blocks_in_context` runs over the whole context **synchronously**.
- **Per-event work that is too expensive**: O(scrollback-tail) allocation + copy (1,024 lines × line length) per pane per publish **and** per redraw present — twice per frame for each unfocused pane — before the cheap signature test at `session.rs:3651/3662` can say "nothing changed"; the signature is computed *after* the context it summarizes. Per-event allocation at frame rate, growing with scrollback.
- **Trigger**: every composed publish (any source) and every present with a pending frame, in any window with ≥ 2 visible panes or any primary-screen session (Claude Code-style TUIs are primary-screen).
- **Measured cost**: 30,776 presents × (1 + unfocused panes) context builds; 1,184 synchronous scans over the run (548 trace lines); this is the bulk of `publish_frame_inner`'s own ledger (owner history: 160.0 s total, p50 10 ms, p90 382 ms, max 10.1 s) once the traced run's cheap frames are accounted for.
- **Smallest correct fix**: memoize the context on the facts that change it — `(grid_generation, live_rows revisions, history length)` — rebuild only when the terminal actually damaged; and check a cheap signature of those facts *before* building, not after. Move `scan_live_math_blocks_in_context` to the existing math worker (it is already off-thread for frozen lines).
- **Expected effect**: publish_frame_inner p50 10 ms → ~3 ms, p90 382 ms → tens of ms; every source's p50 improves (the tax is paid on all of them); removes the worst 10-s outlier class.

### P-5 · `advance_web_page` — seconds-scale synchronous WebView2 teardown on the window thread — high

- **Where**: `crates/bt-app/src/main.rs:98597-98705`; the blocking calls are `web.close()` → `ICoreWebView2Controller::Close` (`main.rs:98671-98673`, stationed `WebRetire`) and the teardown/rebuild deadline path inside `web.tick` → `apply` (`crates/bt-app/src/webhost.rs:2666-2673`, `AwaitBrowserExitBeforeCleanup` → `host.close()`) — which runs inside the `WebPage` station and is what the ledger's `advance_web_page` entries actually measure.
- **Per-event work that is too expensive**: synchronous COM calls that run the browser process's page teardown while this thread waits. Not per-turn cost — the routine walk (orphan scan, `set_claims`, deadline compares) is p50 6 ms.
- **Trigger**: a web page retiring (pane closed, replaced, tab gone) or a browser-exit deadline firing while a page seat exists.
- **Measured cost**: owner history 1,840.9 s total, but **86.6 % is three events** (one 26.4-minute outlier — almost certainly a sleep/deadline pathologic, not routine — plus 5.9 s and 4.8 s); the routine tail is p90 592 ms with hundreds of 0.6–6 s entries. Traced run: p90 469 ms, max 1.33 s.
- **Smallest correct fix**: nothing in `tick`'s deadline arm needs the window thread — post `host.close()` and the rebuild `apply` to the existing worker infrastructure (the file already runs captures/decodes off-thread for exactly this class of cost), and let outcomes arrive as they already do through `WebOutcome`. Keep the station split so the ledger can prove it.
- **Expected effect**: removes 0.5–6 s full-thread stalls whenever pages retire; no direct change to the four latency numbers (no web pane in the traced run's hot windows), but it is one of the two largest single-stall causes in the owner's own ledger.

### P-6 · A keystroke that catches the caret mid-blink composes two full frames back to back — low

- **Where**: `crates/bt-app/src/main.rs:95930-95935` (`if self.reset_cursor_blink(now) { publish_frame(Keyboard) }`); `reset_cursor_blink` (`main.rs:82497-82503`) returns `changed`, so the publish fires only when the caret was actually in its off phase; the echo then arrives as the drain-published frame stamped by `pending_keyboard_at` (`main.rs:81834-81872`).
- **Per-event work that is too expensive**: two full GPU passes for one key — the blink frame (caret solid) and, 10–25 ms later, the echo frame, which would have shown the caret solid anyway. Skip is never granted (`Keyboard` source, `main.rs:64644`). This is the pause-then-type pattern; a fast typist keeps the caret solid and pays nothing.
- **Trigger**: a keystroke landing during the caret's off phase in a terminal pane.
- **Measured cost**: bounded by the blink duty cycle — a fraction of the 3,989 Keyboard presents, each p50 7.5 ms compose + a present slot.
- **Smallest correct fix**: the blink frame's only reader is the screen the echo is about to repaint; publish the blink reset into the pending slot (or under the same revision gate P-1 installs) and let the echo's frame carry it — one present instead of two for the same visible result.
- **Expected effect**: small on its own (Keyboard p50 improves only for pause-then-type); it is the same gate P-1 builds, applied at one more call site.

### P-7 · The 42.4 s "Keyboard" latency is a measurement artifact — correction (medium, no runtime change)

- **Where**: `crates/bt-app/src/main.rs:81834-81872` — `publish_pty_drain_frame` stamps `occurred_at: keyboard_at.unwrap_or(now)` and `source: Keyboard` whenever `pending_keyboard_at` is set; the stamp is cleared **only** when `published || !sync_open`.
- **What actually happens**: a TUI that does not echo (Claude Code thinking for 42 s) leaves the stamp standing; the next frame any drain publishes — for any reason — is tagged Keyboard with a 42-s-old `occurred_at`. Walk-back of the 42.4 s window shows 161 retained Expose presents of byte-identical content while the stamp waited: the window was presenting fine; the *shell* was silent. The real defer mechanism (DEC 2026, 150 ms cap in vendored vte 0.15.0) fired **zero** times in this run.
- **Smallest correct fix**: clear the stamp once the drain has consumed the keystroke's bytes (the reader thread knows the offset), or age it out after one echo window. This changes the reported numbers, not the behavior: Keyboard max 42.4 s → real values, p99 ~230 ms.
- **Why it matters for this review**: it redirects effort — the keyboard tail to fix is ~230 ms (P-2's queue waits + P-4's tax), not 42 s.

### P-8 · `rectangles` is the flat tax every present pays, including retained ones — medium

- **Where**: measured in every frame line (`crates/bt-render/src/lib.rs:9665`); the rect instance build runs per `compose_frame` call regardless of what changed.
- **Per-event work**: p50 5.7 ms / p90 10.8 ms / max 163 ms per present — 212.6 s over the run, 4.1 % of wall time — for window chrome and cell fills that are identical frame to frame on a retained present.
- **Trigger**: every present (30,776 in the run).
- **Smallest correct fix**: cache the rect instances with the picture revision (invalidate on content/chrome revision change); on a retained present, re-submit the cached buffers and skip the rebuild. P-1 removes most retained presents outright; this makes the ones that remain honest.
- **Expected effect**: retained/composed-unchanged frame totals 8.2 ms → ~2–3 ms; multiplies through P-1.

### P-9 · A DPI change re-keys every leaf of every tab before the first frame at the new size — medium

- **Where**: `crates/bt-app/src/main.rs:97538-97617` (`reconcile_authoritative_dpi`: renderer resize → solve → `resize_leaves_to_layout`), `main.rs:83479-83562` (active tab's leaves) and `:83541` (`resize_hidden_leaves_to_layout` — **every** tab), then `publish_frame(Expose)`.
- **Per-event work**: swapchain recreate (waits for GPU idle) + grid re-derivation for all leaves of all tabs + one full compose at the new size, all on the window thread before anything is shown at the new rectangle. The traced run's 14 Resize presents are healthy (p50 11.4 ms, p90 61.9 ms) because they land on a 2-pane window; the owner's `publish_frame_inner` ledger (p90 382 ms, max 10.1 s) is what a DPI move across many tabs costs, and it lands — via the known span split — billed under `flush_wheel`.
- **Trigger**: monitor-DPI crossing at the end of a window drag, or a `Resized` after a deferred settlement.
- **Smallest correct fix**: resize hidden tabs lazily — on tab activation, not on settlement (the machinery to detect "was never shown at this size" is `shells_settled_revision`, written two lines earlier).
- **Expected effect**: Resize p90 61.9 → p50 territory on multi-tab windows; removes the multi-second settlement entries from the owner's ledger tail.

### P-10 · A full ~60-station `turn()` runs after every event batch — low

- **Where**: `crates/bt-app/src/main.rs:110478-110643` (`about_to_wait_inner` → `runtime.turn(now)` per window) and the station walk `main.rs:100465-100930`, closing with the wake fold `main.rs:100717-100927` (~50 deadline readers).
- **Per-event work**: each `.due()` check is cheap (µs), but the walk is O(stations) per batch and runs at input/PTY event rate; it is the amplifier that turns any per-publish cost (P-1/P-4) into a per-event cost. The idle path itself is correct: `ControlFlow::Wait` with deadlines absent (the past-deadline 100 % CPU bug, RB-1, is already fixed at `main.rs:100896`).
- **Measured cost**: not separable from stations' own work in this instrumentation; `woken` in the owner's ledger (343.1 s, p90 1,017 ms) is the wake-side slice of the same structure.
- **Smallest correct fix**: none urgent at current station cost — record it as the structural reason P-1..P-4 multiply, and gate the heaviest stations (web tick, preview rails, artifact scheduling) behind the dirty flags they already compute.
- **Expected effect**: jitter smoothing only; do not spend effort here before P-1..P-4.

---

## The three to fix first

1. **P-2 — make present non-blocking** (coalesce at the redraw slot; never present twice per consumed frame). This is the direct mechanism of "the pointer stutters": input waits behind a blocked `queue.present`.
2. **P-1 — stop presenting unchanged pictures** (extend the `content_revision`/`presented_revision` gate to the retained/chrome path). Removes ~45 % of all GPU passes and de-pressurizes the same queue P-2 fixes.
3. **P-3 — one wheel flush point, at the turn boundary, skippable when unchanged.** This is "typing stalls" whenever typing and scrolling overlap.

**Single first action**: install the unchanged-present gate (P-1) — it is the smallest correct change (one revision comparison at the `present_seats_and_commit` door plus a chrome-quad signature), it is measurable in one run of the existing `BT_PERF_TRACE` (retained=1 count and the four latency p90s before/after), and it directly reduces the queue pressure that produces P-2's worst blocking, so the trace after it will show how much of the tail still needs P-2's coalescing.

## Expected movement on the four numbers, all fixes in

| source | now (p50/p90) | after P-1+P-2+P-3 | driver removed |
|---|---|---|---|
| Keyboard | 20.9 / 107.8 ms (max 42.4 s †) | ~13 / ~45 ms, max < 150 ms | queue waits (P-2), per-publish tax (P-4), inline wheel flush (P-3), mid-blink double compose (P-6); † becomes impossible (P-7) |
| Expose | 9.4 / 41.3 ms (max 1.08 s) | ~0 idle / ~15 ms, max tens of ms | 45.7 % no-op presents (P-1), queue waits (P-2) |
| PtyOutput | 14.1 / 31.9 ms | ~10 / ~20 ms | per-publish tax (P-4), queue waits (P-2) |
| Resize | 11.4 / 61.9 ms | p50-class on multi-tab windows | hidden-tab re-key (P-9) |

## Phase log
- Phase 1 — evidence read: stderr.log full-run statistics (present/frame/projection/skip/held), perf-diagnostics.log full station table, pty chunk stats, hang file. ✔
- Phase 2 — window-thread paths mapped A–F: keystroke (keyboard_input → write non-blocking → reader wake → drain → publish_pty_drain_frame → redraw), pointer move (pointer_moved 86196-86877 cascade, update_chrome_hover early-out, observe_hovered_pane crossing-only), wheel (WheelBurst coalescing → one routing → full publish), advance_web_page (sync COM teardown), resize (settle_dpi_* → resize_leaves_to_layout → publish), turn loop (per-batch turn, idle Wait correct). ✔
- Phase 3 — findings P-1…P-10 written, ranked. ✔
- Phase 4 — every finding re-verified by re-opening its cited lines. Corrections made: **P-3** — the proposed removal of the inline `window_event` flush was wrong (the comment at `main.rs:111324-111328` shows the same-batch ordering is deliberate: a click must answer the scrolled window); fix replaced with the digest gate on the wheel publish. **P-6** — `reset_cursor_blink` (`main.rs:82497-82503`) returns `changed`, so the blink publish fires only when the caret was mid-blink, not on every keystroke; demoted to low and re-worded. Dropped during Phase 2/3 (own-draft corrections, kept for the record): wheel events are already coalesced per batch (376 events → 373 routings); terminal Local scroll is an O(1) projection offset, not O(scrollback); `retained` presents do run a full GPU pass (frame line printed each time), which is the basis of P-1. ✔
