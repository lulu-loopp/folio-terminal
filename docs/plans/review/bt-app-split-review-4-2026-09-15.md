STATUS: COMPLETE — R-BT-APP-SPLIT-4; docs only; both tickets require corrections before dispatch.

# R-BT-APP-SPLIT-4

**Step 0 baseline ticket: dispatchable with these corrections — R4-1 and R4-2.**

**Step 1 cut ticket: dispatchable with these corrections — R4-3.**

## Review boundary

Reviewed revision 4 and the inventory at `1aac358ca69498eb47fbe0a741a20608af397796`, branch `docs/bt-app-split-plan`. Read the priority plan and third review first. All workspace operations remained in `D:/Developer/bt-wt/bt-app-split-plan`. Plan/inventory references below are line numbers at that HEAD; Rust references are under `crates/bt-app/src/` at **`ee771130`**, unless stated otherwise.

Installed Cargo: **1.94.1**, commit **`29ea6fb6a5db279426f4cc4e17aa385f05a0cfbc`**, host `x86_64-pc-windows-msvc`, matching `rust-toolchain.toml`. Verification used `cargo --version --verbose` and read the upstream serializers/report writer at that exact commit. This worktree contains no saved Cargo timing HTML or compiler-artifact stream to inspect. No build/check/test was run, and no emitted-record or timing experiment is claimed. Source verification establishes what the installed version emits. Evidence is retained in `target/review4-notes.md`.

## Step 0: seven entry boxes

A checkable prerequisite is not necessarily a populated prerequisite. The plan remains a ticket template in the following respects.

| §4.0 box | Checkability and disposition |
| --- | --- |
| 1. Released green base with both fixes | Checkable after release with the recorded release/gate evidence and two ancestry queries. Both `8e6cf0cc` and `5fd6f935` are ancestors of `ee771130` (exit 0); neither is an ancestor of this worktree's HEAD (exit 1). Do not use the documentation branch as the measurement base. This review does not certify that 0.4.1 has shipped or its release gate is green. |
| 2. Named owner and runner | Checkable ticket fields; neither is populated here. A repository and a Windows machine alone do not identify the intended owner/runner. |
| 3. Own target directory | Checkable once the runner worktree and resolved absolute target path are recorded, including environment/config overrides. The path is not populated here. |
| 4. Exact command sequence | Four commands are present at plan 515–518, but they do not produce all the evidence the table requires; R4-1. |
| 5. Repeat/reset procedure | Cold resets and warm priming are explicit. App-edit cache restoration and evidence survival across cold resets remain unspecified; R4-1/R4-2. |
| 6. Literal edit/restoration | Checkable in principle, **absent in the plan**. Plan 567–570 tells a ticket writer to supply it; it does not supply it. R4-2. |
| 7. Evidence schema/link declaration | The underlying quantities are obtainable, but capture and context requirements need R4-1. Explicitly choosing `unmeasured` for uninstrumented link-only measurements is acceptable. |

### §4.2 evidence fields, checked against installed Cargo

| Evidence row | What is actually available |
| --- | --- |
| Whole compiler-unit elapsed | The timestamped HTML exists at the stated default path. The unit table's actual column is **`Total`**, backed by embedded **`UNIT_DATA[].duration`** in seconds; the table rounds to one decimal, embedded durations to centiseconds. Only executed/dirty units have individual timing entries; fresh units contribute aggregate counts, not per-unit elapsed entries. Build-script execution is also a timed unit and must be distinguished from compilation. [Exact report writer](https://github.com/rust-lang/cargo/blob/29ea6fb6a5db279426f4cc4e17aa385f05a0cfbc/src/cargo/core/compiler/timings/report.rs#L267), [timing collection](https://github.com/rust-lang/cargo/blob/29ea6fb6a5db279426f4cc4e17aa385f05a0cfbc/src/cargo/core/compiler/timings/mod.rs#L190). |
| Compiler-artifact records | **Every named JSON field exists:** `package_id`, `target.name`, `target.kind`, `target.test`, `profile.opt_level`, `profile.debuginfo`, `profile.test`, `features`, `filenames`, `fresh`. `target.test` is target test eligibility; `profile.test` is the invocation's test mode. `opt_level` is a string; debuginfo permits numeric or named values. These are artifact records, not an enumeration of every internal unit or build-script execution. [Artifact/profile serializer](https://github.com/rust-lang/cargo/blob/29ea6fb6a5db279426f4cc4e17aa385f05a0cfbc/src/cargo/util/machine_message.rs#L42), [target serializer](https://github.com/rust-lang/cargo/blob/29ea6fb6a5db279426f4cc4e17aa385f05a0cfbc/src/cargo/core/manifest.rs#L380), [emission and test-mode assignment](https://github.com/rust-lang/cargo/blob/29ea6fb6a5db279426f4cc4e17aa385f05a0cfbc/src/cargo/core/compiler/mod.rs#L686). |
| Command context | A `-v` log supplies commands that actually run; it does not reliably serialize all requested settings, especially effective flags for reused units. Record explicit context in addition to logs; R4-1. |
| Link-only seconds/peak memory | These come from the specified external instrumentation, **not stable Cargo artifacts**. Without it, record both link-only time and linker peak memory as `unmeasured`. The declaration at plan 546–550 is honest; whole-unit `Total` cannot replace it. The documented stable report does not expose the binary codegen/link boundary. [Version-matched timing documentation](https://github.com/rust-lang/cargo/blob/29ea6fb6a5db279426f4cc4e17aa385f05a0cfbc/src/doc/src/reference/timings.md#L23). |
| EXE/harness/PDB bytes | Obtainable from the filesystem, using artifact paths and immediate per-command capture before later commands replace them. Path/mtime identify a captured observation, not a permanent copy of overwritten contents. |
| Suite wall-clock | Obtainable with an external command timer. It measures the final Cargo command, including any preparation/rebuild it performs; it is not a Cargo unit duration or necessarily test-execution-only time. |

## Step 1: five entry boxes and source references

| §5.0 box | Checkability and disposition |
| --- | --- |
| 1. Five destinations | Fixed and source-checkable; verified below. |
| 2. Named test table | One existing test and five explicitly new test identities; the seventh row is a source comparison, not a test identity. Fixtures need the caller/isolation correction in R4-3. |
| 3. Refreshed references | The substantive definitions, callers and fixtures match `ee771130`; refresh again against the actual release base as required. |
| 4. Per-test global-language route | A pure inner or a contained child is implementable. The prescribed route for the picker and whole `picture_shown_at` conflicts with their acceptance rows; R4-3. |
| 5. One commit, separate from extraction | Checkable as an entry scope commitment and later as a landing property. |

### Five destination edits at `ee771130`

| Destination | Independently checked source |
| --- | --- |
| `i18n.rs` | `colour_name` starts at 6359; `profile_entry_fault` at 6378; `picture_shown_at` at 6550 calls the preview formatter at 6554. The latter function stays and calls the received formatter locally. |
| `settings.rs` | Only production color-name caller at 4083, inside `SettingsRow::option_label` (3984); it calls `.text()`. `option_labels` (3979) exposes the picker labels. |
| `profiles.rs` | Existing fault-formatting assertion at 23009, within `profiles::tests::an_entry_that_is_neither_a_builtin_nor_a_program_is_dropped_and_named`, 22990–23013. |
| `preview.rs` | Formatter definition at 4238–4240; internal caller at 4320 can continue through the re-export. |
| `main.rs` | Fault calls at **36750**, inside `Runtime::create` (36605), and **51453**, inside `Runtime::reread_profiles` (51376). Pixel-format calls at 22723 and **69720** remain preview-qualified. |

The unchanged external caller is `file_peek.rs:2852`, with `(1180, 800)`. `MarkColour::ALL` is at `marks.rs:72`; color literal pairs are `i18n.rs:3462–3469`; fault literals are at 6381/6384/6387/6390; the remaining error-type references are at 6579 and 6695. Minor locator precision: `CURRENT` is declared at **249** (248 is its comment), and the ineffective guard's function is at **8849** (8848 is `#[test]`). `in_lang` is at 2689, but the comment explicitly describing the multi-column test entry point belongs to **`on` at 2693–2696**. These do not prevent locating the code.

### Named test identity check

All six test identities target package `bt-app`, binary `folio` (`crates/bt-app/Cargo.toml:15–17`). Their containing `tests` modules exist.

| Full identity | Result |
| --- | --- |
| `settings::tests::profile_colour_labels_preserve_both_languages` | Absent; explicitly **new**. |
| `profiles::tests::an_entry_that_is_neither_a_builtin_nor_a_program_is_dropped_and_named` | Exists at 22990; formatter assertion is `contains("fish")`. Retargeting preserves the existing test. |
| `profiles::tests::profile_entry_fault_preserves_exact_outputs` | Absent; explicitly **new**. Both named variants and all four frozen format strings match source. |
| `i18n::tests::pixel_size_preserves_exact_output` | Absent; explicitly **new**. All four decimal/space/U+00D7 outputs match the formatter. |
| `i18n::tests::picture_shown_at_preserves_exact_output` | Absent; explicitly **new**. Needs frozen complete sentences and actual-function coverage; R4-3. |
| `preview::tests::pixel_size_reexport_preserves_output` | Absent; explicitly **new**. `(1180, 800)` matches the cited caller. |

The two main call sites are correctly offered as bounded before/after source-and-caller evidence rather than assigned an invented test identity. This read-only review does not prove the broader negative claim that no existing runtime test can ever reach either method.

## Remaining findings

### R4-1 — blocking Step 0 dispatch: the copied commands cannot deliver the promised evidence bundle

**Evidence:** plan 515 has no JSON flag, although 540 requires JSON on build as well as test. Clippy at 517 also lacks it, leaving no artifact census for that measured command. All commands within one case/repetition share the proposed `<case>-<rep>-artifacts.jsonl` and `-verbose.log` names (540–541), with no append framing or command discriminator. HTML is retained only under `target` (539), which cold repetitions delete in full (560–563). The context row attributes all settings to `-v`, including values it does not necessarily print for fresh work.

**Correction:** put `--message-format=json` on each of the three measured preparation commands, before Clippy's `-- -D warnings`. Capture stdout and stderr separately, with names containing **case, repetition and command**. Archive each HTML and image-size observation immediately after its command into a named evidence directory **outside the reset target directory**, before the next command/reset. Record command, working directory, exit status, timer readings, source hash, toolchain, resolved target/flags/jobs/target directory, configuration and feature evidence explicitly; do not reconstruct omitted context from Fresh lines. Name HTML `Total`/`UNIT_DATA[].duration` and label fresh units as having no individual timing entry. Keep artifact census and build-script-execution records distinct. The link instrumentation may remain absent with both associated quantities explicitly unmeasured.

### R4-2 — blocking verbatim Step 0 dispatch: the literal edit is missing and repetitions do not restore the same build state

**Evidence:** plan 347–348 requires a literal line/edit/restoration, but 567–570 delegates writing it to the ticket. Ledger 2289 says these fields were filled; they were not. Restoring only source bytes after an edited build leaves compiled/incremental artifacts for the edited program. Reapplying the same edit on repetitions two and three therefore need not start from repetition one's unedited build state. No measured timing is needed to identify that procedural difference.

**Correction:** before dispatch, populate the owner, runner, release/gate evidence and isolated absolute target path. Include the exact release-base file/line, original bytes, replacement bytes, replacement-count assertion and byte-for-byte restore procedure. State whether the probe is comment-only or a production-code edit. After each restoration, rerun the full sequence on the **unedited** source to green, discard those priming measurements, then apply the same edit for the next measured repetition. Alternatively specify a reproducible cache-snapshot restore that preserves Cargo's freshness assumptions. Explicitly label the chosen cache regime; restoring source alone is insufficient. Keep all such re-priming outside measured repetitions. Remove the ledger claim that the literal procedure is already supplied.

### R4-3 — blocking Step 1 dispatch: the table and isolation rule require different execution paths

**Evidence:** plan 906 requires both languages through the picker, `.text()` included. Plan 931–934 instead requires `in_lang` and no language switch. The actual picker calls `.text()` (`settings.rs:4083`), and `.text()` reads `current()` (`i18n.rs:2678–2679`). `in_lang` cannot set the language that caller observes. Similarly, `picture_shown_at` calls `.text()` internally (6553); constructing its two-language result from `Text::PictureShownAt.in_lang` tests a reconstruction, not that function. The oracle in plan 910 also references the live `Text::PictureShownAt.text()` instead of freezing both complete outputs.

**Correction:** choose contained-child coverage for the actual picker and `picture_shown_at`. Within each child's exact selected test, install English and Chinese sequentially and invoke **`SettingsRow::ProfileColour.option_labels()`** and **`i18n::picture_shown_at`** respectively. Keep `in_lang` checks as additional pure table checks. The parent must never install a language; launch `current_exe()` with the full test identity and `--exact`, use a child marker to prevent recursion, require success and a completion marker written after all assertions, and bound execution. This preserves the five-file boundary and prevents a zero-test child success from becoming acceptance.

Freeze the picture oracle from `i18n.rs:3248`: English prefix **`shown at `**, Chinese prefix **`显示为 `**, each followed by exactly `0 × 0`, `1 × 1`, `1180 × 800`, or `4294967295 × 4294967295`. Store the eight expected strings independently of the function/table being tested. For the fault formatter, either documented alternative remains implementable: a pure language-taking inner with one unchanged public `current()` read, or the same contained-child protocol exercising the public formatter. Write the chosen route for every row before dispatch.

### R4-4 — should-fix before Step 2 preparation: the scheduling table retains the universal subject rule

**Evidence:** plan 1058 still demands a missing-subject failure **on every pin**, repeated at 1383–1385. The corrected class table at 1205–1210 explains that whole-source prohibitions have intentionally absent forbidden text and instead need forbidden-text injection plus complete source enumeration. A copied preparation row can reinstate the rule R3-4 rejected.

**Correction:** replace that scheduling requirement with a reference to class-specific preparation/acceptance: unique-subject checks for named/scoped bodies, coverage enumeration for whole-source negatives, and preserved scope/count ownership for arity. For negatives, distinguish a missing input source from an absent forbidden spelling. Apply the same qualification in the freeze protocol.

### R4-5 — should-fix for the withheld Step 3 queue: Brief D still calls whole-file mass an actual move

**Evidence:** plan 1755 says **10,824 actually move**, and 1774–1775 calls this the extent after removing only version's 305 lines. That contradicts 1693's retained app-crossing tests and 1823–1831's exclusion rules. The caller account at 1772–1774 still omits production `crate::version::VERSION` consumers at `update.rs:517` and `:641`, explicitly identified in R3-5. The dispatch withholding at 1662–1683 remains operative.

**Correction:** replace both local extent claims with conditional whole-file mass, subject to the promoted item/test/build-input manifest. Add the two update VERSION consumers to Brief D's required caller inventory. Retain all 23 modules as blocked. No new extraction decision is required in this review.

### R4-6 — should-fix: the compile model still ranks and counts an unmeasured saving

**Evidence:** plan 2095–2102 and ledger 2295 call the build-script fix **probably the larger half** while acknowledging neither fix has a measured size. Plan 2108 still promises removal of **one full bt-app codegen and one link** from a script-inclusive gate. That is an outcome count, despite §4.1's withdrawal of guaranteed reuse and §4.2's post-fix-only evidence boundary. The two causes can invalidate the same work; their removed invocations cannot be assigned independently from the mechanism alone.

**Correction:** describe the two removed invalidation causes without ranking their contributions or guaranteeing an invocation count. Reserve relative importance and removed-work claims for matched before/after command sequences. Preserve the eleven-crate correction and the exclusion of fmt savings.

## R3 ledger dispositions

Closure means the requested documentary correction is present consistently; it does not mean implementation or runtime acceptance has happened.

| Ledger item | Disposition | Body evidence and remaining issue |
| --- | --- | --- |
| R3-1 | **Partially closed** | Plan 521–555 withdraws link-tail timing and package Fresh census; all named JSON keys exist. Capture commands, context and retention still need R4-1. |
| R3-2 | **Partially closed** | Plan 133–165 separates Entry/Acceptance/Landing; 327–355 separates baseline from guard/trials. Cold and warm procedures are written. Literal edit and repeat-state requirements remain R4-2; ledger 2289 overstates completion. |
| R3-3 | **Partially closed** | Plan 904–912 names six tests and a non-test comparison; the identities/existing-new labels check out. Caller coverage versus language isolation and the full-sentence oracle need R4-3. |
| R3-4 | **Partially closed** | Plan 1151–1154 corrects 89 const/two let; 1191–1195 corrects the two-constant precedent; 1205–1236 supplies class-specific mutations and source-union preservation. The scheduling/protocol shorthand remains inconsistent, R4-4. |
| R3-5 | **Partially closed** | Plan 1662–1695 withholds all 23 modules and supplies promotion/test-ownership requirements; 1823–1861 labels the sums and separates E's options. Brief D retains the conflicting extent/caller account, R4-5. |
| R3-6 | **Closed** | Inventory 248–255 withdraws total non-reuse; 813–826 withdraws excess-image attribution; 881–890 withdraws the platform-array work order; 657–680 distinguishes root ownership, strict free and avoid-largest, including 48/55,302 versus 51/66,273. These land in the inventory body. |
| R3-7 | **Closed** | Plan 1054–1069 puts destination visibility/imports with moves; 1373–1387 requires an owner-confirmed overlap inventory; 1398–1405 says to lift the freeze and schedule retries separately; 1409–1431 rejects empty-ref cost inference. Dated lane names remain evidence to refresh, not permission to freeze. |
| R3-8 | **Partially closed** | Plan 1888–1903 counts eleven new crates and labels the line-fraction model; 2116 removes fmt savings and requires per-gate measurement. The remaining unmeasured ranking/invocation-count claim is R4-6. |

## Verification record

Only this review and `target/review4-notes.md` were authored. No plan, inventory, previous review, script, Rust source, manifest or lockfile was edited. No graph regeneration, formatter, build, check, test, mutation execution or remote operation was needed for this pass. The report and notes are UTF-8, LF-only, without BOM; STATUS is the report's first line. Protected tracked files and branch/HEAD were checked after writing.
