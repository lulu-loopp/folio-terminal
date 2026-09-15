STATUS: COMPLETE — fix these first; phase 4 complete; 20 third-ledger rows verified (8 closed, 12 partially closed); eleven closure sentences checked (6 complete, 5 partial); 12 findings (4 blocking, 8 should-fix); LF and file scope verified.

# R-PASTE-PATHS-4 — fourth-pass review, 2026-09-15

Subject: `docs/plans/paste-paths-design.md` at `55cd5f9f9c32a8419630bca2ebe2cc349986035b` (2,529 lines), branch `docs/paste-paths-design`. `D:n` means line n of that design. `R3` is `docs/plans/review/paste-paths-review-3-2026-09-15.md`. Rust paths abbreviated below are explicitly identified at their first use.

Review scope: the complete design, R3's 20 corrections, both requested ledgers, relevant repository source and primary API documentation. These are document/source checks, not executed clipboard, shell, storage, image or drag probes. No cargo, builds, tests or application launches. Working evidence and phase records: `target/review4-notes.md`.

## Phase 1 — third-ledger verification

Statuses assess the accepted correction, its body text, fixtures and remaining contradictory instructions. R3 finding 5 is assessed against the owner's settled scope: preserving the standing pair and isolating T-PASTE-CYG. No table for the rejected derivation is required.

| R3 finding / third-ledger line | Status | Evidence and remaining correction |
| --- | --- | --- |
| 1 / D:2505 | **partially closed** | D:225–251 removes Windows sequence equality, keeps one open interval and retains macOS change-count equality. D:1864–1869 and D:1943–1950 add delayed-render success. The accompanying Windows replacement/refusal fixture contradicts that interval's exclusion guarantee; finding 2. |
| 2 / D:2506 | **partially closed** | D:259–293 and D:1475–1483 distinguish Folio's absent deadline from the system backstop, name `ClipboardRead`, and give both worker/helper options; D:2289 and D:2319–2322 agree. The claimed removal everywhere misses D:2442's measured-latency instruction; finding 12. |
| 3 / D:2507 | **closed** | D:179–189 and D:216–224 make the survey candidate selection, permit another rung after acquired `Absent`, and name the picture-list exception. D:1856–1862 tests empty HDROP plus text and all-non-file URLs. The ordinary-case one-rung statement no longer prohibits either exception. |
| 4 / D:2508 | **partially closed** | D:104–128 and D:1687–1720 carry all five payload values and a separate acquisition error path; D:2149–2156 and D:2174–2178 make the unimplemented picture rung silent. D:2104–2115 nevertheless puts promise-only refusal in the non-payload channel; finding 10. |
| 5 / D:2509 | **partially closed** | The accepted owner scope lands at D:725–749, D:772–816, D:848–852, D:1699–1702 and D:2143–2194. Automatic Cygwin integration yields Msys; explicit None yields Windows. The declined alternative needs no resolution table. But D:1831 says no derivation reads a program, contradicting the grammar derivation and T-PASTE-1; finding 11. |
| 6 / D:2510 | **partially closed** | D:798–816 removes the home field and states the mount assumption; D:818–846 covers the wrapper and six outcomes with all outcomes unchanged before PROBE 11. D:1840–1845 and D:1979–1984 add classifier fixtures. Candidate eligibility and evidence combined across two directories remain undefined; finding 7. |
| 7 / D:2511 | **closed** | D:594–682, D:1613–1622 and D:2432 use the Windows/POSIX split, scope shlex identity, withdraw universal multi-path failure and retain unverified recipient inheritance. D:1805–1809 and D:1986–1998 include roots, apostrophes and branch-sensitive measurements. The new override composition issue is assessed under row 9. |
| 8 / D:2512 | **partially closed** | D:450–468 labels unknown recipients best-effort and removes the REPL guarantee. Its replacement native-consumer fixture, D:1959–1966, uses a cmd row but demands no percent refusal; finding 3. |
| 9 / D:2513 | **partially closed** | D:859–888 defines `paste_paths_as`, insertion-only, with detector/environment untouched and Cygwin unavailable before its ticket. The exact Git Bash windows-slash configuration appears in D:1835–1838, D:1974–1978, D:2171–2172 and D:2285. PROBE 4 now names an expressible configuration. The grammar/spelling product, fallback string classification and WSL context are not fully specified; finding 1. |
| 10 / D:2514 | **closed** | D:1422–1469 supplies 64/256/64/512 MiB caps, the 24-hour floor, permanent lock file, crash-released flock or Windows no-share/LockFileEx lock, complete accounting/reservation/write exclusion and off-loop two-second contention refusal. D:1555–1606 specifies sweep ownership and the age-only pass exception. D:1880–1893 tests contention, crash release and the floor. No bypass or age-based lock stealing remains operative in the body. |
| 11 / D:2515 | **partially closed** | D:1201–1233 and D:1629–1633 distinguish configured Windows temp and Folio's Unix policy, disclose squatting and rename privacy paths. D:1280 still creates/vets Unix `Folio`; finding 8. |
| 12 / D:2516 | **partially closed** | D:1293–1316 adds owner SID, DACL, inheritance, ACL-capable storage and created-file read-back; D:2026–2037 adds the requested storage cases. The protected-DACL rule is reversed and the directory/file principal sets disagree; finding 4. |
| 13 / D:2517 | **closed** | D:1149–1164 separates file/folder verbs for both hover and commit, matching D:1071–1074. D:2004–2008 tests both refusal mirrors and Placeholder. |
| 14 / D:2518 | **closed** | D:484 and D:554–588 define the literal and refuse the unmeasured Nu arm. D:1801–1805 gates fence growth; D:1969–1973, D:2151–2153, D:2172–2173 and D:2291 carry the baseline/refusal policy. |
| 15 / D:2519 | **closed** | D:669–682, D:2166–2171 and D:2265–2272 use the same recorded Agent inheritance at both release sizes. D:2122–2131 and D:2290–2303 remove the child-read question from the operative probe and define macOS enabled-on-assumption with actual-denial refusal. PROBE 1 remains mandatory at D:2282. Historical first-ledger text still needs supersession labels; finding 12. |
| 16 / D:2520 | **partially closed** | D:1522–1551 adds target input generation and cleanup; D:1903–1909 tests Enter, typing, text paste, K144, drop and unchanged generation. The event list omits editing/navigation input that changes the line without typing or submission; finding 5. |
| 17 / D:2521 | **partially closed** | D:1507–1520 and D:1541–1547 add all three destination types and revalidation. D:1911–1917 and D:2010–2018 add delayed Preview/edge fixtures, carried into D:2250–2255. D:1493–1505 still requires a captured shell recipient for destinations explicitly lacking one, and root-rim capture lacks its own definition/fixture; finding 6. |
| 18 / D:2522 | **closed** | D:1375–1406 chooses complete APNG preservation, default-image plus all-frame validation under cumulative caps, refusal rather than truncation and an animation toast. D:1871–1878 specifies separate-default, truncated-animation and over-cap fixtures; D:2200–2203 includes the policy in 2a. Historical first-frame ledger text is addressed in finding 12. |
| 19 / D:2523 | **partially closed** | D:1239–1265 separates representability from per-recipient pre-write encoding and deletes every undelivered output. D:1895–1901 tests generic terminal preflight. The promised concrete redirected-temp-percent and unmeasured-grammar rows do not occur in §6.2; finding 9. Nonterminal preflight remains finding 6. |
| 20 / D:2524 | **closed** | D:2205–2216 explicitly enables `image/bmp`, assigns its own dependency/lockfile/notices check and makes absent-feature DIB-only input `Absent`; D:2388–2391 cites the gate. `Cargo.toml:107` confirms the current feature list is only gif/jpeg/png/webp. |

## Phase 2 — the eleven §10.2 closure sentences

The checked rows are exactly 1, 2, 3, 4, 5, 7, 9, 10, 11, 13 and 18 (`rg -n 'Closed by the third revision' docs/plans/paste-paths-design.md`, limited to §10.2, returns D:2464–2468, D:2470, D:2472–2474, D:2476 and D:2481). The table assesses the appended closure sentence against the body, not the superseded text preceding it.

| R2 row / sentence line | Body correspondence | Evidence |
| --- | --- | --- |
| 1 / D:2464 | **partial** | The deadline correction, platform coherence split and candidate selection appear at D:179–293 and D:1475–1483. The new Windows replacement fixture still lacks an implementable refusal condition (D:1864–1869, D:1948–1950; finding 2). |
| 2 / D:2465 | **partial** | Five states and silent T-PASTE-1 Nothing appear at D:1687–1720 and D:2174–2178. The claim that reporting channels are consistently distinct misses the contrary promise-only routing at D:2109–2115; finding 10. |
| 3 / D:2466 | **partial** | The owner decision, separate ticket, dark automatic classifier and removed home field appear at D:725–852 and D:2180–2194. D:1831's derivation assertion remains overbroad; the classifier has the gaps in finding 7. The settled owner decision itself is closed. |
| 4 / D:2467 | **complete** | D:1613–1622 and D:2432 carry the platform split; D:649–668 and D:1805–1809 narrow multiple-path and shlex assertions as claimed. |
| 5 / D:2468 | **partial** | D:450–468 makes the best-effort limitation explicit and D:1959–1966 replaces the REPL row. The replacement still passes through cmd while requiring the opposite of its percent refusal; finding 3. |
| 7 / D:2470 | **complete** | D:1425–1469 contains the crash-released lock, whole transaction, two-second off-loop refusal, no bypass and retention floor; D:1555–1595 withdraws live-prompt protection and defines the two sweep passes. |
| 9 / D:2472 | **partial** | Discovery, privacy and squatting changes appear at D:1201–1233 and D:1629–1633, but the asserted rename in vetting is false at D:1280; finding 8. |
| 10 / D:2473 | **complete** | D:2166–2171 and D:2265–2272 use recorded Agent inheritance at both release sizes; i18n remains in T-PASTE-1 at D:2160–2163. |
| 11 / D:2474 | **complete** | D:484, D:575–588, D:1801–1805 and D:2291 provide the raw-string formula, unrun whole-arm refusal and measured fence tests. |
| 13 / D:2476 | **complete** | D:1149–1164 splits files/folders, makes Preview-folder refusal explicit and applies it during hover; D:2004–2008 tests the mirrors. |
| 18 / D:2481 | **complete** | D:2122–2131 and D:2290–2303 remove the child-read probe from current body requirements and define enabled-on-assumption plus actual-denial refusal. |

## Phase 3 — fresh review

The full-document pass covers D:1–2529. The numbered findings below include remaining R3 corrections and defects exposed by their replacements. No implementation or native probe is represented as having run.

**Owner-marker check.** `rg -n -U 'OWNER RULING\s+(?:\r?\n)?NEEDED' docs/plans/paste-paths-design.md` returns only D:36–37 (the sentence saying no marker remains) and D:2466 (the historical second-ledger row, followed in that row by the dated closure). There is no active request for a ruling. D:725–749 and D:2324–2326 expressly close it. The requested exception for historical mentions is satisfied.

## Phase 4 — artifact verification

`git status --short --untracked-files=all` reports only `?? docs/plans/review/paste-paths-review-4-2026-09-15.md`; `git diff --name-only` is empty. `git check-ignore target/review4-notes.md` returns that notes path, which remains on disk. `git hash-object` and `git rev-parse HEAD:<path>` agree for the design (`26f422882145d96377a66053a352cf8fead25d85`) and R3 (`19f148d60a36c70558aca90264aec49fee99c8eb`). Byte inspection reports CR=0 for both output files and no UTF-8 BOM in the review. The review has 20 third-ledger assessments (8 closed, 12 partially closed; none not closed or regressed), eleven second-ledger sentence checks (6 complete, 5 partial), and 12 numbered findings (4 blocking, 8 should-fix). Command evidence and phase history remain in `target/review4-notes.md`.

## Verdict

**Fix these first. Do not open T-PASTE-1 from this design yet.** Findings 1–3 and 10–12 affect its contract or acceptance instructions. Findings 4–9 must be corrected before the picture, drop or Cygwin tickets implement the affected sections. The standing namespace ruling is settled; this verdict does not ask to reopen it.

## Numbered findings

### 1. Blocking — the spelling override has no complete composition contract

**Evidence.** D:424–426 permits six `paste_as` grammars. D:865–877 permits five independent spellings and says a row carrying one is believed, with only the pre-T-PASTE-CYG value explicitly refused. D:1835–1838 tests independence and one Git Bash example, not the product of the two keys. D:890–927 defines translation by an existing pane namespace and a same-distribution rule; it does not define where the distribution comes from for `paste_paths_as: wsl` on a wrapper/non-WSL row. The actual type needs that context: `crates/bt-transcript/src/paths.rs:105–108` has `Wsl { distro: Option<String>, home: Option<PathBuf> }`, populated in `crates/bt-app/src/profiles.rs:3205–3212` only in the WSL branch.

The grammar branches also depend on more than the key names. D:485–486 and D:628–631 select Agent quoting by Windows versus POSIX **path**, while D:1617–1618 selects it by the **temp path**; neither says whether an override-translated result or the original host path selects that branch. D:515 assumes a filename cannot contain a double quote, but §2.4's gate at D:690–714 accepts representable POSIX names containing one and the override has no host/domain restriction. D:447–448 and D:529–538 attach expansion refusals to the cmd interpreter specifically without defining whether an explicit `paste_as: cmd` on a wrapper selects that interpreter policy.

**Consequence.** An implementer must invent admissibility, context and branch-selection rules for accepted configurations. Examples needing an answer include Agent plus WSL/MSYS spelling, a WSL spelling override on a non-WSL wrapper, and Cmd grammar applied to a preserved POSIX spelling with an embedded quote. This is a missing contract, not a request to guarantee that every arbitrary spelling names a mounted file. The requested PROBE 4 configuration itself is now expressible: D:882–888 emits `'D:/Demo/a.txt'` without changing detection or the environment.

**Correction.** Specify the grammar × spelling combinations, allowing a common rule where it covers several cells: accepted domain or named refusal, the resulting string's path kind, fallback handling and required namespace context. Define the source/absence policy for WSL distribution context on overridden rows and the cmd-interpreter policy for grammar overrides. Encode the actual translated/fallback string under that contract, rather than relying on the original host filename's restrictions. Add combination fixtures, including quotes, apostrophes, roots, UNC and unsupported-host cases. Keep the override insertion-only and leave the standing detector derivation unchanged.

### 2. Should-fix — the Windows replacement fixture demands an event the open interval excludes

**Evidence.** D:243–248 makes one open interval the Windows coherence guarantee and makes the sequence number diagnostic only. D:1867–1868 nevertheless requires a genuinely replaced Windows clipboard to refuse “by the rule that is not sequence equality”; D:1948–1950 requires another process to rewrite it while the read is in flight. Microsoft's [OpenClipboard contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard) says opening prevents other applications modifying the content and a competing open fails. Its [delayed-render contract](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-operations#delayed-rendering) lets the rendering owner supply the requested data without opening, while Folio retains the open interval.

**Consequence.** The new delayed-render success expectation is consistent with the API. The adjacent ordinary cross-process replacement/refusal test is not: a legitimate competing copier cannot replace the clipboard inside that interval. A replacement before acquisition or after close has different semantics, and the design defines no additional Windows predicate for rejecting it. A fake that simply permits replacement while open would be modelling an API guarantee away.

**Correction.** Make the Windows concurrent-copy fixture assert that the other process cannot open/replace while Folio holds the clipboard, that Folio completes its acquired snapshot, and that a later replacement is available to the next gesture. Keep first-paste delayed-render success and no silent reopen. Keep macOS replacement/change-count refusal separate. If an exceptional Windows invalidation is intended, name the actual observable failure and its refusal path; do not reintroduce sequence equality to make this fixture pass.

### 3. Should-fix — the replacement CRT acceptance row still passes through cmd

**Evidence.** D:1959–1961 labels the fixture “The Cmd encoder outside cmd.exe” but launches the argv printer “from a cmd row, with `%` in the name, which must work rather than refuse.” D:529–530 requires a percent-bearing path to be refused in a cmd.exe pane, and D:1799–1800 asserts that refusal. D:467–468 repeats the proposed cmd-row harness.

**Consequence.** The actual paste recipient is still cmd's interactive line. Choosing a native child does not bypass Folio's row-based refusal or cmd's interpretation. Thus the fixture either fails the declared gate or requires an exception to the very behavior it is supposed to test. A filename containing a paired `%NAME%` also distinguishes cmd expansion from a lone harmless percent sign; the CRT's process-start argument rules do not remove the intervening interpreter.

**Correction.** Separate a pure/full-CRT encoder test from terminal acceptance. For the former, feed the exact encoded command line directly to a defined CRT consumer without a cmd interpreter, or to a documented CRT parser harness. For terminal acceptance, keep percent refusal in a cmd profile. If testing a non-cmd interactive row, identify a recipient whose input protocol actually consumes the chosen grammar. Include a paired-percent case; do not restore the Python/Node REPL guarantee.

### 4. Blocking — Windows inheritance vetting reverses the protection flag and mixes principal sets

**Evidence.** D:1303–1306 requires that the directory **not** carry a protected-inherit flag, claiming that flag could allow a later descriptor to widen child access. Microsoft's [security-descriptor control contract](https://learn.microsoft.com/en-us/windows/win32/secauthz/security-descriptor-control) says `SE_DACL_PROTECTED` prevents the descriptor's DACL from being modified by inheritable ACEs. Child propagation instead follows flags such as `OBJECT_INHERIT_ACE` and `CONTAINER_INHERIT_ACE`, as specified in [ACE Inheritance Rules](https://learn.microsoft.com/en-us/windows/win32/secauthz/ace-inheritance-rules).

D:1298–1302 permits the owner and administrative identities in the directory DACL, while D:1314–1316 and D:2032–2033 require the new file's DACL to grant the owner **only**. D:1303 simultaneously requires the directory entries to inherit. Its claimed repository precedent is also different: `SECURITY.md:39–45` describes a protected, one-entry **logon-session SID** DACL for the attention pipe, not an owner-plus-administrators directory policy.

**Consequence.** A securely protected directory is rejected for the property that prevents permissive parent ACEs entering it. Conversely, requiring unprotected inheritance does not establish the claimed isolation from later parent changes. Under the stated general inheritance rule, an allowed administrative entry can reach the file and then fail its owner-only read-back. The creation and validation contracts do not agree.

**Correction.** State the exact permitted principals and effective permissions for both directory levels and the file. Separate protection against incoming parent ACEs from propagation of vetted ACEs to children. Protect the boundary from broad temp-parent inheritance and explicitly specify the desired child-inheritance flags. Use the same principal policy when constructing and reading back the file descriptor, verifying it before writing picture bytes. Cite the pipe only for the applicable fail-closed precedent, preserving the distinction between user SID and logon SID. Add a protected-directory success case and a permissive-parent inheritance case beside the existing ACL fixtures.

### 5. Blocking — editing and history input can still redirect a completed picture insertion

**Evidence.** D:1529–1533 enumerates generation advances for submission, typed characters, text paste, K144, drops and other path insertions. D:1903–1909 tests that same list. It does not include cursor keys, history recall, deletion, completion or non-character editing commands. The current input encoder treats these as separate inputs: `crates/bt-app/src/input.rs:567–583` emits arrow/Home/End/Delete sequences; `:619–622` separately emits Enter, Backspace, Tab and Escape; `:540–562` emits Ctrl+C and other control-letter bytes.

**Consequence.** A pending picture job can survive an ArrowLeft that moves the cursor into an existing token, or a history-navigation key that replaces the current command, without any listed generation event. It then inserts at the changed cursor/line with the same leaf and incarnation. D:1536–1539 explicitly rejects editing whatever line happens to be current; the new generation contract still permits that case. This is a source-derived scenario, not an executed shell test.

**Correction.** Advance the generation at a target-specific boundary covering every user-originated PTY input that can change the input context, including navigation, editing, control keys and IME commits. Exclude unrelated terminal replies and local-only UI actions deliberately, rather than inferring line stability from printable characters. Keep checks per target. Add delayed-job fixtures for Left/Home, history recall, Backspace/Tab and Ctrl+C, with no insertion and owned-output cleanup, plus unchanged-target and other-pane controls.

### 6. Blocking — typed picture destinations still require a shell, and root-rim capture is missing

**Evidence.** D:1493–1497 says **every** job carries the captured recipient for the pre-write encoder check. D:1253–1255 requires that recipient's namespace and grammar before writing. But D:1501–1505 explicitly says Preview retarget and Split have no shell. D:1515–1520 defines `LayoutSplit { anchor_seat, side, layout_generation }` as the seat an edge belonged to; D:1151 also admits raw pictures on a **root rim**. In the existing routing, `crates/bt-app/src/main.rs:28787` represents that landing as `RootRim { edge }`, and `:28848` maps it to `LayoutAim::Rim(edge)`; it has no seat field. D:1911–1917 and D:2010–2016 test Preview and seat-edge completion, not the missing root-rim capture.

**Consequence.** The destination variants repair terminal-only incarnation checks, but a nonterminal job still cannot satisfy the mandatory common encoder preflight without inventing a shell recipient. A root-rim job must similarly invent a seat or be refused despite its admitted Split cell. This affects the shape T-PASTE-2b defines at D:2222–2224 and the landings T-PASTE-3 admits at D:2247–2255.

**Correction.** Put captured namespace/grammar and shell preflight in `TerminalInsertion`; specify native file-open/path validation for Preview and layout destinations. Keep security, size, cancellation and cleanup checks common. Make split anchors distinguish a seat edge from the captured tab root/rim, with explicit existence/layout-generation revalidation for each. Add delayed root-rim success and changed-root cancellation fixtures alongside Preview and edge cases. Do not choose the currently focused terminal as a surrogate recipient.

### 7. Should-fix — the six Cygwin outcomes lack candidate and cross-directory precedence rules

**Evidence.** D:818–829 admits that a sibling DLL is only an installation hint and checks both the executable's directory and `..\usr\bin\`. The only eligibility restriction at D:833 is an exemption for Folio-shipped Git Bash candidates; D:834 otherwise makes Cygwin-only evidence “at either level” a positive match. No rule restricts the remaining candidates to supported shells, despite R3 finding 6 requesting that restriction. A Cygwin-only executable directory plus an MSYS-only sibling, or one readable positive directory plus an unreadable sibling, does not fit an unambiguous precedence rule across D:834–838.

D:840–842, D:1844–1845 and D:2292 disable unprobed **classifier outcomes**. D:875–877 accepts the explicit `cygwin` override once its arm is added, without separately stating whether the PROBE 11 dark gate also disables that entry path; D:2190 calls the arm dark when the probe is unrun.

**Consequence.** A non-shell executable installed beside `cygwin1.dll` can get a namespace change under the written positive row. Combining two directories can also yield different classifications depending on which is examined first. The manual override's dark-state behavior is unspecified. None of these gaps requires revisiting the standing `(paths, integration)` ruling.

**Correction.** Define eligible resolved programs before checking DLLs; all ineligible programs keep today's answer. Define how evidence from both locations is aggregated, including mixed-runtime and partial-read cases, with conservative precedence. State the unrun-probe policy for the explicit override as well as automatic matching, consistent with the owner's dark gate. Add non-shell, cross-level mixed-DLL, partial-read and unrun-explicit-override fixtures. Keep all of this in T-PASTE-CYG; T-PASTE-1 remains independent.

### 8. Should-fix — Unix vetting still creates the old directory name

**Evidence.** D:1209–1217 selects `$TMPDIR/folio-<uid>/clipboard` through Folio's `runtime_directory()`. D:1230–1233 claims that name is now used everywhere. But D:1280 says `DirBuilder::mode(0o700)` for **Folio** and clipboard. D:2472 and D:2515 claim the vetting rename landed. The repository function confirms the intended parent: `crates/bt-platform/src/instance.rs:331–337` filters an empty TMPDIR, falls back to `/tmp`, and appends `folio-{uid}`.

**Consequence.** The creation/vetting recipe names a different Unix ancestor from the selected output path and privacy disclosure. The ledger's specific closure claim is false.

**Correction.** Replace the Unix ancestor in the vetting recipe with the actual `runtime_directory()` result, keeping Windows `Folio` separate. State that both the returned UID directory and its clipboard child receive the checks. Keep the existing fail-closed squatting limitation.

### 9. Should-fix — the two concrete encoder-preflight acceptance rows were not added

**Evidence.** D:1258–1259 says §6.2 carries a redirected-temp `%` row and an unmeasured-grammar row; D:2523 repeats that claim. The complete §6.2 is D:1926–2042. Its storage rows at D:2026–2037 cover permissions, reparse points and volume ACL support, and its Nu row at D:1969–1973 tests path refusal, not pre-write picture-file absence. D:1895–1901 contains only the generic terminal-job preflight requirement in §6.1.

**Consequence.** The body defines terminal preflight, but the ledger overstates its acceptance coverage. A generic no-write assertion does not exercise the specific otherwise-representable temp path and captured-recipient cases R3 finding 19 requested.

**Correction.** Add §6.2 picture-job rows with a valid redirected temp directory containing `%` and a cmd recipient, and with a valid directory and an unmeasured Nu recipient. Require the correct refusal and no output file created. Pair them with a recipient that can encode the same directory successfully, so a global directory refusal cannot satisfy the test accidentally. Name them in T-PASTE-2b's gates.

### 10. Should-fix — §7.2 assigns promise-only refusal to the wrong channel

**Evidence.** D:307–308 makes a promise-only clipboard `Refused(Promise)`. D:1687–1692 and D:1715–1720 keep payload refusal and acquisition/later failures distinct. D:2109–2111 includes “a promise-only payload” in the list, then D:2113–2115 says **these** use the lane error channel, “which is not the payload's Refused value.”

**Consequence.** An implementer following the interface and an implementer following the refusal catalog route the same promise outcome differently. This is precisely the consistency the five-state correction and D:2465's closure sentence claim to establish.

**Correction.** Split the catalog's routing sentence: promise-only clipboard refusal is the payload value; acquisition errors and later gate/decode/storage/delivery failures use their stated error paths. Keep all required toasts and silent Nothing unchanged.

### 11. Should-fix — the derivation test forbids the grammar derivation's input

**Evidence.** D:1831 says “The suite asserts that no derivation reads a program.” D:412–417 defines `derive_grammar(&ProgramSource)` from the launch program, D:1705 calls it `derive_integration`'s twin, and D:2153 explicitly includes it in T-PASTE-1. D:2509 repeats the overbroad suite claim as evidence of closure.

**Consequence.** The literal acceptance assertion prohibits an input another accepted part of the same ticket requires. The owner's ruling settles **namespace** derivation, not grammar derivation; broadening it here creates an unnecessary contradiction.

**Correction.** Scope the assertion to the namespace resolver used by T-PASTE-1. Test separately that grammar uses its specified program/override rules, namespace keeps the standing pair, and spelling overrides affect insertion only. Update D:2509 to that same scope; do not add a rejected program-keyed namespace design.

### 12. Should-fix — earlier ledgers still present superseded instructions as current resolutions

**Evidence.** §10.1 introduces its rows with “accepted means the design changed, and the section says where” at D:2419–2420. Its row 11 at D:2434 still says ruling ④ adds before/after `GetClipboardSequenceNumber`; row 19 at D:2442 still says “first-frame policy stated” and PROBE 8 gives acquisition “a measured latency contract”; row 20 at D:2443 still gives `std::env::temp_dir()` as discovery without the Unix policy split; row 22 at D:2445 still makes child access part of PROBE 9. These conflict with D:235–248, D:1375–1406, D:1475–1483, D:1209–1217 and D:2294–2303 respectively.

Several §10.2 cells append their correction after unchanged present-tense instructions: D:2464 still says no timeout and that OpenClipboard requires a calling-thread window; D:2470 still permits age-stealing and quota bypass; D:2474 still permits an unprobed width-one Nu arm. Their appended closure sentences establish the later decision, but the old instructions are not consistently marked historical. D:2506 and D:2515 claim the conflicting text was removed everywhere; D:2529 claims all seven blockers are resolved in the body.

**Consequence.** Readers must infer which parts of an accepted row are obsolete. The explicit historical owner-ruling mention is allowed and is not an open decision, but the other old instructions are still presented as resolutions and contradict the claimed in-place cleanup.

**Correction.** Mark the earlier portions explicitly as superseded historical records and point to the current ruling, or update them in place. In particular, withdraw the old sequence check, first-frame policy, latency bound, Unix std discovery and child-read probe instructions in §10.1. Keep the dated owner decision closed. Recalculate the ledger closure claims after the body and fixture corrections above land.
