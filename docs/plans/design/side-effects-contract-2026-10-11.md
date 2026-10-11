# How a side effect is asked for and how it ends: the execution / completion contract — design note, 2026-10-11

Status: **draft, 2026-10-11**, for Codex review. Plan row J5 (T-05-CONTRACTS, 0.4.8), the second of the two contract notes the 2026-09-21 structure review asked for before 0.5. Ledger rows **D-16** (the door pattern, the contract's admission half) and **D-33** (the lane contract as one shape, its completion half), both 0.5 preconditions. D-55 (each publication declares its kind) also waits on this note. The companion is `ownership-contract-2026-10-11.md` (OC), which says *who* owns a fact. This note says how an owner asks the world to change something and how it learns the outcome. Read at `main` = `77399d57` on `design/05-contracts`. No product code, no test, no cargo run for this note. Facts are cited `file:line` at `77399d57` (paths under `crates/bt-app/src/` unless another crate is named).

**Section signs.** A bare `§n` is a section of this note. `A§n` is `docs/ARCHITECTURE.md`; `TD§n` is `docs/plans/design/thread-door-2026-09-26.md`; `WTB§n` is `docs/plans/design/window-thread-budget-2026-09-25.md`; `OC§n` is the companion note; `RULES n` is a row of `docs/RULES.md`. *The registry* is `crates/bt-app/src/window_waits.tsv`, and *row n* is its `# rows` section (= A§5.3). *The lane suite* is `crates/bt-app/src/lane.rs` with `lane_contract_tests`.

**The rulings this answers**

| date | ruling | where |
|---|---|---|
| RULES 52 | A side effect with a door has exactly one named entrance and a pin. A new side effect gets a door. | §1, §2 |
| RULES 53; A§5.1 | Logical lanes with return contracts. The window lane never waits for another lane. | §2 |
| A§5.4 step 1 | Before any move: define request identity, resource ordering, capacity, completion, abandonment and wake obligations; wrap existing implementations, do not re-derive their policy. | this note |
| A5 (2026-09-25) | Eight lane claims, held by a suite; a failing lane is a dated row of `lane::EXPECTED_FAILURES`, never declared conformant. | §1.3, §3 |
| B3/B4 (0.4.8) | No timer anywhere. A failure is *unknown*, never cached as a verdict. A probe is re-asked by the next request. | §2 C6 |
| A§4.2 | Five ownership classes. Supersession is solved by generations; durability, freshness, loss and admission are not. | §2 C5 |

---

## 0. In one page

- **The present** (§1). Twenty-three effect classes, each with its door, thread, completion signal and failure road (§1.2).
  - **Eleven conform** to the shape this note states. The reference instances are the PTY birth (`pty_door::ShellBirth`, `pty_door.rs:369`), the OS hand-off lane, the B4 probes and the preview read's `land_read`.
  - **Eight are partial.** They have a door but miss one obligation: a dead worker is unobservable, a superseded request gets no outcome, the completion is polled rather than woken, or the effect is still an owner-thread wait on the way to being moved.
  - **Four run on the window thread with no request door**: the document save and the store writes (registry row 20), the integration installs (rows 2–3) and the clipboard write (row 26).
  - The machinery to hold all of this exists in pieces: the thread door's `WorkerCtx` (TD§0), the registry's `# effects` section (owner-door and worker-door bodies), the `AppEvent::*` wakes (46 variants, `main.rs:457–854`), and the lane suite's eight claims. **What does not exist** is one ledger that names, for every effect class, how it ends.
- **The contract** (§2), seven rules:
  - **C1** A request door returns at once.
  - **C2** Every request ends in exactly one completion, delivered once and always (success, failure, cancellation, supersession).
  - **C3** A failure is a value and reaches `diagnostics.log` once.
  - **C4** A superseded request is still completed, and its completion says so.
  - **C5** No timer retries. A retry rides the next request or the next ear event.
  - **C6** An effect that outlives its requester completes into a named sink.
  - **C7** Owner-thread effects (the platform's affinity, A§5.2) are a declared exception. They are registered, bounded and metered, and their completion is their return value.
- **Three sites that violate it today** (§2.3), with the registry or lane rows that show them:
  - **V1** `Runtime::save_preview_on` writes the file on the window thread (row 20; C1).
  - **V2** The Windows clipboard write sleeps and retries on the window thread (row 26; C1, C5).
  - **V3** The font, taskbar and program-walk lanes give a superseded request no outcome, and a dead worker leaves no fault (`lane::EXPECTED_FAILURES` → D-72, D-73, D-85; C2, C4).
- **Migration and tests** (§3):
  - The ledger is a new `# completions` section of the registry, one row per class, held equal to the code.
  - A `bt-source` guard holds "a request door returns at once": no vocabulary effect in the body of an item named as a request door.
  - The lane suite's adapter grows a fake worker that completes in every order (success, failure, panic, supersession, requester gone, worker death). Each order must give exactly one completion.
  - Seven tickets, in order.
- **0.5** (§4): an agent's long-running command (one terminal completion, plus progress published as a latest value), a web page load (navigation id, supersession), and a save both the agent and the person asked for (one storage lane per document, completions joined by revision). All three are satisfied. The web engine's creation stays an owner-thread wait (row 21), which is the one gap.
- **Five owner questions** (§6), each with a recommendation.

---

## 1. The present

### 1.1 The pieces that already exist

- **Doors and threads.** Every `bt-app` and `bt-platform` thread comes from `spawn_at_priority`, which lends its body a `WorkerCtx` (RULES 52; TD§0). A worker-only door takes `&WorkerCtx`, so a call from the window thread does not compile. The registry's `# effects` section lists every function whose own body holds a waiting effect, with its authority (`WaitToken` for an admitted owner-thread wait, `WorkerCtx` for a worker body) and is held equal to the code by `the_effects_section_is_the_effect_functions_inside_the_doors` (`window_waits_tests.rs:6782`). `every_bare_site_is_a_row_and_every_row_a_site` (`:6986`) holds the bare window-thread sites to `# rows`.
- **Wakes.** A worker publishes, then sends one `AppEvent` (`main.rs:457`: `PtyOutput`, `FilesReady`, `PreviewReady`, `GitReady`, `MathReady`, `HandoffAnswered`, `ProgramsAnswered`, `WatchStewardAnswered`, `PsReadLineProbed`, `UpdateChecked`, `UpdateJobProgress`, `ClipboardPictureReady`, … 46 variants, `main.rs:457–854`, not all of them answers). The receiving turn drains what was published. *Publish before waking* is A§5.1's ingress rule.
- **The lane suite** (`lane.rs:1–26`, test-only today). It has eight claims (`lane.rs:218`):
  1. A full lane answers without waiting.
  2. Every request has its own identity.
  3. Order is as declared.
  4. An abandoned or older answer is not raised.
  5. Every request ends exactly once.
  6. The wake follows the publication.
  7. The answers held are bounded.
  8. A dead worker is observable.
  It runs over five lanes (`LaneName`: Handoff, Font, Taskbar, Computation, Programs), and twelve failures are dated rows of `EXPECTED_FAILURES` (`lane.rs:376`).
- **One-shot probes, B4's shape** (A§4.4, *the one-shot PowerShell answers*): last answer plus request number, one probe out, merged requests said once, an older answer refused, and a failure that is the newest answer (`Unknown`) and never a cached verdict. The next request asks again.
- **Owed and retried work today.**
  - The session backup re-dirties a failed write and tries again at the next quiet window (`persist.rs`, `hand_over` / `report_write`, `SESSION_DEBOUNCE` 1.5 s, `:388`).
  - A pane in birth owes its pseudoconsole the resize it missed (`ShellLanding::owed_physical`, `main.rs`) and its held input (`PaneBirth::deliver`).
  - The window title is a wanted value: *a refusal keeps the title wanted, and the next turn writes it* (`runtime/windows.rs:676`).
  - A trial holds durable writes back and releases them on `AppEvent::TrialWritesReleased` (A§4.4).
  - B6's owed settings edits have a brief (`trace/tickets-048/B6-settings-reread-design.md`) but no note in the repository yet. This note does not depend on it.

### 1.2 Every effect class

Legend: **✓** conforms to §2; **◐** has a door, misses one obligation (named); **✗** violates C1 (no request door: the effect runs on the window thread inside the gesture).

| # | effect class | door (entrance) | thread | completion signal | failure road | supersession / sink | status |
|---:|---|---|---|---|---|---|---|
| 1 | a pane's shell (process spawn) | `create_leaf_session` → `pty_door::spawn` → `bear(&WorkerCtx)` (`pty_door.rs:178`; registry `# effects` worker row) | `bt-pty-birth` | answer published under its generation; the pane's own wake (`PtyOutput`) → `Runtime::land_births` (`runtime/panes.rs:60`) | `Born` is a `Result`; a worker panic becomes `PtyError::Backend` (`pty_door.rs:340`); birth death → fallback shell, then the kept face | a pane closed first: `Drop for ShellBirth` marks the number abandoned and the answer is retired off the window thread (`:397`) | ✓ the reference |
| 2 | shell retirement | `bt_pty::retire_session` (`close_pane`, `runtime/panes.rs:1466`) | `pty-retirement` (bt-pty) | none per pane (nothing waits); quit's bounded `wait_for_retirements` (row 15, `PaneRetirementWait`) | counted at quit | the retirement thread is the sink | ✓ (D-81: `PtySession::drop` still waits when the thread cannot start) |
| 3 | input to a shell | `offer_pty_input` → `InputRing::try_push` | window thread, one lock (row 19, ruled to stay) | the push's `Result` | `PtyError::InputRefused` value | a pane in birth holds input on the ring's terms (`PaneBirth::hold`) | ✓ |
| 4 | a shell's resize | `commit_leaf_resize` → `pty_door::resize` (`WaitToken<PtyResize>`, `pty_door.rs:419`) | window thread | the return | error value | owed to a shell still in birth (`owed_physical`) | ◐ row 12, open (0.5→0.6: needs OC's session owner) |
| 5 | machine probes (PSReadLine, `$PROFILE`, Copilot, locale) | `bt_platform::spawn_probe` / `probe_output` (A§2.2) under one owner cell per fact | `psreadline-probe`, `powershell-profile-observation`, `copilot-version-probe`, `locale-reading` | `PsReadLineProbed`, `PowerShellProfileProbed`, `CopilotProbed`, `LocaleAnswered` | `Unknown` as the newest answer, never cached (B4) | one probe out; merged requests said once; older answer refused | ✓ |
| 6 | foreground program of a pane | `foreground_program::request` | `bt-foreground-program-worker` | an answer routed by window, tab, seat **and incarnation** | `Unknown` | one pending successor behind the probe out | ✓ |
| 7 | OS hand-off (open, reveal, URL) | `handoff_lane` → `ShellThread::hand_over` (`&WorkerCtx`) | `bt-os-handoff` | `HandoffAnswered`, by request id into the asking window's `Pending` | the door's own `Result` | bound 32, no coalescing; an answer for a gone window is raised nowhere | ◐ D-70 (a dead worker leaves ids owed), D-71 (answers unbounded) |
| 8 | directory watch start / stop | `watch_steward` (`WatchSteward::serve`, `# effects` worker row) | `watch-steward` | `WatchStewardAnswered` | the watch's `take_failure` | — | ✓ (TD-1/TD-2: the `DirWatch` drops' unbounded join, on that worker) |
| 9 | file, preview, git and index reads | the files, preview, git and index workers; reads charged to `bt_platform::file_reads` lanes | `bt-files-worker`, `bt-preview-worker`, `bt-git-worker` | `FilesReady`, `PreviewReady`, `GitReady`, `FileIndexReady` | worker disabled and said once (`disable_*_worker`, `*_worker_notice_pending`) | preview: `PreviewBuffer::land_read` → `ReadLanded::Kept` refuses a read the buffer has moved past (`preview.rs:6024`, `ReadLanded` `:6262`) | ✓ (preview); ◐ the others are not in the lane suite (D-33) |
| 10 | computation (formula, decode, scale, path verification) | `MathWorker`'s queues | `bt-math-worker`, `bt-image-scale-worker`, `bt-path-verify-worker` | `MathReady` | answers routed by owner and `LeafId`; stale and closed panes ignored | — | ◐ D-74 (unbounded), D-75 (no request identity), D-76 (one thread's death invisible) |
| 11 | the font list | the font lane (`settings::FontLane`) | `font-families` | `FontsScanned` | — | coalesced requests get no outcome; a dead walk leaves `ScanState::running` set | ◐→**V3** D-72 |
| 12 | taskbar auto-hide state | `taskbar_lane` | `taskbar-state` | `TaskbarAnswered` | — | requests between two served ones get no outcome | ◐→**V3** D-73 |
| 13 | what this machine can start | `programs_lane` | `program-walk` | `ProgramsAnswered` | `diagnostics.log` line at the walk's end | requests made during a walk are answered by the next walk and get no outcome of their own | ◐→**V3** D-85 |
| 14 | the session file | `SessionStore::flush_if_due` → `SessionWriter` (`persist.rs:564`) | `session-writer` | a receipt by generation, **polled** at each turn's flush (`take_receipts`) — no wake | `report_write`, one alert per streak; the document is re-dirtied and **retried at the next quiet window** | a stale receipt is judged and counted as landed | ◐ C5 (a timer retry) and no wake (§6 Q3) |
| 15 | a document's save | `Runtime::save_preview_on` (`runtime/preview.rs:7761`) → `PreviewBuffer::save` (`preview.rs:5778`) | **window thread** (`hang_watch::Station::PreviewSave`, no admission) | the return value → the pane's notice | `SaveOutcome::{Conflict, Failed}` → notice, `eprintln` | — | **✗ V1**, row 20 |
| 16 | store writes (settings, keybindings, profiles, pins) and renames | `persist.rs`'s store methods; `rename_preview_file`, `rename_files_row` | window thread | the return | alert/toast | — | ✗ row 20 (B6's storage transaction; D-47) |
| 17 | shell integration installs (`$PROFILE` marks, PSReadLine) | `Runtime::add_to_profile`, `apply_psreadline` | window thread for rows 2–3 (the marks lock: a wait, then `try_lock` and `sleep` up to 2 s); `powershell-profile-install` and its siblings for the enable road | — | — | — | ✗ rows 2–3, open |
| 18 | the clipboard's text | `write_terminal_clipboard_text` (`main.rs:68195`) → `bt_platform::set_clipboard_text` | **window thread** (`hang_watch::Station::ClipboardWrite`, no admission) | the return | `anyhow` error | `retry_open_clipboard` sleeps 5, 10, 20, 40 ms between five attempts (`crates/bt-platform/src/lib.rs:12412`) | **✗ V2**, row 26 |
| 19 | a clipboard picture saved | the `clipboard-picture` worker | `clipboard-picture` | `ClipboardPictureReady` | — | — | ✓ |
| 20 | the folder chooser | `FolderPicker::request` (posted; the modal dialog opens after the callback returns, `crates/bt-platform/src/lib.rs:12831`) | the window's message loop, outside winit's callback | collected the next turn (`apply_folder_pick_result`) | `Result` | — | ✓ |
| 21 | network: the update check, the update's download | `update::begin`; `update_job` | `bt-update-check`; the job's worker | `UpdateChecked`; `UpdateJobProgress` | a refusal stamps `attempted_at_ms`; the job's `Failed` state | one job per process, its offer immutable | ✓ (D-82: the WinHTTP request's `Drop` waits up to 5 s) |
| 22 | a web page coming up | `WebSeat::step` → `WebHost::request_controller` (`WaitToken<WebController>`) and `request_environment` | window thread (engine affinity) | the engine's callback → `WebPageSpoke` | — | — | ◐ row 21 open (D-64); C7 class |
| 23 | window title, cursor, visibility, IME caret area, compositor, present | `owner_door::*` and the `bt-render` entrances, each `WaitToken<door>` (registry `# doors`) | window thread | the return; title and caret area are wanted values, written once a frame | a refusal keeps the value wanted (`runtime/windows.rs:685–691`) | latest value | ✓ under C7 (rows 9, 14, 22 and §5.2) |

**Counts.** 11 ✓ (rows 1, 2, 3, 5, 6, 8, 19, 20, 21, 23, and 9 for the preview), 8 ◐ (4, 7, 10, 11, 12, 13, 14, 22), and 4 ✗ (15, 16, 17, 18). Row 9 counts once, in ✓: its non-preview half is the D-33 gap.

### 1.3 What the code already teaches

- **The birth is the model.** `ShellBirth` (`pty_door.rs:369–415`) has every property this note asks for:
  - a request numbered by the asker;
  - an answer that is always produced, with a panic converted to a value;
  - a wake that follows the publication;
  - an answer taken exactly once (`settled`);
  - a requester that may vanish: its drop marks the number abandoned, and whichever side arrives second retires the answer off the window thread.
  C6's sink is that last property, stated generally.
- **Supersession is solved where an owner checks a revision** (`land_read`, the session writer's generations, B4's request numbers). It is unsolved where a lane coalesces requests without keeping their identities (V3). A§4.2 already says generations solve this. This note adds that a coalesced request is **still completed**, as *superseded by n*.
- **The lane suite already states C2's half for workers.** It runs only in tests and only over five lanes. §3 makes the ledger cover every class and makes the suite's fake worker the shared harness.

---

## 2. The contract

### 2.1 The rules

- **C1 — A request door returns at once.**
  - A request door may validate, number the request, put it on a lane, and record what it owes. It may not wait on another thread, a process, the file system, the network or the GPU. The vocabulary is the registry's `# vocabulary`.
  - The gate is mechanical (§3.2). The window thread's census of bare sites cannot grow (`every_bare_site_is_a_row_and_every_row_a_site`). A request door is an item named in `# completions`, and its body holds no vocabulary entry and calls no `owner-door-body`.
- **C2 — Exactly one completion per request, always.**
  - Every admitted request ends in one terminal value delivered to its requester's sink. A wake carries `Done(result)`; otherwise the requester gets an `Ended(reason)`, where the reason is `Failed(error)`, `Cancelled`, `Superseded { by }` or `WorkerGone`.
  - It is delivered once: taking it consumes it, as `ShellBirth::take` does. It is delivered always: success, failure, cancellation, supersession, and the worker's death.
  - A completion may be *read* by polling (the session writer's receipts) only when a wake also announces it (§6 Q3).
- **C3 — A failure is a value, and `diagnostics.log` hears it once.**
  - No panic crosses a lane. The worker converts one to `Failed`, as `pty_door` does at `:340`.
  - The diagnostics line is written by the completion's consumer, once per request. A streak of identical failures is one line with a count, as `report_write` does.
- **C4 — A request may be superseded, and the later one wins.** The earlier request still completes, with `Superseded { by }`. Coalescing is a lane policy (A§4.2: *keep the differing operation policies*). Dropping the earlier request's completion is not a policy.
- **C5 — No timer retries.**
  - A failed effect is retried by the next request for it, or by the next ear event that bears on it: a turn that has a new value to write, an `AppEvent` that says the world changed, a trial's release.
  - A failure is never a cached verdict (B4).
  - A debounce that coalesces *requests* is allowed, because it schedules requests. Re-arming the debounce because a write *failed* is a timer retry (row 14, §6 Q3).
- **C6 — An effect that outlives its requester completes into a sink.**
  - A pane closed with its birth out, a window closed with a hand-off out, a document closed with a save out: the completion goes to the registry entry that outlives the requester. That is the abandoned set and the retirement worker for a birth, a window-less `Pending` drop for a hand-off, and the document's pool entry for a save.
  - The sink may discard a success silently. It writes a failure to `diagnostics.log` (C3).
- **C7 — Owner-thread effects are a declared class.**
  - Some effects must run on the window thread by platform rule (A§5.2): the title, cursor, visibility, IME caret area, compositor, presentation and the web engine's creation.
  - Each is a `WaitToken` door with a registry row, a bound and a meter. Its completion is its return value. A latest value is retried by the next turn that still wants it, which is C5's "next ear event".
  - This class is closed: a new member needs a row ruled by the owner.

### 2.2 What it forbids, with today's code as the example

| forbidden | example today |
|---|---|
| a request door that waits | V1, V2; rows 2–3, 12, 16b, 20, 21, 24–28 (each `open` or `pending` in the registry) |
| a request with no completion | V3 (D-72, D-73, D-85); D-70: a dead hand-off worker leaves accepted ids owed for ever |
| a completion delivered twice | none found. The lane suite's `EveryRequestEndsExactlyOnce` has no `Twice` failure in `EXPECTED_FAILURES` |
| a panic as a failure road | none on a lane. `pty_door` converts. `install_panic_log_hook` is the process's emergency road (A§11), not a completion |
| a retry on a timer | V2's backoff; row 2's marks-lock sleeps; the session writer's re-armed debounce (row 14 of §1.2) |
| a completion lost when the requester is gone | none for the birth; D-70 for the hand-off's dead worker |
| a wait in a `Drop` | D-78…D-82, the closed `Drop` inventory (A1e). They stand where they are, listed, and shrink only |

### 2.3 The three violating sites

- **V1 — a document's save runs on the window thread.**
  - The site: `Runtime::save_preview_on` (`runtime/preview.rs:7761`) calls `PreviewBuffer::save` (`preview.rs:5778`): the preserving atomic write, its `sync`, and the conflict check against the disk baseline. It runs inside `hang_watch::during(Station::PreviewSave, …)`, which meters but does not admit (no `WaitToken`, no `# doors` row).
  - The registry: row 20 is `open` (`window_waits.tsv:74`, *the preserving atomic preview save … storage lane, with document-revision preconditions and receipts*).
  - The census: the write appears only as an access lend, `TabState.preview_pool` → `Runtime::preview_buffer_on_mut` (`call:get_mut`), because `PreviewBuffer` is not one of its structs (OC§7). The annotation is `TabState.preview_pool  DUR  preview`.
  - It breaks C1. On a share or a synced folder the save is an unbounded `fsync` inside the keystroke. The quit's `save_dirty` and the failure road's `keep_dirty` make the same call (A§11: *the writes are made on the window thread and are done when the call returns*). Whether the exit phase may keep them is S-6's question. The same lane answers it, with the quit waiting for the document receipts across turns as it already waits for the session's (row 16).
- **V2 — the clipboard write sleeps and retries on the window thread.**
  - The site: `write_terminal_clipboard_text` (`main.rs:68195`) → `bt_platform::set_clipboard_text` → `retry_open_clipboard` (`crates/bt-platform/src/lib.rs:12412`), which sleeps 5, 10, 20 and 40 ms between five `OpenClipboard` attempts while another process holds the clipboard.
  - The registry: row 26 is `open` (`window_waits.tsv:80`, ruled interim).
  - It breaks C1 (up to 75 ms of sleep inside a copy gesture) and C5 (a backoff timer). It is also C3's shape gone wrong. The last attempt's error is an `anyhow` that `copy_selection` returns up through the gesture, so the outcome of the copy depends on how long another program held a lock.
- **V3 — superseded requests and dead workers end in silence.**
  - The sites: the font lane (`settings::FontLane`), `taskbar_lane` and `programs_lane`.
  - The lane rows: `lane::EXPECTED_FAILURES` (`lane.rs:376` ff.):
    - Font × `EveryRequestEndsExactlyOnce` → `NoTerminal` (D-72), and Font × `ADeadWorkerIsObservable` → `DeathUnobservable` (D-72: *a walk that dies leaves `ScanState::running` set for ever — no fault, no later walk*);
    - Taskbar × both (D-73);
    - Programs × `EveryRequestEndsExactlyOnce` (D-85: *the requests made while a walk is out are answered by the next walk and get no outcome of their own*).
  - It breaks C2 and C4. Coalescing is the right policy for all three, because a newer answer serves every waiting asker. What the contract adds is that each coalesced request is completed with `Superseded { by }` or with the serving walk's answer, and that a dead worker turns every owed request into `WorkerGone`.

---

## 3. The migration and the tests

### 3.1 The ledger: a `# completions` section in the registry

The registry gains a seventh section. Its header line and one row per effect class of §1.2:

```
# completions
class	door	thread	signal	failure	supersession	sink	status
```

- `door` is the item key of the request door (the `bt-source` item, as `file_reads_doors.txt` keys are).
- `signal` is an `AppEvent` variant, or `return` for C7.
- `status` is `conforms`, `owner-thread` (C7), or the D-row / registry row that repairs it.

The ledger holds the code in both directions:

- every `AppEvent` variant that answers a request names a row;
- every `lane::LaneName` names a row;
- every `# rows` entry that is `open` or `pending` names the class it belongs to;
- every `worker-door-body` in `# effects` is reached from a row's door.

A class whose status is a D-row may only change to `conforms`, the same shrink-only rule as `# rows`. `generate-window-waits-table.ps1` renders the section into A§5.

### 3.2 The source guard: a request door returns at once

`hang_watch::window_waits_tests::every_request_door_returns_at_once` takes each `door` of `# completions` that is not `owner-thread`, resolves its body through `bt-source`'s product view (item identity, as the `# effects` guard does), and fails on any of:

- a `# vocabulary` path;
- a call to an `owner-door-body`;
- a call to an item that is itself a request door's *worker* body (a `WorkerCtx` parameter cannot be satisfied on the window thread anyway; this catches a closure run inline).

**MUTATION:** put `std::fs::write` in `handoff_lane::submit`'s body. Then remove the guard's `owner-door-body` check and call `pty_door::resize` from a request door. Each must go red.

### 3.3 The completion harness: a fake worker that completes in every order

`lane.rs` grows a scripted worker, `lane::Scripted`, behind the existing `LaneUnderTest` adapter. It holds N = 3 admitted requests and plays every interleaving of six outcomes per request: `Ok`, `Err`, a panic in the body, superseded by a later request, the requester dropped before or after the answer, and the worker thread ending with requests owed. The claims, added to `Claim::ALL`:

- `EveryRequestEndsExactlyOnce` (exists): one terminal per request in every interleaving;
- `ASupersededRequestSaysSo` (new): a coalesced request's terminal is `Superseded { by }` and names a request that itself completed;
- `AFailureIsSaidOnce` (new): exactly one diagnostics line per failed request (through a counting seam over `diagnostics::note`), one per streak where the lane declares streaks;
- `AnOrphanedAnswerReachesItsSink` (new): with the requester dropped, the answer reaches the declared sink and no target.

Each lane gets the harness through its adapter. A lane that cannot pass a claim gets an `EXPECTED_FAILURES` row with its D-row, as today. **MUTATIONS** (each run once, then reverted):

- the scripted worker drops the superseded request's completion → `ASupersededRequestSaysSo` red;
- a cancel racing an answer delivers both → `EveryRequestEndsExactlyOnce` red with `Twice`, a new `FailureKind`;
- the panic arm re-raises → `AFailureIsSaidOnce` red;
- `ShellBirth`'s `Drop` forgets to mark the number abandoned → `AnOrphanedAnswerReachesItsSink` red.

No test is timing-bound. The interleavings are played by the script, not by sleeps (CONVENTIONS §三).

### 3.4 Tickets, in order

| # | ticket | size | after | what it closes |
|---|---|---|---|---|
| S-1 | T-COMPLETIONS-LEDGER: the `# completions` section, its four held equalities, the A§5 table | S | — | the ledger (§3.1); no behaviour change |
| S-2 | T-REQUEST-DOOR-GUARD: §3.2's guard and its two canaries in `gates-can-fail` | S | S-1 | C1 as a gate |
| S-3 | T-COMPLETION-HARNESS: `lane::Scripted`, the three new claims, `Twice` | M | S-1 | C2–C4, C6 as tests; new `EXPECTED_FAILURES` rows for whatever fails |
| S-4 | T-LANES-SAY-SUPERSEDED: the font, taskbar and program-walk lanes complete every coalesced request and fault on a dead worker | M | S-3 | **V3**; D-72, D-73, D-85 |
| S-5 | T-HANDOFF-DEATH: the hand-off lane's owed ids end `WorkerGone`; answers bounded | S | S-3 | D-70, D-71 |
| S-6 | T-DOCUMENT-SAVE-LANE: a document's save as a request on a storage lane, `save(revision)` → `Saved { revision }` / `Conflict` / `Failed`; the dirty flag clears only on the receipt for the current revision; the close gate waits for an owed receipt across turns (§6 Q5); B6's settings stores join the same lane when B6 is built | M | S-3, OC O-7 | **V1**; row 20's save half; D-47's part |
| S-7 | T-CLIPBOARD-LANE: the clipboard text write on a worker with its own message-only window; one contention wait declared in its door (§6 Q2); completion `ClipboardWritten(result)` drives the copy's acknowledgement | S/M | S-3 | **V2**; row 26 |

After these: the session writer's wake and its failure retry (§6 Q3, one line in S-1's ledger until ruled); rows 2–3 (the marks lock: the storage lane of S-6); row 12 (the resize behind OC's session owner, 0.5→0.6); the computation lane (D-74…D-76).

---

## 4. What 0.5 needs

- **An agent's long-running command, as a side effect with progress.**
  - Starting it is a request on the session, a command written through the input door, which is row 3's shape. Its completion is one terminal value: `Done { exit }` from the shell's `OSC 133;D`, `Ended(Cancelled)` only when the person ended it, or `Ended(WorkerGone)` when the shell died. This is the A2A mapping the workbench ruled (WB, *`canceled` only when the user ended it*).
  - Progress is a separate publication of kind *latest value* (A§4.2, *projections*). The consumer may not promote it into a completion.
  - A pane closed with the command running completes into the session registry's sink (OC O-6) with `WorkerGone`, and the agent rail hears it once.
  - Satisfied by C2, C3, C6 and the projection rule.
- **A web page load.**
  - A navigation is a request with an id. A later navigation supersedes it, and the earlier one completes as `Superseded`.
  - The engine reports a cancelled navigation as a completed one that did not succeed. Whether WebView2 and WKWebView each report every superseded navigation exactly once is **experiment E-1**, owed before S-3's web adapter.
  - Satisfied, except that the engine's creation stays a C7 owner-thread wait (row 21, D-64).
- **A save the agent and the person both asked for.**
  - Both are `save(revision)` requests on the document's one storage lane, serialised per document (A§5.1's storage rule: *serialize by affected resource; retain operation identity*).
  - A second request at the same revision joins the first: it completes with the same receipt, as `Superseded { by: first }` carrying the first's outcome. A request at a later revision runs after it.
  - Each asker gets its own completion. The dirty flag follows only the receipt for the document's current revision. The disk conflict check (`SaveOutcome::Conflict`) is unchanged.
  - Satisfied by C2, C4 and OC's document doors.

---

## 5. Experiments owed

- **E-1** (before S-3's web adapter): on WebView2 and WKWebView, start navigation A, then B before A commits. Record whether A gets exactly one completion and how it is marked. Same for a navigation cancelled by closing the page.

## 6. Questions for the owner (recommendation first)

1. **Should owner-thread effects (title, cursor, IME caret area, compositor, presentation, the web engine's creation) be a closed, declared class whose completion is their return value, instead of being moved?** **Recommend yes** (C7). The platform requires the window thread for them. They are already registered, bounded and metered. New members need a ruling.
2. **May the clipboard's contended `OpenClipboard` wait inside its door, on a worker, bounded and declared?** **Recommend yes, on a worker only.** A copy that fails because another program held the clipboard for 5 ms is worse than a short wait no one sees. The wait is declared in the door's row, and the window thread never sleeps.
3. **Should a failed session backup be retried by the next change (or by quit's judged write) rather than by the debounce clock?** **Recommend yes**, which is C5's letter. The previous good copy and the launch backup stay on disk, and quit's write is judged, so nothing is lost that a timer would have saved. The receipt also gains a wake. A failure stays said once per streak.
4. **Should superseded completions ever reach the person?** **Recommend no.** They go to the requester's code (to settle its bookkeeping) and, only if they failed, to `diagnostics.log`. Nothing is drawn for a request that a newer one answered.
5. **Should the close gate wait for a save in flight?** **Recommend waiting, without asking.** The gate waits for the owed receipt across turns (a clock on the turn, as quit's update receipt does with `UPDATE_RECEIPT_DEADLINE`, never a wait on the thread). It asks only when the receipt says `Failed` or `Conflict`. A person who pressed `Ctrl+S` and then closed should not see a question about what they just saved.

## 7. Sources

`crates/bt-app/src/window_waits.tsv` (sections `# rows`, `# doors`, `# vocabulary`, `# effects`); `crates/bt-app/src/window_waits_tests.rs` (`:6782`, `:6986`); `crates/bt-app/src/lane.rs` (`:1–26`, `Claim` `:218`, `EXPECTED_FAILURES` `:376`); `crates/bt-app/src/pty_door.rs` (`bear` `:178`, the worker's panic arm `:340`, `ShellBirth` `:369–415`, `resize` `:419`); `crates/bt-app/src/persist.rs` (`SessionWriter` `:564`, `flush_if_due`, `hand_over`, `take_receipts`); `crates/bt-app/src/runtime/preview.rs:7761`; `crates/bt-app/src/preview.rs:5778`; `crates/bt-app/src/main.rs` (`AppEvent` `:457`, `write_terminal_clipboard_text` `:68195`); `crates/bt-platform/src/lib.rs` (`retry_open_clipboard` `:12412`, `FolderPicker` `:12831`); `crates/bt-app/src/runtime/windows.rs:676` (`flush_title`); `docs/ARCHITECTURE.md` §2.2, §4.2, §4.4, §5.1–§5.4, §6, §11; `docs/RULES.md` 52, 53, 55; `docs/plans/structural-debt.md` D-16, D-33, D-47, D-55, D-64, D-70…D-76, D-78…D-82, D-85; `docs/plans/design/thread-door-2026-09-26.md` §0; `docs/plans/design/agent-workbench-0.5-2026-09-20.md`.

## 8. Architecture impact

- **Owned facts.** None moves. Each lane's owed-request bookkeeping gains a terminal state (`Superseded`, `WorkerGone`) in S-4 and S-5. Those are facts of the lane that already owns the requests.
- **Doors.** S-6 adds a storage lane door for document saves, and S-7 a clipboard worker door. Each is a new `# completions` row and a `# effects` worker row. Rows 20 (its save half) and 26 leave `# rows` when they land. No door is added by S-1…S-3.
- **Debt.** Repays D-72, D-73, D-85 (S-4), D-70, D-71 (S-5), and row 20's save half and row 26 (S-6, S-7). Narrows D-33 (the ledger covers every class) and D-16 (the admission half, by S-2). This note adds no debt row. The ledger's statuses are the list of what remains.
