STATUS COMPLETE

# Independent verification of GLM report findings P-1, P-3, P-4, P-7, P-9 (2a261ebf)

Read-only verification of `docs/plans/review/perf-review-glm-5.3-2026-09-16.md`. Each finding re-opened at its cited lines, plus one level up/down on each code path. No cargo, no launch, no tracked-file change. Verdicts cite lines at commit 2a261ebf.

---

## P-1 · Nearly half of all presents re-present an unchanged picture as a full GPU pass — CONFIRMED

1. **Verdict**: CONFIRMED (one precision caveat: "byte-identical" is content-identical, chrome may differ).

2. **Claim vs. code.** The claim: a chrome-visible tick (hover mark, tween, blink) that finds the terminal content unchanged still pays a full GPU compose + swapchain present + DirectComposition commit. Decisive lines:
   - `crates/bt-app/src/main.rs:64644` — `let skip_unchanged = matches!(trigger.source, FrameSource::PtyOutput);` — only `PtyOutput` may skip; Expose (chrome/retained) never does.
   - `crates/bt-app/src/main.rs:64677-64681` — `publish_chrome_frame` → `chrome_tick_reuses_picture` → `chrome_present_pending = true; request_redraw()` (no publish, so the retained path runs).
   - `crates/bt-app/src/main.rs:100048, 100139` — `present_retained_picture` → `present_seats_and_commit`.
   - `crates/bt-app/src/main.rs:99927` — `present_seats_and_commit` → `renderer.present_frame(gpu, seat_frames, trigger)`.
   - `crates/bt-render/src/lib.rs:8278-8284` — `present_frame` → `self.compose_frame(gpu, seats, trigger)`.
   - `crates/bt-render/src/lib.rs:9631-9640` — `gpu.queue.submit(...)` then `gpu.queue.present(texture)`; the retained path pays both.
   The retained path genuinely skips projection/capture (doc comment at `main.rs:100042-100047`), but `compose_frame` still builds rect instances, encodes, submits and presents. `main.rs:85844` (`repaint_pane_change` → `publish_frame_inner(trigger, false)`) confirms `repaint_pane_change` publishes with `skip=false`.

3. **Is the cost real on the owner's paths?** Yes. `present_retained_picture` runs whenever `chrome_present_pending` is set — hover marks on mouse move, caret blink, tween phases, pane notices. Cost is O(visible) GPU work plus a synchronous `queue.submit`/`queue.present` on the event thread (the same blocking calls that are GLM-P-2 / report-1-P-1). It is not O(document) and does no I/O, but it is a full present, and it feeds the queue pressure P-2 measures.

4. **Is the fix smallest-correct?** The direction is right, but it is not "one revision comparison" as the finding implies. `chrome_tick_reuses_picture` (`main.rs:116041-116054`) already gates on `presented_revision == content_revision`; what is missing is a signal for "did renderer-held state (chrome quads / blink / tween) move since the last present". Renderer state is written through `set_chrome` / `set_cursor_blink_visible` with no revision, so the gate needs a new chrome-quad/blink revision counter plus a "tween is running" bit — that is the smallest correct addition. The finding's caveat "or a blink/tween phase actually moved" is essential: skipping while a tween is animating would freeze it.

5. **Missed**: the no-op retained presents are produced upstream — `present_chrome_change` "requests a redraw whether or not it found a picture to re-queue" (`main.rs:115998-116000`). A gate at `present_seats_and_commit` stops the pass but leaves those requesters spinning a redraw/`present_retained_picture` loop; the cheapest cut is at the requesters (skip `request_redraw` when the renderer signature is unchanged), not only at the present door.

---

## P-3 · Wheel burst flushed synchronously in front of every non-wheel event — CONFIRMED

1. **Verdict**: CONFIRMED.

2. **Claim vs. code.** The claim: any non-wheel event flushes the accumulated wheel burst — full routing ladder, `settle_math_toggle`, thumb wake, then `repaint_pane_change` → a never-skippable publish+present — all before the triggering event is dispatched. Decisive lines:
   - `crates/bt-app/src/main.rs:111329-111334` — `if !matches!(event, WindowEvent::MouseWheel { .. }) && let Err(error) = runtime.flush_wheel()`.
   - `crates/bt-app/src/main.rs:95170-95177` — `flush_wheel` takes the burst and calls `self.mouse_wheel(burst.delta())`.
   - `crates/bt-app/src/main.rs:95839-95862` — `scroll_view_exact_in`: `settle_math_toggle` → `projection.scroll_by_subpixels` → `woke_terminal_thumb` → `repaint_pane_change(seat)`.
   - `crates/bt-app/src/main.rs:85839-85848` — `repaint_pane_change` → `publish_frame_inner(trigger, false)`, i.e. `skip_unchanged=false`.
   The ordering is deliberate and documented at `main.rs:111324-111328` ("whatever arrives next answers the window the wheel has already moved") — the finding correctly retracted its earlier "remove the inline flush" idea and I confirm that retraction is right.

3. **Is the cost real?** Yes. A keystroke or a pointer move landing while a wheel burst is pending pays the whole flush (O(visible) publish + synchronous present) before its own handler runs. Mouse-move is included: `pointer_moved` is a non-wheel event, so motion during/after a scroll gesture queues behind the flush. This is the concrete mechanism for "typing stalls while scrolling".

4. **Is the fix smallest-correct?** Mostly. Granting `skip_unchanged` to the wheel publish (change `publish_frame_inner(trigger, false)` → `true` at `main.rs:85844`) is one line and correct for a clamp-edge notch — the digest at `main.rs:64908` already knows content is unchanged. One overstatement: the finding says "clamp-edge wheeling costs nothing". `skip_unchanged` only skips the slot/present; the earlier work in `publish_frame_inner` (`refresh_search` at `:64738`, `refresh_projection`/`viewport_frame` at `:64771/:64783`, `schedule_visible_artifacts` at `:64817`) still runs. The fix removes the GPU pass, not the per-publish tax (which is P-4).

5. **Missed**: nothing material. The finding already notes the Wheel-station span also swallows DPI settling and defers that attribution split to the in-progress ticket; that is accurate.

---

## P-4 · Every present re-projects every unfocused pane; each re-clones up to 1,024 scrollback lines — PARTLY CONFIRMED

1. **Verdict**: PARTLY CONFIRMED. Mechanism real; scope overstated on three counts (multiplier, retained presents, screen taxonomy).

2. **Claim vs. code.** The real part:
   - `crates/bt-app/src/main.rs:100257-100287` — `redraw`'s per-unfocused-pane loop runs `refresh_projection` + `viewport_frame` + `absorb_printed_path_probes` + `schedule_visible_artifacts`.
   - `crates/bt-app/src/main.rs:64814-64818` — `publish_frame_inner` calls `schedule_visible_artifacts` on the focused shell.
   - `crates/bt-term/src/session.rs:7634-7646` — `schedule_visible_artifacts` is gated only by `primary_parked` / `decorations_allowed`, then unconditionally builds the `stable` vector and calls `schedule_live_artifacts`.
   - `crates/bt-term/src/session.rs:3643, 3651, 3662` — `live_detection_context()` is built *before* `live_detection_context_signature`; the per-candidate signature test at `:3662` is the cheap "nothing changed" gate the finding says arrives too late. Confirmed ordering.
   - `crates/bt-term/src/session.rs:5743-5802` — `live_detection_context` allocates a fresh `Vec`, clones `entry.line.text.clone()` + `frozen_cell_boundaries` for up to `LIVE_FENCE_HISTORY_CONTEXT_LINES` (1024) history lines **only when `live_screen == ScreenId::Primary`** (`:5745`), plus every visible grid row.
   - `crates/bt-term/src/session.rs:3705` — `resolve_live_detection_tasks(&mut new_tasks)` runs synchronously on the window thread; `crates/bt-detect/src/lib.rs:2763-2814` → `scan_live_math_blocks_in_context` over the whole context, but only when `new_tasks` is non-empty (it early-returns on empty `tasks.first()`).

3. **Is the cost real on the owner's paths?** Partly. It is a per-composed-publish tax (typing, PTY output, resize), O(visible rows) per pane per composed frame — not per retained present. It is *not* a mouse-move tax unless the move triggers a present. The 1,024-line scrollback clone is **primary-screen only**; on the traced run's alternate-screen workload (report-1: 2,205/2,458 frames `alt=1`) that clone does not run — only the visible-row clone does.

4. **Overstatements found**: (a) "twice per frame for each unfocused pane" — actually once per unfocused pane in `redraw`, plus once for the focused pane in `publish_frame_inner`; total = 1 + unfocused panes per composed frame. (b) "30,776 presents × (1 + unfocused panes)" — retained presents take `present_retained_picture`, which reuses `last_presented_frame` and never re-projects (`main.rs:100112-100134`); only the ~16,712 composed presents pay. (c) "(Claude Code-style TUIs are primary-screen)" — backwards: Claude Code is an alternate-screen TUI, so the 1,024-line clone is *skipped* there.

5. **Is the fix smallest-correct?** Directionally yes (gate on `grid_generation` / `live_rows` revisions / history length before building the context). Caveat: the current signature (`live_detection_context_signature(&inputs, ...)`) summarizes the built `inputs`, so it cannot be checked before the build; the proposed "cheap signature of the facts" is a new gate that must be proven equivalent, not a reordering of existing code. Moving the live scan off-thread needs the same revision fencing as the frozen path already has.

---

## P-7 · The 42.4 s "Keyboard" latency is a measurement artifact — CONFIRMED

1. **Verdict**: CONFIRMED.

2. **Claim vs. code.** The claim: `publish_pty_drain_frame` stamps the next published frame with a stale `pending_keyboard_at`, so a TUI that does not echo for 42 s produces a "Keyboard" record with a 42 s `occurred_at`. Decisive lines:
   - `crates/bt-app/src/main.rs:81835-81845` — `let keyboard_at = self.pending_keyboard_at;` then `occurred_at: keyboard_at.unwrap_or(now)` and `source: if keyboard_at.is_some() { Keyboard } else { PtyOutput }`.
   - `crates/bt-app/src/main.rs:81863-81867` — `sync_open` = a synchronized-update deadline exists; `if published || !sync_open { self.pending_keyboard_at = None; }` — cleared only on publish or outside sync mode.
   - `crates/bt-app/src/main.rs:85969` — `self.pending_keyboard_at = Some(Instant::now())` at `send_user_input`.
   So a keystroke into a DEC-2026 synchronized-update shell whose drain keeps skipping (`published=false`, `sync_open=true`) leaves the stamp standing; the next drain-published frame, for any reason, is tagged `Keyboard` with the old timestamp. The code does exactly what the finding says.

3. **Is the cost real?** It is not a runtime cost at all — it is a labeling error. The window presented fine during the gap (the finding's 161 retained Expose presents); the shell was silent. No synchronous I/O or blocking call is implicated. This redirects the keyboard-tail investigation from 42 s to ~230 ms (the queue-wait + per-publish tax), which is the useful consequence.

4. **Is the fix smallest-correct?** Yes. Clearing the stamp once the reader has consumed the keystroke's bytes (the reader thread knows the offset), or aging it out after one echo window, changes only the reported number, not behavior. Either is correct; the age-out is simpler and does not thread reader offsets into the app.

5. **Missed**: nothing. The finding correctly distinguishes this from the real defer mechanism (DEC 2026 150 ms cap) which fired zero times.

---

## P-9 · A DPI change re-keys every leaf of every tab before the first frame — PARTLY CONFIRMED

1. **Verdict**: PARTLY CONFIRMED. The swapchain recreate + shown-leaf reflow + compose are synchronous and real; the "every leaf of every tab" re-key synchronously is refuted — hidden-tab reflow is already deferred to the quiet boundary.

2. **Claim vs. code.** The claim: `reconcile_authoritative_dpi` recreates the swapchain (waits for GPU idle), re-derives grids for all leaves of all tabs, and composes — all on the window thread before anything shows. Decisive lines:
   - `crates/bt-app/src/main.rs:97547-97550` — `renderer.resize(...)` (swapchain recreate) then `resolve_seat_layout`.
   - `crates/bt-app/src/main.rs:97606-97615` — `resize_leaves_to_layout(...)` then `publish_frame(Expose)`.
   - `crates/bt-app/src/main.rs:83514-83537` — shown panes → `schedule_leaf_grid_change(..., LeafOnStage::Shown, ...)`.
   - `crates/bt-app/src/main.rs:83541, 83605-83659` — `resize_hidden_leaves_to_layout`: per hidden tab, `solve_tree` (pure) + `schedule_leaf_grid_change(..., LeafOnStage::Behind, ...)`.
   - `crates/bt-app/src/main.rs:18474-18476` — `if on_stage == LeafOnStage::Behind { return Ok(false); }` — `resize_at` (the vendor reflow) is **not** called for `Behind` leaves.
   - `crates/bt-app/src/main.rs:83678-83714` — `flush_pending_pty_resize` performs the deferred reflow + ConPTY resize at the quiet boundary, after the first frame.

3. **Is the cost real on the owner's paths?** The swapchain recreate and the *shown* tab's reflow + one compose are genuinely synchronous on the event thread and block until the GPU idles — real, and this is the DPI-move tail. But hidden tabs do **not** reflow before the first frame: they are re-solved (pure, O(seats)) and their grid change is queued, with the actual `resize_at` deferred to `flush_pending_pty_resize` at the turn boundary. The code comments at `main.rs:18406-18422` state this explicitly: "Behind schedules and does not reflow … arrives once per gesture rather than once per event". The finding's premise — that the multi-second re-key of every tab lands synchronously — is contradicted by the code it cites.

4. **Is the fix smallest-correct?** No — it is largely already implemented, and its proposed form would regress a documented fix. Moving hidden-tab resize to "on tab activation only" reintroduces the exact defect documented at `main.rs:83576-83584` (restored tabs born at a stale width until clicked), which the every-tab solve+queue exists to prevent. `activate_tab` already re-solves and reflows the tab it puts on stage (`main.rs:18420-18422`), so a tab is correct when shown; the remaining genuine cost is the synchronous swapchain recreate + shown-leaf reflow, which the finding does not isolate.

5. **Missed**: the deferred hidden reflow still lands at the quiet boundary and can stall a subsequent turn on a many-tab window; that residual (not "before the first frame") is the real multi-tab cost and is not what the finding's fix targets.

---

## Ranked summary

1. **P-1 — CONFIRMED, critical**: retained/chrome presents pay a full GPU compose+present of an unchanged picture; the fix needs a new renderer-state revision, not just the existing content gate.
2. **P-3 — CONFIRMED, high**: the wheel flush before every non-wheel event is real and is the "typing while scrolling" stall; `skip_unchanged` on the wheel publish is a correct one-liner that removes only the GPU pass.
3. **P-4 — PARTLY CONFIRMED, high**: unconditional context rebuild before the signature check is real, but the scrollback clone is primary-screen-only (skipped for the alt-screen TUI), retained presents don't pay it, and the multiplier is 1+N, not 2N.
4. **P-7 — CONFIRMED, medium**: the 42.4 s Keyboard record is a stale-stamp labeling artifact; measurement-only fix, no runtime change.
5. **P-9 — PARTLY CONFIRMED, medium**: swapchain recreate + shown-leaf reflow are synchronous, but hidden-tab reflow is already deferred to the quiet boundary; the proposed "lazy on activation" fix would regress a documented stale-width fix.

STATUS COMPLETE
