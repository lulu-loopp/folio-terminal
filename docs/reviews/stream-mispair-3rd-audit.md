# Independent audit (3rd round): "流式块错位滞留" — compress-rewrite.vt

_Read-only 审因. Worktree HEAD `ca3b062` (last product commit `3875209`), based on main.
Capture `.tmp-repaint-capture/compress-rewrite.vt` (+`.chunks`), 265 490 bytes, 104×26,
one initial RESIZE marker only (104×26 @0.45s — no zoom, no manual resize). All findings
reproduced with throwaway probes that were reverted; worktree tree clean._

## TL;DR

- **The root is the SAME failure class the two prior audits named — one unbalanced `$$` desyncs
  every later block via the detector's global parity toggle — but round 3 has a THIRD, more
  fundamental trigger the seam-patches and the red gate cannot see: a live-rendered block whose
  opener has scrolled ABOVE the live grid's row 0 (Codex's in-place scroll-region "compression")
  while its body+closer remain in the grid. The orphaned in-grid closer breaks `$$` parity for
  every block below it, and the last-streamed block (`\zeta(s)`) — which arrives *after* the break
  and therefore never had a clean detection to hold — is stranded at source.**
- **Two compounding sources of the identical odd-parity poison, both proven:**
  1. **Frozen-history desync (round-1 class, still live).** Codex's compression rewrite ate the
     opening `$$` (and blank) before `\sum_{n=1}^{\infty}\frac{1}{n^2}=\frac{\pi^2}{6}`
     (`FROZEN[97]`), leaving history one `$$` short. From id ~97 on, every `$$` block in history
     mispairs — the full scan detects **nothing between id 106 and 173** (8+ blocks lost).
  2. **Grid-seam clip.** The Fourier block straddles the seam: opener `$$` (`FROZEN[172]`) + first
     body line (`FROZEN[173]`) are frozen; the rest of the body + closer are in the live grid, whose
     top row starts **mid-body** (`\int_{-\infty}^{\infty}`). Because (1) leaves history one delimiter
     short, the scan reaches the seam in the **closed** phase when it should be **open**, so the
     Fourier opener is never registered — the grid's Fourier closer becomes an orphan, and the grid's
     seven `$$` (odd) desync everything below.
- **Decisive isolation test.** Grid-only clean scan → **0 blocks**. Prepend one synthetic `$$`
  opener (supply the clipped Fourier opener) → **4 blocks** incl. `\zeta`. Drop the leading orphan
  `$$` → **3 blocks** incl. `\zeta`. The stranding is *purely* the one-delimiter parity offset;
  `\zeta`'s blank-line body and position are fine.
- **Why both prior rounds' gates stayed green (the systematic blind spot):**
  - The `isolation_gap` red gate is defined as *(grid-only-provable) − (full-provable)*. It fires
    only when the poison is a **history prefix** and the grid alone is clean. Round 3's grid is
    **itself** poisoned (opener clipped above row 0 → grid-only = 0) → gap = 0 → **structurally
    blind**. Timeline: gap=3 for 2.4 s (82.16–84.53 s, catchable), then **gap=0 for the rest of the
    run** once the clip lands; `\zeta` streams at ~92 s, deep in the blind window. `isolation_gap`
    is also **not wired as a hard gate anywhere** (no script/test reads it; only the oracle prints
    it, and the *final* value is 0).
  - The flash oracle / `trace_blocks.py` derive "rendered" from placement history, so a block
    **never placed** (`\zeta`) produces no `R→S` flip → exit 0. (Both prior audits filed this.)
  - **Holds mask the dead detection.** The 3 blocks that still show Rendered (Fourier / entropy /
    Bayes) are detected **nowhere** in the current scan (full = 0 grid blocks) — they are **stale
    preservation holds** (`d7adce8`/`002acc7`) from before the clip. As long as every on-screen
    block was detected once and is held, the screen *looks* correct while detection is actually
    dead. The bug only becomes visible when a block arrives with no pre-existing hold. This is why
    round-2's own recording ends clean (below) yet round 3 strands.

---

## Symptom & final resting state (frame evidence)

Final frame `frame=4006 elapsed_us=97420583 state=Mixed`
`rendered=[Fourier \mathcal{F}…, entropy H(X)…, Bayes P(A\mid B)…]`
`source_rows=["$$","$$"]  flash=false detections=107 invalidations=0 isolation_gap=0`
`source_plane=… "$$ \zeta(s) [blank] \sum_{n=1}^{\infty}\frac{1}{n^s} $$" … "› Implement {feature}"`.

The `\zeta(s)` block completes fully (opener+body+closer present) at `frame=3999` (~92.75 s) and
**never renders** — it is Rendered 0 times across the whole 4 007-frame run. The user's earlier
`\det` mispair is the same class one reflow-generation earlier (that block is now frozen at
`FROZEN[158-160]`). The user's `$$ e^{i\theta}=…` screenshot is the euler block, which in this
capture spends its life as `[ e^{i\theta}=… ]` (Codex ate `\[`→`[`, ledger "非我方" non-render) and
was later reformatted to `$$` by the same compression pass — its stranding is the same parity
desync, not a separate bug.

Both the synchronous oracle and the async model (`BT_PROBE_MATH_LATENCY_US=50000`, with
`drain_to_quiescence`) converge to the **identical** stuck state — this is deterministic detector
logic, not a timing race.

## Root-cause chain (frame → byte → code)

### Level 1 — detection (the immediate cause)

`live_detection_context` (`crates/bt-term/src/session.rs:1160-1198`) feeds the scanner **up to
`LIVE_FENCE_HISTORY_CONTEXT_LINES = 1024`** (`session.rs:53`) frozen lines **+ the live grid**, and
`scan_math_blocks_impl` (`crates/bt-detect/src/lib.rs:807-1051`) re-derives all `$$`/environment
pairing with a **single global toggle** (`opening = Some/None`, `lib.rs:816,905,917,1051`), rejecting
empty bodies via `valid_display_body` (`lib.rs:1085`) **after the toggle already consumed the
delimiter**. Probe dump at the final quiescent flush:

```
initial_context = { fence: None, opening: None, prefix: Known }   boundary(grid start) = 174
FULL history+grid scan → 7 blocks, ALL in frozen history (ids 56–105); ZERO grid blocks
GRID-ONLY clean scan   → 0 blocks
grid rows (ids 1..26)  → [1]\int_{-\infty}  [2]f(t)e^{-i…}  [3]$$  [4]  [5]$$  [6]H(X) …
                         seven "$$" at grid rows 3,5,9,11,15,17,21  (ODD)
```

### Level 2 — the offset is one clipped opener (decisive)

```
EXP-A  prepend one synthetic "$$" opener to the grid → 4 blocks:
        Fourier(bridge 900000..3), entropy(5..9), Bayes(11..15), ZETA(17..21)
EXP-B  drop grid rows through the first "$$" (row 3)  → 3 blocks:
        entropy(5..9), Bayes(11..15), ZETA(17..21)
```

Supplying the missing opener, or removing the orphan closer, recovers **every** block including
`\zeta`. The stranding is exactly the parity-offset-by-one; nothing about `\zeta` itself is
undetectable.

### Level 3 — where the delimiters were lost (byte / upstream trigger)

- **Frozen-history drop.** `FROZEN[94-98]` reads `$$ / \int_a^b / $$ / \sum…\frac{\pi^2}{6} / $$` —
  the `\sum…π²/6` line at `FROZEN[97]` has **no opener** (the blank + `$$` before it were eaten). A
  toggle over frozen history is one `$$` short of correct, so the scan arrives at the seam in the
  wrong phase. Tracing that block: it was `[ … ]` (eaten `\[`) at 46–51 s (frames 984–1322) and was
  **rewritten in place to `$$ … $$`** by ~68 s (frame 2073) — Codex reformatting already-displayed
  output, dropping a delimiter in the reflow.
- **The compression mechanism.** No `\x1b[3J`/`\x1b[2J` in the whole capture (not a scrollback
  clear). Instead **170 DECSTBM scroll-region sets, 161 of them `\x1b[1;21r`** — a top-anchored
  region over rows 1-21 (the transcript area above the input box). Codex reprints/compresses the
  transcript *within* rows 1-21 rather than scrolling naturally — precisely the user's "输出的时候
  会压缩之前的输出而不是正常上滚". Lines pushed above row 1 of a `start==0` region commit to
  scrollback (vendored grid, per `55f5c41`); the reformat reflow drops a `$$` there. **Which layer
  didn't catch it:** the delimiter is lost in the PTY→history commit of the in-place reflow (same
  family as `55f5c41` / the `\\`-and-`\[`-eating), and the detector's global toggle then propagates
  that single-delimiter error across the entire downstream — history *and* grid.

### Why the seam resync (`3875209`) cannot fix it

The frozen→live `$$` resync (`lib.rs:860-907`) only acts when the scan reaches the seam with
`opening = Some(Dollars)` whose `start_index < boundary`. Here the upstream drop leaves the toggle
**closed** at the boundary (`opening: None`), so the resync **never fires** — the grid's Fourier
closer is read as a fresh opener and the cascade begins. The resync makes a *local* seam decision; it
cannot repair an *upstream-history* parity phase. Its `3875209` convergence guard
(`grid_dollars_opens_valid_block`, `lib.rs:899`) is working as designed and does **not** misbehave
here — it is simply out of the causal path.

---

## Are the two recordings the same root? Yes.

`stream-mispair.vt` (round-2 material) at its **final** state: grid-only scan = **0 blocks**, EXP-A
(prepend opener) = **5 blocks** — the identical grid-clip topology. But its final frame is
`state=Rendered source_rows=[]` — **clean**, because all five grid blocks were detected *before* the
clip and are **held**; `full_blocks=24` (much healthier history, `isolation_gap max=1`). The only
difference from round 3 is that no block streamed in after the break with no hold. **Same root; the
holds hid it in round 2.** This is the concrete proof that "红门绿、真机红" is holds masking a dead
detector, not two different bugs.

---

## Red-gate blind-spot characterization + how to actually catch it

The current gates check detection health *indirectly* and are all defeated by holds:

| gate | why green here |
|---|---|
| flash oracle / `trace_blocks` R→S | `\zeta` never placed → no flip |
| `isolation_gap` (grid-only − full) | grid itself poisoned (opener above row 0) → grid-only=0 → gap=0; also final=0; also **not enforced** anywhere |

**补法 (detection-truth gate, hold-independent).** Reconstruct the full document (history + live
grid) into one logical line stream and assert, on the **final** state and at a few mid-run
checkpoints: *every `$$`/environment delimiter present in the reconstructed document must belong to a
detected-or-legitimately-rejected block; a lone unmatched structural `$$` (odd parity in any
contiguous rendered region) is a red failure.* Two concrete signals that would have fired
immediately on this capture:

1. **Parity gate.** Count structural `$$` over the reconstructed live region (grid + the seam-frozen
   tail that renders into it). An **odd** count with a completed prompt below = a stranded block.
   (Here the grid holds 7.) This does not depend on any block ever being placed.
2. **Clipped-open gate.** When the live grid's row 0 is inside a block body (first grid `$$` closes
   something whose opener is not in the grid **and** not carried as an open fence from history), flag
   it — that is the exact round-3 topology and the exact thing the current `isolation_gap` cannot
   express (both scans see the same clip).
3. **Wire it as a hard gate.** `isolation_gap max>0` (and the parity gate) must fail the oracle exit,
   not just print. `max=3` was printed on this very run and ignored.

The `BT_PROBE_MATH_LATENCY_US` async model is **not** the gap — it converges to the same stuck state.
The gap is entirely in *what the gates measure*, not in replay fidelity.

---

## Architecture judgment (authorized overturn)

**The model is structurally unstable for this input, and seam-patching cannot converge.** The live
detector re-derives *all* `$$`/environment pairing with one global toggle over
`[≤1024 frozen lines + grid]` on every pass (`session.rs:1160-1198`, `lib.rs:807-1051`). Codex's
in-place compression (`\x1b[1;21r` reprints, format conversions `[ ]`→`$$`) is an **open-ended
source of single-delimiter parity damage anywhere in that prefix**, and a single unbalanced `$$`
desyncs everything downstream — including freshly streamed blocks. The three rounds patched three
*triggers* (odd `$$` residue; `\end{pmatrix},`; now a clipped/eaten opener) at the *seam*; but the
parity break is a **global property of the whole re-scanned window**, so trigger-by-trigger patching
is chasing an unbounded set. The prior audits' shared conclusion ("one broken delimiter in history
desyncs every later block") was correct; what was under-appreciated is that (a) the poison can live
in the **grid itself** (clip), not just history, and (b) **holds** keep the screen looking healthy
while the detector is dead, hiding the regression from every placement-based gate.

### Fix directions (suggestions only — not implemented)

1. **Bound parity blast radius: seed from the nearest *neutral* checkpoint, not a flat 1024 lines.**
   `DetectionContext::is_neutral` (`lib.rs:155-157`) already marks parser boundaries and the session
   retains per-line frozen contexts (`frozen_detection_contexts`, `session.rs:1206`). Begin the live
   scan at the nearest proven-neutral frozen line *above the grid* instead of re-deriving over the
   whole tail. An ancient eaten `$$` (id 97) then cannot reach the grid at all — it is walled off by
   the first balanced boundary between it and the seam. This bounds *every* upstream trigger at once,
   not one at a time, and is the highest-leverage change.

2. **Resync on parity, not just on a seam opener.** Generalize `860-907`: when the scan crosses into
   the grid (`index >= boundary`) and the grid's first structural `$$` would close a block whose
   opener is neither in the grid nor a carried open fence — i.e. the **row-0-clip** case — treat the
   grid as entering **closed** and let it re-pair from clean (this is what EXP-B proves recovers all
   blocks). Today the resync only handles the *Some(Dollars)* phase; the clipped-opener case leaves
   `opening = None` and is unhandled.

3. **Make holds honest about detection death.** A preserved live artifact whose source is no longer
   re-detected in the current context (full scan yields nothing ending at it) is a **stale hold over
   a dead detector**. Either (a) re-anchor holds only while the block is still detectable, so a
   parity break surfaces instead of being papered over, or (b) emit a diagnostic when a hold persists
   with zero backing detection — that is the observable that distinguishes "correctly rendered" from
   "detector died, hold survives". Round 2 passing on a dead detector is the danger this removes.

4. **Integrate the row-0 clip with the `0848375` bridge / occlusion (the ledger's staged item).** A
   block whose opener has scrolled above grid row 0 is the mirror image of the frozen→live bridge:
   here the *opener* (not the closer) is off-screen-up. Re-project such a clipped-top block via
   `clipped_top` (already contemplated for the boundary-scroll family, handoff ledger) so its opener
   is carried as an open fence into the grid — EXP-A shows this re-pairs everything including the
   fresh `\zeta`.

5. **Upstream containment of the compression reflow.** Confirm whether our scroll-region→scrollback
   commit drops the `$$`/blank under the `\x1b[1;21r` in-place reprint (a fixable commit-path bug,
   `55f5c41` cousin) versus Codex emitting genuinely unbalanced bytes during the `[ ]`→`$$`
   reformat. If ours, fixing the commit removes the *history* source of poison directly. Either way
   (1)/(2) contain the detector-side damage.

6. **Detection-truth red gate (see 补法 above)** as the standing guard so the next trigger in this
   open-ended family fails a gate instead of shipping.

---

## Evaluation of the prior fixes (保/改/废)

- **`3da6d64` + `3875209` (frozen→live boundary resync + convergence guard + prose tokenizer) —
  保 (keep).** Correct for their scenario (a single dangling opener at the seam over otherwise-
  balanced history) and they do not misbehave here — they are simply out of the causal path
  (`opening = None` at the seam ⇒ resync never fires). They neither背锅 nor漏锅; they are just
  *insufficient* against upstream-history + row-0-clip poison. The convergence guard `899` in
  particular is a genuine improvement (prevents round-2's over-abandon that stranded whole screens).
  **改 (extend), not replace:** widen the resync to the parity/closed-phase clip case (fix direction
  2), or subsume it under the neutral-checkpoint seeding (fix direction 1).
- **`0848375` (frozen/live bridge) — 保, 改.** The bridge machinery is exactly what recovers the
  Fourier block once its opener is carried as open (EXP-A). Extend it to the **opener-clipped-above-
  row-0** direction (fix direction 4).
- **`d7adce8` / `002acc7` (primary reprint/resize holds) — 保, but add a truth check.** They are load-
  bearing and correct at preserving rasters across reprints; they are *inert to the bug* here (they
  cannot cause a never-detected block to strand). Their side effect is masking dead detection from
  the gates (fix direction 3) — that is a test-observability fix, not a reason to change the holds
  themselves.
- **`89ed339` (environment-closer punctuation + swallow-radius bound) — 保.** Orthogonal; covers the
  Environment-opener case only (`lib.rs:851-859`), not this Dollars/parity case, exactly as its own
  audit stated. No change needed.
- **`isolation_gap` diagnostic (`3da6d64`) — 改 (must upgrade).** Right instinct, wrong metric for the
  grid-clip class, and never enforced. Replace/augment with the detection-truth parity gate and wire
  it to the exit code (补法).

---

## Reproduction (probes throwaway; oracle EXIT not trusted)

```
BT_PROBE_INPUT=.tmp-repaint-capture/compress-rewrite.vt \
BT_PROBE_CHUNKS=.tmp-repaint-capture/compress-rewrite.vt.chunks \
BT_PROBE_COLUMNS=104 BT_PROBE_ROWS=26 \
  cargo run --locked --offline -p bt-term --bin bt-repaint-oracle | Out-File -Encoding utf8 frames.txt
# final frame: rendered=[Fourier,entropy,Bayes] (all stale holds), source_rows=["$$","$$"] = the
#   stranded \zeta block; flash=false, exit=0, isolation_gap final=0 (max=3, ignored).
# BT_PROBE_FROZEN=1 → FROZEN[97] "\sum…\frac{\pi^2}{6}" has no opener; FROZEN[172]="$$" (Fourier
#   opener) + FROZEN[173] body straddle the seam; grid row 0 starts at "\int_{-\infty}" (mid-body).
# Core proof (throwaway detector probe, reverted):
#   grid-only clean scan               → 0 blocks
#   grid with one synthetic "$$" front → 4 blocks incl. \zeta   (missing opener supplied)
#   grid minus the leading orphan "$$" → 3 blocks incl. \zeta   (orphan closer removed)
# stream-mispair.vt final: state=Rendered, source_rows=[], yet grid-only=0 / EXP-A=5 → same root,
#   holds mask it (nothing streamed in after the break).
```

Tree clean after all probes reverted (`git status --short` empty).
