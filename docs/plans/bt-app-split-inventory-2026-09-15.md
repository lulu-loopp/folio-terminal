# `bt-app` — the measured inventory, 2026-09-15

Taken against `main` at `76ca0788` in the worktree
`D:\Developer\bt-wt\bt-app-split-plan`. It is the evidence file for
`docs/plans/bt-app-split.md`; that document cites these tables and does not
restate the method. Nothing here was built: no `cargo build`, no `cargo check`,
no `cargo test`. Every number below is either read out of the source text, read
out of `cargo metadata --no-deps --offline`, or read off files that were already
in `D:\Developer\BetterTerminal\target\debug` from the owner's own runs.

**How to read the labels.** A number marked *measured* came out of a tool run on
this tree and can be reproduced. A number marked *estimated* is a model with its
inputs named. There is no third kind, and a model is never written without its
inputs.

---

## 0. Method, and what it can get wrong

Four things did the counting:

1. **A brace-depth scanner over stripped Rust.** The scanner blanks line
   comments, block comments, string literals (including raw strings with any
   number of hashes) and character literals, keeping line numbers and file
   length, and then counts `{` and `}` to find the span of every item. It is not
   a Rust parser. It cannot be confused by a brace inside a string or a comment,
   which is what a naive count gets wrong; it *can* be confused by a macro body
   with unbalanced braces. None was found — the scanner's spans cover 152,758 of
   `main.rs`'s 165,816 lines and every gap is a run of top-level `use`, `const`,
   `static` and doc comment, which is what the remainder should be.
2. **`cargo metadata --no-deps --format-version 1 --offline`**, for the
   workspace member list, each member's first-party dependencies, and `bt-app`'s
   target list.
3. **`grep` and `wc`**, for counts that are a single pattern.
4. **`ls` on the existing `target\debug`**, for artifact sizes. Those artifacts
   were produced by the owner's own earlier runs, on this machine, at the dates
   the listing shows; they are evidence of *size*, not of *time*.

**The dependency edges are the one place where the method has a real limit and
where a first pass got it wrong.** `crate::foo::Bar` appears in this codebase in
two very different roles: as code, and as an intra-doc link inside a `///`
comment — this tree writes a great many of the latter. Counting both produced a
strongly connected component of 69 modules; counting only the code produced one
of 43. Every dependency number in this file is **code only** — doc links are
stripped before the pattern runs — and the difference is named here because a
reviewer who re-runs the grep without stripping will get the larger number and
should know why.

**The theme classification is a sort, not an audit.** Method names were matched
against an ordered list of regular expressions, first match wins. It puts 95.7%
of the 1,310 `Runtime` methods somewhere; 4.3% (99 methods, 2,086 lines) are
left as `unclassified` rather than forced. A name-based sort will misfile some
methods — `apply_row_verb` sits in `unclassified` and is really a settings
method — so the theme table is a **map of roughly where the mass is**, accurate
to a few percent per row, and is not a work order for any individual function.

---

## 1. The workspace, and where the mass is

*Measured.* 16 workspace members (15 first-party plus the vendored
`alacritty_terminal`). Rust lines under `crates/` and `vendor/`: **695,785**.

| Crate | Lines | Share of `crates/` | First-party deps |
| --- | ---: | ---: | --- |
| `bt-app` | 456,556 | 65.6% | 12 |
| `bt-platform` | 65,700 | 9.4% | 1 |
| `bt-term` | 47,155 | 6.8% | 6 |
| `bt-render` | 39,809 | 5.7% | 4 |
| `bt-viewport` | 14,219 | 2.0% | 3 |
| `bt-persist` | 12,677 | 1.8% | 0 |
| `bt-transcript` | 10,744 | 1.5% | 1 |
| `bt-pty` | 9,693 | 1.4% | 2 |
| `bt-detect` | 8,690 | 1.2% | 3 |
| `bt-layout` | 5,296 | 0.8% | 0 |
| `bt-math` | 3,108 | 0.4% | 1 |
| `bt-corpus` | 2,719 | 0.4% | 4 |
| `bt-winres` | 1,266 | 0.2% | 0 |
| `bt-doc` | 1,002 | 0.1% | 1 |
| `bt-unicode` | 111 | 0.0% | 0 |

**The seams the workspace already has are real and they are acyclic.** The
first-party dependency graph from `cargo metadata` is a DAG: `bt-unicode`,
`bt-layout`, `bt-persist` and `bt-winres` depend on nothing of ours;
`bt-transcript` on `bt-unicode`; `bt-doc` on `bt-transcript`; `bt-render` on
four; `bt-term` on six; and `bt-app` on twelve. `bt-app` is the only member
nothing depends on, which is what makes it the place everything accretes.

*Measured.* `#[test]` attributes per crate: `bt-app` 3,749, `bt-term` 524,
`bt-render` 298, `bt-platform` 261, `bt-persist` 184, `bt-transcript` 177,
`bt-detect` 138, `bt-viewport` 130, `bt-pty` 82, `bt-layout` 51, `bt-math` 44,
`bt-corpus` 16, `bt-winres` 11, `bt-doc` 7, `bt-unicode` 3. **`bt-app` holds
66% of the workspace's 5,675 tests.**

---

## 2. `bt-app` as a compilation unit

*Measured, from `cargo metadata`.* `bt-app` declares **no `[lib]` target**. Its
targets are:

| Kind | Name | Path |
| --- | --- | --- |
| bin | `folio` | `src/main.rs` |
| example | `container-probe` | `examples/container-probe.rs` |
| example | `gif-fixture` | `examples/gif-fixture.rs` |
| example | `video-probe` | `examples/video-probe.rs` |
| test | `macos_glyph_surface` | `tests/macos_glyph_surface.rs` (`harness = false`) |
| custom-build | `build-script-build` | `build.rs` |

Three consequences follow from "bin, no lib", and all three constrain the plan:

- **No other crate can depend on `bt-app`.** A binary crate has no library
  target to link against.
- **Nothing in `tests/` can reach `bt-app`'s items.** The one file in `tests/`
  is `harness = false` and tests a macOS window through `bt-platform`; it does
  not `use bt_app::…` and could not. So all 3,749 tests are `#[cfg(test)]`
  modules compiled into the bin's own test harness, in **one** test binary.
- **`cargo test` codegens `bt-app` twice** — once as the product bin, once as
  the test harness with `cfg(test)` on.

*Measured.* `[profile.test] opt-level = 1` in the workspace manifest; the dev
profile is otherwise cargo's default. `.cargo/config.toml` on this machine sets
`jobs = 19`.

### 2.1 Source size

*Measured.* `crates/bt-app/src`: **102 files, 455,438 lines**, of which
**263,942 are code lines** (non-blank, after comments are stripped) — 58%. The
remaining 42% is prose. That ratio matters for every estimate downstream: a
line count is not a compile-cost count in this crate, and dividing work by raw
lines overstates it by roughly 1.7×.

*Measured.* **`#[cfg(test)]` bodies are 197,157 lines — 43% of the crate.**
`main.rs` alone: 165,816 lines, 60,059 of them under `#[cfg(test)]` (36%).

### 2.2 The files

*Measured.* The twenty largest files, with the `#[cfg(test)]` split and the
number of times the file's production code names `Runtime` or `WindowRuntime`:

| File | Lines | Production | Test | `Runtime` | `WindowRuntime` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `main.rs` | 165,816 | 105,757 | 60,059 | — | — |
| `seats.rs` | 51,303 | 21,799 | 29,504 | 18 | 2 |
| `settings.rs` | 28,233 | 13,552 | 14,681 | 0 | 0 |
| `profiles.rs` | 24,280 | 13,142 | 11,138 | 0 | 0 |
| `preview.rs` | 13,883 | 7,390 | 6,493 | 7 | 0 |
| `i18n.rs` | 9,435 | 7,074 | 2,361 | 0 | 0 |
| `git_graph.rs` | 8,418 | 5,061 | 3,357 | 0 | 0 |
| `git_panel.rs` | 8,031 | 4,372 | 3,659 | 3 | 0 |
| `git.rs` | 7,088 | 3,951 | 3,137 | 0 | 0 |
| `marks.rs` | 7,003 | 4,065 | 2,938 | 0 | 0 |
| `shortcuts.rs` | 6,692 | 3,185 | 3,507 | 1 | 0 |
| `webhost.rs` | 6,594 | 3,848 | 2,746 | 5 | 0 |
| `float.rs` | 5,085 | 2,392 | 2,693 | 0 | 0 |
| `focus_thumb.rs` | 4,122 | 1,683 | 2,439 | 5 | 4 |
| `attention/*` | 4,063 | 1,755 | 2,308 | 0 | 0 |
| `restore.rs` | 3,885 | 2,470 | 1,415 | 0 | 0 |
| `file_peek.rs` | 3,884 | 1,674 | 2,210 | 0 | 0 |
| `first_run.rs` | 3,794 | 1,968 | 1,826 | 0 | 0 |
| `cmdrail.rs` | 3,688 | 1,896 | 1,792 | 0 | 0 |
| `shell_integration.rs` | 3,480 | 1,645 | 1,835 | 0 | 0 |

The `Runtime` columns count occurrences in production code only. **Only seven
modules outside `main.rs` name `Runtime` or `WindowRuntime` at all in production
code**: `seats` (18/2), `preview` (7), `focus_thumb` (5/4), `webhost` (5),
`git_panel` (3), `shortcuts` (1), `preview_viewport` (1). That is a much smaller
reach-back than the god-object framing suggests, and it is the reason the
obstacle to extraction turns out to be the module *cycle*, not `Runtime`.

### 2.3 Growth

*Measured, from `git show <tag>:crates/bt-app/src/main.rs | wc -l`:*

| Point | `main.rs` lines |
| --- | ---: |
| `v0.1.0-preview` (2026-08-31) | 131,051 |
| `v0.2.5-preview` | 142,573 |
| `9acd482` (`v0.3.0-preview`, 2026-09-12) | 156,381 |
| `76ca0788` (HEAD, 2026-09-15) | 165,815 |

**+34,764 lines in fifteen days**, about +2,300 lines a day. The handoff document
still describes `main.rs` as 79,000 lines; it has more than doubled since that
sentence was written.

*Measured.* Of the **last 150 commits that touch `crates/bt-app`, 98 also touch
`main.rs`** — 65%. That is the merge-conflict number: two tickets dispatched in
parallel have about a 4-in-10 chance of both landing in the same file, and in
practice within a few hundred lines of each other, because both will be adding
methods to the same `impl Runtime` block.

---

## 3. Inside `main.rs`

### 3.1 The modules

*Measured.* `main.rs` declares **147 modules**: **97 file modules** (`mod foo;`,
resolving to a sibling file) and **50 inline `mod … { … }` blocks**. Every one
of the 50 inline blocks carries `#[cfg(test)]`. There is no inline production
module in `main.rs` — the "147 inline modules" in the brief is 97 files plus 50
test modules.

*Measured.* The 50 inline test modules total **59,731 lines**. The largest is a
single `mod tests` at L116318–162953 — **46,636 lines holding 770 of the file's
1,043 `#[test]` functions**. The next largest are `floated_page_tests` (1,503),
`cross_window_drag_tests` (1,014), `tab_identity_tests` (966),
`files_locate_door_tests` (914), `pty_drain_budget_tests` (681) and
`palette_wiring_tests` (601); the remaining 43 are each under 600 lines.

**91 of the tests in `main.rs` read the file as text**: `const SOURCE: &str =
include_str!("main.rs")`. These are the architecture pins — `layer_shape_tests`
destructures `Runtime` out of the source to prove the facade is two layers and
nothing else; `check-portable-core.ps1` parses `FILES_THAT_MAY_NAME_A_PLATFORM`
out of the same file from PowerShell. Nine other modules pin themselves the same
way (`animation.rs`, `attention_codex.rs`, `attention_copilot.rs`,
`attention_hooks.rs`, `diagnostics.rs`, `first_run.rs`, `formula_tools.rs` and
others), each with `include_str!` of its *own* file, which is the pattern that
survives a move.

### 3.2 The top-level items

*Measured.* 1,006 top-level brace items in `main.rs`:

| Kind | Count | Lines |
| --- | ---: | ---: |
| `impl` blocks | 130 | 73,691 |
| inline `mod` (all `#[cfg(test)]`) | 50 | 59,731 |
| free `fn` | 305 | 3,929 |
| `struct` | 199 | 6,139 |
| `enum` | 105 | 2,130 |
| other (`const`, `trait`, `static`, `extern`, blanket impls) | 217 | 7,138 |

The two largest `impl` blocks are both `impl Runtime<'_>`:
**L36598–61746 (25,149 lines)** and **L62188–100525 (38,338 lines)** —
**63,487 lines, 38% of the file, in two blocks**. The next largest is
`impl FolioApp` (2,951) followed by `impl ApplicationHandler<AppEvent> for
FolioApp` (743). Everything after that is under 700 lines.

The largest free functions are `new_window_runtime` (331), `main` (269) and
`drain_leaf_pty` (177).

### 3.3 `Runtime` is a facade; `App` and `WindowRuntime` are the god objects

*Measured.* `Runtime<'a>` (L13938) has **two fields**:

```rust
struct Runtime<'a> {
    app: &'a mut App,
    window: &'a mut WindowRuntime,
}
```

and the file pins that shape with a `const _: fn(Runtime<'_>) = |Runtime { app: _, window: _ }| ();`
plus the source-reading `layer_shape_tests`. So the god object is not `Runtime`.
It is the pair it borrows:

- **`App` (L11015): 78 fields.** The process layer — the `wgpu` device, the four
  background workers and their running/notice flags, the persistence stores, the
  quit transaction, the window list, the drag broker, the quake window.
- **`WindowRuntime` (L11826): 245 fields.** The window layer — everything else.

*Measured.* **1,310 methods hang off the two `impl Runtime` blocks**, totalling
48,879 lines inside method bodies (the remaining ~14,600 lines of the blocks are
the doc comments between them). `impl WindowRuntime` is 38 lines; `WindowRuntime`
has essentially no methods of its own. **All the behaviour is on the facade.**

The twenty largest methods:

| Lines | At | Method |
| ---: | --- | --- |
| 1,099 | L90777 | `mouse_input` |
| 1,019 | L39271 | `refresh_chrome` |
| 1,012 | L89735 | `chrome_mouse_input` |
| 864 | L36610 | `create` |
| 864 | L95303 | `keyboard_input` |
| 682 | L85623 | `pointer_moved` |
| 535 | L94610 | `mouse_wheel` |
| 467 | L60924 | `rebuild_preview_document` |
| 454 | L37507 | `open_window` |
| 446 | L40312 | `rebuild_tooltip_anchors` |
| 446 | L99848 | `turn` |
| 443 | L44501 | `refresh_overlay` |
| 402 | L63998 | `refit_preview_picture` |
| 390 | L62319 | `build_preview_body_in` |
| 373 | L69574 | `file_peek_card_layers` |
| 364 | L79216 | `preview_float_layer` |
| 346 | L82397 | `advance_strip_animation` |
| 333 | L81271 | `apply_math_results` |
| 292 | L64885 | `apply_git_results` |
| 264 | L99570 | `redraw` |

### 3.4 The themes

*Measured for the counts; classification is the name-based sort described in §0.*

| Theme | Methods | Lines | Share |
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

Two rows carry most of the mass and both are single event handlers: **`mouse_input`
+ `chrome_mouse_input` + `pointer_moved` + `mouse_wheel` are 3,328 of the mouse
theme's 5,064 lines**, and `refresh_chrome` alone is 1,019 of `frame & present`'s
2,311. A split by theme is therefore not a split into equal files: five methods
account for 4,600 lines.

---

## 4. The state hubs

*Measured.* For every one of the 1,310 methods, which `self.window.<field>` and
`self.app.<field>` names appear in its body, cross-tabulated against the theme
sort.

- **239 of `WindowRuntime`'s 245 fields are touched from `impl Runtime`.** (The
  other six are written at construction and read elsewhere.)
- **72 of `App`'s 78 fields** likewise.

Distribution of `WindowRuntime` fields by how many themes touch them:

| Themes touching the field | Fields |
| ---: | ---: |
| 1 | 86 |
| 2 | 68 |
| 3 | 25 |
| 4 | 22 |
| 5–9 | 27 |
| 10–13 | 7 |
| 21–25 | 4 |

**86 fields — 36% — are touched by exactly one theme.** Those are privately
owned state that a subsystem could take with it. **38 fields are touched by five
or more themes**; those are the hubs, and they are what a clean cut runs into.

The hubs, worst first:

| Field | Methods | Themes |
| --- | ---: | ---: |
| `window.renderer` | 220 | 25 |
| `window.tabs` | 233 | 23 |
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
| `window.drag` | 22 | 8 |
| `window.tab_scroll` | 14 | 8 |
| `window.modifiers_held` | 10 | 8 |
| `app.gpu` | 64 | 21 |
| `app.settings_store` | 84 | 20 |
| `app.motion` | 90 | 18 |
| `app.profile_programs` | 24 | 11 |
| `app.shortcuts` | 12 | 8 |

Read as an interface problem, these fall into three kinds and only one of them
is hard:

1. **Services** — `app.gpu`, `app.settings_store`, `app.motion`,
   `app.shortcuts`, `app.profile_programs`, `window.renderer`. Read far more
   often than written, and already behind types of their own. These pass as
   `&` parameters or a context struct. Not an obstacle.
2. **Ephemeral input state** — `pointer_position`, `modifiers`,
   `modifiers_held`, `drag`, `seat_pointer`. One writer (the event handler),
   many readers. These pass by value in the event that already carries them.
   Not an obstacle.
3. **The document model** — `window.tabs` (233 methods, 23 themes),
   `window.active_tab` (154 methods, 21 themes), and `window.window` (the
   `Arc<Window>` itself). **This is the real hub.** `tabs` is the tree of tabs,
   panes, seats and their contents; almost every subsystem reads it and a large
   minority mutate it. No subsystem boundary that leaves `tabs` on the far side
   of an interface will hold, and no interface that hands `&mut tabs` to a
   subsystem is a boundary.

Per-theme ownership, from the same cross-tabulation — "own" is fields touched by
this theme and no other, "hubs" is the count of ≥5-theme fields it reads:

| Theme | Own fields | Hub fields needed |
| --- | ---: | ---: |
| preview & documents | 23 | 22 |
| input: mouse & drag | 9 | 33 |
| file peek | 10 | 12 |
| frame & present | 6 | 21 |
| windows & session | 5 | 13 |
| attention & notifications | 5 | 6 |
| panes & layout | 3 | 27 |
| input: keyboard & IME | 3 | 17 |
| git panel & graph | 2 | 15 |
| tabs & rename | 2 | 20 |
| focus cards | 2 | 14 |
| math & formula | 2 | 9 |
| files column | 2 | 10 |
| web seats | 2 | 10 |
| search | 2 | 4 |
| DPI & resize | 2 | 8 |
| floats & menus | 2 | 16 |
| profiles | 1 | 7 |
| settings panel | 0 | 8 |
| terminal & PTY | 0 | 10 |
| first run / integration | 0 | 5 |
| command palette | 0 | 6 |
| quake / summon | 0 | 4 |
| clipboard & paste | 0 | 2 |
| i18n | 0 | 1 |

**`attention & notifications`, `search`, `first run`, `quake` and `math &
formula` are the honest one-way subsystems**: few hub fields, and the hubs they
read are services rather than the document model. **`panes & layout`, `input:
mouse & drag` and `floats & menus` are not subsystems at all** — they are the
orchestrator, which is why they read 27 to 33 hub fields each.

---

## 5. The module graph — the finding that changes the plan

*Measured.* Over `crates/bt-app/src`, with `main.rs` excluded and each
directory-module folded into one node: **99 modules, 242 edges** (`crate::x::…`
in production code, doc links stripped).

**The graph is not a DAG. It has one strongly connected component of 43 modules
and 232,495 lines** — 51% of the crate:

```
attention, attention_codex, attention_copilot, attention_hooks, attention_map,
attention_wire, attention_words, cli, cmdrail, explorer_menu, files, first_run,
focus_thumb, git, git_graph, git_panel, hang_watch, i18n, icons, marks, notice,
persist, preview, preview_edit, preview_provenance, preview_select,
preview_text, preview_undo, profiles, psreadline, quit, recent_folders, restore,
schemes, search, seats, seed, settings, shell_integration, shortcuts, tooltip,
update, webhost
```

Cargo crates cannot depend on each other in a cycle. **Extracting any one of
those 43 modules into a crate means extracting all 43.** That includes every
candidate on the brief's Step-2 list except one: `settings`, `profiles`,
`i18n`, `first_run`, the git panel model and the preview document logic are all
inside it. `formula_tools` is the single named candidate that is outside.

**What is extractable today**, in the sense of "its transitive closure never
enters the component": **33 modules, 24,572 lines (13,910 production)** — 5.4%
of the crate.

| Module | Lines | Prod | Depends on | `bt-*` crates used |
| --- | ---: | ---: | --- | --- |
| `webnav` | 2,669 | 1,205 | — | — |
| `animation` | 2,627 | 1,319 | — | `bt-render`, `bt-transcript` |
| `input` | 1,669 | 734 | — | `bt-platform` |
| `web_thumb` | 1,652 | 856 | — | — |
| `preview_viewport` | 1,321 | 1,221 | — | `bt-render` |
| `preview_wrap` | 1,253 | 576 | `preview_viewport` | `bt-render` |
| `pdf` | 1,224 | 669 | — | — |
| `palette_index` | 1,213 | 604 | — | `bt-platform` |
| `formula_tools` | 1,195 | 426 | — | `bt-render`, `bt-viewport` |
| `preview_watch` | 1,040 | 566 | — | `bt-platform` |
| `preview_viewport_tests` | 775 | 775 | — | `bt-render` |
| `card_trace` | 751 | 369 | `trace` | `bt-layout` |
| `favicon` | 698 | 377 | `webnav` | — |
| `text_field` | 696 | 426 | — | — |
| `settling` | 643 | 392 | — | `bt-render` |
| `preview_typing` | 641 | 641 | — | `bt-render` |
| `git_watch` | 574 | 308 | — | `bt-platform` |
| `wsl` | 495 | 247 | — | `bt-platform` |
| `files_watch` | 456 | 266 | — | `bt-platform` |
| `focus_thumb_restore_tests` | 368 | 368 | — | — |
| `linebreak` | 341 | 234 | — | — |
| `trace` | 309 | 158 | — | — |
| `version` | 306 | 53 | — | — |
| `table_block` | 299 | 132 | — | `bt-detect`, `bt-render` |
| `hex_peek` | 237 | 133 | — | — |
| `preview_trace` | 223 | 190 | `trace` | `bt-render` |
| `dir_news` | 191 | 123 | — | `bt-platform` |
| `watch_clock` | 183 | 115 | — | — |
| `source_pin` | 133 | 97 | — | — |
| `web_trace` | 132 | 132 | `trace`, `webnav` | `bt-platform` |
| `glyph_trace` | 113 | 85 | `trace` | `bt-render` |
| `app_delegate_wire` | 77 | 45 | — | `bt-platform` |
| `attention_trace` | 68 | 68 | `trace` | — |

The remaining 23 modules (`float`, `file_peek`, `palette`, `peek_strip`,
`toast`, `video_seat`, `quake`, `preview_live`, `termscroll`, `menubar`,
`launch_wire`, `keyhint`, `diagnostics`, `cardhint`, `highlight`, `notify`,
`arrival`, `websheet`, `pins`, `mouse_trace`, `context_menu`, `scheme_watch`,
`storage_watch` — 26,692 lines) are outside the component but *downstream* of
it: they can only follow it.

### 5.1 Cutting the component, measured

The component was re-computed with individual modules forced to have no outgoing
edges, to find where the cheap cuts are.

**`i18n` has exactly three outgoing edges, and all three are one function each:**

| At | What | Why it is an edge |
| --- | --- | --- |
| `i18n.rs:6359` | `pub fn colour_name(colour: crate::marks::MarkColour) -> Text` | a nine-arm match from `marks`'s colour enum to a `Text` variant |
| `i18n.rs:6378` | `pub fn profile_entry_fault(fault: &crate::profiles::ProfileFault) -> String` | a formatter over `profiles`'s fault enum |
| `i18n.rs:6554` | one call to `crate::preview::format_pixel_size(width, height)` | a size formatter that lives in `preview` |

Moving those three to the modules whose types they name makes `i18n` a sink.
*Measured effect:*

| Graph | Extractable set | Largest component |
| --- | --- | --- |
| today | 33 modules / 24,572 lines (13,910 prod) | 43 modules / 232,495 lines |
| `i18n` a sink (3 functions moved) | **51 modules / 66,273 lines (36,844 prod)** | 33 / 196,657 |
| + `quit`→`webhost` and `attention`→`attention_wire` cut (2 edges) | 57 / 79,353 (46,158 prod) | 25 / 175,842 |
| + `preview` a sink (8 edges) | 68 / 108,648 (63,134 prod) | 19 / 157,843 |
| + `seats` a sink (13 edges) instead of `preview` | 68 / 156,724 (81,504 prod) | 19 / 106,524 |
| + `seats`, `settings`, `preview`, `profiles` all sinks | 81 / 181,850 (100,450 prod) | 1 / 51,303 (`seats` alone) |

**Three functions move and the extractable set multiplies by 2.7.** After that
cut the dependency-ordered layers are, in order and with nothing skipped:

```
1  i18n                     9,435   (7,074 prod)
2  webnav / animation / input / web_thumb / preview_viewport / pdf /
   palette_index / formula_tools / preview_watch / text_field / settling /
   preview_typing / git_watch / wsl / files_watch / …   (the 33 of §5)
3  shortcuts                6,692   (3,185)
4  update                   1,428   preview_wrap 1,253   seed 1,176
5  favicon 698  card_trace 751
6  hang_watch 2,851   quake 1,707   menubar 1,261
7  persist 2,151   diagnostics 982
8  schemes 1,609   pins 549
9  {icons, marks} as one crate      10,203  (4,570)
10 arrival 768
── the remaining component: 33 modules / 196,657 lines ──
```

`icons` and `marks` are a two-module cycle (`marks` → `icons` for the action-icon
registry, `icons` → `marks` for `ChromeMark`); they extract as one crate, and
after `i18n` is out, that crate's only remaining outgoing edges are to `favicon`
and `webnav`, which are already out. **{`marks`, `icons`, `favicon`, `webnav`}
is a closed group of 13,570 lines.**

---

## 6. Artifacts, and what the gate is actually waiting for

*Measured*, by `ls` on `D:\Developer\BetterTerminal\target\debug` on 2026-09-15.
These are the owner's own artifacts from earlier runs; they are evidence of size,
not of time.

| Artifact | Size |
| --- | ---: |
| `folio.exe` (the product bin, dev profile) | 85.8 MiB |
| `deps\folio-<hash>.exe` (the unit-test harness, most recent) | 209.8 MiB |
| `deps\folio-<hash>.pdb` (the same harness's debug info) | 654.6 MiB |
| the largest test harness in the directory (2026-09-14) | 431.4 MiB |
| `target\debug` total | 74.5 GiB |
| `target\debug\deps` | 72.9 GiB |

**The test harness is 2.44× the product binary and carries a 654 MiB PDB.** Two
codegen-and-link passes over `bt-app` happen in one `cargo test`, and the larger
of the two is the one nobody ships. Linking a single 210 MiB image with 654 MiB
of debug information is a serial step — one linker process, at the end, after
every codegen unit is done — and it cannot be parallelised by adding cores.

*Measured, from `CONTRIBUTING.md` and the scripts.* The gate is three commands
plus six scripts:

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

`cargo clippy` runs under `RUSTC_WORKSPACE_WRAPPER`, which gives workspace
members a **different fingerprint** from the one `cargo test` produced: clippy
does not reuse cargo test's artifacts for `bt-app` and cargo test does not reuse
clippy's. That is the second full pass over the crate, and `--all-targets` makes
it a pass over six targets rather than two.

`scripts/check-shortcuts-table.ps1` and `scripts/generate-shortcuts-table.ps1`
both run:

```powershell
cargo test --package bt-app --bin folio --locked -- --exact shortcuts::tests::docs_shortcuts_md_is_the_bindings_table
```

*Estimated, not measured:* that invocation uses the same profile and the same
flags as the gate's `cargo test --workspace --locked`, so in a clean tree it
should hit the cache and cost seconds. It becomes a third full compile whenever
it runs against a tree the gate's `cargo test` has not just built — which is what
happens when the script is run before the gate, or after an edit, or in a
worktree whose `target` is its own. A reviewer who wants this nailed down should
time `check-shortcuts-table.ps1` immediately after a green
`cargo test --workspace --locked`; if it is seconds, the third compile is a
sequencing accident and not a flag mismatch.

*Measured, from the handoff.* `cargo test --release` on this machine **cannot
compile `main.rs`**: rustc dies with `STATUS_STACK_BUFFER_OVERRUN`. A release
number for the focus-thumbnail budget could not be taken for that reason. The
file is already past what the compiler will do on this machine in one profile.

---

## 7. Everything that is coupled to `main.rs` as a file

Anything that moves `main.rs` has to move these with it. *Measured.*

| Coupling | Count | What breaks |
| --- | ---: | --- |
| `include_str!("main.rs")` in `main.rs`'s own tests | 91 | each reads the whole file as `SOURCE` and greps it; after a split each pin sees only a fragment |
| `scripts/check-portable-core.ps1` | 1 | parses `FILES_THAT_MAY_NAME_A_PLATFORM` out of `main.rs` by regex and throws if the array is not there |
| references to `main.rs` in `docs/DESIGN.md` | 151 | path references in prose; they name the file, not line numbers |
| references to `main.rs` in `docs/` overall | 209 | as above, across plans, handoff and milestone documents |
| `[[bin]] path = "src/main.rs"` in `crates/bt-app/Cargo.toml` | 1 | stays correct if `main.rs` remains the crate root |

The 91 source-reading pins are the single largest mechanical cost in a
theme-split of `main.rs`, and they are not optional: they are what holds
`layer_shape_tests`' claim that `Runtime` has exactly two fields, and what holds
the platform-naming allow-list. The pattern that survives a move is the one the
other nine modules already use — `include_str!` of the *file the test lives in* —
but a pin whose subject is "the whole of `main.rs`" cannot be rewritten that way
without deciding what its new subject is, one pin at a time.

---

## 8. Reproducing this

The scanners are throwaway and live in the session scratchpad, not in the tree.
Each is under 120 lines of Python over the stripped source; the `strip` routine
(comments, strings, raw strings, char literals, preserving line numbers) is the
only part with any subtlety, and any equivalent will reproduce the tables. The
`cargo` invocations are `cargo metadata --no-deps --format-version 1 --offline`
and nothing else. `git` invocations are `git show <rev>:<path> | wc -l` for §2.3
and `git log -150 --format=%H -- crates/bt-app` piped through
`git show --name-only` for the churn number.
