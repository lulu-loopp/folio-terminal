# M1 direct-grid width probe

`width-probe-input.vt` is a raw UTF-8/VT byte fixture for `bt-app`. It uses CUP to place the legacy and mode-2027 cases side by side, so the whole matrix fits in one non-scrolling window. It must not be printed by PowerShell or copied through ConPTY.

Run from the repository root:

```powershell
$env:BT_PROBE_INPUT = (Resolve-Path .\scripts\dev\width-probe-input.vt)
cargo run -p bt-app --release --locked
```

The left half is legacy mode; the fixture then sends `CSI ? 2027 h` and draws the right half before restoring with `CSI ? 2027 l`. For every pair, the closing `|` on the content row must align with the closing `|` on its `#` ruler row. Column occupancy is the assertion. M1.5 additionally requires real color emoji; use `glyph-probe-input.vt` and its companion instructions for that visual check.

| Case | Legacy cells | Mode 2027 cells |
|---|---:|---:|
| family ZWJ sequence | 8 | 2 |
| thumbs-up + skin tone | 4 | 2 |
| US regional-indicator flag | 2 | 2 |
| `e` + combining acute | 1 | 1 |
| `A☆中│Ｂ` mixed text | 7 | 7 |
| umbrella + VS15 | 1 | 1 |
| umbrella + VS16 | 1 | 2 |

Unset the variable after the check to return to the normal ConPTY-backed shell:

```powershell
Remove-Item Env:BT_PROBE_INPUT
```
