# Who owns a pane: the session / document / view contract, and how `LeafSession` splits — design note, 2026-10-11

Status: **draft, 2026-10-11**, for Codex review. Plan row J5 (T-05-CONTRACTS, 0.4.8), the first of the two contract notes the 2026-09-21 structure review asked for before 0.5; ledger row **D-1** (*session state has no owner independent of the window*, the 0.5 precondition). The companion note is `side-effects-contract-2026-10-11.md` (D-16, D-33). Read at `main` = `77399d57` on `design/05-contracts`. No product code, no test. One cargo run: `cargo test -p bt-source --test census` (10 passed), whose query reports under `target/ownership-census/` are §1's measured column. Facts about Folio are cited `file:line` at `77399d57` (paths under `crates/bt-app/src/` unless another crate is named) with the symbol beside the line.

**Section signs.** A bare `§n` is a section of this note. `A§n` is a section of `docs/ARCHITECTURE.md`; `DESIGN §n` of `docs/DESIGN.md`; `SE§n` of the companion note; `CEN§n` of `docs/plans/design/ownership-census-2026-09-25.md`; `SPLIT§n` of `docs/plans/bt-app-split.md`; `WB§n` of `docs/plans/design/agent-workbench-0.5-2026-09-20.md`.

**The rulings this answers**

| date | ruling | where |
|---|---|---|
| 2026-09-21 (structure review, D-1) | Three owners: **session** (identity, incarnation, PTY lifecycle, launch facts, parser and transcript, attention), **document** (content, revision, undo, encoding, dirty, disk baseline), **view** (selection, scroll, focus, layout, native resources, caches, the last presented picture). A view disappearing is not a session ending. The entry that replaces `create_leaf_session` accepts the narrower objects. | A§4.1 (`docs/ARCHITECTURE.md:563`) |
| A§4.3 | The `Deref` hub and `Runtime`'s `&mut App` + `&mut WindowRuntime`: a file move is not an ownership change. | §3 |
| DESIGN §7.1.4 | The last pane closing closes its tab; the last tab refuses to close (the window's shut road); an unanswered restore card is folded back into the session file when the window closes; a tab of one folder or one file has no shell (§7.1.6h). | §2.3 |
| DESIGN §2.7 (2026-08-20), §2.9 (2026-08-23) | Closing a non-last window sends it to Recent; closing the last is the process ending with its photograph kept; Quit writes every window whole (`docs/DESIGN.md:711`, `:742`). | §2.3 |
| DESIGN §7.0 (2026-07-17) | *State belongs to the content and travels with it; the container holds none.* A buffer is one per file per tab, dropped clean. A session has one inputtable view; further projections are read-only (`docs/DESIGN.md:1290`) — the last clause narrowed by WB§11 for the drag-out view. | §2, §4, Q1, Q2 |
| 2026-09-20 (WB§11, owner) | A dragged-out pane is a second interactive view of the same terminal: **one PTY, one size, never reflowed**. | §2.2, §4 |
| RULES 55 | A change of who owns a fact gets a Codex-reviewed note first; each implementation ticket states its Architecture impact. | this note; §3.4 |

---

## 0. In one page

- **The measured present** (§1). `LeafSession` (`main.rs:11564`) has **41 fields**. The census proves a writer after construction for 35: **28** have one writer module, **7** more than one (`pty`, `spawn_fallback`, `wake`, `session`, `attention`, `projection`, `frame_image_references`), and 6 have none outside construction (`incarnation`, `attention_capability`, `successor` (only hub membership), and `card_skip`, `pending_pty_resize`, `pending_psreadline_resize_reanchor`, which are only lent by `&mut`). Sorted by the plan row's definition: **32 session** (25 plain, plus the 7 that make the shell's birth and death), **9 view**, **0 document**. A terminal pane has no document. Its transcript is the parser's output, which the session owns. The document owner already exists elsewhere: `preview::PreviewBuffer` (`preview.rs:4750`) in the tab's `PreviewPool` (`preview.rs:6285`), with `PreviewPane` (`main.rs:4723`) as its view.
- **The hidden fourth.** The `session` field (`DualPlaneSession`, 39 writer functions in 10 modules) holds view state too. The census's own `call:` column shows it: `set_view_selection`, `set_math_hover`, `set_math_toggle_presentation`, `scroll_math_block`, `set_cell_width_subpixels`/`set_font_size_subpixels`, `set_layout_key`. Splitting `LeafSession` does not separate session from view while those live in `bt-term`. §1.3 rules what the contract does with them.
- **The contract** (§2). Each owner has its own type, and each write is a method on that type. A **view** reads its session's frame and its document's content through read doors. It talks to the session through four input doors: input, a resize proposal, a "seen at revision N" report, and an attention answer. It never writes the session's fields. A **session** never reads a view (true of `drain_leaf_pty` today: `main.rs:42498` touches no view field). A **document** changes `dirty` only through its edit, save, discard and disk-news doors. That is true of every product write today (`preview.rs:5469`, `:5710`, `:5800`, `:6257`), but the field is `pub` (`preview.rs:4781`). Lifetimes: a pane close drops the view, then the session (retired off the window thread), while a document stays in its pool under the eviction law. The census is the gate for one writer per fact.
- **The migration** (§3). Nine moves, no change in behaviour. View fields go first, least-referenced first. They carry no census unknown rows and no multi-writer facts except `projection` and `frame_image_references`. Then the launch facts and the attention account (each with one writer today), then the birth/death record, then the session. The structural step (an application registry of sessions keyed by incarnation, and windows that hold views) is 0.5's first ticket. J3 owes three products (§3.1). Every move keeps its field names, so `ownership-census-unknowns.tsv` rows (keyed `?.field`) do not change. The census `STRUCTS` list grows by one type per step.
- **What 0.5 gets** (§4). The contract satisfies a session handed to another window and an agent writing a document the person is viewing. A document in two views is satisfied within a tab and **across tabs only after Q1**. Two views of one terminal (the drag-out) share one selection and one math hover until `bt-term` splits its view state, which is §4.2's unsatisfied requirement.
- **Five owner questions** (§6), each with a recommendation. **Nine tickets** (§5).

---

## 1. The measured present

### 1.1 How the table was taken

- **Writers.** `bt_source::FieldCensus` at `77399d57` (`cargo test -p bt-source --test census`, 10 passed; `target/ownership-census/ownership-census-{inventory,sites}.tsv`).
  - A *write* is proven: an assignment, a mutating call, or hub membership.
  - ↗ marks a function that only lends the field by `&mut` (the census's `access` column).
  - The module is the declaring item's module. `main` is `main.rs`. A bare name is `runtime/<name>.rs`.
- **Readers.** The census does not count reads. The *readers* column is a lexical count, in product code (`*tests.rs` files and inline `#[cfg(test)] mod … { }` blocks excluded, comments and literals masked), of `.<field>` not followed by `(`, per file.
  - † marks a field name that other structs share (`profile`, `program`, `integration`, `successor`, `wake`, `session`, `metrics`, `grid`). For those the count is an upper bound.
  - It is a scratch measurement, not committed. Producing it properly is J3's job (§3.1).
- **Owner.** **S** = session, **S·life** = the session's birth and death record (§1.2), **V** = view, **D** = document. The definitions are the plan row's.

### 1.2 The table

| # | field (`main.rs`) | census writers — module: functions | readers (lexical) | owner |
|---:|---|---|---|---|
| 1 | `pty` :11565 | **n** — main: `finish_leaf_birth`, `TabState::retire_all_shells`; panes: `close_pane`; tabs: `reap_exited_tabs`; ↗ `apply_owed_shell_resize`, `release_due_leaf_resize`, clipboard `send_paste`, terminal `insert_path_into_terminal` | main 9, panes 2, tabs 2, foreground_program, dpi, keyboard | S |
| 2 | `shell_exit_age` :11567 | tabs: `reap_exited_tabs`, `land_birth_death_fallback` | tabs 2 | S·life |
| 3 | `shell_exit_said` :11574 | tabs: `reap_exited_tabs`, `keep_birth_death_face`, `land_birth_death_fallback` | tabs 4 | S·life |
| 4 | `birth_death_fallback` :11576 | tabs: `reap_exited_tabs`, `land_birth_death_fallback`, `owe_birth_death_fallback_resize` | tabs 5, panes 1 | S·life |
| 5 | `birth_death_kept` :11578 | tabs: `keep_birth_death_face` | tabs 2 | S·life |
| 6 | `spawn_fallback` :11580 | **n** — main: `finish_leaf_birth`, `land_birth_death_fallback_shell`; panes: `land_own_shell`, `land_restart` | panes 3, main 2 | S·life |
| 7 | `birth` :11591 | main: `await_shell`, `create_leaf_session`, `land_shell_birth`, `release_due_leaf_resize`; ↗ clipboard `send_paste`, terminal `insert_path_into_terminal` | main 10, panes, tabs, terminal | S·life |
| 8 | `successor` :11596 | membership — main: `land_successor`; panes: `land_pane_birth`; terminal: `restart_shell` | main 4, panes 3, terminal 2 † | S·life |
| 9 | `foreground_program_cadence` :11597 | foreground_program: `apply_answer`, `dispatch`, `request` | foreground_program 4 | S |
| 10 | `incarnation` :11615 | — (construction only) | foreground_program 3, clipboard 3, math, preview, main | S |
| 11 | `wake` :11627 | **n** — main: `finish_leaf_birth`; tabs: `keep_birth_death_face` (and `LeafWake::rebind`, `main.rs:12507`, through the `Arc`: inner mutability, not counted) | main 1, tabs 2 † | S |
| 12 | `profile` :11650 | main: `finish_leaf_birth` | main 16 and twelve others † | S |
| 13 | `paste_recipient` :11652 | main: `finish_leaf_birth` | main 3, clipboard 2, terminal 1 | S |
| 14 | `integration` :11668 | main: `finish_leaf_birth` (1 unknown site: *incomplete*) | main 4, profiles 12 † | S |
| 15 | `program` :11680 | main: `finish_leaf_birth` (1 unknown: *incomplete*) | main 8 † | S |
| 16 | `spawn_place` :11704 | main: `place_leaf` | main 4 | S |
| 17 | `born_named` :11716 | main: `place_leaf` | main 3 | S |
| 18 | `pending_typing` :11732 | main: `Runtime::queue_typing_in_the_focused_pane`, `drain_leaf_pty` | main 2 | S |
| 19 | `session` :11733 | **n** — 39 functions in 10 modules (main 17, math 9, terminal 3, panes 2, tabs 2, foreground_program 2, dpi, frame, mouse, preview 1 each); 2 unknowns: *incomplete* | main 80, math 18, panes 14, terminal 14, and ten more † | S, with view-state setters (§1.3) |
| 20 | `attention` :11756 | **n** — main: `FolioApp::transfer_tab`, `answer_attention_in`, `deliver_attention`, `deliver_osc_attention`, `expire_leaf_attention`, `settle_attention`; attention: `mark_attention_seen` | main 19, attention 3 | S |
| 21 | `bell_reported` :11768 | main: `settle_attention` | main 3 | S |
| 22 | `attention_clock` :11770 | main: `deliver_attention`, `settle_attention` | main 4 | S |
| 23 | `attention_capability` :11777 | — | main 1 | S |
| 24 | `projection` :11778 | **n** — main: `return_to_live_for_input`, `TabState::clear_leaf_selection`, `finish_leaf_birth`; keyboard: `keyboard_input`; panes: `scroll_seat_by_columns`, `scroll_seat_to_column`, `scroll_seat_to_subpixels`; search: `clear_search_highlights`, `install_search_highlights`, `reveal_current_search_hit`; terminal: `jump_to_command_mark`, `scroll_view`, `scroll_view_exact_in`; ↗ `apply_leaf_metrics`, frame `publish_frame_inner`, `redraw`, clipboard `copy_selection` | terminal 16, main 12, panes 7, keyboard 4, search 4, and five more | V |
| 25 | `text_scale` :11788 | main: `step_leaf_text_scale` | panes 5, main 3, tooltips 2, terminal 1 | V |
| 26 | `metrics` :11796 | main: `apply_leaf_metrics` | main 5, frame 4, math 3 † | V |
| 27 | `thumb_awake` :11809 | main: `Runtime::wake_terminal_thumb` | main 2, terminal 1 | V |
| 28 | `column_awake` :11819 | terminal: `wake_terminal_column` | main 2, terminal 1 | V |
| 29 | `card_skip` :11822 | ↗ main: `aim_card_window` | main 9 | V |
| 30 | `grid` :11823 | main: `release_due_leaf_resize`, `schedule_leaf_grid_change` | main 12, dpi 3, terminal 2 † | S (§1.3 ruling c) |
| 31 | `conpty_grid` :11824 | main: `release_due_leaf_resize` | main 6, dpi 2, tabs 2 | S |
| 32 | `pending_pty_resize` :11825 | ↗ main: `release_due_leaf_resize`, `schedule_leaf_grid_change` | main 2 | S |
| 33 | `pending_psreadline_resize_reanchor` :11826 | ↗ main: `release_due_leaf_resize`; dpi: `finish_resize_if_quiescent` | main 1, dpi 1 | S |
| 34 | `last_finished_command` :11848 | main: `drain_leaf_pty` | main 2 | S |
| 35 | `has_rail` :11872 | main: `LeafSession::hear_first_mark` | main 3 | S (§1.3 ruling c) |
| 36 | `output_revision` :11881 | main: `drain_leaf_pty` | main 6 | S |
| 37 | `last_seen_revision` :11888 | main: `mark_leaf_painted`, `mark_leaf_seen` | main 5 | S (§1.3 ruling d) |
| 38 | `last_presented_frame` :11897 | frame: `redraw` | frame 7, preview 7, panes 3, peek 2, and five more | V |
| 39 | `presented_metrics` :11905 | frame: `redraw` | panes 3, preview 3, frame 2 | V |
| 40 | `frame_image_references` :11914 | **n** — frame: `publish_frame_inner`; panes: `rescan_pane_references` | panes 2, preview 2, peek 1 | V |
| 41 | `pending_paste` :11937 | main: `stage_paste` (`main.rs:1301`), `take_pending_paste` (`:1346`); ↗ clipboard `press_paste_card_key` | main 4, clipboard 1 | S (§1.3 ruling e) |

**Agreement with the committed annotations.** `ownership-census-annotations.tsv` rows 58–64 hold exactly the seven multi-writer facts above, with module owners: `attention` → attention (→ bt-workbench), `frame_image_references` and `projection` → frame, `pty` → panes, `session` → terminal, and `spawn_fallback` and `wake` → the leaf-birth road. Those annotations say which **module** writes a fact. This note says which **owner type** holds it. The two are independent: a field can stay single-module and still move from `LeafSession` to `TermView`.

### 1.3 Rulings on the fields that fit no owner cleanly

- **(a) The birth and death record is the session's** (rows 2–8). These fields are the PTY lifecycle in the plan row's sense. A shell can be deciding (`birth: Programs/Locale`), being born on its worker (`birth: Shell`), alive (`pty`), replaced in place (`successor`, a whole second `LeafSession` in a `Box`), dead at birth with a fallback coming (`birth_death_fallback`), kept dead on purpose (`birth_death_kept`), or exited and reported (`shell_exit_age`, `shell_exit_said`). `spawn_fallback` is a one-time hand-off from the birth road to the code that knows the pane's seat, which is the binding between session and view (§2.1), not a view fact (annotation row 63). These are **mutually exclusive states written as seven independent fields**. The contract makes them one session-owned value, `ShellLife` (§3.3 M6). That ticket is a pure refactor, and its proof is the census (seven facts become one) plus the existing birth tests.
- **(b) `wake` is the session's, and the registry writes it.** It is the window end of the reader thread's closure (`LeafWake`, `main.rs:12483`). Which window to nudge is a fact about where the session is shown, so only the door that binds a session to a window may write it. Today that is `transfer_tab`'s commit (`main.rs:63093`, `rebind`). `keep_birth_death_face` also clears it when it keeps a dead pane. Under the contract that clear becomes the session's own lifecycle transition (a session with no PTY has no wake), so `wake` gets one writer per kind of change.
- **(c) The grid is the session's, and the view proposes it.** The PTY has one size (WB§11, *one PTY, one size*). `grid`, `conpty_grid`, `pending_pty_resize` and `pending_psreadline_resize_reanchor` are the resize transaction with the child, so they are session facts. A view's metrics, text size and body rectangle produce a **resize proposal**. That is `cmdrail::terminal_grid_for(&metrics, body, has_rail)` in `create_leaf_session` (`main.rs:40027`) and in `schedule_leaf_grid_change` (`:21207`). `has_rail` is an input to the grid that the shell's first `OSC 133` mark sets (`hear_first_mark`), so it is the session's, and the view reads it.
- **(d) "Unread" is the session's, and views report what they have seen.** `output_revision` counts what the shell said. `last_seen_revision` records what reached the glass. A view's paint is an observation the session receives (A§4.1 lists *seen observations* among a view's inputs). The rule is that a session takes "seen up to revision N" through one door, `mark_seen(n)`, which today is two free functions, `mark_leaf_painted` and `mark_leaf_seen` (`main.rs:41379`, `:41361`). Two views of one session then cannot make the unread count go backwards: the door takes the maximum.
- **(e) A pending paste is the session's, and the card is the view's.** The comment on `pending_paste` already says so: *text promised to this process … dies with it*. The card's hover (`WindowRuntime.paste_card_hover`) stays window view state.
- **(f) `session: DualPlaneSession` is a session with view setters inside it.** Of its 39 writer functions, these set **view** state: `TabState::{set,clear}_leaf_selection` → `set_view_selection`; math's `set_hovered_math`, `present_math_toggle`, `sample_math_toggle`, `settle_math_toggle` → `set_math_hover`, `set_math_toggle_presentation`; mouse `mouse_wheel` → `scroll_math_block`; `apply_leaf_metrics` → the four cell-size setters; `sync_math_layout_key` and `apply_line_wrapping` → `set_layout_key`; frame `publish_frame_inner` → `release_presentation_hold_for_user_input`. The rest are session state: `feed`/`feed_at`, `resize_at`, `set_pty_transport`, `take_notifications`, `clear_attention`, `set_spawn_*`, `apply_foreground_program`, `reset_program_modes`, `set_frozen_quota`, and others.
  - **The ruling for this contract:** `DualPlaneSession` stays whole on the session side. Its view setters are reached only through the **view's doors**, so the bt-app half of the split is clean. The `bt-term` half (selection and hover per view) is §4.2's unsatisfied requirement. It is a separate note, not this one.
  - Under one PTY, one size, the cell metrics and the layout key are the same for every view of a session: the sizing view's (§2.2).
- **(g) The orphaned doc comment.** Lines `main.rs:11915–11927` describe `integration_offer` (*What this pane owes the reader about its shell integration*), and no field follows them. The field is gone (it was a census row on 2026-09-25: `ownership-census-2026-09-25.tsv`, `LeafSession.integration_offer`), and its doc now reads as the first half of `pending_paste`'s. This is recorded in §7. The fix is a one-line deletion in M1's commit.

### 1.4 What already has the shape

- **Document**: `PreviewBuffer` holds content, line index, `dirty`, `revision`, the edit log for views (`viewport_edits`), `incarnation`, disk baseline (`disk_mtime`, `disk: DiskNews`), load state, encoding and `undo` (`preview.rs:4750–4950`). `PreviewPool::open` is its one door in, and the eviction law (*earliest clean, not displayed, not the one just opened; else evict nothing*, `preview.rs:6285` ff.) is what makes "a view without a document" impossible today.
- **View**: `PreviewPane` (`main.rs:4723`) is *the content reference and the view of it* — buffer by identity, the parse at this width, scroll, zoom, block scroll, caret, selection, the paint artifacts. Its own doc states the rule this note generalises: *two panes on one buffer are one buffer at two scroll positions* (ruling 8⑧). `PreviewPanes::remove` (`main.rs:5245` ff.) drops a view and nothing else.
- **What is not in either**: input ownership singletons — `TabState.preview_edit_focus`, `preview_link_hover`, `terminal_thumb_hover` and the `WindowRuntime` hover/press/menu fields (census class VIEW, CEN§2 rows 40–100). They *name* a view; they are the window's (one pointer, one keyboard). The contract leaves them where they are.

---

## 2. The contract

### 2.1 Types, and who may touch what

| owner | type (target name) | holds | written only by | creates | drops |
|---|---|---|---|---|---|
| **Session** | `ShellSession` (today the S rows of `LeafSession`) | identity (`SessionId` = today's `incarnation`), `ShellLife`, launch facts, `DualPlaneSession`, grid and resize transaction, attention account, unread revisions, owed input (`pending_typing`, `pending_paste`) | its own methods | the birth door (`create_leaf_session`'s session half) | the retire door — `bt_pty::retire_session` off the window thread (`runtime/panes.rs:1466` ff.); see SE§2 |
| **Document** | `PreviewBuffer` (renamed when 0.5 adds kinds) | content, revision, edit log, undo, dirty, encoding, disk baseline | its own methods: edit, undo/redo, save, discard, `land_read`, disk news | `PreviewPool::open` | the pool's eviction law, or `discard_dirty`/`keep_dirty` on the failure and quit roads |
| **View** | `TermView` (today the V rows) and `PreviewPane` | scroll, projection, selection *reference*, text size, metrics, the last presented frame and its metrics and image references, fade clocks, card skip | its own methods and the frame commit | the seat that shows it (split, tab, float, card) | the seat closing (`close_pane`, `PreviewPanes::remove`) |
| **Binding** | `TabState.sessions` today; an application `SessionRegistry` in 0.5 | `SeatId → SessionId`, `PreviewSurface → PreviewSource`, and which window each session wakes | the doors that move panes and tabs (`transfer_tab`, `move_seat_content`, `pane_into_new_tab`) | — | — |

**Reads and writes across owners.** *Through a door* means a method on the owner, never a field reached through another type.

| from \ to | session | document | view |
|---|---|---|---|
| **session** | — | never (a shell writes files on disk; the document hears it as disk news, SE§4.2) | **never** |
| **document** | never | — | never (views pull; the document publishes an edit log by revision) |
| **view** | reads the frame, unread state, `has_rail` and grid. Writes only through four input doors: `offer_input`, `propose_size(grid)`, `mark_seen(revision)`, `answer_attention(action)` (A§4.1's list) | reads content and edits by revision (`viewport_edits`). Writes only through the edit door (`edit(at_revision, change, author)`), save and discard | — |

### 2.2 Invariants

1. **One writer per fact.** Every field of the three types has its writers inside its own type's `impl`. The census is the gate: `FieldCensus` with `STRUCTS` naming the three types, and an annotation row for any fact with writers in more than one module. A multi-writer fact on a contract type is a finding, not an annotation.
2. **A view without a document is impossible** (document views). `PreviewPane.buffer = Some(source)` always resolves in its pool. This holds today because of the eviction law, which never evicts a displayed source. It becomes a type rule when `PreviewPane`'s content becomes `enum { Document(source), Picture(..), Page(..), GitGraph(..) }` (ticket O-8). A terminal view without a session is impossible for the same reason it is today: `sessions_match_terminals` (`main.rs:35135`).
3. **A document's `dirty` flag changes only through its own doors.** True today: `settle_after_a_change` (`preview.rs:5694`), `the_body_is_the_files` (`:6253`) and `say` (`:5453`). The census does not see it, because `dirty` is a field of `PreviewBuffer`, which is not one of the census's four structs, and `pub` (`:4781`). Ticket O-7 makes `dirty`, `revision`, `content`, `disk` and `undo` private, with read accessors.
4. **A session never reads a view**, and the sizing view is unique. A session shown in several views (§4.2) has exactly one **sizing view**, the pane in the layout. A secondary view (a float, a card, a focus thumbnail) scales or clips that grid and proposes nothing.
   - DESIGN §7.0 gave a session one inputtable view and made every other projection read-only. WB§11 widened that for the drag-out: *scroll history, type, close*.
   - The contract allows both. Input is the session's door, and it orders input from any view. Whether a secondary view may offer input is policy, not structure (DESIGN §7.0, law ③).
5. **Unread never goes backwards.** `mark_seen` takes the maximum (§1.3 d).

### 2.3 Lifetimes and the product rules they keep

- **A pane closes** (`close_pane`, `runtime/panes.rs:1466`):
  1. The dirty gate goes first, and a refusal stops everything.
  2. If this is the last pane, the tab closes instead (DESIGN §7.1.4, `closing_this_pane_closes_the_tab`).
  3. Otherwise the **view** drops (`clear_preview_view`, or the `TermView` half of the leaf).
  4. Then the **session** drops: attention is expired (`expire_leaf_attention`), then the PTY is retired off the window thread (`bt_pty::retire_session`).
  5. A **document** is not dropped by a close. It stays in the pool, and the eviction law or the quit road decides. *Drops the document if clean* is the eviction law's job, not the close's.
- **A tab without a shell** (DESIGN §7.1.6h; `TabState.sessions` may be empty, `main.rs:12137`): documents and views with no session. This is the rule's own case, not an exception to it.
- **Quit, failure and the trial.**
  - The quit transaction (`quit::Quit`; A§11) and `stop_every_window` read sessions and documents through their doors: the photograph reads the session's launch facts and the view's `card_skip`, and `keep_dirty` and `save_dirty` are document doors.
  - The trial's write gate (`update_trial::defer`) holds document saves in the same place it holds every other durable write.
  - The contract adds nothing to these roads. It only names whose door each one calls.
- **The restore card and window close.** An unanswered card's plan is the window's, not any session's (DESIGN §2.7, `docs/DESIGN.md:711`; §7.1.4). Closing a window with the card up folds the plan back exactly as today, and no session lifetime is involved.
- **A session that loses its last view** happens in 0.4.x only through close, and close also retires the session explicitly. "A view disappearing is not a session ending" becomes a real case only when a second view exists (0.5, §4). Then the registry retires a session when **its sizing view's seat closes**, not when any view drops.

---

## 3. The migration — no change in behaviour

### 3.1 What J3 (T-RUNTIME-DEP-MAP) must produce first

1. **Reads by item, per field**, for the three types and for `PreviewBuffer`/`PreviewPane`: the census's `access` and `write` columns, plus a `read` column through the same receiver resolution. §1.2's lexical counts are the stand-in. They over-count the † names, and J3 replaces them.
2. **Topic → topic call edges** over `runtime/*.rs` and `main.rs`'s free functions, so each move below knows which files it opens.
3. **A per-field "doors already present" list**: the free functions that already act as a field's door (`apply_leaf_metrics`, `step_leaf_text_scale`, `mark_leaf_seen`, `drain_leaf_pty`, `schedule_leaf_grid_change`, `release_due_leaf_resize`, `settle_attention`, `stage_paste`/`take_pending_paste`). Each one becomes a method on its owner type.

J3 is generated by a script kept in the repository (plan row J3). It must read through `bt-source` (the tripwire refuses `.ps1` and Python source readers: CEN§1, *Why no script is committed*).

### 3.2 Order and why

- **K2 lands first.** K2 (T-SPLIT-2B) moves `main.rs`'s free functions into `runtime/*`. Doing the field moves before K2 would move every writer twice.
- **Then view before session**, and least-referenced first. View fields carry **no** census unknown rows (the inventory's `unknown_sites` for `LeafSession` are `session` 2, `attention` 1, `attention_clock` 1, `integration` 1, `program` 1). Only two of them are multi-writer (`projection` and `frame_image_references`).
- **Then the single-writer session blocks** (launch facts, attention), then the lifecycle record, which has the most interlocking writers.
- **The registry comes last**, because it is the one step that changes where a value lives across windows.

### 3.3 The moves

| step | what moves (lexical refs) | into | writers that change file | census delta |
|---|---|---|---|---|
| **M0** | nothing; `STRUCTS` gains `TermView`, `ShellSession` (empty or nested) | — | — | none; the test `every_field_of_the_four_structs_has_an_inventory_row` becomes "of the declared structs" |
| **M1** | `thumb_awake` (3), `column_awake` (3), `frame_image_references` (5), `presented_metrics` (8), `card_skip` (9); delete the orphaned doc (§1.3 g) | `LeafSession.view: TermView` (nested; same lifetime) | none (paths become `leaf.view.x`) | 5 inventory rows renamed `LeafSession.x` → `TermView.x`; annotation row 59 renamed |
| **M2** | `text_scale` (11), `metrics` (15) | `TermView` | `step_leaf_text_scale`, `apply_leaf_metrics` become `TermView` methods; `apply_leaf_metrics`'s four `DualPlaneSession` setters become one session door `set_cell_metrics` called by the view's door | 2 rows renamed |
| **M3** | `last_presented_frame` (24) | `TermView` | `redraw`'s commit writes both presented fields through one view method `commit_presented(frame, metrics, references)` | 1 row renamed |
| **M4** | `projection` (50) | `TermView` | the 13 writers call view methods. `return_to_live_for_input` and `clear_leaf_selection` write session and view together, so each becomes one view door that calls one session door | 1 multi-writer row renamed (annotation row 60) |
| **M5** | `profile`, `paste_recipient`, `integration`, `program`, `spawn_place`, `born_named` | `ShellSession.launch: LaunchFacts` | `finish_leaf_birth`, `place_leaf` | 6 renamed; the one unknown each on `integration` and `program` keeps its `?.field` key |
| **M6** | rows 2–8 → `ShellLife` (one enum) | `ShellSession.life` | `reap_exited_tabs`, `keep_birth_death_face`, `land_*`, `await_shell`, `create_leaf_session` call `ShellLife` transitions | 7 rows → 1. The multi-writer `spawn_fallback` (annotation 63) becomes a variant's payload, taken once |
| **M7** | `attention`, `bell_reported`, `attention_clock`, `attention_capability` | `ShellSession.attention: AttentionAccount` (`bt-workbench` holds the ledger already, D-57) | the seven attention writers call account methods | 4 → 1 multi-writer row (annotation 58) until census-4 |
| **M8** | what remains: `LeafSession` → `ShellSession`; `TabState.sessions: BTreeMap<SeatId, TerminalPane { session, view }>` | — | `transfer_tab`, `move_seat_content`, `pane_into_new_tab`, `absorb_tab_sessions` | struct renamed; `TabState.sessions` row unchanged |

M8 still keeps session and view in one map entry with one lifetime: **no behaviour changes**. The 0.5 registry (O-6) is the first ticket that separates their lifetimes, and it carries its own tests (§3.4).

### 3.4 What must read the same after each move, and what pins it

- **The census.**
  - Inventory and sites: the only diff is the renamed `fact` column for the moved rows. The rows' functions and modules stay equal, which is the census's own *moved changes only one column* property (`census.rs:115`, MUTATION line there).
  - Annotations are renamed in the same commit. Unknowns do not change, because field names are kept.
  - `check-census-unknowns.ps1` stays green.
- **The window-waits registry.** `every_bare_site_is_a_row_and_every_row_a_site` and `the_effects_section_is_the_effect_functions_inside_the_doors` (`window_waits_tests.rs:6986`, `:6782`) must hold with `window_waits.tsv` byte-for-byte unchanged. No effect moves in M0–M8. A diff to the TSV is a finding.
- **Test shells.** `bt-source`'s `test_shells` is unaffected (no test starts a shell differently). It runs as part of the guard set.
- **The test count is equal before and after each step**, by the K1 census. No test is rewritten, except where it named a moved field path (`leaf.text_scale` → `leaf.view.text_scale`). Such an edit is mechanical and is listed in the commit body.
- **Freeze windows.** Each step's commit owns `main.rs`'s `struct LeafSession` block and the files its "writers" column names, from its first edit until it merges (at most one working day). Other work in those files waits or rebases after. The plan's freeze slot is shared with K2's batches.
- **New pins, only for M8 and O-6** (the steps that state a rule rather than move a field):
  - `a_closed_pane_retires_its_session_after_its_view` — MUTATION: retire the session before the view drops (an attention expiry for a seat whose view is gone becomes observable).
  - `a_view_reports_seen_and_unread_never_goes_back` — MUTATION: `mark_seen` stores instead of taking the maximum.
  - (O-6) `a_session_outlives_a_secondary_view` — MUTATION: retire the session when any view drops.
  - (O-6) `the_sizing_view_alone_proposes_the_grid` — MUTATION: a float view's `propose_size` reaches `resize_at`.
  - Tests carry Chinese and mixed-script samples where they type into a shell (CONVENTIONS §三).

---

## 4. What 0.5 needs from it

### 4.1 Satisfied

- **A session handed to another window.** Today `transfer_tab` moves the whole `LeafSession` and rebinds its wake in a commit that cannot fail (`main.rs:62936`, `:63093`). Under the contract the hand-over is three steps: the binding moves the `SessionId`, the old window drops its view, and the new window creates one. `wake` follows the binding (§1.3 b). After O-6 the session itself stays in the registry and never moves.
- **An agent writing a document the person is viewing.**
  - *Through the disk* (an agent's own process writes the file): the document hears it as disk news (`note_disk_moved`). A clean document re-reads. A dirty one keeps its edit and says *changed on disk* (`SaveOutcome::Conflict`).
  - *Through Folio* (0.5's CLI/MCP, WB§6): the agent calls the same edit door as the person, `edit(at_revision, change, author)`. An edit against a stale revision is refused. The document never merges two authors silently.
  - The views heal their carets from the edit log. This is what `step_preview_history` already does for two panes (`runtime/preview.rs:7728`): *the other pane's caret … is clamped into the body the next time it is used*.
- **A document shown in two views within a tab**: exactly today's pool and pane design.
- **The agent rail's routing credential** (WB§11.2: `FOLIO_ATTENTION`, 128 bits, *held on the leaf, gone the instant the leaf is*). It becomes `ShellSession.attention.capability`, with the same lifetime.

### 4.2 Not satisfied, and why

1. **Two interactive views of one terminal** (WB§11 *drag it out*) share one selection, one math hover and one math-toggle presentation. Those live in `DualPlaneSession` (§1.3 f), which is a `bt-term` type. Splitting them is a `bt-term` ownership change that gets its own note. Until then the float view and the pane show the same selection, which is a reasonable reading of *a shared view, not takeover*. The grid half is satisfied (§2.2 invariant 4).
2. **A document shown in two views across tabs or windows** is two buffers today, because the pool is per tab (the 2026-07-17 ruling: *a file open in two panes must not become two buffers*, scoped to the tab: `TabState.preview_pool`, `main.rs:12290`). The contract supports either scope. Which one is Q1.

---

## 5. Tickets, in order

| # | ticket | size | after | notes |
|---|---|---|---|---|
| O-1 | J3 T-RUNTIME-DEP-MAP's three products (§3.1) | S/M | I1 | already planned; this note is its consumer |
| O-2 | T-CONTRACT-M0-M1: census `STRUCTS` declared, `TermView` nested, five view fields, the orphaned doc removed | S | K2, O-1 | first commit of the split |
| O-3 | T-CONTRACT-M2-M4: text size, metrics, presented frame and projection into `TermView`; `set_cell_metrics` and the paired view-then-session doors | M | O-2 | freeze `runtime/{terminal,panes,frame,search,keyboard}.rs` per batch |
| O-4 | T-CONTRACT-M5-M7: `LaunchFacts`, `ShellLife`, `AttentionAccount` | M | O-3 | M6 is the one with logic: seven fields become one enum; birth tests unchanged |
| O-5 | T-CONTRACT-M8: `ShellSession` and `TerminalPane`; the M8 pins | S | O-4 | ends 0.4.8 / opens 0.5 |
| O-6 | T-SESSION-REGISTRY (0.5, D-1's first slice): `SessionId`, the application registry, windows hold views, the sizing view, the O-6 pins | L | O-5, Q2 | the first lifetime change; its own Codex round on the diff |
| O-7 | T-DOCUMENT-PRIVATE: `PreviewBuffer`'s `pub` state fields private with accessors; `PreviewBuffer` and `PreviewPane` join the census | S | — | can run beside O-2 |
| O-8 | T-VIEW-NEEDS-DOCUMENT: `PreviewPane`'s content as one enum (§2.2 invariant 2) | M | O-7 | — |
| O-9 | T-DOCUMENT-REGISTRY (0.5): documents application-wide | M | Q1, O-6 | only if Q1 is answered "per application" |

---

## 6. Questions for the owner (recommendation first)

1. **Should one file be one buffer per tab (today) or per application?** Today's answer is a ruling: DESIGN §7.0 says *one copy of one file per tab*, and the pool's 2026-07-17 placement says the same. **Recommend per application.** 0.5 shows documents in floats, cards and other windows, and an agent's edit should land in the one buffer the person sees, wherever it is. The cost is that the dirty gate on closing a tab must ask about documents that only that tab shows.
2. **Should a dragged-out terminal view share its selection with the pane until `bt-term` splits view state?** **Recommend yes, for 0.5**, and record it as a known limit (§4.2 item 1). Per-view selection is a `bt-term` note.
3. **Should "unread" be the session's, with every view reporting what it painted?** **Recommend yes** (§1.3 d). One unread count per shell, whichever view shows it.
4. **Does a secondary view (float, card) have its own text size?** **Recommend no.** The grid is the sizing view's (one PTY, one size). A secondary view scales the picture and keeps the 2026-09-23 ruling (*a pane's text size is never persisted*) unchanged for the pane.
5. **Does a session end only when its sizing view's seat closes, not when a secondary view closes?** **Recommend yes.** It is the reading of *a view disappearing is not a session ending* that keeps DESIGN §7.1.4's close rules word for word.

---

## 7. Found along the way

- `main.rs:11915–11927`: a doc comment for a field that no longer exists (`integration_offer`) sits in front of `pending_paste` and reads as part of its doc. M1 removes it.
- `dirty`, `revision`, `content`, `disk` and `undo` on `PreviewBuffer` are `pub` (`preview.rs:4757–4949`). Every product write is in the type's own methods, but tests write `dirty` directly (`preview.rs:8244`, `:8481` ff.), so O-7 also touches those tests.
- The census cannot see writes into a document. `PreviewBuffer` is not one of its four structs, so `save_preview_on`'s write shows up only as an access lend of `TabState.preview_pool` through `preview_buffer_on_mut` (sites report). O-7 makes the census cover `PreviewBuffer`.

## 8. Sources

`crates/bt-app/src/main.rs` (`struct LeafSession` 11564–11938, `TabState` 12137 ff., `PreviewPane` 4723, `PreviewPanes` 5245, `create_leaf_session` 40027, `drain_leaf_pty` 42498, `LeafWake` 12483, `transfer_tab` 62936); `crates/bt-app/src/preview.rs` (`PreviewBuffer` 4750, `PreviewPool` 6285); `crates/bt-app/src/runtime/panes.rs` (`close_pane` 1466, `land_births` 60); `crates/bt-source/tests/census.rs`; `docs/plans/design/ownership-census-annotations.tsv`, `-unknowns.tsv`; `target/ownership-census/*` at `77399d57`; `docs/ARCHITECTURE.md` §4.1–§4.4, §11; `docs/DESIGN.md` §2.7 (:711), §2.9 (:742), §7.0 (:1290), §7.1.4 (:1748), §7.1.6h (:3076); `docs/plans/structural-debt.md` D-1, D-17, D-54, D-57; `docs/plans/bt-app-split.md` §8; `docs/plans/design/agent-workbench-0.5-2026-09-20.md` §6, §11.

## 9. Architecture impact

- **Owned facts.** All 41 `LeafSession` facts change their owner type (§1.2). None changes its writer module in M0–M8. O-6 moves sessions from per-tab to per-application, which is an ownership change under RULES 55. O-6 writes its A§4.4 row and A§4.1's table, and O-9 writes another if Q1 is answered "per application".
- **Doors.** None added or moved by M0–M8. `window_waits.tsv` is unchanged.
- **Debt.** D-1 is narrowed by O-5 and repaid in part by O-6 (the backend half stays 0.6). D-17 gets its document owner from O-7 and O-8. D-54's epochs come from the registry's `SessionId`. This note adds no debt row. It records the orphaned doc comment (§7) for M1.
