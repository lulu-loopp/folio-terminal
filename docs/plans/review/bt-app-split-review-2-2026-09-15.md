STATUS: COMPLETE — Phases 1–5 complete; 24 ledger dispositions, 12 findings and per-step verdicts; docs only, no build/check/test.

# R-BT-APP-SPLIT-2

Subject: `docs/plans/bt-app-split.md` and `docs/plans/bt-app-split-inventory-2026-09-15.md` at `e647415e65bd672be761f934341519c10d76ff38`. Worktree: `D:/Developer/bt-wt/bt-app-split-plan`, branch `docs/bt-app-split-plan`. Source references below are to this snapshot unless labelled as local branch observations. Findings here use **R2-N**; ledger numbers refer to the first review.

## Phase 1 — evidence boundary

The priority plan and first review were read before writing. Initial status was clean and HEAD matched the requested commit. The inventory, both retained analysis scripts, source consumers, manifests, CI and local Git history were then inspected. No code, plan, inventory or script was edited. No build, check, test, formatter or product process was run. `cargo tree` and `cargo metadata` were read-only, offline and locked. No other worktree was entered, and no remote compute resource was used. Official Cargo documentation was consulted for profile and timing semantics.

`target/review2-notes.md` retains command results, counterfactual graph calculations and intermediate observations. Counterfactuals operate on an in-memory graph copy; neither the graph input nor the scripts were altered to manufacture a failure. Historical EXE/PDB sizes remain unverified here. A source count of 3,749 is not an observed libtest census; the AST count is 3,809, and neither establishes how many tests run on a particular platform.

## Phase 2 — script reruns and the root-item claim

### Reproduction and the actual self-check

Both commands completed successfully:

```text
python scripts/dev/bt-app-graph.py
python scripts/dev/bt-app-split-table.py
```

| Measurement | Reproduced result |
| --- | --- |
| Parsing | 102 files; no parse-error flags |
| Source | 455,336 physical lines; 197,346 test-span lines |
| root_prod, baseline | 96 nodes / 404 edges; largest SCC 80 nodes / 442,903 graph-weighted lines; free 16 / 11,761 |
| root_prod, simulated i18n cut | 401 edges; largest SCC 74 / 423,306; free 22 / 31,358 |
| Generated extraction rows | 13 rows: 11 new crates and two placements; 22 modules / 31,336 physical lines / 18,206 production lines |
| Generated blocked rows | Same 23 modules and physical-line values as the plan; total 32,673 |
| Four test-only files | preview_typing 640; source_pin 132; focus_thumb_restore_tests 367; preview_viewport_tests 774; zero production lines in each |
| Generator result | Exit 0; `manifest agrees with the graph` |

The 13 free rows match the plan **row by row in module membership and both numeric columns**. They are not byte-identical Markdown: the plan styles placements differently, formats crate names in notes, and changes row 13's note. The blocked table is reordered and its explanatory prose is edited. The six-prerequisite table is handwritten; the generator does not produce or validate it. The 22-line difference is the graph's additional one per source file for these nodes; 31,336 / 455,336 = 6.88%. It is a source fraction, not measured compile savings.

The generator's main() checks listed membership against cuts.root_prod.i18n.free, duplicate membership, omitted free modules, blocked-listed modules that have become free, and the four all-test assertions. Any accumulated problem returns 1. Thus making a listed module newly root-dependent **and rerunning the builder** removes it from free, and the membership check rejects its row; freeing an unlisted module is also rejected. This establishes the failure path by source inspection, without a planted edit. Exit 1 was **not** experimentally induced in this review.

It does not check dependency order, correctness/completeness of BLOCKED_NOTE, item ownership, public APIs, new crate dependency cycles, build-script inputs, test relocation, the plan's prose, or whether the JSON describes current source. I independently checked predecessor order against the post-i18n graph: the present 13 rows pass. Reordering rows 2 and 3 would violate their dependency order while still passing this generator. Its free definition rejects every nontrivial SCC, so a later valid marks/icons bundle would need grouped-SCC handling rather than treating each member as independently free. These are limits of the guard, not reasons to discard its current numerical output.

The graph remains a source approximation: fixed external test-file/context rules, qualified-path scans, no macro expansion or rustc name resolution. After Step 2 changes module topology, its hard-coded context rules must be revalidated before it authorizes Step 3. A successful old JSON/table comparison cannot certify a new topology.

### What each of the six rows actually costs

In-memory experiments started from root_prod with the three i18n out-edges removed. They removed only the named modules' edges to @root, then examined remaining paths and SCCs. Removing a whole @root edge is optimistic: it stands for satisfying **all** root references hidden by that edge, not one symbol.

| Advertised root prerequisite | Re-derived constraint and honest relocation cost |
| --- | --- |
| AppEvent | Removing the five root edges frees exactly dir_news, files_watch, git_watch, preview_watch, palette_index: **3,469 lines**. The enum is at main.rs:302. Moving the whole application event vocabulary into a library creates a shared event contract and exposes unrelated variants. Prefer a separately designed wake adapter supplied by the app. Preserve each existing wake variant, thread-safety, ignored send failures, debounce deadlines, worker lifetime and cancellation. Evidence: preview_watch.rs:228,478–498; files_watch.rs:106,221–233; git_watch.rs:99,213–236; dir_news.rs:79; palette_index.rs:403–424. This is interface design, not pure text relocation; it need not wait for a whole-app Step 4 design, but cannot hide inside a Step 3 move. |
| Motion + RevealTween + cubic_bezier | Cutting settling/arrival root edges alone frees **only settling, 642 lines**. Arrival still reaches marks. Definitions are at main.rs:24847,26053,24711. Moving the trio also moves required tween methods, visibility and tests; default curve EASE is already re-exported from bt_render::motion (main.rs:24764). Choose a motion-policy module with explicit clock inputs, in an existing suitable library or a deliberately expanded animation crate. The current bt-anim row contains a GIF decoder: sharing a name is not an ownership argument. Keep native preference reads in the app and preserve Motion::from_client_area_animation semantics. Arrival's **767** lines follow only after the marks bundle is available too. |
| StatusDot | Two plain fields, ink: [u8; 3] and hollow: bool (main.rs:23694). A concrete small cut places this value type with marks and re-exports/imports it at the root; leave StatusClaim::dot policy in the app. marks.rs:1703 is its consumer. With i18n and favicon available, **marks + icons can leave together: 10,201 lines**. Their cycle survives, so two mutually dependent crates cannot leave separately. Formula_tools does **not** follow merely from this cut: it also needs Motion and tooltip's still-app-bound functionality. |
| APP_NAME plus build identity | **The 11,129-line production-graph claim is real, conditionally.** Version's only production root edge is crate::APP_NAME (version.rs:50); the other direct edge is menubar.rs:459. Remove both after the i18n cut and exactly eight modules become free: version 305 + update 1,427 + hang_watch 2,850 + persist 2,150 + schemes 1,608 + pins 548 + diagnostics 981 + menubar 1,260 = **11,129**. However, version compiles env!(FOLIO_COMMIT), and its tests read FOLIO_VERSION_RESOURCE (version.rs:40,61). Both are emitted by **bt-app's** build.rs:36,48–51; a dependency does not inherit them. Its cross-product test calls root panic_report and app diagnostics (version.rs:291–297). A constants-only crate solves none of that. Either retain version/build-resource verification in bt-app and inject build identity/banner and app name into consumers (**do not count version's 305 lines as moved**), or justify a build-identity library with its own correct commit-refresh inputs, manifest/lock/CI work and app-owned cross-product tests. Preserve CLI, PE, plist and diagnostic identity, singleton watchdog/channel state, startup order and banner formatting. hang_watch.rs:1217 and diagnostics.rs:194–214 expose this caller work. **No new crate solely for one string is required.** |
| TabId / LeafId | Today both are in **main.rs**, not bt-layout: pub(crate) struct TabId(u64) at 672; private LeafId { tab: TabId, seat: SeatId } at 731. SeatId alone comes from bt_layout (main.rs:144). TabIds at 703 mints process-wide non-reused IDs and is owned by App (:11071). Cutting the two consumers' root edges frees **2,401 lines**, but moving the values needs a real shared identity owner, constructors/accessors, caller/test edits and preserved Eq/Hash/Ord semantics. A designed bt-layout identity module is possible, or a shared identity library if keeping layout independent requires it; the plan chooses neither. Keep one process allocator, never one per new crate/window. Moving only into runtime/identity.rs does not expose the types to external crates. Alternatively retain card_trace as an app adapter and parameterize the thumbnail cache by an opaque key; these are API changes with their own tests. Evidence: card_trace.rs:67, web_thumb.rs:89,858–868, main.rs:15167 onward. |
| Preview document types | **Not a sufficient cut.** preview_wrap.rs:3 also imports markdown_prose_face, measure_markdown_local, preview, seats; table_block.rs:32 imports markdown_table_columns, push_markdown_table, preview, seats beyond the listed types. App/preview/seats paths remain after removing the three root edges. Preview_viewport appears free only because deleting its wildcard-root edge erases its entire app dependency; its inherent impl Runtime at 994 cannot move to a foreign crate. Retain that adapter and app-private tests; design independent document/measurement types and callbacks first, then remeasure actual movable items. The advertised **2,870 lines** are three file sizes, not an unlocked library surface. |

The six subtotal rows add to **31,479**, exactly **1,194 short** of the 23-module total: formula_tools is waved through as “behind” marks without satisfying its other edges. Its ChromeSprite is **not root-owned**: it is defined in marks.rs:1617 and imported from marks at formula_tools.rs:43–47. The generated note and plan both misidentify it. tooltip.rs:29,391,478,517,530 additionally reaches root motion/preview types and many cyclic app modules. Six headings are work categories, not six independent or sufficient item moves.

## Phase 3 — ledger rows 1–24 checked against the body

“Closed” means the documentary correction is present and consistent enough for that finding; it does not mean implementation occurred. “Partial” means material work or contradictory prose remains. “Regressed” means the attempted remedy introduces an execution error. The ledger's acceptance count is not a closure count.

| Original # | Disposition | Body evidence and remaining issue |
| ---: | --- | --- |
| 1 | partial | §§7.1–7.3 use root_prod and withdraw 66,273 as ready surface. Item manifest remains incomplete/misowned in the six-root reduction; R2-3. |
| 2 | partial | Both scripts are retained and reproduce the chosen rows. Guard covers set agreement, not row order, root-item cuts or stale inputs; R2-4. |
| 3 | closed | §7.4 excludes the four test-only files; generator checks their full-test spans. |
| 4 | partial | Counting units and regex rules are retained (inventory **§0.3**, despite ledger's §0.1 citation). Declining a 1,310-row descriptive theme map is defensible, but cannot replace Step 2's item manifest. 99/1,310 is 7.56%, not 4.3%; R2-12. |
| 5 | partial | §§0,3 withdraw facade/one-way ownership claims and name Deref/destructuring. §3.2 still asserts services are “read far more often than written” without an audit; R2-12. Step 4 remains withheld. |
| 6 | partial | §4 adds baseline cases and diagnostic intent. Stable timing artifacts and real matching-image/PDB hang acceptance remain unspecified; R2-8/9. |
| 7 | closed | §4.5 scopes the Windows target linker trial to its command, names static-CRT composition, pinned 1.94.1, native/build-script inputs and PDB identity. Exact executable/driver invocation belongs in the trial ticket. |
| 8 | partial | §5 corrects eight colours, main/settings callers, strings and ineffective guard. “Picker or type owner” and “shared ... re-exported” leave the five-file ticket ambiguous; R2-5. |
| 9 | partial | §6.1 allows reviewed visibility changes and requires item destinations. Arithmetic remains off by one; a two-impl cut differs from the launch/free-function cut; R2-1/12. |
| 10 | partial | §§6.2(c),6.3 name the child, completion proof, recursive shell-page guard and test identity map. Pre-freeze selector schedule is invalid; pin classification has a new error; R2-1/2. |
| 11 | partial | §§6.3,9.3 replace sorted lines with ordered tokens/allow-list, name formatting, cfg/lint and platform limits. Body-only comparison remains narrower than equivalence; R2-11. |
| 12 | regressed | §6.5 adds source-commit/branch inventory and conflict-aware rollback, but lands destination paths before relocation and lacks a bounded release-ticket protocol; R2-1/10. |
| 13 | partial | §7.2 repairs surviving rows and lists some APIs/fixtures; §7.3 overstates downstream availability. Row 13's receiving crate remains undecided; R2-3/4. |
| 14 | closed | §8 withdraws Services/Turn/Intent as a design, records synchronous order and requires identity/lifecycle/publication contracts. |
| 15 | partial | §§7.2,7.5,10 use 22 / 31,336 / 18,206 and 6.9%; graph's 31,358 is explained. “Six root items unlock the rest” is not established; R2-3. |
| 16 | partial | §6.2(a) covers recursive paths, strangers/silent assertions and arity. It misses physical array storage in 2b and says moving **any** cfg makes main silent; R2-1. |
| 17 | partial | §4.1 correctly identifies vte feature difference, independently reproduced. Replacement unnecessarily removes --bin folio; already-workspace ignored-test script unmentioned; R2-7. |
| 18 | partial | §4.3 supplies both package overrides. Separate app harness, workspace/vendor exclusions and repeated links remain unaccounted for; R2-6. |
| 19 | partial | §4.4 targets PDB/debug work and names developer's product EXE. Exact image-size attribution and “will not move it” exceed the evidence; R2-8. |
| 20 | partial | §4.4 requires a frame guard before debug changes; §4.3 records margins. Guard lacks a source-line/real-hang criterion; “none touches crates” contradicts adding it; R2-8. |
| 21 | partial | 112/150 no-merges and 29/24/17/12 reproduce. Four inline name-only readings reproduce. “98 ... under no reading” is false under enumerate-then-show; R2-12. |
| 22 | closed | §8.2 defines compiler-enforced substitution at a named call site and requires feature plus CI configuration for later compile-out. |
| 23 | closed | §6.4 removes release-test recovery as a Step 2 benefit; stack/profile trials remain separate and untested here. |
| 24 | closed | §1 and §9.7 acknowledge tests directory, bin-only API limitation and two unscheduled reasons for a future library. Metadata confirms targets. |

## Phase 4 — numbered findings and executable corrections

### R2-1 — blocking: Step 2's prescribed preparatory commits cannot land green

**Evidence:** plan §6.2 says all three non-relocation pieces should land before the freeze; §6.5(1) explicitly includes final array entries, repointed pins and the child selector. Before relocation, a new runtime/launch.rs allow-list entry is silent and fails both platform guards. A new include path can be nonexistent or lack its subject. A child selector aimed at a future test identity finds zero tests; adding the proposed completion marker makes that failure visible. Even after preparations, moving private methods still needs visibility/import/path edits, so the freeze cannot truthfully contain “text relocation only.”

Platform mechanics are more specific than the prose. Rust sources() is recursive and normalizes relative names (main.rs:163045–163066); strangers and silent are checked separately (:163103–163140). PowerShell does the same (check-portable-core.ps1:248–289). **Main becomes silent only after its last detected platform use leaves.** The guards scan lines, not parsed Rust: the test needle strings at main.rs:163322 themselves contain cfg/platform spellings, so retaining that test in main can keep main on the list even after startup code moves. At this snapshot actual platform cfg lines start at main.rs:112433, beyond both Runtime impl ranges. A strict two-impl Step 2a moves none of them. A launch/free-function expansion changes that file list and must say so. If 2b externalizes platform_gate_tests, PowerShell's physical main.rs regex reader (:216–233) loses the array even if a #[path] declaration preserves its Rust identity.

**Correction:** before freezing, land only topology-compatible guard infrastructure: recursive enumeration, precise pin subject mappings, missing-subject failures and child completion proof using its **current** identity. Land actual path/selector/allow-list/arity changes atomically with the corresponding move. Keep the array physically in main with a deliberate test visibility rule, or retarget its sole PowerShell reader in that same move. Demonstrate both unlisted-new-file and stale-listed-file failure directions during implementation. Permit narrowly enumerated visibility/import changes during relocation. Review commits separately; merge only a validated batch.

### R2-2 — blocking: one of the three prescribed whole-crate negative pins is a scoped method invariant

**Evidence:** §6.2(b) and inventory §3.1 classify main.rs:103570 as !SOURCE.contains(...). It is actually **!source.contains(fetch)**, with source = body("    fn mini_source") at 103555 and a positive buffer.content.is_some() assertion at 103557. It forbids claim_head_read, preview_worker, files_worker, capture_page and web_thumbs **inside the projection method**. Those names legitimately occur elsewhere. Converting it to a whole-crate prohibition would reject the current program or force somebody to weaken the guard. This error was inherited from the first review; accepting it in the ledger does not make it correct.

**Correction:** keep this assertion scoped to mini_source, with a required unique subject and loud failure when absent. Main.rs:15772 and :105705 are the two cited whole-SOURCE negatives and need their intended full-source coverage preserved. Reclassify all 91 invocations by actual subjects and assertions; do not prescribe “88 loud + three global” as a proven partition. A shared SOURCE can support several subjects destined for different files, so one include replacement per invocation is not generally sufficient. Whole-crate scans must define test-literal/comment handling to avoid self-matches. Record and mutation-check each converted guard's scope during implementation.

### R2-3 — blocking for the blocked Step 3 table: six headings are not six sufficient cuts

**Evidence:** Phase 2 derives the conditional APP_NAME result, arrival's additional marks prerequisite, the marks/icons bundle, tooltip's remaining cycle and preview's missing function/model cuts. The six subtotal rows omit formula_tools' 1,194 lines numerically while implying it follows. ChromeSprite ownership is wrong in both generated and handwritten notes. The 11,129 includes version tests that cannot simply leave their app/build-resource owner.

**Correction:** replace “move this root item and these follow” with a prerequisite DAG naming exact items, receiving owner, API changes, test/build inputs and retained adapters. Count only actual moved extents. Keep all 23 modules blocked until their individual contracts exist; do not create a constants crate or a whole AppEvent library to preserve a line-count claim. The current 13 free rows are a separate queue and need not wait for all these designs.

### R2-4 — should-fix: the generator is presented as a broader safety net than it implements

**Evidence:** §7.6 says the generator exits 1 “on exactly” a row needing a root item mid-ticket. It detects that only when its source approximation captures the edge, the graph is rebuilt, and it still models actual topology. The ROWS comment promises dependency order without a corresponding assertion. No check reads the plan; no check derives BLOCKED_NOTE or the six-root table.

**Correction:** state the contract as set/cfg agreement and require builder then generator on the recorded source commit. For future tooling work, add row predecessor validation, grouped-SCC semantics where needed and source/graph identity; keep API and build/test ownership review explicit. A successful invocation is not approval to extract a row. Resolve context_menu's home before its ticket: placement in bt-persist adds an i18n dependency to a currently independent low-level crate; placement in bt-i18n changes that crate's remit.

### R2-5 — should-fix: Step 1 still has no single executable five-file change

**Evidence:** §5 lists five files while allowing colour_name to move to “the picker or the type owner.” The type owner is marks.rs, making a sixth touched file. The pixel formatter's receiving owner is unspecified. Keeping it in preview or moving it to main retains an outgoing i18n edge. Other callers exist at file_peek.rs:2852, main.rs:22723,69698 and preview.rs:4320.

**Correction:** one concrete five-file cut is:

| File | Intended edit |
| --- | --- |
| crates/bt-app/src/i18n.rs | Remove colour_name and profile_entry_fault; receive the one pixel formatter from preview; make picture_shown_at call it locally. Preserve Text and language selection semantics. |
| crates/bt-app/src/settings.rs | Own colour_name beside its only caller at 4083; adjust that caller. This chooses the picker, not marks. |
| crates/bt-app/src/profiles.rs | Own profile_entry_fault with one current-language read per call; update the test at 23009. |
| crates/bt-app/src/preview.rs | Replace the pixel formatter definition with an i18n formatter re-export, preserving existing preview-qualified callers. |
| crates/bt-app/src/main.rs | Retarget the fault-formatting calls at 36750 and 51453. |

No new crate, manifest, lockfile or file_peek edit is required for this choice. Keep the cut as one independently gated commit; bt-i18n extraction is later. Before dispatch name the receiving tests and output fixtures: all eight colour mappings in both languages, both fault variants with representative IDs, exact width × height spaces/U+00D7 including boundary numbers, picture-shown text and unchanged English/Chinese Text tables. The profiles test only checks output contains “fish”; it is not an exact-output comparison. Rebuild the graph after the actual cut rather than accepting simulated deletion of every i18n out-edge as proof.

### R2-6 — should-fix: aligned opt levels do not remove the app harness or its link

**Evidence:** §4.3 says the graphs “share nothing” and overrides make flags match “unit for unit.” Cargo test inherits dev settings; package."*" excludes **workspace members**, including the vendored alacritty_terminal here. Normal folio and its --test/cfg(test) harness remain different units. The latter still compiles app test code and links a large executable even when dependencies and the normal bin can be reused. Build dependencies also have separate default treatment. [Cargo profiles and overrides](https://doc.rust-lang.org/cargo/reference/profiles.html#overrides).

**Correction:** make reuse conditional on matching target, mode, features, profile and flags; record which units become fresh. Price harness codegen/link and suite runtime at app opt-level 0. The wildcard leaves bt-app, bt-math, bt-render, bt-term and the vendor workspace member at 0; it does not mean “all app dependencies are optimized.” Generic code instantiated in the app may also use app settings. [Cargo generic optimization rules](https://doc.rust-lang.org/cargo/reference/profiles.html#overrides-and-generics). Both explicit overrides are valid; documented test→dev inheritance is not an unanswered semantic question, although effective flags should still be recorded. Do not promise the historically described “3,749-test harness” disappears: no harness census/timing was run here, and alignment does not split it.

### R2-7 — should-fix: use the exact shortcut selection diff and name the ignored-test consumer

**Evidence:** offline feature trees reproduce the vte default difference and corresponding bit-set default difference. Both scripts have the same invocation at check-shortcuts-table.ps1:31–32 and generate-shortcuts-table.ps1:32–33. The §4.1 replacement removes their target restriction, selecting unrelated workspace harnesses and harness-free targets too. Metadata finds exactly one folio target, in bt-app.

**Correction:** in **both** scripts replace only --package bt-app with --workspace, retaining target selector, call operator, continuation, pipeline and downstream handling:

```powershell
& cargo test --workspace --bin folio --locked -- --exact `
    shortcuts::tests::docs_shortcuts_md_is_the_bindings_table | Out-Host
```

This aligns selected workspace package features while running the intended bin test target. The checker must fail on cargo failure or changed table; the generator must delete previous output, require a freshly written file and copy it even when the renderer's expected comparison fails. Do not give the two scripts an identical error policy. Check that exactly the intended renderer test executed. Step 3's later library extraction must retarget both scripts and package-relative output paths again.

scripts/ci/check-ignored-tests.ps1:46 **already** uses `cargo test --workspace --locked --color never -- --ignored --list`; no analogous package-selection fix is needed. It compares two ways against scripts/ci/ignored-tests.txt, but discards target identity and deduplicates names; it is not Step 2b's target/full-name/multiplicity map. Include it in the relocation gate and update its list in the same commit if an ignored identity changes. Its current bt-app entries are in marks, preview_typing and preview, rather than main's inline tests; a main-only split should not invent an ignored-list edit.

### R2-8 — should-fix: the diagnostic prerequisite is underspecified and contradicts Step 0's file scope

**Evidence:** §4.4 targets the PDB and recognizes that a test-profile build can replace target/debug/folio.exe. But the exact ~124 MiB “test code plus libtest” decomposition compares historical binaries from different profiles/builds without symbol/section analysis. Shared generic libraries in both images do not establish equal instantiated/reachable machine code. PDB work is the principal target; “will not move” the image at all is unestablished.

The child at main.rs:116321 uses the production panic hook but its log assertion only looks for “unwrap” (:116432). A **named** frame alone does not check filename/line quality or the hang resolver against the matching image/PDB. §4.4 asks for a new source assertion while §4.5 says Step 0 touches only scripts/manifest and rollback is only those lines. Both cannot hold.

**Correction:** make a separate source-guard prerequisite with main.rs ownership and its own review/gate. Require a resolved application frame **and source location** in a contained panic, plus a real hang sample resolved using the matching image/PDB; preserve that association in evidence. Demonstrate guard failure under a deliberately inadequate debug artifact in the later implementation trial, not this docs-only review. Retain the guard when reverting a profile experiment unless the guard itself is faulty. Measure the 60-second outer watchdog, 30-second warm-up and 1/5/10-second request budgets—five distinct limits, with three request budgets—on the actual runner. A green containment run is not evidence of diagnostic fidelity; an opt-level change can make an existing timeout test red, so “no Step 0 profile change can make any existing test go red” is false literally. Quantify PDB bytes, image bytes and link time separately.

### R2-9 — should-fix: the baseline asks stable Cargo for evidence it does not directly emit

**Evidence:** §4.2 asks to retain --timings JSON and says HTML names the link step alone. Stable Cargo's timing report is HTML reporting **compiler units**, not an isolated linker-process trace; binary codegen boundaries are not generally shown. Ordinary HTML is not a supported machine-readable JSON artifact. [Cargo timing report](https://doc.rust-lang.org/cargo/reference/timings.html), [cargo test timing output](https://doc.rust-lang.org/cargo/commands/cargo-test.html#compilation-options).

**Correction:** retain the pinned stable toolchain, timestamped HTML and command/verbose logs; specify a separate Windows process-time/RSS measurement if link-only seconds and memory are acceptance inputs. Do not silently switch to nightly for JSON. Fix a repeatable edit/restoration procedure and hold features, target directory, jobs and flags constant. Remeasure the final combined profile: changing only test debuginfo after aligning opt levels can split newly shared units again. Separate trial wins are not additive evidence for the final gate.

### R2-10 — should-fix: release work has a rebase instruction, but no bounded freeze protection

**Evidence:** §6.5 names a source commit and asks to inventory open branches, yet starts by claiming “essentially every open branch” from history. It supplies no maximum window, coordinator, feature-priority rule or abort point. The handoff's long running entry is at docs/handoff/HANDOFF-2026-08-21.md:6, not a reliable live ticket census at the cited 216. It records formula work, issue #1/#2 paste, T-REMOTE-INPUT/TSF and card work, with already-landed and pending states intermixed.

Read-only local refs show docs/paste-paths-design at 6ab4942d (design/review files), and chore/root-tidy-and-one-feature-set at 8e6cf0cc, whose changed-file list overlaps main, Cargo.toml and both shortcut scripts. Main is 6a414963. These are **observed local refs**, not proof of all current work, unpublished edits or permission to inspect other worktrees. They suffice to defeat an assumption that the snapshot's first script fix remains unowned. No branch was checked out or modified.

**Correction:** before freezing, record each affected ticket's owner, base/tip, touched items, outstanding acceptance and land-before/rebase-after disposition. Explicitly protect these lanes:

| 0.4.1 lane | Overlap to map | Acceptance to retain after controlled rebase |
| --- | --- | --- |
| Formula block/toggle/hover | Main math/input/frame methods, formula_tools, preview/seats; robustness child | Per-block gesture/hover identity, source toggle/copy, result routing, stale results and worker survival; preserve existing regression evidence. |
| Paste files/images, issues #1/#2 | Main keyboard/clipboard/drag routes, profiles/settings and platform clipboard/drop interfaces | Payload precedence/quoting, active leaf/namespace ownership, cancellation/stale completion and app-focus behavior; preserve design decisions awaiting implementation. |
| T-REMOTE-INPUT / TSF | Main event dispatch, focus/IME state, native platform adapter | Real text-service input, composition/focus and candidate behavior. Synthetic keys/unit gates do not prove the missing TSF path. |
| Card/thumbnail/wheel | Main input/frame/resize methods, focus_thumb, web_thumb, seats | Reflow/content anchors, mixed-DPI/fullscreen transitions and reverse-wheel response; retain real-window checks. |

Land ready release fixes first. Prepare relocation/guard infrastructure outside the merge window, then refresh the manifest on a single green base. Freeze **merges touching the manifest**, with one coordinator, a recorded maximum duration and an abort/reopen rule; unrelated work can continue. Gate the final batch under repository policy, including after conflict resolution; never merge half to save the window. Feature owners rebase once and revalidate tests plus meaningful platform/UI acceptance. If owner/base/acceptance facts are unavailable, delay the split instead of blocking release lanes indefinitely.

### R2-11 — should-fix: ordered bodies plus a full gate do not prove zero behavior change

**Evidence:** §§6.3,9.3 compare each relocated item's parsed token **body**, permitting paths/visibility. This detects reordered effects within a body, but does not itself compare signatures, cfg/other attributes, module ancestry, impl/trait context, import resolution or macro expansion. Bodies can retain tokens while bare helpers resolve differently or source locations change. An extractor that blanks strings like the dependency graph would also erase literal changes Step 1 forbids. Source pins inspect unexpanded source and can accept a wrong or test-literal subject. --list reports identities for one built configuration; it does not prove a child or test body ran.

**Correction:** pair old/new fully qualified item identities, kinds, spans, signatures, attributes/cfg, impl context and **literal-preserving ordered tokens**; detect missing/duplicate pairings. List each permitted change with old/new tokens and reason, not a broad regex exemption. Audit bindings and macro/file-relative inputs separately. Keep tests under their production owner's scope or list the minimum visibility needed; root callers often require pub(crate), not only pub(in crate::runtime). Keep exact-name child completion evidence and multi-subject pin mappings. Review rustfmt-only deltas separately against the same semantic pairs.

The future landing gate remains necessary: workspace tests, all-target clippy, fmt, portable-core, shortcut and ignored-test checks, plus other applicable repository script gates. Windows behavior and macOS all-target checks have different coverage; Linux excludes bt-app. Unit success cannot establish timing, native IME/TSF, window focus, rendering or platform behavior without targeted observations. State “no intended product behavior change, supported by these comparisons and tests,” and identify changed test/source/diagnostic paths. Do not call those checks a formal equivalence proof.

### R2-12 — should-fix: the corrected census still mixes units and overstates history

**Evidence and correction:**

- **165,815 − 63,487 = 102,328**, not 102,329 (plan §6.1, inventory §3.2). The latter retained the earlier newline-plus-one minuend. This does not restore ~40k: the post-2a root remains about 102k before imports/declarations, with 59,903 test-span lines there.
- Plan §2 and inventory §0.3 say 99 unclassified methods are 4.3% of 1,310. They are **7.56%** (92.44% classified). 2,086 / 48,879 body lines is **4.27%**. Label method and line shares separately; a claimed 95.7% method assignment cannot replace an item relocation manifest.
- The inventory's exact no-merges procedure reproduces **112/150**, seats29/i18n24/preview17/settings12. Inline git log -150 --name-only reproduces the four readings **56,45,112,119** for default/full-history/no-merges/first-parent. But enumerating default git log -150 --format=%H commits then calling git show --name-only on each yields **98/150**. Retract “98 reproduces under no reading”; specify revision, traversal **and file-display procedure**. Keep 112/150 as the chosen metric, never a probability for live branches.
- §3.2's “read far more often than written” is unsupported by the lexical census that preceding paragraphs correctly limit. Remove it or provide a read/write/callee audit. Concentration establishes neither access mode nor a compiler-enforced document-model boundary.

## Execution order and exact change boundary

This is a review prescription for later implementation, not an implementation authorization or an edit to the plan.

1. **Step 0 selection commit:** both shortcut scripts plus CONTRIBUTING's invocation/ordering. Reconcile ownership with the feature-set branch first. The ignored-test script already uses workspace selection. Baseline next; then isolated opt-level trial; then diagnostic source-guard prerequisite; then debug trial; then scoped linker trial. Keep results/reverts separable.
2. **Step 1 cut commit:** exactly R2-5's five source paths for the selected destinations, with named test/output evidence. Gate and land before bt-i18n extraction. Refresh against release edits first. One revert restores the cut while no later consumer depends on it; after extraction, rollback follows dependency order.
3. **Step 2 preparation commits:** guard infrastructure compatible with current main/test identity, then an item/visibility/test-consumer manifest on a recorded base. The theme list is not an exact file list. State whether 2a moves **only** two Runtime impls or also free functions/trait impls: that determines platform ownership and residue.
4. **Step 2a batch:** crates/bt-app/src/main.rs, crates/bt-app/src/runtime/mod.rs and the **enumerated** receiving theme files, with necessary visibility/import/source-pin/array changes in each move's commit. Twenty-six unnamed wildcard destinations are not a dispatch manifest. Preserve test identities where possible. Within-crate splitting requires no Cargo manifest/lock edit; any such change needs separate justification.
5. **Step 2b batch:** main's enumerated test modules into enumerated files. Prefer root #[path] declarations where tests do not need inaccessible child-private items; otherwise use the explicit identity map. Include the child selector, all moved file-relative includes (main.rs, seats.rs, persist.rs, preview_edit.rs and others actually affected), shared fixtures and physical platform-array reader decision. scripts/check-portable-core.ps1 changes if array storage changes; scripts/ci/ignored-tests.txt changes only for actual ignored identity changes. No library target is necessary.
6. **Gate and thaw:** validate the final combined tree and all post-conflict edits; record merge base/result and item map; rebase affected feature branches once. The batch must pass on the tree actually merged. Independent green commits on different bases do not compose automatically.

**Rollback:** before dependent feature work lands, revert the relocation batch as a unit, including path/visibility/array/selector changes. Keep useful independent guard infrastructure that still passes against restored topology. After feature work lands, preserve its patches and port them to restored item locations, or revert dependent work in reverse order with an explicit restoration sequence; a blind revert may conflict or remove feature intent. Re-run the applicable full gate and give owners the inverse item map. Restore only the exact lockfile delta if later extraction introduced one; Step 2 alone should have none. A timeout/failed gate during the freeze means abandon the unmerged batch and reopen the window, not land a partial tree.

## Phase 5 — final verification

The review has all 24 ledger dispositions and 12 consecutively numbered findings. Both generated table comparisons passed: 13 free rows by membership/physical/production columns and 23 blocked rows by module/physical lines. Protected source, scripts, manifests, plan and inventory have no diff against e647415e. The only new tracked-scope path is this review; target/review2-notes.md remains retained as scratch evidence. UTF-8 decoding, LF-only endings, absence of BOM/replacement characters and the STATUS-first requirement were checked. No build/check/test was run, and no performance or runtime-equivalence result is claimed.

## Verdict per step

| Step | Verdict | Dispatch condition |
| --- | --- | --- |
| 0 | **go with changes** | Target-preserving two-script selection fix; measured unit/link baseline; separate diagnostic source guard; effective-profile and combined-trial validation. Resolve overlapping feature-set branch first. |
| 1 | **go with changes** | Choose the five-file cut or explicitly revise the file list; name output/receiver tests and refresh against release edits. |
| 2a | **go with changes** | Replace impossible preparatory order; fix pin scope; supply item manifest and bounded release-aware freeze. Strict two-impl residue is 102,328. |
| 2b | **go with changes** | Exact test/fixture/source-consumer map, physical platform-array ownership, same-commit path/selector updates and child completion proof. |
| 3 | **go with changes** for 13 free rows; **rethink** six-root unlocking table | Membership/numbers reproduce. Ticket each row only after API, features, tests, fixtures and receiving owner are fixed. All 23 blocked modules require explicit cuts; grouped cycles/build identity cannot be inferred away. |
| 4 | **rethink; do not dispatch** | Retain withholding. Require candidate state/call audit and executable identity/order/lifetime/publication contract before promising substitution. |

## Single first action

**Reconcile and land one target-preserving workspace-selection fix for the two shortcut scripts, coordinated with the already-observed chore/root-tidy-and-one-feature-set owner.** Use R2-7's exact invocation and preserve each script's check/generate behavior; establish that one renderer test ran. This precedes the baseline and requires no Step 1/2 freeze. These changes remain future work; this review changed documentation only.
