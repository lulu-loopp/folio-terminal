# Animation pacing review, round 2 — 2026-09-18

Candidate: a971af50 (3b85c5cc + a971af50); round-1 report and candidate diff inspected.
Verdict: HOLD. Original P1 closed; one new blocking hot-path regression.
Report created before detailed inspection/tests. Product code unchanged; no application launched or process terminated.
Locations are crates/bt-app/src/main.rs unless another file is named.
Original P1 schedule: CLOSED for the 5 ms PTY-present flood.
publish_frame_inner:66668 samples the flight before projection:66763; refusal now suppresses only its own frame request.
advance_math_toggle_if_due:88309 settles above the gate; math_toggle_deadline:88333 takes min(frame, lands_at()).
Re-ran the flood clock/three-curve tests and 60/144 Hz sampling test: all pass (18 increasing transitions; exact 90 ms endpoint).
These are clock/curve tests plus source-wiring checks, not a GUI reproduction or end-to-end presentation measurement.

P1 — the supposedly idle carry guard never retires; ordinary PTY/keyboard publishes rebuild the whole UI forever.
animation_frame_is_due:85276 calls note_running unconditionally. Tooltip:45049, key hint:47771,
float:79485 and math tools:87762 ask BEFORE checking whether anything exists/moves; turn calls them even with empty hosts.
pace.rs:273-279 copies running_this_turn into running, then each empty-host turn sets it true again (is_running:246).
Repro: no journeys, one completed turn, then ordinary publishes. Every publish takes sync_math_tools + refresh_chrome + refresh_overlay.
A source-recurrence replay of 1,001 empty-host turns counted 1,000 carries before those turns' gates; the flag remained true.
This is indefinite extra work, not the claimed bool/Option fast path or merely one cleanup turn after landing.
Cost bound by call count: each carry adds ONE full chrome build and TWO full overlay builds; refresh_chrome:41844 already rebuilds overlay.
At one publish/5 ms: 18 chrome + 36 overlays over 90 ms, 44 + 88 over 220 ms (excluding the start frame), then 200 + 400 per second indefinitely.
Chrome walks all tabs:40930, measures text:41004/41019, refreshes thumbnails:41026 and builds files/preview/chrome collections:41166/41727.
Both overlay passes build/flatten/resolve layers:46221/46633; renderer equality runs only AFTER construction (bt-render/src/lib.rs:8387/8419).
This bounds added rebuild counts, not milliseconds: work scales with window content; no runtime latency or allocation-byte figure is claimed.
Must fix: record actual live motion separately from gate/deadline checks, eliminate the duplicate overlay build, and test empty-host/finished-journey publishes.

Settlement: no double toggle found. settle_math_toggle:88224 takes the Option before mutation/repaint; subsequent settlement is a no-op.
It may precede presentation of the endpoint sample: it clears the override, switches the owed face, then repaints:88255.
repaint_pane_change:88506 passes skip_unchanged=false, publishes the equivalent final document state and requests redraw:66960; no final-sample prerequisite loses that state.
Hidden/occluded: windows still turn:115619; the unpaced landing remains in the fold:104554. If the loop pauses, the first resumed turn settles.
redraw:104015 retains skipped frames; SkippedNotVisible does not retry-spin:112010; Occluded(false):116630 republishes for return.
Re-entrancy: no synchronous publish/redraw-request edge found from carry's rebuild path (310 reachable self methods scanned, renderer setters inspected).
is_running itself schedules no wake; the blocker is rebuild cost on incoming publishes, not a demonstrated autonomous idle redraw loop.
Recorded for later: marks/face geometry still reads last_presented_frame:87679/88374 before this publish's projection; same-frame alignment needs a render-level check.
Recorded for later: full rebuild timing with many tabs/cards, native occlusion recordings, and separate Instant::now samples; no visual/performance recording claimed.
File-peek closing/dwell fold entries are now clamped:104467/104477. LIVE-plane and reduced-motion one-frame switching remains the documented pre-existing boundary.
Validation: only authorized cargo test -p bt-app --bin folio FILTER -j 4, sequentially; all passed:
pace 35; formula_tools 25; formula_tool_seat_tests 17; math_hover 3; deadline 8; wake 11. No heavier Cargo command run.
