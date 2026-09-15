STATUS: COMPLETE — Phases 1–5 complete; 12 ledger dispositions, 8 findings and per-step dispatch verdicts; docs only; no cargo build/check/test.

# R-BT-APP-SPLIT-3

Subject: revision 3 of `docs/plans/bt-app-split.md` and its inventory at `d91b66ca1093593aadfe0b5ad3c91f1615311423`, branch `docs/bt-app-split-plan`.

## Phase 1 — evidence boundary

Read the priority plan and second review before writing. Initial Git status was clean. Work is confined to `D:/Developer/bt-wt/bt-app-split-plan`. The review and `target/review3-notes.md` are the only authored files; permitted graph tooling may regenerate its target artifact. No cargo build/check/test is permitted or used.

## Phase 2 — independently reproduced evidence

Unless prefixed otherwise, source paths below are under `crates/bt-app/src/`; plan and inventory line numbers refer to the reviewed HEAD. `target/review3-notes.md` retains script output, source excerpts, graph calculations, metadata, feature trees and history counts. No runtime result is inferred from these reads.

### Graph and contract extents

Both retained commands exited 0, in this order:

```text
python scripts/dev/bt-app-graph.py
python scripts/dev/bt-app-split-table.py
```

The builder parsed 102 files with no parse-error flags: 455,336 physical lines and 197,346 test-span lines. `root_prod` baseline has 96 nodes/404 edges, an 80-node/442,903-line largest component, and 16 free modules/11,761 graph-weighted lines. Simulating removal of i18n's three outgoing edges gives 401 edges, a 74-node/423,306-line component, and 22 free modules/31,358 graph-weighted lines.

All thirteen plan rows match regenerated module membership and both numeric columns: 31,336 physical and 18,206 production lines. The 22-line difference is the graph's +1 convention. The generator's stale row-13 destination and formula_tools note remain the two explicitly documented differences in meaning; exit 0 does not validate those strings or the contracts. The 23 blocked module sizes sum to 32,673.

The following sums were recomputed from the JSON `stats`, not copied from the contract total:

| Contract | Counted modules and physical lines | Recount | What this establishes |
| --- | --- | ---: | --- |
| A | dir_news 190 + files_watch 455 + git_watch 573 + preview_watch 1,039 + palette_index 1,212 | 3,469 | Conditional whole-file mass after replacing their root event dependencies. All four watcher modules also need watch_clock. |
| B | settling | 642 | Removing settling/arrival root edges alone leaves arrival dependent on marks. |
| C | marks 7,002 + icons 3,199 | 10,201 | The production two-cycle must be one bundle, with favicon and i18n available. It is not a complete test move. |
| C and B | arrival | 767 | Requires both motion and marks prerequisites. |
| D, preferred option | update 1,427 + hang_watch 2,850 + persist 2,150 + schemes 1,608 + pins 548 + diagnostics 981 + menubar 1,260 | 10,824 | Conditional on replacing every production version/app-name dependency; app-dependent tests remain. |
| E | card_trace 750 + web_thumb 1,651 | 2,401 | Conditional on satisfying both root identity edges; card_trace also needs trace. The alternative retaining card_trace cannot claim this same moved extent. |
| F | No approved item move | 0 | preview_wrap, table_block and preview_viewport remain app-bound. |
| A–E plus arrival | Above module sizes | **28,304** | Whole-file sum, not a measured relocation manifest. |
| Left outside that sum | version 305 + formula_tools 1,194 + preview_wrap 1,252 + table_block 298 + preview_viewport 1,320 | **4,369** | Reproduces 32,673 − 28,304. |

An in-memory production-graph copy, after the i18n cut, removes the named A/B/C/E root edges, menubar's root edge and the consumers' version edges. The counted modules then have no outgoing edge beyond the free-plus-counted set; marks/icons remains a grouped cycle. This deliberately optimistic experiment satisfies entire graph edges and does not implement their APIs. The graph's `root_all` evidence exposes the omitted tests in R3-5 below. No source or graph input was altered for the counterfactual.

### Whole-source negatives and arity assertions

Independent searches:

```text
rg -n 'SOURCE\.matches|!SOURCE\.contains|!MAIN\.contains|!source\.contains\(fetch\)' crates/bt-app/src/main.rs
rg -n 'include_str!\("main.rs"\)|const SOURCES' crates/bt-app/src/main.rs
```

| Site in main.rs | Actual scope / assertion |
| --- | --- |
| 15772 | Whole SOURCE negative over the retired transfer spellings assembled at 15767–15769. |
| 105705 | Whole SOURCE negative over escaped focus-report spellings assembled at 105701–105703. |
| **113789** | Whole MAIN negative, bound to `include_str!("main.rs")` at 113779. The loop at 113782–113786 assembles four withdrawn tray spellings. This is the newly identified third whole-source negative. |
| 103570 | Scoped negative over `body("    fn mini_source")` at 103555; positive in-memory-content assertion at 103557; five forbidden worker/capture names at 103563–103567. It is not a global prohibition. |

| Arity expression line | Expected count | Source detail |
| ---: | ---: | --- |
| 14201 | 1 | For each of three head-moving spellings. |
| 14217 | 2 | checkout_at callers. |
| 103965 | 1 | `calls` asserted at 103966–103967. |
| 104432 | 1 | `calls` asserted at 104433, for three hit functions. |
| 104975 | 1 | session_store.record. |
| 105163 | 2 | vault_this_window declaration and caller. |
| 105784 | 6 | commit_leaf_resize declaration, production release and four test callers, as the comment at 105785–105787 states. |
| 131933 | 1 | surface_takes_image_zoom(surface). |
| 151992 | 1 | file-row opener. |
| 152393 | 1 | Docked ladder call. |

There are ten syntactic arity sites, some evaluated for multiple loop values. Their positive expected counts reject loss of all counted occurrences; they do not prove that a reduced file set still detects an additional forbidden caller elsewhere.

AST inspection finds **91** exact main.rs include invocations: 38 at module scope and 53 within functions; **89 const bindings** (87 SOURCE, two MAIN) and **two let bindings** named source at 125096 and 157543. `source_region(` occurs zero times in main.rs. The multi-file precedent at 165696–165697 is two constants, SOURCE and PREVIEW_EDIT; there is no `const SOURCES` declaration.

### Two-impl scope and all eleven platform-detector hits

AST filtering for inherent impls, excluding Deref and DerefMut, gives exactly **36598–61746** and **62188–100525**: 25,149 + 38,338 = 63,487 lines. Thus 165,815 − 63,487 = **102,328** before new declarations/imports. The initially broader scratch filter included the two trait impls; the corrected filter and result are explicitly retained in the notes.

Reapplying the guards' lexical rule—strip `//` suffixes, require cfg/cfg!/cfg_attr and one of the five platform words at main.rs:163043—gives:

| Lines | Owner | Inside either moving impl? |
| --- | --- | --- |
| 112433, 112442 | set_option_as_alt | No |
| 114252, 114261, 114272, 114293 | window_surface_target area | No |
| 114545, 114547 | native_window | No |
| 116199, 116223 | fn main | No |
| 163322 | Test needle array | No |

These are **ten compiler cfg attributes plus one code line containing test strings**, not eleven platform attributes. Plan 852–857 acknowledges the self-match. Both detector implementations are line scanners: main.rs:163086–163088 and scripts/check-portable-core.ps1:252–267. The Rust stranger/silent assertions are at 163116–163140; the PowerShell checks are at 276–288. Its physical array parser is at script lines 216–233. Leaving platform_gate_tests in main.rs preserves that reader; strict 2a moves none of the eleven hits and needs no platform-array edit.

### context_menu placement and the accepted script ruling

`context_menu.rs:70–74` imports std, bt_platform and i18n::Text; its registry effects call bt_platform at 156–161 and 182–186. `i18n.rs:91` already imports bt_platform::HostPlatform and 2690 calls host_platform. `crates/bt-persist/Cargo.toml:10–13` declares only serde, serde_json and thiserror. Consequently, placing context_menu into bt-persist adds **two direct Cargo dependencies**, bt-platform and bt-i18n; placing it with i18n adds **zero direct dependency names**. That is an edge-count result, not proof of API or runtime safety. Row 1 must carry the effects and the source-reader test at context_menu.rs:266–292, whose path is package-relative. The owner-selected placement is not reopened.

`git show 8e6cf0cc:<script>` confirms both landed script invocations use `--workspace --locked` and omit `--bin folio`. The reviewed branch's physical scripts still use `--package bt-app --bin folio` at lines 31/32. Offline locked feature trees reproduce vte default and bit-set default differences between workspace and package selection. The ignored-test script selects the workspace at line 46 and deduplicates names at 60–62. These are source/feature checks; no renderer or harness was executed.

### Freeze and stable Cargo evidence

Plan 1082–1086 does contain an abort rule: gate failure or expiry abandons the unmerged batch, forbids partial landing and delays the split when owner/base/acceptance facts are unavailable. Plan 1072–1081 requires a named coordinator, a recorded maximum duration and validation of the tree actually merged. Those are checkable fields once populated; none is populated for a present relocation ticket. A historical enumerate-then-show recount at 76ca0788 gives 98/150 under default traversal, and 112/150 under no-merges, with seats/i18n/preview/settings = 29/24/17/12 under the latter. These counts cannot supply the missing live-ticket inventory.

The installed Cargo reports **1.94.1**, commit **29ea6fb6a5db279426f4cc4e17aa385f05a0cfbc**, host x86_64-pc-windows-msvc. Only version, offline locked metadata and tree queries were run. Metadata confirms 16 members and bt-app's bin, three examples, integration-test and custom-build targets; no library target.

| Plan 393–400 evidence row | Verification |
| --- | --- |
| Compiler-unit timing from timestamped HTML | Supported; units are compiler invocations. |
| Complete fresh/dirty unit set from `-v` | Insufficient as specified: Fresh output identifies packages, with conditional/deduplicated emission, not each target/profile artifact. |
| Link time from the binary-unit tail | Unsupported: the report does not expose binary codegen start, so the proposed tail has no observable boundary. |
| Separate link-only process seconds/memory | Valid evidence category, but the actual instrumentation and memory metric must be named in the ticket. |
| Filesystem image/PDB sizes | Obtainable, with artifact identity preserved before a subsequent command replaces an uplifted image. |
| Suite wall time | Obtainable from a separately identified run; it is not a compiler timing. |

Cargo documents HTML unit timing and the missing binary codegen boundary in [Reporting build timings](https://doc.rust-lang.org/cargo/reference/timings.html). The exact installed source emits Fresh using `unit.pkg` under package-count conditions at [job_queue/mod.rs:1198–1204](https://github.com/rust-lang/cargo/blob/29ea6fb6a5db279426f4cc4e17aa385f05a0cfbc/src/cargo/core/compiler/job_queue/mod.rs#L1198). Stable [compiler-artifact messages](https://doc.rust-lang.org/cargo/reference/external-tools.html#artifact-messages) provide target, profile, features, filenames and fresh status; they are distinct from unstable timing JSON and from libtest JSON.

## Phase 3 — second-review ledger checked against the body

Closure here means a consistent documentary correction, not implemented code or a passing runtime gate. The author's ten accepted and two accepted-in-part decisions are not closure counts.

| Ledger row | Disposition | Evidence in revision 3 and remaining correction |
| --- | --- | --- |
| R2-1 | partially closed | Plan 737–758 chooses the two-impl scope; 792–860 keeps destination-dependent changes with moves and leaves platform_gate_tests physically in main. But the visibility/import row at 798 is in the before-freeze column while 1075–1076 puts those edits inside each move; inventory 795–818 still orders the abandoned platform-array change. |
| R2-2 | partially closed | Plan 890–924 corrects mini_source, finds the third global negative and ten arity sites, and withdraws the 88+3 partition. The 89-const/two-let census also contradicts 877. The universal remove-subject mutation at 917–919 cannot validate a negative assertion whose forbidden text is already absent; arity handling must preserve the original covered source union. |
| R2-3 | partially closed | Plan 1329–1465 supplies A–F and the arithmetic reproduces. These remain conditional file-mass estimates: C and D leave app-dependent tests unresolved, receiving owners remain alternatives, and moved root-item extents are not counted. |
| R2-4 | closed | Plan 1162–1195 states set/cfg agreement, stale-JSON and topology limits, missing order checks and grouped-SCC limits; script defects are explicitly deferred. Plan 1255–1274 fixes context_menu's owner and remit. The no-script-edit ruling is respected. |
| R2-5 | partially closed | Plan 642–651 fixes all five file destinations and 669 requires a graph rebuilt after the real cut. Acceptance 679–692 names categories of checks, but no exact receiving test identities, concrete boundary/ID fixture values or call-site test names. The heading and ledger overstate this as already named. |
| R2-6 | partially closed | Plan 441–473 states workspace exclusions, test-to-dev inheritance and the remaining harness. Inventory 226–231 still calls the two graphs entirely unshared; plan 337–338 again guarantees every unit Fresh from feature selection alone. |
| R2-7 | closed | Plan 320–370 records the wider landed selection, checker follow-up, different script error policies and ignored-list limitations. Read-only git show confirms 8e6cf0cc's workspace invocations; current review-branch scripts retain the older selection. The baseline must verify its actual release base includes the fix, as 378 already requires. No duplicate script fix is requested. |
| R2-8 | partially closed | Plan 489–520 supplies a separately owned guard, real panic frame/source location, matching-image/PDB hang evidence, inadequate-artifact demonstration and retained-guard rollback. Inventory 763–766 still asserts the withdrawn image decomposition; profile evidence remains a future trial. |
| R2-9 | partially closed | Plan 395–400 replaces timing JSON with HTML and separates process measurement, but still invents a binary-unit tail for link time and treats package-oriented verbose logs as a complete unit census. |
| R2-10 | partially closed | Plan 1058–1086 defines priority, live inventory, coordinator, maximum duration, final-tree validation and abort without partial landing. Yet 1045–1049 still infers most open branches from 112/150, contradicting 1060–1066; empty refs at 1092–1099 do not establish absence of unpublished work. |
| R2-11 | closed | Plan 962–1019 and 1637–1651 require identity/signature/cfg/context pairing, literal-preserving ordered tokens, enumerated edits, binding/macro audits and an explicitly limited behavior claim. Actual comparisons, child completion and platform acceptance remain implementation evidence. |
| R2-12 | partially closed | Plan 213–218 and inventory 130–135 separate 7.56% methods from 4.27% lines; plan 744–748 and inventory 426–433 give 102,328; plan 175–191 corrects churn provenance. Plan 1045–1049 nevertheless repeats the live-branch inference it withdrew. |

## Phase 4 — numbered findings from the full-plan reread

### R3-1 — blocking for Step 0 baseline dispatch: the evidence table still requires an unobservable link boundary and an incomplete unit census

**Evidence:** plan 395–398 and ledger R2-9 at 1843 still assign link time to the binary unit's tail. Cargo's documented binary timing has no codegen-start boundary, so neither link-only time nor a separate codegen-plus-link tail can be read there. Plan 396 assigns the full fresh/dirty unit set to verbose logs, but the installed Cargo source cited above emits package-level Fresh status conditionally. One package can have a reused normal binary and a rebuilt harness. HTML plus those status lines is not the promised artifact-by-artifact census.

**Correction:** call the HTML value **whole compiler-unit elapsed time**. Use stable `--message-format=json` compiler-artifact records alongside verbose command logs to identify package, target, profile/test mode, features, artifacts and fresh status; retain command context for target triple and flags. Do not treat the JSON as a complete serialized internal Cargo unit graph. Retain timestamped HTML separately. For link-only acceptance, name a Windows process trace or instrumented linker invocation that records PID/parent, executable/arguments, start/end, CPU versus wall time, and a specifically defined peak-memory metric. Associate the trace with the correct EXE and PDB. If that instrumentation is omitted, report link-only time as unmeasured. Make it required before §4.4b's instruction to measure the link on its own can be claimed complete. No build is needed to correct this ticket specification.

### R3-2 — should-fix, required before dispatch: the gate table confuses entry requirements with ticket outputs

**Evidence:** plan 119–122 says nothing goes to an implementer until its gate row is satisfied. Row 0 at 126 then requires a measured baseline, a reviewed diagnostic guard, isolated trials and the chosen combined measurement—all work Step 0 is supposed to dispatch. Its closing gate at 607 points back to that row. Rows 2a/2b at 128–129 similarly include mutation checks and proof that moved child bodies ran. Those cannot all exist before the move is implemented. Step 1 has a usable precondition shape but lacks the named evidence described in R3-3. Step 3's API/features/tests/fixtures/owner gate is document-checkable, although no populated row ticket accompanies it. Step 4's explicit withholding remains operative.

**Correction:** separate **dispatch prerequisites**, **implementation acceptance**, and **landing prerequisites** for each ticket. Step 0 baseline entry should be: released green base containing the accepted selection fix, named owner/runner, isolated target directory, exact command sequence, repeat/reset/edit procedure and evidence schema. Its output is the baseline. That output authorizes the opt-level trial; the separately reviewed diagnostic guard authorizes the debug trial; chosen combined results authorize retaining settings. For Step 2, dispatch guard-preparation and manifest work on the existing topology, require their evidence before the freeze, and require moved-item/child evidence before landing. The plan must not require results of an undispatched ticket as permission to implement it.

For §4.2's three repetitions, specify whether each cold repetition resets the full measured sequence or each command, how warm caches are established, and the precise app-only edit/restoration. Hold the declared source/configuration and command order constant. Otherwise three sequential invocations may become one cold run and two warm runs while all are labelled cold. This is a ticket field to fill, not a reason to perform builds during this review.

### R3-3 — should-fix for Step 1: the five-file cut is fixed, but its claimed named acceptance is still absent

**Evidence:** plan 642–651 resolves all destinations. Yet 679–690 gives categories—representative IDs, boundary numbers, full tables—and 692 says to run call-site tests by name without naming them. Ledger R2-5 at 1839 says those tests and fixtures have been named. The existing profiles test is `profiles::tests::an_entry_that_is_neither_a_builtin_nor_a_program_is_dropped_and_named`, at profiles.rs:22990–23013; it only checks a substring. The two main calls are inside `Runtime::create` and `Runtime::reread_profiles`, not tests. `i18n::CURRENT` is process-global at i18n.rs:248; tests that switch it in a shared harness can perturb other tests.

**Correction:** the ticket must include a table of full target/test identities, marked existing or new, exact inputs and expected-output source, and the caller each test reaches. A concrete specification can keep the five-file boundary:

- Add a new `settings::tests::profile_colour_labels_preserve_both_languages` test for all eight `MarkColour::ALL` entries, matching the literal pairs at i18n.rs:3462–3469, including the picker path and its `.text()` behavior.
- Keep and retarget the existing profiles test above; add a new `profiles::tests::profile_entry_fault_preserves_exact_outputs` test for both ProfileFault variants in both languages, using fixed IDs such as `fish` and `profile-42`, with the four original format strings at i18n.rs:6380–6391 as the frozen oracle.
- Add new `i18n::tests::pixel_size_preserves_exact_output` and `i18n::tests::picture_shown_at_preserves_exact_output` tests with cases `(0, 0)`, `(1, 1)`, `(1180, 800)`, `(u32::MAX, u32::MAX)`; compare exact spaces and U+00D7, and both full picture_shown_at sentences. Exercise the preview re-export in a new `preview::tests::pixel_size_reexport_preserves_output` test too. These are proposed new identities, not assertions that the tests already exist; their Cargo target is `bt-app --bin folio`.
- Name or add the main call-site coverage for create and reread_profiles; do not invent an existing test identity. An explicitly bounded source/caller comparison plus the receiving formatter test may be named as such if no direct unit seam exists.
- Preserve Text variants/table literal tokens and the single current-language read in the moved fault formatter. Run any actual global-language-switch coverage in a contained child process, or specify an equally effective isolation mechanism. Do not assume a mutex used only by new tests protects existing readers.

All added tests must live within the chosen five files unless a revised file list is approved in writing. Refresh the call sites and tests against the released 0.4.1 base. These are acceptance details for the already selected cut; they do not change its destinations or add a crate.

### R3-4 — blocking for Step 2 guard conversion: mutation rules do not cover negatives or preserve arity scope

**Evidence:** plan 917–919 instructs every converted guard to remove its subject and fail. The third whole-source negative at main.rs:113789 is green precisely because the four withdrawn spellings are absent. Removing forbidden text cannot make it fail. Conversely, a count guard narrowed to one destination file can retain its expected count and still miss a newly added unauthorized caller in another former-main fragment. The six-count example at 105784 includes four tests; plan 925–929 requires a changed count during 2b without specifying whether the original source union is retained or split into production/test invariants.

The binding census in plan 877 and inventory 369–370 is also false: two source bindings are `let`, at main.rs:125096 and 157543. Plan 915–916 and inventory 402–404 cite 165696 as precedent for a SOURCES slice, but it declares separate SOURCE/PREVIEW_EDIT constants. These mistakes matter to a manifest enumerator that filters by declaration kind or copies an assumed helper shape.

**Correction:** use class-specific mutation acceptance:

| Guard kind | Required mutation evidence |
| --- | --- |
| Named body / positive requirement | Remove or rename the unique subject; also remove the required condition inside the correct subject. Reject missing/duplicate subjects and test-literal matches. |
| Whole-source prohibition | Inject one forbidden spelling into each covered destination class and require failure; remove it and recover. Separately prove all expected source files are enumerated. |
| Arity / uniqueness | Delete a required occurrence and add an unauthorized occurrence in another covered fragment; both must fail. Record expected production and test occurrences with their owners. |

Define the covered union from the original main.rs scope and its relocated descendants; any expansion to unrelated existing modules requires an explicit semantic decision. Preserve whole-source coverage across 2a/2b or replace it with named production/test invariants of equal intended strength. Do not merely lower six to the number remaining in one file. Put path/arity changes with the move they serve. Correct the census to 89 const plus two let bindings, and describe the multi-file precedent as two constants. The proposed SOURCES slice may still be designed; it is not an existing implementation.

### R3-5 — blocking for dispatch of the blocked Step 3 queue: A–F are still contract briefs, and 28,304 is not an actual moved extent

**Evidence:** plan 1320–1323 claims each contract names exact items, owner, API, tests/build inputs and actual moved extent. B leaves its receiving library open at 1354–1357; E explicitly chooses neither owner at 1419–1421 and offers retaining card_trace at 1426–1429. A names a callback but does not select its receiving library or executable signature. The sums reproduce from whole-file `stats`, not item manifests, and omit the incoming root definitions and required methods.

More concretely, the advertised C/D file moves carry tests that still reach the binary's private modules:

| Contract | Directly inspected unresolved test ownership |
| --- | --- |
| C | icons.rs is under `#[cfg(test)] mod tests` from 1034. `draw_sites` at 1051–1384 reads seats constants at 1124 onward, git_panel at 1253, git_graph at 1278, and several other app modules. `head_runs` at 1599–1629 also reads app layout constants. Their callers at 1502, 1530 and 1656 are not addressed by moving StatusDot. |
| D | `update::tests::the_row_offers_the_release_page_whatever_machine_this_is` at 1344–1426 calls settings at 1400–1414. `persist::tests::the_storage_directory_is_named_for_the_product` at 2030–2033 reads root APP_NAME. The schemes test at 1550–1607 calls seats::push_corner_tag at 1582. None is version's retained cross-product test, so subtracting only version's 305 lines is insufficient. |

Contract D also needs update's production VERSION consumers at update.rs:517 and 641, not only the two banner callers named in plan 1399–1401. Injecting build identity could solve those uses, but the actual fields and callers must be enumerated.

**Correction:** retain all 23 blocked modules as blocked. Relabel 28,304 as **conditional whole-file mass associated with the briefs** and 4,369 as **excluded whole-file mass under the preferred options**. Before an extraction ticket, select each owner/API and produce a dependency list plus an item/test/build-input manifest, including app-retained adapters and the added root-item spans. Keep app-crossing tests in bt-app against the extracted public API, or explicitly redesign their inputs; do not add a reverse dependency on the bin-only app. Recount actual moved spans after that choice. Contract E must have separate extents for shared-identity and retain-card_trace options. No constants crate or whole-AppEvent library is needed for this correction.

### R3-6 — should-fix: the inventory still contradicts the operative plan

**Evidence:** the plan calls this inventory its evidence source at 39–42, yet the following are presented there as current conclusions, not historical rejected proposals:

| Inventory lines | Remaining claim | Contradicting plan/source evidence |
| --- | --- | --- |
| 226–231 | Different optimization levels share no artifacts. | Plan 454–464 explicitly withdraws total non-reuse and requires matched settings. |
| 763–766 | The approximately 124 MiB excess is app test code/libtest; common generics cannot explain it. | Plan 542–549 withdraws that attribution because the historical artifacts were not comparable or section-analyzed. |
| 795–818 | Platform array must be edited; launch code moves; moving any cfg makes both directions fail. | Strict 2a and the retained platform_gate_tests exception at plan 737–758 and 825–860; the eleven-line recheck above. Only removal of the last detected use makes a listed file silent. |
| 593, 627–634 | The graph excludes main as a node and free means avoiding only the largest component; regex_full after-i18n free is 51/66,273. | Builder represents main as @root in root_prod and rejects every nontrivial cycle. Its regex_full strict free is **48/55,302**, while **51/66,273** is `avoid_largest`. |

The last distinction does not change the current root_prod free row set, where the two measures coincide. It does change the claimed general definition and reproduction of the historical comparison.

**Correction:** synchronize the inventory's operative statements with revision 3's chosen scope and measurement limits, preserving old readings only as explicitly rejected history. Label strict `free` and `avoid_largest` separately. The script itself need not be edited during this review. Until the author makes that documentation correction, ticket writers must not import inventory §7's obsolete platform work into Step 2a.

### R3-7 — should-fix before a freeze: the scheduling table and live-branch claims still conflict with the protocol

**Evidence:** plan 798 places visibility/import changes in the before-freeze column, while 1075–1076 says each move contains its own visibility/import edits; §6.1 also ties minimal visibility to the receiving module. Plan 1045–1049 deduces most open branches require rebasing from 112/150, contradicting its inventory-first rule at 1060–1066. Plan 1094–1099 infers no freeze cost from refs with no new commits, despite explicitly acknowledging that refs are not a census of current work. The quoted local commit 8e6cf0cc's changed-file list also does not contain main.rs, contrary to 1097–1098.

**Correction:** put destination-specific import/visibility edits in the with-move column; list only demonstrably topology-compatible exceptions before the freeze. Base overlap and rebase obligations on the populated owner/base/tip/item/acceptance inventory, including owner-confirmed unpublished work. Remove the deductions about most live branches and zero cost from empty refs. Preserve the existing coordinator, agreed maximum duration, final-tree gate and no-partial-landing abort rule. Spell the abort operation as **lift the merge freeze and resume affected merges; schedule any retry separately**, so “reopen the window” cannot be read as automatically restarting an expired freeze.

### R3-8 — should-fix: the remaining compile model claims more than the evidence supports

**Evidence:** plan 337–338 says either workspace-selection shape makes every unit Fresh after the gate, while 457–458 correctly makes reuse conditional on mode/profile/features/flags. Plan 1493 counts thirteen new crate boundaries although 1224 and the row table specify eleven new crates and two placements, one of which rides with row 1. Plan 1708 claims a 6.9% benefit across all three gates and explicitly includes fmt. The actual fmt gate is `cargo fmt --all -- --check` (CONTRIBUTING.md:18): extraction retains that source within the workspace, so the line fraction does not establish a formatting-time saving.

**Correction:** state that the selection change removes the identified feature mismatch, with actual reuse measured on the released baseline. Count eleven new crates. Keep the 6.9% as a source fraction and a labelled compiler-work hypothesis, without assigning it to fmt or every rebuild; measure each gate separately and report any added manifest/module overhead. Do not infer a numerical win for the already-landed selection fix from a post-fix-only baseline; a before/after claim needs comparable runs on both configurations.

## Verdict per step and ticket dispatch after 0.4.1

These verdicts apply to the documents as written at the reviewed HEAD. Proposed commands and tests above are future implementation acceptance; this review authorizes no implementation or release action.

| Step | Verdict | Is its dispatch gate checkable, and what remains? |
| --- | --- | --- |
| 0 | **go with changes** | The baseline/guard/trial sequence is separable, but the aggregate entry gate is circular. Correct R3-1/R3-2; verify the released base contains the owner-accepted script fix; fill runner/reset/edit/evidence fields. Treat baseline, guard, individual trials and combined validation as separate tickets or phases with distinct entry/exit criteria. |
| 1 | **go with changes** | Five file destinations are fixed. The entry gate can be checked once exact test identities/fixtures and the refreshed release-base references are supplied under R3-3. Preserve the cut as one commit, separate from bt-i18n extraction. |
| 2a | **go with changes** | Not ready for relocation dispatch: needs the item/destination/visibility map, class-specific pin evidence, corrected scheduling columns and populated freeze fields. Strict residue 102,328; no platform-array change. Preparation can be ticketed independently on the current topology after R3-2/R3-4. |
| 2b | **go with changes** | Not ready: exact target/full-name/cfg/ignore/multiplicity and source-consumer maps remain future artifacts. Keep platform_gate_tests physically in main; update file-relative inputs and any changed selector in the subject's move. Child completion is landing evidence, not a pre-implementation result. |
| 3, free rows | **go with changes** | Per-row API/features/tests/fixtures/owner/build-input fields are checkable, but no complete extraction ticket is present. context_menu rides with row 1; refresh graph topology after Step 2. A generated row is not a dispatch contract. |
| 3, blocked queue | **rethink** | A–F are design briefs. C/D tests and unselected APIs/owners prevent claiming actual moved extents. Require R3-5's complete contracts and remeasurement; do not dispatch the 23 modules from the 28,304 sum. |
| 4 | **rethink; do not dispatch** | Withholding is explicit and checkable. The bounded state/call audit and identity/order/lifetime/publication contract remain prerequisites; no substitute implementation is established by this review. |

**Step 0 baseline ticket now, after 0.4.1 ships: no, not verbatim from this plan.** It becomes dispatchable with R3-1's observable evidence schema and R3-2's entry/reset procedure, after recording a released green base containing the accepted selection fix. It does not have to wait for the diagnostic guard or completed profile trials; those are later Step 0 work.

**Step 1 cut ticket now, after 0.4.1 ships: no, not under its own currently stated gate.** The five-file implementation scope can be copied into a ticket now, but dispatch still requires R3-3's named tests/fixtures and refreshed release-base caller references. No additional crate-design decision, Step 2 freeze or Step 4 work is needed for that cut.

## Phase 5 — final verification

The review contains one disposition for every R2-1 through R2-12 ledger row, eight consecutively numbered findings with severity/evidence/correction, the requested independent rechecks, and separate Step 0-baseline/Step 1-cut dispatch decisions. The contract counts are labelled as conditional whole-file arithmetic; no build, test execution, timing, mutation-run or runtime-equivalence result is claimed.

Final `git diff --exit-code HEAD` over protected tracked paths returned 0. Git status lists only this new review; `target/review3-notes.md` is retained as ignored scratch evidence. HEAD remains `d91b66ca1093593aadfe0b5ad3c91f1615311423` on `docs/bt-app-split-plan`. The plan, inventory, previous reviews, Rust source, manifests, lockfile and scripts were not edited. The permitted graph script regenerated only its target JSON. No other worktree was entered; no cargo build/check/test, formatter, product process or remote-compute operation was run.

The report and notes decode as UTF-8 with LF-only endings and no BOM or replacement characters. STATUS is the first line. Finding sequence and complete ledger coverage were checked mechanically.
