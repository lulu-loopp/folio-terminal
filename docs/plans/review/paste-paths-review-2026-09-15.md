STATUS: COMPLETE — phase 4 complete; 24 findings (7 blocking, 16 should-fix, 1 nit); citations and artifact scope verified.

# R-PASTE-PATHS — adversarial design review, 2026-09-15

Reviewed `docs/plans/paste-paths-design.md` on `docs/paste-paths-design`, HEAD `dee7d4b38fb2ea2bc3538116301436c6ffe0e60b`. `D:n` means line n of the design. `registry/` means `C:/Users/Weiyi/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`. Source shorthand: main.rs, input.rs, profiles.rs and shell_integration.rs are in crates/bt-app/src/; paths.rs is crates/bt-transcript/src/paths.rs; macos_files.rs, macos_services.rs, launch_pipe_unix.rs and instance.rs are in crates/bt-platform/src/. Other citations are worktree-relative.

Method: full design, earlier review format, cited code/callers, house rules and dependency backends read; primary upstream sources checked. No cargo build/test, app launch, clipboard mutation or native drag experiment. Examples are deductions from source/contracts unless identified as upstream reports. No code or design edits. Working evidence: `target/review-notes.md`.

## Verdict

**Fix these first. Rethink §2's literal/recipient contract and §3.1's macOS ownership plan.** Blocking findings can change filenames, expose executable input, disclose screenshots or deliver to a different input session. The proposed acceptance matrix misses several of them.

## Numbered findings

### 1. Blocking — the universal safe set changes paths and exposes PowerShell syntax

**Evidence:** D:286–300 permits bare backslash and every non-ASCII character; D:220–225 escapes only ASCII apostrophes. PowerShell recognizes smart quotation marks as syntax. [PowerShell quoting rules](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_quoting_rules?view=powershell-7.5).

**Trigger / breakage:** `/tmp/a\b.txt` is emitted bare and POSIX parsing consumes the backslash. A Windows name containing `’` can terminate the proposed single-quoted literal: legal filename component `a’;Write-Host PASTE_PROBE;#` exposes command syntax when the user submits the line. Non-ASCII whitespace also cannot be assumed lexically inert in PowerShell. Ordinary CJK letters are not the problem.

**Correction:** Specify literal encoding per grammar. Keep a shared safe subset only where proven; exclude POSIX backslash and handle PowerShell Unicode quotes/whitespace. Include Unicode delimiter attacks beside CJK/emoji, and verify argument identity, not pretty text.

### 2. Blocking — String and paste sanitization cannot preserve every accepted macOS name

**Evidence:** D:188–191 specifies `Option<String>`; D:757–759 requires invalid UTF-8. `macos_files.rs:33–42` explains lossy identity changes; `macos_services.rs:177–205` preserves URL bytes. But K144 uses `to_string_lossy` (`main.rs:8840`), `paste_text` takes `&str` (`110689`), and `input.rs:711–731` deletes controls and changes LF to CR. D:680–681 incorrectly says quoted paths have no controls/newline.

**Trigger / breakage:** POSIX names can contain LF, tab, ESC or invalid UTF-8. Quotes do not remove them. Sanitization changes the filename; without bracketing, LF becomes Enter-like CR, including in a bare agent path. Bypassing sanitization admits terminal controls and bracket terminators.

**Correction:** Check representability before quoting/writing. Visibly refuse paths the UTF-8 transport cannot preserve safely, or design a verified byte-safe shell literal encoding. Never substitute U+FFFD or send raw filename controls. Amend the invalid-UTF-8 acceptance promise; retain ordinary-text sanitization.

### 3. Should-fix — cmd quoting conflates its interpreter and one argv parser

**Evidence:** D:235–243 claims every cmd child uses the C runtime parser and doubled trailing backslash works for builtins. Microsoft scopes these rules to its C parser. D:303–310 acknowledges `%NAME%` but omits delayed `!NAME!`. [C argument parsing](https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments?view=msvc-170), [cmd options](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/cmd).

**Trigger / breakage:** `cmd /v:on` expands `"C:\a!NAME! b.txt"` inside quotes. `echo "C:\a b\\"` retains the extra separator; a raw-command-line consumer can too. Under CRT rules `"D:\"` contains a literal quote after consuming the backslash, not simply `D:` as claimed. N trailing backslashes in a CRT-quoted argument require 2N, not one extra slash. In a simple quoted cmd argument, `^` is normally literal; blanket caret escaping would introduce a defect.

**Correction:** Distinguish builtin/raw-command-line and CRT consumers. Explicitly handle or refuse `%` and delayed `!`. Resolve safe-bare-root versus quoted-trailing-root rules. Test actual builtin/native argv, expansion on/off and multiple trailing slashes. Drop universal cmd safety.

### 4. Should-fix — fish and nushell cannot share the complete POSIX encoder

**Evidence:** D:245–260 generalizes from POSIX apostrophe concatenation to fish and postpones nu. Fish single quotes interpret `\\` and `\'`; nushell supports double-quoted/raw strings, so apostrophe-containing paths are not intrinsically unspellable. [Fish quoting](https://fishshell.com/docs/current/language.html#quotes), [Nushell strings](https://www.nushell.sh/book/working_with_strings.html).

**Trigger / breakage:** Two literal backslashes in a quoted Mac filename collapse to one in fish; a backslash before a quote boundary can escape it. The isolated `'\''` sequence does not prove the entire encoder. A nu login shell receives incompatible syntax. D:279's “every shell” claim contradicts its nu exception.

**Correction:** Add fish backslash handling and a versioned nu encoder, or refuse unsupported cases. Retain POSIX encoding for bash/zsh/sh. Narrow “every shell” and test combined backslash/apostrophe cases.

### 5. Should-fix — launcher stem does not identify the current input reader

**Evidence:** `profiles.rs:799–835` specially handles `PowerShellSeven` and the first `FirstOf` candidate. `shell_integration.rs:248–260` asks WSL for its login shell and executes an unrestricted fallback. User profiles exist (`profiles.rs:1988`, `2145`). D:273–282 treats unknown Unix programs as only usershell; D:32–33 equates launch identity with the reader.

**Trigger / breakage:** `wsl.exe -e nu`, a PowerShell wrapper, `env`, or a custom Python profile defeats inference. Claude launched inside PowerShell retains the shell row; a nested shell inside an agent reverses the error. `pwsh.exe` and `powershell.exe` share basic quote syntax, but native argv marshalling differs across versions and `$PSNativeCommandArgumentPassing` modes. [PowerShell native arguments](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing?view=powershell-7.4).

**Correction:** Define spawn-time default versus actual-recipient guarantee. Use the resolved launch, retain PowerShellSeven handling, and give wrappers/unknowns a capability override or conservative refusal. Do not infer foreground programs from prose. A profile override has real use cases; test custom/nested programs, not only builtin IDs.

### 6. Should-fix — integration-off Git Bash loses the independent namespace

**Evidence:** `profiles.rs:3205` matches paths and integration: Windows+BashInitFile/ZshDotDir becomes Msys (`3214`), Windows+None becomes Windows (`3219`). `served_by` honors explicit integration (`853–856`). D:199–205 separates grammar from integration while reusing this result.

**Trigger / breakage:** Git Bash with integration None receives Windows spelling. Bare backslashes disappear; corrected quoting still does not produce the promised `/d/...`. This is an existing counterexample to D:213's “the day a real row is measured wrong.”

**Correction:** Derive insertion namespace from resolved filesystem/launcher capability independently of integration. State whether reading-direction derivation also changes or remains debt. Pin integration-off Git Bash as well as PowerShell.

### 7. Should-fix — the inverse knows no mounts and is not a universal round-trip

**Evidence:** `paths.rs:134–158` recognizes `/mnt/<letter>` before distro paths; `drive_mount_to_local_path` (`218–228`) knows no mount facts. Non-Windows `to_local_path` returns None (`185–188`). D:341–346 promises unmounted-drive fallback; D:714–717 requires round-trip equality for every corpus path. WSL allows disabling/changing automount. [WSL configuration](https://learn.microsoft.com/en-us/windows/wsl/wsl-config).

**Trigger / breakage:** An unmounted D: still becomes `/mnt/d/...`. `\\wsl.localhost\Ubuntu\mnt\d\x` becomes `/mnt/d/x`, then reads back as `D:\x`. General UNC/different-distro fallbacks have no inverse. Drive case normalizes. External sources can supply `\\wsl$\Ubuntu\...`; `paths.rs:235` excludes it only because the old producer never emitted it.

**Correction:** Define partial translation over a normalized domain. Separate lexical rules from mount facts: cache facts off the input path (a measured wslpath probe is an option), or disclose best-effort default mounts. Never inject `$(wslpath ...)`. Cover legacy alias, same/different distro, UNC, disabled/custom mounts, roots and literal tildes. Prefer absolute paths: quoted `~/...` does not expand home. Test supported round-trips and explicit failures separately.

### 8. Should-fix — Git Bash converts again after Folio

**Evidence:** D:314–332 treats `/d/...` as final. MSYS2 converts POSIX-looking arguments for native children; `MSYS2_ARG_CONV_EXCL` disables that globally/by prefix. Quotes do not disable that stage. [MSYS2 paths](https://www.msys2.org/docs/filesystem-paths/).

**Trigger / breakage:** MSYS tools accept `/d/Demo/a.txt`; with exclusions set, a Windows executable receives it unchanged instead of converted. A native agent inside Git Bash adds the same boundary. Folio's round-trip test cannot observe it.

**Correction:** Test MSYS/native consumers with exclusions unset/set. Do not change the user's environment to force success. Document fallback/unsupported behavior and when a forward-slash Windows spelling suits a known native recipient.

### 9. Blocking — the bare-agent rule has a concrete Codex counterexample

**Evidence:** D:262–270 says quotes make agents seek apostrophe-prefixed names; D:358–359 removes multi-file delimiters. Codex's `normalize_pasted_path` accepts surrounding quotes and uses shlex for POSIX paths, requiring one token. The composer attempts attachment from that normalized path. [Codex parser](https://github.com/openai/codex/blob/a8964cb1bad67bc26a826fb07d1bef99c6a3f008/codex-rs/tui/src/clipboard_paste.rs#L251-L286), [paste consumer](https://github.com/openai/codex/blob/a8964cb1bad67bc26a826fb07d1bef99c6a3f008/codex-rs/tui/src/bottom_pane/chat_composer.rs#L1248-L1270).

**Trigger / breakage:** Bare `/Users/ann/My Pictures/screen.png` fails that single-path parser; quoted succeeds. Windows spaced paths have a special case, so Windows success does not establish Mac behavior. Several paths in one paste are not several attachments. `a b.txt c d.txt` in prose loses boundaries.

**Correction:** Remove universal apostrophe failure. Separate prose references from automatic attachments and version expectations. Claude documents prose paths/`@`; Copilot documents `@` attachments, not a seven-agent bare-path grammar. Preserve delimiters and verify spaces/apostrophes/multiple files in Claude Code, Codex and Copilot. [Claude workflows](https://code.claude.com/docs/en/common-workflows), [Copilot attachments](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/overview). Model interpretation of prose is not deterministic filename parsing.

### 10. Should-fix — priority is a preference, not proof of source intent

**Evidence:** D:108–116 says image+text always means text and screenshot tools provide no text. D:640–644 says files+text receives text, contradicting files-first and Finder at D:101–104. ShareX #8501 documents an existing custom PerformActions image+path workflow; its combined builtin implementation is proposed, not a shipping default. [ShareX multi-format workflow](https://github.com/ShareX/ShareX/issues/8501).

**Trigger / breakage:** A screenshot source also supplying a path/OCR text never reaches the picture rung; raw spaced path text gets no file quoting. Finder file+leaf-name selects Files, contrary to the setting rationale. Text-first preserves ordinary Excel ranges and Word/browser text selections, but presence cannot distinguish those from screenshot text representations. Excel charts/copy-as-picture and browser images are different source gestures.

**Correction:** State the order as a preference with losses. Define how the reader obtains the picture when text is present; an explicit paste-as-picture action is one option without another persistent setting. Add versioned source/gesture/advertised-format/result fixtures for Explorer, Finder, Excel cells/pictures, Word text/images, browser selection/copy-image, snips and mixed screenshot workflows. Do not call the custom ShareX workflow its default.

### 11. Should-fix — first-rung semantics omit errors, emptiness and snapshot coherence

**Evidence:** D:83–90 gives no acquisition-error semantics. Windows returns Result and sometimes empty text (`lib.rs:5925–5964`); macOS public clipboard_text returns Result (`11358–11368`), not Option. Private `paths_on` collapses missing/unreadable lists to empty Vec (`macos_services.rs:181–209`). D:719–722 tests only presence.

**Trigger / breakage:** Empty/malformed Files may beat text; failed delayed PNG may suppress DIB; invalid text may become a picture file or abort depending on interpretation. Separate acquisitions can mix generations. RDP/WSLg exposes bridged/synthesized formats; clip.exe text differs from Linux GUI file copy. [Delayed rendering](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-operations), [WSLg architecture](https://github.com/microsoft/wslg).

**Correction:** Specify absent/present-empty/malformed/temporarily-unavailable and fallback within encodings versus between semantic rungs. Take a bounded coherent snapshot or detect generation change and cancel. Extract a privacy-safe shared decoder instead of blindly reusing the private service helper and its service-specific logs. Record host-visible WSL formats; do not infer file identity from arbitrary text URI lists. Correct Result/Option descriptions.

### 12. Should-fix — bracketed paste does not guarantee the resulting input line

**Evidence:** `main.rs:110695` and `input.rs:696–708` provide the envelope, not shell quote-context awareness. `input_line_needs_a_space_first` (`main.rs:8861–8870`) sees one cell. D:577–579 claims that recheck ensures a well-formed line. zsh bracketed-paste-magic can process widgets and requote the whole paste. [zsh paste widgets](https://zsh.sourceforge.io/Doc/Release/User-Contributions.html).

**Trigger / breakage:** An independently quoted literal pasted after `Get-Content '` or inside a zsh quoted argument changes the parse. Wrapped column zero loses the preceding character. zsh quote-paste can make a multi-file run one string, including trailing space. PSReadLine editing and PowerShell native argv are later stages, not properties of the envelope.

**Correction:** Limit the promise to a fresh argument boundary. Keep bracketed inheritance; do not disable it to bypass hooks. Document inside-token/quote behavior. Add default/custom zsh and Windows PowerShell 5.1/PowerShell 7 PSReadLine, bracketing on/off, mid-line/wrapped cursor and final argv/file-open checks. Formatter snapshots are insufficient.

### 13. Blocking — delayed pictures lack session identity and input ordering

**Evidence:** D:573–580 rechecks only leading spacing. Named-seat paste is synchronous and indexes the active tab (`main.rs:96290–96312`); modal/keyboard gating is separate (`96257–96269`). `CONVENTIONS.md:154–155` requires globally unique worker addresses and one shared-answer owner.

**Trigger / breakage:** Paste, then switch tabs, restart/close the shell, open a modal or press Enter during encoding. Completion can target a reused seat, leak behind a modal or append to the next prompt. Out-of-order jobs reverse paste order. One cell recheck solves none of these.

**Correction:** Capture window/tab/leaf, session incarnation and request sequence. Define cancellation on ownership/input-context changes and input ordering while pending, with feedback. Revalidate original target, modal gate and setting at completion. Apply this to delayed drops and clean up abandoned files through an owned path.

### 14. Blocking — the Mac probe targets the wrong class and omits hover updates

**Evidence:** D:399–414 says winit's view implements three selectors requiring class_replaceMethod. Registry `winit-0.30.13/src/platform_impl/macos/window_delegate.rs:367–430` implements them on **WindowDelegate**. Registration is on `window` (`666–669`). There are entered/prepare/perform/conclude/exited methods, no draggingUpdated. D:479–482 requires continuous feedback.

**Trigger / breakage:** Changing the assumed view does not replace the registered window/delegate route as claimed. Missing draggingUpdated leaves stale landings. Class-wide replacement affects all instances/future windows without a per-window lifetime contract.

**Correction:** Rebuild the probe around actual NSWindow/NSView/delegate dispatch. Evaluate an application-owned destination NSView in the existing hierarchy or a narrow upstream extension before class-wide replacement. Verify hit testing, responder/IME, wgpu/CAMetalLayer and embedded web panes. Specify entered/updated/exited/prepare/perform/conclude, native ABI return types, per-window teardown and AppKit-points-to-physical-coordinates conversion. [Apple destination contract](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/DragandDrop/Tasks/acceptingdrags.html).

### 15. Should-fix — disabling winit's Windows target disables OLE initialization too

**Evidence:** D:393–398 specifies with_drag_and_drop(false) plus RegisterDragDrop. Registry `winit-0.30.13/src/platform_impl/windows/window.rs:1167–1194` gates both OleInitialize and registration. `event_loop.rs:1262` still revokes on destruction. Registering twice on one HWND fails; ordinary COM initialization is insufficient. [RegisterDragDrop](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-registerdragdrop).

**Trigger / breakage:** Following the ticket removes required initialization. Missing a quake/auxiliary/new-window constructor leaves winit's target and causes DRAGDROP_E_ALREADYREGISTERED. Incorrect effects can tell a source that a move happened when Folio inserted only a name.

**Correction:** Own OLE STA initialization/balancing, target retention and every HWND registration. Define teardown against winit's unconditional revoke. Preserve owned payloads before releasing IDataObject/STGMEDIUM; bound callbacks and return only allowed non-destructive effects. Pin screen-to-client/DPI conversion, source masks, multi-window teardown, cancellation and reentrancy. Two targets cannot coexist on one HWND.

### 16. Should-fix — the table misstates the strip and omits nonterminal batch behavior

**Evidence:** D:433–475 calls internal strip drops Refused and new tabs deferred debt. `main.rs:28995–29004` calls that arm unreachable; `row_strip_landing` (`29555–29559`) supports StripAdopt/StripExtract outside row_verb. The table and commit at `88732` use a singular payload. D:498 only defines list insertion.

**Trigger / breakage:** Implementing this baseline can regress internal strip actions. Fifty files or mixed file/folder drops over preview/edge have no defined result: repeated retarget, fifty panes, one item or refusal. Hover cannot promise a definite commit. K144 directly moves focus (`main.rs:76921–76930`), contrary to D:507.

**Correction:** Describe layout/strip routing accurately; preserve internal verbs and identify any OS-only refusal. Define batch admission for one-item actions versus multi-item terminal insertion, mixed kinds, partial failures and limits. Use named-target insertion without K144's focus changes, while preserving those changes for K144.

### 17. Should-fix — picture drops are not automatically covered by clipboard decoding

**Evidence:** D:399–401 registers only file URLs on Mac; D:500–503 promises raw images. D:756 asserts Safari supplies public.png without a file URL. Apple's sample handles promises for Safari/Mail/Photos image drags. [Apple file-promise sample](https://developer.apple.com/documentation/appkit/supporting-drag-and-drop-through-file-promises?changes=_8).

**Trigger / breakage:** A promised Safari image is unsupported by §1.2, not a free success. Raw image+URL/text through the clipboard ladder selects text. PNG-only drag does not match file-URL-only registration and may never arrive.

**Correction:** Specify a drop admitted-type/action matrix while sharing decoders/insertion. Register raw image types, refuse promises before successful highlighting and measure browser/version-specific offers. Define picture drops on preview/edge too. Remove unconditional Safari success until measured.

### 18. Should-fix — TIFF is disabled and DIB alpha conversion is underspecified

**Evidence:** D:553–560 promises DIBV5/DIB/TIFF; T-PASTE-2 adds only bmp. `Cargo.toml:107` disables defaults and enables gif/jpeg/png/webp; registry `image-0.25.10/Cargo.toml:120` gates TIFF separately. BMP already has new_without_file_header explicitly for CF_DIB (`src/codecs/bmp/decoder.rs:534–541`). Alpha depends on compression/nonzero mask (`752–775`), not clipboard format ID.

**Trigger / breakage:** TIFF cannot decode with the proposed features. A synthetic file header needs correct offsets across headers/masks/palettes/profiles; fourteen bytes are not the whole algorithm. V5+BI_RGB is not automatically alpha-preserving in this decoder. Premultiplied channels treated as straight darken translucent edges; undefined legacy alpha can make opaque pixels transparent.

**Correction:** Add TIFF feature/dependency/lockfile/notices work. Prefer the audited headerless BMP entry. Specify layouts, orientation/padding, bitfields, zero/absent alpha, premultiplication and color-profile policy. Add known-pixel producer fixtures; reject unsupported layouts instead of emitting changed pictures. [BITMAPV5HEADER](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapv5header).

### 19. Blocking — per-picture caps do not bound acquisition or aggregate work

**Evidence:** D:548–568 accepts PNG after header/dimensions and caps source 64 MiB/RGBA 256 MiB. D:670–675 puts bounds in bt-app after native bytes are handed over. No job/queue/output/disk quota exists. `CONVENTIONS.md:34` requires boundary validation; `docs/DESIGN.md:86` and `318` govern bounded work/worker ownership.

**Trigger / breakage:** Native copying can allocate beyond the cap before rejection. Repeated legal 256 MiB jobs exhaust memory/disk. Valid-looking IHDR plus corrupt/truncated PNG becomes a successful filename. RGBA16 intermediates cost eight bytes per pixel; animation/multipage and decoder scratch are not bounded by one RGBA8 multiplication.

**Correction:** Bound native lengths before copying, checked dimensions before allocation, actual decode/output buffers, pending requests and aggregate bytes. Define single-frame/multipage policy and PNG validation beyond dimensions while retaining original bytes if desired. Specify saturation/cancellation, write/flush/close errors, partial-file deletion and disk pressure. Off-thread encoding does not bound delayed native retrieval; give acquisition a measured latency/failure contract.

### 20. Blocking — vetting misses existing permissions and parent redirection

**Evidence:** D:529–534 checks final symlink/uid and creates new Unix directories 0700. Its reference vets a **socket**, including exact 0600 (`launch_pipe_unix.rs:470–483`). Directory precedent `instance.rs:360–388` checks/repairs existing mode too. D:529 vets once per run despite D:791's no-link rule.

**Trigger / breakage:** A same-owner world-readable directory passes the listed checks; default file permissions may expose screenshots. Intermediate Folio symlink/junction redirects a final non-symlink directory. Replacement after first-use vetting redirects later creates/sweeps. create_new protects the final name, not parents. No Windows ACL/reparse counterpart to uid is specified.

**Correction:** Define Unix/Windows trust contracts including existing permissions/ACLs and intermediate app directories. Anchor creates/cleanup to a verified directory handle with relative/no-follow operations or equivalent race resistance. Use owner-only files too. Start from trusted platform temp discovery; do not indiscriminately reject Mac's standard /var alias. Vet cleanup and writes; fail closed with sanitized errors.

### 21. Should-fix — seven-day startup cleanup guarantees neither history nor maximum retention

**Evidence:** D:584–598 claims seven days exceeds any session and preserves history; sweep is startup-only and matches clip-*.png. D:518 chooses system temp.

**Trigger / breakage:** A live eight-day session still needs its file; another Folio startup removes it. Continuous Folio retains files for weeks. OS cleanup can remove them earlier. User-created clip-family.png matches the supposed ownership test. Timestamp source, clock changes and DST are unspecified.

**Correction:** State best-effort retention, no history-validity guarantee and no seven-day maximum. Specify age clock, exact owned-name grammar, cutoff, failures and active-write exclusion across processes. Choose quota/periodic cleanup or explicitly accept startup-only retention; cleanup is not clipboard watching. Explain how a reader retains a picture permanently before relying on its history path.

### 22. Should-fix — privacy and the one switch have contradictory scope

**Evidence:** D:618 says only paste writes; §3.4 writes drops. D:780–783 forbids persisting clipboard content, while §4 persists PNG and §6.2 requests BT_PTY_DUMP. `docs/PRIVACY.md:186–188` discloses terminal dumps; echoed paths enter them. D:772 says nothing has an address although §2.5 accepts UNC and §3 opens dropped files. D:633–645 defines the switch only for clipboard pictures.

**Trigger / breakage:** Drops may still write with the switch off; pending jobs may write after disabling it. Remote/redirected TEMP writes screenshots over a share; network-backed preview drops can cause filesystem network I/O. Folder deletion does not remove shell history, agent records, recordings or recipient copies.

**Correction:** Make the switch cover all Folio-created clipboard/drop picture files and pending work; existing-file insertion remains separate. State the storage exception and distinguish feature diagnostics from opt-in recordings/child history. Disclose drops, original PNG metadata, cleanup limits and downstream handling in both languages. Narrow network wording to no feature-initiated upload or enforce local storage/read boundaries. The current Mac package is explicitly unsandboxed (`packaging/macos/entitlements.plist:20–23`); do not invent a current App Sandbox blocker. Use platform temp discovery; probe child/sandbox access and TCC refusal without fallback copies. [Apple temp directories](https://developer.apple.com/documentation/foundation/nstemporarydirectory%28%29).

### 23. Should-fix — sizes and gates do not support independently shippable tickets

**Evidence:** D:699 calls pure tests the whole specification. D:744–764 omits native argv, recipient versions, async ownership, storage attacks and decoder fixtures. D:805–847 sizes M–L/M/L and assigns T-PASTE-1 all §6.1 except temp tests, including picture/i18n cases belonging to T-PASTE-2. Only Claude appears in agent manual acceptance.

**Trigger / breakage:** Snapshots pass while fish/MSYS/PowerShell/paste widgets change argv. Presence-only fixtures miss malformed/deferred sources. T-PASTE-2 M hides two native readers, decoder work, secure lifecycle, delayed delivery and settings/privacy. Mac feasibility is last despite determining the abstraction.

**Correction:** Run the bounded drop probe early without preselecting swizzling. Treat T-PASTE-1 as **L** until contracts narrow. Split T-PASTE-2 into acquisition/decoding and storage/delivery/settings, or size **L–XL**. Make Windows/Mac adapters and batch routing separately reviewable within **L–XL** T-PASTE-3. Assign applicable gates and explicit refusal for unshipped lanes. Require byte-to-edit-buffer-to-argv/file-open validation, consistent with `CONVENTIONS.md:79–83`.

**K144 belongs in T-PASTE-1:** it shares paste_text and needs the same encoder. Its defect exists on Windows too: `main.rs:8841` quotes only whitespace, and PowerShell double quotes expand `$` even when spacing causes quoting. Do not defer repair behind pictures/drops. Preserve K144 focus behavior while making drop insertion target-specific. If 0.4.1 shrinks, ship this correction and supported file-clipboard cases before claiming every shell/source.

### 24. Nit — the survey and some anchors overstate their evidence

**Evidence:** D:141–152 infers usage/quietness from WT report timing; this is not adoption data. Ghostty #10517 is specifically **SSH transfer** and links a proposed implementation, not the full local clipboard state. The WezTerm Lua recipe has no URL/revision. D:278 cites `profiles.rs:1541` for usershell, but it is the zsh seed; usershell ID is `1512`. [WT #16627](https://github.com/microsoft/terminal/issues/16627), [Ghostty #10517](https://github.com/ghostty-org/ghostty/discussions/10517).

**Breakage / correction:** Implementers cannot reproduce the survey or distinguish released behavior from workarounds. Remove the adoption inference; cite URLs/revisions and narrow Ghostty to SSH. Mark the unlocated WezTerm recipe unverified. Keep WT regression/apostrophe history as examples, not proof of the grammar table. Correct anchors; nearby doc-comment line numbers alone are not separate defects.

## Top three risks in implementation order

1. **Literal and recipient correctness — T-PASTE-1.** Resolve byte/control handling, grammar/namespace capability and actual argument/attachment identity. Findings 1–12. Absorb K144 here.
2. **Picture ownership and bounded lifetime — T-PASTE-2.** Bound acquisition/decoding, protect storage, preserve input/session identity and align setting/cleanup/privacy. Findings 13 and 18–22.
3. **Native drop ownership and batch admission — T-PASTE-3.** Probe early; implement after insertion/storage contracts settle. Verify Mac destination, Windows OLE, coordinates/effects/focus and nonterminal batches. Findings 14–17.

## Remaining release gates

Exact Explorer/Finder selection ordering/formats, Office/browser/snipping-tool versions, PSReadLine buffers, Claude/Copilot spaced references, real DIB alpha, Safari promised/raw offers, Mac TCC/child access and native callbacks require platform probes. None was executed in this docs-only review. Unsupported universal claims are findings; an unrun probe is not evidence that every named source fails.
