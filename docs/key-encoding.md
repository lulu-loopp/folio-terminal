# What each key sends

<!-- generated from crates/bt-app/src/key_encoding.tsv by scripts/dev/generate-key-encoding-table.ps1; edit the table, not this page -->

The bytes a key sends to the program in a pane, in each mode a program can ask for: nothing asked (*legacy*), the kitty keyboard protocol's first tier (*kitty flag 1*), or xterm's modifyOtherKeys (*1* or *2*). When a program asks for both, kitty wins. A program that never asks receives the legacy column, which is what Folio sent before either protocol existed here.

On Windows, a program that never asks reads the *Windows records* column instead, while ConPTY has win32-input-mode on (it turns it on for every session): the chords whose legacy bytes cannot tell them apart — a modified Enter, Tab, Backspace or Space, Shift+Escape, and Ctrl with a key that has no control code — go as the key records `CSI Vk;Sc;Uc;Kd;Cs;Rc _` (down, then up), which ConPTY turns into those exact key events. Every other chord sends its legacy bytes. A program that asks for kitty or modifyOtherKeys gets that protocol instead.

`CSI` is `ESC [`. `—` sends nothing. *(table)* is a chord Folio's shortcut table claims on Windows, so no program receives it. *(system)* is taken by Windows first; *(n/a)* never arrives on Windows and is encoded for macOS. Chords are as a US layout produces them. The design is `docs/plans/design/keyboard-protocol-2026-09-29.md`.

| Key | Where | Mods | DECCKM | legacy | kitty flag 1 | modifyOtherKeys 1 | modifyOtherKeys 2 | Windows records |
|---|---|---|---|---|---|---|---|---|
| Enter | standard | - | off | `\r` | `\r` | `\r` | `\r` | `\r` |
| Enter | standard | S | off | `\r` | `CSI 13;2u` | `CSI 27;2;13~` | `CSI 27;2;13~` | `CSI 13;28;13;1;16;1_CSI 13;28;13;0;16;1_` |
| Enter | standard | A | off | `\r` | `CSI 13;3u` | `CSI 27;3;13~` | `CSI 27;3;13~` | `CSI 13;28;13;1;2;1_CSI 13;28;13;0;2;1_` |
| Enter | standard | SA | off | `\r` | `CSI 13;4u` | `CSI 27;4;13~` | `CSI 27;4;13~` | `CSI 13;28;13;1;18;1_CSI 13;28;13;0;18;1_` |
| Enter | standard | C | off | `\r` | `CSI 13;5u` | `CSI 27;5;13~` | `CSI 27;5;13~` | `CSI 13;28;10;1;8;1_CSI 13;28;10;0;8;1_` |
| Enter | standard | SC | off | `\r` (n/a) | `CSI 13;6u` | `CSI 27;6;13~` | `CSI 27;6;13~` | `CSI 13;28;0;1;24;1_CSI 13;28;0;0;24;1_` (n/a) |
| Enter | standard | AC | off | `\r` | `CSI 13;7u` | `CSI 27;7;13~` | `CSI 27;7;13~` | `CSI 13;28;0;1;10;1_CSI 13;28;0;0;10;1_` |
| Enter | standard | SAC | off | `\r` | `CSI 13;8u` | `CSI 27;8;13~` | `CSI 27;8;13~` | `CSI 13;28;0;1;26;1_CSI 13;28;0;0;26;1_` |
| Tab | standard | - | off | `\t` | `\t` | `\t` | `\t` | `\t` |
| Tab | standard | S | off | `CSI Z` | `CSI 9;2u` | `CSI 27;2;9~` | `CSI 27;2;9~` | `CSI 9;15;9;1;16;1_CSI 9;15;9;0;16;1_` |
| Tab | standard | A | off | `\t` (system) | `CSI 9;3u` | `CSI 27;3;9~` | `CSI 27;3;9~` | `CSI 9;15;9;1;2;1_CSI 9;15;9;0;2;1_` (system) |
| Tab | standard | SA | off | `CSI Z` | `CSI 9;4u` | `CSI 27;4;9~` | `CSI 27;4;9~` | `CSI 9;15;9;1;18;1_CSI 9;15;9;0;18;1_` |
| Tab | standard | C | off | (table) | (table) | (table) | (table) | (table) |
| Tab | standard | SC | off | (table) | (table) | (table) | (table) | (table) |
| Tab | standard | AC | off | `\t` | `CSI 9;7u` | `CSI 27;7;9~` | `CSI 27;7;9~` | `CSI 9;15;0;1;10;1_CSI 9;15;0;0;10;1_` |
| Tab | standard | SAC | off | `CSI Z` | `CSI 9;8u` | `CSI 27;8;9~` | `CSI 27;8;9~` | `CSI 9;15;0;1;26;1_CSI 9;15;0;0;26;1_` |
| Backspace | standard | - | off | `\x7f` | `\x7f` | `\x7f` | `\x7f` | `\x7f` |
| Backspace | standard | S | off | `\x7f` | `CSI 127;2u` | `\x7f` | `CSI 27;2;127~` | `CSI 8;14;8;1;16;1_CSI 8;14;8;0;16;1_` |
| Backspace | standard | A | off | `\x7f` | `CSI 127;3u` | `\x7f` | `CSI 27;3;127~` | `CSI 8;14;8;1;2;1_CSI 8;14;8;0;2;1_` |
| Backspace | standard | SA | off | `\x7f` | `CSI 127;4u` | `\x7f` | `CSI 27;4;127~` | `CSI 8;14;8;1;18;1_CSI 8;14;8;0;18;1_` |
| Backspace | standard | C | off | `\x7f` | `CSI 127;5u` | `\x7f` | `\x7f` | `CSI 8;14;127;1;8;1_CSI 8;14;127;0;8;1_` |
| Backspace | standard | SC | off | `\x7f` | `CSI 127;6u` | `\x7f` | `CSI 27;6;127~` | `CSI 8;14;0;1;24;1_CSI 8;14;0;0;24;1_` |
| Backspace | standard | AC | off | `\x7f` | `CSI 127;7u` | `\x7f` | `CSI 27;7;127~` | `CSI 8;14;0;1;10;1_CSI 8;14;0;0;10;1_` |
| Backspace | standard | SAC | off | `\x7f` | `CSI 127;8u` | `\x7f` | `CSI 27;8;127~` | `CSI 8;14;0;1;26;1_CSI 8;14;0;0;26;1_` |
| Escape | standard | - | off | `\e` | `CSI 27u` | `\e` | `\e` | `\e` |
| Escape | standard | S | off | `\e` | `CSI 27;2u` | `\e` | `CSI 27;2;27~` | `CSI 27;1;27;1;16;1_CSI 27;1;27;0;16;1_` |
| Escape | standard | A | off | `\e` | `CSI 27;3u` | `CSI 27;3;27~` | `CSI 27;3;27~` | `\e` |
| Escape | standard | SA | off | `\e` | `CSI 27;4u` | `CSI 27;4;27~` | `CSI 27;4;27~` | `\e` |
| Escape | standard | C | off | `\e` | `CSI 27;5u` | `\e` | `CSI 27;5;27~` | `\e` |
| Escape | standard | SC | off | `\e` | `CSI 27;6u` | `\e` | `CSI 27;6;27~` | `\e` |
| Escape | standard | AC | off | `\e` | `CSI 27;7u` | `CSI 27;7;27~` | `CSI 27;7;27~` | `\e` |
| Escape | standard | SAC | off | `\e` | `CSI 27;8u` | `CSI 27;8;27~` | `CSI 27;8;27~` | `\e` |
| Space | standard | - | off | `\x20` | `\x20` | `\x20` | `\x20` | `\x20` |
| Space | standard | S | off | `\x20` | `\x20` | `\x20` | `CSI 27;2;32~` | `CSI 32;57;32;1;16;1_CSI 32;57;32;0;16;1_` |
| Space | standard | A | off | `\e ` | `CSI 32;3u` | `CSI 27;3;32~` | `CSI 27;3;32~` | `CSI 32;57;32;1;2;1_CSI 32;57;32;0;2;1_` |
| Space | standard | SA | off | `\e ` | `CSI 32;4u` | `CSI 27;4;32~` | `CSI 27;4;32~` | `CSI 32;57;32;1;18;1_CSI 32;57;32;0;18;1_` |
| Space | standard | C | off | — | `CSI 32;5u` | — | `CSI 27;5;32~` | `CSI 32;57;32;1;8;1_CSI 32;57;32;0;8;1_` |
| Space | standard | SC | off | — | `CSI 32;6u` | — | `CSI 27;6;32~` | `CSI 32;57;0;1;24;1_CSI 32;57;0;0;24;1_` |
| Space | standard | AC | off | — | `CSI 32;7u` | `CSI 27;7;32~` | `CSI 27;7;32~` | `CSI 32;57;0;1;10;1_CSI 32;57;0;0;10;1_` |
| Space | standard | SAC | off | — | `CSI 32;8u` | `CSI 27;8;32~` | `CSI 27;8;32~` | `CSI 32;57;0;1;26;1_CSI 32;57;0;0;26;1_` |
| e | standard | - | off | `e` | `e` | `e` | `e` | `e` |
| e | standard | S | off | `E` | `E` | `E` | `CSI 27;2;69~` | `E` |
| e | standard | A | off | `\ee` | `CSI 101;3u` | `CSI 27;3;101~` | `CSI 27;3;101~` | `\ee` |
| e | standard | SA | off | `\eE` | `CSI 101;4u` | `CSI 27;4;69~` | `CSI 27;4;69~` | `\eE` |
| e | standard | C | off | `\x05` | `CSI 101;5u` | `\x05` | `CSI 27;5;101~` | `\x05` |
| e | standard | SC | off | `\x05` | `CSI 101;6u` | `\x05` | `CSI 27;6;69~` | `\x05` |
| e | standard | AC | off | `\e\x05` | `CSI 101;7u` | `CSI 27;7;101~` | `CSI 27;7;101~` | `\e\x05` |
| e | standard | SAC | off | `\e\x05` | `CSI 101;8u` | `CSI 27;8;69~` | `CSI 27;8;69~` | `\e\x05` |
| i | standard | - | off | `i` | `i` | `i` | `i` | `i` |
| i | standard | S | off | `I` | `I` | `I` | `CSI 27;2;73~` | `I` |
| i | standard | A | off | `\ei` | `CSI 105;3u` | `CSI 27;3;105~` | `CSI 27;3;105~` | `\ei` |
| i | standard | SA | off | `\eI` | `CSI 105;4u` | `CSI 27;4;73~` | `CSI 27;4;73~` | `\eI` |
| i | standard | C | off | `\t` | `CSI 105;5u` | `\t` | `CSI 27;5;105~` | `\t` |
| i | standard | SC | off | `\t` | `CSI 105;6u` | `\t` | `CSI 27;6;73~` | `\t` |
| i | standard | AC | off | `\e\t` | `CSI 105;7u` | `CSI 27;7;105~` | `CSI 27;7;105~` | `\e\t` |
| i | standard | SAC | off | `\e\t` | `CSI 105;8u` | `CSI 27;8;73~` | `CSI 27;8;73~` | `\e\t` |
| m | standard | - | off | `m` | `m` | `m` | `m` | `m` |
| m | standard | S | off | `M` | `M` | `M` | `CSI 27;2;77~` | `M` |
| m | standard | A | off | `\em` | `CSI 109;3u` | `CSI 27;3;109~` | `CSI 27;3;109~` | `\em` |
| m | standard | SA | off | `\eM` | `CSI 109;4u` | `CSI 27;4;77~` | `CSI 27;4;77~` | `\eM` |
| m | standard | C | off | `\r` | `CSI 109;5u` | `\r` | `CSI 27;5;109~` | `\r` |
| m | standard | SC | off | (table) | (table) | (table) | (table) | (table) |
| m | standard | AC | off | `\e\r` | `CSI 109;7u` | `CSI 27;7;109~` | `CSI 27;7;109~` | `\e\r` |
| m | standard | SAC | off | `\e\r` | `CSI 109;8u` | `CSI 27;8;77~` | `CSI 27;8;77~` | `\e\r` |
| [ | standard | - | off | `[` | `[` | `[` | `[` | `[` |
| [ | standard | S | off | `{` | `{` | `{` | `CSI 27;2;123~` | `{` |
| [ | standard | A | off | `\e[` | `CSI 91;3u` | `CSI 27;3;91~` | `CSI 27;3;91~` | `\e[` |
| [ | standard | SA | off | `\e{` | `CSI 91;4u` | `CSI 27;4;123~` | `CSI 27;4;123~` | `\e{` |
| [ | standard | C | off | `\e` | `CSI 91;5u` | `\e` | `CSI 27;5;91~` | `\e` |
| [ | standard | SC | off | — | `CSI 91;6u` | — | `CSI 27;6;123~` | `CSI 219;26;0;1;24;1_CSI 219;26;0;0;24;1_` |
| [ | standard | AC | off | `\e\e` | `CSI 91;7u` | `CSI 27;7;91~` | `CSI 27;7;91~` | `\e\e` |
| [ | standard | SAC | off | — | `CSI 91;8u` | `CSI 27;8;123~` | `CSI 27;8;123~` | `CSI 219;26;0;1;26;1_CSI 219;26;0;0;26;1_` |
| 1 | standard | - | off | `1` | `1` | `1` | `1` | `1` |
| 1 | standard | S | off | `!` | `!` | `!` | `!` | `!` |
| 1 | standard | A | off | `\e1` | `CSI 49;3u` | `CSI 27;3;49~` | `CSI 27;3;49~` | `\e1` |
| 1 | standard | SA | off | `\e!` | `CSI 49;4u` | `CSI 27;4;33~` | `CSI 27;4;33~` | `\e!` |
| 1 | standard | C | off | — | `CSI 49;5u` | `CSI 27;5;49~` | `CSI 27;5;49~` | `CSI 49;2;0;1;8;1_CSI 49;2;0;0;8;1_` |
| 1 | standard | SC | off | (table) | (table) | (table) | (table) | (table) |
| 1 | standard | AC | off | — | `CSI 49;7u` | `CSI 27;7;49~` | `CSI 27;7;49~` | `CSI 49;2;0;1;10;1_CSI 49;2;0;0;10;1_` |
| 1 | standard | SAC | off | — | `CSI 49;8u` | `CSI 27;8;33~` | `CSI 27;8;33~` | `CSI 49;2;0;1;26;1_CSI 49;2;0;0;26;1_` |
| F1 | standard | - | on | `\eOP` | `CSI P` |  |  | `\eOP` |
| F1 | standard | - | off |  | `CSI P` |  |  |  |
| F1 | standard | C | on | `CSI 1;5P` | `CSI 1;5P` |  |  | `CSI 1;5P` |
| F1 | standard | C | off |  | `CSI 1;5P` |  |  |  |
| F3 | standard | - | on | `\eOR` | `CSI 13~` |  |  | `\eOR` |
| F3 | standard | - | off |  | `CSI 13~` |  |  |  |
| F3 | standard | S | on | `CSI 1;2R` | `CSI 13;2~` |  |  | `CSI 1;2R` |
| F3 | standard | S | off |  | `CSI 13;2~` |  |  |  |
| F4 | standard | - | on | `\eOS` | `CSI S` |  |  | `\eOS` |
| F4 | standard | - | off |  | `CSI S` |  |  |  |
| F5 | standard | - | on | `CSI 15~` | `CSI 15~` |  |  | `CSI 15~` |
| F5 | standard | - | off |  | `CSI 15~` |  |  |  |
| ArrowUp | standard | - | on | `\eOA` | `CSI A` |  |  | `\eOA` |
| ArrowUp | standard | - | off |  | `CSI A` |  |  |  |
| ArrowUp | standard | C | on | `CSI 1;5A` | `CSI 1;5A` |  |  | `CSI 1;5A` |
| ArrowUp | standard | C | off |  | `CSI 1;5A` |  |  |  |
| Home | standard | - | on | `\eOH` | `CSI H` |  |  | `\eOH` |
| Home | standard | - | off |  | `CSI H` |  |  |  |
| Enter | numpad | - | on | `\r` | `CSI 57414u` |  |  | `\r` |
| Enter | numpad | - | off |  | `CSI 57414u` |  |  |  |
| Enter | numpad | C | on | `\r` | `CSI 57414;5u` |  |  | `CSI 13;28;10;1;264;1_CSI 13;28;10;0;264;1_` |
| Enter | numpad | C | off |  | `CSI 57414;5u` |  |  |  |
| ArrowUp | numpad | - | on | `\eOA` | `CSI 57419u` |  |  | `\eOA` |
| ArrowUp | numpad | - | off |  | `CSI 57419u` |  |  |  |
| 8 | numpad | - | on | `8` | `8` |  |  | `8` |
| 8 | numpad | - | off |  | `8` |  |  |  |
