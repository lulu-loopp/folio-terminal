# The runtime dependency map (J3, T-RUNTIME-DEP-MAP = ownership ticket O-1)

The measured dependency graph inside `bt-app`, which the ownership contract
(`../ownership-contract-2026-10-11.md`, §3.1) asks for before any field moves, and
which K2 (T-SPLIT-2B) uses to order its topic moves. Four tab-separated files,
**generated**: `bt_source::DependencyMap`, taken from the same `bt_source::FieldCensus`
the ownership census gate uses, over the same structs (`App`, `WindowRuntime`,
`TabState`, `LeafSession`, `PreviewBuffer`, `PreviewPane`, `DocumentView`,
`PictureView`).

- Regenerate: `pwsh -File scripts/generate-runtime-dep-map.ps1` (runs cargo; writes
  the four files and the `# measured at <commit>` line under each header).
- Guard: `crates/bt-source/tests/census.rs`
  `the_committed_runtime_dep_map_is_the_code`. Every line of the four files must be
  the code's at this tree, except the `# measured at` line. A hand edit fails it, and
  so does any change to the code that moves a row. Nothing here is edited by hand.
- The rules: `crates/bt-source/tests/dep_map.rs`, one fixture each.

The numbers in this README were read off the files at the commit their headers
name. They are prose, and nothing holds them. The files are the authority.

## How to read the four files

A **topic** is a file under `crates/bt-app/src/`: `main.rs`, `runtime/panes.rs`,
`preview.rs`. An **item** is a callable named as the census names it: the declaring
module, then the label (`crate::runtime::panes::Runtime::close_pane`,
`crate::drain_leaf_pty`, `crate::<Runtime as Deref>::deref`). A `cfg` variant counts
as the same item.

| file | one row is | columns |
|---|---|---|
| `fields.tsv` | an item × a field × a kind of use | `struct field item kind count`. `kind` is `read`, `access` (a `&mut` lend, `get_mut`), `write` (proven), `membership` (a hub's contents), or `inner` (through a shared reference). `count` is sites. The four census kinds are exactly the census's columns. `read` is new: a field evaluated where nothing mutable is done to it (a value, a `&` borrow, a `&self` method, a non-`ref mut` pattern, a step on the way to another field that is read). |
| `topics.tsv` | a pair of topics joined by calls | `from_topic to_topic calls example_item`. `calls` is call sites in `from_topic` that reach an item of `to_topic`. `example_item` is the caller → callee pair with the most sites. A call within one topic is not an edge. |
| `doors.tsv` | a door already present | `struct field door_item writes other_writers status`. The door is an item **outside the field's own type** that is its only writer, or one of two (write and membership sites). `other_writers` names the other writer, or `-`. `status` is `incomplete` when an unknown census site names the field. |
| `unclassified.tsv` | a free function or `Runtime` method still written in `main.rs` | `item shape called_by calls called_from calls_into`, **least-referenced first**. `called_from` is the topics its callers are written in, most call sites first (the candidate home). `calls_into` is the topics of what it calls. `-` means none. |

**What the map does not see** is printed as a count in each header. Nothing is
guessed. A read through a receiver the census rules do not type is not attributed:
168 sites. A call is not followed through a trait object, a stored closure, a
function value, or a receiver the rules do not type:

- 2,160 sites call a method name only the package has, through an untyped receiver.
  The largest share is `i18n::Text::X.text()`: the receiver is an enum variant's
  path, and the census does not type a path value.
- 3,903 sites call a method name that a standard type also has.
- 285 sites reach more than one item, for example two types of the same name in two
  modules.

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
- **doors.tsv**: 334 doors. 224 are on `WindowRuntime`, 55 on `App`, 34 on
  `LeafSession` and 21 on `TabState`. `PreviewBuffer`, `PreviewPane`,
  `DocumentView` and `PictureView` have none: O-7 and O-8 put their writes inside
  their own types.
- **unclassified.tsv**: **667 items, 495 free functions and 172 `Runtime`
  methods**, still in `main.rs` (at K2 batch 13's head; 696, 509 and 187 before it,
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
   | — | `runtime/profiles.rs` | 1 (1 + 0): `tab_surface`, tied with `runtime/tabs.rs` | 2 |
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
   | 14 | `runtime/keyboard.rs` | 27 (13 + 14) | 50 |
   | 15 | `runtime/math.rs` | 18 (12 + 6) | 50 |
   | 16 | `runtime/tabs.rs` | 20 (16 + 4) | 54 |
   | 17 | `runtime/panes.rs` | 33 (24 + 9) | 65 |
   | 18 | `runtime/mouse.rs` | 49 (29 + 20) | 81 |
   | 19 | `runtime/preview.rs` | 91 (72 + 19) | 144 |

   **An item stays in `main.rs`, whatever its `called_from` row proposes, when moving it would make a field it
   writes a multi-writer fact** (the ownership census refuses a new one without a ruled owner; the standing rule is
   that such a move goes back through the original single writer). `release_due_leaf_resize` is the first: it writes
   `LeafSession.birth` and `LeafSession.grid`, whose other writers stay at the root, and batch 3 moved it back. It
   stays in `main.rs` until an owner is ruled for those two facts; `unclassified.tsv` keeps proposing
   `runtime/dpi.rs` for it (that column is generated), and no later batch retries the move.

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

   **A product caller in another `runtime/` file imports the moved item from its new path** (`use
   crate::runtime::<topic>::<item>;`, an import line and no body text; ruled at batch 7). What stays forbidden is a
   non-test `use runtime::…` at the crate root to keep a root caller compiling: an item with a product caller at the
   root waits until that caller moves. A caller inside the target module that named the item by its root path drops
   the `crate::` prefix (`crate::x(` becomes `x(`; ruled at batch 8, where five calls in `runtime/peek.rs` did); no
   root re-export.

   **Held items** (each row's generated `called_from` home is not where it goes):
   - `release_due_leaf_resize` — stays in `main.rs` (the census rule above).
   - `files_float_content_height` (tied `runtime/dpi.rs` / `runtime/floats.rs`) — the owner column
     (`docs/ARCHITECTURE.md` §13) gives it to `runtime/floats.rs` ("floating windows"); batch 10 moved it there.
   - `Runtime::reveal_in_explorer` — a three-way tie (`runtime/attention.rs` / `runtime/configuration.rs` /
     `runtime/floats.rs`) that the owner column does not settle: §13's "Still in `main.rs`" table lists it
     unassigned, and its asks column names "hand-off (reveal)" for both `files.rs` and `floats.rs`. At batch 10
     its callers were seven, one in each of seven `runtime/` files, and it has no census owner; it stays held,
     and its home is ruled at the §13 re-sort at the end of this table, not per batch.
   - `Runtime::focus_seat` — its first `called_from` place is `runtime/palette.rs`, but that is three of eight
     callers (the others are `runtime/web.rs`, `runtime/attention.rs`, `runtime/preview.rs` and `main.rs`) and it
     is a general focus door, not the command palette's; batch 5 held it, and it waits for the topic that owns
     seats (`runtime/panes.rs`, "seat layout"), so that it moves once.
   - `drain_leaf_pty` — its `called_from` home is `runtime/terminal.rs`, but it writes `LeafSession.pending_typing`,
     whose other writer, `Runtime::queue_typing_in_the_focused_pane`, stays at the root (the census rule above).
     Its doc comment is not above it: it stands above `synchronized_update_committed`, the function before it, and
     runs into that function's own doc (`main.rs`, from "Drain one shell's pipe into its own screen." to
     "…a leaf can supply none of them about itself."). `move.py` takes only the comment directly above an item, so
     the batch that moves it carries that block by hand.
   - `hyperlink_activation` — its `called_from` home is `runtime/terminal.rs`, but `ControlClickHint::of` at the root
     calls it in the product (the rule above); it moves when `ControlClickHint` does.
   - `offer_pty_input` — its `called_from` home is `runtime/clipboard.rs`, but `write_pty_input` at the root calls it
     in the product (the rule above); it moves when `write_pty_input` does.
   - `Runtime::paste_into_settings_field` — its `called_from` home is `runtime/clipboard.rs`, but it writes
     `WindowRuntime.settings`, whose census owner is `main[settings]`; the owner column wins, and K2 opens no
     `runtime/settings.rs`, so it stays in `main.rs` with the settings topic. The hold rests on that write:
     `Runtime::settings_field_holds_the_keyboard`, which only reads the settings field and whose one caller is
     `Runtime::clipboard_focus`, went to `runtime/clipboard.rs` as a tail in batch 10.
   - `Runtime::note_card_hint` and `Runtime::advance_card_hint` (batch 13) — their `called_from` home is
     `runtime/frame.rs`, but `note_card_hint` writes `App.settings_store`, whose census owner is
     `main[settings]`, and `runtime/frame.rs` does not write it; K2 opens no `runtime/settings.rs`, so it stays,
     as `paste_into_settings_field` does. The two are `WindowRuntime.card_hint`'s only writers, so
     `advance_card_hint` stays with it: moving one alone would make that fact a new two-module one.
   - `Runtime::advance_scheme_watch`, `Runtime::advance_storage_watch` and `mark_leaf_painted` (batch 13) — their
     `called_from` home is `runtime/frame.rs`, but they write `App.scheme_watch` (other writers `add_scheme` and
     `create`), `App.storage_watch` (other writer `create`) and `LeafSession.last_seen_revision` (other writer
     `mark_leaf_seen`), whose other writers stay at the root (the census rule above). `advance_storage_watch`
     also writes facts of several census owners.
   - `math_copy_window` (batch 13) — tied `main.rs` / `runtime/frame.rs` / `runtime/math.rs`, and
     `retire_spent_math_copy` at the root calls it in the product (the rule above).
   - `Runtime::glass_here` — tied `runtime/clipboard.rs` / `runtime/mouse.rs`; its doc makes it the one door
     `Self::drive_drag` asks before it surveys, and the owner column gives "drag and drop" to `runtime/mouse.rs`;
     it moves with the mouse batch.
   - `Runtime::sweep_foot_phrases` — its `called_from` home is `runtime/floats.rs`, but it writes
     `WindowRuntime.foot_phrases`, whose other writer, `Runtime::foot_saying`, stays at the root (the census rule
     above); it moves when `foot_saying` does.
   - `Runtime::open_minted_page_on` and `revealable_preview_file` — tied `runtime/floats.rs` /
     `runtime/preview.rs`; the owner column gives preview panes and their doors to `runtime/preview.rs`, and both
     move with the preview batch.
   - `Runtime::open_local_path` — its `called_from` home is `runtime/floats.rs`, but
     `Runtime::apply_settings_choice_announcing` at the root calls it in the product (the rule above).
   - `reveal_ask` — its `called_from` home is `runtime/floats.rs`, but `Runtime::reveal_in_explorer` at the root
     calls it in the product (the rule above); it stays with `reveal_in_explorer`.
   - `Runtime::focus_the_pane_a_path_landed_in` — tied `runtime/files.rs` / `runtime/panes.rs`, and it writes facts
     of two census owners, `WindowRuntime.float` (floats) and `WindowRuntime.search` (search); held for a ruling
     (the two-owners clause above).
   - `home_shortened_path` — its `called_from` home is `runtime/files.rs`, but `cwd_whole` at the root calls it in
     the product (the rule above).
   - `float_dock_label` (batch 11) — a three-way tie (`runtime/files.rs` / `runtime/panes.rs` /
     `runtime/preview.rs`) that none of the three owns; it is a float's dock button, and the owner column gives
     floating windows to `runtime/floats.rs`, so it went there (a ruled owner-column exception), imported by all
     three.
   - `revive_plan`, `create_tab_state` and `scrollback_quota` (batch 12) — their `called_from` home is
     `runtime/windows.rs`, but the root calls each in the product (the rule above); they wait for that caller.

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
   - The four `PointerHost` methods stay in `main.rs` until D-30 opens the settings
     topic (ruled 2026-10-24: K2 opens no `runtime/settings.rs`).
   - The rest are reached only as function values or through a receiver the map
     does not type (`web_page_cursor`, `seeded_tab`, `path_door`, `scale_door`, …).
     Read their call sites before moving them.

## O-2…O-5: the field moves, in order, with the files each opens

The order is the contract's §3.2, view before session and least-referenced first,
measured. "Sites" counts every use of the field (all kinds). The doors come from
`doors.tsv`. `LeafSession` now has **42** fields: `attention_ended` is new since
the note and belongs to M7.

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
    - `Runtime::wake_terminal_thumb` (`main.rs`)
    - `runtime::terminal::Runtime::wake_terminal_column`
    - `runtime::frame::Runtime::redraw` for `presented_metrics`
    - the two writers of `frame_image_references`: `publish_frame_inner` and
      `rescan_pane_references`
  - `card_skip` has no writer, only one `&mut` lend (`aim_card_window`). Its door
    is new.
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
- **O-5 (M8).** What remains goes into `ShellSession`: `session` (177 sites),
  `pty` (20), `grid` (17), `conpty_grid` (10), the revisions, the owed input, `wake`
  and `incarnation`.
  - Doors present:
    - `drain_leaf_pty` (`pending_typing`, `last_finished_command`,
      `output_revision`)
    - `mark_leaf_seen` / `mark_leaf_painted` (`last_seen_revision`)
    - `stage_paste` / `take_pending_paste` (`pending_paste`)
    - `release_due_leaf_resize` / `schedule_leaf_grid_change` (`grid`; `conpty_grid`
      alone)
    - `finish_leaf_birth` / `keep_birth_death_face` (`wake`)
  - `TabState.sessions` changes membership in eight items:
    - `move_seat_content`, `pane_into_new_tab`
    - `runtime::panes::Runtime::{close_pane, land_pane_birth, land_restart, split_seat}`
    - `runtime::preview::Runtime::pop_out_preview`
    - `runtime::terminal::Runtime::replace_restarted_shell`

    These are the files M8's new map value opens.

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
