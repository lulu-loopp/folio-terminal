# Animation pacing review, round 3 — 2026-09-18

Candidate: c823abe2 only; round-2 report and candidate diff inspected.
Verdict: HOLD — empty-host P1 closed, but the new toast predicate leaves a residual unbounded carry case.
Report written early; only this report will be committed. No application launched or process terminated.
Locations below are crates/bt-app/src/main.rs unless another file is named.

Round-2 source-recurrence replay: 1,000 ordinary publishes after empty-host reports yield 0 carries, 0 chrome, 0 overlay rebuilds.
Gate questions/refusals no longer mark liveness; pace.rs:266 replaces both lanes, so a cleared report stays quiet.
Lane replay: 1,000 overlay-only carries give 0 chrome/1,000 overlays; both lanes give 1,000 chrome/1,000 overlays.
Exclusive rebuild verified: refresh_chrome:40814-41847 reaches refresh_overlay:41844 on every normal path.
Its return/Option shortcuts are inside closures; none bypasses that final overlay rebuild. Counts exclude baseline publish work.

P1 — an invisible, departed toast can keep the overlay carry alive indefinitely under unrelated presents.
toast.rs:662-667 reports every leaving.is_some() as moving, without testing the exit's elapsed time.
Dismiss an arrived toast at t=0; ordinary successful presents every 5 ms keep the 60 Hz gate refused (pace.rs:235-237).
advance_toasts:42652 returns above ToastHost::advance; only that admitted cleanup retires this otherwise untouched toast (toast.rs:627-636).
At t=90 ms its opacity is exactly zero and build skips it (toast.rs:1070-1072), but running_journeys:85375 still reports overlay=true.
carry_live_journeys:85318-85342 therefore rebuilds the overlay once per incoming publish, including after the exit has finished.
Real ToastHost/FrameClock probe: 1,000 simulated publishes over 5 seconds give 1,000 overlay carries, 983 at/after the 90 ms endpoint, zero chrome rebuilds.
The recurrence has no retirement while refusals continue: 200 redundant overlay rebuilds/second after landing on this schedule.
An admitted turn after the flood stops retires it; this is not a claimed autonomous idle redraw loop or measured wall-time cost.
Must fix: bound exit liveness by its tween end or retire expired exits above the gate; preserve endpoint/cleanup delivery and test refused-turn retirement.

Other predicates: tip/key-hint/peek opacity clamps at the endpoint and drawable receipts converge; waits themselves are excluded from their fade predicates.
Card nudge, command flash/rail, float fades, formula follow, and strip finite tweens use elapsed-time bounds; unchanged follow targets do not re-arm.
Strip periodic work uses working/loading state, engine.playing and drawn-animation membership; an already-paused video alone does not latch it.
Reduced motion/time-expired predicates can omit carries; existing gated advancers/deadlines remain, including thumb endpoint debt and float sweep.
Recorded for later: thumb rest and video-bar intent/rest still count as motion through shared deadlines; these waits are bounded absent new input.
Recorded for later: card_hint.nudge_moving is assigned overlay although focus_card_nudge_rows is chrome (:41773); its own chrome repaint/expiry remains scheduled.
Recorded for later: render-level endpoint/occlusion coverage and rebuild timings; source/clock replay is not a GUI or presentation measurement.
Validation: only cargo test -p bt-app --bin folio FILTER -j 4, sequentially: pace 36; formula_tools 25; formula_tool_seat_tests 17; wake 11; deadline 8 passed.
Temporary pace probe reproduced the toast defect (37/37 with probe); removed afterwards, product sources verified identical to c823abe2. No full-suite run.
