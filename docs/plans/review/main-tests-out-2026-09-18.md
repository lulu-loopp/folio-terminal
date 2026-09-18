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
