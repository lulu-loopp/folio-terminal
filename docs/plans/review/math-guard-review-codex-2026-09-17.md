# Formula nesting guard review, 2026-09-17

Target: `8a8cb616`. Provisional verdict: **hold**; validation still running.
Scope: F4 of the final formula review, parser/converter guards, downstream
Typst recursion, vendoring, and realistic-formula acceptance.

Product code is read-only. Only the four authorized cargo test suites will run.
Deep probes require an inspected rejecting guard and an explicit stack of at
least 16 MiB; suspected guard bypasses will be reported without execution.

## Findings and validation

## F1 — must-fix: executable Typst bypasses the source-depth argument

`vendor/mitex-parser/src/parser.rs:452,627-655` accepts `\iftypst...\fi`
and collects its body without parsing its structure. `vendor/mitex/src/converter.rs:347-348`
emits that body verbatim. `crates/bt-math/src/lib.rs:726-747` and
`crates/bt-math/src/macro_budget.rs:47-77,103-119` do not reject it.
Thus `lib.rs:788-791` is false: MiTeX can emit a Typst loop accumulating content.
A shallow four-iteration rendering probe is pending; no deep loop will be run.
Increasing a loop's numeric bound does not increase source, MiTeX tree, or Typst
parse depth. The unguarded math resolver can then descend that generated content.
Reject executable Typst at a trustworthy boundary (including macro expansion),
or isolate/actually bound downstream content creation and recursive consumption.

Parser/converter inspection so far supports their local height bound. Their
success does not establish the end-to-end release bar.
