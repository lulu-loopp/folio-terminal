STATUS COMPLETE

# Math pipeline as an untrusted-input parser — verify f42d9b57

Read-only review. No cargo, no program launched, no process ended. File:LINE are at
`f42d9b57`. Two findings: one HIGH (a cell-budget bypass that restores the ~7M-cell
sparse-rectangle attack the budget was built to stop), one LOW (a contained panic,
already pinned by a test). The three stated claims — three budgets, no executable
Typst, containment — otherwise hold.

## Claim 1 — the three budgets, plus the macro cap. SOUND, with one hole.

* 8 KiB source — `validate_source` refuses `source.len() > MAX_SOURCE_BYTES` (8192)
  (`crates/bt-math/src/lib.rs:765`, const at `:181`), plus the
  `\input`/`\include`/`\includegraphics`/`\write`/`\openout` blocklist (`:768-779`).
* Syntax-tree depth 256 — `BoundedBuilder` mirrors rowan's child stack and refuses a
  level *before* descending (`vendor/mitex-parser/src/depth.rs:108-122`, wrap-aware
  `start_node_at` at `:168-180`); `tree_depth` re-measures iteratively (`:210-223`);
  `parse_bounded` refuses on either the flag or the measured tree
  (`vendor/mitex-parser/src/lib.rs:83-94`); the converter counts its own recursion
  (`vendor/mitex/src/converter.rs:277-279`). Every parser cycle passes `content`, whose
  gate is `if self.builder.at_limit()` (`vendor/mitex-parser/src/parser.rs:395`).
* Laid-out cells 4096 — `MAX_LAYOUT_CELLS` (`vendor/mitex/src/converter.rs:107`),
  `charge_cells` (`:134-156`) charges `rows × widest` before writing, and it is charged
  for `Matrix | Math | Cases` only (`:913-915`).
* Macro work 32 KiB — `MAX_WORK`/`MAX_DEFINITIONS` (`crates/bt-math/src/macro_budget.rs:33-34`),
  `unsupported` names (`:62-78`), additive DAG cost with cycle detection (`:191-214`),
  argument charge `bytes × uses` (`:236-279`). The macro engine pushes expansion back into
  the token stream iteratively (`mitex-lexer/src/macro_engine.rs:390-447`, `expand_tokens`
  loop `:877-931`); it never recurses.

**Hole:** `charge_cells` counts only *direct* children — see Finding 1.

## Claim 2 — no executable Typst. SOUND.

* The three verbatim-copy sites are refused in math-content-only mode
  (`vendor/mitex/src/converter.rs:158-164`, `math_content_only` flag `:110-124`): `\iftypst`,
  `\includegraphics`, `\label` (see `vendor/mitex/CHANGES-FOLIO.md:49-70`).
* Everywhere else a source byte reaches the output through the token map
  (`vendor/mitex/src/converter.rs:357-416`), which escapes `# " ^ _ * @ ; , / ( ) [ ] ~`
  and drops `$ { }` comments. The `Word` class excludes every one of those
  (`mitex-lexer/src/token.rs:107`, regex `[^\s\\%\{\},\$\[\]\(\)\~/_\*@'";&^#]+`).
  In math mode a `Word` is split to single characters (`converter.rs:324-335`), so a
  2+-letter identifier (`range`, `read`, `image`, `sys`) can never be emitted bare; in text
  mode it is markup text, and only `#` names a call, which is escaped. Backtick and `<`/`>`
  are not in the exclusion set and can reach a `#textmath[...]` block, but a Typst code
  span is literal and `<x>` is a label reference — neither executes.
* The only `#`-names emitted are fixed spec aliases plus `math.equation`/`strong`/`emph`;
  aliases come from `mitex_spec_gen::DEFAULT_SPEC` (a build-time `default.rkyv`), never from
  the input. The invariant is tested by `every_formula_that_converts_calls_only_names_mitex_itself_wrote`.
* The template evals `source` only, with `#eval(source, scope: mitex-scope)` where `scope`
  is `base-mitex-scope + (diff, sect, planck)` (`crates/bt-math/src/lib.rs:214-224`); no
  asset calls `eval`. Colors and lengths parse, not eval (`mitex-length` regex with a
  10000 pt cap, `get-tex-color` via `float`/`int`/`rgb`/`cmyk`/`luma`).

## Claim 3 — containment. SOUND.

* Whole render inside `contained` → `catch_unwind` (`crates/bt-math/src/lib.rs:73-85`); the
  conversion has its own boundary too (`:155-179`); the panic hook escalates only what is not
  marked contained (`render_panic_is_contained`, `:94-96`).
* `MATH_WORKER_STACK_BYTES = 16 MiB` (`:196`), chosen against `MAX_NESTING_DEPTH` = 256
  (`:795`), 64 KiB/level, measured by
  `every_recursive_shape_renders_at_the_limit_on_the_worker_stack` (`:190-195`).
* Fonts: a static resolver over exactly three files (`:450-474`), and the SVG reader's
  image resolver returns `None` for every href (`:1148-1152`) — no filesystem/network.
* Raster dims are checked *before* `Pixmap::new`: `checked_mul` of width×height×4, then the
  `width>131072 || height>16384 || bytes>64MiB` caps (`crates/bt-math/src/lib.rs:1036-1098`).
  `float as u32` saturates, so a huge SVG length cannot overflow the arithmetic.

Stated remaining limit confirmed: no wall-clock/memory bound, and the widest admitted
formula at the cell budget is measured in the tens of ms (`:2047-2049`, ~60µs/cell). That
measurement only holds while the cell budget actually sees the cells — see Finding 1.

## Finding 1 (HIGH) — a `&` hidden in a `{…}` group bypasses `charge_cells`

`charge_cells` walks only the environment's direct children:

    for child in env.as_node().unwrap().children_with_tokens() {
        match child.kind() {
            LatexSyntaxKind::TokenAmpersand => { columns += 1; … }
            LatexSyntaxKind::ItemNewLine   => { rows += 1; … } … }}
    // vendor/mitex/src/converter.rs:136-148

A `&` inside `{…}` is a child of `ItemCurly`, not of the environment, so it is not counted.
But it is still emitted as a *column separator*: in math mode `convert_curly_group` swaps the
environment to `MathCurlyGroup` before converting its children
(`vendor/mitex/src/converter.rs:502-505`), and the `&` then takes the `_` arm:

    TokenAmpersand => match self.env { LaTeXEnv::Matrix => f.write_str("zws ,")?,
                                        _ => f.write_str("&")? }
    // vendor/mitex/src/converter.rs:407-410

so the group's `&` is written raw, inline (the group adds no delimiters), into the `mat(…)`
that a `matrix`/`pmatrix`/`array`/`aligned`/`cases` body becomes. `mat` pads every row out to
the widest one — the same premise the existing budget was built on
(`vendor/mitex/CHANGES-FOLIO.md:78-80`, `crates/bt-math/src/lib.rs:2042-2043`).

Construction (all budgets pass): `\begin{matrix}{a&a&…&a}\\…\\\end{matrix}` with K `&` inside
the group and N `\\` at top level. `charge_cells` sees 0 direct `&` and N `\\`, so it charges
N+1 cells (≤ 4096 passes). The converter emits one row of K+1 columns plus N empty rows; Typst
pads to (N+1)×(K+1). At 8 KiB, K+N≈4080 and N≤4095, so ~4.2M cells at ~60µs each
(`lib.rs:2047-2049`) ≈ 4 minutes of the single math worker and roughly a gigabyte of frames —
the exact "minutes of the one math worker and gigabytes" failure the budget exists to stop
(`lib.rs:2045-2047`). `array` is equally exposed through its explicit
`matrix.map(row => row + (m - row.len()) * (none,))` padding
(`assets/mitex-specs/latex/standard.typ:1111-1114`).

**Smallest general fix:** make the charge match what is emitted. Walk into `ItemCurly` (and any
non-environment node) counting `TokenAmpersand`/`ItemNewLine`, stopping only at a nested
`ItemEnv` (which charges itself). Alternatively, emit a group's `&` as `zws ,` inside a `Matrix`
env — but the count-first fix is the one that keeps the invariant "charged = emitted separators".

## Finding 2 (LOW) — contained panic on a trailing `#` in a macro body

`macro_budget::validate` admits `\newcommand{\a}{#}` (`crates/bt-math/src/macro_budget.rs:456`,
and it counts the `#` as a use at `:167-168`). The lexer's macro engine then panics converting
that body:

    let next = def.get_mut(i + 1).unwrap();   // mitex-lexer/src/macro_engine.rs:958

with the `#` the last token, so `i+1` is out of range. This is *contained*: it fires inside
`parse_bounded`, which runs inside `convert_math`'s `catch_unwind` (`lib.rs:167-171`), so it
returns `MathRenderError::ConversionPanic`, a neutral refusal, and the engine stays usable —
exactly what `a_conversion_panic_is_a_neutral_refusal_and_clears_its_guard` pins. Not a
process-down; recorded only to confirm the containment claim, not as a new defect.

## Attack areas, tersely

* **A (injection):** sound — see Claim 2. No `eval(`, no executable identifiers, verbatim
  sites refused, `sys.inputs` values are numeric/font arrays and only the sanitized `source`
  is evaled.
* **B (amplification):** size commands are `ignore-sym` (`standard.typ:293-302`) so font-size
  cannot compound; `\hspace`/`\vspace`/`\raisebox` parse through `mitex-length` with the
  10000 pt cap and the raster-dimension check runs before any pixmap (the
  `999999999999999999999999pt` case refuses in ~10 ms, `lib.rs:2135-2141`). Nesting
  (`\frac`, `\binom`, `\boxed`, over/under-braces, `\left…\right`) is depth-bounded. Sound.
* **C (cell budget):** the `{}`-group bypass (Finding 1). `\hline`/`\cr` are `ignore-sym`
  (`standard.typ:1142-1144`), no row; `\substack`'s `\\` collapses to `\ ` (no rectangle);
  macro bodies are charged through their *expanded* `&`/`\\` (`lib.rs:2091-2102`). `tabular`
  (is-table) is *not* charged, but Typst `table` fills only the last row — linear in the
  8 KiB budget (~8k cells ≈ 0.2 s) — so it is uncharged but harmless.
* **D (depth):** sound — parser gate + wrap tracking + independent `tree_depth` + converter
  counter + `bound_converted_nesting` (`lib.rs:840-846`) + Typst `MAX_DEPTH` 256 + usvg 1024;
  the iterative macro engine cannot recurse deeper than the expanded tree, which the gate sees.
* **E (panics/overflow):** `\label` bare / `\label{` / `\begin{tabular}` bare / `\begin{tabular}{`
  hit `.expect`/slice panics in the converter (`vendor/mitex/src/converter.rs:659,662,1122,1125`),
  all inside `convert_math`'s `catch_unwind`, so neutral refusals; width×height×4 is
  `checked_mul` with a saturation-cast width. Sound.

## Ranked summary

1. **HIGH** — `charge_cells` under-counts `&` inside `{…}` groups, which are still emitted as
   raw `mat`/`aligned`/`array` column separators: the 4096-cell budget can be driven to ~4.2M
   cells ≈ minutes and ~1 GB on the one math worker. Fix in `vendor/mitex/src/converter.rs:134-156`.
2. **LOW** — trailing `#` in a macro body panics in the lexer's `process_macro_def`
   (`mitex-lexer/src/macro_engine.rs:958`); contained, neutral, already test-pinned.

The three budgets (other than the Finding-1 hole), the no-executable-Typst invariant, and the
containment boundary are verified at the cited lines.

## Verification of 20b00282 (the emitted-scope cell counter)

`charge_cells` and its `Matrix|Math|Cases` call site are gone. `convert_node` now charges after
conversion, on the very string Typst will parse:

    if math_content_only && layout_cells(&output) > MAX_LAYOUT_CELLS {
        return Err(BoundedConvertError::TooManyCells); }
    // vendor/mitex/src/converter.rs:1379-1380, MAX_LAYOUT_CELLS = 4096 at :197

**1. The Finding-1 construction is charged (N+1)×(K+1).** `\begin{matrix}{a&a&…&a}\\…\\\end{matrix}`
(K `&` in the group, N `\\`) becomes `matrix(a & a … a zws ; zws ; … zws ;)`. `convert_curly_group`
swaps the env to `MathCurlyGroup` and writes no delimiters (`:553-567`), so the group's `&` takes the
raw-`&` arm (`:458-459`) and `\\` in the `Matrix` env emits `zws ;` (`:462-465`). `layout_cells` pushes
a scope at `matrix`'s `(`, counts `&` a column and `;` a row, pops at `)`, and charges
`rows × widest` (`:105-177`) — (N+1)×(K+1), plus the 1×1 root — refused for any N+1,K+1 ≥ 64. The same
construction through `array` converts to `mitexarray(arg0: l, x zws , … zws ; …)`: the named-arg
`arg0: l,` adds one spurious column, so it charges (N+1)×(K+2) ≥ the rectangle Typst's explicit
`matrix.map(row => row + (m - row.len()) * (none,))` padding lays out (`standard.typ:1111-1114`).
`cases` maps `&` raw and `\\`→`,` (`:462-465`), so a body never emits `;` and both sides see one row of
columns. `aligned` and the bare form map `&` raw and `\\`→`\ `; the counter reads `\ ` as a row and `&`
as a column, so `aligned(a & b \ c & d)` and a bare `a & b \ c & d` charge rows×cols in their scope —
the no-environment bypass the other reviewer found is charged in the root scope. (`zws` is inert: the
counter's four separators `& , ; \ ` are exactly the ones `mat`/`math.display`/`math.cases` use.)

**2. The scanner is bracket-faithful, so it cannot be desynchronised.** It reads the same characters
Typst's parser reads and pushes/pops `(` `[` `{` exactly where Typst groups; the converter never emits
`{`, and its `(` `)` are only the balanced pairs of `convert_env` (`:1003,:1022`), `convert_clause_lr`
(`:597,:613`), attach `_(`/`^(` and the math-arg arm (`:868,:888,:890,:903`), with `[` `]` the balanced
content blocks (`:909,:913`). Every divergence is in the safe direction:
* `\left(` without `\right)` emits an unbalanced `lr(`; the end-of-scan loop `for scope in &open { … }`
  (`:173-175`) charges the open scope, and Typst refuses the unclosed `(` — no rectangle.
* A stray `)` pops only when `open.len() > 1` (`:163`); at the root it is ignored, and Typst treats an
  extra `)` as a syntax error, never as a grid surviving the pop. So rows and columns of one rectangle
  always share one counter-scope, and the SUM charges rows×cols, never rows+cols.
* Escapes, strings, `[...]` markup and ordinary call commas (`frac(a, b)`) are all handled or
  over-counted: `\(` `\)` `\,` `\#` are consumed as escapes, `"…"` is skipped with `\"`/`\\` honoured, a
  `\ ` inside a string cannot fire `row()`, a trailing `\` is a no-op not a panic, and `scope.cells()`
  is a `saturating_mul`/`saturating_add` that cannot wrap into an under-count.

**3. No rectangle is built from a separator the converter did not emit — except fixed-size asset
mats.** `matrix`/`pmatrix`… are `matrix-handle(…) = define-env(…, handle: math.mat.with(delim: …))`
(`standard.typ:136-141,1091-1097`); `array`'s `grid` is fed by `args.pos()` split on the emitted `,`/`;`
(`:1111-1125`); `cases`/`aligned` use the emitted separators. The only rectangles whose `;` is written
by an asset, not the converter, are `atop`→`mat(delim: none, a; b)` (`:1171`), `brace`/`brack`→
`mat(delim: "{", n;; k)` (`:1173-1174`) and `binom` (`:337`): 2–3 cells of fixed shape, the two
arguments spliced as single cells. `substack` drops `\\` (`no rectangle`). No asset re-splits an
argument's own separators, so none can multiply an argument into a rectangle; the counter under-counts
`brace`/`brack` by one cell — a constant, bounded by the 8 KiB source, non-amplifying.

**4. No false refusal.** `\frac ab`→`frac(a, b)` charges 2; a 20-deep fraction ≈ 40, a 10×10 matrix 100,
a 100-line 3-point alignment 300, a 50-branch `cases` 50. The densest comma emitter is a bare 2-arg
command (`frac`/`binom`) at ≈ 2048 cells for a full 8 KiB of commands — still under 4096. Only a genuine
64×64+ matrix (or thousands of rows/columns) trips it, which is exactly what the budget must stop.

**Verdict: CORRECT.** The emitted-scope counter closes Finding 1 and the bare-alignment bypass (rows ×
widest is charged on the very string Typst parses), is bracket-faithful (no desync; unbalanced brackets
refuse or are charged, never split a grid), over-counts wherever its semantics differ from Typst's, and
builds no unbounded rectangle from asset-written separators. The single constant 2-vs-3-cell under-count
of `brace`/`brack` cannot amplify; if one wanted to erase it, the smallest general change is to charge a
call whose alias is a fixed grid-builder its known rectangle — but no fix is required for the budget to
hold.

STATUS COMPLETE
