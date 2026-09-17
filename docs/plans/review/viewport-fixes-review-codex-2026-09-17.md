# Viewport fixes: narrow review, 2026-09-17

Reviewed `19389094` and `8c9f9264` over `d1db4abd`; all product file:line references below are at `8c9f9264`. Scope: K3 bridge height, K4 command-jump identity, their accounting and presentation callers. Product code remained unchanged.

## Verdicts

- **19389094: merge with must-fixes** — R1 below. The ordinary one-row-prefix K3 case is fixed, including staging-only prefixes, but prefix IDs are not a height measurement.
- **8c9f9264: merge with must-fixes** — R2 below. Keeping the requested anchor fixes the tested K4 sequence; it also exposes a distinct zero-offset picture for a contracting live band. The follow-output choice is described separately, without making the product ruling.

## R1 — medium: wrapped frozen prefixes overcharge the live band

`crates/bt-viewport/src/lib.rs:1792` counts `frozen_prefix.len() + staging_prefix.len()` and subtracts that count times cell height. A frozen prefix element is a transcript ID, not necessarily one displayed row: plain history is measured with `frozen_visual_line_count` at `crates/bt-viewport/src/lib.rs:4652`, and its height is assigned at line 4679.

Reproduction: extend the new bridge fixture with a finalized 48-character body line after the finalized opener, projected at width 32. Keep its staged body and live closer, 18 px cells and 96 px raster. The two frozen IDs occupy three rows; staging occupies another. Actual prefix height is 72 px, but the helper subtracts 54 px. It assigns 42 px to the live closer instead of 24 px: the bridge is **114 px rather than 96 px**, with needless whitespace and displacement of following text. This can occur when a frozen formula body wraps after narrowing the pane.

The visible-text floor amplifies this: change the same fixture's raster to 140 px. Its actual live share is 68 px, within the 12-row pane's 72 px limit, but the calculated 86 px is rejected at `crates/bt-viewport/src/lib.rs:4265`. The ready bridge falls back to source. Both failures were reproduced with scratch integration tests.

The claimed plain-prefix precondition is **not enforced where either share is computed** (`crates/bt-viewport/src/lib.rs:4248`, line 4355). The later bridge check verifies tail identity and rejects `math_artifacts` at line 3029; it does not prove one row per ID. Even plain wrapped prefixes pass it. Staging is explicitly one cell per row (line 3316). Frozen lines can also carry appended path-image height (line 4657); the bridge check does not test that map. Session retirement rejects rendered frozen display duplicates (`crates/bt-term/src/session.rs:10949`), not every non-cell-height prefix.

Must-fix: derive the residual from validated, measured prefix geometry shared with bridge construction; do not equate IDs with rows. Apply the same geometry to admission and row allocation. Pin wrapped-prefix sizing and the 140 px admission case, and explicitly reject or account for decorated/nonplain prefixes before charging their height.

## R2 — medium: a short live raster gives offset zero two pictures

Reproduction: take the new jump fixture and change its raster from 96 px to **36 px**, leaving the three-row band, 18 px cells, history, live-row-1 mark and -8 px lift unchanged. Primary all-live bands intentionally retain free height (`crates/bt-viewport/src/lib.rs:1796`): these three source rows now occupy 36 px instead of 54 px.

The first clamped press still reports offset zero and the same extent, but the scratch test fails the new fixture's row-map equality assertion: the resting frame starts with live row 0 at y=0; the anchored frame starts with the last history row and puts live row 0 at **y=18 px**. Typing or scrolling back to Bottom removes that row again. This is a visible regression introduced by retaining the formerly demoted state.

Cause: `crates/bt-viewport/src/lib.rs:2889` now retains Anchored unconditionally. Bottom chooses the live-plane start at line 2926 and uses nonnegative overflow at line 2984; Anchored resolves the relief-reduced ceiling through the document at line 2935. When total live height contracts, that ceiling lies *before* the live plane, so the line 2945 normalization does not apply.

Must-fix: preserve anchor intent while making its clamped resting presentation agree with Bottom for contracted bands too. Add this 36 px case alongside the expanded-band gate. This finding does **not** justify removing relief from the anchored ceiling.

## Remaining bridge attacks and accounting

- Negative residual is safe: saturating subtraction followed by `.max(source_band_height)` (`crates/bt-viewport/src/lib.rs:1802`) leaves the live source rows intact. A 12 px raster against a 36 px frozen/staging prefix passed the scratch fit and non-overlap assertions. A staging-only 80 px raster also passed.
- Viewport clipping does not rescale the share: allocation precedes window slicing (`crates/bt-viewport/src/lib.rs:4355`, line 3322). A bridge reads its combined height from accumulated presented rows and extrapolates missing prefix rows at cell pitch (line 3406). Partial visibility is still clipping, not a promise to show the entire raster in every viewport.
- Terminal-edge clipping/occlusion is distinct: genuine bottom clipping or fresh occlusion retains `full_clipped_rows` and `clipped_top_rows` at `crates/bt-viewport/src/lib.rs:4347`; only the visible distributed slice is assigned at line 4367. Stale-preview and phantom-top rules remain. The admission call uses visible band rows, whereas allocation can include clipped rows; the two calls share the helper, not necessarily identical arguments. No additional concrete clipping regression was reproduced.
- `validate_shape` exempts bridges only from top alignment (`crates/bt-viewport/src/lib.rs:888`). `MathBlockBeyondBand` still checks a present band-end row at line 913, and outside-live-row overlap is checked at line 919. It does not assert raster containment and cannot catch R1's oversized band. The new bridge test separately checks containment and movement of the next row (lines 9594, 9601, 9618).
- Grown live row prefixes feed live height and total height (`crates/bt-viewport/src/lib.rs:2722`, line 2796), relief (line 2757), and the common extent (line 2806). Rows-above uses remaining unrelieved overflow (line 2988); lines-below uses pixel offset in cell units minus blank tail capacity (line 2973), not the unread counter. R1's excess height therefore propagates into these calculations; there is no independent correction downstream.
- **Prior F3 remains unchanged:** the entire placement loop is inside the live-window intersection at `crates/bt-viewport/src/lib.rs:3322`, and line 3371 additionally culls by live-band intersection. A viewport containing only the frozen portion still loses the bridge picture. Neither commit changes those gates; this is not a new regression or a claim that K3 fixes F3.

## Preserved anchor: follow-output and input behavior

At offset zero the clicked mark remains Anchored (`crates/bt-viewport/src/lib.rs:2889`). Without intervening input, another 50 output rows do **not** establish Bottom-follow mode: each frame resolves the same source anchor and clamps its requested y (lines 2864, 2872). Once enough content exists below it, the view stays at that command with the requested lift while the offset grows, provided its source survives. Deleted/unresolvable anchors take the existing fallback at line 2890.

Typing into the shell **does restore Bottom**, even if the offset is still zero. Keyboard/IME/paste/file-row input qualifies at `crates/bt-app/src/main.rs:18949`; `send_user_input` invokes the rule at line 87226; `return_to_live_for_input` calls `scroll_to_bottom` unconditionally at line 87163. That method explicitly handles Anchored-at-zero (`crates/bt-viewport/src/lib.rs:2512`). The paste helper also resets it (`crates/bt-app/src/main.rs:114762`). A downward wheel step reaching zero resets Bottom at `crates/bt-viewport/src/lib.rs:2491`.

Thus passive output after clicking a newest mark can stop following once the ceiling permits, while resumed shell typing follows again. This matches the existing intent of jumping to an old, unclamped mark. **Product question:** should a near-bottom explicit jump retain that review intent or resume passive following? This review describes the change and does not choose the ruling.

The requested command walk is named `step_command_mark` here: `crates/bt-app/src/main.rs:42931` reads `scroll_anchor().source`, line 42936 finds the command at/before it, and line 42941 steps from that index. The retained source is the clicked command even when its visual landing was clamped; it no longer substitutes the newest mark via the no-anchor branch.

## Origin, unread state, relief claim, and traces

`viewport_origin` now exposes Anchored (`crates/bt-viewport/src/lib.rs:3743`). `record_published_frame` only records that boolean in resize diagnostics (`crates/bt-term/src/session.rs:3525`); publication revision increments regardless at line 3514. No origin-based history-reading gate or repaint-protection timeout was found. Review hold depends on resize plus displaced-anchor state (`crates/bt-viewport/src/lib.rs:2922`), not merely Anchored.

`present_gate::pictures_match` delegates to presentation equivalence (`crates/bt-app/src/present_gate.rs:10`), which compares origin at `crates/bt-app/src/main.rs:118830`. Changing Bottom to Anchored can therefore require a publication even when geometry matches; it does not suppress the landing. The frames are not literally byte-identical.

The Bottom-only unread reset is skipped (`crates/bt-viewport/src/lib.rs:2910`), so existing unread count can persist. Growth increments it only when the previous offset was nonzero (line 2781); merely keeping Anchored-at-zero does not count the first subsequent growth. Repository callers use it for diagnostics/tests, not badge text or repaint gating. The badge is built from geometric counts at line 3717.

The author's expanded-band measurement is supported: extent is `total - pane - relief` (`crates/bt-viewport/src/lib.rs:2444`), relief is bounded by live inflation and blank tail (line 2757), and `continuous_frame` uses that exact extent (line 2806). Keeping that ceiling avoids introducing a relief-sized extra travel range. The new test asserts zero offset, unchanged extent, equal row_map and status at lines 10443-10453, then persistent anchoring and repeat-press offset/row_map equality at lines 10465-10485. It covers one expanded/full-relief fixture, not every picture component or contracting bands; R2 disproves the general claim. The three cited bt-term relief gates were not run under this review's test limit.

Instrumentation is coherent: the rail trace reports a prediction from the prior composed geometry, explicitly documented at `crates/bt-app/src/main.rs:42835`; it can differ if content moves before composition. The performance trace now follows `viewport_frame` (line 65882), reporting that frame's offset, extent and relief while retaining refresh-only timing (line 65879). No additional defect found in these changes.

## Validation and boundaries

`cargo test -p bt-viewport -j 4`: **139 unit tests and 3 integration tests passed; 0 doc-tests**. Five additional scratch tests, run only in bt-viewport: staging-only and negative-residual cases passed; wrapped-prefix size, wrapped-prefix admission and short-raster resting equivalence failed as described. Scratch source was deleted. No application was launched or process terminated; no heavier crate tests were run. This document is the only committed change.
