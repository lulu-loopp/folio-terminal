# Who owns a pane: the session / document / view contract, and how `LeafSession` splits — design note, 2026-10-11

Status: **revision (d), 2026-10-11**, after the scoped Codex look (`reports/J5-05-contracts.review-codex-c.md`); (d) stands over (c). Before that, **revision (c)**, after the Codex confirmation round (`reports/J5-05-contracts.review-codex-b.md`); revision (c) stands over (b) where they differ, and (b) over (a). Before that, **revision (b)**, after the Codex review (`reports/J5-05-contracts.review-codex.md`, both notes *adopt with changes*). Revision (b) is the dated section at the end. **Where revision (a)'s text and revision (b) differ, revision (b) stands.** Revision (a) was the draft for review. Plan row J5 (T-05-CONTRACTS, 0.4.8), the first of the two contract notes the 2026-09-21 structure review asked for before 0.5; ledger row **D-1** (*session state has no owner independent of the window*, the 0.5 precondition). The companion note is `side-effects-contract-2026-10-11.md` (D-16, D-33). Read at `main` = `77399d57` on `design/05-contracts`. No product code, no test. One cargo run: `cargo test -p bt-source --test census` (10 passed), whose query reports under `target/ownership-census/` are §1's measured column. Facts about Folio are cited `file:line` at `77399d57` (paths under `crates/bt-app/src/` unless another crate is named) with the symbol beside the line.

Owner rulings 2026-10-11 (afternoon, "全按推荐" — every recommendation stands as the ruling; `trace/tickets-048/00-PLAN-048.md`, Owner rulings 2026-10-11 afternoon): Q1 one file = one buffer per application; Q2 a dragged-out terminal view shares its selection with the pane until bt-term splits its view state (0.5 known limit); Q3 unread belongs to the session, every view reports what it painted; Q4 the sizing role passes to the most recently focused remaining view; Q5 the session ends when its sizing seat in the layout closes (DESIGN §7.1.4 word for word). Tickets O-1…O-9 and T-ATTENTION-WINDOW-EXPIRY may be opened.

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

---

## Revision (b), 2026-10-11, after the Codex review

Review: `reports/J5-05-contracts.review-codex.md` (trace tree, 0.4.8). Verdict on this note: **adopt with changes**, six items, N1-1 to N1-6. All six are applied, and none is argued. The review's findings that stand unchanged are: all 41 fields placed; the census writer evidence; reader counts as lexical upper bounds until J3; `frame_image_references`, `paste_recipient` and `pending_paste` placed as in §1.2; *0 document fields*; the least-referenced-first order; field names kept for the unknown rows; the freeze windows; and O-6's separate review.

### (b)0 · What changed, item by item

| item | the review's words (abridged) | disposition | where |
|---|---|---|---|
| N1-1 | *"The four-door contract is incomplete, and `drain_leaf_pty` is not the stated proof"*; *"Replace 'four doors' with: durable session doors including keyboard-focus reporting; a resize proposal carrying all required sizing facts, not just the grid; explicitly named transitional `bt-term` view-state ports"* | applied | (b)1; replaces §0's contract bullet, §2.1's *view → session* cell, and §1.3 (f)'s "reached only through the view's doors" |
| N1-2 | *"`ShellLife` is not presently one set of mutually exclusive states … Replace the simple-enum premise with an explicit state/transition table … M6 needs new transition and mutation tests"* | applied | (b)2; replaces §1.3 (a)'s "mutually exclusive" sentence and §3.3's M6 row |
| N1-3 | *"one window-valued `LeafWake` is not an intrinsic session fact. Mark its present placement as transport debt and state that O-6 moves routing into the application binding/registry"* | applied | (b)3; replaces §1.3 (b) and the owner of §1.2 row 11 |
| N1-4 | *"Q5's recommendation contradicts the governing lifetime rule … Recommend instead that the registry owns session lifetime … O-6 must depend on Q4 and Q5, not only Q2"* | applied | (b)4; replaces §2.3's last bullet and §6 Q4–Q5 |
| N1-5 | *"it expressly supersedes DESIGN §7.0's per-tab buffer ruling. The note must specify"* the four consequences; *"Make O-9 an L ticket with its own design/Codex round"* | applied | (b)5; replaces §6 Q1's cost sentence and §5 O-9 |
| N1-6 | *"O-4 should be L … O-5 is at least M … O-9 is L. Add the `ShellLife` transition pins to O-4 and make O-6 depend on the lifetime/sizing rulings"* | applied | (b)6, the ticket table re-sized and re-ordered |

### (b)1 · The doors between a view and its session (N1-1)

The review is right on both counts.
- `drain_leaf_pty` takes `holds_the_keyboard`, a fact derived from the view and the window, and passes it to `DualPlaneSession::set_keyboard_focus` (`main.rs:42498`, `:42615`). So the drain does consume something the view knows.
- M2 needs the cell metrics and the layout key, not just a grid.

The doors from a view (or the window acting for it) to its session become three groups.

1. **Durable session doors**, which outlive the split:
   - `offer_input(bytes)`: the input ring, or the birth's held queue;
   - `report_keyboard_focus(bool)`: today the `holds_the_keyboard` argument of `drain_leaf_pty`, computed by the caller from window focus, the active tab and the focused seat (WB§13.2's one definition of "focused pane");
   - `mark_seen(revision)` (§1.3 d);
   - `answer_attention(action)`;
   - `stage_paste(text)` / `take_pending_paste()` (§1.3 e).
2. **One resize proposal carrying every sizing fact**: `propose_size(SizeProposal { body_px, cell: CellMetrics, wrapping: bool })`.
   - The session derives `grid` from `body_px`, `cell` and its own `has_rail` (`cmdrail::terminal_grid_for`).
   - It applies the four cell-size setters and `set_layout_key` to `DualPlaneSession`, and schedules the PTY resize (`schedule_leaf_grid_change`).
   - Only the sizing view may call it (§2.2 invariant 4).
   - It replaces `apply_leaf_metrics`'s direct `DualPlaneSession` calls, and the `set_layout_key` calls in `sync_math_layout_key` and `apply_line_wrapping`.
3. **Transitional `bt-term` view-state ports**, named so they can be counted and removed. Each is a `ShellSession` method that forwards to `DualPlaneSession` and is documented as a port:
   - `port_set_selection` / `port_clear_selection` (`set_view_selection`);
   - `port_math_hover` (`set_math_hover`);
   - `port_math_toggle_presentation` (`set_math_toggle_presentation`, `toggle_math_source`);
   - `port_scroll_math_block` (`scroll_math_block`);
   - `port_release_presentation_hold` (`release_presentation_hold_for_user_input`).

   They exist because this state lives in `bt-term` today (§1.3 f). A separate `bt-term` ownership note removes them. Until then each port has exactly one caller, a `TermView` door. As the review notes, search and selection-driven copy read the **view** once `projection` has moved, so they need no port.

**The proof, corrected.** The drain *consumes a focus report* and reaches into no `TermView` field. `drain_leaf_pty` reads no field of §1.2 rows 24–29 or 38–40 (`main.rs:42498–42700`). Its one view-derived input is the `holds_the_keyboard` parameter, which becomes `report_keyboard_focus`. O-3 pins it with `the_drain_takes_focus_as_a_report_and_reads_no_view_field`. MUTATION: the drain reads `leaf.view.projection` to decide whether to scroll to the bottom.

### (b)2 · The shell's life is a phase plus observations (N1-2)

The premise in revision (a) is wrong.
- `keep_birth_death_face` sets `birth_death_kept` **and** `shell_exit_said`, **and** clears `wake` (`runtime/tabs.rs:115–117`).
- On an ordinary exit, `shell_exit_age` is set at the first observation (`tabs.rs:3605`, `get_or_insert_with`) and `shell_exit_said` once the line is written (`tabs.rs:3678`). Both coexist with a `pty` that is still `Some`: the exited process stays held until the tab ends.

The record is **one phase** (exactly one at a time), the **payloads** each phase holds, and **orthogonal observations**:

| phase | holds (payload) | entered by | left by |
|---|---|---|---|
| `Deciding` | `PaneBirth` with `BirthWait::{Programs, Locale}`; no `pty` | `create_leaf_session` when the rows or the locale are unanswered | `land_pane_birth` → `Bearing`, or → `Shellless` (`Started::Nothing`) |
| `Bearing` | `PaneBirth` with `BirthWait::Shell(ShellLanding)` | `create_leaf_session` with everything decided | `land_shell_birth` → `Live`, or → `Shellless` (the birth failed) |
| `Live` | `pty: PtySession`; `wake` until O-6 ((b)3) | the landing | `reap_exited_tabs` first sees `try_wait` answer → `Exited`, or → `FallingBack` (a birth death with a fallback), or → `Kept` (a birth death without one); `close_pane` → retired |
| `Exited` | `pty` (exited, still held), `exit_age` | `reap_exited_tabs` | the tab ending (`tab_has_ended`) → retired |
| `FallingBack` | `BirthDeathFallback { landing, exit }`, `exit_age`; the old `pty` already retired | `reap_exited_tabs` (a birth death, fallback available) | `land_birth_death_fallback` → `Live` (the fallback took the pane), or → `Kept` |
| `Kept` | `exit_age`; no `pty`, no `wake` | `keep_birth_death_face` | `close_pane` → retired |
| `Shellless` | nothing (the no-program banner; a `BT_PROBE_INPUT` pane) | a landing with `Started::Nothing` | `close_pane` |

Orthogonal to the phase:
- `shell_exit_said` is an observation: *the one diagnostics line has been written*. It is valid in `Exited`, `FallingBack` and `Kept`.
- `successor: Option<Box<…>>` is a *Restart shell* in birth beside a `Live` or `Exited` phase. It is a second session record, not a phase of this one, and it lands into this slot through `land_successor`.
- `spawn_fallback` is a one-shot payload handed from the birth to the binding at the landing, taken once by `land_own_shell` or `land_restart`.

So M6 becomes `ShellLife { phase: Phase, said: bool, successor: Option<Box<ShellSession>>, spawn_fallback: Option<ShellFallback> }`, with `pty`, `exit_age` and the birth records moved into the phases that hold them. **This is a conversion with logic.** Every edge in the table becomes a method, and the existing birth tests are not enough on their own. O-4 ships these pins, each with Chinese and mixed-script samples in the typed input wherever a shell is started, and each followed by its MUTATION:
- `every_life_transition_is_one_method_and_no_other`: a table test over the seven phases that drives each listed edge through its method and refuses every unlisted edge. MUTATION: let `keep_birth_death_face` leave a `Live` phase directly.
- `an_exited_shell_keeps_its_pty_until_the_tab_ends`. MUTATION: drop the `pty` on entering `Exited`.
- `the_exit_line_is_said_once_across_exited_falling_back_and_kept`. MUTATION: reset `said` on `FallingBack → Kept`.
- `a_successor_lands_into_a_live_or_exited_slot_and_nowhere_else`. MUTATION: let a successor land over `Bearing`.

### (b)3 · `wake` belongs to the binding, and where it sits today is transport debt (N1-3)

Applied. `wake` names *which window to nudge*, and `transfer_tab` rewrites it when the binding moves (`main.rs:63083–63093`). Once a session is attached to views in more than one window, one window-valued cell is not a fact about the session.

- **Target owner: the binding** (§2.1's fourth row). The application registry routes a session's output wake in one of two ways:
  - it keeps one `LeafWake` per session and rebinds it to the window of the sizing view; or
  - it replaces the per-window bit with one application-wide wake whose drain asks the registry which windows show the session.

  Choosing between them is O-6's design. Both keep the reader-thread rule: *a thread's wake is handed once, and the cell is where "which window" may change* (`LeafWake`, `main.rs:12460–12476`).
- **Today:** it stays a field of the session record through M0–M8. O-5 marks it as **transport debt** in its doc comment. No new ledger row is needed, because it is D-1's own content. `keep_birth_death_face`'s clear moves into the `Kept` transition ((b)2).
- **§1.2 recount:** **31 session** (24 plain + 7 life), **1 binding** (`wake`), **9 view**, 0 document. Row 11's owner now reads **Binding (transport debt until O-6)**.

### (b)4 · Session lifetime belongs to the registry (N1-4)

Applied. Revision (a)'s Q5 made one particular view's disappearance end the session. That is the rule A§4.1 forbids, and WB§13.3 item 3 lists *the source pane closing* among drag-out's open questions. The lifetime rule becomes:

- **The registry owns a session's lifetime.** Closing a view **detaches** it.
- **Closing the sizing view** either passes the sizing role to another attached view or keeps the last grid; the PTY is not resized while no view holds the role. Which of the two is Q4.
- **A session ends** in exactly two ways:
  - by an explicit session-close: the pane's close verb, *Restart shell*'s replacement, or the close of its tab or window and quit, all as today;
  - or when its last attached view detaches, if the owner rules so (Q5).
- **Nothing changes in 0.4.x.** There is only ever one view, so "the last view detaches" and "the pane closes" are the same event, and DESIGN §7.1.4's close rules read word for word.

These four points replace §2.3's last bullet. O-6 now depends on **Q2, Q4 and Q5**.

### (b)5 · Application-wide documents, and what they cost (N1-5)

Applied. "Per application" supersedes DESIGN §7.0's *one copy of one file per tab* (`DESIGN.md:1290`). The ticket that builds it states these consequences:

1. **Closing a tab whose document another view still shows loses nothing**, so it raises **no** dirty gate. The old per-tab gate (`GateRequest::ClosePane` and close-tab over the tab's pool) narrows to documents with no other attached view.
2. **The dirty gate moves to the boundaries where an edit can be lost:**
   - the **last view** of a dirty document detaching;
   - an **explicit document close** (a future verb);
   - **application quit**;
   - the controlled-failure road (A§11), which keeps its recovery copy.
3. **Two tabs edit one document through its one revision-checked door**, `edit(at_revision, change, author)`. Each view keeps its own caret, selection, scroll and parse (`PreviewPane`'s fields), healed from the edit log as `step_preview_history` already does within a tab.
4. **Restore, eviction, disk watching and file identity leave `TabState.preview_pool`:**
   - the session document's `tab.preview.pool` becomes a list of references into an application pool, with no schema change;
   - the eviction law counts views application-wide;
   - `preview_watch` subscribes per document, not per tab;
   - `PreviewSource` identity is checked once per application.

**O-9 is L**, and gets **its own design note and Codex round** before dispatch (RULES 55: an ownership change). These four consequences replace Q1's cost sentence.

### (b)6 · Tickets, re-sized and re-ordered (N1-6)

| # | ticket | size | after | notes |
|---|---|---|---|---|
| O-1 | J3 T-RUNTIME-DEP-MAP's three products (§3.1) | S/M | I1 | unchanged |
| O-2 | T-CONTRACT-M0-M1 | S | K2, O-1 | unchanged |
| O-3 | T-CONTRACT-M2-M4: the view fields; `propose_size(SizeProposal)`; the five `port_*` forwards; `report_keyboard_focus`; (b)1's pin | M | O-2 | the doors of (b)1 |
| O-4 | T-CONTRACT-M5-M7: `LaunchFacts`; **`ShellLife` per (b)2's table, with its four transition pins**; `AttentionAccount` | **L** | O-3 | was M |
| O-5 | T-CONTRACT-M8: `ShellSession` and `TerminalPane`; the map's new value shape through `transfer_tab`, `move_seat_content`, `pane_into_new_tab`, `absorb_tab_sessions`; `wake` marked as transport debt | **M** | O-4 | was S |
| O-6 | T-SESSION-REGISTRY (0.5): `SessionId`, the registry, views attaching and detaching, the sizing role, wake routing ((b)3) | L | O-5, **Q2, Q4, Q5** | its own Codex round |
| O-7 | T-DOCUMENT-PRIVATE | S | — | unchanged |
| O-8 | T-VIEW-NEEDS-DOCUMENT | M | O-7 | unchanged |
| O-9 | T-DOCUMENT-REGISTRY (0.5): (b)5's four consequences | **L** | Q1, O-6, **its own design note and Codex round** | was M |

### (b)7 · The owner questions, revised (still five)

1. **Should one file be one buffer per tab (today, DESIGN §7.0) or one per application?** **Recommend per application**, with (b)5's four consequences:
   - no dirty gate when another view still shows the document;
   - the gate at last-view detach, explicit close, quit and the failure road;
   - one revision-checked edit door, with a caret per view;
   - restore, eviction, watching and identity taken out of the tab.

   O-9 builds it, after its own note.
2. Unchanged: **should a dragged-out view share its selection until `bt-term` splits view state?** **Recommend yes, for 0.5.** The sharing runs through (b)1's named ports.
3. Unchanged: **should "unread" be the session's, with views reporting what they painted?** **Recommend yes.**
4. **When the sizing view closes while other views remain, does the sizing role pass to another view, or does the session keep its last grid?** **Recommend that the role passes to the most recently focused remaining view.** A shell sized for a pane nobody can see is the stale state DESIGN §7.53 already had to repair once, for a minimised window. Revision (a)'s Q4 (*a secondary view has no text size of its own*) stands as a consequence: whichever view holds the sizing role sizes the session, and the others scale.
5. **When does a session end?** **Recommend** both:
   - by its explicit close verbs, as today;
   - **and when its last attached view detaches**, so that closing a drag-out's source pane leaves the dragged-out view's shell running.

   This keeps A§4.1 (*a view disappearing is not a session ending*), keeps every 0.4.x close rule (with one view, the last view is the pane), and never leaves a session without a view to reach it. (Revision (a)'s recommendation, *only when the sizing view's seat closes*, is withdrawn per N1-4.)

### (b)8 · Architecture impact, revised

§9 stands, with two additions:
- `wake`'s owner becomes the binding (O-6), recorded in A§4.1's table by O-6.
- O-9 changes who owns the document pool (per tab → per application). It writes its A§4.4 row and a DESIGN entry that supersedes §7.0's per-tab clause.

---

## Revision (c), 2026-10-11, after the Codex confirmation round

Review: `reports/J5-05-contracts.review-codex-b.md`. Verdict on this note: **adopt with changes (blockers only)**. N1-1 and N1-3 through N1-6 are confirmed against the code. One blocker remains under N1-2, in three parts, and the review makes two observations under N1-1. All five are applied; none is argued. **Where (b) and (c) differ, (c) stands.**

### (c)0 · What changed

| # | the review's words (abridged) | disposition | where |
|---|---|---|---|
| (a) | *"The drain proof is guardable with today's `bt-source`: after `TermView` enters the census, assert that `drain_leaf_pty` has no access site for any `TermView` field … This does not require a call graph."* | applied | (c)1 replaces the guard sentence in (b)1 |
| (b) | *"`report_keyboard_focus` belongs on the session door: the window/view computes the truth, but the session consumes it to maintain DEC 1004 protocol state and emit PTY bytes."* | confirmed; kept, with the reason written in | (c)1 |
| (c) | *"The birth-death rule is specifically `elapsed <= BIRTH_GRACE && !had_output && has_fallback` → `FallingBack`; within the grace, either output or no fallback → `Kept`; beyond it → ordinary `Exited` … the `output_is_drained` gate in `reap_exited_tabs`"* | applied | (c)2 replaces (b)2's table |
| (d) | *"Retirement/close edges exist from every phase, not only the phases where the table mentions them."* | applied | (c)2's close column |
| (e) | *"The proposed pin 'successor lands into a live or exited slot and nowhere else' contradicts main. `restart_shell` checks only that no successor already exists … make successor birth/replacement genuinely orthogonal to the old phase. A kept-pane restart needs an explicit pin."* | applied: main's behaviour is the rule, and it is pinned. No product change is proposed. | (c)3 replaces (b)2's fourth pin |

### (c)1 · The drain proof, as a census assertion

Once O-2 adds `TermView` to the census `STRUCTS`, O-3 adds `the_drain_reads_no_view_field` to `bt-source`'s `census` test: **the census has no `write`, `access` or `inner` site of any `TermView.*` fact whose function is `drain_leaf_pty`**. This uses what the census already resolves (fields through receivers), and needs no call graph. MUTATION: `drain_leaf_pty` reads `leaf.view.projection` to decide whether to scroll to the bottom. That adds an access row, and the assertion fails. It replaces (b)1's runtime pin `the_drain_takes_focus_as_a_report_and_reads_no_view_field`.

**Why `report_keyboard_focus` is a session door** (the review confirms it). The view and the window compute the truth (WB§13.2's definition of the focused pane). The **session** consumes it: `DualPlaneSession::set_keyboard_focus` keeps the DEC 1004 focus-reporting state and queues the `CSI I` / `CSI O` bytes for the child (`main.rs:42605–42617`). So the door is the session's, and the drain's `holds_the_keyboard` parameter is its present spelling.

### (c)2 · The shell's life, exactly as main decides it

`reap_exited_tabs` (`runtime/tabs.rs:3585–3730`) and `birth_death_disposition` (`tabs.rs:42–52`; `BIRTH_GRACE` = 3 s, `tabs.rs:34`) give eight phases. *Close* means any of `close_pane`, `close_tab`, a window close, or the quit's retirement.

| phase | payload | entered when | leaves to | what a close does |
|---|---|---|---|---|
| `Deciding` | `PaneBirth { waiting: Programs \| Locale }` | `create_leaf_session` with rows or locale unanswered | `land_pane_birth` → `Bearing`, or → `Shellless` (`Started::Nothing`) | drops the `PaneBirth`; `PaneWaits` released |
| `Bearing` | `PaneBirth { waiting: Shell(ShellLanding) }` | decided at creation or at `land_pane_birth` | `land_shell_birth` → `Live`, or → `Shellless` (the birth failed) | drops `ShellBirth` → the number abandoned, and the answer retired off thread (`pty_door.rs:397`) |
| `Live` | `pty`, `wake` | a landing | `try_wait` answers → `ExitDraining` or `Exited` (below) | `bt_pty::retire_session(pty)` |
| `ExitDraining` | `pty` (exited), `exit_age` | `try_wait` answered, `elapsed <= BIRTH_GRACE`, and `!pty.output_is_drained()` (`tabs.rs:3613–3617`; the tab counts as live) | once drained, the disposition below | `retire_session(pty)` |
| `FallingBack` | `BirthDeathFallback { landing, exit }`, `exit_age`; old `pty` retired | drained, `elapsed <= BIRTH_GRACE && !had_output && has_fallback`, **and** `wake` present **and** `pty_door::request` accepted (`tabs.rs:3626–3661`) | `land_birth_death_fallback`: `FellBack` → `Live`; `Kept` → `Kept`; `Waiting` stays | drops the landing's `ShellBirth` → abandoned, as `Bearing` |
| `Kept` | `exit_age`; no `pty`, no `wake`; `said` set | drained, `elapsed <= BIRTH_GRACE` and any of: `had_output`, `!has_fallback`, no `wake`, or the fallback request refused (`tabs.rs:3663–3678`, `keep_birth_death_face`) | close only | nothing to retire |
| `Exited` | `pty` (exited, held), `exit_age`; `said` set once | `try_wait` answered with `elapsed > BIRTH_GRACE` (no output gate on this road) | the reaper itself: `close_pane` for the active tab's exited panes when the tab has other panes; `close_tab` when every pane of the tab has ended (`tab_has_ended`, `exited_tabs_the_loop_may_close`, `tabs.rs:3700–3728`) | `retire_session(pty)` |
| `Shellless` | nothing (the no-program banner; a `BT_PROBE_INPUT` pane) | a landing with `Started::Nothing` | close only | nothing |

Every close also expires the pane's attention (`expire_leaf_attention`).

Orthogonal to the phase, as in (b)2: `said`; `spawn_fallback` (taken once at a landing); and `successor` ((c)3).

O-4's table test `every_life_transition_is_one_method_and_no_other` drives exactly these edges, including the four guards of the `FallingBack`/`Kept` split and the `ExitDraining` hold, plus a close from each of the eight phases. Mutations, each going red:
- treat `had_output` as falling back;
- skip the `output_is_drained` hold;
- use `<` for `<=` at `BIRTH_GRACE`;
- leave a closed `Bearing` without abandoning its number.

`an_exited_shell_keeps_its_pty_until_the_tab_ends` stays. `the_exit_line_is_said_once_across_exited_falling_back_and_kept` stays.

### (c)3 · A restart is orthogonal to the phase, as main has it

`restart_shell` (`runtime/terminal.rs:640–720`) refuses only when a `successor` already exists. It reads the old record only for its profile, its place and its text size. A replacement that is in birth waits in `successor`. One that is not in birth replaces the record at once. `land_restart` (`runtime/panes.rs:171–206`) replaces the **whole record** whatever the old phase is. So a restart is available from **every** phase, including `Kept`, `Shellless` and a pane whose own birth is still pending. In that last case, the successor's landing drops the old `PaneBirth`, and its number is abandoned by `ShellBirth`'s drop.

**That is the contract.** No product change is proposed, so no ticket is needed. The pins replacing (b)2's fourth:
- `a_restart_replaces_the_record_from_every_phase` — a table over the eight phases. MUTATION: refuse a restart unless the phase is `Live` or `Exited` (revision (b)'s wrong pin, as a mutation).
- `a_kept_pane_restarts_into_a_live_shell` — the explicit pin the review asks for: a birth death kept with its face, *Restart shell*, the successor lands, and the pane is `Live`, with Chinese and mixed-script input typed after the landing reaching the new shell. MUTATION: `land_restart` keeps a `Kept` record.
- `a_second_restart_while_one_is_in_birth_is_ignored`. MUTATION: replace the existing successor.
- `a_restart_over_a_pending_birth_abandons_the_old_birth` (a fake birth door). MUTATION: let the old `PaneBirth` survive the replacement.

### (c)4 · Tickets and questions

The tickets are unchanged from (b)6. O-4 stays **L** and carries (c)2's table test and (c)3's four pins. O-3 carries (c)1's census assertion. The owner questions are unchanged from (b)7.

---

## Revision (d), 2026-10-11, after the scoped Codex look

Review: `reports/J5-05-contracts.review-codex-c.md`. Verdict on this note: **adopt with changes (blockers only)**, three factual corrections to (c)2's table. Everything else in scope is closed. All three are applied; none is argued. **Where (c) and (d) differ, (d) stands.**

### (d)0 · What changed

| # | the review's words | disposition | where |
|---|---|---|---|
| 1 | *"`FallingBack` still retains `wake` after an accepted fallback request. The table must list it or explicitly keep `wake` outside `Phase`; showing it only under `Live` is inaccurate."* | applied: `wake` is kept **outside** `Phase` and dropped from the `Live` row | (d)1 |
| 2 | *"`Shellless` is also created directly for no-program and `BT_PROBE_INPUT` panes, and reached after a failed `Bearing` birth — not only 'a landing with `Started::Nothing`.'"* | applied: the entry edges are corrected | (d)1 |
| 3 | *"'Every close also expires attention' is false. `close_pane` and `close_tab` call `expire_leaf_attention`, but window close and quit retirement only retire the shells in `let_go_of_this_window`."* | applied: the close column is split by road, and the gap is recorded as finding F-1 with a ticket, not written into the contract | (d)2 |

### (d)1 · `wake` and `Shellless`, corrected

**`wake` is not part of `Phase`.** Under (b)3 it belongs to the binding and is transport debt until O-6, so `ShellLife` does not hold it. In today's code the field is `Some` from the moment a shell is asked for until the pane is left without one. It stays `Some` through `Live`, `ExitDraining`, `Exited` and `FallingBack`: the accepted fallback request is handed `leaf.wake`'s output (`runtime/tabs.rs:3632–3645`), and the field is not cleared. `keep_birth_death_face` clears it (`tabs.rs:115`), so it is `None` in `Kept`. In O-4 that clear stays where it is, on the `Kept` transition. The field itself stays on the session record, beside `ShellLife` and outside it, until O-6 moves it to the binding. (c)2's `Live` row reads **payload: `pty`**, without `wake`.

**`Shellless` is entered three ways:**
1. **Directly at creation**, for a pane with no program on this machine (the no-program banner) and for a `BT_PROBE_INPUT` pane, which has no ConPTY behind it;
2. **from `Deciding`**, when `land_pane_birth` decides `Started::Nothing`;
3. **from `Bearing`**, when the shell's birth fails (`land_shell_birth`'s refusal: the pane is said over with no shell).

(c)2's `Shellless` row reads **entered when: (1)–(3) above**. The table test in O-4 drives all three entries.

### (d)2 · Closes, by road; finding F-1

The close column of (c)2 is right about **what each phase's payload needs** (drop the pane birth; abandon a pending birth's number; retire the PTY; nothing). It was wrong to say every close does the same around that. The roads differ:

| road | retires the shell (`retire_session` / `retire_all_shells`) | expires the pane's attention (`expire_leaf_attention`, `Event::LeafGone`) |
|---|---|---|
| `close_pane` (`runtime/panes.rs:1466` ff.) | yes | **yes** |
| `close_tab` | yes | **yes** |
| a window close and the quit's retirement, through `let_go_of_this_window` (`runtime/windows.rs:2055` ff.) | yes | **no** |
| `abandon_a_window_nothing_arrived_in` (`windows.rs:2030` ff.) | yes (`retire_all_shells`) | no |

**Finding F-1.** The window-ending roads retire the shells without emitting their panes' `LeafGone`. `expire_leaf_attention`'s own doc names only two doors, *"a pane closed on its own, and every pane of a tab closed together"* (`main.rs:28164–28168`), and no DESIGN entry says the window roads leave it out on purpose. So this note reads it as **an omission, not a design**.

What it costs, as far as this read establishes: the window's place counter (`attention_next_place`) dies with the window, so no place is leaked. But the attention trace (`emit_attention_lines`) gets no closing line for any episode still standing in a window that closed. Whether a desktop notification or a row in another window is left pointing at such a pane is **not established here**, and the ticket checks it first.

- **Ticket: T-ATTENTION-WINDOW-EXPIRY (S).** `let_go_of_this_window` and `abandon_a_window_nothing_arrived_in` expire each leaf's attention through `expire_leaf_attention` before retiring the shells. Every road that ends a pane then emits `LeafGone` once.
  - Pin: `every_road_that_ends_a_pane_emits_leaf_gone_once`, a table over the four roads, with a recording attention trace.
  - MUTATION: remove the call from `let_go_of_this_window`.
  - It can land independently of the contract tickets, before O-4 or with it. O-4's `ShellLife` close method is where the four roads meet afterwards.

**The contract, as it now reads.** Every road that ends a pane retires its shell **and** expires its attention exactly once. Today two of the four roads do the second; F-1's ticket makes it four. Until then, (c)2's sentence *"Every close also expires the pane's attention"* is withdrawn as a statement about today, and stays only as the rule the ticket enforces.

### (d)3 · Tickets and questions

T-ATTENTION-WINDOW-EXPIRY (S) is added beside the O-series. It has no dependency, and O-4 keeps its pin. All other tickets are as (b)6 and (c)4, and the owner questions are as (b)7.
