# Formula hover review — 2026-09-18

Candidate: c41b0cdf (one commit above main 51dd5f78). Verdict: HOLD.
Product code unchanged; scratch tests removed; no application launched. Locations: crates/bt-term/src/session.rs unless qualified.

P1 — source-face clipping defeats the new ownership gate (8387–8406).
Repro: freeze $$ / x^2 / y^2 / $$, render, turn to source, scroll down two rows.
The opener is above row 0; decorate_math_frame:9530 requires that exact opener
(frame_row_for_history:14899), so frame.math_blocks is empty despite visible body/closer.
Scratch: full source face repeats [0,0,0,0,0,0] renders; clipped face [1,1,1,1,1,1].
Must fix this remaining unbounded loop: retain ownership for a visible suffix, with invalidation live.

P1 — failed multiline rendering still retries forever (8062–8105, 8398).
The closer returns to None; only the owner becomes Failed (crates/bt-detect/src/lib.rs:512).
No Ready placement can refuse the next scan. Injected Compile failure produced an initial
attempt plus [1,1,1,1,1,1] repeated attempts, unchanged text/layout and no user action.
Inherited, but must close now under “fix it properly”; queue capacity bounds occupancy,
not lifetime work. Remember the failed proof until source/detector/layout invalidation.
Trace check: 641 frozen math_render_us lines, 638 source=12; its lone failure is NotDetected,
not evidence of a compiler failure. The injected failure independently proves this hole.

P2 — new ordinary-picture per-frame allocation violates release bar 4 (8387–8406).
A Ready multiline picture, no hover and no source face, still collects owned_rows and
searches it for visible/lookahead ids: O(blocks × candidate rows), plus record lookups.
Scratch placement-cleared control, identical cells: 39551 B/101 allocations versus
39487 B/100 per scheduler call: an extra 64 B/allocation even on this one-block frame.
Must fix: avoid this ownership work on the ordinary picture path; preserve clipped-source safety.

Other attack results / Recorded for later:
Face return, scroll out/in, DPI and detector re-proof passed scratch checks; resize passed
frozen_inline_math_survives_window_resize. Stale layout pixels are NOT Ready:
crates/bt-detect/src/lib.rs:483 demotes to None; theme also drops stale ink (session.rs:11743).
Two blocks sharing $$ closers, source-face theme change and live source-face edit passed scratch checks.
Scheduling uses the freshly projected terminal frame (crates/bt-app/src/main.rs:66708,103391),
not necessarily the presented frame. No intervening math/source mutation invalidates it;
queued completions still check candidate versions and input text (session.rs:8029–8082).
Thus the face flip does not itself admit stale geometry; no new stale-text regression found.
Hover (crates/bt-app/src/main.rs:87538,103799) asks the last presented frame before spending grace.
Misses arm 500 ms once; continuing misses cannot extend it; returning hits cancel it.
The deadline wakes an idle window; same_block retains the 90 ms mark journey, avoiding re-arrival.
Hit testing is O(visible blocks + failure placements) in one pane, not a scan of all cells
(crates/bt-render/src/lib.rs:7689); known-pointer animation frames pay it even over plain text.
No pointer: Option check plus revision read/write. Retained cursor-blink presents do not
advance the revision (crates/bt-app/src/main.rs:103201); no extra twice-per-second hit test.

Validation (all cargo invocations -j 4, only the four permitted commands):
bt-term --lib: 506 passed. lifecycle_matrix: 46 originals passed; both scratch probes passed after fixture checks.
Scratch caught/pinned the clipping loop; added hover tests inspect source strings and neither app filter selects them.
bt-app math_hover and schedule_visible: each ran 0 tests (4001 filtered); commands succeeded.
Lifecycle budget output (confirmation run; all existing budget assertions passed):
BT_RESIZE_BENCH sparse frames=200 sum_us=66724 shrink_p50_ns=465400 frame_p50_ns=224500 frame_p90_ns=855600 frame_max_ns=1999300 heap_bytes=128089299 heap_allocations=107372
BT_RESIZE_BENCH full frames=200 sum_us=30490 shrink_p50_ns=136300 frame_p50_ns=96000 frame_p90_ns=425900 frame_max_ns=820700 heap_bytes=35604923 heap_allocations=35073
BT_RESIZE_BENCH shape shrink_paired_p50=3.26
BT_SETTLE_BENCH prose history=3971 armed_before=0 armed_after=0 heap_bytes=2244719 heap_allocations=71 us=1561
BT_SETTLE_BENCH formulas history=3971 armed_before=0 armed_after=64 heap_bytes=3261024 heap_allocations=8025 us=3764
Other budget lines (allocations/bytes per cycle): TUI 120x40=16/21304, 120x80=16/21504;
IDENTICAL=636/398730; CARRIED on/off-band=644/566946,1018/760939;
RESTORED one/eight blocks=2974/1989859,3467/2199925. All within their asserted ceilings.
