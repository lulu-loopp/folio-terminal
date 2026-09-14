# Adversarial review: the formula pipeline (terminal road and preview road) — 2026-09-14

Read-only Codex review (high reasoning) at main `8ad6d382`, commissioned before 0.4.0. Findings 2, 3, 4 and 7 are taken into 0.4.0 (T-MATH-ROBUSTNESS, T-MATH-INLINE-INFTY); the rest are queued for 0.4.1. The three defects the owner reported the same day are qualified at the end.

STATUS: Static review complete; requested report file **not written** because this session permits filesystem reads only. No files changed, Cargo invoked, tests run, or commits created. Reviewed HEAD: `8ad6d382775c3f9f7af51703848b4965ea96f43a`.

The findings below are source-verified; proposed regression tests were not executed.

1. **High — Tall inline preview formulas overlap adjacent text.**

   **Location:** [main.rs:4486](crates/bt-app/src/main.rs:4486), [main.rs:7493](crates/bt-app/src/main.rs:7493), [bt-render/lib.rs:1491](crates/bt-render/src/lib.rs:1491).

   **Input:** A wrapped paragraph containing `$\dfrac{1}{\dfrac{1}{2}}$`, with text immediately above and below.

   **Cause:** A resolved formula contributes only its width to the text layout. Painting subsequently places its full raster using its baseline, without fitting its ascent/descent into the line or enlarging that line. Its clip is the page/block window, so a sufficiently tall formula can paint over neighbouring lines. Late arrival changes wrapping but does not make vertical occupancy correct.

   **Pin:** Resolve a formula whose ascent/descent exceed the prose line box between two text lines. Assert nonintersecting ink bounds after arrival, including in a narrow peek card. The test must check vertical geometry, not merely the placeholder’s width.

2. **Medium — An incomplete macro definition can panic the shared decoration worker.**

   **Location:** [bt-math/lib.rs:216](crates/bt-math/src/lib.rs:216), [main.rs:1456](crates/bt-app/src/main.rs:1456); pinned dependency [macro_engine.rs:958](<registry: mitex-lexer-0.2.4/src/macro_engine.rs:958>).

   **Input:** `$$\newcommand{\a}{#}$$`

   **Cause:** MiTeX reads the definition without its enclosing braces, then unconditionally unwraps the token after `#`. Here there is no next token. Folio’s direct conversion call has no panic containment around it. Release uses unwinding, so this kills the worker thread rather than returning `MathRenderError`. Subsequent formulas and other tasks serviced by that thread lose their worker.

   This is a realistic intermediate editing state, hence medium under the requested input-based rubric.

   **Pin:** Submit this input through the worker, followed by `x+1`. Require a refusal for the first and a successful completion for the second.

3. **Medium — Recursive macros can permanently occupy the shared worker.**

   **Location:** [bt-math/lib.rs:392](crates/bt-math/src/lib.rs:392), [main.rs:1412](crates/bt-app/src/main.rs:1412); dependency [macro_engine.rs:400](<registry: mitex-lexer-0.2.4/src/macro_engine.rs:400>), [macro_engine.rs:615](<registry: mitex-lexer-0.2.4/src/macro_engine.rs:615>).

   **Input:** `$$\newcommand{\a}{\a}\a$$`

   **Cause:** Validation bounds original bytes and brace depth. Macro expansion repeatedly puts expanded tokens back into the input stream without an expansion budget. The self-reference makes no output progress. Folio has neither a conversion deadline nor isolation that can terminate this work. Raster limits apply much later.

   **Pin:** Run recursive and exponentially expanding macros in a disposable test process with a watchdog. Require a bounded refusal and subsequent successful work. A timeout around an unkillable background thread is insufficient.

4. **Medium — Two formulas on different soft-wrapped rows suppress the entire inline composite.**

   **Location:** [session.rs:11454](crates/bt-term/src/session.rs:11454), [session.rs:13719](crates/bt-term/src/session.rs:13719). Both frozen and live callers use this placement function.

   **Input:** At eight columns, command output `$x$     $y$`, with OSC 133 command-output eligibility. Neither formula itself crosses the wrap.

   **Cause:** Detection groups the logical line’s formulas together. Rendering builds one horizontal composite. Placement then requires every rendered run to begin on the first run’s physical row; otherwise it returns `None`. Widening the terminal can make both formulas reappear.

   This is distinct from the already-ticketed single formula crossing a wrap.

   **Pin:** Test this two-run line on live primary, alternate screen, and frozen scrollback; resize between eight and sixteen columns. Both formulas must retain independent pictures and leave surrounding text untouched.

5. **Medium — An earlier price consumes a later valid formula’s delimiters.**

   **Location:** [bt-detect/lib.rs:683](crates/bt-detect/src/lib.rs:683), especially advancement at line 709.

   **Input:** Command output `$5; $x$`.

   **Cause:** The dollar before `x` cannot close the price candidate because it precedes an identifier. The scanner instead pairs the first price dollar with the final dollar. After rejecting that combined body, it advances past the entire candidate. It never retries the opener belonging to `$x$`.

   **Pin:** Assert exactly one run, source `x`, with its correct byte range. Include an escaped price, several prices, and two valid formulas following a price.

6. **Medium — The terminal completeness gate rejects valid interval notation and asymmetric delimiters.**

   **Location:** [bt-detect/lib.rs:835](crates/bt-detect/src/lib.rs:835), [bt-detect/lib.rs:842](crates/bt-detect/src/lib.rs:842).

   **Input:** `$[0,1)$`; also `$\left[0,1\right)$`.

   **Cause:** The gate demands matching literal bracket types. Mathematical intervals deliberately use different endpoint delimiters. Likewise, TeX’s `\left`/`\right` can use asymmetric or invisible delimiters, which this character-stack check does not understand.

   **Pin:** Accept half-open intervals and `\left.\frac{dy}{dx}\right|_{0}` while retaining rejection of genuinely unfinished expressions such as `$f(x$`.

7. **Medium — A display formula followed by prose can absorb the remaining Markdown document.**

   **Location:** [preview.rs:2386](crates/bt-app/src/preview.rs:2386).

   **Input:**
   ```text
   $$x$$ tail

   # Heading
   ```

   **Cause:** The same-line branch recognises a closer only when `$$` is the line’s suffix. Otherwise it treats the whole remainder as the beginning of an unterminated block and consumes subsequent lines until another suffix closer or EOF. The heading becomes formula source. The empty `$$$$` case also falls into the multiline branch.

   **Pin:** Preserve the trailing prose and following heading, or conservatively preserve the original paragraph when that spelling is unsupported. Neither outcome may absorb unrelated blocks. Test empty delimiters separately.

8. **Medium — Refused display formulas have clipped, horizontally unreachable source.**

   **Location:** [main.rs:61945](crates/bt-app/src/main.rs:61945), [main.rs:6309](crates/bt-app/src/main.rs:6309), overflow handling at [main.rs:5794](crates/bt-app/src/main.rs:5794).

   **Input:** In a narrow preview, a long display formula beginning with unsupported `\nosuchcommand`, followed by enough `+x` terms to exceed the measure.

   **Cause:** The fallback reserves height from source line count but no measured width. Painting uses `wrap: false`. Horizontal scrolling depends on recorded width exceeding the measure, so the clipped source cannot be reached through the block’s scrollbar.

   **Pin:** Force refusal for a long single-line formula. Require either complete wrapped source with matching height or a measured, scrollable source block. Repeat while pending and in the peek card.

9. **Medium — Viewport realization does not bound formula work or retained formula memory.**

   **Location:** [main.rs:4811](crates/bt-app/src/main.rs:4811), [main.rs:61338](crates/bt-app/src/main.rs:61338), [main.rs:1364](crates/bt-app/src/main.rs:1364), [main.rs:2277](crates/bt-app/src/main.rs:2277).

   **Input:** Open—or briefly hover—a document containing thousands of distinct `$$x_{i}$$` blocks, then switch to a small document.

   **Cause:** Formula discovery traverses every block and sends every cache miss to an unbounded FIFO shared with terminal rendering and other decoration tasks. It has no viewport admission or cancellation of obsolete preview requests. `DocumentMath` retains raster `Arc`s after cache eviction, so the 48 MiB cache ceiling does not bound a document’s retained pixels. Pending and refused entries also retain their source keys without contributing to that byte accounting.

   **Pin:** Resolve a large document into a small viewport, then replace it. Assert bounded queued work and bounded total retained storage, including keys and document-held rasters. A subsequent visible terminal formula must not wait behind the abandoned document.

10. **Medium — Missing-glyph refusal covers only selected Unicode ranges.**

    **Location:** [bt-math/lib.rs:500](crates/bt-math/src/lib.rs:500), [bt-math/lib.rs:510](crates/bt-math/src/lib.rs:510).

    **Input:** `\text{𠮷}` on a font environment lacking that character; use `with_system_fonts(false)` for a controlled regression fixture.

    **Cause:** The missing-glyph check requires a character to pass `is_cjk_character`. That predicate excludes supplementary Han, Hiragana, Katakana, Hangul, and non-CJK symbols. Missing glyphs for these inputs can therefore be accepted as successful raster output. U+2212 is also outside this backstop, although that alone does not establish the reported macOS failure.

    **Pin:** Use a controlled font set and verify unsupported characters cause an explicit refusal rather than an accepted tofu raster. Include supplementary Han and a missing mathematical symbol.

11. **Low — `smallmatrix` discards its custom sizing handler.**

    **Location:** [standard.typ:76](assets/mitex-specs/latex/standard.typ:76), [standard.typ:1037](assets/mitex-specs/latex/standard.typ:1037).

    **Input:** `\begin{smallmatrix}a&b\\c&d\end{smallmatrix}`.

    **Cause:** `matrix-handle` accepts a `handle` parameter but always installs `math.mat.with(delim: delim)`. The caller’s inline-size handler is never used.

    **Pin:** Compare `smallmatrix` and `matrix` under the same render key. Require the intended smaller geometry while preserving rows, columns, and absent delimiters.

The three existing tickets need these qualifications:

- **Single formula crossing a soft wrap:** the current source contains a repair, including logical-line rendering, multirow cell collection, and dedicated tests at [session.rs:27431](crates/bt-term/src/session.rs:27431) and [session.rs:27570](crates/bt-term/src/session.rs:27570). Pictures still require enough space before their starting row’s edge. Finding 4 remains outside those single-run tests.
- **One-column `pmatrix`:** I could not establish that it remains broken in this checkout. Pinned MiTeX emits row semicolons; pinned Typst 0.15 explicitly groups semicolon-separated arguments into arrays and accepts array rows. This needs an executed two-row, one-column raster test before calling the ticket fixed or reproduced.
- **macOS unary minus:** not reproduced here. Both roads request **New Computer Modern Math**, with embedded fonts and system fallback, in [bt-math/lib.rs:33](crates/bt-math/src/lib.rs:33). Formula glyphs reach `bt-render` as pixels, not platform-specific math glyph runs. Compare `-x`, `x-y`, and literal `−x` through the actual math engine on macOS; terminal glyph-atlas tests cannot settle this ticket.

Other checks did not justify additional findings: terminal font revision and DPI enter layout invalidation; preview keys include source, mode, size, and ink; late picture arrival invalidates viewport exactness. The wrap cache includes resolved inline widths, so omission of global math generation is intentional and appropriate for its present measurement inputs. GPU formulas are tiled to device limits, with a 64 MiB texture-cache budget matching the individual raster ceiling. Peek overlays now gather both page-level and scrolling-block rasters. None of these source checks substitutes for executed resize, redraw, or platform tests.

**Ranked release order:** 1 → 2 → 3 → 4 → 8 → 9 → 7 → 5 → 6 → 10 → 11.

**The three fixes I would make first:**

1. Contain converter panics and bound macro expansion, so one formula cannot disable shared work.
2. Give inline preview formulas explicit ascent/descent handling and correct line occupancy.
3. Replace the terminal’s single-row composite assumption with independently placed formula runs.
