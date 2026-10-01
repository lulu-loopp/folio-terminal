# The kitty keyboard protocol's disambiguate tier and xterm's modifyOtherKeys in Folio's terminal — design note, 2026-09-29

0.4.7 ticket KP-0 (docs only). T-KEYBOARD-PROTOCOL implements it after Codex's review. Base: main `38b5743b`. **Revision (b), 2026-09-29**, folds in Codex's review (`keyboard-protocol-review-codex-2026-09-29.md`, beside this note); **Revision (c), 2026-09-29**, records the owner's rulings on Q1–Q3 (§7.1). **Revision (d), 2026-09-29**, records where T-KEYBOARD-PROTOCOL's code had to deviate. **Revision (e), 2026-09-29**, records where T-KEYBOARD-RECORDS' measurements made it deviate from §7.3. The lists of what changed are at the end, and the sections below are the current text.

The owner's rulings of 2026-09-28 that this note works under: this is the first ticket of 0.4.7; there is **no Alt+Enter ESC-prefix hack** (PSReadLine reads a lone ESC as clear-line, and ConPTY turns `ESC CR` into an Alt+Enter key record); **only the disambiguate tier (flag 1) is built now**, and the other kitty flags wait until a program we care about needs them; **win32-input-mode is not planned**. §1 reports a fact that bears on the third ruling. The owner ruled on it on 2026-09-29 (§7.1), narrowing "not planned" to T-KEYBOARD-RECORDS; that ruling does not change what this note decides for T-KEYBOARD-PROTOCOL.

Notation: `CSI` is `\e[`; `\e` is ESC; `\r \t \x7f` are CR, HT, DEL. S, A and C are Shift, Alt and Ctrl. The kitty/xterm modifier number is `1 + S·1 + A·2 + C·4`, plus `Super·8`, which Folio never produces (§4.1).

---

## 0. The decisions, in one screen

1. **State lives in `bt-term`, in the vendored `alacritty_terminal`, per pane and per screen.** The vendor already stores the kitty sequences' state in two stacks, but it is switched off by `Config::kitty_keyboard = false`. That switch is why Folio "swallows" the requests today. The ticket turns it on and replaces the vendor's model with kitty's: a current value per screen plus a stack of saved values. The vendor's model has four defects that matter (§2.2). modifyOtherKeys gets a per-terminal field, which the vendor does not have.
2. **`vte` is vendored too, for three dispatch arms** (§3): an explicit `CSI < 0 u` is a no-op, `CSI > m` resets modifyOtherKeys, and the flag values reach the terminal as their full `u16`, so the refused-bits diagnostic can name bits 8–15.
3. **Only flag 1 is honoured.** Every other requested bit is dropped where the request is handled, so the query answers with what is actually in force. The first time a session drops a bit, it writes one line to `diagnostics.log`.
4. **The encoder reads the mode the same way it reads DECCKM**: directly from the session-owned terminal state, at the moment of the key. `TerminalModes` gains `keyboard: KeyboardProtocol { kitty: u8, modify_other_keys: Off|One|Two }`. There is no polling, no lock and no new wait.
5. **Encoding follows kitty's own encoder (`kitty/key_encoding.c`) for flag 1, and xterm for modifyOtherKeys** (xterm's reference table for which chords are encoded, xterm's default `formatOtherKeys=0` for the wire form). Folio's standing policies sit on top: Super never reaches the child, and on a Mac Option types text unless the setting makes it Alt. §4.4 has the full table; the ticket moves it into a TSV that the test and this document share, and keeps a separate hard-coded oracle for the normative spot checks.
6. **No prompt-time recovery in this ticket.** A program that dies with flags on leaves them on until `RIS`, a pop, or a new shell (§2.4 says what the person does). Restoring at OSC 133 was proposed in the first draft and withdrawn: the marker cannot tell a nested shell's prompt from the outer shell's return, and restoring there can strip a live program's flags.
7. **The shortcut table, the cards, the paste roads and IME composition are untouched.** The encoder is the last rung of the key ladder, and it is the only rung that changes.
8. **One ticket, T-KEYBOARD-PROTOCOL, size M–L**, with two commits (the bt-term state with vendored `vte`, and the bt-app encoder) merged together. Either half on its own is worse than neither (§7). **T-FKEYS** (legacy F1–F12, which send nothing today) is its prerequisite, so the flag-1 forms of F1–F4 land here on top of it. The full order is in §7.2: T-FKEYS → T-KEYBOARD-PROTOCOL → T-KEYBOARD-RECORDS, with T-CTRL-SPACE and T-ALT-BACKSPACE on their own.
9. **Flag 1 cannot reach some programs on Windows** (§1, confirmed by the review). A program that reads the console's key records never asks for the protocol, and could not read its bytes if it did. This covers Codex, PowerShell/PSReadLine, cmd and anything built on crossterm's Windows backend. Issue #13's reporter (Codex on Windows 11) is one of them. **Owner ruling 2026-09-29 (Q1 = A):** those programs are reached by **T-KEYBOARD-RECORDS** (§7.3). It sends the chords VT cannot express as win32-input-mode key records, the way Windows Terminal and WezTerm do. Ctrl+Enter submit therefore works for Codex on Windows in 0.4.7, and #13 closes with that ticket. Its change to PowerShell is intended and is disclosed in the release note.

---

## 1. On Windows, the transport decides which programs the protocol can reach

On Windows, Folio's children sit behind ConPTY (the vendored `Microsoft.Windows.Console.ConPTY.1.25.260710002-preview`). Three facts from the Windows Terminal source (`microsoft/terminal` main, read 2026-09-29) settle what the protocol can do there:

- **The request reaches Folio.** `WriteCharsVT` (`src/host/_stream.cpp`) parses a client's VT output *and* writes the same string to the terminal verbatim. ConPTY's own handling of the four kitty sequences returns early when it is running as ConPTY (`AdaptDispatch::SetKittyKeyboardProtocol` … `PopKittyKeyboardProtocol`: `if (_api.IsConPTY()) return;`), and `VtIo` turns off its own KKP translation (GH#19847). So a `CSI > 1 u` from a WSL program, from Node, or from anything else that writes VT arrives in `bt-term` unchanged. ConPTY itself writes `\e[>4m\e[<u` on teardown; `adapter.rs`'s `a_teardown_that_conpty_re_enables_does_not_type_at_the_pane` feeds exactly those bytes.
- **The answer and the keys go back through ConPTY's input parser, which does not know any `CSI … u`.** `InputStateMachineEngine::ActionCsiDispatch` has no case for it. ConPTY hands an unrecognised CSI to the client **as plain characters** (`ActionPassThroughString` → `WriteStringRaw`). A client in VT-input mode (WSL's relay, or anything that set `ENABLE_VIRTUAL_TERMINAL_INPUT`) therefore reads `\e[13;5u` as bytes, which is what it asked for. modifyOtherKeys' form, `CSI 27;…;…~`, lands in the `Generic` (`~`) case instead. There it is passed through to a VT-input client, but **silently dropped** for a key-record client, because `_GetGenericVkey(27)` has no matching key.
- **A key-record client never asks.** On Windows, crossterm's `PushKeyboardEnhancementFlags` has `is_ansi_code_supported() == false` and `execute_winapi() == Err(Unsupported)` (`crossterm/src/event.rs`). Codex's `tui/src/tui/keyboard_modes.rs` returns the same `Err` for its reset and for modifyOtherKeys under `cfg(windows)`. Codex on Windows reads `KEY_EVENT_RECORD`s, and so do PSReadLine and .NET's `Console.ReadKey`.

In Windows Terminal, Ctrl+Enter reaches Codex through **win32-input-mode**, not through the kitty protocol. ConPTY requests win32-input-mode at the start of every session (`?9001h`; measured on a real ConPTY, see the doc comment on `TerminalAdapter::retire_program_input_modes`). Windows Terminal and WezTerm answer it by sending a full key record for every key; in WezTerm this is `allow_win32_input_mode`, which defaults to true on Windows and takes precedence over every other encoding. Folio already writes such records in one place: PSReadLine's `AddLine` for a multi-line paste (`input::SHIFT_ENTER_RECORDS`, 0.4.4 ticket 03). ConPTY turns them into the exact key event.

So, on Windows:

| Child | Asks for flag 1? | What T-KEYBOARD-PROTOCOL gives it |
|---|---|---|
| WSL / ssh programs (neovim, fish, helix, Codex on Linux, Claude Code in WSL) | yes | the full protocol, through ConPTY in both directions |
| Node programs on Windows (Claude Code running natively) | Claude Code asks: `CSI ? u`, then `CSI > 5 u` and `CSI > 4 ; 2 m` (anthropics/claude-code#96526, #97501) | the reply and the `CSI … u` keys arrive as characters. libuv reassembles the characters of key records into bytes, so the program reads the sequence. **The ticket's real-ConPTY test asserts this with a native Node raw-stdin reader (§6.3); if it fails, the ticket stops and reports rather than shipping the claim.** |
| Key-record programs (Codex, PSReadLine, cmd, .NET) | no | nothing changes, by construction |

macOS has no such layer, so every program there is in the first row.

---

## 2. State

### 2.1 Where it lives

The state belongs to each pane: it lives in the vendored `Term` inside that pane's `TerminalAdapter`, **per screen**. The primary screen and the alternate screen each own `{ current: KeyboardModes, stack: Vec<KeyboardModes> }`. The spec requires this ("Terminals must maintain separate stacks for the main and alternate screens"), and every implementation has this shape: kitty, WT's `_kittyMainStack`/`_kittyAltStack`, and Ghostty. modifyOtherKeys is **one value per terminal**, not per screen, as in xterm.

| Event | Keyboard state | modifyOtherKeys |
|---|---|---|
| enter the alternate screen (`?1049h`, `?1047h`, `?47h` — whatever reaches `swap_alt`) | the alternate screen starts **empty** (`current = 0`, stack cleared); the primary's state is kept as it was | unchanged |
| leave the alternate screen | the alternate screen's state is **discarded**; the primary's is in force again | unchanged |
| `RIS` (`ESC c`) | both screens cleared | `Off` |
| `DECSTR` (`CSI ! p`) | **preserved** (as in WT and WezTerm; kitty assigns DECSTR no meaning here) | **not reset — a stated divergence from xterm** (below) |
| OSC 133 markers | nothing (§2.4) | nothing |
| the child exits; *Restart shell*; a pane restored from `session.json` | a new `LeafSession` and a new adapter: nothing to carry over, and nothing is persisted | same |

**Clearing the alternate screen's state on entry and on exit is Folio's policy, not the spec's.** The spec requires independent main and alternate stacks; it does not say the alternate stack is cleared. Implementations differ: Windows Terminal clears it (`UseAlternateScreenBuffer`/`UseMainScreenBuffer`), while Ghostty and WezTerm keep keyboard state on persistent screen objects and preserve it across switches until a full reset. Folio takes WT's side for a safety reason: with preserved state, a crashed `nvim` would hand its flags to the next `less`. A program that follows kitty's quickstart (push on entering the alternate screen, pop before leaving it) sees no difference. The vendor's `swap_alt` currently preserves it, which is defect 4 of §2.2 only under this policy.

**DECSTR and modifyOtherKeys.** xterm's soft reset returns its key-modifier resources to their initial values, and WezTerm resets modifyOtherKeys on DECSTR. Folio does not implement DECSTR at all today — `vte` 0.15 does not dispatch `CSI ! p`, so none of its effects (cursor, margins, modes) happen — and resetting modifyOtherKeys alone would give one effect of a sequence whose others are all missing. So under this ticket DECSTR leaves modifyOtherKeys as it is, and that is a divergence from xterm, stated here and in the DESIGN entry. When a ticket implements DECSTR, it resets modifyOtherKeys to Off and leaves the kitty state alone.

### 2.2 Why the vendor's model is replaced rather than switched on

`vendor/alacritty_terminal/src/term/mod.rs` has `keyboard_mode_stack` / `inactive_keyboard_mode_stack`, `push_keyboard_mode`, `pop_keyboard_modes`, `set_keyboard_mode` and `report_keyboard_mode`, all guarded by `config.kitty_keyboard`. Turning that switch on is not enough:

1. **Overflow evicts from the wrong stack.** `push_keyboard_mode` checks `keyboard_mode_stack.len() >= KEYBOARD_MODE_STACK_MAX_DEPTH` and then removes `self.title_stack[0]`. That corrupts the title stack (it loses its oldest entry) **or panics when the title stack is empty** (`Vec::remove(0)` on an empty vector), and the keyboard stack itself is never trimmed (4096 is the title stack's cap, not this stack's).
2. **The query reports the top of the stack, not the flags in force.** `CSI = 5 u` (fish's form, which sets and does not push) changes `TermMode` but no stack entry, so a following `CSI ? u` answers `0`. A program that follows the spec's detection advice ("set the desired progressive enhancements and then query") gets the wrong answer.
3. **A switch between screens recomputes the mode from the top of the stack.** Flags set with `CSI =` on the primary screen are therefore lost on the way back from `less`.
4. The alternate stack survives from one alternate-screen session to the next, which Folio's policy (§2.1) rules out.

The replacement is kitty's model:

- `current` is what is in force.
- `push(f)` saves `current` on the stack (evicting the oldest entry when the stack is at its cap) and sets `current = f`.
- `pop(n)`, for `n ≥ 1`: if `n` is less than the stack's length, `current` becomes the `n`-th saved value from the top and those `n` entries are removed; otherwise the stack is emptied and `current = 0` (kitty: "If a pop request is received that empties the stack, all flags are reset"). `pop(0)` does nothing.
- `set(f, mode)` changes `current` only.
- `query` answers `current`.

After every change, the `TermMode::KITTY_KEYBOARD_PROTOCOL` bits are derived from the active screen's `current`, so the rest of the vendor code that reads `TermMode` is untouched. The cap is **8**. The spec requires a cap but not its size; 8 matches Windows Terminal (`KittyStackMaxSize`) and Ghostty, and WezTerm uses 128. The eviction is the one the spec requires ("the oldest entry from the stack must be evicted").

`vendor/alacritty_terminal` is Folio's own fork and is edited in place, as `set_write_provenance` and the transcript hook already are. The resize oracle (`ResizeCanonical`) is fed the same bytes, so it reaches the same state. Its listener's replies are never drained to the PTY, and the ticket pins this with a test: a query that arrives while a resize is armed is answered once.

### 2.3 How the encoder learns the mode

The window thread owns every `DualPlaneSession` directly. It already reads `leaf.session.application_cursor_mode()` at the one place that calls `input::keyboard_bytes` (`runtime/keyboard.rs`, the last rung of `keyboard_input`). The new value is read the same way, directly from the session-owned terminal state; no cross-thread publication is involved:

```rust
// bt-term
pub enum ModifyOtherKeys { Off, One, Two }
pub struct KeyboardProtocol { pub kitty: u8 /* masked to SUPPORTED_KITTY_FLAGS */, pub modify_other_keys: ModifyOtherKeys }
pub struct TerminalModes { /* existing five */, pub keyboard: KeyboardProtocol }
```

`TerminalAdapter::modes()` fills it in from the vendor `Term` for the active screen. The call site reads `terminal_modes()` once and passes `(application_cursor_mode, keyboard)` to the encoder. This reads a field of data the window thread already holds. It adds no door, no lock and no wait, so neither the thread-door note's §C-5 nor the window-thread budget has anything to register: those govern waits, and this is not one.

### 2.4 A program's leftovers: not recovered by this ticket

A program that pushes flag 1 and dies without popping it leaves the pane encoding `Ctrl+C` as `CSI 99;5u`. In a Unix shell that is an inconvenience; the spec keeps plain Enter/Tab/Backspace legacy so that `reset` can still be typed. **Behind ConPTY it is worse.** PSReadLine is a key-record client, so `CSI 99;5u` arrives as six typed characters and `Ctrl+C` no longer interrupts anything. (Only a program that writes VT can have set the flags, so this needs a WSL or Node program to die in a pane whose shell is PowerShell.)

**This ticket does not recover from it automatically.** The first draft restored the primary screen's state at OSC 133 `A` to its value at the preceding `C`. That boundary is not ownership-safe, and the rule is withdrawn. OSC 133 is in-band and unauthenticated, and `session.rs` already records that a nested program can emit a complete marked prompt that the bytes cannot attribute (`shell_prompt_opened_in_order`). Mouse cleanup at `A` is safe because a shell never needs mouse tracking at its prompt; keyboard enhancement has no such invariant, because shells and line editors use it (fish sets `CSI = 5 u` while it reads a command line).

**The negative trace any future proposal must pass** (it goes into that proposal's tests, and it is written here so nobody re-proposes the rule without it):

1. The outer shell emits OSC 133 `C` (flags 0 at that moment).
2. A primary-screen TUI or REPL pushes flag 1 (`CSI > 1 u`) and, still running, launches a subshell that has OSC 133 integration. A TUI that leaves the alternate screen before launching the shell has the same shape.
3. The subshell emits OSC 133 `A` on the primary screen.
4. The subshell exits and the TUI resumes.

**Required:** at step 4 the flags are still 1. The withdrawn rule set them to 0 at step 3, silently removing a live program's encoding. Requiring a `D` before the `A` avoids some of these cases but loses the crash case the rule was for. A sound design needs an ownership-capable boundary, such as authenticated shell-integration state (a per-session nonce in Folio's own integration) or an explicit reset the person asks for. It is not in 0.4.7.

**What the person does meanwhile**, when a dead program has left the keys encoded:

- In a Unix shell (WSL, ssh, macOS): type `reset` and Enter. Plain letters and Enter are unaffected, `reset` sends `RIS`, and `RIS` clears both screens' state and modifyOtherKeys (§2.1).
- In PowerShell: type ``Write-Host -NoNewline "`e[<99u`e[>4m"`` and Enter. This pops every saved value, which resets the flags, and turns modifyOtherKeys off; the bytes pass through ConPTY to Folio (§1). `` `e `` is PowerShell 7's escape; in Windows PowerShell 5.1, write `$([char]27)` in its place. Or use the pane's *Restart shell*, or open a new tab.
- A Folio "reset keyboard protocol" verb is a possible later door. It is recorded here, not proposed.

---

## 3. Grammar

`vte` 0.15 (`src/ansi.rs`) already dispatches all four kitty sequences and both xterm ones, but three of its arms lose information this ticket needs. **The ticket vendors `vte` 0.15.0 at `vendor/vte`** (`[patch.crates-io] vte = { path = "vendor/vte" }`, beside the existing `alacritty_terminal`, `mitex` and `portable-pty` patches) and changes exactly these arms:

1. `('u', [b'<'])` — pop. Upstream calls `next_param_or(1)`, which cannot tell an omitted count from an explicit `0`. The vendored arm passes `1` when the parameter is omitted and the value as written otherwise, so `CSI < 0 u` reaches the terminal as `0` and does nothing (kitty: the count "defaults to 1 if unspecified" — an explicit 0 is specified; WT agrees).
2. `('m', [b'>'])` — XTMODKEYS. Upstream treats `CSI > m` (no parameters) as unhandled. xterm specifies that XTMODKEYS with no parameters resets all key-modifier resources to their initial values; Folio's only such resource is modifyOtherKeys, so the vendored arm calls `set_modify_other_keys(Reset)`.
3. `('u', [b'>'])` and `('u', [b'='])` — push and set. Upstream casts the parameter to `u8` and `from_bits_truncate`s it before the handler sees it, so bits 5–15 of a request vanish without trace. The vendored arms pass the parameter as its full `u16`, so the terminal masks it and the refused-bits diagnostic can name every bit that was asked for.

The `Handler` trait's `push_keyboard_mode` and `set_keyboard_mode` therefore take a `u16`. The only implementor is the vendored `Term` (plus the adapter's `ParserTailSink`, which keeps the trait's defaults); `bt-corpus` uses `vte`'s `Parser`/`Perform` only and is unaffected. The alternative — intercepting these sequences in the adapter's boundary parser — was rejected, because that parser runs ahead of the processor and would have to replay its decisions at the processor's pace.

| Bytes | Dispatch (vendored `vte`) | Folio does |
|---|---|---|
| `CSI > flags u` | `push_keyboard_mode(p: u16)`, `p` default 0 | push; `current = p & SUPPORTED` |
| `CSI < n u` | `pop_keyboard_modes(n)`, `n` = 1 when omitted, as written otherwise | pop `n` (§2.2); `n = 0` does nothing |
| `CSI = flags ; mode u` | `set_keyboard_mode(p: u16, Replace/Union/Difference)` for mode 1/2/3, default 1 | `current` = / \|= / &=! `p & SUPPORTED` |
| `CSI ? u` | `report_keyboard_mode()` | reply `CSI ? current u` |
| `CSI > 4 ; v m` | `set_modify_other_keys(Reset/EnableExceptWellDefined/EnableAll)` for `v` = 0/1/2; `CSI > 4 m` means `v = 0` | Off / One / Two |
| `CSI > m` | `set_modify_other_keys(Reset)` (vendored arm 2) | Off |
| `CSI ? 4 m` | `report_modify_other_keys()` | reply `CSI > 4 ; v m` |
| `CSI > 4 ; 3 m`, `CSI > 1 ; … m`, other XTMODKEYS resources | unhandled | nothing |

**Bounds.** No value is treated as an error; each case below says what happens.

- Parameters are `u16`, and the flags value is a bit set, so no value is out of range: `SUPPORTED_KITTY_FLAGS = 0b1` masks it.
- A pop count of `0` is a no-op; a count at least the stack's length empties it and resets the flags.
- `vte` reads a `mode` other than 1–3 as 1 (replace), unchanged.
- The stack holds 8 saved values.

**Requested but unsupported bits.** When the mask drops bits, the vendor `Term` reports them, as the full `u16` that was asked for, through the listener (a new `Event` variant that `CaptureListener` records). The adapter ORs them into a per-session `u16`. The first time a bit appears, the session writes one line to `diagnostics.log`: `keyboard protocol: flags 0b111 requested, 0b1 in force` (Codex on Unix asks for `>7u`, Claude Code for `>5u`). There is no card and no setting. The log line is how we will learn that "a program we care about needs them".

**Replies** go through `Event::PtyWrite` and the adapter's ordered `PendingReply` queue, together with every other reply. So `CSI ? u` followed by `CSI c` is answered in that order. The spec's detection recipe depends on that order: "If an answer for the device attributes is received without getting back an answer for the progressive enhancement the terminal does not support this protocol".

---

## 4. Encoding

### 4.1 Inputs and standing policies

This is what winit 0.30.13 gives the encoder, and what each input means here:

- `logical_key`. On Windows this is the layout's key with every modifier **except Ctrl** applied (`platform_impl/windows/keyboard.rs`: `layout.get_key(mods_without_ctrl, …)`), so `Ctrl+Shift+1` is `"!"` and `Ctrl+a` is `"a"`. On macOS a Ctrl chord may arrive as the control character itself (`"\u{2}"`); today's encoder already accepts that.
- `key_without_modifiers()`. This is the layout's key with no modifier applied. On Windows it comes from `layout.get_key(NO_MODS, …)`, with dead keys reported as their character; on macOS it is the character with no modifiers. **This is kitty's "unicode-key-code".** The spec says the code is "always the lower-case (or more technically, un-shifted) version of the key", and this value is exactly that, for whatever layout is in use. For example, Russian `ф` is 1092, which is what kitty itself sends without flag 4.
- `location`. `KeyLocation::Numpad` marks keypad keys (winit's `get_location`, from the scan code).
- The modifiers. These come from `WindowRuntime::modifiers`, which has already been through `input::effective_modifiers`:
  - On a Mac with *Option key sends Alt* off, there is no Alt, and `⌥a` is the text `å` in every mode. With the setting on, Alt is Alt (`CSI 97;3u` under flag 1).
  - On Windows, winit removes Ctrl and Alt while AltGr is held (`keyboard_layout.rs`: `filter_out_altgr`), so AltGr text is text in every mode.
- **Caps Lock and Num Lock are never reported**, because `ModifiersState` has no lock bits. Kitty itself adds them (64, 128) to non-text keys under flag 1. A program has to read a missing bit as "unlocked" when the terminal does not report locks.
- **Super never reaches the child.** On Windows this is §7.54d; on a Mac, Command belongs to the application (§13.13). A chord that includes Super still produces no bytes, in every mode, so bit 8 is never produced.

### 4.2 The rule under flag 1 (disambiguate)

The encoder applies the first of these rules that matches:

1. `Process`, a paste chord, or Super held → nothing (unchanged).
2. The key produces printable text (a `Character` with no control character, or `Space`), and no modifier other than Shift is held → **the text** (unchanged).
3. Enter, Tab or Backspace with **no** modifier → `\r`, `\t`, `\x7f`. This is the spec's exception, so that `reset` can still be typed.
4. Arrows, Home and End send `CSI 1;m X` with a modifier (unchanged) and `CSI X` without one — **the CSI form even under DECCKM**. Kitty's encoder uses `SS3` only in legacy mode (`encode_function_key`: `cursor_key_mode && legacy_mode`), and Ghostty does the same. Insert, Delete, PageUp and PageDown are unchanged.
5. A keypad key that produced no text → `CSI code;m u`, using the spec's keypad code (`KP_ENTER` 57414, `KP_LEFT` 57417 … `KP_DELETE` 57426). This covers Numpad Enter, and the keypad's arrows, Home, End, Insert, Delete, PageUp and PageDown with Num Lock off. Keypad digits and operators produce text, so rule 2 handles them. (Owner ruling Q2, 2026-09-29: follow the spec.)
6. Everything else that has a code → `CSI code;m u`, leaving out `;m` when `m = 1`. The codes are: Escape 27, Enter 13, Tab 9, Backspace 127, Space 32, and for a text key the character from `key_without_modifiers`. This rule covers Esc on its own, every Alt chord, every Ctrl chord (including `Ctrl+i`, `Ctrl+m`, `Ctrl+[` and `Ctrl+c`), Shift+Tab (`CSI 9;2u`, not `CSI Z`), Ctrl+digit, and every modified Enter, Tab, Backspace and Space.

7. Function keys (on top of T-FKEYS, the prerequisite that gives F1–F12 their legacy forms: `SS3 P/Q/R/S` and `CSI 15~ … CSI 24~`, with modifiers `CSI 1;m P/Q/R/S` and `CSI n;m ~`): under flag 1, F1, F2 and F4 are `CSI P`, `CSI Q`, `CSI S` without modifiers and `CSI 1;m P/Q/S` with them; F3 is `CSI 13~` / `CSI 13;m~` (kitty removed `CSI R` because it collides with the cursor position report); F5–F12 are as legacy. This is kitty's `encode_function_key`. Folio's encoder has **no case for F-keys today**, so F1–F12 send nothing (§8 F-1); that legacy fix is T-FKEYS's, not this ticket's, because it changes bytes for programs that never asked.

### 4.3 The rule under modifyOtherKeys

The encoder uses modifyOtherKeys only when the kitty flags are 0. **When both are set, kitty wins**, as in kitty, Ghostty and WezTerm (Windows Terminal does not implement xterm's modifyOtherKeys in its encoder, so it is no precedent here); Claude Code asks for both.

The wire form is xterm's default, `formatOtherKeys=0`: `CSI 27 ; m ; k ~`. `k` is the character **with Shift applied** and without Ctrl, which is xterm's keysym; for example, `Ctrl+Shift+a` gives `k = 65`. The fixed codes are Enter 13, Tab 9, Escape 27, Space 32 and Backspace **127**. Folio's Backspace sends DEL; xterm's reference output used 8 because its Backspace was `^H`.

Which chords are encoded follows xterm's own reference table (`alacritty/vte` `doc/modifyOtherKeys-example.txt`, the output of xterm's `vttests/modify-keys.pl`). That table was generated with `formatOtherKeys=1`, so it prints `CSI k;m u`; it is the evidence for *which* chords are encoded and with which `k` and `m`, not for the bytes. The bytes are the `formatOtherKeys=0` form above. By class of key:

| Class | Mode 1 encodes | Mode 2 encodes |
|---|---|---|
| Enter, Tab | every modified chord | every modified chord |
| Escape | chords with Alt | every modified chord |
| Backspace | none | every modified chord except Ctrl alone |
| keys with a legacy Ctrl code (the domain of Folio's `control_byte`: letters, Space, `@ [ \ ] ^ _ ?`) | chords with Alt | every modified chord, including Shift alone (`Shift+a` → `CSI 27;2;65~`) |
| other printable keys (digits, other punctuation, non-ASCII) | chords with Ctrl or Alt | chords with Ctrl or Alt |

A chord that a mode does not encode takes the legacy row.

`k` needs the shifted character while Ctrl is held. On Windows, `logical_key` is exactly that. On macOS, a Ctrl chord can arrive as its control character, so `k` is `key_without_modifiers` uppercased for letters. For digits, the ticket checks the rows by hand on the Mac mini before trusting them (§6.2).

On Windows, a key-record child that asked for modifyOtherKeys alone would lose these chords entirely, because ConPTY drops an unknown `~` sequence for such a child (§1). No such child is known: Claude Code asks for both, and flag 1 wins. The real-ConPTY test asserts the drop (§6.3), so the fact is pinned rather than assumed.

### 4.4 The table

The table covers ten keys × eight modifier sets × four modes. The markers mean:

- `(table)`: claimed by Folio's shortcut table on Windows (`docs/shortcuts.md`), so the key never reaches the encoder.
- `(system)`: taken by the OS on Windows.
- `—`: no bytes.
- `(n/a)`: `Ctrl+Shift+Enter` does not arrive on Windows at all (measured 2026-08-19 and 2026-08-25, `shortcuts.rs`). The row is still encoded, for macOS.

The **legacy** column is what Folio sends today, and it does not change.

| Key | Mods | legacy (today) | kitty flag 1 | modifyOtherKeys 1 | modifyOtherKeys 2 |
|---|---|---|---|---|---|
| Enter | – | `\r` | `\r` | `\r` | `\r` |
| Enter | S | `\r` | `CSI 13;2u` | `CSI 27;2;13~` | `CSI 27;2;13~` |
| Enter | A | `\r` | `CSI 13;3u` | `CSI 27;3;13~` | `CSI 27;3;13~` |
| Enter | SA | `\r` | `CSI 13;4u` | `CSI 27;4;13~` | `CSI 27;4;13~` |
| Enter | C | `\r` | `CSI 13;5u` | `CSI 27;5;13~` | `CSI 27;5;13~` |
| Enter | SC | `\r` (n/a) | `CSI 13;6u` | `CSI 27;6;13~` | `CSI 27;6;13~` |
| Enter | AC | `\r` | `CSI 13;7u` | `CSI 27;7;13~` | `CSI 27;7;13~` |
| Enter | SAC | `\r` | `CSI 13;8u` | `CSI 27;8;13~` | `CSI 27;8;13~` |
| Tab | – | `\t` | `\t` | `\t` | `\t` |
| Tab | S | `CSI Z` | `CSI 9;2u` | `CSI 27;2;9~` | `CSI 27;2;9~` |
| Tab | A | `\t` (system) | `CSI 9;3u` | `CSI 27;3;9~` | `CSI 27;3;9~` |
| Tab | SA | `CSI Z` | `CSI 9;4u` | `CSI 27;4;9~` | `CSI 27;4;9~` |
| Tab | C | (table) | (table) | (table) | (table) |
| Tab | SC | (table) | (table) | (table) | (table) |
| Tab | AC | `\t` | `CSI 9;7u` | `CSI 27;7;9~` | `CSI 27;7;9~` |
| Tab | SAC | `CSI Z` | `CSI 9;8u` | `CSI 27;8;9~` | `CSI 27;8;9~` |
| Backspace | – | `\x7f` | `\x7f` | `\x7f` | `\x7f` |
| Backspace | S | `\x7f` | `CSI 127;2u` | `\x7f` | `CSI 27;2;127~` |
| Backspace | A | `\x7f` | `CSI 127;3u` | `\x7f` | `CSI 27;3;127~` |
| Backspace | SA | `\x7f` | `CSI 127;4u` | `\x7f` | `CSI 27;4;127~` |
| Backspace | C | `\x7f` | `CSI 127;5u` | `\x7f` | `\x7f` |
| Backspace | SC | `\x7f` | `CSI 127;6u` | `\x7f` | `CSI 27;6;127~` |
| Backspace | AC | `\x7f` | `CSI 127;7u` | `\x7f` | `CSI 27;7;127~` |
| Backspace | SAC | `\x7f` | `CSI 127;8u` | `\x7f` | `CSI 27;8;127~` |
| Escape | – | `\e` | `CSI 27u` | `\e` | `\e` |
| Escape | S | `\e` | `CSI 27;2u` | `\e` | `CSI 27;2;27~` |
| Escape | A | `\e` | `CSI 27;3u` | `CSI 27;3;27~` | `CSI 27;3;27~` |
| Escape | SA | `\e` | `CSI 27;4u` | `CSI 27;4;27~` | `CSI 27;4;27~` |
| Escape | C | `\e` | `CSI 27;5u` | `\e` | `CSI 27;5;27~` |
| Escape | SC | `\e` | `CSI 27;6u` | `\e` | `CSI 27;6;27~` |
| Escape | AC | `\e` | `CSI 27;7u` | `CSI 27;7;27~` | `CSI 27;7;27~` |
| Escape | SAC | `\e` | `CSI 27;8u` | `CSI 27;8;27~` | `CSI 27;8;27~` |
| Space | – | ` ` | ` ` | ` ` | ` ` |
| Space | S | ` ` | ` ` | ` ` | `CSI 27;2;32~` |
| Space | A | `\e ` | `CSI 32;3u` | `CSI 27;3;32~` | `CSI 27;3;32~` |
| Space | SA | `\e ` | `CSI 32;4u` | `CSI 27;4;32~` | `CSI 27;4;32~` |
| Space | C | — | `CSI 32;5u` | — | `CSI 27;5;32~` |
| Space | SC | — | `CSI 32;6u` | — | `CSI 27;6;32~` |
| Space | AC | — | `CSI 32;7u` | `CSI 27;7;32~` | `CSI 27;7;32~` |
| Space | SAC | — | `CSI 32;8u` | `CSI 27;8;32~` | `CSI 27;8;32~` |
| e | – | `e` | `e` | `e` | `e` |
| e | S | `E` | `E` | `E` | `CSI 27;2;69~` |
| e | A | `\ee` | `CSI 101;3u` | `CSI 27;3;101~` | `CSI 27;3;101~` |
| e | SA | `\eE` | `CSI 101;4u` | `CSI 27;4;69~` | `CSI 27;4;69~` |
| e | C | `\x05` | `CSI 101;5u` | `\x05` | `CSI 27;5;101~` |
| e | SC | `\x05` | `CSI 101;6u` | `\x05` | `CSI 27;6;69~` |
| e | AC | `\e\x05` | `CSI 101;7u` | `CSI 27;7;101~` | `CSI 27;7;101~` |
| e | SAC | `\e\x05` | `CSI 101;8u` | `CSI 27;8;69~` | `CSI 27;8;69~` |
| i | – | `i` | `i` | `i` | `i` |
| i | S | `I` | `I` | `I` | `CSI 27;2;73~` |
| i | A | `\ei` | `CSI 105;3u` | `CSI 27;3;105~` | `CSI 27;3;105~` |
| i | SA | `\eI` | `CSI 105;4u` | `CSI 27;4;73~` | `CSI 27;4;73~` |
| i | C | `\t` | `CSI 105;5u` | `\t` | `CSI 27;5;105~` |
| i | SC | `\t` | `CSI 105;6u` | `\t` | `CSI 27;6;73~` |
| i | AC | `\e\t` | `CSI 105;7u` | `CSI 27;7;105~` | `CSI 27;7;105~` |
| i | SAC | `\e\t` | `CSI 105;8u` | `CSI 27;8;73~` | `CSI 27;8;73~` |
| m | – | `m` | `m` | `m` | `m` |
| m | S | `M` | `M` | `M` | `CSI 27;2;77~` |
| m | A | `\em` | `CSI 109;3u` | `CSI 27;3;109~` | `CSI 27;3;109~` |
| m | SA | `\eM` | `CSI 109;4u` | `CSI 27;4;77~` | `CSI 27;4;77~` |
| m | C | `\r` | `CSI 109;5u` | `\r` | `CSI 27;5;109~` |
| m | SC | (table) | (table) | (table) | (table) |
| m | AC | `\e\r` | `CSI 109;7u` | `CSI 27;7;109~` | `CSI 27;7;109~` |
| m | SAC | `\e\r` | `CSI 109;8u` | `CSI 27;8;77~` | `CSI 27;8;77~` |
| [ | – | `[` | `[` | `[` | `[` |
| [ | S | `{` | `{` | `{` | `CSI 27;2;123~` |
| [ | A | `\e[` | `CSI 91;3u` | `CSI 27;3;91~` | `CSI 27;3;91~` |
| [ | SA | `\e{` | `CSI 91;4u` | `CSI 27;4;123~` | `CSI 27;4;123~` |
| [ | C | `\e` | `CSI 91;5u` | `\e` | `CSI 27;5;91~` |
| [ | SC | — | `CSI 91;6u` | — | `CSI 27;6;123~` |
| [ | AC | `\e\e` | `CSI 91;7u` | `CSI 27;7;91~` | `CSI 27;7;91~` |
| [ | SAC | — | `CSI 91;8u` | `CSI 27;8;123~` | `CSI 27;8;123~` |
| 1 | – | `1` | `1` | `1` | `1` |
| 1 | S | `!` | `!` | `!` | `!` |
| 1 | A | `\e1` | `CSI 49;3u` | `CSI 27;3;49~` | `CSI 27;3;49~` |
| 1 | SA | `\e!` | `CSI 49;4u` | `CSI 27;4;33~` | `CSI 27;4;33~` |
| 1 | C | — | `CSI 49;5u` | `CSI 27;5;49~` | `CSI 27;5;49~` |
| 1 | SC | (table) | (table) | (table) | (table) |
| 1 | AC | — | `CSI 49;7u` | `CSI 27;7;49~` | `CSI 27;7;49~` |
| 1 | SAC | — | `CSI 49;8u` | `CSI 27;8;33~` | `CSI 27;8;33~` |

For these keys, the only changes are the DECCKM, keypad and function-key rules of §4.2 (the F-key legacy cells are T-FKEYS's, which lands first):

| Key | Mods | legacy, DECCKM on | kitty flag 1, DECCKM on or off |
|---|---|---|---|
| F1 | – | `\eOP` (T-FKEYS) | `CSI P` |
| F1 | C | `CSI 1;5P` (T-FKEYS) | `CSI 1;5P` |
| F3 | – | `\eOR` (T-FKEYS) | `CSI 13~` |
| F3 | S | `CSI 1;2R` (T-FKEYS) | `CSI 13;2~` |
| F4 | – | `\eOS` (T-FKEYS) | `CSI S` |
| F5 | – | `CSI 15~` (T-FKEYS) | `CSI 15~` |
| ArrowUp | – | `\eOA` | `CSI A` |
| ArrowUp | C | `CSI 1;5A` | `CSI 1;5A` |
| Home | – | `\eOH` | `CSI H` |
| Numpad Enter | – | `\r` | `CSI 57414u` |
| Numpad Enter | C | `\r` | `CSI 57414;5u` |
| Numpad 8, Num Lock off | – | `\eOA` | `CSI 57419u` |
| Numpad 8, Num Lock on | – | `8` | `8` |

Every kitty cell that is not `(table)` agrees with kitty's `key_encoding.c` for the same key and modifiers, apart from the lock bits (§4.1). Every modifyOtherKeys cell agrees with xterm's reference output, apart from Backspace's code (§4.3). Where Ghostty differs from kitty (Ghostty's `effectiveMods` would encode `Shift+Space`), this note follows kitty, which is the protocol's reference implementation.

---

## 5. Folio's own ladder

`Runtime::keyboard_input` is unchanged above its last rung. A key passes through these rungs in order:

1. The gate for synthetic presses, and the rewrite of injected keys.
2. The hint card and the peek flyout.
3. Esc closing drags, menus, capsules and cards, one layer at a time.
4. The keys of the **update card** and the **paste card**.
5. The clipboard seats. `Ctrl+V`, `Ctrl+Shift+V`, `Shift+Insert` and their Mac equivalents go to the paste door, never to the encoder.
6. The **shortcut table** (`Shortcuts::lookup`, exact modifiers). This takes every `(table)` cell above, the terminal-scope `Ctrl+=`/`Ctrl+-`/`Ctrl+0`, and `Ctrl+F`.
7. The transcript keys on the primary screen (`Shift+PageUp`, `Ctrl+Home`, `Ctrl+End`).
8. Popups.
9. The files column.
10. The preview editor.
11. The search field.
12. The encoder. Every change in §4 happens here, and only here.

**IME composition** never reaches the encoder. On Windows a composing key is `NamedKey::Process` (rule 1). On macOS a composition arrives as `Ime::Commit` with no `KeyboardInput` (§13.16, X-3). A committed string goes by the IME road, not through `keyboard_bytes`, in every mode.

**Paste** has its own road: bracketed paste, and `input_line_bytes` with its win32-input-mode Shift+Enter records for PSReadLine. The protocol's flags do not affect it.

The rule that `Ctrl+C` copies a selection is decided before the encoder, so it is unchanged. With no selection, `Ctrl+C` is `CSI 99;5u` under flag 1, which is what the requesting program asked for.

**There is no setting.** Windows Terminal ships a toggle because its implementation was new (#19995). The escape hatches that exist belong to the programs themselves (for example `CODEX_TUI_DISABLE_KEYBOARD_ENHANCEMENT`), and a program that does not ask is never affected.

---

## 6. Tests

### 6.1 `bt-term` (parser and state)

These tests run on `TerminalAdapter`, observing `modes().keyboard` and `take_pty_writes()`, unless they say otherwise:

- push/pop/set/query: `CSI > 1 u` → 1; `CSI ? u` → `CSI ? 1 u`; `CSI = 0 u` → 0 and the query answers 0 (the vendor's defect 2); `CSI = 1 ; 2 u` then `CSI = 1 ; 3 u`; `CSI < u` on an empty stack → 0; `CSI < 5 u` past the bottom → 0.
- **pop zero**: `CSI > 1 u` then `CSI < 0 u` → still 1, and the stack still holds one saved value (a following `CSI < u` → 0). `CSI < u` (omitted) pops one.
- masking: `CSI > 31 u` → 1, the query answers `CSI ? 1 u`, and the session reports the refused bits `0b11110` once (a second identical push adds no line). `CSI > 257 u` → 1, and the refused bits reported are `0b1_0000_0000` (bit 8 survives to the diagnostic, which the vendored `vte` arm 3 exists for).
- **the cap, on the state model itself** (a unit test of the per-screen `KeyboardState` with the mask set to all bits, because under the adapter's mask every value is 0 or 1 and eviction would be invisible): from `current = 0`, push 1, 2, … 9. The ninth push evicts the oldest saved value (0), leaving saved `[1..8]` and `current = 9`. Seven pops reach `current = 2` with saved `[1]`; the eighth pop empties the stack and resets `current` to **0** (kitty's empty-stack rule), not 1. The same unit shows `pop(0)` leaves everything as it was.
- the cap, through the adapter: 1,000 pushes leave at most 8 saved values, and the title stack is untouched both when it holds entries and **when it is empty** (defect 1's panic).
- two screens, each of `?47`, `?1047` and `?1049` separately: push 1 on the primary; `h` → 0, and the query answers 0; push 1; `l` → 1; `h` again → 0, because the alternate screen starts empty (Folio's policy, defect 4). A `CSI = 1 u` on the primary survives a round trip through the alternate screen (defect 3).
- `RIS` clears both screens and modifyOtherKeys.
- modifyOtherKeys: `CSI > 4 ; 2 m` → Two; `CSI ? 4 m` → `CSI > 4 ; 2 m`; `CSI > 4 m` → Off; after `CSI > 4 ; 1 m`, **`CSI > m` → Off** (vendored `vte` arm 2); `CSI > 4 ; 3 m` changes nothing.
- DECSTR: after `CSI > 1 u` and `CSI > 4 ; 2 m`, `CSI ! p` leaves both as they were (§2.1's stated divergence; the test names it).
- reply order: `CSI ? u CSI c` → the kitty reply, then DA1.
- the resize oracle: a query fed while a resize is armed is answered once.
- ConPTY's teardown bytes: `a_teardown_that_conpty_re_enables_does_not_type_at_the_pane` still passes, and afterwards the state is 0 / Off.
- OSC 133 does not touch the keyboard state: after `C`, `CSI > 1 u`, `A` on the primary screen, the flags are still 1 (the §2.4 negative trace, in the shape this ticket guarantees).

### 6.2 `bt-app` (the encoder)

- **The table is data.** `crates/bt-app/src/key_encoding.tsv` holds the rows of §4.4: key, location, modifiers, DECCKM, mode, and the bytes as escaped text. `input::tests::every_row_of_the_key_encoding_table` runs each row through `keyboard_bytes`. §4.4 of this note is the table's first version; after the ticket, the TSV is the source. `scripts/dev/generate-key-encoding-table.ps1` generates `docs/key-encoding.md` from it, and a test keeps the two equal. This follows the pattern of `window_waits.tsv` and ARCHITECTURE §5.3. `(table)` rows are checked against `Shortcuts::lookup` instead, because the chord is claimed and the encoder is never asked.
- **An independent oracle beside the TSV.** A wrong TSV would make the generated document and the exhaustive test agree with each other, so `input::tests::the_protocols_normative_cases` asserts, as literals in Rust source and not read from the TSV, the cases the specs themselves state: Esc → `CSI 27u`; Ctrl+I → `CSI 105;5u` while Tab → `\t`; Ctrl+M → `CSI 109;5u` while Enter → `\r`; Shift+Tab → `CSI 9;2u`; Alt+a → `CSI 97;3u`; Ctrl+Space → `CSI 32;5u`; Numpad Enter → `CSI 57414u`; Ctrl+Enter → `CSI 13;5u`; plain Up under DECCKM → `CSI A`; F1 → `CSI P`, F3 → `CSI 13~`; and for modifyOtherKeys 2, Ctrl+Enter → `CSI 27;5;13~`, Shift+a → `CSI 27;2;65~`, Ctrl+Shift+a → `CSI 27;6;65~`; for mode 1, Ctrl+a → `\x01` and Alt+a → `CSI 27;3;97~`.
- Some cases are tested separately rather than as plain table rows:
  - every letter `a`–`z` under C and AC in all four modes, generated so that no letter is special;
  - a non-ASCII key: `é` with C → `CSI 233;5u`;
  - `key_without_modifiers` on a Russian layout: `ф` → 1092;
  - AltGr text: with Ctrl and Alt absent, it is text in every mode;
  - Super on both platforms → nothing, in every mode;
  - macOS Option with the setting off (text) and on (`CSI 97;3u`).
- **The golden replay.** Feed a `TerminalAdapter` the bytes crossterm sends for Codex on Unix: `CSI ? u`, then `CSI > 7 u` (`DISAMBIGUATE | REPORT_EVENT_TYPES | REPORT_ALTERNATE_KEYS`). Then encode using its `modes().keyboard`. Expected: plain Enter → `\r`, Ctrl+Enter → `CSI 13;5u`, Shift+Enter → `CSI 13;2u`, Esc → `CSI 27u`, `a` → `a`, and no release event for any key. Then feed Codex's exit bytes (`CSI < 1 u`, `CSI < u`, `CSI > 4 ; 0 m`), after which Ctrl+Enter is `\r` again. Repeat with Claude Code's `CSI > 5 u` + `CSI > 4 ; 2 m`: kitty wins, so Ctrl+Enter is `CSI 13;5u`, not `CSI 27;5;13~`.
- **A manual platform check on the Mac mini**: winit's `logical_key` and `key_without_modifiers` for `Ctrl+Shift+1`, `Ctrl+Shift+[` and `Ctrl+Shift+a` on the US layout, to confirm §4.3's `k` there. It is run by hand in Folio's own checkout on that machine and touches nothing outside it; it neither needs nor authorises any change to anything else running there.

### 6.3 Through a real ConPTY (Windows)

`crates/bt-pty/tests/keyboard_protocol_through_conpty.rs` follows the pattern of `color_query_through_conpty.rs`: the child is spawned the way a pane spawns it. **Every outcome below is an assertion (pass/fail), not an observation.**

1. **The request crosses.** A PowerShell child writes `\e[?u\e[>1u`. Both sequences arrive in the bytes the pty hands us, the session's flags become 1, and the reply `\e[?0u` reaches the child.
2. **A key-record reader** (PowerShell, `[Console]::ReadKey($true)` in a loop, printing `Key`, `Modifiers` and `KeyChar`):
   - sent `\e[13;5u`, it reads the characters `ESC [ 1 3 ; 5 u` as seven key events and no `Enter`+`Control` — asserted, because it is the fact that makes §1's third row true;
   - sent `\e[27;5;13~`, it reads nothing;
   - sent Folio's win32-input-mode record pair for Ctrl+Enter (built like `SHIFT_ENTER_RECORDS`), it reads `Enter` with `Control`. This is the transport T-KEYBOARD-RECORDS uses (§7.3).
3. **A VT-input reader** (the same script after `SetConsoleMode(…ENABLE_VIRTUAL_TERMINAL_INPUT)`, reading `[Console]::In`): sent `\e[13;5u`, it reads exactly those bytes; sent `\e[27;5;13~`, it reads exactly those bytes.
4. **A native Node raw-stdin reader** — the stand-in for Claude Code on Windows, whose transport this is: `node -e` with `process.stdin.setRawMode(true)`, writing `\e[?u` and printing every chunk it reads as hex. It receives `\e[?0u` for its query, and sent `\e[13;5u` it reads exactly those seven bytes. This case needs `node` on `PATH`. It runs in the ticket's acceptance run on the release machine, where Node is installed; when `node` is missing the test **fails** with that reason rather than skipping, and CI selects it out by name rather than letting it pass silently.

If 4 fails, §1's second row is wrong, and the ticket stops and reports before any CHANGELOG line names Claude Code on Windows.

### 6.4 Real programs (the ticket's acceptance, on the candidate build)

| Program | Where | Expected (pass/fail) |
|---|---|---|
| Codex TUI, with the reporter's config (Ctrl+Enter submits, Enter inserts a newline) | macOS; WSL | Ctrl+Enter submits, Enter inserts a newline |
| Codex TUI, same config | Windows native | **unchanged by this ticket**: Ctrl+Enter inserts a newline, as today. T-KEYBOARD-RECORDS owns this row, where it becomes "Ctrl+Enter submits" (§7.3) |
| neovim, with `:inoremap <C-CR> …` and a `:map <C-i>` distinct from `<Tab>` | macOS; WSL | both fire; Esc leaves insert mode immediately (no `ttimeout` wait) |
| fish, with `bind ctrl-enter …` and `bind ctrl-i …` | macOS; WSL | both fire; after running `nvim` and `:q`, Ctrl+C at the prompt clears the line |
| PowerShell 7 and 5.1 + PSReadLine | Windows | byte-identical to today after this ticket (Ctrl+Enter, Shift+Enter and Alt+Enter run the line). T-KEYBOARD-RECORDS changes this on purpose (§7.3, gate 3) |
| Claude Code, default keybindings | macOS; WSL; Windows native | it answers `CSI ? u` and pushes `CSI > 5 u`; Shift+Enter inserts a newline and Enter submits. On Windows native this row passes if and only if §6.3 case 4 passes, and it is not released as fixed otherwise |
| `kitten show-key -m kitty` | macOS | matches §4.4's kitty column for the ten keys and the F1–F4 rows |
| `cat -v` / `showkey -a` | macOS; WSL | the legacy column is unchanged when no program has asked |
| a program killed with flags on, then §2.4's remedy | WSL (`reset`); Windows pwsh (the `Write-Host` line) | after the remedy, Ctrl+C interrupts again |

---

## 7. Owner rulings and the ticket cut

### 7.1 Rulings (owner, 2026-09-29)

- **Q1 = A.** Folio writes win32-input-mode records on Windows for the chords VT cannot express, in the follow-up ticket T-KEYBOARD-RECORDS, with the three gates of §7.3. This narrows the 2026-09-28 ruling "win32-input-mode not planned": Folio still does not adopt the mode for every key. The PowerShell change (Shift+Enter → *AddLine*, Ctrl+Enter → *InsertLineAbove*, Alt+Enter → nothing, instead of running the line) is **intended**, and the release note discloses it. **#13's wording is confirmed**: Ctrl+Enter submit works in 0.4.7 for Codex on Windows, through the records. #13 closes when T-KEYBOARD-RECORDS ships.
- **Q2 = the spec.** Under flag 1, keypad keys that produce no text send their keypad codes (Numpad Enter → `CSI 57414u`), as §4.2 rule 5 says.
- **Q3 = its own small ticket.** `Ctrl+Space` → NUL is T-CTRL-SPACE, not part of T-KEYBOARD-PROTOCOL.

Record of the alternative not taken: *B — keep "win32-input-mode not planned", leave #13 open and narrowed to key-record programs on Windows, and ship "Codex, Windows native: unchanged".*

### 7.2 The tickets, in order

| # | Ticket | Size | Platform | Depends on |
|---|---|---|---|---|
| 1 | **T-FKEYS** — legacy F1–F12 | S | both | — (prerequisite) |
| 2 | **T-KEYBOARD-PROTOCOL** — kitty flag 1 and modifyOtherKeys | M–L | both | T-FKEYS |
| 3 | **T-KEYBOARD-RECORDS** — win32-input-mode records for the chords VT cannot express | M | Windows only | T-KEYBOARD-PROTOCOL |
| 4 | **T-CTRL-SPACE** — `Ctrl+Space` / `Ctrl+Shift+Space` → NUL (F-2) | S | both | — |
| 4 | **T-ALT-BACKSPACE** — `Alt+Backspace` → `\e\x7f` (F-3), with its ConPTY behaviour checked | S | both | — |

T-CTRL-SPACE and T-ALT-BACKSPACE are independent of each other and of 1–3. Each changes a legacy byte for programs that never ask, so each carries its own acceptance, and whichever of them lands after T-KEYBOARD-PROTOCOL updates `key_encoding.tsv`'s legacy column in the same commit.

**T-FKEYS (S).** F1–F12 get their legacy forms (`SS3 P/Q/R/S`, `CSI 15~ … 24~`, with `CSI 1;m X` / `CSI n;m ~` for modifiers), with table rows and tests. It lands first so that flag 1's F1–F4 rule has something to change, and so that the claim "supports the first tier" is complete. It is its own ticket because it changes bytes for programs that never ask.

**T-KEYBOARD-PROTOCOL (M–L).** The parser half is small but includes a vendored `vte` with three changed arms. The encoder half is a table-driven rewrite of one function plus its call site. The real-ConPTY test, with four readers, is the largest single piece. Two commits on one branch, merged together:

1. `vendor/vte` + `vendor/alacritty_terminal` + `bt-term`:
   - the vendored `vte` and its three arms (§3);
   - kitty's state model per screen, with the cap, the pop rules and the mask (§2.2), and `SUPPORTED_KITTY_FLAGS = 1`;
   - the refused-bits event (full `u16`) and its log line;
   - modifyOtherKeys, its query reply and `CSI > m`;
   - `RIS`, the alternate-screen policy, DECSTR preserved (§2.1);
   - `TerminalModes::keyboard`;
   - the tests of §6.1 and §6.3.
2. `bt-app`:
   - `keyboard_bytes` takes `(key, key_without_modifiers, location, modifiers, application_cursor_mode, keyboard)`;
   - the rules of §4.2 (F1–F4's flag-1 forms and the keypad codes included) and §4.3;
   - `key_encoding.tsv`, the generator, `docs/key-encoding.md`, and the independent oracle;
   - the tests of §6.2.

It is not two tickets, because each half on its own is worse than neither. The state on its own makes Folio answer `CSI ? u`, so programs push flag 1 and then receive legacy bytes they were told they would not get; Claude Code, for one, would switch on its extended-keys mode for nothing. The encoder on its own has no mode to read. Its promise is literal: a program that never asks receives exactly the bytes it received before. PowerShell is byte-identical after this ticket, and changes only with T-KEYBOARD-RECORDS.

Out of scope for T-KEYBOARD-PROTOCOL:

- kitty flags 2, 4, 8 and 16;
- win32-input-mode records (T-KEYBOARD-RECORDS);
- the Alt+Enter ESC-prefix hack (owner ruling 2026-09-28);
- recovery from a dead program's flags at the prompt (§2.4: withdrawn, with the negative trace);
- DECSTR itself (§2.1);
- a user setting;
- lock-modifier bits;
- Ctrl+Space and Alt+Backspace (their own tickets).

### 7.3 T-KEYBOARD-RECORDS (M, Windows only)

On Windows, a chord whose legacy bytes cannot tell it apart is written as a win32-input-mode down/up record pair, built like `SHIFT_ENTER_RECORDS`. These chords are a modified Enter, Tab, Backspace, Escape or Space, and Ctrl with a key that has no C0 code. Every other key keeps its VT bytes. The set is a column of `key_encoding.tsv` ("records"), so the same table and oracle cover it. Three gates, all required:

1. **Track `?9001h`.** The adapter records whether win32-input-mode is currently requested. ConPTY requests it at the start of the session and again after a `RIS`, and a nested ConPTY can turn it off. Records are written only while it is on **and** the pane's kitty flags and modifyOtherKeys are both 0. "No program asked for kitty or modifyOtherKeys" alone is not proof that the transport is there. When a program has asked for kitty or modifyOtherKeys, that program's protocol wins, as §4.3 already rules.
2. **Assert the result through a real ConPTY for all three readers**, extending §6.3's test:
   - a key-record reader gets `Enter` + `Control` for Ctrl+Enter, and likewise for each chord in the set;
   - a VT-input reader gets what ConPTY's own encoder produces for that record, asserted exactly, so that a change in ConPTY is caught;
   - a native Node/libuv reader gets bytes asserted exactly, and no worse than today's `\r` for Ctrl+Enter.
3. **The PowerShell change is intended behaviour, and the release note discloses it.** In PSReadLine's default Windows edit mode, Shift+Enter is `AddLine` and Ctrl+Enter is `InsertLineAbove`. Today Folio collapses both to Enter, which runs the line. After this ticket they do what PSReadLine binds them to, as in Windows Terminal. Alt+Enter is unbound there, so it does nothing instead of running the line. The PowerShell acceptance row is these three expectations, on PowerShell 7 and 5.1.

Acceptance, on the candidate build: Codex on Windows native, with the reporter's configuration: Ctrl+Enter submits and Enter inserts a newline. This row moves here from §6.4. The PowerShell row is as in gate 3. Claude Code on Windows native keeps T-KEYBOARD-PROTOCOL's row, because it asks for kitty, and kitty wins over records.

### 7.4 Texts for the tickets to write when they land

**DESIGN.md entry for T-KEYBOARD-PROTOCOL** (English, dated, at the end, like the entries before it):

> ### 2026-09-xx — A program that asks for the kitty keyboard protocol's disambiguate tier, or for xterm's modifyOtherKeys, gets it, and Folio answers the query with what is in force
>
> 0.4.7 ticket T-KEYBOARD-PROTOCOL; design note `docs/plans/design/keyboard-protocol-2026-09-29.md` (revision (c)) and its Codex review; owner rulings of 2026-09-29; issue #13. Each pane's terminal keeps, per screen, the kitty flags in force and a stack of eight saved values (`CSI > u`, `CSI < u` — an explicit `0` pops nothing —, `CSI = u`, `CSI ? u`); only flag 1 is honoured, other bits are dropped where they are handled and named once in `diagnostics.log`. modifyOtherKeys (`CSI > 4 ; v m`, `CSI > m` resets it, `CSI ? 4 m`) is one value per terminal. `vte` is vendored for those three arms. By Folio's policy the alternate screen starts with no flags and its flags end with it (Windows Terminal does the same; Ghostty and WezTerm keep them); `RIS` clears both screens and modifyOtherKeys; `DECSTR`, which Folio does not implement, changes neither — unlike xterm, which resets modifyOtherKeys there. Nothing at a prompt touches the state: a program that dies with flags on leaves them until `reset`, a pop, or a new shell. The encoder reads the mode from `TerminalModes::keyboard` and encodes from `key_encoding.tsv` (generated into `docs/key-encoding.md`, with an independent oracle beside it): under flag 1, Esc and every Alt, Ctrl and modified Enter/Tab/Backspace/Space chord is `CSI code;m u` with the un-shifted key as the code, plain Enter/Tab/Backspace stay `\r \t \x7f`, arrows ignore DECCKM, F1–F4 take their CSI forms, and non-text keypad keys take their keypad codes (owner ruling Q2); under modifyOtherKeys, `CSI 27;m;k~` (xterm's `formatOtherKeys=0`) for xterm's classes; kitty wins when both are set. Super is never encoded; Option follows *Option key sends Alt*. A program that never asks — PSReadLine, cmd, Codex on Windows — receives exactly the bytes it received before; on Windows, T-KEYBOARD-RECORDS is what reaches those.

**CHANGELOG line for T-KEYBOARD-PROTOCOL** (under *Added*):

> - Programs that ask for it can tell Ctrl+Enter, Shift+Enter, Alt+Enter, Shift+Tab, Ctrl+I, Ctrl+M and Esc apart from Enter, Tab and a lone escape: Folio supports the first tier of the kitty keyboard protocol and xterm's modifyOtherKeys. neovim, fish, helix and Claude Code ask for it, and so does Codex on macOS and in WSL. Programs that do not ask are unaffected.

**CHANGELOG lines for T-KEYBOARD-RECORDS** (under *Added* and *Changed*; the release note carries the second one as well):

> - On Windows, Ctrl+Enter, Shift+Enter and Alt+Enter reach console programs such as Codex as those keys, so a Codex set up to submit with Ctrl+Enter now does.
>
> - In PowerShell, Shift+Enter now adds a line and Ctrl+Enter inserts one above, as in Windows Terminal, instead of running the command; Alt+Enter no longer runs it either. Press Enter to run.

The "first tier" wording assumes T-FKEYS has merged first, as §7.2 orders it.

**Issue #13.** The reply of 2026-09-28 stands as written: Ctrl+Enter submit works in 0.4.7, through the records. The issue is closed by T-KEYBOARD-RECORDS's merge, not by T-KEYBOARD-PROTOCOL's.

---

## 8. Found along the way

- **F-1** `keyboard_bytes` has no case for F1–F12, so a function key sends nothing to the child today, in every program. **T-FKEYS**, the prerequisite (§7.2).
- **F-2** `Ctrl+Space` and `Ctrl+Shift+Space` send nothing. winit reports these as `NamedKey::Space`, and the control-alphabet case never sees them: it was written for `Key::Character`, even though its comment says `Ctrl+Space` is NUL. **T-CTRL-SPACE** (owner ruling Q3) makes them NUL (0x00), as kitty's legacy table, xterm and the existing comment all say.
- **F-3** `Alt+Backspace` sends `\x7f` with no ESC, so readline's backward-kill-word (`\e\x7f`) cannot be typed. Behind ConPTY, `ESC DEL` becomes an Alt+Backspace record, which PSReadLine also binds. **T-ALT-BACKSPACE**, with the same care as the Alt+Enter ruling: the ticket checks what PSReadLine does with it before changing the byte.

---

## 9. Questions for the owner

None open. Q1, Q2 and Q3 were ruled on 2026-09-29 (§7.1).

---

## Revision (b), 2026-09-29 — what changed after Codex's review

| Review finding | Change in this note |
|---|---|
| 1. Windows claims confirmed; the ticket does not fix #13's reporter | §0 item 9 and Q1 say T-KEYBOARD-PROTOCOL does not close #13; §7 writes out both branches — A with the review's three gates (track `?9001h`; assert record, VT and Node/libuv readers through a real ConPTY; the PSReadLine change stated as intended), B with #13 narrowed and "Codex, Windows native: unchanged" as the expected row |
| 2. OSC 133 `C`→`A` restore is not ownership-safe | Withdrawn from §0, §2.1, §2.4, §6, §7, the DESIGN entry and the CHANGELOG. §2.4 records the nested-subshell negative trace, the crash-recovery gap, and what the person does (`reset`; a `Write-Host` line or *Restart shell* in PowerShell; a Folio reset verb as a possible later door); §6.1 pins that OSC 133 leaves the state alone |
| 3. `CSI > m`, explicit `CSI < 0 u`, the cap test, large flag values, DECSTR | `vte` vendored for three arms (§3): `CSI > m` → modifyOtherKeys Off, explicit `0` pop is a no-op, flags reach the terminal as `u16` so the diagnostic names bits 8–15. Pop rule restated with the empty-stack reset (§2.2). Cap test corrected — seven pops reach 2, the eighth resets to 0 — and moved onto the unmasked state model (§6.1). DECSTR: kitty state preserved, modifyOtherKeys not reset, stated as a divergence from xterm (§2.1) |
| 4. Alternate-screen clearing is Folio policy; defect 1 wording | §2.1 names the choice (WT clears; Ghostty and WezTerm preserve); §6.1 tests `?47`, `?1047`, `?1049` separately; §2.2 defect 1 now says title-stack corruption or a panic when it is empty; the cap of 8 is attributed as a choice (WT, Ghostty 8; WezTerm 128) |
| 5. Attribution | modifyOtherKeys precedence cites kitty, Ghostty and WezTerm, not WT (§4.3); the vte reference table is named as the `formatOtherKeys=1` evidence for the classes, the wire form as xterm's default `formatOtherKeys=0`; "published" replaced by "read directly from the session-owned terminal state" (§0, §2.3) |
| 6. Thread-door claim | unchanged (the review passed it); wording as in 5 |
| 7. Size, tests, scope | size M–L; an independent hard-coded oracle beside the TSV (§6.2); explicit pop-zero test; all four ConPTY outcomes are assertions, including a native Node raw-stdin reader (§6.3); the Claude Code row has a pass/fail expectation (§6.4); F1–F4's flag-1 forms are in this ticket, on top of T-FKEYS as a prerequisite (§4.2 rule 7, §4.4, §7), and the CHANGELOG names the chords; Alt+Backspace stays separate (F-3); the Mac check is manual and touches nothing outside Folio's checkout there (§6.2) |

## Revision (c), 2026-09-29 — the owner's rulings

- §7 now carries the rulings, dated (Q1 = A, Q2 = the spec, Q3 = its own ticket). The B branch is reduced to a one-line record.
- The ticket cut is T-FKEYS (S, prerequisite) → T-KEYBOARD-PROTOCOL (M–L) → T-KEYBOARD-RECORDS (M, Windows only) → T-CTRL-SPACE and T-ALT-BACKSPACE (S). T-KEYBOARD-RECORDS has its own section (§7.3), with the three gates and its acceptance.
- The PowerShell change is stated as intended and gets a release-note line. #13's wording is confirmed, and the issue closes with T-KEYBOARD-RECORDS.
- §0, §4.2, §6.4, §8, §9 and the DESIGN pointer entry follow the rulings.

## Revision (d), 2026-09-29 — where the code made T-KEYBOARD-PROTOCOL deviate from the text above

Each is what the implementation does and why; none changes a decision of §0.

- **§3, "exactly three arms".** Arm 1 has to tell an omitted pop count from an explicit `0`, and `vte` 0.15's `Params` cannot: the parser pushes `0` for both (`action_csi_dispatch` always pushes the pending parameter). So the vendored `vte` also records, per entry, whether a digit was written (`Params::written`, set by the parser in `src/lib.rs` and `src/params.rs`). It is additive — `Perform` is unchanged, so `bt-corpus` is unaffected — and it is the only change beyond the three arms (`vendor/vte/CHANGES-FOLIO.md`). `CSI > 0 m` therefore stays unhandled (it names resource 0) while `CSI > m` resets.
- **§2.1 and §6.1, `?47` and `?1047`.** In this terminal neither mode switches screens: both reach the vendored terminal's unknown-mode branch, and `session.rs`'s `nested_and_unimplemented_screen_modes_leave_the_claim_where_the_screen_is` pins that. The policy lives in `swap_alt`, which the table already names ("whatever reaches `swap_alt`"), so for these two the flags simply stay those of the screen that is still showing. The §6.1 test asserts that for `?47` and `?1047`, and the entry-empty/exit-discarded policy for `?1049`. Implementing `?47`/`?1047` is not this ticket's.
- **§4.2, rule order.** Rule 3 (plain Enter is `\r`) would pre-empt rule 5 for Numpad Enter, which §4.4 and owner ruling Q2 send as `CSI 57414u`; and rule 4 (arrows) would pre-empt rule 5 for a keypad arrow with Num Lock off, which §4.4 sends as `CSI 57419u`. The keypad rule is therefore asked before both; the table is what the code answers to.
- **§4.2 rule 1, "(unchanged)", and Super.** Under either protocol a chord holding Super sends nothing. Without one, the legacy encoder is unchanged byte for byte, and it has always sent `\r` for `Super+Enter` (and the other named keys ignore Super there); §7.2's promise that a program which never asks receives exactly what it received before takes precedence, so that stays, pinned by `key_encoding_legacy_{windows,macos}.tsv`.
- **§3, where the refused bits are ORed.** The vendored terminal sends the whole request with an event, the adapter forwards it as `AdapterEvent::KeyboardFlagsRefused`, and the per-session `u16` and the one-line queue live in `DualPlaneSession` (`keyboard_flags_refused`, `take_keyboard_protocol_notes`); the application's drain writes each line with `diagnostics::note`. The adapter is replaced wholesale by a resize reconcile, the session is not.
- **§6.2, the byte-identical test.** The pre-ticket outputs were captured from the encoder at main `5601a484` over a sweep wider than §4.4 — 74 keys × Shift/Alt/Ctrl/Super × DECCKM, 2,368 rows — into `crates/bt-app/src/key_encoding_legacy_windows.tsv`, and on the Mac mini from the same commit into `key_encoding_legacy_macos.tsv` (the pre-ticket encoder already differed by platform: `Shift+Insert` and `Ctrl+V` are paste chords off a Mac, `Alt+F4` is Windows' close), which `input::tests::a_program_that_never_asked_gets_exactly_the_bytes_it_got_before` holds today's encoder to.
- **§6.3 case 3** reads with `ReadConsoleW` after `SetConsoleMode`, which is the call `[Console]::In` reads through; reading it directly keeps .NET's buffering out of what is asserted.

## Revision (e), 2026-09-29 — where T-KEYBOARD-RECORDS' measurements made it deviate from §7.3

Each is what the implementation does and why; none changes a decision of §0 or the owner's ruling Q1.

- **§7.3, "a modified … Escape".** Through the vendored ConPTY, a record for Escape with Ctrl or Alt (any of the six chords that hold either) reaches no reader: the key-record, VT-input and Node readers of `keyboard_protocol_through_conpty.rs` all read nothing for it, and the test pins that. Its legacy ESC reaches every reader as Escape. So Escape with Ctrl or Alt keeps its ESC and only Shift+Escape is a record; Windows takes Ctrl+Esc, Alt+Esc and Ctrl+Shift+Esc for itself in any case.
- **§7.3 gate 1, `RIS`.** Measured, not assumed: a child's `ESC c` passes through ConPTY to Folio and ConPTY follows it at once with `CSI ? 1004 h CSI ? 9001 h`. The terminal's `RIS` clears the mode with every other one and ConPTY's own bytes set it again, so records continue after a `reset`.
- **§7.3, "the 'records' column of `key_encoding.tsv`".** The TSV is one row per key, modifiers, DECCKM and mode, so the column is a fifth mode, `records`, with a row for every chord the table has; `docs/key-encoding.md` shows it as the column *Windows records*. A chord outside the set carries its legacy bytes there.
- **§7.3, "built like `SHIFT_ENTER_RECORDS`".** The record's fields are the press as Windows reported it rather than a table per chord: the virtual key is fixed for the five named keys and the installed layout's for a text key's scan code (`bt_platform::virtual_key_of_scan_code`), a keypad digit or decimal key that typed text is `VK_NUMPADn`/`VK_DECIMAL`, the scan code comes from winit's physical key, and the character is winit's `text_with_all_modifiers` (Windows' `WM_CHAR`). On a US layout this gives Shift+Enter exactly `SHIFT_ENTER_RECORDS`, which a test asserts. The US answers the tests use were measured with `ToUnicodeEx` and `MapVirtualKeyExW` against the US layout.

Added after Codex's review of the branch (`KR-review-codex-2026-09-29`, round 2):

- **§7.3, "Ctrl with a key that has no C0 code", on Windows.** winit keeps Ctrl while Alt is down, because Ctrl+Alt may be AltGr (`WindowsModifiers::remove_only_ctrl`, winit 0.30.13 `platform_impl/windows/keyboard_layout.rs`, applied in `keyboard.rs`'s key-event builder), and when `ToUnicodeEx` types nothing for that state it hands the key over as `Key::Unidentified(NativeKey::Windows(vk))`. On the US layout that is every Ctrl+Alt and Ctrl+Shift+Alt chord on all 47 text keys (measured). Such a press has no character for any rung, so the legacy encoder — and either protocol — sends nothing for it, whatever C0 code the letter would have had. The encoder therefore writes a record for Ctrl (with or without Shift and Alt) on a key whose key without modifiers is text and which has a scan code, carrying the virtual key Windows reported; a media key or a key with no position is not one. The first round's tests had modelled these chords as characters and so promised records the real event never produced; the sweep and the table now build them as winit does. That the kitty and modifyOtherKeys encoders also send nothing for these chords on Windows is T-KEYBOARD-PROTOCOL's, reported, not changed here.
- **§7.3 gate 2, "each chord in the set".** The real-ConPTY test sends every chord the encoder writes on a US layout — 155 of them: the captured sweep's 158 and Numpad Enter's 7, less the 10 the shortcut table claims — as the encoder's own bytes (`crates/bt-app/src/key_records_windows_us.tsv`, which a `bt-app` test holds equal to the encoder), plus the six swallowed Escape chords.
- **The translations byte readers see (owner ruling Q1 = A; coordinator, 2026-09-29: parity with Windows Terminal is the criterion).** A key record is a key event; a program that reads the console as VT (WSL's relay) or through libuv (Node) gets that event back as bytes from ConPTY's own encoder, which is the encoder Windows Terminal's users get for the same records. **A byte reader behind ConPTY sees what it sees under Windows Terminal.** Some of those bytes differ from Folio's legacy ones, and that is intended: Ctrl+1 now reaches such a reader as `1` (0x31) where legacy sent nothing; Ctrl+Shift+[ as ESC (0x1B) where legacy sent nothing; Ctrl+Backspace as BS (0x08) where legacy sent DEL; Ctrl+3 as ESC, Ctrl+8 as DEL, Ctrl+2 and Ctrl+\` as NUL to a VT reader (nothing to libuv), Ctrl+/ as 0x1F; Ctrl+Alt+<letter> as ESC and the letter's C0 code where legacy on Windows sent nothing. The full table is `READ_BACK` in the test.
- **Which ConPTY those bytes are for.** The literals are the ConPTY Folio ships, the vendored `conpty.dll`/`OpenConsole.exe` pair. `BT_CONPTY_FORCE_SYSTEM=1` is a test switch; the product itself uses the ConPTY inbox in Windows only when the packaged pair is missing beside `folio.exe` or fails to load (`load_conpty` in the vendored `portable-pty`). On that path key-record readers are unchanged — every one of the 161 chords reaches `[Console]::ReadKey` as the same key event (measured with `BT_CONPTY_FORCE_SYSTEM=1`, inbox `conhost.exe` 10.0.26100.1) and the Escape records are swallowed there too — while byte readers get the inbox encoder's translations, which differ on 60 of the 161 chords (below; not asserted). The one where a record is worse for a byte reader than the legacy byte is Ctrl+Alt+Enter (and Ctrl+Shift+Alt+Enter): legacy `\r` arrives, the record arrives as nothing; on the vendored ConPTY it arrives as ESC LF.

| chord | vendored VT | inbox VT | vendored Node | inbox Node |
|---|---|---|---|---|
| Ctrl+Shift+Enter (and Numpad) | `0a` | `0d` | `0a` | `0d` |
| Ctrl+Alt+Enter, Ctrl+Shift+Alt+Enter (and Numpad) | `1b0a` | nothing | `1b0a` | nothing |
| Shift+Alt+Tab | `1b1b5b5a` | `1b09` | `1b1b5b5a` | `1b09` |
| Ctrl+Alt+Tab | `1b09` | `09` | `1b09` | `09` |
| Ctrl+Shift+Alt+Tab | `1b1b5b5a` | `09` | `1b1b5b5a` | `09` |
| Ctrl+0 / Ctrl+Shift+0 | `30` / `29` | `10` | `30` / `29` | `10` |
| Ctrl+- | `2d` | `0d` | `2d` | `0d` |
| Ctrl+= / Ctrl+Shift+= | `3d` / `2b` | `1d` | `3d` / `2b` | `1d` |
| Ctrl+; / Ctrl+Shift+; | `3b` / `3a` | `1b` | `3b` / `3a` | `1b` |
| Ctrl+' / Ctrl+Shift+' | `27` / `22` | `07` | `27` / `22` | `07` |
| Ctrl+Shift+, | `3c` | `0c` | `3c` | `0c` |
| Ctrl+. / Ctrl+Shift+. | `2e` / `3e` | `0e` | `2e` / `3e` | `0e` |
| Ctrl+Shift+\` | `1e` | `00` | `1e` | nothing |
| Ctrl+Alt and Ctrl+Shift+Alt with a digit but 2, \`, -, =, [, ], \\, ;, ', , or . (38 chords) | ESC and a byte | nothing | ESC and a byte (ESC alone for Ctrl+Alt+\`) | nothing |

Added after the review's second pass (`KR-review-codex-2026-09-29-b`, round 3):

- **Records are refused on the inbox ConPTY** (coordinator's ruling, 2026-09-29), which supersedes the bullet above that described what byte readers would get there. The table above stays as the reason. A pane learns at spawn which ConPTY it runs on (`bt_pty::PtySession::conpty_kind`: `Shipped`, `Inbox`, or `NotConPty` for a pane with none), the encoder reads it with the key the way it reads `win32_input_mode` (no door, no wait), and `key_records` writes only when it is `Shipped`. On the inbox ConPTY, then, a program that never asked receives exactly the bytes it received before — Ctrl+Enter is `\r` there, as it was. A pane born on it writes one line to `diagnostics.log` naming why the process fell back (`BT_CONPTY_FORCE_SYSTEM` set, `conpty.dll` or `OpenConsole.exe` missing beside the executable, or the DLL failing to load; the vendored `portable-pty`'s `conpty_fallback_reason`). Pinned: `bt-app`'s `a_pane_on_the_inbox_conpty_gets_no_records` and the inbox run of the second-baseline sweep (every chord equals its mode-off bytes, which are the capture); `bt-pty`'s `the_packaged_conpty_is_shipped_and_the_system_one_is_inbox` and `a_spawned_pane_knows_which_conpty_it_runs_on` (under `BT_CONPTY_FORCE_SYSTEM=1`: `Inbox`, the reason naming the switch).
- **A dead key is not a text key.** winit reports a dead key's key without modifiers as the character it composes (`keyboard.rs`: "We convert dead keys into their character"), so on the French layout the dead `^` (`VK_OEM_6`, 221, scan code 26) under Ctrl+Alt arrives in the same shape as a text key and was written as `ESC[221;26;0;1;10;1_…`. The encoder now also asks the installed layout, through `bt_platform::virtual_key_types_a_character` — `MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR)`, whose documented top bit marks a dead key and whose `0` marks a key that types nothing; one synchronous call, no wait — and refuses anything that is not an ordinary character. Pinned by `a_dead_key_under_ctrl_alt_is_not_a_record` (French layout substituted in the test, as the US one is elsewhere).
- **Round 4 (`KR-review-codex-2026-09-29-c`): only "is it dead" is asked.** The round-3 predicate also refused a `0` answer, and `MAPVK_VK_TO_CHAR` answers `0` for ordinary text keys on several layouts — Kazakh `VK_OEM_1` (186, scan code 39) types `ж`, arrives under Ctrl+Alt unidentified with the key without modifiers `ж`, and was wrongly refused. The key without modifiers being text already says the key types; the layout is now asked only whether it is dead: `bt_platform::virtual_key_is_dead`, the top bit of the answer and nothing else (`bt_platform::vk_to_char_marks_a_dead_key`, pure and pinned on `0x8000005E` → dead, `0` → not). `a_dead_key_under_ctrl_alt_is_not_a_record` models both layouts by their map answers: the French `^` is refused, the Kazakh `ж` is `ESC[186;39;0;1;10;1_ESC[186;39;0;0;10;1_`.

## Revision (f), 2026-09-29 — T-KEYBOARD-CTRLALT: the protocols read Ctrl+Alt on Windows too

Revision (e) left a finding open: on Windows a Ctrl+Alt or Ctrl+Shift+Alt chord on a text key the layout types nothing for arrives as `Key::Unidentified(NativeKey::Windows(vk))`, and the kitty and modifyOtherKeys encoders, which knew a text key only as `Key::Character`, sent nothing for it where §4.4 promised `CSI 101;7u` and `CSI 27;7;101~`. The records rung's rule for such a press is now one function the three rungs share (`input::unidentified_text_key`: Ctrl held, the key without modifiers is text, the key has a scan code, the layout does not mark the virtual key dead). Under flag 1 the code is the key without modifiers (§4.2 rule 6). Under modifyOtherKeys the class is the key without modifiers' and `k` is that character, or with Shift held the character the installed layout types on the key with Shift alone (§4.3's "Shift applied"), asked of the layout with `ToUnicodeEx` and only Shift down (`bt_platform::shifted_character_of_virtual_key`), since the press carries no character. The `AC`/`SAC` protocol cells of `e i m [ 1` in `key_encoding.tsv` are §4.4's again and hold on Windows as winit hands the press over; the legacy cells stay `—` on Windows and the records cells are unchanged. A dead key sends nothing under every protocol.

Round 2 (`KC-review-codex-2026-09-30`): flag bit 2 keeps a pending dead key but does not keep it out of the translation, so after the French `^` a Shift-only `ToUnicodeEx` of `VK_E` answers `Ê`. The Shift character now comes from a per-layout table translated once while no dead key is pending (detected by translating Space against `MapVirtualKeyExW(VK_SPACE, MAPVK_VK_TO_CHAR)`) and built ahead of the chord with every key press; a layout with no table yet, asked while a dead key is pending, answers nothing. Pinned on the real French layout by `bt-platform`'s `a_pending_dead_key_leaves_the_shift_character_alone`, and in the encoder by `after_a_dead_key_ctrl_shift_alt_e_is_still_xterms_e` (`CSI 27;8;69~`).

Round 3 (`KC-review-codex-2026-09-30-b`): the Space probe is not a proof and the cold case dropped a chord. Measured: the dead-key state is not per thread (a fresh thread's `ToUnicodeEx` composes with, and a flags-0 call there consumes, a dead key another thread left pending), so no helper thread gives a clean translation. The Shift character is now read from the layout DLL's own `KbdLayerDescriptor()` tables (`kbd.h`), named through `GetKeyboardLayoutNameW` and the `Layout File` registry value and loaded from `System32` — state-free by construction, once per layout, then a map lookup. Pinned by `bt-platform`'s `a_layouts_shift_characters_are_read_from_its_tables` (US and French, no desktop needed) and `a_pending_dead_key_leaves_the_shift_character_alone` (a real pending `^`; fails by name where no dead-key layout can be loaded), and in the encoder by `after_a_dead_key_ctrl_shift_alt_e_is_still_xterms_e` through the product's lookup.

## Revision (g), 2026-09-30 — T-KEYBOARD-CTRLALT round 4: the layout table is a worker answer

Round 3 put a registry read and a first-use `LoadLibraryExW` on the window thread, which is a window-thread wait under ARCHITECTURE §5.3 even though the image is in process. The application now owns one per-HKL table map and a dedicated `folio-layout-tables` observation worker: its startup pass builds every HKL returned by `GetKeyboardLayoutList`, and an active HKL absent later is offered through a bounded nonblocking request; the copied table returns before `AppEvent::LayoutTablesReady`, with the next lookup draining too. The registry read and `LoadLibraryExW`/`GetProcAddress`/`FreeLibrary` are the worker-only `bt_platform::keyboard_layout_shift_table` door, pinned by `WorkerCtx` and registered in `window_waits.tsv`'s effects and owners without a §5.3 window-thread row. While a newly seen HKL's table is not known, the chord uses its un-shifted character as `k`. This is Folio's own rule, not xterm's: xterm takes `k` from the keysym its X lookup returns and says nothing about a keysym that is not available yet; only the wire form, `CSI 27;m;k~`, is xterm's `formatOtherKeys=0`. With it the chord still reaches the program as Ctrl+Shift+Alt on that key, where the encoder before revision (f) sent nothing for it on Windows. The table is used after it lands. `the_worker_builds_the_current_layout_at_startup` and `a_missing_layout_falls_back_for_one_chord_then_uses_the_worker_table` drive the same worker seam job by job, with no sleep.

## Revision (h), 2026-09-30 — T-KEYBOARD-CTRLALT round 7: one terminal answer per layout

Codex's fourth review (`KC-review-codex-2026-09-30-d`) found that revision (g)'s "one chord" was not bounded: a full request queue, a worker gone and a door that could not read a table each took a different silent road, and the last one made the chord send nothing for the rest of the process. Every HKL now ends in `Known(table)` or `Unavailable`. `Unavailable` is reached when the door answers `None`, when eight requests are already admitted and unanswered, or when the worker is gone (a `Disconnected` send or drain turns every pending HKL `Unavailable` at once). An `Unavailable` HKL is never asked again and its chords use the un-shifted character, as while pending, until the process ends; every refusal other than the door's `None` writes one `diagnostics.log` line naming the layout and the reason. The answer channel is a `sync_channel` of the startup layout count plus eight, which is the most answers that can be undrained at once. Pinned by `layout_tables::tests::a_layout_refused_because_the_queue_is_full_is_unshifted_for_good_and_noted_once`, `a_worker_gone_before_a_startup_answer_leaves_that_layout_unavailable_and_noted`, `a_worker_gone_before_a_miss_answer_leaves_the_pending_layout_unavailable_and_noted` and `a_layout_the_door_cannot_read_is_unshifted_for_good_without_a_note`; revision (g)'s `a_missing_layout_falls_back_for_one_chord_then_uses_the_worker_table` is now `a_chord_before_its_layouts_table_lands_is_unshifted_then_uses_the_worker_table`.

The same review found that the road was not inert off Windows: `Runtime::create` started `folio-layout-tables` on every platform, which then waited in `recv` for the life of the process and added a thread start that could fail. `LayoutTables::spawn` now decides from the startup list it is given, the platform's `keyboard_layouts()` answer: an empty list — always the answer off Windows — makes an inert `LayoutTables` with no thread and no channels, whose every lookup is `Unavailable` without a request. No `cfg!` decides it in `bt-app`. Pinned by `layout_tables::tests::an_empty_startup_list_starts_no_worker_and_every_lookup_is_unavailable`.
## Revision (i), 2026-09-30 — §2.4's later door exists

T-RESET-MODES answers §2.4's "a Folio verb is a possible later door" with an explicit pane verb rather than a prompt-time rule. **Reset terminal modes** (the pane head's `⌄` menu and the command palette) goes through `TerminalAdapter::reset_program_modes`: it returns to the primary screen, clears the kitty flags and stack (the alternate screen's are discarded by the return, as `?1049l` discards them), sets modifyOtherKeys to off, and also retires the mouse modes, bracketed paste, DECCKM and keypad mode and shows the cursor. Focus reporting goes back to what the transport keeps; win32-input-mode is left alone. It writes nothing to the child. The nested-shell negative trace is unchanged: OSC 133 still never mutates keyboard state, and only a person choosing the verb crosses this door.

## Sources

- kitty keyboard protocol: https://sw.kovidgoyal.net/kitty/keyboard-protocol/ (source `docs/keyboard-protocol.rst`), and kitty's encoder `kitty/key_encoding.c` (https://github.com/kovidgoyal/kitty).
- xterm modifyOtherKeys: https://invisible-island.net/xterm/modified-keys.html; xterm's reference output as recorded in `alacritty/vte` `doc/modifyOtherKeys-example.txt` (https://github.com/alacritty/vte).
- Windows Terminal / ConPTY (`microsoft/terminal` main): PR #19817 *Implement the Kitty Keyboard Protocol*, issue #19847, PR #19995; `src/host/_stream.cpp` (`WriteCharsVT`), `src/host/VtIo.cpp`, `src/terminal/adapter/adaptDispatch.cpp`, `src/terminal/input/terminalInput.cpp`, `src/terminal/parser/InputStateMachineEngine.cpp`.
- Ghostty `src/input/key_encode.zig` (https://github.com/ghostty-org/ghostty); WezTerm key encoding: https://wezterm.org/config/key-encoding.html.
- crossterm `src/event.rs` (https://github.com/crossterm-rs/crossterm); Codex `codex-rs/tui/src/tui/keyboard_modes.rs` (https://github.com/openai/codex); fish `src/tty_handoff.rs`, `src/terminal.rs`, `src/reader/reader.rs` (https://github.com/fish-shell/fish-shell); Claude Code issues anthropics/claude-code#96526 and #97501; warpdotdev/warp#16175.
- In this tree: `crates/bt-app/src/input.rs` (`keyboard_bytes`, `xterm_modifier`, `meta_prefix`, `control_byte`, `effective_modifiers`, `SHIFT_ENTER_RECORDS`), `crates/bt-app/src/runtime/keyboard.rs` (`keyboard_input`), `crates/bt-app/src/shortcuts.rs`, `docs/shortcuts.md`, `crates/bt-term/src/adapter.rs` (`TerminalAdapter::new`, `modes`, `retire_program_input_modes`, `take_pty_writes`), `crates/bt-term/src/session.rs` (the OSC 133 `A` handler), `vendor/alacritty_terminal/src/term/mod.rs`, `vte` 0.15 `src/ansi.rs`, winit 0.30.13 `platform_impl/windows/{keyboard,keyboard_layout}.rs`, `crates/bt-pty/tests/color_query_through_conpty.rs`; `docs/DESIGN.md` §7.1.5a″, §7.1.5b, §7.54d, §13.13, §13.16; `docs/plans/design/thread-door-2026-09-26.md` and `window-thread-budget-2026-09-25.md` §C-5.
