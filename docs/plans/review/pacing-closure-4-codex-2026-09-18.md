# Landing-frame closure review — 2026-09-18
Tree: `2cbad3e4` (= `82d97f2b` + `2cbad3e4`); read the commit and closure-3 report; scope: its single stale landing-frame counterexample.
Report written early on `docs/pacing-closure-4`; CPU probes and source trace only.
Tests SELECTED/passed, sequential: `cargo test -p bt-app --bin folio journeys_tests -j 4` (8 baseline, then 9 with one temporary probe); `cargo test -p bt-app --bin folio video -j 4` (21); `cargo test -p bt-app --bin folio float -j 4` (119).
Geometry — CLOSED: real PaneMotion plus animated_pane_viewports reproduce clip x=49, width=500 at 100 ms; the 400 px FLIP lands at x=0, width=500 at 200 ms. The old predicate retains x=49; the fixed decision hands over x=0, the second landing call hands over nothing, then 1,000 services hand over nothing.
The existing 5 ms real-host test also passes: exactly one landing snapshot, its predecessor is not resting, and FloatHost entrance/exit each hand over their endpoint once.
Two-call attack: both callers (`main.rs:104612`, `85617`; compose enters at `66811`) invoke the same service; no service return precedes the decision (`85010–85098`).
`mem::replace` consumes prior motion on the first landing call, which immediately hands over the resting layers and records picture debt; the second idle call neither clears layers nor debt.
`refresh_video_layers` stores the list (`main.rs:66115`); `set_video_layers` retains it and advances its revision (`bt-render/src/lib.rs:7965`); composition reads that list (`9053`, `10578`). The first call's layers are therefore the ones presented.
Guard attack: `sweep_video_seats` can return early internally, but its caller continues; the identical `anything_moving` guard includes cached animations and retained renderer layers. A stale held layer makes it true; it skips rebuilding only when all three stores are empty. No landing is consumed without handing over an existing picture.
Hidden landing: service has no visibility gate. Layers and picture debt survive; admitted ticks transfer debt to pending presentation (`main.rs:85376`, `85420`, `85985`); hidden failures re-file chrome/frame work (`104185`, `104472`); reveal publishes (`117148`).
The CPU probe also skips services from 100 to 500 ms: prior motion survives the gap, the first resumed call hands over the resting clip, and the second plus 1,000 idle services retain it. Native visibility behavior is source-traced, not exercised.
Overlap probe: real FloatHost opens at 100 ms and lands at 240 ms while PaneMotion lands at 200 ms; two services per 5 ms turn retain x=0 at the pane endpoint while the float keeps the OR true; exactly one hand-over at/after the final endpoint, then 1,000 services yield zero.
Verdict: **CLOSED — MERGE** (this counterexample only).
Recorded for later: all other issues, native recordings and profiling. No application launched or process terminated; no heavier cargo commands.
Only this report is committed; the temporary probe was removed.
