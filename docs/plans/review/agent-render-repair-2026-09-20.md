# An agent's own redraw eats a formula's row separators — what is repairable

Branch `fix/repair-an-agents-redraw`, from `main` at `95502c1b`. Implementer's report.

## Where a span's text reaches the typesetter

`scan_math_blocks_impl` pairs the delimiter lines and, at the closer, joins the block's logical
lines with `joined_range`, keeping **two** strings on the span: `original_source` (the terminal's
bytes) and `render_source`. The one transformation between them is
`restore_stripped_environment_newlines`, which already existed. `render_task_math` hands
`render_source` to `MathEngine::render`; copy and show-source read `original_source`, so A4 holds
by construction and did before this branch.

**The ticket's premise is partly stale.** H1's multi-line half and H2 are on `main`: measured
there, damaged `aligned` / `pmatrix` / `cases` / column-vector blocks already reach the typesetter
byte-identically to the undamaged ones, and `# $$` opens a block only when a closer answers it.

## H1 — the rule, and the TeX argument for its restriction

The inverse is **not** unique on TeX semantics alone. `\` before a line end is the control-space
primitive — TeX strips trailing spaces, appends the end-of-line character, and `\` + that is the
same token as `\ ` mid-line — and our typesetter agrees:
`display_page_margin_preserves_overshooting_ink` pins `\begin{aligned}… \ …\end{aligned}`
rendering *successfully*, at single-row height. The damage does not reliably fall back to source;
it can set two rows as one, so a wrong repair swaps one wrong picture for another.

What *is* unique is the string transform: CommonMark's `\\` → `\` leaves exactly one backslash
immediately before the newline, and a run of N (even) comes back as N/2, so only N = 2 yields one.
The rule therefore repairs a lone `\` at a line end when, and only when: (1) the innermost
enclosing environment is **row-based** (`is_row_based_environment`) — `equation`/`equation*` are
**out**, one formula, no rows, so `\\` is not a separator there; (2) it stands at **brace depth
zero** relative to that environment's body, a separator being a token of the body while a
backslash one group deep belongs to whatever opened the group; (3) it is a **lone** backslash at
the end of the line; and (4) **another row follows** before `\end{…}`, since on the last body line
a `\\` would add an empty row rather than separate two written ones.

Outside the proof, plainly: a producer *could* have written a control space at a row end and it
would survive the redraw unchanged. Conditions 1–4 make that reading a space at the right-hand
edge of a row rather than a mathematical statement. Trailing whitespace after the backslash is
tolerated because terminal rows are space-padded — the one place the string argument is weaker
than it reads. **H2** holds and is now pinned with its two negatives. **H3** is accepted in full:
`\,`, `\!`, `\[` and the deleted setext `=` have no unique inverse, and are pinned as left exactly
as they arrived.

## Strings awaiting Chinese, budgets, and what is unverified

`crates/bt-app/src/i18n.rs`, both marked CHINESE PENDING with English in both columns:
`Text::RowRepairRowBreaks` = "Repair row breaks" (Settings → Rendered blocks, third row) and
`Text::DescRepairRowBreaks` = "Restores a row break an agent's own redraw dropped from a matrix or
aligned block." A description must wrap to 2 lines (`SETTINGS_DESCRIPTION_MAX_LINES`); 3 fails the
copy gate, 6 is the hard cap. The English is 82 characters, between this page's 74 and 87; the
Chinese should stay near `DescFormulas`' 34.

`bt-detect` has no counted-budget test; `bt-term`'s five pin their measurements as constants, so
the pins are "before" and passing is "after" — all five pass. The repair adds one pass-local
`Vec<(usize, u32)>`, one entry per newline in a span of at most 8 KiB, and no extra pass.
`crates/bt-math/tests/matrix_rows.rs` records one defect found on the way and out of this ticket's
reach: a *single-row* `array` mis-sets, MiTeX mis-reading a column spec with no `\\` after it.
Arrays with separators — the state this repair restores — are correct.

`bt-app` is not compiled here, by the ticket's instruction: its Settings row, i18n entries and
wiring are mirrored from `display_formulas` and checked by eye against every match arm and every
fixed-size array (`Text::ALL` 685 → 687). CI sees them first, and two things may be red — the
Han-character gate, deliberately, until the copy lands, and `fits.max_scroll() == 0.0`, which now
measures a five-row Rendered blocks page where its comment said four.
