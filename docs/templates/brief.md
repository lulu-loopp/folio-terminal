# Brief template

A brief is what an implementer or an investigator is handed. Fill every field;
write `unknown` rather than leaving one out — an unknown is information, a gap is
not. `docs/CONVENTIONS.md` §十 says why each field exists.

```markdown
# <one line: the symptom, in the words of whoever saw it>

## Roles
- project owner: <who accepts risk and approves criteria changes>
- coordinator: <who wrote this brief and decides what merges>
- implementer / investigator: <who receives it>      reviewer: <who reviews — never the implementer>

## Headline scenario
<the one use this change exists for; a finding that breaks it blocks>

## Trees
- baseline: <sha>            (where the symptom reproduces)
- candidate: <sha or "none yet">
- worktree / branch: <path> / <name>

## Scope
- in: <files, subsystems, behaviours this ticket may change>
- out: <what it may not touch, and what was already closed by earlier rounds>
- changes a ruled behaviour or a daily gesture: yes / no
  - if yes — asked the project owner: yes (answer: …) / not yet (MUST ask before dispatching)
- the implementer may not add a refusal, warning, confirmation or check not in this brief

## Observed
What was seen, and nothing else. Recording provenance (which trace, which
moment), the faithful synthetic sequence derived from it, dimensions, read
boundaries, timing. Raw recordings never enter the repository or a test.

## Evidence authority
For each fact the ticket depends on, the thing that is allowed to answer it
(e.g. "what is on screen" → the published frame; "who owns these rows" → the
detection record, off-screen and Failed ones included; "did a commit happen" →
the parser). A projection may not stand in for its authority.

## Hypotheses — UNPROVEN
The coordinator's guesses, labelled as guesses. The first deliverable is an
independent account of the observed transitions; only then are these compared
with it, confirmed or refuted, with the discriminating evidence.

## Owner of the broken fact
<symbol, or `unknown` — then the investigation resolves it before any fix>
Its writers, readers, invalidators and retirement paths, once known.

## Class and general rule
Which class of defect this is, and the one rule that, if it held everywhere,
would make the whole class impossible. If this is the second ticket of the same
class, the fix is structural (CONVENTIONS §十 rule 6).

## Moved work (rule 9 — fill only when the fix moves work to another thread / crate / process)
- reused function: <name of the function both callers share>
- real-producer test for each seam: <test name that runs a real temp file through the real verifier>

## Acceptance — numbered, observable
A1. …
A2. …

## Blocking criteria for review — numbered
B1. …
Out of bar (recorded for a later version, with repro): …

## Budget
Counts on the COMPLETE changed operation — calls, rows scanned, bytes,
allocations — against the baseline, over: active, failure, flood, and
after-the-endpoint workloads. A quiescent subsystem adds zero work.

## Who and what this touches while it runs
Machines, memory, the owner's running Folio, shared boxes. Heavy builds on the
owner's machine are announced first — to the project owner, in the
conversation, before starting, and the brief says so here. Processes are ended only by a recorded PID.

## Allowed validation
<exact cargo commands, -j, what may be launched (usually nothing)>
What will remain unverified after them: …

## Rollback
<how this change is taken out again, and what would make us do so>

## Report
Written early and appended to: <path>. Each finding carries: criterion ID ·
commit + symbol · minimal sequence · expected / actual · baseline result ·
evidence strength (recorded / synthetic / static / untested).
```
