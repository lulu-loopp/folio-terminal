# The shapes a source reader takes, counted

2026-09-21. Written against `main` at `3bf7246a`, on the branch `prep/p3-converter`.

`docs/plans/bt-app-split-prep.md` §6.3 hands 474 named-body pins to ticket P3 and
§6.5 hands the rest to P14 and P16. The question this measurement was asked is
whether those readers are two or three repeated shapes that a converter could
rewrite mechanically, leaving people only the remainder.

The answer is that they are — but the repeated shapes are a **minority of the
rows the tickets carry**, and the largest single shape on the list is not a body
pin at all. The measurement is `scripts/dev/bt-app-reader-shapes.py`, which reads
`docs/plans/MIGRATION-DEBT.tsv`, reads the sources it names and the sources those
readings name, and writes one row per debt row to
`docs/plans/bt-app-reader-shapes-2026-09-21.tsv`. It is read-only, deterministic
and re-runnable:

```
python scripts/dev/bt-app-reader-shapes.py
```

Nothing was rewritten. No test was touched.

---

## 1. What is on the P3 / P14 / P16 list

686 rows: 284 for P3, 339 for P14, 63 for P16. They are four different kinds of
thing, and only the first is what the tickets are about.

| Family | Rows | Share |
| --- | ---: | ---: |
| **Body pins** — a named body, sliced out of source and asserted over | 264 | 38% |
| **Whole-file readings** — the scope of the assertion is the file | 128 | 19% |
| **Not a source reading at all** — a bundled asset, a run-time fixture, a ledger key, or no reading found | 250 | 36% |
| **`include_str!` bindings** — the const itself, with no assertion | 44 | 6% |

**This is the first finding.** A ticket sized from the row count is sized wrong
in both directions: 250 of the 686 rows are not readers whose question
`bt-source` answers, and 44 more are bindings that disappear when their
consumers do. The plan's own P3 row already says the bindings go with their
consumers; the other 250 have no such note.

## 2. The shape table

| Shape | Rows | yes | partial | no | What it is |
| --- | ---: | ---: | ---: | ---: | --- |
| `S-no-source-read` | 98 | — | — | 98 | no source-reading construction found in the test |
| `B1-body-contains` | 70 | 46 | 24 | — | a named body contains / does not contain a spelling |
| `N-whole-file-negative` | 68 | — | — | 68 | the whole file is the scope of a negative |
| `L-ledger-key` | 63 | — | — | 63 | a `file_reads_doors.txt` key naming a file |
| `M2-file-and-body` | 61 | — | — | 61 | one test asserts over the whole file *and* a named body |
| `W-whole-file` | 60 | — | — | 60 | the whole file is the scope |
| `F-fixture` | 49 | — | — | 49 | the subject is not Rust source |
| `H-custom-helper` | 47 | — | — | 47 | the finder does its own slicing |
| `C-include-binding` | 44 | — | 44 | — | the `include_str!` binding itself |
| `Q-runtime-file` | 40 | — | — | 40 | a file the test writes and reads back at run time |
| `B5-loop-over-literals` | 25 | — | 25 | — | the selector comes from a written-out array |
| `A-assembled-needle` | 20 | — | 20 | — | the needle is written in halves |
| `R-runtime-selector` | 13 | — | — | 13 | the selector is not decidable at the call site |
| `B3-body-count` | 11 | — | 11 | — | occurrences counted inside a named body |
| `M-mixed-readers` | 7 | — | 7 | — | one test, several finders |
| `B4-body-other` | 5 | — | 5 | — | a named body, asserted some other way |
| `P-assertion-helper` | 2 | — | 2 | — | the reading is inside a shared assertion helper |
| `U-unresolved-selector` | 2 | — | 2 | — | the selector names no single declaration |
| `B2-body-order` | 1 | 1 | — | — | two spellings in a named body, in order |

**Fully mechanical: 47 rows of 686 — 6.8%, and 17.8% of the 264 rows that really
are body pins.** A further 140 rows (20%) are `partial`: a tool can produce the
rewrite and a person must read what it produced. 499 rows (72%) are a decision.

`yes` means a converter can rewrite the site and nobody need read it. It is
awarded only when all of these hold: one local finder, whose end-of-body rule is
one of the two recognised ones and which does no further slicing; every selector
a string literal at the call site; every selector resolving to exactly one
declaration in the file the reading names; and every assertion over the slice a
positive one. The moment a negative, a count, a second reader or an assembled
needle appears, the verdict drops to `partial` or `no`.

The 47 mechanical rows read through six finders: `body` (26), `fn_body` (11),
`method_body` (4), `row_strip_method` (3), `free_fn_body` (2), `method` (1).

## 3. The readers, and why they are not one reader

Across the files this measurement read there are 74 functions that take a
selector and hand back a slice, 270 fixed readings that wrap one, 248 whole-file
readings and 2 assertion helpers. 64 of the 74 finders are in one of two
end-of-body dialects; the other ten have a terminator of their own.

| End-of-body rule | Finders | What the slice is |
| --- | ---: | --- |
| `next-method` — `rest.find("\n    fn ")` | 39 | from after the signature **to the next declaration**: it carries the method's own closing brace, the blank line, and the next item's doc comment |
| `own-close` — `find("\n    }\n")` / `find("\n}\n")` | 25 | to the first `}` at the item's own indentation: the body, unless a nested block closes at that indentation first, in which case it stops early |

`bt_source::Index::body_of` returns neither. It returns the item's body span, so:

* against a `next-method` finder it **narrows** — the trailing brace and the next
  item's prose leave the reading;
* against an `own-close` finder it **widens** wherever a nested block closed at
  the item's indentation and truncated the old slice.

This is not a hypothesis about the helpers. `tests.rs::runtime_fn_body` says it
in its own comment: a slice that ran to the next `fn` "would swallow" the next
item's doc comment, "which is how a paragraph *about* floats standing above the
next function becomes a float in the" body. That one helper says so; the rest
do not, and 25 of the 47 mechanical rows read through one that does not.

## 4. The target forms

Every template assumes one preamble per batch, written once:

```rust
use std::sync::Arc;
use bt_source::{Index, ItemQuery, Vendor, Workspace, report, universes};

/// `bt-app`'s own `src/`, lowered once per process (§3.1, §5).
fn source() -> Arc<Index> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let workspace = Workspace::read(&root).expect("this workspace");
    let package = workspace.package("bt-app").expect("bt-app");
    let universe = universes::crate_sources(package, Vendor::Excluded).expect("bt-app's own src");
    Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}
```

### 4.1 `B1-body-contains`, positives — fully mechanical

*Anchor:* `main.rs::pages_are_plural_tests::a_pane_with_no_engine_has_one_built_for_it`.

```rust
// was
assert!(body("    fn open_web_page_on(").contains(NEEDLE), MESSAGE);

// becomes
let source = source();
let body = source
    .body_of(&ItemQuery::method(TYPE, METHOD))
    .expect("one `TYPE::METHOD`");
assert!(body.contains(NEEDLE), MESSAGE);
```

Placeholders: `TYPE` is the self type of the `impl` the selector's `fn` is
written in, resolved from the file the reading names; `METHOD` is the name out
of the selector; `NEEDLE` and `MESSAGE` are copied unchanged. A free function is
`ItemQuery::function(NAME)` and takes no `TYPE`. **Only the reader expression
changes.** Nothing about the assertion moves.

The script resolves `TYPE` for every mechanical row and writes it in the
`reason` column, so the converter does not have to guess: a name declared twice
in the file is reported as unresolved and the row is not mechanical.

*Equivalence, per §6.0 rule 3.* Beside the old site, for each needle `N` the
site asserts:

```rust
assert_eq!(old.contains(N), new.contains(N), "the two readings answer alike about N");
assert_eq!(old.len().saturating_sub(new.len()), EXPECTED_BYTES,
    "the bytes the old reading carried past the body, recorded rather than assumed away");
```

The second line is the point. The two readings **do not** return the same
string, and an equivalence that only compared answers would hide by how much.
Recording the width difference per site is what lets a reviewer see that a
`next-method` conversion narrows and an `own-close` conversion may widen.

### 4.2 `B2-body-order` — fully mechanical

*Anchor:* `main.rs::git_header_selection_tests::neither_host_seats_the_keyboard_on_the_row_a_press_may_not_land_it_on`
is the shape, though that site is `B5` because its selectors arrive from an array.

```rust
let body = source.body_of(&ItemQuery::method(TYPE, METHOD)).expect("one `TYPE::METHOD`");
let first = body.find(A).expect(MESSAGE_A);
let second = body.find(B).expect(MESSAGE_B);
assert!(first < second, MESSAGE);
```

*Equivalence:* `assert_eq!(old.find(A) < old.find(B), new.find(A) < new.find(B))`,
and both `find`s must be `Some` under both readings — an order assertion where
one side vanished is a finding, not a pass.

### 4.3 The three shapes a tool may propose but not land

`B3-body-count` (11), `B5-loop-over-literals` (25) and `A-assembled-needle` (20).

```rust
// B3 — the reader changes, the count does not, and §4.2 rule 1 gives it a mutation.
let body = source.body_of(&ItemQuery::method(TYPE, METHOD)).expect("…");
assert_eq!(body.matches(NEEDLE).count(), N, MESSAGE);

// B5 — the array carries identities instead of signatures, so two places are edited.
for (item, writer) in [
    (ItemQuery::method(TYPE, METHOD_A), WRITER_A),
    (ItemQuery::method(TYPE, METHOD_B), WRITER_B),
] {
    let body = source.body_of(&item).expect("…");
    …
}

// A — the halves are joined, and §2.6 provenance is what stops the self-match.
let found = source
    .search(&Search::new(needle!(Pattern::call(NAME)), View::Identifiers)
        .in_scope(Scope::Item(ItemQuery::method(TYPE, METHOD))))
    .expect("the scope names one item");
assert_eq!(found.len(), N, MESSAGE);
```

`A` is the one place where the rewrite is not a pure reader swap: joining a
`[..].concat()` needle changes what the test's own text contains, and the
guarantee that the test no longer matches itself comes from `needle!`'s recorded
site rather than from the spelling. That is P18's whole content, and P18 is
unpriced for exactly this reason.

### 4.4 `P-assertion-helper` — the cheapest rows on the list

Two rows, `main.rs::mouse_trace_station_tests::every_chrome_exit_that_takes_the_event_writes_a_line`
and `…::every_exit_of_the_wheels_road_writes_a_line`, read through
`assert_every_return_is_traced` and `assert_every_exit_is_traced`. The reading
and the assertion are both inside the helper, so converting the helper converts
every call site and **the call sites are not edited at all**. §6.0 rule 4 already
covers the sequencing: the shared helper lands alone, fully green, before its
consumers.

Worth saying because it is the pattern the rest of the migration should move
*towards*: the cheapest reader to migrate is the one that exists once.

---

## 5. The categories a tool must not touch, and why

### 5.1 The selector is not decidable at the call site — `R` (13), `U` (2)

`main.rs::palette_wiring_tests::nothing_leaks_past_the_palette_to_the_shell`
passes a selector that is an expression, not a literal; the sites in
`a_page_lands_where_it_was_aimed_tests` pass a variable bound earlier in the
test. A converter cannot know which item is named without evaluating the test,
and a converter that guessed from the nearest literal would aim a guard at the
wrong method while leaving it green.

`U` is the softer version: the selector is a literal, but the name it spells is
declared more than once in the file, or spells no `fn` at all. Two sites in
`main.rs::live_markdown_edit_tests` are in this state.

### 5.2 The helper does its own slicing — `H` (47)

Three sub-kinds, all of which read like a body finder at the call site and are
not one:

* **A file cut before the tests.** `formula_tools.rs::production_body` splits
  `main.rs` at the text `mod formula_tool_seat_tests {` and searches only what
  is before it. That is the text-separator scope §2.3 and P16 exist to replace,
  and the replacement is a named scope chosen per site, not a substitution.
* **A hand-rolled comment stripper.** `tests.rs::method_text` slices to the next
  method and then cuts every line at its first `//`. §2.1 says exactly what that
  does to a line holding a URL. `View::CodeKeepingLiterals` is the replacement,
  and adopting it is a changed view — §4.2 rule 3 gives it its own mutation.
* **A narrowed slice.** `main.rs::layer_shape_tests` reads through
  `struct_fields`, which takes a struct body and keeps only its field lines. The
  subject is not a callable at all, so `body_of` is not the query; this is an
  item query over a `struct`, and whether the guard's concern is the fields or
  the whole declaration is a reading somebody has to do.

### 5.3 The scope is a file — `W` (60), `N` (68), `M2` (61)

The 68 negatives are the ones the plan is most worried about, and rightly. A
guard whose subject was `attention_codex.rs` covered what that file contained;
`Scope::Everything` over `bt-app` covers a great deal more, and `Scope::Module`
covers what the module contains after the move rather than before it. §4.1 says
the expansion is usually right and always an approved line in the ticket. There
is no default a tool could apply.

`M2` is the trap inside this group: 61 rows assert over the whole file **and**
over a named body in the same test.
`main.rs::pty_drain_budget_tests::a_focus_report_is_never_spelled_where_a_keystroke_could_reach_it`
is the plan's own §2.1 example — a run-time-assembled spelling forbidden across
the whole of `SOURCE`, and, four lines later, an order assertion inside
`drain_leaf_pty`. A converter that recognised the body half and rewrote it would
produce a green test, a shrunken debt row and an untouched file-scoped negative
sitting beside it. **That is the exact failure the preparation exists to
prevent, reproduced by the tool meant to prevent it.**

### 5.4 Not a source reading — `S` (98), `F` (49), `Q` (40), `L` (63)

* `S` — 98 rows where the test contains no source-reading construction at all:
  no const in scope, no `include_str!`, no manifest-relative read, no local
  finder. `files.rs` has seven such rows and **no `include_str!` anywhere in the
  file**; its tests spell fixture paths such as `"/src/main.rs"`. `preview.rs`
  has 21 and `git_panel.rs` 15, neither of which reads source in the named
  tests. These look like hits of the debt list's lexical seed on a `.rs` path
  spelled as data. The debt list's header warns that the seed *under*-reports;
  this is the other direction, and it is untested ground. **These rows should be
  read before they are removed** — the list only shrinks by migration, and
  striking a row for the wrong reason is how a real reader would leave the list
  without being migrated.
* `F` — 49 rows whose subject is not Rust: `schemes.rs` includes ten bundled
  JSON palettes, `attention_codex.rs::the_line_this_installs_is_this` includes a
  TOML fragment from `docs/`, `preview.rs` includes corpus documents.
  `bt-source` enumerates a crate's declarations and has no answer for any of
  them; these belong to §6.1's permanent allowlist or to a ruling that they were
  never on the right list.
* `Q` — 40 rows that read a file the test created at run time, mostly the
  `attention_*` installers.
* `L` — the 63 `file_reads_doors.txt` rows. They have no expression to rewrite:
  the first field of each key *is* a file name, and P6/P16 re-key them by hand.
  The ticket that sent them here called them body-pin rows; they are not.

---

## 6. Recommendation

### 6.1 What a tool should convert

**One shape, under one gate.** `B1` positives and `B2`, and only where the
converter has itself proved that the old and new slices are the same bytes or
that the difference cannot reach the assertion.

Concretely, the gate is computable and should be the tool's first job, not its
last: for every candidate site, compute the old finder's span and `body_of`'s
span, and

* **refuse the site** when the spans differ and the assertion is a negative or a
  count;
* **convert unread** when the spans are identical;
* **convert and list for reading** when the new span is strictly inside the old
  one and every assertion at the site is a positive — narrowing a positive can
  only turn green into red, which is loud;
* **refuse the site** when the new span is strictly larger than the old one,
  whatever the assertion. A widened positive passes for a reason it never
  asserted, and nothing goes red to say so.

On today's tree that gate admits at most the 47 `yes` rows and would drop any of
them whose `own-close` finder truncates early. 25 of the 47 read through a
`next-method` finder and are narrowing conversions; 22 read through `own-close`
and are identical-or-widening, which the gate decides per site.

**Everything else is a proposal a person reads.** `B3`, `B5`, `A`, `M`, `B4`,
`U` and `C` — 140 rows — are where a tool earns its keep by doing the typing:
resolving the identity, writing the `ItemQuery`, leaving the assertion alone and
putting the site in a list. It must not land them.

### 6.2 What acceptance the tool's output needs

Per §4.2, and with no sampling relief for anything a tool wrote:

1. **Every count and every negative gets its own mutation check regardless of
   who wrote it** (§4.2 rules 1 and 2), with the negative's injection in all
   four destination classes including a newly created declared `src/runtime/`
   file. A tool-written site is not evidence of anything; it is the same edit
   with a different author.
2. **Every site the tool converted carries its span-difference number** — the
   bytes the old reading held that the new one does not, and the reverse. A
   conversion that reports zero and is right is cheap to review; one that
   reports a number is the review.
3. **A decoy run over the whole converted batch** (§4.2 rule 6): remove the real
   requirement, plant its text in a comment and in a string literal, require
   red. A `next-method` conversion is precisely the case where the old reading
   *could* be satisfied by the next item's doc comment, so this is the mutation
   that proves the conversion improved the guard rather than moving it.
4. **§6.0 rule 5 applies to the tool without exception.** A batch where the old
   and new readings disagree stops, and the disagreement is a finding with its
   own decision. The one thing a converter must never be allowed to do is adjust
   an expected number until the suite is green — and a tool is much better at
   doing that quietly than a person is.
5. **The `S` rows are not the tool's to remove.** 98 rows where no reading was
   found are a question for a person about the debt list, not rows a converter
   may strike because it found nothing to convert.

### 6.3 The risk to weigh before letting a tool rewrite anything

**The 61 `M2` rows.** A converter that pattern-matches on "a body finder with a
literal selector" will find one in every one of them, rewrite that half
correctly, and leave the whole-file negative beside it untouched — in a test
that now looks migrated, in a batch that is green, with its debt row deleted.
The row would leave the list while the file-bound reader it was about is still
there. Every other risk on this page is a conversion that goes red and is
argued about; this one is a conversion that goes green and is not.

The cheap defence is a precondition the tool refuses to run without: **a site is
a candidate only if the test's every reading is the named body.** If any source
const, any `include_str!` or any manifest-relative read appears in the test
alongside the finder, the tool does not touch the test and says why. That check
costs nothing and removes the failure class.
