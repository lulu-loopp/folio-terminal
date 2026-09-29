# The kitty keyboard protocol's disambiguate tier and xterm's modifyOtherKeys in Folio's terminal — design note, 2026-09-29

0.4.7 ticket KP-0 (docs only). T-KEYBOARD-PROTOCOL implements it after Codex's review. Base: main `38b5743b`.

The owner's rulings of 2026-09-28 that this note works under: this is the first ticket of 0.4.7; there is **no Alt+Enter ESC-prefix hack** (PSReadLine reads a lone ESC as clear-line, and ConPTY turns `ESC CR` into an Alt+Enter key record); **only the disambiguate tier (flag 1) is built now**, and the other kitty flags wait until a program we care about needs them; **win32-input-mode is not planned**. §1 reports a fact that bears on the third ruling. It is question Q1 in §9, and it does not change what this note decides for T-KEYBOARD-PROTOCOL.

Notation: `CSI` is `\e[`; `\e` is ESC; `\r \t \x7f` are CR, HT, DEL. S, A and C are Shift, Alt and Ctrl. The kitty/xterm modifier number is `1 + S·1 + A·2 + C·4`, plus `Super·8`, which Folio never produces (§4.1).

---

## 0. The decisions, in one screen

1. **State lives in `bt-term`, in the vendored `alacritty_terminal`, per pane and per screen.** The vendor already parses all four kitty sequences and keeps two stacks, but they are switched off by `Config::kitty_keyboard = false`. That switch is why Folio "swallows" the requests today. The ticket turns it on and replaces the vendor's model with kitty's: a current value per screen plus a stack of saved values. The vendor's model has four defects that matter (§2.2). modifyOtherKeys gets a per-terminal field, which the vendor does not have.
2. **Only flag 1 is honoured.** Every other requested bit is dropped where the request is parsed, so the query answers with what is actually in force. The first time a session drops a bit, it writes one line to `diagnostics.log`.
3. **The encoder learns the mode the same way it learns DECCKM.** `TerminalModes` gains `keyboard: KeyboardProtocol { kitty: u8, modify_other_keys: Off|One|Two }`. The window thread reads it from the session it already owns, at the moment of the key. There is no polling, no lock and no new wait.
4. **Encoding follows kitty's own encoder (`kitty/key_encoding.c`) for flag 1 and xterm's reference output for modifyOtherKeys.** Folio's standing policies sit on top: Super never reaches the child, and on a Mac Option types text unless the setting makes it Alt. §4.4 has the full table; the ticket moves it into a TSV that the test and this document share.
5. **A program's leftovers are undone at the next prompt.** When OSC 133 `A` arrives on the primary screen after a `C`, the primary screen's keyboard state and modifyOtherKeys go back to what they were at that `C`. The OSC 133 `A` handler already does this for mouse modes. This version is more precise because a shell may use this mode itself, as fish does.
6. **The shortcut table, the cards, the paste roads and IME composition are untouched.** The encoder is the last rung of the key ladder, and it is the only rung that changes.
7. **One ticket, T-KEYBOARD-PROTOCOL, size M**, with two commits (the bt-term state and the bt-app encoder) merged together. Either half on its own is worse than neither (§7).
8. **Flag 1 cannot reach some programs on Windows** (§1). A program that reads the console's key records never asks for the protocol, and could not read its bytes if it did. This covers Codex, PowerShell/PSReadLine, cmd and anything built on crossterm's Windows backend. **Issue #13's reporter (Codex on Windows 11) is one of them.** Q1 asks the owner whether Folio should send those chords the way Windows Terminal and WezTerm send them to such programs: as a win32-input-mode key record, for those chords only, in a second ticket. Whichever answer the owner gives, the reply on #13 needs correcting.

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
| Node programs on Windows (Claude Code running natively) | Claude Code asks: `CSI ? u`, then `CSI > 5 u` and `CSI > 4 ; 2 m` (anthropics/claude-code#96526, #97501) | the reply and the `CSI … u` keys arrive as characters. libuv reassembles the characters of key records into bytes, so this should work. **The ticket's real-ConPTY test measures it; this note does not assume it.** |
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
| `DECSTR` (`CSI ! p`) | unchanged: `vte` 0.15 does not dispatch `DECSTR` at all today, and this ticket does not add it | unchanged |
| OSC 133 `A` on the primary screen, after a `C` (§2.4) | the primary's state is restored to its value at that `C` | restored to its value at that `C` |
| the child exits; *Restart shell*; a pane restored from `session.json` | a new `LeafSession` and a new adapter: nothing to carry over, and nothing is persisted | same |

Starting the alternate screen empty follows WT's `UseAlternateScreenBuffer` and kitty's note under the stacks paragraph: a full-screen program should be able to change the mode "in the alternate screen only … without … knowing what that mode is". The vendor instead *keeps* the alternate stack from one alternate-screen session to the next, because `swap_alt` swaps two stacks. With the vendor's behaviour, a crashed `nvim` would hand its flags to the next `less`.

### 2.2 Why the vendor's model is replaced rather than switched on

`vendor/alacritty_terminal/src/term/mod.rs` has `keyboard_mode_stack` / `inactive_keyboard_mode_stack`, `push_keyboard_mode`, `pop_keyboard_modes`, `set_keyboard_mode` and `report_keyboard_mode`, all guarded by `config.kitty_keyboard`. Turning that switch on is not enough:

1. **Overflow evicts from the wrong stack.** `push_keyboard_mode` checks `keyboard_mode_stack.len() >= KEYBOARD_MODE_STACK_MAX_DEPTH` and then removes `self.title_stack[0]`. The keyboard stack grows without bound (4096 is the title stack's cap, not this stack's), and the title stack loses an entry.
2. **The query reports the top of the stack, not the flags in force.** `CSI = 5 u` (fish's form, which sets and does not push) changes `TermMode` but no stack entry, so a following `CSI ? u` answers `0`. A program that follows the spec's detection advice ("set the desired progressive enhancements and then query") gets the wrong answer.
3. **A switch between screens recomputes the mode from the top of the stack.** Flags set with `CSI =` on the primary screen are therefore lost on the way back from `less`.
4. The alternate stack survives from one alternate-screen session to the next (§2.1).

The replacement is kitty's model:

- `current` is what is in force.
- `push(f)` saves `current` on the stack (evicting the oldest entry when the stack is at its cap) and sets `current = f`.
- `pop(n)` restores the `n`-th saved value, or `0` if that empties the stack.
- `set(f, mode)` changes `current` only.
- `query` answers `current`.

After every change, the `TermMode::KITTY_KEYBOARD_PROTOCOL` bits are derived from the active screen's `current`, so the rest of the vendor code that reads `TermMode` is untouched. The cap is **8**, WT's `KittyStackMaxSize`. The eviction is the one the spec requires ("the oldest entry from the stack must be evicted").

`vendor/alacritty_terminal` is Folio's own fork and is edited in place, as `set_write_provenance` and the transcript hook already are. The resize oracle (`ResizeCanonical`) is fed the same bytes, so it reaches the same state. Its listener's replies are never drained to the PTY, and the ticket pins this with a test: a query that arrives while a resize is armed is answered once.

### 2.3 How the encoder learns the mode

The window thread owns every `DualPlaneSession` directly. It already reads `leaf.session.application_cursor_mode()` at the one place that calls `input::keyboard_bytes` (`runtime/keyboard.rs`, the last rung of `keyboard_input`). The new value is published the same way:

```rust
// bt-term
pub enum ModifyOtherKeys { Off, One, Two }
pub struct KeyboardProtocol { pub kitty: u8 /* masked to SUPPORTED_KITTY_FLAGS */, pub modify_other_keys: ModifyOtherKeys }
pub struct TerminalModes { /* existing five */, pub keyboard: KeyboardProtocol }
```

`TerminalAdapter::modes()` fills it in from the vendor `Term` for the active screen. The call site reads `terminal_modes()` once and passes `(application_cursor_mode, keyboard)` to the encoder. This reads a field of data the window thread already holds. It adds no door, no lock and no wait, so neither the thread-door note's §C-5 nor the window-thread budget has anything to register: those govern waits, and this is not one.

### 2.4 A program's leftovers, and the prompt

A program that pushes flag 1 and dies without popping it leaves the pane encoding `Ctrl+C` as `CSI 99;5u`. In a Unix shell that is an inconvenience; the spec keeps plain Enter/Tab/Backspace legacy so that `reset` can still be typed. **Behind ConPTY it is worse.** PSReadLine is a key-record client, so `CSI 99;5u` arrives as six typed characters and `Ctrl+C` no longer interrupts anything.

Folio already handles the same failure for mouse modes at OSC 133 `A` on the primary screen (`TerminalAdapter::retire_program_input_modes`, called from the handler in `session.rs`: "a prompt is the shell speaking"). That rule cannot be copied as it stands, because a shell may use this mode itself. fish sets `CSI = 5 u` while it reads a command line and `CSI = 0 u` before it runs one (`fish-shell/src/tty_handoff.rs`, `src/reader/reader.rs`: `exec_prompt` first, then `enable_tty_protocols` inside the read loop). fish also repaints its prompt, which sends another `A`, while its flags are on.

**The rule:** at OSC 133 `C` on the primary screen, the session takes a snapshot of the primary's keyboard state and of modifyOtherKeys. At the next OSC 133 `A` on the primary screen, if a `C` has been seen since the previous `A`, both are restored from that snapshot.

The effect is that whatever a command changed is undone when its prompt comes back. Two things are left alone: what the shell set for itself before `C`, and anything between an `A` and the next `C` (repaints, the command line). For fish the snapshot is `0`, because it turned its flags off before `C`, and it turns them on again after drawing the prompt. For PowerShell, which never sets them, the snapshot is also `0`, so a dead program's flags are cleared. A shell without OSC 133 (cmd, a bare bash) gets no help here, just as it gets none for mouse modes.

---

## 3. Grammar

`vte` 0.15 (`src/ansi.rs`) already dispatches all four kitty sequences and both xterm ones. The ticket changes what the vendor `Term` does with them, not the parser.

| Bytes | `vte` dispatch | Folio does |
|---|---|---|
| `CSI > flags u` | `push_keyboard_mode(KeyboardModes::from_bits_truncate(p as u8))`, `p` default 0 | push; `current = flags & SUPPORTED` |
| `CSI < n u` | `pop_keyboard_modes(n)`, `n` default 1 | pop `n` (§2.2) |
| `CSI = flags ; mode u` | `set_keyboard_mode(bits, Replace/Union/Difference)` for mode 1/2/3, default 1 | `current` = / \|= / &=! `flags & SUPPORTED` |
| `CSI ? u` | `report_keyboard_mode()` | reply `CSI ? current u` |
| `CSI > 4 ; v m` | `set_modify_other_keys(Reset/EnableExceptWellDefined/EnableAll)` for `v` = 0/1/2; `CSI > 4 m` means `v = 0` | Off / One / Two |
| `CSI ? 4 m` | `report_modify_other_keys()` | reply `CSI > 4 ; v m` |
| `CSI > m`, `CSI > 4 ; 3 m`, `CSI > 5 ; … m` | unhandled | nothing |

**Bounds.** No value is treated as an error; each case below says what happens.

- Parameters are `u16`.
- The flags value is a bit set. `as u8` keeps its low byte and `from_bits_truncate` drops bits 5–7. Both of these are masking, so no value is out of range.
- `SUPPORTED_KITTY_FLAGS = 0b1` then masks again.
- `vte` reads a pop count of `0` as the default, which is `1`. Kitty's text only says the count "defaults to 1 if unspecified", and WT treats an explicit `0` as a no-op. This note records the difference and does not chase it.
- `vte` reads a `mode` other than 1–3 as 1 (replace).
- The stack holds 8 saved values.

**Requested but unsupported bits.** When the mask drops a bit, the vendor reports the dropped bits through the listener, as a new `Event` variant that `CaptureListener` records. The adapter ORs them into a per-session `u8`. The first time a bit appears, the session writes one line to `diagnostics.log`: `keyboard protocol: flags 0b111 requested, 0b1 in force` (Codex on Unix asks for `>7u`, Claude Code for `>5u`). There is no card and no setting. The log line is how we will learn that "a program we care about needs them".

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
5. A keypad key that produced no text → `CSI code;m u`, using the spec's keypad code (`KP_ENTER` 57414, `KP_LEFT` 57417 … `KP_DELETE` 57426). This covers Numpad Enter, and the keypad's arrows, Home, End, Insert, Delete, PageUp and PageDown with Num Lock off. Keypad digits and operators produce text, so rule 2 handles them. (See Q2.)
6. Everything else that has a code → `CSI code;m u`, leaving out `;m` when `m = 1`. The codes are: Escape 27, Enter 13, Tab 9, Backspace 127, Space 32, and for a text key the character from `key_without_modifiers`. This rule covers Esc on its own, every Alt chord, every Ctrl chord (including `Ctrl+i`, `Ctrl+m`, `Ctrl+[` and `Ctrl+c`), Shift+Tab (`CSI 9;2u`, not `CSI Z`), Ctrl+digit, and every modified Enter, Tab, Backspace and Space.

Function keys: Folio's encoder has **no case for F-keys today**, so F1–F12 send nothing (§8 F-1). Flag 1 would change the form of F1–F4. This ticket adds neither.

### 4.3 The rule under modifyOtherKeys

The encoder uses modifyOtherKeys only when the kitty flags are 0. **When both are set, kitty wins**, as it does in kitty, in WT's `TerminalInput` and in Ghostty; Claude Code asks for both.

The form is xterm's default, `CSI 27 ; m ; k ~`. `k` is the character **with Shift applied** and without Ctrl, which is xterm's keysym; for example, `Ctrl+Shift+a` gives `k = 65`. The fixed codes are Enter 13, Tab 9, Escape 27, Space 32 and Backspace **127**. Folio's Backspace sends DEL; xterm's reference output used 8 because its Backspace was `^H`.

Which chords are encoded follows xterm's own table (`alacritty/vte` `doc/modifyOtherKeys-example.txt`, which is the output of xterm's `vttests/modify-keys.pl`), by class of key:

| Class | Mode 1 encodes | Mode 2 encodes |
|---|---|---|
| Enter, Tab | every modified chord | every modified chord |
| Escape | chords with Alt | every modified chord |
| Backspace | none | every modified chord except Ctrl alone |
| keys with a legacy Ctrl code (the domain of Folio's `control_byte`: letters, Space, `@ [ \ ] ^ _ ?`) | chords with Alt | every modified chord, including Shift alone (`Shift+a` → `CSI 27;2;65~`) |
| other printable keys (digits, other punctuation, non-ASCII) | chords with Ctrl or Alt | chords with Ctrl or Alt |

A chord that a mode does not encode takes the legacy row.

`k` needs the shifted character while Ctrl is held. On Windows, `logical_key` is exactly that. On macOS, a Ctrl chord can arrive as its control character, so `k` is `key_without_modifiers` uppercased for letters. For digits, the ticket measures the rows on the Mac mini before trusting them (§6.2).

On Windows, a key-record child that asked for modifyOtherKeys alone would lose these chords entirely, because ConPTY drops an unknown `~` sequence for such a child (§1). No such child is known: Claude Code asks for both, and flag 1 wins. The real-ConPTY test records what actually happens.

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

For these keys, the only changes are the DECCKM and keypad rules of §4.2:

| Key | Mods | legacy, DECCKM on | kitty flag 1, DECCKM on or off |
|---|---|---|---|
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

These tests run on `TerminalAdapter`, observing `modes().keyboard` and `take_pty_writes()`:

- push/pop/set/query: `CSI > 1 u` → 1; `CSI ? u` → `CSI ? 1 u`; `CSI = 0 u` → 0 and the query answers 0 (the vendor's defect 2); `CSI = 1 ; 2 u` then `CSI = 1 ; 3 u`; `CSI < u` on an empty stack → 0; `CSI < 5 u` past the bottom → 0.
- masking: `CSI > 31 u` → 1, the query answers `CSI ? 1 u`, and the session reports the refused bits `0b11110` once. A second identical push adds no line.
- the cap: nine pushes of distinct values followed by eight pops reach the second value pushed, because the oldest was evicted. The title stack is untouched (defect 1).
- two screens: push 1 on the primary; `?1049h` → 0, and the query answers 0; push 1; `?1049l` → 1; `?1049h` again → 0, because the alternate screen starts empty (defect 4). A `CSI = 1 u` on the primary survives a round trip through the alternate screen (defect 3).
- `RIS` clears both screens and modifyOtherKeys.
- modifyOtherKeys: `CSI > 4 ; 2 m` → Two; `CSI ? 4 m` → `CSI > 4 ; 2 m`; `CSI > 4 m` → Off; `CSI > 4 ; 3 m` and `CSI > m` change nothing.
- reply order: `CSI ? u CSI c` → the kitty reply, then DA1.
- the resize oracle: a query fed while a resize is armed is answered once.
- ConPTY's teardown bytes: `a_teardown_that_conpty_re_enables_does_not_type_at_the_pane` still passes, and afterwards the state is 0 / Off.
- the prompt rule, on `DualPlaneSession` with OSC 133. Feed fish's sequence: `A`, `=5u`, `A` (repaint), `=0u`, `C`, a program's `>1u`, `A`. At the last `A` the flags are 0, and the repaint `A` did not touch the `=5u`. Also check that an `A` on the alternate screen does nothing, that a `>4;2m` inside the command is Off at the next `A`, and that nothing is restored when there is no `C` between two `A`s.

### 6.2 `bt-app` (the encoder)

- **The table is data.** `crates/bt-app/src/key_encoding.tsv` holds the rows of §4.4: key, location, modifiers, DECCKM, mode, and the bytes as escaped text. `input::tests::every_row_of_the_key_encoding_table` runs each row through `keyboard_bytes`. §4.4 of this note is the table's first version; after the ticket, the TSV is the source. `scripts/dev/generate-key-encoding-table.ps1` generates `docs/key-encoding.md` from it, and a test keeps the two equal. This follows the pattern of `window_waits.tsv` and ARCHITECTURE §5.3. `(table)` rows are checked against `Shortcuts::lookup` instead, because the chord is claimed and the encoder is never asked.
- Some cases are tested separately rather than as plain table rows:
  - every letter `a`–`z` under C and AC in all four modes, generated so that no letter is special;
  - a non-ASCII key: `é` with C → `CSI 233;5u`;
  - `key_without_modifiers` on a Russian layout: `ф` → 1092;
  - AltGr text: with Ctrl and Alt absent, it is text in every mode;
  - Super on both platforms → nothing, in every mode;
  - macOS Option with the setting off (text) and on (`CSI 97;3u`).
- **The golden replay.** Feed a `TerminalAdapter` the bytes crossterm sends for Codex on Unix: `CSI ? u`, then `CSI > 7 u` (`DISAMBIGUATE | REPORT_EVENT_TYPES | REPORT_ALTERNATE_KEYS`). Then encode using its `modes().keyboard`. Expected: plain Enter → `\r`, Ctrl+Enter → `CSI 13;5u`, Shift+Enter → `CSI 13;2u`, Esc → `CSI 27u`, `a` → `a`, and no release event for any key. Then feed Codex's exit bytes (`CSI < 1 u`, `CSI < u`, `CSI > 4 ; 0 m`), after which Ctrl+Enter is `\r` again. Repeat with Claude Code's `CSI > 5 u` + `CSI > 4 ; 2 m`: kitty wins, so Ctrl+Enter is `CSI 13;5u`, not `CSI 27;5;13~`.
- A measurement on the Mac mini: winit's `logical_key` and `key_without_modifiers` for `Ctrl+Shift+1`, `Ctrl+Shift+[` and `Ctrl+Shift+a` on the US layout, to confirm §4.3's `k` there.

### 6.3 Through a real ConPTY (Windows)

`crates/bt-pty/tests/keyboard_protocol_through_conpty.rs` follows the pattern of `color_query_through_conpty.rs`: Windows PowerShell is spawned the way a pane spawns it.

1. The child writes `\e[?u\e[>1u`. Both sequences arrive in the bytes the pty hands us, and the session's flags become 1. This confirms §1's first fact: the request crosses ConPTY.
2. A key-record child (`[Console]::ReadKey($true)` in a loop, printing `Key`, `Modifiers` and `KeyChar`) is sent three inputs, and the test records what it reads for each:
   - `\e[13;5u`: expected to arrive as the characters (§1's second fact);
   - `\e[27;5;13~`: expected to arrive as nothing;
   - Folio's win32-input-mode record for Ctrl+Enter: expected to arrive as `Enter` with `Control`. This is the evidence Q1 needs.
3. A VT-input child (the same script after `SetConsoleMode(…ENABLE_VIRTUAL_TERMINAL_INPUT)`, reading `[Console]::In`) is sent `\e[13;5u`, and it reads those bytes.

The test asserts 1 and 3. It records 2 as observations in its output, and the ticket's report quotes them.

### 6.4 Real programs (the ticket's acceptance, on the candidate build)

| Program | Where | Expected |
|---|---|---|
| Codex TUI, with the reporter's config (Ctrl+Enter submits, Enter inserts a newline) | macOS; WSL | Ctrl+Enter submits, Enter inserts a newline |
| Codex TUI | Windows native | **unchanged** (Ctrl+Enter inserts a newline, as today) unless the answer to Q1 is A |
| neovim, with `:inoremap <C-CR> …` and a `:map <C-i>` distinct from `<Tab>` | macOS; WSL | both fire; Esc leaves insert mode immediately (no `ttimeout` wait) |
| fish, with `bind ctrl-enter …` and `bind ctrl-i …` | macOS; WSL | both fire. After running `nvim` and `:q`, and after `kill -9` of a program that pushed flags, Ctrl+C at the prompt still clears the line |
| PowerShell 7 and 5.1 + PSReadLine | Windows | byte-identical to today. From pwsh, start a WSL program that pushes flag 1 in the same pane and kill it; Ctrl+C at the next prompt still interrupts (§2.4) |
| Claude Code | macOS; WSL; Windows native | it detects support (`CSI ? u`), and Shift+Enter / Ctrl+Enter do what its keybindings say. On Windows native, record whether this works (§1, second row) |
| `kitten show-key -m kitty` | macOS | matches §4.4's kitty column for the ten keys |
| `cat -v` / `showkey -a` | macOS; WSL | the legacy column is unchanged when no program has asked |

---

## 7. Ticket cut

**T-KEYBOARD-PROTOCOL is one ticket, size M.** The parser half is small because `vte` already dispatches everything. The encoder half is a table-driven rewrite of one function plus its call site. The real-ConPTY test is the largest single piece. The ticket is two commits on one branch, merged together:

1. `bt-term` + `vendor/alacritty_terminal`:
   - kitty's state model per screen, with the cap and the mask (§2.2), and `SUPPORTED_KITTY_FLAGS = 1`;
   - the refused-bits event and its log line;
   - modifyOtherKeys and its query reply;
   - `RIS`, the alternate-screen rule and the prompt rule (§2.4);
   - `TerminalModes::keyboard`;
   - the tests of §6.1 and §6.3.
2. `bt-app`:
   - `keyboard_bytes` takes `(key, key_without_modifiers, location, modifiers, application_cursor_mode, keyboard)`;
   - the rules of §4.2 and §4.3;
   - `key_encoding.tsv`, the generator and `docs/key-encoding.md`;
   - the tests of §6.2.

It is not two tickets, because each half on its own is worse than neither. The state on its own makes Folio answer `CSI ? u`, so programs push flag 1 and then receive legacy bytes they were told they would not get; Claude Code, for one, would switch on its extended-keys mode for nothing. The encoder on its own has no mode to read.

**Explicitly out of scope:**

- kitty flags 2, 4, 8 and 16 (report event types, alternate keys, all keys as escape codes, associated text);
- win32-input-mode (owner ruling 2026-09-28; but see Q1);
- the Alt+Enter ESC-prefix hack (owner ruling 2026-09-28);
- function keys (F-1);
- `DECSTR`;
- a user setting;
- lock-modifier bits.

**If the answer to Q1 is A, a second ticket follows: T-KEYBOARD-RECORDS (Windows), size S–M.** When the pane's kitty flags and modifyOtherKeys are both 0, some chords cannot be told apart in legacy bytes: a modified Enter, Tab, Backspace, Escape or Space, and Ctrl with a key that has no C0 code. Each such chord is written as a win32-input-mode down/up record pair, built like `SHIFT_ENTER_RECORDS`; every other key keeps its VT bytes. It depends on T-KEYBOARD-PROTOCOL, because it reads the same `KeyboardProtocol`, and on the record observation in §6.3.

**DESIGN.md entry, for the ticket to write when it lands** (English, dated, at the end, like the entries before it):

> ### 2026-09-xx — A program that asks for the kitty keyboard protocol's disambiguate tier, or for xterm's modifyOtherKeys, gets it; Folio answers the query with what is in force and undoes a command's leftovers at its next prompt
>
> 0.4.7 ticket T-KEYBOARD-PROTOCOL; design note `docs/plans/design/keyboard-protocol-2026-09-29.md` and its Codex review; issue #13. Each pane's terminal keeps, per screen, the kitty flags in force and a stack of eight saved values (`CSI > u`, `CSI < u`, `CSI = u`, `CSI ? u`); only flag 1 is honoured, other bits are dropped where they are parsed and named once in `diagnostics.log`. modifyOtherKeys (`CSI > 4 ; v m`, `CSI ? 4 m`) is one value per terminal. The alternate screen starts with no flags and its flags end with it; `RIS` clears both; at OSC 133 `A` on the primary screen after a `C`, the primary's flags and modifyOtherKeys return to their values at that `C`. The encoder reads the mode from `TerminalModes::keyboard` and encodes from `key_encoding.tsv` (generated into `docs/key-encoding.md`): under flag 1, Esc and every Alt, Ctrl and modified Enter/Tab/Backspace/Space chord is `CSI code;m u` with the un-shifted key as the code, plain Enter/Tab/Backspace stay `\r \t \x7f`, arrows ignore DECCKM, and non-text keypad keys take their keypad codes; under modifyOtherKeys, `CSI 27;m;k~` for xterm's classes; kitty wins when both are set. Super is never encoded; Option follows *Option key sends Alt*. A program that never asks — PSReadLine, cmd, Codex on Windows — receives exactly the bytes it received before.

**CHANGELOG line** (under *Added*):

> - Programs that ask for it can tell Ctrl+Enter, Shift+Enter, Alt+Enter, Ctrl+I, Ctrl+M and Esc apart from Enter, Tab and a lone escape: Folio supports the first tier of the kitty keyboard protocol and xterm's modifyOtherKeys. This works for neovim, fish, helix, Claude Code, and Codex on macOS or in WSL; programs that do not ask, such as PowerShell, are unaffected.

If the answer to Q1 is A, T-KEYBOARD-RECORDS adds: "On Windows, Ctrl+Enter, Shift+Enter and Alt+Enter also reach console programs such as Codex and PowerShell as those keys."

---

## 8. Found along the way, not in this ticket

- **F-1** `keyboard_bytes` has no case for F1–F12, so a function key sends nothing to the child today, in every program. It needs its own ticket, covering the legacy forms and flag 1's `CSI P/Q/13~/S` for F1–F4.
- **F-2** `Ctrl+Space` and `Ctrl+Shift+Space` send nothing. winit reports these as `NamedKey::Space`, and the control-alphabet case never sees them: it was written for `Key::Character`, even though its comment says `Ctrl+Space` is NUL. The legacy column keeps `—` for these rows, so this ticket does not change a legacy byte. A follow-up makes them NUL (0x00), as kitty's legacy table, xterm and the existing comment all say.
- **F-3** `Alt+Backspace` sends `\x7f` with no ESC, so readline's backward-kill-word (`\e\x7f`) cannot be typed. Behind ConPTY, `ESC DEL` becomes an Alt+Backspace record, which PSReadLine also binds. This deserves its own look, with the same care as the Alt+Enter ruling.

---

## 9. Open questions for the owner

**Q1. This ticket does not fix the problem for issue #13's reporter.** Codex on Windows reads key records and never asks for the protocol (§1). The reply on #13 (2026-09-28) says Ctrl+Enter should work after this version. There are two possible answers:

- **A (recommended).** Add T-KEYBOARD-RECORDS. On Windows, when no program has asked for anything, Folio writes only the chords VT cannot express as win32-input-mode records: modified Enter, Tab, Backspace, Escape and Space, and Ctrl with keys that have no C0 code. Folio already uses this mechanism for a paste's Shift+Enter. This is narrower than adopting win32-input-mode, because every other key keeps its VT bytes, and it is how Windows Terminal and WezTerm get these chords to console programs. It does change PowerShell. Ctrl+Enter and Shift+Enter become PSReadLine's *InsertLineAbove* and *AddLine*, as in Windows Terminal, instead of running the line. Alt+Enter is unbound in PSReadLine, so it does nothing instead of running the line.
- **B.** Keep the 2026-09-28 ruling, and correct the reply on #13 to say what this version fixes (macOS, WSL, and programs that ask) and what it does not fix (Codex on Windows).

**Q2. The keypad under flag 1.** The spec sends Numpad Enter as `CSI 57414u`, and the keypad arrows with Num Lock off as their own keypad codes; kitty, Ghostty and WT all do this. crossterm, neovim and fish decode these codes. A program that asked for flag 1 but does not know keypad codes would find that Numpad Enter stops working. **Recommended: follow the spec**, as §4.2 rule 5 does. The alternative is to encode keypad keys as their main-keyboard twins.

**Q3. F-2 now or later.** Making `Ctrl+Space` send NUL changes a legacy byte, in a ticket whose promise is "a program that never asks is unaffected". **Recommended: later, as its own small ticket**, so that this ticket's promise stays literally true.

---

## Sources

- kitty keyboard protocol: https://sw.kovidgoyal.net/kitty/keyboard-protocol/ (source `docs/keyboard-protocol.rst`), and kitty's encoder `kitty/key_encoding.c` (https://github.com/kovidgoyal/kitty).
- xterm modifyOtherKeys: https://invisible-island.net/xterm/modified-keys.html; xterm's reference output as recorded in `alacritty/vte` `doc/modifyOtherKeys-example.txt` (https://github.com/alacritty/vte).
- Windows Terminal / ConPTY (`microsoft/terminal` main): PR #19817 *Implement the Kitty Keyboard Protocol*, issue #19847, PR #19995; `src/host/_stream.cpp` (`WriteCharsVT`), `src/host/VtIo.cpp`, `src/terminal/adapter/adaptDispatch.cpp`, `src/terminal/input/terminalInput.cpp`, `src/terminal/parser/InputStateMachineEngine.cpp`.
- Ghostty `src/input/key_encode.zig` (https://github.com/ghostty-org/ghostty); WezTerm key encoding: https://wezterm.org/config/key-encoding.html.
- crossterm `src/event.rs` (https://github.com/crossterm-rs/crossterm); Codex `codex-rs/tui/src/tui/keyboard_modes.rs` (https://github.com/openai/codex); fish `src/tty_handoff.rs`, `src/terminal.rs`, `src/reader/reader.rs` (https://github.com/fish-shell/fish-shell); Claude Code issues anthropics/claude-code#96526 and #97501; warpdotdev/warp#16175.
- In this tree: `crates/bt-app/src/input.rs` (`keyboard_bytes`, `xterm_modifier`, `meta_prefix`, `control_byte`, `effective_modifiers`, `SHIFT_ENTER_RECORDS`), `crates/bt-app/src/runtime/keyboard.rs` (`keyboard_input`), `crates/bt-app/src/shortcuts.rs`, `docs/shortcuts.md`, `crates/bt-term/src/adapter.rs` (`TerminalAdapter::new`, `modes`, `retire_program_input_modes`, `take_pty_writes`), `crates/bt-term/src/session.rs` (the OSC 133 `A` handler), `vendor/alacritty_terminal/src/term/mod.rs`, `vte` 0.15 `src/ansi.rs`, winit 0.30.13 `platform_impl/windows/{keyboard,keyboard_layout}.rs`, `crates/bt-pty/tests/color_query_through_conpty.rs`; `docs/DESIGN.md` §7.1.5a″, §7.1.5b, §7.54d, §13.13, §13.16; `docs/plans/design/thread-door-2026-09-26.md` and `window-thread-budget-2026-09-25.md` §C-5.
