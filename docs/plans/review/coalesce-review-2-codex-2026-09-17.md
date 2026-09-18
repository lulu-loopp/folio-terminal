# Coalesced PTY drain review, round 2

Target: 4bf5291a (8ec9835a + 4bf5291a).
Verdict: **MERGE**. All three round 1 blockers are closed; no new release blocker found.
Scope: static review and permitted Cargo tests; no Folio launch or process termination outside test harnesses.

## Counterexamples and attacks

- Round 1 transport/echo counterexample: CLOSED. Real detector/ring/policy paths publish [1,1,1], [255,255], repeated 8 KiB pipe reads, and an echo mixed with a capped read immediately.
- Round 1 same-slice DEC 2026 counterexample: CLOSED. Exact 1,024-byte BSU + 1,008 bytes + ESU increments once, the drain's named comparison sees it, and policy returns Now despite absent deadlines before/after.
- Round 1 cursor counterexample: CLOSED. Dark caret resets on arrival; intervening publication settles the debt without canceling its reveal. The source-shape test pins one reset before decide in drain_pty.
- Ordinary PTY reads [300, 300] legitimately become a candidate: first publishes, second may wait 3 ms. Acceptable bounded false positive; no claim of perfect classification.
- PASSED: a capped 16 KiB pipe read cut at 8 KiB produces an uncapped first slice and Now; the next-turn remainder retains its capped flag and may wait 3 ms from that turn, without inheriting settled debt.
- PASSED: resize fork/replay, resize, canonical swap and finish do not increment; ESU, forced finish, and marker each increment once, before or after the swap. Repeated finish/ESU do not increment.
- PASSED: real session finish at its deadline and marker commit count once each; forced parser overflow counts once. The counter belongs to the adapter, outside discarded canonical-listener output.
- PASSED: one fed pane commits while another stays open; real sessions feed DrainOutcome::merge and decide, returning Now with the sibling still buffered. Quiet outcomes cannot erase either flag.

## Recorded for later

- Display-bound honesty: ADDRESSED; runtime still supplies None and docs now explicitly promise only the timer bound. A display deadline remains unimplemented.
- Torn-source evidence: ADDRESSED and passing; T=0 asserts half-written $$ cells, T=3 ms asserts no source cells. Separate detector-to-policy tests now cover the transport fixes.
- Still OPEN: elapsed-wall-clock/display latency and full window/event-loop behavior are not demonstrated by injected-time, helper, or source-shape tests; no live benchmark was authorized.
- Nonblocking documentation correction: crates/bt-pty/src/lib.rs:1048 and docs/DESIGN.md:449 infer a guaranteed 256-byte transfer floor from POSIX input-queue limits. Those limits do not establish a PTY-master read cap; call 256 a heuristic. See [POSIX terminal input processing](https://pubs.opengroup.org/onlinepubs/9699919799/basedefs/V1_chap11.html).
- The supplied recording census (123 flagged / 122 genuine / one 508-byte false positive) was not independently replayed in this review.

## Validation

- cargo test -p bt-pty -j 4: PASS, 52 unit + 5 existing integration + 3 temporary counter tests; 19 ignored.
- cargo test -p bt-app --bin folio coalesce -j 4: PASS, 23 existing + 4 temporary adversarial tests.
- cargo test -p bt-app --bin folio pty_drain -j 4: PASS, 22 tests, including cursor order and sibling merging.
- cargo test -p bt-term --lib synchronized -j 4: PASS, 17 tests.
- Seven temporary review tests called production detector/ring/policy/session/counter/blink code; not an end-to-end runtime test. Scratch tests were removed and product files restored exactly. Only this report is committed on docs/coalesce-review-2, directly from 4bf5291a.
