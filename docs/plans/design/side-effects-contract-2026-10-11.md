# How a side effect is asked for and how it ends: the execution / completion contract — design note, 2026-10-11

Status: **revision (c), 2026-10-11**, after the Codex confirmation round (`reports/J5-05-contracts.review-codex-b.md`); revision (c) stands over (b) where they differ, and (b) over (a). Before that, **revision (b)**, after the Codex review (`reports/J5-05-contracts.review-codex.md`, both notes *adopt with changes*). Revision (b) is the dated section at the end. **Where revision (a)'s text and revision (b) differ, revision (b) stands.** Revision (a) was the draft for review. Plan row J5 (T-05-CONTRACTS, 0.4.8), the second of the two contract notes the 2026-09-21 structure review asked for before 0.5. Ledger rows **D-16** (the door pattern, the contract's admission half) and **D-33** (the lane contract as one shape, its completion half), both 0.5 preconditions. D-55 (each publication declares its kind) also waits on this note. The companion is `ownership-contract-2026-10-11.md` (OC), which says *who* owns a fact. This note says how an owner asks the world to change something and how it learns the outcome. Read at `main` = `77399d57` on `design/05-contracts`. No product code, no test, no cargo run for this note. Facts are cited `file:line` at `77399d57` (paths under `crates/bt-app/src/` unless another crate is named).

Owner rulings 2026-10-11 (afternoon, "全按推荐" — every recommendation stands as the ruling; `trace/tickets-048/00-PLAN-048.md`, Owner rulings 2026-10-11 afternoon): Q1 owner-thread effects are a closed declared class; present, PTY resize and the first GPU open stay admitted until their lanes land; Q2 the Windows clipboard worker may hold a declared bounded OpenClipboard wait, macOS stays on the owner thread until experiment E-2 (J5); Q3 a failed session backup is retried only by the next change, ear event or quit; Q4 superseded completions never reach the person; Q5(a) close/quit waits as stated; Q5(b) the preview's link policy is Refuse, S-6a keeps the plain writer until built. Tickets S-1…S-7 may be opened.

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

---

## Revision (b), 2026-10-11, after the Codex review

Review: `reports/J5-05-contracts.review-codex.md` (trace tree, 0.4.8). Verdict on this note: **adopt with changes**, ten items, N2-1 to N2-10. All ten are applied, and none is argued. The review's findings that stand unchanged:
- V1, V2 and V3 are real.
- Dirty clears only on a `Saved { revision }` receipt that matches the document's current revision.
- Waiting across turns, never on the window thread, is the right shape for close and quit.
- Q1–Q5 are the right questions; Q2, Q3 and Q5 are revised below.

**The B6 note** (`docs/plans/design/settings-reread-2026-10-11.md`, revision (a), on `design/settings-reread`, itself under review) is now read. (b)8 makes S-6's lane B6's storage worker.

### (b)0 · What changed, item by item

| item | the review's words (abridged) | disposition | where |
|---|---|---|---|
| N2-1 | *"The 23-row table is neither complete nor normalized enough to generate `# completions` … Derive the table at request-protocol granularity from the registry, `AppEvent` answers, every lane, and all open/pending wait rows. Recompute the 11/8/4 counts"* | applied | (b)1 replaces §1.2 and its counts; §0's counts read as (b)1's |
| N2-2 | *"'Returns at once' should mean 'performs no synchronous external effect and waits on no lock, thread, process, filesystem, network, GPU, or owner door' … Make the guard walk the resolved call graph to the enqueue boundary, fail closed on unresolved dynamic calls"* | applied | (b)2 replaces C1 and §3.2 |
| N2-3 | *"a door invocation either refuses synchronously with its terminal outcome or mints an identity … Row 2's shell retirement … cannot be called conformant without either defining it as detached maintenance … Apply the same distinction to coalesced probe triggers"* | applied | (b)3 adds to C2 |
| N2-4 | *"Same-revision save requests are joined, not superseded … Introduce `Joined { with }` … Reserve `Superseded { by }` for a request displaced by a newer operation"* | applied | (b)4 amends C2's terminal set, C4 and §4's third case |
| N2-5 | *"Rewrite C5 as: no new request is autonomously scheduled after a terminal failure; debounce may schedule a request from new input, and one admitted worker request may contain a declared bounded contention acquisition … Q3 must acknowledge"* the crash-loss trade-off | applied | (b)5 replaces C5 and Q3 |
| N2-6 | *"C7 needs an exact closed list and a meaning for 'bounded' … Reconcile 'presentation stays on the owner thread' with registry row 9's existing 0.5 presentation-lane disposition"* | applied | (b)6 replaces C7 and Q1 |
| N2-7 | *"The scripted-worker harness is not buildable through the present adapters as described … Specify the product seam … Separate caught task panic from worker-thread death. Either reduce S-3 … or size the full injection refactor as L"* | applied: the reduced S-3 plus a named seam that each lane adopts in its repair ticket | (b)7 replaces §3.3 |
| N2-8 | *"S-6 must preserve the B6 conditional-write protocol and define close/quit timeout outcomes … A close timeout must become `Failed`/`WorkerGone` and leave the gate open … S-6 is L or should be split"* | applied: split into S-6a/b/c | (b)8 replaces S-6 and Q5 |
| N2-9 | *"S-7 does not retire registry row 26 as written … Move every Windows `OpenClipboard` road or say the row is merely narrowed. Also scope the message-only-window solution to Windows … Size S-7 as M"* | applied | (b)9 replaces S-7 and Q2 |
| N2-10 | *"S-1 is M … S-2 is M … S-3 is L if it injects … S-6 is L or split; S-7 is M. The order ledger → guard/harness → lane repairs remains right"* | applied | (b)10 replaces §3.4 |

### (b)1 · The effect inventory at request-protocol granularity (N2-1)

**How it was derived.** One row per *request protocol*: one door, one thread, one completion signal. The rows were taken from:
- every `# rows` entry of the registry that is `open`, `pending` or `ruled to stay`;
- every `# doors` entry;
- every worker-door body in `# effects`;
- every `AppEvent` variant that answers a request (`main.rs:457–854`);
- every `lane::LaneName`;
- the thread door's named spawns.

**Kept out on purpose**, and recorded as excluded:
- ingress that no Folio code asked for: IME composition, endpoint listening and admission, watch and OS news delivery (`SystemNews`, `StorageChanged`, `SchemesChanged`, `AttentionSpoke`, `LaunchAsked`);
- the door processes (`--update-apply`, `--update-recover`, `--uninstall*`, `--explorer-command`), which run in another process and whose contract is the update journal (A§4.4).

**Status codes:**
- **✓** conforms, including §2's C7 members and the exit-phase waits ruled to stay;
- **◐** has a door or an admission but misses one obligation, or is an admitted interim wait due to move;
- **✗** a window-thread effect with no admission (a bare registry row);
- **?** not yet classified. S-1 reads it before the ledger is written.

| # | protocol | door | thread | signal | failure road | identity · supersession · sink | status |
|---:|---|---|---|---|---|---|---|
| | **Session transport** | | | | | | |
| 1 | a pane's shell birth | `pty_door::spawn` → `bear(&WorkerCtx)` | `bt-pty-birth` | pane wake `PtyOutput` → `land_births` | `Born::Err`; a panic becomes `PtyError::Backend` | generation; abandoned → retired off thread | ✓ |
| 2 | birth-death fallback shell | `pty_door::request` from `reap_exited_tabs` | `bt-pty-birth` | as 1 → `land_birth_death_fallback` | `BirthDeathLanding::Kept` + line | generation; as 1 | ✓ |
| 3 | shell retirement | `bt_pty::retire_session` | `pty-retirement` | none: **detached maintenance** ((b)3) | the retirement thread's own line | the sink accepts at once | ✓ under (b)3 |
| 4 | input to a shell | `offer_pty_input` → `InputRing::try_push` | window (row 19, ruled) | synchronous `Accepted` / `InputRefused` | the refusal is the terminal | refused synchronously or taken | ✓ |
| 5 | a shell's resize | `pty_door::resize` (`PtyResize`) | window (row 12) | the return | error value | owed while in birth | ◐ interim, 0.5→0.6 |
| 6 | quit's retirement wait | `pty_door::wait_for_retirements` | window, exit (row 15) | the count still going | proceeds at deadline | — | ✓ exit, deadline |
| | **Observation** | | | | | | |
| 7 | PSReadLine version and policy | `PsReadLineProbe` / `spawn_probe` | `psreadline-probe` | `PsReadLineProbed` | `Unknown`, never cached | request number; triggers merge ((b)3) | ✓ |
| 8 | `$PROFILE` observation | probe | `powershell-profile-observation` | `PowerShellProfileProbed` | `Unknown` | as 7 | ✓ |
| 9 | Copilot version | `CopilotProbe` | `copilot-version-probe` | `CopilotProbed` | `Unknown` | as 7 | ✓ |
| 10 | locale reading | `LocaleCell` | `locale-reading` | `LocaleAnswered` | failed reading → asked again by a birth | newest reading wins | ✓ |
| 11 | foreground program | `foreground_program::request` | `bt-foreground-program-worker` | answer by address + incarnation | `Unknown` | one pending successor | ✓ |
| 12 | program walk | `programs_lane` | `program-walk` | `ProgramsAnswered` | line at walk end | coalesced requests get no terminal | ◐ **V3** D-85 |
| 13 | font list | `settings::FontLane` | `font-families` | `FontsScanned` | — | coalesced: no terminal; a dead walk stays `running` | ◐ **V3** D-72 |
| 14 | taskbar auto-hide | `taskbar_lane` | `taskbar-state` | `TaskbarAnswered` | — | skipped requests: no terminal | ◐ **V3** D-73 |
| 15 | install channel | `install_channel` (once per process) | `bt-install-channel` | `InstallChannelRead` | `Channel::Unknown` | one request | ✓ |
| 16 | recovered edits listing | `recovered` | `bt-recovered` | `RecoveredEditsListed` | — | one request | ✓ |
| 17 | Explorer package probe, deploy, removal | `explorer_menu::request` (`BUSY` latch, one queued press) | `folio-explorer-*` | `ExplorerPackageChanged` | the row's refusal line | a press during `BUSY` is queued and only the last is run; earlier presses get no terminal | ◐ C4 |
| 18 | PSReadLine installed copy | `refresh_psreadline_installed` | window (row 4) | the return | — | — | ✗ |
| | **Reads and computation** | | | | | | |
| 19 | files column listing | files worker | `bt-files-worker` | `FilesReady` | worker disabled, said once | not in the lane suite | ◐ D-33 |
| 20 | preview read | preview worker | `bt-preview-worker` | `PreviewReady` | `HeadOutcome` error → the strip | `land_read` refuses a read the buffer moved past | ✓ |
| 21 | git questions (status, refs, log, show, search) | `GitWorker` | `bt-git-worker` | `GitReady` | `GitAnswer` fault per question (`git.rs:2505–2525`) | not in the lane suite | ◐ D-33 |
| 22 | **git mutations** (`Write`, `Checkout`) | the same `GitWorker` (`GitQuestion::Write`, `::Checkout`, `git.rs:2802`) | `bt-git-worker` | `GitReady` (`GitAnswer::Write`, `::Checkout`) | fault answer | a storage-class effect on a read worker: no per-repository serialisation or operation identity is stated | ◐ storage rules (A§5.1) unstated |
| 23 | file index | index worker | (index thread) | `FileIndexReady` | — | — | ◐ D-33 |
| 24 | formula and decoration | `MathWorker` | `bt-math-worker` | `MathReady` | stale and closed panes ignored | no request identity | ◐ D-74/75/76 |
| 25 | image scale | `MathWorker`'s scale queue | `bt-image-scale-worker` | `MathReady` | — | as 24 | ◐ D-76 |
| 26 | path verification | `MathWorker`'s path queue | `bt-path-verify-worker` | `MathReady` | — | as 24 | ◐ D-76 |
| 27 | background picture decode | `background-picture` worker | `background-picture` | `BackgroundPictureReady` | — | — | ? |
| 28 | folder chooser | `FolderPicker::request` (posted) | window's own loop, outside the callback | collected next turn | `Result` | one dialog | ✓ |
| | **Hand-offs and notifications** | | | | | | |
| 29 | OS hand-off | `handoff_lane::submit` | `bt-os-handoff` | `HandoffAnswered` | `Result` | request id; gone window → nowhere | ◐ D-70, D-71 |
| 30 | desktop notification | `notifications.show` (`runtime/attention.rs:789`) | window | the return | `refusal` kept | — | ? (no registry row; S-1 measures whether it waits) |
| | **Storage** | | | | | | |
| 31 | the session file | `SessionStore::flush_if_due` → `SessionWriter` | `session-writer` | receipt by generation, polled | re-dirtied, retried at the next quiet window | stale receipts judged | ◐ C5, no wake ((b)5) |
| 32 | quit's session write wait | `SessionWriter::wait_for` (`SessionWriteWait`) | window, exit (row 16) | the verdict | timeout → `stalled`, quit proceeds | — | ✓ exit, deadline |
| 33 | session writer close | `SessionWriter::close` (`SessionWriterRetire`) | window, exit (row 16b) | the return | — | — | ◐ pending |
| 34 | trial receipt write | `SessionStore::write_receipt` | `session-writer` | `ReceiptWritten` on its channel | `Err` | one per trial | ✓ |
| 35 | update hand-off to the applier | `update_handoff` at `QuitStep::Exit` | `session-writer` | job report | `Failed` | one job | ✓ |
| 36 | **document save** | `save_preview_on` → `PreviewBuffer::save` | window (row 20) | the return | `SaveOutcome::{Conflict, Failed}` | — | ✗ **V1** |
| 37 | renames (preview file, files row) | `rename_preview_file`, `rename_files_row` | window (row 20) | the return | toast | — | ✗ |
| 38 | settings, keybindings, profiles, pins writes | the stores' `write_now` | window (row 20) | the return | `DocumentWrites` streak | — | ✗ (B6 moves them, (b)8) |
| 39 | `$PROFILE` marks install | `add_to_profile` → `install_recorded` | window (row 2) | the return | — | lock + `sleep` up to 2 s | ✗ |
| 40 | PSReadLine apply | `apply_psreadline` | window (row 3) | the return | — | — | ✗ |
| 41 | shell-integration enable, undo, removal | `profile_runtime` jobs | `powershell-profile-install`, `-undo`, `-removal` | `PowerShellProfileProbed` | the row's refusal | — | ? |
| | **Clipboard** | | | | | | |
| 42 | clipboard text write | `write_terminal_clipboard_text` → `set_clipboard_text` | window (row 26) | the return | `anyhow` | sleep-and-retry inside | ✗ **V2** |
| 43 | clipboard text read (paste) | `clipboard_text` | window (row 26) | the return | `Err` | as 42 | ✗ |
| 44 | clipboard picture: open | `WindowsClipboard::begin` | window (row 26) | the return | `Err` | as 42 | ✗ |
| 45 | clipboard picture: save | `clipboard-picture` worker | `clipboard-picture` | `ClipboardPictureReady` | — | — | ✓ |
| | **Network and update** | | | | | | |
| 46 | update check | `update::begin` | `bt-update-check` | `UpdateChecked` | a refusal stamps `attempted_at_ms` | one check out | ✓ |
| 47 | update job (download, verify, stage) | `update_job::Job` | the job's worker | `UpdateJobProgress` | `Failed` state | one job per process | ✓ (D-82: the request's `Drop` waits) |
| 48 | trial watch | `update_trial` watch | `folio-trial-watch` | `TrialWritesReleased` | the transaction is handed back | one per trial | ✓ |
| 49 | trial claim | `update_trial::take_the_claim_within` | main, before the loop (row 28) | the return | — | 100 ms tries | ✗ pre-loop |
| 50 | update leave guard | `update_handoff::leave_armed` (`UpdateLeave`) | main, after the loop (row 29) | the answer | `LEAVE_WITHIN` deadline | — | ✓ exit, deadline |
| | **Web** | | | | | | |
| 51 | web environment, controller, rehost | `WebHost::request_environment` / `request_controller` / `rehost` (`WebEnvironment`, `WebController`, `WebRehost`) | window (row 21) | engine callback (`Controller { generation }`) → `WebPageSpoke` | `error` in the callback | generation | ◐ C7 member with an open cost (D-64) |
| 52 | **web navigation** | `WebHost::navigate` (`webview.rs:3029`) | window; returns at once | `NavigationStarting` / `NavigationCompleted { uri, success, status, http_status }` → `WebPageSpoke` | `success: false` | **no request id**: matched by uri; supersession unstated (E-1) | ◐ C2, C4 |
| | **Presentation and GPU** | | | | | | |
| 53 | present | `PresentFrame`, `CompositorCommit` | window (row 9) | the return | device loss → row 54 | frame identity | ◐ interim C7 (0.5 lane, (b)6) |
| 54 | device recovery | `recovered_from_a_lost_device` | window (row 10) | the return | three attempts with 150/450 ms sleeps | — | ✗ |
| 55 | first GPU open | `gpu_door::open_first_window` (`GpuOpen`) | window (row 23) | the return | — | once per process | ◐ pending |
| 56 | surface and compositor birth, window size | `SurfaceBirth`, `CompositorBirth`, `CompositorWindowSize` | window | the return | — | per window, per resize | ✓ C7 |
| 57 | title, cursor, visibility, focus, IME caret area | `TitleFlush`, `SetCursor`, `SetVisible`, `FocusWindow`, `ImeCaretArea` | window | the return; title and caret are wanted values | a refusal keeps the value wanted | latest value | ✓ C7 |
| 58 | font family lookup | `FontFamilyLookup` | window (row 5, done) | the return | — | one per stored face | ✓ C7 |
| | **Lifecycle** | | | | | | |
| 59 | data-directory endpoints start | `open_the_data_directorys_endpoints` | window (row 24) | the return | `recv_timeout(5 s)` + join | — | ✗ |
| 60 | video seat shutdown | `VideoSeats::{close, open, put}` | window (row 25) | the return | `SHUTDOWN_BUDGET` poll + join | — | ✗ |
| 61 | media quiet at exit | `video::shutdown_media_session` | main, after the loop (row 27) | the return | `MEDIA_QUIET_BUDGET` | — | ✗ exit |
| 62 | trace flush | `trace_sink::flush` (`TraceFlush`) | main, exit (row 17) | the return | `FLUSH_TIMEOUT` | — | ✓ exit, deadline |
| 63 | launch hand-over | `launch_wire::hand_over` (`LaunchHandOver`) | main, before the loop (row 18) | the return | `HANDOVER_BUDGET` | — | ✓ pre-loop, deadline |

**Counts, recomputed.** 63 protocols:
- **✓ 27**: rows 1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 15, 16, 20, 28, 32, 34, 35, 45, 46, 47, 48, 50, 56, 57, 58, 62, 63;
- **◐ 19**: rows 5, 12, 13, 14, 17, 19, 21, 22, 23, 24, 25, 26, 29, 31, 33, 51, 52, 53, 55;
- **✗ 14**: rows 18, 36, 37, 38, 39, 40, 42, 43, 44, 49, 54, 59, 60, 61;
- **? 3**: rows 27, 30, 41.

S-1's ledger is generated from this list. These counts replace §0's and §1.2's *11/8/4*. Revision (a)'s row 9 (*file, preview, git and index reads*) was the conflation N2-1 names. Git's mutations are now row 22, a ◐ with its storage-lane rules unstated.

### (b)2 · C1, operationally, and a transitive guard (N2-2)

**C1, replaced.** A request door **performs no synchronous external effect and waits on nothing**:
- no lock that another thread may hold across work;
- no thread, process, file system, network, GPU or owner door.

It may:
- read and write the window's own memory;
- take its lane's mailbox lock, which is declared and held only across a queue operation;
- send on a channel;
- return a synchronous refusal.

C1 sets no time limit. It names the kinds of operation that may not occur on the way from the door to its enqueue.

**§3.2, replaced: `every_request_door_returns_at_once`.**
- **Each door's row names its enqueue boundary**: the send, submit or `Condvar::notify` after which the request belongs to the worker.
- **The guard walks the resolved call graph** (`bt-source`'s item resolution, as the census and the `# effects` guard use it) from the door to that boundary. It fails on:
  - any `# vocabulary` path;
  - any `# entrances` path;
  - any `owner-door-body` item;
  - a blocking synchronisation primitive (`Mutex::lock`, `RwLock::{read, write}`, `Condvar::wait*`, `Receiver::recv*`, `JoinHandle::join`), except the declared mailbox lock.
- **It fails closed:**
  - on a dynamic call it cannot resolve (a trait object, a stored closure, a function pointer), unless the row lists that callee as reviewed;
  - on a macro invocation whose expansion it cannot see, unless the macro is on a short standard allow-list (`format!`, `vec!`, `matches!`, `debug_assert!`, and the like).
- **MUTATIONS** (each must go red):
  - a helper that calls `std::fs::write`, called from `handoff_lane::submit`;
  - a `Mutex::lock` on a lock the worker holds, in a door's helper;
  - a `Box<dyn Fn>` call on the path that the row does not list.

S-2 is **M**.

### (b)3 · Admission, synchronous refusal, and detached work (N2-3)

**Added to C2.** A door invocation does exactly one of two things:
- it **refuses synchronously**, and the refusal is its terminal outcome (`InputRefused`; the hand-off lane's refusal over its bound of 32);
- or it **mints an identity**.

Every minted identity then receives exactly one terminal completion. Nothing else counts as a request.

Two shapes are named, so the ledger can state them:
- **Detached maintenance** (row 3, shell retirement). The door's synchronous return is the terminal, `Detached`: the requester has handed the work to a sink that accepts it at once and owes nobody an answer. The sink's own failures go to `diagnostics.log` (C3). A detached protocol has no completion signal, and its ledger row says `detached`. The quit's bounded wait over all retirements (row 6) is a separate protocol with its own completion.
- **Triggers that are not requests.** A probe trigger with nobody waiting on its answer (a page opening, a birth asking the PSReadLine cell, a volume arriving) is an **ear event**. It mints no identity, and coalescing it into the probe already out owes nothing. A trigger *becomes* a request only when its caller holds the identity and waits for it (the walk a pane in birth waits on: `programs_lane::PaneWaits`). Then it must complete, which is why row 12 is ◐ and rows 7–9 are ✓.

### (b)4 · `Joined` is not `Superseded` (N2-4)

The terminal set becomes `Done(result)`, `Failed(error)`, `Cancelled`, `Superseded { by }`, `Joined { with }` and `WorkerGone`.
- **`Superseded { by }`** is reserved for a request displaced by a **newer, different** operation: a later navigation, or a later walk answering for a different program set.
- **`Joined { with }`** is for a request that is the **same** operation as one already admitted (the same document at the same revision; the same probe question). It completes with the joined request's own outcome, delivered independently to each identity.

§4's third case reads: a second `save(revision)` at the same revision completes as `Joined { with: first }`, carrying the first's receipt. C4 keeps "the later wins" for `Superseded` only.

### (b)5 · C5, two kinds of retry, and the honest trade-off (N2-5)

**C5, replaced.**
1. **No new request is scheduled autonomously after a terminal failure.** A failed effect is tried again only by a new request: new input, the next ear event, a trial's release, or the quit.
2. **A debounce may schedule a request from new input.** It coalesces requests that input made; it is not a retry.
3. **One admitted worker request may contain one declared, bounded contention acquisition** before it produces its single terminal result. Its row in the ledger names the resource, the bound, and that it runs on a worker. On the window thread, no wait is allowed (C1).

This makes the relationships precise:
- `WatchClock` and B5's watches ("tried again once at the next reader-triggered event, and never because time passed") are rule 1;
- B6's owed edits (*retried when a write could succeed, never on a clock*, B6 §4.1) are rule 1;
- the clipboard's `OpenClipboard` contention (S-7) is rule 3, on a worker.

**Q3, replaced: should a failed session backup be retried only by the next change, the next ear event or the quit's write, not by the debounce clock?** **Recommend yes, with this trade-off stated.** If the write fails and nothing changes afterwards, the newest session state stays unwritten until the next change or the quit. A crash in that interval loses it. The previous snapshot on disk (the last landed write, and `session.prev.json` from launch) limits what is lost to the changes since the last landed write, **but does not make the loss zero**. Today's re-arm narrows that interval for a transient failure. The recommendation keeps C5's letter because a disk that refused once on a quiet machine usually refuses again, and the person is told once per streak either way.

### (b)6 · C7 as a closed list, with what "bounded" means (N2-6)

A `WaitToken` admits and meters a call. It supplies **no elapsed-time bound**. Each member is therefore marked with its bound:
- **count**: how many calls per turn, request or window;
- **deadline**: an explicit timeout inside the call;
- **measured**: metered by `hang_watch` and nothing more.

| member | door(s) | bound | permanence |
|---|---|---|---|
| window title | `TitleFlush` | count (≤ 1 per frame) · measured | permanent (platform affinity) |
| IME caret area | `ImeCaretArea` | count (≤ 1 per frame) · measured | permanent |
| cursor, visibility, focus | `SetCursor`, `SetVisible`, `FocusWindow` | count (on change, per gesture) · measured | permanent |
| surface and compositor birth, window size | `SurfaceBirth`, `CompositorBirth`, `CompositorWindowSize` | count (per window, per resize) · measured | permanent |
| compositor commit | `CompositorCommit` | count (per present or seat change) · measured | permanent |
| web environment, controller, rehost | `WebEnvironment`, `WebController`, `WebRehost` | count (per page, per rehost) · measured | permanent (engine affinity); the cost is D-64's |
| font family lookup | `FontFamilyLookup` | count (one per stored face) · measured | permanent |
| **present** | `PresentFrame` | count (per frame) · measured | **interim**: row 9's disposition (*0.5 — presentation lane*) stands; C7 holds it until that lane lands |
| PTY resize | `PtyResize` | count (per leaf per settle) · measured | **interim**: row 12, 0.5→0.6 |
| first GPU open | `GpuOpen` | count (once per process) · measured | **interim**: row 23, pending |
| exit and pre-loop waits | `PaneRetirementWait`, `SessionWriteWait`, `SessionWriterRetire`, `TraceFlush`, `UpdateLeave`, `LaunchHandOver` | **deadline** each (`wait_for_retirements` budget, `SESSION_SAVE_BUDGET`, the close poll, `FLUSH_TIMEOUT`, `LEAVE_WITHIN`, `HANDOVER_BUDGET`) | permanent; outside the running phase |

The list is closed. A new member needs an owner ruling and a row here, and the guard of (b)2 refuses an unlisted owner-door call reached from a request door.

**Q1, replaced: should these members (the permanent rows above) be a closed class whose completion is their return value, with present, PTY resize and the first GPU open admitted only until their lanes land?** **Recommend yes.** "Present stays on the owner thread" in revision (a) is withdrawn. It stays there only until row 9's 0.5 lane.

### (b)7 · The harness: a ledger in the product, and a seam each lane adopts (N2-7)

The review is right. `LaneUnderTest` drives five real lanes, and a scripted fake lane would prove only itself. §3.3 is replaced.

**The product seam** (`bt-app::lane`, made product code for the first time) has three parts that a lane adopts:
1. **`Executor`**, a worker factory. The real one spawns through `spawn_at_priority`; the test one runs a scripted body on a test-owned thread. A lane is built from an `Executor` instead of calling the thread door itself.
2. **`Ledger<Id, Owed>`**, the request ledger. It mints identities (or refuses synchronously), records what each identity is owed, and turns every identity into exactly one terminal: `Done`, `Failed`, `Cancelled`, `Superseded`, `Joined` or `WorkerGone`.
3. **Hooks**: `publish(id, terminal)` is called by the worker, and `worker_ended(cause)` is called by the `Executor` when the worker's thread ends.

Two causes are kept separate:
- **A task panic** is caught *inside* the worker body around one request. It becomes `Failed(Panicked)` for that request, and the worker goes on.
- **Worker death** is the thread ending, whether by an uncaught panic outside a task or by returning. `worker_ended` turns every owed identity into `WorkerGone` and leaves the lane in a fault state the consumer can see.

**S-3, reduced (M):**
- the `Executor`, `Ledger` and hooks;
- an exhaustive state-machine proof over `Ledger` alone: N = 3 identities, every interleaving of the six outcomes and of requester drop and `worker_ended`, each identity exactly one terminal. This proves product code, the ledger every lane will use;
- the new claims (`ASupersededRequestSaysSo`, `AJoinedRequestCarriesItsTwin`, `AFailureIsSaidOnce`, `AnOrphanedAnswerReachesItsSink`) and the `Twice` failure kind.

**Per-lane conformance** moves into each lane's repair ticket (S-4, S-5, S-6a, S-7). Each lane adopts the seam, and its `LaneUnderTest` adapter injects the scripted `Executor` into the **real** lane, so the suite drives the lane's own admission and acceptance.

*Correction, 2026-10-15 (T-STORAGE-WORKER-ON-THE-SEAM):* S-6a did not adopt the seam: it moved the preview's save onto the storage worker's own request protocol, and the worker stayed off the seam (`docs/plans/structural-debt.md` D-97). T-STORAGE-WORKER-ON-THE-SEAM adopts it for the storage worker: `persist::StorageLane` on `lane::Ledger` and `lane::Executor` with a line of its own (one FIFO, the trial's receipt served ahead of it, each job kind's own answer road), every job admitted by the ledger up to `persist::STORAGE_BOUND` (50, coordinator ruling R1), a panic in one job that request's `Failed` with the worker going on (R2), and `LaneName::Storage` keeping every claim. `Terminal::Cancelled`'s first adopter is S-7 (T-CLIPBOARD-LANE, R3): the storage worker drops no request at its requester's word.

MUTATIONS:
- in the ledger, drop a superseded identity's terminal;
- deliver twice on a cancel racing an answer;
- let `worker_ended` skip the owed set;
- let a caught task panic end the worker.

### (b)8 · S-6 is B6's storage worker; close and quit time out explicitly (N2-8)

**One storage worker, two protocols.** S-6's lane **is B6's storage worker**: `persist::SessionWriter` on `session-writer`, which B6 extends with `StorageJob::Document` and `StorageJob::Read` (B6 §3.2) and wakes with `AppEvent::DocumentsAnswered`.

A document save becomes one more job kind on that worker, `StorageJob::Save { source, revision, bytes, baseline, answer }`. It shares **scheduling and per-resource serialisation**: one worker, one channel, in order, so a quit's final session job lands after every save queued before it. Each job kind keeps its own protocol:

- **Configuration documents keep B6's protocol unchanged**, as B6 states it:
  - revision = exact bytes (`Revision<T> { bytes, document }`);
  - the precondition read, then the conditional exchange (`replace_keeping_displaced` / `RENAME_SWAP`) that hands back the displaced file;
  - `Landed`, `Moved`, `Displaced { kept }` and `Refused`;
  - the three-way merge;
  - owed edits defined as *the in-memory document differs from its revision*, retried only on the triggers of B6 §4.1.
- **Document saves keep the preview's own baseline-and-conflict protocol.** That is `PreviewBuffer::save`'s preserving save, judged against the buffer's disk baseline (`disk_mtime`, `DiskNews`): `SaveOutcome::{Saved, Conflict, Failed}`, carried as `Saved { revision }`, `Conflict { revision }` or `Failed { revision, error }`. Dirty clears only on `Saved` for the document's current revision. A document save is not merged three ways. A conflict is said to the person, as today.

**Timeouts, defined separately:**
- **A pane, tab or window close** whose dirty gate is waiting on an owed save receipt waits across turns, up to a close deadline (a new constant, `SAVE_RECEIPT_DEADLINE`, named by S-6b; a clock on the turn, never a wait on the thread). If the deadline passes, the save's outcome counts **for the gate** as `Failed(TimedOut)`. The gate **stays open** and asks its question (*Save · Discard · Cancel*) over a document that is still dirty. Dirty is never cleared by the timeout. A receipt that arrives later still settles the document if its revision is still current.
- **An ordinary application quit** hands every owed save to the worker before its final session job (B6 §4.3's rule, the same order). The quit's existing wait (row 16, `SESSION_SAVE_BUDGET`) covers them, so there is no new wait. A save not landed by the budget is a refused write. Under A§11 (*a refused disk means do not leave*) the quit is **cancelled** and the gate comes back saying the save did not finish. The quit does not leave with an unsaved document behind a timeout.
- **A forced stop** (`FolioApp::fail`, `exiting`: `stop_every_window`, A§11) **cannot wait**. Per the recovery-copy ruling (G7, T-RECOVERED-FOLDER), each dirty document goes to `keep_dirty`. When a save of that document is still owed on the worker, the forced road does **not** write over the file. It writes the recovery copy into the data directory's `recovered` folder, which is the rule's *never over the source file merely because Folio is failing*, and says where in one `diagnostics.log` line.

S-6 is split into three tickets: S-6a (the job kind and lane adoption, M), S-6b (the close gate's wait and its timeout, M) and S-6c (quit and forced-stop integration, M). B6 carries the cross-reference into its own review round; this note states it here and in (b)10.

**Q5, replaced: should a close wait for an owed save, and what happens at the deadline?** **Recommend:**
- the close waits silently across turns;
- at the deadline the gate asks, with the document still dirty;
- a quit whose saves have not landed by its budget is cancelled, not completed;
- a forced stop writes the recovery copy.

### (b)9 · S-7 moves every Windows `OpenClipboard` road; macOS is a separate question (N2-9)

Row 26 covers three roads: `clipboard_text` (paste), `set_clipboard_text` (copy) and `WindowsClipboard::begin` (the picture's open) (`window_waits.tsv:80`). S-7 moves **all three** onto one Windows clipboard worker, which owns a message-only window (`HWND_MESSAGE`) for `OpenClipboard`. Only then is **row 26 retired**. Each road becomes a request:
- `ClipboardWrite(text)` → `ClipboardWritten(result)`;
- `ClipboardRead` → `ClipboardRead(result)`;
- `ClipboardPictureOpen` → the existing picture road.

The bounded contention acquisition is declared in the worker's row (C5 rule 3).

**Paste becomes a request with a completion.** Keys typed after a paste request and before its completion are held behind it in the pane's input order, the way a pane in birth holds typed bytes (`PaneBirth::hold`), so the paste never lands after them.

**macOS is out of S-7's scope.** The macOS pasteboard arm leaves *the main-thread question* open (`crates/bt-platform/src/lib.rs:19213`). Until **experiment E-2** answers it (do `NSPasteboard` reads and writes off the main thread behave on the macOS versions Folio supports), the macOS clipboard stays a C7 owner-thread member, *pending* (bound: count, one call per gesture · measured). It is not promised a worker.

**Q2, replaced: may the Windows clipboard worker hold a declared, bounded contention wait for `OpenClipboard`?** **Recommend yes, on that worker only, for Windows.** macOS waits on E-2. S-7 is **M**.

### (b)10 · Tickets, re-sized and in order (N2-10)

| # | ticket | size | after | closes |
|---|---|---|---|---|
| S-1 | T-COMPLETIONS-LEDGER: classify (b)1's three `?` rows; write `# completions` from (b)1's 63 rows, each with its enqueue boundary and its terminal shape (`completes`, `detached`, `owner-thread`, `exit`); the four held equalities | **M** | — | the ledger |
| S-2 | T-REQUEST-DOOR-GUARD: (b)2's transitive guard and its three canaries | **M** | S-1 | C1 as a gate |
| S-3 | T-COMPLETION-HARNESS: (b)7's `Executor`, `Ledger`, hooks, the exhaustive ledger proof, and the new claims | **M** (reduced) | S-1 | C2–C4, C6 proven on the shared ledger |
| S-4 | T-LANES-SAY-SUPERSEDED: the font, taskbar and program-walk lanes adopt the seam; per-lane conformance through scripted `Executor` injection | **L** | S-3 | **V3**: D-72, D-73, D-85 |
| S-5 | T-HANDOFF-DEATH: the hand-off lane adopts the seam; owed ids end `WorkerGone`; answers bounded | M | S-3 | D-70, D-71 |
| S-6a | T-DOCUMENT-SAVE-JOB: `StorageJob::Save` on B6's storage worker; the preview's baseline protocol; `Saved`/`Conflict`/`Failed { revision }` | M | S-3, B6's T-STORE-CONDITIONAL-WRITE, OC O-7 | **V1**, row 20's save half |
| S-6b | T-CLOSE-GATE-WAITS: the gate's wait across turns, `SAVE_RECEIPT_DEADLINE`, the timeout keeping the gate open | M | S-6a | (b)8's close rule |
| S-6c | T-QUIT-AND-STOP-SAVES: owed saves before the final session job; a cancelled quit on a missed budget; the forced stop's recovery copy | M | S-6b | (b)8's quit and stop rules |
| S-7 | T-CLIPBOARD-LANE (Windows): the three `OpenClipboard` roads on one worker; paste ordering; row 26 retired | **M** | S-3 | **V2**, row 26 |

The order the review confirmed stands: ledger, then guard and harness, then lane repairs. Left after these:
- the session writer's wake and its re-arm (Q3);
- rows 2–4 (onto the storage lane);
- git mutations' storage rules (row 22);
- web navigation identity (row 52, after E-1);
- rows 10, 24, 25, 27 and 28;
- the computation lane (D-74…D-76).

### (b)11 · The owner questions, revised (five)

1. **(b)6** Should the permanent owner-thread members be a closed class completed by their return value, with present, PTY resize and the first GPU open admitted only until their lanes land? **Recommend yes.**
2. **(b)9** Should the Windows clipboard worker hold a declared, bounded `OpenClipboard` contention wait, with macOS left on the owner thread until E-2? **Recommend yes.**
3. **(b)5** Should a failed session backup be retried only by the next change, ear event or quit, accepting that a crash before then loses the changes since the last landed write? **Recommend yes.**
4. Unchanged: **should superseded or joined completions ever reach the person?** **Recommend no.** Only failures reach `diagnostics.log`.
5. **(b)8** Should a close wait silently for an owed save, ask at the deadline with the document still dirty, cancel a quit whose saves missed the budget, and write a recovery copy on a forced stop? **Recommend yes.**

### (b)12 · Experiments owed, revised

- **E-1** as §5: does each engine complete every superseded or cancelled navigation exactly once, and how is it marked? It also decides whether row 52 needs Folio's own navigation id (WebView2's `NavigationStarting` carries a `NavigationId`; the event as Folio records it today does not keep it, `webview.rs:176–195`).
- **E-2** (new, before any macOS clipboard ticket): `NSPasteboard` read and write off the main thread, on the supported macOS versions.

### (b)13 · Architecture impact, revised

As §8, with three changes:
- **Doors.** S-6a adds a job kind to the existing storage worker, not a new door.
- **Product code.** `bt-app::lane` becomes product code (S-3).
- **Registry.** Row 26 leaves `# rows` only with all three Windows roads moved (S-7). Row 20 narrows twice: once by B6 (the four stores) and once by S-6a (the save). The rename half remains.

---

## Revision (c), 2026-10-11, after the Codex confirmation round

Review: `reports/J5-05-contracts.review-codex-b.md`. Verdict on this note: **adopt with changes (blockers only)**. N2-3 to N2-7 are confirmed. Four blockers remain: the inventory (N2-1), the guard (N2-2), the save lane (N2-8) and the paste ordering (N2-9). Each is applied below and none is argued.

Item 3 is also the **coordinator's ruling for both notes**: one storage worker, one request protocol (B6's), and two semantic policies. B6's revision (c) is being written to the same words. **Where (b) and (c) differ, (c) stands.**

### (c)0 · What changed

| # | the review's words (abridged) | disposition | where |
|---|---|---|---|
| 1 | *"The three `?` rows are already classifiable … Row 27 … partial … Row 30 … window-thread effect without admission … Row 41 … must be at least two rows … 'S-1 must explicitly split row 41 and recount … The minimum is 64 protocols.'"* | applied | (c)1 amends (b)1's rows 27, 30 and 41, its counts, and S-1 |
| 2 | *"The C1 guard requires a new `bt-source` feature. Today `Index` explicitly says that no query resolves a path to its definition … split that engine into an L prerequisite and leave the request-door assertion as S/M. S-2 as M is not credible"* | applied: split | (c)2 replaces S-2 in (b)10 |
| 3 | *"S-6 contradicts B6 revision (b) … the conditional exchange is the lane protocol … must use B6's shared request `Document { expected: exact_disk_bytes, bytes, mode: Restore, link_policy }`. It must not retain the old mtime-only `PreviewBuffer::save` … S-6c must also use B6's multi-turn `Documents` quit phase"* | applied; this is also the coordinator's ruling | (c)3 replaces (b)8's "two protocols" paragraph and S-6a/c |
| 4 | *"S-7 needs a per-pane held-input cap plus an enqueue-to-terminal paste deadline. On timeout/`WorkerGone`, it must cancel the paste, release the buffered keys in order, and ignore a late paste result."* | applied, with the numbers and the tests | (c)4 amends (b)9 and S-7 |

### (c)1 · The three unclassified rows, classified; row 41 split (64 protocols)

**Row 27, background picture decode: ◐.**
- the spawn's refusal is discarded;
- a superseded generation gets no terminal outcome;
- the worker's death has no completion.

**Row 30, desktop notification: ✗.** `notifications.show` (`runtime/attention.rs:787–792`) is a window-thread call with no admission. It is **not** a member of (b)6's closed C7 list. S-1 writes it as a window-thread effect, and its disposition is an owner question if it is to stay there (Q1's closed list does not include it).

**Row 41 splits into two protocols:**

| # | protocol | door | thread | signal | failure road | identity · sink | status |
|---:|---|---|---|---|---|---|---|
| 41a | shell-integration enable and undo | `profile_runtime` install/undo jobs | `powershell-profile-install`, `powershell-profile-install-undo` | an addressed `ProfileInstallOutcome` (`profile_runtime.rs:14`) | the outcome's refusal | addressed to its asker | ◐ (the spawn refusal and a worker's death have no stated terminal; to measure in S-1) |
| 41b | shell-integration removal | `profile_runtime::begin_removal` (`profile_runtime.rs:655`) | `powershell-profile-removal` | a global `REMOVAL` slot plus `WAKE`, taken by `take_removal` | the `Report` | no identity: the newest report overwrites the slot, and a spawn refusal is discarded (`let _ =`) | ◐ C2 |

The review counts *arguably three doors* (install, undo, removal). S-1 decides whether install and undo are one protocol (one outcome type, one address) or two, and the count moves with that decision.

**Counts, recomputed (64):** ✓ 27, ◐ 22 (19 plus rows 27, 41a and 41b), ✗ 15 (14 plus row 30), and nothing unclassified. **S-1 takes this list as its floor, not as a fixed input.** It re-derives the inventory from the registry, the `AppEvent` answers, the lanes and the spawns, splits wherever one row has two doors or two answer shapes, and recounts.

### (c)2 · The C1 guard needs a call-graph engine first

Revision (b)'s guard needed resolution that `bt-source` does not have. `Index` says so explicitly: *no query in this crate resolves a path to a definition* (`crates/bt-source/src/index.rs:795–797`). The census's receiver resolver is private and specific to field writes. The `# effects` guard examines only an effect function's own body. S-2 therefore splits:

- **S-2a T-BT-SOURCE-CALL-GRAPH (L).** A public, resolved, execution-aware call graph over the product universe:
  - paths resolved to their item (functions, methods by receiver type, through `use` and the module tree), reusing the census's receiver rules as a public resolver;
  - edges annotated with what they cannot see: dynamic dispatch (trait objects, stored closures, function pointers) and macro token trees, each marked **unresolved**;
  - a walk from a set of roots that **stops at declared boundary items** and reports every unresolved edge on the way;
  - its own fixtures, each going red under a named mutation: a helper two calls deep, a method resolved through `Deref`, a trait-object call that must report as unresolved, and a macro that must report as unresolved.
- **S-2b T-REQUEST-DOOR-GUARD (S/M), after S-2a.** The assertion of (b)2 on top of the engine:
  - from each `# completions` door to its enqueue boundary, no `# vocabulary` or `# entrances` path, no `owner-door-body` item, and no blocking primitive outside the declared mailbox lock;
  - **fail closed** on every unresolved edge that the row does not list as reviewed;
  - (b)2's three canaries.

**Until S-2a lands**, C1 is held by what exists today, stated as such:
- `every_bare_site_is_a_row_and_every_row_a_site` keeps the window thread's bare waits from growing;
- the `# effects` guard keeps waiting effects inside registered bodies;
- each request door's **own body** is checked against `# vocabulary` (a direct-body check over the `# completions` items, which `bt-source` can do now) as part of S-1.

This interim check is explicitly **not** a C1 gate. A helper bypasses it, which is the review's point.

### (c)3 · The save lane: B6's request, the preview's policy

**The coordinator's ruling, for both notes.** One storage worker (`persist::SessionWriter`), **one request protocol, B6's**, and two semantic policies. Revision (b)'s *"document saves keep the preview's own baseline-and-conflict protocol"* and its `StorageJob::Save` are **withdrawn**.

- **The request** is B6's shared job:
  `Document { expected: exact_disk_bytes, bytes, mode, link_policy }`.
  - `expected` is the disk revision **as exact bytes**: what the buffer last read, or last landed.
  - The worker runs B6's conditional exchange: a precondition read, an exchange that hands back the displaced file, and a verification. Its answers are B6's: `Landed`, `Moved`, `Displaced` and `Refused`.
  - The old mtime-only check in `PreviewBuffer::save` (`preview.rs:5778` ff.) is **retired**. It does not survive as a separate mechanism or job kind.
- **The preview's semantic policy** sends `mode: Restore`.
  - When the worker finds displaced bytes that are not `expected`, it puts them back (B6's `Restore`) and answers **`Conflict`**: nothing lost, and no three-way merge.
  - `Moved` is also `Conflict` for the preview.
  - The four configuration stores send `Merge` and keep B6's merge and owed-edit rules.
- **Two revisions per preview request.** The request carries the buffer's **body revision** beside `expected`.
  - `Landed` for the current body revision maps to `Saved { revision }` and clears `dirty`.
  - `Landed` for an older body revision updates the disk revision only. Dirty stays, and the newer body is owed.
  - `Conflict` and `Refused` leave `dirty` as it is.
- **`link_policy`**: the preview keeps `PlainWriter` until a ruling. That ruling is J5's to ask, and it is asked as part of Q5 below. The configuration stores keep B6's `Refuse`.
- **S-6c uses B6's multi-turn quit phases** (B6 revision (c)2), not FIFO followed by the blocking session wait. That wait cannot process `DocumentsAnswered`.
  - **Documents phase**, until `QUIT_DOCUMENTS_BY`: owed preview saves and owed stores are sent, and each answer is processed on the window thread across turns.
  - **Recovery phase**, until `QUIT_RECOVERY_BY`: configuration debts with a terminal refusal take B6's road for them (B6's recovery copy).
  - **Then the session write**, behind landed copies, waiting as today (row 16).
  - **An ordinary quit with an unresolved preview save is cancelled.** This is (b)8's rule, and B6 (c)2 states the same cancellation. B6's *a second quit for the same refusal leaves, and says so* applies to the preview as well.
- **Close and forced stop** are as (b)8:
  - a close that reaches its deadline counts the save as `Failed(TimedOut)` for the gate, which stays open with the document still dirty;
  - a forced stop writes the recovery copy and never writes over the file.

**Tickets, replacing S-6a and S-6c** (S-6b unchanged):

| # | ticket | size | after | closes |
|---|---|---|---|---|
| S-6a | T-PREVIEW-ON-THE-STORAGE-LANE: the preview sends B6's `Document` with `mode: Restore` and its body revision; the disk revision becomes exact bytes; `PreviewBuffer::save`'s mtime check is retired | M | B6's lane ticket, S-3, OC O-7 | **V1**, row 20's save half |
| S-6c | T-QUIT-AND-STOP-SAVES: preview saves inside B6's `Documents` phase; the quit cancelled on an unresolved save; the forced stop's recovery copy | M | S-6b, B6's quit ticket | (b)8's quit and stop rules |

### (c)4 · The paste hold, bounded

A paste is a request (`ClipboardRead`). Keys typed after it are held behind it in the pane's input order, under two bounds:

- **A per-pane held-input cap of 1 MiB**: `bt_pty::PTY_INPUT_RING_BYTES` (`crates/bt-pty/src/lib.rs:183`), the same capacity a pane in birth holds (`PaneBirth::hold`). A key that would push the held bytes over the cap **cancels the paste early**, as the deadline does.
- **An enqueue-to-terminal paste deadline of 1 s**, `PASTE_DEADLINE`, a clock on the turn and never a wait. It is measured from the request's enqueue to its terminal answer.
  - It covers the worker backlog, the contention wait (at most 75 ms: five `OpenClipboard` attempts with 5 + 10 + 20 + 40 ms between them, the bound S-7 keeps), and the source application's delayed rendering inside `GetClipboardData`, which has no bound of its own.
  - One second is long enough for a contended clipboard and short enough that held keys never read as a lost keystroke.

**On the deadline, on `WorkerGone`, or on the cap:**
1. the paste is **cancelled** (`Cancelled`);
2. the held keys are **released to the shell in their order**;
3. a later answer for that paste is **ignored**, because its identity is no longer owed;
4. one `diagnostics.log` line says the paste was cancelled and why.

**Backlog.** The clipboard worker admits at most one outstanding request per pane and four per process. A fifth is refused synchronously (`ClipboardBusy`, C2's synchronous refusal), and the copy or paste gesture says so once.

**Text size.** No cap is added. A paste goes whole, as today (`bt-pty` *one paste is one write*), and the deadline is what bounds the wait for it.

Tests in S-7. Each uses a fake clipboard worker and a fake clock, types Chinese and mixed-script keys after the paste, and goes red under the named mutation:
- `keys_typed_after_a_paste_reach_the_shell_after_the_pasted_text`. MUTATION: deliver held keys before the paste's answer.
- `a_paste_past_its_deadline_is_cancelled_and_the_held_keys_go_in_order`. MUTATION: no deadline (the keys are held for ever).
- `a_late_paste_answer_after_cancellation_is_ignored`. MUTATION: write the late text.
- `worker_gone_cancels_the_paste_and_releases_the_keys`. MUTATION: leave the identity owed.
- `held_keys_over_the_cap_cancel_the_paste_and_go_in_order`. MUTATION: drop the key that crossed the cap.
- `a_fifth_outstanding_clipboard_request_is_refused_at_once`. MUTATION: queue it.

S-7 stays **M**.

### (c)5 · Tickets, in order, revised

| # | ticket | size | after |
|---|---|---|---|
| S-1 | T-COMPLETIONS-LEDGER: re-derive from (c)1's 64-row floor, split where needed, recount; write `# completions`; the interim direct-body check ((c)2) | M | — |
| S-2a | T-BT-SOURCE-CALL-GRAPH | **L** | — |
| S-2b | T-REQUEST-DOOR-GUARD | **S/M** | S-1, S-2a |
| S-3 | T-COMPLETION-HARNESS (as (b)7) | M | S-1 |
| S-4 | T-LANES-SAY-SUPERSEDED | L | S-3 |
| S-5 | T-HANDOFF-DEATH | M | S-3 |
| S-6a | T-PREVIEW-ON-THE-STORAGE-LANE ((c)3) | M | B6's lane ticket, S-3, OC O-7 |
| S-6b | T-CLOSE-GATE-WAITS (as (b)10) | M | S-6a |
| S-6c | T-QUIT-AND-STOP-SAVES ((c)3) | M | S-6b, B6's quit ticket |
| S-7 | T-CLIPBOARD-LANE (Windows), with (c)4's bounds and tests | M | S-3 |

### (c)6 · The owner questions, revised (still five)

Q1–Q4 are as (b)11. Q1's closed list does not include the desktop notification (row 30); S-1 brings it back as a request door or as a ruled member. Q5 replaces (b)11's Q5:

5. **(a)** Should a close wait silently for an owed save, ask at the deadline with the document still dirty, cancel a quit whose preview saves are unresolved at `QUIT_DOCUMENTS_BY`, and write the recovery copy on a forced stop? **Recommend yes.**

   **(b)** What should a preview save do with a file that is a symbolic link? Today the save **replaces the link with a file**: the link's target keeps the old bytes, and nobody has ruled on it (`preview.rs:5766–5773`, *a rename replaces the link, not what it points at*). B6's `link_policy` offers `PlainWriter`, which keeps today's behaviour, or `Refuse`. **Recommend `Refuse` for the preview, with the refusal said on the pane.** Silently turning a person's link into a copy breaks the link without telling them, and a refusal they can see is better.

   Writing *through* the link to its target is a third policy that the lane does not have. If wanted, it is a separate ticket with its own ruling. Until this question is answered, S-6a keeps `PlainWriter`, so today's behaviour does not change by accident.

*Revision line, 2026-10-17 (T-BT-SOURCE-CALL-GRAPH):* S-2a landed. `bt_source::call_graph` is the engine (c)2 asked for: `Resolver::new(&[(package, &Index)], Arms)` places every name through the module tree and types receivers by the census's steps; `Resolver::resolve_callees(item) -> Vec<Edge>` turns a body's call sites into `Edge { from, site, to, via }` with `to` one of `Target::Item`, `Constructor`, `Closure`, `Nested`, `Foreign(path)` (a boundary) or `Unresolved(kind)` (`Dynamic`, `Ambiguous`, `Macro`, `OtherArm`, `Generated`, `Path`, `Cutoff`, `Unparsed`); and `CallGraph::walk(roots, boundary) -> Reach { reached, stopped_at, foreign, unresolved, implicit }` stops at the boundary items and carries every foreign and unresolved edge with the path that reached it. What S-2b inherits: roots from each `# completions` door, its enqueue boundary as the `boundary` set, its vocabulary judged over `Reach::foreign` by path, fail-closed over `Reach::unresolved` by kind (`Cutoff` and `Unparsed` never listable), the closures handed to the boundary call told apart by `CallSite::closure`, and `Reach::implicit` (every first-party impl of a foreign trait, `impl Drop for ComputationLane` among them) to review. The door reach in `window_waits_tests` already walks it.
