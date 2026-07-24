# Independent audit: "resize end-flash" + "many formulas stuck at source"

_Read-only audit. HEAD `59b393e`. Capture `.tmp-repaint-capture/resize-endflash.vt`
(+`.chunks`), 208 602 bytes, 5 RESIZE markers. No product code changed; all findings
reproduced with throwaway probes that were reverted (tree clean)._

## TL;DR

- **Symptom 2 ("很多公式不渲染") is a pre-existing detector bug, NOT a regression.** A bare
  `\begin{pmatrix}` whose closer line reads `\end{pmatrix},` (trailing comma) never closes,
  so the scanner stays inside an open math environment and **swallows every subsequent `$$`
  block as environment body**. All three stuck blocks (aligned / pmatrix / gaussian) are the
  victims. Proven with a unit test and a git baseline diff.
- **Symptom 1 ("到新位置后才闪") is the already-ledgered in-stream reprint flash**, surfacing
  in the resize aftermath. The four recent commits (`002acc7`/`59b393e`) genuinely preserve the
  one renderable block *through* the resize and *until its fresh raster lands* — but preservation
  **lifts the instant the fresh render lands**, and Codex's post-resize reprint storm continues
  past that point. The next reprint drops the now-live artifact to source. Not a new regression;
  the residual tail of an incompletely-closed flash.
- **Not the same root.** Different blocks, different mechanisms. Shared upstream trigger only
  (Codex reprinting its transcript).
- **The oracle cannot reproduce either flash** because it completes bt-math synchronously and
  drives `finish_resize_if_quiescent` once per chunk. Both collapse the real async gap. It
  reported `0 R->S flips` / `exit=0` on this capture while the real machine flashed. Tool-gap fix
  below.

---

## Symptom 2 — three `$$` blocks stuck at source (aligned / pmatrix / gaussian)

### Evidence chain (frame → byte → code)

**Frame.** Final resting frame `frame=3710 elapsed_us=141762582 state=Source`:
`source_rows=["$$","\begin{aligned}","\end{aligned}","$$","$$","\begin{pmatrix}",
"\end{pmatrix},","$$","$$","$$"]`. A probe dumping `frame.math_blocks` / `frame.math_failures`
on **every** frame shows both are **empty in all 3 711 frames** — the projection emits *no
placement at all* for these blocks (not a render-failure, not a source-fallback placement: nothing).

**Where they live.** `BT_PROBE_FROZEN` shows frozen history ends at `FROZEN[233] 麦克斯韦方程组：`.
The three blocks sit entirely **below** that in the **live grid** — so this is a *live
detection→render* failure, not frozen scheduling.

**Detection layer.** A `resolve_live_detection_tasks` probe on the final quiescent flush:
`RESOLVE ncand=10 nlogical=148 nblocks=1`. The **only** block the detector finds is
`start=52 end=58` — the *first reply's* `\begin{aligned}…\end{aligned}` environment. All ten
live-grid candidates (the actual `$$` opener/closer rows, logical 116–143) report
`matched=false`. A `complete_live_worker_result` probe confirms it end-to-end: of **136** live
tasks scheduled across the whole capture, only **20 ever `resolved=true`** (all the aligned
environment, 63–87 s); **pmatrix and gaussian never resolved once**, and **nothing resolves after
frame 1465 (~87 s)**. Candidates are scheduled (det climbs to 136) but the scanner never pairs them.

**Byte / code root cause.** The full logical context fed to the scanner (delimiter lines only):

```
LOGID[51] [            LOGID[63] [               LOGID[86..143]  $$ … $$   (8 clean $$ pairs:
LOGID[52] \begin{aligned}   LOGID[65] \begin{pmatrix}            E=mc^2, quadratic, euler,
LOGID[58] \end{aligned}     LOGID[68] \end{pmatrix},            gaussian-int, fourier,
LOGID[59] ]                 LOGID[71] ]                          aligned, pmatrix, gaussian)
```

`$$` parity is perfectly balanced (16 `$$` = 8 pairs). The block detected is the `aligned`
**environment** at 52–58 (bare `\begin{aligned}` is a standalone display environment). At
`LOGID[65]` `\begin{pmatrix}` opens another environment — `is_math_environment` accepts pmatrix
(`crates/bt-detect/src/lib.rs:493-494`). Its closer `LOGID[68] \end{pmatrix},` **fails to close it**:

```rust
// crates/bt-detect/src/lib.rs:1261-1265  closing_delimiter(), Environment arm
let closing = format!(r"\end{{{environment}}}");
let start = trimmed_end.checked_sub(closing.len())?;      // trimmed_end trims only [' ','\t']
(text.get(start..trimmed_end) == Some(closing.as_str())).then_some((start, trimmed_end))
```

`trim_end_matches([' ', '\t'])` does not strip the trailing `,`, so the suffix is `matrix},`,
not `\end{pmatrix}` → no close. The opening persists, and the scanner's catch-all
`if opening.is_some() { continue }` (`crates/bt-detect/src/lib.rs:940-943`) then **swallows every
later line**, including all `$$` openers/closers (86–143), as environment body. The same happens
independently at the *second* reply's `LOGID[130]`→`LOGID[133] \end{pmatrix},`.

**Direct proof (throwaway unit test, reverted):**

```
POISONED  [ "matrix:", "\begin{pmatrix}", "a & b", "\end{pmatrix},", "energy:", "$$","E=mc^2","$$" ]
   → detect_math_blocks_in_context_with_options = []        (the $$ is swallowed)
CLEAN     [ …same but "\end{pmatrix}" (no comma)… ]
   → = [(2,4),(6,8)]                                        (pmatrix env AND the $$ block)
```

### Regression classification: **NOT a regression (pre-existing)**

- The same capture replayed at `4ff9fc6` (the commit *before* the four) produces a
  **byte-identical final stuck state** (same 10 `source_rows`, only the aligned env ever rendered).
- `git log 4ff9fc6..HEAD -- crates/bt-detect/src/lib.rs` = only `0848375`; its diff does **not**
  touch `closing_delimiter`, `is_math_environment`, or the environment scanner. The bug predates
  the whole line.
- Upstream enabler (not our bug): Codex eats `\[`→`[` (known, `restore_stripped_environment_newlines`
  territory). The eaten bracket demotes `[ … \begin{pmatrix} … ]` to a bare top-level
  `\begin{pmatrix}`, exposing it to the environment scanner. This is why "此前这些块类型能渲染":
  in sessions without a preceding unterminated matrix environment, `$$` blocks pair normally.

### Fix directions (suggestions only)

1. **Preferred / principled:** matrix-family environments (`matrix`/`pmatrix`/`bmatrix`/`Bmatrix`/
   `vmatrix`/`Vmatrix`/`smallmatrix`) are *inner* environments — they are never a standalone
   display trigger without an enclosing `$$`/`\[`. Split `is_math_environment` so only
   display-level environments (`align*`, `equation*`, `gather*`, `aligned`, `multline*`, `split`,
   `cases`, `alignat*`, `flalign*`, `gathered`, `alignedat`) can *open* a top-level block in
   `opening_delimiter`. A bare `\begin{pmatrix}` then never opens, and later `$$` pair normally.
2. **Defensive / orthogonal:** bound an unterminated environment opening. A display opening that
   does not close should be *abandoned* at a hard structural boundary (blank line followed by a
   prompt/`•`/`›` marker, or the start of a new proven `$$`/`\[` region) instead of consuming the
   rest of the window. This also hardens the ledgered "某些块不渲染" Codex cases, which are likely
   the same unterminated-opening class.
3. **Weakest (avoid as sole fix):** tolerate trailing punctuation after `\end{env}`. Higher
   over-match risk; only worth it combined with (1).

---

## Symptom 1 — "公式到新位置后才闪一下"

### Why the replay shows nothing, and what actually flashes

The **only** block that ever renders in this session is the first reply's `\begin{aligned}`
environment (logical 52–58) — it renders 63–87 s (band shrinking `6..12`→`0..0` as it froze
upward, i.e. the `0848375` bridge). **No `$$` block ever renders** (Symptom 2). So the block that
flashes is this aligned environment, a *different* block from the stuck `$$` set.

In the replay it reverts at `frame=1476 elapsed_us=87215124` (chunk seq ~1447, byte offset
~104 912; the reprint that fed `› 请用$$包裹` is seq 1442–1446, bytes ~104 580–104 711). At that
instant **no resize epoch is active** (first RESIZE is at 107 s), so
`primary_resize_preservation_active` is false, the record holds a *live* artifact (not
stale-pending), and `invalidate_live_row` (`crates/bt-term/src/session.rs:1596-1621`) takes the
`else` branch and **drops it** — `inv 0→1`. This is a pure **in-stream reprint flash**, the exact
mechanism the handoff ledger already lists ("流内重印闪(非 resize)").

### Mechanism of the *post-resize* flash (code path)

The four commits preserve correctly, but preservation has a hard end:

- `primary_resize_preservation_active` = `resize_epoch.is_active() || has_pending_resize_relayout()`
  (`session.rs:1391-1394`). `has_pending_resize_relayout` (`:1405-1410`) is true only while some
  record is **stale-pending** (`artifact.is_none() && stale_artifact.is_some()`).
- When the fresh raster lands, `apply_live_worker_completion` (`:1917-1949`) installs a record
  with `artifact: Some(fresh), stale_artifact: None` — atomically (verified: it `insert`s the
  fresh record and `retain`s away overlaps in the same call, so there is no neither-frame). That
  clears the last stale-pending record → `has_pending_resize_relayout` becomes false.
- `finish_resize_if_quiescent` (`:939-960`) then `mark_quiescent()`s the epoch and, on Primary,
  **`self.offscreen_decorations.clear()`** (`:955-957`).
- After both, `primary_resize_preservation_active` is **false**. The very next Codex reprint hits
  `invalidate_live_row`: the record now has a live artifact, `stale_pending=false`,
  `offscreen_preservation_active=false` → `else` branch → **dropped to source** → re-detection
  re-renders it a few frames later. **Flash.**

This is exactly "到新位置后才闪": reanchor + fresh render land the block at its new position
(protection lifts), *then* the next reprint flashes it. The observation *changed* across
`002acc7`→`59b393e` precisely because each fix pushed the flash later in the timeline:
`002acc7` (preserve during epoch) → flash moved to epoch-close; `59b393e` (preserve until fresh
lands) → flash moved to *after* the fresh render at the new position. The tail — the first reprint
after protection lifts — is still open.

Secondary gap in the same area: `finish_resize_if_quiescent` clears `offscreen_decorations`
unconditionally on Primary (`:956`). A stale-pending record sitting off-band at the quiescence
instant is dropped. In practice this only fires when output is already silent (else the epoch is
not quiescent), so it is a narrower window than the reprint path above, but it is a real second
edge.

### Regression classification: **NOT a new regression**

Before the four commits, primary had *no* preservation: a resize ran
`invalidate_all_live_decorations` → full wipe → immediate flash *at* resize. The commits are
working as designed and strictly reduce the flash window. The residual is the pre-existing
in-stream reprint flash (already on the ledger), now only visible in the brief post-protection
window. `59b393e` did not introduce a flash; it narrowed one and shifted its timing.

### Fix directions (suggestions only)

- **Real fix = the ledger's larger item:** generalize the alternate-screen clear+home
  snapshot-repaint (`snapshot_alternate_repaint`/`finish_alternate_repaint`,
  `session.rs:1158-1220`) to **primary**, so *any* transcript reprint re-anchors renderable live
  artifacts by exact-source equality instead of dropping them — decoupled from the resize epoch.
  That covers the whole Codex reprint storm, of which the post-resize reprints are just a subset,
  and subsumes both Symptom 1 and the standalone "流内重印闪".
- **Narrower stopgap:** keep a short primary preservation *grace* after the epoch quiesces / the
  fresh render lands, so a reprint arriving immediately after protection lifts still re-anchors.
  This is a timing heuristic (violates the "no clocks" preference) and only masks the common case;
  prefer the snapshot generalization.

---

## Are the two symptoms the same root? **No.**

| | Symptom 1 (flash) | Symptom 2 (stuck source) |
|---|---|---|
| Block | first-reply `\begin{aligned}` **environment** (renders) | the three `$$` blocks (never render) |
| Layer | live preservation / reprint timing (bt-term session) | detector environment pairing (bt-detect) |
| Root | protection lifts before Codex's reprint storm ends | `\end{pmatrix},` never closes → swallows `$$` |
| Regression? | no (residual of ledgered reprint flash) | no (pre-existing; predates the four commits) |
| Determinism | timing-dependent (async) | deterministic (pure detector logic) |

Only shared factor: Codex reprinting/reflowing its transcript is the upstream trigger that
*exercises* both. The mechanisms are independent, and they even affect **disjoint blocks**.

---

## Why the replay could not catch this, and how to fix the tool

The oracle (`crates/bt-term/src/bin/bt-repaint-oracle.rs`) is **synchronous in two places that
matter**:

1. `complete_pending_math()` renders every queued bt-math task **immediately** inside `feed()` /
   `advance_before()` (`:57`, `:71`). Real bt-math is off-thread and lands one or more stability
   intervals later.
2. `advance_before` calls `finish_resize_if_quiescent` **once per chunk** (`:55`). The handoff
   already flags this; combined with (1) it means the *epoch-close → fresh-render-lands* interval,
   and the *stale → fresh* interval, are both compressed to zero.

Consequence: the stale→(gap)→fresh→reprint ordering that produces Symptom 1 never forms in replay.
The block either stays rendered or reverts cleanly; the transient is squeezed out. On this exact
capture the oracle returns `exit=0` and `trace_blocks.py` reports `TOTAL R->S flips: 0` — a **false
green**, the precise failure mode the handoff warns about.

Two additional oracle blind spots surfaced:

- `trace_blocks.py` `source_exposes()` does not recognise a **multi-line** block's body when its
  rows are split across `source_rows` (it only matches whole-string or single-`$$` forms). It
  printed `flips=0 final=RENDERED` for the aligned block while the final frame actually shows it at
  source. Multi-line exposure must be matched row-by-row against the block's logical lines.
- `FormulaFlashOracle` derives "rendered" from `frame.math_blocks` display transitions, so a block
  that is **never placed at all** (Symptom 2) is invisible to it — no placement, no flip. It cannot
  distinguish "correctly not math" from "provable block the detector dropped".

### Tool-gap fixes (to actually catch these next time)

1. **Model bt-math latency.** Give the oracle a deferred completion queue: when a task is taken,
   stamp it with `now + LIVE_MATH_STABLE_INTERVAL` (or a configurable latency) and only render/apply
   it when the replay clock passes that stamp — never synchronously in the same feed. This restores
   the protection-lift-vs-fresh-landing ordering.
2. **Drive the lifecycle on the app's cadence, not per chunk.** Advance
   `finish_resize_if_quiescent` / `advance_live_stability` on deadline-driven ticks between chunks
   (mirroring the winit `WaitUntil`), so the post-quiescence gap physically exists in replay, and so
   a reprint chunk can land *after* protection lifts.
3. **Add a document-level detection assertion** independent of placement history: for the fully
   reconstructed final scrollback, any `$$`/environment block that is *provable in isolation* (its
   opener+body+closer detect when scanned alone) but is **absent from `math_blocks`** is a detection
   regression. That would have flagged the pmatrix-swallow immediately, without depending on whether
   the block ever rendered earlier.
4. **Fix `trace_blocks.py` multi-line exposure** so `RENDERED→SOURCE` is counted when a multi-line
   body is exposed as split `source_rows`.

## Reproduction recipe (for the fixer)

```
# Symptom 2 — detector unit (add to crates/bt-detect tests, or a scratch bin):
detect_math_blocks_in_context_with_options(
    [ "\begin{pmatrix}", "a & b", "\end{pmatrix},", "$$","E=mc^2","$$" ], Known, default)
  == []            # bug; with "\end{pmatrix}" (no comma) it detects the $$ block.

# Full-capture confirmation (probes are throwaway; oracle EXIT is not trusted):
BT_PROBE_INPUT=.tmp-repaint-capture/resize-endflash.vt \
BT_PROBE_CHUNKS=.tmp-repaint-capture/resize-endflash.vt.chunks \
BT_PROBE_COLUMNS=104 BT_PROBE_ROWS=26 \
  cargo run --locked --offline -p bt-term --bin bt-repaint-oracle | Out-File -Encoding utf8 frames.txt
# final frame: math_blocks empty, source_rows = the 3 stuck $$ blocks.
# Baseline 4ff9fc6 → byte-identical final state (proves pre-existing).
```
