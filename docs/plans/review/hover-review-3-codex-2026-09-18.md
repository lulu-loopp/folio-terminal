# Formula hover review, round 3 — 2026-09-18

Candidate: a9868f5c (d8e897a8 + a9868f5c). Verdict: MERGE.
Scope: P1 reproduction, P2 timing, and raster ownership on failed completion in both orders.

P1 CLOSED: the exact phantom-prefix/inner-environment test passes its whole [2,0,0,0,0,0] vector.
The inner block is legitimately tried once before the recovered outer block establishes ownership.
P2 CLOSED: repeated round-2 paired timing probe, 200-row grid, 200 calls per arm, test profile.
Settled candidate prose: ownership gate disabled/enabled 2.57/2.45 ms per call; identical 3,282 allocations / 2,014,885 bytes.
Empty-map candidate control: 2.42/2.30 ms. Plain prose: 2.93/3.47 ms; empty map 2.95/2.98 ms (timing noise).
Both arms retain may_be_scanned(): settled candidates now return before the walk or scan-window construction.

Raster attack CLOSED: use normally fed P1 source, resolve genuine queued tasks for inner 4..7 and outer 2..8.
Complete inner with a synthetic raster: assert Ready, artifact present, and an actual 4..7 frame placement.
Then fail outer with Compile: accepted; outer Failed, inner Suppressed, artifact and stale_artifact both cleared.
Refresh projection: zero picture placements. Withdrawal is correct: the outer owns the whole range despite render failure.
Reverse CLOSED: complete outer with a raster first, then deliver the already-in-flight inner Compile failure.
The inner completion is rejected as stale; outer remains Ready with the identical raster key and 2..8 frame placement.
Suppression invalidates the inner candidate before worker_task_is_current; its failure cannot reach outer suppression.
Both orders settle at [0,0,0,0,0,0] subsequent attempts. No stale sub-range picture or outer-picture loss reproduced.
Evidence: session.rs apply_worker_completion/suppress_block_interior/worker_task_is_current; bt-detect DecorationRecord::suppress.

Validation: scratch bt-term lib 511; clean bt-term lib 509, lifecycle_matrix 46, bt-detect lib 162: all passed (-j 4).
Only the three authorized cargo commands ran; scratch product-file changes removed. Report is the sole committed change.
No application launched or process ended. No blocking finding; Recorded for later: none added in this bounded round.
