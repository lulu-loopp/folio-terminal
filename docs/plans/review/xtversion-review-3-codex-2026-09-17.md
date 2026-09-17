# XTVERSION round 3 review — 2026-09-17

Target: 908921f3; previous review: docs/xtversion-review-2.
Verdict: merge into 0.4.2; round 2's overflow item is CLOSED. No new must-fix found.
References: adapter = crates/bt-term/src/adapter.rs at 908921f3; L = 2,097,152.
Both original repros rerun (adapter:3345): BSU + XTVERSION, then L NUL + DA1,
with/without trailing BSU, now return [XTVERSION, DA1] immediately; no old debt remains.
Committed seam and tripping-DA1 regressions (3367,3392) also pass.

One temporary instrumented attack (removed) checked exact slices at adapter:1041-1075:
pending = 0 and L-2 give distances L-1 and 1; prefixes are L-2 and 0 bytes.
Length distance-1 stays one piece and owes its answer; length distance gives [d-1,1,0];
length distance+1 gives [d-1,1,1]. Concatenated slices equal the original bytes.
Zero-pending debt was injected to challenge the gate; empty input cannot release it.
An already-armed resize fork gets the same pieces; buffer/deadline, visible grid after
reconciliation, and discarded duplicate replies pass at both pending extremes.
Zero pending CAN mean an open block just after BSU; it is not a universal commit predicate.
Here, if the original block survives the prefix, pending reaches L-2; the next byte forces
vte's commit and clear BEFORE parsing (vte 0.15 ansi.rs:375-381). Repeated BSU cannot clear it.
If the prefix already committed, the original debt is due anyway. A tripping final BSU byte
can open a NEW empty block: tested, old answer leaves once while its deadline stays set.
Thus bypassing the deadline in push_xtversion_replies is safe at this call site (1059,1809).
No debt: the first guard forwards the identical entire slice once; an overflowing no-query
slice was compared byte-for-byte. No new allocation; ordinary event/boundary path unchanged.

Nonblocking existing edge (1233,1656): arm the fork AFTER BSU + (L-2) NUL and its buffer
is 0 while the displayed parser holds L-2; the retained-tail cap loses BSU. The first scratch
run exposed this; the final attack isolates and pins it separately. This no-debt path and
tail/seed code are unchanged from round 2; the new three-piece forwarding is not its cause.
Allowed suites: adapter 80/80 with scratch, then pristine 79/79; lifecycle_matrix 42/42.
Four 200-frame budgets, measured / limit: sparse bytes 128,089,299 / 147,456,000;
sparse allocations 107,372 / 120,000; full bytes 35,604,923 / 44,236,800;
full allocations 35,073 / 38,400. All pass and exactly match round 2.
No application launched or process stopped; only this LF report is committed.
