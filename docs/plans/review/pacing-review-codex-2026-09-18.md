# Animation pacing and formula hover review — 2026-09-18

Candidate: 3b85c5cc = main 4e6194e7 + 0f0065bc + 3b85c5cc.
Verdict: HOLD. One release blocker (bars 1 and 3); both commits inspected.
Report created before the audit/tests. Product code unchanged; no application launched or process terminated.
Locations below are in crates/bt-app/src/main.rs unless another file is named.

P1 — unrelated presents can starve a formula's entire journey indefinitely.
pace.rs:174,215 admits only after an interval with no present; trace_present:103330
resets that interval for EVERY present, including PTY output. turn:103922 reads it.
Repro schedule: start a toggle in one pane; another visible pane presents changing
output every 5 ms. Every intervening turn refuses; each present postpones the debt.
A scratch Rust test importing the unchanged FrameClock observed ZERO admissions
in 1,001 turns over 1,000 ms; admission returned only after output stopped.
This is not harmless skipping: advance_math_toggle_if_due:88169 gates BOTH sampling
and settlement. present_math_toggle:88061 caches height/opacity in the session;
publish_frame_inner:66747 projects that cache without sampling the flight again.
The 90 ms toggle can remain at its initial/intermediate height until output pauses,
then jump to the end. A continuously active sibling can prevent settlement indefinitely.
The injected schedule proves the clock failure; no GUI reproduction is claimed.
Must fix: make animation progress/settlement accompany input-driven frames, or keep
a non-postponable animation service deadline. Keep input/output publication ungated;
add a mixed-traffic regression test rather than only isolated animation clocks.
Thirteen-path audit / other release bars:
All thirteen advancers contain the turn gate: strip, tooltip, key hint, card hint,
toasts, command flash, command rails, terminal thumbs, file peek, float,
drag autoscroll, formula marks, and formula toggle. Each primary animated deadline clamps.
Searched present_chrome_change, request_redraw and Some(now): no missing gate in
these autonomous advancers; redraw dispatch consumes published/pending work.
Admission is shared for the turn, so block and marks are not separately denied.
Tweens/fades use elapsed time; drag autoscroll integrates elapsed duration; decoded
animation catches up against due_at. No new per-tick increment slowdown found.
Formula sampling is at its gated advance, NOT every compose (P1).
Keyboard echo, PTY publication, resize, selection and wheel paths have no new gate.
The 3 ms coalescer and DEC 2026 timeout remain independent minimum-fold entries
(104264–104273), consumed unconditionally at 104024/104029; pacing does not gate them.
Conversely their successful presents can starve animation service (P1).
FrameClock/last_present_at are per WindowRuntime; window deadlines are merged by min.
Stale pre-hide/minimise/occlusion timestamps yield now, not a past/far-future wake.
Scratch checked 60↔144 Hz, old timestamps, None/0/absurd rates, and debt retirement.
Tab/session exits settle the toggle through BandLeftTheScreen; pointer exits do not.
Hover misses clear the band immediately (87417/87865); marks leave over 90 ms.
Renderer math_hit_test:7689 accepts marks and band through shared geometry.
Toggle/copy execute on press (94973), so exit before release cannot lose the action;
release still consumes MouseRoute::MathBlock. No separate hover blocker found.
Recorded for later:
File-peek closing_at/dwell remain raw in the fold (104321–104333), while their consumers
are gated (71619). Scratch confirms an expired raw deadline beats future pacing debt:
busy wakes until admission, normally ≤ one interval; continuous presents extend this
via P1. Clamp both deadlines or retire these clocks outside the animation gate.
Unconditional gate callers book one extra wake after a final present even when idle;
the admitted no-work turn clears it, so no self-sustaining idle loop was found.
Strip/autoscroll retain a 16 ms floor. VRR/display-mode changes without move/scale
are not tracked; missing/invalid rates retain the prior interval or 16 ms default.
Nominal-rate pacing and macOS acquire interaction need recordings; none claimed here.
Validation: authorized cargo filters, sequentially, all -j 4, all passed:
pace 32; math_hover 3; formula_tool_seat_tests 14; deadline 8; wake 11; station 21; coalesce 23.
Scratch: 3 adversarial clock/fold probes plus 5 imported pace tests passed; files removed.
