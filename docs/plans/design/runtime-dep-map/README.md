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
- **unclassified.tsv**: **811 items, 602 free functions and 209 `Runtime`
  methods**, still in `main.rs`.
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

1. **The 408 items whose callers are mostly in one `runtime/*` topic move into that
   topic, one batch per topic.** Order the batches by the sum of their items'
   `called_by`, smallest first. Each batch opens `main.rs` and its topic.

   | # | topic | items (functions + methods) | call sites in |
   |---:|---|---:|---:|
   | 1 | `runtime/diagnostics.rs` | 1 (1 + 0) | 1 |
   | 2 | `runtime/profiles.rs` | 1 (1 + 0) | 1 |
   | 3 | `runtime/attention.rs` | 2 (2 + 0) | 2 |
   | 4 | `runtime/search.rs` | 2 (1 + 1) | 3 |
   | 5 | `runtime/tooltips.rs` | 3 (3 + 0) | 3 |
   | 6 | `runtime/web.rs` | 9 (7 + 2) | 9 |
   | 7 | `runtime/dpi.rs` | 6 (5 + 1) | 10 |
   | 8 | `runtime/palette.rs` | 7 (1 + 6) | 14 |
   | 9 | `runtime/terminal.rs` | 14 (12 + 2) | 15 |
   | 10 | `runtime/git.rs` | 14 (11 + 3) | 21 |
   | 11 | `runtime/clipboard.rs` | 15 (13 + 2) | 25 |
   | 12 | `runtime/peek.rs` | 11 (10 + 1) | 26 |
   | 13 | `runtime/files.rs` | 18 (16 + 2) | 35 |
   | 14 | `runtime/floats.rs` | 15 (6 + 9) | 37 |
   | 15 | `runtime/windows.rs` | 19 (18 + 1) | 41 |
   | 16 | `runtime/math.rs` | 18 (12 + 6) | 45 |
   | 17 | `runtime/frame.rs` | 34 (16 + 18) | 46 |
   | 18 | `runtime/keyboard.rs` | 26 (12 + 14) | 49 |
   | 19 | `runtime/tabs.rs` | 20 (16 + 4) | 54 |
   | 20 | `runtime/panes.rs` | 33 (24 + 9) | 64 |
   | 21 | `runtime/mouse.rs` | 50 (30 + 20) | 82 |
   | 22 | `runtime/preview.rs` | 90 (72 + 18) | 132 |

2. **The 379 items whose callers are mostly other `main.rs` items follow their
   callers.** Regenerate the map after each batch. An item whose callers have moved
   now shows a `runtime/*` home and joins the next batch for that topic. Repeat
   until no batch is left.
3. **The 10 items whose callers are mostly in a module outside `runtime/`** go
   beside those callers, as ordinary module functions:
   - `preview_viewport.rs` (3)
   - `preview_neighbours.rs`, `update_job.rs`, `restore.rs`, `file_peek.rs`,
     `explorer_menu.rs`, `preview_wrap.rs`, `ime_outbound.rs` (1 each)
4. **The 14 items with no measured caller stay put or go with their topic:**
   - `Runtime`'s `Deref`/`DerefMut` stay with the struct.
   - `main` stays.
   - The four `PointerHost` methods go with the settings topic (D-30).
   - The rest are reached only as function values or through a receiver the map
     does not type (`web_page_cursor`, `seeded_tab`, …). Read their call sites
     before moving them.

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
