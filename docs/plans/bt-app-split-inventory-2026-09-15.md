# `bt-app` — the measured inventory, 2026-09-15

Taken against `main` at `76ca0788` in the worktree
`D:\Developer\bt-wt\bt-app-split-plan`. It is the evidence file for
`docs/plans/bt-app-split.md`; that document cites these tables and does not
restate the method.

**Revision 3**, after two adversarial reviews.

Round 1 — `docs/plans/review/bt-app-split-review-2026-09-15.md` — rebuilt the
dependency graph independently with tree-sitter and NetworkX, re-ran the
counting, and disagreed with this file in nine places. **Where the review's
count differs, the review's is used and the first draft's is kept beside it with
the reason** — a measurement that was wrong once is worth being able to
recognise again. The largest correction is §5: the graph this file first
published is not the graph a crate split has to obey.

Round 2 — `docs/plans/review/bt-app-split-review-2-2026-09-15.md` — re-ran both
retained scripts against revision 2 and reproduced their output, and corrected
this file in four more places, each marked below: the theme sort's two
percentages (§0.3), the churn number's provenance (§2.3), the classification of
the 91 source pins (§3.1), and one subtraction (§3.2). The same rule applies:
the corrected number is used and the wrong one is kept beside it.

Nothing here was built: no `cargo build`, no `cargo check`, no `cargo test`.

**How to read the labels.** A number marked *measured* came out of a tool run on
this tree and can be reproduced. A number marked *estimated* is a model with its
inputs named. A number marked *historical* was read off artifacts produced by an
earlier run and cannot be re-derived from this tree today. There is no fourth
kind.

---

## 0. Method, and what it got wrong

### 0.1 The tools

1. **The retained graph builder — `scripts/dev/bt-app-graph.py`.** This is the
   reviewer's reconstruction, moved into the tree so the plan's numbers can be
   re-run. Tree-sitter parses all 102 files with zero parse-error flags;
   comments and literals are blanked with line positions retained;
   `#[cfg(test)]` item bodies are excluded for the `*_prod` variants and other
   target `cfg` branches are unioned rather than evaluated; macro token trees
   are scanned for qualified paths; NetworkX computes the components. It emits
   six graph variants and writes `target/bt-app-graph.json`. Re-run in this
   worktree, it reproduces the review's `cuts` object byte for byte.
   It needs `tree_sitter`, `tree_sitter_rust` and `networkx`, which are **not**
   workspace dependencies and never become ones.
2. **The retained table generator — `scripts/dev/bt-app-split-table.py`.** Holds
   the plan's Step 3 row manifest and exits 1 if the manifest and the graph
   disagree.
3. **A brace-depth scanner over stripped Rust**, in the session scratchpad, for
   the item spans, the method census and the theme sort in §§3–4. It is not a
   Rust parser; its spans cover 152,758 of `main.rs`'s lines and every gap is a
   run of top-level `use`, `const`, `static` and doc comment.
4. **`cargo metadata --no-deps --format-version 1 --offline`**, `cargo tree
   --offline --locked`, `grep`, `wc`, `git log`, and `ls` on artifacts produced
   by earlier runs.

### 0.2 The two things the method got wrong, and why they are written down

**(a) Doc links are not code.** `crate::foo::Bar` appears in this tree both as
code and as an intra-doc link inside a `///` comment. Counting both produced a
component of 69 modules; counting only code produced 43. Every dependency number
here is code only. *(This was caught in the first pass and is recorded because a
reviewer re-running the grep without stripping will get the larger number.)*

**(b) A module-name graph is not a crate graph, and this is the correction that
changed the plan.** The first draft matched `crate::(\w+)` and treated every
module it did not reach as free. That misses three things the review names:
grouped imports (`preview_wrap.rs:3` pulls in root-owned `PreviewDocument`,
`MarkdownBlockLayout`, `WrapMeasure` *and* the `preview` and `seats` modules),
`super::` paths, and — the big one — **items owned by the crate root**.
`preview_viewport.rs:2` is `use crate::*;`. `marks.rs:1703` takes a root
`StatusDot`. **A new crate cannot name a type that lives in the binary it was cut
out of**, so a module that names one is not extractable however clean its
module-level edges look. §5 publishes both graphs and says which is which.

### 0.3 The theme sort, in full

Finding 4 asked for the classification rules to be retained. They are 28 ordered
regular expressions, matched anywhere in the snake_case method name, first match
wins, applied to the 1,310 `impl Runtime` method names:

```
settings panel            (settings|advanced_row|editor_choice|scheme|colour|palette_colour)
profiles                  (profile|root_menu)
first run / integration   (first_run|psreadline|invite|powershell_integration|
                           context_menu_install|explorer_package|claude_hook|
                           codex_notify|copilot)
git panel & graph         (git|graph|checkout|repo|commit_message|branch)
math & formula            (math|formula|equation|tex)
preview & documents       (preview|markdown|document|pdf|image|picture|video|
                           animation|hex|table_block|linebreak|wrap)
files column              (files|file_row|file_index|folder|directory|dir_news|
                           locate|recent_folder)
file peek                 (peek)
focus cards               (focus|card|thumbnail|thumb)
web seats                 (web|favicon|browser|address_bar)
tabs & rename             (tab|rename|blank_page|strip)
panes & layout            (pane|split|divider|seat|leaf|rail|layout|dock|
                           geometry|reflow|refit)
quake / summon            (quake|summon)
search                    (search)
command palette           (palette)
attention & notifications (attention|notif|notice|agent|taskbar|toast)
windows & session         (window|session|restore|quit|reopen|monitor|work_area|
                           dirty_gate|handover|activate_window)
dpi & resize              (dpi|resize|rescale|scale|size|lawful|settle_size)
input: keyboard & IME     (keyboard|key_|_key|keys|ime|preedit|chord|shortcut|
                           modifier|text_input|caret|blink)
input: mouse & drag       (mouse|pointer|wheel|click|hover|press|drag|drop|tear|
                           foreign|cursor)
clipboard & paste         (clipboard|paste|copy|cut_)
floats & menus            (float|popup|popover|menu|overlay|chevron|context)
tooltips & hints          (tooltip|hint)
frame & present           (publish|redraw|present|chrome|draw|paint|compose|
                           frame|^turn$|dump|refresh_overlay|ink$)
terminal & PTY            (pty|terminal|term_|shell|selection|hyperlink|mark|
                           cmdrail|command_rail|scroll|screen|host)
diagnostics & trace       (trace|diagnostic|hang|perf|log_)
launch & CLI              (launch|cli|seed|arrival|startup|create)
i18n                      (i18n|lang|translat|localis|localiz)
```

**It is a sort, not an audit.** It places **1,211 of the 1,310 methods** and
leaves **99** unclassified rather than forcing them. *Two shares, because they
are two quantities and revision 2 printed one of them as the other* (round 2,
R2-12):

| Share | Value |
| --- | ---: |
| methods classified | 1,211 / 1,310 = **92.44%** |
| methods **un**classified | 99 / 1,310 = **7.56%** |
| body lines unclassified | 2,086 / 48,879 = **4.27%** |

Revision 2 said "95.7% placed, 4.3% unclassified", which mixed the line share
into the method sentence; 4.3% was never the method figure. It will also misfile
some — `apply_row_verb` sits in `unclassified` and is really a settings method.
The theme table is a map of roughly where the mass is, accurate to a few percent
per row, and **is not a work order for any individual function.** No method
share, however high, is a substitute for the item-level relocation manifest
Step 2 carries.

---

## 1. The workspace, and where the mass is

*Measured.* 16 workspace members (15 first-party plus the vendored
`alacritty_terminal`).

| Denominator | Lines | `bt-app`'s share |
| --- | ---: | ---: |
| `crates/` — first-party only | 678,745 | **67.3%** |
| `crates/` + `vendor/` | 695,785 | 65.6% |

The plan quotes **67.3%**. The first draft quoted 65.6% and did not say its
denominator included somebody else's terminal emulator.

| Crate | Lines | First-party deps |
| --- | ---: | ---: |
| `bt-app` | 456,556 | 12 |
| `bt-platform` | 65,700 | 1 |
| `bt-term` | 47,155 | 6 |
| `bt-render` | 39,809 | 4 |
| `bt-viewport` | 14,219 | 3 |
| `bt-persist` | 12,677 | 0 |
| `bt-transcript` | 10,744 | 1 |
| `bt-pty` | 9,693 | 2 |
| `bt-detect` | 8,690 | 3 |
| `bt-layout` | 5,296 | 0 |
| `bt-math` | 3,108 | 1 |
| `bt-corpus` | 2,719 | 4 |
| `bt-winres` | 1,266 | 0 |
| `bt-doc` | 1,002 | 1 |
| `bt-unicode` | 111 | 0 |

**The seams the workspace already has are real and they are acyclic.** The
first-party graph from `cargo metadata` is a DAG: `bt-unicode`, `bt-layout`,
`bt-persist` and `bt-winres` depend on nothing of ours; `bt-transcript` on
`bt-unicode`; `bt-doc` on `bt-transcript`; `bt-render` on four; `bt-term` on
six; `bt-app` on twelve. `bt-app` is the only member nothing depends on, which
is what makes it the place everything accretes.

*Measured.* `#[test]` attributes: the reviewer's AST count over
`crates/bt-app/src` is **3,809**; a `grep -c` over the same files gives 3,748;
the first draft said 3,749. The AST number is the right one. **None of the three
is a libtest census** — a `#[test]` behind a platform `cfg` is counted and may
not run — so none of them should be used as "the number of tests that execute".
`main.rs` raw count: **1,043**, which all three agree on.

Per-crate `grep` counts, for scale only: `bt-term` 524, `bt-render` 298,
`bt-platform` 261, `bt-persist` 184, `bt-transcript` 177, `bt-detect` 138,
`bt-viewport` 130, `bt-pty` 82, `bt-layout` 51, `bt-math` 44, `bt-corpus` 16,
`bt-winres` 11, `bt-doc` 7, `bt-unicode` 3. `bt-app` holds roughly two thirds of
the workspace's tests by any of these counts.

---

## 2. `bt-app` as a compilation unit

*Measured, from `cargo metadata`.* `bt-app` declares **no `[lib]` target**:

| Kind | Name | Path |
| --- | --- | --- |
| bin | `folio` | `src/main.rs` |
| example | `container-probe` | `examples/container-probe.rs` |
| example | `gif-fixture` | `examples/gif-fixture.rs` |
| example | `video-probe` | `examples/video-probe.rs` |
| test | `macos_glyph_surface` | `tests/macos_glyph_surface.rs` (`harness = false`) |
| custom-build | `build-script-build` | `build.rs` |

Three consequences, all constraining the plan:

- **No other crate can depend on `bt-app`.** There is no library target to link
  against.
- **An integration test cannot reach `bt-app`'s items.** *(Corrected: the first
  draft said "`tests/` is not available". `crates/bt-app/tests/` exists and holds
  the `harness = false` macOS target and a `fixtures/` directory. The constraint
  is the missing library, not the missing directory.)* So every one of the
  crate's `#[cfg(test)]` modules compiles into **one** harness.
- **`cargo test` codegens `bt-app` twice** — the product bin, and the harness
  with `cfg(test)` on.

*Measured, with the review's line-number correction.* **`[profile.test]
opt-level = 1` is at `Cargo.toml:193–194`**, and **there is no `[profile.dev]`
in this workspace** — the only `^[profile` lines in the manifest are 193
(`[profile.test]`) and 199 (`[profile.release]`). So dev is the built-in default
at `opt-level = 0`, and `cargo build` and `cargo test` compile the entire graph
— dependencies included — at two different optimisation levels, sharing no
artifacts. `.cargo/config.toml` on this machine sets `jobs = 19` and carries
`target-feature=+crt-static` at line 39.

### 2.1 Source size

*Measured.* `crates/bt-app/src`: **102 files, 455,336 physical lines**. (The
first draft said 455,438; it counted newline + 1 per file, which is what `wc -l`
reports for a file with no trailing newline. The graph builder keeps the +1
convention, so its totals run one higher per node; where that matters it is said
out loud rather than adjusted away.)

*Measured.* **`#[cfg(test)]` bodies are 197,346 lines — 43.3% of the crate** by
the review's count, which inherits `cfg` from external module declarations and
counts overlapping item spans once. `main.rs` alone: **59,903**. The first
draft's 197,157 / 60,059 came from a different span rule and is superseded.

*Measured.* After blanking comments and literals, **263,947 lines** are
non-blank. **The first draft called the complement "42% prose" and divided
estimates by 1.7×. Both are withdrawn**: the complement is blank lines and
string literals as well as comments, and no estimate in the plan uses that
factor any more.

### 2.2 The files

*Measured.* The largest files, with the `#[cfg(test)]` split and the number of
times the file's production code names `Runtime` or `WindowRuntime`:

| File | Lines | Production | Test | `Runtime` | `WindowRuntime` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `main.rs` | 165,815 | 105,912 | 59,903 | — | — |
| `seats.rs` | 51,302 | 21,857 | 29,445 | 18 | 2 |
| `settings.rs` | 28,232 | 14,016 | 14,216 | 0 | 0 |
| `profiles.rs` | 24,279 | 13,144 | 11,135 | 0 | 0 |
| `preview.rs` | 13,882 | 7,780 | 6,102 | 7 | 0 |
| `i18n.rs` | 9,434 | 7,156 | 2,278 | 0 | 0 |
| `git_graph.rs` | 8,417 | 5,063 | 3,354 | 0 | 0 |
| `git_panel.rs` | 8,030 | 4,382 | 3,648 | 3 | 0 |
| `git.rs` | 7,087 | 3,951 | 3,136 | 0 | 0 |
| `marks.rs` | 7,002 | 4,133 | 2,869 | 0 | 0 |
| `shortcuts.rs` | 6,691 | 3,185 | 3,506 | 1 | 0 |
| `webhost.rs` | 6,593 | 3,859 | 2,734 | 5 | 0 |
| `float.rs` | 5,084 | 2,392 | 2,692 | 0 | 0 |
| `focus_thumb.rs` | 4,121 | 1,682 | 2,439 | 5 | 4 |
| `attention/*` † | 4,061 | 1,753 | 2,308 | 0 | 0 |
| `restore.rs` | 3,884 | 2,474 | 1,410 | 0 | 0 |
| `file_peek.rs` | 3,883 | 1,674 | 2,209 | 0 | 0 |
| `first_run.rs` | 3,793 | 1,967 | 1,826 | 0 | 0 |
| `cmdrail.rs` | 3,687 | 1,896 | 1,791 | 0 | 0 |
| `shell_integration.rs` | 3,479 | 1,648 | 1,831 | 0 | 0 |

† `attention/tests.rs` is an external module declared `#[cfg(test)] mod tests;`.
The graph builder's cfg span covers the declaration, not the declared file, so
its own `test` figure for this node is 1; the 2,308 here is the brace scanner's,
which reads the file. Every other row is the graph builder's.

**Only seven modules outside `main.rs` name `Runtime` or `WindowRuntime` in
production code**: `seats` (18/2), `preview` (7), `focus_thumb` (5/4),
`webhost` (5), `git_panel` (3), `shortcuts` (1), `preview_viewport` (1).

That last one matters more than its count suggests: **`preview_viewport.rs:994`
is an inherent `impl Runtime`**, and an inherent impl may only be written in the
crate that defines the type ([E0116]). It is a hard block on extracting that
module, not a stylistic one.

[E0116]: https://doc.rust-lang.org/stable/error_codes/E0116.html

### 2.3 Growth and churn

*Measured, from `git show <rev>:crates/bt-app/src/main.rs | wc -l`:*

| Point | `main.rs` lines |
| --- | ---: |
| `v0.1.0-preview` (2026-08-31) | 131,051 |
| `v0.2.5-preview` | 142,573 |
| `9acd482` (`v0.3.0-preview`, 2026-09-12) | 156,381 |
| `76ca0788` (HEAD, 2026-09-15) | 165,815 |

**+34,764 lines in fifteen days.** The in-tree handoff still describes `main.rs`
as 79,000 lines.

**The churn number, corrected twice.** Revision 2 published 112 of 150 and said
the first draft's "98 of 150" reproduces under no reading of `git log`. **That
second half is wrong** (round 2, R2-12): 98 is what you get by enumerating the
hashes under **default** history simplification and then running `git show
--name-only` on each. A churn number needs three things stated — the revision
range, the traversal, **and the file-display procedure** — because those three
choices span 45 to 119 over one window. All five readings, each over the 150
most recent commits touching `crates/bt-app` ending at `76ca0788`, counting
commits whose file list contains `crates/bt-app/src/main.rs`:

| Reading | touches `main.rs` |
| --- | ---: |
| inline `git log -150 --name-only` (default simplification; merges list no files) | 56 / 150 |
| inline `git log -150 --full-history --name-only` | 45 / 150 |
| inline `git log -150 --first-parent --name-only` | 119 / 150 |
| enumerate `git log -150 --format=%H` (default), then `git show --name-only` on each | **98 / 150** — the first draft's number, reproduced |
| **enumerate `git log -150 --no-merges --format=%H`, then `git show --name-only` on each** | **112 / 150 (75%)** |

**The plan quotes 112 of 150 `--no-merges`** — one developer commit, one answer.
The exact command:

```bash
git log -150 --no-merges --format=%H -- crates/bt-app > /tmp/commits
while read h; do git show --name-only --format="" $h; done < /tmp/commits \
  | grep -c '^crates/bt-app/src/main.rs$'
```

The same pass over the same 150 commits: `seats.rs` **29**, `i18n.rs` **24**,
`preview.rs` **17**, `settings.rs` **12**.

**What the number is and is not.** It is a historical frequency over merged
work, in that window, under that traversal and that file-display procedure. It
says nothing about lines, nothing about merges, and nothing about commits
outside the window. It is **not** a probability for the branches that happen to
be open on the day Step 2 is dispatched; that has to be looked at rather than
inferred, which is what the plan's §6.5 does.

---

## 3. Inside `main.rs`

### 3.1 The modules and the pins

*Measured.* `main.rs` declares **147 modules**: **97 file modules** and **50
inline `mod … { … }` blocks**, every one of the 50 carrying `#[cfg(test)]`.
There is no inline production module.

*Measured.* The 50 inline test modules total 59,731 lines by the scanner's span
rule. The largest is a single `mod tests` at L116318–162953 — **46,636 lines
holding 770 of the file's 1,043 `#[test]` functions**. The next largest are
`floated_page_tests` (1,503), `cross_window_drag_tests` (1,014),
`tab_identity_tests` (966), `files_locate_door_tests` (914),
`pty_drain_budget_tests` (681) and `palette_wiring_tests` (601).

**The 91 source-reading pins, corrected.** `include_str!("main.rs")` appears
**91 times as a macro invocation — 53 inside functions and 38 at module level
inside a test module. They are not 91 independent tests**; a shared `SOURCE` can
serve several. 87 bind it to a `const SOURCE`, two to `MAIN`, two to `source`.

**None of the 91 uses `source_pin::source_region`.** The first draft said that
helper was already the mechanism; `source_region(` appears zero times in
`main.rs`, and its three users are `first_run.rs`, `profiles.rs` and
`shell_integration.rs`. The 91 slice the string with hand-rolled
`body(signature)` helpers that `panic!("{signature} is declared in this file")`
when the header is gone — **loud**, which is why re-pointing them is safe.

**The classification of the 91, corrected** (round 2, R2-2). Revision 2
published the table below as a partition, with three "genuinely silent"
whole-crate negatives. **Two of its three cells are wrong**, and the corrected
counts are given beside them:

| Class | Revision 2 | Corrected, at this snapshot |
| --- | ---: | --- |
| `body(signature)` slicing helpers | ~81 | unchanged in kind: they panic loudly with `"{signature} is declared in this file"` when the header is gone |
| `SOURCE.matches(..).count() == N`, `N > 0` | 7 | **10** — `main.rs:14201`, `:14217`, `:103965`, `:104432`, `:104975`, `:105163`, `:105784`, `:131933`, `:151992`, `:152393`, with `N` of 1, 2 or 6. All fail loudly. |
| whole-source negative, passes vacuously on a fragment | 3 — `:15772`, `:103570`, `:105705` | **3, but not those three.** `:15772` and `:105705` are `!SOURCE.contains(…)` over the whole file, and `:113789` is `!MAIN.contains(…)` over `const MAIN` at `:113779` — **which neither review's partition contains.** |
| **scoped** negative, misfiled as whole-source | — | **`:103570`** is `!source.contains(fetch)` where `source = body("    fn mini_source")` at `:103555`, beside a positive assertion at `:103557`. It forbids five worker names **inside one projection method**, where they are legitimate elsewhere in the crate. |

**So there is no published partition of the 91 that survives inspection**, and
this file does not supply a new one: the counts above are a re-count at one
snapshot, not an audit by subject. The plan's §6.2(b) says what the audit is and
requires it to happen during implementation, with every converted guard's scope
recorded and mutation-checked.

Note also that three of the four negatives assemble their needles rather than
writing them — `:15767` and `:113781` with `concat!`, `:105701` with a built
escape — precisely because a file that reads itself matches its own text. Any
assertion converted to a whole-crate scan has to keep that discipline.

A multi-file pin already exists in the tree — `main.rs:165696` reads both
`main.rs` and `preview_edit.rs` — so the `const SOURCES: &[&str]` form has
precedent. Nine other modules pin themselves with `include_str!` of their *own*
file, which is the pattern that survives a move. And a single `SOURCE` can serve
several subjects bound for different files, which is why "one include
replacement per invocation" is not a general rule.

### 3.2 The top-level items

*Measured.* 1,006 top-level brace items:

| Kind | Count | Lines |
| --- | ---: | ---: |
| `impl` blocks | 130 | 73,691 |
| inline `mod` (all `#[cfg(test)]`) | 50 | 59,731 |
| free `fn` | 305 | 3,929 |
| `struct` | 199 | 6,139 |
| `enum` | 105 | 2,130 |
| other (`const`, `trait`, `static`, `extern`, blanket impls) | 217 | 7,138 |

The two largest are both `impl Runtime<'_>`: **L36598–61746 (25,149)** and
**L62188–100525 (38,338)** — **63,487 lines, 38% of the file, in two blocks**.
Next: `impl FolioApp` (2,951) and `impl ApplicationHandler<AppEvent> for
FolioApp` (743).

**165,815 − 63,487 = 102,328.** That is what stays in `main.rs` after the two
impl blocks move and before any module or import lines are added. Revision 2
printed 102,329, keeping a newline-plus-one minuend from an earlier count (round
2, R2-12); the file is 165,815 lines by `wc -l`. The plan's first draft said
"roughly 40,000", which is a possible post-test-move scale and was never a
post-2a one — and it is the number Step 2a's residue is measured against,
because the plan takes 2a to be **the two impl blocks and nothing else**.

The largest free functions: `new_window_runtime` (331), `main` (269),
`drain_leaf_pty` (177).

### 3.3 `Runtime` is two fields, and that is not the same as narrow

*Measured.* `Runtime<'a>` (L13938):

```rust
struct Runtime<'a> {
    app: &'a mut App,
    window: &'a mut WindowRuntime,
}
```

pinned by a `const _` destructure and a source-reading `layer_shape_tests`.

**But it also implements `Deref<Target = TabState>` (`main.rs:16477`) and
`DerefMut` (`:16485`)**, so every method reaches the active tab implicitly, with
no `self.window.` prefix for any census to see. And `answer_attention`
(`:92786`) destructures `WindowRuntime` and mutates `tabs[index]` directly.

So the god objects are the two it borrows —

- **`App` (L11015): 78 fields.** The `wgpu` device, four background workers and
  their running/notice flags, the persistence stores, the quit transaction, the
  window list, the drag broker, the quake window.
- **`WindowRuntime` (L11826): 245 fields.**

— and **no field-name census can establish ownership.** §4 is a lower bound.

*Measured.* **1,310 methods** hang off the two `impl Runtime` blocks, 48,879
lines inside method bodies. `impl WindowRuntime` is 38 lines; `WindowRuntime`
has essentially no methods of its own.

The twenty largest methods:

| Lines | At | Method | | Lines | At | Method |
| ---: | --- | --- | --- | ---: | --- | --- |
| 1,099 | L90777 | `mouse_input` | | 446 | L40312 | `rebuild_tooltip_anchors` |
| 1,019 | L39271 | `refresh_chrome` | | 446 | L99848 | `turn` |
| 1,012 | L89735 | `chrome_mouse_input` | | 443 | L44501 | `refresh_overlay` |
| 864 | L36610 | `create` | | 402 | L63998 | `refit_preview_picture` |
| 864 | L95303 | `keyboard_input` | | 390 | L62319 | `build_preview_body_in` |
| 682 | L85623 | `pointer_moved` | | 373 | L69574 | `file_peek_card_layers` |
| 535 | L94610 | `mouse_wheel` | | 364 | L79216 | `preview_float_layer` |
| 467 | L60924 | `rebuild_preview_document` | | 346 | L82397 | `advance_strip_animation` |
| 454 | L37507 | `open_window` | | 333 | L81271 | `apply_math_results` |
| | | | | 292 | L64885 | `apply_git_results` |

### 3.4 The themes

*Measured for the counts; the sort is §0.3's regex list.*

| Theme | Methods | Lines | Share of the 48,879 body lines |
| --- | ---: | ---: | ---: |
| preview & documents | 270 | 9,033 | 18.5% |
| input: mouse & drag | 59 | 5,064 | 10.4% |
| git panel & graph | 101 | 3,638 | 7.4% |
| panes & layout | 124 | 3,416 | 7.0% |
| tabs & rename | 71 | 2,630 | 5.4% |
| frame & present | 19 | 2,311 | 4.7% |
| floats & menus | 57 | 2,159 | 4.4% |
| *unclassified* | 99 | 2,086 | 4.3% |
| file peek | 56 | 2,023 | 4.1% |
| files column | 57 | 1,845 | 3.8% |
| settings panel | 31 | 1,664 | 3.4% |
| input: keyboard & IME | 31 | 1,656 | 3.4% |
| windows & session | 34 | 1,474 | 3.0% |
| web seats | 34 | 1,301 | 2.7% |
| focus cards | 49 | 1,246 | 2.5% |
| math & formula | 32 | 1,152 | 2.4% |
| terminal & PTY | 33 | 1,006 | 2.1% |
| launch & CLI | 4 | 900 | 1.8% |
| attention & notifications | 26 | 681 | 1.4% |
| first run / integration | 24 | 665 | 1.4% |
| search | 22 | 612 | 1.3% |
| tooltips & hints | 9 | 573 | 1.2% |
| DPI & resize | 11 | 523 | 1.1% |
| profiles | 21 | 457 | 0.9% |
| command palette | 15 | 454 | 0.9% |
| quake / summon | 8 | 123 | 0.3% |
| clipboard & paste | 7 | 89 | 0.2% |
| diagnostics & trace | 4 | 65 | 0.1% |
| i18n | 2 | 33 | 0.1% |
| **Total** | **1,310** | **48,879** | |

**Two rows are mostly single handlers.** `mouse_input` + `chrome_mouse_input` +
`pointer_moved` + `mouse_wheel` are 3,328 of the mouse theme's 5,064;
`refresh_chrome` alone is 1,019 of `frame & present`'s 2,311.

---

## 4. The state census — a lexical lower bound

**What this section is.** For every one of the 1,310 methods, which
`self.window.<field>` and `self.app.<field>` names appear in its body,
cross-tabulated against the theme sort.

**What it is not.** It cannot see `Deref`, it cannot see destructuring, it does
not distinguish reads from writes, and it says nothing about what a callee does
with the `&mut Runtime` it was handed. **Every conclusion the first draft drew
from it about ownership or one-way dependency is withdrawn** (review finding 5).
The numbers stay because concentration is still worth knowing.

*Measured.* 239 of `WindowRuntime`'s 245 fields are reached by contiguous
direct access; **242** allowing whitespace between the dot and the name. All
**78** of `App`'s are reached.

Distribution of `WindowRuntime` fields by how many themes touch them
(contiguous match):

| Themes touching the field | Fields |
| ---: | ---: |
| 1 | 86 |
| 2 | 68 |
| 3 | 25 |
| 4 | 22 |
| 5–9 | 27 |
| 10–13 | 7 |
| 21–25 | 4 |

The hubs, worst first:

| Field | Methods | Themes |
| --- | ---: | ---: |
| `window.renderer` | 220 | 25 |
| `window.tabs` | 233 (**255** whitespace-aware) | 23 |
| `window.active_tab` | 154 | 21 |
| `window.window` | 79 | 21 |
| `window.pointer_position` | 38 | 13 |
| `window.search` | 31 | 12 |
| `window.float` | 51 | 11 |
| `window.settings` | 29 | 11 |
| `window.seat_pointer` | 21 | 11 |
| `window.web` | 33 | 10 |
| `window.modifiers` | 22 | 10 |
| `window.rename` | 27 | 9 |
| `window.rail` | 19 | 9 |
| `app.settings_store` | 84 | 20 |
| `app.motion` | 90 | 18 |
| `app.gpu` | 64 | 21 |
| `app.profile_programs` | 24 | 11 |
| `app.shortcuts` | 12 | 8 |

Three fields — **`tabs`, `active_tab`, `window`** — are the document model, and
they are what a subsystem boundary runs into. That is the one conclusion §3.2 of
the plan still draws from this table, and it is drawn from concentration rather
than from ownership.

Per-theme single-theme field counts and hub counts are in the first revision's
history; they are not reproduced here, because with `Deref` and destructuring
unaccounted for they invited exactly the conclusion the review struck.

---

## 5. The module graph — two graphs, and only one of them is the crate graph

*Measured by `scripts/dev/bt-app-graph.py`, over `crates/bt-app/src` with
`main.rs` excluded as a node and each directory-module folded into one.*

Six variants are emitted. Two matter here:

| Variant | What it counts | What it is good for |
| --- | --- | --- |
| **`regex_full`** | `crate::(\w+)` in production code, module nodes only | reproducing the first draft; **it overstates what can leave** |
| **`root_prod`** | the same, plus an `@root` node for items owned by `main.rs`, with imports and `super::` paths resolved | **the graph a crate split has to obey** |

### 5.1 The component

| Variant | Nodes | Edges | Largest component |
| --- | ---: | ---: | --- |
| `regex_full` | 99 | 242 | **43 modules / 232,495 lines** |
| `root_prod` | 96 | 404 | **80 nodes / 442,903 lines** |

The 43-module component under `regex_full`:

```
attention, attention_codex, attention_copilot, attention_hooks, attention_map,
attention_wire, attention_words, cli, cmdrail, explorer_menu, files, first_run,
focus_thumb, git, git_graph, git_panel, hang_watch, i18n, icons, marks, notice,
persist, preview, preview_edit, preview_provenance, preview_select,
preview_text, preview_undo, profiles, psreadline, quit, recent_folders, restore,
schemes, search, seats, seed, settings, shell_integration, shortcuts, tooltip,
update, webhost
```

Under `root_prod` that component absorbs `@root` and grows to 80 of the 96
nodes. **Cargo crates cannot be mutually dependent, so extracting any member of
a component means extracting all of it**; and under the graph that counts root
ownership, "all of it" is nearly the whole crate.

### 5.2 The cut table, both graphs

*Measured.* "Free" means: this module's transitive closure never enters the
largest component, so it could leave.

| Cut | `regex_full` free | `root_prod` free | `root_prod` largest |
| --- | --- | --- | --- |
| baseline | 33 / 24,572 | **16 / 11,761** | 80 / 442,903 |
| after the `i18n` cut | 51 / 66,273 | **22 / 31,358** | 74 / 423,306 |

**The multiplier the `i18n` cut buys survives the correction almost exactly —
2.67× against 2.70×. The absolute mass does not: 31,358 / 456,556 is 6.9% of the
crate, not 14.5%.** The plan is written against the `root_prod` column.

The 22 modules that need no root item moved: `animation`, `app_delegate_wire`,
`attention_trace`, `context_menu`, `favicon`, `glyph_trace`, `hex_peek`, `i18n`,
`input`, `linebreak`, `pdf`, `preview_trace`, `quake`, `recent_folders`, `seed`,
`shortcuts`, `text_field`, `trace`, `watch_clock`, `web_trace`, `webnav`, `wsl`.

`scripts/dev/bt-app-split-table.py` turns that set into the plan's §7.2 rows and
fails if the two disagree.

### 5.3 `i18n`'s three edges

*Measured, and independently confirmed by every variant of the reviewer's
reconstruction: `i18n`'s successors are exactly `['marks', 'preview',
'profiles']`, and `i18n` has no `@root` edge, so no root-owned item hides a
fourth.*

| At | Item | Caller today |
| --- | --- | --- |
| `i18n.rs:6359` | `colour_name(colour: crate::marks::MarkColour) -> Text` — **eight** arms, not nine | **`settings.rs:4083`** |
| `i18n.rs:6378` | `profile_entry_fault(&crate::profiles::ProfileFault) -> String` | **`main.rs:36750`, `main.rs:51453`**, and a `profiles` test at `:23009` |
| `i18n.rs:6554` | one call to `crate::preview::format_pixel_size(…)`, inside `picture_shown_at` | — |

**So the cut is five files — `i18n`, `settings`, `profiles`, `preview`,
`main` — not four excluding `main.rs`**, which is what the first draft said.

And the cut makes `i18n` an **intra-app sink**, not a dependency-free library:
it still formats `bt_term::InlineImageDecodeError` (`:6579`) and
`BackgroundImageError` (`:6695`).

One guard in that file is already ineffective and is recorded so nobody cites
it: `no_profile_title_has_been_pulled_into_the_language_table` (`i18n.rs:8848`)
stops at the first `#[cfg(test)]`, near the top of the file, and never reaches
the table it is named for.

### 5.4 Test-only files are not modules that can leave

*Review finding 3.* Four files under `crates/bt-app/src` are entirely
`#[cfg(test)]`, and the first draft counted three of them as independent
production surface:

| File | Lines | How it is attached |
| --- | ---: | --- |
| `preview_viewport_tests.rs` | 774 | imports its parent's private scope |
| `preview_typing.rs` | 640 | `cfg`-gated at `main.rs:97` |
| `focus_thumb_restore_tests.rs` | 367 | attached at `focus_thumb.rs:4120`; begins `use super::*;` |
| `source_pin.rs` | 132 | `cfg`-gated at `main.rs:121` |

They have **zero** production lines and belong to the module that declares them.

---

## 6. The gate, and what it is waiting for

### 6.1 The three gates and the scripts

*Measured, from `CONTRIBUTING.md:11–19`:*

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

plus six scripts. Two facts about that list matter to the plan:

- `cargo clippy` runs under `RUSTC_WORKSPACE_WRAPPER`, so it has a **different
  fingerprint** from `cargo test`'s artifacts and reuses none of them;
  `--all-targets` makes it six targets rather than two.
- **`cargo clippy` is a check-mode command — it emits metadata and never
  links.** So every codegen or link setting reaches only the first of the three
  gates, and only its codegen-and-link half.

### 6.2 The third compile is a feature-set difference

*Measured by the review with `cargo tree`, no build.* `check-shortcuts-table.ps1:31`
and `generate-shortcuts-table.ps1:32` run `cargo test --package bt-app --bin
folio --locked`; the gate runs `cargo test --workspace --locked`. The two
resolve different features:

```
--workspace : vte v0.15.0 | ansi,bitflags,cursor-icon,default,log,serde,std
              bit-set v0.8.0 | default,std
-p bt-app   : vte v0.15.0 | ansi,bitflags,cursor-icon,      log,serde,std
              bit-set v0.8.0 |         std
```

`default` arrives because **`bt-corpus`** — a workspace member selected by
`--workspace` and not by `-p bt-app` — takes `vte.workspace = true` without
`default-features = false` (`crates/bt-corpus/Cargo.toml:18`, `Cargo.toml:189`).
Features reach rustc as `--cfg feature="…"` and are part of the unit
fingerprint, so `vte` is a different unit under the two commands. `vte` is under
the vendored `alacritty_terminal`, under `bt-term`, under `bt-app` — **so the
whole stack above it rebuilds, harness and link included, every time the
shortcuts gate runs.** It buys nothing: `vte`'s `default` is `["std"]` and `std`
is already active in both resolutions.

**The first draft guessed this was a sequencing accident and proposed a
`Measure-Command` to confirm it. That measurement would have shown the symptom
and confirmed the wrong cause.**

Neither script runs in CI — no `.github/workflows` file names either — so this
cost is entirely the owner's local gate.

### 6.3 Artifacts — historical, not reproducible here

*Historical.* Read by `ls` on `D:\Developer\BetterTerminal\target\debug`, from
the owner's own earlier builds:

| Artifact | Size |
| --- | ---: |
| `folio.exe` (product bin, dev profile) | 85.8 MiB |
| `deps\folio-<hash>.exe` (unit-test harness, most recent) | 209.8 MiB |
| `deps\folio-<hash>.pdb` | 654.6 MiB |
| largest harness in the directory (2026-09-14) | 431.4 MiB |
| `target\debug` total | 74.5 GiB |

**The review could not verify these**: they are outside the authorised worktree,
and this worktree's own `target` has no such artifacts. They are evidence of
size at a past moment, never of link time, and the plan labels them so.

**What they are and are not evidence for.** The harness is 2.44× the bin, and
this repository has already measured, in its own manifest, that on MSVC the
symbols are in the PDB and not the image (`Cargo.toml:212`, `:236`, `:239`). So:

- the **654.6 MiB PDB** is debug information, and `debug = "line-tables-only"`
  is aimed at it;
- the **209.8 MiB image** is code, of which the ~124 MiB over the bin is the
  crate's own test code plus `libtest`;
- `bt-render` and `winit` monomorphisation is in **both** images, so it is not
  what makes the harness the larger one.

**They are two separate wins on two separate quantities and must not be added
together**, which the first draft did.

### 6.4 What no existing test can see

*Review finding 20.* `panic_report` (`main.rs:115969`) formats
`"panic: {panic_text}\nbacktrace:\n{backtrace}"` from a production hook that
calls `Backtrace::force_capture` (`:115798`). The only production-shaped panic
test in the crate asserts
`read_to_string(log).unwrap().contains("unwrap")` (`:116432`) — satisfied by the
`panic:` line alone, with the backtrace section empty, unresolved or absent.
`hang_watch`'s report tests use synthetic frames (`hang_watch.rs:2403`), not a
real PDB.

**So no change to debug information can make any existing test go red.** A green
run of that test is evidence about containment and nothing else.

What Step 0 *can* break there is a clock: a 60-second process watchdog
(`:116340`), a 30-second font warm-up (`:116384`), and 1/5/10-second request
budgets.

---

## 7. Everything coupled to `main.rs` as a file

*Measured.*

| Coupling | Count | What it does on a split |
| --- | ---: | --- |
| `include_str!("main.rs")` macro invocations in its own tests | 91 (53 in fns, 38 at module level) | ~88 fail loudly on a fragment; **3 pass vacuously** (§3.1) |
| `FILES_THAT_MAY_NAME_A_PLATFORM` at `main.rs:163012` | 1 array of 11 | **must be edited** — see below |
| `the_shell_page_is_gone` at `main.rs:161410` | 1 | scans `src` with **non-recursive** `read_dir`; a `runtime/` subdirectory silently leaves its scope |
| `scripts/check-portable-core.ps1` | 1 | parses the array out of `main.rs` by regex (`:227`), requires ≥5 entries (`:240`), walks `src` **recursively** (`:248`) |
| references to `main.rs` in `docs/DESIGN.md` | 151 | path references in prose, not line numbers |
| references to `main.rs` across `docs/` | 209 | as above |
| `[[bin]] path = "src/main.rs"` | 1 | stays correct while `main.rs` is the crate root |

**The platform array is the hard one, and the first draft got it wrong.** It
said the array stays in `main.rs` and the script is untouched, "the cheapest
possible answer". The array lives inside `#[cfg(test)] mod platform_gate_tests`
(`:163007`); one of its eleven entries is `"main.rs"` itself, admitted for the
startup path and the five platform calls M1-1 made non-fatal — code a theme
split moves into `runtime/launch.rs`. Its test,
`only_the_named_files_decide_what_platform_this_is` (`:163103`), walks
`sources()` (`:163045`) recursively with `\`→`/` normalised relative names, so a
moved file arrives as `runtime/launch.rs`. **And it asserts in both directions**
— `strangers` empty (a new file naming a platform fails) *and* `silent` empty (a
listed name that has stopped naming one fails). Moving any platform `cfg` out of
`main.rs` trips both at once, and the PowerShell twin fails the same way.

Note the disagreement sitting in one file: one whole-program guard walks `src`
recursively and the other does not.

---

## 8. Reproducing this

**In the tree**, and this is the review's finding 2 answered:

```bash
pip install tree_sitter tree_sitter_rust networkx   # documentation tools only
python scripts/dev/bt-app-graph.py > target/bt-app-graph-output.txt
python scripts/dev/bt-app-split-table.py
```

The first writes `target/bt-app-graph.json` with all six graph variants; the
second turns the plan's row manifest into §7.2's and §7.3's tables and **exits 1
if the manifest and the graph disagree**.

**Not in the tree**: the brace-depth scanners behind §§3–4. They are throwaway
Python over the stripped source, and the only part with subtlety is the `strip`
routine — comments, strings, raw strings with any number of hashes, char
literals, preserving line numbers. Any equivalent reproduces those tables; the
theme sort needs §0.3's regex list, which is why it is printed in full.

`cargo` invocations: `cargo metadata --no-deps --format-version 1 --offline`,
and for §6.2 `cargo tree --offline --locked --target x86_64-pc-windows-msvc -e
normal,build,dev -f "{p}|{f}" --prefix none`, once with `--workspace` and once
with `-p bt-app`. `git` invocations are in §2.3.
