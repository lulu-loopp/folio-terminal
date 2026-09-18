# `main.rs`'s `mod tests` moved to `src/tests.rs`

Branch `refactor/main-tests-out`, from `main` at `dc99be53`. One commit, no product change.

`crates/bt-app/src/main.rs:123485..171649` — `#[cfg(test)] mod tests { … }` — became
`crates/bt-app/src/tests.rs`, declared where it stood. `main.rs`: 176,152 → 128,011 lines;
`tests.rs`: 48,039. Nothing else in the crate was touched, LF throughout. De-indented by
`rustfmt --edition 2024 crates/bt-app/src/tests.rs`, never by a text de-indent; both files pass
`rustfmt --check` (1.8.0), which `main.rs` also did before the move.

## A2 — proof it is a move

    git show main:crates/bt-app/src/main.rs | sed -n '123487,171648p' > old_body.rs
    diff -u <(sed 's/^[ \t]*//' old_body.rs) <(sed 's/^[ \t]*//' crates/bt-app/src/tests.rs)

81 hunks, all of one kind: lines rustfmt re-joined because they fit inside 100 columns four
spaces further left. A Rust-aware token comparison makes that exact. **6,415 string, byte-string,
raw-string and char literals — identical, every one**, which is the check a blind de-indent fails:
raw and multi-line literals carry their own leading spaces. The token streams differ in 16 places,
all rustfmt's — **12 trailing commas** dropped from lists it put on one line and **2 pairs of
braces** from single-expression closures; dropping those, they are equal.

**A3 — `#[test]` counts:** old `main.rs` 1110 = new `main.rs` 325 + `tests.rs` 785.

## A4 — the pins whose result changed

* `pty_drain_budget_tests::the_window_thread_sends_a_child_bytes_through_exactly_one_door` —
  marker `"\n#[cfg(test)]\nmod tests {"` → `"\n#[cfg(test)]\nmod tests;"`. The cut is the
  declaration, standing where the module stood, so the slice is the same text: count stays **1**.
* `…::the_only_road_from_a_solved_rectangle_to_conpty_is_the_quiet_window` — **7 → 2**: the
  declaration and the one production release. The five test callers are in `tests.rs`. Its
  mutation note, which said "goes to two", now says three.
* `quit_transaction_tests::a_pane_that_closes_is_taken_apart_somewhere_else` — **1 → 0**: the one
  synchronous `pty.shutdown()` in the file was the one its own test drove, and that test left.
  Stricter, not weaker.
* `tests::the_reduction_happens_on_the_worker_not_the_window_thread` (now in `tests.rs`) —
  `SOURCE.split("\nmod tests {").next()` → the whole of `SOURCE`. The separator is gone, so the
  split would have silently widened to the whole file anyway; taking it on purpose is stricter,
  because the fixture that built the second decoder is in `tests.rs` now. Count **1**.

Every value above was recomputed against the tree as committed, not assumed.

## Correction to the brief

Its fourth Observed item is wrong: `tests::no_zoom_gesture_reaches_past_the_door_that_knows_a_video`
does **not** split on `mod tests` — it counts `surface_takes_image_zoom(surface)` over the whole
file, all of whose occurrences are above the module, and is unaffected. The test that splits is
`the_reduction_happens_on_the_worker_not_the_window_thread`, which the brief missed. The rest held.

## Swept, and unverified

Swept for anything the module's text answers: every `.matches()`, `.match_indices()`, `.rfind()`,
`.split()`, `.lines()` over a whole-file slice; every `concat!`/`[…].concat()` needle, evaluated
and counted per region; every literal on a `find`/`contains` line. The two slices that reach
across the module — `before_this_fixture` and `window()` — hold no needle inside it.
`platform_gate_tests` and `scripts/check-portable-core.ps1` walk `src/` and now see `tests.rs`: it
names no platform and `main.rs` still names 11, so that list and both its readers stand. Nothing
was compiled or run here — CI is the gate, and needles are assembled at run time, so this sweep is
a hint: a red test outside the list above is one whose `format!` or slice it did not reach.

# The one it did not reach: `bt-platform`'s gate on `stand_in`

Second commit on the same branch. CI: `bt-app` 4017 passed / 0 failed, identical to `main`; one red,
`bt_platform::native_window_door_tests::a_stand_in_window_is_only_named_by_tests`, on Windows and
macOS. It walks `crates/*/src` and exempts a `stand_in(` call only when it lies inside a
`#[cfg(test)]` module's braces **in the same file**. `tests.rs` holds two such calls and its gate is
on the declaration in `main.rs`, so they read as shipped code.

Two defects, one owner. The fact "this text is test-only" belongs to the gated *declaration*, and a
declaration is `mod x { … }` or `mod x;` — the same statement, written two ways.

1. **A gate ran past its own item.** `test_module_spans` took the next `{` after a gate wherever it
   was. On a gated declaration or a gated `const` that brace belongs to the *following* item, so the
   scan handed out a span over product code and every `stand_in` inside it was exempted in silence —
   an over-exemption, which is the failure a gate cannot show you. A gate's item now ends at the
   first `{` or `;` in code, whichever comes first. On this tree that withdraws bogus spans in 29
   files (`bt-platform/src/lib.rs` 70 → 59, `preview.rs` 15 → 4, `main.rs` 70 → 64); the oldest
   example is `main.rs:232`, where a gated `const` claimed `struct AnimationWork`'s body.
2. **An out-of-line gated module was invisible.** `wholly_test_files` now resolves every
   `#[cfg(test)] mod x;` to the file it names — Rust's own rule, `#[path = "…"]` relative to the
   declaring file's directory, otherwise `x.rs` or `x/mod.rs` under the directory that file owns —
   and closes transitively, because a file compiled only under `cfg(test)` compiles its children
   under it too. A declaration this walk cannot follow now fails the gate by name rather than being
   passed over. Reading is shared: one `code_indices` iterator steps over strings, raw strings, char
   literals and line comments, so a literal spelling `{`, `;` or `mod` answers nothing.

**Guards touched: one.** `a_stand_in_window_is_only_named_by_tests` is the only reader of
`test_module_spans`. The other walk family, `quiet_door_tests::no_command_is_built_outside_the_quiet_door`,
exempts test modules from nothing on purpose (its own note says why) and is untouched and green.
`the_native_window_door_has_no_windows_type_in_its_signature` reads `pub` declarations, not spans.

**Newly classified wholly-test: 8 files**, all in `bt-app` — `tests.rs`, `journeys_tests.rs`,
`preview_typing.rs`, `source_pin.rs`, `attention/tests.rs`, `attention_words/tests.rs`,
`focus_thumb_restore_tests.rs`, `preview_viewport_tests.rs`. Three are reached through `#[path]`.
Each is there because a gated declaration names it; none because of what it is called, and none
declares a child module, so transitivity adds nothing today. **Verdict changes across the whole
workspace: exactly two**, both the `stand_in` calls in `tests.rs`. Nothing that was reported became
exempt: the withdrawn spans of (1) hold no `stand_in` at all, so the change is strictly stricter.

Red gates, each isolated by putting one half of the fix back:
`b';' => None` (the old reading) → `a_gate_reads_its_own_item_and_the_file_it_names` fails on its
first assertion, alone. Switching off the wholly-test skip →
`a_stand_in_window_is_only_named_by_tests` fails naming `bt-app\src\tests.rs:20489` and `:20500`,
which are CI's two lines. Green: `cargo test -p bt-platform --lib -j 4` over
`native_window_door_tests` (5 passed) and `quiet_door_tests` (1 passed); cold build 1m53s, peak well
inside the machine's spare memory. Nothing of `bt-app` was built.

Still unverified: only Windows ran here — the reading is platform-independent and the walk is over
the same tree, but macOS and Linux are CI's. Block comments are still not stepped over by the scan,
as before this change; no file in the tree puts an unbalanced brace in one.
