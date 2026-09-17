# XTVERSION round 2 review — 2026-09-17

Target: ce2d0da2 on d8a59187; compared with the committed round-1 report.
Verdict: merge with must-fixes before 0.4.2 (overflow can retain an earlier block's debt).
References below use ce2d0da2 lines; adapter = crates/bt-term/src/adapter.rs.
Round-1 closure:
- PARTLY CLOSED: block-commit debt/order, adapter:981-988,1728. Ordinary ESU now flushes
  before next BSU/DA1; regressions at 3122,3137,3151. Overflow reopening remains open below.
- CLOSED: payload/matching and cost wording, adapter:961-963,1686-1692; DESIGN:107-113,132-134.
  Existing tail growth can still allocate; no zero-CPU-cost claim is established.
- CLOSED: no-query events and paused-marker bells, adapter:3249,3271; accepted query-event
  ordering pinned at 3303; independent consumers at session.rs:9660,9666.
- CLOSED: canonical UTF-8 split regression, adapter:3326; near-limit grid equivalence, 3186.

Cut attacks (one temporary adapter test, removed after execution):
Repeated BSU before ESU, with queries before/after the repeat, passes at every feed split:
one answer before following DA1, no deadline left. BSU extends, not nests, vte's single block.
Debt-bearing ESU split across feeds also passes: the boundary parser retains the partial CSI;
the completing byte cuts and advances through ESU before flushing (adapter:975-988).
An ESU with no processor block open cannot simultaneously close an earlier still-open block:
there is only one sync state; BSU does not push a stack (vte ansi.rs:410-414).
Compound CSI ? 2026 ; 5 l causes a cut but no premature answer: vte's buffered scanner only
accepts exact ESU (ansi.rs:402-414); deadline stays set, DA1 stays buffered, exact ESU later
returns [DA1, XTVERSION]. A subsequent stray ESU emits nothing (adapter:1728).

P2 OPEN — overflow can hold committed debt behind the next block (adapter:991,1676,1728).
Repro feeds: BSU XTVERSION; then 2 MiB NUL + DA1 + BSU. Second feed returns only DA1,
with one XTVERSION still owed; a third feed ESU finally releases it. Without that last BSU,
the second feed returns [DA1, XTVERSION]: an outside-block reply CAN overtake overflow debt.
vte ansi.rs:375-381 commits then parses the remaining slice; a new BSU restores its deadline.
Must fix: release the overflowed block's debt despite a later block opening, and pin this repro.
Qualify the absolute outside-block promise (adapter:1701-1714; DESIGN:136-148): the documented
overflow exception permits overtaking, and its claimed end-of-feed release currently fails.

No query and no carried debt: byte-for-byte identical processor/canonical slices to d8a59187,
one advance per Bytes action; same boundary state updates, event drain and settlement.
The added bool/guard cannot cut with zero debt (adapter:981); no added allocation. A query-free
feed carrying earlier debt may cut, so the documentation's bare "no query" shorthand is too broad.
Allowed suites only: adapter 77/77 (76 committed + 1 scratch); lifecycle_matrix 42/42.
Four 200-frame budgets, measured / limit (lifecycle_matrix.rs:867-870): sparse bytes
128,089,299 / 147,456,000; sparse allocations 107,372 / 120,000; full bytes
35,604,923 / 44,236,800; full allocations 35,073 / 38,400. All match round 1 and pass.
TUI: 16 allocations/cycle at both heights, 21,304/21,504 B; paired shrink ratio 3.13 < 6.
No application launched or process stopped; only this report is committed.
