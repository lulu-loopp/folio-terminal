# KP-0 review — keyboard-protocol design note (2026-09-29)

Reviewed branch `design/keyboard-protocol` at `b2d296d1` against main `38b5743b`, the KP-0 brief and author report. No code was built or changed.

## Verdict: not yet

The transport investigation and most of the encoder table are strong, but the note is not ready to adopt as the implementation contract. Three blockers remain:

1. T-KEYBOARD-PROTOCOL, as scoped, **does not fix issue #13 for its Windows-native Codex reporter**. Q1 still requires an owner decision and the issue/release claim must follow that decision.
2. The OSC 133 `C`→`A` restore rule is not ownership-safe. A primary-screen TUI or REPL that enables the protocol and launches an integrated subshell can have its live mode removed by the nested prompt.
3. The grammar/test contract contains concrete errors: `CSI > m` must reset xterm key-modifier resources, explicit `CSI < 0 u` should be a no-op, and the eight-pop overflow test has the wrong expected result.

After those are resolved, the core per-pane state plus encoder design is adoptable. I would keep the protocol state and encoder atomic, remove prompt recovery from that ticket, and keep the Windows key-record work separate.

## Findings — blockers first

### 1. [Blocker] The Windows claims are substantially true; this ticket does not fix the reporter

The important claims in §§0–1 check out, with one qualification about the two ConPTY input paths:

| Claim | Review result |
|---|---|
| Native Windows Codex does not request kitty keyboard enhancement | **Confirmed.** Crossterm's Windows support probe always returns `Ok(false)`, and its push/pop commands report the legacy WinAPI path as unsupported. Current Codex gates keyboard enhancement on that probe, then reads through crossterm's Windows console backend. Sources: [crossterm Windows terminal backend](https://github.com/crossterm-rs/crossterm/blob/master/src/terminal/sys/windows.rs), [crossterm commands](https://github.com/crossterm-rs/crossterm/blob/master/src/event.rs), [Codex startup gate](https://github.com/openai/codex/blob/main/codex-rs/tui/src/tui.rs), [Codex keyboard modes](https://github.com/openai/codex/blob/main/codex-rs/tui/src/tui/keyboard_modes.rs). |
| ConPTY has no `CSI ... u` input handler | **Confirmed.** `InputStateMachineEngine::ActionCsiDispatch` has no `u` case; the default returns false and the state machine passes the original sequence through as raw input characters. A VT-input client reads bytes; a console-record client sees character records, not a structured Ctrl+Enter event. [ConPTY input parser](https://github.com/microsoft/terminal/blob/main/src/terminal/parser/InputStateMachineEngine.cpp) |
| ConPTY drops `CSI 27;...;...~` | **Confirmed for a key-record client, not for a VT-input client.** The `Generic` handler passes through when `ENABLE_VIRTUAL_TERMINAL_INPUT` is on. Otherwise it consumes the CSI; code 27 is absent from `_GetGenericVkey`, so it writes no key record. The note states this qualification correctly in §1. |
| Windows Terminal and WezTerm use `?9001h` win32-input-mode records | **Confirmed.** Windows Terminal chooses `_makeWin32Output` when W32IM is active and no kitty flags are active; its ConPTY host requests `?9001h`. WezTerm changes its pane encoding to `Win32` and calls `encode_win32_input_mode`; `allow_win32_input_mode` defaults true on Windows. Sources: [Windows Terminal input encoder](https://github.com/microsoft/terminal/blob/main/src/terminal/input/terminalInput.cpp), [ConPTY VT I/O](https://github.com/microsoft/terminal/blob/main/src/host/VtIo.cpp), [WezTerm key-event path](https://github.com/wezterm/wezterm/blob/main/wezterm-gui/src/termwindow/keyevent.rs), [WezTerm documentation](https://wezterm.org/config/key-encoding.html). |

Therefore the note's own Windows-native Codex matrix entry is the correct one: **unchanged**. Kitty flag 1 fixes Codex on macOS/WSL and requesting byte-stream clients; it cannot fix native Codex's `KEY_EVENT_RECORD` path. T-KEYBOARD-PROTOCOL must not close issue #13 or retain wording that the reporter's configuration is fixed.

Q1-A is technically the right follow-up if the product goal remains “fix the reporter”: serialize only otherwise ambiguous chords as W32IM down/up records when W32IM is active and neither kitty nor modifyOtherKeys is active. It mirrors the transport used by Windows Terminal and WezTerm and is much narrower than converting every key. Before adoption, the note should add these gates and consequences explicitly:

- Track/require active `?9001h`; “no program has asked for kitty/MOK” alone is not proof that serialized W32IM is an available transport.
- Assert the behavior through a real ConPTY for a record reader, a VT-input reader, and a native Node/libuv reader. The last is the useful stand-in for native Claude Code.
- Treat the PSReadLine change as intended behavior, not a harmless implementation detail. In the default Windows edit mode, Shift+Enter is `AddLine` and Ctrl+Enter is `InsertLineAbove`; today Folio collapses both to Enter. [PSReadLine bindings](https://github.com/PowerShell/PSReadLine/blob/master/PSReadLine/KeyBindings.cs), [Microsoft's PSReadLine function reference](https://learn.microsoft.com/en-us/powershell/module/psreadline/about/about_psreadline_functions?view=powershell-7.6)

If the owner retains ruling B, that is coherent, but the issue response, acceptance matrix and changelog must say plainly that native Windows Codex remains unfixed.

### 2. [Blocker] Restoring at the next OSC 133 `A` can break a live program

The simple fish trace in §2.4 is correct: fish disables its own flags before `C`, and a repaint `A` before the next `C` is left alone. That example does not establish ownership.

OSC 133 is in-band and unauthenticated. The existing session code already records the exact limitation: a nested program can emit a complete marked prompt, and the bytes cannot prove which shell emitted it (`crates/bt-term/src/session.rs`, around `shell_prompt_opened_in_order`). Mouse cleanup is safe under a narrower invariant—an interactive shell does not need mouse tracking at its prompt. Keyboard enhancement has no such invariant because shells and line editors do use it.

A breaking sequence is:

1. The outer shell emits `C`; Folio snapshots flags 0.
2. A primary-screen TUI/REPL pushes flag 1 and later launches an OSC-133-enabled subshell. A TUI that temporarily leaves the alternate screen before launching the shell has the same shape.
3. The nested shell emits `A` on the primary screen.
4. The proposed rule restores 0 while the outer TUI is still alive. When the subshell exits and the TUI resumes, its requested encoding is gone.

The current “next `A` after `C`” predicate cannot distinguish that nested prompt from the outer shell returning after a killed command. Requiring `D` would avoid some false restores but loses the crash-recovery case that motivated the rule. A shell that never emits OSC 133 receives no recovery at all, as the note acknowledges.

Required change: remove keyboard/MOK prompt restoration from T-KEYBOARD-PROTOCOL and from the proposed DESIGN/changelog text. If recovery is pursued later, it needs an ownership-capable boundary—such as authenticated shell-integration state/nonce or an explicit user reset—not a bare `C`→`A` pair. Add the nested-subshell trace as a negative test to any future proposal.

### 3. [Blocker] Three grammar/state tests are wrong, and DECSTR needs an explicit compatibility decision

The parser cannot simply be left unchanged as §3 proposes:

- **`CSI > m`:** the KP-0 brief explicitly requires reset. Xterm specifies that XTMODKEYS with no parameters resets all key-modifier resources to their initial values; for Folio's MOK state that means `Off`. The note instead makes it a no-op and tests that no-op (§3 line 121; §6.1 line 336). This must be fixed at the parser/adapter boundary. [xterm control sequences](https://www.invisible-island.net/xterm/ctlseqs/ctlseqs.html)
- **`CSI < 0 u`:** kitty says the pop count defaults to 1 when omitted. An explicit zero is not omitted, and Windows Terminal deliberately treats it as a no-op. Vte 0.15's `next_param_or(1)` conflates zero with omission. Exact protocol support should correct this rather than merely record the difference.
- **Cap test:** with current 0 and pushes 1 through 9 into a saved-value stack capped at 8, the ninth push leaves saved values `[1..8]` and current 9. Seven pops reach 2; the eighth empties the stack and must reset current to 0. The expected “second value after eight pops” in §6.1 line 333 contradicts the note's own pop rule and kitty's empty-stack rule.
- **Large flag values:** vte casts the `u16` parameter to `u8` before the proposed refused-bit event. That produces the right supported bit after a final `& 1`, but it loses evidence of requested unsupported bits 8–15. Either preserve the original parameter for diagnostics or narrow the logging promise to bits vte actually exposes.

For reset behavior, `RIS` clearing both screen stacks and MOK is sound and matches Windows Terminal/Ghostty full-reset behavior. Fresh state on child exit, restart and session restore is also sound. Kitty does not assign DECSTR semantics here. Windows Terminal preserves kitty state on DECSTR; WezTerm preserves its kitty stack but resets modifyOtherKeys. Xterm's soft-reset behavior also resets its key-modifier resources. The note may preserve kitty state, but leaving MOK unchanged while claiming xterm compatibility is a deliberate divergence and should either be implemented or stated as such—not hidden behind “vte does not dispatch it.” Sources: [Windows Terminal soft/hard reset](https://github.com/microsoft/terminal/blob/main/src/terminal/adapter/adaptDispatch.cpp), [WezTerm DECSTR handling](https://github.com/wezterm/wezterm/blob/main/term/src/terminalstate/mod.rs), [xterm manual](https://invisible-island.net/xterm/manpage/xterm.html).

### 4. [Major] The replacement fixes four real vendor defects, but the spec/implementation comparison is overstated

All four defects in `vendor/alacritty_terminal/src/term/mod.rs` are real:

1. Overflow checks the keyboard stack but removes `title_stack[0]`. The note should say this can corrupt the title stack **or panic when it is empty**, not merely let the keyboard stack grow without bound.
2. Query reads `keyboard_mode_stack.last()` while `CSI = ... u` changes active `TermMode` without changing that stack, so the reply can be stale.
3. `swap_alt` recomputes active mode from the new stack top, losing set-only state after a round trip.
4. Swapping the active/inactive vectors preserves alternate-screen keyboard state across later alternate-screen sessions.

The proposed external model—active value plus saved-value stack, per screen; query returns the supported flags actually active; cap and oldest eviction; partial support reported as flag 1—is coherent. Kitty permits partial implementations to be detected by set-then-query, although it strongly encourages all flags. A cap of 8 is allowed and matches Windows Terminal and Ghostty; it is not required by the spec, and WezTerm currently caps at 128. Sources: [kitty protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/), [Windows Terminal stack](https://github.com/microsoft/terminal/blob/main/src/terminal/input/terminalInput.cpp), [Ghostty flag stack](https://github.com/ghostty-org/ghostty/blob/main/src/terminal/kitty/key.zig), [WezTerm handler](https://github.com/wezterm/wezterm/blob/main/term/src/terminalstate/performer.rs).

The alternate-screen lifecycle is a Folio policy, not “kitty's required model.” The spec requires independent main/alternate stacks but does not say the alternate stack must be cleared on every entry/exit. Windows Terminal clears it; Ghostty and WezTerm attach keyboard state to persistent screen objects and preserve it across switches until full reset. Sources: [Ghostty screen switch](https://github.com/ghostty-org/ghostty/blob/main/src/terminal/Terminal.zig), [Ghostty screen state](https://github.com/ghostty-org/ghostty/blob/main/src/terminal/Screen.zig), [WezTerm screen state](https://github.com/wezterm/wezterm/blob/main/term/src/screen.rs). Clearing is a defensible stale-state safety policy, but §2.1 should name the interoperability choice and test `47`, `1047` and `1049` individually.

### 5. [Pass with edits] The requested encoder spot-checks pass

The 80+7 table agrees with kitty's flag-1 encoder for the requested cases:

- Esc alone `CSI 27u`; Ctrl+I `CSI 105;5u` versus Tab `HT`; Ctrl+M `CSI 109;5u` versus Enter `CR`; Shift+Tab `CSI 9;2u`.
- Alt+letter uses CSI-u, Ctrl+Space is `CSI 32;5u`, and keypad Enter is `CSI 57414u`.
- Modifier encoding is `1 + Shift + 2*Alt + 4*Ctrl`; unmodified Enter/Tab/Backspace remain legacy.
- Flag 1 overrides DECCKM for unmodified arrows/Home/End (`CSI A/H`, not SS3), while modified cursor keys retain the usual `CSI 1;m X` form.
- macOS Option-as-text versus Option-as-Alt is correctly kept as Folio policy before encoding.
- When kitty and MOK are both active, choosing kitty is correct. Windows Terminal is not a useful MOK precedent—it does not implement xterm MOK in this encoder—so remove that attribution and cite kitty/Ghostty/WezTerm behavior instead.

Sources: [kitty protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/), [kitty reference encoder](https://github.com/kovidgoyal/kitty/blob/master/kitty/key_encoding.c), [xterm modifyOtherKeys description](https://invisible-island.net/xterm/modified-keys.html), [xterm ctlseqs](https://www.invisible-island.net/xterm/ctlseqs/ctlseqs.html).

One documentation correction is needed: the vendored vte `doc/modifyOtherKeys-example.txt` is generated in the `formatOtherKeys=1` CSI-u form. It supports the note's selection classes, but it is not byte-for-byte evidence for the table's default xterm `CSI 27;m;k~` form. Say that the classes come from the reference table and the wire form comes from xterm's default `formatOtherKeys=0`. The deliberate Backspace 127 divergence is already disclosed correctly.

### 6. [Pass] The thread-door claim is correct

`runtime/keyboard.rs` currently reads `leaf.session.application_cursor_mode()` directly from the focused session immediately before `keyboard_bytes`. `TerminalAdapter::modes()` reads fields from its owned vendor `Term`; it does not acquire the listener mutex used by reply queues, perform I/O, or wait on another thread. Reading the new keyboard protocol value in the same `terminal_modes()` snapshot therefore adds no wait and no registry row under the window-thread rules.

The wording should say “read directly from the session-owned terminal state,” not “published,” because no cross-thread publication mechanism is involved. That is editorial, not a door violation.

### 7. [Major] Keep the atomic core, but revise size, tests and out-of-scope claims

The reason not to split state/query from encoding is correct: advertising support without encoding it is worse than doing neither. With the required parser fixes, vendor refactor, diagnostics, dual-plane/resize handling, generated table, and platform acceptance, one ticket is closer to **L** than M. It becomes a plausible M–L only after removing prompt recovery and leaving W32IM to a follow-up.

Test changes required before implementation:

- Keep the TSV as the shared documentation/test corpus, but add a small independent hard-coded oracle for the normative kitty/xterm spot checks. A wrong TSV otherwise makes the generated document and exhaustive test agree with each other.
- Correct the cap test, add explicit `CSI < 0 u`, and require `CSI > m`→MOK Off.
- The §6.3 PowerShell probe is not a Claude Code test. Assert all three ConPTY outcomes rather than printing case 2 as an observation, and add a native Node raw-stdin/libuv probe for Claude's transport. The real-app Claude row must have a pass/fail expectation; “record whether this works” is investigation, not release acceptance.
- If Q1-B is chosen, native Windows Codex must be an explicit expected failure/unchanged row and issue #13 must remain open or be narrowed. If Q1-A is chosen, T-KEYBOARD-RECORDS owns that acceptance.
- The Mac keyboard-event measurement is fine as a manual platform check, but it does not require or authorize changes to the Mac mini's WorldQuant production environment.

Out-of-scope judgment:

- Flags 2/4/8/16: acceptable under the owner's ruling because query reports only flag 1, despite kitty's recommendation to implement all tiers.
- Alt+Backspace: yes, a separate ticket. Its defect is in the no-protocol legacy path, and changing it would violate this ticket's “non-requesting programs unchanged” promise.
- F1–F12: separable as an existing baseline encoder bug, but not separable from the **claim** of complete flag-1 support. Kitty flag 1 changes F1–F4 from SS3 to CSI forms, and the brief expressly says to change function keys when the spec does. Either include at least the protocol-required F1–F4 behavior here, or keep the separate F-key ticket as a prerequisite/follow-up in 0.4.7 and narrow the changelog to the named disambiguated chords rather than “supports the first tier.”

## Adoption checklist

1. Resolve Q1 A/B and align issue #13, acceptance and changelog.
2. Remove the OSC-133 keyboard restore rule or replace it with an ownership-safe design.
3. Fix `CSI > m`, explicit pop-zero, the cap test, and the refused-high-bit logging promise.
4. Reword alternate-screen clearing as Folio policy; state the DECSTR/MOK choice explicitly.
5. Correct the xterm reference attribution and the scope of the flag-1 support claim.
6. Make the real-ConPTY/Claude outcomes assertions, not observations.
