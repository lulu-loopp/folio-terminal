# Formula hover review, round 2 — 2026-09-18

Candidate: d8e897a8 (c41b0cdf + a35ca06e + d8e897a8). Verdict: HOLD.
Both new commits inspected. Product code unchanged; scratch probes removed; no application launched or process ended.
Locations below are crates/bt-term/src/session.rs unless qualified.

P1 — overlapping Failed records defeat the first-record stopping rule (12058–12078).
Repro: 100x12 session; CRLF-separated `$$ / $$ / A= / \begin{pmatrix} / a & b\\ / c & d / \end{pmatrix} / $$`, then 15 tail rows.
Feed normally, scroll projection to top, schedule visible work, resolve detection, inject Compile("r2") at render completion.
Six unchanged frames attempt [2,1,1,1,1,1]; reproduced both with fresh records and the untouched feed/worker lifecycle.
The recovered outer block owns ids 2..8; the earlier inner record owns 4..7; both remain Failed.
block_that_owns(8) stops at 4..7 and returns None instead of reaching 2..8, re-arming the outer closer forever.
Cause: apply_worker_completion:8110 suppresses interior records only when `applied && rendered`.
Must fix: establish non-overlapping ownership on failed completion too, or make lookup tolerate retained overlaps;
preserve revision-triggered retries and pin this phantom-prefix/inner-environment failure case.

Round-1 status: source-face clipping CLOSED; simple failed-render loop CLOSED; picture allocation regression CLOSED.
Independent full/clipped probes: Ready and Failed each repeat [0,0,0,0,0,0] after their first attempt.
Layout retry is covered by the existing failure test; scratch theme and redetect changes each retry exactly once.
Picture/control with identical cells and cleared placements: both 112 allocations / 66,175 bytes per scheduler call.

Bound/extent audit: frozen windows copy consecutive resident entries under the byte cap (11990–12015,12154–12185);
skipping retired/suppressed record states cannot insert extra source rows. The disjoint-record invariant fails in P1.
Inline runs, including split-row joins, have block_end == start (bt-detect/src/lib.rs:2001–2007), so cannot claim a later closer.
Tables carry their actual end; scratch table 1..3 followed by display 5..7 preserves both. One scan excludes overlaps (detect:2190–2253).
Two adjacent Failed blocks settle after two attempts; re-arming the second owner still admits its closer independently.
Deletion callers remove prefixes/all history; quota and ED3 probes remove the owner. No surviving stale-end refusal demonstrated.
Demotion sites: bt-detect/src/lib.rs:471 source_changed,477 detector_changed,483 layout_changed.
redetect:8240 visits every record; theme goes through invalidate_layout:11700. RIS preserves unchanged frozen text/records,
resetting staging; ED3 deletes them. No missed detector-revision demotion found; source records are freshly created at 11518.

P2 follow-up — the walk also runs for settled candidates, before record.schedule_scan rejects their lifecycle (12084 onward).
Paired gate-disabled control, 200-row grid, test profile, 200 calls/arm: settled candidate prose 8.42 -> 8.97 ms/call;
earlier runs added 0.64–1.16 ms. Allocation counts/bytes are identical. Empty record map: 7.72 -> 7.45 ms (noise).
Plain prose never enters schedule_scan; its paired timings are noisy. No completed blocks is not an empty map: every frozen line has a record.
Worst case adds O(visible candidates * byte-bound) record/document visits on every call; move the cheap lifecycle refusal ahead of the walk.

Recorded-for-later recheck: presented-frame hover ordering, one-shot 500 ms grace, 90 ms mark continuity,
retained cursor-blink revision, and stale-worker checks remain as reviewed. Hover tests now select, but still inspect source strings.
Validation: only the four authorized cargo commands, all -j 4. Baseline lib 508 passed; scratch lib 509 passed; lifecycle 46 passed.
App filters: math_hover 3 passed; formula_tool_seat_tests 14 passed. These are filtered runs, not the full app suite.
Seven allocation/byte budgets unchanged: 16/21,304; 16/21,504; 636/398,730; 644/566,946;
1,018/760,939; 2,974/1,989,859; 3,467/2,199,925. All lifecycle assertions passed.
