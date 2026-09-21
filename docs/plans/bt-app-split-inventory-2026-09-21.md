# `bt-app` — the measured inventory, 2026-09-21

Taken against `main` at `1f1d2daa`, three documentation-only commits after the
`0.4.3` tag `1bc7cad5`, in a worktree of this repository on the branch
`docs/split-inventory-043`. It is the evidence file for
`docs/plans/bt-app-split.md` (revision 5) and it replaces
[`bt-app-split-inventory-2026-09-15.md`](bt-app-split-inventory-2026-09-15.md),
which was measured at `76ca0788` — before `0.4.1`, `0.4.2` and `0.4.3` shipped.

**Every number below came out of a tool run on this tree**, and §0 gives the one
command that reproduces each. The tables between the `GENERATED` markers are
rewritten in place by `scripts/dev/bt-app-split-freshness.py` on every run, so
they cannot drift from the tree while the prose around them stands; the tables
outside those markers are transcribed from the same runs' printed output, named
where they appear. Nothing was built: no `cargo build`, no `cargo check`, no
`cargo test`. Product code and tests are untouched.

**The blocks are anchored by method name, not by line number.** A line number in
a file that grew by 34,000 lines in six days is a fact with a shelf life of
hours; the first and last method of each block are the anchor that survives.

---

## 0. Reproducing this — one line per artefact

From a worktree root, with the parsers vendored once into `target/`
(documentation tools; they are not and never become workspace dependencies):

```bash
python -m pip install --target target/review-python tree_sitter==0.25.2 tree_sitter_rust==0.24.2 networkx==3.6.1
```

| Artefact | The one command that writes it |
| --- | --- |
| `target/bt-app-graph.json` — the module graph, all six variants | `python scripts/dev/bt-app-graph.py > target/bt-app-graph-output.txt` |
| the plan's §7.2/§7.3 row tables, and the manifest-versus-graph check | `python scripts/dev/bt-app-split-table.py > target/bt-app-split-table-output.txt` |
| `bt-app-split-2a-manifest-2026-09-21.tsv` — a row per moving method | `python scripts/dev/bt-app-split-freshness.py --base 76ca0788 --commit 1f1d2daa --date 2026-09-21 --report docs/plans/bt-app-split-inventory-2026-09-21.md` |
| `bt-app-split-pins-2026-09-21.tsv` — a row per source-reading test | the same command |
| `bt-app-split-method-delta-2026-09-21.tsv` — a row per method name, both baselines | the same command |
| `bt-app-split-changes-2026-09-21.tsv` — modules and impls added since the baseline | the same command |
| `bt-app-split-freshness-data-2026-09-21.json` — everything the four tables are cut from | the same command |
| every `GENERATED` table in this document | the same command, which rewrites this file in place |

The freshness run must follow the graph run, on the same tree: it asserts that
the working tree it was given and the Git blobs it read are the same bytes, and
that its own method inventory and the graph's agree name for name, line for
line. If either has drifted it stops rather than publish a mixed reading.

`--base` is the tree the delta is measured against — `76ca0788` is the tree the
2026-09-15 inventory was measured on. `--date` names the artefacts, so a re-run
on another day reproduces the same file names rather than a second set.

---

## 1. The two `impl Runtime<'_>` blocks, by name

Step 2a is these two blocks and nothing else. `preview_viewport.rs` holds a
third, separate inherent `impl Runtime`; it is a retained sibling and is not in
scope here.

<!-- GENERATED BLOCK EXTENT -->
| Block | First method | Last method | Methods | Block lines |
| --- | --- | --- | ---: | ---: |
| baseline block 1 | `create` | `complete_markdown_raster` | 578 | 25,149 |
| baseline block 2 | `preview_content_extent` | `vault_this_window` | 732 | 38,338 |
| candidate block 1 | `create` | `complete_markdown_raster` | 590 | 26,142 |
| candidate block 2 | `preview_content_extent` | `vault_this_window` | 800 | 42,064 |

`main.rs` is **131,328** lines; the two blocks are **68,206** of them (**1,390** methods, **53,142** lines of method extent), leaving a strict residue of **63,122** before any scaffolding.
Baseline: **165,815** / **63,487** / **1,310** methods / residue **102,328**.
<!-- END GENERATED BLOCK EXTENT -->

Two readings of that table are worth spelling out.

**`main.rs` shrank by 34,487 lines while the blocks grew by 4,719.** The file is
smaller than the 2026-09-15 inventory recorded because the largest inline
`mod tests` left it for `tests.rs` — 48,608 lines — on 2026-09-18, which was the
split's first cut, not because anything moved out of the two blocks. The residue
Step 2a leaves behind
is therefore **63,122** lines, not the 102,328 the old inventory measured — the
whole of that improvement is the test module, and none of it is the relocation.

**Block lines and method-extent lines are two different numbers.** A block's
extent includes both braces, the space between items, and the doc comments and
attributes above each method; a method's extent starts at its `fn` item. The
gap — 68,206 against 53,142 — is text the implementer still has to pair with
its method, and it is not proposed file size for anything.

---

## 2. The topic classification

The sort is the plan's own: 28 ordered regular expressions matched anywhere in
the snake_case method name, first match wins, printed in full at
[§0.3 of the 2026-09-15 inventory](bt-app-split-inventory-2026-09-15.md#03-the-theme-sort-in-full)
and held in `THEMES` in the generator. It is unchanged; only the method set it
is applied to has moved.

**It is a sort, not an audit.** It places methods by their names. The generated
destinations below are draft destinations for the item-level manifest, not an
ownership finding, and no method share however high replaces the manifest.

<!-- GENERATED 2A SUMMARY -->
| Proposed destination | Methods | Method extent lines |
| --- | ---: | ---: |
| `runtime/attention.rs` | 27 | 724 |
| `runtime/clipboard.rs` | 13 | 203 |
| `runtime/diagnostics.rs` | 5 | 115 |
| `runtime/dpi.rs` | 11 | 542 |
| `runtime/files.rs` | 59 | 1,950 |
| `runtime/first_run.rs` | 24 | 755 |
| `runtime/floats.rs` | 59 | 2,817 |
| `runtime/focus.rs` | 51 | 1,344 |
| `runtime/frame.rs` | 23 | 2,015 |
| `runtime/git.rs` | 103 | 3,787 |
| `runtime/i18n.rs` | 2 | 33 |
| `runtime/keyboard.rs` | 35 | 1,835 |
| `runtime/launch.rs` | 4 | 917 |
| `runtime/math.rs` | 49 | 2,159 |
| `runtime/mouse.rs` | 63 | 5,188 |
| `runtime/palette.rs` | 15 | 459 |
| `runtime/panes.rs` | 130 | 3,714 |
| `runtime/peek.rs` | 57 | 2,088 |
| `runtime/preview.rs` | 280 | 9,619 |
| `runtime/profiles.rs` | 23 | 471 |
| `runtime/quake.rs` | 8 | 125 |
| `runtime/search.rs` | 22 | 630 |
| `runtime/settings.rs` | 31 | 1,879 |
| `runtime/tabs.rs` | 71 | 2,687 |
| `runtime/terminal.rs` | 34 | 1,248 |
| `runtime/tooltips.rs` | 9 | 601 |
| `runtime/web.rs` | 34 | 1,319 |
| `runtime/windows.rs` | 36 | 1,523 |
| `unassigned` | 112 | 2,395 |
| **Total** | **1,390** | **53,142** |

Visibility proposals: **317** `private`, **124** `pub(crate)`, **949** `pub(in crate::runtime)`.
Thus **1,073** methods have proposed widening; **112** destinations remain unassigned.

Census output: **574** consumer rows; **207** new test identities and **36** changed bodies. These include non-guard fixtures and are not a structural-pin total.
<!-- END GENERATED 2A SUMMARY -->

---

## 3. The methods that fit no topic

These are the methods a human has to place. They are listed by name because
that is the only handle on them that a ticket can carry.

<!-- GENERATED UNASSIGNED -->
**112** of the **1,390** methods match no topic expression. In name order:

- `adopt_option_as_alt`, `advance_command_flash`, `advance_foot_reveal`, `advance_page_foot_clocks`, `advance_retirement`, `advance_storage_watch`
- `advanced_open`, `after_current_hit_moved`, `animated_surfaces`, `animating_deadline`, `announce_persistence_faults`, `announce_pins_fault`
- `append_edge`, `apply_acrylic`, `apply_always_on_top`, `apply_block_max_height`, `apply_explorer_place`, `apply_minimum_contrast`
- `apply_option_sends_alt`, `apply_repair_row_breaks`, `apply_row_verb`, `apply_slider`, `apply_theme`, `apply_theme_mode`
- `apply_update_check`, `ask_the_worker_about_a_link_target`, `browse_for_program`, `browse_for_root`, `cancel_composition`, `carry_live_journeys`
- `command_flash_deadline`, `command_flash_is_running`, `command_flash_layer`, `command_tick_anchor`, `composition_origin_now`, `editor_subject`
- `expand_commit`, `finish_synchronized_update_if_due`, `flip_page_source_on`, `follow_the_display`, `foot_reveal_is_fresh`, `foot_saying`
- `gate_dirty_names`, `gated_minted_target`, `glancing_row_at`, `glass_here`, `head_run`, `head_that_raised_a_layer`
- `honour_command_line`, `keep_what_the_modal_covers`, `land_page_refusal_on`, `land_page_source_on`, `leave_detached_head`, `line_height_subpixels`
- `load_more_commits`, `note_user_typing`, `open_address_here`, `open_broker`, `open_crumb_draft`, `open_local_path`
- `open_local_path_verified`, `open_minted_page`, `open_minted_page_on`, `open_path_in_default_app`, `owns`, `page_carried_by`
- `page_is_the_typing_target`, `page_keepsake_icons`, `page_shrinker`, `page_source_shown_on`, `pages_are_gone`, `plan_for`
- `plan_inputs`, `plan_inputs_for`, `play_button_rasters`, `push_action_candidates`, `push_command_candidates`, `push_file_candidates`
- `push_place_candidates`, `re_ask_the_worker_about_a_link_target`, `reference_cell_index`, `reread_pins`, `reset_advanced_group`, `retire_the_stand_in`
- `return_to_live_for_input`, `reveal_in_explorer`, `reveal_verified`, `root_choices`, `row_adopt_fits`, `row_payload`
- `row_under`, `running_journeys`, `say_address_refused`, `send_user_input`, `settle_composition_owner`, `settle_home`
- `settle_landed_head`, `slot_starts`, `stage`, `stage_departure`, `standing_block_range`, `standing_source_block`
- `sweep_foot_phrases`, `toggle_advanced_group`, `toggle_pin`, `toggle_switcher_pin`, `track_grabbed`, `trigger_rect`
- `trigger_root`, `uncommitted_edit_name`, `verified_target`, `write_editor_field`
<!-- END GENERATED UNASSIGNED -->

The 2026-09-15 inventory already said why some of these are not hard: the sort
misfiles by name, and `apply_row_verb` — still on this list — is a settings
method that no settings expression happens to match. Others are genuinely
cross-cutting and their placement is a design call, not a lookup.

---

## 4. The visibility draft

The generator proposes the narrowest visibility that the caller edges it can
see allow: callers only in the same destination → private; a caller in another
runtime destination → `pub(in crate::runtime)`; a caller in the retained root or
an existing sibling module → `pub(crate)`. An unassigned caller is treated as
possibly a different destination, so **placing the 112 above can only narrow
this draft, never widen it.**

| Visibility | Methods |
| --- | ---: |
| `private` | 317 |
| `pub(in crate::runtime)` | 949 |
| `pub(crate)` | 124 |
| **proposed widening** | **1,073** |

**This is a conditional minimum for the caller graph the parser can see, not a
certified minimum for the program Rust resolves.** Tree-sitter is not rustc:
a same-named method on another receiver type can overstate a widening, and an
alias or a macro can hide an edge. Each row carries its caller witnesses and an
`unresolved_callers` count so the implementer can see which ones rest on an
unresolved receiver. Sufficiency to compile is unverified; the first gate is
compilation on CI.

---

## 5. The source-reading tests

This is the input the preparation's ticket batches are cut from:
[`bt-app-split-pins-2026-09-21.tsv`](bt-app-split-pins-2026-09-21.tsv), a row
per test or guard that reads source text, carrying the files it names, the
methods it pins, whether those methods are in the moving blocks, and what Step
2a does to it. The classification follows the plan's §6.2(b) classes; a mixed
guard carries several, so the class totals sum to more than the row count.

<!-- GENERATED READERS -->
| Reader class (a row may carry several) | Rows |
| --- | ---: |
| named body / positive requirement | 433 |
| scoped negative | 143 |
| path / selector (§6.2(c)) | 101 |
| source fixture / path (§6.2(c)) | 72 |
| arity / uniqueness | 68 |
| whole-source prohibition | 18 |

| What Step 2a does to the row | Rows |
| --- | ---: |
| subject moves: retarget atomically | 274 |
| no 2a subject move identified | 195 |
| retained subject: no 2a move | 47 |
| fixture/manifest input retained in 2a | 38 |
| coverage changes: preserve union / audit needles | 8 |
| retained source file read dynamically: no 2a move | 3 |
| read-main: inspect subject/needle | 3 |
| recursive enumeration: coverage retained in 2a | 2 |
| enumeration: audit recursion and original scope | 2 |
| source-list loses moved occurrence | 1 |
| arm moves: retarget atomically | 1 |

| Against the baseline | Rows |
| --- | ---: |
| existing | 331 |
| NEW | 207 |
| CHANGED | 36 |

**574** rows in all, over **50** files.
`include_str!("main.rs")` invocations in `main.rs`: **55** (baseline **91**). Parsed `#[test]` attributes under `crates/bt-app`: **4,228** (baseline **3,809**).
<!-- END GENERATED READERS -->

Four things that table does not say, and a ticket writer must not read into it:

- **A row is not a test that will turn red.** "Subject moves" means the input
  loses the production subject it was selecting. Some helpers fall through to a
  test literal and stay green while guarding nothing, which is the failure mode
  the plan's §6.2(b) mutation rules exist to catch.
- **The count of `include_str!("main.rs")` sites inside `main.rs` fell from 91
  to 55 without a single guard being removed.** The readers that used to live in
  the inline `mod tests` now read `main.rs` from `tests.rs` — a sibling file —
  so they left the self-inclusion count and stayed in the census. 104 of the 574
  rows sit in `tests.rs` and 233 in `main.rs`, and **334 rows name `main.rs` as
  an input** whatever file they live in. A count of self-inclusions is no longer
  a count of the readers, and only the census answers the question.
- **The classification is conservative and generated**, with read-audited
  overrides for the arm, field and list selectors named in the 2026-09-18
  investigation. It is not a mutation-tested conversion map, and no guard was
  converted or mutation-fired in this run.
- **`the_shell_page_is_gone` still walks `src` non-recursively**, so a
  `runtime/` directory leaves its scope silently; `platform_gate_tests` and the
  environment-document guard already descend. That disagreement between two
  whole-program guards is unchanged since 2026-09-15 — the guard has moved into
  `tests.rs` since, and moving it did not fix it.

---

## 6. What changed since 2026-09-15

The delta is measured by the same generator, running the same sort over the
baseline tree `76ca0788` — which is the tree the 2026-09-15 inventory was taken
on — and diffing by method name.

**The sort reproduces the old inventory exactly.** Run over `76ca0788` it
finds 1,310 methods in two blocks of 63,487 lines with a residue of 102,328 and
**99 unclassified**, which is what §0.3 and §3.2 of that file report. The
2026-09-21 numbers are therefore a like-for-like reading, not a new method.

| | 2026-09-15 (`76ca0788`) | 2026-09-18 (`dc99be53`) | 2026-09-21 (`1f1d2daa`) |
| --- | ---: | ---: | ---: |
| `main.rs` lines | 165,815 | 176,152 | **131,328** |
| block lines | 63,487 | 66,686 | **68,206** |
| methods | 1,310 | 1,356 | **1,390** |
| method-extent lines | 49,633 | 51,810 | **53,142** |
| strict residue | 102,328 | 109,466 | **63,122** |
| unassigned | 99 | 106 | **112** |
| `include_str!("main.rs")` in `main.rs` | 91 | 105 | **55** |
| source-reading rows | — | 502 | **574** |

188 commits touched `main.rs` between the two baselines.

<!-- GENERATED DELTA -->
**84** method names added, **4** gone, **1,306** retained; **0** retained names land in a different topic than they did at the baseline. A rename appears here as one gone and one added, which is why the two are listed rather than netted.

| Gone since the baseline | Was |
| --- | --- |
| `advance_drag_autoscroll` | `runtime/mouse.rs` |
| `clear_math_hover_if_due` | `runtime/math.rs` |
| `strip_animation_deadline` | `runtime/preview.rs` |
| `terminal_thumb_deadline` | `runtime/focus.rs` |

| Added since the baseline | Is |
| --- | --- |
| `a_modal_holds_the_window` | `runtime/windows.rs` |
| `adopt_clipboard_picture` | `runtime/preview.rs` |
| `advance_math_toggle_if_due` | `runtime/math.rs` |
| `aimed_seat_kind` | `runtime/panes.rs` |
| `animating_deadline` | `unassigned` |
| `animation_frame_is_due` | `runtime/preview.rs` |
| `apply_repair_row_breaks` | `unassigned` |
| `ask_about_the_link_under_the_pointer` | `runtime/mouse.rs` |
| `ask_the_worker_about_a_link_target` | `unassigned` |
| `begin_present_attempt` | `runtime/frame.rs` |
| `bring_this_window_forward` | `runtime/windows.rs` |
| `carry_live_journeys` | `unassigned` |
| `check_picture_freshness` | `runtime/preview.rs` |
| `clamp_animation_deadline` | `runtime/preview.rs` |
| `collect_dropped_file` | `runtime/mouse.rs` |
| `command_flash_is_running` | `unassigned` |
| `composition_origin_now` | `unassigned` |
| `default_profile_id` | `runtime/profiles.rs` |
| `deliver_paste` | `runtime/clipboard.rs` |
| `dropped_files_seat` | `runtime/files.rs` |
| `emit_ime_observation` | `runtime/keyboard.rs` |
| `end_a_divider_drag_that_lost_its_pointer` | `runtime/panes.rs` |
| `file_peek_is_fading` | `runtime/peek.rs` |
| `finish_present_attempt` | `runtime/frame.rs` |
| `finish_pty_coalesce_if_due` | `runtime/terminal.rs` |
| `flush_dropped_files` | `runtime/files.rs` |
| `focus_the_pane_a_path_landed_in` | `runtime/focus.rs` |
| `follow_the_display` | `unassigned` |
| `formula_overlay_is_active` | `runtime/math.rs` |
| `formula_toggle_layers` | `runtime/math.rs` |
| `glass_here` | `unassigned` |
| `graph_filter_menu_stand` | `runtime/git.rs` |
| `hovered_pane_path_verdict` | `runtime/panes.rs` |
| `ime_native_facts` | `runtime/keyboard.rs` |
| `leave_hovered_math` | `runtime/math.rs` |
| `live_paste_target` | `runtime/clipboard.rs` |
| `math_band_trace_for_present` | `runtime/math.rs` |
| `math_band_trace_line` | `runtime/math.rs` |
| `math_toggle_deadline` | `runtime/math.rs` |
| `math_toggle_faces` | `runtime/math.rs` |
| `math_toggle_heights` | `runtime/math.rs` |
| `next_animation_deadline` | `runtime/preview.rs` |
| `next_animation_frame` | `runtime/preview.rs` |
| `observe_ime_focus` | `runtime/focus.rs` |
| `observe_ime_key` | `runtime/keyboard.rs` |
| `open_local_path_verified` | `unassigned` |
| `paste_offer_at` | `runtime/clipboard.rs` |
| `paste_offer_kept` | `runtime/clipboard.rs` |
| `paste_paths_into` | `runtime/clipboard.rs` |
| `paste_target` | `runtime/clipboard.rs` |
| `picture_is_owed` | `runtime/preview.rs` |
| `platform_pointer_now` | `runtime/mouse.rs` |
| `pointer_reference_at` | `runtime/mouse.rs` |
| `present_conditions` | `runtime/frame.rs` |
| `present_math_toggle` | `runtime/math.rs` |
| `present_signature` | `runtime/frame.rs` |
| `press_math_toggle` | `runtime/math.rs` |
| `preview_menu_stand` | `runtime/preview.rs` |
| `re_ask_the_worker_about_a_link_target` | `unassigned` |
| `refresh_agent_rows` | `runtime/attention.rs` |
| `refresh_chrome_with_overlay` | `runtime/floats.rs` |
| `refresh_chrome_without_overlay` | `runtime/floats.rs` |
| `refresh_formula_overlay_for_present` | `runtime/math.rs` |
| `refresh_math_hover_against_the_picture` | `runtime/math.rs` |
| `refresh_overlay_with_formula` | `runtime/math.rs` |
| `repaint_pane_change_inner` | `runtime/panes.rs` |
| `retained_seats` | `runtime/panes.rs` |
| `reveal_verified` | `unassigned` |
| `root_menu_stand` | `runtime/profiles.rs` |
| `running_journeys` | `unassigned` |
| `sample_math_toggle` | `runtime/math.rs` |
| `save_clipboard_picture` | `runtime/preview.rs` |
| `seat_path_verdict` | `runtime/panes.rs` |
| `service_drag_autoscroll` | `runtime/mouse.rs` |
| `service_ime_report` | `runtime/git.rs` |
| `service_pictures` | `runtime/preview.rs` |
| `settle_math_toggle` | `runtime/math.rs` |
| `strip_animation_work` | `runtime/preview.rs` |
| `switch_math_source_now` | `runtime/math.rs` |
| `terminal_thumb_work` | `runtime/focus.rs` |
| `trace_drain` | `runtime/diagnostics.rs` |
| `trace_math_band` | `runtime/math.rs` |
| `verified_target` | `unassigned` |
| `write_ime_observation` | `runtime/keyboard.rs` |
<!-- END GENERATED DELTA -->

**Nothing moved topic.** Every one of the 1,306 methods that kept its name kept
its destination, so the plan's 28 destination files are the same 28 files they
were, with the same names in them plus 84 more. The growth is concentrated:
18 of the 84 are formula and math work, 11 preview, 6 each clipboard and panes.

**Four names are gone, and the four went four different ways.** A name diff
cannot tell a rename from a deletion, so each was looked at:

| Gone | What is there instead |
| --- | --- |
| `clear_math_hover_if_due` | renamed: `clear_math_hover` is in the added list |
| `strip_animation_deadline` | no longer a method; the name is now a local binding inside `turn` |
| `terminal_thumb_deadline` | the same — a local binding inside `turn` |
| `advance_drag_autoscroll` | no counterpart in the added list; `service_drag_autoscroll` and `drag_autoscroll_deadline` are both older than the baseline |

So two of the four are the clock fold into the frame turn, one is a rename, and
one is work that left without a new name. None of them is a method that
silently moved to another file.

The full row-level delta, including every retained method's extent on both
trees, is
[`bt-app-split-method-delta-2026-09-21.tsv`](bt-app-split-method-delta-2026-09-21.tsv).

### 6.1 What else the baselines disagree about

- **22 source files are new** since `76ca0788`, six of them wholly test,
  `tests.rs` among those. They are in
  [`bt-app-split-changes-2026-09-21.tsv`](bt-app-split-changes-2026-09-21.tsv)
  together with every impl and test module added inside existing files.
- **`FILES_THAT_MAY_NAME_A_PLATFORM` is fifteen entries**, not the eleven the
  plan's §6.2(a) baseline records, and not the thirteen the 2026-09-18
  investigation found. Strict Step 2a still admits no entry and silences none,
  so the arity is preserved — but at fifteen. Both the array and its guard
  `only_the_named_files_decide_what_platform_this_is` are still physically in
  `main.rs`, which is what the plan's §6.2(a) requires of them, because the
  PowerShell twin reads the array out of that file by name.
- **Parsed `#[test]` attributes under `crates/bt-app` are 4,228**, against
  3,809 at the 2026-09-15 baseline. That is a parse count, not a libtest
  census: an attribute behind a platform `cfg` is counted and may not run.

---

## 7. The two generators, and what changed in them

### 7.1 `scripts/dev/bt-app-split-freshness.py` now lives on `main`

The generator was written on the investigation branch and could not be re-run
from `main`. It is on `main` from now on so these numbers can be re-derived.
Four changes, all of them about being runnable on a baseline other than the one
it was written for; the analysis is untouched:

1. **The run date is an argument** (`--date`), not a constant, so the artefact
   names follow the run instead of being edited by hand.
2. **The revision-5 locator map is opt-in** (`--references`). Its table of
   line numbers and its 22-line normalisation belong to the single base
   `ee771130`; against any other base it would emit line correspondences that
   mean nothing. Off by default, and the references TSV is not written.
3. **The document carrying the generated tables is an argument** (`--report`),
   so this inventory can hold them instead of the investigation report.
4. **A method-level delta is emitted** — `manifest()` run over the baseline tree
   as well, diffed by name, written to `bt-app-split-method-delta-<date>.tsv`
   and into §6's tables. §6 used to be an eyeball comparison of two prose
   documents; it is now generated.

The block anchors in §1 are the fifth change and the smallest: each block now
carries the name of its first and last method, because that is what the plan
needs to cite and a line number is not.

### 7.2 `scripts/dev/bt-app-graph.py` no longer lists its test files by hand

The graph builder carried two hand-written lists — which files are wholly
`#[cfg(test)]`, and which file names are not their own module's name. **Both had
gone stale.** The list named five wholly-test files; **there are twelve**:

| Wholly-test file | How it is attached |
| --- | --- |
| `tests.rs` | `#[cfg(test)] mod tests;` in `main.rs` |
| `journeys_tests.rs` | `#[cfg(test)] #[path] mod journeys_tests;` in `main.rs` |
| `file_reads_source_tests.rs` | `#[cfg(test)] mod file_reads_source_tests;` in `main.rs` |
| `preview_typing.rs` | `#[cfg(test)] mod preview_typing;` in `main.rs` |
| `source_pin.rs` | `#[cfg(test)] mod source_pin;` in `main.rs` |
| `attention/tests.rs` | `#[cfg(test)] mod tests;` in `attention.rs` |
| `attention_words/tests.rs` | `#[cfg(test)] mod tests;` in `attention_words.rs` |
| `focus_thumb_restore_tests.rs` | `#[cfg(test)] #[path] mod restore_tests;` in `focus_thumb.rs` |
| `preview_viewport_tests.rs` | `#[cfg(test)] #[path] mod tests;` in `preview_viewport.rs` |
| `ime_report_tests.rs` | `#[cfg(test)] #[path] mod tests;` in `ime_report.rs` |
| `present_diagnostics_tests.rs` | `#[cfg(test)] #[path] mod tests;` in `present_diagnostics.rs` |
| `uninstall_tests.rs` | `#[cfg(test)] #[path] mod tests;` in `uninstall.rs` |

**The lists are gone rather than corrected.** The builder now walks the module
declarations from `main.rs` outward — resolving `#[path]` against the declaring
file's directory and a plain `mod x;` against that file's own module directory
— and derives three sets from what it finds: which files are wholly test
(inherited down the tree), which physical file names are not their semantic
owner, and which graph nodes hold nothing but test code. A hand-written list
goes stale the first time a test module moves into a file of its own, and it
did: six of the seven it was missing did not exist on 2026-09-15, and the
seventh — `attention/tests.rs` — was missing from it already then. Every one of
the 124 files is reached from `main.rs`; nothing falls back to its file name.

**Two production files were being counted as separate modules**, which no
hand-written list had caught either: `hang_watch_detail.rs` is
`hang_watch::detail` and `settings_geometry.rs` is `settings::geometry`, both
attached by `#[path]`. They now fold into their owners.

What that fixes in the numbers, on this tree:

| Reading | Hand-written lists | Derived |
| --- | ---: | ---: |
| wholly-test files | 5 | **12** |
| test lines across `crates/bt-app/src` | 168,436 | **223,084** |
| `root_prod` nodes | 116 | **108** |
| `root_prod` baseline free | 15 / 13,658 | **14 / 13,346** |
| `root_prod` after the `i18n` cut | 22 / 35,200 | **21 / 34,888** |

The `regex_*` variants are unchanged by design: they reproduce the first
inventory's convention, which reads test-only files as written. `main.rs`'s own
statistics and the method inventory are unchanged, and so is every number in
§§1–6: the freshness generator parses Git blobs directly and takes nothing from
the graph but its assertions — same method names, same lines, same `main.rs`
length — and the `root_prod` figures reproduced above. Those assertions still
hold after the fix, which is the check that the two tools read one tree.

### 7.3 `bt-app-split-table.py` disagrees with the graph, and that is a fact not a gate

The table generator exits 1. It holds the plan's §7.2 row manifest, which was
written against the 2026-09-15 graph, and six of its rows now name modules the
graph no longer frees — `trace`, `glyph_trace`, `preview_trace`,
`attention_trace`, `web_trace`, `context_menu` — while five newly free modules
have no row: `clipboard_picture`, `coalesce`, `file_reads`, `pace`,
`shell_literal`. That is recorded as factual drift in the plan's Step 3 row
design, which Step 2a does not depend on. No redesign follows from it here.

---

## 8. What this inventory does not establish

Unchanged from the 2026-09-18 investigation, and repeated because a fresher set
of numbers reads like more certainty than it is:

1. **The destination map is a draft.** 112 methods are unplaced, and the caller
   edges behind the visibility column include unresolved receivers.
2. **No guard was converted or mutation-fired.** The census is the input to that
   work, not a record of it.
3. **Nothing was compiled.** Visibility sufficiency, import resolution, `cfg`
   and lint inheritance and old-to-new test identity are all open, and the
   implementer's first gate is compilation on CI.
4. **The freeze fields are still empty** — coordinator, runner, recorded green
   base, affected-ticket inventory, maximum duration, gate and merge target.
   No count in this file fills one of them.
