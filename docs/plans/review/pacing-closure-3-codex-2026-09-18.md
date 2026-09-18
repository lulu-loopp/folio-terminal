# Pacing leftovers closure review — 2026-09-18

Tree: `82d97f2b` = `78b5ee16` + `360fd833` + `82d97f2b`; read both `git show`s and the prior O4 report.
Scope: P1 debt and P2 hand-over only, plus O2/O3 unchanged check. Report written early on `docs/pacing-closure-3`.
Sequential `cargo test -p bt-app --bin folio FILTER -j 4`: baseline SELECTED/passed: `journeys_tests` 7; `pace` 36; `video` 21; `termscroll` 27; `autoscroll` 2; `wake` 12; `deadline` 8.
Temporary `journeys_tests` SELECTED/passed 9 (7 baseline + 2 observational probes); probes removed after passing. No other cargo commands run.

P1 — CLOSED (source/CPU check). Both doors reject 0/1/3/6.943/6.944 ms and admit 7/16 ms at 144 Hz; interval 6,944,444 ns.
The actual fold books a picture arriving at 3 ms for 9.944444 ms (wait exactly one interval); the supplied test's 6.944444 ms is an earlier eligible tick, not that fold's actual wake.
Debt is deadline-only; the fold reads it and the admitted strip tick has exactly one taker (`main.rs:85953`, `85344`). Removal changes membership.
Occlusion attack, source trace: arrival updates retained layers; an admitted tick transfers its debt to chrome/pending-frame work; `SkippedNotVisible` re-files that work without self-waking (`104147`, `104434`, `112496`).
`Occluded(false)` republishes once (`117116`); redraw clears pending chrome (`104177`), successful presentation records the signature (`103837`), and an unchanged repeat is suppressed (`present_gate.rs:77`). No lost or double-paid picture debt found.

P2 — OPEN; idle-count subclaim CLOSED: 1,000 paused services -> 0 hand-overs; real 64-frame ring, 1,001 services at 5 ms over 5 s -> 50 hand-overs.
Three empty-float passes return before their walks/allocations. These are hand-over counts, not a claim that the entire nonempty-seat service allocates nothing.
Event attack: divider drag -> `commit_seat_geometry` -> `resolve_seat_layout` -> unconditional refresh (`90063`, `66627`, `40729`); DPI/resize reaches the same layout path (`101267`, `101129`); palette adoption refreshes (`51059`).
Preview document scroll refreshes its own body (`58894`); video placement reads layout/FLIP, not document scroll (`66002`). No missing event invalidation found in these paths.
Endpoint counterexample (CPU geometry probe): paused picture, 400 px FLIP translation into a 500x500 box; last hand-over at 100 ms has clip x=49, landing at 200 ms requires x=0 (width 500 in both).
At landing, frames/membership/motion = false/false/false (`main.rs:85018`, `85061`); the next 1,000 unchanged services hand over 0 layers. The clip remains 49 px stale until another invalidation.
The tick pays pane debt then retires the tween (`85333`); chrome/overlay/pane-draw paths do not refresh video layers. `video_shape_of` retains `placement.clip` (`66025`), and `place_preview_image` updates only still images.
P2 needs final-geometry debt paid by a layer hand-over, without making an ended tween live again; restoring unconditional idle rebuilds would reopen the cost defect.

O2 remains CLOSED: `running_journeys` and `carry_live_journeys` are unchanged from `78b5ee16`; the 15-host endpoint table passes.
O3 remains CLOSED: input, compose, PTY drain, coalescer, synchronized-update completion and turn bodies are unchanged; turn still drains PTY before picture service (`104513`, `104580`).
Verdict: **P1 CLOSED; P2 OPEN — HOLD**.
Recorded for later: native occlusion/decoder recordings, CPU/allocator profiling, and all other issues; no full suite run, application launched, or process terminated.
Only this report is committed; no implementation changes.
