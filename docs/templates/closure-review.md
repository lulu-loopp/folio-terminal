# Closure review template

A closure review is the last review of a branch. It does not read the branch
again from the top: it checks the final tree against the obligation ledger the
earlier rounds produced. It is bounded by obligations, not by the reviewer's
appetite. `docs/CONVENTIONS.md` §十 rule 6 says why.

```markdown
# Closure review — <branch> at <sha>

## Round number: <N>
A small fix gets at most two review rounds (CONVENTIONS §十 rule 6). If this is
round 3 or higher and the findings are still "differs from what it replaced",
stop patching and ask whether the approach itself is wrong.

## Trees and scope
- final tree: <sha>      baseline: <sha>
- scope is FIXED: the commits since the last reviewed tree, and the ledger below.
  Everything already closed stays closed unless a listed commit touches it.

## Blocking criteria (copied from the brief, unchanged)
B1. …
Any change to these since the brief is listed here with who approved it. The
old counterexample is kept either way.

## Obligation ledger
| ID | Semantic obligation | First raised | Counterexample (permanent test) | Status |
|----|---------------------|--------------|----------------------------------|--------|
| O1 | …                   | round 1      | `tests::…`                       | closed / open |

Two findings are the same obligation when the same rule, held everywhere, would
have prevented both — not when they touch the same struct. The same obligation
failing in two rounds pauses patching for an owner-and-lifecycle review; the
repair after that is structural.

## The review
1. Rerun every must-fix counterexample on the final tree: closed / open.
2. Attack the lifecycle boundary the last commits changed (failure, resize,
   hidden window, retirement, two callers, overlap).
3. Budgets on the complete changed operation, after the endpoint included.
4. Tests actually selected and their counts — a green command that ran zero
   tests is not evidence.

## Findings
Each: criterion ID (or "none") · obligation ID · commit + symbol · minimal
sequence · expected / actual · baseline result · evidence strength.
A finding BLOCKS only if it is (1) introduced or made worse by this branch,
(2) a hard bar reachable in ordinary use, or (3) a break of the headline case.
Everything else goes under "Recorded for <next version>" with its repro and does
not change the verdict. Untested suspicions go under "Unknown".

## Verdict
CLOSED — MERGE / MUST-FIX (only findings meeting the bar) / HOLD.
A HOLD is an input to the coordinator's decision, not the decision. Owner or
hardware acceptance still outstanding: …
```
