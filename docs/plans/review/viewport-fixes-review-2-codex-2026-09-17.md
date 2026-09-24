# Viewport fixes: round 2, 2026-09-17

Reviewed `6192b5df` and `c733728c` over `8c9f9264`, including the round-1 R1/R2 reproductions. All product references below are at `c733728c`; `V` means `crates/bt-viewport/src/lib.rs`, `S` means `crates/bt-term/src/session.rs`, and `A` means `crates/bt-app/src/main.rs`.

## Verdicts

- **K3 / bridge sizing (`19389094` + `6192b5df`): merge into 0.4.2. R1 closed.** The shared measurement at V:4180 removes the reproduced wrapped-prefix overcharge and false admission rejection.
- **K4 / command jump (`8c9f9264` + `c733728c`): merge into 0.4.2. R2 closed.** V:2892 chooses the resting picture by offset while V:2847 retains the requested anchor.
- **Combined branch `fix/bridge-band-and-idempotent-jump` at `c733728c`: merge into 0.4.2.** No new must-fix found in this review's scope; validation limits and the unchanged F3 limitation are below.

## R1: measurement and height overrides

V:4180 verifies the contiguous projected tail, rejects both math and inline-path artifacts, sums measured visual rows, and requires each measured height to equal its row count times cell height. V:4206 adds staging length separately. Admission (V:4296), row allocation (V:4419), and bridge/frozen-prefix geometry (V:2991) share that measurement. An unmeasurable math prefix is refused at V:4311 with `reason=bridge-prefix-not-measurable`.

Not rejecting `artifact_heights` membership is acceptable: membership alone does not prove non-cell geometry. `project` derives both rows and height from an override (V:4703); a 36 px override at 18 px pitch has two measurable rows, while 37 px fails V:4195's equality. Scratch checks confirmed containment/non-overlap for the former and source fallback for the latter. A pending override does not change the already-projected tree until projection; the required subsequent live sync must read that new tree. In production, S:8360 calls `sync_math_artifacts`, which rebuilds the override map from actual frozen artifacts (V:4060); no bt-term caller independently sets bare height overrides.

## Ordering attack: no published intermediate frame found

The complete bt-term caller search found one direct `project` call (S:8375), one `sync_live_math_artifacts` call (S:8465), and one `continuous_frame` call (S:7637); no `live_frame` bypass. `sync_projection_state` is called only by `new_projection` (S:7612) and `refresh_projection` (S:8284). Its live sync at S:8373 indeed precedes projection and can read stale tail IDs or heights.

Every bt-term composition passes through `viewport_frame`: S:7623 re-syncs live artifacts before S:7637 composes. That sync reconstructs candidates from session records (S:8378), redoes admission and every live-row height, and replaces both accepted artifacts and the prefix map (V:4466). The first refusal does not discard a session record. Relayout and detection-revision changes can also project internally (V:4866, V:4879), but introduce no composition bypass. Thus the second sync repairs both provisional refusal and stale sizing before presentation; no one-frame source flash or stale-height publication was found along these callers.

The extra scratch sequence starts with unprojected history, syncs, projects, re-syncs, and successfully renders the 140 px wrapped bridge. This confirms the recovery mechanism; direct viewport clients must preserve that ordering when history changes. A trace from the first sync can still report a provisional refusal without a displayed source frame.

## R2: every `bottom_identity` reader

The local boolean at V:2892 has exactly four readers: initial window/first-row selection (V:2894), live-plane normalization (V:2913), presentation offset (V:2921), and frame-top positioning (V:2945). All describe the picture; none enables following output or changes scroll state.

- **Passive output:** source resolution and clamping occur before that boolean (V:2820), retaining `Anchored` at V:2847. When output gives the requested landing room, its offset becomes positive. The existing expanded-band gate checks that sequence and repeat-press equality (V:10697); offset-zero presentation does not undo K4.
- **Unread:** growth still uses the previous `is_scrolled()` offset (V:2739), and reset still explicitly tests `Bottom` (V:2868). Anchored-at-zero retains existing unread count and does not count the first growth from zero, unchanged from round 1. Geometric status counts remain separate.
- **Input:** A:87163 still calls `scroll_to_bottom` unconditionally for qualifying input; V:2471 explicitly handles non-Bottom state even at zero offset. Input intentionally restores following; merely rendering the resting picture does not.
- **Resize hold:** V:2880 still requires primary screen, active resize, and displaced review state. It does not read `bottom_identity`. Origin also remains `Anchored` (V:3698), so equal row maps/status do not imply identical frame metadata.

## Recreated round-1 scratch tests

Each used a temporary bt-viewport integration test with product code untouched and history projected before final live sync.

| Test | Result at `c733728c` |
| --- | --- |
| Staging-only, 80 px raster | PASS: one prefix row; 80 px band; next row below band. |
| Negative residual, 12 px raster / 36 px prefix | PASS: 54 px source-band floor; raster contained; next row below band. |
| Wrapped prefix, 96 px raster | PASS: four prefix rows; exactly 96 px total, not 114 px. |
| Wrapped prefix, 140 px raster | PASS: admitted with 68 px live share; exactly 140 px total. |
| Short live raster, 36 px / three source rows | PASS: clamped zero-offset row map and status equal rest; extent unchanged; requested anchor retained. |

The sixth scratch test covered the second-sync recovery and the 36/37 px bare height overrides described above; it passed. The committed idempotence gate also passes with both 36 and 96 px rasters (V:10663).

## Validation and boundaries

- `cargo test -p bt-viewport -j 4`: 141 unit tests, 3 existing integration tests, and 6 scratch tests passed; 0 doc-tests. An initial scratch-only import error was corrected before this successful run.
- `cargo test -p bt-term --lib relief -j 4`: succeeded, but **0 tests matched; 445 filtered out**. The three regression gates at S:22940, S:23054, and S:23097 contain `reliev`, not `relief`, in their names; they were not run under the exact allowed commands. This is a coverage gap, not a passing relief gate.
- `cargo test -p bt-term --lib bridge -j 4`: all 4 tests passed; 441 filtered out.
- Prior **F3 remains unchanged**: frozen-only review can still lose a bridge because placement requires live-window and live-band intersection (V:3276, V:3325). These commits do not claim to fix it.
- Scratch source was deleted. Product code remains unchanged; no application was launched or process terminated. This review is the only committed change.
