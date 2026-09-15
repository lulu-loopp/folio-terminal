STATUS COMPLETE

## C-1 — high — Parameter forwarding defeats the macro work bound

Location at 6a414963: `crates/bt-math/src/macro_budget.rs:189` and `crates/bt-math/src/macro_budget.rs:248`.

```rust
let cost = children.iter().fold(definition.bytes, |total, name| {
    total.saturating_add(costs[**name]).min(MAX_WORK + 1)
});
```

```rust
work = work
    .saturating_add(bytes.saturating_mul(definition.uses.max(1)))
    .min(MAX_WORK + 1);
```

Trigger: render this 360-byte formula source (inside display delimiters):

```tex
\newcommand{\a}[1]{\b{#1#1#1#1#1#1#1#1#1#1}}
\newcommand{\b}[1]{\c{#1#1#1#1#1#1#1#1#1#1}}
\newcommand{\c}[1]{\d{#1#1#1#1#1#1#1#1#1#1}}
\newcommand{\d}[1]{\e{#1#1#1#1#1#1#1#1#1#1}}
\newcommand{\e}[1]{\f{#1#1#1#1#1#1#1#1#1#1}}
\newcommand{\f}[1]{\g{#1#1#1#1#1#1#1#1#1#1}}
\newcommand{\g}[1]{\h{#1#1#1#1#1#1#1#1#1#1}}
\newcommand{\h}[1]{#1#1#1#1#1#1#1#1#1#1}\a{x}
```

The dependency graph is acyclic. Each source argument contains only parameter tokens or literal `x`, so the defined-macro argument guard accepts it. The DAG costs omit parameter amplification, and the second pass charges the original `#1...#1` groups rather than the expanded arguments. Its work charge is below 4 KiB, beneath `MAX_WORK = 32 KiB`.

Consequence: each of eight expansions multiplies the argument by ten, producing 100,000,000 `x` tokens. The pinned `mitex-lexer 0.2.4` eagerly builds these vectors (`macro_engine.rs:619-624`, `877-892`, including `result.extend(arg.iter().cloned())`) without a runtime budget. A tiny formula therefore causes multi-gigabyte allocation and prolonged worker blockage; allocation failure can abort Folio. The conversion unwind boundary does not contain allocation aborts.

Smallest correct fix: conservatively refuse calls to user-defined parameterized macros from within macro definitions, including through redefinitions, until the DAG cost propagates argument-dependent expansion costs. Keep direct parameterized calls with bounded literal arguments.

## C-2 — medium — Formula tools use the focused pane's dimensions for another pane

Location at 6a414963: `crates/bt-render/src/lib.rs:7067`.

```rust
math_tool_boxes_for(self.metrics, self.seat, frame, hovered)
```

Trigger: split a window into a 200-pixel-wide focused pane and a 600-pixel-wide unfocused pane. At scale 1, with 8-pixel padding, hover a rendered display formula in the wider pane whose visible block is `[16, 50, 316, 90]`. Hover inside its left portion so the existing hit test admits it; the wider pane's frame now carries the hovered anchor and `toolbar_visible`.

`compose_frame` draws that block using `entry.seat` (`lib.rs:8174`) and then restores `self.seat = focused_seat` (`lib.rs:8371`). The new tool accessor consequently clamps the wider frame against width 200. `math_block_geometry_px` uses that seat width and height (`lib.rs:382-386`, `420-422`). The app calls this accessor for the hovered pane's presented frame and only translates the returned rectangles afterward (`crates/bt-app/src/main.rs:85028-85039`).

Consequence: the returned block ends at x=200, and the two tools occupy x=150..200 instead of x=316..366 beside the actual block. They cover formula ink despite ample room in its pane. Different pane heights can also suppress tool geometry for a block that is visible in the taller pane.

Smallest correct fix: pass the frame's actual `SeatViewport` into `math_tool_boxes` and use it for `math_tool_boxes_for`; obtain the hovered pane's body viewport before the app calls the accessor. Pass that same viewport through its formula hit-test path so drawing and interaction use the same pane dimensions.
