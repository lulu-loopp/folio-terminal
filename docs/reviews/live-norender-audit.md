# Independent audit: "吃输出" + "完整 $$ 块间歇性不渲染,zoom 才出" (pure-stream Codex)

_Read-only 审因. HEAD `d0dda35` (worktree, based on main HEAD). Main capture
`.tmp-repaint-capture/live-norender.vt` (+`.chunks`), 186 711 bytes, 104×26, 2 390 chunks,
one initial RESIZE marker only (104×26 @0.5s — no zoom, no manual resize). All findings
reproduced with throwaway probes that were reverted; worktree tree clean._

## TL;DR

- **Both symptoms are one root: a detector `$$`-pairing desync in the scanned history, and it
  is PRE-EXISTING — not any of today's commits.** The live detector re-scans the *entire* frozen
  history tail (1 024 lines) prepended to the live grid on every pass. The committed history holds
  an **odd number of structural `$$` delimiters (41)** — one display block (a `\int_{-\infty}` /
  Gaussian) lost its opening `$$` in Codex's scroll-region reflow, so the scanner reaches the live
  grid **already inside an open `$$`**. The grid's first `$$` is then consumed as that dangling
  block's *closer*, shifting every grid block's opener/closer role by one, so **all five clean
  `$$` blocks on screen (Einstein / det(A−λI) / A⁻¹ / Bayes / entropy) mispair and never render.**
- **Proven by isolation:** the same five blocks scanned *without* the poisoned history prefix (a
  clean-context grid-only scan — exactly what a fresh reprint / zoom produces) detect **5/5**,
  including the Einstein block that has a blank line in its body. Scanned *with* the full history
  they detect **0/5**. That divergence is the whole bug, and it is why **zoom "fixes" it**: zoom
  forces a fresh reprint / clean re-detection that bypasses the poisoned history re-scan.
- **"吃输出" is not literal output loss and not a scrollback clear.** The capture contains **zero
  `\x1b[3J` and zero `\x1b[2J`** — Codex is not clearing scrollback here (so this is neither the
  handoff's "Codex 主动清史" nor the `55f5c41` scope-class bug). Every formula line is present in
  the final document as source. The user's "吃输出 / 部分内容不显示" is the same non-render:
  formulas frozen as raw `$$…$$` source read as "eaten / missing output."
- **The prime suspect `d7adce8` (primary suppress window) is exonerated.** Instrumented: 385
  open/close cycles, dwell **1–2 feeds median (max 23)**, and only **13** row-damages ever
  suppressed under a decoration (two bursts). It works as a brief storm, is never resident, and
  masks no streaming output. It cannot cause either symptom.
- **Today's line did the opposite of "吃输出": it removed the flash.** Baseline `4ff9fc6` flags
  **1 476 flash frames** (exit 1) on this capture; HEAD flags **0** (exit 0), even in the async
  latency model. HEAD also renders *more* (1 209 vs 620 content frame-instances). No content is
  hidden by any of today's commits.

---

## Symptom 2 — "完整 $$ 块间歇性不渲染" (the primary root)

### Evidence chain (frame → scan → parity → byte)

**Frame.** Final resting frame `frame=2441 elapsed_us=383036120 state=Source
detections=91 invalidations=0`, `source_rows` = ten `$$` = five complete blocks stuck at source:

```
$$ G_{\mu\nu}+\Lambda g_{\mu\nu} [blank] \frac{8\pi G}{c^4}T_{\mu\nu} $$   (Einstein)
$$ \det(A-\lambda I)=0 $$
$$ A^{-1}=\frac{1}{\det A}\operatorname{adj}(A) $$
$$ P(A\mid B)=\frac{P(B\mid A)P(A)}{P(B)} $$
$$ H(X)=-\sum_x P(x)\log P(x) $$
```

A probe on every frame confirms **none of these five ever render once** in the whole 2 442-frame
run (they are Rendered 0 times; the flash oracle sees no placement at all for them).

**Detection layer.** At the final quiescent flush the live initial context is **clean**
(`DetectionContext { fence: None, opening: None, prefix: Known }` — the frozen checkpoint at the
head of the 1 024-line tail is default/Known). `live_candidate_rows` finds all ten delimiter rows
`[0,4,6,8,10,12,14,16,18,20]`, but `resolve_live_detection_tasks` marks **every one
`resolved=false`**. The authority is one full-context scan
(`resolve_live_detection_tasks`, `crates/bt-detect/src/lib.rs:1470-1507`, calling
`detect_math_blocks_in_context_with_options`) over **frozen tail + grid**.

**What the full scan produces vs. the grid alone (the decisive test):**

```
FULL history+grid scan  →  8 blocks, all in early history (logical ids 83–126), NONE in the grid.
GRID-ONLY scan (clean)  →  5 blocks:  Einstein 1–5 (blank-line body included), det 7–9,
                                       A^-1 11–13, Bayes 15–17, entropy 19–21.
```

The five on-screen blocks are perfectly valid display math. They fail **only** because of the
history prefix the live scanner is forced to re-derive over.

**Parity / root cause.** The scanned history carries **41 structural `$$` delimiters** — 39
standalone `  $$` logical lines + 2 list-item openers `• $$` (a `›`-prefixed prose `$$` inside
`用$$包裹` is correctly *not* structural). The `$$` scanner is a pure toggle: each structural
`$$` opens (if closed) or closes (if open). **41 is odd → the scanner ends the history with
`opening = Some(Dollars)` — a dangling opener.** Entering the grid:

```
grid row 0  "  $$"   ← consumed as the CLOSER of the dangling history block (NOT Einstein's opener)
grid row 1  body …   ← now orphaned
grid row 4  "  $$"   ← now read as an OPENER … and so on, every block shifted by one
```

so the Einstein/det/A⁻¹/Bayes/entropy blocks never pair. The odd delimiter is a **lost opener**:
in the committed history the Gaussian block `\int_{-\infty}^{\infty}e^{-x^2},\mathrm{d}x=\sqrt{\pi}`
(logical id 114) sits with **no `$$` before it** — the preceding `$$` (id 113) is the pmatrix
block's closer, and the Gaussian's own opener is absent, so that block contributes a single
unmatched `$$` (its closer, id 115). This is produced by Codex's top-anchored scroll-region reflow
(`\x1b[1;21r` + `\x1b[21;1H` + LF commits to history, raw byte ≈130 306), the same reflow family
that eats `\\`/`\[` (handoff "已知非我方问题").

Code path: `live_detection_context` (`crates/bt-term/src/session.rs:1160-1198`) prepends up to
`LIVE_FENCE_HISTORY_CONTEXT_LINES = 1 024` frozen lines (`session.rs:53`);
`live_initial_detection_context` (`session.rs:1200-1213`) seeds a clean context and the scanner
re-derives all pairing from the top; `scan_math_blocks_in_context_with_options`
(`crates/bt-detect/src/lib.rs:784-986`) toggles on each structural `$$`; empty `$$`-blank-`$$`
pairs downstream are rejected by `valid_display_body` (`lib.rs:988-995`) but the toggle already
consumed them, so the parity break persists all the way into the grid.

### Why zoom rescues it

Zoom changes the layout and forces Codex to reprint the visible transcript into the live grid; the
freshly reprinted blocks pair cleanly in isolation (the desync lives in *older* frozen history, not
the fresh reprint). The audit's **grid-only clean-context scan is exactly that re-detection, and it
renders all 5**. So zoom does not repair the underlying history desync — it sidesteps it with a
clean re-scan. (This capture contains no zoom; the mechanism is proven by the isolation scan, and a
zoom capture would confirm it end-to-end — see tool gap.)

### Why "intermittent"

Blocks that stream **before** a desync-inducing lost opener enters the scanned history pair and
render normally (8 such blocks render here, logical 83–126). Once one unpaired `$$` lands in the
1 024-line tail, **every later block stops** until a clean reprint/zoom resets. As the session
accretes history, more blocks fall behind the break — hence "有时渲染有时不渲染" and eventually a
whole screen of source (the user's 6+-block screenshot).

### Regression classification: **NOT a regression (pre-existing)**

- The same capture replayed at **`4ff9fc6`** (the commit before the entire suspect line) yields a
  **byte-identical final stuck state** — same ten `$$` source rows, same five blocks never
  rendered.
- `git log 4ff9fc6..HEAD -- crates/bt-detect/src/lib.rs` = only `0848375` (bridge) and `89ed339`
  (env-closer punctuation + swallow-radius bound). **Neither touches the Dollars-opener desync.**
  `89ed339`'s swallow-radius guard (`lib.rs:815-835`) abandons a stale **Environment** opening when
  a `$$`/`\[` appears — it does **not** cover a stale **Dollars** opening, which is exactly this
  case. The bug predates the whole line and is not on any 2026-07-24 commit.

---

## Symptom 1 — "好像这次还吃输出了 / 部分内容不显示"

Distinguishing the three candidates the handoff named:

1. **Codex `\x1b[3J` scrollback clear (legitimate, non-ours):** does **not** occur — the capture
   has **0** `\x1b[3J` and **0** `\x1b[2J`. Codex is not clearing scrollback in this session.
2. **`55f5c41` scope-class swallow (our old bug, fixed):** not implicated — that produced Ignore
   classification + stuck scroll; here scroll/commit works (history grows to ~186 lines) and no
   region is dropped.
3. **A new third cause today:** none found. Comparing baseline `4ff9fc6` vs HEAD, HEAD renders
   **more** content (1 209 vs 620 content frame-instances) and hides nothing; today's commits only
   preserve rendering longer and remove the flash.

**Conclusion: "吃输出 / 不显示" is the same phenomenon as Symptom 2** — complete formulas frozen as
raw `$$…$$` source (and, at the trigger point, the one `$$` *opener line* that Codex's reflow lost,
which is the desync's cause). There is no separate output-eating mechanism, no scrollback clear, and
the suppress window is benign (below). No literal content is missing from the final document.

### Was the flash (the other reading of Symptom 1) reintroduced? No — it was removed

Under the async latency model (`BT_PROBE_MATH_LATENCY_US=50000`) HEAD produces **0 flash frames**
on this capture; baseline `4ff9fc6` produces **1 476** (the pre-desync blocks flashing to source
through Codex's reprint storm — the ledgered "流内重印闪"). The two-phase reprint work
(`1f963c9` atomic + `d7adce8` progressive) genuinely closed it here.

---

## Symptom 3 — suppress-window behaviour in this capture (d7adce8)

Instrumented (`BT_PROBE_SUPPRESS`, reverted):

| metric | value |
|---|---|
| window opens / closes | 385 / 385 (always closes; never resident) |
| dwell (feeds) | 92× =1, 222× =2, tail 3–23; **median ~2**, max 23 |
| chunks ending mid-synchronized-update | 1 487 / 2 390 (62%) — this is what *opens* windows so often |
| row-damages suppressed **under a decoration** | **13 total**, in two bursts (feeds 978, 1625) |

Although Codex's synchronized updates span chunks 62% of the time (so `primary_repaint_snapshot`
arms constantly), each window closes within 1–2 feeds and — because so few blocks are ever rendered
(the desync keeps decorations sparse) — it suppresses only 13 decorated-row damages all session.
It **masks no streaming output and blocks no detection**. The prime suspect `d7adce8` is not the
cause of either symptom. (`002acc7`/`59b393e`/`33fb866` resize preservation are inert here: the only
resize is the initial 104×26 at 0.5 s, long before any formula streams.)

---

## Are the two symptoms the same root? **Yes.**

Both are the live detector re-scanning a poisoned history: a lost `$$` opener (Codex reflow) leaves
odd `$$` parity in the 1 024-line tail, the scanner enters the grid inside an open block, and every
downstream display block mispairs → stuck at source. "吃输出" is that source-state perceived as
missing output; "间歇不渲染" is the same, block by block; zoom clears it with a fresh clean scan.
Different from the `resize-endflash` audit's Symptom 2 only in the *trigger* (there: a
`\end{pmatrix},` trailing comma; here: a lost `$$` opener) — the failure class is identical:
**one broken display delimiter in history silently desyncs every later block.**

## Culprit commit

**None of 2026-07-24.** Byte-identical stuck state at `4ff9fc6`. The suspect `d7adce8` is benign
here (13 suppressions). The detector desync predates the entire line; `89ed339`'s swallow-radius
bound is the closest existing guard and it deliberately covers only the Environment-opener case.

## Fix directions (suggestions only — not implemented)

1. **Resync at the frozen→live grid boundary (preferred).** The live grid's row 0 is a fresh render
   target; a `$$` that has been "open" since deep history has almost certainly lost its closer to a
   reflow. When the scanner crosses from the history prefix into the live-grid rows with a
   still-open **Dollars** opening whose opener sits far above, abandon that opening (mirror
   `89ed339`'s Environment swallow-radius bound, extended to Dollars) so the grid re-pairs from
   clean. This is what zoom achieves accidentally; doing it deterministically removes the symptom
   without a clock and without violating M1.9p (the grid rows are proven present).
2. **Cap the scanned prefix at the nearest neutral boundary, not a flat 1 024 lines.**
   `advance_detection_context` already tracks neutral parser boundaries (`DetectionContext::is_neutral`,
   `crates/bt-detect/src/lib.rs:153-157`). Seeding live detection from the nearest neutral checkpoint
   *above* the grid — instead of re-deriving over the full tail — stops an ancient desync from ever
   reaching the grid. Bounds blast radius generally, not just for this trigger.
3. **Upstream trigger.** The lost `$$` opener is produced in Codex's top-anchored scroll-region
   reflow commit (the `55f5c41` / `\\`-eating cousin). Worth confirming whether our scroll→history
   commit drops a delimiter line under rapid double-scroll, versus Codex genuinely emitting an
   unpaired `$$`; either way (1)/(2) contain the detector-side damage.

## Tool gap (another false green)

The oracle returns **`exit=0`, `flash=false`** on this capture while five provable blocks sit at
source — the exact blind spot the `resize-endflash` audit already filed (its tool-gap fix #3): the
flash oracle derives "rendered" from placement history, so a block that is **never placed** is
invisible to it. The signal that would have caught this immediately is the one this audit used:
**a document-level detection assertion** — any `$$`/environment block that is provable *in isolation*
(clean-context grid-only scan) but absent from the full-context `math_blocks` is a detection
regression. The grid-only-vs-full-scan divergence (5 vs 0 here) should be a red gate. A **zoom
capture** is also missing from the corpus and is needed to confirm the zoom-rescue path end-to-end.

## Reproduction (probes throwaway; oracle EXIT not trusted)

```
BT_PROBE_INPUT=.tmp-repaint-capture/live-norender.vt \
BT_PROBE_CHUNKS=.tmp-repaint-capture/live-norender.vt.chunks \
BT_PROBE_COLUMNS=104 BT_PROBE_ROWS=26 \
  cargo run --locked --offline -p bt-term --bin bt-repaint-oracle | Out-File -Encoding utf8 frames.txt
# final frame: math_blocks empty, source_rows = the 5 stuck $$ blocks; detections=91.
# Baseline 4ff9fc6 → byte-identical final state (pre-existing).
# Core proof (throwaway probe): scan the grid rows alone from a clean DetectionContext →
#   all 5 blocks detect; scan frozen-tail+grid → 0 of them (history $$ parity = 41, odd).
```
