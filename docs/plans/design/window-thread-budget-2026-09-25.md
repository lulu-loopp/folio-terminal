# The window thread's budget, replacing the list

Status: **draft, 2026-09-25**, for Codex review before any ticket is dispatched
(`CONVENTIONS` §十 rule 11: two of the moves below change who writes a fact
during an operation). Base `f7826bd4`. It answers ledger row **D-2** (*the window
thread's blocking set is a list, not a budget*) and orders the §5.3 rows that
D-2 closes with: D-33, D-34, D-35, D-36, D-39, D-40, D-41, D-42 and D-47, plus
the deferred D-43 and D-44 and the 0.4.5 row D-64. Code is cited by symbol.
Numbers are quoted from the ticket reports that measured them, which are named. A
number nobody has measured is marked *estimate*, and the note says what the
ticket that owns the row has to measure.

Everything targeted here is **0.4.6**. By the owner's ruling of 2026-09-25, 0.4.6
code merges only after the 0.4.5 tag, so every ticket at the end of this note
ends at "committed, CI green on the branch".

## 0. Where things stand on `f7826bd4`

§5.3 has 21 rows:

- **Five are done**: 1 (the hand-off lane, `2657e5e3`), 5 (ticket 50), 6
  (ticket 51), 13 (ticket 48) and 14 (ticket 49).
- **Five are ruled to stay**: 15–19. They are on the way out, before the loop
  exists, or bounded ring operations.
- **Eleven are open**:

| row | ledger | what | version | measured? |
|---|---|---|---|---|
| 2 | D-34 | marks lock, install half | 0.4.6 | no |
| 3 | D-35 | `psreadline::apply_recorded`, nine files under the lock | 0.4.6 | no |
| 4 | D-36 | `psreadline::installed_copy`, recursive walk | 0.4.6 | no |
| 7 | D-39 | macOS locale children | 0.4.6 | no |
| 8 | D-40 | macOS `DirWatch` start and drop | 0.4.6 | no |
| 9 | D-41 | acquire, submit, present, configure, DirectComposition commit | 0.4.5 on the ledger; the owner deferred building it on 2026-09-24 | **yes**, next89/next90 |
| 10 | D-42 | device recovery: `block_on` plus sleeps | 0.4.6 | no |
| 11 | D-43 | PTY birth | deferred → 0.5 toward 0.6 | no |
| 12 | D-44 | `ResizePseudoConsole` round trip | deferred → 0.5 toward 0.6 | no |
| 20 | D-47 | renames, the preserving save, store writes | 0.4.6 | no (stations exist) |
| 21 | D-64 | WebView2 first page | 0.4.5, ticket 54 in flight | **yes**, ticket 43 (headless) |

**The table and the ledger disagree on versions.** §5.3 still says **0.4.4** for
rows 2, 3, 4, 7, 8 and 20, and **0.5** for rows 9 and 10. The ledger says 0.4.6
for all of them except D-41, which it gives 0.4.5. Ticket B1 below aligns §5.3
with the ledger.

**next92** (`1296e992`) contains tickets 43, 48, 49, 50 and 51: the five
"self-inflicted waits" the owner asked to have fixed and measured before ruling on
D-41. It does not contain ticket 46 or ticket 54. No next92 stall report is in
the trace as of this note.

**D-41's reviewed design** (`docs/plans/design/presentation-lane-2026-09-24.md`
and its Codex review) is on the branch `docs/presentation-lane-design` at
`c1a6beb9`. That branch is not merged. This note cites it by that path.

## 1. What "a budget" means, operationally

### 1.1 Why a list is not enough

§5.3 is a list of places the window thread waits, each with a ruling. Nothing
connects it to the code. The prose says "a call not on this list that blocks on
something outside the process is a defect", but no test or script enforces that
sentence. Rows 13, 14 and 21 were all found by a person reading a stall line, not
by a gate.

The list also states no quantity. Three decisions were each made from first
principles, and none of them can be read as the rule for the next:

- ticket 50 accepted a 2.6–3.4 ms lookup because it was "under one frame";
- ticket 51 sized a slice at "a quarter of a frame";
- `DRAIN_TURN_BUDGET` is "half of a 60 Hz frame".

### 1.2 The unit: one turn, against the frame interval

A **turn** is one hold of the window thread, from `Heartbeat::woke` to
`Heartbeat::park`: events and `Runtime::turn` together, across every window.
`hang_watch` already measures exactly this span, and it is the span the user
feels, because keys arrive on the same thread.

The reference interval is `pace::DEFAULT_FRAME_INTERVAL`, 16 ms. Every existing
allowance was derived from it.

| name | value | derived from | bounds |
|---|---|---|---|
| `TURN_BUDGET` | 16 ms, one interval | §5.1: *work is budgeted across all windows* | a whole turn |
| `WAIT_BUDGET` | 8 ms, half a turn | `DRAIN_TURN_BUDGET`'s argument: the other half is the frame this turn publishes | the sum of the admitted waits in one turn |
| `WAIT_ALLOWANCE` | 4 ms, a quarter | ticket 51's slice sizing; covers ticket 50's measured 3.4 ms cold lookup | one call to one admitted wait |

Whether these shrink on a faster display is open question 2. The drain's 8 ms
does not shrink today.

### 1.3 Two kinds of spend

- **Work** is the process's own CPU: the drain, composition, search, layout. Work
  is budgeted by **slicing**. A loop stops when its allowance is spent, books a
  deadline in the wake fold, and resumes on the next turn. The reference instances
  are `DRAIN_TURN_BUDGET` (with `coalesce::decide`) and
  `search::SEARCH_HISTORY_SLICE` (with `Runtime::advance_search_scan`).
- **A wait** is a call that can block on something outside the process: the disk,
  another process, the driver, the compositor or the OS. A wait cannot be sliced,
  and once started it cannot be interrupted (§5.3: *a timeout does not cancel an
  uninterruptible call*). So a wait is budgeted **at admission**, by where it runs,
  and **accounted at run time**. It is never refused mid-call.

### 1.4 Five dispositions; every wait has exactly one

1. **Lane** (the default). The call moves to a lane that meets the D-33 contract.
   The window thread publishes and drains, and never waits.
2. **Slice.** The wait is split into pieces, each under `WAIT_ALLOWANCE`, spread
   across turns. This fits bounded work over external data (ticket 51's scan).
3. **Idle.** The call stays on the window thread but moves *in time*: to a turn
   with no input pending and no picture owed. The reference instance is ticket
   54's warm-up. This fits a thread-affine call that has to happen once.
4. **Bound.** The call stays where it is, with a measured bound at or under
   `WAIT_ALLOWANCE` and its own station. The reference instance is ticket 50's
   `monospace_family_named`: 2.6–3.4 ms cold, against a 4 ms allowance.
5. **Stay.** Rows 15–18: on the way out, or before the loop exists. No turn is
   running, so they are not charged.

**§5.2's native-affinity calls** (IME, the web view's controller,
DirectComposition's commit, title, focus, cursor) are there because the OS
requires this thread. They take disposition 3 or 4, each with **its own measured
bound**, which may exceed `WAIT_ALLOWANCE` only by a ruling (open question 3).
They are accounted like every other wait. Native affinity exempts a call from the
move, not from the measurement.

### 1.5 Who owns it, who spends it, how a spend is counted

- **The owner.** The coordinator's rulings own the numbers and the admission
  table. They are written twice: in `docs/ARCHITECTURE.md` §5.3, which becomes
  the budget's prose plus the table, and in `crates/bt-app/src/window_waits.tsv`,
  the table's machine form (§5.1). A script holds the two equal.
- **The spenders** are the call sites, one per door. A spend is
  `hang_watch::during(Station::X, || call)` around that one call, and each §5.3
  row names its station.
- **The accounting** is `hang_watch`. It already has the clock, the stations, the
  exclusive call tree (`hang_watch_detail::Tree`, keyed with parent and pane) and a
  queue the window thread never waits on. Reading `hang_watch.rs` on the base, it
  lacks six things:
  1. **A kind per station.** Nothing says whether `Station::RenameDisk` is a wait
     or work, or which §5.3 row it serves. `present_progress` is the only
     classifier, and it covers the present stages only.
  2. **Sub-millisecond resolution.** `Heartbeat::now_ms` truncates to
     milliseconds, and `charge` adds millisecond differences. A 4 ms allowance
     cannot be checked on that clock, and a 0.9 ms call charges zero.
  3. **Anything below 500 ms.** `SLOW_HOLD_THRESHOLD` is thirty frames, so a turn
     that spent 40 ms waiting (the budget's whole subject) leaves no trace. There
     is no per-station maximum or count for the run.
  4. **Stations on five rows.** Rows 2, 3 and 4 (`Runtime::add_to_profile`,
     `spend_powershell_intent`, `apply_psreadline`, `refresh_psreadline_installed`)
     and rows 7 and 10 (`system_locale_declaration`, `recovered_from_a_lost_device`)
     are charged to whatever handler or clock encloses them. Row 8 has only the
     shared parent `Watches`.
  5. **Headroom.** A station is one byte (`Station::from_byte`; the call-tree key
     keeps it in its low 8 bits). `STATION_COUNT` is 207, which leaves 48.
  6. **A tie between station and list.** A new blocking call can be added with no
     station, and nothing goes red.

### 1.6 When a spend would overrun

- **At admission (design time).** A new wait is admitted with a disposition. A
  wait that cannot show a bound within `WAIT_ALLOWANCE` gets a lane, a slice or an
  idle turn. Only a ruling accepts a larger bound.
- **At run time.** `hang_watch` reports and never intervenes. Its module doc: *a
  watchdog that acts is a watchdog that can be wrong*. When a turn's waits exceed
  `WAIT_BUDGET`, or one wait exceeds its row's bound, a **budget line** goes to the
  diagnostics log: row, station, microseconds, bound. The watchdog formats and
  writes it. The window thread only `try_lock`s and pushes, as it does for slow
  holds. A recorded overrun of an admitted row is a finding against that row: the
  line is its evidence, and it reopens the ruling. It is never a trigger for a
  runtime fallback.
- **Work that overruns** is already stopped by its own slice. A budget line
  naming work is a finding against that slice's allowance.

### 1.7 The rule, in one paragraph a ticket can implement

> **The window thread's budget.** A turn (`Heartbeat::woke` to `Heartbeat::park`)
> has `TURN_BUDGET` (16 ms). The waits in it, meaning calls that can block outside
> the process, have `WAIT_BUDGET` (8 ms) together and `WAIT_ALLOWANCE` (4 ms)
> each. A wait may run on the window thread only if it is a line of
> `crates/bt-app/src/window_waits.tsv`. The line carries the wait's §5.3 row, its
> door, its owning function with the exact number of calls, its station, and one
> disposition: `lane (pending)`, `slice`, `idle`, `bound=<µs>` or `stay`. Every
> such call is wrapped in `hang_watch::during` with that station. The source guard
> `window_waits_tests::every_wait_the_window_thread_makes_is_on_the_list` refuses
> a vocabulary call whose owner is neither a declared lane body nor a list line,
> and refuses a line the code no longer matches exactly.
> `scripts/ci/check-window-waits.ps1` refuses a line added since the merge base
> unless its ruling column names a `docs/DESIGN.md` entry added on the same
> branch. `hang_watch` classifies each station by `Station::spend()` and writes a
> budget line when a turn's waits exceed `WAIT_BUDGET` or one wait exceeds its
> bound. Nothing is refused at run time. An overrun reopens its row.

## 2. The remaining rows, one by one

*Door* is the call's one entrance. *D-33* says whether the move creates or wraps a
lane, which then has to be born conforming.

### Row 2 · D-34 · the marks lock's install half

- **Cost.** Not measured. From `profile_runtime::install_recorded`,
  `profile_marks::lock` and ticket 34's report:
  1. An `Asker::InApp` waits in the queue behind our own writer **with no
     deadline**. That writer is the `powershell-profile-enable` or `-removal`
     worker, so its whole transaction is this turn's bound.
  2. Then `try_lock` and `sleep`, up to `OUR_TURN` (2 s), for a holder in another
     process.
  3. Then a dated copy of `$PROFILE`, an atomic write, and two `Marks::write`
     calls.

  *Estimate*: single-digit milliseconds on a quiet SSD, **2 s plus the worker's
  transaction at worst**.
- **Door.** `shell_integration::install_into_profile` →
  `profile_runtime::install_recorded`. Two roads reach it:
  - `Runtime::add_to_profile`: the strip's *Add* press, through `attention.rs`'s
    notice arm;
  - `Runtime::spend_powershell_intent`: the first-run intent, spent from the pane
    notice pass in `runtime/panes.rs`, inside a turn.
- **Disposition: lane.** This is the storage lane, *serialized by affected
  resource*, and the resource is the marks record. The enable and removal halves
  are already on workers (`begin_enable`, `begin_removal`). The install half joins
  them on **one** serialized executor for the marks resource, and
  `install_recorded` runs in it unchanged (rule 9). The answer comes back through
  `profile_runtime::WAKE` and is applied between frames. Until the answer lands,
  the strip keeps its verb, as it already does after a failure. The wait behind
  our own writer becomes lane order, off the window thread.
- **D-33.** Depends. This is a new instance: FIFO per resource, no coalescing
  (§5.1: *do not coalesce commands because their results share a slot*).

### Row 3 · D-35 · `psreadline::apply_recorded`

- **Cost.** Not measured. It copies nine files, about 429 KB (§5.3), under the
  same lock, so row 2's worst case applies on top. *Estimate*: 5–50 ms on a warm
  SSD, and hundreds of milliseconds when antivirus scans each `.dll` and `.psd1`
  file as it lands. The ticket measures it through a station.
- **Door.** `Runtime::apply_psreadline` → `psreadline::apply_recorded`, from the
  Settings row and the invitation.
- **Disposition: lane**: row 2's executor, because it is the same lock. The toast
  is shown when the answer lands. 0.4.5 ticket 56 changes this road's *decision*
  but, by its brief, not its thread; this ticket is based after 56.
- **D-33.** Depends; same instance as row 2.

### Row 4 · D-36 · `psreadline::installed_copy`

- **Cost.** Not measured. It walks the module directory under `Documents`
  recursively. `Documents` can be redirected (OneDrive, a network home), where a
  directory read waits on hydration or the network. *Estimate*: under 1 ms locally,
  unbounded when redirected.
  - **A second wait of the same class, not on §5.3:** on the same edge,
    `Runtime::psreadline_documents` asks the known-folder API again when the last
    answer was `None`.
- **Door.** `Runtime::refresh_psreadline_installed` → `psreadline::refresh_installed`.
  It is called from the Settings page's open edge (`take_psreadline_open_edge`) and
  after each apply outcome.
- **Disposition: lane**, the observation lane, as a versioned request with
  latest-value replacement: the font slot's shape since ticket 50. The row draws
  the last adopted answer. The `Documents` re-ask joins the same request and is
  written into row 4. `psreadline::begin_probe` is already a one-shot probe and is
  unchanged.
- **D-33.** Depends. It is the second latest-value instance: the test of whether
  the contract fits more than one lane.

### Row 7 · D-39 · macOS locale children

- **Cost.** Not measured. It runs two children through `quiet_command`
  (`defaults read -g AppleLocale`, `locale -a`), once per process, inside
  `system_locale_declaration`'s `OnceLock`, on the first pane's birth. *Estimate*:
  10–50 ms each.
- **Door.** `bt_platform::system_locale_declaration`, from `shell_integration`'s
  child environment on the pane-birth road.
- **Disposition: lane, started before the loop.** Spawn the read at the top of
  `fn main` through `spawn_at_priority`, so it overlaps the making of the event
  loop and the first window.
  - **Hazard:** `OnceLock::get_or_init`, called while another thread is
    initialising the lock, waits for that thread. So the window thread reads only
    with `OnceLock::get()`, and never calls `get_or_init` inside a turn.
  - The first pane is born on the startup road, before the first frame. If the
    answer has not arrived by then, the ticket measures how long it would wait,
    then chooses between two options: a bounded wait on the startup road (row 18
    is the precedent: no turn exists yet), or a shell born without the
    declaration. *Estimate*: the answer arrives first. The Mac mini confirms or
    refutes this.
- **D-33.** Weak: this is a one-shot probe of D-3's kind. It records its exception
  (no request identity, one answer per process).

### Row 8 · D-40 · macOS `DirWatch` start and drop

- **Cost.** Not measured. `DirWatch::start_scoped` runs `fs::canonicalize` and
  `fs::metadata` on the calling thread, spawns `bt-dir-watch`, and blocks on
  `listening.recv()` until the stream is running. `Drop` signals, then `join()`s
  with no bound. *Estimate*: 1–5 ms on local APFS. **Unbounded** on a stalled
  network volume, because `canonicalize` and the join have no bound.
  - `bt-dir-watch` is also a bare `std::thread::Builder` at inherited priority: a
    second bypass of the thread door, beside `folio-web-thumb`.
- **Door.** `DirWatch::{start, start_shallow, start_shallow_named}` and its
  `Drop`, reached from the watch clocks under `Station::Watches` in
  `Runtime::turn`.
- **Disposition: lane**, in two halves:
  - **Start.** `canonicalize`, `metadata` and the stream start move onto the
    watcher thread. `start_scoped` returns at once with a watch in an *arming*
    state. The `armed` word (or the refusal) arrives through the watch's own wake:
    §5.1's mechanism (2), since this is `bt-platform`.
  - **Drop.** Signal, then hand the `JoinHandle` to a reaper that joins it off the
    window thread, with a retirement bound and a count at exit. Whether the reaper
    may abandon a thread past that bound is open question 5.
  - The thread goes through `spawn_at_priority`.
- **D-33.** Weak: mechanism (2), and it must not name `AppEvent`. The adapter is
  optional, and the ticket states the exception.

### Row 9 · D-41 · presentation

- **Cost. Measured** (ticket 52's evidence). next89 (`f5e321ca`) and next90
  (`40fda278`) recorded 161 window-thread holds. 68 of them name
  `swapchain present` at 300 ms or more: typically 1–4 s, the largest 14,220 ms.
  Thread CPU in each was 0–31 ms, so the thread waits; it does not work.
- **Door.** `Runtime::present_seats_and_commit` → `WindowRenderer::present_frame*`.
  Stations: `SurfaceConfigure`, `SurfaceAcquire`, `QueueSubmit`,
  `SwapchainPresent`, `CompositorSize`, `CompositorCommit`.
- **Disposition: the owner's decision** (§4).
  - **If built:** a lane on Windows (the reviewed note's steps (i)–(v)). The
    residue (configure, GPU preparation, the compositor's size and commit) becomes
    a split row at **bound**.
  - **If not built:** row 9 is **bound** at its measured distribution, far past
    `WAIT_ALLOWANCE`, and every stall will print a budget line saying so.
- **D-33.** The reviewed note makes presentation the contract's second client; its
  step (i) *is* D-33.

### Row 10 · D-42 · device recovery

- **Cost.** Not measured, and rare: once per device-loss episode (a driver update,
  a TDR). By the code:
  - `DeviceLossPilot::answer(&mut machine, std::thread::sleep)` rests 150 ms and
    then 450 ms across three attempts;
  - each attempt runs `pollster::block_on(rebuild_after_device_loss)`, whose
    adapter and device requests are *estimated* at hundreds of milliseconds.

  *Estimate*: 1–2 s of held window thread per episode.
- **Door.** `FolioApp::fail` → `recovered_from_a_lost_device` →
  `TheDeviceAndItsWindows::rebuild` → `GpuContext::rebuild_after_device_loss`.
- **Disposition: lane plus deadlines.**
  - The pilot already takes `sleep` as a parameter, so its rests become deadlines
    in the wake fold: a state machine, `Recovering { attempt, rest_until }`,
    stepped by the turn.
  - Adapter and device creation run on a worker, one request per attempt,
    identified by the device epoch the reviewed note defines (`GpuContext::epoch`).
  - Surfaces are recreated on the window thread when the answer lands.
  - No frame is admitted while recovering.
- **D-33.** Depends: a request-per-attempt lane with epoch identity. It does
  **not** depend on D-41. With a synchronous presenter no lease is ever out, so the
  reviewed note's barrier (§2.7) reduces to "no frame admitted while recovering".
  If D-41 is built later, it inherits this machine; this machine does not wait for
  D-41.

### Rows 11 and 12 · D-43 and D-44 · PTY birth and resize

Deferred to 0.5 toward 0.6 by the owner's ruling: they need D-1's session owner
to keep input and resize order. Under the budget they are list lines with
disposition `lane (deferred → D-43/D-44)`, each with a station (`PtyResize`
exists; B1 adds one for PTY birth). They are accounted, so their cost stays
visible until they move.

### Row 20 · D-47 · renames, the preserving save, store writes

- **Cost.** Not measured. Every piece already has a station: `SettingsWrite`,
  `KeybindingsWrite`, `ProfilesWrite`, `PreviewSave`, `RenameDisk`,
  `DiagnosticWrite`. So next92's stall lines show any that passed 500 ms, and B2
  will show any that pass 4 ms. *Estimates*:
  - a store write (a small JSON file, written atomically): 1–10 ms locally;
  - saving the preview of a large document: tens of milliseconds;
  - either one on a network share or a slow USB volume: unbounded.
- **Door.**
  - Store writes: `persist.rs`'s store methods and `runtime/configuration.rs`.
  - Documents: `Runtime::save_preview_on` (`buffer.save()`),
    `Runtime::rename_preview_file` and `Runtime::rename_files_row`.
  - Diagnostic writes: `runtime/frame.rs`, `runtime/keyboard.rs` and
    `runtime/preview.rs`.
- **Disposition: lane**, the storage lane, in two tickets:
  - **Store writes.** Serialized per file, and **latest value wins** for settings,
    keybindings and profiles. Each write is a whole snapshot, so replacement is
    permitted, unlike row 2's commands. The in-memory store stays the owner, and
    readers are unchanged. `SessionWriter` is the shape to follow.
  - **Documents.** A rename or preserving save carries the document revision it
    was made from as a precondition, and answers a **receipt** (`Saved { revision }`
    or `Refused { reason }`). The preview adopts the receipt only if its revision
    still matches, so "saved" can appear a turn or more later (open question 4).
    This touches D-4 (dirty edits on a controlled failure), D-53 (durability) and
    the quit road: an in-flight save is waited for under the rule
    `SESSION_SAVE_BUDGET` already follows, as row 16 waits for the session.
- **D-33.** Depends: the storage lane's second resource class (per file, per
  document).

### Row 21 · D-64 · a web page coming up

This row is 0.4.5's: ticket 54 moves the environment and the first controller to
an **idle** turn. Ticket 43 measured the rest headless: a later page costs about
3 ms synchronous, then one engine dispatch of about 50 ms on the pump. Under the
budget, that residue is native-affine (§5.2) and **bound** at its measured value.
That is above `WAIT_ALLOWANCE`, so it is the case open question 3 asks about.

### What the done rows leave, as budget lines

| row | residue | disposition | bound |
|---|---|---|---|
| 1 | nothing on the window thread | — | — |
| 5 | `monospace_family_named` (`FontLookup`) | bound | 2.6–3.4 ms cold, measured (ticket 50) |
| 6 | one `SEARCH_HISTORY_SLICE` per turn (`SearchScan`) | work, sliced | about 4 ms at 100,000 lines, measured (ticket 51) |
| 13 | `sample_window_place` and its four probes, once per turn | bound | not measured; next92 carries the stations |
| 14 | `Runtime::flush_title`, at most once a frame | native, bound | not measured; single writes on next89/next90 reached 2,206 ms |

## 3. D-33: one lane contract

The reviewed presentation note (§1) has already stated the obligations, and Codex
adopted them with changes (R6). This section adopts them unchanged and settles
the four points this note was asked about: identity, bounds, replacement and
cancellation.

**Where it lives: `bt-app::lane`.** `bt-render` and `bt-platform` supply
transports and never name `AppEvent` or `Pending` (Codex R6, and the crate-graph
fact of §5.1).

```rust
/// Minted by the lane's owner; unique within one incarnation of the lane.
pub(crate) struct RequestId { lane: LaneIncarnation, sequence: u64 }

pub(crate) enum Replacement {
    /// Every request is executed and answered, in order (hand-off, storage commands).
    Fifo,
    /// A newer request supersedes an unstarted older one; an answer older than
    /// the adopted one is dropped (fonts, the PSReadLine probe, store snapshots).
    LatestValue,
    /// One question, one answer, keyed by the question (path verify).
    PerQuestion,
}

pub(crate) struct Bounds {
    waiting: usize,     // admitted, not started
    executing: usize,   // in the executor
    answers_held: Held, // Bounded(n), or Unbounded as a recorded exception
    when_full: Full,    // what a full lane answers: never a wait
}

pub(crate) enum Cancellation {
    /// The call cannot be interrupted: the asker's going only drops the answer,
    /// refused by the target incarnation (OS calls, the disk).
    Abandon,
    /// A request not yet started is removed; a started one is abandoned.
    BeforeStart,
}

pub(crate) enum Outcome<A> { Answered(A), Refused(Refusal), Superseded, Lost, Abandoned }

pub(crate) trait Lane {
    const NAME: &'static str;
    const REPLACEMENT: Replacement;
    const BOUNDS: Bounds;
    const CANCELLATION: Cancellation;
    const RETURN: Return; // §5.1's (1) AppEvent, (2) park-and-wake, (3) slot
    type Target;          // the acceptance incarnation, e.g. Pending<Duty>
    type Answer;
}
```

**The trait only declares.** A trait made of types and constants cannot drive a
test (Codex R6). Conformance is the reviewed note's `LaneUnderTest` adapter
(`submit`, `hold`, `release`, `abandon`, `drain`, `events`), one per lane. One
suite, `lane_contract_tests`, runs the same four claims on every adapter:

1. a full lane answers without blocking;
2. an abandoned answer is not raised;
3. the wake follows the publication;
4. a dead worker leaves an observable terminal state.

A lane that fails a claim is listed with its exception and its own repair ticket.
It is never declared conformant to make the suite pass.

**Instances, from the base and Codex R6:**

| lane | replacement | bounds | cancellation | exception recorded |
|---|---|---|---|---|
| **OS hand-off** (`handoff_lane`): **the reference instance** | Fifo | 32 waiting + 1 executing; `LANE_FULL` | Abandon (`Pending` refuses a stale answer) | the answer channel and `turned_away` are unbounded; `LANE_GONE` answers new submissions only |
| **font** (`settings::MonospaceFamilySlot`) | LatestValue | 1 executing, 1 answer held | BeforeStart (the `again` round) | numbered since ticket 50, in its own slot; `take_offer` is a short mutex swap (allowed: a value swap) |
| **observation** (`run_path_verify_worker`; the PSReadLine probe after B5) | PerQuestion | unbounded channel | Abandon | path verify has no request id, so two questions in one shell incarnation cannot be told apart |
| **computation** (`MathWorker`: math, image scaling) | PerQuestion | its queue | Abandon | three jobs share one result type (D-2's evidence); recorded, not split |
| **storage** (new: rows 2, 3 and 20) | Fifo per resource for commands; LatestValue per file for snapshots | stated per resource | BeforeStart for snapshots, Abandon for commands | none by design |
| **presentation** (if built) | one attempt in flight + the newest waiting | 1 + 1 per window | abort at every stage | none by design (reviewed note §2) |

**Why the hand-off lane is the reference.** It is the only lane on the base that
already has all six obligations in code: an id, an incarnation, a bound, an order,
a set of completions and a wake. Wrapping it changes no behaviour, so the suite's
first green run proves the harness rather than a repair.

## 4. The presentation lane: the decision for the owner

> On 2026-09-24 the owner deferred building the presentation lane (D-41) until
> the self-inflicted waits were fixed and measured. next92 carries those fixes
> (tickets 43, 48, 49, 50 and 51). The decision is whether the holds that remain
> are the GPU and driver waiting on the window thread, which only moving acquire,
> submit and present off that thread removes, or something else. **Evidence that
> D-41 is needed** is next92's `diagnostics.log`, over a session as long as
> next89/next90's, still showing `held control for` lines whose longest exclusive
> station is `swapchain present`, `surface acquire` or `queue submit`
> (`outcome=in_progress:present`, `:acquire` or `:submit`). Those lines must show
> thread CPU and page faults near zero (a wait: not work, not paging), and no
> `sample_window_place`, `Window::set_title`, `font family lookup` or search scan
> over threshold in the same line (the fixed waits are not riding along), at a
> rate comparable to the 68 in 161 seen before. **Evidence that it is not** is any
> of the following: present-led holds largely gone on next92 (they were induced by
> the waits now fixed); remaining holds led by `surface configure`, which the
> reviewed design keeps on the window thread; holds led by `message pump` or
> `request_controller` (row 21, ticket 54); holds whose thread CPU is close to
> their wall time (work); holds with high fault counts (memory pressure, which no
> lane fixes); or present-led holds that coincide to the second with the
> machine-wide stall beat, where a lane would keep keys alive but the picture
> would still freeze (the reviewed note's ruling Q1). The first cut also moves
> nothing on macOS. Classify each line by its longest exclusive station, not its
> parent. The decision is the owner's.

## 5. Risks, non-goals, and the guard

### 5.1 The guard's exact shape

The one fact, which waits may run on the window thread, is kept in two places:
the code and the table. So the guard has two parts. A guard that reads only one
of the two goes green by reading nothing.

**Part A, the code: a Rust test through `bt_source`.**
`crates/bt-app/src/window_waits_tests.rs`, test
`every_wait_the_window_thread_makes_is_on_the_list`, modelled on
`handoff_lane::tests::no_handoff_runs_on_the_window_thread`. It reads three
sections of `window_waits.tsv`:

- **`# vocabulary`**: one path per line, each with its reason.
  - The std primitives that wait: `std::fs::{rename, write, copy, canonicalize,
    metadata, read_dir, create_dir_all}`, `File::create`, `std::thread::sleep`,
    `JoinHandle::join`, `Receiver::{recv, recv_timeout}`,
    `Command::{output, status}`, `pollster::block_on`.
  - The cross-crate entrances that wait inside: `WindowRenderer::present_frame*`,
    `configure_window_surface`, `PtySession::spawn_shell_in`, `PtySession::resize`,
    `DirWatch::start*`, `bt_platform::system_locale_declaration`,
    `monospace_family_named`, `sample_window_place`, `profile_marks::lock`,
    `psreadline::apply_recorded`, `psreadline::installed_copy`.
- **`# lanes`**: the functions whose calls run on a worker
  (`handoff_lane::run_handoff_lane`, `settings::scan_monospace_families`,
  `main::run_path_verify_worker`, …). Each one is pinned: it either contains a
  `spawn_at_priority` call, or a spawn names it as the worker body.
- **The list**: one line per (row, door, owner, count, station, disposition,
  ruling).

**Its universe** is bt-app's product code (`Index::of_package("bt-app")`,
`in_the_product`). The test states that universe and compares the enumerated file
set before and after with `bt_source`'s `FileSetDiff` (split-prep review S-2:
every reader names its universe).

For every vocabulary path, in both spellings (the full path, and a `use`-imported
name), it asserts:

1. no occurrence outside an item, so a bare call cannot hide behind an import (the
   hand-off test's `outside_items`);
2. every owner is a lane body or a list line, **with the exact count**
   (split-prep review S-3: identity and multiplicity, never a total);
3. every list line is matched exactly, so a stale line goes red and the list
   cannot keep a row the code has lost;
4. every list owner's body names `Station::<its station>`;
5. every vocabulary path occurs at least once in the workspace. A misspelt entry
   would otherwise read nothing and pass: this is the hand-off test's "the gate is
   reading nothing" assert.

**Part B, the table: `scripts/ci/check-window-waits.ps1`**, modelled on
`check-migration-debt.ps1`.

- It compares the list section with the merge base's, **whole rows, with
  multiplicity**.
- An added or widened line is red unless its `ruling` column names a
  `### 2026-…` heading of `docs/DESIGN.md` that is absent at the merge base.
- It holds §5.3's open rows and the list's rows equal, by row number.
- It **fails** when there is no merge base, as the debt script does: a comparison
  that did not run must not pass.

**Why it cannot quietly go green.** These answer the split-prep review's three
blockers (S-1 to S-3) of 2026-09-21:

- **Literals (S-1).** The test matches identifiers and paths
  (`View::Identifiers`), never text with the literals stripped. A path spelled
  inside a string does not count.
- **Universe (S-2).** It is stated, and checked by the file-set diff.
- **Ownership (S-3).** Each line records owner and count, and acceptance runs a
  **full mutation** set. Each of these must turn the guard red:
  - a vocabulary call added to a `Runtime` method;
  - a listed call moved to another owner, keeping the total;
  - a listed call deleted;
  - a vocabulary entry misspelt;
  - a station dropped;
  - a list line added without a ruling.
- **Proof that it fires.** `gates-can-fail` in `.github/workflows/ci.yml` plants a
  `std::fs::rename` in a `Runtime` method and a line without a ruling in the list,
  and requires each part to go red. This is the job's existing pattern for the
  adapter boundary and the debt list.

### 5.2 Risks

- **The vocabulary cannot be complete.** A new cross-crate function that waits
  inside is invisible to Part A until it is added. The backstop is the run-time
  accounting: its time lands on the enclosing station and shows in a budget line.
  That is the honest limit of a static guard, and the test's doc comment says so.
- **Lane bodies are declared, not proven.** A declared lane body that also does
  window-thread work hides its waits. The pin (it spawns, or a spawn names it)
  narrows this without closing it.
- **Asynchronous saves change when "saved" is true** (row 20). A preview that
  shows "saved" only on the receipt, and a quit that waits for in-flight saves,
  are new trigger sources of durable facts (D-4, D-53). The documents half needs
  its own design note before dispatch.
- **Station headroom.** 48 stations are left. B1 adds about six; splitting rows
  and D-41's lane record would take more. Widening the byte is a change to the
  `hang_watch_detail` key, and outside this note.
- **Clock change.** Microseconds in `hang_watch` touch every `spent_ms` reader.
  The slow-hold line keeps its millisecond text, because its readers grep it. Only
  the new budget line prints microseconds.
- **Ticket 56 (0.4.5)** edits the PSReadLine install decision, so B4 is based
  after it.

### 5.3 Non-goals

- Rows 11 and 12 stay deferred: listed and accounted, not moved.
- No keyboard thread, and no second UI thread for WebView2 (§5.2 and the
  2026-09-24 ruling stand).
- No runtime refusal or fallback. The budget reports; it does not act.
- D-41 is not decided here, and no 0.4.6 ticket assumes it is built.
- The ten one-shot probes (D-3) are not folded into `bt-app::lane` beyond
  recording their exceptions.

## 6. Tickets (0.4.6)

The ids **B1–B9** are provisional; the coordinator assigns the real numbers.
Every ticket carries `_standing-rules.md`. Every ticket **ends at "committed, CI
green on the branch"**: 0.4.6 code merges after the 0.4.5 tag (owner ruling
2026-09-25).

Order: B1 → B2 → B3, then B4–B9 in any order, each after B3. Each is mergeable on
its own.

### B1 — The window thread's waits are one list the build checks · M

- **Who.** Opus, on the local lane with name-filtered tests. The guard's CI half
  needs one pushed run.
- **True on BASE.**
  - §5.3 is prose, and no test or script reads it.
  - Rows 2, 3, 4, 7 and 10 have no station.
  - §5.3 gives versions 0.4.4 and 0.5 where the ledger gives 0.4.6 (and 0.4.5 for
    D-41).
- **Goal.**
  - `crates/bt-app/src/window_waits.tsv` (vocabulary, lanes, list), Part A's test,
    Part B's script and its `gates-can-fail` plant.
  - Stations for the rows without one, appended, with `STATION_COUNT` widened:
    `MarksInstall`, `PsReadLineApply`, `PsReadLineProbe`, `LocaleProbe`,
    `DeviceRecovery`, `PtyBirth`.
  - §5.3 rewritten as the budget (§1.7 above) plus the table, with its versions
    aligned to the ledger.
- **Design.** §5.1 above. The list is seeded from the code as it stands, and the
  report gives the seed count. Every line has a §5.3 row: a wait found with no row
  becomes a new §5.3 row and a new ledger row, in the same commit.
- **Tests red on BASE.**
  - `every_wait_the_window_thread_makes_is_on_the_list`;
  - `the_window_wait_list_and_the_architecture_table_name_the_same_rows` (in the
    script);
  - the full mutation set of §5.1.
- **Docs in the same commit.** ARCHITECTURE §5.3 and §5.4; a DESIGN entry, *The
  window thread's waits are one list the build checks*; RULES row 53's **Open**
  sentence points at the budget; the ledger's D-2 status. CHANGELOG: nothing a
  reader sees.
- **Architecture impact.**
  - (a) The admission table: a new fact, owned by `window_waits.tsv` and mirrored
    by §5.3. The heartbeat's station gains six writers.
  - (b) No new doors; the guard *reads* every door.
  - (c) Pays part of D-2's "a list, not a budget"; B2 pays the rest. Adds a row
    for each unlisted wait the seed finds.
  - (c′) New stations change where time prints. A reader who greps `window_event`
    for a PSReadLine press must now also grep the new labels.
  - (d) No.

### B2 — A turn's waits are accounted against the budget · S–M

- **Who.** Opus, local lane.
- **True on BASE.** `hang_watch` counts in milliseconds, records nothing below
  500 ms, and has no kind per station (§1.5).
- **Goal.**
  - `Station::spend() -> Spend { Work, Wait(row), Native(row), Scope }`,
    exhaustive with no default arm.
  - A microsecond clock beside the millisecond one, and a per-turn sum of waits.
  - A budget line (row, station, µs, bound), queued the way slow holds are.
  - A per-run summary at exit: maximum, count and sum per wait station.
  - `TURN_BUDGET`, `WAIT_BUDGET` and `WAIT_ALLOWANCE` named in `hang_watch`.
- **Tests red on BASE.**
  - `a_turn_whose_waits_exceed_the_wait_budget_writes_one_budget_line`;
  - `a_wait_under_its_bound_writes_nothing`;
  - `every_station_has_a_spend_kind_and_every_wait_names_a_listed_row` (checked
    against B1's list);
  - `the_slow_hold_line_keeps_its_shape`.
  - Mutations: drop the sum; add a default arm to `spend`.
- **Docs.** A DESIGN entry; one sentence in ARCHITECTURE §10; the new line's
  format in `docs/BT-ENVIRONMENT.md`.
- **Architecture impact.**
  - (a) The heartbeat's accounting: the window thread writes it, the watchdog
    formats it.
  - (b) None.
  - (c) With B1, repays D-2's shape. The rows stay their own.
  - (c′) A new diagnostics line; its readers are the owner's log greps.
  - (d) No.

### B3 — One lane contract, with the hand-off lane as its reference · M

- **Who.** Opus, local lane.
- **True on BASE.** Each lane states its policy in its own module doc. The font
  slot numbers its requests in its own slot (ticket 50). There is no shared type.
- **Goal.** `bt-app::lane` (§3), the `LaneUnderTest` adapters for hand-off, font
  and path verify, `lane_contract_tests`, and the recorded exceptions.
- **Tests red on BASE.**
  - `every_lane_answers_a_full_queue_without_waiting_and_raises_no_abandoned_answer`.
    Path verify's exception is recorded as expected-failing by name, **not** by
    `#[ignore]`.
  - `the_handoff_lane_behaves_exactly_as_before_behind_the_contract`.
- **Docs.** ARCHITECTURE §5.1 ("results come back three ways") gains its rule; a
  DESIGN entry; D-33's status on the ledger; new ledger rows for the hand-off and
  path-verify exceptions.
- **Architecture impact.**
  - (a) No owner changes; each lane's policy is declared.
  - (b) Threads unchanged.
  - (c) Advances D-33 without repaying it (the exceptions remain); adds the
    exception rows.
  - (c′) None.
  - (d) No.

### B4 — The marks record is written only on the storage lane · M · rows 2 and 3 · D-34, D-35

- **Who.** Opus, local lane, **after** B3 and after 0.4.5 ticket 56. It needs a
  one-page design note reviewed before dispatch: rule 11 applies, because the
  marks record's writer moves to another thread.
- **True on BASE.** `add_to_profile`, `spend_powershell_intent` and
  `apply_psreadline` take the marks lock on the window thread, and the lock's
  `InApp` queue has no deadline.
- **Goal.**
  - One serialized executor for the marks resource, carrying install, apply,
    enable and removal.
  - `install_recorded` and `apply_recorded` run in it unchanged (rule 9).
  - Answers land between frames. The strip and the toast show what they show
    today, when the answer lands.
- **Tests red on BASE.**
  - `a_press_on_add_to_profile_takes_no_lock_on_the_window_thread`
    (`bt_source`: `profile_marks::lock` has no owner outside the lane);
  - `an_install_queued_behind_a_removal_is_answered_after_it_in_order`;
  - `a_refused_install_leaves_the_strip_offering_the_same_verb`.
- **Docs.** §5.3 rows 2 and 3 marked done and their list lines removed; RULES row
  4; a DESIGN entry; the ledger's D-34 and D-35, and a note on D-53.
- **Architecture impact.**
  - (a) The marks record's writer moves to the lane. Its owner stays the file plus
    the lane's order.
  - (b) One storage executor through `spawn_at_priority`, `BelowNormal`.
  - (c) Repays D-34 and D-35.
  - (c′) The strip's `Offer::Added` and the PSReadLine toast change on the answer,
    not the press. Their readers are the notice pass and the toast queue.
  - (d) **Yes**, hence the design note.

### B5 — PSReadLine's presence is observed on a lane · S · row 4 · D-36

- **Who.** Opus, local lane, after B3.
- **Goal.** `refresh_psreadline_installed` and the `psreadline_documents` re-ask
  become one versioned LatestValue request, and the row draws the last answer.
- **Tests red on BASE.**
  - `opening_the_powershell_page_walks_no_directory_on_the_window_thread`;
  - `an_older_probe_answer_is_never_adopted`.
- **Docs.** §5.3 row 4 marked done, with the `Documents` re-ask added to it; a
  DESIGN entry; the ledger.
- **Architecture impact.**
  - (a) `psreadline_installed` gets one writer: the adopt.
  - (b) The observation lane.
  - (c) Repays D-36.
  - (c′) The row can lag the disk by one answer.
  - (d) No.

### B6 — The macOS locale is read before the loop · S · row 7 · D-39

- **Who.** Opus. The Mac mini check is mandatory.
- **Goal.** The read starts on a lane at the top of `fn main`. The window thread
  reads only with `OnceLock::get()`. The ticket measures and states what the
  startup road does when the answer is late (§2, row 7).
- **Tests red on BASE.** `no_turn_initialises_the_locale_declaration`
  (`bt_source`: the only owner of the `get_or_init` is the lane body).
- **Architecture impact.**
  - (a) The locale declaration's writer moves to the lane.
  - (b) One thread.
  - (c) Repays D-39.
  - (c′) None if the answer arrives first; stated if it does not.
  - (d) No.

### B7 — A macOS directory watch starts and retires without the window thread waiting · S–M · row 8 · D-40

- **Who.** Opus. The Mac mini check is mandatory.
- **Goal.**
  - `start_scoped` returns an arming watch. The root is resolved and the stream
    started on the watcher thread, and `armed` or the refusal arrives by the wake.
  - The drop hands the handle to a reaper.
  - `bt-dir-watch` goes through `spawn_at_priority`.
- **Tests red on BASE.**
  - `starting_a_watch_on_an_unanswering_root_returns_at_once` (using the existing
    `Held` gate in `macos_watch`'s tests);
  - `dropping_a_watch_joins_nothing_on_the_calling_thread`.
- **Architecture impact.**
  - (a) A watch gains an *arming* state. Its readers are the files column,
    `preview_watch` and the watch clocks.
  - (b) The thread door: one bypass repaired.
  - (c) Repays D-40.
  - (c′) A refusal now arrives by wake.
  - (d) No.

### B8 — Settings, keybindings and profiles are written on the storage lane · S–M · row 20, part one · D-47

- **Who.** Opus, local lane, after B3.
- **Goal.** A per-file LatestValue executor; the in-memory store stays the owner.
  Diagnostic writes join the existing trace queue where they are not on it
  already.
- **Tests red on BASE.**
  - `a_settings_change_writes_no_file_on_the_window_thread`;
  - `two_quick_changes_leave_the_newer_on_disk`;
  - `quit_waits_for_the_last_write_within_its_budget`.
- **Architecture impact.**
  - (a) The writer of the stores' on-disk copies moves.
  - (b) A thread.
  - (c) Part of D-47, and a note on D-53.
  - (c′) The disk lags memory by one answer.
  - (d) No.

### B9 — Device recovery rests on deadlines and rebuilds on a worker · M · row 10 · D-42

- **Who.** Opus, local lane, after B3. It touches `bt-render`, so the Mac mini
  check applies. It needs a one-page design note: a worker holds the device while
  it is being created.
- **Goal.**
  - `Recovering { attempt, rest_until }`, stepped by the turn.
  - Adapter and device creation as one request per attempt, carrying a device
    epoch.
  - Surfaces recreated on the owner.
  - No frame admitted while recovering.
- **Tests red on BASE.**
  - `a_lost_device_never_sleeps_on_the_window_thread` (the pilot with a recording
    clock);
  - `a_stale_rebuild_answer_is_refused_by_its_epoch`;
  - `frames_are_not_admitted_while_the_device_is_recovering`.
- **Architecture impact.**
  - (a) `GpuContext` gains `epoch`. The device is created off the thread and
    adopted on its owner.
  - (b) A thread.
  - (c) Repays D-42.
  - (c′) Recovery spans turns. Its readers are frame admission,
    `present_diagnostics` and `fail`.
  - (d) **Yes**, hence the design note.

**Not ticketed here:**

- row 20's document half (renames and the preserving save), which waits for open
  question 4 and its own design note;
- D-41 (§4);
- rows 11 and 12 (deferred).

## 7. Open questions for the owner

1. **When is D-2 repaid?** When B1 and B2 land (the list becomes a checked budget,
   and each row keeps its own ID), or only when every §5.3 row has closed, as the
   ledger says today? The second keeps D-2 open through D-43 and D-44 to 0.6.
2. **Do the allowances scale with the display?** Either fixed at 16, 8 and 4 ms
   from the 60 Hz default, as `DRAIN_TURN_BUDGET` is today, or fractions of each
   window's `FrameClock::interval`, which halves them at 120 Hz.
3. **May a native-affine call keep a bound above `WAIT_ALLOWANCE` by ruling?**
   Examples: a later web page's engine dispatch of about 50 ms; a single
   `set_title` write. Or must every such call move to an idle turn or be split?
4. **May a preview save show "saved" a turn later**, when the storage lane's
   receipt lands, with quit waiting for in-flight saves within the session-save
   budget? The answer decides row 20's document half.
5. **May a macOS directory watcher that will not stop be abandoned to process
   exit** after a retirement bound, as ruling Q4 allows for an unreturned present?
   Or must retirement always join?
