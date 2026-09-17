# Math guard review, round 2

Target: `d7aeb9b64594fa8b1e1674656fed8ba01a3b518a` (`fix/math-recursion-guard`).
Review in progress; no verdict yet. Product code is read-only; scratch tests will be removed.
Safety: no stack-overflow experiments, unbounded loops, application launches, or process termination.
Each confirmed finding is recorded immediately. References below are to the target commit.

## Findings

### F1 — high: sparse arrays amplify into millions of elements before any size check

`assets/mitex-specs/latex/standard.typ:1089-1102` computes the widest row, pads **every** row to that width, flattens the rectangle, and wraps every cell in an equation for `grid`.
A source built as `\begin{array}{l}x` + `&` repeated N + `\\` repeated N + `x\end{array}` has `3N+29` bytes, constant nesting, and requests `(N+1)^2` cells (including padding).
At N=2700 this is 8,129 bytes and 7,295,401 cells. This large case was NOT executed; it is a direct count from the helper, with N=8 (53 bytes) confirmed to render in 14 ms.
`crates/bt-math/src/lib.rs:499-523` checks bytes/macros/depth, then compiles; `:473-489` completes layout and SVG generation before `:1038` checks raster size.
There is no bound on expanded rectangular area or allocated layout elements. Allocation failure can abort the process, outside `contained`/`catch_unwind`; the single worker can also be occupied far beyond the documented sample.
Must fix before 0.4.2: cap expanded rows × columns and total generated elements before padding/layout (including macro-expanded input and `subarray`/matrix paths). A post-layout raster cap is too late.

## Coverage and validation

Original seven attack items plus emission safety and availability are being reviewed.
