STATUS COMPLETE - All four phases finished; nine findings re-verified against source and the fixed evidence prefix.

# Review P — where the window thread's time goes

Audit target: **2a261ebfb84409f3ab196a2dd71574ba7f776aee**, detached, `D:/Developer/bt-wt/perf-audit`. English/LF report. Source/log analysis only; no cargo build/check/test, application launch, tracked source edit, or commit. Notes: `target/review-notes.md`.

## Decision and top three

1. **P-1: synchronous swapchain presentation blocks the event thread.** Strongest measured explanation for pointer stutter: 111 of 150 Expose presents lasting at least 50 ms spend more than half their latency inside the call after submit. This also delays subsequent input delivery.
2. **P-2: every present rebuilds chrome text layouts and transient GPU geometry, including retained pictures.** The broad preparation bucket costs about 4 ms at the median and has 70–163 ms outliers. Cheap terminal row composition does not make the renderer cheap.
3. **P-3: the echo path repeatedly scans the whole visible screen for image references.** Enabled in this build, including alternate screen: each composed frame scans references twice, and each nonempty PTY feed performs additional whole-screen path/link detection. Concrete unnecessary CPU work; its milliseconds are not separately measured.

**Single first action:** take blocking acquire/submit/present off the window thread through one bounded, latest-frame renderer handoff (P-1). Preserve input/window/COM ownership and resize generations; never replace the stall with an unbounded frame queue. The trace already identifies this ownership problem. A frame-latency or present-mode setting change alone is an experiment, not a demonstrated fix for the 100 ms waits.

Remaining findings rank by relevance to this run first, then conditional stall potential. A severe scrollback bug ranks below an always-active cost because the measured focused pane had **zero projected frozen lines**. No critical finding is established.

## Phase 1 — evidence and measurement boundaries

Read `D:/Developer/trace/next67/{stderr.log,mouse.log,card.log,thumb.log,ime.log,notes.txt}`, all five PTY streams/chunk manifests, and the supplied `review3/perf-diagnostics.log` and `hang-20260916011136410.txt`. The latter files are under `C:/Users/Weiyi/AppData/Local/Temp/claude/D--Developer-BetterTerminal/ccea9546-63d0-4a20-ba77-75caa4e8533c/scratchpad/`.

next67 files were growing. Final statistics use the **first 3,070,381 bytes of stderr.log**, its initial observed size; SHA256 `262fb66fccebb991171160ac4257b2242dfdbb8c01b53737ea9d605abc874d9d`. Frame/present records are paired in log order. Quantiles use sorted samples at `floor(n*q)`, zero-based. These describe a fixed evidence prefix, not a benchmark.

| Source | Owner's supplied n / p50 / p90 / max, ms | Fixed-prefix n / p50 / p90 / max, ms |
|---|---|---|
| Keyboard | 275 / 19.4 / 97.2 / 296.9 | 298 / 20.335 / 111.517 / 296.867 |
| Expose | 673 / 5.9 / 77.5 / 223.2 | 1064 / 6.010 / 74.692 / 223.179 |
| PtyOutput | 387 / 10.6 / 24.0 / 122.3 | 1082 / 10.620 / 24.759 / 124.628 |
| Resize | 14 / 13.9 / 61.9 / 129.9 | 14 / 13.857 / 61.880 / 129.942 |

| Source | Event→submit p90, ms | Post-submit present call p90 / max, ms | Whole renderer p90, ms | Preparation (`rectangles_us`) p50 / p90 / max, ms |
|---|---:|---:|---:|---:|
| Keyboard | 83.084 | 30.253 / 104.808 | 45.766 | 3.993 / 5.092 / 163.463 |
| Expose | 12.147 | 48.729 / 122.639 | 72.174 | 4.026 / 4.880 / 119.713 |
| PtyOutput | 14.181 | 0.589 / 111.363 | 13.033 | 4.232 / 5.020 / 70.707 |
| Resize | 44.894 | 16.087 / 115.550 | 28.742 | 1.088 / 3.621 / 4.014 |

Percentiles are not additive. Post-submit duration is computed per record as `event_to_present_us - event_to_submit_us`. At `crates/bt-render/src/lib.rs:9632`, submit returns before its timestamp; the next substantial call is `gpu.queue.present(texture)` (:9635), then the second timestamp (:9640). This measures call completion, **not physical scanout**. The subsequent DirectComposition commit (`crates/bt-app/src/main.rs:99963`) is outside the receipt. Frame `digest_us` is diagnostic work after `total_us` is sampled (`bt-render/src/lib.rs:9658–9662`), not a comprehensive application-composition measurement.

Corrections to the premise:

- Expose is not stamped at CursorMoved. `present_chrome_change` stamps it after hover work (`crates/bt-app/src/main.rs:87673`); retained redraw stamps it at retained presentation (:100060, :100135). It also labels animations/caret/other exposes. Thus high Expose p90 does **not** prove pre-frame delay, nor measure all mouse-handler time.
- Keyboard normally starts at `send_user_input` (:85969), after routing/provenance/return-to-live. The next published output consumes that stamp (:81834). It includes input-queue wait, ConPTY, child/TUI processing, output-ring wait, Folio work and rendering. Later input overwrites it; mouse reports also set it (:85928). Cursor reveal can produce a Keyboard frame before echo (:95930). It is not a one-key/one-echo correlation.
- PtyOutput starts after drain (:82375–82381), omitting preceding parse/read work. Resize starts after surface resize/DPI reconciliation (:97298). Felt latency can be absent from all four metrics.
- Every focused projection record has `lines_measured=0`, `projected_lines=0`; maximum refresh is **39 us**. Of 2458 paired frames, 2205 have `alt=1`. This excludes focused frozen-history relayout as the sampled tail cause, but not uninstrumented other-pane work.
- Initial PTY prefixes contain 8,534,304 / 2,035,760 / 1,944,722 / 170,764 / 307 bytes; largest reader chunks are 6401 / 5006 / 4578 / 5317 / 91 bytes. The first four contain 1553 / 484 / 495 / 50 matched DEC 2026 open/close sequences. These are reader-arrival boundaries, not application feed boundaries or key receipts. No key/send timestamps join them to stderr. Assigning a 294 ms pre-render Keyboard residual to ConPTY, Claude, or Folio alone would be fabricated.
- Historical diagnostics mix builds: :5968 starts 0.4.1 (2a261ebfb8); :5985 bills 1183 ms to WebPage; :5988–5990 bill 2537/1104/1094 ms to Wheel. **Faults +0 does not establish pure CPU**: COM/driver/kernel waits and scheduling can also have zero faults. Older extreme holds are not transplanted onto this build.
- The supplied close-time hang is 0.4.0 (552487867c), with 5.641 s silence. Unsymbolicated candidate return addresses can include stale stack entries. It proves an unresponsive pump, not a specific hot function here. next67's sole >500 ms prefix hold is startup (:72), 1949 ms and +87888 faults, not steady-state typing.

## Phase 2 — window-thread path map

References below without a crate prefix are to `crates/bt-app/src/main.rs`.

### A. Keystroke through echo

1. :111329 flushes queued wheel work before every non-wheel window event. :111389 dispatches `keyboard_input` (:95874), filtering synthetic/releases, handling ownership/shortcuts/overlays, revealing the caret and dismissing hover UI. Caret reveal can call full `publish_frame` at :95931 **before sending the key**. Ordinary terminal encoding at `crates/bt-app/src/input.rs:630` is key/modifier mapping plus small byte allocation, not a document scan.
2. :96767 answers attention; :96769 calls `note_user_typing` → session → ledger flag. Its lookup is P-8. Command text is absorbed at shell-integration boundaries (`crates/bt-term/src/session.rs:4280`), not persisted per character. There is no always-on terminal search-index update in send; search is the gated P-7.
3. `send_user_input` (:85957) returns a scrolled view to live, stamps input and writes. `crates/bt-pty/src/lib.rs:1467` calls `InputRing::try_push` (:956): short mutex, byte copy and wake; full queue returns InputRefused rather than waiting for pipe space. The writer spawned at :1438 owns `pump_pty_input` (:1022), the `write_all`/`flush` caller. **A key does not synchronously write a full ConPTY pipe on the reviewed UI path.**
4. ConPTY/child output reaches the reader (`bt-pty/src/lib.rs:412,1429`), then the bounded OutputRing and a wake. The app coalesces wakes with an atomic bit (:10888). AppEvent::PtyOutput itself does nothing (:111000); about_to_wait drains. A full output ring blocks the reader, not directly the UI. Dumps run on the reader with deferred flushing; this can perturb timing but is not per-key UI disk I/O.
5. `drain_pty` (:82167) visits every tab; `drain_tab_pty` (:36001) every leaf, including hidden ones. Each takes up to **256 KiB** (:35628; `bt-pty/src/lib.rs:65,842,1528`), concatenates reader chunks and calls `session.feed_at` (:2891). A byte cap already exists; it is not a wall-time budget. The existing drain-time-slice ticket owns that issue.
6. `feed_at` runs VT parsing, event application, damage/staging updates (`bt-term/src/session.rs:2960–2976`), path reconciliation (:3003) and repaint identity work synchronously. Capture/finalization (:10225,:10468) builds frozen source/anchors/styles, detects frozen image paths and schedules detection (:10530). Damage tracking fingerprints changed rows (:6675). Formula typesetting/path verification go to workers. P-3/P-5 describe avoidable per-chunk/publication work beyond the ticketed drain budget.
7. `publish_pty_drain_frame` (:81834) selects Keyboard/PtyOutput. `publish_frame_inner` (:64689) refreshes open search, dispatches tasks, refreshes projection (:64771), builds the viewport (:64785), scans references (:64840), composes preedit, offers IME geometry and publishes to a latest-frame slot. The unchanged gate (:64908) occurs **after** that work. `redraw` (:100187) also reconstructs each visible unfocused terminal frame (:100265), then renders/presents synchronously.
8. IME: `offer_ime_caret` (:60068) returns when inactive; otherwise geometry goes through `ime_cursor_throttle`. `flush_ime_cursor_area` (:81922) only applies due changes; OS IME/system-caret calls are at :81912. No document scan found.
9. Math tools: every turn calls :85287, but `sync_math_tools` (:85097) exits immediately with no tool/hover anchor. Otherwise :85207 searches presented pane/formula geometry, hit-tests, and updates follow/fade. O(visible formulas), not O(scrollback), and no formula typesetting. No measurement makes it the 100 ms typing cause.

### B. Mouse motion

:111426 → `pointer_moved` (:86196) handles web forwarding, rails/thumbs/drags, overlays, chrome (:86686), tooltip/file/layout-peek intents, command rail (:86769), hovered pane (:86794), math (:86800), hyperlink (:86806), image underline and peek. Multiple branches repeat chrome hit resolution; P-4 identifies substantial hidden allocation.

Chrome hover has an equality return (:87483); math updates only on anchor change (:85048); pane references rescan on pane entry (:84086), not every move. **A plain move within unchanged terminal content does not unconditionally refresh chrome or present.** Crossing a file row/tab/card/link/tool can rebuild whole chrome; hover/fade timers create additional frames. `present_chrome_change` (:87663) queues a retained terminal frame and requests redraw, not an immediate present per input event.

`crates/bt-render/src/lib.rs:7298` reverse-scans visible math failures/blocks and clones a successful anchor, without constructing a list of every formula. `crates/bt-viewport/src/lib.rs:957` starts at the hit cell, resolves a link span and allocates URI/id; it does not scan scrollback links. Command-rail rebuilding (P-9) is confined to its band and disabled on alternate screen (:41213; `cmdrail.rs:901`). Tab/card hits construct tab-level lists (:87053); files hits reconstruct expanded trees (P-4). The known card-wheel walk is excluded.

### C. Terminal wheel

`queue_wheel` (:95131) merges compatible deltas; incompatible deltas flush. Pending bursts flush before non-wheel events (:111329) and at turn entry (:100470). Coalescing is within the event burst, not arbitrary future turns. One routing per raw event in a trace is compatible with this design.

`scroll_rail` (:94759) is the tab/card rail, **not terminal history**. Terminal routing (:95642) sends reports/arrows to the PTY in the relevant TUI mode, or reaches local `scroll_view_exact_in` (:95839), updates subpixel offset (:95855) and republishes. Offset arithmetic is O(1); projection work is not. P-5 covers history/artifact walks, P-6 intersecting frozen lines. The height index seeks the visible range (`bt-viewport/src/lib.rs:2880`) and breaks at its end (:3105). The final presented `row_map` is viewport-sized; it is not the O(scrollback) component.

### D. Web preview / WebPage station

`advance_web_page` (:98597) exits with no web seats, otherwise checks orphan panes, shortcut claims, deadlines and outcomes. Claims are equality-gated (`webhost.rs:2108`); tick is deadline-driven (:2823). **No nested WebView2 pump, JS evaluation loop or screenshot-completion wait runs in an ordinary tick.** Callbacks drain through WebSeat::drive (:2119); install/navigation effects are at :2521. Capture requests are asynchronous (`bt-platform/src/webview.rs:2866`); returned PNG jobs go to page_shrinker (main :98546). Inspected next67 thumb summaries show no page captures.

Retirement does have synchronous IPC: main :98672 → `webhost.rs:2667` → `bt-platform/src/webview.rs:2978`, controller Close. The deadline is set after Close returns and cannot bound it. Neither the supplied raw close stack nor the 1183 ms wide station proves that call caused that particular hold. It is not promoted to a new measured finding. If Close is measured slow, proper isolation needs a browser-owning STA/pump with marshalled commands; an arbitrary worker cannot safely take an apartment-bound controller.

`hang_watch::at(WebPage)` (:98598) is **not restored on return**, even in the empty-web branch. Later strip animation, watches, persistence and hover/fade work can be billed there until the next station. Present and Drain are also sticky. The already-ticketed station split should preserve scoped attribution across these tails. A one-second WebPage label is not evidence of one second of browser work.

### E. Resize / historical two-second publish

`resize` (:97243) updates compositor/surface, reconciles DPI, solves seat layout, resizes shown leaves, updates keys, publishes and redraws synchronously (:97336). Hidden geometry is solved too (:83479), but `schedule_leaf_grid_change` (:18474) queues hidden changes without immediate local reflow. Later :83707 releases due PTY resizes for all leaves; PTY resize is still synchronous (`bt-pty/src/lib.rs:1476`). Not every hidden grid reflows on every size event.

`bt-term/src/session.rs:3149` captures reflow witnesses, resizes the terminal, reconciles decorations/anchors and updates layout keys. `invalidate_layout` (:10859) walks records and enqueues work: **formulas are not all typeset synchronously**. Whole-history synchronous projection measurement at a new key is P-5; visible-line materialization is P-6. Redraw repeats projection for other visible panes (main :100265).

The historical 2 s Present station cannot be assigned to those operations from these logs. It is not scoped to publication and may cover later rendering. next67's worst Resize (:295) is 129.942 ms, including **115.550 ms after submit**, with zero projected frozen lines. That sample supports P-1, not two-second frozen layout/formula typesetting.

### F. Turn scheduling

`about_to_wait_inner` (:110478) settles app-level work then turns **every window** (:110621), sharing a timestamp captured before the loop. `turn` (:100465) flushes wheel/DPI/picker results, drains all PTYs, advances notices/watches/web/animations, flushes session state, settles IME/resize/math/hover, and releases PTY resizes. Input queued during this waits. Worker-ready events apply batches synchronously before later input (:111033–111048). Wheel work explicitly precedes key dispatch; cursor-reveal composition can precede key enqueue.

Deadlines combine at :100717/:100927; the outer loop uses Wait/WaitUntil (:110641), and an empty window registry uses Wait (:110608). A long turn can exhaust a deadline calculated from the earlier time and cause a catch-up turn, but that alone proves no perpetual spin. No new unconditional Poll/no-work redraw loop established. Existing macOS occlusion/drain tickets are excluded. Windows GPU timings are not measurements of macOS driver behavior.

## Phase 3 — numbered findings

### P-1 — high — acquire/present waits run on the event thread

**Locations:** `crates/bt-app/src/main.rs:100337`; `crates/bt-render/src/lib.rs:9632`, :9635, :9701; swapchain latency configuration :4506.

**Trigger/work:** any composed or retained present. Acquisition and queue presentation synchronously occupy the winit owner thread: blocking GPU/platform work, not row composition. Latency=1 may increase backpressure, but the trace neither identifies the driver/DWM cause nor proves that setting alone is wrong.

**Measured cost:** Expose stderr :3986–3987 takes 128.209 ms: only 5.570 ms through submit, then 122.639 ms in present. :3334 is 133.790 ms with 118.828 ms after submit. Resize :295 spends 115.550 of 129.942 ms there. The surface configure/acquire/view-creation bucket reaches 95.413 ms for Expose (`bt-render/src/lib.rs:9122-9139`); it does not isolate acquisition alone. Among >=50 ms samples, present exceeds half of latency for 111/150 Expose, 27/69 Keyboard, 65/75 PtyOutput and 1/2 Resize. This directly explains many stalls with otherwise cheap row composition.

**Smallest correct fix:** isolate the surface/GPU lane behind a capacity-one latest-frame handoff and completion wake; maintain resize/surface generations and resource ownership, leaving UI/COM-required work on its owner. Never synchronously wait for the renderer in an event callback. Platform frame-readiness admission could be narrower if it eliminates the waits, but must not itself wait on the UI. Merely changing FIFO/latency flags cannot promise removal of driver-call stalls.

**Expected effect:** protects keyboard/mouse dispatch and PTY draining from 30–123 ms holds for all four sources. GPU stalls may persist on the renderer; faster physical presentation is not promised. Idealized removal of only post-submit time leaves the recorded event→submit p90s at 83.084/12.147/14.181/44.894 ms (Keyboard/Expose/PtyOutput/Resize): an illustration of the component, not predicted post-fix percentiles. Keyboard's 296.867 ms maximum has only 0.224 ms after submit and needs separate attribution.

### P-2 — high — retained redraws reshape chrome and recreate GPU buffers

**Locations:** `crates/bt-render/src/lib.rs:8803`, :11892–11941, :8771, :8518–8573, :8695–8702; retained caller `crates/bt-app/src/main.rs:100139`.

**Trigger/work:** every present, including an unchanged terminal picture with a hover/fade/caret change. `shape_chrome_labels` creates a cosmic-text Buffer, sets text and shapes each drawable label. Preview paragraphs are also shaped anew. Ground/ink/status/math-overlay and chrome vertex buffers are recreated; empty per-seat status/math overlays even allocate dummy buffers. Icon lists are cloned/partitioned, then cloned again in `prepare_chrome_icon_draws` (:10013). Terminal row-cache hits cache none of this. O(drawable label/preview text + visible geometry), with repeated CPU allocation, shaping and GPU resource creation.

**Cost/evidence:** `rectangles_us` measures this broad preparation interval (:9694), not just rectangles. Fixed-prefix medians are about 4.0–4.2 ms for Keyboard/Expose/PtyOutput, maxima 163.463/119.713/70.707 ms. Keyboard stderr :1070–1071 has 163.463 ms here in a 177.819 ms renderer / 272.146 ms event. These are measured **bucket costs, not shaping-only timings**; scheduling/driver allocation waits within the bucket remain indistinguishable.

**Smallest correct fix:** retain shaped chrome/preview text by text/font/width/layout revision; separate position, clip, hover color and opacity where they do not change shaping. Reuse growable instance/vertex buffers, upload changed ranges and omit empty layers. Invalidate on DPI/font/content/width changes; share caches between composed and retained presentation.

**Expected effect:** reduces recurring preparation for Keyboard, Expose and PtyOutput; smaller benefit on the 14 Resize samples, where layout genuinely changes. The ~4 ms median bucket is an upper bound on removing the entire bucket, not a guaranteed saving. It cannot remove P-1 waits by itself.

### P-3 — medium — whole-screen image/path detection repeats on feed and twice on frame publication

**Locations:** `crates/bt-term/src/session.rs:3003`, :9694–9708, :9845, :9892, :9934, :7618, :2854; `crates/bt-app/src/main.rs:64840`. Defaults: app :34112 and session :687.

**Trigger/work:** any nonempty PTY feed, including small echo/repaint chunks, and every publication attempt, including one later discarded as unchanged (:64908). Reconciliation builds logical strings/boundaries for all visible rows and detects paths, then scans all rows again for OSC image links. `viewport_frame` calls `decorate_image_reference_affordance`, scanning frame references; the app immediately repeats `frame_image_references` for hover storage. The scan reconstructs text/cell maps (:2743) and detects paths (:2809). Default image bands are off, so their alternate-screen exclusion does **not** disable this work. Row-capture caching does not eliminate these string/boundary/detection passes.

**Estimated cost:** at least two live traversals plus two frame-reference traversals per feed-and-publish cycle, O(visible cells/text) with temporary vectors/strings; matches additionally search stored image records. At an illustrative 7000-cell viewport, roughly 28,000 cell visits before row fingerprinting and other viewport work, even for a one-character change. Source proves the work; milliseconds are not isolated. This is pre-render work, not the 100–400 us digest, and cannot honestly be assigned all of Keyboard's residual.

**Smallest correct fix:** compute frame references once and return/cache them with the frame for both decoration and hover. Key live path results by logical-line content and CWD/link/settings revisions; scan damaged logical lines and wrapped neighbors only. Preserve reflow/screen-switch/hyperlink/verification invalidation. On caret reveal (`main.rs:95930`), enqueue accepted terminal input before optional full composition and reuse the retained picture when only visibility changes.

**Expected effect:** reduces Keyboard/PtyOutput work on this alternate-screen workload and composed Expose/Resize work. Pure retained Expose avoids publication and mainly benefits from P-2. No numeric reduction is established.

### P-4 — medium — pointer hit-testing rebuilds expanded files rows before rejecting the point

**Locations:** `crates/bt-app/src/main.rs:86319`, :87440, :87347–87349, :67845, :16666–16670; `crates/bt-app/src/files.rs:563`, :606–639.

**Trigger/work:** pointer motion whose chrome query reaches the files fallback, even over terminal content. `files_tree_contents` walks every files seat and `tree_view` recursively reconstructs all expanded cached rows, cloning names/keys and allocating maps/vectors, before `hit_files_tree` tests geometry (`seats.rs:18068`). The normal path queries for both the free-body gate and chrome hover, so it can happen at least twice per unchanged move. O(all expanded cached rows), not O(visible rows). The 2000-entry per-directory cap (`files.rs:50`) does not cap their expanded sum. This walk does not perform directory I/O.

**Estimated cost:** F expanded rows implies at least 2F row constructions on that path, generally two owned strings per file row. F=10,000 implies ~20,000 row constructions and ~40,000 string allocations per motion, before subsequent chrome rebuilds. This is an operation estimate, not an observed tree size or millisecond measurement; the trace does not establish F.

**Smallest correct fix:** reject by seat/body rectangles first, then query only the hit files seat. Cache flattened tree rows by directory-cache/open/edit revisions, share between paint and hit-test, and index with existing scroll/row geometry. Reuse the resolved target within one event.

**Expected effect:** improves pointer response with populated files columns and reduces work ahead of Keyboard/PtyOutput/Resize. Much precedes Expose's timestamp, so recorded event→present may barely change despite better interaction. No contribution claimed with no populated files column.

### P-5 — high — projection walks all unchanged history; a cold width remeasures everything

**Locations:** `crates/bt-term/src/session.rs:8207–8300`; `crates/bt-viewport/src/lib.rs:4236–4255`, :4300–4339, :4482–4500, :3904–3921; other-pane caller `crates/bt-app/src/main.rs:100265`.

**Trigger/work:** terminal wheel, echo/publication, or a new layout key with frozen history. `project` always allocates next-ID/entry vectors, visits every entry and compares the full old ID prefix before discovering no change. Artifact synchronization reconstructs whole maps too. At an uncached width/font/DPI key, `relayout` immediately projects, measuring every retained line's columns/wrap count and rebuilding height indexes. `sync_projection_state` then calls project again. O(scrollback entries) on a warm unchanged frame; O(total frozen text) on cold reflow, independent of the visible window. Distinct from the ticketed toggle-animation remeasurement.

**Estimated cost:** H=100,000 unsuppressed lines implies 100,000 entry visits and ~1.6 MB of ID/reference payload in the two temporary vectors per pass on 64-bit, excluding capacity overhead. Cold widths additionally traverse all source text, potentially multiple times. No benchmark run. **Focused next67 contribution is negligible: zero frozen lines, <=39 us refresh.** Historical station labels alone cannot assign their seconds to this code.

**Smallest correct fix:** gate before enumeration using document structure/source, layout and artifact revisions; incrementally append/remove/update entries/heights. Retain width-independent measurements and wrap checkpoints; perform remaining cold-history layout in bounded background/incremental work, publishing consistent generations and preserving the reading anchor. Never use partially updated height indexes for input/hit-testing.

**Expected effect:** makes unchanged-history wheel/typing work independent of H and removes large cold-reflow spikes from Resize. Helps Keyboard/PtyOutput/Expose with substantial primary history or another visible pane; should not materially change this prefix's focused alternate-screen projection.

### P-6 — high — one visible row of a long wrapped line materializes the entire line

**Locations:** `crates/bt-viewport/src/lib.rs:2994`, :3085–3096, :3604–3623, :5352–5455.

**Trigger/work:** scrolling within a long logical frozen line with wrapping enabled. Height lookup calls `layout_frozen_line` just for a row count; visible materialization calls it again. It builds owned cells/anchors for every grapheme/wrapped row, then the caller validates all rows before skipping/taking the visible portion. The outer history iterator is window-bounded, but an intersected logical line is not. O(entire logical-line length), with per-cell allocation per notch, rather than O(visible cells). The unwrapped horizontal-window branch already avoids this particular full materialization.

**Estimated cost:** a hypothetical 400,000-character ASCII logical line at 100 columns creates ~4000 rows and 400,000 cells/anchors per full materialization, even with only 40 visible rows. The default staging quota is 4096 source rows (`bt-transcript/src/lib.rs:11`); quota enforcement finalizes/splits a continuing line (:1416-1424). This bounds growth but still permits this large amplification; the finding is not an infinitely growing logical line. Not evidence that such a line occurred tonight. Warm line-count cache hits do not cache these VisualRows.

**Smallest correct fix:** use cached source-row counts for ordinary text height lookup; cache wrap-start checkpoints by line generation/width and materialize only intersecting row ranges. Restore style/link state from checkpoints and bound the materialized-row cache. Preserve grapheme/wide-cell and wrapped-link semantics.

**Expected effect:** removes long-line wheel stalls and interference with all four sources; helps Resize when the viewport intersects that line. No benefit claimed for this measured alternate-screen case.

### P-7 — medium — open search recompiles/rescans on publication and appends invalidate all history hits

**Locations:** `crates/bt-app/src/main.rs:64738`, :42260, :42292–42343; `crates/bt-app/src/search.rs:855`, :887, :990.

**Trigger/work:** nonempty terminal search open during typing/output/scrolling, or query edits. Each refresh recompiles before cache checks, clones cached historical hits, captures/converts all live rows and scans volatile text before discovering no change. History identity includes frozen length/front/back: one append causes all frozen lines to be searched again. Query changes require new results, but a full synchronous scan per typed character is avoidable. O(visible+staging text+cached hits) on a hit; O(all frozen text+matches) on append/query miss.

**Cost:** unmeasured; a B-byte history is fully scanned for each such miss (B=10 MB implies a 10 MB scan), plus match allocations. No active search cost asserted for Claude's TUI: closed search returns at :42204, alternate terminals are excluded at :42020.

**Smallest correct fix:** cache compilation by flags/query revision; check content revisions before cloning/capturing. Incrementally append/evict history matches, share hit storage, and run changed-query full searches off-thread against immutable revisions with stale-result rejection. Search-field editing stays immediate.

**Expected effect:** lowers Keyboard/PtyOutput tails with open primary search and redundant work on Expose/Resize. No expected change to the dominant alt=1 sample.

### P-8 — medium — provenance marking searches old command marks on every key

**Locations:** `crates/bt-app/src/main.rs:96769`, :85950; `crates/bt-term/src/command_marks.rs:378`, :442–444.

**Trigger/work:** user input while a ledger command is open. `note_user_input` calls `open_mark_mut`, searching `marks.iter_mut().find(...)` from the beginning, then sets an already-true flag. The open command is normally last. O(retained command marks) per key/IME commit/paste, not command parsing or persistence. An executing TUI can keep the shell command's open mark until OSC completion; no open mark makes this O(1).

**Estimated cost:** C marks implies up to C ID comparisons per key; C=10,000 at 20 keys/s implies ~200,000 comparisons/s plus memory traversal. Not a demonstrated 100 ms stall. Initial TUI prefixes contain only three OSC 133 markers each, so no evidence establishes large C tonight. The normal Keyboard timestamp is taken **after** this work.

**Smallest correct fix:** keep a stable open index/ID map updated on retirement, or use the last mark with a checked ledger invariant. Avoid scanning and repeated marking after the first real input; preserve provenance for restored/program-generated input.

**Expected effect:** removes history-dependent pre-send Keyboard/IME work. Recorded event→present may not change because it starts later. No direct Expose/PtyOutput/Resize benefit.

### P-9 — medium — command-rail hover rebuilds history before the cache check

**Locations:** `crates/bt-app/src/main.rs:41672–41703`, :41632, :41234; `crates/bt-app/src/cmdrail.rs:955`, :1079–1145, :1320–1352.

**Trigger/work:** motion inside a primary-screen command rail. After cached-band rejection, `settle_command_rail` reconstructs command/search stacks and `resolve` lays out folded/expanded alternatives. Only afterward does the caller ask `needs_rebuild(key)`. Staying on one tick still incurs O(all retained commands/search hits) work and vector allocation. Fisheye relayout is bounded (`FISHEYE_RELAYOUT_CAP=2`), not an infinite loop.

**Estimated cost:** C commands means C Entry constructions plus one or more complete scans/layouts per in-band motion. 10,000 commands at 100 motions/s implies at least one million Entry constructions/s. Scaling estimate only; no measured C/time. Plain terminal-body motion and alternate screen reject early, so it does not explain ordinary Claude alt-screen hover.

**Smallest correct fix:** cache the stack/folded layout by command/search revision and geometry; reuse expanded layout while within its holding band. Recompute expansion only when its target changes. Check the cache before building the expensive data.

**Expected effect:** improves primary rail hover and reduces CPU interference with the other sources. Expose timing can miss much of the improvement because it starts after hover resolution.

## Attribution limits and existing tickets

Keyboard stderr :906–907 takes 296.867 ms with 2.662 ms in the renderer: ~294.205 ms precedes it. Expose :1785–1786 takes 223.179 ms with 6.014 ms in the renderer: ~217.165 ms precedes it. These real residuals do not prove an O(history) scan. The logs lack joined handler-entry/enqueue/reader-arrival/drain spans to partition them. **Do not close the typing investigation on P-1 alone.**

Existing drain time slicing remains important: per-leaf byte limits cannot bound a whole turn across all panes/windows. Card-wheel walk, drain slicing, station split, toggle-animation remeasurement and macOS occluded redraw spin are deliberately not duplicate findings. No new multi-second web algorithm is established; the close-time hang is not a typing/hover stack.

After the first action, retain event identity/handler-entry time through enqueue, reader arrival, drain start/end, publication, acquire, submit and present, with scoped station restoration. These spans are required to assign the remaining latency, not substitutes for the confirmed fixes.

## Phase 4 — verification ledger

Completed by reopening implementation lines, callers and early-return gates after drafting:

| Finding | Re-verification and limiting condition |
|---|---|
| P-1 | Synchronous renderer caller and submit/present timestamps; configure/acquire bucket includes surface setup and view creation. Windows call duration is not scanout or a macOS measurement. |
| P-2 | Fresh Buffer creation/shaping and transient geometry on composed and retained paths. Broad preparation time is not a shaping-only measurement. |
| P-3 | Feed reconciliation, frame decoration and second app reference scan; default image settings keep this active in alternate screen. |
| P-4 | Both pointer queries and eager files-tree argument before geometric rejection; ordinary terminal body passes the earlier chrome gate. Empty files state limits cost. |
| P-5 | Unconditional history vectors/prefix comparison, cold relayout and artifact sync; focused traced projection is empty. |
| P-6 | Complete wrapped-line layout/validation precedes visible clipping; horizontal mode differs. Reopened the 4096-row staging quota and constrained the illustrative line size accordingly. |
| P-7 | Compilation/cache cloning/live scan before unchanged return; closed search and alternate screen reject early. |
| P-8 | The input flag calls a linear mark lookup; no open command returns immediately. No evidence of a large mark count tonight. |
| P-9 | Full stack/resolve precedes needs_rebuild; cached-band and alternate-screen gates limit reachability. |

Re-read timestamp sites, asynchronous PTY writer/reader ownership, wheel coalescing, resize scheduling and sticky station boundaries. Recomputed the pinned stderr SHA256 and all 2458 frame/present pairs, with zero source mismatches; all four n/p50/p90/max values match the report. No new fact assigns the remaining pre-render latency to a single component.

Final worktree check: HEAD remains `2a261ebfb84409f3ab196a2dd71574ba7f776aee`; tracked diff is empty. The requested report is the only untracked git-status entry; `target/review-notes.md` is retained. Both report and notes were written with LF. No cargo command, application launch, source modification or commit was performed. Validation was read-only source/log analysis, not a runtime benchmark.
