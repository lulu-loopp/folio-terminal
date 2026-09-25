# The preparation for Step 2a — make every source reader ask the crate, not a file

2026-09-21. Written against `main` at `1f1d2daa`. This is the plan behind
`bt-app-split.md` §6.2's first column: the guard preparation that has to land
before `main.rs` can be split, on the topology that exists today.

`bt-app` pins hundreds of facts about itself by reading its own source text —
`include_str!("main.rs")`, hand-rolled body finders, whole-source counts and
negatives, text-sliced scopes, directory walks. Every one of those readers is
bound to a **file**. Step 2a moves two `impl Runtime<'_>` blocks out of
`main.rs`, and a reader bound to a file does not go red when its subject moves
to another file: it silently reads a smaller universe and stays green. The
preparation replaces the binding. After it, a reader asks the crate a question
about an item, and the answer does not depend on which file the item is written
in. The mechanism is one dev-only crate, `bt-source`, and the work is the
migration of every reader onto it.

Two reviews were run against the design this document condenses. The second
reviewer raised three blocking findings and four should-change findings; all
seven are adopted, and §§2–5 are where they live. Three places where that
reviewer was wrong on a point of fact are in the appendix, each re-verified for
this document.

---

## 1. The scope, ruled

Two scopes were priced side by side, because that was the decision to make.

* **Scope W — workspace-wide.** Before the move, no reader anywhere in the
  workspace names a file, outside a small permanent allowlist. The migration
  debt list reaches zero.
* **Scope N — narrow.** Migrate only what the move disturbs. Everything else
  stays on the debt list and pays its migration when the file it reads is next
  moved.

| | Scope W | Scope N |
| --- | ---: | ---: |
| Reader touch-points (a site can appear twice, e.g. a binding and its call sites) | ~1,120 | ~780 |
| Body-finder call sites | 523 | 474 |
| Module batches | 56 | 40 |
| Tickets | 14 | 13 |
| Agent-hours, as the ticket rows sum | **196–321** | **148–250** |
| Saving | — | ~48–71 h |

The design recommended N, on the argument that the two differ by about thirty
per cent of the hours and by very little of the risk, and that the shorter
window between the mechanism landing and the move landing is worth more than
the hours.

**The ruling, 2026-09-21: Scope W.** The reason is that this preparation is not
overhead attached to Step 2a. It is the foundation every later move stands on —
`seats.rs`, the free functions and `static`/`const` declarations left behind by
a strict two-impl 2a, and the crate extractions of Step 3. Each of those moves
re-opens exactly the rows Scope N would leave open, and re-opens them in a tree
where more readers exist than today. It is done once, completely. Hours order
the tickets; they do not decide whether the tickets happen.

Nothing of Scope N's analysis is discarded. What N would have deferred is
**the second half of the ticket list**: it happens after the move rather than
before it, and it is what carries the debt list to zero.

*Correction to the priced table.* The design stated Scope N's total as 136–248.
Its own rows sum to **148–250**; the W rows sum to 196–321 as stated. The
difference between the scopes is therefore ~48–71 hours, not ~60–73, and it is
the second half of §6's list exactly. That arithmetic is the check on this
document: first half 148–250, second half 48–71, total 196–321.

---

## 2. The reading contract

### 2.1 Four views, none of them a default

The first design made "comments and literals removed" the default reading. That
is wrong, and wrong in the way this work exists to prevent: it would have turned
live prohibitions quietly green. Five readers in the tree today search text
whose subject lives **inside a string literal**:

| Reader | What it forbids |
| --- | --- |
| `tests.rs::the_shell_page_is_gone` | `VideoShell`, `--autoplay-policy`, `mint_player_shell`, `shell_html`, an opening video tag, a player path spelling |
| `main.rs::tab_identity_tests::the_transfer_has_a_door_a_person_can_press_and_no_environment_variable` | the transfer probe's environment-variable name and its two type names, inside the string argument of an environment read |
| `main.rs::pty_drain_budget_tests::a_focus_report_is_never_spelled_where_a_keystroke_could_reach_it` | the four-character focus-report spellings, themselves assembled at run time so the test does not match itself |
| `webnav.rs::no_file_url_is_compared_as_text` | comparison lines containing a file-URL scheme spelling |
| `diagnostics.rs::bt_environment_doc_tests::names_in_source` | requires whole quote-delimited environment-variable literals; the literal **is** the subject |

Under a literal-stripping default all five would have kept returning zero on
today's tree — equivalence would have passed — and stopped detecting the
mutation each exists to detect. The counter-example already in the tree is
`source_pin.rs::code_of`, which strips comments while deliberately preserving
string contents, pinned by
`source_pin.rs::one_item_is_taken_whole_and_its_comments_are_not_its_code`.

So there is no default. A query names one of four views and the compiler
requires it:

| View | What it contains | What it is for |
| --- | --- | --- |
| `View::Raw` | every byte of the enumerated source, unchanged | prohibitions whose subject is a spelling wherever it appears; the reading every whole-source negative takes today |
| `View::CodeKeepingLiterals` | comments and doc comments (including `#[doc]` attributes) removed; **string, raw-string, byte-string and char literals preserved verbatim** | the replacement for every hand-rolled comment stripper in the tree |
| `View::Identifiers` | token positions classified as identifiers, paths and call heads, with identifier boundaries | "is this function called", "is this type named" |
| `View::LiteralValues` | the literals themselves, each carrying **both** its source spelling and its decoded value | any question about what a string *is* rather than how it is written |

`View::CodeKeepingLiterals` is a strict improvement on both strippers in the
tree: `the_shell_page_is_gone`'s line filter drops whole comment lines and sees
neither block comments nor trailing ones, and `platform_gate_tests::code_lines`
cuts each line at its first comment marker, which mangles any line containing a
URL. Neither touches literals, so neither loses coverage; both under-strip.

**Spelling is not value.** An escape spelled in six characters and the three
bytes it decodes to are different facts, and a query says which it means. A
query that conflated them would accept an assembled equivalent — which is
exactly the trick the focus-report test uses to avoid matching itself.

### 2.2 Offsets, removed regions, file boundaries

1. **Byte offsets are preserved.** Every view is a *masked* projection of the
   raw union, not a rebuilt string. A match in any view reports its `View::Raw`
   offset, so two views' results are directly comparable — which is what the
   equivalence commit needs.
2. **No match may cross a removed region.** Masked bytes are opaque, not absent:
   a needle may not span the gap a masked comment or literal leaves.
3. **No match may cross a file boundary.** The union is an ordered
   concatenation with recorded boundaries. A whole-crate count over a union is
   only meaningful if the union cannot manufacture occurrences.

### 2.3 What "test code" is

The fact belongs to the owner of the `#[cfg(test)]` declaration, and a
declaration is `mod x { … }` or `mod x;` — the same statement written two ways —
transitively. `file_reads_source_tests::product_cfg` is already a correct
three-valued evaluator over `not`/`all`/`any` and is taken as written.

One rule the first design did not have: **one physical file can be reached
through both a product and a test declaration, and being reachable from a test
declaration must not remove its product reachability.** An occurrence is
product-reachable if **any** owning declaration path permits product
compilation. The classification is a property of a *path to the byte*, not of
the byte, and the index records the set of paths rather than a single flag.

### 2.4 Stable item identity

"The full module path is unique" is false in this tree. Eleven callable
identities in `bt-app/src` are declared twice under mutually exclusive
conditions — 22 declarations, each confirmed:

| Identity | The two variants |
| --- | --- |
| `attention_copilot::run_probe` | `cfg(windows)` / `cfg(not(windows))` |
| `explorer_menu::read_state` | `cfg(windows)` / `cfg(not(windows))` |
| `files::is_concealed` | `cfg(windows)` / `cfg(not(windows))` |
| `psreadline::run_probe` | `cfg(windows)` / `cfg(not(windows))` |
| `shell_integration::installed_powershells` | `cfg(windows)` / `cfg(not(windows))` |
| `shell_integration::run_profile_probe` | `cfg(windows)` / `cfg(not(windows))` |
| `wsl::<CurrentUser as Registry>::string` | `cfg(windows)` / `cfg(not(windows))` impl blocks |
| `wsl::<CurrentUser as Registry>::subkeys` | `cfg(windows)` / `cfg(not(windows))` impl blocks |
| `hang_watch::run_selftest_if_due` | `cfg(debug_assertions)` / `cfg(not(debug_assertions))` |
| `main::FolioApp::surface_selftest_if_due` | `cfg(debug_assertions)` / `cfg(not(debug_assertions))` |
| `main::panic_selftest_if_due` | `cfg(debug_assertions)` / `cfg(not(debug_assertions))` |

A sweep of every other conditional found no twelfth. **None is a method in the
two moving blocks**, so 2a touches none of them; they constrain the mechanism,
not the move. Identity is the tuple

```
module path  ·  type owner (None for a free function)  ·  trait (None for an inherent impl)  ·  conditional variant
```

with two rules:

* **Expected variant multiplicity is an argument, not an inference.** A query
  states how many variants it expects — one for almost everything, two for the
  eleven above. A different number is a loud failure naming every declaration
  with its predicate.
* **The host platform never selects.** A query never resolves a conditional
  identity by asking which machine is running it. The reading is `cfg`-blind on
  purpose: a leak gated behind `cfg(windows)` must be caught on every runner,
  and a reading that quietly picked the local arm would make the two CI
  platforms disagree about what a guard means.

`method_body(self_ty, name)` narrows to inherent impls whose self type prints as
`self_ty` ignoring lifetimes and generic arguments, so `Runtime<'_>` and
`Runtime<'a>` are the same type. Zero matches panics; more than expected panics,
listing every candidate with its file, position and predicate. `bt-source`'s
real-tree test asserts this exact set of eleven, regenerated rather than copied,
so the day a twelfth appears is a red test.

### 2.5 Declaration exemptions and identifier boundaries

`bt_platform::native_window_door_tests::a_stand_in_window_is_only_named_by_tests`
is the first consumer, and a one-line occurrence count is **not** equivalent to
it. `NativeWindow::stand_in` is product code — a `pub const fn` — and the guard
does two things a substring count does not:

* it **exempts its own declaration**, refusing a match whose preceding text ends
  with the declaration keywords;
* it **checks an identifier boundary**, refusing a match whose preceding
  character is alphanumeric or `_`, with a comment naming why:
  `Runtime::strip_stand_in` and `Runtime::retire_the_stand_in` both exist in
  `main.rs`, and both are methods in the moving blocks.

So the contract carries both explicitly: every identifier query is
boundary-checked on both sides by default (`View::Identifiers` is a token
reading — a substring reading cannot express this), and **declaration exemption
is a named argument** that excludes the spans of the queried item's
declarations, recorded in the report so a reviewer sees what was not counted.
A migration of this guard that does not reproduce both fails its own equivalence
commit on the current tree, which is the desired outcome.

### 2.6 Needle provenance when the caller is outside the queried crate

`crates/bt-term/tests/shell_integration_cmd.rs::the_prompt_this_test_runs_is_the_prompt_the_product_sets`
and
`crates/bt-term/tests/shell_integration_wsl.rs::the_question_this_test_runs_is_the_question_the_product_asks`
both join a relative path onto their own manifest directory and read `bt-app`'s
`shell_integration.rs`. Their caller source names an integration-test target in
a different package and will never resolve into `bt-app`'s enumeration.

1. **Caller-source resolution is separate from the queried enumeration.** A
   needle records file, line, column and call index. If the caller lives outside
   the queried universe — the normal case for a cross-crate reader — that is a
   legitimate, recorded answer, not a panic.
2. **Exclusion is by construction span, not by containing item.** Excluding the
   whole enclosing function silently removes genuine occurrences in a test that
   both constructs a needle and calls the thing it is about;
   `the_press_and_the_hover_ask_one_router` is exactly that shape. Only the
   expression constructing the needle is excluded.
3. **Column and call index disambiguate** two constructions on one line.

The hard failure is a different condition: **if a needle's caller site *is*
inside the queried universe and its construction span cannot be located.** A
self-exclusion that silently excludes nothing is the quiet failure this design
exists to prevent; one that correctly excludes nothing because the caller is
elsewhere is not. `bt-source` carries a real-tree test for each case —
self-exclusion from a `#[path]`-reached file, and non-resolution from an
integration-test target in another package.

### 2.7 Macros: an explicit lexical traversal, loud about what it cannot do

The parser's visitor does not descend into macro token trees: the token-stream
visit has an empty default body, so
`file_reads_source_tests.rs::Doors::visit_expr_call` cannot see a file read
introduced inside a macro invocation today. Lifting that visitor would lift the
hole with it. `bt-app` has exactly two `macro_rules!` definitions —
`psreadline::asset` and `shell_integration::profile_marks::managed_line` — and
neither generates `Runtime` methods, so 2a is unaffected; the mechanism outlives
2a, so its claim has to be wider.

* **Every macro invocation's token tree is traversed lexically.** Tokens inside
  a macro are tokens: they are classified for `View::Identifiers` and
  `View::LiteralValues` and they count. This is the coverage a text search has
  today and it must not be lost.
* **Lexical candidates are distinguished from resolved calls.** A call head
  found inside a token tree is a *candidate*; one found in a call expression is
  *resolved*. Every report says which, and a guard that needs resolved calls
  says so and gets a loud failure rather than a candidate.
* **Unsupported executable macro shapes are reported, never silently
  examined** — an attribute or derive macro that replaces an item body, a
  `macro_rules!` arm that constructs an item, a source inclusion. `bt-app` has
  no source-inclusion, module-path or compile-error invocation today, and the
  single line-number invocation is in
  `first_run::done_with_the_powershell_row_off_removes_nothing_and_says_nothing`,
  naming a temporary directory. Those are facts about today, asserted by a
  real-tree test so the day they stop being true is a red test, not a silent
  gap.

---

## 3. Each reader's universe is its own

### 3.1 A universe is declared, not derived

A module graph rooted at a crate root is not a universe any current guard uses.
Four different universes are in the tree, three of them in one file:

| Guard | Its universe |
| --- | --- |
| `bt_platform::native_window_door_tests::a_stand_in_window_is_only_named_by_tests` | the whole workspace `crates/` tree, recursive, **no directory exclusions**, retained to paths containing a `src` component — so it **includes `src/bin/` targets** |
| `bt_platform::quiet_door_tests::shipped_sources` | `crates/`, retained to `src`, **excluding directories named `bin`, `tests`, `target`**, inline test code deliberately included |
| `bt_app::diagnostics::bt_environment_doc_tests::shipped_sources` | **`crates/` and `vendor/`**, recursive, excluding `bin`, `tests`, `target` |
| `bt_platform::…::crate_sources` | `bt-platform`'s own `src/` only, recursive |

A library's module graph does not contain its `src/bin/` targets, so replacing
the first of those with a per-crate module graph would silently drop every
binary target in the workspace. So a universe is a value the reader constructs,
with four knobs, and no query runs without one:

```
Universe {
    target roots:   which compilation roots (lib, bin, each integration test), per package
    disk scopes:    which directories are read as text regardless of compilation
    exclusions:     directory names never descended (bin / tests / target / …)
    vendor:         included or not, stated
}
```

Product reachability is computed per §2.3. A disk scope with no owning
declaration is neither product nor test; it is `Undeclared`, and a guard that
wants it says so — the platform guards do, because a file that names a platform
and is not yet compiled is still a file someone will compile tomorrow.

**Unresolved or ambiguous module declarations are rejected.** Neither existing
resolver does this: `bt_platform::…::module_file` returns the `name.rs` form if
it exists and never checks whether the `name/mod.rs` form also exists, and
`file_reads_source_tests::scan` does the same and additionally does not retain
inline-module ancestry, so a `mod foo;` nested inside an inline `mod bar { … }`
resolves against the declaring file's directory as if it were top-level. In
`bt-source` both are hard failures: two candidate files for one declaration is
ambiguity; a declaration that resolves to nothing is unfollowed. Neither is a
skip.

### 3.2 A migrated walker ships a file-set diff

**Every migrated walker produces a before/after file-set comparison, and every
difference is reviewed and signed off in the ticket.** Not a count — the sorted
list of paths, old and new, with each added and each removed path carrying a
one-line reason. This is the only evidence that a universe was preserved rather
than approximated, and it is cheap: both readings run in the same process during
the equivalence commit. A non-empty diff is a finding, and the ticket says
whether the new paths are a repair or a widening. No walker is migrated with a
non-empty diff and no explanation.

### 3.3 Three walkers are non-recursive; one of them is losing coverage today

| Walker | Consumer | Subdirectories under its root today | Missing right now |
| --- | --- | --- | --- |
| the inline walk in `bt_app::tests::the_shell_page_is_gone` | itself | `attention/`, `attention_words/`, `shell_integration/` | **yes — three directories, live, today** |
| `bt_render::…::crate_sources` | `every_headless_device_in_a_test_is_taken_through_the_lock` | **none — the directory is flat** | no; latent |
| `bt_layout` `tests/red_lines.rs::sources` | `the_solver_uses_no_floating_point` | **none — the directory is flat** | no; latent |

Stated honestly because briefs have overstated it: **only the `bt-app` walker is
losing coverage today.** The other two are wrong in the same way and will lose
coverage the first time anyone adds a subdirectory to those crates, which
happens without anyone thinking about it. They are cheap latent repairs.

`the_shell_page_is_gone` is different twice over. It is wrong today, and 2a
makes it worse: a `src/runtime/` directory would remove every moved file from
its coverage, and it is the guard whose whole point is that a retired product
mechanism has not come back. **Before proposing its repair, run it recursively
on a scratch branch.** If a recursive reading finds a real violation in one of
those three directories, that is a product finding with its own ticket, and the
preparation ticket must not make it green by narrowing. §4's rule applies: a
difference between old and new is a finding, never a number to adjust.

Already correct and needing only the mechanism: `platform_gate_tests::sources`,
`webnav::no_file_url_is_compared_as_text`, `diagnostics::…::shipped_sources`,
and `bt-platform`'s three.

---

## 4. Ownership is an assertion, and mutation is the acceptance

### 4.1 Owners are executable

Turning a whole-source count into two totals with the owners named **in the
assertion message** weakens three readers that assert ownership today:

* `tests.rs::the_press_and_the_hover_ask_one_router` collects the distinct
  enclosing method names around each occurrence, sorts them, and asserts the
  **exact seven-name set**. Two totals cannot express that.
* `main.rs::layer_shape_tests::every_verb_that_moves_head_is_issued_from_the_one_place_that_asks_first`
  asserts each spelling occurs exactly once *and* that the occurrence is inside
  `checkout_at`.
* `main.rs::focus_mode_door_tests::the_projection_still_takes_its_text_only_from_memory`
  requires one predicate inside `TabState::mini_source` and forbids five names
  *there* — names that are legitimate elsewhere in the program.

So the replacement shape is a set of owner identities with multiplicities,
compared as a whole:

```rust
let calls = source.occurrences(&needle!("float_hit_at("), View::Identifiers);
assert_eq!(
    calls.owners(),                                   // BTreeMap<ItemId, usize>
    expected_owner_set,                               // seven names, each once
    "the press and the hover reach one router",
);
```

`owners()` returns identities in the §2.4 sense, so the assertion is stable
under a move: relocating a caller from `main.rs` to `runtime/panes.rs` changes
no key and no value. A total split by product and tests is still available and
is still right where the old assertion really was a total — but **a total may
not replace an owner set**, and a ticket that proposes one is rejected.

**Preserve the covered concern.** A guard whose subject was `main.rs` covered
exactly what `main.rs` contained. Migrating it to a crate-wide reading *expands*
it to modules it never covered. That expansion may be right — usually it is —
but it is an explicit, approved line in the ticket, never a side effect. Where
the concern is genuinely one item, `Scope::Item` says so and the expansion does
not happen.

### 4.2 The acceptance rule

**Equivalence proves old and new agree on today's tree. Only mutation proves
they will still disagree with the right things on the post-2a tree.**

1. **Every count gets its own mutation check.** Not a sample. A count is a
   number someone will trust.
2. **Every negative gets its own mutation check**, injected into each covered
   destination class: a file directly in `src/`, a file in an existing
   subdirectory, a `#[path]`-reached file, and a newly created, declared
   `src/runtime/` file. Removing the injection must restore green.
3. **Every changed selector or scope contract gets its own mutation check.**
   "Changed" includes a selector that went from a full signature to a name, a
   scope that went from a text prefix to an item, and a view that went from raw
   to code-keeping-literals.
4. **The relocation mutation is mandatory wherever an owner set or a per-owner
   count is asserted:** move a required occurrence from its correct owner to an
   unrelated owner **while keeping every total equal**. The guard must go red. A
   guard that stays green under this mutation is not asserting ownership,
   whatever its message says.
5. **Positives may be sampled only under two conditions together**: the
   conversion across the sampled group is *mechanically identical* (same helper,
   same end-of-body semantics, same view, same scope kind), **and** every site in
   the group has had its selector and its required predicate read and recorded.
   Sampling replaces re-running mutations, not reading.
6. **A decoy test removes the real requirement first**, then plants its text in
   a comment and in a string literal, and requires **red** — the guard must not
   be satisfied by prose. Then restore, and require green. Planting text while
   leaving the real code in place proves only that the guard did not get *more*
   sensitive.
7. **Mutation evidence is scoped to the tree it was taken on.** A merge that
   changes any file a ticket's guards read invalidates that ticket's evidence
   for the affected guards, even when the textual merge is clean. The ticket
   records the base it was taken on.

Mutation acceptance is necessary and not sufficient. It certifies exactly the
failure modes it exercises, and it is complemented by the structural evidence in
§2 (named scopes that cannot degrade), §3.2 (file-set diffs) and §3.1 (universes
declared, ambiguity rejected). A ticket cites all three.

---

## 5. The index, not the trees

The parser's span types are thread-local and not `Sync`, so a shared static
**cannot hold parse trees**, and a thread-local cache would multiply the cost by
the number of test threads. `crates/bt-app/Cargo.toml` already enables the
parser with its `full` and `visit` features as a dev-dependency, which supports
"no new third-party package" and nothing more: a lock file says nothing about
compile cost.

**Parse once per process per root; lower into an immutable, `Send + Sync` index;
drop the parser objects before the index is published.** The index holds only
owned, plain data:

| In the index | Not in the index |
| --- | --- |
| owned file texts, with recorded boundaries in the union | parse trees, items, spans |
| numeric spans (byte offsets into the union, plus file-relative line and column) | any parser-owned span, identifier or token stream |
| item identities per §2.4, with their body spans | — |
| declaration-ownership paths per §2.3, each with its `cfg` predicate | — |
| token classification for `View::Identifiers` (kind, span, boundaries) | — |
| literal records for `View::LiteralValues` (span, spelling, decoded value) | — |
| masks for `View::CodeKeepingLiterals` (comment and doc-comment spans) | — |
| the unsupported-macro-shape report per §2.7 | — |

Every query is then a lookup over plain data, shared by every consumer in the
process.

**The measurement, in P1b, runs before any reader is migrated**, and nothing
depends on an unmeasured assumption. It builds the index for the real `bt-app` —
124 files, 498,804 lines, roughly 23 MB — and reports, on the workspace test
profile on both CI platforms:

| Reported | Budget — exceeding it fails the ticket |
| --- | --- |
| wall time to build the index, cold, single process | **≤ 20 s** |
| peak resident memory of the index, steady state after the parser objects are dropped | **≤ 600 MB** |
| wall time of a representative query set (one body lookup, one identifier count over the union, one whole-source negative, one owner-set query) | **≤ 200 ms total** after the index exists |
| index build cost × the number of concurrent harness processes CI actually runs | reported, with the peak-memory product stated |

The last row decides whether this is viable: ten independent harness processes
multiply CPU work tenfold and, if they overlap, multiply peak memory too. If the
budget is exceeded, the output is a finding and the design changes — the
candidate fallbacks, in order, are lowering the index once and serialising it to
a file under `target/` keyed by content hash, and narrowing the default universe
from the union to the queried crate. **Neither is adopted in advance; the
measurement decides.** CONVENTIONS §八 applies: this is a heavy operation and it
runs on CI or on an idle machine, announced.

---

## 6. The tickets, in the ruled order

The order is: **what blocks the move, then the move, then the rest to zero.**

**The preparation freezes nothing.** Every ticket below lands alone, green on
its own base and re-validated on the merge result, through CI like any other
change. Ordinary feature work continues beside it. The **only** freeze in this
plan is the relocation commit itself, and that freeze belongs to
`bt-app-split.md` §6.5, not here.

### 6.0 Rules every ticket carries

1. **No product code changes.** A ticket that wants to change product code has
   found a bug; that becomes a separate ticket and the preparation does not
   absorb it.
2. **Green on its own base and re-validated on the merge result.**
3. **The equivalence commit**: add the new computation beside the old, assert
   they agree, land; then delete the old. Two commits, so a reviewer sees the
   agreement and a bisect can land between them.
4. **Shared-helper changes are serialised.** `bt-source` itself,
   `file_reads_source_tests::scan`, and each walker helper more than one test
   consumes land one at a time, fully green, before any consumer batch starts
   against them. No two tickets edit the same helper. **Consumers are batched by
   owning module**, one `(file, module)` pair per ticket; a batch deletes that
   module's local helper, converts its call sites, and carries its own mutation
   table.
5. **A batch that finds old ≠ new stops.** The difference is a finding with its
   own decision. It is never resolved by adjusting the expected number until CI
   is green. This is the most important procedural rule in the document.
6. **Two test counts, reported separately.** The count of **pre-existing test
   identities** must be unchanged, compared as identities from `--list` rather
   than as a total; the count of infrastructure tests the ticket adds is listed
   by name. Identities, not totals, because a total can stay equal while
   something moves to the wrong place — the same defect §4.1 is about, one level
   up. `scripts/ci/ignored-tests.txt` stays at its 35 entries;
   `FILES_THAT_MAY_NAME_A_PLATFORM` stays `[&str; 15]` with both readers green.
7. **No new `#[cfg(windows)]` in `bt-app`; no new `#[ignore]`; no
   `cargo fmt --all`** — each ticket formats what it touched and says which.
   Mutation evidence is a throwaway-branch record in the ticket.
8. **A green suite is not acceptance.** Passing is the failure mode this
   document is about.

### 6.1 The two lists

Two lists, and confusing them is what made the first ordering impossible.

| | **MIGRATION-DEBT** | **The file-scoped allowlist** |
| --- | --- | --- |
| What it is | readers not yet migrated | readers whose concern really is a file |
| Lifetime | temporary; reaches zero at P20 | permanent |
| Direction | **only shrinks**; a CI check compares it with the merge base | only shrinks; growth needs a written reason |
| Form | one row per reader: file · owning item · mechanism · subject · the ticket that will migrate it · whether 2a disturbs it | a typed enum variant with a reason in its doc comment |
| Enforcement | the tripwire fails on a reader that is on **neither** list | `Scope::File` is constructible only through a variant |

**Every remaining workspace reader is assigned a row on one of the two lists
before the first migration ticket lands.** A reader with no row is a gap, and
re-counting found three such gaps.

The tripwire, `no_source_reader_names_a_file_outside_the_two_lists`, is a
**plain lexical scan** over the whole workspace for `include_str!` of a `.rs`
file, a string read of a path ending in `.rs`, a `Scope::File` construction, and
a directory read whose root is built from the manifest directory. It is written
**without `bt-source`'s query layer**, so a bug in the mechanism cannot disable
the guard against the mechanism. It is a tripwire, not a completeness proof, and
the debt list says so in its own header. It lands **early**, because everything
not yet migrated is on the debt list and therefore allowed, and every ticket
that lands removes rows.

### 6.2 First half — the mechanism

| Id | Ticket | Hours | Acceptance |
| --- | --- | ---: | --- |
| **P0** | Inventory and disposition ledger: one row for every reader in the workspace, with its mechanism, question, binding, universe, view, scope kind, subject, whether 2a disturbs it and which list it is on. **Hand-read all 96 body-finder call sites whose selector a static classifier cannot resolve** and assign each to the moving or the non-moving set. Refresh `scripts/dev/bt-app-graph.py` and the freshness generator per §6.6. | **5–9** | every reader has a row; the 96 are individually dispositioned with the reasoning recorded; both generators derive their tables from declarations and make an unresolved override key a hard error |
| **P1a** | `bt-source`: universes as declared values, module enumeration with inline-module ancestry retained, **ambiguity and non-resolution as hard failures**, declaration-ownership paths with their `cfg` predicates. | **12–18** | a panic test per rejection class; the four universes of §3.1 reproducible as values; the product-reachability rule of §2.3 tested on a file reached both ways |
| **P1b** | `bt-source`: lower the parse into the immutable `Send + Sync` index of §5, drop the parser objects, and **run the measurement against the stated budget**. | **10–16** | the index is `Send + Sync` and holds no parser type; the four budget rows reported on both CI platforms; exceeding a budget fails the ticket and its output is a finding, not an adjustment |
| **P1c** | `bt-source`: the four views with byte offsets preserved and the three no-crossing rules; identity with conditional variants and expected multiplicity; needle provenance; named scopes; lexical macro traversal with candidate/resolved distinction and an unsupported-shape report. | **12–18** | a panic test for every loud-failure rule; the eleven duplicated identities asserted as a regenerated set; the two real-tree provenance cases; the macro facts of §2.7 asserted as facts about today |
| **P2** | Publish MIGRATION-DEBT and the allowlist, land the tripwire. | **4–6** | the tripwire is written without the query layer and fires on a planted reader that is on neither list; the debt list's CI check refuses growth against the merge base |

### 6.3 First half — the readers the move disturbs

| Id | Ticket | Hours | Acceptance |
| --- | --- | ---: | --- |
| **P3** | **Named-body pins of the moving methods**: 474 call sites in 40 owning `(file, module)` batches, deleting each module's local body finder. The 132 `include_str!("main.rs")` bindings in 17 files are removed by the batches that convert their consumers, not by a ticket of their own. | **60–110** | per batch: equivalence commit, then deletion; a mutation table including the relocation mutation of §4.2 rule 4; end-of-body semantics recorded per helper before it is deleted |
| **P4** | **Counts**: the 18 disturbed whole-source `.matches().count()` sites — twelve in `main.rs`, four in `tests.rs`, two in `journeys_tests.rs`. | **14–22** | every count individually mutated; every raw-versus-view difference reconciled per site and written down |
| **P5** | **Negatives and text scopes**: the six disturbed whole-source negatives plus **all 27 `before_this_fixture` prefix slices**, each decided individually into `Scope::Item`, `Scope::Module` or the product/test ownership reading. | **12–18** | injection across the four destination classes of §4.2 rule 2, including a newly created declared `src/runtime/` file; no text-separator scope survives in a disturbed file |
| **P6** | **`file_reads_doors.txt`**: re-key the 21 rows / 16 distinct owners keyed to a moving method, and the filename-bearing `counted` keys inside the guard. | **5–8** | the guard is green with no row naming a file that moves; owner-name collisions, if any, are a finding with a chosen new key, not a silent merge |
| **P7** | **`the_shell_page_is_gone`**: make it recursive, migrate it, ship its file-set diff. Run it recursively on a scratch branch **first**. | **5–10** | the diff names the three added directories with a reason each; if the recursive reading finds a real violation, that is a product ticket and this one does not narrow to hide it |
| **P8** | The two latent non-recursive walkers — `bt_render::…::crate_sources` and `bt-layout`'s `tests/red_lines.rs::sources` — one ticket. | **2–4** | empty file-set diff expected and shown; a planted file in a new subdirectory of each crate is picked up |
| **P9** | The child-process completion proof: a zero-match child selector must be loud, not silently green. | **2–3** | a deliberately mis-aimed selector goes red |
| **P10** | `scripts/check-portable-core.ps1` and its Rust counterpart: add the agreement test `the_gate_and_its_script_walk_the_same_files`. The script's array reader is **unchanged** — `fn main` can never leave `main.rs`, strict 2a does not move the array, and the script is the one gate that runs in five seconds on a tree that does not compile. It is allowlist entry 1. | *inside P2's 4–6* | both readers green in both directions with the array untouched; the agreement test fails if either walk changes |

### 6.4 First half — the move

| Id | Ticket | Hours | Acceptance |
| --- | --- | ---: | --- |
| **P11** | **The dry run.** Move the smallest topic with real pinned subjects on a throwaway branch off the prep tip. `quake` is the choice: eight methods, `crates/bt-app/src/quake.rs` carries two `include_str!("main.rs")` pins and one whole-source negative, and `quake::the_summon_claim_is_asked_for_on_every_platform_at_every_turn` is a genuine cross-file source pin — a pin in one file whose subject moves to another, the exact shape this preparation exists to neutralise. Confirm against the **final** manifest before committing to it. | **5–8** | the seven steps of §7.3 |

**Correction, 2026-09-22 (after the run).** The sentence above about `quake::the_summon_claim_is_asked_for_on_every_platform_at_every_turn` is wrong: its subject is `FolioApp::settle_quake`, a `FolioApp` method that strict 2a does not move (`MIGRATION-DEBT.tsv` marks the row `2a disturbs it: no`). The cross-file pin that carried the run was `floated_page_tests::a_summoned_window_is_not_shown_by_the_door_that_opens_it` in `main.rs`, which body-pins `show_quake_window` and `hide_quake_window` through `bt-source` and went red under mutation reading `src/runtime/quake.rs`. The manifest's one row of class *subject moves: retarget atomically* (`uninstall_tests::uninstall_source_guard_pins_known_writers_and_inventory`, subject `add_to_profile`) was a name collision — the guard reads `shell_integration::add_to_profile`, a free function — so that class is **empty**; the generator now refuses a subject whose declarations disagree about the move. The run itself, and a second over the `profiles` topic (23 methods), both declared pure under §7.5. Two things the run proved must land before 2a: **P7** (`the_shell_page_is_gone` is blind to `src/runtime/`) and the `Scope::Modules([exact("crate"), tree("crate::runtime")])` re-scoping of `arrival_wiring_tests`' two register gates and `journeys_tests`' root negative.

Then the relocation commit itself, under `bt-app-split.md` §6.5's bounded
freeze. That freeze is the only one.

### 6.5 Second half — after the move, to zero

These are the rows Scope N would have left open. They land after the move, each
alone through CI, each removing rows from the debt list.

| Id | Ticket | Hours | Acceptance |
| --- | --- | ---: | --- |
| **P12** | `bt-platform`'s three walkers and the `stand_in` guard, **one ticket** — they share a file, and splitting them would create the shared-helper collision §6.0 rule 4 forbids. Carries the declaration exemption and identifier boundary of §2.5 and three file-set diffs. | **9–14** | the `stand_in` migration reproduces both the declaration exemption and the boundary check, shown by mutation; three empty-or-explained diffs |
| **P13** | The remaining three source-text walks. | **7–8** | file-set diff each |
| **P14** | The remaining named-body pins: 49 call sites, 13 helpers, 16 module batches. | **20–30** | same per-batch acceptance as P3 |
| **P15** | The remaining three whole-source counts (`persist.rs` twice and `bt-platform`'s `hang.rs`, each reading its own file) and the one remaining negative. | **2–4** | each mutated individually |
| **P16** | The remaining 70 `file_reads_doors.txt` rows, and the **10 text `#[cfg(test)]` splits** in 7 files, each decided individually into a named scope. | *inside P4/P6's ranges — the design does not price the remainder separately* | no text-separator scope survives anywhere in the workspace |
| **P17** | Cross-crate and script readers: the two `bt-term` integration-test readers, `uninstall_tests`' hand-supplied file list, and `context_menu`/`msix` moved to module scope. | **8–12** | the two cross-crate readers exercise the non-resolution path of §2.6; `uninstall_tests` reads a module graph rather than a list |
| **P18** | The 110 `[..].concat()` needle halves written whole. Deferred here deliberately: they exist only to prevent self-match, which §2.6's provenance now does exactly, so this is a readability change with no coverage consequence and ~110 chances to get an edit wrong. | *unpriced* | every converted needle's guard still red under its own mutation |
| **P19** | The CI dependency-direction guard of §8.4, and recording `bt-term → bt-math` as debt. | *unpriced in the design's ticket table* | §8.4's acceptance |
| **P20** | MIGRATION-DEBT closed to **zero**; the allowlist final, ≤ 4 entries, each with a written reason; tripwire green. | **2–3** | the debt list is empty and the tripwire is the only thing standing between the tree and a new file-bound reader |

### 6.6 The two generators

`scripts/dev/bt-app-graph.py` carries two hand-written tables that are stale.
Its `whole_test` set names five files; there are **twelve** wholly-test files in
`bt-app/src`, and the seven missing ones include `tests.rs` — the largest — and
`file_reads_source_tests.rs`, `ime_report_tests.rs`, `present_diagnostics_tests.rs`
and `uninstall_tests.rs`, all four of which contain source readers. Its
`contexts` table hand-corrects four `#[path]`-reached entries and misses those
same four. The freshness generator's `overrides` dictionary and its
`reference_maps()` line lists are keyed to a commit that is now a release old,
so a renamed test silently loses its override — this document's own trap, living
in the tooling everyone will trust.

P0 therefore derives both tables from the declarations instead of listing them,
asserts that the derived `whole_test` set matches what `bt-source` computes (a
disagreement is a finding in whichever is wrong), and makes an unresolved
override key a hard error in both generators.

The twelve wholly-test files, by the declaration that makes each one:

| Declaring file | Declaration | File it makes wholly test |
| --- | --- | --- |
| `main.rs` | `mod tests;` | `tests.rs` |
| `main.rs` | `mod journeys_tests;` (`#[path]`) | `journeys_tests.rs` |
| `main.rs` | `mod preview_typing;` | `preview_typing.rs` |
| `main.rs` | `mod source_pin;` | `source_pin.rs` |
| `main.rs` | `mod file_reads_source_tests;` | `file_reads_source_tests.rs` |
| `attention.rs` | `mod tests;` | `attention/tests.rs` |
| `attention_words.rs` | `mod tests;` | `attention_words/tests.rs` |
| `focus_thumb.rs` | `mod restore_tests;` (`#[path]`) | `focus_thumb_restore_tests.rs` |
| `ime_report.rs` | `mod tests;` (`#[path]`) | `ime_report_tests.rs` |
| `present_diagnostics.rs` | `mod tests;` (`#[path]`) | `present_diagnostics_tests.rs` |
| `preview_viewport.rs` | `mod tests;` (`#[path]`) | `preview_viewport_tests.rs` |
| `uninstall.rs` | `mod tests;` (`#[path]`) | `uninstall_tests.rs` |

**A thirteenth since 2026-09-24** (0.4.5 ticket 37): `main.rs` declares `#[cfg(test)] mod text_size_tests;`, which makes `text_size_tests.rs` wholly test. `bt-source`'s `the_wholly_test_files_of_bt_app_are_the_thirteen` names it.

**Twelve again since 2026-09-25** (0.4.6 census-3): `attention.rs` and `attention/tests.rs` left `bt-app` for `bt-workbench` (D-57), so the table's `attention.rs` row no longer describes `bt-app`. The test is now `the_wholly_test_files_of_bt_app_are_the_twelve`.

---

## 7. The end measurement

### 7.1 Regenerated inventory

Run at the prep tip, not carried from P0:

1. `scripts/dev/bt-app-graph.py` and `scripts/dev/bt-app-split-freshness.py` on
   the **same** tree. The freshness generator already asserts that the graph's
   `main.rs` line count and method set agree with its own, and that the working
   tree matches the git blobs it reads, LF-normalised. Those assertions stay.
2. Both generators' hand-written tables refreshed per §6.6, with an unresolved
   key a hard error.
3. The date and stem redirected so a read-only run writes nothing into
   `docs/plans/`.
4. The theme regex list's `unassigned` method count recorded — 2a's
   destination-map problem, not the preparation's, but the regeneration is the
   moment to write it down.
5. The reader inventory re-emitted as a TSV: one row per reader, with mechanism,
   question, binding, universe, view, scope kind, subject, the subject's 2a
   destination, whether the subject moves, and which list it is on.

### 7.2 List state

MIGRATION-DEBT is **empty** (P20). The allowlist has at most four entries, each
with a written reason, and entry 1 is `scripts/check-portable-core.ps1`'s array
reader. The tripwire is green.

### 7.3 The dry run (P11)

1. Create `src/runtime/mod.rs` and `src/runtime/quake.rs`; declare
   `mod runtime;`; move the eight methods with only the visibility and import
   edits they need. Nothing else. **Dependency cleanup is not in this commit**
   and not in this comparison.
2. `cargo test -p bt-app --bin folio -- --list` before and after. The two lists
   must be identical as **identities**: same names, same multiplicity, same
   `#[ignore]` set. A leaf-name set is not enough.
3. Full workspace CI after the move, every verdict recorded. Acceptance: **no
   test changed its verdict and no test's text was edited by the relocation
   commit.** A guard that needed an edit to stay green was not migrated — name
   it, open a ticket.
4. **Re-run the mutation tables** of every guard whose subject moved. This is
   the only step that catches a guard that was green before and is green after
   for a different reason.
5. `platform_gate_tests::only_the_named_files_decide_what_platform_this_is` and
   `scripts/check-portable-core.ps1` both green with the array untouched — the
   proof that strict 2a moves no platform `cfg`. Plant a needle in
   `runtime/quake.rs` and require `the_shell_page_is_gone` to go red — the proof
   that its repaired recursion reaches a new directory.
6. Confirm no guard still on the debt list changed verdict. If one did, its row
   was misclassified and the boundary is re-drawn before 2a proper.
7. Throw the branch away.

### 7.4 Body comparison, kept separate

The ordered, literal-preserving token comparison of each moved body is **its own
audit** and reports its own result. It is not mixed with, and does not excuse,
the three audits beside it:

| Audit | Question |
| --- | --- |
| **Body comparison** | is each moved body token-for-token what it was, literals preserved, with every permitted change named individually rather than waved through by a regex exemption? |
| **Declaration comparison** | is each moved declaration verbatim apart from the one permitted visibility prefix — **allowing rustfmt to re-wrap a signature that the prefix pushed past 100 columns** (two of the 23 `profiles` signatures did; the re-wrap is reported by name, never waved through)? |
| **Import and visibility** | which `use` bindings changed, which items gained `pub(crate)`, and does any of it change resolution — a name that resolved to one item at the crate root and another inside `runtime::`? |
| **Inherited attributes** | what did each item inherit from its old module that its new module does not supply, or supplies differently — `#![allow]`, `#[cfg]`, lint levels? |
| **Relative inputs** | every `include_str!`, `include_bytes!` and `#[path]` in a moved item, whose resolution is relative to the *declaring file* and therefore changes when the file changes. |

### 7.5 The declaration

2a is a pure move when, and only when: `--list` identities are identical
including multiplicity and the ignored set; every migrated guard's verdict is
identical **and** its mutation table still holds; no `#[cfg(test)]` text outside
the moved items changed; the allowlist did not grow and the debt list did not;
`FILES_THAT_MAY_NAME_A_PLATFORM` kept its arity with both readers green in both
directions; and §7.4's four audits each pass on their own terms. Not a
byte-identical binary and not an IR comparison — a moved item's module path is
part of its symbol name.

---

## 8. Dependency edges

### 8.1 `bt-pty → bt-term` is not deletable as stated

`crates/bt-pty/Cargo.toml` declares `bt-term` in **both** `[dependencies]` and
`[dev-dependencies]`, and the only `bt_term` use in `crates/bt-pty/src/lib.rs`
is inside `#[cfg(test)] mod tests`. But
**`crates/bt-pty/src/bin/bt-conpty-width-probe.rs` uses
`bt_term::DualPlaneSession` in its ordinary `fn main`**, and a `src/bin/` target
links against the package's *normal* dependencies, not its dev-dependencies. So
the edge cannot simply be deleted; deleting it leaves a broken binary target.
This is the same class of mistake §3.1 is about — a package is more than its
library's module graph.

Three faithful alternatives, **and this plan does not choose between them. The
choice is a ticket of its own (P21), outside the preparation and outside the
relocation commit, before or after either — never inside a comparison that is
supposed to show a pure move.**

1. Move `bt-conpty-width-probe` to `bt-corpus`, which already depends on
   `bt-pty` **and** `bt-term`. The graph loses the edge, no code is rewritten,
   and the probe keeps working. Verified by building the moved target, not by
   reading the manifest.
2. Make the probe's need a feature, so the normal dependency is optional and the
   default graph does not carry it.
3. Accept the edge and record it as debt with that reason.

What is **not** an option is deleting the normal dependency and leaving a broken
`src/bin/` target.

### 8.2 Lifting `file_reads` does not remove `bt-term → bt-platform`

`crates/bt-term/src/inline_image.rs::resample_pool` builds a worker pool whose
start handler sets a thread priority through `bt-platform` — product code, on
the decode path. A second product edge is in the same crate:
`crates/bt-term/src/session.rs::verify_path` calls
`bt_platform::resolved_for_a_door`. So the edge survives lifting the file-read
ledger, and the ledger itself must keep one owner: `Ledger::add`, `Lane`, the
wrappers and the process-wide static are read by `bt-app`, `bt-render`,
`bt-term`, `bt-persist` and `bt-math`, and splitting them would be a second copy
of a fact.

Smallest faithful form, **all of it 0.5 work**, because both halves change
product code, which §6.0 rule 1 forbids in a preparation ticket, and both are
composition-layer design:

* **Worker priority becomes an injected policy.** The pool builder takes a start
  handler supplied by whoever constructs the decode pool, and the application
  supplies the platform one at its own boundary. Nothing about the running
  program changes.
* **Path-verification orchestration moves up; the function does not move.**
  CONVENTIONS §十 rule 9: when work moves to another thread or crate, the
  original function moves with it or is shared — it is not rewritten.
  `bt_platform::handoff::resolved_for_a_door` combines canonicalization with
  verbatim-prefix stripping and is the single answer; `bt-term`'s `verify_path`
  and `bt-app`'s `run_path_verify_worker` both consume it. The orchestration may
  be hoisted to the application so `bt-term` stops calling it directly; the disk
  work stays off the window thread and the result must be bit-identical.

One loose end found while reading: `session.rs`'s `opening_it_would_run_it` has
a doc comment saying `bt_platform::names_a_program` answers the question on
non-Unix, while the `cfg(not(unix))` arm returns `false` unconditionally and
`names_a_program` is never called in `bt-term`. A documentation defect with its
own small ticket, not part of this work.

### 8.3 `bt-term → bt-math` is recorded debt

Not mechanical cleanup and not attempted. `session.rs` imports six math types
and calls `bt_math::key_for_em_px` in product code;
`inline_image.rs::decode_svg_bytes` calls `bt_math::rasterize_svg_document`;
`crates/bt-term/src/lib.rs` re-exports `MathEngine`; and
`crates/bt-term/src/bin/bt-repaint-oracle.rs` uses it in a binary target — the
same binary-target trap as §8.1. **Recorded as explicit debt until the
composition layer of §9 is designed.**

### 8.4 The CI direction guard

A small script over
`cargo metadata --no-deps --format-version 1 --locked --offline`, reading
**normal and build dependencies, including target-specific tables and renamed
packages**, checking them against an allowed-direction list, with the exception
set **compared against the PR's merge base so it can only shrink** and **stale
exceptions rejected**. Dev-dependencies are judged separately against their own
list. No compilation, and no resolution limited to the current platform — which
is what catches a target-conditional dependency table on a machine that is not
that platform.

**One limit, stated because §8.1 is an instance of it:** `cargo metadata`
reports dependencies per *package*, not per *target*. It cannot tell that
`bt-pty`'s normal `bt-term` edge exists only for a `src/bin/` target. The guard
therefore pairs the metadata reading with a per-target scan of which targets
reference which workspace crate, so an exception can say "normal dependency,
used only by target X" and be checked rather than believed.

### 8.5 Where each piece sits

| Item | Where |
| --- | --- |
| The CI direction guard (§8.4) | **Second half — P19.** It is cheap and independent of `bt-source`, and it is what keeps the direction from drifting. It moves to the first half only if the move turns out to need it; nothing in §6.3 does. |
| Recording `bt-term → bt-math` as debt (§8.3) | **Second half — P19.** It is a line in a file. |
| Choosing among §8.1's three alternatives | **P21 — its own ticket, outside the preparation.** |
| Injecting the worker policy; hoisting path-verification orchestration (§8.2) | **0.5.** Both change product code. |
| **The freeze window** | **Nothing dependency-related.** The freeze belongs to the relocation commit alone. The whole point of a pure move is that it changes no dependency and needs no dependency change to land. |

---

## 9. What the move should measure for 0.5

The 26-topic destination map is a file-assignment exercise; it will not locate a
composition boundary. What 2a should record, while it is touching every method
anyway, is **method-level evidence, before any aggregation by topic**:

* per call: caller, callee, receiver and type owner, conditional predicate,
  call-site identity, and whether resolution is certain or only a lexical
  candidate (§2.7);
* per field: reads, writes, shared and mutable borrows, escaping references, and
  whether the access is direct or through a helper;
* per effect: worker queues, filesystem calls, native and window calls, renderer
  operations, clocks, cancellation, completion routing;
* per candidate boundary: the data that crosses it — types, sizes where they
  matter, generations and revisions, invalidation rules, required ordering;
* per boundary: the tests and source pins that cover it.

**The trap that will corrupt the field census if it is not handled first:**
`main.rs` declares `impl Deref for Runtime<'_>` and `impl DerefMut for
Runtime<'_>` with `type Target = TabState`, resolving through
`active_item`/`active_item_mut` on the active tab. A census that reads every
`self.foo` in an `impl Runtime` method as a field of `Runtime` or
`WindowRuntime` will be wrong for every field that actually belongs to
`TabState`, and wrong in the direction that makes `Runtime` look like the owner
of state it only borrows. The census must resolve each `self.` access to
`Runtime`, to `WindowRuntime`, or through `Deref` to `TabState`, and say which.

**The first composition boundary already straddles the crate line.**
`bt-term/src/session.rs::DualPlaneSession::viewport_frame` syncs live worker
artifacts into the projection, hands it the printed-path link data, builds a
continuous frame from the document, the staged rows and the visible rows with a
cursor and the terminal modes, applies math and image-reference decoration, then
validates the frame's shape. `Runtime::publish_frame_inner` in `main.rs` adds
the application half: carrying live journeys forward, settling the PTY coalesce
debt, a debug tripwire on seat and shell revision consistency, the chrome-only
short circuit for a tab with no shell, decoration-state tracing, releasing the
presentation hold on keyboard input, re-running search over the frame about to
be drawn, and dispatching tab decoration work. **Measure both sides.** A 0.5
plan built from the `Runtime` topic graph alone cannot see that half of its
first extraction is already in another crate.

For the 0.5 subsystems themselves: expose explicit input/state/output contracts
from library crates with no dependency on `Runtime`, on window ownership, or on
native event-loop types; inject effects and clocks at the application boundary;
and for the composition layer, keep the existing `ViewportFrame` shape and its
revision invariants, making native workers and rendering adapters clients of
that layer rather than authors of it.

---

## Appendix A — where the review is wrong on a point of fact

Three items. None changes a verdict; each changes a sentence a ticket would
otherwise be written from. All three were re-verified against the tree for this
document.

**A-1. The `bt-pty → bt-term` deletion is blocked by a binary target.** The
reviewer said the import is inside `#[cfg(test)] mod tests`, a dev-dependency
already exists, and the normal dependency should be removed. The first two are
correct. The third is not: `crates/bt-pty/src/bin/bt-conpty-width-probe.rs`
opens with `use bt_term::DualPlaneSession;` and constructs a `DualPlaneSession`
inside its ordinary `fn main`, and a `src/bin/` target links against the
package's normal dependencies. Deleting the edge breaks the probe. §8.1 gives
the faithful alternatives.

**A-2. `webnav::no_file_url_is_compared_as_text` filters on a two-slash
spelling.** The review's supporting list says the test searches comparison lines
containing the three-slash file-URL spelling. It does not: the line filter is
built as a two-part `concat!` producing the seven-character scheme-and-authority
prefix, and the three-slash spelling appears only inside the single expected
line that the final `assert_eq!` compares against. The finding is unaffected —
the needle still lives inside a string literal and literal removal would still
defeat the guard — but a ticket written from the review's wording would look for
the wrong needle and conclude the test had been changed.

**A-3. Two of the three non-recursive walkers are losing nothing today.** The
review correctly reports that `bt_render::…::crate_sources` and `bt-layout`'s
`tests/red_lines.rs::sources` are non-recursive. But `crates/bt-render/src/` and
`crates/bt-layout/src/` are both flat — no subdirectories — so neither walker is
currently missing a file. Only `bt_app::tests::the_shell_page_is_gone` is losing
coverage today, and it is losing three directories. This matters for sequencing:
the `bt-app` walker (P7) is the one walker ticket that cannot slip, while P8's
two are cheap latent repairs that can ride with anything. Any brief that says
"three walkers are losing coverage today" is overstating two of them.

*Everything else the review asserts was checked and holds*, including: the
eleven duplicated conditional identities (all 22 declarations confirmed, no
twelfth found); the twelve wholly-test files; the exact `commit_leaf_resize`
accounting (one definition, one product call, six test call sites);
`NativeWindow::stand_in` being a product `pub const fn` with a declaration
exemption and an identifier-boundary check that exists specifically because
`Runtime::strip_stand_in` and `Runtime::retire_the_stand_in` would otherwise
match; the two `bt-term` integration-test readers joining a relative path into
`bt-app`'s source; the exact two `macro_rules!` definitions in `bt-app/src`; the
parser's empty token-stream visit; `bt-app/Cargo.toml` enabling the parser's
`full` and `visit` features for tests; 124 files and 498,804 lines under
`bt-app/src`; the four distinct source universes; `module_file` preferring the
`name.rs` form without rejecting the ambiguous case;
`file_reads_source_tests::scan` not retaining inline-module ancestry;
`uninstall_tests` parsing a hand-supplied list rather than a module graph;
`bt-app-graph.py`'s manually specified `whole_test`/`contexts`;
`impl Deref`/`DerefMut for Runtime` targeting `TabState`; and
`resolved_for_a_door` combining canonicalization with verbatim-prefix stripping
while `names_a_program` is mentioned in `bt-term` documentation and never called
there.

---

## Appendix B — the counts this plan is priced from

Re-counted on `main` at `1f1d2daa`.

| Measurement | Value |
| --- | ---: |
| `crates/bt-app/src/main.rs` physical lines | 131,328 |
| The two `impl Runtime<'_>` blocks | two, contiguous, no platform `cfg` inside |
| Methods in those blocks | 1,390, all distinct names |
| Combined block lines | 68,206 |
| Files / lines under `crates/bt-app/src` | 124 / 498,804 |
| Wholly-test files in `bt-app` | 12 |
| `#[test]` attributes under `crates/bt-app` | 4,170 |
| `scripts/ci/ignored-tests.txt` non-comment entries | 35 |
| `FILES_THAT_MAY_NAME_A_PLATFORM` | `[&str; 15]` |
| `include_str!("<x>.rs")` invocations | 316, in 42 files, naming 83 distinct files |
| — of which name `main.rs` | 132, in 17 files |
| Locally-defined body finders | 53 definitions |
| Their call sites | 523, across 56 owning `(file, module)` pairs |
| — selector names a **moving** method | 348 |
| — selector names another function | 79 |
| — **selector not readable by a static classifier** | 96 |
| — owning pairs containing at least one moving site | 40, holding 474 sites (91%) |
| End-of-body needles | 111 |
| Whole-source `.matches().count()` over a file-bound const | 21, in 5 files |
| Whole-source negatives | 7, in 4 files |
| Text `#[cfg(test)]` exclusion by splitting | 10, in 7 files |
| `before_this_fixture` prefix slices | 27, all in `main.rs` |
| `[..].concat()` needle halves | 110 |
| Source-text directory walks | 9, of which 3 non-recursive |
| Parser-based source consumers | 3 |
| PowerShell gates that read `.rs` text | 5, of which 1 names `main.rs` |
| `file_reads_doors.txt` rows | 91, of which 29 keyed `main.rs`, 21 keyed to a moving method |
| Duplicated conditional callable identities | 11 identities, 22 declarations |

The 96 unresolved selectors are why P0 is priced above a pure inventory: they
are needles assembled at run time that a static sweep cannot enumerate, and any
one of them may name a moving method. They are hand-read, not deferred.

Compile-time guards outside the reader taxonomy, which the tripwire does not
see and which P0 gives rows of their own: the anonymous exhaustive `Runtime`
destructuring const in `main.rs`, `animation.rs`'s `is_send` checks, and the
anonymous assertions in `seats.rs`, `settings.rs` and `peek_strip.rs`.

---

## Appendix C — 2026-09-22: Step 2a landed, and the end measurement (§7.1, §7.2, §7.5)

Branch `prep/move-2a` from `main` at `b031cfd2`; one commit per topic, each
compiling and passing its gate on its own, then one narrowing commit
(§6.5 step 5), then one import commit. The per-topic record, with every
`pub(crate)` count, `use` line and rustfmt re-wrap by name, is kept outside
the tree, in the coordinator's trace (`move-2a-report-2026-09-22.md`).

**What moved.** 25 of the 28 topics — **1,195 methods** — into
`src/runtime/*.rs`; `runtime/mod.rs` declares 25 modules and imports nothing.
`main.rs` went from 133,794 lines / 6,732,197 bytes to **74,203 / 3,689,978**.
The two `impl Runtime<'_>` blocks keep **198** methods: the **112**
unassigned ones (not moved in 2a, by the brief) and three topics that could
not be a pure move:

| Topic | Methods | Why it stayed |
| --- | ---: | --- |
| `launch` | 4 | `shell_integration::tests::shell_integration_startup_and_removal_doors_are_above_window_work` reads `include_str!("main.rs")` for `shell_integration::begin_startup_migration();`, which `Runtime::create` calls. |
| `settings` | 31 | `focus_mode_door_tests::only_the_chord_and_the_settings_row_write_the_bit` compares the `self.set_focus_mode(` lines in **universe order** against a fixed-order list; `runtime/settings.rs` reads after `main.rs`, so the two doors come back reversed. |
| `focus` | 51 | `focus_mode_door_tests::the_cards_offer_is_spent_in_one_place_and_given_back_in_one`, the same shape over `settings.cards_gesture_hint_offer =`. Moving settings and focus together fixes this one and still breaks the other. |

Each is a reader the move turns red, which by §6.0 is the reader's defect,
not the move's: a file-bound `include_str!` positive, and two sets compared
as ordered lists. They need rows of their own before those topics can move.

**Manifest corrections applied** (item-level rule over the theme regex):
`apply_quake_profile` → quake; `drag_preview_text`, `release_preview_text`,
`press_preview_text` → preview; `spend_preview_press`,
`preview_press_keeps_the_caret_seat`, `drop_preview_selection` (newer than
the 2026-09-21 manifest) added under preview.

### §7.1 — the inventory, regenerated

`scripts/dev/bt-app-graph.py` and `scripts/dev/bt-app-split-freshness.py` on
the same tree (the branch head), the second with the new `--out` redirected to
`target/inventory-2a` so nothing was written into `docs/plans/`. Both of the
generator's working-tree assertions held. The regenerated manifest has **198**
rows — the residue — of which the theme regex leaves **112 unassigned**, the
same number as before the move. Census: **250** reader rows, all `existing`;
impacts 197 "no 2a subject move identified", 38 "fixture/manifest input
retained", 7 "retained subject: no 2a move", 4 "retained source file read
dynamically", 2 "recursive enumeration: coverage retained", 2 "enumeration:
audit recursion and original scope". Parsed `#[test]` attributes: 4,244 before
and after. No `include_str!`/`#[path]`/`file!()`/`module_path!` moved (the
mover's refusal never fired), and no platform `cfg` moved.

### §7.2 — list state

MIGRATION-DEBT: **280 rows before, 280 after, 0 added, 0 removed** — the
second half (P20) has not run, so the list is not empty; 2a moved none of its
subjects' rows. The allowlist is `bt_source::FileScoped`, **three** entries,
unchanged. The tripwire is green. `FILES_THAT_MAY_NAME_A_PLATFORM` still has
15 entries and both platform readers are green.

### §7.5 — the declaration

For the 25 topics that moved, 2a is a pure move:

* `cargo test -p bt-app --bin folio -- --list` at the head is **identical** to
  `b031cfd2`'s — 4,243 names, same order and multiplicity — and
  `--list --ignored` is identical (10). The whole bin suite passes (4,233 + 10
  ignored); `cargo test --workspace --exclude bt-pty --exclude bt-render` and
  `cargo clippy -p bt-app --all-targets -- -D warnings` pass.
* Every guard's verdict is unchanged, and the one guard edited — the two
  register gates' scope in `arrival_wiring_tests`, flipped in the first move
  commit to `[exact("crate"), tree("crate::runtime")]` with its executable
  reminder deleted, as the brief requires — keeps its mutation: a `.settling`
  reader planted in `runtime/quake.rs` turns the settling gate red, naming the
  planted method. That is the only `#[cfg(test)]` text changed outside the
  moved items. (Two crate-root imports that only `tests.rs` still reads gained
  `#[cfg(test)]`; that is a `use` declaration, not test text.)
* The allowlist did not grow and the debt list did not change.
* §7.4, each on its own terms:
  **bodies** — 1,195 / 1,195 byte-identical to the pre-move blob
  (`bt-app-move-topic.py --check` per topic against its parent);
  **declarations** — verbatim apart from the visibility prefix, except **102**
  that rustfmt re-wrapped after the prefix pushed them past 100 columns, each
  named in its topic's commit;
  **imports and visibility** — each topic file imports only crate-root
  bindings, so no name can resolve differently from the root; 14 trait imports
  came from rustc's E0599 suggestion, always the candidate the root itself
  imports; 9 imports rustc reported unused were removed (7 root imports whose
  last user moved, 2 over-imports in topic files); visibility ends at 251
  `pub(crate)`, 635 `pub(in crate::runtime)`, 309 private, with 145 kept
  `pub(crate)` above the manifest column because a witnessed caller is still
  in `main.rs`;
  **inherited attributes** — `main.rs` carries one inner attribute,
  `#![windows_subsystem = "windows"]`, which is crate-level and reaches every
  module alike; no moved item depended on an outer attribute of its old
  position;
  **relative inputs** — none moved.

The declaration does not cover `launch`, `settings` and `focus`, which did
not move.
