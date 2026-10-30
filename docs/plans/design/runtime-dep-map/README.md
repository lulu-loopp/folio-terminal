# The runtime dependency map (J3, T-RUNTIME-DEP-MAP = ownership ticket O-1)

The measured dependency graph inside `bt-app`, which the ownership contract
(`../ownership-contract-2026-10-11.md`, §3.1) asks for before any field moves, and
which K2 (T-SPLIT-2B) uses to order its topic moves. Four tab-separated files,
**generated**: `bt_source::DependencyMap`, taken from the same `bt_source::FieldCensus`
the ownership census gate uses, over the same structs (`App`, `WindowRuntime`,
`TabState`, `ShellSession`, `PreviewBuffer`, `PreviewPane`, `DocumentView`,
`PictureView`).

- Regenerate: `pwsh -File scripts/generate-runtime-dep-map.ps1` (runs cargo; writes
  the four files and the `# measured at <commit>` line under each header).
- Guard: `crates/bt-source/tests/census.rs`
  `the_committed_runtime_dep_map_is_the_code`. Every line of the four files must be
  the code's at this tree, except the `# measured at` line, which
  `scripts/ci/check-generated-stamps.ps1` reads instead. A hand edit fails it, and
  so does any change to the code that moves a row. Nothing here is edited by hand.
- The rules: `crates/bt-source/tests/dep_map.rs`, one fixture each.

The numbers in this README were read off the files at the commit their headers
name. They are prose, and nothing holds them. The files are the authority.

## How to read the four files

A **topic** is a file under `crates/bt-app/src/`: `main.rs`, `runtime/panes.rs`,
`preview.rs`. An **item** is a callable named as the census names it: the declaring
module, then the label (`crate::runtime::panes::Runtime::close_pane`,
`crate::mark_leaf_seen`, `crate::<Runtime as Deref>::deref`). A `cfg` variant counts
as the same item.

| file | one row is | columns |
|---|---|---|
| `fields.tsv` | an item × a field × a kind of use | `struct field item kind count`. `kind` is `read`, `access` (a `&mut` lend, `get_mut`), `write` (proven), `membership` (a hub's contents), or `inner` (through a shared reference). `count` is sites. The four census kinds are exactly the census's columns. `read` is new: a field evaluated where nothing mutable is done to it (a value, a `&` borrow, a `&self` method, a non-`ref mut` pattern, a step on the way to another field that is read). |
| `topics.tsv` | a pair of topics joined by calls | `from_topic to_topic calls example_item`. `calls` is call sites in `from_topic` that reach an item of `to_topic`. `example_item` is the caller → callee pair with the most sites. A call within one topic is not an edge. |
| `doors.tsv` | a door already present | `struct field door_item writes other_writers status`. The door is an item **outside the field's own type** that is its only writer, or one of two (write and membership sites). `other_writers` names the other writer, or `-`. `status` is `incomplete` when an unknown census site names the field. |
| `unclassified.tsv` | a free function or `Runtime` method still written in `main.rs` | `item shape called_by calls called_from calls_into`, **least-referenced first**. `called_from` is the topics its callers are written in, most call sites first (the candidate home). `calls_into` is the topics of what it calls. `-` means none. |

**What the map does not see** is printed as a count in each header. Nothing is
guessed. A read through a receiver the census rules do not type is not attributed:
182 sites. A call is not followed through a trait object, a stored closure, a
function value, or a receiver the rules do not type. The counts below were read at
plan row 84 (2026-10-28); the header is the authority:

- 2,288 sites call a method name only the package has, through an untyped receiver.
  The largest share is `i18n::Text::X.text()`: the receiver is an enum variant's
  path, and the census does not type a path value.
- 4,315 sites call a method name that a standard type also has.
- 301 sites reach more than one item, for example two types of the same name in two
  modules.
- A call through an associated type of a generic (`B::Glass`, `B::Window`, `G::Device`)
  is followed to the bound trait's impl on the one type the product impls bind it to:
  the window seam's forwards in `window_seam.rs`, never the bound type's own inherent
  method (row 84: the 567 sites row 83 listed are the `<topic> → window_seam.rs` rows,
  31 of them new). Bindings that differ, a binding to the impl's own generic, no
  product binding, or bindings the rules cannot compare are listed by kind: none at
  row 84.
- 21 sites call through a generic's trait by its path (`T::from_bytes`,
  `B::open_window`): each reaches whichever implementation the argument names, and the
  census follows it to none (plan row 84 left this class as it was).
- 27 sites call a method that only other types of the receiver's name declare, and
  21 call a type's path whose function the rules do not resolve (`Window::set_title` on
  a type the rules place at a workspace `Window`). Both were silent before row 83. Where
  the receiver's type has a `Deref` target, such a call goes on to the target (row 84):
  none of the 27 at row 84 has one.
- 2 sites call a type outside the workspace by its path where a workspace trait
  implements the function on it (`ActiveEventLoop::exit(self)` in the window seam): the
  language picks the outside type's own function first, which the census does not read
  (row 84; dropped before). A method call on an outside type still takes the workspace
  trait's impl without that check: the same blind spot, not measured.
- 1 site calls through a qualified path (`<T as Trait>::f`), and 1 through a path
  the module's declarations and `use` items do not reach.
- A trait method the subject implements on another package's type is followed to the
  impl when the impl's self type is placed for sure at the receiver's type (declared in
  the impl's own module or where its path says, a path rooted at a named workspace
  crate to one declaration, or a name the workspace declares once), and listed as not
  followed otherwise: none at row 84. A limit: a path's module part is matched by its
  ending, so `use term as t; impl X for t::Session` is placed exactly when some module
  whose path ends in `::t` happens to declare a `Session` (a coincidence of names the
  rules do not tell from a real path; none at row 84).
- The call graph's `Ambiguous` calls with exactly one candidate (every call through an
  untyped receiver whose name one item declares; a rename to such a name is one way an
  edge gets here) are not listed by site in `docs/plans/UNFOLLOWED-EDGES.tsv`; its header
  counts them (row 84: 1,765), the call graph test holds the count to the tree, and
  `check-unfollowed-edges.ps1` reads it and reports its change against the merge base.

These are S-2a's call graph's to close. The map is a lower bound on edges and an
exact count of what it does see.

## The measured present (at the commit the headers name)

- **fields.tsv**: 5,653 rows. 4,566 read sites, 1,928 write, 857 access, 33
  membership, 4 inner.
- **topics.tsv**: 1,409 edges, 12,220 call sites.
  - `main.rs` calls into 146 other topics (2,335 sites).
  - 54 topics call into `main.rs` (2,056 sites). The heaviest are
    `runtime/preview.rs` (535), `runtime/mouse.rs` (245), `runtime/panes.rs` (154),
    `runtime/tabs.rs` (118), `runtime/floats.rs` (95), `runtime/keyboard.rs` (93),
    `runtime/windows.rs` (90) and `runtime/frame.rs` (81).
  - The `runtime/*` topics call each other along 372 edges (2,782 sites).
- **doors.tsv**: 317 doors at O-5a (2026-10-27; 334 at O-1) and at O-5b
  (2026-10-28). 221 are on `WindowRuntime`, 55 on `App`, 19 on `TabState`, 9 on
  `ShellSession`, 7 on `LaunchFacts` and 6 on `TermView`. `PreviewBuffer`,
  `PreviewPane`, `DocumentView`, `PictureView` and `TerminalPane` have none: O-7
  and O-8 put their writes inside their own types, and the pane's writes are its
  own methods (O-5b).
- **unclassified.tsv**: **252 items, 220 free functions and 32 `Runtime`
  methods**, still in `main.rs` (at K2 batch 26's head; 260, 226 and 34 before it,
  at batch 25's head; 275, 241 and 34 before batch 25,
  at batch 24's head; 285, 241 and 44 before batch 24,
  at plan row 59's head: K2 batch 23's 284 and row 66's `say_in_a_box_or_on_stderr`;
  329, 242 and 87 before batch 23,
  368, 274 and 94 before batch 22,
  419, 303 and 116 before batch 21,
  459, 341 and 118 before batch 20,
  503, 383 and 120 before batch 19,
  538, 411 and 127 before batch 18,
  593, 446 and 147 before batch 17,
  613, 460 and 153 before batch 16,
  640, 476 and 164 before batch 15,
  667, 495 and 172 before batch 14,
  696, 509 and 187 before batch 13,
  713, 525 and 188 before batch 12,
  734, 545 and 189 before batch 11,
  754, 559 and 195 before batch 10,
  769, 573 and 196 before batch 9,
  783, 587 and 196 before batch 8,
  799, 600 and 199 before batch 7,
  813, 612 and 201 before batch 6,
  819, 613 and 206 before batch 5,
  823, 617 and 206 before batch 4,
  828, 620 and 208 before batch 3,
  837, 627 and 210 before batch 2, and 846, 634 and 212 before batch 1).
  - The plan's "106 unclassified" was the 2026-09-18 theme-regex count of
    `Runtime` methods whose names no theme matched (112 on 2026-09-21). The name
    sort is retired.
  - The measured number of `Runtime` methods with no topic file is 209. They
    include D-29…D-31's unmoved `launch`, `settings` and `focus` topics.

## K2 (T-SPLIT-2B): the ordered proposal

The rule is the contract's §3.2: K2 lands before any field moves, least-referenced
first. A row's home is the first topic in its `called_from`. The move does not
change behaviour, and the window-waits, test-shells and census guards must read the
same facts after it.

Read at batch 1's head (2026-10-24; batch 1 moved the first six topics' ten items,
`docs/plans/design/runtime-dep-map/` regenerated with it). K2's rulings (2026-10-24):
the six smallest topics landed as one batch, one commit per topic, and every later
topic is a landing of its own; a row whose first `called_from` place is tied goes to
the tied topic `docs/ARCHITECTURE.md` §13's "owns" column names for it, written in the
batch's record; a moved free function is re-exported from `runtime/mod.rs` and named
again at the crate root by `use runtime::{…}`, so no caller changes.

1. **The 410 items whose callers are mostly in one `runtime/*` topic move into that
   topic, one batch per topic.** Order the batches by the sum of their items'
   `called_by`, smallest first (equal sums by file name). Each batch opens `main.rs`
   and its topic.

   | # | topic | items (functions + methods) | call sites in |
   |---:|---|---:|---:|
   | 1 | `runtime/pointer/ending.rs`, `runtime/pointer/mod.rs`, `runtime/diagnostics.rs`, `runtime/profiles.rs`, `runtime/search.rs`, `runtime/tooltips.rs` | moved by batch 1 | — |
   | — | `runtime/profiles.rs` | closed by batch 16: `tab_surface`, tied with `runtime/tabs.rs`, went to `runtime/tabs.rs` by the owner column (§13 gives it the tab strip) | — |
   | 2 | `runtime/web.rs` | moved by batch 2 | — |
   | 3 | `runtime/dpi.rs` | moved by batch 3 (4 items; `files_float_content_height`, tied with `runtime/floats.rs`, waits for that batch; `release_due_leaf_resize` stays in `main.rs`, the single writer road of `LeafSession.birth` and `.grid`), with `Runtime::page_shrinker` as the web batch's tail | — |
   | 4 | `runtime/attention.rs` | moved by batch 4 (4 items; `Runtime::reveal_in_explorer`, tied with `runtime/configuration.rs` and `runtime/floats.rs`, held for a ruling, below) | — |
   | 5 | `runtime/palette.rs` | moved by batch 5 (6 items; `Runtime::focus_seat` held, below) | — |
   | 6 | `runtime/terminal.rs` | moved by batch 6 (13 items; `deliver_osc_attention` went to `runtime/attention.rs` instead, below) | — |
   | 7 | `runtime/git.rs` | moved by batch 7 (14 items, `Runtime::expand_commit` by a ruled exception, below), with `seat_holds_the_keyboard` and `apply_leaf_metrics` as the terminal batch's tail | — |
   | 8 | `runtime/peek.rs` | moved by batch 8 (14 items; five callers inside `runtime/peek.rs` dropped their `crate::` prefix, below) | — |
   | 9 | `runtime/clipboard.rs` | moved by batch 9 (13 items; `offer_pty_input`, `Runtime::paste_into_settings_field` and `Runtime::glass_here` held, below), with `unstarted_pixels` and `peek_cache_key_for_decode` as the peek batch's tail | — |
   | 10 | `runtime/floats.rs` | moved by batch 10 (10 items with `files_float_content_height`; `Runtime::settled_card_ink` went to `runtime/frame.rs` by its census owner; `Runtime::sweep_foot_phrases`, `Runtime::open_minted_page_on`, `revealable_preview_file`, `Runtime::open_local_path` and `reveal_ask` held, below), with nine clipboard tails | — |
   | 11 | `runtime/files.rs` | moved by batch 11 (16 items; `Runtime::focus_the_pane_a_path_landed_in` and `home_shortened_path` held, below; `float_dock_label` went to `runtime/floats.rs` by the owner column), with `Runtime::reference_cell_index`, `text_field_holding`, `paste_spelled` and `page_keepsake_icon` as tails | — |
   | 12 | `runtime/windows.rs` | moved by batch 12 (15 items; `revive_plan`, `create_tab_state` and `scrollback_quota` held, below; `end_leaves_of` went to `runtime/panes.rs` by the owner column), with `files_key_within` as a tail | — |
   | 13 | `runtime/frame.rs` | moved by batch 13 (24 items; five more went to the census owners of the facts they write, `runtime/terminal.rs`, `runtime/handoff.rs`, `runtime/web.rs`, `runtime/math.rs` and `runtime/keyboard.rs`, below; `Runtime::note_card_hint`, `Runtime::advance_card_hint`, `Runtime::advance_scheme_watch`, `Runtime::advance_storage_watch`, `mark_leaf_painted` and `math_copy_window` held, below) | — |
   | 14 | `runtime/math.rs` | moved by batch 14 (17 items with `math_copy_window`; the ties `Runtime::root_choices` and `rail_overlay_layer` went to `runtime/profiles.rs` and `runtime/panes.rs` by the owner column; `math_em_milli` and `window_layout_key` held, below), with eight tails: six into `runtime/frame.rs`, `composition_outlived_its_field` into `runtime/keyboard.rs` and `Runtime::collecting_card_speakers` into `runtime/terminal.rs` (`presentation_equivalent` held, below) | — |
   | 15 | `runtime/keyboard.rs` | moved by batch 15 (19 items; `Runtime::answer_quit_card` and `Runtime::note_user_typing` went to `runtime/windows.rs` and `runtime/terminal.rs` by their census owners and `Runtime::page_carried_by` to `runtime/floats.rs` by the owner column, below; `Runtime::show_settings_choice_at`, `Runtime::shortcut_focus`, `Runtime::apply_settings_choice`, `Runtime::toggle_settings_panel`, `Runtime::woke_terminal_thumb`, `ime_owner`, `popup_takes_the_key`, `terminal_ime_cursor_area`, `Runtime::send_user_input`, `Runtime::return_to_live_for_input` and `Runtime::cancel_composition` held, below), with five tails into `runtime/math.rs` and `runtime/frame.rs` | — |
   | 16 | `runtime/tabs.rs` | moved by batch 16 (14 items, with `tab_surface` by the owner column, closing the batch 1 tie row above; `tear_pane_into_tab` and `Runtime::arrival_fits` went to `runtime/panes.rs` and `Runtime::line_height_subpixels` to `runtime/mouse.rs`, below; `strip_insert_slot`, `solve_seats`, `presentation_physical_size`, `pane_can_become_a_tab`, `land_birth_death_fallback_shell`, `pane_into_tab`, `absorb_tab_sessions`, `pane_into_new_tab` and `Runtime::toggle_pin` held, below), with `grabbed_offset` and `new_tab_cwd` as tails | — |
   | 17 | `runtime/panes.rs` | moved by batch 18 (29 items with the tie `Runtime::plan_for` and the tails `birth_due`, `row_arrival_seat`, `frame_row_of_anchor` and `Runtime::head_that_raised_a_layer`; `expire_leaf_attention` went to `runtime/attention.rs` by its census owner, and `picture_channel_owner` and the tails `pane_can_become_a_tab` and `dirty_gate_names`, with its own tails `lines_phrase` and `unsaved_line`, to `runtime/preview.rs`, `runtime/tabs.rs` and `runtime/windows.rs` by the owner column, below; `restore_row_seed`, `Runtime::append_edge`, `auto_split_axis`, `decided_birth`, `resolved_birth_seed`, `create_leaf_session`, `born_in_tab`, `Runtime::focus_rail_geometry_now`, `Runtime::focus_rail_geometry_at`, `Runtime::plan_inputs_for`, `animated_pane_viewports`, `closing_this_pane_closes_the_tab`, `solve_tree`, `Runtime::aim_focus_card_window`, `schedule_leaf_grid_change`, `land_shell_birth`, `deliver_held_input`, `Runtime::focus_the_pane_a_path_landed_in`, `pane_into_new_tab` and `Runtime::focus_seat` held, below) | — |
   | 18 | `runtime/mouse.rs` | moved by batch 17, before row 17 by the live map (46 items with the ties `Runtime::glass_here`, `Runtime::note_terminal_thumb_hover`, `route_forwarded_mouse_button` and `Runtime::focus_pane_at` and four tails; `Runtime::browse_for_root`, `Runtime::toggle_switcher_pin` and the tail `switcher_pin_is_allowed` went to `runtime/files.rs` and `Runtime::settle_home` to `runtime/tabs.rs` by their census owners, and the tails `Runtime::command_tick_card`, `Runtime::woke_terminal_thumb`, `Runtime::wake_terminal_thumb`, `page_destination` and `switcher_row_destination` to `runtime/tooltips.rs`, `runtime/terminal.rs` and `runtime/web.rs` by the owner column, below; `Runtime::scroll_settings`, `Runtime::settings_mouse_input`, `Runtime::open_settings_on_row`, `Runtime::toggle_settings_panel`, `Runtime::send_user_input` and `Runtime::toggle_pin` held, below) | — |
   | 19 | `runtime/preview.rs` | cut into three batches by sub-topic (ruled at batch 19, below): batch 19 moved the document bodies (44 items, with `markdown_gap_paragraph` by the owner column, below; `push_markdown_table` held, below); batch 20 moved pictures, zoom, video and surfaces (40 items, with `animated_pane_viewports` by the owner column and `Runtime::play_button_rasters` by its census owner, below; eight held, below), and batch 21 moved pages, sources, the foot, crumbs, hand-off, refusal cards, switcher rows, the taskbar and landing (51 items: 50 into `runtime/preview.rs`, with the ties `Runtime::open_minted_page_on`, `revealable_preview_file`, `Runtime::gated_minted_target` and `Runtime::resizing_cards_frame` by the owner column and the foot pair `Runtime::foot_saying` and `Runtime::sweep_foot_phrases` together, and `Runtime::plan_inputs_for` to `runtime/panes.rs` by the owner column, below; `markdown_image_extent` held, below) | — |
   | — | the §13 re-sort (`docs/ARCHITECTURE.md` §13's "Still in `main.rs`") | moved by batch 22 (ruled 2026-10-26: 39 items out of `main.rs`, 7 methods and 32 free functions, and the whole `Runtime` blocks of `ime_outbound.rs` (8 methods) and `preview_viewport.rs` (4) into `runtime/keyboard.rs` and `runtime/preview.rs`; `Runtime::reveal_in_explorer` and `reveal_ask` to `runtime/handoff.rs` by clause (b); the six focus-card rows to `runtime/panes.rs` by clause (a); `Runtime::focus_seat`, `Runtime::focus_the_pane_a_path_landed_in` and `Runtime::settle_focus_on` to `runtime/mouse.rs` by the multi-owner rule, below; the three function values `web_page_cursor`, `files_a_tab_stands_on` and `seeded_tab` by first caller; the five holds the two blocks released (`ime_owner`, `popup_takes_the_key`, `terminal_ime_cursor_area`, `preview_caret_prose` and `scale_ppm`) with the tail `window_ime_cursor_area`; twelve rows beside their callers outside `runtime/`, with seven tails: `window_seam.rs` 4, `computation_lane.rs` 7, `preview_viewport.rs` 3, and one each into `handoff_lane.rs`, `update_job.rs`, `present_gate.rs`, `file_peek.rs` and `explorer_menu.rs`, below; `native_window_of`, `option_as_alt`, `is_format_or_bidi_control`, `measure_markdown_local` and `markdown_prose_face` held, below) | — |
   | — | `runtime/settings.rs` (re-sort D5) | moved by batch 23 (ruled 2026-10-26, which opened the file: 45 items out of `main.rs`, 43 into `runtime/settings.rs` (38 methods, the four `PointerHost` methods as one trait-impl block, and `contains_point`) and `Runtime::open_local_path` with `open_local_path_ask` into `runtime/handoff.rs` (D6); the four facts that already had several writer modules and gained `settings` are accepted by the shared-fact rule, below; `Runtime::add_scheme`, `Runtime::advance_scheme_watch`, `Runtime::advance_storage_watch` and `Runtime::adopt_option_as_alt` held by the census rule and `Runtime::reread_schemes`, `Runtime::apply_scheme`, `Runtime::open_scheme_for_editing` and `Runtime::settings_closed` with their root callers, below) | — |
   | — | `runtime/settings.rs` and `runtime/mouse.rs` (the settings topic's rest) | moved by batch 24 (ruled 2026-10-27: 10 `Runtime` methods out of `main.rs`; `Runtime::settings_layout`, `Runtime::drag_settings_menu_bar` and `Runtime::apply_slider`, with the six tails `Runtime::settings_content`, `Runtime::settings_dialog`, `Runtime::settings_values`, `Runtime::editor_subject`, `Runtime::advanced_open` and `Runtime::scheme_in_force` (the last at the review fold), into `runtime/settings.rs`; `Runtime::send_user_input` into `runtime/mouse.rs` by the multi-owner rule, below; `rail_state_for` and `Runtime::return_to_live_for_input` held, below) | — |
   | — | `runtime/panes.rs`, `runtime/tabs.rs`, `runtime/attention.rs`, `runtime/terminal.rs`, `runtime/clipboard.rs` and `runtime/frame.rs` (the K2 tail's first batch) | moved by batch 25 (ruled 2026-10-28: the fifteen free functions O-4b and O-5a released, out of `main.rs` to the owners their release lines name, below; the input door's pair split by first caller, `write_pty_input` to terminal and `offer_pty_input` to clipboard) | — |
   | — | `runtime/tabs.rs` and `runtime/terminal.rs` (the K2 tail's second batch) | moved by batch 26 (ruled 2026-10-29: G5's six free functions and G15's `Runtime::toggle_pin` into `runtime/tabs.rs`, which replaces the root as a writer of the ten `TabState` facts G5 alone wrote at the root (the "replace, not split" rule, below); `Runtime::return_to_live_for_input` into `runtime/terminal.rs` by the owner tie-break; G3's seven held for plan row 105, below) | — |

   **From batch 14 on, the order is the live map's** (ruled at batch 14). The smallest-sum-first rule above is
   applied to the map regenerated at each batch's base, not to this table's numbers, which were read at batch 1's
   head and drift as items move. At batch 14's base `runtime/math.rs` had 16 items (11 + 5) with a sum of 44 and
   `runtime/keyboard.rs` 26 (14 + 12) with a sum of 52, so math went first. The rows not yet moved keep their batch
   1 numbers; each batch's judgement reads the live ones. At batch 17's base `runtime/mouse.rs` had 44 items
   (28 + 16) with a sum of 70 and `runtime/panes.rs` 33 (24 + 9) with a sum of 75, so row 18 went before row 17.
   At batch 18's base `runtime/panes.rs` had the same 33 items (24 + 9) and `runtime/preview.rs`, next, a sum of 144;
   every topic with a smaller sum held only held rows.
   At batch 19's base `runtime/preview.rs` was next, with 91 items (72 + 19) and a sum of 144; with its tails to a
   fixed point it came to 139 items, 134 after the holds, so it was cut into three batches by sub-topic, each under
   about 60 items (ruled at batch 19): the document bodies (44), then pictures, zoom, video and surfaces (39), then
   pages, sources, the foot, crumbs, hand-off, refusal cards, switcher rows, the taskbar and landing (51). The order
   is the one in which every tail's root callers move in the same batch or an earlier one. At batch 20's base the
   pictures group, re-derived against the live map, was still 39, and `animated_pane_viewports` joined it as a tail,
   its only root caller, `preview_image_placement`, moving in the same batch: 40. At batch 21's base the third group
   was 50, the 51 less `animated_pane_viewports`, which batch 20 took as a tail; the foot pair (ruled at batch 21,
   below) made it 51 again.

   **An item stays in `main.rs`, whatever its `called_from` row proposes, when moving it would make a field it
   writes a multi-writer fact** (the ownership census refuses a new one without a ruled owner; the standing rule is
   that such a move goes back through the original single writer). `release_due_leaf_resize` is the first: it writes
   `LeafSession.birth` and `LeafSession.grid`, whose other writers stay at the root, and batch 3 moved it back. It
   stays in `main.rs` until an owner is ruled for those two facts; `unclassified.tsv` keeps proposing
   `runtime/dpi.rs` for it (that column is generated), and no later batch retries the move. Re-cited 2026-10-27
   (O-5a, the contract's M8 rename): the two facts are `ShellSession.life` (O-4b folded `birth` into it) and
   `ShellSession.grid`, and `grid`'s other writer is `ShellSession::propose_size`, the session's own method in
   `main.rs`; the hold stands (G3, ruling E12: it closes only by a ruling that absorbs the release into
   `ShellSession`). Re-cited 2026-10-28 (O-5b): both facts are still the record's and keep these writers (the
   pane's map value changes neither), so G3's seven rows stay held.

   **A fact with no census owner moves with all its writers to the first caller's module, never one writer alone**
   (ruled at batch 21). Moving one of its writers would leave the fact written from two modules with no owner to
   rule between them, which the rule above refuses; moving all of them keeps it at one writer module. Batch 21's
   `Runtime::foot_saying` and `Runtime::sweep_foot_phrases` are the only writers of `WindowRuntime.foot_phrases`,
   which the census does not list and §13 gives no owner; both went to `runtime/preview.rs`, `foot_saying`'s first
   caller, and the fact's writer modules went from the root to `runtime/preview.rs`.

   **An item writing facts of two or more owners moves to a module that already writes every one of those facts,
   if exactly one exists; otherwise it stays held** (ruled 2026-10-26, at the §13 re-sort). Batch 22's
   `Runtime::focus_seat`, `Runtime::focus_the_pane_a_path_landed_in` and `Runtime::settle_focus_on` write
   `WindowRuntime.float` (owner floats), `WindowRuntime.search` (owner search) and `TabState.seats` (owner panes), and
   `runtime/mouse.rs` is the only module that already writes all three, so they went there and it became a writer of
   no new fact. The writer modules: `WindowRuntime.float` went from {root, dpi, files, floats, git, mouse, panes, peek,
   preview} to {dpi, files, floats, git, mouse, panes, peek, preview}; `WindowRuntime.search` stays {root, git, mouse,
   search}, because the held `Runtime::cancel_composition` keeps the root a writer; `TabState.seats` stays {root,
   attention, files, floats, mouse, panes, preview, terminal, web}, because `absorb_tab_sessions` and `pane_into_tab`
   stay. Batch 26 (2026-10-29) moved both to `runtime/tabs.rs`, so the root left that set and tabs joined it.

   **A row whose callers are outside `runtime/` moves beside them as a module function, with its tails** (ruled
   2026-10-26, at the §13 re-sort). Root callers import it from the child module (`use window_seam::…`, the root to
   child direction, which the import rule below does not forbid); `runtime/` callers import it from
   `crate::<module>`; a caller in the target module drops the `crate::` prefix, as in batch 8. A root function whose
   only product callers move in the same batch into such a module follows them there, as a runtime tail does. Batch 22
   moved twelve such rows and seven tails.

   **K2 also moves the `Runtime` impl blocks of files outside `runtime/` into the topic §13's column names** (ruled
   2026-10-26, at the §13 re-sort; §13's title is every `Runtime` method in `runtime/`). A file whose one `Runtime`
   block moves whole loses the block, header and all. The private helpers the moved bodies call in their old file
   are widened to `pub(super)`, visibility only, so the bodies move unchanged (the precedent for widening: no body is
   rewritten to reach a private item); a moved method that a held root item calls becomes `pub(crate)`. Batch 22
   moved `ime_outbound.rs`'s block (8 methods) into `runtime/keyboard.rs` and `preview_viewport.rs`'s (4) into
   `runtime/preview.rs`; `Runtime::trace_ime_cancel` and `Runtime::destroy_ime_caret`, which the held
   `Runtime::cancel_composition` calls, are `pub(crate)`.

   **K2 opens `runtime/settings.rs`** (ruled 2026-10-26, re-sort D5, reversing 2026-10-24's "K2 opens no
   `runtime/settings.rs`"). Batch 23 moved the settings group there, with `Runtime::paste_into_settings_field` and
   `Runtime::write_editor_field` (D3 of the re-sort); the held entries below that rested on the 2026-10-24 rule say
   which closed. The module now exists, so the census owner `main[settings]` is renamed `settings` in the four
   annotation rows that named it (`App.settings_store`, `WindowRuntime.advanced_reveal_sample`,
   `WindowRuntime.settings`, `WindowRuntime.settings_scroll`), and `runtime/settings.rs` is that owner's module for
   clause (a). A whole trait-impl block for `Runtime` moves as one block of its own, after the destination's
   inherent block (ruled at batch 23, D5: `move.py`'s trait-block shape), as the `PointerHost` impl did.

   **A fact that already has several writer modules may gain the module of the page that writes it** (ruled
   2026-10-26, at batch 23, D1). The census rule above holds an item whose move would make a single-writer fact a
   two-module one; a fact that is multi-writer already does not hold the move when the module it gains is the owner
   of the page whose code writes it. Batch 23's four: `App.update_job` ({root, update_card} → {root, settings,
   update_card}; `apply_settings_choice_announcing` through `show_failure`, the Updates row), `LeafSession.session`
   (ten writer modules → eleven; `apply_block_max_height` and `apply_repair_row_breaks` through the terminal's own
   setter `set_math_layout_options`), and `WindowRuntime.focus_mode` and `WindowRuntime.focus_reveal` ({root,
   windows} → {root, settings, windows}; `set_focus_mode`, the Appearance page's toggle). A fact going from one
   writer module to two still holds the item.

   **When the ownership census names an owner for a field an item writes, the owner column wins over the first
   caller** (a judged exception, batch 6). `deliver_osc_attention` is called only from `Runtime::drain_pty` in
   `runtime/terminal.rs`, so `unclassified.tsv` proposed that file; it writes `LeafSession.attention`, whose census
   owner is "attention (→ bt-workbench)", and `docs/ARCHITECTURE.md` §12.1 lists it with the attention routing. It
   went to `runtime/attention.rs`, which keeps that fact at two writer modules (the root and `runtime/attention.rs`)
   instead of three. An item that writes facts of two different census owners is held for a ruling instead.
   Ruled so far: `Runtime::expand_commit` (batch 7) writes `TabState.files` (owner files) and `TabState.git_trees`
   (owner git); it went to `runtime/git.rs`, its first caller and one of its owners, because both facts were
   already written from the root, `runtime/files.rs`, `runtime/git.rs` and `runtime/panes.rs`, so the move adds no
   writer module.

   **The owner column decides only when it changes something** (refined at batch 12; reworded after its review).
   A census owner does not win just because it exists. The owner column decides in two cases:
   (a) when the move would change the writer set of a fact the item writes, the owner's module takes the item;
   (b) when the callers tie, §13's owner column settles the tie, as it did for `float_dock_label`,
   `files_float_content_height`, `Runtime::glass_here`, `Runtime::open_minted_page_on` and
   `revealable_preview_file`. Otherwise the first caller decides. The rule exists to keep a fact's writer modules
   from growing and to move a writer toward its owner. `end_leaves_of` (batch 12) writes `LeafSession.pty`, whose
   census owner is panes. Either home adds a writer module: its first caller, `runtime/windows.rs`, would add a
   non-owner, and `runtime/panes.rs` is a new writer of that fact too. What decides is that panes is the owner, so
   it went to `runtime/panes.rs` (with `LeafSession.attention_ended`, which it alone writes). `answer_an_exit_over`
   (batch 12) writes `TabState.preview_pool`, whose census owner is preview. `runtime/windows.rs` and
   `runtime/preview.rs` both write that fact already, so either home leaves the writer set as it is, and it has one
   first caller and no tie. Neither case applies, so the first caller stands, and it went to `runtime/windows.rs`,
   beside `answer_exit`, as an exit-gate step.
   Batch 13 sent five methods whose first caller is `runtime/frame.rs` to their facts' owners by clause (a),
   because `runtime/frame.rs` would have been a new writer and the owner's module already writes the fact:
   `Runtime::advance_command_flash` (`WindowRuntime.command_flash`) to `runtime/terminal.rs`,
   `Runtime::advance_foot_reveal` (`WindowRuntime.revealed_foot`) to `runtime/handoff.rs`,
   `Runtime::advance_page_foot_clocks` (`WindowRuntime.web`) to `runtime/web.rs`, `Runtime::carry_live_journeys`
   (`WindowRuntime.formula_overlay_owed`, of which math becomes the only writer) to `runtime/math.rs`, and
   `Runtime::settle_composition_owner` (`WindowRuntime.composing`) to `runtime/keyboard.rs`.
   Batch 14 settled two ties by clause (b): `Runtime::root_choices` (math/mouse/profiles) went to
   `runtime/profiles.rs`, which owns the root menu, and `rail_overlay_layer` (math/panes) to `runtime/panes.rs`,
   which owns command rails and holds `rail_overlay_layers`. `Runtime::collecting_card_speakers`, tied
   frame/terminal where §13 names neither (both callers are drain roads), went to `runtime/terminal.rs`, the home of
   `Runtime::drain_pty`, by a ruling.
   Batch 15 sent two methods whose first caller is `runtime/keyboard.rs` to their facts' owners by clause (a):
   `Runtime::answer_quit_card` (`App.quit`, written by the root and `runtime/mouse.rs`, so either home adds a writer
   module) to `runtime/windows.rs`, the owner, as for `end_leaves_of`; and `Runtime::note_user_typing`
   (`LeafSession.session`, which `runtime/terminal.rs` already writes and `runtime/keyboard.rs` does not) to
   `runtime/terminal.rs`. It settled `preedit_caret_byte` (keyboard/preview) by clause (b), since §13 gives the IME
   caret to `runtime/keyboard.rs`, and sent `Runtime::page_carried_by` (keyboard/panes, a float's page, which §13
   gives to neither) to `runtime/floats.rs` by a ruled owner-column exception, as `float_dock_label`.
   Batch 16 sent `tear_pane_into_tab`, whose first caller is `runtime/tabs.rs`, to `runtime/panes.rs` by clause (a):
   it writes `TabState.seats` (owner panes), which `runtime/panes.rs` writes and `runtime/tabs.rs` does not, and
   `TabState.landing`, of which it is the only writer, moved with it. It settled three ties by clause (b):
   `Runtime::arrival_fits` (panes/tabs) to `runtime/panes.rs`, which owns the move of panes;
   `Runtime::line_height_subpixels` (mouse/tabs, a wheel notch's distance) to `runtime/mouse.rs`, which owns the
   wheel; and `tab_surface` (profiles/tabs, the batch 1 tie row) to `runtime/tabs.rs`, which owns the tab strip.
   Batch 17 sent three methods whose first caller is `runtime/mouse.rs` to their facts' owners by clause (a):
   `Runtime::browse_for_root` (`WindowRuntime.folder_pick`) and `Runtime::toggle_switcher_pin` (`App.pins_store`),
   both owned by files, to `runtime/files.rs`, with `switcher_pin_is_allowed` as a tail; and `Runtime::settle_home`
   (`TabState.flip`, owner tabs) to `runtime/tabs.rs`. It settled its ties by clause (b): `Runtime::glass_here`
   (clipboard/mouse) and `Runtime::note_terminal_thumb_hover` and `route_forwarded_mouse_button` (mouse/pointer,
   which §13 does not list) to `runtime/mouse.rs`, which owns drag and drop, hover and `mouse_input`'s rungs;
   `Runtime::focus_pane_at` (mouse/preview/web) to `runtime/mouse.rs`, which already writes its one fact; and,
   among the tails, `link_target_to_ask_about` (mouse/terminal) to `runtime/mouse.rs`, which owns asking about the
   link under the pointer, `Runtime::command_tick_card` (mouse/tooltips, the rail's glance card) to
   `runtime/tooltips.rs`, `Runtime::woke_terminal_thumb` (keyboard/terminal) to `runtime/terminal.rs`, which owns
   the scroll and column bars, `Runtime::wake_terminal_thumb` (mouse/terminal, the clock that keeps the same bar
   awake; ruled at the batch 17 review, and `LeafSession.thumb_awake`, of which it is the only writer, went with it)
   there too, and `page_destination` (files/preview/web, a stored row's navigation, which
   `runtime/web.rs`'s `revived_page_of` already asks) to `runtime/web.rs`, with its tail
   `switcher_row_destination`.
   Batch 18 sent `expire_leaf_attention`, whose first caller is `runtime/panes.rs`, to `runtime/attention.rs` by
   clause (a): it writes `LeafSession.attention` (owner attention), which the root and `runtime/attention.rs` write
   and `runtime/panes.rs` does not. It settled its ties by clause (b): `Runtime::plan_for` (panes/preview) to
   `runtime/panes.rs`, which owns seat layout and the move of panes, with its tail `row_arrival_seat`;
   `picture_channel_owner` (panes/preview, a preview picture's placement) to `runtime/preview.rs`; and, among the
   tails, `frame_row_of_anchor` (panes/search, read by the command flash) to `runtime/panes.rs`, which owns command
   rails, `Runtime::head_that_raised_a_layer` (panes/floats/tooltips, the second of `Runtime::head_run`'s two facts,
   beside `popup_owner`) to `runtime/panes.rs`, `pane_can_become_a_tab` (panes/tabs) to `runtime/tabs.rs`, which owns
   tear out, and `dirty_gate_names` (panes/tabs/windows) to `runtime/windows.rs`, which owns the dirty gate, with its
   own tails `lines_phrase` and `unsaved_line`.
   Batch 20 sent `Runtime::play_button_rasters`, whose first caller is `runtime/preview.rs`, to `runtime/frame.rs` by
   clause (a): it writes `WindowRuntime.chrome_marks` (owner frame), whose writer modules were the root and
   `runtime/floats.rs`; its first caller would have left them at `runtime/floats.rs` and `runtime/preview.rs`, and
   the owner's module leaves them at `runtime/floats.rs` and `runtime/frame.rs`. It settled the tail
   `animated_pane_viewports` (panes/preview, the pane-motion viewports) by clause (b) to `runtime/panes.rs`, which
   owns the move of panes.
   Batch 21 settled its ties by clause (b): `Runtime::open_minted_page_on` and `revealable_preview_file`
   (floats/preview, ruled at batch 10) to `runtime/preview.rs`, which owns opening preview panes and their doors;
   among the tails, `Runtime::gated_minted_target` (preview/tabs, the gate a minted navigation leaves by) to
   `runtime/preview.rs` with `Runtime::open_minted_page_on`, called by `runtime/tabs.rs`'s `open_minted_page`;
   `Runtime::resizing_cards_frame` (floats/preview) to `runtime/preview.rs` beside `Runtime::drawn_resizing_card`,
   called by `runtime/floats.rs`'s `refresh_chrome_with_overlay`; and `Runtime::plan_inputs_for` (panes/preview, the
   drop plan's inputs and cache key) to `runtime/panes.rs` beside `Runtime::plan_for`, which owns seat layout and the
   move of panes.

   **A product caller in another `runtime/` file imports the moved item from its new path** (`use
   crate::runtime::<topic>::<item>;`, an import line and no body text; ruled at batch 7). What stays forbidden is a
   non-test `use runtime::…` at the crate root to keep a root caller compiling: an item with a product caller at the
   root waits until that caller moves. A caller inside the target module that named the item by its root path drops
   the `crate::` prefix (`crate::x(` becomes `x(`; ruled at batch 8, where five calls in `runtime/peek.rs` did); no
   root re-export. A caller in another `runtime/` file that named the item by its root path imports it and drops
   the prefix the same way (ruled at batch 17, where `runtime/pointer/release.rs` called
   `crate::button_router_position(`).

   **A test re-export whose test callers are all platform-gated carries the same cfg** (ruled 2026-10-26, after
   batch 21's chain failed on macOS: `unverified_reference_handoff`'s only test caller, in `tests.rs`, is
   `#[cfg(windows)]`, and the ungated re-export was an unused import there). Both sides carry it: the
   `runtime/mod.rs` re-export and the root's `use runtime::…` line, each `#[cfg(all(test, windows))]` on a line of
   its own, which puts `runtime/mod.rs` on `FILES_THAT_MAY_NAME_A_PLATFORM`. The review checks the platform of
   every re-export's callers.

   **The topic modules are `pub(crate)` for rustdoc, and the import rule above is kept by a guard** (ruled
   2026-10-27, plan row 59; the guard 2026-10-27, plan row 80). A doc link names a moved item by its path
   (`runtime::<topic>::name` at the root, `super::<topic>::name` inside `runtime/`), and rustdoc resolves that
   path with privacy, so the eighteen topic modules a link names are `pub(crate) mod` in `runtime/mod.rs`, and
   eleven linked private functions are widened (`pub(super)` or `pub(crate)`). The trade is wider than an
   import: any path into a topic module compiles at the crate root and in every module outside `runtime/`, a
   `use runtime::<topic>::…` item and a full path in an expression or a type (`runtime::clipboard::paste_road(…)`)
   alike, with no `use` at all. `crates/bt-source/tests/root_topic_paths.rs` refuses them, reading every path,
   not `use` items alone (`docs/ARCHITECTURE-GUARDS.md`, "Runtime topics from outside"). `runtime/pointer` counts
   as a topic module (ruled 2026-10-27, row 59's review); the root's seventeen product paths into
   `runtime::pointer::…` the guard found first now go through twelve names `runtime/mod.rs` re-exports.

   **A batch updates `docs/ARCHITECTURE.md` §13 in three places**: the topic table's method counts for the
   files it fills, the "through batch N" list of what each batch moved in (the paragraph above the table), and
   the count sentence under "Still in `main.rs`" (the 2026-09-21 table under it is retired, 2026-10-27, plan
   row 59).

   **A moved reader's imports are counted by needle** (ruled 2026-10-27, in K2 batch 23).
   `POINTER-DEBT.tsv`'s rows carry the needle each occurrence matched. `check-pointer-debt.ps1` accepts an
   `(outside any item)` row in the module a reader moved into once per needle that the moved owner rows at that
   module and door read, and refuses a needle none of them reads. Before this ruling the cap was the count of
   moved owners (ruling Q1 (b)), which refused batch 23's `runtime/settings.rs`: two moved owners there read three
   button and wheel types, so the file has three imports and no new reader. An import row matches a base row only
   of its own needle (or one written before the column), so the imports judged are the ones added, whatever their
   order in the list (batch 23's review, finding 1).

   **Held items** (each row's generated `called_from` home is not where it goes):
   - **Batch 26 (2026-10-29) moved eight of the fifteen rows batch 25 left** (the batch 26 rulings 1–3), and
     their entries leave this list: G5's `pane_into_tab`, `pane_into_new_tab`, `absorb_tab_sessions`,
     `move_seat_content`, `forget_work_in_flight_for_seat` and `closing_this_pane_closes_the_tab`, with G15's
     `Runtime::toggle_pin`, into `runtime/tabs.rs`; `Runtime::return_to_live_for_input` into
     `runtime/terminal.rs` by the owner tie-break (terminal owns `ShellSession.session`; it and panes already
     wrote both its facts, so no module joined either). **The "replace, not split" rule** (ruling 1): moving the
     only root writers of a fact replaces the root on that fact and adds no second writer module. G5 was the
     root's only writer of ten `TabState` facts (`file_trees`, `files`, `git_scroll`, `git_trees`, `pinned`,
     `preview_panes`, `preview_views`, `seat_layout`, `seat_overflow` and `seats`), and each now names
     `runtime/tabs.rs` where it named the root; `focused_leaf` and `git_graph_view`, whose other root writers
     (`TabState::refocus_after_losing`, a door, and the held `Runtime::cancel_composition`) stay, gain exactly
     `runtime/tabs.rs`. `TabState.pinned` moves whole to its owner, tabs (ruling 2). Seven stay held: G3's
     `release_due_leaf_resize` and its six tails, for plan row 105 (below).
   - **Batch 25 (2026-10-28) moved the fifteen rows O-4b and O-5a released** (rulings D15, E8, E9), and their
     entries leave this list: `land_shell_birth`, `deliver_held_input` and `apply_owed_shell_resize` into
     `runtime/panes.rs`; `land_birth_death_fallback_shell` into `runtime/tabs.rs`; `settle_attention` and
     `tab_queue_leader` into `runtime/attention.rs`, where `ShellSession.session` gains its eleventh writer
     module, as D15 admitted; `drain_leaf_pty`, `synchronized_update_committed`, `terminal_palette`,
     `terminal_palette_in_force`, `output_revision` and `write_pty_input` into `runtime/terminal.rs`;
     `offer_pty_input` into `runtime/clipboard.rs`; `mark_leaf_painted` and `seen_revision` into
     `runtime/frame.rs`. `drain_leaf_pty`'s doc, which stood above `synchronized_update_committed`, now stands
     above it. Fifteen stay held: `Runtime::return_to_live_for_input`, G3's seven, G5's six and G15's one.
     The `//` note that stood above `tab_queue_leader` ("`attention_is_asked` and `attention_ticket` stood here
     until A2…") is blank-separated from it and stays in `main.rs`, for row 78 (ruling R6).
   - `release_due_leaf_resize` — stays in `main.rs` (the census rule above). Re-cited 2026-10-29 (K2 batch 26,
     ruling 4): absorbed by plan row 105 (`T-SHELL-SESSION-RELEASE-DUE-SIZE`), which makes it
     `ShellSession::release_due_size`, a door beside `ShellSession::propose_size`, so `grid` and `conpty_grid` are
     written only by the session's own methods. Its six tails, `service_pending_pty_resize`,
     `take_due_pty_resize`, `pty_resize_wake_deadline`, `commit_leaf_resize`,
     `replace_psreadline_resize_reanchor_debt` and `psreadline_resize_repaint_input`, stay root-called beside it;
     the seven rows stay held until row 105 lands.
   - `files_float_content_height` (tied `runtime/dpi.rs` / `runtime/floats.rs`) — the owner column
     (`docs/ARCHITECTURE.md` §13) gives it to `runtime/floats.rs` ("floating windows"); batch 10 moved it there.
   - `Runtime::reveal_in_explorer` — a three-way tie (`runtime/attention.rs` / `runtime/configuration.rs` /
     `runtime/floats.rs`) that the owner column does not settle: §13's "Still in `main.rs`" table lists it
     unassigned, and its asks column names "hand-off (reveal)" for both `files.rs` and `floats.rs`. At batch 10
     its callers were seven, one in each of seven `runtime/` files, and it has no census owner; it stays held,
     and its home is ruled at the §13 re-sort at the end of this table, not per batch. The re-sort (2026-10-26)
     gave it, with `reveal_ask`, to `runtime/handoff.rs` by clause (b): §13 gives that file the `Runtime` side of
     the OS hand-off lane. Batch 22 moved both.
   - `Runtime::focus_seat` — its first `called_from` place is `runtime/palette.rs`, but that is three of eight
     callers (the others are `runtime/web.rs`, `runtime/attention.rs`, `runtime/preview.rs` and `main.rs`) and it
     is a general focus door, not the command palette's; batch 5 held it, and it waits for the topic that owns
     seats (`runtime/panes.rs`, "seat layout"), so that it moves once. Batch 18, that topic, leaves it: its one root
     product caller is `Runtime::focus_the_pane_a_path_landed_in`, held below for a ruling (the rule above).
     Batch 22 moved both, with `Runtime::settle_focus_on`, to `runtime/mouse.rs` by the multi-owner rule (above).
   - `hyperlink_activation` — its `called_from` home is `runtime/terminal.rs`, but `ControlClickHint::of` at the root
     calls it in the product (the rule above); it moves when `ControlClickHint` does.
   - `Runtime::paste_into_settings_field` — its `called_from` home is `runtime/clipboard.rs`, but it writes
     `WindowRuntime.settings`, whose census owner is `main[settings]`; the owner column wins, and K2 opens no
     `runtime/settings.rs`, so it stays in `main.rs` with the settings topic. The hold rests on that write:
     `Runtime::settings_field_holds_the_keyboard`, which only reads the settings field and whose one caller is
     `Runtime::clipboard_focus`, went to `runtime/clipboard.rs` as a tail in batch 10.
     What holds it is that `runtime/clipboard.rs` would have been a new writer of `WindowRuntime.settings`.
     Batch 15 moved `Runtime::record_settings_key` and `Runtime::settings_field_key` to `runtime/keyboard.rs`,
     which already writes that fact, so neither adds a writer module. `settings_field_key` writes nothing else, and
     its first caller stands. `record_settings_key` also writes `App.shortcuts`, whose owner is keyboard and which
     `runtime/keyboard.rs` already writes; it went to one of its two owners, as `Runtime::expand_commit` did, and the
     root stops writing `App.shortcuts`. Batch 23 moved `paste_into_settings_field` to `runtime/settings.rs`, once K2
     opened that file (2026-10-26, above).
   - `Runtime::note_card_hint` and `Runtime::advance_card_hint` (batch 13) — their `called_from` home is
     `runtime/frame.rs`, but `note_card_hint` writes `App.settings_store`, whose census owner is
     `main[settings]`, and `runtime/frame.rs` does not write it; K2 opens no `runtime/settings.rs`, so it stays,
     as `paste_into_settings_field` does. The two are `WindowRuntime.card_hint`'s only writers, so
     `advance_card_hint` stays with it: moving one alone would make that fact a new two-module one.
     Batch 23 moved both to `runtime/settings.rs` (2026-10-26, above); `WindowRuntime.card_hint` went from the root
     to `runtime/settings.rs` whole.
   - `Runtime::advance_scheme_watch` and `Runtime::advance_storage_watch` (batch 13) — their
     `called_from` home is `runtime/frame.rs`, but they write `App.scheme_watch` (other writers `add_scheme` and
     `create`) and `App.storage_watch` (other writer `create`), whose other writers stay at the root (the census
     rule above). `advance_storage_watch`
     also writes facts of several census owners. At batch 23 the first two still hold in the settings topic's
     rest: `App.scheme_watch` and `App.storage_watch` would go from the root alone to the root and
     `runtime/settings.rs` (`Runtime::create` keeps writing both), and `Runtime::add_scheme`, the other writer of
     `App.scheme_watch`, holds with them.
   - `Runtime::adopt_option_as_alt` (batch 23) — in the settings group, but it writes `WindowRuntime.modifiers`
     (owner keyboard), which `FolioApp::on_window_event` writes at the root, so `runtime/settings.rs` would make it a
     two-module fact (the census rule above).
   - `Runtime::reread_schemes`, `Runtime::apply_scheme`, `Runtime::open_scheme_for_editing` and
     `Runtime::settings_closed` (batch 23) — in the settings group and blocked by none of their own facts, but each
     has a root product caller that holds (`advance_scheme_watch`; `add_scheme` and `reread_schemes`; `add_scheme`;
     `open_scheme_for_editing`), so they stay with their callers (ruled 2026-10-26, D2) and move when the scheme
     watch does.
   - `Runtime::settings_values`, `Runtime::settings_layout`, `Runtime::drag_settings_menu_bar` and `rail_state_for`
     (after batch 23; batch 23's review, finding 6) — not judged in batch 23. Their generated `called_from` home is now
     `runtime/settings.rs`, because their callers moved there. Batch 24 (2026-10-27) judged them: the three methods
     moved to `runtime/settings.rs`, `Runtime::settings_layout` and `Runtime::drag_settings_menu_bar` with
     `Runtime::apply_slider`, and `Runtime::settings_values` as one of five root-only tails (with
     `Runtime::settings_content`, `Runtime::settings_dialog`, `Runtime::editor_subject` and `Runtime::advanced_open`).
     Batch 24's review fold moved a sixth tail, `Runtime::scheme_in_force` (private; its only product caller is
     `settings_values`): the judgement's name scan had counted bt_render's free `scheme_in_force()` as a root caller.
     `rail_state_for` stays: its root product caller is `Runtime::create`, which holds, so it moves when `create`
     does.
   - `math_copy_window` (batch 13) — tied `main.rs` / `runtime/frame.rs` / `runtime/math.rs`, and
     `retire_spent_math_copy` at the root called it in the product (the rule above); batch 14 moved both to
     `runtime/math.rs`.
   - `Runtime::glass_here` — tied `runtime/clipboard.rs` / `runtime/mouse.rs`; its doc makes it the one door
     `Self::drive_drag` asks before it surveys, and the owner column gives "drag and drop" to `runtime/mouse.rs`;
     batch 17 moved it there.
   - `Runtime::sweep_foot_phrases` — its `called_from` home is `runtime/floats.rs`, but it writes
     `WindowRuntime.foot_phrases`, whose other writer, `Runtime::foot_saying`, stayed at the root (the census rule
     above); it moved when `foot_saying` did. A fact with no owner moves with all its writers to the first caller's
     module, never one writer alone (ruled at batch 21, above): batch 21 moved both to `runtime/preview.rs`, called by
     `runtime/floats.rs`'s `refresh_chrome_with_overlay`.
   - `Runtime::open_minted_page_on` and `revealable_preview_file` — tied `runtime/floats.rs` /
     `runtime/preview.rs`; the owner column gives preview panes and their doors to `runtime/preview.rs`; batch 21,
     the third of the preview batches, moved both there.
   - `Runtime::open_local_path` — its `called_from` home is `runtime/floats.rs`, but
     `Runtime::apply_settings_choice_announcing` at the root calls it in the product (the rule above). Batch 23
     moved that caller to `runtime/settings.rs`, which closed the hold; its callers are then `runtime/floats.rs`,
     `runtime/files.rs` and `runtime/settings.rs`, and the re-sort's D6 sent it, with `open_local_path_ask`, to
     `runtime/handoff.rs`, which §13 gives the `Runtime` side of the OS hand-off lane.
   - `reveal_ask` — its `called_from` home is `runtime/floats.rs`, but `Runtime::reveal_in_explorer` at the root
     calls it in the product (the rule above); it stays with `reveal_in_explorer`. Batch 22 moved both to
     `runtime/handoff.rs` (above).
   - `Runtime::focus_the_pane_a_path_landed_in` — tied `runtime/files.rs` / `runtime/panes.rs`, and it writes facts
     of two census owners, `WindowRuntime.float` (floats) and `WindowRuntime.search` (search); held for a ruling
     (the two-owners clause above). The multi-owner rule (ruled 2026-10-26, above) sent it to `runtime/mouse.rs`
     in batch 22.
   - `home_shortened_path` — its `called_from` home is `runtime/files.rs`, but `cwd_whole` at the root calls it in
     the product (the rule above).
   - `float_dock_label` (batch 11) — a three-way tie (`runtime/files.rs` / `runtime/panes.rs` /
     `runtime/preview.rs`) that none of the three owns; it is a float's dock button, and the owner column gives
     floating windows to `runtime/floats.rs`, so it went there (a ruled owner-column exception), imported by all
     three.
   - `revive_plan`, `create_tab_state` and `scrollback_quota` (batch 12) — their `called_from` home is
     `runtime/windows.rs`, but the root calls each in the product (the rule above); they wait for that caller.
   - `presentation_equivalent` (batch 14) — its root caller, `pty_frame_is_unchanged`, moved to `runtime/frame.rs`
     in batch 13, but `present_gate.rs`, a module outside `runtime/`, calls it by its root path
     (`crate::presentation_equivalent(`). Moving it would change that call's text or need a root re-export, so it
     stays. Batch 22 moved it into `present_gate.rs`, beside its caller there, `pictures_match` (the
     outside-callers rule, above): that call drops its `crate::` prefix, and `runtime/frame.rs` imports it from
     `crate::present_gate`.
   - `math_em_milli` and `window_layout_key` (batch 14) — their `called_from` home is `runtime/math.rs`, but
     `PreviewMathCache::picture` and `bare_leaf` at the root call them in the product (the rule above).
   - `Runtime::cancel_composition` (batch 14) — no root product caller, and its home would be
     `runtime/keyboard.rs`, but it writes three facts that keyboard.rs does not write, of three census owners,
     none of them keyboard (`TabState.git_graph_view`, git; `WindowRuntime.palette`, palette;
     `WindowRuntime.search`, search). Its other writes (`WindowRuntime.rename`, owner tabs, and the keyboard-owned
     `composing`, `preedit` and `ime_cursor`) keyboard.rs writes already. Held for a ruling (the two-owners clause
     above); at batch 24 (2026-10-27) the multi-owner rule finds no module that writes all its facts, so it stays
     held. The re-sort (2026-10-26) kept it held; it calls `Runtime::trace_ime_cancel` and
     `Runtime::destroy_ime_caret`, which batch 22 moved into `runtime/keyboard.rs` as `pub(crate)`.
   - `Runtime::show_settings_choice_at`, `Runtime::shortcut_focus`, `Runtime::apply_settings_choice`,
     `Runtime::toggle_settings_panel` and `Runtime::woke_terminal_thumb` (batch 15) — their `called_from` home is
     `runtime/keyboard.rs` (tied for the last three), but `Runtime::show_open_settings_choice`,
     `FolioApp::refresh_main_menu` and `install_main_menu`, `Runtime::settings_mouse_input`,
     `Runtime::open_settings_on_row` and `Runtime::press_terminal_thumb` at the root call them in the product (the
     rule above). Batch 17 moved `Runtime::press_terminal_thumb` to `runtime/mouse.rs` and
     `Runtime::woke_terminal_thumb` with it, as a tail, to `runtime/terminal.rs` (above).
   - `ime_owner`, `popup_takes_the_key` and `terminal_ime_cursor_area` (batch 15) — `ime_outbound.rs`, a child of
     the crate root outside `runtime/`, names them through `use super::*`; moving them would need a root re-export,
     so they stay, as `presentation_equivalent` did. Their only callers there were in `ime_outbound.rs`'s `Runtime`
     block, which batch 22 moved into `runtime/keyboard.rs` (the impl-block rule, above); the three followed it
     there, with `window_ime_cursor_area` as a tail.
   - `Runtime::send_user_input` (batch 15) — tied `runtime/keyboard.rs` / `runtime/mouse.rs`, and it writes facts of
     two census owners, `TabState.pending_keyboard_at` (keyboard) and `WindowRuntime.placeholder_tab` (windows);
     either home would be a new writer of the other's fact. Held for a ruling (the two-owners clause above). Batch 24
     (2026-10-27) moved it to `runtime/mouse.rs` by the multi-owner rule: mouse.rs is the one module that already
     writes both facts (through `send_mouse_input_to`, since batch 17). It was eligible from the multi-owner rule's
     ruling at batch 22 and was not re-judged at batch 22 or 23. The placement is the rule's, not §13's: §13 gives
     "session (input)" to `runtime/keyboard.rs`, which would be a new writer of windows' `placeholder_tab`, so
     `runtime/mouse.rs` is not the input path's home by design. It is re-judged when `placeholder_tab` gets a door
     (the ownership contract's O-5 / registry work).
   - `strip_insert_slot`, `solve_seats`, `presentation_physical_size` and `pane_can_become_a_tab` (batch 16) — their
     `called_from` home is `runtime/tabs.rs` (tied with `main.rs` for the last), but `cli_tab_slot`,
     `create_tab_state` and `Runtime::create`, `Runtime::create`, and `plan_for` at the root call them in the
     product (the rule above). Batch 18 moved `Runtime::plan_for` to `runtime/panes.rs` and
     `pane_can_become_a_tab` with it, as a tail, to `runtime/tabs.rs` (above).
   - `Runtime::scroll_settings`, `Runtime::settings_mouse_input` and `Runtime::open_settings_on_row` (batch 17) —
     their `called_from` home is `runtime/mouse.rs`, but each writes `WindowRuntime.settings_scroll`, whose census
     owner is `main[settings]` and whose writers are the root, `runtime/keyboard.rs` and `runtime/profiles.rs`;
     `runtime/mouse.rs` would be a new writer, and K2 opens no `runtime/settings.rs`, so they stay, as
     `paste_into_settings_field` does. `Runtime::toggle_settings_panel` (tied with `runtime/keyboard.rs` and the
     root) stays with them: its root caller is `Runtime::open_settings_on_row`, and it writes `settings_scroll` too.
     Batch 23 moved all four to `runtime/settings.rs` (2026-10-26, above).
   - `restore_row_seed`, `Runtime::append_edge`, `decided_birth`, `create_leaf_session`,
     `Runtime::focus_rail_geometry_now` and `solve_tree` (batch 18) — their
     `called_from` home is `runtime/panes.rs` (tied with `main.rs` for the last), but
     `FolioApp::settle_restore_answer`, `FolioApp::settle_arrival`, `birth_here`, `create_tab_state`,
     `Runtime::card_mini_size` and `Runtime::refresh_focus_thumbnails`, and `solve_seats` at the root
     call them in the product (the rule above). `closing_this_pane_closes_the_tab`, held here until its root
     caller `pane_into_tab` moved, went with it to `runtime/tabs.rs` in batch 26 (2026-10-29, above). Batch 22 moved `Runtime::refresh_focus_thumbnails` to
     `runtime/panes.rs`; `Runtime::focus_rail_geometry_now`'s root caller is now `Runtime::card_mini_size` alone.
     Their ties wait with them: `auto_split_axis` behind `Runtime::append_edge`,
     `resolved_birth_seed` behind `decided_birth`, `born_in_tab` behind `create_leaf_session` and
     `Runtime::focus_rail_geometry_at` behind `Runtime::focus_rail_geometry_now`.
   - `schedule_leaf_grid_change` (batch 18) — its `called_from` home was `runtime/panes.rs`, but it is
     `LeafSession.grid`'s single-writer road, which stays with that fact, as `release_due_leaf_resize` does. O-3
     (2026-10-27) made it `LeafSession::propose_size`, the session's own method beside its struct in `main.rs`, which
     is no free item for a batch to move.
   - `Runtime::aim_focus_card_window` (batch 18) — its `called_from` home is `runtime/panes.rs`, but it writes
     `WindowRuntime.focus_thumbs`, whose other writer, `Runtime::refresh_focus_thumbnails`, stayed at the root.
     Batch 22 moved both to `runtime/panes.rs` (the focus cards, by clause (a)); the fact, which has no owner, went
     from {root} to {`runtime::panes`} with all its writers, and `WindowRuntime.cards` went from {root, panes} to
     {panes}.
   - `markdown_gap_paragraph` (batch 19) — tied `runtime/git.rs` / `runtime/preview.rs` once
     `build_preview_markdown_body` moved; it is a markdown body's paragraph, and the owner column gives preview
     panes and their documents to `runtime/preview.rs`, so it went there (clause (b)), imported by `runtime/git.rs`.
   - `push_markdown_table` (batch 19) — a tail of `build_preview_markdown_body`, but `table_block.rs`, a module of
     the crate root outside `runtime/`, imports it by its root path (`use crate::{…, push_markdown_table, …}`);
     moving it would change that import or need a root re-export, so it stays (it is in D8's markdown-layout
     cluster, plan row 69, below).
     The moved `build_preview_markdown_body` imports it from the root.
   - `markdown_table_columns` (found at batch 24's judgement, 2026-10-27) — no root product caller, but
     `table_block.rs`, outside `runtime/`, imports it by its root path, and that module's own doc says it "lives in
     `main.rs` beside the rest of the markdown layout"; it stays with D8's markdown-layout cluster (plan row 69), as
     `push_markdown_table` does.
   - `preview_open_lane` and `path_opens_as_a_page` (held at batch 19's judgement) — the first is a tail of
     `source_opens_as_a_page`, but `preview_neighbours.rs`, outside `runtime/`, imports it by its root path, so it
     stays (the same rule); `path_opens_as_a_page`, whose root callers are it and `preview_page_hand_off`, waits with
     it. Batch 21 moved `source_opens_as_a_page` and `preview_page_hand_off` and left both held; the moved
     `preview_page_hand_off` imports `path_opens_as_a_page` from the root.
   - `animated_pane_viewports` (held at batch 18, behind `preview_image_placement`) — tied `runtime/panes.rs` /
     `runtime/preview.rs` once its root caller moved; it is the pane-motion viewports, and the owner column gives
     the move of panes to `runtime/panes.rs` (clause (b)); batch 20 moved it there, imported by `runtime/preview.rs`.
   - `markdown_image_card` and `markdown_item_runs` (batch 20) — their `called_from` home is `runtime/preview.rs`
     (tied with `main.rs`), but `measure_markdown_block` at the root calls them in the product (the rule above).
     `markdown_image_extent` (batch 21) waits with them for the same reason: moving `answer_one_picture` in batch 20
     made its row preview-first, and `measure_markdown_block` still calls it.
   - `Runtime::plan_inputs_for` (held at batch 18, behind `Runtime::plan_inputs`) — tied `runtime/panes.rs` /
     `runtime/preview.rs` once its root caller moved; it is the drop plan's inputs and cache key, and the owner column
     gives seat layout and the move of panes to `runtime/panes.rs` (clause (b)); batch 21 moved it there, beside
     `Runtime::plan_for`, called by `runtime/preview.rs`'s `Runtime::plan_inputs`.
   - `markdown_prose_runs` (batch 20) — its `called_from` home is `runtime/preview.rs` (tied with `main.rs`), but
     `measure_markdown_local` at the root calls it in the product (the rule above).
   - `reference_activation` (batch 20) — its `called_from` home is `runtime/preview.rs` (tied with `main.rs`), but
     its root caller is `hyperlink_activation`, held above, so it waits for it.
   - `note_link_sites`, `note_math_sites` and `note_text_site` (batch 20) — their `called_from` home is
     `runtime/preview.rs`, but their root caller is `push_markdown_table`, held above, so they wait for it.
   - `tab_owes_frame` (batch 20) — its `called_from` home is `runtime/preview.rs`, but `PaneMotion`'s
     `settle_frame_debt` at the root calls it in the product (the rule above).
   - `files_row_display_name` (found at batch 21's review) — its `called_from` home is `runtime/preview.rs` (five
     of six), but `create_tab_state` at the root calls it in the product (the rule above).
   - `home_crumb_for`, `open_local_path_ask` and `printable_address` (found at batch 21's review) — tied `main.rs` /
     `runtime/preview.rs`, but `home_shortened_path_on`, `Runtime::open_local_path` and
     `HyperlinkHover::status_text_in` at the root call them in the product (the rule above). `open_local_path_ask`
     waits with `Runtime::open_local_path`, held above; batch 23 moved both to `runtime/handoff.rs`. Batch 22 moved `home_shortened_path_on` into `file_peek.rs`,
     a module outside `runtime/`, which imports `home_crumb_for` from the root; `home_crumb_for` is now tied
     `file_peek.rs` / `runtime/preview.rs` and stays for the next re-sort.
   - `row_verb` (since batch 18, found at batch 21's review) — its `called_from` home is `runtime/panes.rs` (two of
     three), but `DropLanding::caption` at the root calls it in the product (the rule above).
   - `preview_caret_prose` (ruled at batch 21's review) — tied `preview_viewport.rs` / `runtime/preview.rs`, but
     `preview_viewport.rs`, a module of the crate root outside `runtime/`, names it through `use crate::*;` (in
     `Runtime::prepare_markdown_caret_view`); moving it would need a root re-export or an import there, so it stays,
     as `push_markdown_table` does. Batch 22 moved that `Runtime` block into `runtime/preview.rs` (the impl-block
     rule, above), and `preview_caret_prose` and `scale_ppm` followed it there.
   - `native_window_of` and `option_as_alt` (held at the §13 re-sort, 2026-10-26) — beside-caller rows for
     `window_seam.rs`, but both carry platform `cfg` arms (`#[cfg(windows)]` / `#[cfg(target_os = "macos")]`), and
     `window_seam.rs` is not on `FILES_THAT_MAY_NAME_A_PLATFORM`. That list is a `docs/DESIGN.md` change, which K2
     may not make, so they stay at the root, whose entry is the native-window door's two arms.
   - `is_format_or_bidi_control` (held at the §13 re-sort, 2026-10-26) — it has two root callers
     (`HyperlinkHover` and `clean_title_capped`) against one outside (`attention_wire.rs`), and its doc is the second
     half of `clean_title_capped`'s, so it stays beside it.
   - `measure_markdown_local` and `markdown_prose_face` (held at the §13 re-sort, 2026-10-26) — beside-caller rows
     for `preview_wrap.rs`, but `measure_markdown_local` is the only product caller of `measure_markdown_block`, whose
     four callees are held above, and `markdown_prose_face` is tied with the root's `markdown_source_block`. Both
     belong to D8's markdown-layout cluster: plan row 69 moves that cluster into a module of its own outside
     `runtime/`, after K2.

   Batch 2 (`runtime/web.rs`) onward moves rows five shrink-only checks key by module
   or file (`check-census-unknowns.ps1`, `check-pointer-debt.ps1`,
   `check-env-doors.ps1`, `check-window-seam-native.ps1`, `check-embedded-spaces.ps1`):
   since T-MOVED-NOT-ADDED-RULE they share `check-dead-code.ps1`'s "a row whose only
   change is where it lives is the same row moved" (`scripts/ci/moved-rows.ps1`), so
   a batch regenerates their lists and the moved rows pass as moved.
   `REQUEST-DOOR-SITES.tsv` and `UNFOLLOWED-EDGES.tsv` keep their dated-reason rule: a
   batch re-keys their moved rows with a dated reason, as batch 1 did.
   **The census's unknown sites are the exception among the five** (added at batch 11, after batch 10 found the problem): `docs/plans/design/ownership-census-unknowns.tsv`
   is hand-edited and `scripts/generate-ownership-census.ps1` removes a resolved row but adds none, so the batch
   does not regenerate a moved row into place. Each batch's judgement lists the unknown-site rows of the items it
   moves, and the move commit or the generated commit re-keys their `module` column to the new module (the same
   site and reason); bt-source's census test refuses a moved site as a new unknown until then, and
   `check-census-unknowns.ps1` passes it as moved. Batch 10 found its two rows (`trigger_root`, `trigger_rect`)
   at the bt-source gate. **The census's annotations are the eighth list** (added at batch 14, after batch 13 found
   it): `docs/plans/design/ownership-census-annotations.tsv` is hand-edited and names the owner of every proven
   multi-writer fact. A move that leaves a fact with exactly one writer module makes that fact's row stale, and
   the batch removes it in its generated commit. Batch 13 moved `Runtime::carry_live_journeys` into
   `runtime/math.rs` and removed `WindowRuntime.formula_overlay_owed`'s row. bt-source's census test names a stale
   row ("is not a proven multi-writer fact"). The other direction, a move that makes a fact multi-writer, would
   need a new row, but the census rule above already holds such an item in `main.rs`, so the stale direction is
   the one a judgement checks: for each fact a moved item writes, it asks whether the move leaves one writer
   module. So a batch's judgement covers eight lists: these five, `REQUEST-DOOR-SITES.tsv`,
   `UNFOLLOWED-EDGES.tsv` and the census annotations.
   **Two hand-kept tables name items by path and are read too** (added at batch 22, after it found both):
   `crates/bt-app/src/window_waits.tsv` (its `# effects` rows, whose keys bt-source's request-door walk looks items
   up by) and `docs/plans/window-thread-bare-sites.tsv`. bt-source's loader (`read_registry` in
   `crates/bt-source/tests/request_doors.rs`) refuses nothing in `# effects`: a row whose key matches no item is
   skipped there without a word, though its `# vocabulary` and `# entrances` sections refuse an unknown key, so a
   moved `worker-door-body` loses its stop and the walk enters the worker's body. bt-app's own `window_waits`
   tests do go red on such a row (an `# effects` row for a function that holds no listed effect, and the moved
   body's effects observed with no row), but in the other crate's gate, after the generators have run; plan row
   76 makes the loader refuse too. The judgement greps both tables for every moved item's root path. **The
   `window_waits.tsv` re-key goes in the move commit** (ruled 2026-10-26, at batch 22's review), as the §7
   re-paths do, so every commit is green; the generated commit re-keys the bare-sites rows it finds.

2. **The 395 items whose callers are mostly other `main.rs` items follow their
   callers.** Regenerate the map after each batch. An item whose callers have moved
   now shows a `runtime/*` home and joins the next batch for that topic. Repeat
   until no batch is left.
3. **The 15 items whose callers are mostly in a module outside `runtime/`** go
   beside those callers, as ordinary module functions:
   - `window_seam.rs` (5)
   - `preview_viewport.rs` (3)
   - `preview_neighbours.rs`, `update_job.rs`, `restore.rs`, `file_peek.rs`,
     `explorer_menu.rs`, `preview_wrap.rs`, `ime_outbound.rs` (1 each)
4. **The 16 items with no measured caller stay put or go with their topic:**
   - `Runtime`'s `Deref`/`DerefMut` stay with the struct.
   - `main` stays.
   - The four `PointerHost` methods stayed in `main.rs` while K2 opened no
     `runtime/settings.rs` (ruled 2026-10-24); batch 23 moved their impl block whole
     into it (2026-10-26, above).
   - The rest are reached only as function values or through a receiver the map
     does not type (`web_page_cursor`, `seeded_tab`, `path_door`, `scale_door`, …).
     Read their call sites before moving them. Batch 22 moved these four and
     `files_a_tab_stands_on` by their call sites: `web_page_cursor` to `runtime/mouse.rs`,
     `seeded_tab` to `runtime/windows.rs`, `files_a_tab_stands_on` to `runtime/preview.rs`, and
     `path_door` and `scale_door` beside `computation_lane.rs`'s lane.

## O-2…O-5: the field moves, in order, with the files each opens

The order is the contract's §3.2, view before session and least-referenced first,
measured. "Sites" counts every use of the field (all kinds). The doors come from
`doors.tsv`. `LeafSession` had **45** fields before O-2, four more than the note's
41: `attention_ended` (it belongs to M7), `process_folder`, `program_kind` and
`paste_hold` are new since the note. It has **41** after O-2, `view` included
(recounted 2026-10-27).

- **O-2 (M0–M1).** Five fields, in this order:

  | field | sites |
  |---|---:|
  | `thumb_awake` | 3 |
  | `column_awake` | 3 |
  | `frame_image_references` | 6 |
  | `presented_metrics` | 7 |
  | `card_skip` | 8 |

  - Files opened: `main.rs`, `runtime/preview.rs`, `runtime/panes.rs`,
    `runtime/frame.rs`, `runtime/terminal.rs`, `runtime/peek.rs`.
  - Doors present:
    - `Runtime::wake_terminal_thumb` (`runtime/terminal.rs`)
    - `runtime::terminal::Runtime::wake_terminal_column`
    - `runtime::frame::Runtime::redraw` for `presented_metrics`
    - the two writers of `frame_image_references`: `publish_frame_inner` and
      `rescan_pane_references`
  - `card_skip` has no writer, only one `&mut` lend (`aim_card_window`). Its door
    is new.
  - **Done** 2026-10-27 (O-2, `ad657a414`): the five are `TermView`'s, nested as
    `LeafSession.view`, with the rows they had re-keyed. `LeafSession` goes from
    45 to 41 fields (with `view`), and `TermView` has 5. The nest adds 13
    `LeafSession.view` read rows (where a view field is read) and takes the
    unattributed-read header from 174 to 180. `card_skip`'s door went to O-3
    (ruled at O-2's judgement).
- **O-3 (M2–M4).** Four fields:

  | field | sites | door present |
  |---|---:|---|
  | `last_presented_frame` | 10 | `redraw` |
  | `text_scale` | 12 | `step_leaf_text_scale` |
  | `metrics` | 12 | `apply_leaf_metrics` |
  | `projection` | 67 | none: 15 write sites in 13 items |

  - Measured, `last_presented_frame` is less referenced than `text_scale` and
    `metrics`. The note's lexical 24 also counted `WindowRuntime.last_presented_frame`,
    so M3 may go first.
  - Files opened: `runtime/terminal.rs`, `runtime/frame.rs`, `main.rs`,
    `runtime/panes.rs`, `runtime/mouse.rs`, `runtime/keyboard.rs`,
    `runtime/search.rs`, `runtime/tooltips.rs`, `runtime/clipboard.rs`,
    `runtime/math.rs`, `runtime/preview.rs`, `runtime/peek.rs`, `ime_outbound.rs`.
    `projection` alone touches ten of them.
  - **Done** 2026-10-27 (O-3, `5d07b49dd`): the four are `TermView`'s, with their rows
    re-keyed. `LeafSession` goes from 41 to 37 fields, and `TermView` has 9. The
    doors: `LeafSession::propose_size` (grid; the old `schedule_leaf_grid_change`)
    with its stages `propose_cell` and `propose_layout`, the seven
    `TermViewPorts::port_*` forwards, `LeafSession::report_keyboard_focus`, and
    `TermView::commit_presented`, now `last_presented_frame`'s and
    `presented_metrics`' one writer (a method of the field's own type, so it is
    not a `doors.tsv` row). `projection` keeps its 13 writers in 5 modules. The
    nest adds `LeafSession.view` rows where a moved field is reached, two of them
    access rows (the nested patterns of `insert_path_into_terminal` and
    `send_paste`), and takes the unattributed-read header from 180 to 182.
    `card_skip`'s door, sent here by O-2, was not built: it was not in O-3's
    judgement, and the ruling makes `commit_presented` the one `TermView` method.
  - **Blind spot (review F2, 2026-10-27):** the map does not count a call into a
    bt-app trait implemented on another package's type, so no call into
    `TermViewPorts` is in it. `crates/bt-source/src/census/resolve.rs:539-543`
    (`workspace_methods`) keeps a method's callees only from the type's own
    packages, and `DualPlaneSession` is bt-term's. The writes stay right (the class
    comes from the declaration), and no unknown or unfollowed row shows the drop.
    Undercounted: the `topics.tsv` rows `runtime/math.rs -> main.rs` (8 port calls
    missing; 36 -> 37 counts only `propose_layout`), `runtime/clipboard.rs ->
    main.rs` (2), `runtime/mouse.rs -> main.rs` (1) and `runtime/frame.rs ->
    main.rs` (1), and the `TermViewPorts` items' `called_by` and their callers'
    `calls`. The fix is a bt-source plan row of its own; the map regenerates then.
    **Closed** by plan row 83 (2026-10-27): those four rows now count 45, 10, 176
    and 49, and `Runtime::return_to_live_for_input`'s `calls` 5 (`main.rs` 4).
- **O-4 (M5–M7).**
  - **M5, the launch facts.** Six fields, opening mostly `main.rs`, plus
    `runtime/{panes,tabs,clipboard,terminal,floats,mouse,peek,profiles,dpi}.rs` for
    reads:

    | field | sites | door |
    |---|---:|---|
    | `integration` | 3 | `finish_leaf_birth` |
    | `spawn_place` | 3 | `place_leaf` |
    | `born_named` | 3 | `place_leaf` |
    | `paste_recipient` | 6 | `finish_leaf_birth` |
    | `program` | 8 | `finish_leaf_birth` |
    | `profile` | 16 | `finish_leaf_birth` |

  - **M6, the lifecycle record.** Seven fields:

    | field | sites |
    |---|---:|
    | `shell_exit_age` | 2 |
    | `birth_death_kept` | 2 |
    | `shell_exit_said` | 4 |
    | `spawn_fallback` | 5 |
    | `birth_death_fallback` | 7 |
    | `successor` | 10 |
    | `birth` | 17 |

    - Files opened: `runtime/tabs.rs`, `main.rs`, `runtime/panes.rs`,
      `runtime/terminal.rs`, `runtime/clipboard.rs`.
    - Doors present: `keep_birth_death_face`, and `reap_exited_tabs` with
      `land_birth_death_fallback` (`shell_exit_age`).
    - The other five fields have three or more writers, which the `ShellLife`
      transitions replace.
  - **M7, the attention account.** Five fields:

    | field | sites | door |
    |---|---:|---|
    | `attention_capability` | 1 | none |
    | `attention_ended` | 2 | `end_leaves_of` |
    | `bell_reported` | 3 | `settle_attention` |
    | `attention_clock` | 3 | `deliver_attention`, `settle_attention` |
    | `attention` | 22 | none: 14 write sites in 7 items |

    Files opened: `main.rs` and `runtime/attention.rs`.
  - **Done** 2026-10-27 (O-4a, M5 `532293578` and M7 `b0728e128`): M5's seven launch
    facts (the six above and `program_kind`) are `LaunchFacts`', nested as
    `LeafSession.launch` and declared in the census (ten structs), with their rows
    re-keyed and their one writer each unchanged. M7's ledger, bell edge, clock and
    capability are `LeafSession.attention: AttentionAccount`, one fact whose
    writers are the seven items its four parts had (`attention_ended` stays on
    the leaf); `attention_clock`'s pair and `bell_reported`'s row leave
    `doors.tsv` with their facts. Typing `attention_ledger_deadline`'s closure
    removes the `?.attention_clock` unknown and its unfollowed `deadline` edge.
    `LeafSession` goes from 37 to 28 fields; the unattributed-read header stays
    at 182. M6 is O-4b.
  - **Done** 2026-10-27 (O-4b, M6): the seven fields above but `successor`, and `pty`
    with them (moved from O-5, ruling D4: the contract's (c)2 puts the pseudoconsole inside
    the phases), are `LeafSession.life: ShellLife { phase, spawn_fallback }`, one fact of
    nine phases written only by its transitions; `shell_exit_said` folded into the phase
    (D5). Its writer functions go from the eight facts' 20 to 16 in the same five
    modules: D10 folded `owe_birth_death_fallback_resize` into `ShellLife::owe_landing_size`,
    and the successor's three (`land_pane_birth`, `land_successor`, `restart_shell`) write
    `LeafSession.successor` through the record's own methods (`await_successor`,
    `successor_mut`, `take_successor`), which stays its own fact beside the life (E5).
    The census annotations `LeafSession.pty` (panes) and `.spawn_fallback` (main) go
    for one `LeafSession.life` row (main). The K2 rows `land_shell_birth`,
    `deliver_held_input`, `land_birth_death_fallback_shell` and `apply_owed_shell_resize`
    are released, and `settle_attention` / `tab_queue_leader` with them (D15); batch 25 moved all six (above).
- **O-5 (M8).** What remains goes into `ShellSession`: `session` (177 sites),
  `grid` (17), `conpty_grid` (10), the revisions, the owed input, `wake`
  and `incarnation`. `pty` (20) moved at O-4b instead, into `ShellLife`'s phases
  (2026-10-27, ruling D4: the contract's (c)2 gives each phase its pseudoconsole).
  - Doors present:
    - `drain_leaf_pty` (`pending_typing`, `last_finished_command`,
      `output_revision`)
    - `mark_leaf_seen` / `mark_leaf_painted` (`last_seen_revision`)
    - `stage_paste` / `take_pending_paste` (`pending_paste`)
    - `release_due_leaf_resize` / `LeafSession::propose_size` (`grid`; `conpty_grid`
      alone; the second was `schedule_leaf_grid_change` until O-3)
    - `finish_leaf_birth` / `keep_birth_death_face` (`wake`)
  - `TabState.sessions` changes membership in eight items:
    - `move_seat_content`, `pane_into_new_tab`
    - `runtime::panes::Runtime::{close_pane, land_pane_birth, land_restart, split_seat}`
    - `runtime::preview::Runtime::pop_out_preview`
    - `runtime::terminal::Runtime::replace_restarted_shell`

    These are the files M8's new map value opens.
  - **O-5a done** 2026-10-27 (T-CONTRACT-M8, rulings E1, E8, E9, E11): `LeafSession` is
    `ShellSession`, every census row re-keyed with its writers unchanged (bt-source
    `the_session_record_is_shell_session`); `last_seen_revision` behind
    `ShellSession::mark_seen` (bt-source `the_seen_revision_has_one_door`, bt-app
    `a_view_reports_seen_and_unread_never_goes_back`) and `pending_typing` behind
    `ShellSession::{queue_typing, take_typing}`, so both pairs leave `doors.tsv`;
    `wake`'s doc marks it transport debt until O-6. The K2 rows released: G2's seven
    and G11's two, which batch 25 moved (2026-10-28, above). The counts in this bullet were O-1's: `pty` moved at
    O-4b (ruling D4), and the eight membership items are `TabState.sessions`'
    writers, not new owners (the contract's §3.3 note of 2026-10-27). O-5b is
    `TerminalPane`.
  - **O-5b done** 2026-10-28 (T-CONTRACT-M8, rulings E2–E7, E10, E14):
    `TabState.sessions` holds `TerminalPane { session, view, successor }`, the
    census's eleventh struct, so the map stays a hub. `ShellSession.view` and
    `ShellSession.successor` are `TerminalPane.view` and `TerminalPane.successor`,
    with the same writers (the successor's three moves are the pane's methods,
    E5), and `TerminalPane.session` is a hub lent by 35 functions. The session
    accessors keep their names and answer the session (E4); `TabState.sessions`'
    two raw lends move from `focused_mut` and `leaves_mut` to `focused_pane_mut`
    and `panes_mut`. `TermView.card_skip`'s one writer is
    `TerminalPane::aim_card_skip` (E14; it was `aim_card_window`'s lend). The
    census's unattributed reads stay 182 and its unknowns do not change. The K2
    rows still held are the fifteen O-5a left (G2's `return_to_live_for_input`,
    G3's seven, G5's six, G15's one), each re-cited above with its reason at
    O-5b's head; none is released. Batch 26 (2026-10-29) moved eight of them
    (G2's, G5's and G15's, by the batch 26 rulings 1–3); G3's seven stay held for
    plan row 105 (above).

## The note's §1.2 lexical counts, measured

§1.2 counted `.field` lexically, per file. The † names are shared with other
structs, so their counts were upper bounds. Here are the measured `LeafSession`
uses, all kinds (reads in parentheses):

| field | §1.2 lexical | measured |
|---|---|---|
| `profile` | main 16 + twelve others | 16 (15) |
| `program` | main 8 | 8 (7) |
| `integration` | main 4, profiles 12 | 3 (2) |
| `successor` | main 4, panes 3, terminal 2 | 10 (6) |
| `wake` | main 1, tabs 2 | 5 (3) |
| `session` | main 80, math 18, panes 14, terminal 14, ten more | 177 (107) |
| `metrics` | main 5, frame 4, math 3 | 12 (11) |
| `grid` | main 12, dpi 3, terminal 2 | 17 (15) |
