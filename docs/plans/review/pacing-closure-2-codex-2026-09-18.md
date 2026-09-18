# O4 pacing closure review — 2026-09-18

Tree: `78b5ee16`; read `git show` for `5968b861`, `6c9c4977`, `78b5ee16` and `docs/pacing-closure`'s prior report.
Scope: O4 plus O2/O3 regression. Report written early on new branch `docs/pacing-closure-2`; no application launched or process terminated.
Sequential `cargo test -p bt-app --bin folio FILTER -j 4`, baseline SELECTED/passed: `journeys_tests` 5; `pace` 36; `video` 21; `autoscroll` 2; `toast` 26; `tooltip` 44; `wake` 11; `deadline` 8.
`video`, `autoscroll`, `toast` repeated to recover truncated counts: same selections, all passed. Temporary `journeys_tests` SELECTED 8 (5 existing + 3 observational probes), all passed; probes removed.

Verdict: **O4 OPEN — HOLD**. The original starvation probes close, but the split's debt scheduling and idle-service cost do not.
Decoded animation: CLOSED. Real 64-frame host, 1,001 refused samples at 5 ms through 5 s: every frame equals its clock, final index 50; gated control index 0.
Drag: CLOSED. 100 integrations at 5 ms travel 125 px in 500 ms; 31 at 16 ms travel 124 px in 496 ms; old wholly refused schedule permits 0 integrations/0 px.
Duplicate-now attack: 1,001 second animation advances report no change; 100 zero-elapsed drag repeats add 0 px. Toast expiry and final retirement each change once, second call false; float exit sweep likewise changes once.
Only pictures repeat in `carry_live_journeys` (`main.rs:85543`); drag/toasts/float/resize/directory/git services are not called there. State clocks precede their gates (six advancers checked by the existing test).
Decoder source: `Engine::frame` consumes each generation once (`bt-platform/src/video/engine.rs:523`); a new asynchronous generation is new input, not a duplicated advance. No real decoder/window instantiated.
Float source: resize writes only unequal frames (`main.rs:80643`); directory/git asks mark pending before dispatch (`79780`, `79747`), so unchanged state does not issue them twice.

P1 — picture debt has no independent wake (`main.rs:85085`, `85307`, `85690`, `104762`).
Clock/source counterexample: last strip tick and present at t=0, 144 Hz display; a final/paused decoder picture arrives on an otherwise idle, non-publishing turn at t=7 ms.
Display gate admits, but the strip's 16 ms guard returns before taking `pictures_owe_a_frame`; other idle advancers admit and record no refusal. With playback/bar/other clocks settled, neither deadline fold reads the picture debt.
Probe confirms admission at 7 ms, strip refusal until 16 ms, and 0 frame-clock debt. The picture bit can remain set indefinitely without another event; it must schedule the strip's next eligible tick without becoming liveness.
Lifecycle source: removal refreshes even the last retained layer (`85028`); an admitted strip tick takes the bit and includes it in the present decision (`85351`). Hidden presents retain chrome/pending-frame debt (`104102`, `104389`), and visibility republishes; no separate loss found there.
P2 — paused-seat idle cost remains allocating work (`main.rs:56284`, `65771`, `66102`, `85028`): seat presence alone forces full layer reconstruction on every service, even with unchanged pixels/geometry.
Source-counted budget, one paused visible docked video, no animations/peek/drag, all floats closed: per turn, 1 sweep + 1 pump + 1 animation-service call + 1 layer refresh; each compose repeats all four.
Conservative heap-allocation lower bound per service: 4 (two nonempty seat-tree walks, one file-path clone, one nonempty layer Vec); worker-enabled request scans add more. Thus 1,000 turns + 1,000 composes cost 2,000 layer rebuilds and at least 8,000 allocations, despite 0 live journeys.
This is source accounting, not an allocator/CPU measurement. Closed floats add 1 sweep, 1 resize call and 2 ask calls per turn, with empty collections/0 worker requests; no drag returns before integration. Cache unchanged geometry/membership while retaining ungated frame/clock service.

O2 remains CLOSED: 1,000 refused-turn empty-real-host reports/publishes select 0 lane carries (0 extra chrome/overlay builds); the 15-host endpoint table also passes. Picture-service work above is charged to O4.
O3 remains CLOSED: source probes find no animation gate in input, compose, PTY drain/publication, coalescer or synchronized-update completion; turn still drains PTY before picture service (`main.rs:104462`).
Recorded for later: native presentation/occlusion recordings, actual decoder races and CPU/allocator profiling remain unmeasured; no full suite run. These limitations do not add verdict blockers.
Only this report is committed; no implementation changes.
