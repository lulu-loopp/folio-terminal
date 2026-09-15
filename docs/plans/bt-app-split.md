# Splitting `bt-app` — the plan

2026-09-15. Written against `main` at `76ca0788`, in the worktree
`D:\Developer\bt-wt\bt-app-split-plan`. Every number in it comes from
`docs/plans/bt-app-split-inventory-2026-09-15.md`, which was taken on the same
tree and which this document does not restate; where a section leans on a
measurement, it names the inventory section it came from.

No code was moved to write this, and none is moved by it. Nothing here is
dispatched until the owner has read it and a Codex review has been through it
adversarially — §11 is the list of things that review is specifically asked to
attack.

---

## 0. The ruling this is written under, and the framing it revises

**The owner's direction, 2026-09-15: "modular, reusable, pluggable, with
well-designed interfaces; plan first; Codex reviews and improves the plan."**

The framing put to the owner before the measurement was: follow the seams the
workspace already has — `bt-term`, `bt-viewport`, `bt-render`, `bt-math`,
`bt-doc`, `bt-platform`, `bt-persist` are already separate crates — the disease
is the god object `Runtime` in `bt-app`, and the goal is a thin orchestrator
over subsystems behind narrow interfaces, each with its own tests. Not a
third-party plugin system.

**That framing survives in its shape and is wrong in its diagnosis, and this
document exists mostly to say so before any code moves.** Three measurements
changed it:

1. **`Runtime` is not a god object. It is a two-field facade.**
   `struct Runtime<'a> { app: &'a mut App, window: &'a mut WindowRuntime }`, and
   the crate already pins that shape with a `const _` destructure and a
   source-reading test. The god objects are the two things it borrows: **`App`
   has 78 fields and `WindowRuntime` has 245**. 1,310 methods hang off
   `impl Runtime`. (Inventory §3.3.)

2. **The obstacle to extracting subsystems is not `Runtime` at all.** Only seven
   modules outside `main.rs` name `Runtime` or `WindowRuntime` in production
   code. The obstacle is that **the module graph of `bt-app` is not a DAG**: 43
   of its 99 modules — 232,495 lines, 51% of the crate — form **one strongly
   connected component**. Cargo crates cannot be mutually dependent, so
   extracting any one of those 43 means extracting all 43. (Inventory §5.)

3. **Every candidate the brief named for Step 2 is inside that component except
   one.** `settings`, `profiles`, `i18n`, `first_run`, the git panel model and
   the preview document logic are all in it. `formula_tools` is the only one
   outside. The honest extractable set *today* is **33 modules and 24,572
   lines** — 5.4% of the crate. (Inventory §5.)

So the plan gains a step the brief did not have, and it is the cheapest step in
the document: **cut the cycles first**, starting with three functions in
`i18n.rs` whose removal takes the extractable set from 24,572 lines to
**66,273** — a 2.7× multiplication for about a day. Everything else follows in a
dependency order that the measurement, rather than taste, decides.

### 0.1 How the steps here map onto the brief

| Brief | Here | Why it moved |
| --- | --- | --- |
| — | **Step 0** — baseline and profile quick wins | the gate-time claims in §10 are a model; nothing should be spent on the model's strength before one afternoon is spent on real timings |
| — | **Step 1** — the `i18n` cut (three functions) | a prerequisite the brief did not know about; it is also the cheapest and least conflict-prone ticket in the plan |
| Step 1 | **Step 2** — `main.rs` → `runtime/*.rs` by theme | unchanged in intent; the cost is 91 source-reading pins, not the moves |
| Step 2 | **Step 3** — extract crates in dependency order | the candidate list changes completely; the order is measured |
| Step 3 | **Step 4** — the orchestrator boundary | unchanged in intent; §8.3 says where "pluggable" is honest and where it is a lie |

---

## 1. The problem, in numbers

*All measured; inventory §§1–3.*

- `bt-app` is **456,556 lines — 65.6% of all first-party Rust** in a 16-member
  workspace whose other fifteen members total 222k.
- `main.rs` is **165,816 lines**, of which **63,487 lines (38%) are two
  `impl Runtime<'_>` blocks** holding 1,310 methods. Five methods —
  `mouse_input`, `refresh_chrome`, `chrome_mouse_input`, `keyboard_input`,
  `create` — are 4,858 lines between them.
- **43% of the crate is `#[cfg(test)]`** — 197,157 lines. `main.rs` alone holds
  **1,043 tests**, 770 of them inside a *single* `mod tests` of 46,636 lines.
- `bt-app` has **no `[lib]` target**. All 3,749 of its tests compile into **one**
  test binary, and `cargo test` codegens the crate twice — once as `folio.exe`
  (85.8 MiB) and once as the harness (**209.8 MiB, with a 654.6 MiB PDB**).
- `main.rs` grew **+34,764 lines in the fifteen days** from `v0.1.0-preview` to
  HEAD. `cargo test --release` on the owner's machine already **cannot compile
  it** — rustc dies with `STATUS_STACK_BUFFER_OVERRUN`.
- **98 of the last 150 commits that touch `crates/bt-app` also touch
  `main.rs`** (65%). That is the merge-conflict number, and it is not about
  taste: two tickets in parallel are both adding methods to the same
  `impl Runtime` block.

The consequences the owner feels — a 15-minute gate at 7–9 GB per compile,
batches that conflict, and agents navigating a god object — are all downstream
of those six lines.

---

## 2. What is actually in `main.rs`, by theme

*Measured for the mass; the classification is a name-based sort with a 4.3%
residue, described in inventory §0 and tabulated in §3.4.*

| Theme | Methods | Lines | | Theme | Methods | Lines |
| --- | ---: | ---: | --- | --- | ---: | ---: |
| preview & documents | 270 | 9,033 | | web seats | 34 | 1,301 |
| input: mouse & drag | 59 | 5,064 | | focus cards | 49 | 1,246 |
| git panel & graph | 101 | 3,638 | | math & formula | 32 | 1,152 |
| panes & layout | 124 | 3,416 | | terminal & PTY | 33 | 1,006 |
| tabs & rename | 71 | 2,630 | | launch & CLI | 4 | 900 |
| frame & present | 19 | 2,311 | | attention & notifications | 26 | 681 |
| floats & menus | 57 | 2,159 | | first run / integration | 24 | 665 |
| *unclassified* | 99 | 2,086 | | search | 22 | 612 |
| file peek | 56 | 2,023 | | tooltips & hints | 9 | 573 |
| files column | 57 | 1,845 | | DPI & resize | 11 | 523 |
| settings panel | 31 | 1,664 | | profiles | 21 | 457 |
| input: keyboard & IME | 31 | 1,656 | | command palette | 15 | 454 |
| windows & session | 34 | 1,474 | | quake, clipboard, trace, i18n | 21 | 310 |

**A split by theme is not a split into equal files.** `preview & documents` is
18.5% on its own, and two themes — mouse and frame — are dominated by single
thousand-line event handlers that will have to be split *within* the theme or
left whole. The plan leaves them whole in Step 2 and does not pretend otherwise.

---

## 3. The interface map

For each candidate subsystem: what state it owns, what it needs, what it hands
back, and whether the dependency is one-way today. "Owns" means
`WindowRuntime`/`App` fields touched by that theme and no other — measured, 86
of 239 touched `WindowRuntime` fields are single-theme (inventory §4).

### 3.1 The three hub kinds

The 38 `WindowRuntime` fields touched by five or more themes are not one
problem. They are three, and only one is hard:

| Kind | Fields | Interface | Obstacle? |
| --- | --- | --- | --- |
| **Services** | `renderer` (220 methods / 25 themes), `app.gpu` (64/21), `app.settings_store` (84/20), `app.motion` (90/18), `app.shortcuts`, `app.profile_programs` | pass as `&` / `&mut` in a context struct | **No.** They are read far more than written and already have types of their own. |
| **Ephemeral input** | `pointer_position` (38/13), `modifiers` (22/10), `modifiers_held`, `drag`, `seat_pointer` | pass by value on the event that already carries them | **No.** One writer, many readers, per turn. |
| **The document model** | `tabs` (233 methods / 23 themes), `active_tab` (154/21), `window` (79/21) | — | **Yes. This is the boundary problem.** |

`window.tabs` is the tree of tabs, panes, seats and their contents. Almost every
subsystem reads it; a large minority mutate it. **No subsystem boundary that
leaves `tabs` on the far side of an interface will hold, and no interface that
hands a subsystem `&mut tabs` is a boundary.** Step 4 is built entirely around
that sentence: the orchestrator keeps `tabs` and subsystems return *intentions*
about it.

### 3.2 The subsystems, by how one-way they already are

Measured by "hub fields needed" (inventory §4). The fewer, the cleaner the cut.

**Genuinely one-way today — these are the Step 4 candidates:**

| Subsystem | Owns | Needs | Hands back | One-way? |
| --- | --- | --- | --- | --- |
| **attention & notifications** (26 methods / 681 lines) | `toasts`, `toasts_drawn`, `toast_pointer_drawn`, `notice_layouts`, `notice_hover` | `renderer`, `pointer_position`, `active_tab`/`tabs` (read only: which seat is speaking), `window` (taskbar) | toast layouts to draw; "open this thing" intents | **Yes.** Its only writes are to its own five fields. |
| **search** (22 / 612) | `search_layout`, `search_hover` | `renderer`, `modifiers`, `window`, `search` (its own hub field) | a capsule to draw; a scroll-to intent | **Yes.** |
| **first run / integration** (24 / 665) | — (reads `first_run`, `psreadline_invite`) | `renderer`, `settings_store` | a card to draw; an install intent | **Yes.** The installers are already `attention_hooks`/`explorer_menu`/`psreadline` modules. |
| **quake / summon** (8 / 123) | — | `renderer`, `window` | show/hide a window | **Yes**, and it already has its own `quake` module holding the state on `App`. |
| **math & formula** (32 / 1,152) | `math_tools`, `math_copied` | `renderer`, `app.math_worker`, `pointer_position`, `hover_pane` | rendered formula results; a follow-the-cursor tool strip | **Yes**, through the worker mailbox that already exists. |
| **command palette** (15 / 454) | — | `palette`, `modifiers`, `tabs` (read: candidates), `window` | a chosen action | **Yes**, and it already returns an action enum. |
| **diagnostics & trace** (4 / 65) | — | `active_tab`, `tabs`, `window` | text to a log | **Yes.** |

**Not one-way, and not subsystems — these are the orchestrator:**

| Theme | Hub fields needed | Why it is not a subsystem |
| --- | ---: | --- |
| input: mouse & drag | 33 | it *is* the routing layer; a trait around it would have one implementation and a `&mut WindowRuntime` parameter |
| panes & layout | 27 | it mutates `tabs` structurally — the document model is its subject |
| frame & present | 21 | it reads every other subsystem's drawn state by construction |
| tabs & rename | 20 | same as panes & layout |
| floats & menus | 16 | the popup mutual-exclusion controller is a DESIGN §7.1 invariant (#11) and has to sit above everything that can raise a layer |

**In between — extractable as *models*, not as subsystems:** `preview &
documents` (22 hub fields, but 23 owned fields — the document model and its
layout can leave, the pane wiring cannot), `git panel & graph` (15 / 2 —
`git`, `git_graph` and `git_panel` are already separate modules and the
`impl Runtime` part is menu and hover wiring), `file peek` (12 / 10), `focus
cards` (14 / 2), `files column` (10 / 2), `settings panel` (8 / 0 — the panel's
*model* is already `settings.rs`; the 1,664 lines in `main.rs` are the
apply-a-choice wiring and belong to the orchestrator).

---

## 4. Step 0 — the baseline, and the quick wins that need no split

**Independently landable. Zero behaviour change. Buys the only honest numbers in
this document.**

Everything in §10 is a model. The model should not be trusted for a day longer
than it takes to replace it.

1. **Take a real baseline.** On the owner's machine, in a clean worktree with
   its own `target`:
   - `cargo build --timings --workspace --locked` — three runs, cold, keeping
     the HTML.
   - `cargo test --workspace --locked --no-run --timings` — three runs.
   - `cargo clippy --workspace --all-targets --locked -- -D warnings` — three
     runs, wall-clock.
   - `Measure-Command { ./scripts/check-shortcuts-table.ps1 }` immediately after
     a green `cargo test --workspace --locked`. **This one settles the "three
     compiles" question**: the script's flags already match the gate's, so if it
     is seconds, the third compile is a sequencing accident and not a flag
     mismatch (inventory §6).
   - Peak RSS of the linker during the harness link, from Task Manager or
     `Get-Process link`.

2. **`[profile.test] debug = "line-tables-only"`.** Measured fact: the harness
   PDB is 654.6 MiB and the harness itself 209.8 MiB. The release profile
   already uses exactly this setting, with a manifest comment recording the
   measurement that chose it. Function names and source lines survive; types and
   locals do not. **Expected: a large cut in link time and in the 74.5 GiB
   `target\debug`. Estimated, not measured.** Risk: a panicking test's backtrace
   loses locals, which it did not have anyway in a `--nocapture` run.

3. **Question `[profile.test] opt-level = 1`.** It optimises 456k lines
   including 197k lines of test code. Whoever set it presumably had a slow
   suite in mind (corpus replay, render). **The measurement is one afternoon**:
   compile time and suite run time at `1` and at `0`. If run time is flat, the
   compile saving is free. If it is not, the number goes in the manifest beside
   the setting, the way the release profile's numbers already are.

4. **`rust-lld` as the linker for the dev/test profiles.** Rust 1.89 ships
   `rust-lld`; a 210 MiB image with this much debug info is exactly the case it
   is fastest on. **Must be trialled, not adopted**: `hang_watch` symbolises
   `folio.exe+<offset>` against the PDB, and the release profile's `debug =
   "line-tables-only"` line exists because a hang report that could not be read
   cost a day. If the trial is only on `[profile.test]`, the release path is
   untouched and the risk is confined to test backtraces.

5. **Order the gate so the scripts follow it.** `check-shortcuts-table.ps1`,
   `generate-shortcuts-table.ps1` and `check-portable-core.ps1` run after
   `cargo test --workspace --locked`, in the same tree and the same target
   directory. Write that order into `CONTRIBUTING.md`'s gate section so it is a
   rule rather than a habit.

**Tests that guard it:** the three gates themselves, unchanged. A profile change
that breaks a test breaks the gate.
**Rollback:** revert the manifest lines. Nothing else moved.
**Interaction with 0.4.1:** none. No source file changes.

---

## 5. Step 1 — the `i18n` cut: three functions, 2.7× the extractable surface

**Independently landable. Zero behaviour change. One day.**

*Measured (inventory §5.1).* `i18n` has exactly three outgoing edges and each is
a single function:

| At | Item | Move it to |
| --- | --- | --- |
| `i18n.rs:6359` | `pub fn colour_name(colour: crate::marks::MarkColour) -> Text` | `marks` (it is a match over `marks`'s own enum) or `profiles` (its only caller) |
| `i18n.rs:6378` | `pub fn profile_entry_fault(fault: &crate::profiles::ProfileFault) -> String` | `profiles` |
| `i18n.rs:6554` | the one call to `crate::preview::format_pixel_size(…)` | inline the two-line formatter, or move `format_pixel_size` down into `i18n` |

After that, `i18n` depends on nothing in the crate:

| | Extractable set | Largest cycle |
| --- | --- | --- |
| today | 33 modules / 24,572 lines (13,910 prod) | 43 modules / 232,495 lines |
| after this step | **51 modules / 66,273 lines (36,844 prod)** | 33 / 196,657 |

**Why this comes first, before the `main.rs` split.** It touches four files,
none of them `main.rs`, so it conflicts with nothing in flight. It is the
prerequisite for every crate in Step 3 — `bt-i18n` is what 30 modules depend
on, and while it depends on them, none of them can leave. And it is the smallest
ticket in the plan by an order of magnitude.

**The shape of the rule this enforces**, and the one sentence to put in
`CONVENTIONS.md` with it: *a translation module renders strings; it does not
know the types of the things being described.* The three functions are all
formatters that took another module's enum. Each module formats its own faults
using the generic `Text` lookup.

**Risks.** `Text` has thousands of variants and `i18n.rs` is 9,435 lines; a
mechanical move of three functions cannot break it, but a *reflex* to tidy while
in there can. The ticket says: move three items, change no strings, add no
variants.

**Tests that guard it.** The i18n suite is 2,361 lines in `i18n.rs` plus the
per-module string tests; `cargo test -p bt-app --bin folio -- i18n` before and
after must produce an identical list and identical results. The two
`profiles`/`marks` receiving modules gain the moved tests.

**Rollback.** One revert of one small commit.

---

## 6. Step 2 — `main.rs` into `runtime/*.rs`, by theme

**Independently landable. Zero behaviour change. Days, most of them spent on the
91 source pins rather than on the moves.**

### 6.1 What moves, and why it is genuinely mechanical

Rust allows `impl Runtime<'_>` blocks in as many files of one crate as you like.
And **no visibility changes are needed**: an item declared private at the crate
root is visible in every descendant module of that root, so
`crate::runtime::preview` can `use crate::{Runtime, WindowRuntime, TabState};`
and touch private fields of `WindowRuntime` exactly as `main.rs` does today. The
move is text relocation and `use` lines. Nothing else.

Proposed shape, in two sub-steps:

**2a — the production methods.**

```
crates/bt-app/src/
  main.rs                  the crate root: `mod` list, the type declarations,
                           `App`, `WindowRuntime`, `FolioApp`, `main`,
                           FILES_THAT_MAY_NAME_A_PLATFORM, and nothing else
  runtime/mod.rs           `mod preview; mod input_mouse; …` and nothing else
  runtime/preview.rs       ~9,000  impl Runtime<'_> { … }
  runtime/input_mouse.rs   ~5,100
  runtime/git.rs           ~3,700
  runtime/layout.rs        ~3,400
  runtime/tabs.rs          ~2,700
  runtime/frame.rs         ~2,300
  runtime/floats.rs        ~2,200
  runtime/file_peek.rs     ~2,000
  runtime/files.rs         ~1,900
  runtime/settings.rs      ~1,700
  runtime/input_keys.rs    ~1,700
  runtime/session.rs       ~1,500
  runtime/web.rs           ~1,300
  runtime/cards.rs         ~1,250
  runtime/math.rs          ~1,150
  runtime/term.rs          ~1,000
  runtime/launch.rs        ~900
  runtime/attention.rs     ~700
  runtime/first_run.rs     ~700
  runtime/search.rs        ~600
  runtime/hints.rs         ~600
  runtime/dpi.rs           ~520
  runtime/profiles.rs      ~460
  runtime/palette.rs       ~450
  runtime/small.rs         ~2,400  quake, clipboard, trace, i18n, and the 99
                           methods the sort could not place
```

Twenty-six files, none over 9,000 lines, most under 2,500. **`main.rs` drops
from 165,816 lines to roughly 40,000** — the type declarations (L1–L13937, which
are `struct`/`enum`/`impl` for the 199 structs and 105 enums), `impl FolioApp`,
`impl ApplicationHandler`, `main`, and the test modules Step 2b has not moved
yet.

**2b — the tests.** `tests/` is **not available**: `bt-app` is a bin-only crate,
so an integration test cannot reach its items (inventory §2). The tests go to
`runtime/<theme>_tests.rs`, declared `#[cfg(test)] mod <theme>_tests;` beside
the theme they cover. The 46,636-line `mod tests` splits by the same sort as the
methods; the 43 already-named modules (`tab_identity_tests`,
`pty_drain_budget_tests`, `quit_transaction_tests`, …) move whole and keep their
names.

### 6.2 The real cost: 91 source-reading pins

*Measured.* 91 tests in `main.rs` do `const SOURCE: &str = include_str!("main.rs")`
and then grep it. After a split each of them sees a fragment. This is not
optional work: those pins hold `layer_shape_tests`' claim that `Runtime` has
exactly two fields and nothing else, and they hold rules — "which table a
platform ships, which flag a family of shells is told" — that a runtime
assertion cannot check.

**The tool already exists.** `crates/bt-app/src/source_pin.rs` (133 lines) has
`source_region(source, header)`, which finds one item by brace counting and
panics loudly if the header is gone, and `code_of`, which strips its comments.
(It moves into `bt-trace` in Step 3, §7.1 row 6; the 91 pins then call
`bt_trace::source_pin::source_region`, which changes nothing about how they
work.) Three modules already use it, and nine other modules already pin themselves with
`include_str!` of *their own* file — `animation.rs`, `first_run.rs`,
`formula_tools.rs`, `diagnostics.rs`, the three attention installers. **That is
the pattern that survives a move**, and the migration is: for each of the 91,
decide what its subject is, and re-point it at the file that subject now lives
in.

Estimated, from the distribution of the pins across the file: roughly 60 of the
91 have a single item as their subject and convert straight to
`source_region(include_str!("<new file>"), "<header>")`. The rest — the ones
whose subject is "everywhere in the program that does X" — need a helper that
reads several files, which is a `const SOURCES: &[&str] = &[include_str!(…), …]`
and a loop. **Neither is hard; there are 91 of them, and each is a separate
judgement about what the pin was for.** This is the step's schedule.

`scripts/check-portable-core.ps1` parses `FILES_THAT_MAY_NAME_A_PLATFORM` out of
`main.rs` by regex. **That array stays in `main.rs`**, and the script is
untouched — the cheapest possible answer, and it should be written into the
array's own doc comment so the next split does not move it.

### 6.3 How it is verified

- **2a: the test list must be byte-identical.** No test moves in 2a, so
  `cargo test -p bt-app --bin folio --locked -- --list` before and after must
  diff empty. That is the whole acceptance gate for the production move, and it
  is a strong one: a method that changed behaviour cannot change the list, but a
  method that failed to compile or a `cfg` that stopped applying will.
- **2b: the list changes by prefix only.** Test paths gain a module segment
  (`tests::foo` → `runtime::preview_tests::foo`). The check is: sort both lists
  by the *leaf* name, diff, and demand the count is 1,043 on both sides and the
  leaf sets are equal. Write that comparison as a script under `scripts/dev/`
  and keep it; the next reorganisation will want it.
- **The three gates green**, and `scripts/check-shortcuts-table.ps1` still green
  (its `--exact shortcuts::tests::docs_shortcuts_md_is_the_bindings_table` path
  is in `shortcuts.rs` and does not move).
- **`git diff --stat` must show only moves.** A line that is neither a deletion
  from `main.rs` nor an identical insertion elsewhere is a bug in this ticket.
  Verify with a whole-file normalised diff: concatenate `main.rs` and
  `runtime/*.rs` before and after, sort, diff — the multiset of non-`use`,
  non-`mod` lines must be equal.

### 6.4 Expected effect

**Compile time: none, and the plan should not claim any.** One crate is one
compilation unit for type checking; codegen units in the dev profile already
default to 256, so rustc already parallelises codegen within `bt-app`. Moving
text between files of the same crate changes the dependency graph not at all.
CI sets `CARGO_INCREMENTAL: 0`, and the local `target\debug\incremental` is
empty, so there is not even an incremental-granularity argument to make.

**What it does buy**, and these are the reasons to do it:

- **Navigation.** The handoff already tells agents "别整篇读,用 grep 定位" —
  do not read the file, grep it. That instruction exists because the file cannot
  be read. Twenty-six files of 1–9k lines can be.
- **Merges.** Measured: 98 of the last 150 `bt-app` commits touch `main.rs`.
  After the split, a preview ticket and a git-panel ticket touch different
  files. That is the owner's stated pain and this step is its direct answer.
- **It is the precondition for Step 4.** A trait boundary cannot be drawn around
  a theme whose methods are interleaved with twenty-five others.

### 6.5 Risks, interaction with in-flight work, rollback

**The dominant risk is not correctness. It is the merge.** This step rewrites
`main.rs` completely. Any branch open when it lands has — measured — about a
65% chance of touching `main.rs`, and the conflict will be one git cannot help
with, because the hunk's file no longer exists. Git's rename detection does not
survive one file becoming twenty-six.

**The rule, and it is not negotiable:** *Step 2 lands in a window where no other
`bt-app` branch is open.* Concretely — drain the 0.4.1 user-visible queue first,
land it, then dispatch Step 2 as the only `bt-app` ticket in flight, then
re-dispatch. Step 1 (§5) and Step 3 (§7) have no such constraint, which is the
reason Step 1 goes first and part of the reason Step 3 can be interleaved with
feature work later.

**Other risks:**

| Risk | Guard |
| --- | --- |
| a pin silently stops checking anything (greps a fragment that no longer contains its subject) | `source_pin::source_region` panics when the header is gone; for pins converted to the multi-file form, plant the violation and watch it go red, per `CONTRIBUTING.md`'s "a new guard proves it fires" |
| a `#[cfg(windows)]` or `#[cfg(target_os = "macos")]` changes meaning across a module boundary | `cargo check --locked --all-targets -p bt-app` on Windows *and* the Linux/macOS CI jobs, which already exist and already compile `--all-targets` |
| 151 references to `main.rs` in `docs/DESIGN.md` go stale | they are path references, not line numbers; a sweep in the same commit, and the DESIGN change is part of the ticket's definition of done |
| someone "tidies" during the move | the normalised-diff check in §6.3 fails on any changed line |

**Rollback:** one `git revert` of one merge commit. Because the step is pure
moves, the revert is clean by construction; the normalised-diff check is what
makes that true.

---

## 7. Step 3 — extract crates, in the order the graph allows

**Each crate is independently landable. Zero behaviour change per crate.**

### 7.1 The order, measured

After Step 1, the dependency-ordered layers are these. Nothing may be extracted
before the layer above it. (Inventory §5.1.)

| # | Crate | Modules | Lines | Prod | What it is |
| ---: | --- | --- | ---: | ---: | --- |
| 1 | **`bt-i18n`** | `i18n` | 9,435 | 7,074 | the `Text` table and `Lang`; 30 modules depend on it |
| 2 | **`bt-web-nav`** | `webnav`, `favicon`, `web_thumb`, `web_trace` | 5,151 | 2,570 | URL grammar, site labels, the favicon store, page thumbnails |
| 3 | **`bt-preview-text`** | `preview_viewport`, `preview_wrap`, `preview_typing`, `table_block`, `linebreak`, `hex_peek`, `preview_viewport_tests` | 4,867 | 3,712 | wrapping, viewport geometry, typing geometry, table and hex blocks |
| 4 | **`bt-anim`** | `animation`, `settling` | 3,270 | 1,711 | GIF/animated-picture streaming, crossfade clocks |
| 5 | **`bt-watch`** | `preview_watch`, `files_watch`, `git_watch`, `watch_clock`, `dir_news` | 2,444 | 1,378 | the four filesystem watchers, over `bt-platform` |
| 6 | **`bt-trace`** | `trace`, `card_trace`, `glyph_trace`, `preview_trace`, `attention_trace`, `source_pin` | 1,597 | 967 | the `BT_*_TRACE` facilities and the source-pin helper |
| 7 | **`bt-keys`** | `input` | 1,669 | 734 | key/modifier translation over `bt-platform` |
| 8 | **`bt-pdf`** | `pdf` | 1,224 | 669 | the glance card's first page, over hayro |
| 9 | **`bt-file-index`** | `palette_index` | 1,213 | 604 | the bounded directory walk and its worker |
| 10 | **`bt-formula-tools`** | `formula_tools` | 1,195 | 426 | the formula tool strip model |
| 11 | *(into `bt-platform`)* | `wsl`, `app_delegate_wire` | 572 | 292 | already platform bridges; they belong next door, not in a new crate |
| 12 | **`bt-text-field`** | `text_field` | 696 | 426 | the single-line editor model |
| — | *rows 13–16 need Step 1 first; rows 2–12 do not, and row 1 **is** Step 1* | | | | |
| 13 | **`bt-shortcuts`** | `shortcuts` | 6,692 | 3,185 | the binding table and chord matcher |
| 14 | **`bt-marks`** | `marks`, `icons` | 10,203 | 4,570 | chrome marks and action icons — a two-module cycle, so one crate |
| 15 | **`bt-persist-app`** | `persist`, `schemes`, `pins`, `diagnostics`, `hang_watch`, `scheme_watch`, `storage_watch` | 8,444 | 4,752 | the app's own stores over `bt-persist`, the two watchers that follow them, and the hang watch with its report |
| 16 | *(small)* | `update`, `seed`, `menubar`, `quake`, `arrival`, `context_menu`, `recent_folders`, `focus_thumb_restore_tests` | 7,295 | 3,680 | judgement calls; group them or leave them where they are |

**Total reachable in Step 3 without touching the big component: 66,273 lines,
36,844 of them production and 29,429 of them `#[cfg(test)]`.** The sixteen rows
above come to 65,967; the remaining 306 is `version`, which is the next
paragraph. The component that remains is 33 modules and
196,657 lines, and Step 3 stops there.

**`version` is deliberately not on the list.** It reads
`env!("CARGO_PKG_VERSION")` and `concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml")`,
and its whole job is that four surfaces agree about one line in the manifest.
Moving it into another crate changes what both of those macros resolve to. It
would still work — `version.workspace = true` makes the value identical and the
relative path lands on the same file — but it is the one module in the list
where a move is not obviously a no-op, and the manifest comment that names it as
the gate would need re-reading first.

### 7.2 The interface for each crate

Each of these is a **leaf model**, not a subsystem with a lifecycle: it owns
types and pure functions, `bt-app` owns the state that holds them. So the
interface is the crate's public API and nothing more — no traits, no
registration, no injection. What each must *not* have is the thing that kept it
in: a dependency back on `bt-app`.

The one design decision worth taking deliberately is **`bt-i18n`'s shape**. Its
`Text` enum has thousands of variants named after the places they appear
(`ProfilesColourBlue`, `GitWorkerStopped`, `RefNameEmpty`). As a crate that is
fine and no worse than it is today. **It is not an invitation to redesign it in
the same ticket.** The rule from Step 1 holds: a translation crate renders
strings, it does not know the types of what is being described.

### 7.3 Expected compile-time effect — a model, honestly labelled

*Estimated. The inputs are named; replace this with Step 0's measurements before
anyone spends a week on the strength of it.*

The first pass costs the same or slightly more: the same code is compiled, plus
sixteen more crate boundaries and their metadata. What changes is everything
after the first pass:

1. **Parallelism.** The layers above are mostly independent — layers 2 through
   12 have no edges between them, so cargo can build eleven crates at once
   where it built one. On 19 jobs that is real, but it applies only to 66k of
   455k lines.
2. **Not rebuilding.** An edit in `main.rs` today recompiles 456k lines. After
   Step 3 it recompiles 390k. **Estimated gain: 14.5% of the crate's compile,
   on every single edit.** That is the compounding one.
3. **The test binary.** Measured, the harness is 209.8 MiB against the bin's
   85.8 MiB, so the test link is the expensive half. 29,429 of the 66,273
   extracted lines are `#[cfg(test)]`; each extracted crate links its own,
   much smaller, test binary against only its own dependencies. **Estimated:
   roughly 15% off the harness's size and its link, and — more usefully — those
   tests stop being relinked when `main.rs` changes.**

**The estimate's weakness, stated plainly:** compile time in a crate this size
is not linear in lines. Macro expansion, trait resolution and the monomorphising
of `bt-render`'s and `winit`'s generics dominate in ways line counts do not
predict, and the extracted modules are the *simplest* code in the crate
(`bt-trace` is 967 production lines of string formatting). The true gain from
Step 3 could plausibly be half the 14.5%. **Nobody should promise the owner a
number here until Step 0 has run.**

### 7.4 Risks, guards, in-flight work, rollback

| Risk | Guard |
| --- | --- |
| a `pub(crate)` becomes `pub` and the crate's surface silently widens | each extraction ticket lists the exact public API in its description; `cargo doc` of the new crate, read once |
| a `#[cfg(test)]` helper used across the module boundary stops compiling | expected and fine — it surfaces at once; the helper either goes public in the new crate or is duplicated, and the ticket says which |
| a module's tests were relying on a sibling's test fixture | same; it appears as a compile error in the extraction commit, never as a silent pass |
| `THIRD-PARTY-NOTICES.md` drift | `scripts/check-notices.ps1` and `scripts/generate-notices.ps1` already walk `cargo metadata`; a new first-party crate with `publish = false` adds nothing to the notices, and the check proves it |
| workspace manifest churn | each new crate needs `[lints] workspace = true`, `version.workspace`, `publish.workspace`; CONVENTIONS.md §113 records that a crate without the lints line makes clippy a green empty gate. **A crate added without that line is the incident repeating.** |

**Tests that guard each extraction:** the moved module's own `#[cfg(test)]`
suite, run from its new crate; `cargo test --workspace --locked` for the whole;
and the `--list` leaf-name comparison from §6.3, which is by now a script.

**Interaction with 0.4.1.** Far gentler than Step 2. An extraction moves whole
files into a new directory — git rename-detects it cleanly — and touches
`main.rs` only at the `mod x;` line and the `use` paths. A branch that edits
`preview.rs` and a ticket that extracts `bt-trace` do not meet. **Step 3 tickets
can be interleaved with feature work**, one crate at a time.

**Rollback:** revert the extraction commit. The `Cargo.lock` change is the only
messy part, and it is a `cargo update -w` away from correct.

---

## 8. Step 4 — the orchestrator boundary, and where "pluggable" is honest

**This step is a design, not yet a schedule.** It should not be dispatched until
Steps 1–3 have landed and the measurement in §3 has been re-taken on the smaller
`main.rs`.

### 8.1 What `Runtime` keeps

Three things, and nothing else:

1. **Event routing.** `impl ApplicationHandler<AppEvent> for FolioApp` and the
   dispatch beneath it — `mouse_input`, `keyboard_input`, `pointer_moved`,
   `mouse_wheel`, `ime_input`, `user_event`.
2. **The document model.** `window.tabs`, `window.active_tab`, and the structural
   mutations of them — open, close, split, move across tabs, tear out. This is
   §3.1's hard hub and it does not get an interface; it gets an owner.
3. **Frame publication.** `turn`, `redraw`, `publish_frame`, `refresh_chrome`,
   `refresh_overlay`.

### 8.2 What a subsystem gets

The narrowest interface that the measurement supports, and it is not a trait
object:

```rust
// sketch, not a proposal to write today
struct Services<'a> {            // the "service" hub fields of §3.1, borrowed
    renderer: &'a mut Renderer,
    settings: &'a Settings,
    motion: Motion,
    shortcuts: &'a Shortcuts,
    lang: Lang,
}

struct Turn<'a> {                // the "ephemeral input" hub fields of §3.1
    pointer: Option<PhysicalPosition<f64>>,
    modifiers: Modifiers,
    now: Instant,
}

// a subsystem owns its own state and never sees `tabs`
impl Attention {
    fn pointer_moved(&mut self, turn: &Turn<'_>, out: &mut Vec<Intent>);
    fn layers(&self, services: &Services<'_>) -> Vec<Layer>;
}
```

- **Subsystems never take `&mut WindowRuntime`.** That is the whole rule, and it
  is checkable: a source pin that greps `runtime/*.rs` for a subsystem method
  signature naming `WindowRuntime` is exactly the kind of guard this crate
  already writes 91 of.
- **Subsystems hand back intents, not mutations.** `Intent::OpenPath(PathBuf)`,
  `Intent::FocusSeat(SeatId)`. The orchestrator applies them to `tabs`. This is
  what keeps §3.1's hard hub owned by one place.
- **No `dyn` in the frame path without a measurement.** The frame loop runs at
  60 Hz and this product has a history of measuring before it accepts a cost
  (see the release profile's manifest comments). A trait with one implementation
  and a virtual call per frame is a tax; a generic parameter or a plain struct
  method is not.

### 8.3 Where "pluggable" is honest, and where it is not

**Honest — these can be switched off or replaced in tests**, because §3.2
measured that their hub reads are services rather than the document model:
`attention & notifications`, `search`, `first run / integration`,
`quake / summon`, `math & formula`, `command palette`, `diagnostics & trace`,
and the Step 3 crates (`bt-file-index`, `bt-web-nav`, `bt-anim`, `bt-watch`).
For each, "switched off" has a real meaning — a window with no attention desk
still opens, draws and runs a shell — and a no-op implementation is a legitimate
test double.

**Not honest — do not write an interface for these:** `input: mouse & drag`
(33 hub fields), `panes & layout` (27), `frame & present` (21),
`tabs & rename` (20), `floats & menus` (16). They are the orchestrator. An
interface around them would be a trait with one implementation whose methods all
take `&mut WindowRuntime`, which is the abstraction tax with none of the
benefit, and which would make the next reader believe a boundary exists where
none does. **Saying so in the document is part of the deliverable.**

**And "pluggable" never means third-party in this pass.** No plugin API, no ABI,
no dynamic loading, no manifest format. The word means: a subsystem of this
program can be compiled out, stubbed in a test, or replaced by another
implementation *in this repository*.

---

## 9. What NOT to do

1. **No new abstraction for its own sake.** Every trait in this plan must name
   the second implementation it exists for, and "a test double" counts only for
   the seven subsystems §8.3 lists as honest. `CONVENTIONS.md` already records
   the incident where a lint table inherited into nothing and passed every run;
   an interface with one implementation is the same shape of decoration.
2. **No plugin API in this pass.** No dynamic loading, no stable ABI, no
   third-party surface, no manifest format. The owner's word "pluggable" is
   answered by §8.3 and nothing more.
3. **No behaviour change mixed into a move.** The normalised-diff check in §6.3
   is what enforces it for Step 2, and the `--list` comparison for Step 3. A
   move ticket that also fixes a bug is rejected and re-cut as two.
4. **No shared `CARGO_TARGET_DIR` tricks.** Two worktrees sharing one target
   directory have already cost this project a day — green tests with red clippy,
   and a signature that did not match, because each tree was reading the other's
   stale artifacts. Every worktree keeps its own `target`.
5. **No renaming during a move.** Method names, test names, module names all
   survive Step 2 and Step 3 unchanged. A rename in the same commit as a move
   destroys the only cheap verification either step has.
6. **No splitting `seats.rs` in the same pass as anything else.** It is 51,303
   lines, it is the single module that remains after every cut in the inventory's
   §5.1 table,
   and it is its own plan.
7. **No adding a `[lib]` to `bt-app` as a "quick win".** It does not save a
   compile — `cargo test` still builds the crate body twice, once as an rlib and
   once with `cfg(test)` — and moving tests to `tests/` would give each file its
   own binary linking the whole 456k-line rlib, which is very plausibly worse
   than one harness. It is a prerequisite for some later options, and it should
   be taken when one of those options is actually being taken, with a
   measurement.
8. **No promising the owner a compile-time number before Step 0 has run.**

---

## 10. The gate-time model

**Current, as the owner reports it: 60–90 minutes for a full gate, 7–9 GB per
compile, 15+ minutes per `bt-app` pass.** Nothing in this document measured
that; it is the owner's observation and it is taken as given.

*Measured facts the model is built on (inventory §§2, 6):*

- `cargo test` codegens and links `bt-app` **twice** — the 85.8 MiB bin and the
  209.8 MiB harness with its 654.6 MiB PDB.
- `cargo clippy --workspace --all-targets` runs under `RUSTC_WORKSPACE_WRAPPER`
  and so has a **different fingerprint** from `cargo test`'s artifacts. It does
  not reuse them and they do not reuse it. `--all-targets` makes it six targets
  rather than two.
- `check-shortcuts-table.ps1` uses the *same* profile and flags as the gate's
  `cargo test`, so its cost is a sequencing question, not a flag question.
  **Step 0 settles it with one `Measure-Command`.**
- 58% of `bt-app`'s lines are code; 42% is prose. Estimates that divide by raw
  lines overstate work by about 1.7×.

*The model, with every step's expected effect and its label:*

| Step | What it changes | Effect on the gate |
| --- | --- | --- |
| **0** — `[profile.test] debug = "line-tables-only"` | 654.6 MiB PDB → a fraction of it | *estimated*: the largest single-line win available, because the harness link is serial and cannot be parallelised by cores |
| **0** — `[profile.test] opt-level` 1 → 0 | no optimisation of 456k lines including 197k of test code | *estimated*, and a trade: compile down, suite run-time possibly up. **Measure both.** |
| **0** — `rust-lld` on the test profile | the link itself | *estimated*, commonly 2–5× on large MSVC links; must be trialled against `hang_watch` symbolisation |
| **0** — scripts ordered after the gate | removes a third compile *if* there is one | *unknown until measured*; possibly zero |
| **1** — the `i18n` cut | nothing compiles differently | **zero.** It buys Step 3. |
| **2** — `main.rs` → `runtime/*.rs` | nothing compiles differently | **zero, and the plan says zero.** It buys navigation and merges. |
| **3** — sixteen crates, 66,273 lines out | an edit in `main.rs` recompiles 390k lines instead of 456k; eleven crates build in parallel; ~15% of the test mass links separately | *estimated* **14.5% off every rebuild**, plausibly as little as 7% because the extracted code is the simplest in the crate |
| **4** — the orchestrator boundary | nothing, by itself | **zero.** It is a correctness and reviewability step, not a speed one. |

**The honest summary for the owner: no step in this plan makes the gate fast.**
Step 0 might take a real bite out of the link, and that is the only place a large
single win is plausible. Step 3 compounds — 14.5% off every rebuild, forever,
and growing as more crates come out. Steps 1, 2 and 4 buy correctness,
navigability and the ability to run two tickets in parallel without a conflict,
which is a different currency and, at 98-in-150, arguably the more expensive one
today.

**The structural win nobody should overlook**: `cargo test --release` already
cannot compile `main.rs` on this machine. Step 2 alone may fix that, because
rustc's stack exhaustion is per-item and per-file recursion in the front end.
That is a *hypothesis*, not a measurement — but it is cheap to test the day
after Step 2 lands, and if it holds it recovers a whole profile the project
currently cannot build.

---

## 11. Open questions for the Codex review

This document goes to an adversarial read before any code moves. These are the
places it is weakest, named so the review does not have to find them first.

1. **Is the strongly connected component real?** It is the load-bearing claim of
   the whole plan. It was computed from `crate::x::` occurrences in
   comment-stripped production code (inventory §0). A `use crate::x::Y;` at the
   top of a file counts as an edge even if `Y` is only a type alias, and a
   module reached only through a macro would be missed. **Re-derive it
   independently** — ideally from `cargo modules` or from rustc's own resolution
   rather than from grep — and say whether 43 and 232,495 hold.
2. **Are `i18n`'s three edges really three?** Everything in Step 1 and most of
   Step 3's ordering rests on it. If there is a fourth reached through a macro
   or a re-export, the step still works but the table in §5 is wrong.
3. **Is Step 2's "zero compile-time gain" right?** The claim is that one crate is
   one compilation unit and that rustc already parallelises codegen at 256 units.
   If there is a front-end effect of file size — parsing, resolution, incremental
   granularity — that makes twenty-six files materially cheaper than one, the
   plan is understating Step 2 and should say so.
4. **The 91 source pins.** Is re-pointing them the right answer, or should the
   pins that are about "everywhere in the program" be moved out of tests
   entirely and into a PowerShell gate beside `check-portable-core.ps1`, which
   is already doing exactly that for one of them?
5. **Is `runtime/*.rs` the right shape, or should the themes be *modules with
   their own state* from the start?** This plan splits by theme first and draws
   interfaces later, on the argument that a boundary is easier to find once the
   code is visible. The opposite order — extract one subsystem completely,
   state and all, and leave the rest in `main.rs` — is defensible and would give
   a working example earlier. Which is cheaper given 98-in-150?
6. **The test-binary estimate.** §7.3 assumes the harness's cost is roughly
   proportional to test lines. Is there a better model? Specifically: is the
   209.8 MiB dominated by test code, by `bt-render`/`winit` monomorphisation
   pulled in by tests, or by debug info that
   `debug = "line-tables-only"` removes for free?
7. **Is `bt-i18n` a good crate or a bad one?** A `Text` enum with thousands of
   variants, imported by thirty modules, is a compile-time hub: touching one
   string variant invalidates everything downstream of it. Extracting it might
   make rebuilds *worse* for string changes while making them better for
   everything else. Is that trade the right way round, and does it argue for
   splitting `Text` by area rather than making it a crate?
8. **`seats.rs`.** 51,303 lines, the last module standing in inventory §5.1's
   table, and
   deliberately out of scope here. Is leaving it for a plan of its own right, or
   does that guarantee it is never done?
9. **Step 2's landing window.** The plan demands an empty `bt-app` queue for one
   ticket. Is there a cheaper protocol — landing the split as a sequence of
   smaller moves, one theme per commit, each rebaseable — that gets the same
   result without freezing the queue? What does that cost in review?
10. **Anything in §9 that is wrong to forbid.** In particular #7: is there a
    version of "add a `[lib]`" that does pay for itself, and is the
    "N integration binaries each link the rlib" objection actually true for a
    crate this size?

---

## Review record

*(empty — this document has not yet been reviewed. The Codex review's findings,
and what this plan changed in response, go here, in the form
`docs/plans/port/macos-plan-2026-09-12.md` uses.)*
