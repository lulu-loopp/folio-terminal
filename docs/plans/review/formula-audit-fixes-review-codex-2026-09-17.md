# Formula audit fixes review — 2026-09-17

Reviewed `e5bb7d583caad4ed2a94669bb1bdcb3d5767d7a8` in the requested worktree.
Read A–E and +1 individually with `git show`; inspected their final callers and pinned dependencies.
The separately reviewed `66145615` / `d6c6f34f` changes are not reviewed again here.
Product changes: none. Temporary test additions were removed. No application was launched or process terminated.
All source locations below refer to `e5bb7d58`, unless a pinned dependency is explicitly named.

## Findings

**R1 — P2, A: the folded crop still does not name its complete pixel recipe.**
`crates/bt-term/src/session.rs:14407` computes the crop's right edge from the runs on that row;
`:14459` hashes only its left edge (`#x{x0}`). Equal composites can fold differently when their
first run starts at different absolute columns. Two partial first-row crops then share `#x0`
while the crops contain different numbers of runs. `crates/bt-render/src/lib.rs:10122` reuses that texture
without comparing its dimensions or bytes. A's new relative run offsets do not distinguish these crops.
Required: include both crop bounds (or the resulting pixel identity), with a regression using equal
relative run geometry and different leading-prose widths. Real-render/frame probe: a 40-column pane,
five runs separated by five spaces, trailing ` end`, prefixes of 1/10 characters: key `math:3559e361c28fca35#x0`,
widths 264/139 pixels and different RGBA. The shorter three-run probe did not reproduce this collision.

A second scratch probe found equal base keys with different RGBA after varying the row height/baseline
under the same `LayoutKey`: `session.rs:11776`, `:11824`, `:12043` omit vertical placement and fitted
font size except for their indirect effect on integer width. This is an API-level counterexample,
not a demonstrated ordinary settings route: current app font/DPI changes also update layout identity.
Include the measured geometry/fitted render key if the key is intended to identify every accepted task.

**R2 — P2, C: refusal fixes burst loss, but gives no fairness guarantee under continuing repaint.**
`crates/bt-term/src/session.rs:3707` always starts at the top; `:12631` hashes every structural input,
including row revisions, into every candidate's signature. Changing an early formula therefore
re-arms already serviced early rows as well as the unchanged tail. A scratch sequence with 65 formulas,
a drained 64-task queue, and eight successive row-0 updates serviced only rows 0–63 on every pass;
row 64 stayed refused with no queued/in-flight task. Stopping the updates admitted it on the next pass.
Required: preserve retry priority/age or rotate admission independently of context-signature changes.
The author's monotone-drain argument applies only while the relevant context remains unchanged.
This is scheduler-level evidence, not a launched UI repaint-loop reproduction.

**R3 — P2, D: the delayed context-menu target still lacks a tab identity.**
`crates/bt-app/src/main.rs:12917` stores `(SeatId, MathBlockAnchor)` at `:93690`; `:87059` later resolves
it through the active tab's `sessions` (`:87034`). Seats are tab-local and restart at 1 (`:770`).
Switch tabs between the deferred request and result: an equal seat/anchor in the arriving tab supplies
its LaTeX. `activate_tab` changes the owner at `:39377`; `close_every_popup` (`:47393`) does not clear
this pending field or cancel the native math menu. The platform explicitly permits nested Runtime
callbacks while that menu is open (`crates/bt-platform/src/lib.rs:6199`).
Required: retain a stable tab/leaf identity (and reject a replaced shell), or cancel the request on
owner changes. A closed, absent seat already returns harmlessly; another tab's equal seat does not.
The existing source-pin test verifies the immediate pane lookup, not this delayed ownership transition.

**R4 — P1, E: the pre-conversion stack-depth argument is false.**
`crates/bt-math/src/lib.rs:448` converts before `bound_converted_nesting`; `:751` claims at most 4,096
levels from an 8 KiB input and a two-byte minimum recursive command. In pinned `mitex-parser 0.2.4`,
`parser.rs:391` dispatches one-byte `^`/`_` to `attach_component`, which calls `content` recursively
at `:815`. Validation accepts 8,191 carets followed by `x`; a small conversion confirms the nesting.
Macro expansion breaks the byte argument independently: validation accepts a macro containing 100
consecutive `\sqrt` commands invoked 48 times, yielding 4,800 nested commands. The macro work cap is
32 KiB (`crates/bt-math/src/macro_budget.rs:33`), not the original 8 KiB. Both acceptance probes passed.
The dangerous deep conversions were deliberately not executed; this is a disproved safety bound,
not a claimed observed process crash. Even 4,096 recursive levels was never a stack-capacity proof.
Required: bound parser/converter recursion, including expanded input, before those recursive walks.
A post-conversion check and `catch_unwind` cannot contain a stack overflow.

**R5 — P3, +1: the six fixture defaults have the wrong fail-closed polarity requested in this review.**
`non_output_write: false` means no cell vetoes command-output eligibility: an authoritative primary
session turns it into `CommandOutput` at `crates/bt-term/src/session.rs:10780`. Unknown fixture provenance
should be `true`, not `false` (`crates/bt-viewport/src/horizontal.rs:1099`, `src/lib.rs:6685`, `:6718`,
`:7175`, `:12339`, and `tests/horizontal_budget.rs:128`). Change those explicit fixture defaults.
This is a fixture-contract gap, not evidence of production prompt misclassification: viewport fixtures
do not run the session classifier, and `HistoryEntry` itself defaults to `Ineligible` (`bt-doc/src/document.rs:274`).

## Checks that do not add findings

- **A:** base keys include run index, rounded pixel x, width, source, mode, kind, detection and full layout.
  DPI, physical em, font/theme revisions and wrap width are in layout (`bt-doc/src/versions.rs:111`).
  SGR bold/colour inside source cells is not consumed by this math rasterizer; ink comes from the theme
  (`session.rs:11646`; `bt-app/src/main.rs:10411`).
  Width remaining before a fold admits/rejects a run; it does not horizontally refit it (`session.rs:11773`).
  Reprinting identical geometry does not add occurrence IDs or row generations to the key. Genuine
  geometry changes can churn textures, but the renderer's 64 MiB LRU remains bounded (`bt-render/src/lib.rs:6592`).
- **B:** `exact_live_source_match` refuses multiple matches before rebasing (`session.rs:12507`). A scratch
  test moved a unique source to a different row and wrap width and checked every rebased row offset,
  both source offsets, band length and unchanged birth fields. Duplicate visible sources were refused.
  A unique replacement after the original disappears remains the pre-existing text-only identity
  limitation; rebasing does not establish occurrence ownership, but does not introduce that matching policy.
  Only a complete all-grid match is restored; genuinely bridged/staging-prefix sources remain parked.
  A complete match clears both obsolete prefixes (`:6580`) and rebuilds grid segments (`:12424`).
  `created_start` has no current-band containment reader: its remaining assertion is against
  `source_start_offset`, now zero (`:13400`). The 40/20/40/20 sentinel test is the correct resize gate.
- **C:** a refused row's signature is cleared (`:3781`). Although settled rows have no new stability
  timer (`:3647`), frame publication retries them through `schedule_visible_artifacts` (`:7664`), and
  accepted completions publish a frame (`bt-app/src/main.rs:82913`). Quiet bursts therefore drain
  without new damage; the remaining defect is continuing-change fairness, not the original lost-row case.
- **D:** immediate hits, copy, both toggle measurements and the delayed toggle carry the press seat.
  Missing seats fail closed; owner changes settle an animated toggle before switching tabs
  (`main.rs:86673`). This correctly fixes same-tab split-pane ambiguity.
- **E:** release explicitly uses `panic = "unwind"` (`Cargo.toml:221`, `:245`), with fat LTO and one codegen unit.
  The whole render is caught and the thread-local guard unwinds cleanly (`bt-math/src/lib.rs:75`, `:408`).
  The engine is reused; pinned `typst-as-lib 0.16.0` builds a fresh world in `lib.rs:117`, and
  `comemo 0.5.1` inserts results only after successful computation (`memoize.rs:71`). Fault-injection
  tests and subsequent rendering pass; injected faults occur before engine stages, not mid-engine mutation.
- **+1:** the deleted private call chain was history coverage -> region coverage -> `selection_covers`.
  No app or copy caller used it. Selection and command-output text still use `selection_overlaps`
  (`session.rs:7831`, `:7894`). Folding before padding trim is consistent across live/frozen cells.
  A scratch primary-screen test confirms first-byte non-output provenance and frozen `Ineligible`
  without integration. Literally "Ineligible everywhere" has an existing alternate-screen exception:
  `inline_math_site` returns `AltScreenContent` at `:14671`; +1 does not change that previously reviewed policy.
  Terminal output history is not persisted (`bt-persist/src/session.rs:503`; terminal schema in `layout.rs:109`),
  so there are no deserialized history entries needing a migration/default for this field.

## Executed checks

Every invocation used `cargo test` with `-j 4`. Baseline totals: **759 passed, 0 failed, 1 ignored**.
- `-p bt-term --lib session::tests`: **312**; `-p bt-term --test lifecycle_matrix`: **42**.
- `-p bt-detect`: **151** (150 unit + 1 integration); `-p bt-viewport`: **144** (141 + 3).
- `-p bt-math`: **51 passed, 1 ignored** (40 unit + 11 integration); `-p bt-render --lib math`: **4**.
- `-p bt-app --bin folio formula`: **50**; `-p bt-app --bin folio panic`: **5**.
Scratch runs: `bt-term --lib review_scratch` **5 passed**; repeated `bt-math` **53 passed, 1 ignored**, including two additional probes. All seven temporary probes were removed; they characterize the above counterexamples/checks, not fixes.

## Verdicts

| Commit | Verdict | Reason |
|---|---|---|
| A `8bdbe606` | merge with must-fixes | R1: crop key still aliases different pixels |
| B `30a4f846` | merge into 0.4.2 | matched geometry is consistently rebased |
| C `fe1cb58f` | merge with must-fixes | R2: retry has no continuing-change fairness |
| D `c2158209` | merge with must-fixes | R3: delayed target needs its tab owner |
| E `0a71adef` | hold | R4: converter stack safety is unproven and the stated bound is false |
| +1 `e5bb7d58` | merge with must-fixes | R5: fixture polarity; production cell fold is sound |
| Branch | **hold** | resolve R1–R5, especially the pre-conversion recursion guard |
