# Focus follows a dropped path: round 2, 2026-09-17

Reviewed `10c1f10e` against round 1 (`a5938a22`), baseline `d1db4abd`.
References below mean `crates/bt-app/src/main.rs` at 10c1f10e unless named.
**Verdict: merge with must-fixes; do not merge unchanged into 0.4.2.**
Bar (a), retained-address drop protection: passes the requested comparison.
Bar (b), keyboard/composition following the accepted drop: still incomplete.

## The four round-1 findings

- **P1 keyboard owners: closed in source.** `:54998` releases files focus,
  `:55002` leaves the editor, `:55005` blurs search, `:55008` blurs floats,
  then `:55011` focuses the seat. The actual owner reads those states
  (`:60746`, `:55860`); graph search loses its preview keyboard surface too.
- **P1 modal/false aim: closed.** `:18039` includes menus, cards, rename,
  git prompts and palette; `:96865` and `:96792` gate collection and flush.
  `:90753` adds internal admission; `:114935` removes the focused-seat fallback.
- **P2 PTY acceptance: closed.** `:18664` returns Queued/Refused/NoChild;
  `:98875`, `:98893`, `:98900` propagate only Queued as success.
  `:18644` keeps the swallowing wrapper via `map(drop)`; queued is not consumed.
- **P2 preedit: partly closed, must-fix remains.** `:55105` guards a real
  shell-seat change and `:55109` requests cancellation before `:55111` moves it.
  That does not guarantee the old composition cannot subsequently reach B.

## Requested attacks (source traces; no native reproduction)

Dirty editor: `leave_preview_page` (`:60033`) changes only `md_caret`,
`md_caret_wanted` and `preview_edit_focus`. It neither saves nor discards the
buffer and cannot raise a dirty modal after the write. Unsaved edits survive.

Palette/menu present by flush: `:96773` takes the batch before `:96792`
filters its target. The None arm (`:96815`) writes nothing, focuses/raises
nothing, and never restores the batch: closing the modal cannot retry it.
A keyboard event opening a menu itself flushes *before* handling that event
(`:114265`); the recheck covers a modal already present when flush runs.

First file refused, modal then closed: collection still creates a batch with
target None (`:96863`, `:96871`). Later files only append (`:22431`), so that
pending batch is wholly refused even after the modal closes. Boundary caveat:
a non-drop event or turn flushes it (`:114265`, `:102702`); files arriving
afterward open a new batch. There is no cross-boundary native gesture token,
so arbitrary interleaving is not an all-or-nothing guarantee across batches.

Same-pane/caret-only click: `:55105` returns before cancellation when the
shell seat is unchanged; preview caret moves also do not satisfy the Shell
composition plus different live shell-seat guard. No new cancellation there.

**P2 must-fix: cancellation is a request, not a stale-commit barrier.**
In the stipulated Sogou/cancel-not-honoured case, A has a live preedit, a drop
writes its path to B, cancellation is requested, and focus moves to B.
`cancel_composition` ignores the platform bool and clears local ownership
(`:99253`); Windows returns `ImmNotifyIME`'s bool
(`crates/bt-platform/src/lib.rs:6891`). A later `Ime::Commit` resolves the
current owner (`:99064`) and writes through `self.focused()` (`:99174`): B.
DESIGN section 7.1.5a'' requires that this never happen. Preserve composition
destination/generation or suppress stale delivery across the handoff; test
cancel refusal followed by Commit, while preserving fresh IME input at B.
Tests `:172399` / `:172229` pin source/enums, not late commits or actual Enter.

## Bar (a): exact method comparison with d1db4abd

`live_paste_target` (`:98821`) is byte-identical. The complete
`paste_offer_kept` (`:90741`) is byte-identical after removing this sole line:
```diff
             self.paste_offer_at(at_release),
             plan.fits(),
+            self.a_modal_holds_the_window(),
         )
```
The fifth term is `!modal` (`:29687`); addressed write still precedes focus
(`:90823`, `:90836`), and delivery indexes the retained target (`:98865`).

Validation: nine `cargo test -p bt-app --bin folio <filter> -j 4` runs passed:
294 test executions (overlapping filters), zero failures. Filters/counts:
`clipboard_path_tests` 15; `row_splits` 2; `a_rows_centre_says` 1; `focus` 114; `drop` 98; `ime` 61;
`a_pane_offers_one_middle_and_four_bands_at_every_scale` 1;
`a_content_drop_into_a_window_below_its_minimum_is_refused` 1; `a_stale_aim_is_refused_however_it_went_stale` 1.
Product code stayed read-only; no scratch tests, application launch or process
termination. Native activation remains best effort as recorded in round 1.
