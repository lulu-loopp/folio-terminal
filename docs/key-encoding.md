# What each key sends

<!-- generated from crates/bt-app/src/key_encoding.tsv by scripts/dev/generate-key-encoding-table.ps1; edit the table, not this page -->

The bytes a key sends to the program in a pane, in each mode a program can ask for: nothing asked (*legacy*), the kitty keyboard protocol's first tier (*kitty flag 1*), or xterm's modifyOtherKeys (*1* or *2*). When a program asks for both, kitty wins. A program that never asks receives the legacy column, which is what Folio sent before either protocol existed here.

`CSI` is `ESC [`. `—` sends nothing. *(table)* is a chord Folio's shortcut table claims on Windows, so no program receives it. *(system)* is taken by Windows first; *(n/a)* never arrives on Windows and is encoded for macOS. Chords are as a US layout produces them. The design is `docs/plans/design/keyboard-protocol-2026-09-29.md`.

| Key | Where | Mods | DECCKM | legacy | kitty flag 1 | modifyOtherKeys 1 | modifyOtherKeys 2 |
|---|---|---|---|---|---|---|---|
| Enter | standard | - | off | `\r` | `\r` | `\r` | `\r` |
| Enter | standard | S | off | `\r` | `CSI 13;2u` | `CSI 27;2;13~` | `CSI 27;2;13~` |
| Enter | standard | A | off | `\r` | `CSI 13;3u` | `CSI 27;3;13~` | `CSI 27;3;13~` |
| Enter | standard | SA | off | `\r` | `CSI 13;4u` | `CSI 27;4;13~` | `CSI 27;4;13~` |
| Enter | standard | C | off | `\r` | `CSI 13;5u` | `CSI 27;5;13~` | `CSI 27;5;13~` |
| Enter | standard | SC | off | `\r` (n/a) | `CSI 13;6u` | `CSI 27;6;13~` | `CSI 27;6;13~` |
| Enter | standard | AC | off | `\r` | `CSI 13;7u` | `CSI 27;7;13~` | `CSI 27;7;13~` |
| Enter | standard | SAC | off | `\r` | `CSI 13;8u` | `CSI 27;8;13~` | `CSI 27;8;13~` |
| Tab | standard | - | off | `\t` | `\t` | `\t` | `\t` |
| Tab | standard | S | off | `CSI Z` | `CSI 9;2u` | `CSI 27;2;9~` | `CSI 27;2;9~` |
| Tab | standard | A | off | `\t` (system) | `CSI 9;3u` | `CSI 27;3;9~` | `CSI 27;3;9~` |
| Tab | standard | SA | off | `CSI Z` | `CSI 9;4u` | `CSI 27;4;9~` | `CSI 27;4;9~` |
| Tab | standard | C | off | (table) | (table) | (table) | (table) |
| Tab | standard | SC | off | (table) | (table) | (table) | (table) |
| Tab | standard | AC | off | `\t` | `CSI 9;7u` | `CSI 27;7;9~` | `CSI 27;7;9~` |
| Tab | standard | SAC | off | `CSI Z` | `CSI 9;8u` | `CSI 27;8;9~` | `CSI 27;8;9~` |
| Backspace | standard | - | off | `\x7f` | `\x7f` | `\x7f` | `\x7f` |
| Backspace | standard | S | off | `\x7f` | `CSI 127;2u` | `\x7f` | `CSI 27;2;127~` |
| Backspace | standard | A | off | `\x7f` | `CSI 127;3u` | `\x7f` | `CSI 27;3;127~` |
| Backspace | standard | SA | off | `\x7f` | `CSI 127;4u` | `\x7f` | `CSI 27;4;127~` |
| Backspace | standard | C | off | `\x7f` | `CSI 127;5u` | `\x7f` | `\x7f` |
| Backspace | standard | SC | off | `\x7f` | `CSI 127;6u` | `\x7f` | `CSI 27;6;127~` |
| Backspace | standard | AC | off | `\x7f` | `CSI 127;7u` | `\x7f` | `CSI 27;7;127~` |
| Backspace | standard | SAC | off | `\x7f` | `CSI 127;8u` | `\x7f` | `CSI 27;8;127~` |
| Escape | standard | - | off | `\e` | `CSI 27u` | `\e` | `\e` |
| Escape | standard | S | off | `\e` | `CSI 27;2u` | `\e` | `CSI 27;2;27~` |
| Escape | standard | A | off | `\e` | `CSI 27;3u` | `CSI 27;3;27~` | `CSI 27;3;27~` |
| Escape | standard | SA | off | `\e` | `CSI 27;4u` | `CSI 27;4;27~` | `CSI 27;4;27~` |
| Escape | standard | C | off | `\e` | `CSI 27;5u` | `\e` | `CSI 27;5;27~` |
| Escape | standard | SC | off | `\e` | `CSI 27;6u` | `\e` | `CSI 27;6;27~` |
| Escape | standard | AC | off | `\e` | `CSI 27;7u` | `CSI 27;7;27~` | `CSI 27;7;27~` |
| Escape | standard | SAC | off | `\e` | `CSI 27;8u` | `CSI 27;8;27~` | `CSI 27;8;27~` |
| Space | standard | - | off | `\x20` | `\x20` | `\x20` | `\x20` |
| Space | standard | S | off | `\x20` | `\x20` | `\x20` | `CSI 27;2;32~` |
| Space | standard | A | off | `\e ` | `CSI 32;3u` | `CSI 27;3;32~` | `CSI 27;3;32~` |
| Space | standard | SA | off | `\e ` | `CSI 32;4u` | `CSI 27;4;32~` | `CSI 27;4;32~` |
| Space | standard | C | off | — | `CSI 32;5u` | — | `CSI 27;5;32~` |
| Space | standard | SC | off | — | `CSI 32;6u` | — | `CSI 27;6;32~` |
| Space | standard | AC | off | — | `CSI 32;7u` | `CSI 27;7;32~` | `CSI 27;7;32~` |
| Space | standard | SAC | off | — | `CSI 32;8u` | `CSI 27;8;32~` | `CSI 27;8;32~` |
| e | standard | - | off | `e` | `e` | `e` | `e` |
| e | standard | S | off | `E` | `E` | `E` | `CSI 27;2;69~` |
| e | standard | A | off | `\ee` | `CSI 101;3u` | `CSI 27;3;101~` | `CSI 27;3;101~` |
| e | standard | SA | off | `\eE` | `CSI 101;4u` | `CSI 27;4;69~` | `CSI 27;4;69~` |
| e | standard | C | off | `\x05` | `CSI 101;5u` | `\x05` | `CSI 27;5;101~` |
| e | standard | SC | off | `\x05` | `CSI 101;6u` | `\x05` | `CSI 27;6;69~` |
| e | standard | AC | off | `\e\x05` | `CSI 101;7u` | `CSI 27;7;101~` | `CSI 27;7;101~` |
| e | standard | SAC | off | `\e\x05` | `CSI 101;8u` | `CSI 27;8;69~` | `CSI 27;8;69~` |
| i | standard | - | off | `i` | `i` | `i` | `i` |
| i | standard | S | off | `I` | `I` | `I` | `CSI 27;2;73~` |
| i | standard | A | off | `\ei` | `CSI 105;3u` | `CSI 27;3;105~` | `CSI 27;3;105~` |
| i | standard | SA | off | `\eI` | `CSI 105;4u` | `CSI 27;4;73~` | `CSI 27;4;73~` |
| i | standard | C | off | `\t` | `CSI 105;5u` | `\t` | `CSI 27;5;105~` |
| i | standard | SC | off | `\t` | `CSI 105;6u` | `\t` | `CSI 27;6;73~` |
| i | standard | AC | off | `\e\t` | `CSI 105;7u` | `CSI 27;7;105~` | `CSI 27;7;105~` |
| i | standard | SAC | off | `\e\t` | `CSI 105;8u` | `CSI 27;8;73~` | `CSI 27;8;73~` |
| m | standard | - | off | `m` | `m` | `m` | `m` |
| m | standard | S | off | `M` | `M` | `M` | `CSI 27;2;77~` |
| m | standard | A | off | `\em` | `CSI 109;3u` | `CSI 27;3;109~` | `CSI 27;3;109~` |
| m | standard | SA | off | `\eM` | `CSI 109;4u` | `CSI 27;4;77~` | `CSI 27;4;77~` |
| m | standard | C | off | `\r` | `CSI 109;5u` | `\r` | `CSI 27;5;109~` |
| m | standard | SC | off | (table) | (table) | (table) | (table) |
| m | standard | AC | off | `\e\r` | `CSI 109;7u` | `CSI 27;7;109~` | `CSI 27;7;109~` |
| m | standard | SAC | off | `\e\r` | `CSI 109;8u` | `CSI 27;8;77~` | `CSI 27;8;77~` |
| [ | standard | - | off | `[` | `[` | `[` | `[` |
| [ | standard | S | off | `{` | `{` | `{` | `CSI 27;2;123~` |
| [ | standard | A | off | `\e[` | `CSI 91;3u` | `CSI 27;3;91~` | `CSI 27;3;91~` |
| [ | standard | SA | off | `\e{` | `CSI 91;4u` | `CSI 27;4;123~` | `CSI 27;4;123~` |
| [ | standard | C | off | `\e` | `CSI 91;5u` | `\e` | `CSI 27;5;91~` |
| [ | standard | SC | off | — | `CSI 91;6u` | — | `CSI 27;6;123~` |
| [ | standard | AC | off | `\e\e` | `CSI 91;7u` | `CSI 27;7;91~` | `CSI 27;7;91~` |
| [ | standard | SAC | off | — | `CSI 91;8u` | `CSI 27;8;123~` | `CSI 27;8;123~` |
| 1 | standard | - | off | `1` | `1` | `1` | `1` |
| 1 | standard | S | off | `!` | `!` | `!` | `!` |
| 1 | standard | A | off | `\e1` | `CSI 49;3u` | `CSI 27;3;49~` | `CSI 27;3;49~` |
| 1 | standard | SA | off | `\e!` | `CSI 49;4u` | `CSI 27;4;33~` | `CSI 27;4;33~` |
| 1 | standard | C | off | — | `CSI 49;5u` | `CSI 27;5;49~` | `CSI 27;5;49~` |
| 1 | standard | SC | off | (table) | (table) | (table) | (table) |
| 1 | standard | AC | off | — | `CSI 49;7u` | `CSI 27;7;49~` | `CSI 27;7;49~` |
| 1 | standard | SAC | off | — | `CSI 49;8u` | `CSI 27;8;33~` | `CSI 27;8;33~` |
| F1 | standard | - | on | `\eOP` | `CSI P` |  |  |
| F1 | standard | - | off |  | `CSI P` |  |  |
| F1 | standard | C | on | `CSI 1;5P` | `CSI 1;5P` |  |  |
| F1 | standard | C | off |  | `CSI 1;5P` |  |  |
| F3 | standard | - | on | `\eOR` | `CSI 13~` |  |  |
| F3 | standard | - | off |  | `CSI 13~` |  |  |
| F3 | standard | S | on | `CSI 1;2R` | `CSI 13;2~` |  |  |
| F3 | standard | S | off |  | `CSI 13;2~` |  |  |
| F4 | standard | - | on | `\eOS` | `CSI S` |  |  |
| F4 | standard | - | off |  | `CSI S` |  |  |
| F5 | standard | - | on | `CSI 15~` | `CSI 15~` |  |  |
| F5 | standard | - | off |  | `CSI 15~` |  |  |
| ArrowUp | standard | - | on | `\eOA` | `CSI A` |  |  |
| ArrowUp | standard | - | off |  | `CSI A` |  |  |
| ArrowUp | standard | C | on | `CSI 1;5A` | `CSI 1;5A` |  |  |
| ArrowUp | standard | C | off |  | `CSI 1;5A` |  |  |
| Home | standard | - | on | `\eOH` | `CSI H` |  |  |
| Home | standard | - | off |  | `CSI H` |  |  |
| Enter | numpad | - | on | `\r` | `CSI 57414u` |  |  |
| Enter | numpad | - | off |  | `CSI 57414u` |  |  |
| Enter | numpad | C | on | `\r` | `CSI 57414;5u` |  |  |
| Enter | numpad | C | off |  | `CSI 57414;5u` |  |  |
| ArrowUp | numpad | - | on | `\eOA` | `CSI 57419u` |  |  |
| ArrowUp | numpad | - | off |  | `CSI 57419u` |  |  |
| 8 | numpad | - | on | `8` | `8` |  |  |
| 8 | numpad | - | off |  | `8` |  |  |
