# M1.5 glyph-quality probe

`glyph-probe-input.vt` is a raw UTF-8/VT fixture for the renderer's glyph-quality checks. Run it from any working directory by assigning an absolute path to `BT_PROBE_INPUT`:

```powershell
$env:BT_PROBE_INPUT = (Resolve-Path .\scripts\dev\glyph-probe-input.vt)
cargo run -p bt-app --release --locked
```

The fixture enables mode 2027 for the emoji matrix, then restores legacy mode before the ambiguous-symbol and italic checks. Expected results:

| Row | Expected presentation | Expected slot |
|---|---|---:|
| `👨‍👩‍👧‍👦` | one color family glyph | 2 cells |
| `👍🏽` | one color glyph including skin tone | 2 cells |
| `🇺🇸` | one color US flag glyph, not two letter boxes | 2 cells |
| `☂️` | color emoji (VS16) | 2 cells |
| `☂︎` | monochrome text symbol (VS15) | 1 cell |
| `☆` / `│` | monochrome text symbols; ink stays between adjacent `|` markers | 1 cell each |
| italic `fj/` | primary Consolas keeps its natural bearings/overhang | unchanged |

This is a direct-grid visual check: do not print the fixture through PowerShell or ConPTY. The M1 width matrix remains in `width-probe-input.vt`; M1.5 does not replace or reorder its fourteen content/ruler baseline rows.

Unset `BT_PROBE_INPUT` after the check to return to the normal shell path.

