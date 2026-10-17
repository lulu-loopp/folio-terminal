# Folio's own PSReadLine in every Windows PowerShell pane — design note, 2026-10-15

0.4.8 plan row G6, ticket T-INTEGRATION-INJECT-2 (docs only). The build that follows it is size L (§6). Base: main `9021e411`. The note is written from the code at that commit and from the T-INTEGRATION-INJECT spike of 2026-10-04 (the coordinator's trace, sections Q1, Q7 and Follow-up A–C), which measured every load order this note relies on. Nothing in this note was measured again: no PowerShell module was fetched or written, and no shell was started.

---

## 0. The decisions, in one screen

1. **A Windows PowerShell 5.1 pane loads Folio's PSReadLine, chosen before the host imports one.** Folio composes a pane-only `PSModulePath` whose first entry is a directory holding only Folio's `PSReadLine`. The host's own startup import, which runs before every profile, then finds Folio's copy. Nothing is imported a second time, and the account's profile runs against the editor the pane will keep. An import from inside `-Command` cannot do this: by then 2.0.0 is loaded and a second import binds to it (§2.3).
2. **Binary-only, and that is what makes it work under every execution policy.** Folio's copy is the DLLs, `License.txt`, and a manifest Folio writes whose `RootModule` is the DLL. It has no `.psm1` and no `.ps1xml`. Execution policy governs those two kinds of file and not a manifest or a DLL. The one function the module's `.psm1` would define, `PSConsoleHostReadLine`, is already defined by the injected loader text. Measured: Restricted, AllSigned and RemoteSigned all load it, silently.
3. **The binary is Microsoft's signed PSReadLine 2.4.5, taken from the PowerShell Gallery package, pinned and verified like the ConPTY package (recommended; owner question Q1).** Today's bundled module is not a Microsoft release. It is Folio's own unsigned fork build, `2.4.6-bt.2`, whose patch fixes the resize anchor. Upstream's newest stable release is 2.4.5. The resize repair that 2.4.5 lacks is already in `folio.ps1`: the reflection repair it runs for every 2.4.x on Folio's resize chord. Recommending 2.4.5 retires the fork and leaves one editor version and one repair road on both editions.
4. **It lives in Folio's local data folder, content-addressed, and is written from bytes embedded in the executable.** Its home is `%LOCALAPPDATA%\Folio\psmodules\<artefact>\PSReadLine\2.4.5\`, written on the birth worker the way `folio.ps1` is written today. Nothing is added to the release archive, the MSIX manifest, or the winget, Scoop or cask manifests. The uninstall's existing "Local data" row removes it whole.
5. **The invitation, the Terminal page's PSReadLine row, the out-of-band probe, the launch-time upgrade and the Documents installer all retire.** Nothing replaces the invitation. A Documents copy that an earlier Folio wrote stays where it is, with a removal row and the uninstall verbs (Q4). The marks record is only read after this, never written.
6. **Folio's copy wins in a Folio 5.1 pane, whatever the account has installed (Q2).** The account's editor is what the pane gets wherever Folio does not compose the pane: the Shell integration switch is off, or the row's own words cannot be composed.
7. **pwsh 7 panes and macOS are untouched.** pwsh 7 puts its own module folders ahead of anything inherited, and its in-box 2.4.5 is already the version recommended here. The composition is Windows-only, and so are the bytes.
8. **The build is one ticket in four steps** (§6): the artefact and its check, the pane composition, the retirement, then the documents.

---

## 1. Today's shape and its debts

### 1.1 What a 5.1 pane gets today

- **The account's PSReadLine, whichever the host finds first.** On a stock machine that is the in-box 2.0.0 (assembly `3.0.0.0`, strong-named, .NET Framework 4.6). 2.0.0 derives the edit anchor from the cell count before a resize, so narrowing the pane redraws a wrapped, unsubmitted input line over text that has moved (`docs/shell-integration.md`, "Default shell and `BT_SHELL`"). Folio's resize chord (`Ctrl+Alt+Shift+F12`) is consumed there as a no-op, because 2.0.0's only repair, `InvokePrompt`, clears the viewport.
- **Folio's integration, process-scoped.** Since T-INTEGRATION-INJECT-1, `shell_integration::powershell_load_command` appends `-NoExit -Command <loader>` to every composable PowerShell row. The loader evaluates `folio.ps1`'s text, so execution policy is not involved. The loader also defines `PSConsoleHostReadLine` itself whenever a PSReadLine module is loaded and the function is missing. That is the pwsh 7 revive road, and §2.3 reuses it unchanged.
- **The invitation** (`psreadline::invite_decision`, drawn in `restore.rs`, raised from `runtime/first_run.rs`). It is a modal dialog, raised once when the first 5.1 pane's out-of-band probe answers `< 2.4.6`, and once more after a font-size change if the answer was `Declined`. Its `Install` writes the nine bundled files into `<Documents>\WindowsPowerShell\Modules\PSReadLine\2.4.6\` through `psreadline::install_recorded`, after recording the root in `integration-marks.json` under the marks lock. Under Restricted or AllSigned, `Install` is dark and a reason line names the policy (§7.47).
- **The Terminal page's PSReadLine row** (`RowState`, eight states), an On/Off picker that installs or removes the same copy.
- **The launch-time upgrade** (`psreadline::upgrade_recorded`, RULES §38's 2026-09-21 ruling). Before the first window, it replaces an older `-bt.` build that Folio recorded installing.
- **The probe** (`psreadline::PsReadLineProbe`). One `powershell.exe -NoProfile -NonInteractive` per Terminal-page open, and one per 5.1 birth while no usable answer is held. It reads the highest installed version and the effective policy.

Folio starts pwsh 7 by default when it is present, so 5.1 is the default only on machines without pwsh 7, or when a reader chose it. That is still the shell most Windows users have.

### 1.2 What each road costs a person

| Road | Cost |
|---|---|
| The invitation | A modal that interrupts the first 5.1 pane to ask a question most people cannot evaluate ("install a newer PSReadLine into your Documents?"). It appears while they are typing, which is why Enter presses nothing. A `Declined` is asked once more. |
| The install | A write outside Folio's folders that changes **every** Windows PowerShell host on the account (Windows Terminal, an editor's terminal, the Start menu's PowerShell), not only Folio's panes. On a machine whose policy is Restricted or AllSigned, the unsigned fork cannot load. The spike measured the consequence on the owner's machine: with the copy in Documents, a 5.1 started under Restricted gets **no PSReadLine at all** ("Cannot load PSReadline module. Console is running without PSReadline."), because 5.1 does not fall back to the 2.0.0 later on the path. Today's copy therefore makes other hosts worse wherever policy refuses it. That is the case `install_available` avoids by darkening the switch, but a machine's policy can change after the install. |
| The policy card | On a fresh Windows (Restricted is the client default), the only live answer is "decline". T-POLICY-FIRST-RUN recorded this: no way forward exists. |
| The upgrade | A silent write into the account's Documents at every launch that finds an older Folio build there. |
| The probe | One child process per Terminal-page open, and at 5.1 births. It is one of the census's PowerShell spawns, with its own patience, turn and failure states. |
| The marks | `psreadline_module_roots` in `integration-marks.json`, and the window-thread lock debt of RULES §38 (`ARCHITECTURE.md` §5.3 rows 2–4: `apply_psreadline`, `apply_recorded`, `installed_copy`). |

### 1.3 Four premises of the ticket that the code does not bear out

The build must not inherit these, so they are stated here:

1. **The module Folio ships is not "the signed Microsoft release".** `assets/psreadline/2.4.6/` holds Folio's fork build. `PATCHED_BUILD = "2.4.6-bt.2"` is the `ProductVersion` stamp in the DLL, and DESIGN §7.1.6c-3b names the fork's shipping branch, `bt/anchor-fix-2`. Its DLL is Authenticode NotSigned and has no public key (spike, "What the DLLs are built for"). The PowerShell Gallery's newest stable PSReadLine is **2.4.5** (2025-10-22). Upstream has no 2.4.6. The fork took the next number.
2. **There is no PowerShellGet road.** Folio's invitation writes embedded bytes itself. `Install-Module` appears only as advice in `docs/shell-integration.md`.
3. **The licence is BSD 2-Clause, not MIT** (`assets/psreadline/2.4.6/License.txt`: Copyright (c) 2013, Jason Shirk). The notices row already says so. A binary distribution must reproduce the notice, which Folio does today and keeps doing (§3).
4. **"DLL + psd1/psm1 from the signed release" would block a pane's startup.** The spike's Q7 put the gallery's signed 2.4.5 first on a 5.1 pane's module path. Under Restricted and under AllSigned, startup **stopped at an interactive prompt** ("Do you want to run software from this untrusted publisher? … PSReadLine.format.ps1xml"), because Microsoft is not in TrustedPublisher on a stock machine. Only the binary-only shape loads under every policy. The signed `.psm1`, `.psd1` and `.ps1xml` are not shipped. Folio writes its own manifest (§2.1).

A fifth fact bears on the build: **`folio.ps1`'s gate `$psReadLineVersion -ge [version]'2.4.6'` means "the fork" while saying "a version"**. A future upstream 2.4.6 that keeps upstream's anchor bug would pass that gate, and Folio's resize chord would turn into a no-op for it. The build removes the gate (§2.4).

### 1.4 The two open rows it closes

- **T-FIRST-RUN-MERGE** (tickets-047): one first-launch surface instead of three. T-INTEGRATION-INJECT-1 took the integration half. Its own status line says the PSReadLine half is superseded when this ticket lands. After the build, no first-launch surface concerns PowerShell at all.
- **T-POLICY-FIRST-RUN** (tickets-047): a fresh, Restricted Windows has a way through. The integration half went with INJECT-1, which runs a text load that policy does not govern. The PSReadLine half is closed by §0.2: a binary-only module is not governed by policy either, so there is no question left to ask and no policy to change.

Both rows are marked superseded in the ledger by the build's last step (§6), and neither is built.

---

## 2. The proposal

### 2.1 The artefact

**Recommended (Q1 = M): Microsoft's PSReadLine 2.4.5, as published on the PowerShell Gallery.**

| | |
|---|---|
| Source | the gallery's raw `.nupkg` for `PSReadLine` 2.4.5 (the "Download the raw nupkg file" link on the package page; the gallery serves the package's files under `/packages/PSReadLine/2.4.5/Content/<file>`) |
| Kept in the repository | `vendor/psreadline/PSReadLine.2.4.5.nupkg`, with `vendor/psreadline/README.md` stating its source, version, SHA-256 and signature facts, exactly as `vendor/conpty/README.md` does for the ConPTY package. It is a published binary artefact, not source. No upstream source is vendored, and no file in it is modified. |
| Entries taken | `Microsoft.PowerShell.PSReadLine.dll`, `Microsoft.PowerShell.Pager.dll`, `netstd/Microsoft.PowerShell.PSReadLine.Polyfiller.dll`, `net6plus/Microsoft.PowerShell.PSReadLine.Polyfiller.dll`, `License.txt`. **Not taken:** `PSReadLine.psd1`, `PSReadLine.psm1`, `PSReadLine.format.ps1xml` (§1.3 item 4), and `Changes.txt` (it is not needed to run the module, and the notices carry the licence). The entry list is the first thing the build step reads from the package. If the package's layout differs from the fork's (which mirrors upstream 2.4.x), the step records the difference and does not guess. |
| Verified | **At build time**, in Rust, by the build script that extracts the entries: the package SHA-256 and each entry's SHA-256 against a pinned table. This is the `conpty_sidecar` shape (`crates/bt-pty/src/conpty_sidecar.rs`), with the same refusal on a mismatch. **When the pin is set or moved**, by a check script on Windows (§3.5): each extracted DLL's Authenticode signature is `Valid`, its signer subject is `Microsoft Corporation`, and the hashes in the README equal the table's. The gallery's own feed hash is not relied on. Our SHA-256 of the downloaded package is the pin, as it is for ConPTY. |
| Fetched | **Once, by the person who moves the pin**, never by a build, a test or `package.ps1`. CONVENTIONS §三 D-60 says a test never reaches the network. A release that re-fetched at packaging time would make the release depend on the gallery being up and unchanged, and a pinned hash already proves it unchanged. This differs from the ticket's sketch ("re-fetched by `package.ps1`"), and the ConPTY precedent is the reason. |
| Folio's own file | `assets/psreadline/PSReadLine.psd1`: `RootModule = 'Microsoft.PowerShell.PSReadLine.dll'`, `ModuleVersion = '2.4.5'`, upstream's `GUID`, `PowerShellVersion = '5.1'`, `CmdletsToExport` = the five PSReadLine cmdlets, no `NestedModules`, no `FormatsToProcess`, no `FunctionsToExport`. It is derived from upstream's manifest, with the same notice. A test reads its `ModuleVersion` and binds it to the one Rust constant (§2.4), the way `PATCHED_VERSION` is bound today. |

**The alternative (Q1 = F): keep the fork.** The artefact would be `2.4.6-bt.2`'s five binaries plus `License.txt`, kept in `assets/` as today and pinned by the same SHA-256 table. Under F, the fork should also define `PSConsoleHostReadLine` itself on import, through an `IModuleAssemblyInitializer` (spike, conclusion B). That removes the one cost of the binary-only shape: a nested `powershell.exe` typed inside a pane finds Folio's module through the inherited path, and without the function it gets the legacy line editor. F keeps that fork build and its maintenance, an unsigned DLL loaded into `powershell.exe` (which Smart App Control or an application-control policy may refuse; unmeasured), and a source branch that is not public today. M gives up the fork's native anchor fix and keeps the nested-shell cost. In return, M ships a Microsoft-signed binary, with no fork to rebuild and nothing to publish.

The rest of this note is written for M. Under F only the version literal, the entry list and the stamp check change. §2.4 says what F keeps in `folio.ps1`.

### 2.2 Where it lives, and who writes it

**`%LOCALAPPDATA%\Folio\psmodules\<artefact>\PSReadLine\2.4.5\`.** `<artefact>` is the first twelve hex digits of the pinned package SHA-256. The decision, against "beside the executable":

- **A loaded DLL is a held file.** A live 5.1 pane, or a nested shell it started, holds `Microsoft.PowerShell.PSReadLine.dll` open. If the module sat beside `folio.exe`, a self-update could not replace the installation while any such pane lived. A content-addressed directory under local data is never rewritten: a build that ships the same artefact finds the same bytes, and a build that ships a new one writes a sibling.
- **One road on every channel.** The zip, winget, Scoop and the MSIX sparse package (which carries no program files) all start the same `folio.exe`. Writing from embedded bytes makes the module identical everywhere and adds no archive member (§3.1).
- **No Mark of the Web.** Bytes written by Folio carry no zone identifier. The spike's copies were prepared the same way, and RemoteSigned's zone check never applies.
- **Local, not roaming.** `%APPDATA%\Folio` roams. A module is a machine-local cache of what the executable carries, which is what `%LOCALAPPDATA%\Folio` (WebView2's root) already is.
- **The uninstall takes it whole.** `uninstall.rs`'s "Local data (including WebView2)" mark removes `%LOCALAPPDATA%\Folio`. The held-file rule there ("any process holding any file in a data root stops the whole run", and the run names the file) covers a 5.1 shell left running after Folio quits.

**The writer** is the birth worker, beside `powershell_script_for_birth`, which already compares and maybe writes `folio.ps1` off the window thread. Its rules are the script's:

- Compare the six files' bytes and write only what differs, through the atomic replace (the INJECT follow-up F4 lesson). A failure is said once (`WriteNotice`) and retried by the next birth (T-PROBE-NO-CACHED-FAILURE).
- `update_trial::defer` gains `Writer::PsReadLineModule`. During a trial, the module is prepared under the trial's temp directory, as the script is, and the trial's cleanup removes it.
- At launch, a sibling `<artefact>` directory that is not this build's is removed when nothing holds it. One that is held stays, and the next launch tries again.

### 2.3 How a 5.1 pane loads it: the order, and the honest rule

**What the spike measured about order.**

- 5.1's console host imports PSReadLine while it creates the runspace (`DoCreateRunspace`), before the four profiles run, and runs `-Command` after them. The first statement of `-Command` saw `2.0.0` already loaded under all three policies.
- `Remove-Module PSReadLine; Import-Module <another psd1>` from `-Command` yields a module that *claims* the new version, but whose `ImplementingAssembly` is the 2.0.0 DLL. PowerShell binds the manifest's DLL by name to the assembly already loaded.
- A `LoadFrom` of the new DLL loads side by side, but `[Microsoft.PowerShell.PSConsoleReadLine]` keeps resolving to 2.0.0.

So **the editor is chosen before the host imports one, or not at all.** The only door at that moment is the module path the host searches.

**The composition** (spike Follow-up C, measured). For a pane whose program is Windows PowerShell and whose argv the composer loads into:

```
PSModulePath = <FolioModules> ; [<Documents>\WindowsPowerShell\Modules ;] <V>
```

- `<FolioModules>` is the `psmodules\<artefact>` directory, which holds only `PSReadLine`, so the prefix shadows nothing else.
- `<V>` is the value the pane would otherwise have: the environment Folio already builds for the birth, after the row's own environment layer. A row that sets `PSModulePath` keeps its value, with Folio's entry in front.
- `<Documents>` is `bt_platform::documents_directory()` (the known folder, honouring redirection). It is added **only when 5.1 would have added it itself**: `<V>` unset or equal to the registry machine value, and the entry not already in `<V>`. 5.1 keeps any other inherited value verbatim and adds nothing.
- It is **declared on the command explicitly**. `portable_pty`'s `CommandBuilder` rebuilds `PSModulePath` from the registry otherwise; DESIGN §7.1.6c-3b found that out the hard way (`declare_probe_module_path`).

**The loader text needs no new branch.** The guard already in `powershell_load_command` (`if PSReadLine is loaded and no PSConsoleHostReadLine function exists, define it from the module's ImplementingAssembly`) is exactly what a binary-only module needs. The spike's end-to-end runs used that shape: 2.4.x from Folio's folder, OSC 133 `A,B,C,D;0`, coloured echo, first `$?` True, `LASTEXITCODE` untouched, under Restricted, AllSigned and RemoteSigned.

**The prefix and the loader are one decision.** A pane gets the prefix only when it also gets the loader, because without the injected function a binary-only module leaves the host on its legacy line editor (measured). So the Shell integration switch off, an uncomposable row (`-File`, `-NonInteractive`, `-CommandWithArgs`, stdin, an unknown switch), or Constrained Language Mode all mean no prefix. Those panes get the account's PSReadLine, exactly as today.

**The honest rule on double loading.**

| What the account's profile does | What the pane has |
|---|---|
| nothing about PSReadLine | Folio's 2.4.5 |
| `Set-PSReadLineOption …`, `Set-PSReadLineKeyHandler …` | Folio's 2.4.5, configured by the profile, because the host imported Folio's before the profile ran |
| `Import-Module PSReadLine` (by name, no version) | a no-op: the module of that name is loaded. Measured harmless in the outer pane (B, "user types `Import-Module PSReadLine`", prefix kept). |
| an explicit other version (`-RequiredVersion`, `-MinimumVersion` above 2.4.5, a path, or `Remove-Module` and re-import) | **the account's explicit choice, and its consequence.** A second PSReadLine is loaded beside Folio's, which breaks editing the way the spike's "restored" row broke (`ReadLine` overload error at every prompt, C/D marks lost). It breaks the same way in any host whose preloaded version differs from the one the profile demands. Folio does not detect it, does not repair it, and does not refuse the pane. The person's road is the Shell integration switch, which gives them the account's editor again. This row is a measurement the build owes (§5.1), not a measured fact. |

**The prefix stays in the pane's environment** (spike conclusion B, disposition (i)). Removing it after the load was measured to make any later by-name import or command lookup pull 2.0.0 in as a second module, which breaks the pane. Keeping it costs a nested `powershell.exe` typed in the pane its line editor (§2.1, F). A nested pwsh 7 is unaffected: it puts its own three folders first (measured).

**When Folio's module cannot load at all.** Under every execution policy it loads (measured). What remains unmeasured is an application-control refusal of the DLL itself (WDAC, AppLocker DLL rules, Smart App Control). Under M the DLL is Microsoft-signed, which those policies generally admit. Under F it is unsigned. The spike measured what 5.1 does when the first-found PSReadLine fails: it prints its one line ("Cannot load PSReadline module. Console is running without PSReadline.") and does not fall back. The loader's guard then installs nothing, so the pane is plain and nothing red comes from Folio. The VM row in §5.4 measures this case. If it happens, the remedy is a ruling on that evidence, not a branch written in advance.

### 2.4 The resize repair

Under M, Folio's 5.1 panes run the same 2.4.5 that pwsh 7 ships, and `folio.ps1` already repairs 2.4.x's anchor by reflection on Folio's resize chord (the `Major -eq 2 -and Minor -eq 4` branch, with its continuation-prompt and wide-character arithmetic). DESIGN §7.1.6c-3b records that this road was measured to work (A/B: chord sent → prompt intact, chord not sent → carved). It was not adopted then only because it would have contradicted the Terminal row's sentence "2.4.6 … already anchors itself", and this ticket retires that row. The build:

- **Removes the `$psReadLineSelfAnchors` gate.** Every 2.4.x gets the repair, and a version outside the proven shape keeps consuming the chord as a no-op, as today. This also closes the latent defect in §1.3.
- **Keeps one Rust constant**, `psreadline::SHIPPED_VERSION = "2.4.5"`. It is bound by test to Folio's `PSReadLine.psd1` and to the pinned table's package name. `PATCHED_VERSION`, `PATCHED_BUILD`, `family_prefix` and `Build` survive only as long as the legacy removal row needs them (§2.5).
- **Under F**, the gate stays, but it keys on the fork's stamp rather than on `-ge 2.4.6`: the loader reads `ProductVersion` from the module's assembly, the same fact `installed_copy` reads.

### 2.5 What retires, and what stays

| Retires | Where | Note |
|---|---|---|
| the invitation dialog and its trigger table | `psreadline::{Invite, InviteDecision, invite_decision, state_after_decline, invite_body}`, its drawing in `restore.rs`, the raise in `runtime/first_run.rs`, `InviteTarget`, the Enter/Esc ladder rows | nothing replaces it (Q3) |
| the `psreadline_invite` settings key | `bt_persist::PsReadLineInviteV1`, `SettingsV1::psreadline_invite` | **retired the way `powershell_integration_offer` was**: serde ignores the key, so old files load and new files omit it. The v7→v8 migration keeps inserting it for files that old, which is harmless. A test pins that a file carrying each of the four values still loads. |
| the Terminal page's PSReadLine row and `Capability::PsReadLineModule` | `settings.rs`, `first_run.rs` | replaced only by the legacy removal row below |
| the probe | `psreadline::{PsReadLineProbe, PROBE, begin_probe, ProbeTrigger, run_probe, parse_probe_output}`, `BT_PSREADLINE_PROBE`, the census and ARCHITECTURE rows (`psreadline-probe`, `PsReadLineProbed`) | DESIGN's two reasons for reading out of band no longer hold: the script is now injected into every composable pane, and the text load is not refused by AllSigned. Nothing reads the answer any more, and one child process per 5.1 birth goes. |
| the Documents installer and the launch upgrade | `install_recorded`, `install_checked`, `install_into`, `apply`, `apply_recorded`, `upgrade_decision`, `upgrade_recorded`, `Writer::PsReadLineUpgrade`, `Runtime::apply_psreadline` | Folio no longer writes outside its own folders for PowerShell at all. This pays off RULES §38's window-thread debt and the PSReadLine half of `ARCHITECTURE.md` §5.3 rows 2–4. |
| marks **writes** | `psreadline_module_roots` gains no entry from this build on | |

| Stays | Why |
|---|---|
| **a legacy removal row**, shown only while a Documents copy is there that an earlier Folio wrote (`installed_copy` is `ThisBuild` or `OlderBuild` against the stamp table, or the marks record names the root) | It follows the `$PROFILE`-line precedent: an earlier Folio's mark keeps working and is left alone, and Settings offers its removal only while the remover would act. `remove_from`'s byte- and stamp-guarded delete is unchanged. |
| the uninstall's "PSReadLine module" mark and `BT_PSREADLINE_DOCUMENTS` | The same legacy copy is still Folio's to remove on `--uninstall` and `--uninstall-cleanup`. |
| marks **reads** of `psreadline_module_roots` | They are how the remover finds a copy written to a redirected Documents. |

---

## 3. Packaging and policy

### 3.1 The release archive

**No member is added.** `scripts/release/archive-members.txt` is unchanged, and so is the manifest `folio.exe` carries. The module rides inside `folio.exe` (§2.2), as the fork's 437 KB does today. `package-tests.ps1` gains one assertion: no `PSReadLine` file is a member of the zip.

### 3.2 The MSIX sparse package

Unchanged. `packaging/msix/AppxManifest.xml` is a sparse package that names `folio.exe` through `AllowExternalContent` and carries no program files. The module is written by the running executable into local data, outside the package, which is the same place for every channel.

### 3.3 winget, Scoop, cask

- **winget**: no manifest text changes. Under the 2026-10-08 ruling, the description and tags name no third-party product. The existing "It starts PowerShell 7, Windows PowerShell, …" line and the `powershell` tag are not touched by this ticket; whether they pass `Policy-Test-1.2` is the release's R10 step, as for every release. The release notes are not winget text and may say "Windows PowerShell panes now use a current line editor".
- **Scoop**: the same zip, so the same files. Nothing changes.
- **Homebrew cask**: macOS has no Windows PowerShell. The bytes are `cfg(windows)`, so the macOS executable stops carrying the 437 KB it carries today.

### 3.4 Notices

- `licenses/bundled-assets.md` and its generated `THIRD-PARTY-NOTICES.md` row become **"PSReadLine 2.4.5 — BSD 2-Clause, Copyright (c) 2013, Jason Shirk"**. The text says the files are embedded in the executable and written to Folio's local data folder for Windows PowerShell panes, not "installed when the user asks". The verbatim marker points at the `License.txt` extracted from the pinned package, which `generate-notices.ps1` already resolves through `<!-- verbatim: path -->`. If a build-time extracted file cannot be read there, the README beside the package carries the text verbatim, as `vendor/conpty/LICENSE-MICROSOFT-TERMINAL` does.
- `licenses/README.md`'s table row moves to `vendor/psreadline/…`.
- `check-vendor-notices.ps1` is about modified vendored crates (Apache-2.0 §4(b)) and does not apply: nothing in the package is modified. The README states this, so the next reader does not add a row there.

### 3.5 The hash check in CI: `scripts/ci/check-psreadline-artefact.ps1`

1. The package file's SHA-256 equals the README's and the Rust table's.
2. Every entry the table names exists in the package with the pinned SHA-256, and no entry the build extracts is missing from the table.
3. On a Windows runner: each extracted DLL's `Get-AuthenticodeSignature` is `Valid` with signer subject `Microsoft Corporation` (under F: the stamp is `PATCHED_BUILD`, and the signature is whatever Q1 rules).
4. Folio's `PSReadLine.psd1` names `ModuleVersion` equal to the table's version, has no `NestedModules`, `FormatsToProcess` or `ScriptsToProcess`, and its `RootModule` is the DLL. The binary-only property is a checked fact, not a convention.

The build script's own refusal on a hash mismatch is the gate every build passes. The script is the gate for a pin change, and the place where the signature is asserted. `scripts/ci/tracked-binary-files.tsv` gains the `.nupkg` and loses the four `assets/psreadline/2.4.6/*.dll` rows.

### 3.6 Size

The executable's delta is the 2.4.5 binaries minus the fork's (330 KB + 17 KB + 5 KB + 7 KB of DLLs, plus 41 KB of `Changes.txt`, which is dropped), so it is about zero on Windows; the exact figure comes from the pinned package. macOS loses 437 KB. The repository gains one `.nupkg`. The release zip's size moves by the executable's delta only.

---

## 4. What changes for a person

| Road | Before | After |
|---|---|---|
| **Fresh install, 5.1 pane** (policy Restricted, the client default) | the invitation appears with `Install` dark and a policy sentence. The pane keeps 2.0.0, and the resize corrupts wrapped input. | No question. The pane edits with 2.4.5 at once, the resize chord repairs the anchor, and nothing is written outside Folio's folders. |
| **Fresh install, RemoteSigned** | the invitation; `Install` writes the fork into Documents for every host | no question; Folio's panes only. Other hosts keep what they had. |
| **The account has the gallery 2.4.5 in Documents** | the probe says current; no invitation. Under Restricted or AllSigned the host's own import of that copy stops at the publisher prompt (spike Q7), in Folio and everywhere. | Folio's binary-only 2.4.5 is first, so no prompt appears in Folio's panes. Other hosts are unchanged. |
| **The account has a newer upstream PSReadLine** (none exists today) | the probe says current | Folio's 2.4.5 in Folio's panes (Q2). The switch gives the account's back. |
| **The account has stock 2.0.0 only** | the corruption, unless they accepted the invitation | Folio's 2.4.5 |
| **The account has Folio's fork in Documents from an earlier Folio** | Folio's panes use it, and so does every other host | Folio's panes use 2.4.5 from local data. Other hosts keep the fork. The legacy row offers its removal, and the uninstall removes it (Q4). |
| **A locked-down policy** | Restricted / AllSigned: dark `Install`, a reason sentence | **No policy refuses the binary-only module** (measured under Restricted, AllSigned and RemoteSigned). The pane says nothing. Constrained Language Mode: no prefix and no integration, silent, exactly as INJECT-1. An application-control refusal of the DLL is the unmeasured case in §2.3 and §5.4. |
| **Shell integration switched off, or an uncomposable row** | the account's PSReadLine | unchanged: the account's PSReadLine |
| **A pwsh 7 pane** | in-box 2.4.5 with the repair | unchanged |
| **A nested `powershell.exe` typed in a 5.1 pane** | the account's PSReadLine | **the legacy line editor** under M (no colouring, no PSReadLine keys); under F with the initializer, Folio's. This is the one regression in this table, and Q1 decides it. |
| **macOS** | no row, no probe | unchanged; the bytes are gone from the executable |

---

## 5. Tests

Every new test goes red once under a named mutation (CONVENTIONS §三). Real-shell tests start PowerShell with `-NoProfile` and `HistorySaveStyle SaveNothing` set before any typed line, and they refuse to run otherwise, as the spike's driver did. They use the existing headless ConPTY machinery and no window, and they write only under `bt_testpath` directories.

### 5.1 ConPTY integration tests, `crates/bt-pty/tests/shell_integration_*.rs` and the `bt-app` real-shell cases

- **A 5.1 pane composed with the shipped module** reaches a prompt with `Get-Module PSReadLine` = 2.4.5 from the test's `psmodules` directory, OSC 133 `A,B` then `C,D;0,A,B`, and a first `$?` of True. The test runs once per policy (`-ExecutionPolicy Restricted / AllSigned / RemoteSigned` on the child) and asserts that none of the three prints a line before the prompt. Mutation: drop the prefix from the composition → 2.0.0.
- **The resize carve, on 5.1 with the shipped module.** `a_recall_carved_into_the_prompt_is_the_child_s_own_console_buffer` gets a 5.1 arm through `BT_PSREADLINE_MODULE_PATH`: the carve appears 0/5 times with the chord and 5/5 with it disabled. Mutation: restore the `-ge 2.4.6` gate with the version reading 2.4.5 → the chord becomes a no-op → carved.
- **The profile's own order**, emulated as the spike's Q6 did, with a dot-sourced temp file inside `-Command`. That file runs after the host's import, as a real profile does. It covers each row of §2.3's double-load table: `Set-PSReadLineOption` takes effect on Folio's module, and a by-name `Import-Module PSReadLine` leaves one module. The explicit-other-version row is **recorded, not asserted green**: the test states what happens, and the table in §2.3 is corrected to the measurement before the build merges.
- **A nested pwsh 7 from a 5.1 pane** keeps its in-box module (measured in the spike; pinned here).
- **The composition**, pure: Documents inserted only when `V` is unset or equals the machine value, never twice, after the row's own layer; no prefix when the loader is not composed (switch off, each refused argv shape, Constrained Language Mode); no prefix for pwsh 7; no prefix off Windows. Paths cover spaces, Chinese characters and each of the five quote characters.

### 5.2 Package and artefact tests, `scripts/release/package-tests.ps1` and the build script

- The zip carries no `PSReadLine` member (§3.1).
- The extraction refuses a package whose SHA-256 is wrong, and refuses an entry whose SHA-256 is wrong. Each is a unit test over injected bytes, the `conpty_sidecar` way. Mutation: skip the entry check → a tampered DLL is accepted.
- `check-psreadline-artefact.ps1` goes red when the README's hash, the table's hash or the manifest's `ModuleVersion` is moved alone, and when a `.psm1` or a `FormatsToProcess` line is added to Folio's manifest.

### 5.3 The retirement

- The settings round trip: a file with `psreadline_invite` set to each of the four values loads, and the key is absent from the file written back.
- No birth raises a dialog. The model has no invitation type left to construct, so the test is that the Terminal page's rows, for a machine with and without a legacy Documents copy (`BT_PSREADLINE_DOCUMENTS` sandbox), are exactly the expected set: the legacy removal row appears only when its remover would act. Mutation: show the row on the predicate's substring form, as INJECT follow-up F8 did → a row with nothing to remove.
- The census and `window_waits.tsv` lose the probe, the apply and the installed-copy rows. Their own gates (`every_bare_site_is_a_row`, `bt-source` census) are the tests.
- The uninstall's PSReadLine mark still removes a legacy copy in the sandbox (the existing `uninstall_tests` case, kept green).

### 5.4 The Mac, and the VM rows

- **macOS**: the composition adds nothing to any pane's environment, and the executable embeds no PSReadLine bytes (a `cfg(windows)` assertion on the embedded table's length). The Mac gate runs `bt-source` and the whole `bt-app` suite as usual.
- **Clean-VM rows** (cannot run on a developer machine): a Windows 11 with Smart App Control on, and a Windows 10 with an AppLocker DLL rule, each opening a 5.1 pane. They record whether Folio's DLL loads and what the pane shows. The result is written into this note as a revision before the release.

---

## 6. Build ticket outline (L)

One branch, four commits, merged together. Each step keeps the tree green.

| Step | What | Files | Size |
|---|---|---|---|
| 1. The artefact | Fetch the 2.4.5 package once (the step's author, with the coordinator's approval, never a build). Record its SHA-256, entries and signatures. Add `vendor/psreadline/` with the README, the extraction with its pinned table in the build script (a `psreadline_artefact` module beside the extraction, on the `conpty_sidecar` model), Folio's `assets/psreadline/PSReadLine.psd1`, `check-psreadline-artefact.ps1`, the notices rows and `tracked-binary-files.tsv`. The old `assets/psreadline/2.4.6/` stays until step 3, because the legacy row's stamp check still reads it. | `vendor/psreadline/*`, `crates/bt-app/build.rs`, `crates/bt-app/src/psreadline.rs` (the table, `SHIPPED_VERSION`), `licenses/*`, `scripts/ci/*` | M |
| 2. The pane | The module writer on the birth worker (`update_trial` writer, atomic replace, one notice, retry on the next birth, prune at launch). The pure `PSModulePath` composer and its declaration on the command. The `folio.ps1` gate removal. The tests of §5.1. | `shell_integration.rs` (composition, writer), `pty_door` birth path, `scripts/shell-integration/folio.ps1`, `crates/bt-pty/tests/shell_integration_script.rs` and the `bt-app` real-shell cases | M |
| 3. The retirement | Everything in §2.5's first table; the legacy removal row; the settings key retired; the census, window waits and ARCHITECTURE rows; `assets/psreadline/2.4.6/` reduced to what the legacy stamp check needs (only the stamp string and the nine byte hashes, not the bytes). | `psreadline.rs`, `restore.rs`, `runtime/first_run.rs`, `settings.rs`, `first_run.rs`, `i18n.rs` (retired strings in both columns, the new row's English with `// zh: pending`), `bt-persist/src/settings.rs`, `main.rs` | M |
| 4. The documents | `docs/RULES.md` §38 **rewritten**: the ticket changes that rule's text, and says so. `docs/shell-integration.md` ("Default shell", "PowerShell 7 and Windows PowerShell 5.1": no more `Install-Module` advice). `docs/ARCHITECTURE.md`, `docs/BT-ENVIRONMENT.md` (`BT_PSREADLINE_PROBE` goes). One DESIGN entry. `CHANGELOG.md` Changed/Removed in a reader's words. The ledger marks T-FIRST-RUN-MERGE and T-POLICY-FIRST-RUN superseded. | docs only | S |

**What lands behind a headless seam.** The ticket expects "the Profiles page roads are `Runtime<B>` now". At `9021e411`, `Runtime` in `main.rs` is not generic over a backend. The seams this build tests through are therefore:

- the pure functions: the `PSModulePath` composer, the extraction over injected bytes, the legacy row's predicate over an injected disk answer;
- the sandbox doors: `BT_PSREADLINE_DOCUMENTS` for the legacy copy, and a test-only local-data root door for the module writer (one name, in BT-ENVIRONMENT's test-only section);
- the existing ConPTY real-shell machinery.

No step needs a window.

---

## 7. Owner questions

**Q1. Which binary goes into Folio's 5.1 panes?** The licence is BSD 2-Clause (not MIT) either way, and the notices row already exists.
- **(M, recommended)** Microsoft's signed PSReadLine 2.4.5 from the PowerShell Gallery, kept as a pinned package like ConPTY. The fork retires, and the resize anchor is repaired by `folio.ps1`'s existing 2.4.x repair. Cost: a nested `powershell.exe` typed inside a pane gets the legacy line editor.
- **(F)** Folio's fork `2.4.6-bt.2`. It needs a fork change (define `PSConsoleHostReadLine` on import) to spare nested shells, publishing its source branch, and a decision on signing it with Folio's certificate. It is an unsigned DLL in `powershell.exe` until then.

**Q2. Whose PSReadLine wins in a Folio 5.1 pane?**
- **Folio's always (recommended):** one tested version, and no publisher prompt from an account's signed gallery copy under Restricted or AllSigned.
- **Folio's unless the account's first-found copy is newer:** this costs a disk read of manifests at each birth, and it runs a version whose repair is unproven.

**Q3. What replaces the invitation?**
- **Nothing (recommended):** the switch "Shell integration" already gives the account's editor back.
- **A Terminal page row "Windows PowerShell line editor: Folio's / this account's":** this keeps the account's editor with Folio's integration on.

**Q4. The copy an earlier Folio wrote into Documents.**
- **Leave it, with a removal row while it is there and the uninstall verbs (recommended):** this is the `$PROFILE`-line precedent; Windows Terminal and the other hosts keep what they have.
- **Remove it at the first launch of this build without asking:** the other hosts return to 2.0.0.

---

## 8. What this note did not measure, and who does

- **An explicit other version demanded by a profile** (§2.3, last row of the table): the build measures it in §5.1 and corrects the table before merging.
- **Application-control refusal of the DLL** (WDAC, AppLocker, Smart App Control): §5.4's VM rows.
- **The 2.4.5 package's exact entries, sizes and signatures**: step 1 records them. Two things suggest the layout: the fork, which mirrors upstream 2.4.x (five binaries, three script files, `Changes.txt`, `License.txt`), and the spike's `Save-PSResource` copy of 2.4.5 (Q7, "Microsoft-signed, no Mark-of-the-Web"). pwsh 7.6's in-box 2.4.5 DLL is netstandard2.0 with no public key ("What the DLLs are built for").
- **pwsh 7 under AllSigned**: the host's own publisher prompt arrives before any Folio code runs. That is unchanged by this ticket and outside it.
