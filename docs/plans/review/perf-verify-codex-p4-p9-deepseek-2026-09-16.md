STATUS COMPLETE

# Independent verification of P-4 … P-9 (perf-review-2026-09-16.md)

Read-only. Re-opened every cited line at `2a261ebf`, plus one caller level up/down. No cargo, no launch, no tracked edit. Verdicts: all six CONFIRMED; none are misattributed or overstated. Each is a real per-event cost on its claimed path; several are conditional (empty files column, no frozen history, closed search, no open mark, no primary command rail) and the source states those conditions honestly.

---

## P-4 — pointer hit-testing rebuilds expanded files rows before rejecting the point — CONFIRMED

Verdict against `crates/bt-app/src/main.rs:87347-87349`, `:67845`, `:16666-16670`, `crates/bt-app/src/files.rs:563`, `:606-639`, `crates/bt-app/src/seats.rs:18068`.

**Claim vs code.** The claim is that the files fallback is evaluated eagerly as an argument, and the whole-tree walk runs before any geometry test. The chain is exactly that. `docked_chrome_target_at`'s last `.or_else` closure is:

```
.or_else(|| {
    seats::hit_files_tree(
        &self.seat_layout,
        &self.files_tree_contents(),   // main.rs:87349 — argument evaluated first
        ...
    )
})
```

`files_tree_contents` (`main.rs:67845`) → `files_tree_walk` (`main.rs:16661`), whose body is `for seat in self.seats.files() { … files::tree_view(&state, cache) }` (`main.rs:16666`, `:16670`). `tree_view` (`files.rs:563`) recurses via `walk_dir`, and per file entry clones twice: `key: child` (built by `format!("{parent}/{name})"`, `files.rs:667`) and `name: entry.name.clone()` (`files.rs:611`, `:623`). Only then does `hit_files_tree` (`seats.rs:18068`) iterate `layout.rects` with `if placement.kind != SeatKind::Files { continue; }` (`seats.rs:18078`) before any `row_at` test. The walk is pure cache: `DirCache::get` is `self.dirs.get(key)` (`files.rs:303`) and `canonical` reads the same map (`files.rs:456`) — no directory I/O. `DIR_ENTRY_CAP = 2000` (`files.rs:50`) caps one directory's entries, not the sum across open directories.

**Cost reality.** Mouse motion only; typing does not reach it. On a plain move both the free-body gate (`main.rs:86319`, `self.pointer_target_at(position).is_none()`) and `update_chrome_hover` (`main.rs:87440`, reached from `pointer_moved` at `main.rs:86686`) call `pointer_target_at`, so the walk runs twice per move whenever no earlier hit test answers. O(all expanded cached rows), synchronous CPU, no I/O, on the winit owner thread. Empty files state (`self.seats.files()` empty) makes it free.

**Fix.** The proposed fix — reject by seat/body rectangles first, then query only the hit seat — is the smallest correct one. `hit_files_tree` can test `SeatKind::Files` membership of the point before the tree is built, since the walk is only needed once a Files body is under the pointer. Nothing depends on the full map being built for non-hit seats.

**Missed.** The fix is not novel: `DirCache` already carries a `revision` damage counter whose doc comment states the exact cost — "re-walking a hundred thousand cached names to discover that nothing moved is the cost the whole budget exists to refuse" (`files.rs:267-283`) — but the hit-test path never reads it. Caching flattened rows by that revision is already the design's own vocabulary.

---

## P-5 — projection walks all unchanged history; cold width remeasures everything — CONFIRMED

Verdict against `crates/bt-viewport/src/lib.rs:4236-4255`, `:4300-4377`, `:4482-4488`, `crates/bt-term/src/session.rs:8207-8300`, `crates/bt-app/src/main.rs:100265`.

**Claim vs code.** `project` begins `let mut next_ids = Vec::new(); let mut next_entries = Vec::new();` and `for (id, entry) in document.entries()` (`lib.rs:4241-4244`), and `HistoryDocument::entries()` returns the whole `&BTreeMap<TranscriptId, HistoryEntry>` (`bt-doc/src/document.rs:44`). The unchanged detection is a full-prefix compare: `append_only = … && self.ordered_ids == next_ids[..self.ordered_ids.len()]` (`lib.rs:4253-4255`). The warm path does skip the per-line measure loop via `skip(start)` (`lib.rs:4300`), so the O(H) cost on an unchanged frame is the entry walk + two vector allocations + prefix compare, not re-measurement — the finding says this precisely. `relayout` (`lib.rs:4482-4488`) sets `projection_dirty` and calls `project` on a key change, and `sync_projection_state` then calls `projection.project(&self.document)` again (`session.rs:8300`), so a cold width/font/DPI key measures every line twice in one `refresh_projection` (`session.rs:8207-8208`). Artifact sync rebuilds whole maps: `frozen_artifacts` is collected fresh (`session.rs:8233-8252`) into `sync_math_artifacts`, which rebuilds `artifact_heights` (`lib.rs:3904-3921`). The other-pane caller is `leaf.session.refresh_projection(&mut leaf.projection)` per visible unfocused leaf (`main.rs:100265`). `TranscriptId(pub u64)` (`bt-transcript/src/lib.rs:378`) makes the 1.6 MB / 100k-line figure exact.

**Cost reality.** Typing/echo/scroll and every other visible pane's redraw pay the O(H) walk + allocation on the event thread. It is O(document) warm, O(total frozen text) on cold reflow, synchronous CPU, no I/O. The finding's own limit — the traced focused pane had zero frozen lines — is correct and prevents over-claiming.

**Fix.** Gating before enumeration by document/structure/layout/artifact revision, and incremental append/remove, is correct. The one subtlety the fix already names: the height index must never be partially updated where input/hit-testing reads it, so a background/incremental cold relayout must publish a consistent generation.

**Missed.** The `append_only` fast path already exists and is cheap per appended line; the residual cost is dominated by the two full vectors built before that check, which is exactly what a revision gate would avoid.

---

## P-6 — one visible row of a long wrapped line materializes the entire line — CONFIRMED

Verdict against `crates/bt-viewport/src/lib.rs:3604-3622`, `:2994-2995`, `:3085-3096`, `:5352-5455`, `crates/bt-transcript/src/lib.rs:11`, `:1416-1424`.

**Claim vs code.** Height lookup materializes the whole line just to count it: `history_row_heights` calls `layout_frozen_line(&entry.line, …, &[], self.vertical_reading()).len()` (`lib.rs:3616-3622`), and `vertical_reading` returns `None` when wrapping is on (`lib.rs:3593-3602`), so `layout_frozen_line` takes the full path. Visible materialization calls it again at `lib.rs:2994-2995`. The full path iterates `graphemes(&line.text)` and pushes a `CapturedCell` + `CellAnchor` (plus a spacer for wide glyphs) per grapheme, per wrapped row (`lib.rs:5383-5455`). All rows are validated before clipping: `for (local_row, row) in laid_out.iter().enumerate() { validate_visual_row(…) }` (`lib.rs:3085-3087`), then `.skip(local_start).take(visible_rows)` (`lib.rs:3091-3096`). The horizontal-window branch short-circuits to `window_flattened_line` before the grapheme loop (`lib.rs:5358-5369`), so this amplification is wrapping-only. `DEFAULT_STAGING_QUOTA = 4096` (`bt-transcript/src/lib.rs:11`) and `enforce_staging_quota` splits a continuing line (`bt-transcript/src/lib.rs:1416-1424`), bounding line length as the finding states.

**Cost reality.** Scroll into a long wrapped frozen line, or a width change intersecting it. O(whole logical line) cells/anchors per materialization, synchronous, event thread, no I/O. Not on the measured alt-screen (wrapping disabled there, and zero frozen lines).

**Fix.** Caching source-row counts for height lookup and wrap-start checkpoints for range materialization is the smallest correct fix. Restoring style/link state from checkpoints is the delicate part (wide-grapheme spacer and wrapped-link `continues` semantics live in the grapheme loop), which the fix names.

**Missed.** Nothing material on this path.

---

## P-7 — open search recompiles/rescans on publication; appends invalidate all history hits — CONFIRMED

Verdict against `crates/bt-app/src/main.rs:64738`, `:42204`, `:42256-42260`, `:42292-42328`, `crates/bt-app/src/search.rs:990`, `:855`, `crates/bt-transcript/src/search.rs:135-156`.

**Claim vs code.** `refresh_search(false)` runs on every publication (`main.rs:64738`). It compiles before any cache check: `search::engine(flags, query)` at `main.rs:42260` → `compile(&flags.query(text))` (`search.rs:990`) → `RegexBuilder::new(&pattern).build()` (`bt-transcript/src/search.rs:151-154`), with no memoization; the `search_scan` reuse check is later (`main.rs:42302-42309`). Live rows are captured and volatile text scanned before "unchanged" is known: `let live: Vec<search::LiveRow> = …` and `search::scan_volatile(…)` at `main.rs:42314-42322`, with the early return only at `main.rs:42329`. Cached history hits are cloned (`main.rs:42309`, then again `:42339`). The history key is `(frozen.len(), frozen.front()…, frozen.back()…, source_generation)` (`main.rs:42296-42301`), so one append changes `len()`/`back()` and forces `search::scan_history`, which walks all of `transcript.frozen()` (`search.rs:855-873`). Closed search returns at `main.rs:42204`; alternate screen is excluded by `seat_can_search` → `!leaf.session.terminal_modes().alternate_screen` (`main.rs:42026`).

**Cost reality.** Only when search is open on a primary-screen terminal. Per publication it does a regex compile + live-grid capture + volatile scan (O(visible+staging)) even when unchanged; on append/query change it adds an O(all frozen text) synchronous scan on the event thread.

**Fix.** Caching compilation by flags/query revision and checking content revisions before the live/volatile capture is smallest and correct; off-thread full scans against immutable revisions (with stale rejection) is the correct shape for the O(history) part. The field itself must stay synchronous for immediate feedback, which the fix states.

**Missed.** `scan_volatile` is deliberately "every time" by design (the comment at `search.rs:848-853` explains history is ~12 ms/100k lines and is scanned only when it moves) — the finding's claim is about the *compile and capture* that run before that split pays off, so the two do not conflict.

---

## P-8 — provenance marking searches old command marks on every key — CONFIRMED

Verdict against `crates/bt-app/src/main.rs:96769`, `:85950-85954`, `crates/bt-term/src/command_marks.rs:378-383`, `:442-445`, `:202`.

**Claim vs code.** `keyboard_input` calls `self.note_user_typing(self.focused_leaf)` before `send_user_input` (`main.rs:96769-96770`); `note_user_typing` → `leaf.session.note_user_input()` (`main.rs:85953`). `note_user_input` is `let Some(mark) = self.open_mark_mut()?; mark.typed_by_user = true;` (`command_marks.rs:378-383`), and `open_mark_mut` is `let open = self.open?; self.marks.iter_mut().find(|mark| mark.id == open)` (`command_marks.rs:442-445`). `marks` is a `Vec<CommandMark>` (`command_marks.rs:202`), so this is a linear scan from the front, and the open mark is normally the most recently pushed, i.e. last — the flag is then set to `true` with no `if !mark.typed_by_user` guard. No open mark returns at `self.open?` in O(1).

**Cost reality.** Per key/IME commit/paste while a command is open. O(retained marks) comparisons per event, synchronous, no I/O, event thread; the Keyboard timestamp is taken after this (`main.rs:85950` per the report), so recorded event→present would not reflect its removal. C is small in the traced TUI (three OSC 133 markers), as the finding states.

**Fix.** Keeping a stable open index (or the ledger invariant "open is last") is smallest and correct. The only correctness hazard is restored/program-generated input also calling this path, which the fix flags.

**Missed.** The in-code comment at `command_marks.rs:376-377` calls this "one branch and a store", which understates the actual `iter_mut().find` scan — the finding's core point is that the comment, not the cost, is what is wrong at small C; the cost only matters at large C.

---

## P-9 — command-rail hover rebuilds history before the cache check — CONFIRMED

Verdict against `crates/bt-app/src/main.rs:41672-41703`, `:41632`, `:41202-41214`, `:41234-41266`, `crates/bt-app/src/cmdrail.rs:1320-1351`, `:809-811`, `:955-971`, `:901-903`, `:258`.

**Claim vs code.** `settle_command_rail` reconstructs the stack and lays out before the cache is asked. It calls `let resolved = cmdrail::resolve(body, &self.command_rail_stack(seat), …)` (`main.rs:41689-41695`); `command_rail_stack` maps every mark into an `Entry` (`main.rs:41234-41266` → `cmdrail::commands`, `cmdrail.rs:955-971`), and `resolve` runs `lay_out` at least once (the folded rail, `cmdrail.rs:1331`) and up to `FISHEYE_RELAYOUT_CAP = 2` more times (`cmdrail.rs:258`, `:1339-1351`). Only after `settle_command_rail` returns does the caller check `if cache.needs_rebuild(key)` (`main.rs:41632`), and `needs_rebuild` is just `self.key != Some(key)` (`cmdrail.rs:809-811`). The band test that precedes the stack build uses the cached rail (`cache.rail()` at `main.rs:41679-41680`). Alternate screen disables the whole path: `command_rail_body` → `cmdrail::host_rect(body, alternate_screen)` returns `(!alternate_screen).then_some(body)` (`cmdrail.rs:901-903`), and `settle_command_rail` bails at `main.rs:41677`.

**Cost reality.** Motion inside a primary-screen command rail only; typing and terminal-body/alt-screen motion reject before the stack is built. O(all retained commands + search hits) `Entry`/`Tick` constructions per in-band move, synchronous, no I/O, event thread — paid even when `needs_rebuild` is false.

**Fix.** Caching the stack and folded layout by command/search revision + geometry is smallest and correct. Note `expanded` is an *output* of `resolve` and part of the key (`main.rs:41700`), so you cannot simply hoist `needs_rebuild` above `resolve`; the folded layout depends only on `body/stack/scale` (not on `y` or `expanded`) and is the cacheable unit, which is what the finding's fix proposes.

**Missed.** Nothing material. `FISHEYE_RELAYOUT_CAP` already bounds the relayout loop, and the finding records it as such.

---

## Ranked summary

1. **P-5 CONFIRMED** — warm frame still walks/allocates over the whole document, twice on cold reflow; real O(H) typing/redraw cost with any frozen history or second pane.
2. **P-7 CONFIRMED** — open search recompiles and re-captures every publication before its own cache check; O(history) on append.
3. **P-6 CONFIRMED** — a wrapped long line is fully materialized for a one-row height look and again for the visible slice.
4. **P-4 CONFIRMED** — files hit-test walks every expanded row twice per move before rejecting the point; O(expanded rows) on pointer motion.
5. **P-9 CONFIRMED** — command-rail hover relayouts the whole stack before `needs_rebuild`, per in-band move.
6. **P-8 CONFIRMED** — per-key linear scan of command marks to set an already-set flag; real but small at the observed C.

STATUS COMPLETE
