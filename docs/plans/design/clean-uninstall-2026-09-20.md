# Leaving no trace: how Folio is removed, on every channel

Status: **revision 2, 2026-09-20, after three independent reviews** (Codex: RETHINK the ownership and channel mechanics, not the shape; Kimi and GLM: GO WITH CHANGES; all three in docs/plans/review/clean-uninstall-review-*-2026-09-20.md). **Section 6 at the end supersedes sections 2 to 5 wherever they disagree**; revision 1 stands unchanged above it so the reviews can be read against it. Original status: design for review, 2026-09-20. Owner's principle (2026-09-20): *installing and uninstalling must cost the user nothing; "before uninstalling, please first …" is a defect, not a solution.* The owner has approved the three-layer shape below and asked that it be reviewed before tickets are cut.

Evidence base: a read-only inventory of everything the shipped product writes outside the folder it runs from (2026-09-20; 24 rows, each with file:symbol). Its findings are restated in §1; the inventory itself is not in the repository.

## 1. What Folio leaves outside its own folder

Three classes. The class decides the rule.

**X — edits to files that belong to OTHER programs** (must always be undone):
- Agent hooks. Claude Code: 18 entries under `hooks` in `~/.claude/settings.json` (or `$CLAUDE_CONFIG_DIR`), each `"<abs>\folio.exe" attention claude-code:<event>`, `async: true`, covering `UserPromptSubmit`, `PostToolUse`, `Stop`, `PermissionRequest` and more (`attention_hooks.rs`). Codex: `notify` in `~/.codex/config.toml` (`attention_codex.rs`). Copilot: `~/.copilot/hooks/folio.json` (`attention_copilot.rs`). Each has a `remove_from`, reachable ONLY from the Settings row.
- The PowerShell `$PROFILE` line `. "$env:APPDATA\Folio\shell-integration\folio.ps1"` (`shell_integration.rs:add_to_profile`). **No code anywhere removes it.** bash and zsh write nothing into rc files (`--init-file`, per-process `ZDOTDIR`) — the clean design, kept.
- The patched PSReadLine 2.4.6, nine files under the user's PowerShell modules (`psreadline.rs`); removable from its Settings row only.

**R — registrations with the system:**
- Classic Explorer verb (HKCU registry, two trees) and the sparse MSIX package `WeiyiShi.Folio`. Removable non-interactively since 2026-09-20 by `folio --remove-explorer-menu` — which no channel calls.
- Windows toast identity `HKCU\Software\Classes\AppUserModelId\Folio.Terminal` — deliberately never removed today.
- macOS notification grant (held by the OS, per bundle id).

**O — Folio's own data:** `%APPDATA%\Folio` / `~/Library/Application Support/Folio` (settings, session, profiles, pins, schemes, shell-integration scripts, diagnostics, hang reports, update-check state, `settings.json.rejected-*`); **`%LOCALAPPDATA%\Folio\WebView2`** (cache and COOKIES — a different root, easy to miss); macOS `~/Library/{WebKit,Caches,HTTPStorages}/io.github.lulu-loopp.folio`, Preferences plist, Saved Application State; `%TEMP%\folio\clipboard`, `%TEMP%\folio-panic.log`. Runtime-only objects (mutex, pipes, sockets, hotkey) already vanish with the process.

Confirmed absent: Start-menu shortcut, URL scheme, file association, login item, LaunchAgent.

What happens today when the folder is simply deleted — the ranking that drives this design:
1. **Agent hooks point at a missing `folio.exe`**: every turn and every tool call of an agent the user still uses daily spawns a program that is not there.
2. **The `$PROFILE` line** survives everything; once `%APPDATA%\Folio` is also gone (a careful user, or Homebrew's `zap`), **every new PowerShell opens with an error**.
3. The Explorer menu keeps a dead row and Folio stays listed in Settings ▸ Apps (the sparse package).

## 2. The shape: three layers

### Layer 0 — every mark degrades harmlessly when Folio is gone (the foundation)
No uninstall path can be relied on: a zip user deletes the folder, a Mac user drags the app to the Trash, winget's zip type has no hook. So the FIRST rule is about the marks themselves: **a mark Folio leaves in another program's file must be inert when `folio.exe` (or the file it names) is missing — silent, fast, and never an error in that program.**
- `$PROFILE`: the line becomes a guarded load (`if (Test-Path <script>) { . <script> }`), written as ONE line carrying the same recognisable marker so it can still be found and removed. Existing installs are upgraded to the guarded form the next time Folio runs (same atomic-write-with-backup discipline as today).
- Agent hooks: the command must do nothing, quietly, when the executable is absent. OPEN DESIGN POINT (§5 Q1): each agent runs hook commands differently (shell vs direct exec; how a spawn failure is surfaced). The candidate forms are (a) a shell-level existence guard inside the command string, per platform and per agent; (b) a tiny stable launcher outside the app folder — rejected below; (c) accept the agent's own handling where it is already silent (`async: true` may make Claude Code swallow the failure — UNVERIFIED). Whatever the form, it must not slow the agent's turn when Folio IS present.
- PSReadLine patch: inert by nature (it is a working module); stays until removed by layer 1.
- Explorer menu / sparse package: cannot be made inert from outside; covered by layer 1 and by the existing self-repair on next launch.

### Layer 1 — one non-interactive door, and ways to reach it without reading documentation
`folio --uninstall-cleanup` — no window, no hand-off to a running instance, bounded in time, one line per mark on stdout saying what it removed / left and why, non-zero exit only when a removal was attempted and refused. Default: undo every X and R mark **that belongs to this copy** (the ownership test `explorer_menu` already has, generalised: a hook or a profile line naming ANOTHER existing copy's path is left alone; one naming a path that no longer exists is removed). `--purge` additionally deletes class O, on BOTH roots on Windows and all five locations on macOS; never by default — a user's settings are theirs. `--remove-explorer-menu` remains as the narrower door it is.
The only NEW removal code is the `$PROFILE` line remover; everything else calls functions that exist.

Entrances, from most to least discoverable:
1. **In the app: Settings ▸ About ▸ "Uninstall Folio…".** Shows exactly what will be undone and what will be kept, a checkbox for "also delete my settings and data", then runs the cleanup and removes the application: on Windows a detached system command deletes the folder after Folio exits (a running executable cannot delete itself); on macOS the app moves itself to the Trash. For a package-managed copy (layer-1 marker, below) it instead says which command to run (`scoop uninstall folio`, `brew uninstall --cask folio`, `winget uninstall …`) — the package manager owns the files.
2. **Windows Settings ▸ Apps.** *Whoever leaves a mark also leaves the door to remove it:* the first time the user turns on ANY integration that writes outside the folder (hooks, `$PROFILE`, Explorer menu, PSReadLine patch), Folio also registers a per-user Uninstall entry (HKCU) whose command runs the cleanup and then deletes the folder. A purely portable use — no integration on — registers nothing. The entry is itself an R mark: the cleanup removes it, and Folio's existing "the folder moved" self-repair covers it.
3. **`uninstall.cmd` in the archive** (and its macOS counterpart is unnecessary: entrance 1 plus the cask hook). A two-line wrapper over the same door, for people who look in the folder.
4. **Package managers call the door themselves.** scoop: `pre_uninstall` → `folio --uninstall-cleanup`; and `post_install` writes the **channel marker** (one text file beside the executable naming the channel) — Folio reads that fact and never infers a channel from its path. Homebrew cask: `uninstall_preflight` (runs while the binary still exists) → the door; `zap` extended to the five macOS locations. winget (zip type): no hook exists — covered by layers 0 and 1.2 until the package type changes.
The marker also answers two other questions with the same fact: a managed copy does not self-update, and (owner's ruling 2026-09-20) the first-run card pre-ticks the Explorer menu only where an uninstall hook exists.

### Layer 2 — a Windows installer (optional, decided later)
With layers 0 and 1 the zip is clean enough; an installer is no longer REQUIRED for uninstalling. Its remaining value: a wizard and Start-menu shortcut, the conventional winget package type, a more finished first impression for the public launch, and — the substantive one — a much simpler updater ("download the signed installer, run it silently, restart" instead of Folio's own unpack-replace-recover). It is therefore decided together with the updater design (0.4.5), not here. If built: a per-user installer under `%LOCALAPPDATA%\Programs\Folio`, no elevation, signed with the existing certificate; not full MSIX (container and file virtualisation fight shell integration, the PTY and editing other programs' files).

## 3. Rules this design commits to
1. A mark in another program's file is inert without Folio. (Layer 0.)
2. Every mark has a non-interactive undo owned by the module that wrote it; one door calls them all.
3. Undo only what is THIS copy's; a mark naming a path that no longer exists is nobody's and is removed.
4. User data is never deleted without an explicit request, and when requested, ALL of it is (both roots; five macOS locations).
5. Facts about how Folio was installed are written down by the installer/package at install time and read; never inferred from paths.
6. Nothing here may slow an agent's turn or a shell's start when Folio IS installed (budget: the guarded `$PROFILE` line costs one `Test-Path`; a hook command's guard costs no extra process).

## 4. Slicing
- **0.4.3** — T-A: guarded `$PROFILE` line + silent upgrade of existing installs + the line remover. T-B: agent hooks inert without the executable (starts with the investigation in §5 Q1). T-C: `--uninstall-cleanup` / `--purge`, the ownership rule generalised, scoop `pre_uninstall` + channel marker, cask `uninstall_preflight` + `zap` list, `docs/install.md` "Uninstalling" rewritten around ONE command.
- **0.4.4** — T-D: Settings ▸ About ▸ "Uninstall Folio…" (both platforms). T-E: the Settings ▸ Apps entry registered with the first outside-writing integration. T-F: `uninstall.cmd`.
- **0.4.5** — installer decision, with the updater.

## 5. Open questions for review
Q1. How does each agent (Claude Code, Codex, Copilot CLI) execute a hook command and surface a failure — through a shell (which?), synchronously or not, with what on screen? What is the cheapest command form that is silent when `folio.exe` is missing and adds no process when it is present, on Windows, macOS and Linux? Is `async: true` already enough for Claude Code?
Q2. Is registering a Settings ▸ Apps entry from a portable zip acceptable practice on Windows, and what must the entry carry (DisplayName, Publisher, DisplayVersion, InstallLocation, UninstallString, QuietUninstallString, NoModify) so that it behaves — including after the folder is moved, after an in-place upgrade to a new zip, and with two copies on one machine?
Q3. Self-removal: the exact, safe form of "delete this folder after I exit" on Windows (working directory, quoting, a path with spaces or non-ASCII, the folder being the current directory of some shell, antivirus heuristics against a program that spawns `cmd /c rd /s /q`); and whether moving itself to the Trash is permitted for a notarised, non-sandboxed macOS app without a prompt.
Q4. Ownership for X marks: a hook or profile line names a path. Two copies of Folio on one machine (the owner's own set-up) — which copy's cleanup may remove which line, and what does the SECOND copy do when it finds the first copy's hook already installed?
Q5. `--purge` and the WebView2 user-data folder: can it be deleted while another Folio (or a WebView2 process that outlived it) still holds it, and what is reported when it cannot?
Q6. What is missing from §1? (The inventory was made by reading code, not by diffing a machine before and after.)
Q7. Does anything here conflict with the package managers' own policies (scoop bucket rules on `pre_uninstall` scripts, Homebrew cask audit on `uninstall_preflight` and `zap`, winget validation for an app that registers its own Uninstall entry)?

## 6. Revision 2 — what the reviews changed (this section rules)

All three reviewers kept the shape — degrade first, one door second, installer last — and all three broke the same parts of layer 1. What follows is the coordinator's ruling on their findings, applying the repository's stop rule: a finding changes the design when it is reachable in ordinary use and breaks a hard requirement (a broken shell, a complaining agent, lost settings, deleted user files) or the scenario this design exists for; the rest is recorded.

### 6.1 Corrections of fact
- **Stale agent hooks are already quiet.** Verified from the agents' own documentation and source (Kimi Q1, Codex Q1): Claude Code suppresses the completion notice of an `async: true` hook by default (it shows only in verbose/debug); Codex's `notify` is a direct exec with null stdio whose spawn failure is a log line; Copilot logs and skips Folio's non-gating events. So revision 1's harm ranking was wrong: **what users FEEL today is the bare `$PROFILE` line (an error at every PowerShell start once the data folder is gone) and the dead Explorer row; the hooks are untidy, not noisy.** T-B shrinks accordingly.
- **"The only new removal code is the `$PROFILE` remover" was false.** Every agent remover recognises Folio's entries by MARKER (`attention claude-code:` and friends), never by executable path — so copy A's cleanup removes copy B's live hooks, and copy B's install silently replaces A's (three reviewers, file:line in each report). Ownership-by-path is new code in three modules.
- Inventory gaps: the legacy `%APPDATA%\BetterTerminal` root; SIX macOS data locations, not five; the Windows toast identity has no remover; macOS `NSServices` can stay cached by Launch Services after the bundle is gone; OS-written keys (`…\Notifications\Settings\Folio.Terminal`, the sparse package's AppModel entries) are outside Folio's control and are stated as such.

### 6.2 Two kinds of mark, two rules (replaces rule 3)
- **Per-copy marks** carry an executable path: agent hooks, the Explorer registrations. Cleanup removes one only if its path is THIS executable (compared as paths, as `explorer_menu::same_path` does) or names a file that no longer exists. A mark naming ANOTHER existing copy is left, and INSTALLING over it requires an explicit take-over rather than a silent replace.
- **Per-account marks** carry no copy's path: the `$PROFILE` line (it names the shared data folder) and the patched PSReadLine module. They belong to the account, not to a copy; both copies share `%APPDATA%\Folio` and therefore one answer to "is shell integration on". Rule: layer 0 makes them harmless if left; cleanup removes them and says so; a second copy that finds the integration gone offers it again the ordinary way. No reference-counting ledger — two copies on one account is the owner's own set-up, not an ordinary user's, and the guard already makes the worst case a missing convenience rather than an error. (Codex's install-UUID ledger is recorded, not adopted; it returns if an installer with per-copy identity is built.)
- **Where a mark was written is recorded when it is written** (Kimi B7, Codex): the resolved `$PROFILE` path(s) — PowerShell 5.1 and 7 have DIFFERENT profile files and Documents may be redirected to OneDrive — the PSReadLine module root, and the agent config roots in force at the time (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `COPILOT_HOME`). One small file in the data folder; the door reads those facts and ALSO probes today's locations, and reports each file it touched.

### 6.3 Layer 0, as revised
- `$PROFILE`: the line is written in ONE exact managed form (guarded load + a unique marker comment); recognition and removal match that exact form or the exact legacy form — never "a line containing `folio.ps1`", which could be the user's own code (Codex). **Migration of existing installs happens at Folio start-up**, not when a PowerShell pane happens to open, across every recorded and probed profile file. Read-only, locked, symlinked or oddly encoded profiles: refuse, report, leave the file byte-identical.
- Agent hooks: no shell guard strings (unwritable across `sh`, Git Bash, PowerShell and direct exec — Kimi). Instead: install in the agent's DIRECT-EXEC form wherever it has one (Claude Code `command` + `args`, Copilot `exec` / `args`; Codex already is), which also ends the quoting hazard Codex found (Claude Code on Windows now runs shell-form hooks through Git Bash, where a path containing `$` or a backtick is expanded); keep `async`. Refuse to install any per-copy mark when the executable path is under macOS App Translocation or when `current_exe` is unavailable (today's fallback to a bare `folio.exe` becomes a refusal).

### 6.4 Layer 1, as revised
**The door.** `folio --uninstall-cleanup`: exit 0 = the machine is as asked; 1 = a removal was attempted and refused (file locked, malformed, unknown schema — left byte-identical, named on stderr); 2 = a Folio instance is running (the door refuses to act: scoop runs `pre_uninstall` BEFORE its own running-process check and then abandons the uninstall — without this, marks vanish while the app stays). `--purge` additionally requires that no Folio or WebView2 process holds the data, covers both Windows roots, the legacy root and all six macOS locations, and reports what it could not delete.

**Addendum 2026-09-20 (T-C1 closure review, findings 1–5, 7a).** Four rules the door
keeps that this section did not state, and one promise it cannot keep:
- **An uninstaller creates nothing.** A data root that does not exist means "no marks"
  for every per-account row; the door reads the record read-only, writes no Off record,
  no lock and no root, and the root is still absent when the run ends.
- **`BT_UNINSTALL_ROOT` is a test instrument.** A shipped build does not read it: a
  redirected production cleanup would report every real mark "not present" and exit 0.
- **A recorded path is data, not authority.** Every path read from `integration-marks.json`
  is checked before use, in every build: absolute, no `..`, never a filesystem root, and
  the kind of place its mark may name. Refused paths are named; exit 1.
- **Exit 2 names the file that is held** (basename), the rule itself unchanged: any
  process holding any file in a data root stops the whole run.
- **macOS purge cannot detect held data.** `probe_file` is a no-op off Windows, so this
  section's "requires that no Folio or WebView2 process holds the data" is unmet there.
  The door does not guess: a macOS `--purge` states that it cannot tell and that Folio
  must be quit first (the flock still covers a second native Folio; unlinking an open
  file is safe on Unix, so this is a promise gap, not data loss).
- A purge row reports what was observed at the check that decided the deletion, so a root
  created after the preflight reads `removed` rather than `not present`.

**Entrances, reduced:**
1. `uninstall.cmd` in the archive and the rewritten `docs/install.md` — in 0.4.3, cleanup ONLY.
2. Settings ▸ About ▸ "Uninstall Folio…" (0.4.4): shows what will be undone and what is kept, closes windows, runs the door, then **reveals the application folder (Windows) or the app in Finder (macOS) for the user to delete**, or names the package manager's command for a managed copy.
3. Package managers: scoop `pre_uninstall` guarded by `$cmd -eq 'uninstall'` (scoop also runs it on UPDATE — Codex, verified); Homebrew `uninstall_preflight_steps` with declared `writable_paths` and `must_succeed: false`, after proving whether it fires on upgrade; the channel marker lives OUTSIDE the signed macOS bundle (writing into it breaks the signature); winget (zip) has no hook.

**Withdrawn from this design: self-deleting the folder, and registering a Settings ▸ Apps entry from a portable copy.** Two reviewers asked to hold the Apps entry (winget already registers one for portable packages; the sparse package already produces a second "Folio" row; two copies collide on one key; a moved folder leaves a dead row), and recursive deletion of "the folder" destroys user files when the zip was unpacked beside them. Doing either properly needs a manifest of shipped files, a per-copy identity and a signed helper — that IS an installer. So these two belong to **layer 2**, and the reviews strengthen the case for building it; the decision still travels with the updater (0.4.5).

### 6.5 Existing users, and people who already deleted Folio
- 0.4.3's release note carries one plain line: run 0.4.3 once and everything Folio has written becomes harmless if Folio is later deleted; `folio --uninstall-cleanup` takes the integrations out; `--purge` also removes settings.
- A short recovery page in `docs/` for someone who deleted an older Folio and now sees a PowerShell error at start-up: the exact line to remove from which file(s), and the hook entries to delete — copy-and-paste, no Folio required.

### 6.6 Tests touch no real machine
The repository already has sandbox doors (`BT_POWERSHELL_PROFILE`, `BT_PSREADLINE_DOCUMENTS`, the agents' config-dir variables). Every new remover and migrator is a pure decision over injected file contents plus those doors: LF / CRLF / BOM / UTF-16 profiles; read-only, locked, symlinked, malformed and future-schema agent files; two copies, a moved folder, dead owners, take-over, rerun (idempotence), partial failure. Registry, MSIX, scoop and winget behaviour is exercised only in a disposable VM or Windows Sandbox, never on the owner's machine.

### 6.7 Slicing, revised
- **0.4.3** — T-A `$PROFILE`: exact managed form, start-up migration, multi-profile remover, the marks record. T-B agent hooks: direct-exec form, ownership by path, take-over instead of silent replace, translocation / missing-executable refusal. T-C1 the door with its exit codes, running-instance refusal, `--purge`, the toast identity + legacy root + six macOS paths, `uninstall.cmd`, install.md, the recovery page, the release-note line. T-C2 scoop and cask hooks (gated on uninstall, proved against upgrade) and the channel marker.
- **0.4.4** — Settings ▸ About ▸ "Uninstall Folio…" (cleanup, then reveal).
- **0.4.5, with the updater** — the installer decision, which now also owns the Settings ▸ Apps entry and the removal of shipped files.

Recorded, not adopted: an install-UUID ledger and reference counting for shared marks; a signed self-delete helper; byte-preserving rewrites of agent JSON (today's re-serialisation reorders keys — acceptable, and said in the release note).
