# Formula pipeline review, round 3

Target: `f42d9b57`; review date: 2026-09-17. Product code remains unchanged.
Verdict: pending completion.

## Method and safety
Read-only source review; bounded permitted tests only. Dangerous constructions are reasoned about, never executed.
Both round-1 and round-2 briefs apply. Findings and completed attack items are appended as they are established.

## Findings and attack coverage
### F1 - high: alignment rectangles bypass the cell budget (priority 1 / item 9)
`vendor/mitex/src/converter.rs:134` counts only immediate environment children, and `:913` charges only environments.
But `:407` emits `&` and `:411` emits line breaks outside matrices too; `:491` flattens curly groups, preserving their ampersands (`:503` changes context, not scope).
Construction A: bare `x` + `&` repeated N + `\\x` repeated N, without any environment. No cells are charged.
Construction B: inside `aligned`, put the wide row's ampersands inside `{...}` and keep row breaks outside; the counter sees N+1 rows and only one column, while Typst sees N+1 columns.
Typst 0.15.0 `typst-library/src/math/ir/process.rs:54` pads every multiline row to the widest one; `multiline.rs:31` materializes each missing item. Thus both request (N+1)^2 cells with linear source and shallow trees.
The dangerous scale was NOT executed. Enforce the budget over emitted alignment scopes, including the root and flattened groups, before Typst IR construction; counting named environments is insufficient.
