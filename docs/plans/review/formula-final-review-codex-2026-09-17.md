# Formula final review, 2026-09-17

Target: `95d5bad6`, including merged live-site work. R2 (`951f7788`) and R3 (`95d5bad6`) are excluded. References are at the target; S = crates/bt-term/src/session.rs, T = vendor/alacritty_terminal/src/term/mod.rs, N = crates/bt-math/src/nesting.rs. The report was saved after the first finding and updated during the review.

## Verdicts

- `7abdd6ae`, positive cell claim: **hold**; relocation laundering and provenance gaps below.
- `bc4eb3e9`, R4 recursion: **hold**; the pre-conversion bound remains false.
- `c71fd102`, R1 crop/cache identity: **merge into 0.4.2**.
- Branch excluding R2/R3: **hold** under both requested release bars.

## F1 — must-fix: an append can grant ownership of the old prompt glyph

T:1905-1918 widens a one-cell grapheme at the right margin by relocating its whole cluster through `write_grapheme_at_cursor`; T:1541 stamps the current writer onto it. At 60 columns: output writes `energy $x^2 + ` and spaces through column 59; `D A` switches to prompt provenance; the prompt writes `↔` in column 60; `C` switches to output; append U+FE0F, then `$ here CRLF`. The relocated cell is `↔\u{fe0f}` with `command_output_write: true`, although its base came from the prompt. The ordinary `append_zerowidth` rule is bypassed by reconstruction through the replacement path. Preserve the old claim across width changes/relocation, and intersect it with the appender's provenance. The relocated prompt glyph reaches a **Rendered inline** block in the scratch test. A second reproduction hits T:2330: output writes `energy $x^2$ here `; the prompt appends U+0301 to its trailing space; output CUPs to that cell and writes a tab. `put_tab` retains the prompt's extra combining mark while claiming the cell: `text = "\t\u{301}", command_output_write = true`. The mixed row renders too. Clear replaced extra text or preserve its unclaimed provenance; stamping `.c` alone is insufficient. Both safe-behavior assertions failed before observation checks.

## F2 — must-fix for the stated bar: phase is not producer authentication

S:3946 calls integration authoritative when `shell_region_screens` contains the screen. S:4217-4245 accepts **any** in-band C, creates that membership, and enters Output, even from Prompt or without B. S:3043 then stamps subsequent bytes. Replay `A PS> B C CRLF energy $x^2$ here CRLF`: the forged C makes an inline block render. Frozen replay likewise retains the claim and detects inline math. No out-of-band/authenticated producer check was found. Enforcing A/B/C order alone would not establish who emitted those bytes; the protocol's trust contract needs an explicit resolution before claiming the requested absolute guarantee. Type-ahead **echo arriving during C..D** is also stamped output. A replay of typed `energy $x^2$ here` renders inline; an `echo ...` variant happened to be rejected by lexical heuristics, which is not provenance protection. Input buffered until the next B is a different case. No interactive shell was launched for this audit. Alternate-screen content is explicitly eligible by policy (S:14708), independently of the output flag; thus the literal global bar also needs that scope clarified.

## F3 — false refusal: screen swaps do not split provenance segments

Feed `C ESC[?1049h`, then separately `ESC[?1049l energy $x^2$ here CRLF` (without the illustrative space after the mode sequence). The second feed starts on alternate screen without authoritative output, so primary output after the swap gets an unclaimed cell and no rendered frame block. The scratch assertion requiring its claim fails. `adapter.rs:775-845` pauses at shell markers, not screen transitions; a single `write_provenance` value spans the swap. Restate it at every screen transition.

## Cell audit and ordinary-use coverage

- Tabs over blank interior cells, wide text/spacers, REP, and charset-mapped output
  render. `carries_unclaimed_text` ignores whitespace-only cells and wide spacers;
  the base answers for wide text. A leading tab can instead trigger code indentation.
- Partial output followed by a prompt on the same row is Ineligible, live and frozen.
  RPROMPT residue on an output row also refuses the row; this is conservative loss
  of output decoration, not laundering. An ordinary newline keeps those rows apart.
- Both live and freeze use the same predicate (`bt-transcript/src/lib.rs:827,1497`).
  Existing prompt reprint, tabs, combining marks, DECALN, moves and resize gates pass.
- All production `.c =` assignments in T are replacement (:1573), DECALN (:2077),
  and tab (:2330); grid has no production character assignment. DECALN resets first
  and stays unclaimed, including when output invokes it. Erase resets flags/extra.
  REP calls `Handler::input`; input paths reach stamping or zero-width handling.
  DECSC/DECRC restore cursor/template/charset, not the separate provenance field.
  Fork/clone and grid moves carry cells; resize uses an empty template; alt swaps
  retain primary cells. These do not cure the relocation and segment defects above.
- `folio.ps1:552-604` emits C after OriginalReadLine returns, after redraw/newline;
  [PSReadLine AcceptLine](https://github.com/PowerShell/PSReadLine/blob/v2.4.5/PSReadLine/BasicEditing.cs#L286-L359)
  confirms that order. `folio.zsh:195` emits C in preexec, after ZLE finishes its
  redraw/newline ([ZLE source](https://github.com/zsh-users/zsh/blob/zsh-5.9/Src/Zle/zle_main.c#L1292-L1325),
  [preexec order](https://zsh.sourceforge.io/Doc/Release/Functions.html#Hook-Functions)).
  Normal editor echo therefore precedes C. A custom later hook/redraw inside C..D
  receives the output claim; no distinction is made for its command-line text.

## F4 — must-fix: R4 undercounts the parser/converter's recursive structure

Scratch tests reuse the exact production nesting and macro-budget modules, then convert modest 300-level examples on a 16 MiB thread. All fit the source/work caps and pass the complete macro-budget validator; none exceeds the raw brace limit.

| Input construction | Scan depth | Converted bracket depth |
| --- | ---: | ---: |
| `x\over ` x300, then `x` | 0 | 300 |
| `\displaystyle x ` x300 | 1 | 300 |
| `x`, then `\limits` x300 | 0 | 300 |
| `\sqrt[2]` x300, then `x` | 2 | 301 |
| Define `\f` as `\frac`; `\f a ` x300, then `x` | accepts | 300 |

N:64-76 treats left/infix nesting as zero and greedy/glob commands as one term. Pinned MiTeX 0.2.4's `command` wraps previous syntax for Left1/InfixGreedy; greedy argument matching recursively consumes the rest of scope. sqrt's glob is `{,b}t`: closing its optional bracket does not finish the required radicand. N:196 replaces macro continuations by Terms(1), losing pending arity. Two scalar depth values cannot represent these parser states. Fix the simulation, including argument substitution, or enforce a proven bound inside the recursive implementation before descent. Syntactic arities alone are insufficient. `x^` x300 is a negative control (depth 1); the existing bare-caret rejection passes. Flat `\frac12+` x300 is falsely refused: the parser takes two character terms from Word `12+`; the scan settles that token only once. Array column arguments, `&`/`\\`, and `\text{a $x^2$ b}` validate in ordinary probes. The plain lexer uses iteration/bounded token buffering, not recursive tokenization. The scan uses Vec frames, but `close` searches them backwards: not strictly linear. The full render test really compiles/layouts/rasterizes its deepest accepted fraction on exactly 16 MiB, and passes; a 30-level continued fraction also fully renders. That measures one shape, not all accepted recursion. `lib.rs:463-465` checks converted nesting only after conversion (and delimiter normalization). No overflow was attempted; the 256-level precondition is disproved, not a measured production stack crash. There is one 16 MiB math worker per App instance (:38123), shared by its panes; no worker-per-formula multiplication. Other worker stacks are not 16 MiB by this change.

## R1 and validation

At 40 columns, two five-run lines with short/long prefixes yield one base key with `#x0-380`, `#x0-259`, `#x480-498`, `#x360-498`; all key/bitmap identities agree. The old left-edge suffix collides at x0. The committed 264/139 crop regression passes. Frozen/live base builders share `shared_math_artifact_key` (S:11994,12023); width, height and ASCII baseline are hashed at S:12085. Both placement paths call the same crop builder (S:9040,9140). The same five-run probe also passes after freezing and scrolling to history, comparing keys against the live bitmap cache.

All cargo commands used the allowed `cargo test` forms with `-j 4`; no heavier cargo command was run.

| Allowed command suffix | Passing stock tests |
| --- | ---: |
| `-p bt-term --lib inline / site / no_road / crop / artifact / prompt` (six runs) | 81 / 14 / 1 / 1 / 8 / 19; 114 unique |
| `-p bt-term --test lifecycle_matrix` | 42 |
| `-p alacritty_terminal` | 146 unit + 45 reference + 1 doc |
| `-p bt-detect` | 150 unit + 1 integration |
| `-p bt-math` | 43 unit + 11 integration; 1 ignored machine probe |
| `-p bt-viewport` | 141 unit + 3 integration |
| `-p bt-render --lib math` | 4 |
| `-p bt-app --bin folio hostile_math` | 1 parent test; its child probe also passes |

Total: 702 distinct stock tests passed, one ignored. Six lifecycle scratch tests cover 11 live sequences, four frozen sequences, both crop planes and the three new cell/transition defects. Four new math scratch tests pass, plus 12 duplicated macro-budget tests pulled in by the harness. Passing observation tests reproduce failures; they do not make the branch safe. Scratch import/fixture errors were corrected before final runs. Scratch sources were deleted; restored lifecycle and math suites were rerun. Product code is unchanged. No application was launched, no existing process was ended, and no crash-inducing stack-overflow test was run.
