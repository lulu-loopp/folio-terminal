STATUS: COMPLETE — phase 4 complete; 23 ledger rows verified (12 closed, 11 partially closed); 20 findings (7 blocking, 13 should-fix); verdict: fix these first; LF and file scope verified.

# R-PASTE-PATHS-3 — third-pass review, 2026-09-15

Reviewed `docs/plans/paste-paths-design.md` at `6ab4942dd8550018a3fbb4c0f424accf013f4ae3`, branch `docs/paste-paths-design`. `D:n` means that design's line n. `R2:n` means line n of `docs/plans/review/paste-paths-review-2-2026-09-15.md`. Unqualified Rust filenames below are in `crates/bt-app/src/`; platform files are explicitly qualified. `registry/` means `C:/Users/Weiyi/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.

Method: read the complete 1,831-line design, both ledgers, the earlier reviews, the standing namespace ruling, relevant implementation and dependency source, and primary upstream sources. Source deductions below are not executed clipboard, shell, agent, drag or storage probes. No builds, tests, app launches, code changes or design edits. Working evidence remains in `target/review3-notes.md`.

## Second-ledger verification

Statuses concern the complete correction, including conflicting instructions elsewhere. A closed row does not certify unrelated behavior in the same section. Finding numbers refer to the numbered findings below.

| R2 finding / ledger line | Status | Body evidence and remaining issue |
| --- | --- | --- |
| 1 / D:1802 | **partially closed** | D:184–230 specifies survey then fetch, the event-loop thread, `ClipboardRead`, and a latency-distribution probe. D:1285–1286 and D:1589 carry the worker/station allocation. But D:1125–1129 retains the old measured timeout, and the snapshot and exactly-one-rung rules have independent defects: findings 1–3. |
| 2 / D:1803 | **partially closed** | D:102–116 adds `Refused(UnsupportedKind)` and silent `Nothing`; D:244–253, D:931 and D:934–940 register promises solely for visible refusal, never delivery. The fifth state is absent from D:1269's crate interface, and D:1604–1605 still gives `Absent` a message: finding 4. |
| 3 / D:1804 | **partially closed** | D:608–645 cites the dated owner ruling and presents both sides; D:659–669 adds Cygwin and excludes MinGW as a separate namespace. D:626–628's unchanged derivation conflicts with D:1279 and D:1580–1587. The DLL rule and PROBE 11 fallback remain incomplete: findings 5–6. |
| 4 / D:1805 | **partially closed** | D:401–402 and D:503–538 contain the platform split. The pinned parser supports the POSIX apostrophe example and Windows double quotes; R2's POSIX objection is withdrawn on that evidence. D:1205, D:1368–1369 and first-ledger row 9 (D:1770) still specify incompatible behavior: finding 7. |
| 5 / D:1806 | **partially closed** | D:371–384, D:397 and D:403 separate CRT encoding from `cmd.exe` expansion refusals, and D:1455–1457 adds the requested non-cmd row. But the claimed Python/Node REPL correctness confuses startup argv with terminal input: finding 8. |
| 6 / D:1807 | **closed** | D:169–182 fetches an ordered picture-encoding list in one transaction, bounds the complete list, and makes exhausted/truncated fallback terminal. D:160–164 confines `Unreadable` to acquisition; D:1402–1404 tests decode fallback above that layer. |
| 7 / D:1808 | **partially closed** | D:1103–1119 adds 24 hours, `.lock`, stale recovery and bypass-on-contention; D:1171–1176 includes unsubmitted lines. The bypass defeats serialization and the hard quota, while elapsed time alone cannot identify a dead lock owner: finding 10. |
| 8 / D:1809 | **closed** | D:1336–1343 puts Paste picture only in the terminal context menu, plus a bindable command. D:1504–1517 permits the type-list read there and disables on read failure. D:1245–1247 disables the row when saving is off. No menu-bar row remains specified. |
| 9 / D:1810 | **partially closed** | D:974–985 names `folio-<uid>/clipboard`, including an explicit `/tmp` fallback. D:1013 and D:1214–1215 retain `Folio`; the claimed equivalence to `std::env::temp_dir()` is false on macOS: finding 11. |
| 10 / D:1811 | **partially closed** | D:1591–1594 and D:1784 keep per-ticket i18n. D:555–565 and D:1598–1603 allow recorded Agent inheritance. D:1653–1655 still requires unmeasured recipients to refuse: finding 15. |
| 11 / D:1812 | **partially closed** | D:470–490 defines fence construction and PROBE 10; D:1717–1723 cites the book's growth rule. No supported version is pinned, and the width-one fallback assumes the unmeasured argument-position answer: finding 14. |
| 12 / D:1813 | **closed** | D:806–819 names winit's additional COM initializer and teardown ordering; D:1477–1479 adds fullscreen coverage. Verified against `registry/winit-0.30.13/src/platform/windows.rs:491–497` and `src/platform_impl/windows/window.rs:1432–1444`. |
| 13 / D:1814 | **partially closed** | D:927–932 supplies every requested landing and uses verb names; D:1475 adds Placeholder. The file-URL/Preview cell fails to distinguish a folder, contradicting D:852: finding 13. |
| 14 / D:1815 | **closed** | D:1439–1448 explicitly includes a WSLg file, `clip.exe` text, and RDP file/image, with bridge versions. D:1665 repeats the inclusion. |
| 15 / D:1816 | **closed** | D:991–999 runs the representability gate on the directory before writing and deletes a completed unrepresentable output; D:1422–1423 tests it. A separate later encoder-refusal case is finding 19. |
| 16 / D:1817 | **closed** | D:1178–1184 puts the sweep on the picture worker, shared with writes, and names the station for any loop work. D:1285–1286 carries both in the crate map. Cross-process exclusion remains finding 10, not a missing worker assignment. |
| 17 / D:1818 | **closed** | D:1646–1651 specifies Windows-only shipping if all macOS shapes fail, with bilingual README/features disclosure. D:1669 gives the same answer when PROBE 5 is unrun. |
| 18 / D:1819 | **partially closed** | D:420–428 gives the five-code-point PowerShell fallback; D:1665 correctly makes PROBE 1 a shipping fixture requirement. D:1673 and D:1677–1682 narrow PROBE 9, but D:1565–1568 and D:1783 retain its former child-read question; the unrun outcome needs an operational definition: finding 15. |
| 19 / D:1820 | **closed** | D:26–30 points to §9.1, counts eleven probes, and identifies the owner ruling. D:1665–1675 has eleven numbered rows. |
| 20 / D:1821 | **closed** | D:579–591 now accurately describes CR/CRLF, LF, retained TAB, non-controls and dropped controls. Compared with `input.rs:718–728`; this is an accurate paraphrase of the five arms. |
| 21 / D:1822 | **closed** | D:745–749 names DragEnter, DragOver and Drop. `registry/winit-0.30.13/src/platform_impl/windows/drop_handler.rs:83`, `:109`, `:137` each takes `_pt`. |
| 22 / D:1823 | **closed** | D:1785 says the sentence carrying the wrong usershell anchor was removed, rather than claiming the body contains a corrected anchor. |
| 23 / D:1824 | **closed** | D:969–973 withdraws the named Windows std implementation API. The remaining per-account assumption and macOS discovery error are addressed in finding 11. |

## Eleven-probe fallback audit

| Probe | Unrun behavior checked against the body |
| --- | --- |
| 1 | D:1665 blocks shipping until the source matrix exists; D:1439–1448 defines that matrix. It is a release fixture requirement. |
| 2 | D:1666 points to the concrete refusal set at D:422–428. This fallback can be implemented without discovering an unknown set. |
| 3 | D:1667 records inheritance, but D:1655 refuses unmeasured recipients: finding 15. Source inspection of Codex does not replace §6.2's actual recipient acceptance. |
| 4 | D:1668 requires a spelling that the specified override cannot request: finding 9. |
| 5 | D:1669 and D:1646–1649 give Windows-only drops and bilingual disclosure. |
| 6 | D:1670 applies the admitted-type matrix, including promise refusal; D:934–949 states the corresponding registration and delivery policy. |
| 7 | D:1671 refuses unverified layouts; D:1066–1072 requires supported-layout fixtures. T-PASTE-2a must enumerate those layouts (D:1611–1614); no captured layout was validated in this review. |
| 8 | D:1672 keeps acquisition on-loop with a station; D:1125–1129 still contradicts it, and the Win32 facts need correction: finding 2. |
| 9 | D:1673 does not define what is disabled when no TCC measurement exists; old probe scope also survives: finding 15. |
| 10 | D:1674 limits width but assumes argument-position acceptance: finding 14. |
| 11 | D:1675 specifies only the neither-DLL case; positive, ambiguous and inaccessible evidence is unspecified: finding 6. |

## Verdict

**Fix these first. Do not open T-PASTE-1 yet.** The owner's §2.5 ruling remains required; it cannot resolve the clipboard transaction, recipient-contract and contradictory-fallback findings below by itself.

## Numbered findings

### 1. Blocking — the sequence-number check rejects legitimate delayed rendering

**Evidence.** D:198–204 discards the read whenever `GetClipboardSequenceNumber` changes between survey and fetch. D:1399–1400 makes this a test expectation. Microsoft's [GetClipboardSequenceNumber contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber) states: “If clipboard rendering is delayed, the sequence number is not incremented until the changes are rendered.” [Clipboard Operations](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-operations#delayed-rendering) requires the owner to fulfill `WM_RENDERFORMAT` without opening the clipboard, because the requesting application holds it open.

**Consequence.** A successful first fetch of delayed data can change the sequence number without introducing another user's copy. The design classifies that success as a mixed snapshot and refuses the paste. The before/after test cannot distinguish the event it fears from the rendering it requested. This affects text and files in T-PASTE-1, not only pictures.

**Correction.** Specify coherence separately for Win32 and AppKit. On Windows, retain the single open interval and account for legitimate rendering changes; do not use sequence equality across `GetClipboardData` as the acceptance predicate. Keep macOS's change-count rule separately justified. Add a delayed-render success fixture whose first paste succeeds, alongside a real replacement case. Do not repair this by silently reopening and taking a different clipboard.

### 2. Should-fix — the latency contract still has two answers, and the new Win32 explanation overstates both timeout and worker requirements

**Evidence.** D:209–223 specifies a loop stall, `ClipboardRead`, and a distribution probe. D:1125–1129 still says “the read is bounded” and asks PROBE 8 for that bound; D:1780 retains a measured latency contract. D:1802 says the word “bounded” is gone. Microsoft's [delayed-render explanation](https://devblogs.microsoft.com/oldnewthing/20220608-00/?p=106727) describes a system wait of up to 30 seconds followed by `NULL` for an owner that does not render in time. This contradicts D:209–214's categorical “There is no timeout”. It does not establish a short application-controlled deadline for the entire transaction.

D:224–230 and D:1694–1696 also present a worker-owned message-pumping window as a Win32 requirement for reading. [OpenClipboard](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard) accepts a null HWND; clipboard ownership changes through `EmptyClipboard`. `crates/bt-platform/src/lib.rs:5832–5840` explains the window requirement in terms of `EmptyClipboard`/`SetClipboardData`, while `:5903–5919` shows Folio's current helper restriction.

**Correction.** Remove the old measured-bound requirement everywhere. State that Folio imposes no responsive deadline or cancellation, acknowledge the documented system timeout for the described rendering path, and retain the hang station. Describe an owner window as one possible worker design or a cost of reusing Folio's current helper, not an unavoidable read-only Win32 requirement. No native timing probe was run here.

### 3. Should-fix — surveying types cannot establish which rung answers, so exactly one fetched rung contradicts empty-file fallthrough

**Evidence.** D:155 defines an advertised zero-file `HDROP` as `Absent` and requires falling through; D:180–182 repeats the empty-file rule. D:184–204 says survey first and fetch exactly one rung, allowing multiple fetches only within the picture rung. D:1397–1402 tests both promises.

**Consequence.** For an advertised zero-file `CF_HDROP` plus readable text, the type list cannot reveal the file count. Folio must fetch the file rung to learn that it is empty and then fetch text to satisfy the ladder. Those are two different rungs. On macOS, an advertised URL representation can similarly produce no admitted local file URLs after filtering.

**Correction.** Define survey as candidate selection. Within the same transaction, fetch candidates in priority order until `Present` or `Unreadable`; permit another rung only after acquired content proves `Absent`. Describe the normal case as one fetched rung, and explicitly test empty-file-plus-text. Alternatively change the empty-file policy, but do not leave both promises in force.

### 4. Should-fix — the refusal state and T-PASTE-1's empty-result behavior still disagree across sections

**Evidence.** D:102–116 declares `Refused(UnsupportedKind)` and silent `Nothing`. D:1269's implementation map omits `Refused` and still describes the earlier payload shape. D:1583–1585 makes the unimplemented picture rung `Absent`, while D:1604–1605 says such a paste reports “nothing to paste”. An absent picture rung and no other answer produces `Nothing`, which D:116 says stays silent. D:160–165 and D:1553–1556 separately require acquisition failures and other refusals to report, so “only it routes to a toast” at D:115 also needs its scope stated.

**Correction.** Carry the same five-state payload and explicit error channel into §5 and T-PASTE-1. Choose silent `Nothing` for the unimplemented picture lane, or a named refusal such as `PictureNotEnabledYet`; do not request a message from indistinguishable `Absent` states. Clarify that only `Refused` among the empty/unsupported payload outcomes generates a refusal toast, while acquisition/encoding errors have their own reporting path. Keep promise registration and the no-delivery rule as written.

### 5. Blocking — the owner is offered a choice that §5 and T-PASTE-1 still preempt

**Evidence.** D:626–628 says T-PASTE-1 calls `printed_path_namespace` unchanged and does not repoint the detector. D:1279 still allocates `derive_namespace()` as “one derivation, both directions”; D:1580 calls the ticket “a two-direction namespace change”, and D:1586–1587 includes `derive_namespace` plus a new Cygwin arm while saying the old function is used as it stands. `profiles.rs:3205–3219` currently matches only the profile's namespace/integration pair; `crates/bt-transcript/src/paths.rs:61–109` contains no Cygwin variant.

**Owner-ruling assessment.** D:611–624 accurately cites `docs/plans/shell-matrix-2026-09-07.md:365–372`. The pro-change example is supported by `profiles.rs:3219`. The concern about changing detection outside paste is supported by the function's detector contract at `profiles.rs:3181–3199`. D:647–654 fairly corrects R2's assertion that Cygwin necessarily worked under the standing rule: automatic integration for a `bash.exe` is `BashInitFile` (`profiles.rs:826–836`), which selects `Msys`; explicit integration `None` selects `Windows`. Both configurations should be named, rather than treating every Cygwin row alike.

The two motivations are stated, but the proposed alternative has no complete input-to-namespace table. Calling the pair a “definition” at D:640–645 establishes the standing decision, not that every program using that integration actually has MSYS mount semantics.

**Correction.** Give the owner two concrete scopes: preserve the pair for both directions, or adopt a specified resolution table with an explicit detector migration. Put Cygwin in that decision as an explicit exception or separate change. State how WSL, integration-off Git Bash, actual MSYS executables, wrappers, Cygwin with integration on/off and unresolved programs map in each option. Make §5, §8, tests and both ledgers conditional on that same decision. Do not request a ruling on a prose motivation while retaining the alternative as implementation scope.

### 6. Should-fix — Cygwin's sibling-DLL classifier and `/cygdrive` mapping are not a complete namespace contract

**Evidence.** D:659–669 uses `cygwin1.dll` versus `msys-2.0.dll` beside the resolved program. Folio's shipped Git Bash candidates are `<Git>\bin\bash.exe` (`profiles.rs:1119–1137`), identified there as a wrapper. Git for Windows' [portable packaging source](https://github.com/git-for-windows/build-extra/blob/main/portable/release.sh#L100-L103) installs `compat-bash.exe` as that `bin/bash.exe`; its [installer source](https://github.com/git-for-windows/build-extra/blob/main/installer/install.iss) locates the runtime under `usr\bin\msys-2.0.dll`. Checking beside Folio's wrapper is not checking beside the actual MSYS executable.

D:1675's unrun fallback covers neither DLL, but says nothing about one DLL, both DLLs, unreadable directories or a non-shell executable located in a runtime directory. D:659 introduces `Cygwin { home }` without defining where that home comes from. The [Cygwin path guide](https://cygwin.com/cygwin-ug-net/using.html#cygdrive) documents configurable cygdrive prefixes and a stable `/proc/cygdrive` route; `/cygdrive` is a default, not a runtime identity proved by a DLL.

**Correction.** Treat sibling presence as a limited installation hint, not proof of the executable's runtime. Restrict candidates, handle the shipped wrapper explicitly, and define ambiguous/missing/inaccessible evidence. Give PROBE 11 a fallback for every outcome; an unrun probe must not silently enable unverified positive matches. State the Cygwin drive-prefix assumption or use the documented stable route for insertion; do not infer detector mount facts from it. Define `home` or leave it unknown and decline home-based detection. Add wrapper, integration-off, ambiguous-DLL and changed-prefix fixtures.

### 7. Should-fix — the corrected Agent encoding is contradicted by the picture rule, first ledger and acceptance assertion

**Evidence.** At [the pinned Codex parser](https://github.com/openai/codex/blob/a8964cb1bad67bc26a826fb07d1bef99c6a3f008/codex-rs/tui/src/clipboard_paste.rs#L251-L286), the quote-stripped string reaches the Windows recognizer before shlex; shlex receives the original string. The recognizer at lines 333–360 accepts drive-rooted/backslash-UNC forms. Thus D:531–538's split is supported for the named Windows/macOS examples, including the POSIX apostrophe example. This was a source read, not a recipient execution.

But D:1205 still requires an agent picture path to be single-quoted, and D:1770 explicitly says POSIX quoting on both platforms. D:1368–1369 requires every Agent output to be one shlex token: the valid Windows Agent output `"C:\"` has a trailing backslash escaping the closing quote under shlex. Also D:550–554's claim that two paths cause `None` is only valid when the shlex branch runs; a Windows run can reach the earlier recognizer as one combined, invalid path. The [consumer](https://github.com/openai/codex/blob/a8964cb1bad67bc26a826fb07d1bef99c6a3f008/codex-rs/tui/src/bottom_pane/chat_composer.rs#L1263-L1282) then attempts image decoding.

**Correction.** Update §4.4 and first-ledger row 9 to the platform split. Test against the recipient's branch order, including Windows roots, UNC, apostrophes and multi-path insertion; reserve shlex identity for POSIX outputs. Say multi-file automatic attachment is not guaranteed without claiming every branch returns `None`.

**Other recipients.** [Claude Code's documented workflow](https://code.claude.com/docs/en/common-workflows#work-with-images) allows an image path in prose; its file-reference section describes `@`. [Copilot CLI's documentation](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/overview#attach-images-and-pdfs) describes `@`, drag-and-drop and clipboard-image attachment. Neither page establishes that Folio's particular shell-escaped apostrophe sequence becomes a literal filename in prose or an attachment. D:543–545's “quotes are read correctly there” should remain an unverified inheritance claim; it is not documented parser behavior. Retain versioned recipient measurements rather than promoting those docs to a quoting proof.

### 8. Should-fix — removing cmd refusals does not make CRT syntax correct for a Python or Node prompt

**Evidence.** D:371–384 says unknown Windows programs use their argv convention and that Python/Node paths work; D:1455–1457 requires a Python/Node profile to open the file. D:345–346 promises correctness for the program the row starts. But these profiles receive pasted terminal input after process creation. [Python's string-literal rules](https://docs.python.org/3/reference/lexical_analysis.html#escape-sequences) interpret backslash escapes in ordinary quoted strings.

**Consequence.** A Windows Python REPL given the CRT output `"C:\new\test.png"` interprets `\n` and `\t`; the value names a different path. Its process's startup argv parser never sees this input. No switch to one of the six specified grammars proves general REPL correctness.

**Correction.** Keep `%`/`!` restrictions scoped to the cmd interpreter, but label unknown-program encoding as a best-effort default with no literal-identity guarantee. Replace the claimed REPL acceptance row with a defined CRT command-line consumer, or add actual Python/JavaScript literal modes and test those. Do not assert that the platform's process-creation convention is the recipient's interactive language.

### 9. Blocking — PROBE 4's promised `paste_as` fallback cannot select a path spelling

**Evidence.** D:325–333 separates namespace spelling from grammar. D:359–365 defines `paste_as` exclusively as `powershell | cmd | posix | fish | nu | agent`. D:721–723 and D:1668 nevertheless promise `D:/Demo/a.txt` through that key. D:1387–1389 pins the standing namespace independently of the grammar override.

**Consequence.** None of the allowed values requests Windows forward-slash spelling. Changing a Git Bash row to `cmd` changes quotes while its namespace can remain `Msys`; leaving it `posix` retains `/d/...`. The documented unrun fallback is impossible to express.

**Correction.** Define an independent spelling override, or explicitly make `paste_as` a structured grammar-plus-spelling policy with all combinations specified. Show the exact configuration that emits `D:/Demo/a.txt` with POSIX quoting without changing detection or environment variables. Carry that configuration into PROBE 4 and the owner-ruling options.

### 10. Blocking — `.lock` neither serializes the quota nor has safe stale recovery

**Evidence.** D:1100–1106 promises a 512 MiB aggregate bound and refusal when protected files prevent eviction. D:1111–1119 calls the read/evict/write sequence serialized, then lets a contending writer bypass the lock and exceed the quota. D:1417–1418 makes a ten-second-old lock reclaimable. D:1178–1180 admits network-backed temp I/O; D:1167–1168 still calls 512 MiB a maximum. No liveness, lease renewal, unique lock ownership or conditional-unlink protocol is specified.

**Consequence.** A live write stalled for ten seconds is indistinguishable from a dead owner's lock. Another process can replace its lock; the original process's eventual cleanup can remove the replacement. Independently, bypassing a held lock permits overlapping check/write sequences and repeated overshoot with no stated upper bound. A later sweep cannot necessarily restore 512 MiB while honoring the 24-hour floor. D:1108–1110's absolute protection of an unsubmitted line is also contradicted by D:1171–1174's correct admission that a line can outlive that floor.

**Correction.** Choose one quota contract. For a hard bound, serialize accounting, reservation, eviction, writing and quota sweeps across processes with a crash-released OS lock or an equivalently specified protocol; on contention, wait off-loop with a defined limit or refuse. Never steal a lock solely because its age exceeds ten seconds, and never unlink another owner's lock. If overshoot is a product choice, define its maximum and remove hard-bound claims everywhere. Apply the 24-hour floor to every eviction path and describe it as a retention floor, not protection for every live prompt.

### 11. Should-fix — the new macOS temp name did not reach vetting or privacy, and the claimed std fallback is wrong

**Evidence.** D:974–985 selects `$TMPDIR/folio-<uid>/clipboard` or `/tmp/folio-<uid>/clipboard`; D:1013 still creates/vets `Folio`, and D:1214–1215 still tells privacy documentation to publish `$TMPDIR/Folio/clipboard`. `crates/bt-platform/src/instance.rs:331–337` explicitly filters an empty `TMPDIR` and supplies `/tmp`. This is not what std necessarily does: at the repository's minimum Rust version (`Cargo.toml:37`, 1.89), [std's source](https://github.com/rust-lang/rust/blob/1.89.0/library/std/src/sys/pal/unix/os.rs#L621-L633) uses the Darwin-specific fallback when `TMPDIR` is unset and does not filter an empty value. [Rust's current API documentation](https://doc.rust-lang.org/std/env/fn.temp_dir.html#platform-specific-behavior) also identifies Darwin's system-provided temp and Windows environment-based selection, not an unconditional per-account directory.

**Correction.** Choose explicitly between the existing `instance::runtime_directory()` policy and platform std discovery. If the requested `/tmp` fallback is retained, identify it as Folio's own policy. Use `folio-<uid>` consistently in Unix creation, vetting, privacy and deletion instructions. Describe Windows temp as configured, then vetted; `TMP` can take precedence over `TEMP`. A predictable UID suffix separates normal users' names but does not prevent someone precreating another UID's directory, so keep the fail-closed recovery limitation explicit rather than treating the suffix as proof against squatting.

### 12. Blocking — Windows vetting specifies creation permissions but no check of existing directory security

**Evidence.** D:1013–1018 explicitly repairs existing Unix permissions. D:1019–1023's Windows counterpart only rejects reparse points and creates directories with a user-only DACL; files inherit it. D:1024–1031 promises vetting before each operation. [CreateDirectoryW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectoryw) applies the supplied security descriptor to a new directory, returns `ERROR_ALREADY_EXISTS` for an existing one, and requires a filesystem supporting persistent ACLs for that descriptor to have effect.

**Consequence.** An existing ordinary `%TEMP%\Folio\clipboard` directory with broad permissions passes the specified reparse check. Merely requesting a secure descriptor at creation neither repairs that directory nor proves that new files inherit restricted access. A redirected temp volume without ACL support is another admitted path that cannot satisfy the stated owner-only promise.

**Correction.** Specify owner/SID, DACL and inheritance validation for both existing Windows levels, repair only an owned directory or refuse, and refuse storage that cannot enforce the access contract. Verify the created file's access restriction rather than relying on an unverified parent. Add existing broadly readable and non-ACL-capable storage cases beside the reparse fixture in §6.2.

### 13. Should-fix — a folder on Preview centre still has two verbs

**Evidence.** D:852 says Preview centre refuses a folder. D:929's “file URLs” row includes folders—its Files-centre cell distinguishes them—but gives Preview centre unconditional `Retarget (one)`. `main.rs:28988–28993` retargets Preview only for `RowPayloadKind::File`.

**Correction.** Split files and folders into separate rows, or qualify Preview centre as `Retarget` for one file and `Refused` for a folder. Apply the same distinction to hover and commit. Preserve the existing Placeholder and edge/rim entries; their addition does not resolve this cell.

### 14. Should-fix — nushell's unrun fallback assumes the other half of its probe succeeded

**Evidence.** D:484–490 asks PROBE 10 to establish both maximum fence width and argument-position acceptance, then allows width-one literals before either is known. D:1674 repeats that fallback; D:1367–1368 unconditionally tests fence growth. The [Nushell book](https://www.nushell.sh/book/working_with_strings.html#raw-strings) documents raw strings and growing fences, but the design pins no supported nushell version in D:470–491 or D:1717–1723.

**Consequence.** Limiting fence width addresses only the width question. It cannot establish argument-position support on the recipient version. “If the probe finds argument position does not take a raw string” (D:489–490) admits that the proposed unrun build could already ship an invalid arm.

**Correction.** Establish a version-pinned baseline for width-one literals in both builtin and external argument positions before enabling this fallback. Otherwise refuse the Nushell lane until that baseline is measured. Gate the growing-fence test on the selected supported width, and give the table at D:400 a literal/example plus the construction formula rather than the ambiguous repeated ellipsis notation.

### 15. Should-fix — PROBE 3 and PROBE 9 still have contradictory shipping instructions

**Evidence.** D:555–565, D:1599–1601 and D:1667 permit unmeasured agents to inherit Codex spelling with a recorded risk. D:1653–1655 says a T-PASTE-1-only release refuses unmeasured recipients. D:1677–1682 says child temp-file access is no longer a PROBE 9 question, but D:1565–1568 and D:1783 still ask it. D:1673 says an unrun TCC probe produces “refusal”, without identifying whether that disables the Mac picture lane or means handling a future OS denial.

**Correction.** Use the same Agent policy for a T-PASTE-1-only release and the full release; name any intentional difference in the table. Remove the obsolete child-read probe text. Define PROBE 9's unrun shipped state explicitly: either disable the affected picture lane with a named refusal until measured, or record a supported assumption and handle actual errors. An unrun experiment cannot provide a per-request TCC result. Keep PROBE 1's mandatory fixture requirement distinct from these optional fallbacks.

### 16. Blocking — a unique picture-job address does not preserve the input request after the reader presses Enter

**Evidence.** D:1135–1136 names Enter and another paste as hazards. D:1138–1152 captures window/tab/leaf, session incarnation and a globally unique request number, but completion revalidates only target existence/incarnation, modal state and the setting. D:1120–1122 rejects only another picture job. D:1420–1424 tests none of the intervening Enter, typing or ordinary-text-paste cases. `main.rs:96320–96322` already records ordinary paste as user typing.

**Consequence.** The reader starts a picture job, presses Enter, and starts another command before encoding finishes. The same terminal session and job address still exist; every listed check passes, so the path enters the next command or the running program's input. One picture in flight prevents two picture jobs from racing, not a job from racing subsequent user input.

**Correction.** Capture a per-target input generation and invalidate the job on subsequent submitted input, typing, ordinary paste, K144 or another path insertion according to a stated policy. On invalidation, suppress delivery and clean up the owned output. Add Enter-before-completion and text-paste-before-completion fixtures. If insertion into the then-current line is intentional, state that product contract explicitly and stop presenting the job identity as preserving the original input request.

### 17. Blocking — asynchronous picture drops on Preview and edges have no compatible completion target

**Evidence.** D:930 sends a raw picture through §4 and then `Retarget` or `Split` on nonterminal landings. D:1138–1140 requires every delayed job to carry a terminal session incarnation, and D:1152 applies this to picture drops. D:1144–1151 defines completion only as terminal insertion. The code's `LeafSession` is explicitly a Terminal leaf's PTY/session (`main.rs:10283–10292`); a Preview target has no such incarnation, and a future split has no created destination seat to capture.

**Consequence.** An implementer cannot apply the stated identity contract to a raw-image drop onto Preview or a root rim. Recomputing the landing after the worker returns can open the picture in a layout the reader did not drop on; rejecting all targets without sessions silently removes the admitted cells.

**Correction.** Give picture jobs typed destinations: terminal insertion with session/input generation, preview retarget with pane/content generation, and split with a captured layout anchor/side and a defined revalidation rule. State cancellation and file cleanup when those targets change. Add delayed Preview and edge/rim fixtures. Do not describe a terminal-only completion protocol as covering all picture drops.

### 18. Should-fix — original PNG preservation contradicts the first-frame policy

**Evidence.** D:1047–1052 writes original PNG bytes after validation. D:1082–1084 and D:1410 say multi-frame sources write only frame one. The [PNG specification](https://www.w3.org/TR/png-3/#4Concepts.APNG) includes animated PNG. `registry/image-0.25.10/src/codecs/png.rs:140–161` distinguishes full animation decoding from the default image, which need not be an animation frame.

**Consequence.** A valid animated PNG remains animated if the original bytes are written. Decoding its default image alone neither validates all animation frames nor necessarily extracts animation frame one. A GIF is not directly among D:108–110's advertised-format rungs, so the GIF example does not settle the actual PNG case.

**Correction.** Choose a policy for APNG: preserve the complete animation with appropriately bounded validation and disclosure, flatten and re-encode the first animation frame with a stated metadata exception, or refuse animation. Align the full-decode, original-byte and frame-count claims and add an APNG fixture with a separate default image.

### 19. Should-fix — picture cleanup covers representability refusal but misses later grammar refusal

**Evidence.** D:991–999 vets the directory using §2.4 and deletes an “unspellable” result; D:1422–1423 tests only the representability gate. That gate checks UTF-8 and controls (D:573–598), not the cmd `%` rule (D:443–446), the unprobed PowerShell quote class (D:420–428), or the Nushell fence limit (D:487–490). D:1201–1205 applies all those encoders only after the picture exists.

**Consequence.** A valid UTF-8 `%TEMP%` path containing `%`, or a Mac temp path requiring an unsupported Nu fence, passes directory representability and is written, then receives an encoder refusal. None of the explicitly listed cleanup paths addresses this refusal. The new directory check therefore closes R2's invalid-UTF-8 case but not the broader claim that vetting proves a usable shell literal.

**Correction.** Preflight the prospective output's namespace and grammar against the captured recipient before writing, then clean up on every post-write failure to deliver, including encoder refusal. Keep representability separate from recipient-specific encodability; a globally vetted directory can be usable for one pane and refused for another. Add a redirected-temp `%` case and an unprobed-grammar case.

### 20. Should-fix — the DIB decoder requires an image feature that the ticket never enables

**Evidence.** D:1053–1058 and D:1611–1614 require `BmpDecoder::new_without_file_header`. `Cargo.toml:107` uses `image = 0.25.10` with defaults disabled and only `gif`, `jpeg`, `png`, `webp`; `crates/bt-app/Cargo.toml:64` inherits that dependency. `registry/image-0.25.10/src/lib.rs:245–246` gates `pub mod bmp` on `feature = "bmp"`, and its `Cargo.toml:70` declares `bmp = []`. D:1074–1080 discusses missing TIFF support but does not mention this required BMP feature.

**Correction.** Add enabling `image/bmp` to T-PASTE-2a's planned manifest scope, and check the resulting dependency/notices impact rather than assuming TIFF's impact applies to it. Make the DIB lane contingent on that feature. With the currently stated feature set, the proposed decoder import is unavailable.
