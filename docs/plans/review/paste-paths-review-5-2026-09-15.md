STATUS: READY TO OPEN T-PASTE-1 — COMPLETE, phase 4 verified; §10.4: 8 closed, 4 partially closed; one blocker for T-PASTE-2b / terminal picture drops, five nonblocking documentation findings; LF and file scope verified.

# R-PASTE-PATHS-5 — fifth-pass review, 2026-09-15

Subject: `docs/plans/paste-paths-design.md` at `c5d6721e569d16dc15b91188bcbf125b28f22fb6` (3,159 lines), branch `docs/paste-paths-design`. `D:n` denotes a line in that design; `R4` denotes `docs/plans/review/paste-paths-review-4-2026-09-15.md`.

Evidence and phase history: `target/review5-notes.md`. Review only; no cargo, builds, tests, application launches, code edits or design edits.

## Phase 1 — reading and scope

Read D:1–3159 and all of R4, including its twelve findings and both verification tables. HEAD and branch match the requested subject. Initial `git status --short --untracked-files=all` was empty. The standing namespace derivation, insertion-only spelling override, Windows-only rule E, and the Cygwin dark gate for automatic classification and explicit override are treated as settled design choices.

## Phase 2 — §10.4 verification

Statuses cover the correction, its operative text and its fixtures. An accepted decision in the ledger alone does not close a row. None is not closed or regressed.

| R4 finding / ledger line | Status | Evidence |
| --- | --- | --- |
| 1 / D:3139 | **partially closed** | D:981–1049 specifies the thirty cells through rules A–E and C1–C4, including WSL context, host fallback, named-cmd policy and Windows-only load-time rejection. D:662–672 and D:1952–1955 select Agent quoting from the emitted string; D:2203–2207 tests translated versus fallback inputs. Remaining issues are the unconditional PowerShell fixture and rule A's overstatement of encoder independence (findings 2–3). |
| 2 / D:3140 | **closed** | D:254–265, D:2254–2279 and D:2389–2406 all require delayed-render success, competing-open failure during the held interval, Folio snapshot completion, and the next gesture reading the replacement made after close. macOS retains change-count refusal. These agree with Microsoft's [OpenClipboard contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard) and [delayed-rendering contract](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-operations#delayed-rendering). |
| 3 / D:3141 | **partially closed** | D:477–483 and D:2423–2435 separate direct CRT consumption from cmd-profile refusal, including paired percent signs. No cmd interpreter remains in the encoder row. The harness still needs an explicit program-name token so the tested literal occupies argv[1] (finding 4). |
| 4 / D:3142 | **closed** | D:1505–1551 requires TOKEN_USER SID + FILE_ALL_ACCESS, exactly one allow ACE, protected OI/CI descriptors at both directories, and an explicit protected file descriptor read back before bytes are written. Owner, repair, reparse and ACL-capability checks are stated. The three new fixtures are protected-directory success (D:2519–2523), permissive-parent isolation (D:2524–2529), and logon-SID refusal (D:2536–2538). D:2540–2542 repeats file read-back. Microsoft's [descriptor-control definition](https://learn.microsoft.com/en-us/windows/win32/secauthz/security-descriptor-control) and [ACE inheritance rules](https://learn.microsoft.com/en-us/windows/win32/secauthz/ace-inheritance-rules) support the distinction between incoming protection and child propagation. `SECURITY.md:39–47` confirms the separate logon-session pipe policy. |
| 5 / D:3143 | **partially closed** | D:1830–1853 and D:2313–2340 add navigation, history, editing, control and IME inputs, with unchanged-generation, reply and other-pane controls. `input.rs:530–632` and the four `note_user_typing` call sites match the citations. The inherited exclusion of all wheel forwarding misses the existing wheel-to-arrow route, which sends those same navigation bytes without that call (finding 1). |
| 6 / D:3144 | **closed** | D:1431–1451 and D:1733–1785 put recipient/namespace/grammar only in TerminalInsertion; PreviewRetarget and LayoutSplit use native path validation. D:1796–1805 defines SeatEdge and TabRoot with captured-tab/layout checks. This matches `main.rs:28780–28791` and `:28845–28850`. D:2342–2363 includes root-rim success, changed-root cancellation, no surrogate seat, nonterminal success with cmd/% elsewhere, and native overlength refusal. T-PASTE-3 names the anchors at D:2807–2811. |
| 7 / D:3145 | **closed** | D:874–915 gives the eligible stem set before disk access, shipped-wrapper exemption, union aggregation, absent-versus-unreadable precedence and both dark entry paths. D:2216–2235 and D:2456–2469 test ineligible programs, cross-level mixtures, partial reads, absent siblings and unrun explicit override. D:2735–2743 keeps it separate; PROBE 11 at D:2849 agrees. The stem extraction matches `profiles.rs:826–829`. |
| 8 / D:3146 | **closed** | D:1387–1406 and D:1472–1487 consistently select and vet runtime_directory()/folio-<uid> plus clipboard; D:2511–2514 tests both levels. `instance.rs:331–337` confirms unset/empty TMPDIR fallback and effective UID; `:360–388` confirms the cited creation, owner, link and mode-repair recipe. D:3096 corrects the historical claim. |
| 9 / D:3147 | **closed** | D:2554–2563 has all four concrete rows: redirected percent-bearing temp with cmd refusal/pwsh success, and valid temp with unmeasured Nu refusal/bash success. Refusals explicitly require no output file. D:1431–1439 scopes the preflight per recipient; D:2780–2784 names the four gates. |
| 10 / D:3148 | **closed** | D:2646–2660 now routes promise-only clipboard to Refused(Promise), acquisition failure to Unreadable/error reporting, and later failures to the app's error path. This agrees with D:104–130, D:318–319 and D:2024–2029. The phrase “picture lane's own error channel” is broader than the picture feature, since the listed representability/encoder failures also apply to ordinary paths; see finding 6. |
| 11 / D:3149 | **closed** | D:2173–2187 replaces the prohibited-program blanket assertion with namespace/grammar/insertion-override assertions; D:2043–2047 and D:3090 agree. `profiles.rs:799–821` confirms the structural grammar precedent; `:3205–3219` confirms selection by the standing pair. “Nothing else” needs qualification for existing namespace payload data, not a new derivation (finding 6). |
| 12 / D:3150 | **partially closed** | D:2988–2994 and D:3036–3041 explicitly mark earlier records historical. D:3008, D:3016–3019, D:3045, D:3051 and D:3055 retire the requested obsolete clauses. D:3087, D:3090, D:3096 and D:3104 repair the four overclaims. §10.3 still has an unmarked obsolete CRT row, and D:3111–3122 misidentifies the fourth review's blockers (finding 5). |

## Phase 3 — fresh review

### Thirty-cell audit

The following is the decision table obtained from D:493–502, D:519–527, D:537–572 and D:981–1049. It includes the row-wide rules even where the design abbreviates a cell. W means a Windows-recognised emitted string; P means a POSIX emitted string. A successful translation to WSL/MSYS/Cygwin gives P; an untranslated UNC or foreign-distribution share keeps the host string and selects W. C1–C4 retain the design's meanings. “CYG” requires the arm and PROBE 11; before that, the explicit value is refused under D:953–957. All five columns reject the key once at load on Unix under rule E, retaining host spelling; the grammar override remains independently available.

| Grammar | windows | windows-slash | wsl | msys | cygwin |
| --- | --- | --- | --- | --- | --- |
| PowerShell | Single quotes; C1 | Single quotes; C1 | Single quotes around P or fallback W; C1 | Same; C1 | CYG, then same; C1 |
| Cmd | Double quotes, 2N trailing backslashes; C2/C3 | Same encoder on slash string; C2/C3 | Same encoder on P or fallback W; C2/C3 | Same; C2/C3 | CYG, then same; C2/C3 |
| Posix | Single quotes with apostrophe splice | Same | Same on P or fallback W | Same | CYG, then same |
| Fish | Single quotes; escape every backslash and apostrophe | Same encoder on slash string | Same on P or fallback W | Same | CYG, then same |
| Nushell | C4 until baseline; then measured raw fence | Same | Same on P or fallback W | Same | CYG and C4; then measured raw fence |
| Agent | W double quotes; C2; no CRT backslash doubling | W for drive-slash output; C2 | P single quotes/apostrophe splice; fallback W double quotes/C2 | Same branch selection | CYG, then same branch selection |

**Decidability.** All thirty cells have a specified operational answer when the configured grammar policy and probe state are retained. C2 applies across the Cmd row even where a cell omits its label. C3 depends on named-versus-default Cmd and the row's delayed-expansion args, not on the path alone. No WSL home field is needed; distribution input is concretely supplied by `profiles.rs:3237–3253`, including None. The unconditional PowerShell example is inconsistent with the unrun state; rule A must describe a configured encoder rather than erase its policy inputs (findings 2–3). These are local corrections; they require no new grammar/spelling ruling.

**Agent consistency.** D:500–501, D:662–672 and D:1952–1955 agree on emitted-string selection. D:2202–2207 covers a Windows root, untranslated UNC, and a distribution share whose translation changes the branch. Drive-slash output remains Windows-recognised. The quoted branch order agrees with the [pinned Codex source, lines 251–286 and 333–360](https://github.com/openai/codex/blob/a8964cb1bad67bc26a826fb07d1bef99c6a3f008/codex-rs/tui/src/clipboard_paste.rs#L251). That source also has a Linux/WSL conversion inside the Windows recogniser; the design's choice of quoting branch does not assert that downstream conversion is an identity. Multi-path attachment stays explicitly unguaranteed at D:692–711. No agent execution was performed.

### Body, fixtures, fallbacks and ticket scopes

The fresh pass covers D:1–3159. Sections 1, 2, 5 and the T-PASTE-1 parts of 6.1 specify the clipboard states, acquisition semantics, representability, spelling, grammar, derivation, K144 integration and paste envelope (D:104–337, D:397–1131, D:2023–2119, D:2132–2279 and D:2365–2370). The T-PASTE-1 scope at D:2694–2725 assigns those pieces and explicitly excludes picture acquisition/delivery, drops and Cygwin. No storage or asynchronous-destination decision from §4 is needed to implement that scope. Its shell and agent acceptance measurements remain shipping gates rather than prerequisites to opening the ticket (D:2711–2719).

PROBE 1 remains mandatory before shipping T-PASTE-1 (D:2839), even though its measured formats do not alter ladder precedence. PROBEs 2–4 agree with the body and T-PASTE-1 gates on quote refusal, recorded Agent inheritance and demonstrated windows-slash fallback (D:519–527, D:712–725, D:959–968, D:2711–2717, D:2840–2842); finding 2 identifies the fixture exception. PROBE 10 retains whole-arm refusal with no baseline (D:609–622, D:2446–2450, D:2848). PROBEs 5–9 agree on Windows-only drops if no acceptable Mac adapter, actual offered-type admission, unsupported-layout refusal, on-loop acquisition without a Folio deadline, and macOS write-denial refusal (D:1352–1359, D:1593–1605, D:1715–1723, D:2673–2676, D:2815–2819, D:2843–2847). PROBE 11 agrees with both automatic and explicit dark gates (D:910–929, D:2735–2743, D:2849).

T-PASTE-2a carries acquisition/decode/APNG and the missing bmp feature, with layout work assigned to PROBE 7 (D:2749–2765); no new prerequisite was found for that scope. T-PASTE-2b carries storage, preflight, generation, switch and disclosure work (D:2767–2787); finding 1 blocks its deferred terminal delivery. T-PASTE-3 uses that lane for terminal picture drops and therefore inherits finding 1, while its Preview/SeatEdge/TabRoot destination shapes and pure fixtures are present (D:2342–2363, D:2794–2811). T-PASTE-CYG has its own eligibility, union and dark-state tests (D:2216–2235, D:2735–2743); no additional Cygwin blocker was found. The seven current classifier table rows at D:896–902 supersede §8's obsolete count of six (finding 6).

## Phase 4 — artifact verification

`git status --short --untracked-files=all` reports only the new review; `git diff --name-only` and `git diff --check` are empty. `target/review5-notes.md` exists and is ignored. The design's working-file hash equals its HEAD blob (`a14579297832f5fa8730c10d10e0cfb7a07d4bb0`); R4 likewise equals HEAD (`881fab9be9bf7a3a848bb0b832b2a3ae85c1f397`). Byte checks find zero CR bytes, no UTF-8 BOM and no trailing whitespace in either output. The review contains twelve ledger assessments, six product rows covering five spellings each, and six numbered findings. Only the requested review and retained notes were written. No cargo, tests, builds or native probes were run.

## Verdict

**Ready to open T-PASTE-1.** Findings 2–6 are nonblocking documentation corrections: the controlling rules already supply the behavior, and the corrections need no new owner decision. They should be carried into the ticket's specification and acceptance instructions. This is permission to begin implementation, not evidence that its native shipping gates have passed.

**Fix finding 1 before T-PASTE-2b or terminal raw-picture delivery in T-PASTE-3.** No section needs a redesign. T-PASTE-2a and T-PASTE-CYG retain the gates already stated in the design.

## Numbered findings — later-ticket blocker

### 1. Blocking for T-PASTE-2b / terminal picture drops — wheel-generated navigation bypasses the generation

**Evidence.** D:1830–1853 requires generation advancement for every user-originated PTY byte but implements it exactly at `note_user_typing`; D:1846–1865 deliberately inherits the existing wheel/mouse exclusions. D:2325–2331 requires keyboard history/editing input to invalidate a pending job. In the repository, `main.rs:95117–95136` handles `WheelRoute::ArrowKeys` by calling `input::alternate_scroll_bytes` and `send_mouse_input_to`, with no `note_user_typing`. `input.rs:456–468` constructs those bytes by calling the very same `keyboard_bytes` for ArrowUp/ArrowDown. `main.rs:110813–110823` distinguishes this alternate-screen route from mouse reports. Thus it is neither a local-only scroll nor merely a mouse-protocol report.

**Consequence.** A terminal recipient using an alternate-screen input editor can receive navigation from the wheel during an encode. Keyboard Up invalidates the job; the same Up bytes from the wheel leave the generation unchanged. The job can then insert into the changed input context. This is a source-derived counterexample, not an executed native fixture. The existing `note_user_typing` function serves command-history provenance (`main.rs:85377–85384`); its narrower definition is not proof that excluded inputs cannot affect a delayed insertion.

**Correction.** Give picture invalidation its own per-target boundary that also advances on user-generated arrow emulation. Keep local scrolling, terminal replies, replay and repair bookkeeping excluded. For forwarded mouse reports, either invalidate conservatively too or state a narrower supported-recipient contract; D:1860–1861's assertion that a mouse-tracking program cannot be at an input prompt is not established by the cited enum. Add a delayed-job fixture for wheel-to-Up/Down in the same target, requiring cancellation and owned-output deletion, with local-wheel and other-pane controls. Update the mouse-report negative fixture if its policy changes. This does not require changing the existing command-history meaning of `note_user_typing`.

## Numbered findings — nonblocking nits and acceptance corrections

### 2. Should-fix — the new PowerShell product fixture assumes PROBE 2 succeeded

**Evidence.** D:519–527 and C1 at D:1005–1007 refuse U+0027 in an unprobed build. D:1051–1053 and D:2197–2200 nevertheless require `D:\John's Archive\a.txt` × wsl × PowerShell to emit `'/mnt/d/John''s Archive/a.txt'` without a measured-state qualification. D:2711–2719 simultaneously permits the unrun refusal fallback and requires the product fixtures green.

**Correction.** Split the fixture by probe policy: unrun means C1 refusal with no insertion; after the recorded result enables U+0027 doubling, assert the displayed literal. State that pure fixture setup selects the policy explicitly. Apply the same distinction to §6.2's shell matrix (D:2408–2411): a refused filename is a refusal observation, not a line to run/open. The existing C1 rule supplies the answer; the fixture should express it.

### 3. Should-fix — rule A describes a path-only encoder but C3 requires captured policy

**Evidence.** D:981–986 says the grammar sees one string and nothing else, including no pane. D:556–572 and D:1013–1015 require different outcomes for identical emitted strings under named Cmd versus unknown-program Cmd, and for `!` with versus without delayed expansion in the row args. D:1761–1766 describes the captured recipient only as namespace and grammar. Bare `ShellGrammar::Cmd` plus a string does not contain those distinctions.

**Correction.** Say that spelling produces the string consumed by an encoder configured at the gesture/spawn boundary. Preserve named-versus-default Cmd and the delayed-expansion flag in that policy; preserve applicable probe policy and effective insertion spelling/context in the captured recipient. The pure encoder need not query the live pane or original host path. Add paired `%` and `!` fixtures for the two Cmd origins. This makes the existing decisions implementable without contradicting rule A; it does not change any table cell or the standing namespace derivation.

### 4. Should-fix — the direct CRT fixture must put the literal after argv[0]

**Evidence.** D:2423–2432 specifies the printer as the image and the encoded line as the command line, but gives no program-name token or tested argument index. If the encoded path alone is lpCommandLine, it occupies argv[0]. Microsoft documents that argv[0] has special parsing and the subsequent backslash rules do not apply to it; CreateProcessW does not prepend the separate lpApplicationName as an argument for this harness. [CRT parsing rules](https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments?view=msvc-170), [CreateProcessW parameters](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw).

**Correction.** Specify a defined MSVC CRT `wmain` consumer, lpApplicationName pointing to it, and a mutable lpCommandLine containing a correctly quoted printer-name token, a space, then the exact tested literal. Assert argc == 2 and argv[1] equals the intended UTF-16 path. Retain the trailing-backslash and lone/paired-percent cases, and the separate cmd-profile refusal row. This closes the harness ambiguity without restoring an interpreter or REPL.

### 5. Should-fix — §10.3 is still partly current-tense history, and its blocker attribution is wrong

**Evidence.** D:3093 still calls the CRT replacement “an argv printer launched from a cmd row,” contradicting D:2423–2435. Unlike §10.1 and §10.2 (D:2988–2994, D:3036–3041), §10.3's introduction at D:3079–3082 does not mark its unamended answers as historical. D:3102 similarly retains the older seat-only LayoutSplit description. D:3113–3119 correctly repeats R4's eight-closed/twelve-partial total, but its list of four blockers includes the concurrent-copy fixture (R4 finding 2, should-fix), omits the grammar/spelling composition blocker (R4 finding 1), and includes preflight fixture row 9 in the blocker mapping. R4's numbered blocking findings are exactly 1, 4, 5 and 6.

**Correction.** Give §10.3 the same historical/supersession convention and explicitly point row 8 to §10.4 row 3 and row 17 to §10.4 row 6. Distinguish R3 row statuses from R4 finding severities; list R4 blockers 1/4/5/6 when describing its four blockers. Retain the verified historical 8/12 and 6/5 counts from R4, but do not use them as a current closure certification: this review's §10.4 assessment is 8 closed/4 partial. The owner-ruling reference in D:3047 remains an explicitly historical, subsequently closed question.

### 6. Nit — narrow three residual scope/count sentences

**Evidence and corrections.**

- D:2180–2182 says the namespace resolver reads the standing pair “and nothing else.” `profiles.rs:3205–3217` selects its variant by that pair but obtains WSL distribution and MSYS home payloads separately. Say that **variant selection** uses the pair, retaining its existing context population; no derivation change is requested.
- D:2651–2656 assigns ordinary path representability/encoder and over-count-drop failures to the “picture lane's own error channel,” although T-PASTE-1 ships path refusals before pictures (D:2705–2725). Name the shared app-level refusal reporting path, with picture failures as callers, so §7.2 does not imply a dependency on a picture worker.
- D:2732 says the Cygwin classifier has six outcomes; D:896–902 has seven rows after adding eligibility. Replace the count with seven or refer to the table without a count. D:2218–2219 already lists all seven, so this requires no new classifier decision.
