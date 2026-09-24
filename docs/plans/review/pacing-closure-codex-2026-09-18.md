# Animation pacing closure review — 2026-09-18

Tree: `8c0a65bb`; reviewed `d8ac80c0`, its documentation follow-up, and all three `docs/pacing-review*` reports against O1–O5.
Report created before checks; documentation branch `docs/pacing-closure` starts at that tree. No application launched or process terminated.
Validation: only sequential `cargo test -p bt-app --bin folio FILTER -j 4`; untouched-tree selections all passed:
`pace` 36; `journeys_tests` 3 (15 host rows); `toast` 26; `formula_tools` 25; `formula_tool_seat_tests` 17; `a_resting_bar` 1; `deadline` 8; `wake` 11.
Temporary `journeys_tests` runs selected 6, then 7; all passed (3 original + respectively 3 and 4 observational closure probes). No full suite or GUI measurement.

## Prior must-fix counterexamples and budgets
Round 1, flood starvation: CLOSED for the formula journey. Real clock refuses all 1,000 turns at 5 ms; three curves give 19 samples/18 increasing transitions through the exact 90 ms endpoint.
Compose samples the toggle before projection (`main.rs:66722`); settlement precedes the gate (`88459`), and the landing deadline remains unpaced. Source-wiring tests passed.
Round 2, empty hosts: CLOSED. Replayed 1,000 reports/publishes with empty real hosts: 0 carries, 0 added chrome builds, 0 added overlay builds despite gate refusals.
Round 3, departed toast: CLOSED. Unretired real toast, 1,000 publishes at t=5..5,000 ms: 17 overlay carries before 90 ms; 0 across the 983 publishes at/after landing (formerly 983); 0 chrome builds.
Finished-host table: 15 rows × 1,001 samples, 0 carries after each endpoint; first admitted turn settles each host. Budgets count work added after fresh lane reports, not baseline publishing or elapsed CPU time.
Lane recurrence: 1,000 overlay-only carries add 0 chrome/1,000 overlays; both lanes add 1,000/1,000, never duplicate overlays. Idle/finished guards retain the cheap bool/Option reads.

## Lifecycle boundary and coverage
Marks retarget and toggle reversal at 45 ms preserve the current sample, remain live past the old 90 ms endpoint, and retire at the new 135 ms endpoint; unchanged marks targets do not restart the clock.
Same-anchor tooltip observations preserve its epoch; hide/re-show requires a fresh intent and starts a fresh 90 ms fade. Repeated rail targets are inert; changed rail targets sample the current value and replace the epoch.
A 1,000-change/5 ms rail-and-toggle loop extends liveness only with new targets; after its last input, rail motion ends within 140 ms and toggle motion at 90 ms. No cleanup flag is needed.
Grepped `running_journeys`, `strip_animation_work`, and `terminal_thumb_work`; outside the table's single generic strip tween are these concrete inputs:
Tab ring sweep, pin reveal, tab FLIP/landing; chevron; dock reveal; resizing-card transition; focus reveal; rail open/text/fold; docked and floating file triangles; pane FLIP/fade.
Popup passages, settling register, foot-phrase crossfades, Advanced disclosure, and video-bar fades. Each finite journey reads its own start/span; delayed reveals include their delay. Video-bar rest/intent are excluded by the separate 1-test check.
Other inputs are condition-driven tab working/indeterminate/awaiting, page loading, playing video, and drawn animated-image membership; these are periodics, not finite journeys. Reading them fresh does not ensure their frames advance (O4 below).
`cards.owes_frame()` is projection debt, not a journey: chrome pays it; a throttle refusal retains it until the 100 ms projection window permits service, and absent geometry clears it (`focus_thumb.rs:219`, `main.rs:95793,95994`).

## Remaining ledger failure — O4 / P1
`advance_strip_animation` returns at the display gate (`main.rs:84910`) before the sole `video.pump(now) | advance_animations(now)` call (`85127`). Carry rebuilds chrome/overlay but never services either clock.
Consequently a playing decoded image/video can stay on its cached frame throughout the same 5 ms unrelated-present flood, although its periodic condition correctly keeps reporting live. The 15-row test and periodic-condition test do not exercise this service boundary.
Confirmed with a real decoded-animation host (64 synthetic frames, 100 ms each): 1,000 refused turns leave frame index 0 after 5 s versus index 50 when the identical host is serviced on those presents; source gate/carry wiring checked alongside it.
Drag autoscroll has the same uncovered boundary: its only integration call is below the gate (`main.rs:77184,77201`; sole caller `104418`), so that schedule also permits 0 integration calls. This is source/clock evidence, not a GUI reproduction.
Closure requires servicing these elapsed-time consumers on incoming frames or a non-postponable service deadline while keeping input publication ungated. This is the existing O4 obligation, not an unrelated feature request.

## Obligation verdict
O1 CLOSED — formula progress and ungated settlement survive unrelated presents.
O2 CLOSED — empty and finished journeys retire liveness; no unbounded post-endpoint carry or duplicate rebuild found.
O3 CLOSED — input echo, PTY, resize, selection and wheel retain ungated publication; coalescer and DEC 2026 deadlines remain independent (`main.rs:104335,104339,104596`).
O4 OPEN — elapsed-time arithmetic alone does not prevent gated decoded-animation/video service and drag integration from being skipped indefinitely.
O5 CLOSED — hover clears with pointer exit; shared mark hit geometry remains reachable; actions run on press; only `BandLeftTheScreen` settles a running toggle (`main.rs:88155,95245–95300`; renderer `7689`).
Decision: HOLD on O4; the three prior must-fix counterexamples themselves are closed.

## Recorded for later
Pre-existing `RevealTween` delay/zero-distance behavior (`main.rs:27174–27211`): 1,000 label open/reverse cycles at 5 ms kept opacity exactly 0 and liveness true; it died 100 ms after the last reversal. This requires ongoing retarget input; normal rail-width motion shares the lane. It limits the literal “waits are never motion” claim but is not the post-endpoint retirement failure.
Native presentation/occlusion recordings, rebuild timings, and same-frame marks/projection geometry remain unmeasured. Temporary probes removed; only this report is committed.
