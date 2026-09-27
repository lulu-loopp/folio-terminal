# The thread door lends a capability: `spawn_at_priority`'s new contract, and ticket A1 — design note, 2026-09-26

Status: **draft, 2026-09-26**, for Codex review before A1 is dispatched. It is
the prerequisite `docs/plans/design/window-thread-budget-2026-09-25.md` revision
(c) §C-8 names for A1: *"a reviewed design note for `spawn_at_priority`'s new
closure signature (rule 11: the thread door's contract changes)"*. Read at
`main` = `78a3699a`. No code, no cargo run; anchors are names, never lines.

**Section signs.** A bare `§n` is a section of this note; `B§x` is a section of
the budget note (`B§C-5`, `B§R-A`); `A§n` is a section of `docs/ARCHITECTURE.md`;
`RULES n` is a row of `docs/RULES.md`; `CONV rule n` is a rule of
`docs/CONVENTIONS.md` §十.

**Why this is a rule-11 note.** `spawn_at_priority` is a door (A§6, RULES 52).
Today it promises one thing to a closure: *you run in your band*. After A1 it
also decides a fact no code owns today — **which kind of thread this is** — and
hands the closure a value that proves it. The fact is new, its owner is new
(`bt_platform::admission`), and every spawn site's contract with the door
changes. That is an ownership change in rule 11's sense, and §13 states it in
the ticket fields' terms.

**What this note is not.** It does not re-open B§C-1…B§C-7. Where it departs from
revision (c), §7 lists the departure, the evidence and the reason, so the review
can accept or refuse each one by number.

---

## 0. In one page

- **The signature.** `spawn_at_priority(name: &'static str, band, body)` and
  `spawn_at_priority_with_stack(name, band, stack, body)` take
  `body: FnOnce(&WorkerCtx) -> T + Send + 'static`. The door moves into
  `bt_platform::admission`, one definition instead of today's two copies (the
  Windows arm and the portable arm), re-exported at the crate root under the
  same paths. Inside the new thread, in this order: the band (still the first
  statement, RULES 53), the role `Worker(name)`, then a `WorkerCtx` built on the
  thread's own stack and lent to `body`. `WorkerCtx` has private fields, is
  `!Send` and `!Sync`, has no `Clone`, `Default` or public constructor, and is
  built in exactly one place (§2).
- **The role** is a thread-local `Cell<Role>` private to `admission`
  (`Unset | Window | Worker(&'static str) | Callback(&'static str)`), written by
  two permanent entries — the thread door and `enter_window_thread` — and by one
  scoped entry, `enter_callback`, whose guard restores `Unset`. `Unset` is never
  a worker and never the window. §3 classifies every thread that runs
  first-party code, 46 spawn sites plus the OS-owned callback threads.
- **The phase** lives with the window thread, not the process: a thread-local
  meaningful only where the role is `Window`. It has three values and **four**
  writers, not three: the code has a road *back* from `Exiting` (a quit whose
  session write is refused is abandoned and the application carries on), and
  the way out has three roads, not one (§4).
- **The census** (§5): **46** spawn sites in product code, not the 45 of A§0.1 —
  `taskbar-state` (ticket 62, `29f3842e`) was added after that count. **24** go
  through the door, all in `bt-app`; **22** are bare (`bt-app` 6, `bt-platform`
  12, `bt-pty` 4); plus the one rayon pool in `bt-term`.
- **The migration** (§6): the signature change and all 24 sites in **one**
  commit, because any split needs a second spawner for the interval, which is a
  second entrance to the door (RULES 52). One site gains a real use at once:
  the hand-off lane (`ShellThread::enter` takes `&WorkerCtx`). The other 23 add
  `_ctx`. The bare threads come in afterwards, by theme, at their current band.
- **Seven departures from revision (c)** (§7), each checked in the code. The two
  that change A1's shape: `bt-pty` does not depend on `bt-platform`, so B§C-5's
  premise is false and rows 11–12's owner doors must live in `bt-app`; and the
  meter must have an *enter* half, or a call that never returns is invisible to
  the watchdog's station stack, which is what `hang_watch::during` gives today.
- **A1 is five tickets** (§11): A1a the admission module and the registry; A1b the
  thread door; A1c the bare threads; A1d the owner-thread doors; A1e the source
  guard's prohibitions. S or M each; each lands alone with a true intermediate
  architecture.
- **Two questions for the owner** (§12): whether door processes' threads owe the
  thread door, and whether the video engine and the two endpoints are "workers"
  under RULES 53's band.

---

## 1. The thread door as it stands on `78a3699a`

`bt_platform::spawn_at_priority` and `spawn_at_priority_with_stack` are defined
**twice**, once in the Windows arm of `crates/bt-platform/src/lib.rs` and once
in `portable_priority`, with identical bodies:

```rust
pub fn spawn_at_priority_with_stack<T: Send + 'static>(
    name: &str,
    priority: ThreadPriority,
    stack_bytes: Option<usize>,
    body: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<T>> {
    let mut builder = std::thread::Builder::new().name(name.to_owned());
    if let Some(bytes) = stack_bytes { builder = builder.stack_size(bytes); }
    builder.spawn(move || {
        set_current_thread_priority(priority);
        body()
    })
}
```

The only platform difference is `set_current_thread_priority`, which is already
its own safe function in each arm (the Windows one wraps `SetThreadPriority` in
`unsafe`; the portable one returns `false`). `spawn_at_priority` itself contains
no `unsafe`.

What the door promises today: a named thread whose first statement puts it in
its band. What it does not know: what kind of thread it made. Nothing in the
process records that `bt-os-handoff` is a worker, that the thread running
`fn main` is the window thread, or that a Media Foundation work-queue thread
running `IMFMediaEngineNotify::EventNotify` is neither.

Two precedents in the tree already carry thread identity as a value:

- **`bt_platform::ShellThread`** (`handoff.rs`): a `!Send` value created by
  `ShellThread::enter()` on the lane thread; its `hand_over` is the only road
  to the seven hand-off verbs from `bt-app`. It proves "this thread entered its
  COM apartment", not "this thread is a worker", and anyone can call `enter()`.
- **objc2's `MainThreadMarker`**, which the macOS code takes wherever AppKit
  requires the main thread. It is minted by a runtime check (`new()` returns
  `None` off the main thread).

`WorkerCtx` is the first shape with the second property the precedents lack:
**no constructor outside the door**.

---

## 2. The new signatures (question 1)

### 2.1 The code

```rust
// crates/bt-platform/src/admission.rs
#![forbid(unsafe_code)]

/// Lent by the thread door to the body of every thread it starts, and to nothing else.
pub struct WorkerCtx {
    name: &'static str,
    _local: PhantomData<*const ()>, // !Send, !Sync
}

impl WorkerCtx {
    /// The name the thread was started with.
    pub fn name(&self) -> &'static str { self.name }
}

pub fn spawn_at_priority<F, T>(
    name: &'static str,
    band: ThreadPriority,
    body: F,
) -> io::Result<JoinHandle<T>>
where
    F: FnOnce(&WorkerCtx) -> T + Send + 'static,
    T: Send + 'static,
{
    spawn_at_priority_with_stack(name, band, None, body)
}

pub fn spawn_at_priority_with_stack<F, T>(
    name: &'static str,
    band: ThreadPriority,
    stack_bytes: Option<usize>,
    body: F,
) -> io::Result<JoinHandle<T>>
where
    F: FnOnce(&WorkerCtx) -> T + Send + 'static,
    T: Send + 'static,
{
    let mut builder = std::thread::Builder::new().name(name.to_owned());
    if let Some(bytes) = stack_bytes {
        builder = builder.stack_size(bytes);
    }
    builder.spawn(move || {
        crate::set_current_thread_priority(band); // first statement, RULES 53
        ROLE.with(|role| role.set(Role::Worker(name)));
        let ctx = WorkerCtx { name, _local: PhantomData };
        body(&ctx)
    })
}
```

`lib.rs` re-exports both at the crate root, so every call site keeps the path
`bt_platform::spawn_at_priority`. The two duplicated arms are deleted; each arm
keeps only `set_current_thread_priority` and `current_thread_priority`.

### 2.2 Where the `WorkerCtx` is made, and why there

**Inside the new thread's running closure, after the band and the role.** Three
reasons, each a property the review can test:

1. **It proves the thread, not the spawner.** A `WorkerCtx` made by the spawner
   and moved in would prove only that *some* thread called the door. Made on the
   new thread's stack, it exists only on a thread the door started.
2. **Band first.** RULES 53 rules the band as the first statement "because
   Windows hands a new thread `Normal` whatever its creator stands in". The role
   write and the construction are two register writes after it; they do not
   move the band.
3. **Role and capability cannot disagree.** The role is set and the
   capability built in consecutive statements of one function, with the same
   `name`. There is no state in which a thread holds a `WorkerCtx` and its role
   is not `Worker`, or the reverse.

### 2.3 What it carries

- `name: &'static str` — the thread's name. The signature narrows `name` from
  `&str` to `&'static str`; all 24 product sites pass a literal or a `const`
  (`palette_index::INDEX_WORKER_THREAD`), so no site changes its argument.
  `name()` exists for diagnostics a door may print (the refusal counters, §3.4).
- `_local: PhantomData<*const ()>` — makes the type `!Send` and `!Sync`.
  Consequences, all by the compiler: a `WorkerCtx` cannot be sent to another
  thread; `&WorkerCtx` cannot be sent either (`&T: Send` needs `T: Sync`); a
  closure capturing `&WorkerCtx` cannot be handed to `std::thread::spawn`,
  `std::thread::scope`, a rayon pool or the thread door itself.
- Nothing else. It does not carry the band (the thread already has it), a lane
  identity (a lane is not a thread; B§R-D's adapters own lane identity) or a
  generation.

It is **lent by reference**. `F: FnOnce(&WorkerCtx) -> T` is higher-ranked over
the reference's lifetime and `T` is chosen outside it, so neither the reference
nor anything borrowing it can be returned from `body` or stored in a `'static`
place. The value itself lives on the spawn closure's frame and dies when `body`
returns.

### 2.4 Why no other constructor exists

A worker-only door's signature takes `&WorkerCtx`. The window thread has none,
so a window-thread call does not compile (M3). That guarantee is only as strong
as the claim "a `WorkerCtx` exists only on a thread the door started", so every
other road to one is closed, each by a named mechanism:

| road | closed by |
|---|---|
| a struct literal outside `admission` | private fields |
| a struct literal inside `admission` | the source guard counts `WorkerCtx {` in product code: exactly one, in `spawn_at_priority_with_stack` (A1e) |
| `Default`, `Clone`, `Copy`, `From`, a public `new` | not derived, not implemented; the guard refuses an `impl … for WorkerCtx` other than the inherent one holding `name()` (A1e) |
| moving one from a worker to the window thread | `!Send` (M3r) |
| lending one to another thread | `!Sync` (M3r) |
| `unsafe` fabrication (`transmute`, `zeroed`, `MaybeUninit`, a pointer `read`) | `#![forbid(unsafe_code)]` on `admission` (compiler-enforced; a `#[allow]` below a `forbid` is itself an error) and B§C-5's fence on naming the type inside `unsafe` anywhere else (A1e, M14) |
| a test double | none exists; a test gets a `WorkerCtx` only by starting a thread through the door (CONV rule 4: the real producer) |

The `forbid` is a strengthening of B§C-5, which fenced `admission` by the source
guard alone. `bt-platform`'s other modules keep their `unsafe`; the workspace's
`unsafe_code = "deny"` is lowered for `bt-platform` exactly as today.

### 2.5 The worker-only door's shape

```rust
impl ShellThread {
    pub fn enter(ctx: &WorkerCtx) -> Self { … }       // was: enter()
}
```

A door takes `&WorkerCtx` and may ignore it after the signature has done its
work. **`expect_worker()` is not added** — see §7, departure 5.

---

## 3. The role (question 2)

### 3.1 The type and its storage

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Unset,
    Window,
    Worker(&'static str),
    Callback(&'static str),
}

thread_local! {
    static ROLE: Cell<Role> = const { Cell::new(Role::Unset) };
}

pub fn role() -> Role { ROLE.with(Cell::get) }        // read by anyone
```

A `const` thread-local: no lazy initialisation, no destructor, one load to read.
The cell is private to `admission`; only the three functions below write it.

### 3.2 The writers

| writer | sets | on a thread whose role is not `Unset` | where it is called |
|---|---|---|---|
| the thread door (`spawn_at_priority_with_stack`) | `Worker(name)`, permanently | cannot happen: the thread is new | every spawn through the door |
| `enter_window_thread()` | `Window`, and the phase `Starting` (§4) | changes nothing; counts a violation (§3.4) | product: **once**, in `fn main` directly after `cli::parse` succeeds — below the five argv doors, above `persist::is_writer_of` and `launch_wire::hand_over` (row 18 is a `Starting` wait). Pinned: one product caller (A1e) |
| `enter_callback(name)` → `CallbackScope` | `Callback(name)` until the scope drops, then `Unset` again | changes nothing, and its scope restores nothing | at the entry of each OS-owned callback in §3.3's table |

Two consequences stated so they can be tested:

- **The "window thread" is the front-door thread of a launch that got past the
  argument parse**, including a launch that then hands its request to a running
  Folio and leaves. That is what row 18 needs (`hand_over` is a window-thread
  wait in `Starting`), and it matches A§5.3's wording ("`fn main`, before the
  loop exists").
- **Door processes never enter `Window`.** `--uninstall-cleanup`, the
  `attention` verb, `--explorer-command`, `--remove-shell-integration` and
  `--remove-explorer-menu` all leave `fn main` above the parse, so their main
  threads are `Unset`. Whether their *spawned* threads owe the door is
  question 1 of §12.

**`enter_window_thread` is once per thread, not once per process.** B§C-5 says
it "can be called only once"; a process-wide `OnceLock` would do that, but the test harness runs each test on a thread of its own,
and tests that drive a `Runtime` must enter `Window` on that thread. Its product
caller is pinned to one by the source guard, and it cannot turn a worker or a
callback into the window thread (the refusal column). The review should weigh
this against a once-per-process `OnceLock`, which would forbid every such test.

**`enter_callback` on a thread with a role keeps the role.** A callback that the
system happens to deliver on the window thread (AppKit's main queue, WebView2's
UI-thread events) *is* running on the window thread, and `Window` is the true
answer there. Only a thread the process did not start and cannot otherwise
name becomes `Callback`.

### 3.3 Every thread that runs first-party code, classified

**By entry.** The 46 spawn sites are in §5; this table gives the role each
kind gets, and every OS-owned thread that calls into first-party code.
"Doors reached" means the doors of B§R-A's table the code on that thread
reaches today.

| thread | whose | how first-party code gets on it | role after A1 | doors reached |
|---|---|---|---|---|
| `fn main`'s thread in the window process | ours | it is `fn main` | `Window` (A1a) | the owner-thread doors (A1d) |
| the 24 door-spawned threads (§5 table A) | ours | the thread door | `Worker(name)` (A1b) | §5 table A |
| `bt-platform`'s 12 bare threads: `bt-dir-watch` ×2, `folio-attention-endpoint` ×2, `folio-launch-endpoint` ×2, `folio-video-frame` ×2, `folio-video-prewarm`, `folio-video-engine` ×2, `folio-video-canplay` | ours | `std::thread::Builder` | `Worker(name)` after A1c, at their current band | the dir-watch thread runs FSEvents' `on_events` (macOS) and `watch_loop` (Windows) and calls `bt-app`'s `wake`; the endpoints call `bt-app`'s `decide`, `commit`, `deliver`; the video threads read through `file_reads` (`Lane::Animation`, `Lane::Peek`). None reaches a door A1 converts |
| `bt-app`'s window-process bare threads: `folio-web-thumb`, `explorer_menu::begin_probe`, `explorer_menu::run_request` | ours | `Builder` / `thread::spawn` | `Worker(name)` after A1c | the explorer threads deploy and read the sparse package (child processes); none A1 converts |
| `bt-pty`'s four: the reader and writer of `PtySession::spawn`, `spawn_dump_publisher`, `retire_within`'s `pty-retirement` | ours | `thread::spawn` / `Builder` | **`Unset`, by design** — `bt-pty` cannot name `admission` (§7, departure 1) | no admission door; their waits are the PTY transport's own, and the retirement thread's joins are row 15's other half |
| `bt-term`'s resample pool, `bt-image-resample-{index}` | ours (rayon) | `ThreadPoolBuilder::start_handler` | **`Unset`, by design** — only pure resampling runs there, and a closure capturing `&WorkerCtx` cannot be sent to it | none |
| Windows console control handler (`install_console_ctrl_handler`'s `handle`) | the OS's | `SetConsoleCtrlHandler`; the system starts a thread per event | `Callback("console-ctrl")` (A1a) | none — the body is one `matches!` |
| Media Foundation work queue (`video::engine`'s `IMFMediaEngineNotify::EventNotify`) | the OS's | COM callback | `Callback("mf-engine-notify")` | none — it sends one `Command::Event` on a channel |
| macOS `NSURLSession` delegate queue (`macos_http`'s delegate class) | the OS's | the session's serial operation queue | `Callback("http-session")` | none — it parks bytes for `bt-update-check` |
| macOS `UNUserNotificationCenter` completion and delegate (`macos_notify`) | the OS's | a block or delegate call on a queue the framework picks | `Callback("notification-center")` | none — it records the answer |
| macOS `NSWorkspace` open completion (`handoff::open_folder_in_finder`) | the OS's | completion block | `Callback("finder-open")` | none — it activates the running Finder |
| macOS `AVAssetImageGenerator` completion (`macos_video`) and `AVPlayer`'s end notification (`macos_player`'s observer class) | the OS's | completion block; notification observer | `Callback("video-frame")`, `Callback("video-end")` | none |
| AppKit delegate, menu, services, compose, dialogs and `WKWebView` delegates and completions; the Carbon hot-key handler | the OS's, **on the main thread** | AppKit delivers them on the main run loop | `Window` (they run on it; `enter_callback` keeps it) | as the window thread |
| WebView2 completion and event handlers (`webview.rs`), the window subclasses and message hooks (`lib.rs`, `pump.rs`) | the OS's, **on the window thread** | the pump dispatches them | `Window` | as the window thread |
| `IExplorerCommand` / `IClassFactory` (`explorer_command.rs`) | the OS's, in the `--explorer-command` door process | STA: COM calls arrive on the thread that registered the class object and pumps, `fn main`'s | `Unset` (a door process; §12 Q1) | none A1 converts (`quiet_command(...).spawn()` starts a child and does not wait) |
| winit | — | winit 0.30.13 starts no thread that runs our code on Windows or macOS (its registry source spawns one only on Wayland) | — | — |
| WASAPI | — | **none in the tree**: no `IAudioClient`, `wasapi` or `cpal` anywhere in `crates/` | — | — |
| `portable-pty`'s process-exit waiter (`vendor/conpty`) | vendor | `std::thread::spawn` in its `Future` impl | not ours: `bt-pty` does not poll that future, and the thread runs no first-party code | — |

A1a adds the `enter_callback` lines. None of these callbacks reaches a door
today, so their role matters only in the refusal counters' names (§3.4) and
as the ground a later door stands on: a callback that one day calls a
worker-only door has no `WorkerCtx` and does not compile, and one that calls an
owner-thread door is `Refused`.

### 3.4 What a wrong role costs

Every refusal (§4.3's `admitted`, a role or phase writer called on the wrong
thread) adds to a per-door atomic counter and a process total. Both are
printed in every budget line and in the exit summary once A3 lands; until then
the total goes to the exit line `hang_watch` already writes. Nothing panics and
nothing falls back: the effect does not happen on the wrong thread, and the
caller gets `Refused` as it gets the door's own error.

---

## 4. The phase (question 3)

### 4.1 Where it lives

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase { Starting, Running, Exiting }

thread_local! {
    static PHASE: Cell<Phase> = const { Cell::new(Phase::Starting) };
}
```

**On the window thread, not in a process-wide atomic.** The phase is read only
by `admitted`, which admits only on the `Window` role, and written only by the
writers below, which refuse off it. So a thread-local of the window thread is
the same fact as B§C-5's "process phase" in a product process (one window
thread), and it lets tests in one test binary each run their own window thread
in their own phase without racing. `Phase` is not `quit::Phase`, which is the
quit transaction's own and unrelated.

### 4.2 The writers — four, and why not three

B§C-5 names three pinned calls: "before the loop, at its first turn, and at
`settle_quit`". The code has two facts that make that too few:

- **A quit can be abandoned after its session write.** `QuitStep::Write` calls
  `SessionStore::flush_judged`, which is row 16's `SessionWriter::wait_for`. A
  write the store refuses answers `WriteVerdict::Refused`, and `quit::Quit`
  then yields `QuitStep::Abandon` — *"This quit is over and the application is
  exactly as it was."* An `Exiting` that cannot go back to `Running` would leave
  a continuing application in the phase that refuses its ordinary doors.
- **The way out has three roads.** Row 16's wait is also reached through
  `App::finish` → `SessionStore::close` → `flush` → `flush_judged`, and
  `App::finish` has five callers in `FolioApp` (the last window closing, the
  `exiting` callback, the quit's `Exit` step and two others). Row 17's
  `trace_sink::flush` runs in `fn main` after `run_app` returns, which is also
  reached when the loop stops with an error.

| writer (in `admission`) | transition | pinned call site(s) |
|---|---|---|
| `enter_window_thread()` | `Unset` role → `Window`, phase `Starting` | `fn main`, after `cli::parse` (§3.2) |
| `loop_running()` | `Starting` → `Running` | `FolioApp::new_events` when the cause is `StartCause::Init` — winit's first callback, before `resumed` |
| `exiting()` | `Running` → `Exiting` (idempotent) | three: the head of `settle_quit`'s `QuitStep::Write` arm; the head of `App::finish` (one function, so its five callers need nothing); `fn main` directly after `run_app` returns |
| `quit_abandoned()` | `Exiting` → `Running` | `settle_quit`'s `QuitStep::Abandon` arm |

Each refuses (and counts) off the `Window` role and on a transition not in its
row. The source guard pins the call sites (A1e).

### 4.3 Which rows admit which phases

Each door type carries `const PHASES: Phases` (a small bit set). The rule:

| rows | admitted in | why |
|---|---|---|
| 18 (`launch_wire::hand_over`) | `Starting` | before the loop exists |
| 15 (`bt_pty::wait_for_retirements`), 16 (`SessionWriter::wait_for`) | `Exiting` | on the way out, and only there |
| 17 (`trace_sink::flush`) | `Exiting` | after the loop |
| every other owner-thread row: 2–5, 7–14, 19–22 (open, done-with-residue, bound), and §5.2's native doors | `Running` **and** `Exiting` | the loop still turns while a quit waits for its pages (`QuitStep::WaitForPages` "keeps pumping the loop"), and a present, a title flush or a PTY resize on those turns is ordinary work. Admitting only `Running`, as B§C-5 has it, would make A1d change behaviour on the way out |

A door that could also run in `Starting` does not exist today: nothing
between `enter_window_thread` and `run_app` in `fn main` is a §5.3 row except
row 18. B6 (the macOS locale read moved before the loop) adds `Starting` to
row 7's door when it lands.

### 4.4 `admitted`, with the role, the phase and the meter

B§C-5's shape, with this note's two changes (the refusal's phase is optional;
the meter has an `enter` half, §7 departure 2):

```rust
pub struct Refused {
    pub door: &'static str,
    pub role: Role,
    pub phase: Option<Phase>, // Some only when role is Window
}

pub fn admitted<D: Door, R>(
    work: impl for<'scope> FnOnce(WaitToken<'scope, D>) -> R,
) -> Result<R, Refused> {
    // 1. role() == Window and D::PHASES contains phase(), or count and return Err
    //    without running `work`;
    // 2. meter.enter(D::KEY), if a meter is installed;
    // 3. start = Instant::now(); r = work(WaitToken::fresh()); end = Instant::now();
    // 4. meter.leave(D::KEY, start, end) — from a drop guard, so an unwinding
    //    `work` still leaves;
    // 5. Ok(r)
}
```

The token is taken **by value** at the door (`fn door(token: WaitToken<'_, D>, …)`),
so one admission pays for one door call: a second call inside the same `work`
has no token to give (M4a's neighbour, covered by the same pair).

---

## 5. The spawn-site census (question 4, first half)

### 5.1 How it was counted

A§0.1's method, on `78a3699a`: the patterns `spawn_at_priority(_with_stack)?\(`,
`thread::spawn\(`, `thread::Builder::new\(\)`, `ThreadPoolBuilder::new\(\)` over
`crates/*/src`, dropping `#[cfg(test)]` items, `*tests.rs`, `tests/`, `src/bin/`
and `build.rs`. Each site's test status was checked against its file's test
module boundary by hand. `bt-source`'s test file, `bt-math`'s deep-formula
test, `bt-render`'s theme test, `bt-corpus`'s recorder, `macos_http`'s test
server and `video::mod`'s quiet-wait test are the test sites the patterns also
match; they are out.

| what | 2026-09-23 (A§0.1, `b6ca4329`) | now (`78a3699a`) | why it moved |
|---|---|---|---|
| through the door | 23 | **24** | `taskbar_lane`'s `taskbar-state`, ticket 62 (`29f3842e`) |
| bare | 22 | **22** | — |
| total | 45 | **46** | — |
| rayon pools | 1 | 1 | — |

A1a corrects A§0.1 and A§5.1's "Forty-five" in the same commit.

### 5.2 Table A — the 24 sites through the door

All in `bt-app`, all product code. *Captures*: what the closure moves in.
*Reaches*: the effects of B§R-A's vocabulary the body reaches today, by door —
`recv` = `Receiver::recv` in the worker's loop (B§R-A's future `wait` door);
`cmd` = `quiet_command(_named)` plus `Command::output`/`wait`; `reads(L)` =
`file_reads` on lane `L`; `writes` = file writes (B§R-A's future `file_writes`);
`lock` = the marks lock's `try_lock`/`sleep`. *A1b*: what A1b does to the site.

| # | file · function | thread name | band | captures | reaches (B§R-A) | A1b |
|---:|---|---|---|---|---|---|
| 1 | `attention_copilot::begin_probe` | `copilot-version-probe` | below | nothing (statics `PROBE`, `WAKE`) | `cmd` (`cmd.exe`), `reads(Attention)` via `pipe_output` | `_ctx` |
| 2 | `files::FilesWorker::spawn` | `bt-files-worker` | below | request receiver, response sender, event proxy | `recv`; `read_directory` (enumeration: **no door**, A§6's known bypass) | `_ctx` |
| 3 | `git::drain` | `bt-git-pipe` | below | one child pipe | `reads(GitPipe)` | `_ctx` |
| 4 | `git::run_git_with_input` | `bt-git-stdin` | below | child stdin, the input bytes | a pipe write (not in the vocabulary) | `_ctx` |
| 5 | `git::GitWorker::spawn` | `bt-git-worker` | below | request receiver, response sender, event proxy | `recv`; `cmd`; `JoinHandle::join` on #3's threads; `find_git`'s `PATH` walk and `is_file` probes | `_ctx` |
| 6 | `handoff_lane::HandoffLane::start` | `bt-os-handoff` | below | request receiver, answer sender, `make_executor`, wake | `recv`; **`handoff`** (via `ShellThread`) | **real**: `make_executor: FnOnce(&WorkerCtx) -> E`, and `HandoffLane::spawn`'s executor calls `ShellThread::enter(ctx)` |
| 7 | `hang_watch::start` | `bt-hang-watch` | below | reports path, the window thread's id, threshold, trace flag | `thread::sleep(WATCH_INTERVAL)`; report `writes`; `bt_platform::hang`'s samplers | `_ctx` |
| 8 | `palette_index::IndexWorker::spawn` | `bt-index-worker` (`INDEX_WORKER_THREAD`) | below | request receiver, response sender, event proxy | `recv`; `walk` (enumeration: no door) | `_ctx` |
| 9 | `persist::SessionWriter::start` | `session-writer` | below | the shared ends (`Arc<Mutex<Option<…>>>`) | `recv`; `bt_persist::atomic_write` (`writes`) | `_ctx` |
| 10 | `main::MathWorker::spawn` | `bt-path-verify-worker` | below | path receiver, a result sender clone, wake | `recv`; `verify_path` → `handoff::resolved_for_a_door` (metadata, canonicalisation) | `_ctx` |
| 11 | `main::MathWorker::spawn` | `bt-image-scale-worker` | below | scale receiver, a result sender clone, wake, trace flag | `recv`; hands passes to `bt-term`'s rayon pool | `_ctx` |
| 12 | `main::MathWorker::spawn` (`_with_stack`, `bt_math::MATH_WORKER_STACK_BYTES`) | `bt-math-worker` | below | task receiver, result sender, wake | `recv`; `reads(Fonts)` via `file_reads::opaque` in `bt-math` | `_ctx` |
| 13 | `preview::PreviewWorker::spawn` | `bt-preview-worker` | below | request receiver, response sender, event proxy | `recv`; `reads(Preview)` | `_ctx` |
| 14 | `psreadline::begin_probe` | `psreadline-probe` | below | nothing (statics) | `cmd` (`powershell.exe`), `reads(Settings)` via `pipe_output` | `_ctx` |
| 15 | `shell_integration::begin_profile_probe` | `powershell-profile-probe` | below | the shell's path | `cmd`; `reads(Settings)` | `_ctx` |
| 16 | `profile_runtime::begin_startup_migration` | `powershell-profile-migration` | below | the data directory | `lock`; `writes`; `reads(Settings)` | `_ctx` |
| 17 | `profile_runtime::begin_enable` | `powershell-profile-enable` | below | nothing | `lock`; `writes` (the marks record) | `_ctx` |
| 18 | `profile_runtime::begin_removal` | `powershell-profile-removal` | below | nothing | `lock`; `writes` and removals | `_ctx` |
| 19 | `settings::FontLane::request` | `font-families` | below | `&'static FontLane` | the machine's font collection, walked (`monospace_font_families`) | `_ctx` |
| 20 | `taskbar_lane::TaskbarLane::ask_locked` | `taskbar-state` | below | `&'static TaskbarLane` | `Condvar::wait` on its requests; `taskbar_is_auto_hidden` (`SHAppBarMessage`) | `_ctx` |
| 21 | `trace_sink::start` | `bt-trace-sink` | below | line receiver, drop counter, done sender | `recv`; trace `writes` | `_ctx` |
| 22 | `update::begin` | `bt-update-check` | below | the data directory | the HTTPS request (`bt_platform::http`); `reads(Settings)`; `writes` (`update-check.json`) | `_ctx` |
| 23 | `runtime/preview` `Runtime::reload_background_picture` | `background-picture` | below | path, ceiling, the decode slot, file name, generation, event proxy | `reads(InlineImage)` via `bt_term::decode_background_image` | `_ctx` |
| 24 | `runtime/preview` `Runtime::save_clipboard_picture` | `clipboard-picture` | **normal**, by its own stated reason ("somebody is waiting for this") | the queue place guard, folder, offered pictures, inbox, generation, target, event proxy | `writes` (`create_dir_all`, the picture file) | `_ctx` |

**Answers to the brief's two columns.** *Whether it needs `&WorkerCtx` today*: no
site can, because the type does not exist; the *reaches* column is what each
will need when its door takes one. *Whether it hosts a worker-only door*: every
site reaches at least one effect of B§R-A's worker vocabulary except #4, #11 and
#19 (whose effects are not in the vocabulary as written); only #6's door exists
in a form A1 converts (§6.3 says why the others wait).

Also through the door, in test code: three `bt-platform` tests of the band
(`bt-portable-band-probe`, `bt-test-worker`, `bt-test-band`), which A1b changes
with the product sites.

### 5.3 Table B — the 22 bare sites

| # | crate · file · function | thread name | band today | what runs there | A1 |
|---:|---|---|---|---|---|
| 1 | `bt-app` `web_thumb::PageShrinker::start` | `folio-web-thumb` | inherited `Normal` | shrinks page pictures; answers on a channel | A1c, at `Normal` |
| 2 | `bt-app` `explorer_menu::begin_probe` | unnamed | `Normal` | reads the sparse package's state; may repair it | A1c, named, at `Normal` |
| 3 | `bt-app` `explorer_menu::run_request` | unnamed | `Normal` | one package deployment | A1c, named, at `Normal` |
| 4 | `bt-app` `explorer_menu::remove_from_explorer_menu` | unnamed | `Normal` | door process `--remove-explorer-menu`: both registrations removed, under `REMOVAL_TIMEOUT` | §12 Q1 |
| 5 | `bt-app` `explorer_menu::cleanup_registrations` | unnamed | `Normal` | door process `--uninstall-cleanup` | §12 Q1 |
| 6 | `bt-app` `attention_wire::payload_on_stdin` | unnamed | `Normal` | door process `attention`: stdin read under `STDIN_BUDGET`, `reads(Attention)` | §12 Q1 |
| 7 | `bt-platform` `lib.rs` `DirWatch::start_scoped` (Windows) | `bt-dir-watch` | `Normal` | `watch_loop`: `ReadDirectoryChangesW` and its waits; calls `wake` | A1c, at `Normal` |
| 8 | `bt-platform` `macos_watch::DirWatch::start_scoped` | `bt-dir-watch` | `Normal` | `CFRunLoopRun`; FSEvents' `on_events` runs here; calls `wake` | A1c, at `Normal` |
| 9 | `bt-platform` `attention_pipe::AttentionPipe::start` (Windows) | `folio-attention-endpoint` | `Normal` | the named-pipe listener; calls `deliver` | A1c, at `Normal` |
| 10 | `bt-platform` `attention_pipe_unix::AttentionPipe::start` | `folio-attention-endpoint` | `Normal` | the socket listener; calls `deliver` | A1c, at `Normal` |
| 11 | `bt-platform` `launch_pipe::LaunchPipe::start` (Windows) | `folio-launch-endpoint` | `Normal` | the listener; calls `decide`, `commit` | A1c, at `Normal` |
| 12 | `bt-platform` `launch_pipe_unix::LaunchPipe::start` | `folio-launch-endpoint` | `Normal` | the same, on a socket | A1c, at `Normal` |
| 13 | `bt-platform` `video::within_budget` | `folio-video-frame` | `Normal` | one frame question, answered under a budget | A1c, at `Normal` |
| 14 | `bt-platform` `video_portable::within_budget` | `folio-video-frame` | `Normal` | the same shape off Windows | A1c, at `Normal` |
| 15 | `bt-platform` `video::prewarm` | `folio-video-prewarm` | `Normal` | the media session's one-time warm-up | A1c, at `Normal` |
| 16 | `bt-platform` `video::engine::Engine::open_on` | `folio-video-engine` | `Normal` | the Media Foundation engine loop | A1c, at `Normal` |
| 17 | `bt-platform` `video::engine::can_play_types` | `folio-video-canplay` | `Normal` | an MTA apartment and one question | A1c, at `Normal` |
| 18 | `bt-platform` `macos_player::Engine::open` | `folio-video-engine` | `Normal` | the `AVPlayer` loop | A1c, at `Normal` |
| 19 | `bt-pty` `PtySession::spawn` | unnamed (the reader) | `Normal` | `read_pty_output` | stays bare (`Unset`) |
| 20 | `bt-pty` `PtySession::spawn` | unnamed (the writer) | `Normal` | `pump_pty_input` | stays bare |
| 21 | `bt-pty` `spawn_dump_publisher` | unnamed | `Normal` | `sync_data` every `PTY_DUMP_PUBLISH_INTERVAL` while recording | stays bare |
| 22 | `bt-pty` `retire_within` | `pty-retirement` | `Normal` | one pane's teardown | stays bare |

Plus `bt-term::inline_image::resample_pool` (`bt-image-resample-{index}`,
`BelowNormal` from its `start_handler`), which stays `Unset` (§3.3).

**"At `Normal`" is the whole of A1c's scheduling change: none.** A bare thread
runs at `Normal` today, and `spawn_at_priority(…, ThreadPriority::Normal, …)`
starts it at `Normal`. Whether any of them should move to `BelowNormal` under
RULES 53 is §12 Q2, and is not decided by a door swap.

---

## 6. The migration (question 4, second half)

### 6.1 One commit for the signature, and why

The closure's type changes from `FnOnce() -> T` to `FnOnce(&WorkerCtx) -> T`.
There is no signature that accepts both. So the 24 sites change **in the same
commit as the door**, or a second spawner exists for the interval between
themed commits (a `spawn_worker` beside `spawn_at_priority`, then a rename that
touches all 24 again). A second spawner is a second entrance to the thread door
for the length of the migration, which is exactly what RULES 52 forbids ("a
side effect with a door has exactly one named entrance"), and the rename commit
is the same 24-site edit anyway.

The edit per site is mechanical and reviewable in one screen: `move || body`
becomes `move |_ctx| body`; `|| body` becomes `|_ctx| body`. The commit
compiles on its own (the house rule: each themed commit compiles), and the only
site whose body changes is #6.

### 6.2 The order

1. **A1a** lands `admission` with no door converted: roles, phases,
   `enter_window_thread` in `fn main`, the phase writers at their call sites,
   `enter_callback` at §3.3's callback entries, `Door`, `doors`, `WaitToken`,
   `admitted`, the meter, the registry. Nothing requires a capability yet, so no
   call site changes except `fn main`, `FolioApp::new_events`, `settle_quit`,
   `App::finish` and the callback entries — each one added line.
2. **A1b**, one commit: the thread door moves into `admission` with the new
   signature; the 24 sites and three band tests take `_ctx`; #6 takes `ctx` for
   real. A second commit on the same branch makes the seven hand-off verbs
   private to `handoff` (reachable only through `ShellThread::hand_over`) and
   adds A1b's compile-fail tests.
3. **A1c**, themed commits, each compiling: (i) `bt-platform`'s two watch
   threads; (ii) the four endpoint threads; (iii) the six video threads; (iv)
   `bt-app`'s window-process three. Door processes' three follow §12 Q1.
4. **A1d** converts the owner-thread doors; **A1e** lands the prohibitions.
   Neither depends on A1b or A1c.

Why A1a first: A1b's executed proof M4g ("`admitted` on a worker is refused")
needs `admitted`, and A1d's doors need door types that equal the registry.

### 6.3 Which sites gain a real use now, and why only one

B§C-8 has A1 convert "the doors of the (b) table … in place to take a token or
`&WorkerCtx`". For the worker-only half of that table, in-place conversion is
possible for **one** door, and this note narrows A1 to it:

| B§R-A worker-only door | why it does or does not convert in A1 |
|---|---|
| `bt_platform::handoff` | **converts** (A1b). Its only product road is `ShellThread::hand_over` on `bt-os-handoff` (pinned by `handoff_lane::no_handoff_runs_on_the_window_thread`), so `ShellThread::enter(&WorkerCtx)` breaks no product caller. |
| `bt_platform::file_reads` | **does not.** Its lanes span both threads: `Lane::Settings` is read on the window thread at launch (`schemes`, `persist`, `bt-persist`'s migration), `Lane::Fonts` in `bt-render` on the window thread, `Lane::Attention` in door processes. "A role per lane" (B§R-A) needs those window-thread reads inventoried as registry lines first — they are unlisted waits today — which is A2's vocabulary work, and the lint forces the same edit. |
| `bt_platform::quiet_command(_named)` | **does not.** The door builds a `Command`; the waits are `Command::output`/`status`/`wait` at the callers. One caller is on the window thread (`read_system_locale_declaration`'s `quiet_command_text`, row 7, until B6) and one in a door process's COM callback (`explorer_menu::serve`). The worker door for the *waits* is A2's. |
| `bt_platform::file_writes`, `bt_platform::wait::{join_bounded, recv_bounded}` | **do not exist.** They are created by A2 with the lint that makes them necessary. |

So after A1 the capability is real at one worker-only door and at every
owner-thread door A1d converts; A2 extends it to the rest as it lints them.
That is the true intermediate architecture A1 lands (§11).

---

## 7. Departures from revision (c), each for the review

1. **`bt-pty` does not depend on `bt-platform`.** B§C-5: "`bt-app`, `bt-render`
   and `bt-pty` all depend on `bt-platform`, so every door in the workspace can
   take them." `crates/bt-pty/Cargo.toml` names `bt-transcript`, `portable-pty`
   and `thiserror`; A§3.1's graph agrees (`bt-platform ← bt-persist, bt-math,
   bt-render, bt-term, bt-app`). Consequences: (a) B§R-A's owner doors for rows
   11–12 cannot be `PtySession::spawn_shell_in` and `PtySession::resize`
   themselves; **they are `bt-app` functions wrapping them** — today's single
   callers `create_leaf_session` and `Runtime::flush_pending_pty_resize` — and
   A2 lists the two `bt-pty` methods as first-party vocabulary, so a second
   caller is refused by the lint; (b) `bt-pty`'s four threads stay `Unset`
   (§3.3). The alternative, a `bt-pty → bt-platform` edge, edits A§3.1 and puts
   the platform crate under the PTY transport for the sake of a type; this note
   does not take it and the review may say it should.
2. **The meter has two halves.** B§C-5: `install_meter(fn(DoorKey, start, end))`.
   A post-hoc callback cannot name a call that never returns, and naming it is
   what `hang_watch::during` does today: the station is pushed before the call,
   so a hang report says where the window thread is stuck. A1a's meter is
   `Meter { enter: fn(DoorKey), leave: fn(DoorKey, Instant, Instant) }`,
   installed once; `admitted` calls `enter` before `work` and `leave` after it,
   `leave` also on unwind (a drop guard, as `during` keeps its stack balanced).
   `hang_watch` registers `enter` as its station push. A3's per-call record is
   `leave`. Before `install_meter` runs (row 18, which precedes
   `hang_watch::start`), `admitted` measures nothing — as today.
3. **The phase has a back edge and three exits** (§4.2), and ordinary rows admit
   `Exiting` too (§4.3).
4. **Several `Drop` impls block today**, so B§C-3's "vocabulary is refused
   inside any `impl Drop`" is red on the base it lands on: `DirWatch` (both
   platforms, `thread.join()`), `AttentionPipe` and `LaunchPipe` (both
   platforms, `listener.join()`), `video::engine::Engine` and
   `macos_player::Engine` (`self.shutdown()`), `VideoSeat` (`self.shutdown()`),
   `PtySession` (`self.shutdown()`). Two things follow. The prohibition lands
   (A1e) with these as a **declared, shrink-only exception list**, each with its
   ledger row (D-40 for `DirWatch`; new rows for the rest). And a lexical check
   of the `Drop` body misses the one-call indirection (`drop` → `shutdown` →
   `join`): only A2's lint, which refuses the `join` inside `shutdown` unless
   `shutdown` is a door, closes it.
5. **No `expect_worker()` backstop.** B§C-5 keeps it "as a backstop, with the same
   counter", and the second review asked for it. With `&WorkerCtx` in the
   signature, the only way to reach a worker-only door off a worker is a
   fabricated `WorkerCtx`, which `#![forbid(unsafe_code)]` and the M14 fence
   exclude. A check that can fire only after both have failed has no executable
   red test (there is no safe program that reaches it), so it would be a
   validation of a case that cannot happen. Refusals that *can* happen —
   `admitted` on a worker or a callback, a phase writer off the window thread —
   are counted and tested (M4g, M4i).
6. **B§C-6 calls M3 both a compile error (its table) and an executed proof (its
   last paragraph).** With departure 5, M3 is compile-only. The executed proofs
   of the boundary are M4g (`admitted` refused on a `Worker` and on a
   `Callback`) and M4i (a phase outside `D::PHASES`), plus a passing control
   for the worker side: a worker-only door run on a door-started thread
   (§9.3).
7. **A compile-fail doctest cannot assert its reason on the pinned stable
   toolchain.** Rustdoc turns on its check of a `compile_fail,E0277`-style
   error code only for a nightly build; on stable any compile error passes the
   block. (Read from rustdoc's source, not run here. A1a's first step confirms
   it with one block tagged with a deliberately wrong code: it must pass on
   stable for this departure to stand.) B§C-6 requires "the compile error
   code". §9.2 answers with a paired control per test.

---

## 8. What A1 does not do (question 5)

So that no A1 ticket drifts:

- **No lint.** No `disallowed-methods`, no `clippy.toml` vocabulary, no
  `[workspace.lints.clippy] disallowed_methods`, no `CLIPPY_CONF_DIR`, no
  `gates-can-fail` plant, no per-target job change. No
  `#[expect(clippy::disallowed_methods)]` anywhere. Mutations M1, M2, M6, M7e,
  M8, M9 and M15 are A2's.
- **No new door.** `file_writes`, `wait::{join_bounded, recv_bounded}` and any
  door for `Command::output` are A2's. `file_reads` and `quiet_command` keep
  their signatures (§6.3).
- **No accounting.** No per-call histogram, no whole-turn record, no loss
  counter, no budget line. `Meter::leave` exists and `hang_watch` registers a
  `leave` that does what `during`'s exit does today; A3 extends it.
  `TurnAllowance` is A4's.
- **No move.** No wait leaves the window thread. The marks lock (B4), the
  PSReadLine probe (B5), the macOS locale (B6), `DirWatch` start and drop (B7),
  the stores' writes (B8), device recovery (B9) stay where they are; A1d wraps
  them in `admitted` where they are owner-thread rows.
- **No band change.** A1c starts every bare thread at the band it has today.
  RULES 53's "every worker below normal" is applied by §12 Q2's answer, not by
  A1.
- **No lane change.** B§R-D's adapters, `lane::EXPECTED_FAILURES` and the lane
  declarations are untouched; a lane's identity is not a thread's.
- **No `bt-pty` edge** (§7, departure 1), no `bt-term` pool entry.
- **No retirement of `hang_watch::during`.** Stations that are not admitted
  waits (`Scope`, `Work`) keep `during`; A1d removes an outer `during` only where
  it wraps an `admitted` call of the same station, which the meter now opens.
- **D-2 is not closed.** By the owner's ruling of 2026-09-25 it closes when A1,
  A2 and A3 have landed; "A1" means A1a–A1e.

---

## 9. The tests A1 ships (question 6)

### 9.1 The source guard's checks

In `bt-app`, reading through `bt_source` with a declared universe (every
first-party product target; test modules excluded by declaration; `vendor/`
excluded). The test is B§R-A's `window_waits_tests::every_door_is_where_the_registry_says`,
split into named assertions so each mutation's message is its own.

| check | ticket | mutation it refuses |
|---|---|---|
| `admission::doors` holds exactly one uninhabited type per registry line, and each type's `ROW`, `STATION` (as the byte of `hang_watch::Station` the registry names) and `PHASES` equal the line | A1a | a door type added, removed or re-numbered without the registry |
| every owner-thread door function named by the registry takes `WaitToken<'_, doors::<its type>>` by value, and names no other door's type | A1d | M4h's source twin |
| `admission.rs` carries `#![forbid(unsafe_code)]` | A1a | M14 (the attribute removed) |
| `WaitToken`, `WorkerCtx`, `admission::doors` never named inside an `unsafe` block or `unsafe fn`, nor inside `transmute`, `zeroed`, `MaybeUninit` or `read` expressions, anywhere in the product | A1e | M14 |
| one `WorkerCtx {` literal in product code, in `spawn_at_priority_with_stack`; no trait `impl` for `WorkerCtx` or `WaitToken` beyond the listed ones | A1e | §2.4's rows |
| the role and phase writers' product call sites are exactly §3.2's and §4.2's | A1e | a second `enter_window_thread`; an `exiting()` moved |
| §C-2 attribute checks: no `allow`/`expect` of `clippy::disallowed_methods`, `clippy::style`, `clippy::all` or `warnings` at item, module or crate level, in either path spelling, including inside `cfg_attr` at any depth. (With no lint yet, the expected count of door `expect`s is zero.) | A1e | M7a–M7d |
| §C-3: no first-party `#[macro_export]` body names a vocabulary path (one exported macro exists today); `msg_send!`/`msg_send_id!` (20 uses), `vtable(` (1), `GetProcAddress` (0), `extern` blocks (6) and `#[link]` (6) only in their listed owners, counts pinned; no vocabulary inside an `impl Drop` beyond §7 departure 4's exception list | A1e | M11, M12, M13 |
| §C-5 synchronous doors: no registered door is `async`, returns a closure, `impl Fn*`, `impl Future`, `impl Iterator`, `Box<dyn …>` or `fn` pointer, or takes a `'static` closure it stores | A1e | M10 |

The vocabulary the §C-3 and `Drop` checks read is the registry's vocabulary
table, which A1a creates and A2 later compares with `clippy.toml`.

### 9.2 Compile-fail proofs, and how they are written here

**The mechanism: `compile_fail` doctests in `bt-platform`, each paired with a
passing control that differs from it by one statement.**

- *Why doctests.* The tree already uses them for exactly this kind of claim:
  census-3's `bt_workbench::attention::Places` carries two `compile_fail`
  blocks with a `MUTATION:` line. `bt-platform` is a library, so its doctests
  run in CI's Windows `cargo test --workspace --exclude bt-pty --exclude
  bt-render`; every type these tests need (`WorkerCtx`, `WaitToken`, `admitted`,
  `spawn_at_priority`, `ShellThread`) is public there. `bt-app` is a binary with
  no library target, so it cannot host doctests — and does not need to, because
  every compile-time property A1 claims is a property of `admission`'s types.
- *Why not `trybuild`.* It is not in `Cargo.lock`. It would add a package set to
  the lock file and `THIRD-PARTY-NOTICES.md` (A§3.3 rule 1), and its assertion
  is the compiler's exact stderr, which changes with every deliberate toolchain
  bump (`rust-toolchain.toml` pins `1.94.1`) and would be re-blessed each time.
- *The gap, and how the pair closes it.* On stable, rustdoc does not check a
  `compile_fail` block's error code (§7, departure 7), so a block that fails
  for an unrelated reason — a typo, a moved path — would pass. **Each
  `compile_fail` block is written beside a control block that is identical
  except for the one statement the mutation names, and the control must
  compile** (`no_run`, since several controls start threads). The pair proves
  the failure is caused by that statement. The expected error (E-code and
  one-line meaning) is written in the block's prose and as the block's
  `compile_fail,E0xxx` tag, which a nightly run checks and stable ignores; the
  note says so rather than claiming a check that does not happen.

| # | mutant (fails to compile) | control (compiles) | expected error | ticket |
|---|---|---|---|---|
| M3 | a function with no `&WorkerCtx` in scope calls `ShellThread::enter` | the same call inside a `spawn_at_priority` body, passing its `ctx` | E0061/E0308: missing or mismatched argument | A1b |
| M3r | a door-started body sends its `&WorkerCtx` (or a closure capturing it) into `std::thread::spawn` | the body calls the door itself | E0277: `WorkerCtx` cannot be shared between threads | A1b |
| M3r′ | a `WorkerCtx` built by a struct literal outside `admission` | — (privacy; the control is M3's) | E0451: private field | A1b |
| M4a | `admitted::<D, _>(\|_\| ()); door(…)` — the door called after the scope | the door called inside `work` with the token | E0425/E0308: no token in scope | A1a |
| M4b | `work` returns its token, or `&token` | `work` returns `()` | E0521/lifetime: `'scope` escapes the higher-ranked binder | A1a |
| M4c | `work` stores the token in a `static` or an outer `Option` | `work` consumes it | lifetime error | A1a |
| M4d | `work` returns `move \|\| door(token)` or an `async move` block using it | `work` calls `door(token)` | lifetime error | A1a |
| M4e | `work` sends the token, or a reference, to `std::thread::spawn` | `work` uses it on its own thread | E0277: `*const ()` is not `Send` / `Sync` | A1a |
| M4f | `WaitToken { … }` built outside `admission` | — | E0451 | A1a |
| M4h | a token of `doors::A` passed to a door that takes `doors::B` | the same with `doors::B` | E0308: mismatched types | A1a |
| M5 (worker side) | `let f: Box<dyn Fn()> = Box::new(\|\| { ShellThread::enter(?) ; })` on a thread with no capability | the same `Box<dyn Fn(&WorkerCtx)>` called inside a door-started body | as M3 | A1b |

Doctests compile against the crate as built, without `cfg(test)`, so M4a–M4h
use two real door types from `admission::doors` (the two lowest rows), which
also shows that real door types behave as the tests claim. The functions they
pass tokens to are declared inside each doctest
(`fn door(_: WaitToken<'_, doors::X>) {}`), so no product door runs.

### 9.3 Executed proofs

In `bt-platform`'s own unit tests, so they can use a `#[cfg(test)]` door type
(`doors::Probe`, outside the registry equality, which reads product items) and
read its per-door counter without racing other tests.

| # | sequence | assertion | ticket |
|---|---|---|---|
| M4g | a thread started through `spawn_at_priority` calls `admitted::<doors::Probe, _>(\|_\| ran = true)` | `Err(Refused { role: Worker("…"), phase: None, … })` (a phase is reported only for a `Window` thread, §4.1); `ran` is false; `Probe`'s counter rose by exactly one | A1b |
| M4g′ | a plain `std::thread` enters `enter_callback("probe")` and does the same | `Refused { role: Callback("probe"), … }`, not run, counted; after the scope drops, `role()` is `Unset` | A1a |
| M4g″ | a plain `std::thread` with no entry does the same | `Refused { role: Unset, … }` | A1a |
| M4i | a plain thread enters `enter_window_thread()`; calls `admitted` for a `Running`-only probe door | refused with `phase: Starting`; after `loop_running()`, admitted and run; after `exiting()`, a `Starting`-only door is refused; after `quit_abandoned()`, the `Running` door is admitted again | A1a |
| M4i′ | `loop_running()` on a worker; `exiting()` on an `Unset` thread | no change to that thread's phase; counted | A1a |
| M5 (owner side) | `Box<dyn Fn()>` and an `fn` pointer that call `admitted` for a probe door, invoked on a worker | refused, not run; **passing control**: the same indirection invoked on a thread that entered `Window`, admitted and run | A1d |

**Passing controls for the worker side.** (i) A door-started thread calls
`ShellThread::enter(ctx)` and hands over a `Handoff` through a recording
executor, and the lane's existing answer path returns the door's result
(the lane adapter A5 landed drives it through `HandoffLane::start`). (ii) The
band tests keep proving the band is set: `current_thread_priority()` read as
the body's first observation on Windows is the requested band, unchanged by the
role write after it.

**The meter.** A1a's test: a probe door admitted with a meter installed calls
`enter` then `leave` once each with `start ≤ end`, and a `work` that panics
still calls `leave` (the unwind is caught by the test). `hang_watch`'s
registration is covered by one `bt-app` test: an admitted door's station is on
the station stack while `work` runs (read by the same accessor the watchdog's
report uses).

### 9.4 What every A1 ticket also runs

The standing rules' guard filters (`file_reads`, `layer_shape`, `station`,
`hang_watch::`, the `bt-source` tripwire), `lane_contract_tests` (A1b changes
`HandoffLane::start`'s signature, which the hand-off adapter drives), and
`handoff_lane::no_handoff_runs_on_the_window_thread` (A1b makes it
belt-and-braces; it stays). A1d adds: every converted owner door has a test
that reaches it through its real caller and asserts the effect happened — a
test that reaches a door as `Unset` now gets `Refused`, the effect does not
happen, and that test goes red rather than quiet.

---

## 10. Risks (question 7)

- **The 24-site edit's blast radius.** Mechanically small (one closure head per
  site), but it touches seventeen `bt-app` files and `bt-platform`'s `lib.rs` in
  one commit, several of which other
  0.4.6 tickets edit (`runtime/preview.rs`, `settings.rs`, `main.rs`,
  `profile_runtime.rs`). The risk is merge conflict, not behaviour: the
  coordinator's merge check (`cargo check -p bt-app --all-targets` on main
  after same-day merges) applies. A1b should be dispatched when no other open
  branch rewrites a spawn site's closure head, and the brief lists the 24 by
  function name so a rebase can re-derive them.
- **`!Send` `WorkerCtx` against closures that move work onward.** Checked per
  site: no product body sends anything it did not create onward except #5
  (`bt-git-worker` spawns #3 and #4, each of which gets its own `WorkerCtx` from
  the door) and #11 (hands resampling passes to `bt-term`'s rayon pool, which
  must not and cannot receive `&WorkerCtx`). No product body uses
  `std::thread::scope`. The lasting cost is a rule an implementer will meet in
  A2: **a worker-only door cannot be called inside a rayon job or a scoped
  thread**; do it before or after the parallel section. The compiler says so
  with E0277, which is the intended failure.
- **macOS run-loop threads.** The window thread is AppKit's main thread, and
  `fn main` runs there, so `enter_window_thread` names the right thread; AppKit
  delegates, menus and `WKWebView` completions arrive on it and are `Window`
  (§3.3). The FSEvents callback runs inside `CFRunLoopRun` on our own
  `bt-dir-watch` thread, so after A1c it is `Worker("bt-dir-watch")`; FSEvents
  never calls it elsewhere, because the stream is scheduled on that thread's
  run loop only. GCD and `NSOperationQueue` threads are pooled: a callback's
  `CallbackScope` must restore `Unset` on drop, including on unwind, or the next
  block on that pooled thread would inherit a stale name. `bt_platform::hang`'s
  "does the main run loop answer" block runs on the main thread and is
  `Window`. `Window` does **not** mint objc2's `MainThreadMarker`; AppKit calls
  keep taking that marker as today.
- **The release-build cost.** A worker-only door: zero (the check is a type).
  The thread door: one thread-local store per thread start. `enter_callback`:
  two stores per callback (Media Foundation's notify rate is tens per second
  during playback). `admitted`: two thread-local loads (role, phase), a bit
  test, two indirect calls to the meter and the two `Instant::now` that
  `during` already takes today — `admitted` replaces the outer `during` at
  each converted door, so the clock reads do not double. Against calls that
  block outside the process, none of this is measurable; A3's overhead
  measurements (B§R-C item 5) include it.
- **`hang_watch` stations.** `Station` is `#[repr(u8)]` with `STATION_COUNT` =
  210, so `Door::STATION: u8` has 45 values of headroom. A1 adds no station: each
  registry line names an existing one. A new owner-thread door with no station
  of its own takes one from that headroom, and the byte check (§9.1) goes red
  before `from_byte` could alias. Nesting: where a converted door sits inside
  an outer `during` of the *same* station, the meter's `enter` pushes it again
  and the hang report shows `X > X`; A1d removes those outer wrappers (§8).
  Where the outer station differs (a `Scope` around a door), both stay and the
  exclusive-time arithmetic is unchanged.
- **Tests that drive a `Runtime`.** After A1d, a test that reaches an
  owner-thread door on the libtest thread without entering `Window` gets
  `Refused`. Each such test must enter `Window` (on its own thread; libtest runs
  each test on a fresh thread). The risk is a test that asserted nothing about
  the door's effect and so stays green while the door is skipped; §9.4's rule
  (every converted door has a test asserting its effect through its real
  caller) is the mitigation.
- **`Drop` exceptions** (§7, departure 4) are real window-thread waits on some
  roads (a `DirWatch` dropped on the window thread is row 8). A1e declares them;
  it does not make them smaller.
- **`folio-web-thumb` panics when the kernel refuses a thread** (the ledger's
  finding list). A1c's swap keeps that behaviour, because changing it is a
  behaviour change a door swap should not carry; the finding stays in the
  defect ledger.

---

## 11. Tickets (question 9)

All 0.4.6. **Each ends at "committed, CI green on the branch"**; merges wait for
the 0.4.5 tag (owner, 2026-09-25). Every brief carries `_standing-rules.md` in
full. "A1" in B§C-8 and in the owner's D-2 ruling means all five.

| id | title | size | prerequisites | lands alone as |
|---|---|---|---|---|
| A1a | `bt_platform::admission` is born: roles, the window thread's phase, door identity and the owner-thread token | M | the 0.4.5 tag; this note reviewed | `admission` with `#![forbid(unsafe_code)]`: `Role`, `role()`, `enter_window_thread` (called once in `fn main`), `enter_callback` at §3.3's callback entries, `Phase` and its four writers at §4.2's call sites, `Door` (sealed), `doors` (one type per registry line), `WaitToken`, `admitted`, `Refused`, the counters, `Meter` and `install_meter` (registered by `hang_watch::start`). The registry `crates/bt-app/src/window_waits.tsv` with B§R7's schema plus the phase column and the vocabulary table, seeded from A§5.3 at the ticket's base; `scripts/dev/generate-window-waits-table.ps1` and the generated A§5.3; the D-33 and D-42 version notes; A§0.1 and A§5.1's counts corrected to 46/24/22. Door-type equality (§9.1 row 1); M4a–M4f, M4h, M4g′, M4g″, M4i, M4i′; the meter tests. **Nothing requires a capability yet**: the architecture it leaves is "every thread that runs our code is classified, and no door asks" |
| A1b | The thread door lends a `WorkerCtx` | S–M | A1a | §2's signatures inside `admission`, re-exported; one commit changing the 24 sites and three band tests (§6.1); `HandoffLane::start`'s `make_executor` takes `&WorkerCtx`, `ShellThread::enter(ctx)`, the seven verbs private to `handoff`; M3, M3r, M3r′, M5 (worker side), M4g; the passing controls of §9.3. RULES 52's thread sentence and A§6's thread row say "lends a `WorkerCtx`" |
| A1c | The window process's bare threads enter through the door | S | A1b; §12 Q1 for the door-process three | `bt-platform`'s twelve and `bt-app`'s three window-process threads through `spawn_at_priority` at `ThreadPriority::Normal` (their band today), each named; A§0.1's bare count becomes 7 (`bt-pty` 4, door processes 3) or 4 (after Q1); A§6's `folio-web-thumb` bypass paragraph repaid. No scheduling change |
| A1d | Owner-thread doors take a `WaitToken` | M | A1a | the owner-thread doors of B§R-A converted in place: the GPU present and configure doors; the §5.2 natives (the title flush, the IME caret-area flush, focus, visibility, cursor, the DirectComposition commit, WebView2's environment and controller requests); rows 5 and 13's residues; rows 15–18's roads; rows 11–12 as `bt-app` wrappers of the `bt-pty` calls (§7, departure 1). Each caller handles `Refused` as it handles the door's error; outer `during`s of the same station removed; M5 (owner side); §9.4's effect tests. Behaviour change: none on the window thread; `Refused` elsewhere |
| A1e | The source guard's prohibitions | M | A1a (the registry's vocabulary) | §9.1's rows marked A1e: §C-2 attribute checks, §C-3 (exported macros, FFI owners and counts, `Drop` with the declared exceptions and their ledger rows), §C-5 fences (unsafe, synchronous doors, the one `WorkerCtx` literal, trait impls, the writers' call sites); M7a–M7d, M10–M14 |

**Why five and not one.** B§C-8's A1 is an M that contains a module, a
workspace-wide signature change, a door conversion across three crates and a
new guard with its own inventories. Each of those has a different reviewer
question and a different conflict surface; together they are one merge that
touches most of `bt-app` at once. Split, each is S or M, and each leaves a
truthful architecture: after A1a, roles exist and nothing depends on them; after
A1b, the one converted worker door is typed; after A1d, the owner doors are
typed; after A1e, the escapes are fenced.

**What waits on what, outside A1.** A2 needs A1a (the vocabulary), A1b (worker
doors take `&WorkerCtx`), A1c (the bare threads have roles, so their waits can
go through `wait::*` with a capability) and A1e (the suppression checks). A3 is
dispatched against A1a's `Meter` and lands after A1a; its per-call lines stay
empty until A1d converts a door. B4, B7 and B9 need A1b (`WorkerCtx`), as B§C-8
says; B7's `retire(self, &WorkerCtx)` needs A1c's `bt-dir-watch` too.

---

## 12. Open questions for the owner (question 8)

1. **Do door processes' threads owe the thread door?** A§6: *"Whether a door
   process's thread owes the door is not ruled."* Three threads are in door
   processes (`explorer_menu::remove_from_explorer_menu`,
   `explorer_menu::cleanup_registrations`, `attention_wire::payload_on_stdin`).
   - *Mine:* yes, at `Normal` (their band today), in A1c. Then RULES 52 reads
     "every thread the product starts, in every process, except `bt-pty`'s four
     and the resample pool, comes from the door", which is simpler to hold than
     a list of which processes count; and A2's lint will need their waits to go
     through a door anyway. The door processes' main threads stay `Unset`: they
     are not the window thread.
   - *The cost of yes:* three more sites in A1c; a door process that is being
     deleted by an uninstaller runs the same thread-start code the window
     process does.
2. **Are the video engine and the two endpoints "workers" under RULES 53?** The
   rule says every worker is below normal. A1c keeps twelve `bt-platform`
   threads and three `bt-app` threads at `Normal`, their band today, so the
   door swap changes no scheduling.
   - *Mine:* lower `folio-web-thumb`, the explorer probe and deployment, the
     directory watches, `folio-video-prewarm`, `folio-video-canplay` and
     `folio-video-frame` to `BelowNormal` in a separate ticket (they are
     workers by the rule's own list — observation and computation); keep
     `folio-video-engine` at `Normal` (a playing video's frames are the picture
     somebody is watching, the reason `clipboard-picture` already stands at
     `Normal`); keep the two endpoints at `Normal` (a second `folio.exe` is
     waiting on the launch endpoint's answer, bounded by `HANDOVER_BUDGET`).
   - *Why it is the owner's:* it decides whether playback and the second
     launch's hand-over may be starved under exactly the load the band exists
     for, and RULES 53 is an owner rule.

---

## 13. Architecture impact

### 13.1 This note's own

(a) Facts touched: none — a document. (b) Doors: none. (c) Debt: none repaid or
added by this commit. (c′) None. (d) No ownership changes by this commit; A1a
and A1b make the ones below, which is why this note exists.

### 13.2 The tickets', for their briefs

| ticket | (a) facts | (b) doors | (c) debt | (c′) new writers or trigger sources | (d) ownership |
|---|---|---|---|---|---|
| A1a | **new**: a thread's role (owner `admission::ROLE`, writers §3.2); the window thread's phase (owner `admission::PHASE`, writers §4.2); the refusal counters (owner `admission`) | the owner-thread token exists; no door takes it yet | advances D-2 (inventory: the registry, generated A§5.3); the D-33 and D-42 version notes; the R-G 1 rows are opened by the owner's ruling when D-2 closes, not here | `FolioApp::new_events`, `settle_quit`'s `Write` and `Abandon` arms, `App::finish` and `fn main` become writers of the phase; every reader of "is this the window thread" is `admitted` (none other exists) | **yes**: "which kind of thread is this" gets an owner where it had none — this note |
| A1b | a thread's role (the door becomes its writer for workers) | **thread**: `bt_platform::spawn_at_priority`, contract changes; **hand-off**: `ShellThread::enter` takes `&WorkerCtx`, the verbs become private | advances D-2 | none: the same 24 threads start at the same points | **yes**: the thread door now also owns "is a worker" — this note |
| A1c | a thread's role (15 or 18 more threads get one) | **thread**: 15–18 more sites through the door | repays A§6's `folio-web-thumb` bypass and the five unnamed `bt-app` spawns (§12 Q1 decides three of them); no scheduling debt changes | none | no |
| A1d | none moved; each converted door's effect stays with its owner | every owner-thread door of B§R-A: GPU present, §5.2 natives, rows 5, 11–13, 15–18 wrappers, each with its registry line | advances D-2; rows 11–12's wrappers recorded against D-43 and D-44 (their location, not their move) | `Refused` is a new outcome of each converted door; every caller that assumed "the door always runs" is listed in the brief | no |
| A1e | none | none; the guard reads the doors | adds the `Drop` exception rows (one per type in §7 departure 4, except `DirWatch` which is D-40) | none | no |

---

## Revision 2026-09-26 (b), after the GLM review

Review: `docs/plans/design/thread-door-review-glm-2026-09-26.md` (GLM, static,
at `61ac2d93`), verdict **adopt with changes**. It confirms the census (46
sites: 24 through the door, 22 bare, plus the pool) and accepts all seven §7
departures, four of them with an obligation (P1–P4). This revision is
appended; the text above stays as written, and **where the two differ, this
section rules**. The coordinator ruled on each of the four; each is taken in
order, checked against the code at `78a3699a`, and marked adopted or refused.
None is refused.

The review's method note is kept for whoever re-counts: `crates/bt-platform/src/lib.rs`
contains a NUL byte, so ripgrep treats it as binary and silently skips it (both
door definitions, the Windows `DirWatch` spawn and its `Drop`). The census of §5
searched that file directly; a re-count must too.

### (b)1 · P1 — the phase has no road from `Starting` to `Exiting`: adopted

**Checked.** `loop_running()` fires only in `FolioApp::new_events` on
`StartCause::Init` (§4.2). A launch whose event loop fails before its first
callback returns from `run_app` with the phase still `Starting`. §4.2's
`exiting()` row admits only `Running → Exiting`, so `fn main`'s call is refused
and counted, and §4.3 then refuses rows 15–17 during that teardown — the trace
flush and both bounded waits skipped, and a violation counted against a
legitimate call. §4.2's own second bullet names the road. I could not show that
winit never fails before `Init`, so the road stands.

**The change.** §4.2's `exiting()` row becomes:

| writer | transition | pinned call sites |
|---|---|---|
| `exiting()` | `Running → Exiting` **or `Starting → Exiting`**; idempotent from `Exiting` | unchanged: `settle_quit`'s `Write` arm, `App::finish`, `fn main` after `run_app` |

`quit_abandoned()` still admits only `Exiting → Running`. A1e still pins the
set of call sites, so the wider transition adds no writer.

**M4i gains the early-error sequence**, as the review wrote it, as a
fourth executed case in §9.3's M4i row (A1a):
1. a plain thread enters `enter_window_thread()` (phase `Starting`);
2. a `Running`-only probe door is `Refused` with `phase: Some(Starting)`;
3. `exiting()` is accepted from `Starting` (no counter rise);
4. an `Exiting` probe door (a row-15–17 shape) is admitted and runs.

**Adopted.**

### (b)2 · P2 — departure 5's "exclude" claims more than the M14 fence does: adopted, option (a)

**Checked.** §9.1's fence refuses the *names* `WorkerCtx` and `WaitToken`
inside `unsafe` blocks, `unsafe fn`s and the listed expressions. In `bt-platform`,
the one crate where `unsafe_code` is lowered, an inference-typed fabrication
(`let x = unsafe { std::mem::transmute(0usize) }; ShellThread::enter(&x)`) names
neither type inside the expression; the type is inferred at `enter`'s parameter,
outside the fence. `#![forbid(unsafe_code)]` covers `admission` only.

**The change.** §7 departure 5's sentence "the only way to reach a worker-only
door off a worker is a fabricated `WorkerCtx`, which `#![forbid(unsafe_code)]`
and the M14 fence exclude" is replaced by:

> Off a worker, a worker-only door is reachable only through a fabricated
> `WorkerCtx`. **Named fabrication is refused**: `admission` forbids `unsafe`
> (compiler), and M14's fence refuses `WorkerCtx`, `WaitToken` and
> `admission::doors` named inside any `unsafe` block, `unsafe fn` or
> `transmute`/`zeroed`/`MaybeUninit`/`read` expression. **What remains is an
> inference-typed `transmute` inside `bt-platform`'s own `unsafe`**, whose
> type is fixed at the door's parameter rather than written. That is
> deliberate evasion. It is below the accident bar the guard targets, and it
> is named here as the fence's limit, as B§R-A names the lint's.

§2.4's table's `unsafe` row reads the same way. The departure itself stands:
`expect_worker()` is still not added, and **no role read is put inside a
worker-only door** — not even as telemetry (the review's option (b) is not
taken). A read that no safe program can make fire has no red test, and the case
it would report is the one this paragraph names as deliberate.

**Adopted** (option (a)).

### (b)3 · P3 — the `Drop` exception list is incomplete and names no shrinkers: adopted

**Checked.** `impl Drop for VideoSeats` (`bt-app::video_seat`) calls
`shutdown_all()`, which takes every seat and calls `VideoSeat::shutdown` →
`Engine::shutdown`. That is the same drop → function → blocking-teardown shape
as the listed `VideoSeat`, and the one-call indirection §7 departure 4 itself
says a lexical check misses. Every other `impl Drop` the review read (`Taskbar`,
`SystemSettingsWatch`, `ShellThread`, the pickers, `ImeSystemCaret`,
`Apartment`) removes a subclass or releases an interface and does not block.

**Why each row needs a shrinker.** As the review says, *"With a shrinker per row
the list is a debt ledger; without one it is a permanent rule."* A shrink-only
list with no row obliged to shrink is the list-shaped debt D-2 records against
§5.3, in a new place.

**The list, replacing §7 departure 4's.** A1e lands it as the guard's declared
exception set. Each row carries either its repaying ticket and version, or
"ruled to stay" in §5.3's register. A1e adds a ledger row for each row that has
none. `structural-debt.md` held no row for any of these but `DirWatch` (D-40),
checked on `78a3699a`.

| type (crate) | what the `Drop` waits on | shrinker | version |
|---|---|---|---|
| `DirWatch` (`bt-platform`, Windows and macOS) | `thread.join()` on `bt-dir-watch` | **B7** — *A macOS directory watch starts and retires without the window thread waiting*: `retire(self, &WorkerCtx)` (D-40). The Windows arm is repaid by the same ticket's shared retirement door | 0.4.6 |
| `AttentionPipe` (Windows and Unix) | `listener.join()` on `folio-attention-endpoint` | new ticket *An endpoint is retired through an explicit door, not by its drop* (new row) | 0.4.7 |
| `LaunchPipe` (Windows and Unix) | `listener.join()` on `folio-launch-endpoint` | the same ticket (the same new row) | 0.4.7 |
| `video::engine::Engine`, `macos_player::Engine` (`bt-platform`) | `self.shutdown()` → the engine thread's join | new ticket *A video engine is shut down through an explicit door, not by its drop* (new row) | 0.4.7 |
| `VideoSeat`, **`VideoSeats`** (`bt-app`) | `shutdown()` / `shutdown_all()` → `Engine::shutdown` | the same ticket (it repays both layers together) | 0.4.7 |
| `PtySession` (`bt-pty`) | `self.shutdown()` → the child's exit and the reader/writer joins | new ticket *A shell is taken apart only through `retire_within`, never by a drop on the window thread* (new row; beside D-43/D-44's session-lifecycle rows, not merged into them). Today the drop runs on `pty-retirement` except when that thread cannot be started, where `retire_within` falls back to the drop on the caller | 0.4.7 |

No row is "ruled to stay": each of these waits can reach the window thread on
some road (a window close, a seat closed, an endpoint dropped at quit), and none
of them has a ruling that it may.

**Adopted.**

### (b)4 · P4 — the meter's unwind policy mis-cites `during`: adopted, `during`'s discipline

**Checked.** `hang_watch::during` is `enter` → `work` → `at(parent)` with no
guard, and the neighbouring `enter` states why: *"Not a guard type: a guard
would run on the unwind path too, and the one thing this module must never do
is add a `Drop` to a thread that is already in trouble."* So `during` does not
keep its stack balanced on unwind, and §7 departure 2's parenthetical ("as
`during` keeps its stack balanced") says the opposite of the precedent it cites.

**The change.** §7 departure 2 and §4.4 step 4 are replaced:

- `admitted` has **no drop guard**. `leave` is called only when `work` returns.
  A panicking `work` unwinds straight out of `admitted`. No meter code runs on
  the unwind path, and no first-party `Drop` is added to a thread that is
  already failing.
- **The stack after a panicking admitted call:** the door's station stays
  pushed on the window thread's station stack, above whatever was there when
  `admitted` was entered. `leave` was never called, so no per-call record is
  kept for that call (A3's histograms skip it; the panic hook's log line is its
  record).
- **The consequence, stated as the intended one:** the next hang report from
  that thread names the door as the innermost station. That is the more
  truthful report — the last admitted call on that thread did not come back —
  and it is what `during` already produces for a panicking `Scope`.
- **The enter half stays.** Naming a call that never returns is what
  `during`'s enter-before-work buys today (§7 departure 2's first reason, which
  the review accepts).

§9.3's meter test changes with it: a probe door whose `work` panics calls
`enter` once and `leave` zero times, and the station accessor still shows the
probe's station afterwards on that test thread.

**Adopted.**

### (b)5 · §12 Q1 — settled by the code, not an owner question

The review answers Q1 yes, and the argument is the code's:
`attention_wire::payload_on_stdin` reads `Lane::Attention` bytes, and the two
explorer threads deploy and read a package through `quiet_command`. These are
first-party effects that A2's lint must see in every process. Exempting them by
process now would bring the exemption back as a lint exemption one ticket
later. **Settled:** the three door-process threads
(`explorer_menu::remove_from_explorer_menu`, `explorer_menu::cleanup_registrations`,
`attention_wire::payload_on_stdin`) go through the door in **A1c, at
`ThreadPriority::Normal`** (their band today). Their main threads stay `Unset`
(§3.2). A1c therefore converts **18** bare sites. Afterwards the bare set is
`bt-pty`'s 4 plus the resample pool, and A§0.1's bare count reads 4.
ARCHITECTURE §6's "Whether a door process's thread owes the door is not ruled"
is replaced in A1c's commit by the sentence above.

§11's A1c row reads accordingly: prerequisite A1b only; "15 or 18" becomes 18.
§13.2's A1c row repays A§6's `folio-web-thumb` bypass and all five unnamed
`bt-app` spawns.

### (b)6 · §12 Q2 — still the owner's, with the review's one line

Q2 stands as written in §12, with one line added for the owner to rule on
explicitly: *`folio-video-frame` would go `BelowNormal` while
`folio-video-engine` stays `Normal`.* That is defensible, because
`within_budget` answers one frame question at seat birth rather than feeding
the playing loop. But it is the one row where the reason "the picture somebody
is watching" and the proposed band point in different directions.

### (b)7 · This revision's own architecture impact

(a) None. (b) None. (c) No row repaid or added by this commit. A1e will add a
ledger row for each §(b)3 row without one: the endpoints, the video engines and
seats, and `PtySession`. (c′) None. (d) No. A1a and A1b's (d) = yes is
unchanged (§13.2).

---

## Revision 2026-09-26 (c), after the Codex review

Review: `docs/plans/design/thread-door-review-codex-2026-09-26.md` (Codex,
static, at `6090cfbd`), verdict **adopt with changes**. It adopts the lent
`WorkerCtx`, typed owner admission, the 46-site census and the atomic 24-site
migration. It raises eight findings (P1–P8) and answers §7 and §12. This
revision is appended. Sections 0–13 and revision (b) stay as written, and
**where this section differs from them, it rules**. The coordinator ruled on
each finding and on §12. Each is taken in order, checked against the code at
`78a3699a` where it names code, and marked adopted or refused. None is refused.

### (c)1 · P1 — the ticket graph does not support its landing claims: adopted

**Checked.** Both spawner bodies on the base set only the band. `admission`
does not exist, `ShellThread::enter()` takes nothing, and `hang_watch::start`
installs no meter. After A1a alone, therefore, the 24 door threads and the bare
threads are still `Unset`. §11's A1a sentence "every thread that runs our code
is classified" is false for that landing. M4i′'s worker half needs A1b's
producer. A1e's one-`WorkerCtx`-literal check needs A1b's literal. A1d's
worker-side M5 needs A1b. And A3 "lands after A1a" departed from budget §C-8
without saying so.

**§11 is replaced by this table.** Sizes are Codex's assessments; each is
provisional only where its own row says so.

| id | size | prerequisites | lands alone as — and what is still pending when it does |
|---|---|---|---|
| A1a | M | the 0.4.5 tag; this note reviewed | **Infrastructure only**: `admission` (`Role`, the phase and its writers per (c)5, `enter_window_thread`, `enter_callback` per (c)7, `enter_standalone_main` per (c)6, `Door`, `doors`, `WaitToken`, `admitted`, `Refused`, counters, `Meter` and `Cookie` per (c)2), the registry and generated A§5.3, the D-33/D-42 notes, A§0.1's counts. **Pending, and said so in A§6 and RULES 52 as landed:** worker threads are still `Unset` until A1b (roles are `Window`, `Callback`, and `Unset` for every spawned thread). No door takes a token or a capability yet. Tests: M4a–M4h, M4g′, M4g″, M4i (window half, including (b)1's early-error sequence and (c)5's cases), the meter tests of (c)2. The worker half of M4i′ moves to A1b |
| A1b | S–M | A1a | The 24 heads and the 3 band tests in one commit (§6.1). Then, **before the ticket is declared done**, the raw hand-off verbs become private, and **every caller of them changes in the same ticket**: `a_refused_handoff_raises_the_same_words_it_did_before` and any other test that calls a raw verb directly, plus the recording-lane factories that `HandoffLane::start`'s tests pass as `make_executor` (now `FnOnce(&WorkerCtx) -> E`). Tests: M3, M3r, M3r′, M4g, the worker half of M4i′, the worker-side M5 (moved here from A1d), and (c)8's worker controls. **Pending:** the bare threads (A1c), the owner doors (A1d), the prohibitions (A1e) |
| A1c | S | A1b | The 18 swaps of (b)5, **each preserving its site's spawn-failure behaviour exactly**. The five `std::thread::spawn` sites panic on a refused thread today, so they keep that with `.expect(…)` carrying the old message. The `Builder` sites that propagate or ignore the `io::Result` keep doing so, and no site discards a `Result` it did not discard before. Every thread keeps its current band ((c)9). RULES 53's exception clause lands here ((c)9). Repays the classification and entrance debt (A§6's bypass paragraph). **Scheduling debt is not repaid.** |
| A1d | M, **provisional until (c)3's table is reviewed** | A1a | (c)3's door table converted, row by row. Its worker-side M5 now lives in A1b, so A1d has no A1b prerequisite. Owner-side M5 and every row's test witness ship here. **Pending:** the rows (c)3 marks deferred |
| A1e | M, for the stated inventories only — it is not a whole-program blocking analysis | A1b (worker-construction assertions), A1d (owner-signature checks) | §9.1's A1e rows as amended by (c)4 and (c)8. **It states in A§6 which guarantees stay pending until A2:** the lint on raw effects, the `file_writes`/`wait` doors, `file_reads`' execution-level design ((c)6), and the transport doors in `bt-pty` ((c)6) |

**Outside A1.** A2 keeps its prerequisite on the completed owner conversions
(A1d) as well as A1b, A1c and A1e. **A3 lands after all of A1, A1a to A1e**, as
budget §C-7 and §C-8 say. The narrower "after A1a" milestone in §11 is
withdrawn, because an installed but unused meter provides no per-call coverage.
The owner's D-2 ruling is unchanged: D-2 closes after A1 (all five), A2 and A3,
and the two named rows open then.

**Adopted.**

### (c)2 · P2 — the meter needs a return context: adopted

**Checked.** `hang_watch::enter` returns `Location::Resume { station, node,
scope }`. `during` keeps that value and passes it to `at`, and
`Heartbeat::resume_at` restores all three parts. `hang_watch_detail::Ledger` is
a bounded, aggregated call tree, not a per-invocation stack. `enter` and `at`
each read the clock. The run footer is `diagnostics::run_footer`, written by
`fn main` through `trace_sink::stderr_line`; `watch_forever` has no exit path.
§7 departure 2's `Meter { enter: fn(DoorKey), leave: fn(DoorKey, Instant,
Instant) }` throws the `Location` away, and a door key cannot rebuild it.

**The change.** This replaces §7 departure 2's `Meter` and §4.4's steps 2–4.
Revision (b)4's no-guard unwind policy stands.

```rust
// bt_platform::admission
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cookie(u64);            // opaque; built and read only by the meter's owner
impl Cookie { pub fn new(raw: u64) -> Self; pub fn raw(self) -> u64 }

pub struct Meter {
    pub enter: fn(DoorKey) -> Cookie,
    pub leave: fn(DoorKey, Cookie, Instant, Instant),
}
pub fn install_meter(meter: Meter) -> Result<(), AlreadyInstalled>; // once per process
```

`admitted` calls `enter` and keeps the `Cookie` on its own stack. It reads
`start` and `end` itself around `work`, then calls
`leave(key, cookie, start, end)`. `Cookie` is plain data, so it carries no
authority, and `admission` never interprets it.

**`hang_watch`'s adapter.** This is new state that `hang_watch` owns.

- **The stack.** `hang_watch` keeps a window-thread-local adapter stack of
  `Location` values of fixed capacity, `ADMITTED_DEPTH` = 16. `enter` pushes
  the `Location` that `hang_watch::enter` returned, and returns
  `Cookie(generation << 8 | depth)`. The generation is a per-thread counter
  that rises on every push.
- **The matching leave.** `leave` pops the entry only when the cookie's depth
  and generation match the top. It then calls `at(location)`, which restores
  station, node and scope exactly as `during` does.
- **Mismatches.** A cookie that does not match the top is a panic left
  unwound under (b)4's policy, or corruption. The adapter then pops down to and
  including the matching entry, if one exists, and counts the mismatch. If no
  entry matches, it restores nothing and counts.
- **Overflow.** Past 16 nested admitted calls, `enter` returns
  `Cookie::OVERFLOW` and pushes nothing, `leave` restores nothing for it, and
  the overflow is counted. Nothing panics. The station is still charged,
  because `hang_watch::enter` still ran.
- **Nesting under `during_pane`.** The saved `Location` is the one the pane
  scope produced, so `at` restores the pane scope unchanged.
- **Recursive or same-door admission.** Each call has its own stack entry and
  cookie. The door key is never used to find an entry.
- **Unwind.** Under (b)4, a panicking `work` leaves its entry on the adapter
  stack. The next matched `leave` below it pops through it (counted). Codex's
  caveat is taken: the next hang report names the door **until** a later
  `at`, a scope restoration or a turn's reinitialisation replaces the station.
  It is not "forever".

**Timestamps and their cost.** `leave` receives `admitted`'s two `Instant`s
and records the inclusive interval from them. `hang_watch::enter` and `at`
still take their own clock reads for the exclusive station charge. So an
admitted call costs **four** clock reads: `enter`, `start`, `end` and `at`.
`during` today costs two. §10's "the clock reads do not double" is withdrawn.
A3 measures that cost in B§R-C's four regimes. Letting `enter` and `at` take
`admitted`'s timestamps is an A3 option, not settled here.

**Refusal summary.** The writer is `diagnostics::run_footer`, as `fn main`
writes it through `trace_sink::stderr_line`, and the footer gains the refusal
total. This replaces §3.4's "the exit line `hang_watch` already writes". The
door-process and early-exit roads print no footer today, and none is added.

**Tests (A1a).** All are deterministic, on a test thread with a test meter or
`hang_watch`'s own:
- station, node and scope are restored after a nested admitted call inside
  `during` and inside `during_pane`;
- two same-door admitted calls, recursive and back to back, restore correctly;
- the 17th nested call counts an overflow and restores its parent correctly;
- a caught panic in `work` leaves the entry, and the next outer `leave` pops
  it (counted);
- an inner call's `[start, end]` lies inside its outer call's.

**Adopted.**

### (c)3 · P3 — owner conversion needs an effect-level inventory: adopted

**Checked.**
- `Runtime::flush_pending_pty_resize` loops over tabs and leaves into
  `release_due_leaf_resize` and then `commit_leaf_resize`. `commit_leaf_resize`
  holds the one `pty.resize(pty_size(…))` call, after the actor may have
  reflowed and before the transaction is reconciled.
- `create_leaf_session` does a lot of preparation before
  `PtySession::spawn_shell_in`.
- Row 15's `bt_pty::wait_for_retirements` cannot take a token, for the same
  crate-edge reason as rows 11–12.
- `SessionStore::close` calls `flush` (row 16's `wait_for`) **and**
  `SessionWriter::close`, which has its own bounded polling and join.

**The change.** A1d converts exactly this table. §8's "A1d wraps them in
`admitted` where they are owner-thread rows" and §11's A1d list are replaced
by it. *Metric* says whether one admission measures one native call or a
declared batch. *Refusal* says what the caller does with `Refused`; in every
row the refusal happens **before** any state the row mutates.

| row · registry identity | effect-containing function (the door) | typed signature | real callers | phases | metric | refusal | witness |
|---|---|---|---|---|---|---|---|
| 11 · `PtyBirth` | new `bt-app` `pty_door::spawn_shell(token, command, size, wake)`, wrapping `PtySession::spawn_shell_in` only (the preparation in `create_leaf_session` stays outside) | `WaitToken<'_, doors::PtyBirth>` by value | `create_leaf_session` | Running, Exiting | single call | returned as the spawn's error (the pane shows its existing birth-failure state) | a headless pane birth through `create_leaf_session` asserts the admitted station and a live session |
| 12 · `PtyResize` | new `bt-app` `pty_door::resize(token, pty, size)`, called **inside `commit_leaf_resize` at the existing `pty.resize` statement**. The admitted boundary is that one raw call, not the outer flush | `WaitToken<'_, doors::PtyResize>` | `commit_leaf_resize` (via `release_due_leaf_resize` ← `flush_pending_pty_resize`) | Running, Exiting | single call **per leaf** | mapped to the resize's existing error road, at the same point in the transaction as today's `resize` error, so reflow-before and reconcile-after ordering is unchanged | two panes resized in one flush give two admitted records; a refused resize leaves the transaction exactly as a failed `resize` does today |
| 15 · `PaneRetirementWait` | new `bt-app` `pty_door::wait_for_retirements(token, deadline)` wrapping `bt_pty::wait_for_retirements` | `WaitToken<'_, doors::PaneRetirementWait>` | `settle_quit`'s `Retire` arm | Exiting | single call | treated as "0 still going" is **not** allowed: a refusal is logged with the count unknown, and quit proceeds as on a timeout | the quit driver's `Retire` step through the real `settle_quit` |
| 16 · `SessionWriteWait` | `SessionWriter::wait_for` | `WaitToken<'_, doors::SessionWriteWait>` | `SessionStore::flush_judged` (from `QuitStep::Write`; from `SessionStore::close` ← `App::finish`) | Exiting | single call | `SaveRefusal` with the existing stalled meaning, so quit proceeds as on a timeout | `flush_judged` through `QuitStep::Write` asserts the admitted record |
| 16b · `SessionWriterRetire` | `SessionWriter::close` (its bounded poll and join) | `WaitToken<'_, doors::SessionWriterRetire>` | `SessionStore::close` | Exiting | single call | the writer is left to process exit, exactly as its own budget-expired branch does today | `App::finish` through the real close road |
| 17 · `TraceFlush` | `trace_sink::flush` | `WaitToken<'_, doors::TraceFlush>` | `fn main` after `run_app`; `Shutdown::drop` ((c)4) | Exiting | single call | lines still queued are lost, as on its timeout | `fn main`'s tail shape, driven by a test calling the same sequence on a window-entered test thread |
| 18 · `LaunchHandOver` | `launch_wire::hand_over` | `WaitToken<'_, doors::LaunchHandOver>` | `fn main` | Starting | single call | `None`: open a window, as every other `None` does | the existing hand-over tests on a window-entered thread |
| 9 · `Present` | `WindowRenderer::present_frame*`, `configure_window_surface` | per door type | `Runtime::present_seats_and_commit` and its configure road | Running, Exiting | **declared batch**: one admission per window's present-and-commit | the frame is not presented; the existing lost-frame road | a headless present asserts one record per window |
| §5.2 natives: `TitleFlush`, `ImeCaretFlush`, `FocusWindow`, `SetVisible`, `SetCursor`, `CompositorCommit` | `Runtime::flush_title`, `flush_ime_cursor_area`, and one `bt-app` door each for focus, visibility and cursor at their existing single call sites; the DirectComposition commit inside `present_seats_and_commit` | per door type | as today | Running, Exiting | single call | the wanted value stays wanted and is retried next turn (title, IME); the others skip the call | each converted door reached through its real caller asserts its effect |
| 21 · `WebController`, `WebEnvironment` | `WebHost::request_controller`, `WebSeat::start_environment` | per door type | `WebSeat::step`, `Runtime::warm_web_engine`, `make_spare_web_controller` | Running | single call | the page stays in its "coming up" state and the step retries | ticket 54/60's warm and spare tests on a window-entered thread |
| 5 residue · `FontFamilyLookup`; 13 residue · `WindowPlaceProbe` | `bt_platform::monospace_family_named`; `Runtime::observe_window_place`'s probes | per door type | as today | Running, Exiting | single call | the previous answer is kept | the existing tests on a window-entered thread |

**Deferred, marked in the registry** (their lines exist with status `open`, and
A1d does not convert them):
- rows 2, 3, 4 and 20 — B4, B5, B8; their owner-thread residue is admitted
  when those tickets move the work;
- row 7 — B6;
- row 8 — B7;
- row 10 — B9;
- row 19 — bound ring operations, no token (B§R-A: never waited on by design);
- row 22's residue, `flush_ime_cursor_area`, is in the table above.

§8's "No move" paragraph is corrected to match: A1d wraps only the rows in the
table.

**Adopted.** A1d's size stays provisional until this table is reviewed.

### (c)4 · P4 — the `Drop` inventory through real destructor chains: adopted

**Checked.**
- `trace_sink::Shutdown::drop` calls `flush`, which reaches `flush_sink`'s
  `recv_timeout`, polling sleeps and a conditional join. `fn main` holds
  `_trace_shutdown` across `builder.build().context(…)?`.
- `attention_wire::open` and `launch_wire::open` put successful endpoints into
  static `OnceLock<Option<_>>` values, and the product never drops them. The
  Unix modules say so.
- `PtySession::shutdown` takes the writer (a detach, **not** a join), bounds
  the reader's join, and drops the native master. `PtySession::drop` also
  finishes an input dump, which writes and calls `sync_data`.
- A resolved lint on `join` cannot see a `Drop` that calls a permitted helper.

**The inventory, replacing (b)3's table.** *Product reach* says how the
product drops the value; public-API or test-only destruction is not counted
as reach. Each row's *authorized chain* is exact: A1e pins these
destructor → helper edges, and nothing else under that `Drop` may reach
vocabulary.

| type | authorized chain | product reach | repayment · version |
|---|---|---|---|
| `DirWatch` (Windows, macOS) | `drop` → `SetEvent`/`stopper.signal()` → `thread.join()` | a directory watch retired on the window thread (row 8) | **B7** (D-40) · 0.4.6. Not "remove the join": the handles must outlive the thread |
| `trace_sink::Shutdown` (**added**) | `drop` → `flush` → `flush_sink` → `recv_timeout`, `sleep`, `join` | only on `fn main`'s early `?` return from `EventLoopBuilder::build`, which (c)5 now admits in `Exiting`. The normal exit calls `flush` explicitly and leaves through `leave_process` | new ticket *The trace writer is retired through its admitted flush door, never by a drop* · 0.4.7 |
| `AttentionPipe`, `LaunchPipe` (Windows, Unix) | `drop` → `listener.join()` | **none in the product**: the successful endpoint lives in a static for the process's life. (b)3's "an endpoint dropped at quit" is withdrawn. A public-API and test-only destruction path exists | new ticket *An endpoint is retired through an explicit door, not by its drop* · 0.4.7 |
| `video::engine::Engine`, `macos_player::Engine` | `drop` → `shutdown` → the engine thread's join | a seat closed on the window thread | new ticket *A video engine is shut down through an explicit door, not by its drop* · 0.4.7 |
| `VideoSeat`, `VideoSeats` | `drop` → `shutdown` / `shutdown_all` → `VideoSeat::shutdown` → `Engine::shutdown` | a pane or window closed on the window thread | the same ticket · 0.4.7 |
| `PtySession` | `drop` → `shutdown` (writer taken and detached; reader joined within its bound; master dropped) → input dump `finish` (write, `sync_data`) | on `pty-retirement`. On the caller only when `retire_within` cannot start that thread | new ticket *A shell is taken apart only through `retire_within`, never by a drop on the window thread* · 0.4.7. Not "make `Drop` a no-op": the thread-refused road must still tear the session down |

**Fencing the indirect paths.** This replaces §7 departure 4's "only A2's lint
… closes it", which is withdrawn: Clippy supplies no call-graph prohibition.
A1e's check reads each `impl Drop` body in the product universe and resolves
its direct calls by item, using `bt_source`'s item identity. It then asserts:

1. a `Drop` whose body calls any function other than the row's pinned first
   edge is red, if that function is a door, reaches vocabulary directly, or is
   itself a pinned chain member;
2. each pinned chain member's body calls only the next pinned member and
   non-vocabulary items;
3. a `Drop` not in the table that calls a registered door or a chain member is
   red.

This is a pinned-edge check, not a whole-program analysis, and the note claims
no more. **Mutation (A1e):** add a call from `VideoSeats::drop` to a new helper
that calls `SessionWriter::wait_for`, or add a `sleep` to `Engine::shutdown` —
both must go red. The passing control is today's chains.

**Adopted.**

### (c)5 · P5 — quit cancellation and the pre-loop trace guard: adopted

**Checked.**
- `Quit::answer(Cancel)` goes from asking to `Abandoned`, and `Quit::saved`
  does the same after an incomplete save. Both reach `settle_quit`'s `Abandon`
  arm **without passing `Write`**, so admission is still `Running`, and §4.2's
  `quit_abandoned()` would count a violation for an ordinary gesture.
- `_trace_shutdown` is built before the fallible `builder.build()`.

**The change to §4.2's table.**

| writer | transition |
|---|---|
| `quit_abandoned()` | `Exiting → Running`, **or idempotent from `Running`**: Cancel and an incomplete save change nothing and count nothing |
| `exiting()` | as (b)1 (`Running` or `Starting` → `Exiting`, idempotent from `Exiting`), with **one more pinned call site**: `fn main`'s event-loop build error arm calls `exiting()` explicitly before returning its error, so `_trace_shutdown`'s drop reaches row 17's flush admitted in `Exiting` (the (c)4 `Shutdown` row) |

The `?` on `build()` becomes a `match` whose `Err` arm calls `exiting()` and
then returns. A1e pins that call. **Ordinary doors are not broadened to
`Starting`.**

**Tests (A1a), through the real quit driver.** `quit::Quit` and
`FolioApp::settle_quit`'s step loop are driven on a window-entered test thread:

| case | expected |
|---|---|
| Cancel | `Running` before and after; no count |
| an incomplete save | `Running`; no count |
| a refused session write | `Exiting` after `Write`, `Running` after `Abandon`; no count |
| successful retirement | `Exiting` through `Retire`; rows 15–17 admitted |

Plus the build-error road: `exiting()` from `Starting`, then `TraceFlush`
admitted.

**Adopted.**

### (c)6 · P6 — the threads left `Unset` need an explicit A2 contract: adopted

**Checked.**
- `bt-pty` has no `bt-platform` edge.
- `InputRing`/`OutputRing` wait on condition variables, and `join_within`,
  `reap_within`, `Retirements::wait_within` and `spawn_dump_publisher` contain
  listed waits and sleeps.
- `attention_wire::payload_on_stdin`, `explorer_menu::remove_from_explorer_menu`
  and `explorer_menu::cleanup_registrations` each `recv_timeout` on their
  process's main thread, which stays `Unset` after (b)5.

**The contract this note now carries** (implemented by A2 and A1a as marked):

1. **`bt-pty`'s transport waits are registered transport doors inside
   `bt-pty`.**
   - Each wait function (the ring waits, `join_within`, `reap_within`,
     `Retirements::wait_within`, the dump publisher's sleep and sync) carries
     its own `#[expect(clippy::disallowed_methods, reason = "<door id>")]` and
     is a registry line with role **`Transport`**. It takes no capability.
   - These lines are listed as effects **outside the admission invariant**,
     with one debt row: *the PTY transport's waits are fenced by owner and
     count, not by thread authority*.
   - A2's lint applies to `bt-pty` like any crate. Nothing is suppressed
     crate-wide, and no `Unset` thread is inferred to be a worker.
   - The rayon pool stays `Unset`, for (c)'s reason (pure resampling). That
     reason does not extend to transport waits.
2. **A standalone process's main thread enters through a sealed entry**
   (A1a):

   ```rust
   pub fn enter_standalone_main<R>(
       name: &'static str,
       body: impl FnOnce(&WorkerCtx) -> R,
   ) -> Result<R, Refused>;
   ```

   - It is callable **once per process** (an `AtomicBool`), and only on an
     `Unset` thread. It sets `Worker(name)` permanently and lends that
     process's one `WorkerCtx` to `body`, built by the same private
     constructor as the door's. This is the second and last `WorkerCtx`
     literal; A1e's count becomes two, each pinned.
   - A second call, or a call on a thread with a role, is `Refused` and
     counted, and `body` does not run.
   - Pinned product callers (A1e): the attention verb's payload reader
     (`attention_wire::payload_on_stdin`'s caller in the `attention` door),
     `explorer_menu::remove_from_explorer_menu` and
     `explorer_menu::cleanup_registrations`.
   - Their `recv_timeout` waits then go through A2's worker `wait` doors like
     any worker's.
3. **`file_reads`' execution-level design is an A2 decision.** One paragraph
   of options, not settled here:
   - `file_reads::Reader` performs its reads after construction, in `Read`
     calls the caller makes later, while `opaque` runs its closure
     synchronously. A capability checked at `open`/`Reader::new` alone would
     authorize construction, not the reads, which is the returned-effect shape
     §C-5 refuses.
   - The options are:
     - (a) `Reader` carries a borrowed `&WorkerCtx` for its whole life, so it
       is `!Send` and cannot outlive the worker body;
     - (b) the reading methods take a capability per call;
     - (c) window-thread lanes get their own owner doors with tokens, and only
       worker lanes take a context;
     - (d) a combination: (a) for worker lanes, (c) for the inventoried
       window-thread reads.
   - A2 chooses, after inventorying which lanes are read on which thread (§6.3).

**Adopted.**

### (c)7 · P7 — the callback inventory: adopted

**Checked.**
- `macos_video::read_first_frame` → `generate` calls the synchronous
  `copyCGImageAtTime_actualTime_error`. The module discusses the asynchronous
  API only to explain why it does not use it, so no completion exists.
- Windows `Notifier::show` (`bt-platform` `lib.rs`) registers a
  `ToastNotification::Activated` `TypedEventHandler` that queues the
  activation and calls the application's wake. `Notifier::new` allows delivery
  on any thread.
- `bt-render`'s `DeviceResources::mint` installs
  `device.set_device_lost_callback` and `device.on_uncaptured_error`.
  `bt-render` depends on `bt-platform`, so it can name `enter_callback`.

**§3.3's table is corrected.** The rows below replace or add to the old ones;
entry symbols are pinned by A1e.

| callback | entry symbol | delivery | role after A1a |
|---|---|---|---|
| ~~`AVAssetImageGenerator` completion~~ | **removed**: `macos_video::generate` is synchronous and runs on the `folio-video-frame` worker (`Worker` after A1c) | — | — |
| `AVPlayer` end notification | `macos_player`'s observer `folioVideoDidPlayToEnd:` | notification queue | `Callback("video-end")` |
| **Windows toast activation** (added) | the `TypedEventHandler` closure registered in `Notifier::show` | any thread the platform picks | `Callback("toast-activated")`, or the thread's existing role if it has one |
| **wgpu device loss** (added) | the closure passed to `set_device_lost_callback` in `DeviceResources::mint` | wgpu's choice: synchronously on the thread that polls or submits (the window thread today), or a backend thread | the existing role if any, else `Callback("gpu-device-lost")` |
| **wgpu uncaptured error** (added) | the closure passed to `on_uncaptured_error` in `DeviceResources::mint` | the thread that made the failing call | the existing role if any, else `Callback("gpu-uncaptured-error")` |

The other rows of §3.3 stand, each now naming its entry symbol in A1a's brief:
- `install_console_ctrl_handler`'s `handle`;
- `Notify_Impl::EventNotify`;
- `macos_http`'s delegate methods;
- `macos_notify`'s completion block and delegate;
- `open_folder_in_finder`'s completion block.

**`CallbackScope`, specified.**

```rust
pub struct CallbackScope {
    restore: Option<Role>,           // Some(Unset) if this scope set Callback; None if it found a role
    _local: PhantomData<*const ()>,  // !Send, !Sync
}
pub fn enter_callback(name: &'static str) -> CallbackScope; // the only constructor
```

- A scope that **finds a role** (`Window`, `Worker`, `Callback`) is
  *non-owning*: it changes nothing and restores nothing.
- A scope that **finds `Unset`** is *owning*: it sets `Callback(name)` and
  restores `Unset` on drop, including on unwind. This is the one guard in
  `admission`. It restores a thread-local, runs no meter code, and is
  therefore not "work" in `hang_watch`'s sense.
- Because the scope is `!Send`, a scope made on an `Unset` thread cannot be
  moved into a door-started worker and dropped there. That closes Codex's
  counterexample, in which the drop would reset the worker's role while its
  `WorkerCtx` lived.

**Tests (A1a):**
- nested `enter_callback` on `Unset` (the outer scope owns, the inner does
  not; `Unset` after both);
- `enter_callback` on `Window` and on a worker (A1b), with the role unchanged
  inside and after;
- a panic inside an owning scope, caught, leaves `Unset`;
- a compile-fail pair: sending a `CallbackScope` to `std::thread::scope`'s
  spawn fails because of `!Send`, and the control drops it locally ((c)8's
  contract).

**Adopted.**

### (c)8 · P8 — the paired proofs, made property-isolating: adopted

**Checked.** On the pinned 1.94.1, rustdoc enables error-code checking only
for a nightly build (the collector's `ErrorCodes::from(…is_nightly_build())`),
as §7 departure 7 said. So a stable `compile_fail` block proves only that it
fails. Codex's overlaps are real:
- M3r and M4e mix `!Send`/`!Sync` with a `'static` obstacle;
- M4b's `&token` fails because of a borrow of a local;
- M4c's mutable `static` fails because of unsafe/static rules;
- M3r′ and M4f have no control;
- M5 names no `fn`-pointer pair;
- nothing tests double consumption;
- a recording executor proves no real affinity;
- shared atomics and the install-once meter are not isolated by fresh
  threads.

**The contract, adopted explicitly.** There is no `trybuild` and no stderr
matching. It is the **property-isolating paired-proof contract**:

1. **Each `compile_fail` case must be sensitive to removing the property it
   claims.** The brief states, per case, the one product edit that would make
   it compile, as a `MUTATION:` line, and the case contains no second obstacle
   that would survive that edit. A stable `compile_fail` block proves no
   error reason, and the note does not claim it does. The departure-7 canary
   only confirms the stable behaviour; it proves nothing about other mutants.
2. **Auto-traits get independent probes**, with no lifetimes involved:
   `fn is_send<T: Send>() {}` and `fn is_sync<T: Sync>() {}` instantiated at
   `WorkerCtx`, `WaitToken<'static, doors::X>` (the type exists at `'static`
   even though no value does), `CallbackScope` and `ShellThread`. Each is a
   `compile_fail` block, and its control instantiates the same probe at
   `u8`.
3. **Cross-thread transfer cases use `std::thread::scope`**, so no `'static`
   bound is involved. The cases are:
   - moving a `&WorkerCtx` into a scoped spawn (`!Sync`);
   - moving a `WaitToken` by value (`!Send`);
   - moving a `CallbackScope` (`!Send`);
   - moving a `ShellThread` (`!Send`).

   Each control is the same scoped spawn capturing a `u8`.
4. **Separate token tests, one property each:**
   - *higher-ranked escape*: `work` returns the owned token as `R` — the
     control returns `()`;
   - *safe outer storage*: `work` stores the owned token into an outer
     `let mut slot: Option<_> = None` declared before `admitted` — the control
     stores a `u8`;
   - *privacy*: `WaitToken { … }` and `WorkerCtx { … }` literals outside
     `admission` — the control is a call of the public road
     (`admitted(|t| door(t))`, a door-started body);
   - *wrong identity*: a `doors::A` token passed to a `doors::B` door — the
     control uses `doors::A`;
   - *double consumption*: `door(token); door(token)` — the control makes one
     call.
5. **Controls exist for every named variant.** The boxed closure and the
   `fn` pointer each get a compile-fail case and a compiling control. The
   **executed** indirect calls also run through both a `Box<dyn Fn>` and a
   `fn` pointer: on the worker side (A1b) a door-started body calls
   `ShellThread::enter` through each; on the owner side (A1d) each is refused
   on a worker and admitted on a window-entered thread.
6. **The hand-off producer control is the real refusal path.**
   `a_refused_handoff_raises_the_same_words_it_did_before` now runs through
   `HandoffLane::spawn` (the real `ShellThread::enter(ctx)` executor) with a
   target the OS refuses, not through a recording executor. `ShellThread`'s
   own non-transferability is (c)8 items 2 and 3.
7. **Isolation protocol for counters and the meter.**
   - `admission`'s counters are per door type, and each executed refusal test
     uses **its own** `#[cfg(test)]` probe door type. `Probe` is not shared, so
     "exactly one" deltas cannot race.
   - The process-wide meter is **not installed** in `bt-platform`'s unit
     tests; the meter tests use `admission::test_meter_scope`, a
     `#[cfg(test)]` thread-local override consulted before the global.
   - `bt-app`'s one `hang_watch` registration test runs in its own test
     binary target, or asserts only on its own thread's station stack.
8. **Duplicate window entry.** After `loop_running()`, a second
   `enter_window_thread()` on the same thread leaves the phase `Running`, with
   no reset to `Starting`, and counts. This is an A1a executed test.
9. **Typed assertions where the compiler can check.**
   - The registry equality instantiates each door type's constants in a
     generated `const` table
     (`const _: () = assert!(doors::X::ROW == …)`, or a test reading
     `<doors::X as Door>::ROW`) rather than parsing their text.
   - Each owner door's signature is checked by coercion:
     `let _: fn(WaitToken<'_, doors::X>, …) -> _ = door_fn;` in a test.
   - Source inspection stays for what only the source can show:
     - call-site inventories (the role and phase writers, `enter_standalone_main`);
     - the `WorkerCtx` constructors — counted by resolved item, covering
       `Self { … }` spellings and derives (a derive of `Clone`, `Default` or
       `Copy` on the type is red);
     - the unsafe/FFI policy;
     - the `Drop` edges of (c)4.

§9.1–§9.3's rows are amended accordingly. Where a row there and an item here
disagree, the item here rules.

**Adopted.**

### (c)9 · §12 Q2 — decided: thread role does not decide priority

**Decided by the coordinator, with the owner's delegation.**
- A1c is behaviour-preserving: every site keeps its current band.
- **RULES 53 gains, in A1c's commit, an explicit exception clause.** Under it,
  these stay at `Normal`:
  - playback engines (`folio-video-engine`, both platforms);
  - first-frame extraction (`folio-video-frame`);
  - launch and attention ingress (`folio-launch-endpoint`,
    `folio-attention-endpoint`);
  - clipboard saves (`clipboard-picture`);
  - the three standalone-process workers.
- Observation and probe threads now at `Normal` move below normal in a separate
  **0.4.7** ticket: *Observation threads started at `Normal` move to the
  workers' band*. They are `bt-dir-watch`, `folio-video-prewarm`,
  `folio-video-canplay`, `folio-web-thumb`, and the explorer probe and
  deployment threads.
- RULES 53 and A1c's brief state that the portable priority setter returns
  `false`. **On macOS a band is requested, not enforced.**

**(b)6's video comparison is corrected.** `video::within_budget` and
`video_portable::within_budget` serve first-frame extraction through
`first_frame`. `Engine::open_on` and `macos_player::Engine::open` start the
playback threads separately. These are **separate workloads**, first frame and
playback. They are not "a frame question at seat birth" set against the
playing loop, and neither source fact establishes behaviour under load. Both
stay at `Normal` by the clause above. §12 has no open questions left.

### (c)10 · §12 Q1 — settled, and §6.1's reason narrowed

Q1 stays as (b)5 settled it: A1c makes 18 conversions, which leaves `bt-pty`'s
four bare sites and the pool. The standalone main threads' own waits are
(c)6's. The 24-head change stays in one commit. Codex is right that §6.1
overstates the necessity. A compatibility adapter that delegates to one
canonical spawning body would still be one effect entrance, as
`spawn_at_priority` already delegates to `_with_stack`. So a staged migration
is possible, but it costs an extra public contract and delays enforcement, and
no benefit has been shown for a small mechanical change. §6.1's "exactly what
RULES 52 forbids" is withdrawn; the reason is now **no demonstrated benefit
over an atomic change**.

### (c)11 · Answers to §7, as they stand after this revision

1. The no-edge choice is accepted. Rows 11, 12 and 15 get `bt-app` doors
   ((c)3), and the transport's waits get registered transport doors ((c)6).
2. The two-half meter stays, with (b)4's unwind policy; the cookie and adapter
   are (c)2's.
3. The back edge, the three exits and ordinary work in `Exiting` stay.
   Cancellation and the build-error arm are (c)5's.
4. Drop debt is temporary, effect-scoped and repaid per row; the inventory and
   fence are (c)4's.
5. `expect_worker()` stays dropped. Codex accepts this, subject to (c)7's
   thread-bound `CallbackScope` and (c)8's auto-trait proofs, both adopted.
6. M3 is compile-only. The worker passing control is the real hand-off path
   ((c)8 item 6).
7. The stable limitation is accepted. The proof contract is (c)8's; it does
   not claim error reasons.

### (c)12 · This revision's own architecture impact

(a) Facts touched: none; this is a document.

(b) Doors: none in this commit. The design now names three new door kinds for
later tickets:
- `bt-app`'s `pty_door` (A1d);
- `bt-pty`'s transport doors (A2);
- `enter_standalone_main` (A1a).

(c) Debt: nothing repaid or added in this commit. The tickets will add:
- A1e: the `Drop` rows of (c)4 without a ledger row (`Shutdown`, the
  endpoints, the video engines and seats, `PtySession`);
- A2: the transport-invariant row of (c)6 item 1;
- a 0.4.7 row for the observation-band ticket of (c)9.

(c′) None.

(d) No ownership changes in this commit. Revision (c) adds one ownership
change for A1a, beyond §13.2's: "which role does a standalone process's main
thread have" gets an owner, `enter_standalone_main`.

---

## Revision 2026-09-26 (d), after the scoped Codex round

Review: `docs/plans/design/thread-door-review-codex-2026-09-26-b.md` (Codex,
scoped, at `80833cc7`), verdict **adopt with changes**. P5–P8 are met. P1–P4
are not met, and each has exactly one change named. This revision adopts the
four changes in substance. It is appended; where it differs from earlier
sections and revisions, **it rules**. It was checked against the code at
`78a3699a`, and each subsection ends adopted or refused. None is refused.
Codex's dispatch gate stands:
- A1a is dispatched after (d)1–(d)3;
- A1e is dispatched after (d)4.

### (d)1 · P1 — the test allocation, explicit: adopted

**The change.** Every test arm whose producer is a thread started by
`spawn_at_priority` belongs to **A1b**, because A1b is the ticket that makes
such a thread a `Worker`. So:
- M4g leaves A1a;
- the worker-refusal arm of owner-side M5 moves to A1b;
- A1d keeps the `Window` control of M5.

The standalone entry is not used anywhere as a stand-in for a spawned worker.
No test outside this table implies a producer. The table below **replaces**
§11's, (c)1's and (c)8's test lists.

| ticket | test | producer of the thread under test | kind |
|---|---|---|---|
| **A1a** | M4a, M4b, M4c, M4d, M4e, M4f, M4h (the token cases, each with its (c)8 control) | none (compile-time) | compile-fail pairs |
| A1a | auto-trait probes for `WaitToken` and `CallbackScope` ((c)8 items 2–3) | none; `std::thread::scope` for the transfer cases | compile-fail pairs |
| A1a | M4g′: `admitted` refused on a `Callback` thread | a plain `std::thread`, then `enter_callback` | executed |
| A1a | M4g″: `admitted` refused on an `Unset` thread | a plain `std::thread` | executed |
| A1a | M4i, every arm: (b)1's early-error sequence, (c)5's four quit-driver cases and the build-error road, and (c)8's rule that a second `enter_window_thread()` cannot reset `Running` | a plain `std::thread`, then `enter_window_thread` | executed |
| A1a | M4i′, the `Window`/phase arms: the phase writers refused and counted on an `Unset` thread; transitions not in their rows refused on the window thread | plain threads | executed |
| A1a | `CallbackScope` on `Unset` and on `Window`: nesting, and restoration on unwind ((c)7) | plain threads | executed |
| A1a | the meter and `Cookie` tests of (d)2 | a window-entered plain thread | executed |
| A1a | `enter_standalone_main`: the first call lends a `WorkerCtx` whose role is `Worker(name)`; a second call is `Refused` | the test binary's own thread, in **its own integration-test target** (`crates/bt-platform/tests/standalone_entry.rs`), because the entry is once per process | executed |
| **A1b** | M3, M3r, M3r′ | none | compile-fail pairs |
| A1b | auto-trait probes for `WorkerCtx` and `ShellThread` | none; scoped threads | compile-fail pairs |
| A1b | M4g: `admitted` refused on a spawned worker, `work` not run, the counter up by exactly one | `spawn_at_priority` | executed |
| A1b | M4i′, the worker arm: `loop_running()`, `exiting()` and `quit_abandoned()` refused and counted on a spawned worker | `spawn_at_priority` | executed |
| A1b | owner-side M5, the **worker-refusal arm**: a `Box<dyn Fn()>` and an `fn` pointer that call `admitted` run on a spawned worker; refused, not run | `spawn_at_priority` | executed |
| A1b | worker-side M5: `ShellThread::enter` reached through a `Box<dyn Fn(&WorkerCtx)>` and through an `fn(&WorkerCtx)` pointer inside a spawned body (executed), and each with no context in scope (compile-fail pair) | `spawn_at_priority` | executed and compile-fail |
| A1b | `CallbackScope` on a spawned worker: non-owning, role unchanged | `spawn_at_priority` | executed |
| A1b | the real hand-off producer control ((c)8 item 6) | `HandoffLane::spawn` | executed |
| A1b | the three band tests, changed to `\|_ctx\|` | `spawn_at_priority` | executed |
| **A1c** | one role witness per crate: a converted thread's body records `role()`, and the test reads `Worker("<its name>")` | the converted site's own start function | executed |
| A1c | each converted site keeps its spawn-failure behaviour; the existing tests of each site run unchanged | the site | executed |
| **A1d** | owner-side M5, the **`Window` control**: the same `Box` and `fn`-pointer calls on a window-entered thread are admitted and run | a plain thread, then `enter_window_thread` | executed |
| A1d | every row's witness in (d)3's table | per row | executed |
| A1d | every row's signature coercion ((c)8 item 9) | none | compiled |
| **A1e** | every guard assertion of §9.1, as amended by (c)4, (c)8 item 9 and (d)4; the mutations M7a–M7d, M10–M14 and (d)4's `Drop` mutations | none (source) | source guard |

**Adopted.**

### (d)2 · P2 — the cookie carries the whole `Location`: adopted

**Checked.**
- `hang_watch::enter` calls `Heartbeat::enter_at`, which charges the new
  station, enters the call-tree node and sets the scope. It returns
  `Location::Resume { station: previous, node: parent, scope }`.
- `station` is a `Station`, which is `#[repr(u8)]` with `STATION_COUNT` = 210.
- `node` and `scope` are `usize` indices into `hang_watch_detail`'s ledger,
  whose `CAPACITY` is 256. `ROOT` = `CAPACITY` = 256 is the sentinel the ledger
  hands out when it is full. So `node` and `scope` each lie in `0..=256`, which
  needs nine bits.
- `resume_at(station, node, scope, now)` restores all three whatever their
  values, `ROOT` included.

(c)2's adapter stack threw away the `Location` of any call it could not push,
which is why its 17th call misattributed its parent's remaining work.

**The change.** This replaces (c)2's adapter stack, `ADMITTED_DEPTH`,
`Cookie::OVERFLOW`, the generation counter and the mismatch counting.

```rust
// bt_platform::admission
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cookie(u64); // an opaque payload; admission never reads it
impl Cookie {
    pub const fn from_raw(raw: u64) -> Self;
    pub const fn raw(self) -> u64;
}
pub struct Meter {
    pub enter: fn(DoorKey) -> Cookie,
    pub leave: fn(DoorKey, Cookie, Instant, Instant),
}
```

**`Cookie` is one `u64`, and a `u64` is enough.** Its layout, owned by
`hang_watch`:

| bits | field | range | why this width |
|---|---|---|---|
| 0–7 | `station` (the `Station` byte) | 0–209 | `#[repr(u8)]` |
| 8–23 | `node` | 0–256 (`ROOT` = 256) | `u16`; nine bits are needed, and sixteen leave room for `CAPACITY` to grow to 65,535 |
| 24–39 | `scope` | 0–256 | the same |
| 40–63 | zero | — | reserved |

`hang_watch` declares `const _: () = assert!(hang_watch_detail::CAPACITY < u16::MAX as usize);`,
so a capacity that stops fitting is a compile error, not a truncation.
`[u32; 4]` is not needed.

- **`enter`** is `hang_watch`'s registered function. It calls
  `hang_watch::enter(station_of(key))` and packs the returned
  `Location::Resume` into the cookie. `enter` only ever returns the `Resume`
  shape, so no tag bit is needed.
- **`leave`** unpacks the cookie and calls
  `resume_at(station, node, scope, now)`, which is `hang_watch::at` of the
  saved `Location`. It does this on every normal return, **including when the
  detail ledger was full**: a `ROOT` node or scope is restored as `ROOT`, so the
  parent's station and position are restored exactly as `during` restores
  them.
- **No stack, no depth limit, no overflow sentinel.** Nesting is carried by
  `admitted`'s own call stack, since each `admitted` frame holds its own
  cookie. So recursive, same-door and `during_pane`-nested admissions all
  restore the `Location` their own `enter` returned. A station byte that does
  not decode (`Station::from_byte` is `None`) cannot come from `enter`. If one
  appears, `leave` restores nothing and counts, and this is the only counter
  left.

**Kept from (c)2.**
- An admitted call costs four clock reads: `enter`, `admitted`'s start and
  end, and `resume_at`'s `now`. This is stated for A3 to measure.
- The refusal total is written by `diagnostics::run_footer`, through
  `fn main`'s `trace_sink::stderr_line`.
- (b)4's unwind policy holds: no guard, and `leave` is not called on unwind,
  so the door's station stays until a later `at`, scope restoration or turn
  reinitialisation replaces it.

**Tests (A1a), deterministic, replacing (c)2's list:**
- a round trip of `Cookie` packing for every station byte and for `node` and
  `scope` at 0, 255 and 256 (`ROOT`);
- station, node and scope restored after an admitted call nested in `during`,
  in `during_pane`, and in another admitted call of the same door;
- restoration with the detail ledger filled to `CAPACITY` before `enter`: the
  parent's `ROOT` node and scope come back;
- a nesting depth of 64 restores every level;
- a caught panic leaves the door's station current;
- an inner call's `[start, end]` lies inside its outer call's.

**Adopted.**

### (d)3 · P3 — the door table at item level: adopted

**Checked.** The facts behind the rows below:
- visibility: `set_visible` is called in `put_the_window_on_the_glass`
  (`true`), `hide_quake_window` (`false`) and `let_go_of_this_window`
  (`false`);
- cursor: `apply_pointer_cursor` makes two `set_cursor` calls, one in its
  early branch and one at its tail;
- focus: `focus_window` is called in `open_from_notification`;
- present: `present_seats_and_commit` wraps its body in
  `during(PresentSeats)`, calls `renderer.present_frame_with_phases` between
  `enter(RenderCompose)` and `at(present_parent)`, and then, only after a
  `Presented`/`PresentedWithoutText` outcome, calls
  `compositor.set_covered_size` under `during(CompositorSize)` and
  `compositor.commit()` under `during(CompositorCommit)`;
- `present` and `present_frame` in `bt-render` are reached only from
  `bt-render`'s tests and `glyph_probe` (itself used only by test targets);
- `configure_window_surface` is private to `bt-render`, reached from
  `WindowRenderer::from_surface` (via `WindowRenderer::new`, whose one product
  caller is `Runtime::open_window`) and from `adopt_new_device` (row 10);
- the other `Compositor::commit` calls are in `WebSeat::stand_parked`,
  `WebSeat::adopt`, and the spare seat's `advance` and `retire`;
- the web engine: `WebHost::request_controller` is called once, in
  `WebSeat::step`, and `WebHost::request_environment` is called once, in
  `WebSeat::start_environment`;
- row 13's residue is `sample_window_place`'s `PlaceHidden` and
  `PlaceExposure` probes (the taskbar probe is ticket 62's lane).

**Conventions for every row.**
- **Minting.** "Minted at" is the statement where `admitted::<doors::X, _>(|t| …)`
  wraps the door call. The token is created by `admitted`, moved into the
  closure, and **consumed by value** by the door function. Every door
  signature below takes `token: WaitToken<'_, doors::X>` as its first
  parameter (after `self`, where there is one).
- **Winit calls.** Native calls on winit's `Window` are foreign methods, so
  each gets a small `bt-app` door in a new module `owner_door`, wrapping the
  one winit call.
- **Refusal.** A `Refused` is handled at the minting statement, before
  anything the row mutates.

| door type · row | door function and exact signature | minted at (the admission boundary) | all callers of the minting function | phases | metric | on `Refused` | witness |
|---|---|---|---|---|---|---|---|
| `PtyBirth` · 11 | `bt-app` `pty_door::spawn_shell(token: WaitToken<'_, doors::PtyBirth>, program: OsString, args: &[OsString], environment: &[(OsString, OsString)], size: PtySize, wake: OutputWake, working_directory: Option<PathBuf>) -> Result<PtySession, PtyError>`, whose body is `PtySession::spawn_shell_in(…)` | `create_leaf_session`, at its `spawn_shell_in` statement | every caller of `create_leaf_session` (unchanged; the brief lists them by grep) | Running, Exiting | single call | returns `Err` into `create_leaf_session`'s existing spawn-failure branch | a headless leaf birth records one `PtyBirth` admission and yields a live session |
| `PtyResize` · 12 | `pty_door::resize(token: WaitToken<'_, doors::PtyResize>, pty: &mut PtySession, size: PtySize) -> Result<(), PtyError>` | `commit_leaf_resize`, at `pty.resize(pty_size(next_grid, physical))`: after the reflow, before the reconcile, keeping the `?` | `release_due_leaf_resize` ← `Runtime::flush_pending_pty_resize` | Running, Exiting | single call **per leaf**; the flush is not admitted | mapped to the same `Err` the `?` propagates today | two leaves in one flush give two admissions; a refused resize takes the existing error road |
| `PaneRetirementWait` · 15 | `pty_door::wait_for_retirements(token: WaitToken<'_, doors::PaneRetirementWait>, budget: Duration) -> usize` | `settle_quit`'s `QuitStep::Retire` arm | `FolioApp::settle_quit` | Exiting | single call | logs that the count is unknown; quit proceeds as on a timeout | the quit driver's `Retire` step |
| `SessionWriteWait` · 16 | `SessionWriter::wait_for(&mut self, token: WaitToken<'_, doors::SessionWriteWait>, generation: u64) -> SessionWaitAnswer` | `SessionStore::flush_judged`, at its `self.writer.wait_for(generation)` statement | `QuitStep::Write`'s arm; `SessionStore::flush` ← `SessionStore::close` ← `App::finish` | Exiting | single call | returns the existing stalled answer (quit proceeds) | `flush_judged` through `QuitStep::Write` records one admission |
| `SessionWriterRetire` · 16b | `SessionWriter::close(&mut self, token: WaitToken<'_, doors::SessionWriterRetire>)` | `SessionStore::close`, at `self.writer.close()` | `App::finish` | Exiting | single call (a bounded poll and a join) | the writer is left to process exit, as its budget-expired branch does | `App::finish` records one admission |
| `TraceFlush` · 17 | `trace_sink::flush(token: WaitToken<'_, doors::TraceFlush>)` | `fn main` after `run_app`; `Shutdown::drop` (conditional, (d)4) | `fn main`; `Shutdown::drop` | Exiting | single call | queued lines are lost, as on its timeout | a window-entered test runs `exiting()` and then the admitted flush |
| `LaunchHandOver` · 18 | `launch_wire::hand_over(token: WaitToken<'_, doors::LaunchHandOver>, directory: &Path, argv: &cli::CliRequest, say: impl Fn(&str)) -> Option<i32>` | `fn main`, at its `hand_over` call | `fn main` | Starting | single call | `None`: carry on and open a window | the existing hand-over tests on a window-entered thread |
| `PresentFrame` · 9 | `WindowRenderer::present_frame_with_phases(&mut self, token: WaitToken<'_, doors::PresentFrame>, gpu: &mut GpuContext, seats: &[SeatFrame<'_>], trigger: FrameTrigger, phase: impl FnMut(PresentPhase)) -> Result<PresentOutcome, RenderError>`. The test-only wrappers `present_frame` and `present` take the same token and forward it | `present_seats_and_commit`, at `renderer.present_frame_with_phases(…)`. The admission **replaces** the `enter(RenderCompose)` / `at(present_parent)` pair, and the meter's `enter` pushes `RenderCompose` | `Runtime::present_seats_and_commit` ← its two callers in `runtime/frame.rs` and `runtime/preview.rs` | Running, Exiting | **declared batch, one per window per present**: composition, surface configure, acquire, submit and present for that window. The `phase` callback still moves `hang_watch` between the inner stations (`SurfaceConfigure`, `SurfaceAcquire`, `QueueSubmit`, `SwapchainPresent`, …), which stay stations, not admissions | an `anyhow` error returned through the existing `?`, the road a `RenderError` takes | a headless present records exactly one `PresentFrame` admission per window, whose interval contains every inner phase station's time |
| `CompositorCommit` · 9 | `bt_platform::Compositor::commit(&self, token: WaitToken<'_, doors::CompositorCommit>) -> Result<(), String>` | `present_seats_and_commit`'s `committed` closure at `compositor.commit()`; `WebSeat::stand_parked`; `WebSeat::adopt`; the spare seat's `advance` and `retire` | the present road above; the web seat's parking and adoption roads; the spare controller's step and retirement | Running, Exiting | single call | present: the existing `FailedCommit` road; the web seat and the spare: the `Err` they already handle or discard | present with a commit gives two admissions per window, `PresentFrame` then `CompositorCommit`; a web page parking records one |
| — `set_covered_size` | **not a door**: a property set on the uncommitted DirectComposition tree, which does not wait. It stays under `during(CompositorSize)` | — | — | — | — | — | — |
| `SurfaceBirth` · 9 | `WindowRenderer::new(token: WaitToken<'_, doors::SurfaceBirth>, gpu: &mut GpuContext, target: WindowTarget, width: u32, height: u32, scale_factor: f64) -> Result<WindowRenderer, RenderError>` (reaches `configure_window_surface` through `from_surface`) | `Runtime::open_window`, at `WindowRenderer::new(…)` | `Runtime::open_window` | Running | single call | the window's existing "open the new window's surface" error | opening a second window records one admission |
| (`adopt_new_device`'s configure) · 10 | **deferred to B9** | — | — | — | — | — | — |
| `TitleFlush` · 14 residue | `owner_door::set_title(token: WaitToken<'_, doors::TitleFlush>, window: &Window, title: &str)` | `Runtime::flush_title`, at `window.set_title(&title)` | `Runtime::turn`; `Runtime::dress_new_window` | Running, Exiting | single call | the wanted title stays wanted and is written next turn | a changed title is written once, with one admission |
| `ImeCaretArea` · 22 residue | `owner_door::set_ime_cursor_area(token: WaitToken<'_, doors::ImeCaretArea>, window: &Window, position: Position, size: Size)` | `Runtime::apply_ime_cursor_area`, at `self.window.window.set_ime_cursor_area(…)` | `Runtime::flush_ime_cursor_area` ← `Runtime::turn`, `Runtime::ime_input` | Running, Exiting | single call | the wanted area stays wanted and is told next turn | a moved caret is told once, with one admission |
| `FocusWindow` · §5.2 | `owner_door::focus_window(token: WaitToken<'_, doors::FocusWindow>, window: &Window)` | `Runtime::open_from_notification`, at `self.window.window.focus_window()` | `FolioApp::route_clicked_notifications` | Running | single call | the route opens without taking focus | a clicked notification records one admission |
| `SetVisible` · §5.2 | `owner_door::set_visible(token: WaitToken<'_, doors::SetVisible>, window: &Window, visible: bool)` | three statements: `Runtime::put_the_window_on_the_glass` (`true`); `Runtime::hide_quake_window` (`false`); `Runtime::let_go_of_this_window` (`false`) | `put_the_window_on_the_glass` ← `Runtime::show_new_window`, `Runtime::show_quake_window`; `hide_quake_window` ← `FolioApp::dismiss_quake`; `let_go_of_this_window` ← `Runtime::retire_window`, `Runtime::close_window` | `true`: Running. `false`: Running, Exiting | single call | `true`: the window stays hidden and the existing show-failure road runs. `false`: the retire or close continues, as when a hide has no effect | each of the three roads records one admission |
| `SetCursor` · §5.2 | `owner_door::set_cursor(token: WaitToken<'_, doors::SetCursor>, window: &Window, cursor: Cursor)` | two statements in `Runtime::apply_pointer_cursor`: the early branch's `set_cursor(cursor)` and the tail's `set_cursor(pointer_cursor(…))` | `apply_pointer_cursor`, called from its many pointer roads (`mouse`, `panes`, `preview`, `peek`, `floats`, `web`, `FolioApp`), all unchanged because the boundary is inside it | Running | single call | the cursor keeps its shape until the next pointer event | each branch records one admission |
| `WebController` · 21 | `bt_platform::WebHost::request_controller(&mut self, token: WaitToken<'_, doors::WebController>, window: NativeWindow, generation: u64) -> Result<(), String>` | `WebSeat::step`, at `self.host.request_controller(window, generation)` | `WebSeat::step`, from the pane's page road and from the spare's `SpareSeat::advance` | Running | single call | the step's existing `Err` road (the page stays coming up) | a warm or spare step records one admission |
| `WebEnvironment` · 21 | `bt_platform::WebHost::request_environment(&mut self, token: WaitToken<'_, doors::WebEnvironment>, folder: &Path, generation: u64) -> Result<(), String>` | `WebSeat::start_environment`, at `self.host.request_environment(&folder, generation)` | `WebSeat::open`, `WebSeat::step` (two arms); the spare's `open` and `step` (two arms) | Running | single call | the existing `Err` road | `warm_web_engine` records one admission |
| `FontFamilyLookup` · 5 residue | `bt_platform::monospace_family_named(token: WaitToken<'_, doors::FontFamilyLookup>, name: &str) -> Option<MonospaceFamily>` (all three platform arms) | `settings::monospace_family_files`, at its call | `apply_stored_terminal_font` (from `Runtime::create` and from `runtime/terminal.rs`'s font-change road), via `monospace_family_files` | Running (`Runtime::create` runs from `resumed`, which winit calls after `new_events(Init)`) | single call | `None`: the family is treated as not found, the existing answer | a stored family name at launch records one admission |
| `PlaceHidden` · 13 residue | `main::window_is_hidden(token: WaitToken<'_, doors::PlaceHidden>, window: &Window) -> bool` (the existing `bt-app` function, calling `bt_platform::is_window_minimized` and `is_window_cloaked`) | `sample_window_place`, at its `PlaceHidden` probe (the `enter`/`at` pair is replaced by the admission) | `Runtime::observe_window_place` | Running, Exiting | single call | the previous place is kept | one turn's head records one admission |
| `PlaceExposure` · 13 residue | `main::window_is_exposed(token: WaitToken<'_, doors::PlaceExposure>, window: &Window) -> bool` (the existing `bt-app` function, calling `bt_platform::window_is_exposed`) | `sample_window_place`, at its `during(PlaceExposure)` | `Runtime::observe_window_place` | Running, Exiting | single call | the previous place is kept | the same |

**The presentation measurement, stated.**
- `during(PresentSeats)` stays an outer **scope station** around the whole
  function. It is not an admission.
- Inside it are at most two **sibling, non-overlapping** admissions:
  `PresentFrame` (always, when the gate lets the frame through) and then
  `CompositorCommit` (only after a presented outcome), so
  `PresentFrame.end ≤ CompositorCommit.start`.
- `set_covered_size` lies between them under its own station and is
  measured by no admission.
- A window's present therefore yields one or two admission records, and the
  per-turn wait union of B§R-C counts both.

**Deferred rows** are unchanged from (c)3: 2, 3, 4, 7, 8, 10, 19 and 20.

**Adopted.**

### (d)4 · P4 — a closed edge-and-effect inventory per exception: adopted

**Checked** (each body read on `78a3699a`):
- Both `video::engine::Engine::shutdown` and `macos_player::Engine::shutdown`
  send `Shutdown`, then poll `stopped` with `std::thread::sleep(2 ms)` under
  `SHUTDOWN_BUDGET`, then join.
- `PtySession::drop` finishes the input dump (`PtyDump::finish`: one
  `writeln!`, then `publish`: two `sync_data`) **before** calling `shutdown`.
- `PtySession::shutdown`, in order:
  1. `input.close()`;
  2. the writer is taken (detached);
  3. the child's `try_wait`, then `kill`, then `reap_within` (which polls with
     `sleep`);
  4. `output.close()`;
  5. the master is dropped;
  6. `join_within` (a `join` and a polling `sleep`).
- `trace_sink::flush` calls `flush_sink`, which does, in order: a polling
  `sleep` on `queue.close()`, `finished.recv_timeout`, a polling `sleep` on
  `is_finished`, and a conditional `join`.
- `VideoSeat::shutdown` is `self.engine.shutdown()`, and
  `VideoSeats::shutdown_all` calls `VideoSeat::shutdown` per seat.

**The rule, replacing (c)4 items 1–3.** For each exception, the inventory
below is **closed**. From the `Drop` body, the guard walks the listed chain,
resolving calls to first-party items by `bt_source` item identity. For each
body it computes:
- (i) the ordered list of calls that resolve to first-party items;
- (ii) the count of each vocabulary effect, by call site in that body, not by
  execution.

The build is red when:
- a first-party call appears that is not in the row's edge list, whatever
  that function does — this closes Codex's counterexample of a
  non-vocabulary helper that reaches a door;
- a listed edge is missing or out of order;
- any effect count differs from the row's;
- a `Drop` that is not in the table reaches any registered door or listed
  chain member.

Calls to std and foreign items outside the vocabulary are not listed and not
counted. The table is the whole baseline.

| exception | ordered chain: first-party edges (→) and effects (·, count by call site) | product reach | repayment · version |
|---|---|---|---|
| `DirWatch` (Windows, `bt-platform` `lib.rs`) | `drop`: · `SetEvent` ×1 · `JoinHandle::join` ×1 · `close` ×3 | a watch retired on the window thread (row 8) | B7 (D-40) · 0.4.6 |
| `DirWatch` (macOS) | `drop` → `Stopper::signal` ×1 (its body: no vocabulary effect, pinned at count 0) · `JoinHandle::join` ×1 | the same | B7 (D-40) · 0.4.6 |
| `trace_sink::Shutdown` | `drop` → `flush` ×1 → `flush_sink` ×1: · `thread::sleep` ×1 (the queue-close poll) · `Receiver::recv_timeout` ×1 · `thread::sleep` ×1 (the writer poll) · `JoinHandle::join` ×1 | **conditional**: only `fn main`'s event-loop build-error return, and a reachable constructor failure is **not established** (Codex, both rounds). The normal exit flushes explicitly and leaves through `leave_process` | *The trace writer is retired through its admitted flush door, never by a drop* · 0.4.7 |
| `AttentionPipe` (Windows) | `drop`: · `SetEvent` ×1 · `JoinHandle::join` ×1 · `CloseHandle` ×1 | none in the product: the endpoint is static | *An endpoint is retired through an explicit door, not by its drop* · 0.4.7 |
| `AttentionPipe` (Unix) | `drop`: · `libc::write` ×1 · `JoinHandle::join` ×1 · `libc::close` ×1 | none (static) | the same · 0.4.7 |
| `LaunchPipe` (Windows) | `drop`: · `SetEvent` ×1 · `JoinHandle::join` ×1 · `CloseHandle` ×1 | none (static) | the same · 0.4.7 |
| `LaunchPipe` (Unix) | `drop`: · `libc::write` ×1 · `JoinHandle::join` ×1 · `libc::close` ×1 | none (static) | the same · 0.4.7 |
| `video::engine::Engine` | `drop` → `Engine::shutdown` ×1: · `Sender::send` (not vocabulary) · `thread::sleep` ×1 (the 2 ms poll) · `JoinHandle::join` ×1 | a seat closed on the window thread | *A video engine is shut down through an explicit door, not by its drop* · 0.4.7 |
| `macos_player::Engine` | `drop` → `Engine::shutdown` ×1: · `thread::sleep` ×1 · `JoinHandle::join` ×1 | the same | the same · 0.4.7 |
| `VideoSeat` (`bt-app`) | `drop` → `VideoSeat::shutdown` ×1 → `Engine::shutdown` ×1 (the platform row's chain). Later drops of the engine find `thread` taken and return early; the chain is the same code | a pane closed on the window thread | the same · 0.4.7 |
| `VideoSeats` (`bt-app`) | `drop` → `VideoSeats::shutdown_all` ×1 → `VideoSeat::shutdown` ×1 (one call site, in a loop) → `Engine::shutdown` ×1 | a window closed on the window thread | the same · 0.4.7 |
| `PtySession` (`bt-pty`) | `drop`, in this order: → `PtyDump::finish` ×1 (inside `if let Some(dump)`): · `writeln!` to the chunks file ×1 → `PtyDump::publish` ×1: · `File::sync_data` ×2 — **then** → `PtySession::shutdown` ×1: → `InputRing::close` ×1 · `Child::try_wait` ×1 · `Child::kill` ×1 → `reap_within` ×1: · `thread::sleep` ×1 → `OutputRing::close` ×1 → `join_within` ×1: · `JoinHandle::join` ×1 · `thread::sleep` ×1 | on `pty-retirement`; on the caller only when that thread cannot be started | *A shell is taken apart only through `retire_within`, never by a drop on the window thread* · 0.4.7 |

The effect vocabulary used above is the registry's (A1a): `thread::sleep`,
`JoinHandle::join`, `Receiver::recv*`, `File::sync_*`, file writes, and the
platform waits (`SetEvent` and `CloseHandle` are listed as effects so that
their counts are pinned, although they do not wait). `writeln!` to a `File`
counts as a file write.

**Controls and mutations (A1e).** The controls are today's complete chains
exactly as tabled, all green. Each mutation must turn the build red:
1. `VideoSeats::drop` calls a new helper that calls `SessionWriter::wait_for`
   — red: unlisted edge;
2. a second `thread::sleep` in `video::engine::Engine::shutdown` — red: the
   count changes;
3. a new non-vocabulary helper added to `Engine::shutdown`'s body that calls
   `trace_sink::flush` — red: unlisted edge (Codex's counterexample);
4. `PtySession::drop` reordered to call `shutdown` before finishing the dump —
   red: order;
5. a new `impl Drop for WebSeat` that calls `Compositor::commit` — red: a
   `Drop` outside the table reaches a registered door.

**Adopted.**

### (d)5 · This revision's own architecture impact

(a) None. (b) None in this commit. The design adds these doors to A1d's
scope:
- the `owner_door` module (`set_title`, `set_ime_cursor_area`,
  `focus_window`, `set_visible`, `set_cursor`);
- the two place probes, `window_is_hidden` and `window_is_exposed`, taking
  tokens;
- `SurfaceBirth`;
- `CompositorCommit`'s four web-seat callers.

(c) None repaid or added in this commit. (c′) None. (d) No.

---

## Revision 2026-09-26 (e), after the Codex confirmation round

Review: `docs/plans/design/thread-door-review-codex-2026-09-26-c.md` (Codex,
static, at `d85ed3fc`). It found:
- **P1 met**;
- **P2 met, with one wording correction**;
- **P3 and P4 not met**, with one change each.

This revision is appended, and it rules over everything before it where they
differ. It is the last revision before A1a is dispatched, so its tables are
built to be complete **by construction**: every inventory below comes from a
full-workspace search at `78a3699a`, and the note gives the patterns so A1a's
implementer can re-run them. Nothing in (e) says "by grep", "as today",
"per door type" or "existing call sites".

**How the inventories were taken.**
- **The universe:** every `.rs` file under `crates/`, `vendor/` excluded.
- **Reading:** each file is read as bytes with NUL removed. This matters:
  `crates/bt-platform/src/lib.rs` contains a NUL byte, so ripgrep skips it as
  binary. Use `grep -a`, or a reader that strips NUL.
- **Test code:** a line is test code when it is:
  - in `tests/`, `examples/`, `benches/` or `src/bin/`, or in a file named
    `*tests.rs`; or
  - inside an item marked `#[cfg(test)]` or `#[cfg(all(test, …))]`, from the
    attribute to the item's closing brace at the item's own indentation.
- **Enclosing function:** the nearest preceding `fn <name>`.
- **The patterns**, one per door, are listed in (e)2's first table.

### (e)1 · P2 — the checked decoder: adopted

**Checked.** `Station::from_byte` returns a `Station`, not an `Option`. An
unknown byte falls back to `Station::Starting`. (d)2's "`Station::from_byte` is
`None`" is withdrawn.

**The decoder** is new, owned by `hang_watch`, and used only by the meter's
`leave`:

```rust
fn decode(cookie: Cookie) -> Option<Location> {
    let raw = cookie.raw();
    let station = (raw & 0xFF) as u8;
    let node = ((raw >> 8) & 0xFFFF) as usize;
    let scope = ((raw >> 24) & 0xFFFF) as usize;
    let rest = raw >> 40;
    (usize::from(station) < STATION_COUNT
        && node <= hang_watch_detail::ROOT
        && scope <= hang_watch_detail::ROOT
        && rest == 0)
        .then(|| Location::Resume { station: Station::from_byte(station), node, scope })
}
```

- `leave` calls `resume_at` only on `Some`. On `None` it restores nothing and
  adds one to the invalid-cookie counter.
- `Station::from_byte`'s fallback is never reached through `decode`, because
  the range check comes first.
- One more A1a test covers the decoder: `decode` of every value `enter` can
  produce is `Some` and round-trips. `decode` is `None` for `station =
  STATION_COUNT`, for `node = ROOT + 1`, for `scope = ROOT + 1`, and for any
  set bit at 40 or above. Each rejection is counted exactly once.

**Adopted.**

### (e)2 · P3 — every `Compositor::commit` caller, and no placeholder left: adopted

**Checked.**
- Besides the five `bt-app` statements (d)3 listed, Windows product code
  calls `Compositor::commit` six more times:
  - `Compositor::new` (`bt-platform/src/lib.rs`), after the tree is built;
  - `Compositor::set_window_size` (the same file), after `place_skirt`;
  - `WebHost::rehost` (`webview.rs`), in its `RehostStep::CommitSource` and
    `CommitTarget` arms;
  - `WebHost::compensate`, twice, `restore.target.commit()?` and
    `restore.source.commit()?`. `compensate` has one caller: `rehost`'s
    failure branch.
- There are three direct test and example callers:
  - `webview.rs`'s `open_the_page` test helper;
  - `portable_impl.rs`'s
    `a_deferred_service_that_is_not_on_this_platform_refuses_when_invoked_not_at_startup`;
  - `crates/bt-app/examples/video-probe.rs`'s `Probe::draw`. The example
    compiles under `clippy --all-targets`, so it must adapt.
- The macOS and portable `Compositor::commit` definitions
  (`macos_compose.rs`, `portable_impl.rs`) have no in-crate product callers.
  No macOS or portable `WebHost` calls `commit`.

**While completing the inventory, an unlisted window-thread wait was found.**
`Runtime::create` (`main.rs`) opens the first window's GPU with
`pollster::block_on(GpuContext::open(…))`. `pollster::block_on` is in B§R-A's
vocabulary, and the call is not a §5.3 row. That is a finding, reported here,
not decided: under A§5.3 "a call not on this list that blocks on something
outside the process is a defect". A1a registers it as **row 23 · `GpuOpen`**
with status `pending`. It is never shown as `bound`, and its ruling cell cites
the DESIGN entry A1a adds to record the finding. A1d wraps it (row below).
Moving it is not decided here.

**How `Compositor`'s commits are admitted.** `Compositor::commit` becomes the
token-taking door. The six internal commits call a new **private**
`Compositor::commit_now(&self) -> Result<(), String>`, the body today's
`commit` has. Each internal statement is covered by the admission of the
public door that encloses it, as in the table below. The macOS and portable
`Compositor` and `WebHost` take the same token parameters, so the platforms
keep one signature, which the signature-parity test in `bt-platform/src/lib.rs`
already compares.

**Test protocol for every row.** A test that reaches a minting statement runs
on a thread that has called `admission::enter_window_thread()` and then
`admission::loop_running()`. Rows admitted only in `Exiting` also call
`admission::exiting()`. `bt-app` gets one test helper, `tests::on_the_window_thread()`,
that makes those calls. Test code is outside A1e's pinned-call-site universe
by declaration, so this adds no product writer. The direct test callers each
row must adapt are listed in its row. The indirect ones are listed after the
table.

#### The patterns (the inventory's search)

| row | pattern(s), run over the universe above |
|---|---|
| `PtyBirth` | `spawn_shell_in\(` · `create_leaf_session\(` |
| `PtyResize` | `pty\.resize\(` · `commit_leaf_resize\(` · `release_due_leaf_resize\(` · `flush_pending_pty_resize\(` |
| `PaneRetirementWait` | `wait_for_retirements\(` |
| `SessionWriteWait` | `writer\.wait_for\(` · `flush_judged\(` |
| `SessionWriterRetire` | `writer\.close\(` · `SessionStore::close` via `session_store\.close\(`/`\.finish\(\)` on `App` |
| `TraceFlush` | `trace_sink::flush\(` · `\bflush\(\)` in `trace_sink.rs` |
| `LaunchHandOver` | `launch_wire::hand_over\(` |
| `GpuOpen` | `GpuContext::open\(` |
| `PresentFrame` | `present_frame_with_phases\(` · `\.present_frame\(` · `\.present\(` on `WindowRenderer` · `present_seats_and_commit\(` |
| `CompositorCommit` | `\.commit\(\)` |
| `CompositorBirth` | `Compositor::new\(` · `spare_parent\(` |
| `CompositorWindowSize` | `\.set_window_size\(` |
| `WebRehost` | `\.rehost\(` · `compensate\(` |
| `SurfaceBirth` | `WindowRenderer::new\(` |
| `TitleFlush` | `\.set_title\(` · `flush_title\(` |
| `ImeCaretArea` | `\.set_ime_cursor_area\(` · `apply_ime_cursor_area\(` · `flush_ime_cursor_area\(` |
| `FocusWindow` | `\.focus_window\(` · `open_from_notification\(` |
| `SetVisible` | `window\.set_visible\(` · `put_the_window_on_the_glass\(` · `hide_quake_window\(` · `let_go_of_this_window\(` |
| `SetCursor` | `window\.set_cursor\(` · `apply_pointer_cursor\(` |
| `WebController` | `request_controller\(` · `self\.apply\(` in `webhost.rs` |
| `WebEnvironment` | `request_environment\(` · `start_environment\(` |
| `FontFamilyLookup` | `monospace_family_named\(` · `monospace_family_files\(` · `apply_stored_terminal_font\(` |
| `PlaceHidden`, `PlaceExposure` | `window_is_hidden\(` · `window_is_exposed\(` · `sample_window_place\(` · `observe_window_place\(` |

#### The door table (replaces (d)3's; every cell is an inventory)

The notation is `file` · `Type::function`, or `file` · `function` for a free
function. "Minted at" is the statement inside the minting function where
`admitted::<doors::X, _>(|t| …)` wraps the door call. The token is consumed by
value there. All signatures take `token: WaitToken<'_, doors::X>` first (after
`self`).

| door · row | door function | minted at | product callers of the minting function (complete) | phases | metric | on `Refused` | witness | direct test callers to adapt |
|---|---|---|---|---|---|---|---|---|
| `PtyBirth` · 11 | `bt-app` `pty_door::spawn_shell(token, program: OsString, args: &[OsString], environment: &[(OsString, OsString)], size: PtySize, wake: OutputWake, working_directory: Option<PathBuf>) -> Result<PtySession, PtyError>`; its body is the one `PtySession::spawn_shell_in` call | `main.rs` · `create_leaf_session`, at `PtySession::spawn_shell_in(` | `runtime/panes.rs` · `Runtime::split_seat`; `runtime/preview.rs` · `Runtime::pop_out_preview`; `runtime/terminal.rs` · `Runtime::restart_shell`; `main.rs` · `create_tab_state` (← `main.rs` · `Runtime::create`; `runtime/tabs.rs` · `Runtime::new_tab_seeded_from`, `Runtime::commit_row_into_new_tab`; `runtime/windows.rs` · `Runtime::open_window`, `Runtime::reopen_recent`, `Runtime::answer_restore`) | Running, Exiting | single call | `Err` into `create_leaf_session`'s existing spawn-failure branch | a headless `split_seat` records one admission and a live session | none (the `bt-pty` tests call `spawn_shell_in` directly and are unaffected) |
| `PtyResize` · 12 | `pty_door::resize(token, pty: &mut PtySession, size: PtySize) -> Result<(), PtyError>` | `main.rs` · `commit_leaf_resize`, at `pty.resize(pty_size(next_grid, physical))`: after the reflow, before the reconcile, keeping the `?` | `commit_leaf_resize` ← `main.rs` · `release_due_leaf_resize` ← `runtime/dpi.rs` · `Runtime::flush_pending_pty_resize` ← `runtime/frame.rs` · `Runtime::turn` | Running, Exiting | single call per leaf | the same `Err` the `?` propagates | two leaves in one flush record two admissions | none directly. Indirectly: `tests.rs` calls `commit_leaf_resize` six times and `release_due_leaf_resize` eleven times, and `text_size_tests.rs` calls `release_due_leaf_resize` once. Each enters the window thread (`on_the_window_thread`); where it passes no `PtySession`, no admission is reached |
| `PaneRetirementWait` · 15 | `pty_door::wait_for_retirements(token, budget: Duration) -> usize` | `main.rs` · `FolioApp::settle_quit`, `QuitStep::Retire` arm | `settle_quit` ← `FolioApp::about_to_wait_inner` (two statements), `FolioApp::window_event` | Exiting | single call | logs the count as unknown; quit proceeds as on a timeout | the `Retire` step through the real `settle_quit` | none (the `bt-pty` tests are unaffected) |
| `SessionWriteWait` · 16 | `persist.rs` · `SessionWriter::wait_for(&mut self, token, generation: u64) -> SessionWaitAnswer` | `persist.rs` · `SessionStore::wait_for_landing`, at `self.writer.wait_for(generation)` | `wait_for_landing` ← `SessionStore::flush_judged` ← `main.rs` · `FolioApp::settle_quit` (`QuitStep::Write`) and `persist.rs` · `SessionStore::flush` ← `SessionStore::close` ← `main.rs` · `App::finish` | Exiting | single call | the existing stalled answer; quit proceeds | `flush_judged` through `QuitStep::Write` records one admission | `persist.rs` · `only_one_thread_ever_takes_the_session_writers_channel` (a direct `wait_for`). Indirectly through `flush_judged`: `a_session_write_that_could_not_happen_is_reported_as_one`, `a_quit_leaves_a_writer_that_never_answers_behind`, `a_store_with_no_writer_thread_reports_rather_than_writing`, `a_document_that_will_not_write_stops_retrying_and_says_so`. Each enters the window thread and `exiting()` |
| `SessionWriterRetire` · 16b | `persist.rs` · `SessionWriter::close(&mut self, token)` | `persist.rs` · `SessionStore::close`, at `self.writer.close()` | `SessionStore::close` ← `main.rs` · `App::finish` ← `FolioApp::close`, `FolioApp::reap_leaving_windows`, `FolioApp::settle_quit`, `FolioApp::fail`, `FolioApp::exiting` | Exiting | single call | the writer is left to process exit | `App::finish` through `FolioApp::close` | `persist.rs`: `a_quit_leaves_a_writer_that_never_answers_behind`, `only_one_thread_ever_takes_the_session_writers_channel`, `a_store_with_no_writer_thread_reports_rather_than_writing`, `a_document_that_will_not_write_stops_retrying_and_says_so` (direct `writer.close()`) |
| `TraceFlush` · 17 | `trace_sink.rs` · `flush(token)` | `main.rs` · `main` after `run_app`, and in `main`'s event-loop build error arm ((c)5); `trace_sink.rs` · `Shutdown::drop` (conditional, (e)3) | `main`; `Shutdown::drop` | Exiting | single call | queued lines are lost, as on its timeout | a window-entered test runs `exiting()` and then the admitted flush | none (`shutdown_does_not_wait_forever_for_a_stalled_writer` calls the private `flush_sink`, not the door) |
| `LaunchHandOver` · 18 | `launch_wire.rs` · `hand_over(token, directory: &Path, argv: &cli::CliRequest, say: impl Fn(&str)) -> Option<i32>` | `main.rs` · `main`, at `launch_wire::hand_over(` | `main` | Starting | single call | `None`: open a window | the hand-over road's existing tests, window-entered | none. Three source needles keep matching: the literal `launch_wire::hand_over(` stays inside the closure |
| **`GpuOpen` · 23 (new, pending)** | `bt-app` `gpu_door::open(token, target: WindowTarget, width: u32, height: u32, scale: f64) -> Result<(GpuContext, WindowRenderer), RenderError>`; its body is `pollster::block_on(GpuContext::open(…))` | `main.rs` · `Runtime::create`, at `pollster::block_on(GpuContext::open(` | `Runtime::create` ← `main.rs` · `FolioApp::resumed` | Running | single call | the existing "initialize wgpu renderer" error | a headless `Runtime::create` records one admission | none (`examples/video-probe.rs` and `tests/macos_glyph_surface.rs` call `GpuContext::open` directly, not the `bt-app` door) |
| `PresentFrame` · 9 | `bt-render` · `WindowRenderer::present_frame_with_phases(&mut self, token, gpu, seats, trigger, phase) -> Result<PresentOutcome, RenderError>`. `present_frame` and `present` take the same token and forward it | `runtime/panes.rs` · `Runtime::present_seats_and_commit`, at `renderer.present_frame_with_phases(`; it replaces the `enter(RenderCompose)`/`at(present_parent)` pair | `present_seats_and_commit` ← `runtime/frame.rs` · `Runtime::redraw`, `runtime/preview.rs` · `Runtime::present_retained_picture` | Running, Exiting | declared batch: one per window per present (compose, configure, acquire, submit, present; the inner phases stay stations) | an `anyhow` error through the existing `?` | one admission per window per present, containing every inner phase station | the twenty `present_frame` statements in `bt-render/src/lib.rs`'s test module, named here by their nearest enclosing `fn` (`HeadlessDevice::ideograph` ×2, `a_4k_chinese_frame_fits_the_glyph_atlas`, `one_ideograph_at_one_size_is_one_bitmap_in_every_lane`, `a_label_behind_a_closed_clip_casts_nothing`, `a_pane_flying_off_the_edge_never_scissors_outside_the_render_target`, `a_long_chinese_session_never_runs_the_atlas_out_of_room`, `a_session_long_enough_to_wear_the_packer_out_gets_its_text_back`, `mixed_size_seats_share_the_atlas_and_get_their_text_back`, `a_cards_text_reaches_the_glass_on_the_frame_its_layout_lands`, `every_picture_handed_over_is_drawn_in_its_own_pane`, `HanStream::present` ×2, `a_chrome_label_lands_in_its_box_on_the_device_a_driverless_machine_gets`, `a_video_layer_fills_its_box_letterboxes_in_its_ground_and_rounds_its_corners`, `a_video_that_stopped_releases_its_texture`, `a_layer_without_a_frame_draws_nothing`, `a_second_playback_is_a_second_texture_however_its_frames_are_numbered`, `one_windows_textures_are_not_another_windows`, `HanStream::one_frame`); `bt-render/src/glyph_probe.rs` · `GlyphFixture::present` (product module, used only by `bt-render/tests/glyph_output.rs` and `bt-app/tests/macos_glyph_surface.rs`); `bt-app/examples/video-probe.rs` · `Probe::draw` ×2 |
| `CompositorCommit` · 9 | `bt-platform` · `Compositor::commit(&self, token) -> Result<(), String>` (Windows, macOS, portable) | five `bt-app` statements: `runtime/panes.rs` · `Runtime::present_seats_and_commit` (`committed` closure); `webhost.rs` · `WebSeat::stand_parked`; `webhost.rs` · `WebSeat::adopt`; `web_spare.rs` · `SpareSeat::advance` for `WebSeat`; `web_spare.rs` · `SpareSeat::retire` for `WebSeat` | `present_seats_and_commit` as above; `stand_parked` ← `runtime/web.rs` · `Runtime::make_spare_web_controller`; `WebSeat::adopt` ← `webhost.rs` · `WebSeat::rehost` (three statements); `advance` ← `web_spare.rs` · `WebSpare::advance` ← `runtime/web.rs` · `Runtime::warm_web_engine`, `main.rs` · `FolioApp::advance_the_spare_while_retiring`, `FolioApp::user_event`; `retire` ← `WebSpare::advance`, `WebSpare::retire` (← `FolioApp::close`, `FolioApp::settle_quit`), `WebSpare::adopt` | Running, Exiting | single call | present: the `FailedCommit` road; `stand_parked`/`advance`: their existing `Err` branches; `adopt`/`retire`: ignored as today (`let _`) | present with a commit gives two sibling admissions; each web road records one | `webview.rs` · `open_the_page` (test helper); `portable_impl.rs` · `a_deferred_service_that_is_not_on_this_platform_refuses_when_invoked_not_at_startup`; `examples/video-probe.rs` · `Probe::draw` |
| ↳ internal: `Compositor::new`'s commit, `set_window_size`'s commit | private `Compositor::commit_now(&self)` | covered by `CompositorBirth` and `CompositorWindowSize` (below) | — | — | part of the enclosing batch | — | — | — |
| ↳ internal: `WebHost::rehost`'s `CommitSource`/`CommitTarget`; `WebHost::compensate`'s two | private `Compositor::commit_now` through `RehostSide.compositor` | covered by `WebRehost` (below) | — | — | part of the enclosing batch | — | — | — |
| **`CompositorBirth` · 9** (new) | `bt-platform` · `Compositor::new(token, window: NativeWindow) -> Result<Compositor, String>` (all three arms), and `spare_parent(token) -> Result<Option<SpareParent>, String>`, which forwards the token to its one `Compositor::new` | `main.rs` · `Runtime::create`, at `bt_platform::Compositor::new(native)`; `runtime/windows.rs` · `Runtime::open_window`, at the same; `runtime/web.rs` · `Runtime::make_spare_web_controller`, at `bt_platform::spare_parent()` | `Runtime::create` ← `FolioApp::resumed`; `Runtime::open_window` ← `main.rs` · `FolioApp::open_pending_window`; `make_spare_web_controller` ← `runtime/web.rs` · `Runtime::warm_web_engine` ← `runtime/frame.rs` · `Runtime::turn` (its `WebWarmup` station) | Running | declared batch: building the DirectComposition device, the visuals and one `commit_now` (and, for `spare_parent`, the never-shown window) | window roads: the existing "open the window's DirectComposition visual tree" error; the spare: the existing `Err` branch (`retired_line`, no spare) | opening a second window and making a spare each record one admission | `portable_impl.rs`: `every_startup_constructor_builds_rather_than_refusing`, `a_deferred_service_that_is_not_on_this_platform_refuses_when_invoked_not_at_startup`; `webview.rs` · `open_the_page`; `tests/macos_compose.rs`: `the_page_slot_shows_through_folios_frame_and_only_where_it_stands`, `the_unflipped_branch_of_the_placement_is_a_flip`; `tests/macos_webview.rs` · `Origin::run`; `examples/video-probe.rs` · `Probe::resumed` |
| **`CompositorWindowSize` · 9** (new) | `bt-platform` · `Compositor::set_window_size(&self, token, width: u32, height: u32) -> Result<(), String>` (all three arms) | `runtime/dpi.rs` · `Runtime::resize`, at `.set_window_size(physical.width, physical.height)` | `Runtime::resize` ← `runtime/dpi.rs` · `Runtime::scale_factor_changed`, `Runtime::resized` | Running, Exiting | declared batch: `place_skirt` (property sets) and one `commit_now`; a size that did not change commits nothing and still records its admission | the existing `?` with "put the window's own ground under the strip a resize opens" | a size change records one admission | `portable_impl.rs` · `a_deferred_service_that_is_not_on_this_platform_refuses_when_invoked_not_at_startup` |
| **`WebRehost` · 21** (new) | `bt-platform` · `WebHost::rehost(&mut self, token, from: &RehostSide<'_>, to: &RehostSide<'_>, rect: (i32, i32, u32, u32), visible: bool) -> RehostOutcome` (all three arms); `compensate` stays private and is reached only inside it | `webhost.rs` · `WebSeat::rehost`, at `self.host.rehost(` | `WebSeat::rehost` ← `runtime/panes.rs` · `Runtime::dock_the_page_of`, `Runtime::carry_the_pages_of_moved_panes`; `runtime/web.rs` · `WindowHandoff::rehost` (← `web_spare.rs` · `WebSpare::adopt`); `main.rs` · `FolioApp::transfer_tab` (two statements) | Running | declared batch: the rehost steps with their two forward commits and, on failure, `compensate`'s two restoring commits | `RehostOutcome::KeptSource` with `failed_at: RehostStep::Hide` and no compensation owed. This is the shape `rehost` already returns before its first step, so `WebSeat::rehost`'s existing kept-source branch keeps the page where it was | docking a page records one admission | none direct. Source needle `main.rs` · `the_transfer_is_a_transaction_and_its_commit_pays_every_debt` (`".rehost("`) keeps matching |
| `SurfaceBirth` · 9 | `bt-render` · `WindowRenderer::new(token, gpu: &mut GpuContext, target: WindowTarget, width: u32, height: u32, scale_factor: f64) -> Result<WindowRenderer, RenderError>` | `runtime/windows.rs` · `Runtime::open_window`, at `WindowRenderer::new(` | `Runtime::open_window` ← `FolioApp::open_pending_window` | Running | declared batch: surface creation and `configure_window_surface` | the existing "open the new window's surface on this application's device" error | a second window records one admission | none |
| `TitleFlush` · 14 residue | `bt-app` `owner_door::set_title(token, window: &Window, title: &str)` | `runtime/windows.rs` · `Runtime::flush_title`, at `window.set_title(&title)` | `flush_title` ← `runtime/frame.rs` · `Runtime::turn`, `runtime/windows.rs` · `Runtime::dress_new_window` | Running, Exiting | single call | the wanted title stays wanted | a changed title is written once, with one admission | none |
| `ImeCaretArea` · 22 residue | `owner_door::set_ime_cursor_area(token, window: &Window, position: Position, size: Size)` | `runtime/keyboard.rs` · `Runtime::apply_ime_cursor_area`, at `self.window.window.set_ime_cursor_area(` | `apply_ime_cursor_area` ← `Runtime::flush_ime_cursor_area` ← `runtime/frame.rs` · `Runtime::turn`, `runtime/keyboard.rs` · `Runtime::ime_input` | Running, Exiting | single call | the wanted area stays wanted | a moved caret is told once | none direct; source needle `ime_outbound.rs` (`".set_ime_cursor_area("`) is updated to `owner_door::set_ime_cursor_area(` in A1d's commit |
| `FocusWindow` · §5.2 | `owner_door::focus_window(token, window: &Window)` | `runtime/attention.rs` · `Runtime::open_from_notification`, at `self.window.window.focus_window()` | `open_from_notification` ← `main.rs` · `FolioApp::route_clicked_notifications` ← `FolioApp::user_event` | Running | single call | the route opens without taking focus | a clicked notification records one admission | none |
| `SetVisible` · §5.2 | `owner_door::set_visible(token, window: &Window, visible: bool)` | `runtime/windows.rs` · `Runtime::put_the_window_on_the_glass` (`true`); `runtime/quake.rs` · `Runtime::hide_quake_window` (`false`); `runtime/windows.rs` · `Runtime::let_go_of_this_window` (`false`) | `put_the_window_on_the_glass` ← `runtime/windows.rs` · `Runtime::show_new_window` (← `Runtime::open_window`, `main.rs` · `Runtime::create`, `FolioApp::settle_tear_out`), `runtime/quake.rs` · `Runtime::show_quake_window` (← `FolioApp::summon_quake`); `hide_quake_window` ← `FolioApp::dismiss_quake` (← `FolioApp::settle_quake` ×2); `let_go_of_this_window` ← `Runtime::retire_window` (← `FolioApp::settle_quit`), `Runtime::close_window` (← `FolioApp::transfer_tab`, `FolioApp::close`, `FolioApp::retire_the_summon_with_the_run`, `FolioApp::settle_tear_out`, `FolioApp::fail`, `FolioApp::exiting`) | `true`: Running. `false`: Running, Exiting | single call | `true`: stays hidden, and the show road's existing failure branch runs. `false`: the retire or close continues | each of the three statements records one admission | none direct; source needles `main.rs` (`"self.window.window.set_visible(false)"`, `"set_visible(false)"`) are updated to the door's spelling in A1d's commit |
| `SetCursor` · §5.2 | `owner_door::set_cursor(token, window: &Window, cursor: Cursor)` | `runtime/mouse.rs` · `Runtime::apply_pointer_cursor`, both `set_cursor` statements | `apply_pointer_cursor` ← `runtime/floats.rs`: `place_float`, `dismiss_float`; `runtime/mouse.rs`: `drive_float_hover`, `press_float`, `promote_float_head_press`, `pointer_moved`, `drive_drag`, `finish_drag`, `chrome_mouse_input` ×3, `mouse_input`; `runtime/panes.rs`: `drive_command_rail_hover`, `dock_float`, `cancel_divider_drag`, `update_chrome_hover_target_in_pane`; `runtime/peek.rs`: `press_file_peek_foot`, `promote_file_peek`; `runtime/preview.rs`: `note_preview_link_hover`, `press_preview_image`, `set_preview_image_zoom`, `pop_out_preview`, `dock_preview_float`; `runtime/web.rs`: `apply_web_outcomes`, `drive_web_pointer`; `main.rs`: `Runtime::toggle_settings_panel`, `FolioApp::window_event` | Running | single call | the cursor keeps its shape | each branch records one admission | none |
| `WebController` · 21 | `bt-platform` · `WebHost::request_controller(&mut self, token, window: NativeWindow, generation: u64) -> Result<(), String>` (all three arms) | `webhost.rs` · `WebSeat::step`, at `self.host.request_controller(window, generation)` | `step` ← `WebSeat::apply` ← `webhost.rs`: `go_adopted`, `go`, `restart_engine`, `reload`, `drive`, `retire_parked`, `tick`, `place`, `rehost`, `go_to` | Running | single call | the step's existing `Err` road | a page coming up records one admission | `webview.rs` · `open_the_page`; `tests/macos_webview.rs` · `Origin::run` ×2; source needle `main.rs` (`"self.host.request_controller("`) keeps matching |
| `WebEnvironment` · 21 | `bt-platform` · `WebHost::request_environment(&mut self, token, folder: &Path, generation: u64) -> Result<(), String>` (all three arms) | `webhost.rs` · `WebSeat::start_environment`, at `self.host.request_environment(&folder, generation)` | `start_environment` ← `webhost.rs` · `WebSeat::open`, `WebSeat::step` ×2 | Running | single call | the existing `Err` road | `warm_web_engine` records one admission | `webview.rs` · `open_the_page`; `tests/macos_webview.rs` · `Origin::run` ×2; source needle `main.rs` (`"self.host.request_environment("`) keeps matching |
| `FontFamilyLookup` · 5 residue | `bt-platform` · `monospace_family_named(token, name: &str) -> Option<MonospaceFamily>` (all three arms) | `settings.rs` · `monospace_family_files`, at its call | `monospace_family_files` ← `main.rs` · `apply_stored_terminal_font` ← `main.rs` · `Runtime::create`, `runtime/terminal.rs` · `Runtime::adopt_terminal_font` | Running | single call | `None`: not found | a stored family at launch records one admission | `settings.rs`: `launching_with_a_stored_font_family_walks_no_font_collection_on_the_window_thread`, `the_launch_face_is_looked_up_by_name_and_the_picker_list_comes_from_the_lane`; `bt-platform/src/lib.rs`: `the_lookup_by_name_answers_the_files_the_walk_does` ×3, `the_lanes_walk_asks_the_collection_for_updates_and_the_launch_lookup_does_not` |
| `PlaceHidden` · 13 residue | `main.rs` · `window_is_hidden(token, window: &Window) -> bool` | `main.rs` · `sample_window_place`, at `window_is_hidden(window)` (replacing its `enter`/`at` pair) | `sample_window_place` ← `runtime/frame.rs` · `Runtime::observe_window_place` ← `Runtime::turn`, `runtime/windows.rs` · `Runtime::dress_new_window`, `runtime/attention.rs` · `Runtime::replace_contradicted_flash`, `main.rs` · `FolioApp::user_event` | Running, Exiting | single call | the previous place is kept | a turn's head records one admission | source needles `present_diagnostics_tests.rs` (`"fn window_is_hidden(window: &Window) -> bool {"`, `"let hidden = window_is_hidden(window);"`) are updated to the new spelling in A1d's commit |
| `PlaceExposure` · 13 residue | `main.rs` · `window_is_exposed(token, window: &Window) -> bool` | `main.rs` · `sample_window_place`, at `window_is_exposed(window)` (inside its `during(PlaceExposure)`, which the admission replaces) | as `PlaceHidden` | Running, Exiting | single call | the previous place is kept | as `PlaceHidden` | none |
| — `set_covered_size` | not a door: a property set on the uncommitted tree, which does not wait; it stays under `during(CompositorSize)` | — | — | — | — | — | — | — |

**Source-reading tests whose needles change, updated in A1d's commit.** These
are all found by searching test code for string literals naming a door call
(pattern: a `"…"` literal containing any of the door names above):
- `main.rs`:
  - `".set_window_size(physical.width,physical.height)"` (the resize-order red
    gate) and `".commit()"` (the present-funnel gate) gain the token argument;
  - `"self.window.window.set_visible(false)"` and `"set_visible(false)"`;
- `tests.rs` · the present funnel's `".commit()"`;
- `ime_outbound.rs` · `".set_ime_cursor_area("`;
- `present_diagnostics_tests.rs`' two `window_is_hidden` needles;
- `bt-platform/src/lib.rs` · `"self.place_skirt()?;self.commit()"`, which
  becomes `commit_now`;
- `bt-render/src/lib.rs`' `present_frame` signature needle
  (`"pubfnpresent_frame(&mutself,gpu:&mutGpuContext,"`), which gains the
  token.

These needles keep matching and need no change: `".rehost("`,
`"self.host.request_controller("`, `"self.host.request_environment("`,
`"app.session_store.flush_judged()"`, `"launch_wire::hand_over("` (three
tests), `"Compositor::new"` (the startup-order list), and `hang_watch`'s
station names.

**§8 and §11, reconciled.** A1d converts exactly the rows above. Its scope now
includes the rows (d)5 named plus `CompositorBirth`, `CompositorWindowSize`,
`WebRehost` and the new row 23 `GpuOpen`. The deferred rows are unchanged:
2, 3, 4, 7, 8, 10, 19, 20.

**Adopted.**

### (e)3 · P4 — complete per-body edge lists: adopted

**Checked.** Every body was walked at `78a3699a`, and each first-party helper
it calls was walked recursively, until only vocabulary items and non-blocking
std or foreign leaves remained. The (d)4 table had four gaps, now closed:
- `flush_sink` calls `Queue::close`;
- `InputRing::close` and `OutputRing::close` each call their own `state`
  helper;
- Windows `DirWatch::drop` calls its `close` helper three times, and that
  helper holds the `CloseHandle`;
- `PtySession::shutdown` has **two** `try_wait` call sites, one of them inside
  the closure passed to `reap_within`.

Walking the bodies again also found `PtySession::shutdown`'s two
`error.into()` conversions to `PtyError`, a first-party `From` impl generated
by `thiserror`'s `#[from]`.

**The rule, unchanged in kind and restated.** Every body the guard reads is a
**pinned body**, identified by name, owner and count.
- Starting from each `Drop` in the table, the guard reads the pinned body and
  resolves each call to a first-party item by `bt_source` item identity.
- It compares, per body:
  - (i) the ordered list of first-party call sites against the row's edge
    list;
  - (ii) the count of each vocabulary effect, per call site, against the
    row's.
- It recurses into every first-party callee. Every callee must itself be a
  pinned body in the table, so the guard reads helpers recursively and
  nothing is exempt.

The build is red when:
- a body calls a first-party item that is not its listed next edge;
- a listed edge is missing or reordered;
- an effect count differs;
- a `Drop` outside the table reaches a registered door or any pinned body.

**Implicit drop glue is not an edge.** Examples are a field dropped at the end
of `drop`, or the seat dropped at the end of `shutdown_all`'s loop. Each type
whose `Drop` runs that way is a row of its own, or it is a `Drop` outside the
table, which the last clause covers.

**Vocabulary used here.** Waits: `thread::sleep`, `JoinHandle::join`,
`Receiver::recv*`. File effects: `File::sync_data`, and `Write::write_fmt`
(`writeln!`) on a `File`. Platform effects: `SetEvent`, `CloseHandle`,
`libc::write`, `libc::close`. The last four are counted so their counts are
pinned, although they do not wait. Every other std or foreign call
(`Option::take`, `Mutex::lock`/`try_lock`/`into_inner`, `Condvar::notify_all`,
`Instant::now`, atomics, `Sender::send`, `eprintln!`, the portable-pty `Child`
methods, and the Core Foundation `signal`/`wake_up`) is a non-blocking leaf.
Leaves are neither edges nor counted. The exception is `Child::try_wait`: it
is counted by call site, as Codex asked, so its multiplicity is pinned.

**The pinned bodies** (name · owner · the ordered first-party edges → and
effects · with counts). Indentation shows the recursion.

| exception (`Drop`) | pinned body chain | product reach | repayment · version |
|---|---|---|---|
| `DirWatch` (Windows) | `DirWatch::drop` (`bt-platform/src/lib.rs`): · `SetEvent` ×1 · `JoinHandle::join` ×1 → `close` ×3 (three call sites, in this order: `dir`, `change`, `stop`) <br> ↳ `close` (the module's `unsafe fn close(handle: HANDLE)`): · `CloseHandle` ×1 | a watch retired on the window thread (row 8) | B7 (D-40) · 0.4.6 |
| `DirWatch` (macOS) | `DirWatch::drop` (`macos_watch.rs`): → `Stopper::signal` ×1 · `JoinHandle::join` ×1 <br> ↳ `Stopper::signal`: leaves only (the run-loop source's `signal`, the run loop's `wake_up`); no edges, no effects | the same | B7 (D-40) · 0.4.6 |
| `trace_sink::Shutdown` | `Shutdown::drop`: → `flush` ×1 <br> ↳ `flush`: → `flush_sink` ×1 <br> ↳↳ `flush_sink`: → `Queue::close` ×1 (inside the poll loop's condition) · `thread::sleep` ×1 · `Receiver::recv_timeout` ×1 · `thread::sleep` ×1 · `JoinHandle::join` ×1 <br> ↳↳↳ `Queue::close`: leaves only (`try_lock`, `Option::take`); no edges, no effects | **conditional**: only `main`'s event-loop build-error return. A reachable constructor failure is not established. The normal exit flushes explicitly and leaves through `leave_process` | *The trace writer is retired through its admitted flush door, never by a drop* · 0.4.7 |
| `AttentionPipe` (Windows) | `AttentionPipe::drop` (`attention_pipe.rs`): · `SetEvent` ×1 · `JoinHandle::join` ×1 · `CloseHandle` ×1; no edges | none in the product: static | *An endpoint is retired through an explicit door, not by its drop* · 0.4.7 |
| `AttentionPipe` (Unix) | `AttentionPipe::drop` (`attention_pipe_unix.rs`): · `libc::write` ×1 · `JoinHandle::join` ×1 · `libc::close` ×1; no edges | none: static | the same · 0.4.7 |
| `LaunchPipe` (Windows) | `LaunchPipe::drop` (`launch_pipe.rs`): · `SetEvent` ×1 · `JoinHandle::join` ×1 · `CloseHandle` ×1; no edges | none: static | the same · 0.4.7 |
| `LaunchPipe` (Unix) | `LaunchPipe::drop` (`launch_pipe_unix.rs`): · `libc::write` ×1 · `JoinHandle::join` ×1 · `libc::close` ×1; no edges | none: static | the same · 0.4.7 |
| `video::engine::Engine` | `Engine::drop` (`video/engine.rs`): → `Engine::shutdown` ×1 <br> ↳ `Engine::shutdown`: · `thread::sleep` ×1 (the 2 ms poll) · `JoinHandle::join` ×1; no edges | a seat closed on the window thread | *A video engine is shut down through an explicit door, not by its drop* · 0.4.7 |
| `macos_player::Engine` | `Engine::drop` (`macos_player.rs`): → `Engine::shutdown` ×1 <br> ↳ `Engine::shutdown`: · `thread::sleep` ×1 · `JoinHandle::join` ×1; no edges | the same | the same · 0.4.7 |
| `VideoSeat` | `VideoSeat::drop` (`video_seat.rs`): → `VideoSeat::shutdown` ×1 <br> ↳ `VideoSeat::shutdown`: → `Engine::shutdown` ×1 (the platform's pinned body above) | a pane closed on the window thread | the same · 0.4.7 |
| `VideoSeats` | `VideoSeats::drop`: → `VideoSeats::shutdown_all` ×1 <br> ↳ `shutdown_all`: → `VideoSeat::shutdown` ×1 (one call site, in a loop; the seat's own drop afterwards is glue, covered by the `VideoSeat` row) | a window closed on the window thread | the same · 0.4.7 |
| `PtySession` | `PtySession::drop` (`bt-pty`), in order: → `PtyDump::finish` ×1 → `PtySession::shutdown` ×1 <br> ↳ `PtyDump::finish`: · `Write::write_fmt` on `File` ×1 → `PtyDump::publish` ×1 <br> ↳↳ `PtyDump::publish`: · `File::sync_data` ×2 <br> ↳ `PtySession::shutdown`, in order: → `InputRing::close` ×1 · `Child::try_wait` ×1 · `Child::kill` (leaf) → `PtyError::from` ×1 (the `kill` error) → `reap_within` ×1 (with the closure: · `Child::try_wait` ×1) → `PtyError::from` ×1 (the `try_wait` error) → `OutputRing::close` ×1 → `join_within` ×1 <br> ↳↳ `InputRing::close`: → `InputRing::state` ×1 <br> ↳↳↳ `InputRing::state`: leaves only (`Mutex::lock`) <br> ↳↳ `OutputRing::close`: → `OutputRing::state` ×1 <br> ↳↳↳ `OutputRing::state`: leaves only <br> ↳↳ `reap_within`: · `thread::sleep` ×1 (plus the closure's call, counted at the closure) <br> ↳↳ `join_within`: · `JoinHandle::join` ×1 · `thread::sleep` ×1 <br> ↳↳ `PtyError::from` (generated): leaves only | on `pty-retirement`; on the caller only when that thread cannot be started | *A shell is taken apart only through `retire_within`, never by a drop on the window thread* · 0.4.7 |

**Edges added over (d)4: eight first-party edges.**
- `close` ×3, in Windows `DirWatch::drop`;
- `Queue::close` ×1, in `flush_sink`;
- `InputRing::state` ×1;
- `OutputRing::state` ×1;
- `PtyError::from` ×2, in `PtySession::shutdown`.

Besides the edges, (e) makes these corrections:
- one effect-count correction: `Child::try_wait` goes from 1 to 2 call sites;
- one effect moved to its real body: `CloseHandle` now sits in `close`, not in
  `DirWatch::drop`;
- the pinned helper bodies are now listed: `close`, `Stopper::signal`,
  `flush`, `flush_sink`, `Queue::close`, both `Engine::shutdown`,
  `VideoSeat::shutdown`, `shutdown_all`, `PtyDump::finish`, `PtyDump::publish`,
  `PtySession::shutdown`, both ring `close` and `state` bodies, `reap_within`,
  `join_within` and `PtyError::from`.

**The controls are green under the rule as written.** Each row above was
checked once more against its body at `78a3699a` before this commit. Every
first-party call in each pinned body is the listed next edge, in the listed
order, and every vocabulary count matches. Today's complete chains are
therefore the passing controls.

**The five mutations of (d)4 stand unchanged, and each is red under the
closed rule:**
1. `VideoSeats::drop` → a new helper → `SessionWriter::wait_for`;
2. a second `thread::sleep` in `video::engine::Engine::shutdown`;
3. a non-vocabulary helper in `Engine::shutdown` that calls
   `trace_sink::flush`;
4. `PtySession::drop` calling `shutdown` before `finish`;
5. a new `impl Drop for WebSeat` calling `Compositor::commit`.

**Adopted.** A1e remains held until Codex confirms this row set.

### (e)4 · §11, as it stands for dispatch

- **A1a is dispatchable once (e) lands.** P1 (the test allocation, (d)1), P2
  (the cookie, (d)2 and (e)1) and P3 (the door inventory, (e)2) are the
  preconditions Codex named for A1a. The registry A1a seeds is (e)2's table
  plus row 23.
- **A1b and A1c** follow A1a, as (c)1 says.
- **A1d** follows A1a. Its brief is (e)2's table.
- **A1e waits until Codex confirms (e)3.**
- **A2** and **A3** keep (c)1's prerequisites.

### (e)5 · This revision's own architecture impact

(a) Facts touched: none; this is a document.

(b) Doors: none in this commit. A1d's scope gains `CompositorBirth`,
`CompositorWindowSize`, `WebRehost` and `GpuOpen`, and `Compositor::commit_now`
becomes private.

(c) Debt: no row repaid or added by this commit. A1a adds §5.3 row 23
(`GpuOpen`, `pending`) with a DESIGN entry recording it. That entry records a
wait found, not a ruling that the wait may stay.

(c′) None.

(d) No.

---

## Revision 2026-09-26 (f), after the last Codex confirmation

Review: `docs/plans/design/thread-door-review-codex-2026-09-26-d.md` (Codex,
static, at `e8290621`). It found:
- the P2 wording **met**;
- P3 and P4 each one narrow change short;
- **no new registry door**.

This revision is appended, and where it differs from earlier text it rules. It
changes two things and nothing else.

### (f)1 · P3 — `commit_now`'s visibility, and a reproducible caller-search closure: adopted

**Checked.**
- `Compositor` is defined inside `mod windows_impl` in
  `bt-platform/src/lib.rs`, while `webview` is a sibling module
  (`mod webview;`) of the crate root.
- A private method of `Compositor` is therefore not reachable from
  `WebHost::rehost` or `WebHost::compensate`. (e)2's "private `commit_now`"
  could not serve four of its six internal statements.

**The contract, replacing (e)2's paragraph on `commit_now`.**

```rust
// bt-platform, mod windows_impl
impl Compositor {
    /// The door: the only public road to a composition commit.
    pub fn commit(&self, token: WaitToken<'_, doors::CompositorCommit>) -> Result<(), String> {
        let _ = token;
        self.commit_now()
    }
    /// Tokenless; crate-visible so `webview` can reach it. Its callers are pinned.
    pub(crate) fn commit_now(&self) -> Result<(), String> { /* today's `commit` body */ }
}
```

`commit_now` is `pub(crate)`. Its callers are **exactly seven**, and A1e pins
this list by owner and count:

| # | caller (owner) | covered by |
|---|---|---|
| 1 | `Compositor::commit`, the forwarding call | `CompositorCommit`'s own admission |
| 2 | `Compositor::new`, after the tree is built | `CompositorBirth` |
| 3 | `Compositor::set_window_size`, after `place_skirt` | `CompositorWindowSize` |
| 4 | `WebHost::rehost`, the `RehostStep::CommitSource` arm | `WebRehost` |
| 5 | `WebHost::rehost`, the `RehostStep::CommitTarget` arm | `WebRehost` |
| 6 | `WebHost::compensate`, `restore.target` | `WebRehost` (its only caller is `rehost`) |
| 7 | `WebHost::compensate`, `restore.source` | `WebRehost` |

- A1e's check is red on an eighth caller, a moved one, or a changed count.
  Codex's review is the base for the list (`rg -a`: the six internal
  statements plus the forwarding call).
- The macOS and portable `Compositor`s get the same pair (`commit` with the
  token, and `pub(crate) commit_now`), each with only the forwarding call as
  its caller, since neither has internal commits today.

**The caller-search closure, replacing (e)2's "The patterns" table.**

This is how the table's caller cells are produced, and how to re-run them.
For each door row, run one search for:
- every **minting function** the row names; and
- every **further caller level the row prints**, for example `CompositorBirth`:
  `Compositor::new`, `spare_parent`, then `Runtime::create` ← `resumed`,
  `Runtime::open_window` ← `open_pending_window`, and
  `make_spare_web_controller` ← `warm_web_engine`.

A level the row does not print is not claimed complete. Each search is:

```
rg -a -n --type rust '<pattern>' crates
```

- **`-a` is required**, because `crates/bt-platform/src/lib.rs` holds one NUL
  byte and ripgrep would otherwise skip it as binary.
- **Filters:**
  - drop comment lines;
  - drop lines in test code (as (e) defines it);
  - drop test-model code, such as the `Recorded` fake seat in `web_spare.rs`'s
    test module;
  - drop source-needle string literals.
- **Homonyms** are resolved by module and receiver, for example:
  - `WebSeat::start_environment` versus the fake's;
  - `Runtime::resize` versus `WindowRenderer::resize` and `PtySession::resize`;
  - `SessionStore::close` versus the fifteen other `close` methods;
  - `App::finish` versus the clipboard ports' and `PtyDump::finish`.

The searches, per row:

| row | searched functions (pattern ⇒ the level printed) |
|---|---|
| `PtyBirth` | `\bcreate_leaf_session\(` · `\bcreate_tab_state\(` |
| `PtyResize` | `\bcommit_leaf_resize\(` · `\brelease_due_leaf_resize\(` · `\bflush_pending_pty_resize\(` |
| `PaneRetirementWait` | `\bsettle_quit\(` (**added**) |
| `SessionWriteWait` | `\bwait_for_landing\(` (**added**) · `\bflush_judged\(` · `\bself\.flush\(\)` in `persist.rs` · `session_store\.close\(`/`fn close\(&mut self\)` (resolve to `SessionStore`) · `app\.finish\(\)` |
| `SessionWriterRetire` | `self\.writer\.close\(` · `app\.finish\(\)` |
| `TraceFlush` | `trace_sink::flush\(` · `^\s*flush\(\);` in `trace_sink.rs` |
| `LaunchHandOver` | `launch_wire::hand_over\(` |
| `GpuOpen` | `GpuContext::open\(` · `Runtime::create\(` |
| `PresentFrame` | `present_frame_with_phases\(` · `present_seats_and_commit\(` |
| `CompositorCommit` | `\.commit\(\)` · `\bstand_parked\(` · `self\.adopt\(from` · `seat\.advance\(parent` · `web_spare\.advance\(`/`\bspare\.advance\(` · `web_spare\.retire\(\)`/`seat\.retire\(parent` · `make_spare_web_controller\(` · `warm_web_engine\(` |
| `CompositorBirth` | `Compositor::new\(` · `spare_parent\(` · `Runtime::create\(` · `Runtime::open_window\(` · `make_spare_web_controller\(` · `warm_web_engine\(` |
| `CompositorWindowSize` | `\.set_window_size\(` · `self\.resize\(`/`runtime\.resize\(` (resolve to `Runtime::resize`) |
| `WebRehost` | `\.rehost\(` · `compensate\(` |
| `SurfaceBirth` | `WindowRenderer::new\(` · `Runtime::open_window\(` |
| `TitleFlush` | `\bflush_title\(` |
| `ImeCaretArea` | `apply_ime_cursor_area\(` · `flush_ime_cursor_area\(` |
| `FocusWindow` | `open_from_notification\(` · `route_clicked_notifications\(` |
| `SetVisible` | `put_the_window_on_the_glass\(` · `show_new_window\(` · `show_quake_window\(` · `\bsummon_quake\(` · `hide_quake_window\(` · `dismiss_quake\(` · `let_go_of_this_window\(` · `\bretire_window\(` · `\bclose_window\(` |
| `SetCursor` | `apply_pointer_cursor\(` |
| `WebController` | `self\.step\(&effect` · `self\.apply\(effect` |
| `WebEnvironment` | `start_environment\(` |
| `FontFamilyLookup` | `monospace_family_files\(` · `apply_stored_terminal_font\(` |
| `PlaceHidden`, `PlaceExposure` | `sample_window_place\(` · `observe_window_place\(` |

**The closure was re-run for every row at `78a3699a`. It changed two rows.**
All other rows' printed caller cells were reproduced exactly.

1. **`SessionWriteWait`:** `SessionStore::flush_judged` calls
   `wait_for_landing` at **two** statements, not one (its two branches). The
   minting statement is inside `wait_for_landing`, so the door and the
   admission are unchanged. The cell now reads: `wait_for_landing` ←
   `SessionStore::flush_judged` (×2) ← …, and the witness asserts one
   admission per `wait_for_landing` call reached.
2. **`WebController`:** `WebSeat::apply` has **eleven** callers, not ten.
   `WebSeat::close` (`webhost.rs`) is the eleventh. The cell now reads:
   `step` ← `WebSeat::apply` ← `go_adopted`, `go`, `restart_engine`,
   `reload`, `drive`, `retire_parked`, `tick`, `place`, `rehost`, `go_to`,
   `close`.
   - The row's phases gain `Exiting`, because `WebSeat::close` runs on the
     way out.
   - A close reaching `request_controller` is not the ordinary case (a
     closing seat does not ask for a controller). But the table admits
     whatever the minting statement can reach, so the phase set must cover
     it, as (c)5's rule for `Exiting` requires.

**Adopted.**

### (f)2 · P4 — the portable engine body pinned: adopted

**Checked.**
- On a target that is neither Windows nor macOS, `video::engine::Engine` is
  `pub use no_player::Engine` (`bt-platform/src/video_portable.rs`).
- Its `shutdown` is `match self._never {}`: an uninhabited field, so the body
  never runs. It has no first-party calls and no vocabulary effects.
- The type has no `impl Drop`.

**The change to (e)3's table.** Under the closed rule every callee must be a
pinned body, so the `VideoSeat` row names **three** platform targets for
`VideoSeat::shutdown` → `Engine::shutdown`:

| platform | pinned `Engine::shutdown` body | edges · effects |
|---|---|---|
| Windows | `video::engine::Engine::shutdown` (`video/engine.rs`) | none · `thread::sleep` ×1, `JoinHandle::join` ×1 |
| macOS | `macos_player::Engine::shutdown` | none · `thread::sleep` ×1, `JoinHandle::join` ×1 |
| other (portable) | **`video::engine::no_player::Engine::shutdown`** (`video_portable.rs`), body `match self._never {}` | **none · none** |

- The `VideoSeats` row follows through `VideoSeat::shutdown` with the same
  three targets.
- The guard resolves the target per compiled configuration.
- It pins all three bodies, and the source guard reads all three whatever the
  host. So every platform's today-chain is a green control under the closed
  rule.
- The rule and the five mutations are unchanged.

**Adopted.**

### (f)3 · Dispatch, as it stands

- **A1a is dispatchable once (f) lands.** P1–P3 are met, and Codex found no
  further registry door.
- A1d's brief is (e)2's table as amended by (f)1.
- A1e's brief is (e)3's table as amended by (f)2, with the `commit_now` caller
  pin of (f)1.
- Everything else in (e)4 stands.

### (f)4 · This revision's own architecture impact

(a) None.

(b) None in this commit. `Compositor::commit_now` is `pub(crate)` with seven
pinned callers.

(c) None repaid or added by this commit.

(c′) None.

(d) No.

---

## Revision 2026-09-27 (g), A1e's check of the exception table against the code

Codex's confirmation `docs/plans/design/thread-door-review-codex-2026-09-27-e.md`
(static, at `e70c94bf`) found all twelve rows of (e)3 as amended by (f)2 matching
their bodies, and said A1e may be dispatched from the table as written. A1e was
built on `a0c96f53`, **after A1d merged**, and read every pinned body there again
with the guard itself (`hang_watch::window_waits_tests::every_door_is_where_the_registry_says`).
Two rows no longer match the table, because code changed after the review's base,
and the table's leaf rule needs one sentence to be checkable. This revision is
appended; where it differs from (e)3 and (f)2, it rules. The table as the guard
holds it is the `EXCEPTIONS` and `PINNED` tables in that test file.

### (g)1 · `trace_sink::Shutdown`: A1d put the admission between the drop and the flush

**Checked.** A1d made `trace_sink::flush` a door (`flush(token: WaitToken<'_,
doors::TraceFlush>)`) and `Shutdown::drop` mints it:
`let _ = admitted::<doors::TraceFlush, _>(flush);`. The first-party edges of the
drop are therefore `admission::admitted`, then `flush`, handed to the admission
by value as its work. The table's `drop → flush` was right at `e70c94bf` and is
not at `a0c96f53`.

**The row, replacing (e)3's.** Under the closed rule every callee is a pinned
body, so the admission's own body and its helpers are pinned too. None of them
has an effect.

| body | edges (→) and effects (·) |
|---|---|
| `Shutdown::drop` | → `admission::admitted` ×1 → `trace_sink::flush` ×1 (by value, as the admission's work) |
| `admission::admitted` | → `role` → `Phases::contains` → `count` → `meter` → `WaitToken::fresh` → `WaitToken::fresh`; no effects |
| `admission::role`, `count`, `meter`, `WaitToken::fresh`, `Phases::bit` | no edges, no effects |
| `admission::Phases::contains` | → `Phases::bit`; no effects |
| `trace_sink::flush` | → `flush_sink` ×1; no effects |
| `flush_sink`, `Queue::close` | unchanged: `flush_sink` → `Queue::close` · `thread::sleep` ×2 · `Receiver::recv_timeout` ×1 · `JoinHandle::join` ×1 |

Product reach and repayment are unchanged: only `fn main`'s event-loop build-error
return, which says `exiting()` first; *The trace writer is retired through its
admitted flush door, never by a drop* · 0.4.7 (D-78).

### (g)2 · A thirteenth row: `http::Request` (Windows)

**Checked.** `crates/bt-platform/src/http.rs` (the WinHTTP arm of `https_download`,
0.4.6 ticket U-7, after `78a3699a`) has `impl Drop for Request`: it closes the
request handle (`WinHttpCloseHandle`, not a vocabulary entry) and then waits, up
to `CLOSE_WAIT` (5 s), on the download's `Condvar` for WinHTTP's closing callback
(`Shared::locked` for the signals, then `raised.wait_timeout`). `Condvar::wait_timeout`
is a registry vocabulary entry that waits, and the row is in no table, so the
closed rule's last clause refuses it as the tree stands. No review read it: it
came after every base the table was checked at.

| exception | pinned body chain | product reach | repayment · version |
|---|---|---|---|
| `http::Request` (Windows) | `Request::drop` → `Shared::locked` ×1 · `Condvar::wait_timeout` ×1 <br> ↳ `Shared::locked`: leaves only (`Mutex::lock`, `expect`) | **none in the product yet**: `https_download` has no product caller on `a0c96f53`; the update's download will call it, on a worker (U-7's own contract) | new ticket *A download's request is closed through its own bounded door, not by its drop* · 0.4.7 (D-82) |

It is bounded and would run on a worker, but the rule is that a `Drop` which
waits is a row with a repayment, whichever thread drops it; it is not ruled to
stay.

### (g)3 · Leaves at a receiver whose type the source does not write

(e)3 lists the leaves by the std or foreign item they are (`Option::take`,
`OnceLock::get`, …). The guard resolves a call by what the source says about it:
a receiver whose type is written — `self`, a field of it, a parameter — narrows the
candidates to that type's methods; a receiver whose type is not written (a local, a
static, a call's result) could be any method of that name the package can reach.
Where a std leaf at such a receiver shares its name with a first-party method, the
row lists the name as a leaf, and a listed leaf that is never called is red, so the
list cannot outlive its reason. At `a0c96f53` that is three names in three bodies:
`flush`'s `SINK.get()` (`get`), and `flush_sink`'s and `Queue::close`'s
`Option::take` (`take`). Two further rules the guard applies, stated so the table
can be read against it:

- `PtySession::shutdown`'s two `error.into()` resolve to `PtyError::from`, which
  `thiserror`'s `#[from]` generates; it is an edge with no body to read.
- A call that is both a vocabulary effect and a first-party name at an untyped
  receiver is the effect: `child.try_wait()` is `Child::try_wait` (counted), not
  `PtySession::try_wait`.

### (g)4 · The inventories the guard pins, as they are on `a0c96f53`

- **The owners of §C-3** are the registry's new `# owners` section
  (`crates/bt-app/src/window_waits.tsv`), counted from the code: `msg_send!` 19
  in 19 owners, one each (the note's 20, at `78a3699a`), `extern` blocks 6, `#[link]` **5**
  (the note's 6), `vtable(` 1 (`video::engine::Machinery::take_frame`),
  `GetProcAddress` 0, `#[macro_export]` 0. The note's "one exported macro exists
  today" is `bt-source`'s `needle!`, which is not in the product.
- **The `enter_callback` entries** are nineteen calls in ten (module, name)
  pairs: (c)7's, plus U-7's four `http-download-session` delegate entries.
- **The expected count of a door's `expect(clippy::disallowed_methods)`** is
  zero until A2 turns the lint on (§9.1's parenthesis); A2 makes it the
  registry's.
- **A `Drop` outside the table** is refused when it names a vocabulary entry
  that waits, or calls a registered door or a pinned body. §C-3's "vocabulary is
  refused inside any `impl Drop`" is read as the entries that wait: three
  `Drop`s outside the table (`attention_pipe`'s `OwnedHandle` and `Overlapped`,
  `instance`'s `DataDirectoryClaim`) close a handle with `CloseHandle`, which the
  registry lists as counted, not as a wait.

### (g)5 · This revision's own architecture impact

(a) None. (b) None. (c) This commit adds nothing to the ledger; A1e's docs
commit opens D-78…D-82 for (e)3's rows without one and for (g)2, and notes D-40
as `DirWatch`'s. (c′) None. (d) No.

---

## Revision 2026-09-27 (h), the leaf list after U-16 and U-22

**Checked**, on `cae97e8a` (A1e merged over U-16 `macos_identity` and U-22
`logon_hook`/`update_recover`). U-22 added a first-party trait method named `get`
(`logon_hook::os::<CurrentUser as Registry>::get`, both arms). `admission::meter`'s
body reads the installed meter with `METER.get()`, a `OnceLock::get` at a receiver
whose type the source does not write (a static), so by (g)3's rule the guard could
no longer tell it from the new method, and the `trace_sink::Shutdown` row went red.
Nothing waits there and no edge changed.

**The change to (g)1's table:** `admission::meter` lists `get` as a leaf, as
`trace_sink::flush` already does for its own `SINK.get()`. No other row moved: U-16
and U-22 added no `Drop` that waits, no `msg_send!`, `extern` block, `#[link]` or
`vtable(` outside the owner table, no role or phase writer call and no callback
entry, all of which the guard re-read on `cae97e8a`.

(a)–(d): none.


---

## Revision 2026-09-26 (i), after A2's survey: the lint lands last, behind doors built by family

**Why this revision.** A2 was dispatched on `fcda5dfa` as one M ticket whose
brief read: turn the lint on, put one `expect` on each of the registry's doors,
count 0 → 26. The agent turned the lint on as specified and ran CI's clippy line
with the lint at warning level so that every site would print
(`trace/tickets-046/reports/A2.md`, census in `A2-census/windows-product-sites.tsv`):

| selection | vocabulary sites | functions |
|---|---|---|
| first-party product code, Windows arms (libraries, `folio`, build scripts; not mitex, bt-corpus, bt-source) | **228** | 171 |
| of which inside a registered door | about 18 | — |
| CI's `--all-targets` line (tests, examples, development binaries added) | 1,123 | — |

By family: waits 68 (`sleep`, `join`, `recv*`, `Condvar::wait*`, `try_wait`,
`block_on`), file writes 57, file observation 45 (`metadata`, `read_dir`,
`canonicalize`), calls into registered door entrances 29, counted Win32 handles
22 (`CloseHandle`, `SetEvent`), child-process waits 6. By crate: bt-app 123
(+3 in its `build.rs`), bt-platform 64, bt-pty 22 (+3 in its `build.rs`),
bt-render 5, bt-persist 5, bt-term 3. The macOS arms (`libc::write`/`close`,
the macOS `DirWatch`, the locale children) add to these and were not counted.

The premise was wrong in this note, not in the agent's reading of it: §6.3 and
§8 give A2 the worker doors that do not exist yet (`file_writes`, `wait::*`, a
door for child-process waits) and (c)6 gives it `bt-pty`'s transport doors and
the `file_reads` execution-level choice, while §11 sized A2 as one M and the
brief's architecture-impact line said "no new door". Nothing committed; the
branch was deleted at BASE. The survey also found four facts that hold whatever
the split (see (i)6).

### (i)1 · A2 is five tickets, and the lint is the last of them

The invariant of B§C-1 is unchanged: every listed effect passes through a door
or the build fails. What changes is the order in which it becomes true. Turning
`disallowed_methods` to `deny` (or leaving it at its default `warn` under CI's
`-D warnings`) on a tree with 228 bare sites cannot be green, and B§C-2 item 1
forbids lowering it, so the lint cannot be switched on before the doors exist
and the sites go through them. The doors come first, by family; the lint comes
last, and between the two a **source-level gate holds the bare-site count
shrink-only**, the way `MIGRATION-DEBT.tsv` holds its rows.

| id | title | size | prerequisites |
|---|---|---|---|
| **A2a** | The bare-site inventory is a shrink-only gate; the vocabulary is corrected; the non-Rust checks and the shields land | S–M | A1e on main (it is) |
| **A2b** | The file doors: `file_writes` is born, `file_reads` takes option (d), observation reads go through it; the window thread's file effects get their interim owner doors | M | A2a |
| **A2c** | The wait doors: `wait::*` and the child-process waits are born; the counted handles get theirs; the deferred owner rows get their interim doors | M | A2a |
| **A2d** | `bt-pty`'s transport doors ((c)6 item 1); the admitted exclusion forms for tests, build scripts and tool crates, and the guard that accepts exactly them | S–M | A2a |
| **A2e** | The lint: `deny` in the two lint tables, the root `clippy.toml` compared with the registry whole and with multiplicity, one `expect` per door, the per-target plants and checks, M1–M15; the bare list is empty and is deleted | M | A2b, A2c, A2d |

A2b, A2c and A2d touch different families and mostly different crates and may
run in parallel; each shrinks the committed bare list, and a merge between them
keeps the smaller of both sides row by row (a row present on either side after
the merge is red, so the merge re-runs the gate). A2e is the only ticket that
flips a lint level, and it does so when the list is empty.

**Version.** A2a–A2e stay 0.4.6 as A2 was; the owner's D-2 ruling ("closes when
A1, A2 and A3 have landed") reads A2 as all five. If lanes do not allow it before
the 0.4.6 release, A2b–A2e move together to 0.4.7 and ARCHITECTURE §6 keeps
saying the lint is pending with the bare count — the owner's call, flagged.

### (i)2 · A2a — the gate before the lint

1. **The bare-site inventory** is a `bt-source` query held by
   `window_waits_tests` (or a sibling test in the same file): every vocabulary
   path or method name (the registry's `# vocabulary` section, both spellings,
   through the identifier view (g)3 already uses) found in first-party product
   code **outside the body of a registered door function** (the `# doors` and
   `# owners` sections, plus the interim doors (i)3–(i)4 add), listed by crate,
   item identity and vocabulary entry. Both platforms' `cfg` arms are read (the
   source view does not resolve `cfg`; a Windows-only site and its macOS twin are
   two rows). Committed as `docs/plans/window-thread-bare-sites.tsv`; the gate
   refuses a row not in the file (a new bare site) and a file row the code no
   longer has (stale — delete it in the same commit). The count is printed in
   the run footer and stated in ARCHITECTURE §6.
2. **The vocabulary is corrected**: `wgpu::SurfaceTexture::present` →
   `wgpu::Queue::present` (wgpu 30: `Queue::present(&self, SurfaceTexture)`;
   bt-render calls `gpu.queue.present(texture)`). A1e's owner and count
   assertions re-read.
3. **The shields**: `vendor/clippy.toml`, `crates/bt-corpus/clippy.toml` and
   `crates/bt-source/clippy.toml`, each `disallowed-methods = []` with a comment
   naming this revision. They are the closed set the script knows; a fourth
   `clippy.toml` anywhere in the tree is red. `CLIPPY_CONF_DIR` is set nowhere
   (it would override the shields) — this supersedes B§C-4's "per-target
   `clippy.toml` directory through `CLIPPY_CONF_DIR`"; see (i)6 for why one root
   file suffices.
4. **The non-Rust checks** in `scripts/ci/check-window-waits.ps1` (new or the
   existing check A1e calls): no `-A`/`-W`/`--cap-lints` touching
   `clippy::disallowed_methods`, `clippy::style`, `clippy::all` or `warnings` in
   `.github/workflows/*.yml`, `.cargo/config.toml`, `RUSTFLAGS`/`CLIPPY_FLAGS`
   in `scripts/**`, or any member `[lints]` table other than the two (i)5 names
   (M7e); a registry line without a ruling refused (M9); every vocabulary entry
   assigned to at least one target; for each product target, every entry
   assigned to it has a first path segment that is a crate in
   `cargo metadata --filter-platform <triple>`'s graph (the misspelled-crate
   half of M8b, which clippy does not report — (i)6). The script runs in `logic`
   and `core-macos` from A2a on.
5. **No lint change and no root `clippy.toml`** in A2a: `disallowed_methods` is
   in clippy's `style` group, warn by default, and CI runs `-D warnings`, so a
   root vocabulary file would make every bare site red at once. The vocabulary
   file is A2e's.

Red on BASE: the inventory test (no file), a planted bare site (added row → red),
a planted stale row, a fourth `clippy.toml`, a planted `-A clippy::disallowed_methods`
in a workflow, a registry line without a ruling, an entry naming `nosuchcrate::wait`.

### (i)3 · A2b — the file doors, and `file_reads` option (d)

- **`bt_platform::file_writes`** is born: `write`, `rename`, `copy`,
  `create_dir_all`, `File::create`, `sync_data`/`sync_all` and the `writeln!`
  family (`Write::write_fmt`) behind functions taking `&WorkerCtx`, each a
  registry door line. `durable_write`/`durable_move` (U-11's `install_txn`) and
  `bt-persist`'s stores are its first callers.
- **`file_reads` takes option (d)** of (c)6 item 3: a worker lane's `Reader`
  borrows `&WorkerCtx` for its life (so it is `!Send` and cannot outlive the
  worker body), and the inventoried window-thread lanes (`Lane::Settings` at
  launch, `Lane::Fonts` in bt-render, and whatever A2b's inventory adds) get
  owner doors with `WaitToken`s. **Observation reads** (`metadata`, `read_dir`,
  `canonicalize`) are `file_reads`' too: `file_reads::observe(...)` on a worker
  context or an owner token, by lane.
- **The window thread's file effects get interim owner doors.** Row 20's renames
  and writes (`runtime/files.rs`, `runtime/preview.rs`, `persist.rs`,
  `schemes.rs`) and the window-thread observation reads that are **not registry
  rows today** (`main.rs`, `runtime/preview.rs`, `runtime/profiles.rs`,
  `preview.rs`, `webhost.rs`) each become a registry row, status `open`, owed to
  B8 (the stores) or to a new **B10** (the window thread's file observation
  moves to a worker or a cache), with a thin owner door function holding exactly
  the one effect — the same shape A1d gave rows 2–20 with `admitted`. B8/B10
  later move the effect and delete the door; the registry line's `status`
  becomes `done` with its residue, if any.

Ruling carried here (coordinator, 2026-09-26, within the owner's delegation of
Q2-class decisions): a window-thread effect found by the inventory and not yet
ruled is admitted **as it is today** behind an interim door and a registry row
marked `open` with the owing B-ticket named. It is not moved by A2 (§8 "No
move" stands), and it is not left bare.

### (i)4 · A2c — the wait doors

- **`bt_platform::wait`** is born: `sleep_bounded`, `join_bounded`,
  `recv_bounded`, `recv_timeout_bounded`, `condvar_wait_bounded`, `block_on`,
  each taking `&WorkerCtx`, each a registry door line; the 68 worker waits go
  through them, including the three standalone-main `recv_timeout`s of (c)6
  item 2 (their threads already hold a `WorkerCtx` from `enter_standalone_main`).
- **Child-process waits move into the door**: `quiet_command(_named)` gains
  `output_within`/`status_within`/`wait_within` on `&WorkerCtx`, and its callers
  stop calling `Command::output`/`status`/`Child::wait` themselves (§6.3's
  "the waits are at the callers" ends).
- **Counted handles** (`CloseHandle`, `SetEvent`; `libc::write`/`close` on
  macOS) — 22 Windows sites in bt-platform — go through one `handles` door per
  platform (`close_handle`, `set_event`), or stay inside the door body whose
  effect they are, listed on that door's line with the count. The rule of B§C-2
  item 3 (one `expect`, the declared number of effects) decides per site.
- **The deferred owner rows get their interim doors**: rows 2, 3, 4
  (`profile_marks`, `psreadline`), 7 (`read_system_locale_declaration`), 10
  (device recovery's `block_on` and `sleep` in `main.rs`) — each a thin owner
  door function, one effect, `open`, owed to B4/B5/B6/B9 as the registry already
  says. Row 8 (`DirWatch::start_scoped` ×3, owed to B7) is A2c's too unless A2b
  reaches it first (say which in the report).

### (i)5 · A2d — `bt-pty`'s transport doors, and the admitted exclusions

- **(c)6 item 1 stands**, and the A2 brief that told the agent not to lint
  bt-pty's doors into existence was wrong against this note: the ring waits,
  `join_within`, `reap_within`, `Retirements::wait_within`, the dump publisher's
  sleep and sync and `try_wait` become registry lines with role `Transport`,
  outside the admission invariant, each function carrying its `expect` from
  A2e on, with the one debt row (c)6 names.
- **The admitted exclusion forms** — B§C-3 puts `build.rs`, tests, examples,
  benches and development tools outside the invariant, but clippy 1.94.1 has
  no in-tests option for this lint, and CI's `--all-targets` lints them all.
  This supersedes B§C-2 item 2's "these forms appear nowhere" for exactly three
  spellings, each pinned by the guard and by the script:
  1. **tests** — `#![cfg_attr(test, allow(clippy::disallowed_methods))]` at
     the crate root of every product crate (`lib.rs` / `main.rs`) and at the
     top of every `tests/*.rs`, `examples/*.rs` and `benches/*.rs` file; that
     exact text, once per file, within the file's leading attribute block. A
     `cfg_attr(test, …)` on a `#[cfg(test)] mod` or anywhere else stays red
     (A1e assertion 5, narrowed to admit the root form).
  2. **build scripts** — `#![allow(clippy::disallowed_methods)]` at the top of
     each `build.rs`, and nowhere else.
  3. **tool crates and vendor** — the shields of (i)2 item 3.
  The guard's product universe already excludes test modules by declaration;
  the amendment is that the root attribute is read and matched by text, not
  treated as a product attribute. Mutations: the root form moved one item down
  → red; the same form on a `mod tests` → red; `#![allow(...)]` in a product
  `lib.rs` → red; a fourth shield → red.
- **`bt-platform`'s own `[lints.clippy]` table** (it cannot inherit the
  workspace table because `unsafe_code` must stay allowed there) carries the
  same `disallowed_methods = "deny"` line from A2e on; the script checks both
  tables and refuses any third table naming the lint. This narrows B§C-2 item 1's
  "fixed in one place" to "fixed in two places, both checked".

### (i)6 · What the survey established, carried into A2e

1. **Clippy refuses neither M8b case.** A misspelled path in a crate that is in
   the graph gives one non-lint warning (`does not refer to a reachable
   function`) that `-D warnings` does not make fatal (rc = 0); a path whose crate
   is not in the graph gives no message at all. So A2e's jobs **scan clippy's
   output for that warning and fail on it**, and the metadata check of (i)2
   item 4 catches the absent crate. With those two checks **one root
   `clippy.toml` carrying every target's entries suffices**: an entry for a crate
   absent on this target is ignored by clippy by construction (measured), and
   the registry's `targets` column plus the metadata check tell "not on this
   target" from "misspelled". No `CLIPPY_CONF_DIR`.
2. **`writeln!` is caught**: `std::io::Write::write_fmt` fires through the
   external macro (15 product sites), so B§C-3's macro row holds for it.
3. **The vendored crates need the shield** of (i)2 item 3:
   `alacritty_terminal` says `#![cfg_attr(clippy, deny(warnings))]` and would
   refuse its own `File::create` under the root vocabulary; with
   `vendor/clippy.toml` it is clean, and the crate directories stay byte-identical
   for `check-vendor-notices.ps1`.
4. **The door `expect` count** A1e pins at 0 becomes, in A2e, the number of
   registry door lines including the transport and interim doors — not 26.

### (i)7 · Plants and mutations, redistributed

A2a: the inventory's planted bare site and stale row; M7e; M9; the absent-crate
entry. A2b/A2c/A2d: each family's planted bare site red on the gate before the
door exists and green after (the passing control), plus the exclusion-form
mutations in A2d. A2e: M1, M2a/M2b, M6, M8a (an entry deleted from
`clippy.toml` or the registry → the whole-and-multiplicity comparison), M8b
(the reachable-function warning and the absent crate, both jobs), M15 (a
Windows-only and a macOS-only violation → each job's lint; the macOS half is
possible now that ticket 72 lints every product crate there), and the
`gates-can-fail` plants per target.

### (i)8 · This revision's own architecture impact

(a) none. (b) none by this commit; A2b/A2c add `file_writes`, `wait`, the
child-process and handle doors, and the interim owner doors, each a registry
line. (c) none by this commit; A2b opens B10 and A2d opens (c)6's transport
debt row. (c′) none. (d) yes — the two amendments to B§C-2 (three admitted
exclusion spellings; two lint tables) and the one to B§C-4 (no `CLIPPY_CONF_DIR`;
one root file plus two checks), each stated here and pointed to from the budget
note in A2a's docs commit.

---

## Revision 2026-09-26 (j), after the Codex review of (i)

Review: `trace/tickets-046/thread-door-review-codex-2026-09-26-i.md` (adopt
with changes; nine findings, P1–P8 high or medium, P9 low; every practical claim
of (i) tested on clippy 0.1.94 / Rust 1.94.1 in a scratch workspace). All nine
are adopted. The family split and the lint-last order stand; what changes is
that **A2a now lands the inventory and the schema the other four need, and
A2b–A2e are briefed only after A2a's inventory is on main** and every effect in
it has a disposition (revision (k), the allocation table). (i)'s counts are
kept as the Windows diagnostic survey they were and are not the baseline.

### (j)1 · P1 — the bare-site gate is shrink-only against the merge base, with multiplicity

(i)2 item 1 compared the observed set with the committed file two ways; that
accepts a new site landed together with its new row, and it cannot see two
identical effects in one function. The gate now holds three things:

1. **Row identity with multiplicity.** A row is `(crate, cfg arm, item
   identity, vocabulary entry, count)`, where the item identity is
   `bt_source`'s (module path and name, the arm from the enclosing `cfg`), and
   `count` is the number of sites of that entry in that item. The file is
   `docs/plans/window-thread-bare-sites.tsv`.
2. **Two comparisons, both required.** `observed == committed` as multisets;
   and `committed ⊆ merge-base committed` as multisets — the historical half
   `scripts/ci/check-migration-debt.ps1` already performs for MIGRATION-DEBT
   (whole rows, against the PR merge base, added rows refused). The seeding
   commit is the one exception, named by sha in the script; there is no
   "missing base → pass" road after it.
3. **A move is a removal.** A function that carries a bare site and changes
   its item identity is one deletion and one addition, and the addition is
   refused; a ticket that must relocate such a function first routes the site
   through a door. Rows whose effects neither merged side removed stay legal
   across a merge; a row whose effect either side removed is stale and red.

At A2e the gate proves the observed multiset empty **and then** the file is
deleted; from then on the check requires the file to be absent and the multiset
empty. A missing file never disables the check. Plants: a new site with its
matching new row; a second identical effect in a listed function with the count
left unchanged; a deleted site with its row retained — each red for its named
reason.

### (j)2 · P2 — the excluded roots are Cargo targets, not file names

(i)5 item 1's conditional form does not activate in an ordinary example (measured:
`--all-targets` fails on an example's sleep with the root `cfg_attr(test, …)` in
place; the survey already lists example sites in `container-probe.rs`,
`gif-fixture.rs`, `video-probe.rs`), and development binaries had no form at all.

- **The universe of roots comes from `cargo metadata`'s `targets`** (kind,
  `src_path`, `harness`), per product package. `lib` and product `bin` targets
  are product roots; `test`, `example`, `bench` and `custom-build` targets are
  excluded roots (B§C-3); a `bin` target of a product crate that is not the
  product (`folio` is the only product binary today) is listed by name in the
  script as excluded, or it is product — no third state.
- **Two forms, by root kind.** Product roots: `#![cfg_attr(test,
  allow(clippy::disallowed_methods))]` (inline `#[cfg(test)] mod` bodies are
  covered by inheritance — measured). Excluded roots, at their `src_path` only:
  `#![allow(clippy::disallowed_methods)]` unconditionally (covers examples,
  custom-harness tests and benches, build scripts alike). Each form once per
  file, in the leading inner-attribute block; the guard matches the text and
  the position, and the script matches the file set against the target list.
- **Mounting is refused.** An excluded root's source file `include!`d or
  `#[path]`-mounted into a product target is inside the product universe (the
  guard follows modules from product roots), so its `#![allow]` is red there.
  Nested test support files under `tests/` that are not roots carry nothing.
- Controls: an ordinary example and a development binary with a bare sleep are
  green; the mounting mutation is red; the guard's placement rule is proven
  by a plant that still parses (the attribute after a `use`), not by a parse
  error.

### (j)3 · P3 — the configuration fence is a closed list of paths and contents, both basenames

Clippy reads `clippy.toml` **and** `.clippy.toml`, nearest first (measured: a
product package's `.clippy.toml` with an empty vocabulary passed a bare sleep);
and the root `clippy.toml` already exists with three test settings, so "root
plus three shields" is four files.

- **The closed list**, with pinned contents: the root `clippy.toml` (today's
  settings; A2e appends the vocabulary), `vendor/clippy.toml`,
  `crates/bt-corpus/clippy.toml`, `crates/bt-source/clippy.toml`, the three
  shields each exactly `disallowed-methods = []` under a comment naming this
  revision. The script hashes each and refuses a difference.
- **Any other `clippy.toml` or `.clippy.toml`** under the tree (`target/`
  excluded) is red — the hidden-basename plant in a product crate is the test.
- **`CLIPPY_CONF_DIR` is asserted unset** by a step in `logic` and
  `core-macos` and refused by the script in workflows, `.cargo/config.toml` and
  `scripts/**`.
- A2a's wording is "no root **vocabulary** before A2e", not "no root file".

### (j)4 · P4 — one root file, and resolution proven per entry per target by a positive control

(i)6 item 1's two checks are not enough: a present crate's off-target function
also prints the reachable-function warning (measured: a dependency loaded on
Windows whose function is `cfg(macos)`), so a blanket scan would reject a
legitimate off-target entry; and `cargo metadata` has no `std` package and
names `portable-pty`, not `portable_pty`, so the first-segment check fails the
existing vocabulary before any typo. The metadata check is withdrawn.

- **One root file** carrying every target's entries stays (no `CLIPPY_CONF_DIR`
  — it would defeat the shields).
- **The warning scan is target-aware**: each job parses clippy's
  reachable-function warnings; a warning on an entry the registry assigns to
  the job's target is fatal; a warning on an entry not assigned to it is
  expected; nothing else is exempt.
- **The positive control** (B§C-4's, kept): a development-only crate
  `crates/bt-lint-probe` (an excluded target, shielded by the (j)2 form on its
  root, outside the product universe) whose one file calls **every vocabulary
  entry assigned to the current target**, generated from the registry by the
  same PowerShell copier pattern (reads the registry, never `.rs`), each call
  under the entry's `cfg`; the job requires clippy to report the lint on each
  call, by entry. A misspelled path or a wrong target assignment fails to
  compile or fails to fire — red either way, in an actual linted unit with the
  target's dependencies. Every assigned target has a job that runs the probe.
- Plants: the absent crate; the misspelled function in a loaded crate; the
  present-crate/off-target function assigned to the wrong target; a correct
  off-target entry (green).

### (j)5 · P5, P9 — every effect gets a disposition, in revision (k), from A2a's inventory

(i) allocated the census by its coarse families and left the 29
"door-entrance" rows, the four `wgpu::Queue::submit` sites, `ShellExecuteW`
in `handoff.rs` and bt-pty's twelve writes unassigned; and its arithmetic was
off by one (58 writes, not 57; 228 includes six build-script sites and sites
already inside door bodies).

- **A2a's inventory is the baseline**: cfg-blind (both arms), the vocabulary
  corrected, product-only by (j)2's target universe. Its total is its own
  measurement; 228 and 171 describe the Windows diagnostic survey on `fcda5dfa`
  and nothing else.
- **Revision (k)** — written by the coordinator from that inventory and
  reviewed before any of A2b–A2e is briefed — gives every row one of five
  dispositions: (1) inside an effect function that will carry the `expect`
  (the `# effects` section of (j)7); (2) routed through a door A2b/A2c/A2d
  creates; (3) excluded by (j)2; (4) a typed-entrance call site, handled by
  (j)5's next bullet; (5) a retained destructor road ((j)6). No sixth.
- **Typed entrances leave the vocabulary when their signature is the door.**
  A cross-crate entrance that takes a `WaitToken` or `&WorkerCtx` (after A1b
  and A1d) cannot be called without a capability, so listing it in the
  vocabulary only makes its admission sites red; A2a removes each such entry
  from `# vocabulary` under a **checked removal**: the guard asserts that every
  entrance removed takes a capability parameter (the synchronous-door scan
  already reads these signatures). An entrance that does not yet take one
  stays listed and its call sites are bare rows to allocate.
- **Effect functions are lexical.** The `expect` goes on the function whose
  body contains the raw effect — bt-render's presentation helpers for
  `Queue::submit`/`Queue::present`, `handoff.rs`'s function around
  `ShellExecuteW`, bt-pty's `PtyDump::create_at`, `write_chunk`,
  `write_input_at`, `finish`, `write_resize` and the publisher — never on a
  caller or a wrapper, and a forwarding call never shares its callee's
  suppression. bt-pty's writes are transport effects like its waits: A2d owns
  every bt-pty row, with no bt-platform edge (§7 departure 1 stands).

### (j)6 · P6 — waits are classified by their executing caller; interim doors carry an effect-level table; no new deadlines

- "68 worker waits" was wrong: the 68 include bt-pty's ten (transport),
  `gpu_door::open_first_window`, `SessionWriter::wait_for`/`close`,
  `trace_sink::flush_sink` (four raw effects in a helper that holds neither
  context nor token — the owner token is consumed by `flush`), device recovery,
  the clipboard retry's sleep in `bt-platform/src/lib.rs`, and the pinned
  destructor chains. (k) classifies each wait by the thread that executes it —
  owner, worker, transport, or a retained destructor road — not by proximity
  to a spawn.
- **Retained destructor roads stay as they are.** The thirteen pinned `Drop`
  rows keep their raw effects in the functions A1e pins; A2e puts the `expect`
  on those lexical effect functions as rows of kind `drop-exception` in
  `# effects`, and no helper is introduced inside a pinned chain (its edges and
  counts are the debt's evidence, and moving them would launder it).
- **Names say what they do, and today's semantics are kept.** `wait::recv`,
  `wait::join`, `wait::sleep`, `wait::recv_timeout`, `wait::condvar_wait`,
  `wait::block_on` on `&WorkerCtx`: an unbounded receive stays unbounded; a
  deadline is introduced by no A2 ticket (that is a behaviour decision with
  its own ticket). (i)'s `_bounded` suffixes are withdrawn.
- **Each interim owner door has a row in the effect-level table** of (k):
  the exact function boundary, role, capability (token or context), phases,
  admission site, refusal behaviour, effect count, witness test, the owing
  B-ticket and version, the discovery date and the execution context as found.
  `open` is never rendered "ruled to stay"; deleting an interim door later
  leaves any residue registered.
- **Ownership is single.** Row 8 (`DirWatch::start_scoped` ×3) is A2c's.
  `profile_marks` and `psreadline` (rows 2–4: observation, writes and waits
  together) are A2c's wholly. bt-pty is A2d's wholly. The registry, the
  admission types and the guard are shared surfaces: A2b lands first among
  the three where they must change shape, and A2c/A2d rebase on it — (k) says
  which, per item, after the allocation, and re-sizes the three.
- **History corrected.** A1d converted the owner doors of B§R-A in place and
  deferred rows 2, 3, 4, 7, 8, 10 and 20 ((c)3, (e)2). A2 now wraps those
  deferred rows using A1d's pattern; (i)3's "the shape A1d gave rows 2–20" is
  withdrawn.

### (j)7 · P7 — the `# effects` section, and the `expect` equation

The registry's `# doors` section holds 24 logical admission identities (not 26;
(i)6 item 4 is withdrawn), some spanning several bodies (`CompositorBirth`:
`Compositor::new` and `spare_parent`; platform arms), and some owner doors hold
no listed effect at all (`owner_door.rs`'s five call winit methods outside the
vocabulary; `trace_sink::flush` forwards to `flush_sink`). An `expect` on a
function with no effect is itself red (`unfulfilled_lint_expectations`,
measured), and an `expect` never reaches a separately defined callee.

- **A2a adds `# effects`** to `window_waits.tsv`: one line per **effect
  function**, keyed by item identity and cfg arm, with its vocabulary entries
  and multiplicities, its kind (`owner-door-body`, `worker-door-body`,
  `transport`, `drop-exception`, `interim-owner`), its authority (`WaitToken`,
  `WorkerCtx`, `none (transport)`, `drop`), and the admission identity it
  serves if any. Admission identities (`# doors`) and FFI construct owners
  (`# owners`) stay distinct sections; neither is a list of `expect` holders,
  and no `# owners` module is exempt from the bare-site gate — the gate reads
  by function.
- **The equation.** The expected `expect` holders are exactly the `# effects`
  rows whose entries the current configuration compiles; zero-effect wrappers
  carry none. The guard checks each holder's identity, reason text (the
  effect-function id), placement (the function item) and effect count against
  its row; a moved `expect` with the total unchanged is red; a second listed
  effect under an unchanged row is red (B§C-2 item 3).
- **A2e re-runs M7a–M7d against a positive baseline**, and adds the
  worker/transport synchronous-door mutations.
- A1e's aggregate count assertion (0 today) becomes this per-row check in
  A2e; until then A2a keeps it at 0.

### (j)8 · P8 — `file_reads` option (d), at execution level

Adopted as (i)3 said, with the contract the review asked for:

- **Worker readers borrow the executing worker's context**: `Reader<'w>` holds
  `&'w WorkerCtx`, is `!Send`, and cannot be transferred or outlive the body;
  `open`/`new`/`read`/`read_to_end`/`seek`/`metadata`/`opaque` are all covered
  by that borrow (the lazy effects of `Read::read` and friends run under the
  same context that opened). This is the one, stated streaming exception to
  B§C-5's synchronous-door rule, and it is narrow: the capability is held for
  the reader's whole life, not checked at construction.
- **Owner observation doors complete inside the admission and return owned
  data**: `observe` returns `Metadata` or an owned `Vec` of entries, never a
  `ReadDir` or a tokenless reader; `read_dir`'s lazy iteration happens inside
  the door.
- **Contexts are inventoried by executing thread**, not by `Lane`: (k) lists
  every lane × thread pair that reads today, including the Attention standalone
  roads and any decoder or reader that moved to a spawned thread.

### (j)9 · What each landing truthfully claims, and the order

- **A2a** leaves: the frozen, exact bare-site inventory (with its own total),
  the `# effects` schema (empty rows are allowed only where (k) will fill
  them — the section exists with its header and the guard reads it), the
  corrected vocabulary with the checked removals of typed entrances, the
  configuration fence, the target-aware warning scan and the lint probe crate
  with its jobs, the non-Rust checks (M7e, M9, `CLIPPY_CONF_DIR`), and the
  (j)2 exclusion forms with their guard — **not** the C-1 invariant, which
  ARCHITECTURE §6 keeps stating as pending with the count.
- **Revision (k)** (coordinator; reviewed): the allocation table over A2a's
  inventory, the effect-level table for every interim door, B10's brief, the
  per-item ownership among A2b/A2c/A2d and their re-sized briefs, the
  `file_reads` context inventory.
- **A2b, A2c, A2d** establish the listed authority and effect boundaries and
  remove exactly the rows (k) assigned to them; they keep A1e's `Drop` and FFI
  checks untouched.
- **A2e** establishes C-1 on both product jobs only after: the observed
  multiset is empty, the configuration fence and the probe pass on both
  targets, and every `# effects` row's `expect` is checked by owner. The
  Windows-only interim of B§C-4 is available only if A2e declares it.

### (j)10 · This revision's own architecture impact

(a) none. (b) none by this commit. (c) none by this commit; A2a opens no row,
(k) opens B10 and the transport debt row when its tickets are briefed. (c′)
none. (d) yes, as (i)8 said, with (i)'s three amendments to B§C-2/C-4 now in
the forms of (j)2, (j)3 and (j)4; the metadata check and the `_bounded` names
of (i) are withdrawn.

### (j)11 · After the scoped Codex review of (j): eight corrections, all carried into A2a

Review: `trace/tickets-046/thread-door-review-codex-2026-09-26-j.md` (adopt
with changes for A2a's dispatch; A2a stays independent of A2b–A2e). Each
finding is adopted as follows; the A2a brief is the binding text.

1. **The mint boundary is fenced before any entrance leaves the vocabulary.**
   A safe sibling of `admitted` inside `admission` (`work(WaitToken::fresh())`
   with no role, phase or meter) would pass every check A1e has, and a helper
   taking `lend_worker` as a function value would pass the caller counter. A2a
   adds to the guard: `WaitToken::fresh` is referenced (as a call **or** a
   value) exactly once, inside `admitted`; `lend_worker` is referenced exactly
   by its two owners; every reference to either mint anywhere in the workspace
   is counted by the identifier view, not by `name(`; the constructors' visibility
   is pinned; and for each entrance removed from the vocabulary the required
   parameter is checked **per cfg arm** as the exact capability type
   (`WaitToken<'_, D>` for the door's `D`, or `&WorkerCtx`), never
   `Option<…>`. Mutations: the safe alternate mint; the indirect mint through a
   function value; the removed parameter; `Option<WaitToken>`. (b)2's
   qualification (inference-typed unsafe fabrication in bt-platform is outside
   the accident-level guarantee) stands and is restated beside the removal.
2. **Private functions are not vocabulary entries.** `DirWatch::start_scoped`
   and `read_system_locale_declaration` are private to bt-platform; no
   external probe can call them, and making them public for a probe would widen
   the product interface. The vocabulary lists library paths and cross-crate
   entrances callable from outside; these two leave it **as entries**, and the
   raw effects inside them (already vocabulary: the spawn and joins, the
   `Command` wait) are inventory rows that (k) allocates to A2c with rows 7 and
   8. Nothing is deleted silently: the report lists both with their effects.
   The external probe then covers every retained entry, with direct
   dependencies on `bt-render`, `bt-pty`, `bt-platform`, `wgpu`,
   `portable-pty`, `pollster`, `windows` (the binding features the recipes
   use) under `cfg(windows)`, `libc` under `cfg(target_os = "macos")`; its
   own `[lints.rust] unsafe_code = "allow"` (a tool crate; the FFI recipes are
   `unsafe` blocks); public typed functions that are compiled and never run.
   Recipes (receiver, arguments, generics) live in a non-`.rs` template keyed
   by entry, beside the registry.
3. **The positive control runs in A2a, in a dedicated invocation.** The probe
   crate has its own `crates/bt-lint-probe/clippy.toml` — generated from the
   registry, equality-checked, and the fifth member of (j)3's closed list — so
   the vocabulary is effective for the probe alone and never for a product
   crate; the step is `cargo clippy -p bt-lint-probe --all-targets --
   --force-warn clippy::disallowed_methods` (measured: `--force-warn` fires
   through the root allowance), whose diagnostics the script matches by source
   span to the entry each generated line names, requiring one diagnostic per
   entry assigned to the job's target, refusing a compile error, and applying
   (j)4's target-aware rule to reachable-function warnings. Mutations: an entry
   missing from the probe file; a misspelled path; a wrong target assignment
   (red on the other job); a correct off-target entry (green). The ordinary
   product invocation stays separate and unchanged.
4. **The fence covers Cargo's legacy configuration road.** `.cargo/config`
   (no extension) anywhere in the tree is red outright; `[env]` entries
   naming `CLIPPY_CONF_DIR` in `.cargo/config.toml` are red (M7e extended);
   the shell assertion stays as a control. Plant: the `[env]` road with the
   shell variable unset, red for the named reason.
5. **Bootstrap without an exit-0 exception.** Two commits: S1 seeds the
   inventory at the ticket's BASE (the equality half only; the coordinator
   reviews the seed against A2's Windows survey for plausibility before S2);
   S2 lands the historical half and pins S1's sha as the **seed baseline**:
   the baseline is the merge base's committed file when it has one, else the
   pinned seed blob (`git show <S1>:<path>`) — so a branch whose merge base
   predates the seed compares against the seed without rebasing, and a dirty
   plant at S1's own HEAD is an addition against the seed blob and red. No
   "HEAD == seed → pass". `core-macos` fetches full history (it is shallow
   today). Plants: a new call plus row at seed HEAD (dirty); a descendant
   adding call plus row; a branch whose merge base lacks the file (compares
   against the seed).
6. **Counts are compared numerically.** Row key = `(crate, cfg arm, item,
   entry)`; `count` is a positive integer; duplicate keys refused;
   `current[key] <= baseline[key]` with a missing key read as zero; a new key
   refused. Plants: `3 → 2` (green), `2 → 3` with the extra site (red).
7. **Target kinds without `harness`; reachability wins; parsing plants.**
   `cargo metadata` gives `kind` and `src_path` and no `harness` field; the
   two-form rule needs only the kind (every `test`/`example`/`bench`/
   `custom-build` root takes the unconditional form). Every target is
   enumerated, `required-features` or not. A file reachable from a product
   root is product whatever its label; a `src_path` shared by a product and an
   excluded target is refused. `bt-source`'s manifest reader gains
   examples/benches/build scripts as target kinds, and `bt-lint-probe` is
   registered as an excluded tool package. The placement plant of (j)2 ("the
   attribute after a `use`") does not parse and is withdrawn; the plants that
   parse and must be refused are a `#[path]`-mounted module carrying a leading
   inner allowance (measured: it inherits and silences), a leading inner
   allowance in a nested module, and an outer allowance on a function.
8. **The copier and the tripwire.** The generator reads the registry TSV and
   a non-`.rs` template and takes its output path as a parameter (a
   double-quoted `.rs` literal beside a read call would trip shape 2 even for
   a writer — measured); it is run against the unchanged tripwire in the
   ticket; equality is `git diff --exit-code -- <generated paths>` in a clean
   checkout on both jobs, with a tracked-file check; `cfg` predicates come from
   the entry's target assignment so the wrong-target mutation really moves the
   call.

Also: M9 refuses only a **newly added** registry line without a ruling — the
baseline `pending` rows (16b, 23) are a shrink-only set like the inventory; and
`flush_sink` is inventory, not in-door by being called from `flush`.

**Dispatch.** With these in the brief, A2a is dispatched on this revision;
(k) is reviewed against A2a's landed commit.

### (j)12 · How the inventory recognises a site (A2a, 2026-09-26; coordinator's instruction on accepting the seed)

(i)2 and (j)1 said the inventory reads the vocabulary "through the identifier
view" and left open how a method entry is found without types: the tree holds
about 1,600 `.join(` calls, of which about twenty are thread joins, so a name
match would freeze some 1,500 rows no door could ever remove. A2a's reader
(`hang_watch::window_waits_tests`, `every_bare_site_is_a_row_and_every_row_a_site`)
recognises a site by **what the source writes**, and only that:

- **A path entry** (`std::thread::sleep`, `pollster::block_on`, `CloseHandle`)
  is a site wherever a path is written in code — called, or handed on as a
  value (`.answer(&mut machine, std::thread::sleep)`) — that resolves to the
  entry through the `use` declarations in scope, a function's own `use`s
  included: both spellings (the full path, and a name a `use` brought in),
  `crate::`, `self::`, `super::`, `Self::`, and a glob. A first-party item is
  named by its crate and itself, so a `pub use` elsewhere changes nothing.
- **A method entry** (`std::thread::JoinHandle::join`, `wgpu::Queue::submit`)
  is a site at `receiver.name(…)` only when the receiver's **written type**
  includes the entry's type: the type written on `self`, on a parameter, a
  field, a `static`, a `let` annotation, or on the declared return of the
  first-party function or method the receiver was made by; carried through
  `let`, `if let`, `while let`, `for`, `match` arms and the parameters of a
  closure handed to a method of the value. The few standard-library
  signatures a vocabulary receiver is made through are written out in the
  reader (`mpsc::channel`, `thread::spawn`, the `Command` and `OpenOptions`
  builders, `Command::spawn`, `OpenOptions::open`, the wrappers and the
  methods that pass a value's type on, such as `Option::take`, `Mutex::lock`
  and `Result::unwrap`). `write!`/`writeln!` are `std::io::Write::write_fmt`
  when their first argument is an `io` writer by the same reading.

**Its limit, stated so the count is not read as exhaustive before A2e.** A
call at a receiver whose type is written nowhere on that road — a local bound
from an untyped foreign call, a closure parameter of a foreign function — is
not seen. The inventory is therefore a reading of the source, not the
compiler's resolution: exact for what it sees, silent for what the source does
not type. Against A2's clippy census on `fcda5dfa` (Windows arms) it found
every census site outside a build script and outside a door body with its
count, and nothing on those arms the census lacked except the two
`wgpu::Queue::present` calls (the census used the wrong spelling) and one
`cfg(not(windows))` statement. What it cannot see, A2e's lint sees: A2e may
find sites the inventory never listed, and each is routed through its door
then, not added to the file.

### (j)13 · A2c's first door, brought forward by U-28

The coordinator's ruling of 2026-09-27 on U-28's stop: the macOS applier is a
standalone process whose main thread is a worker, and it must poll twice (the
old build's data-directory claim, and the trial's receipt); no door admitted a
worker's sleep, and adding bare sites is refused. So the smallest worker door
comes first: `bt_platform::wait::sleep_within(&WorkerCtx, Duration)`, whose
body is one effect (`std::thread::sleep`) and nothing else — the module name
A2c will extend. It is registered in `window_waits.tsv`'s `# effects` section
as kind `worker-door-body`, authority `WorkerCtx`, **with no admission
identity** (the door column empty: the `# doors` identities are the window
thread's). The guard changes by one rule: a `worker-door-body` row may, and
must, have an empty door column, and a function it lists whose parameters take
a `WorkerCtx` is a door body, so its sites are door effects rather than
inventory (`hang_watch::window_waits_tests::worker_door_functions`); an
`owner-door-body` row still needs its door. The bare-site inventory did not
grow: both of the applier's polls sleep only through this door. For (k): every
existing worker sleep in the inventory (`update_trial::watch`,
`take_the_claim_within`, `install_txn::hold_until`,
`macos_update::{run_at, detach_point}`) is a candidate to route through it.

---

## Revision 2026-09-27 (k), the allocation: every bare site gets one disposition, and A2b–A2d are re-cut from it

**Classified against** `b054379b`, and **re-read against `c50adb60`** (`origin/main` when this revision was
committed, with U-28 merged). The inventory `docs/plans/window-thread-bare-sites.tsv` is the same file at both:
**226 rows holding 248 sites**. This is the regeneration at `11b9da8a` after U-27, and the gate's seed moved
with it at `bf652273`. The registry `crates/bt-app/src/window_waits.tsv` has 23 `# rows`, 24 `# doors`,
9 `# entrances` and, since U-28, 8 `# effects`. The eighth is `crate::wait::sleep_within`, of kind
`worker-door-body`, with authority `WorkerCtx` and an empty door column ((j)13).

U-28 changed the thread of one row: `install_txn::hold_until`'s sleep (row 108) is now reached from the macOS
applier's standalone main. It also added two `file_reads` sites, which (k)8 lists. It added no bare site.

(j)13's list of "worker sleeps" is corrected here: `update_trial::take_the_claim_within` (row 89) runs on the
window thread before the loop, not on a worker.

**How each row was read.** Every row was traced from its item to the thread that runs it today: callers
recursively, down to a spawn closure, a `Runtime` method or `FolioApp` handler, `fn main`'s road, a door verb,
or a test. Nothing was built or run. Where a row says "owner (Starting)", the site runs on the window thread
after `admission::enter_window_thread` and before the loop exists. "Owner (Exiting)" means after
`admission::exiting`. "Standalone" means a door process's main thread. "None" means no product caller exists,
and the row says which tests, examples or tools reach it. The table cites items by identity. Call chains are
named by function, never by line.

### (k)1 · The dispositions as applied, and the one interface they force

**The five kinds, read against the table.**

- **(1): the site stays where it is, in a function that becomes an `# effects` holder under an identity that
  already exists.** That is:
  - the helpers inside an existing owner door (`PresentFrame`'s `compose_frame` and `handle_surface_failure`,
    `TraceFlush`'s `flush_sink`);
  - the hand-off lane's door body;
  - bt-pty's transport functions.
- **(2): the site goes through a door that A2b, A2c or A2d creates.** A worker door takes the worker's
  context. An owner road takes an admitted token; the token is of an **interim owner door**, a new admission
  identity and registry line with the `open` status (i)3 ruled.
- **(3): no row.** Every excluded root was already outside the inventory (A2a's (j)2 forms), so no row of the
  inventory is excluded.
- **(4): no row.** A2a's checked removal took all nine typed entrances out of the vocabulary, so no row is a
  typed-entrance call site. What remains of (4) is the `# entrances` section, which is the registry's, not
  the inventory's.
- **(5): the functions A1e pins** (`EXCEPTIONS`/`PINNED` in `window_waits_tests.rs`). Three `Drop`s that only
  close a handle are added to that table as counted-only rows; see (k)2 item 5.

No sixth kind is added. The five rows the kinds cannot hold are listed in (k)4 as unclassified. They are not
forced into a kind.

**The interface: `admission::Authority`.** Of the 172 rows in (2), 90 run on the window thread, and 34 of those run in a function a worker or a standalone main also
executes (the table's thread column says "owner + …"). Examples:

- `bt_persist::atomic`'s rename and `sync_all`, reached from the settings store on the owner and from the
  session writer on a worker;
- `file_replace`;
- `profile_marks::lock`;
- `explorer_menu::same_path`;
- `migrate::read_bounded`.

An `expect` never reaches a separately defined callee ((j)7). So the effect function must be the one function
both roads call, and it must accept both capabilities. **Every family door therefore takes `by: &impl
admission::Authority`.** `Authority` is sealed, with no implementation outside `admission`, and it has exactly two
implementors:

- `WorkerCtx`;
- `WaitToken<'_, D>` for every door `D`.

A worker passes its context. The window thread passes the token of the interim door whose road it is on.
Without either, the call does not compile, so nothing (b)2 and §C-5 guarantee is lost.

**What the guard gains.** A `# effects` row of kind `worker-door-body` gains the authority `Authority` and a
`door` cell listing the admission identities whose mint sites reach it. The guard checks that list against the
`admitted::<D>` calls whose work reaches the door, the way `every_door_is_where_the_registry_says` already
reads mint sites. This amends (j)6's "`wait::*` on `&WorkerCtx`". U-28's `wait::sleep_within(&WorkerCtx,
Duration)` widens to `&impl Authority` without changing a call site.

**Standalone mains.** The attention verb's stdin reader and the two Explorer waits enter
`enter_standalone_main`. The rest of their processes' disk work runs with no role. That covers:

- `--uninstall-cleanup`'s body (it enters only inside `cleanup_waited_on`);
- `--remove-shell-integration`;
- `--explorer-command` (`explorer_menu::serve` reads settings);
- `--update-recover`;
- the command-line refusal in `fn main` before `enter_window_thread` (`report_at_the_front_door` →
  `say_at_the_front_door` → `SettingsStore::open`).

Each verb enters **once, at its top**. The context is lent down to the removal, cleanup and profile roads.
`cleanup_waited_on` and `removal_waited_on` stop entering and take the context. The guard's pinned caller list
for `enter_standalone_main` grows from three to the verbs themselves. This is A2b's (k)7, because A2b's doors
are the first that need it.

**Counts.**

| by disposition | rows | sites |
|---|---:|---:|
| (1) in an effect function under an existing identity | 19 | 23 |
| (2) through a door A2b/A2c/A2d creates | 172 | 188 |
| (3) excluded by (j)2 | 0 | 0 |
| (4) typed-entrance call site | 0 | 0 |
| (5) retained destructor road | 30 | 32 |
| could not classify ((k)4) | 5 | 5 |
| **total** | **226** | **248** |

| (2) by door family | rows | sites |
|---|---:|---:|
| `file_reads::observe` | 57 | 62 |
| `file_writes::*` | 41 | 43 |
| `wait::*` (`sleep_within`, `join`, `recv`, `recv_timeout`, `condvar_wait`, `condvar_wait_timeout`, `block_on`) | 51 | 56 |
| `handles::*` (`close_handle`, `set_event`; `close_fd`, `write_fd` on Unix) | 16 | 20 |
| `quiet_command::*` (`output_within`, `wait_within`) | 7 | 7 |
| of which name an owner identity in the door cell (8 more run on the owner and take the tokens listed on row 97) | 82 | 93 |

| by ticket | rows | sites |
|---|---:|---:|
| A2b | 83 | 90 |
| A2c | 120 | 130 |
| A2d | 18 | 23 |
| A2e only | 0 | 0 |
| none (unclassified) | 5 | 5 |

| by the thread that runs it today | rows |
|---|---:|
| workers only | 66 |
| the window thread only (turn, `Starting`, `Exiting`, or inside an existing door) | 68 |
| a standalone main only | 8 |
| transport only (bt-pty's threads, pinned drops included) | 10 |
| a `Drop` on the window thread only | 3 |
| several of these | 49 |
| no product caller (statics never dropped, tests, examples, tools) | 22 |

**A2e only is empty.** Each (1) row needs a signature or kind change before an `expect` can land: a helper
taking `&WaitToken`, a `transport` or `drop-exception` kind, or `Authority` for the hand-off body. A2a's guard
refuses those until the ticket that owns the row adds them.

### (k)2 · A2a's six items, each given its disposition

1. **The macOS `WebHost::request_environment` `create_dir_all`** stays **(1)**, `owner-door-body` of
   `WebEnvironment`, as it already is in `# effects`.
   - It runs inside the admitted door, on the window thread, and the door's line says so.
   - Routing it through `file_writes` inside the door would give one admission two authorities for one
     effect.
   - The residue belongs to row 21. When a later ticket moves the environment's folder creation, it moves with
     it. It is not A2b's.
2. **`Path::exists` / `is_file` / `is_dir` / `try_exists` / `symlink_metadata`** are observations the
   vocabulary does not list. There are 205 such calls in product sources (a text count, tests included by
   file name only).
   - This is a vocabulary question, not a disposition. It is not answered by adding rows: the inventory never
     grows.
   - **Proposal:** A2e adds `std::path::Path::{exists, is_file, is_dir, try_exists, symlink_metadata}` and
     `std::fs::symlink_metadata` to the vocabulary **only after** A2b's `file_reads::observe` exists. They are
     routed in A2e's own diff through `observe`, with the probe proving each per target.
   - Until then (j)12's limit applies, and they are named as outside the invariant by C-1's first bullet.
   - `read_system_locale_declaration`'s `Path::exists` rides row 7's `LocaleProbe` door when it lands.
   - **Owner decision** (k)10 Q4, because it roughly doubles A2e.
3. **bt-pty's `SystemShellEnvironment::is_file`** (row 215) is **(1)** `transport` under A2d.
   - The inventory's arm is item-level (`–`), and the counted `metadata` is a `cfg(unix)` statement. So on
     Windows the site does not exist, and `path.is_file()` does the same stat unlisted (item 2).
   - A2d's `# effects` row names the arm the statement stands on (`[unix]`) and not the item's. The guard
     reads statement-level `cfg` for `# effects` rows only, not for the inventory, which stays as seeded.
4. **`hang_watch::run_selftest_if_due`** (`[debug_assertions]`, row 92) is **(2)** through an interim
   `SelfTest` door.
   - It is product by (j)2's rules: `debug_assertions` is not `test`.
   - It is a deliberate hold on the window thread, so the honest row is "ruled to stay (debug builds only)".
     That is proposed to the owner, (k)10 Q2.
5. **Counted handles on `Drop`s outside the exception table** are **(5)**: `attention_pipe::Overlapped`,
   `attention_pipe::OwnedHandle` and `instance::DataDirectoryClaim` (rows 161, 162, 173).
   - A `Drop` has no capability, so no door can take them.
   - A2c adds them to the pinned table as **counted-only** rows: a body with a `CloseHandle` ×1, no edges, and
     product reach as found. (g)4's reading ("refused inside a `Drop`" means the entries that wait) stays.
   - They are rows because the lint will fire on them, not because they wait. None of them gets a repayment
     ticket: a handle closed in its owner's destructor is the design.
6. **`write!`/`writeln!` to stderr** is **(2)** through `file_writes::write_line`, the same door as the
   trace file's writes. This covers `trace_sink::write_here` (row 83), and in part `trace::TraceFile::open`,
   whose second write is its error line (row 78).
   - After `diagnostics::enter_resident_run`, stderr *is* the run log (the survey read
     `choose_resident_channel`). Before it, stderr is a console or a pipe that can block on a stalled reader.
     Either way it is an I/O write on the calling thread.
   - The Unix `write_std_error` (`libc::write`, row 127) goes through `handles::write_fd` for the same reason.
   - `append_panic_report` (row 5) is not like these. It runs on whichever thread panics; see (k)4.

### (k)3 · The allocation table

Columns: the inventory's key; the disposition; the door the site goes through, and for an owner road `·` the
admission identity whose token it takes; the thread that runs it today; the ticket that removes the row; and the
B-ticket the interim door is owed to (for (5), the ledger row). "(widened)" and "(proposed)" mark owings that
need an owner ruling ((k)10). "As 97" means the owner tokens listed on row 97.

| # | crate | arm | item | entry ×count | disp | door · identity | thread today | ticket | owed to / note |
|---:|---|---|---|---|---|---|---|---|---|
| 1 | bt-app | – | `crate::<TheDeviceAndItsWindows as LostDevice>::rebuild` | `pollster::block_on` ×1 | (2) | `wait::block_on` · interim `DeviceRecovery` (row 10) | owner | A2c | B9 |
| 2 | bt-app | – | `crate::App::release_trial_writes` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · interim `TrialWritesReleased` (new row) | owner | A2b | B8 (widened) |
| 3 | bt-app | – | `crate::FolioApp::recovered_from_a_lost_device` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` (the path handed to the pilot) · interim `DeviceRecovery` (row 10) | owner | A2c | B9 |
| 4 | bt-app | – | `crate::animation::AnimationStamp::of` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 5 | bt-app | – | `crate::append_panic_report` | `std::io::Write::write_fmt` ×1 | ? | could not classify: the panic hook road | any (panic hook) + owner | — |  |
| 6 | bt-app | – | `crate::attention_hooks::Config::land` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · interim `AgentHooksWrite` (new row) | owner + standalone | A2b | B8 (widened) |
| 7 | bt-app | – | `crate::attention_hooks::Config::land` | `std::fs::write` ×1 | (2) | `file_writes::write` · interim `AgentHooksWrite` | owner + standalone | A2b | B8 (widened) |
| 8 | bt-app | – | `crate::attention_hooks::editable_target` | `std::fs::canonicalize` ×2 | (2) | `file_reads::observe` · interim `AgentConfigState` (new row) | owner + standalone | A2b | B10 |
| 9 | bt-app | – | `crate::attention_ownership::other_live` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · interim `AgentHooksWrite` | owner + standalone | A2b | B8 (widened) |
| 10 | bt-app | – | `crate::attention_wire::payload_on_stdin` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` | standalone | A2c | `attention`; the context is already in scope |
| 11 | bt-app | – | `crate::clipboard_picture::names_in` | `std::fs::read_dir` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 12 | bt-app | – | `crate::clipboard_picture::save` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` | worker | A2b |  |
| 13 | bt-app | – | `crate::diagnostics::last_written` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · interim `RunLog` (new row, Starting) | owner (Starting) | A2b | candidate "stays" |
| 14 | bt-app | – | `crate::diagnostics::newest_crash_report` | `std::fs::read_dir` ×1 | (2) | `file_reads::observe` · interim `RunLog` | owner (Starting) | A2b | macOS in practice; candidate "stays" |
| 15 | bt-app | – | `crate::diagnostics::open_run_log` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · interim `RunLog` | owner (Starting) | A2b | candidate "stays" |
| 16 | bt-app | – | `crate::diagnostics::rotate_if_oversized` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · interim `RunLog` | owner (Starting) | A2b | candidate "stays" |
| 17 | bt-app | – | `crate::diagnostics::rotate_if_oversized` | `std::fs::rename` ×1 | (2) | `file_writes::rename` · interim `RunLog` | owner (Starting) | A2b | candidate "stays" |
| 18 | bt-app | – | `crate::explorer_menu::cleanup_waited_on` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` | standalone | A2c | `--uninstall-cleanup`; the context is in scope |
| 19 | bt-app | – | `crate::explorer_menu::removal_waited_on` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` | standalone | A2c | `--remove-explorer-menu`; the context is in scope |
| 20 | bt-app | – | `crate::explorer_menu::same_path` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` · interim `AgentConfigState` / `AgentHooksWrite` | owner + worker + standalone | A2b | B10 / B8 (widened) |
| 21 | bt-app | – | `crate::facts_of_a_file_the_user_chose` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · `PeekFacts` (new row) | owner | A2b | ruled in DESIGN already; proposed `ruled to stay` |
| 22 | bt-app | – | `crate::files::canonical_path` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 23 | bt-app | – | `crate::files::read_directory` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 24 | bt-app | – | `crate::files::read_directory` | `std::fs::read_dir` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 25 | bt-app | – | `crate::files::run_dir_worker` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` | worker | A2c |  |
| 26 | bt-app | – | `crate::git::run_git_with_input` | `std::process::Child::wait` ×1 | (2) | `quiet_command::wait_within` | worker | A2c |  |
| 27 | bt-app | – | `crate::git::run_git_with_input` | `std::thread::JoinHandle::join` ×2 | (2) | `wait::join` | worker | A2c |  |
| 28 | bt-app | – | `crate::git::run_git_with_input` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` | worker | A2c |  |
| 29 | bt-app | – | `crate::git::run_git_worker` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` | worker | A2c |  |
| 30 | bt-app | – | `crate::handoff_lane::run_handoff_lane` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` | worker | A2c |  |
| 31 | bt-app | – | `crate::hang_watch::prune_reports` | `std::fs::read_dir` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 32 | bt-app | – | `crate::hang_watch::watch_forever` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` | worker | A2c |  |
| 33 | bt-app | – | `crate::hang_watch::write_report` | `std::fs::File::create` ×1 | (2) | `file_writes::create` | worker | A2b |  |
| 34 | bt-app | – | `crate::hang_watch::write_report` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` | worker | A2b |  |
| 35 | bt-app | – | `crate::page_destination` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` · interim `PathResolve` (new row) | owner | A2b | B10 |
| 36 | bt-app | – | `crate::palette_index::IndexWorker::spawn` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` | worker | A2c | the site is in the spawn closure |
| 37 | bt-app | – | `crate::palette_index::canonical` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 38 | bt-app | – | `crate::palette_index::walk_bounded` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 39 | bt-app | – | `crate::palette_index::walk_bounded` | `std::fs::read_dir` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 40 | bt-app | – | `crate::persist::SessionWriter::start` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` | worker | A2c | the site is in the spawn closure |
| 41 | bt-app | – | `crate::persist::make_data_folder` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · interim `StoreOpen` (new row) / `LaunchHandOver` | owner + owner (Starting) + standalone | A2b | B10 |
| 42 | bt-app | – | `crate::persist::relocate` | `std::fs::rename` ×1 | (2) | `file_writes::rename` · interim `StorageRelocate` (new row, Starting) | owner (Starting) + standalone | A2b | candidate "stays" |
| 43 | bt-app | – | `crate::preview::file_mtime` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · `PreviewSave` (row 20) | owner | A2b | row 20, document half |
| 44 | bt-app | – | `crate::preview::read_size` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 45 | bt-app | – | `crate::preview::run_preview_worker` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` | worker | A2c |  |
| 46 | bt-app | – | `crate::preview_watch::Stamp::of` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · interim `PreviewStat` (new row) | owner | A2b | B10 |
| 47 | bt-app | – | `crate::psreadline::install_checked` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · `PsReadLineApply` (row 3) | owner | A2c | B4 |
| 48 | bt-app | – | `crate::psreadline::install_checked` | `std::fs::write` ×1 | (2) | `file_writes::write` · `PsReadLineApply` (row 3) | owner | A2c | B4 |
| 49 | bt-app | – | `crate::psreadline::installed_disk::<System as Disk>::entries` | `std::fs::read_dir` ×1 | (2) | `file_reads::observe` · `PsReadLineProbe` (row 4) | owner + standalone | A2c | B5 |
| 50 | bt-app | – | `crate::raster_peek_page` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 51 | bt-app | – | `crate::read_video_glance` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 52 | bt-app | – | `crate::revived_page_of` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` · interim `PathResolve` | owner | A2b | B10 |
| 53 | bt-app | – | `crate::run_decoration_worker` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` | worker | A2c |  |
| 54 | bt-app | – | `crate::run_path_verify_worker` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` | worker | A2c |  |
| 55 | bt-app | – | `crate::run_scale_worker` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` | worker | A2c |  |
| 56 | bt-app | – | `crate::runtime::files::Runtime::rename_files_row` | `std::fs::rename` ×1 | (2) | `file_writes::rename` · `RenameDisk` (row 20) | owner | A2b | row 20, document half |
| 57 | bt-app | – | `crate::runtime::preview::Runtime::open_preview_web_file_on` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` · interim `PathResolve` | owner | A2b | B10 |
| 58 | bt-app | – | `crate::runtime::preview::Runtime::rename_preview_file` | `std::fs::rename` ×1 | (2) | `file_writes::rename` · `RenameDisk` (row 20) | owner | A2b | row 20, document half |
| 59 | bt-app | – | `crate::runtime::profiles::Runtime::toggle_root_menu` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · interim `RecentFolders` (new row) | owner | A2b | B10 |
| 60 | bt-app | – | `crate::schemes::read_scheme_file` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · interim `SchemeCatalogue` (new row) | owner | A2b | B10 |
| 61 | bt-app | – | `crate::schemes::user_dir` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · interim `SchemeWrite` (new row) | owner | A2b | B8 (widened) |
| 62 | bt-app | – | `crate::schemes::user_sources` | `std::fs::read_dir` ×1 | (2) | `file_reads::observe` · interim `SchemeCatalogue` | owner | A2b | B10 |
| 63 | bt-app | – | `crate::schemes::write_custom_copy` | `std::fs::write` ×1 | (2) | `file_writes::write` · interim `SchemeWrite` | owner | A2b | B8 (widened) |
| 64 | bt-app | – | `crate::shell_integration::cached_profile_answer` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` | worker + standalone | A2c | the profile-probe family |
| 65 | bt-app | – | `crate::shell_integration::install_script_at` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · interim `ShellScriptsInstall` (new row) / `MarksInstall` (row 2) | owner + worker | A2c | B8 (widened) / B4 |
| 66 | bt-app | – | `crate::shell_integration::install_script_at` | `std::fs::write` ×1 | (2) | `file_writes::write` · interim `ShellScriptsInstall` / `MarksInstall` | owner + worker | A2c | B8 (widened) / B4 |
| 67 | bt-app | – | `crate::shell_integration::install_zdotdir` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · interim `ShellScriptsInstall` | owner | A2c | B8 (widened) |
| 68 | bt-app | – | `crate::shell_integration::install_zdotdir` | `std::fs::write` ×1 | (2) | `file_writes::write` · interim `ShellScriptsInstall` | owner | A2c | B8 (widened) |
| 69 | bt-app | – | `crate::shell_integration::profile_marks::Marks::write` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · `MarksInstall` (row 2) / `PsReadLineApply` (row 3) / `AgentHooksWrite` | owner + worker + standalone | A2c | B4 |
| 70 | bt-app | – | `crate::shell_integration::profile_marks::OurTurn::take` | `std::sync::Condvar::wait` ×1 | (2) | `wait::condvar_wait` · `MarksInstall` / `PsReadLineApply` / `AgentHooksWrite` | owner + worker | A2c | B4 |
| 71 | bt-app | – | `crate::shell_integration::profile_marks::lock` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` · `MarksInstall` / `PsReadLineApply` / `AgentHooksWrite` | owner + worker + standalone | A2c | B4 |
| 72 | bt-app | – | `crate::shell_integration::profile_marks::lock` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · `MarksInstall` / `PsReadLineApply` / `AgentHooksWrite` | owner + worker + standalone | A2c | B4 |
| 73 | bt-app | – | `crate::shell_integration::profile_marks::lock` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` · `MarksInstall` / `PsReadLineApply` / `AgentHooksWrite` | owner + worker | A2c | B4 |
| 74 | bt-app | – | `crate::shell_integration::replace_profile` | `std::fs::File::sync_all` ×1 | (2) | `file_writes::sync_all` · `MarksInstall` | owner + worker + standalone | A2c | B4 |
| 75 | bt-app | – | `crate::shell_integration::replace_profile` | `std::fs::create_dir_all` ×1 | (2) | `file_writes::create_dir_all` · `MarksInstall` | owner + worker + standalone | A2c | B4 |
| 76 | bt-app | – | `crate::taskbar_lane::TaskbarLane::serve` | `std::sync::Condvar::wait` ×1 | (2) | `wait::condvar_wait` | worker | A2c |  |
| 77 | bt-app | – | `crate::trace::TraceFile::append` | `std::io::Write::write_fmt` ×1 | (2) | `file_writes::write_line` · `DiagnosticWrite` (row 20) | worker + owner | A2b | B8 (diagnostic writes join the trace queue) |
| 78 | bt-app | – | `crate::trace::TraceFile::open` | `std::io::Write::write_fmt` ×2 | (2) | `file_writes::write_line` · `DiagnosticWrite` (row 20) | worker + owner | A2b | B8; one of the two writes is to stderr |
| 79 | bt-app | – | `crate::trace_sink::flush_sink` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (1) | in the door `TraceFlush` (row 17): `flush_sink` takes `&WaitToken` | owner (Exiting) | A2c | also a pinned body of `trace_sink::Shutdown` |
| 80 | bt-app | – | `crate::trace_sink::flush_sink` | `std::thread::JoinHandle::join` ×1 | (1) | in the door `TraceFlush` (row 17) | owner (Exiting) | A2c | as 79 |
| 81 | bt-app | – | `crate::trace_sink::flush_sink` | `std::thread::sleep` ×2 | (1) | in the door `TraceFlush` (row 17) | owner (Exiting) | A2c | as 79 |
| 82 | bt-app | – | `crate::trace_sink::run_to` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` | worker | A2c |  |
| 83 | bt-app | – | `crate::trace_sink::write_here` | `std::io::Write::write_fmt` ×1 | (2) | `file_writes::write_line` · `DiagnosticWrite` (row 20) | owner + owner (Exiting) + any thread without a sink | A2b | stderr, which is the run log after `enter_resident_run` |
| 84 | bt-app | – | `crate::uninstall::detach_images_under` | `std::thread::JoinHandle::join` ×1 | (2) | `wait::join` | standalone | A2c | `--uninstall-cleanup` |
| 85 | bt-app | – | `crate::uninstall::inspect_tree` | `std::fs::read_dir` ×1 | (2) | `file_reads::observe` | standalone | A2b | `--uninstall-cleanup` |
| 86 | bt-app | – | `crate::uninstall::usable_recorded_directory` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | standalone | A2b | `--uninstall-cleanup` |
| 87 | bt-app | – | `crate::update::Claim::take` | `std::fs::File::create` ×1 | (2) | `file_writes::create` | worker | A2b |  |
| 88 | bt-app | – | `crate::update::Claim::take` | `std::io::Write::write_fmt` ×2 | (2) | `file_writes::write_line` | worker | A2b |  |
| 89 | bt-app | – | `crate::update_trial::take_the_claim_within` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` · interim `TrialClaim` (new row, Starting) | owner (Starting) | A2c | candidate "stays" |
| 90 | bt-app | – | `crate::update_trial::watch` | `std::thread::sleep` ×2 | (2) | `wait::sleep_within` | worker | A2c |  |
| 91 | bt-app | – | `crate::webhost::WebSeat::go_to` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` · interim `PathResolve` | owner | A2b | B10 |
| 92 | bt-app | [debug_assertions] | `crate::hang_watch::run_selftest_if_due` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` · interim `SelfTest` (new row) | owner | A2c | debug builds only; candidate "stays" |
| 93 | bt-app | [windows] | `crate::attention_copilot::run_probe` | `std::process::Command::output` ×1 | (2) | `quiet_command::output_within` | worker | A2c |  |
| 94 | bt-app | [windows] | `crate::psreadline::run_probe` | `std::process::Command::output` ×1 | (2) | `quiet_command::output_within` | worker | A2c |  |
| 95 | bt-app | [windows] | `crate::shell_integration::run_profile_probe` | `std::process::Child::wait` ×1 | (2) | `quiet_command::wait_within` | worker + standalone | A2c |  |
| 96 | bt-app | [windows] | `crate::shell_integration::run_profile_probe` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` | worker + standalone | A2c |  |
| 97 | bt-persist | – | `crate::atomic::commit_rename` | `std::fs::rename` ×1 | (2) | `file_writes::rename` (in `bt_persist::atomic`) | owner + worker + standalone | A2b | owner tokens: row 20's stores, `PreviewSave`, `MarksInstall`, `AgentHooksWrite`, `SchemeWrite` |
| 98 | bt-persist | – | `crate::atomic::write_temp` | `std::fs::File::sync_all` ×1 | (2) | `file_writes::sync_all` (in `bt_persist::atomic`) | owner + worker + standalone | A2b | as 97 |
| 99 | bt-persist | – | `crate::migrate::keep_oversized` | `std::fs::rename` ×1 | (2) | `file_writes::rename` · interim `StoreOpen` / `LaunchHandOver` | owner + owner (Starting) + worker + standalone | A2b | B10 |
| 100 | bt-persist | – | `crate::migrate::keep_rejected` | `std::fs::write` ×1 | (2) | `file_writes::write` · interim `StoreOpen` / `LaunchHandOver` | owner + owner (Starting) + worker + standalone | A2b | B10 |
| 101 | bt-persist | – | `crate::migrate::read_bounded` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · interim `StoreOpen` / `LaunchHandOver` | owner + owner (Starting) + worker + standalone | A2b | B10 |
| 102 | bt-platform | – | `crate::handoff::resolved_for_a_door` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 103 | bt-platform | – | `crate::handoff::reveal_arguments` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b | reached on Windows only |
| 104 | bt-platform | – | `crate::hotkey::SummonTrace::opened` | `std::io::Write::write_fmt` ×1 | (2) | `file_writes::write_line` · `DiagnosticWrite` (row 20) | owner | A2b | `BT_HOTKEY_TRACE` only; also from the message hook |
| 105 | bt-platform | – | `crate::hotkey::SummonTrace::write` | `std::io::Write::write_fmt` ×1 | (2) | `file_writes::write_line` · `DiagnosticWrite` (row 20) | owner | A2b | as 104 |
| 106 | bt-platform | – | `crate::https_download::<File as Store>::seal` | `std::fs::File::sync_all` ×1 | (2) | `file_writes::sync_all` | worker | A2b | reached on macOS only |
| 107 | bt-platform | – | `crate::https_download::Partial::finish` | `std::fs::rename` ×1 | (2) | `file_writes::rename` | worker | A2b | reached on macOS only |
| 108 | bt-platform | – | `crate::install_txn::hold_until` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` | standalone (`--update-apply`, macOS, entered) since U-28 | A2c | reached through `install_txn::hold_within` from `update_apply_macos` (U-28); the worker context is in scope there |
| 109 | bt-platform | – | `crate::instance::canonical_path` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` · interim `ClaimName` (new row) / `LaunchHandOver` | owner + owner (Starting) | A2b | B10; Unix only |
| 110 | bt-platform | – | `crate::launch_agent::sweep` | `std::fs::read_dir` ×1 | (2) | `file_reads::observe` | standalone | A2b | `--uninstall-cleanup` |
| 111 | bt-platform | – | `crate::macos_update::attach_with` | `std::fs::canonicalize` ×3 | (2) | `file_reads::observe` | worker | A2b |  |
| 112 | bt-platform | – | `crate::macos_update::detach_point` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` | worker | A2c |  |
| 113 | bt-platform | – | `crate::macos_update::points_under` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` · interim `UpdateMounts` (new row, Starting) | owner (Starting) + standalone + worker | A2b | candidate "stays" |
| 114 | bt-platform | – | `crate::macos_update::run_at` | `std::process::Child::wait` ×1 | (2) | `quiet_command::wait_within` | worker | A2c |  |
| 115 | bt-platform | – | `crate::macos_update::run_at` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` | worker | A2c |  |
| 116 | bt-platform | – | `crate::macos_update::verify_clone` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 117 | bt-platform | [all(windows, target_arch = "x86_64")] | `crate::hang::capture_thread_stack` | `windows::Win32::Foundation::CloseHandle` ×1 | (2) | `handles::close_handle` | worker | A2c |  |
| 118 | bt-platform | [not(windows)] | `crate::file_replace::replace_file_preserving` | `std::fs::File::sync_all` ×1 | (2) | `file_writes::sync_all` (in `file_replace`) | owner + owner (Exiting) + worker + standalone | A2b | owner tokens as 97 |
| 119 | bt-platform | [not(windows)] | `crate::file_replace::replace_file_preserving` | `std::fs::rename` ×1 | (2) | `file_writes::rename` (in `file_replace`) | owner + owner (Exiting) + worker + standalone | A2b | as 118 |
| 120 | bt-platform | [not(windows)] [target_os = "macos"] | `crate::video::macos_player::Engine::shutdown` | `std::thread::JoinHandle::join` ×1 | (5) | `drop-exception`: `macos_player::Engine::shutdown` (pinned) | owner + drop | A2c | D-80; the direct owner roads are an open item |
| 121 | bt-platform | [not(windows)] [target_os = "macos"] | `crate::video::macos_player::Engine::shutdown` | `std::thread::sleep` ×1 | (5) | `drop-exception`: as 120 | owner + drop | A2c | D-80 |
| 122 | bt-platform | [not(windows)] [target_os = "macos"] | `crate::video::macos_player::Engine::wait_for_metadata` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` | none (examples and tests) | A2c |  |
| 123 | bt-platform | [not(windows)] [target_os = "macos"] | `crate::video::macos_player::Machinery::one_turn` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` | worker | A2c |  |
| 124 | bt-platform | [not(windows)] [target_os = "macos"] | `crate::video::within_budget` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` | worker | A2c |  |
| 125 | bt-platform | [not(windows)] [unix] | `crate::portable_impl::redirect_std_streams_to_file` | `libc::close` ×1 | (2) | `handles::close_fd` · interim `RunLog` | owner (Starting) | A2c | candidate "stays" |
| 126 | bt-platform | [not(windows)] [unix] | `crate::portable_impl::same_file` | `std::fs::metadata` ×2 | (2) | `file_reads::observe` · interim `FilesRowCase` (new row) | owner | A2b | B10 |
| 127 | bt-platform | [not(windows)] [unix] | `crate::portable_impl::write_std_error` | `libc::write` ×1 | (2) | `handles::write_fd` | worker + owner (Starting) + standalone | A2c | stderr |
| 128 | bt-platform | [target_os = "macos"] | `crate::handoff::macos_handoff::openable_target` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 129 | bt-platform | [target_os = "macos"] | `crate::handoff::macos_handoff::openable_target` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 130 | bt-platform | [target_os = "macos"] | `crate::handoff::macos_handoff::reveal_in_explorer` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 131 | bt-platform | [target_os = "macos"] | `crate::handoff::macos_handoff::reveal_in_explorer` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 132 | bt-platform | [target_os = "macos"] | `crate::hang::ask_run_loop_to_answer` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` | worker | A2c |  |
| 133 | bt-platform | [target_os = "macos"] | `crate::http::Download::wait` | `std::sync::Condvar::wait_timeout` ×1 | (2) | `wait::condvar_wait_timeout` | worker | A2c |  |
| 134 | bt-platform | [target_os = "macos"] | `crate::http::Exchange::wait` | `std::sync::Condvar::wait_timeout` ×1 | (2) | `wait::condvar_wait_timeout` | worker | A2c |  |
| 135 | bt-platform | [target_os = "macos"] | `crate::install_txn::arm::<Os as Surface>::rename` | `std::fs::rename` ×1 | (2) | `file_writes::rename` | worker | A2b |  |
| 136 | bt-platform | [target_os = "macos"] | `crate::macos_files::recycle` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · interim `Recycle` (new row) | owner | A2b | B10 |
| 137 | bt-platform | [target_os = "macos"] | `crate::macos_identity::arm::run` | `std::process::Child::wait` ×1 | (2) | `quiet_command::wait_within` | worker | A2c |  |
| 138 | bt-platform | [target_os = "macos"] | `crate::macos_identity::arm::run` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` | worker | A2c |  |
| 139 | bt-platform | [target_os = "macos"] | `crate::macos_watch::<DirWatch as Drop>::drop` | `std::thread::JoinHandle::join` ×1 | (5) | `drop-exception`: macOS `DirWatch::drop` (pinned) | drop (owner) | A2c | D-40, B7 |
| 140 | bt-platform | [target_os = "macos"] | `crate::macos_watch::DirWatch::start_scoped` | `std::fs::canonicalize` ×1 | (2) | `file_reads::observe` · `WatchStart` (row 8) | owner | A2c | B7 |
| 141 | bt-platform | [target_os = "macos"] | `crate::macos_watch::DirWatch::start_scoped` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` · `WatchStart` (row 8) | owner | A2c | B7 |
| 142 | bt-platform | [target_os = "macos"] | `crate::macos_watch::DirWatch::start_scoped` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` · interim `WatchStart` (row 8) | owner | A2c | B7 |
| 143 | bt-platform | [target_os = "macos"] | `crate::macos_watch::DirWatch::start_scoped` | `std::thread::JoinHandle::join` ×2 | (2) | `wait::join` · interim `WatchStart` (row 8) | owner | A2c | B7 |
| 144 | bt-platform | [target_os = "macos"] | `crate::quiet_command_text` | `std::process::Command::output` ×1 | (2) | `quiet_command::output_within` · `LocaleProbe` (row 7) | owner | A2c | B6 |
| 145 | bt-platform | [unix] | `crate::attention_pipe::<AttentionPipe as Drop>::drop` | `libc::close` ×1 | (5) | `drop-exception`: Unix `AttentionPipe::drop` (pinned) | none (a static) | A2c | D-79 |
| 146 | bt-platform | [unix] | `crate::attention_pipe::<AttentionPipe as Drop>::drop` | `libc::write` ×1 | (5) | as 145 | none (a static) | A2c | D-79 |
| 147 | bt-platform | [unix] | `crate::attention_pipe::<AttentionPipe as Drop>::drop` | `std::thread::JoinHandle::join` ×1 | (5) | as 145 | none (a static) | A2c | D-79 |
| 148 | bt-platform | [unix] | `crate::attention_pipe::AttentionPipe::start` | `libc::close` ×2 | (2) | `handles::close_fd` · interim `EndpointStart` (new row) | owner | A2c | B11 (proposed) |
| 149 | bt-platform | [unix] | `crate::attention_pipe::listen` | `libc::close` ×1 | (2) | `handles::close_fd` | worker (reached in tests only) | A2c |  |
| 150 | bt-platform | [unix] | `crate::file_replace::carry_metadata` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` (in `file_replace`) | owner + owner (Exiting) + worker + standalone | A2b | as 97 |
| 151 | bt-platform | [unix] | `crate::install_evidence::imp::owner_of` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 152 | bt-platform | [unix] | `crate::launch_pipe::<LaunchPipe as Drop>::drop` | `libc::close` ×1 | (5) | `drop-exception`: Unix `LaunchPipe::drop` (pinned) | none (a static) | A2c | D-79 |
| 153 | bt-platform | [unix] | `crate::launch_pipe::<LaunchPipe as Drop>::drop` | `libc::write` ×1 | (5) | as 152 | none (a static) | A2c | D-79 |
| 154 | bt-platform | [unix] | `crate::launch_pipe::<LaunchPipe as Drop>::drop` | `std::thread::JoinHandle::join` ×1 | (5) | as 152 | none (a static) | A2c | D-79 |
| 155 | bt-platform | [unix] | `crate::launch_pipe::LaunchPipe::start` | `libc::close` ×2 | (2) | `handles::close_fd` · interim `EndpointStart` | owner | A2c | B11 (proposed) |
| 156 | bt-platform | [unix] | `crate::launch_pipe::listen` | `libc::close` ×1 | (2) | `handles::close_fd` | worker (reached in tests only) | A2c |  |
| 157 | bt-platform | [unix] | `crate::launch_pipe::vet_executable` | `std::fs::metadata` ×2 | (2) | `file_reads::observe` · `LaunchHandOver` | worker + owner (Starting) | A2b | the client half is inside row 18's door |
| 158 | bt-platform | [windows] | `crate::attention_pipe::<AttentionPipe as Drop>::drop` | `std::thread::JoinHandle::join` ×1 | (5) | `drop-exception`: Windows `AttentionPipe::drop` (pinned) | none (a static) | A2c | D-79 |
| 159 | bt-platform | [windows] | `crate::attention_pipe::<AttentionPipe as Drop>::drop` | `windows::Win32::Foundation::CloseHandle` ×1 | (5) | as 158 | none (a static) | A2c | D-79 |
| 160 | bt-platform | [windows] | `crate::attention_pipe::<AttentionPipe as Drop>::drop` | `windows::Win32::System::Threading::SetEvent` ×1 | (5) | as 158 | none (a static) | A2c | D-79 |
| 161 | bt-platform | [windows] | `crate::attention_pipe::<Overlapped as Drop>::drop` | `windows::Win32::Foundation::CloseHandle` ×1 | (5) | `drop-exception`, counted only (added to the pinned table) | worker + standalone + owner (Starting) | A2c | closes, does not wait |
| 162 | bt-platform | [windows] | `crate::attention_pipe::<OwnedHandle as Drop>::drop` | `windows::Win32::Foundation::CloseHandle` ×1 | (5) | `drop-exception`, counted only (added to the pinned table) | owner + owner (Starting) + worker + standalone | A2c | as 161 |
| 163 | bt-platform | [windows] | `crate::attention_pipe::AttentionPipe::start` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` · interim `EndpointStart` (new row) | owner | A2c | B11 (proposed) |
| 164 | bt-platform | [windows] | `crate::attention_pipe::AttentionPipe::start` | `std::thread::JoinHandle::join` ×2 | (2) | `wait::join` · interim `EndpointStart` | owner | A2c | B11 (proposed) |
| 165 | bt-platform | [windows] | `crate::attention_pipe::AttentionPipe::start` | `windows::Win32::Foundation::CloseHandle` ×2 | (2) | `handles::close_handle` · interim `EndpointStart` | owner | A2c | B11 (proposed) |
| 166 | bt-platform | [windows] | `crate::attention_pipe::AttentionPipe::start` | `windows::Win32::System::Threading::SetEvent` ×1 | (2) | `handles::set_event` · interim `EndpointStart` | owner | A2c | B11 (proposed) |
| 167 | bt-platform | [windows] | `crate::file_replace::carry_metadata` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` (in `file_replace`) · `PreviewSave` | owner + owner (Exiting) | A2b | row 20, document half |
| 168 | bt-platform | [windows] | `crate::file_replace::rename_path` | `std::fs::rename` ×1 | (2) | `file_writes::rename` (in `file_replace`) | owner + worker + standalone | A2b | owner tokens as 97 |
| 169 | bt-platform | [windows] | `crate::file_replace::replace_file_preserving_with` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` (in `file_replace`) | owner + worker + standalone | A2b | as 168 |
| 170 | bt-platform | [windows] | `crate::http::<Request as Drop>::drop` | `std::sync::Condvar::wait_timeout` ×1 | (5) | `drop-exception`: `http::Request::drop` (pinned) | none on Windows until U-20 | A2c | D-82 |
| 171 | bt-platform | [windows] | `crate::http::Shared::wait` | `std::sync::Condvar::wait_timeout` ×1 | (2) | `wait::condvar_wait_timeout` | worker (after U-20) | A2c |  |
| 172 | bt-platform | [windows] | `crate::install_evidence::imp::current_account` | `windows::Win32::Foundation::CloseHandle` ×1 | (2) | `handles::close_handle` | worker | A2c |  |
| 173 | bt-platform | [windows] | `crate::instance::<DataDirectoryClaim as Drop>::drop` | `windows::Win32::Foundation::CloseHandle` ×1 | (5) | `drop-exception`, counted only (added to the pinned table) | standalone + owner (Starting) | A2c | as 161 |
| 174 | bt-platform | [windows] | `crate::instance::try_claim_data_directory` | `windows::Win32::Foundation::CloseHandle` ×1 | (2) | `handles::close_handle` · interim `TrialClaim` / owner (Starting) | owner (Starting) + standalone | A2c | candidate "stays" |
| 175 | bt-platform | [windows] | `crate::launch_pipe::<LaunchPipe as Drop>::drop` | `std::thread::JoinHandle::join` ×1 | (5) | `drop-exception`: Windows `LaunchPipe::drop` (pinned) | none (a static) | A2c | D-79 |
| 176 | bt-platform | [windows] | `crate::launch_pipe::<LaunchPipe as Drop>::drop` | `windows::Win32::Foundation::CloseHandle` ×1 | (5) | as 175 | none (a static) | A2c | D-79 |
| 177 | bt-platform | [windows] | `crate::launch_pipe::<LaunchPipe as Drop>::drop` | `windows::Win32::System::Threading::SetEvent` ×1 | (5) | as 175 | none (a static) | A2c | D-79 |
| 178 | bt-platform | [windows] | `crate::launch_pipe::Instance::arm_connect` | `windows::Win32::System::Threading::SetEvent` ×1 | (2) | `handles::set_event` | worker | A2c |  |
| 179 | bt-platform | [windows] | `crate::launch_pipe::LaunchPipe::start` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` · interim `EndpointStart` | owner | A2c | B11 (proposed) |
| 180 | bt-platform | [windows] | `crate::launch_pipe::LaunchPipe::start` | `std::thread::JoinHandle::join` ×2 | (2) | `wait::join` · interim `EndpointStart` | owner | A2c | B11 (proposed) |
| 181 | bt-platform | [windows] | `crate::launch_pipe::LaunchPipe::start` | `windows::Win32::Foundation::CloseHandle` ×2 | (2) | `handles::close_handle` · interim `EndpointStart` | owner | A2c | B11 (proposed) |
| 182 | bt-platform | [windows] | `crate::launch_pipe::LaunchPipe::start` | `windows::Win32::System::Threading::SetEvent` ×1 | (2) | `handles::set_event` · interim `EndpointStart` | owner | A2c | B11 (proposed) |
| 183 | bt-platform | [windows] | `crate::process_image_path` | `windows::Win32::Foundation::CloseHandle` ×1 | (2) | `handles::close_handle` · `LaunchHandOver` | owner (Starting) | A2c | inside row 18's door |
| 184 | bt-platform | [windows] | `crate::video::Readers::quiet_within` | `std::sync::Condvar::wait_timeout` ×1 | (2) | `wait::condvar_wait_timeout` · interim `MediaQuiet` (new row, Exiting) | owner (Exiting) | A2c | candidate "stays" |
| 185 | bt-platform | [windows] | `crate::video::engine::Engine::shutdown` | `std::thread::JoinHandle::join` ×1 | (5) | `drop-exception`: Windows `video::engine::Engine::shutdown` (pinned) | owner + drop | A2c | D-80 |
| 186 | bt-platform | [windows] | `crate::video::engine::Engine::shutdown` | `std::thread::sleep` ×1 | (5) | as 185 | owner + drop | A2c | D-80 |
| 187 | bt-platform | [windows] | `crate::video::engine::Engine::wait_for_metadata` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` | none (examples and tests) | A2c |  |
| 188 | bt-platform | [windows] | `crate::video::engine::Machinery::pump` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` | worker | A2c |  |
| 189 | bt-platform | [windows] | `crate::video::engine::can_play_types` | `std::thread::JoinHandle::join` ×1 | (2) | `wait::join` | none (tests) | A2c |  |
| 190 | bt-platform | [windows] | `crate::video::within_budget` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (2) | `wait::recv_timeout` | worker | A2c |  |
| 191 | bt-platform | [windows] | `crate::windows_impl::<DirWatch as Drop>::drop` | `std::thread::JoinHandle::join` ×1 | (5) | `drop-exception`: Windows `DirWatch::drop` (pinned) | drop (owner) | A2c | D-40; B7 (widened to Windows) |
| 192 | bt-platform | [windows] | `crate::windows_impl::<DirWatch as Drop>::drop` | `windows::Win32::System::Threading::SetEvent` ×1 | (5) | as 191 | drop (owner) | A2c | as 191 |
| 193 | bt-platform | [windows] | `crate::windows_impl::DirWatch::start_scoped` | `std::sync::mpsc::Receiver::recv` ×1 | (2) | `wait::recv` · interim `WatchStart` (row 8, Windows arm added) | owner | A2c | B7 (widened) |
| 194 | bt-platform | [windows] | `crate::windows_impl::DirWatch::start_scoped` | `std::thread::JoinHandle::join` ×1 | (2) | `wait::join` · interim `WatchStart` | owner | A2c | B7 (widened) |
| 195 | bt-platform | [windows] | `crate::windows_impl::close` | `windows::Win32::Foundation::CloseHandle` ×1 | (5) | `drop-exception`: `windows_impl::close` (a pinned body of the `DirWatch` chain) | drop (owner) + owner (inside `WatchStart`) | A2c | as 191 |
| 196 | bt-platform | [windows] | `crate::windows_impl::directory_folds_case` | `windows::Win32::Foundation::CloseHandle` ×1 | (2) | `handles::close_handle` · interim `FilesRowCase` | owner | A2c | B10 |
| 197 | bt-platform | [windows] | `crate::windows_impl::open_clipboard_with_retry` | `std::thread::sleep` ×1 | (2) | `wait::sleep_within` (the path handed as a value) · interim `ClipboardOpen` (new row) | owner | A2c | candidate "stays" (at most 75 ms) |
| 198 | bt-platform | [windows] | `crate::windows_impl::write_to_console` | `windows::Win32::Foundation::CloseHandle` ×1 | ? | could not classify: the panic hook road | owner (Starting) + standalone + any (panic hook) | — |  |
| 199 | bt-platform | [windows] [not(test)] | `crate::handoff::windows_handoff::shell_execute_w` | `windows::Win32::UI::Shell::ShellExecuteW` ×1 | (1) | the hand-off door's body (`ShellThread`) | worker | A2c | row 1 (done) |
| 200 | bt-pty | – | `crate::InputRing::take` | `std::sync::Condvar::wait` ×1 | (1) | `transport` | transport | A2d |  |
| 201 | bt-pty | – | `crate::OutputRing::push_read` | `std::sync::Condvar::wait` ×1 | (1) | `transport` | transport | A2d |  |
| 202 | bt-pty | – | `crate::PtyDump::create_at` | `std::fs::File::create` ×2 | (1) | `transport` (runs inside `PtyBirth`) | owner (inside row 11's door) | A2d | debug dumps only |
| 203 | bt-pty | – | `crate::PtyDump::create_at` | `std::io::Write::write_fmt` ×2 | (1) | `transport` (runs inside `PtyBirth`) | owner (inside row 11's door) | A2d | debug dumps only |
| 204 | bt-pty | – | `crate::PtyDump::finish` | `std::io::Write::write_fmt` ×1 | (5) | `drop-exception`: `PtyDump::finish` (pinned) | transport + drop | A2d | D-81 |
| 205 | bt-pty | – | `crate::PtyDump::publish` | `std::fs::File::sync_data` ×2 | (5) | `drop-exception`: `PtyDump::publish` (pinned) | owner (inside `PtyBirth`) + transport + drop | A2d | D-81 |
| 206 | bt-pty | – | `crate::PtyDump::write_chunk` | `std::io::Write::write_fmt` ×1 | (1) | `transport` | transport | A2d |  |
| 207 | bt-pty | – | `crate::PtyDump::write_input_at` | `std::io::Write::write_fmt` ×1 | (1) | `transport` | owner | A2d | debug dumps only; beside row 19 |
| 208 | bt-pty | – | `crate::PtyDump::write_resize` | `std::io::Write::write_fmt` ×1 | (1) | `transport` (runs inside `PtyResize`) | owner (inside row 12's door) | A2d | debug dumps only |
| 209 | bt-pty | – | `crate::PtySession::shutdown` | `portable_pty::Child::try_wait` ×2 | (5) | `drop-exception`: `PtySession::shutdown` (pinned) | transport + drop | A2d | D-81 |
| 210 | bt-pty | – | `crate::PtySession::try_wait` | `portable_pty::Child::try_wait` ×1 | (1) | `transport` (counted, does not wait) | owner | A2d |  |
| 211 | bt-pty | – | `crate::Retirements::wait_within` | `std::sync::Condvar::wait_timeout` ×1 | (1) | `transport` (runs inside `PaneRetirementWait`) | owner (Exiting, inside row 15's door) | A2d |  |
| 212 | bt-pty | – | `crate::join_within` | `std::thread::JoinHandle::join` ×1 | (5) | `drop-exception`: `join_within` (pinned) | transport + drop | A2d | D-81 |
| 213 | bt-pty | – | `crate::join_within` | `std::thread::sleep` ×1 | (5) | as 212 | transport + drop | A2d | D-81 |
| 214 | bt-pty | – | `crate::reap_within` | `std::thread::sleep` ×1 | (5) | `drop-exception`: `reap_within` (pinned) | transport + drop | A2d | D-81 |
| 215 | bt-pty | – | `crate::shell::<SystemShellEnvironment as ShellEnvironment>::is_file` | `std::fs::metadata` ×1 | (1) | `transport` (a Unix-only statement) | owner + worker | A2d | arm `-`, effect under `cfg(unix)` |
| 216 | bt-pty | – | `crate::spawn_dump_publisher` | `std::fs::File::sync_data` ×2 | (1) | `transport` (the publisher thread) | transport | A2d | the site is written in the spawner |
| 217 | bt-pty | – | `crate::spawn_dump_publisher` | `std::thread::sleep` ×1 | (1) | `transport` (the publisher thread) | transport | A2d | as 216 |
| 218 | bt-render | – | `crate::WindowRenderer::compose_frame` | `wgpu::Queue::present` ×1 | (1) | in the door `PresentFrame` (row 9): `compose_frame` takes `&WaitToken` | owner | A2c |  |
| 219 | bt-render | – | `crate::WindowRenderer::compose_frame` | `wgpu::Queue::submit` ×1 | (1) | in the door `PresentFrame` (row 9) | owner | A2c |  |
| 220 | bt-render | – | `crate::WindowRenderer::handle_surface_failure` | `wgpu::Queue::submit` ×1 | (1) | in the door `PresentFrame` (row 9): `handle_surface_failure` takes `&WaitToken` | owner | A2c |  |
| 221 | bt-render | – | `crate::WindowRenderer::probe_frame` | `wgpu::Queue::present` ×1 | ? | could not classify: a probe outside the product binary | none (bt-corpus tools, tests) | — |  |
| 222 | bt-render | – | `crate::WindowRenderer::probe_frame` | `wgpu::Queue::submit` ×1 | ? | could not classify: as 221 | none (bt-corpus tools, tests) | — |  |
| 223 | bt-render | – | `crate::WindowRenderer::read_back` | `wgpu::Queue::submit` ×1 | ? | could not classify: as 221 | none (tests, one example) | — |  |
| 224 | bt-term | – | `crate::inline_image::LocalImageStamp::of` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 225 | bt-term | – | `crate::session::path_exists` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |
| 226 | bt-term | – | `crate::session::verify_path` | `std::fs::metadata` ×1 | (2) | `file_reads::observe` | worker | A2b |  |

**Sum check.** 226 rows, 248 sites, one disposition each. The table was generated from the inventory at
`b054379b` joined by row number with the allocation, and the counts in (k)1 are computed from the same join.

### (k)4 · Could not classify: five rows, with a proposal each

| # | item · entry | why no kind holds it | proposal (for the review) |
|---:|---|---|---|
| 5 | `append_panic_report` · `Write::write_fmt` | The panic hook (`install_panic_log_hook_at`'s closure) runs on **whichever thread panics**. That includes bt-pty's `Unset` threads and the rayon pool, which hold no capability and cannot be given one. The other road, `report_frame_shape_stop` from `FolioApp::fail`, is the owner's. A panic is not a destructor, so (5) does not hold it as written. | Read (5) as "a retained destructor **or unwind** road", and pin the hook's body in the exception table the way A1e pins a `Drop`: the edge list, and `write_fmt` ×1. The owner road takes a `DiagnosticWrite` token. The alternative is to post the report to the trace sink, which loses the report when the process dies, which is the case it exists for. Owner or Codex to choose; (k)10 Q5. |
| 198 | `windows_impl::write_to_console` · `CloseHandle` | The same hook (`announce_panic`) reaches it from any thread. Its other roads are the front door's messages (owner `Starting`, inside `LaunchHandOver` through `say`) and four standalone verbs. | The same as row 5 for the hook road. Every other road goes through `handles::close_handle` with the verb's context or the `LaunchHandOver` token. |
| 221, 222 | `WindowRenderer::probe_frame` · `Queue::present`, `Queue::submit` | This is a public bt-render function with no caller in the `folio` binary. Its callers are `HeadlessRenderProbe::prepare_frame` (bt-corpus's `bt-replay` and `bt-zoom-perf`) and bt-render's tests. No GPU door family exists or is planned, and a worker door would be wrong: the queue belongs to the owner. | Put the probe behind a non-default `headless-probe` feature of bt-render, which bt-corpus enables. A product configuration (C-4: default features) then does not compile it, and the row goes. This is a code change that changes no product behaviour. It is outside A2b–A2d's families, so it becomes a small ticket of its own or a line in A2e. |
| 223 | `WindowRenderer::read_back` · `Queue::submit` | This is public too. It is reached only from tests, `bt-render/tests/glyph_output.rs`, `bt-app/tests/macos_glyph_surface.rs` and `bt-app/examples/video-probe.rs`. | The same feature. |

### (k)5 · The effect-level table for every interim owner door

Each line is one admission identity: a `# doors` line, an `admission::doors` type and a registry row. For each
line, these hold:

- **Function boundary:** the minted call (`admitted::<D>(|t| …)`) at the site named. The door functions
  reached under it are the family doors of (k)3, each taking `&t`.
- **Role:** `Window`.
- **Capability:** `WaitToken<'_, doors::D>`.
- **Refusal:** in every row it happens before any state the road mutates.

The witness test drives the real caller on a window-entered test thread, and asserts the admitted record and
the effect's count. "Found" is the execution context the survey saw on `b054379b`. **Every version is the
owner's to rule.** The column gives the ledger's version where one exists, and "owner" where none does.
`open` is never rendered "ruled to stay". Where a line is proposed for that ruling, it says so and goes to
(k)10.

| identity (row) | minted at | phases | effects (rows of (k)3) | refusal | witness | owed · version | found · discovered |
|---|---|---|---|---|---|---|---|
| `MarksInstall` (2) | `Runtime::add_to_profile`; `Runtime::spend_powershell_intent` (the `install_into_profile` call) | Running | 65, 66 (the `.ps1` road), 69–75, and through `bt_persist::atomic`/`file_replace` 97, 98, 118, 119, 150, 168, 169 | the strip keeps its verb, as after a failed install | `an_add_press_is_admitted_as_marks_install_and_writes_through_file_writes` | B4 · 0.4.6 (D-34) | turn · 2026-09-23 (row 2) |
| `PsReadLineApply` (3) | `Runtime::apply_psreadline`; `Runtime::create`'s `psreadline::upgrade_recorded`; `App::release_trial_writes`'s upgrade | Running | 47, 48, 69–73 | the Settings row keeps its verb and the toast says it was not applied. At `create`, the upgrade is skipped this launch and offered again at the next. | `apply_psreadline_is_admitted_and_the_launch_upgrade_is_too` | B4 · 0.4.6 (D-35; D-34's ticket-56 note) | turn and window birth · 2026-09-21; the launch road 2026-09-27 (this survey) |
| `PsReadLineProbe` (4) | `Runtime::refresh_psreadline_installed`; `Runtime::raise_psreadline_invite_if_due` | Running | 49 | the row draws the last adopted answer | `the_installed_copy_walk_is_admitted_on_the_page_open_edge` | B5 · 0.4.6 (D-36) | turn · 2026-09-21 |
| `LocaleProbe` (7) | `create_leaf_session`, around `shell_integration::shell_command`; `system_locale_declaration` takes `&t` down to `quiet_command_text` | Running | 144 (two children); `Path::exists` rides it if (k)2 item 2 lands | the shell is born without the declaration, as on a machine that does not say | `the_first_pane_birth_admits_the_locale_children_once` (Mac) | B6 · 0.4.6 (D-39) | pane birth, once per process · row 7 |
| `WatchStart` (8, both arms) | `dir_news::arm`, `files_watch`/`git_watch`/`preview_watch` subscribe roads, minted at `Runtime::advance_*_watch` and `Runtime::create`'s scheme and storage watches | Running | 140–143 (macOS), 193, 194 (Windows), and 195 when it is reached from a refusal | the subscription stays unarmed and the next sync retries, as a refused start does today | `a_watch_start_is_admitted_and_its_refusal_is_retried` | B7 **widened to Windows** · 0.4.6 (D-40) | turn · row 8 (macOS); Windows arm 2026-09-27 |
| `DeviceRecovery` (10) | `FolioApp::fail` → `recovered_from_a_lost_device` | Running, Exiting | 1, 3 | the device is not rebuilt, and `fail` takes its give-up road (the frame-shape stop) | `a_lost_device_is_recovered_under_its_admission` | B9 · 0.4.6 (D-42) | turn · row 10 |
| `SettingsWrite`, `KeybindingsWrite`, `ProfilesWrite` (20) | the store methods in `persist.rs` and `runtime/configuration.rs` (stations exist) | Running, Exiting | 97, 98 (`bt_persist::atomic`) | the in-memory store stays changed, and the next write carries it | `a_settings_change_is_admitted_as_settings_write` | B8 · 0.4.6 (D-47) | turn · row 20 |
| `PreviewSave` (20) | `Runtime::save_preview_on`, the float's save, `Runtime::quit_save` | Running, Exiting | 43, 97, 98, 118, 119, 150, 167 | the buffer stays dirty and the save is not reported | `a_preview_save_is_admitted_on_the_press_and_on_quit` | row 20's document half, not ticketed (owner ruling 2026-09-25, 2) · owner | turn and exit · row 20 |
| `RenameDisk` (20) | `Runtime::finish_rename` → `rename_files_row`, `rename_preview_file` | Running | 56, 58 | the rename editor stays open with the old name | `a_rename_commit_is_admitted_as_rename_disk` | the same · owner | turn · row 20 |
| `DiagnosticWrite` (20) | `trace_sink::write_line`'s sink-less arm; `hotkey::trace`; `glyph_trace::frame` | Running, Exiting | 77, 78, 83, 104, 105 | the line is dropped and counted, as a full sink drops one | `a_sink_less_trace_line_is_admitted_as_diagnostic_write` | B8 ("diagnostic writes join the trace queue") · 0.4.6 | turn and exit · row 20; `BT_GLYPH_CENSUS`'s owner writes found 2026-09-27 |
| `AgentHooksWrite` (new) | `Runtime::apply_claude_hooks`, `apply_codex_notify`, `apply_copilot_hooks` | Running | 6, 7, 9, 20 (the apply half), 69–73 (via `attention_ownership::record`), 97, 98, 168, 169 | the Agents row keeps its verb | `an_agent_hook_install_is_admitted` | B8 **widened** · owner | turn · 2026-09-27 |
| `AgentConfigState` (new) | `Runtime::create` (the three `row_state`s); `Runtime::refresh_agent_rows`; `Runtime::raise_first_run_if_due` | Running | 8, 20 (the state half) | the row draws its last state | `the_agent_rows_state_is_admitted` | B10 · owner | window birth and turn · 2026-09-27 |
| `TrialWritesReleased` (new; station exists) | `App::release_trial_writes` | Running | 2 | the data folder is created at the next write instead | `releasing_trial_writes_is_admitted` | B8 **widened** · owner | turn · 2026-09-27 |
| `SchemeWrite` (new) | `Runtime::add_scheme`, `edit_scheme_at`, `delete_scheme_file`; `import_schemes` | Running | 61, 63 | the scheme is not written, and the toast says so | `adding_a_scheme_is_admitted` | B8 **widened** · owner | turn · 2026-09-27 |
| `ShellScriptsInstall` (new) | `create_leaf_session`'s `Scripts::installed` (outside `PtyBirth`); `App::release_trial_writes` | Running | 65–68 | the pane is born without integration scripts, as when they cannot be written today | `the_first_pane_writes_its_scripts_under_admission` | B8 **widened** · owner | first pane birth · 2026-09-27 |
| `StoreOpen` (new) | `Runtime::create`'s store opens (session, settings, profiles, keybindings, pins, `update::load`); `reread_profiles`, `reread_pins`; `import_settings_from`; `OfferState` `mark_seen`/`skip` | Running | 41, 99–101 (the `LaunchHandOver` road keeps its own token) | the store opens with its defaults, as on a missing file | `window_birth_admits_its_store_reads` | B10 · owner — **candidate "stays"** at window birth, (k)10 Q2 | window birth and turn · 2026-09-27 |
| `ClaimName` (new) | `persist::is_writer_of`, `is_writer_of_document` | Starting, Running | 109 (Unix) | "not the writer", as when the claim is held elsewhere | `the_claim_name_is_resolved_once` | B10 (cache the name; today it is canonicalised on every call, before the cache lookup) · owner | every store open · 2026-09-27 |
| `PathResolve` (new) | `navigate_preview_page`, `toggle_switcher_pin`, `dismiss_web_sheet_on`, `revive_web_pages`, `finish_rename` (the address bar), `open_preview_web_file` roads | Running | 35, 52, 57, 91 | the gesture is refused as for an unresolvable path | `resolving_a_page_path_is_admitted` | B10 · owner | turn · 2026-09-27 |
| `PreviewStat` (new) | `Runtime::advance_preview_watch`; `ask_the_unwatched_preview_files` | Running | 46 | the file is treated as unchanged until the next turn | `a_preview_stamp_is_admitted` | B10 · owner | turn · 2026-09-27 |
| `RecentFolders` (new) | `Runtime::chrome_mouse_input` → `toggle_root_menu` | Running | 59 | the menu lists the folders unchecked | `the_root_menu_stats_are_admitted` | B10 · owner | turn · 2026-09-27 |
| `SchemeCatalogue` (new) | `adopt_stored_schemes` (`create`); `reread_schemes`; `settings_content`; `scheme_labels`; export and import | Running | 60, 62 | the catalogue keeps its last reading | `the_scheme_catalogue_is_read_under_admission` | B10 · owner | window birth and turn · 2026-09-27 |
| `FilesRowCase` (new) | `Runtime::open_files_row_new` → `directory_folds_case` | Running | 126, 196 | the directory is taken as case-sensitive | `a_new_files_row_asks_case_under_admission` | B10 · owner | turn · 2026-09-27 |
| `Recycle` (new) | `Runtime::delete_files_row`, `delete_scheme_file` | Running | 136 (macOS) | the delete is refused, as for a missing file | `a_recycle_is_admitted` | B10 · owner | turn · 2026-09-27 |
| `PeekFacts` (new) | `Runtime::file_peek_card_layers` | Running | 21 | the card shows no facts | `a_peek_cards_one_stat_is_admitted` | none — DESIGN §7.29/§7.37 already admit "once per frame of one hover"; **proposed `ruled to stay`** | frame building · ruled earlier; row 2026-09-27 |
| `EndpointStart` (new) | `Runtime::create` → `open_the_data_directorys_endpoints` | Running | 148, 155, 163–166, 179–182 | the endpoint is absent for this run, as when it fails to start today | `the_endpoints_start_under_admission` | **B11 (proposed)**: the endpoints answer their first word by wake · owner | window birth, up to 5 s each · 2026-09-27 |
| `RunLog` (new) | `diagnostics::enter_resident_run` | Starting | 13–17, 125 | the run goes on with stderr unredirected | `the_run_log_opens_under_admission` | **candidate "stays"** (no loop yet: row 18's reasoning) · owner | before the loop · 2026-09-27 |
| `StorageRelocate` (new) | `fn main`'s `persist::storage_dir()` | Starting | 42 | the old folder is used this run | `the_one_time_relocate_is_admitted` | **candidate "stays"** · owner | before the loop, once · 2026-09-27 |
| `UpdateMounts` (new) | `update_startup::pass` → `mounts_under` | Starting | 113 (macOS) | the retirement is deferred to the next launch | `startup_mounts_are_read_under_admission` | **candidate "stays"** · owner | before the loop · 2026-09-27 |
| `TrialClaim` (new) | `update_trial::take_the_claim` | Starting | 89, 174 (and 173's drop) | the trial refuses to start, which is its existing refusal | `a_trial_takes_its_claim_under_admission` | **candidate "stays"** (bounded by `CLAIM_WAIT`) · owner | before the loop, trials only · 2026-09-27 |
| `MediaQuiet` (new) | `fn main` → `video::shutdown_media_session` | Exiting | 184 | quit proceeds, as on the budget's expiry | `the_media_session_quiets_under_admission` | **candidate "stays"** (bounded exit wait, like rows 15–17) · owner | exit, at most 1.5 s · 2026-09-27 |
| `ClipboardOpen` (new) | `clipboard_text`, `set_clipboard_text` callers | Running | 197 | the clipboard is reported busy, as after the fourth retry | `a_busy_clipboard_retries_under_admission` | **candidate "stays"** (at most 75 ms), or a new B-ticket · owner | turn · 2026-09-27 |
| `SelfTest` (new; station exists) | `FolioApp::about_to_wait` | Running | 92 | no hold | `the_selftest_hold_is_admitted` | **candidate "stays"** (debug builds only) · owner | turn, debug builds with `BT_HANG_SELFTEST` · 2026-09-27 |

**A window-thread wait the table does not admit yet.** The video engines' `shutdown` (rows 120, 121, 185,
186) is reached on the window thread outside any `Drop`, when a pane closes or reopens a file, through
`VideoSeats::close`, `shutdown_all` and `VideoSeat::shutdown`.

- These are pinned bodies of D-80's chain, so an `admitted` edge cannot be added inside them without
  breaking the pin.
- The admission therefore belongs at the callers of `VideoSeats::close` and `shutdown_all` outside a `Drop`,
  as an interim `VideoShutdown` door owed to D-80's ticket (0.4.7).
- A2c places it and names the callers. The survey did not list them all by name, so this is an open item for
  A2c's brief, not a line of this table.

### (k)6 · B10 — the window thread's file observation moves to a worker or a cache

| | |
|---|---|
| **target version** | the owner's (0.4.6 or 0.4.7); after A2b |
| size | M. If the owner rules `StoreOpen` "stays" (k)10 Q2, it drops to S–M. |
| who | Opus, local lane |
| depends on | A2b (the interim doors, `file_reads::observe` and `Authority`); A5's lane contract (`bt-app::lane`) |

**True on BASE.** The window thread stats, lists and canonicalises paths inside turns and at window birth
through ten interim doors, each with an `open` registry row owed to B10:

- `AgentConfigState` (8, 20);
- `StoreOpen` (41, 99–101);
- `ClaimName` (109);
- `PathResolve` (35, 52, 57, 91);
- `PreviewStat` (46);
- `RecentFolders` (59);
- `SchemeCatalogue` (60, 62);
- `FilesRowCase` (126, 196);
- `Recycle` (136).

Each is a gesture's or a frame's blocking call against a disk that can be a network share.

**Goal**, per door, by the class it belongs to:

- **A cache with one owner** for answers that do not change inside a run: `ClaimName` (the directory's
  claim name, computed once per directory). Its door and row are deleted. The residue is none.
- **An observation lane, as a versioned LatestValue request** (B5's shape, which B10 reuses) for answers a
  view draws: `AgentConfigState`, `SchemeCatalogue`, `PreviewStat`, `RecentFolders`. The view draws the last
  adopted answer.
- **A request and answer on the existing lane of the gesture** for answers a gesture needs before it acts:
  `PathResolve` on the hand-off lane's pattern (the path-verify worker already resolves the same paths),
  `FilesRowCase` and `Recycle` on the files worker. The gesture completes when its answer lands.
- **`StoreOpen`, only if the owner does not rule it "stays"**: the stores are read before the first window on
  a worker started at the top of `fn main`, as B6 does for the locale, and adopted by `Runtime::create`.

**Tests red on BASE.**

- `no_turn_stats_a_path_outside_an_admitted_door` (bt-source: `file_reads::observe`'s owner mint sites are
  exactly the doors B10 keeps).
- `a_claim_name_is_resolved_once_per_directory`.
- `an_older_scheme_catalogue_answer_is_never_adopted`.
- `a_path_resolved_after_its_gesture_was_cancelled_does_nothing`.

**Docs.**

- Each door's registry row goes to `done`, with its residue if any.
- The ledger's B10 row, opened when this brief is dispatched.
- A DESIGN entry.

**Architecture impact.**

- (a) The views' observed-file facts get one writer each: the adopt.
- (b) No new thread. It uses the lanes of B5 and the files, preview and hand-off workers.
- (c) It repays B10's row.
- (c′) The rows, menus and catalogues can lag the disk by one answer, and a path gesture completes one turn
  later.
- (d) No.

**Out of scope, found here.** These are the window-thread **content** reads of (k)8, by lane: Attention's
agent configuration files, Settings' `.git` marker, `$PROFILE` and scheme files, and Fonts'
`set_terminal_font` and `svg_document_options`. Those are `file_reads` owner doors under option (d) (A2b), and
B10 covers observation only. Moving them is a separate ticket, (k)10 Q6.

### (k)7 · Ownership among A2b, A2c and A2d; the re-sized briefs; the order

**Shared surfaces, and who changes them.**

- the registry (`window_waits.tsv`: `# rows`, `# doors`, `# effects`);
- `admission` (`Authority`, the interim door types, and `enter_standalone_main`'s callers);
- the guard (`window_waits_tests.rs`: the `# effects` kinds and authorities, the mint-site check for `Authority`
  rows, and statement-level `cfg` for `# effects`).

**A2b lands first and makes every change of shape to them.** A2c and A2d only add lines of an existing shape.

**One amendment to (j)6's "rows 2–4 are A2c's wholly".** `bt_persist::atomic` and `file_replace` are A2b's
(the shared file functions), and they are reached under `MarksInstall` and `PsReadLineApply`. So **A2b creates
and mints every interim identity that any file effect is reached under**. That includes rows 2, 3 and 4's
identities, `RunLog` and `FilesRowCase`. **A2c keeps every row** in `profile_marks`, `psreadline` and
`shell_integration`, and routes them under the tokens A2b has placed. Ownership stays single per row. The
identities that only waits or handles reach (`DeviceRecovery`, `WatchStart`, `LocaleProbe`, `EndpointStart`,
`TrialClaim`, `MediaQuiet`, `ClipboardOpen`, `SelfTest`) are A2c's.

| ticket | rows · sites | families and items | size | order |
|---|---|---|---|---|
| **A2b** | 83 · 90 (bt-app 49, bt-platform 26, bt-persist 5, bt-term 3) | `file_writes` (31 rows) and `file_reads::observe` (52 rows) are born, both on `&impl Authority`; `admission::Authority`; `file_reads` option (d) per (k)8; the standalone verbs' single entry; 26 interim identities minted with their registry rows (the file-reached ones of (k)5, and rows 2–4's); the guard's shape changes | **L** in the repo's convention. **Recommended split:** **A2b1** (M) takes the doors, `Authority`, the guard and the standalone entries, the 35 rows that run only on workers or standalone mains, and option (d). **A2b2** (M) takes the interim identities and the 48 rows that run on the window thread (alone or with others). | first; A2b1 → A2b2 |
| **A2c** | 120 · 130 (bt-platform 71, bt-app 46, bt-render 3) | `wait::*` (51 rows), `quiet_command::*` (7), `handles::*` (16), all on `&impl Authority`; the rows of `profile_marks`, `psreadline` and `shell_integration` (file effects through A2b's doors) (15); the owner-door helpers `compose_frame`, `handle_surface_failure`, `flush_sink` and the hand-off body, as (1) (7 rows, 8 sites); 24 `# effects` rows of kind `drop-exception` for the pinned bodies and the three counted-only drops (5); the eight wait interim identities; `VideoShutdown`'s placement | **L**. **U-28's `wait::sleep_within`** (on main since `c50adb60`), with its `worker-door-body` guard rule, is credited: A2c widens its parameter to `&impl Authority` and adds no second sleep door. The door column stays empty for worker-only rows, as (j)13 has it, and lists the identities only where a window-thread road reaches the door. **Recommended split:** **A2c1** (M) takes the worker and standalone waits, child processes, handles on workers, and `sleep_within`'s widening. **A2c2** (M) takes the owner interim waits, the in-door helpers, the drop-exception rows and the profile-family rows. | after A2b1 (A2c2 after A2b2); A2c1 → A2c2 |
| **A2d** | 18 · 23 (bt-pty only) | the transport effect functions (12 rows, 15 sites, kind `transport`, authority `none (transport)`), including those that run inside `PtyBirth`, `PtyResize` and `PaneRetirementWait`; the pinned `PtySession` chain as `drop-exception` (6 rows, 8 sites); (c)6's debt row; the statement-level arm of row 215 | **S–M**, unchanged | after A2b1; in parallel with A2c |
| **A2e** | 0 | the lint, as (i)1 and (j)9 say; plus (k)2 item 2's vocabulary if the owner rules it, and (k)4's feature line | M (L with item 2) | last |

**Conflicts to expect.**

- A2c and A2d both add `# effects` lines. The section is keyed by crate and item, so the lines do not collide.
- A2b2 and A2c2 both touch `Runtime::create`: A2b2 mints `StoreOpen`, `AgentConfigState`, `SchemeCatalogue`
  and `PsReadLineApply` there, and A2c2 mints `EndpointStart` and `WatchStart`. They land serially.

### (k)8 · The `file_reads` context inventory: every lane × the thread that reads today

58 product call sites, traced on `b054379b`, and U-28's two added at `c50adb60`. The site is the enclosing function. "Ctx unused" means the worker's
closure receives `&WorkerCtx` and does not pass it down. Nothing in bt-pty calls `file_reads`.

| lane | site (file · function · form) | thread today | option (d) |
|---|---|---|---|
| InlineImage | `bt-term inline_image.rs` · `read_and_decode_local_image` (`open`, lane passed in), via `InlineImageDecoder::decode` | worker `bt-math-worker` | worker reader, `&WorkerCtx` passed down |
| InlineImage | `bt-term inline_image.rs` · `decode_background_image` (`open`) | worker `background-picture` | worker |
| Peek | `bt-app animation.rs` · `first_frame` → `file_source_in_lane`; `main.rs` · `peek_pixels` → bt-term `decode_in_lane` | worker `bt-math-worker` | worker |
| Peek | `bt-platform video/mod.rs` · `read_first_frame` (`opaque`) | worker `folio-video-frame` | worker |
| Animation | `bt-app animation.rs` · `FileAnimationSource::open`, `file_source`; its `Read::read` (`LEDGER.add`) on each fill | worker `bt-math-worker` | worker; **but the source is parked on the window thread between fills** (below) |
| Animation | `bt-platform video/engine.rs` · `Machinery::build` (`opaque`, `SetSource`) | worker `folio-video-engine` | worker |
| Preview | `bt-app preview.rs` · `read_up_to` (`open`) | worker `bt-preview-worker` | worker |
| Pdf | `bt-app pdf.rs` · `page_count`, `read_capped` (`open`) | workers `bt-preview-worker`, `bt-math-worker` | worker |
| GitPipe | `bt-app git.rs` · `drain` (`Reader::new`) | worker `bt-git-pipe` | worker (the context is already in scope) |
| Settings | `bt-app git_watch.rs` · `linked_gitdir` (`read_to_string`) | **window, turn** (`advance_git_watch`, a new root) | owner door (a Settings content read; (k)6 out of scope) |
| Settings | `bt-app psreadline.rs` · `run_probe` (`pipe_output`) | worker `psreadline-probe` | worker |
| Settings | `bt-app psreadline.rs` · `installed_disk::System::read` (`read`) | **window**: birth (`upgrade_recorded`), turn (apply, refresh, invite); standalone `--uninstall-cleanup` | owner under `PsReadLineApply`/`PsReadLineProbe`; standalone context |
| Settings | `bt-app schemes.rs` · `read_scheme_file` (`read_to_string`) | **window**: birth and turn | owner under `SchemeCatalogue` |
| Settings | `bt-app shell_integration/profile_marks.rs` · `Marks::read` (`read`) | **window**: birth and turn; workers `powershell-profile-{migration,enable,removal}`; standalone `--remove-shell-integration`, `--uninstall-cleanup` | `Authority`: owner under `MarksInstall`/`PsReadLineApply`/`AgentHooksWrite`; workers; standalone |
| Settings | `bt-app shell_integration.rs` · `install_script_at`, `install_zdotdir` (`read_to_string`) | **window**: first pane, trial release; worker `powershell-profile-migration` | owner under `ShellScriptsInstall`/`MarksInstall`; worker |
| Settings | `bt-app shell_integration.rs` · `read_profile_for_edit` (`Reader::new`) | **window, turn**; workers (migration, removal); standalone (both verbs) | `Authority`, as `Marks::read` |
| Settings | `bt-app shell_integration.rs` · `offer_for` (`read`) | **window, turn** (the terminal notice pass) | owner door (content; (k)6 out of scope) |
| Settings | `bt-app update.rs` · `Claim::take` (`read_to_string`) | worker `bt-update-check` | worker |
| Settings | `bt-persist migrate.rs` · `read_bounded` (`open`) | **window**: before the loop (`say_at_the_front_door`, inside `LaunchHandOver`), birth, turn; worker `bt-update-check`; standalone `--explorer-command` | `Authority`: owner under `StoreOpen`/`LaunchHandOver`; worker; standalone |
| Fonts | `bt-render lib.rs` · `terminal_font_system`, `load_chrome_sans_family` (`opaque`) | **window**, first window, inside `GpuOpen` | owner, already admitted (`GpuOpen`) |
| Fonts | `bt-render lib.rs` · `GpuContext::set_terminal_font` (`opaque` ×2) | **window**: birth and turn (a font change) | owner door (content; (k)6 out of scope) |
| Fonts | `bt-math lib.rs` · `MathEngine::with_system_fonts` (`opaque`) | worker `bt-math-worker` | worker |
| Fonts | `bt-math lib.rs` · `svg_document_options` (`opaque`, a `OnceLock` loading the system fonts) | **whichever thread asks first**: in practice the window thread (chrome marks rasterised in frame building), otherwise `bt-math-worker` | **decide before A2b**: warm it on the math worker at start, or admit it on the owner; its comment ("whichever worker asks first") is wrong |
| Attention | `bt-app attention_{hooks,codex,copilot}.rs` · `state`, `hooks_are_switched_off`, `Config::standing` (`read_to_string`) | **window**: birth (`row_state`), turn (`refresh_agent_rows`, `CopilotProbed`, first run); standalone `--uninstall-cleanup` (`standing`) | owner under `AgentConfigState`; standalone |
| Attention | `bt-app attention_{hooks,copilot}.rs` · `installed_rows` (`read_to_string`) | **window, turn**: every `AttentionSpoke` drain | owner door (content; (k)6 out of scope) |
| Attention | `bt-app attention_hooks.rs` · `Config::land` (`read_to_string`) | **window, turn** (apply); standalone `--uninstall-cleanup` | owner under `AgentHooksWrite`; standalone |
| Attention | `bt-app attention_copilot.rs` · `run_probe` (`pipe_output`) | worker `copilot-version-probe` | worker |
| Attention | `bt-app attention_wire.rs` · `payload_on_stdin` (`Reader::new` on stdin) | worker `folio-attention-stdin`, in the `attention` verb (its main is inside `enter_standalone_main`) | worker (the context is in scope) |
| Attention | `bt-app attention_words.rs` · `lede_in_tail` (`Reader::new`) | standalone `attention`, after the entry has returned | standalone: under (k)1's single entry the context reaches it |
| Install | `bt-app install_channel.rs` · `capped` (`open`); `bt-platform install_evidence.rs` · `attribute` (`LEDGER.add`) | worker `bt-install-channel` | worker |
| Install | `bt-app update_startup.rs` · `run`, `image` (`read`; the whole executable) | **window, before the loop** (`update_startup::pass`) | owner, `Starting`: under `UpdateMounts`, minted at `update_startup::pass`, which both reach |
| Install | `bt-app update_recover.rs` · `run` (`read`) | standalone `--update-recover` | standalone (under (k)1's entry) |
| Update | `bt-app update_prepare_macos.rs` · `matches_its_sum` (`read_to_string`, `open`); `bt-platform macos_update.rs` · `architectures` (`open`), `run_at` (`pipe_output`) | workers `bt-update-job`, `bt-update-sweep`, `folio-update-home-detach` | worker (`run_at` gains the context its public entries already take) |
| Update | `bt-app update_archive.rs` · `expand`; `bt-platform trust.rs` ×4, `trust_windows.rs` · `machine`, `pe_resource.rs` · `read_rcdata`, `launch_agent.rs` · `arm` | none yet (tests; U-20 wires the Windows Prepare) | worker by contract: each takes `&WorkerCtx` when A2b lands, so U-20's caller must hold one |
| UpdateJournal | `bt-app update_trial.rs` · `watch` (`read`) | worker `folio-trial-watch` | worker |
| UpdateJournal | `bt-app update_prepare_macos.rs` · `at_launch` (`read`) | none (tests) | worker |
| UpdateJournal | `bt-app update_apply_macos.rs` · the applier's two journal reads (`read`), U-28 | standalone `--update-apply` (macOS), inside `enter_standalone_main("folio-update-apply")` | standalone: the context is in scope |
| Other | `bt-app main.rs` · `probe_input` (`read`) | **window**, birth, `BT_PROBE_INPUT` only | owner under `StoreOpen` |

**What the inventory says about option (d).**

- **No `Reader` crosses a thread.** Every reader is made and drained in one function, so (j)8's
  `Reader<'w>` borrowing `&'w WorkerCtx` fits every worker site as written.
- **One source outlives its worker body: `FileAnimationSource`.** It is opened on `bt-math-worker`, carried
  to the window thread inside the animation, parked there, and handed back per `AnimationFill`. Its reads
  always run on the worker, but the open handle lives on the owner, and a failed send re-parks it and drops
  it (closing the file) there. Under (j)8 it cannot hold `&WorkerCtx`, because it would outlive the body.
  **A2b must re-cut it:** the worker keeps the source in its own table and the window thread holds only a key.
  This is the one worker lane that needs a change of shape rather than a parameter.
- **Media Foundation reads past its `opaque`.** `opaque` covers only `SetSource` and
  `MFCreateSourceReaderFromURL`. The engine and the abandoned `folio-video-frame` thread go on reading
  uncounted. That is outside C-1 (a third party blocking internally), and is stated so.
- **Window-thread content reads.** Every window-thread read in the table gets an owner door under option
  (d). Most sit under an interim identity of (k)5 or an already admitted door (`GpuOpen`, `LaunchHandOver`).
  Five are content reads with no identity yet: `linked_gitdir`, `offer_for`, `set_terminal_font`,
  `installed_rows` and `svg_document_options`. Each gets an owner door and a registry row owed to (k)10 Q6's
  ticket.

### (k)9 · What the survey found that the registry does not say

These are recorded here for the tickets' briefs. Each becomes a registry edit in the ticket named, never an
inventory row.

1. **Row 2 misses two roads to the marks lock**: `attention_ownership::record`, from the agent-hook installs,
   and `psreadline::upgrade_recorded`, from `Runtime::create` and `release_trial_writes` (A2b2's registry
   edit). `OurTurn::take`'s unbounded wait can hold the window thread for a whole worker transaction on either
   road.
2. **Row 8 names only macOS.** The Windows `DirWatch::start_scoped` also waits unboundedly (`recv`) for the
   watcher's first word, on the window thread (A2c2; B7 widened).
3. **Row 11's road writes the shell integration scripts** (bash, and the three zsh `ZDOTDIR` files, also on
   Windows) outside `PtyBirth`, on the first pane of the run (`ShellScriptsInstall`).
4. **Unlisted window-thread waits:**
   - the endpoints' start in `Runtime::create` (up to 5 s each);
   - a video pane's close (up to 2 s, then a join);
   - the clipboard retry (up to 75 ms);
   - the media session's quiet at exit (1.5 s);
   - the trial's claim before the loop.

   Each becomes a row through (k)5.
5. **`persist::is_writer_of` canonicalises on every call** on Unix, before its cache lookup (`ClaimName`,
   B10).
6. **`BT_GLYPH_CENSUS` alone starts no sink.** So `glyph_trace::frame` writes its file synchronously on the
   window thread every frame (`DiagnosticWrite`).
7. **Stale comments** the tickets fix where they pass:
   - `cached_profile_answer` ("only on workers": the door verbs run it too);
   - `bt_term::session::path_exists` ("what stood on the window thread": only the path-verify worker reaches
     it now);
   - `svg_document_options` ("whichever worker asks first");
   - `update_startup`'s "`mounts_under` waits on nothing".
8. **Ten `Drop`/`listen` rows are unreachable in the product.** `AttentionPipe` and `LaunchPipe` live in
   `OnceLock` statics, so these rows stay (5) and their D-79 repayment stands. The Windows `http` download
   is reached only from the macOS driver until U-20.
9. **`bt-platform/src/lib.rs` holds a NUL byte** inside a `CONOUT$` string literal, so `grep` reads the file
   as binary. The guard reads through `bt_source` and is unaffected, but any script that greps must pass `-a`.
10. **No worker passes its `WorkerCtx` down.** Every worker closure except `bt-os-handoff` (and the macOS
    update job) binds `_ctx` and drops it. A2b1 and A2c1 thread it into each body. That is most of their
    diff, and it is mechanical.

### (k)10 · This revision's own architecture impact, and the owner's decisions

**Impact.**

- **(a)** None by this commit. The tickets it briefs move no fact's owner. B10 (k)6 does (the views' observed
  facts).
- **(b)** None by this commit. The doors A2b–A2d will create are these:
  - the families, on `admission::Authority`;
  - 34 interim owner identities, each a registry line;
  - the standalone verbs' single entries.
- **(c)** None by this commit. When the tickets are briefed, (k) opens:
  - B10;
  - the proposed B11;
  - (c)6's transport debt row (A2d);
  - the new registry rows of (k)5.

  The Q1 rulings may widen B7 and B8.
- **(c′)** None.
- **(d)** Yes: `admission::Authority` amends (j)6 (family doors on `&impl Authority`, not `&WorkerCtx`) and (b)2/§C-5's
  capability list. `# effects` gains the authority `Authority` with a mint-site check. (5) is extended to
  counted-only `Drop`s. (j)6's "rows 2–4 wholly A2c's" is amended as (k)7 says.

**Decisions for the owner.**

1. **Versions and widenings.** Rule each of these:
   - whether A2b–A2e (now A2b1/A2b2, A2c1/A2c2, A2d, A2e) land in 0.4.6 or 0.4.7;
   - whether B7 widens to the Windows watcher;
   - whether B8 widens to the window thread's other file writes (agent hooks, schemes, shell scripts, trial
     release) or a new ticket takes them;
   - the proposed B11 (endpoints);
   - the video shutdown's interim door on D-80's 0.4.7 ticket.
2. **"Stays on the window thread", proposed for ruling** (each bounded, or before the loop, or already ruled
   in DESIGN):
   - `RunLog`, `StorageRelocate`, `UpdateMounts`, `TrialClaim` (before the loop: row 18's reasoning);
   - `MediaQuiet` (a bounded exit wait, like rows 15–17);
   - `ClipboardOpen` (at most 75 ms);
   - `SelfTest` (debug builds only);
   - `PeekFacts` (DESIGN already admits it);
   - `StoreOpen` (the stores must be read before the first frame; the alternative is (k)6's pre-loop worker).
3. **The recommended splits** of A2b and A2c into two M tickets each.
4. **Path predicates in the vocabulary** ((k)2 item 2): in A2e, after A2b; or left outside C-1.
5. **The panic hook road** ((k)4 rows 5 and 198): (5) read as "destructor or unwind road", or the report
   posted to the sink.
6. **The window-thread content reads** ((k)8): a ticket of their own (Attention's agent files on every
   `AttentionSpoke` drain, the `.git` marker, `$PROFILE`'s offer read, the terminal font change), and where
   `svg_document_options` is first paid.

---

## Revision 2026-09-27 (l), after the Codex review of (k): `Authority` withdrawn, the owner→effect relation completed, the interim-door table completed, and A2b–A2d re-cut so each lands alone

**Read against** `0a176ed3` (`origin/main` when this revision was written: U-28 and A4 merged since (k)'s
`c50adb60`; neither adds or removes an inventory row, a registry line or a `file_reads` site that (k) did not
already count). The inventory is still **226 rows holding 248 sites**. The review is
`trace/tickets-046/thread-door-review-codex-2026-09-27-k.md` (verdict *not yet*: keep the allocation and the
family-first, lint-last plan; do not brief A2b–A2d from (k) unchanged). The coordinator adopted its technical
recommendations as the rulings below. (k) stays as written; where (l) says "amends (k)n", the (l) text is the
one that holds.

Every fact the review names was re-read in the source at `0a176ed3`. Where the source says something the
review does not, (l) says which and why ((l)14). Nothing was built or run for this revision except the
registry gates of its last paragraph.

**What this commit changes outside this note.** The registry `crates/bt-app/src/window_waits.tsv` gains five
`# rows` (24–28, the window-thread waits (k)9 item 4 found) and corrects rows 2 and 8; `docs/DESIGN.md` gains
the dated ruling those five rows cite; `docs/ARCHITECTURE.md` §5.3's generated table follows the registry. No
door type, no `# doors` line, no `# effects` line and no code changes: the doors land with the tickets that
build them.

### (l)1 · F1 — `admission::Authority` is withdrawn; a shared operation has two monomorphic entrances

**Amends (k)1's "The interface: `admission::Authority`" and (k)10(d).** The review is right: a blanket
`&impl Authority` accepts any admitted token at any family door. An admitted `FontFamilyLookup` token could be
handed to `wait::sleep_within` or to a file write; a `&WorkerCtx` could reach a function that is meant to be
owner-only; and borrowing a token lets one admission pay for several unrelated calls. Lifetime, `!Send`/`!Sync`
and mint privacy would survive, but C-5's operation-specific authorisation (the *which* door, not only the
*whether*) would not. Verified in the source: `admission::WaitToken<'scope, D: Door>` is consumed by value at
every typed entrance (`trace_sink::flush(token: WaitToken<'_, doors::TraceFlush>)`,
`launch_wire::hand_over(token, …)`), and `wait::sleep_within(_worker: &WorkerCtx, …)`'s module comment says a
window thread cannot call it. Nothing in (l) weakens either.

**The contract, as it now stands.** Four rules; each site of (k)3 falls under exactly one.

- **R1 · Worker-only sites** go through the family doors on `&WorkerCtx`, as (k) allocated them:
  `file_reads::observe`, `file_writes::*`, `wait::*`, `handles::*`, `quiet_command::*`. A family door is a
  `worker-door-body` `# effects` row with an empty door column ((j)13). No family door has an owner entrance.
- **R2 · Owner-only sites stay where they are**, in the function that holds them today, which takes its
  identity's token and becomes an `# effects` row of kind `interim-owner`, authority `WaitToken<'_, doors::D>`
  for the one `D` named in its door column. The token is taken by value at the identity's entrance (the
  function the `admitted::<D>` closure calls) and by `&` in the helpers below it, as `compose_frame` and
  `handle_surface_failure` take `&WaitToken` under `PresentFrame`. Small lexical bodies are duplicated rather
  than shared: an owner sleep is its own one-line body under its own identity (below), not a call of
  `wait::sleep_within`.
- **R3 · A genuinely shared body** (reached on the window thread and on a worker or a standalone main) is a
  private function holding the site; it is an `# effects` row of kind `shared-body`, and it carries the
  `expect`. It has **exactly two monomorphic entrances**: one taking `&WorkerCtx`, and one taking the precise
  owner token `WaitToken<'_, doors::D>` of the one identity that owns the operation on the window thread. The
  row's authority column names both (`WorkerCtx; WaitToken<D>`); its door column names `D`. The guard holds the
  private body's callers to exactly those two entrances (a third caller is red).
- **R4 · An operation reached under several owner identities is its own identity, admitted nested.** When
  several gestures reach one shared operation (the marks record under `MarksInstall`, `PsReadLineApply` and
  `AgentHooksWrite`; a durable file write under the three stores, `PreviewSave`, the marks record and the agent
  hooks), the operation's owner entrance takes the operation's own token, and each gesture mints it inside its
  own admission at the call (`admitted::<doors::MarksRecord>(|t| …)` within the `MarksInstall` closure). The
  nested mint sites are listed on the operation's `# doors` line like any other; `admitted` already admits a
  nested call on the window thread, and A3's meter counts nested admitted calls' union once
  (`hang_watch::accounting::tests::nested_admitted_calls_count_the_union_once`). **What one admission
  measures:** the gesture's admission measures the gesture, nested operations included; the nested admission
  measures one call of the operation. Where an operation already has an identity (`PsReadLineProbe`,
  `ShellScriptsInstall`, `SchemeCatalogue`, `AgentConfigState`, `ClaimName`, `StoreOpen`), that identity is the
  nested one; two identities are new for this rule, `DurableWrite` and `MarksRecord` ((l)2).

`file_reads`' owner half follows R3 and R4 without a new mechanism: an owner content read is a whole read
(`read`, `read_to_string`, a bounded read) whose owner entrance takes the reading identity's token, completes
inside the admission and returns owned data ((j)8's rule; (l)7 gives the two whole-read APIs the review named).
Because file_reads is one crate's module shared by fifteen new reading identities (and `GpuOpen`, already admitted), its owner entrances are written
as **one list** — one monomorphic function per (identity, form), each a two-line forward to the private counted
body, generated from a single table the way `admission::doors!` generates the door types. An identity absent
from the table cannot read on the window thread; the guard holds the table equal to the `# doors` identities
whose effects name a content read.

**Owner sleeps, one per identity.** `wait::sleep_within(&WorkerCtx, Duration)` stays worker-only and its
`# effects` row keeps its shape: nothing in A2 widens it (this also removes the shared-row mutation F8 found).
The window thread's five sleeps are each an `interim-owner` body under their own token: `DeviceRecovery`'s
(the pilot's wait, passed today as the function value `std::thread::sleep`; a closure borrowing the token takes
its place), `MarksRecord`'s (`profile_marks::lock`'s retry), `TrialClaim`'s (`take_the_claim_within`),
`SelfTest`'s (`run_selftest_if_due`) and `ClipboardOpen`'s (`retry_open_clipboard`'s wait, passed today as
`std::thread::sleep`; a closure borrowing the token takes its place).

**Every genuinely dual-context operation today** — the whole of the sharing, from (k)3's thread column, (k)8's
`file_reads` inventory and the `bt_persist::atomic`/`migrate` callers, re-read on `0a176ed3`:

| # | shared operation (private body) | rows of (k)3 | owner entrance takes | the other entrance serves |
|---:|---|---|---|---|
| S1 | `bt_persist::atomic::{write_temp, commit_rename}` under `atomic_write` and `atomic_replace_preserving` | 97, 98 | `DurableWrite` (R4) | the session writer, `bt-update-check`, the profile workers, the standalone verbs |
| S2 | `bt_platform::file_replace::{replace_file_preserving [not(windows)], carry_metadata [unix], carry_metadata [windows], rename_path [windows], replace_file_preserving_with [windows]}` (reached only through S1's preserving replace) | 118, 119, 150, 167, 168, 169 | `DurableWrite` | as S1 |
| S3 | `bt_persist::migrate::{read_bounded, keep_oversized, keep_rejected}` and the content read inside `read_bounded` | 99, 100, 101 | `StoreOpen` (R4: nested under `StoreReread`'s live rereads ((l)9 splits the two contracts), `TrialWritesReleased`'s refused copies and `LaunchHandOver`'s front-door settings read) | `bt-update-check`; `--explorer-command` |
| S4 | `persist::make_data_folder` | 41 | `StoreOpen` (nested as S3) | the standalone verbs |
| S5 | `persist::relocate` | 42 | `StorageRelocate` (Starting) | the standalone verbs' first `storage_dir` |
| S6 | `profile_marks::{lock, OurTurn::take, Marks::write, Marks::read}` | 69–73 (and S1 below `Marks::write`) | `MarksRecord` (R4; nested under `MarksInstall`, `PsReadLineApply`, `AgentHooksWrite`) | `powershell-profile-{migration,enable,removal}`; `--remove-shell-integration`, `--uninstall-cleanup` |
| S7 | `shell_integration::{replace_profile, read_profile_for_edit}` | 74, 75 | `MarksInstall` | the profile workers; both removal verbs |
| S8 | `shell_integration::install_script_at` (and its content read) | 65, 66 | `ShellScriptsInstall` (nested under `MarksInstall`'s `.ps1` road) | `powershell-profile-migration` |
| S9 | `attention_hooks::{Config::resolve → editable_target, Config::standing}`, `attention_ownership::other_live`, `explorer_menu::same_path` | 8, 9, 20 | `AgentConfigState` (nested under `AgentHooksWrite`'s applies) | `--uninstall-cleanup`; `same_path` also a worker |
| S10 | `attention_hooks::Config::land` | 6, 7 (S1 below it) | `AgentHooksWrite` | `--uninstall-cleanup` |
| S11 | `psreadline::installed_disk::<System as Disk>::{entries, read}` | 49 | `PsReadLineProbe` (nested under `PsReadLineApply`) | `--uninstall-cleanup` |
| S12 | `trace::TraceFile::{open, append}`, `trace_sink::write_here` | 77, 78, 83 | `DiagnosticWrite` | `bt-trace-sink`'s writer; the taskbar lane and the hang watch's lines; see (l)6 for callbacks |
| S13 | `portable_impl::write_std_error` [unix] | 127 | `DiagnosticWrite` (Starting-capable, (l)6) | the trace writer; every standalone verb |
| S14 | `macos_update::points_under` | 113 | `UpdateMounts` (Starting) | `--update-recover`; the update job's worker |
| S15 | `launch_pipe::vet_executable` [unix] | 157 | `LaunchHandOver` (the client half, Starting) | the listener worker |
| S16 | `instance::{canonical_path, try_claim_data_directory [windows]}` | 109, 174 | `ClaimName` (Starting, Running; nested under `StoreOpen`, `TrialClaim`, and the plain start's claim in `fn main`) | `--update-apply` (macOS, the applier's claim tries) |
| S17 | `windows_impl::write_to_console` [windows] | 198 | `DiagnosticWrite` (nested under `LaunchHandOver` for the hand-over's refusal line, Starting; and `FolioApp::fail`'s frame-shape stop announcement) | the standalone verbs; the panic hook's road is (l)5's emergency exception |
| S18 | `bt_platform::install_txn::hold_until` (entered by `try_hold`, no deadline, and `hold_within`) | 108 | `UpdateMounts` (`update_startup::pass`'s `try_hold`; the sleep is in the static multiset and runs zero times without a deadline) | `--update-apply` (macOS, `hold_within`); the update job's worker |
| S19 | `bt_math::svg_document_options`'s first use (the `OnceLock` that loads the system fonts through `file_reads::opaque`; no inventory row, a content read) | — | `SvgFontsFirstUse` ((l)3) | `bt-math-worker` |

Not in this list, and why: `append_panic_report` (row 5) is (l)5's emergency exception; bt-pty's
`SystemShellEnvironment::is_file` (row 215) is `transport` under A2d ((l)11); the three counted-only `Drop`s
(161, 162, 173) are (5). Nothing else in (k)3 runs on both sides.

**The `# effects` authority column** then holds only concrete values: `WorkerCtx`; `WaitToken<D>` for one named
`D`; `WorkerCtx; WaitToken<D>` for a `shared-body`; `none (transport)`; `drop`; `emergency (panic hook)` for
(l)5's two holders. Never a trait.

**What the guard still asserts** (A2s's schema and plants, (l)8). The generic-case list the
review wrote falls away with the generic interface; what remains for the monomorphic case:

1. **A wrong `D` at a shared body's owner entrance** does not compile. Plant (a `compile_fail` doctest, as
   admission's own proofs are written, (c)8): pass `WaitToken<'_, doors::FontFamilyLookup>` to
   `atomic_write`'s owner entrance, whose parameter is `WaitToken<'_, doors::DurableWrite>`.
2. **`WorkerCtx` at an owner-only body** does not compile. Plant: pass a `&WorkerCtx` to
   `diagnostics::rotate_if_oversized`'s `RunLog` parameter.
3. **A shared body has exactly its two entrances.** The guard reads the private body's callers through
   `bt_source` and refuses a third (a plant adds a direct call from a third function). It also refuses a
   `shared-body` row whose two entrances do not take exactly `&WorkerCtx` and `WaitToken<'_, doors::D>` for the
   row's `D`, per `cfg` arm.
4. **A returned deferred effect does not carry its capability away.** An effect function runs its effect inside
   its own call: `every_door_runs_its_effect_inside_its_own_call` is extended from owner doors to every
   `interim-owner`, `shared-body` and `worker-door-body` row, and refuses a site inside a closure or `async`
   block that the function returns or stores rather than calls (plant: a body that returns
   `move || std::fs::rename(..)`; a closure that does not capture the capability is still refused, because the
   rule reads the site's position, not the capture).
5. **Functions that start workers are told apart from synchronous doors** by where the site stands, not by the
   signature. A site lexically inside the closure passed to `admission::spawn_at_priority` /
   `spawn_at_priority_with_stack` belongs to that closure's `&WorkerCtx` parameter (the thread door lends it;
   `every_thread_bt_app_and_bt_platform_start_comes_through_the_thread_door` already pins the starters). A site
   in any other closure that escapes its function — a `'static` callback installed for later
   (`bt_render::set_trace_writer`, `pump::time_messages`, an endpoint's `deliver`) — is refused by item 4: such a
   callback has no capability, and it either offers to a queue ((l)6's capability-free road) or is handed the
   context as a parameter by the worker that calls it. No starter is exempt by name.

`admission`'s mint fence, `door_functions` (literal `WaitToken` parameters) and `worker_door_functions` stay; the
guard gains the `shared-body` and `interim-owner` kinds and the two-entrance check (A2s, (l)8).

### (l)2 · F2 — the owner→effect relation is a per-arm static multiset, and the table below is complete

**Amends (k)5's "effects" column and its witness sentence.** Three quantities were one column in (k); they are
three now, and the table holds only the first:

- **The static multiset** (this table): per `cfg` arm, the effect-function sites in the identity's callee
  closure, each as `row ×n` where `n` is the lexical count the inventory gives that function — whether the site
  executes zero times, once or in a loop on a given call. Nested identities are named, not expanded; their own
  lines hold their sites. It is what the guard can compare with the source.
- **Dynamic counts** (per call, per road): what the meter and the admission record measure. They are not in the
  registry. Where a site repeats or is skipped on the ordinary road, the cell says so in words (e.g. "47 ×1,
  nine times per install"), because a witness must not assert a static count as a dynamic one.
- **Path-specific witnesses**: each drives one real road and asserts what that road does. A witness names its
  road; it does not claim to cover the multiset.

**Every witness has a refusal control.** Each identity's test pair is the admitted road (the record names the
identity and the phase; the road's effect happened) and the refused road (admission refused by a plant that
narrows the door's phases, the way A1d's tests refuse a door: no protected effect ran — asserted on the file
system, the kernel object or the counter the effect would have changed — and the state the refusal cell promises
is observed).

**Where the admission stands.** At the identity's entrance and **before the first state mutation on its road**:
where the walk found a mutation ahead of the effect (a consumed consent, a drained mailbox, an editor taken, an
epoch bumped), the entrance moves above it, and the cell says so. Where the gesture's own non-effect work
mutates presentation state first (a popup closed before a menu's stats), the cell says the admission protects the
observation only.

**The walk.** Every identity's callee closure was walked on `0a176ed3` from its entrances, both `cfg` arms, down to
the inventory's items, the `file_reads` calls, an already admitted door, or a spawn (which starts a worker and is
not the door's). The edges the review named are confirmed; the others the walk found are marked **new** in the
table. Two conventions: rows reached only through `persist::storage_dir`'s first call (42) are static-only on
every road after `fn main` filled its `OnceLock`, and are omitted below except under `StorageRelocate`; **T**
names the chrome rebuild tail (`refresh_chrome` / `refresh_overlay` → `ChromeMarkRasters::resolve_on` →
`marks::rasterize` → `bt_math::rasterize_svg_document`, and `file_peek_layer` → `file_peek_card_layers`) that
almost every gesture ends in: T reaches row 21 (`PeekFacts`, nested, only with a glance card up) and the SVG
fonts' first use (`SvgFontsFirstUse`, nested, once per process). Both are admitted at their own entrances, so a
road through T needs no further token.

**Effects the vocabulary does not list**, seen on these roads and outside C-1 as (k)2 item 2 says:
`Path::{exists, is_file, is_dir}`, `symlink_metadata`, `read_link`, `File::open`, `remove_file`/`remove_dir`,
`OpenOptions::open` + `write_all` (the run log's `append_note`), the Unix claim's runtime directory (`mkdir`,
`lstat`, `chmod`, the lock file's `flock`, a stale socket's `unlink`), `install_txn::durable_remove`'s
`remove_dir_all` in `Starting`, the Windows registry writes, `SHFileOperationW`, `CreateFileW`,
`GetFileInformationByHandle(Ex)`, `ReplaceFileW`, and `file_product_version`'s `GetFileVersionInfoW` (which reads a
whole DLL resource outside `file_reads`). They count toward the owner's list, item 4.

### (l)3 · F3 — the interim-door table, complete: 43 identities, 43 lines

**Amends (k)5's table as a whole** (it stays as written; this one holds). (k)5 had 32 lines for 34 identities (the
three stores shared a line), left `VideoShutdown` outside, and named five content-read doors without identities.
This table has **one line per identity: 43 identities, 43 lines** — (k)5's 34; `VideoShutdown`; the five content
reads (`GitMarkerRead`, `ProfileOfferRead`, `TerminalFontLoad`, `AgentHookRowsRead`, `SvgFontsFirstUse`);
`StoreReread`, split from `StoreOpen` ((l)9); and the two operation identities of (l)1 R4, `DurableWrite` and
`MarksRecord`. Every line is a future `# doors` line and `admission::doors` type, created by the ticket in its
first column ((l)8); the role is `Window` and the capability `WaitToken<'_, doors::D>` throughout. Each line's
multiset is per arm: `all` unless marked `[win]`, `[unix]` (macOS included) or `[mac]`. "N:" names nested
identities, whose sites are on their own lines. "S#" is a shared body of (l)1.

| identity · ticket | entrance, where the admission stands | phases | static multiset (rows ×lexical n) | content reads | refusal: what is preserved | witness · refusal control | owed · version |
|---|---|---|---|---|---|---|---|
| `MarksInstall` · A2c2b | `Runtime::add_to_profile`; `Runtime::spend_powershell_intent` — each immediately around `shell_integration::install_into_profile` (in `spend_powershell_intent`, `offer_for` stays outside it, under `ProfileOfferRead`) | Running | S7: 74 ×1 (a profile exists), 75 ×1 (none exists). N: `ShellScriptsInstall` (the `.ps1` road: 65, 66), `MarksRecord` (69–73), `DurableWrite` (the profile write: 97, 98; [unix] 150, 118, 119; [win] 169, 168) — **new**: 65, 66 and the preserving-replace rows; `SettingsWrite` (`record_powershell_install_pending`, after) | S7's `read_profile_for_edit` ×2 (Settings) | the strip keeps its verb, as after a failed install | `an_add_press_is_admitted_as_marks_install_and_writes_its_profile` · refused: `$PROFILE` and the marks file are byte-identical and the verb is still offered | B4 · 0.4.6 (D-34) |
| `MarksRecord` · A2c2b (new, R4) | nested, around `profile_marks::lock … Marks::write` in `profile_runtime::install_recorded` (`MarksInstall`), `psreadline::install_recorded` (`PsReadLineApply`), `attention_ownership::record` (`AgentHooksWrite`) | Running | S6: 72 ×1, 71 ×1, 73 ×1 (repeats every 20 ms up to `OUR_TURN` = 2 s), 70 ×1 (repeats behind our own writer, no deadline), 69 ×1 (0/1 in `record`). N: `DurableWrite` (97, 98) | S6's `Marks::read` ×1 (Settings) | the enclosing gesture reports it was not applied; no lock file, no marks change | `the_marks_record_is_admitted_nested_under_each_of_its_three_gestures` · refused: no `OurTurn` entry, no lock file, marks byte-identical | B4 · 0.4.6 (D-34) |
| `PsReadLineApply` · A2c2b | `Runtime::apply_psreadline` at its top (before the `psreadline_documents` cache write); `Runtime::create` around `psreadline::upgrade_recorded` (before its reads, which today run ahead of the trial's `defer`); `App::release_trial_writes`'s `PsReadLineUpgrade` arm | Running | own: 47 ×1, 48 ×1 (each nine times per install). N: `PsReadLineProbe` (49, through `installed_copy`, and twice on a removal) — **new**, `MarksRecord` (69–73), `DurableWrite` (97, 98) — **new**; after the outcome `SettingsWrite` (`record_psreadline_invite`) and `PsReadLineProbe` (`refresh_psreadline_installed`) | via N | the Settings row keeps its verb and the toast says it was not applied; at `create` the upgrade is skipped and offered at the next launch | `apply_psreadline_is_admitted_and_the_launch_upgrade_is_too` · refused: the module directory and the marks are byte-identical | B4 · 0.4.6 (D-35) |
| `PsReadLineProbe` · A2c2b | `Runtime::refresh_psreadline_installed`; `Runtime::raise_psreadline_invite_if_due` around `installed_on_probe`; nested under `PsReadLineApply` | Running | S11: 49 ×1 (0 when the directory is absent; per subdirectory when unstamped) | `installed_disk::read` up to 10 per `installed_copy` (Settings) | the row draws the last adopted answer; the cache is not written | `the_installed_copy_walk_is_admitted_on_the_page_open_edge` · refused: the cached answer is unchanged | B5 · 0.4.6 (D-36) |
| `ShellScriptsInstall` · A2c2b | `create_leaf_session` around `Scripts::installed()` (after `psreadline::begin_probe`, which is not its road); `App::release_trial_writes`'s `BashScript`/`ZshScripts` arms; nested under `MarksInstall`'s `.ps1` road | Running | S8: 65 ×1, 66 ×1; own: 67 ×1 (0/1: only when stale), 68 ×1 (three files) | S8's read ×1, `install_zdotdir`'s 1–3 (Settings) | the pane is born without the scripts; `INSTALLED` is not filled, so the next pane tries again | `the_first_pane_writes_its_scripts_under_admission` · refused: no script file is written and the pane is born | B12 · owner |
| `LocaleProbe` · A2c2a | `create_leaf_session` (from `create_tab_state`, `Runtime::split_terminal_seat`, `Runtime::step_pane_text_scale` and the preview's terminal road), on its spawn branch, immediately around `shell_integration::shell_command` — after `psreadline::begin_probe`, `LeafWake::bound_to` and `mint_capability`, which are not its road; the token passes down through `shell_command` to `system_locale_declaration` | Running | [mac] own: 144 ×1 (`quiet_command_text`; the one site runs twice — `defaults read -g AppleLocale`, then `locale -a` — on the process's first spawn, and zero times after, the `OnceLock` being filled); [win] and other Unix: none | none (the `Path::exists` on the ctype file is (k)2 item 2's) | the shell is born without the declaration, as on a machine that does not say is born without the declaration, as on a machine that does not say | `the_first_pane_birth_admits_the_locale_children_once` (Mac) · refused: no child process started (`quiet_command`'s spawn count) | B6 · 0.4.6 (D-39) |
| `WatchStart` · A2c2a | around each start (`DirWatch::{start, start_shallow, start_shallow_named}` → `start_scoped`), at the start's caller: `DirNews::arm` (from `SchemeWatch::arm` and `StorageWatch::arm` in `Runtime::create`, and — **new** — `Runtime::add_scheme` on a turn); the `open` closures of `files_watch::subscribe` (`FilesWatch::sync` ← `Runtime::advance_files_watch`), `git_watch::subscribe` (`GitWatch::sync` ← `advance_git_watch`; one or two starts per root), `preview_watch::subscribe` (`PreviewWatch::sync` ← `advance_preview_watch`, and — **new** — `ask_the_unwatched_preview_files`). (k)'s "`advance_scheme_watch`/`advance_storage_watch`" start nothing and are withdrawn. The departures each `sync_with` drops first are the pinned `Drop`s ((5)), outside the admission | Running | [mac] own: 140 ×1, 141 ×1, 142 ×1, 143 ×2 (at most one of the two joins runs); [win] own: 193 ×1, 194 ×1 (0/1); 195 ×1 through `windows_impl::close` (nine call sites, 0–3 run per start, all on refusal arms; also the pinned `Drop`'s). Departures: 139 [mac], 191, 192, 195 [win] — (5), pinned, not this door's. N: `PreviewStat` (46, `sync_with` stamps each new file first) — **new**; `GitMarkerRead` (`linked_gitdir`) | none of its own | **as a refused start is answered today, per subscriber** — the files watch stores `None`, the preview watch marks the file unwatchable, the git watch stores an empty list (none retries until the entry leaves the wanted set), `DirNews` retries at its next arm, the storage watch never retries. (k)'s "the next sync retries" was true of `DirNews` only | `a_watch_start_is_admitted_and_its_refusal_is_retried` (both platforms) · refused: no watcher thread is spawned | B7, both platforms ((l)10) · 0.4.6 (D-40) |
| `DeviceRecovery` · A2c2a | `FolioApp::recovered_from_a_lost_device` (its one caller is `FolioApp::fail`), after the `device_loss().is_none()` check and before `DeviceLossPilot::answer` (which spends an attempt and surrenders every renderer's target); and — **new** — the debug `FolioApp::surface_selftest_if_due`, which calls `rebuild` directly | Running, Exiting | own: 3 ×1 (the `rest` value handed to `DeviceLossPilot::answer`: runs 0–2 times, 150 + 450 ms; a closure borrowing the token replaces the function value `std::thread::sleep`), 1 ×1 (`<TheDeviceAndItsWindows as LostDevice>::rebuild`'s `block_on`: 0–3 times, each unbounded) | none (`diagnostics::note`'s `append_note` on the road is outside the vocabulary) | the device is not rebuilt and `fail` takes its give-up road (the frame-shape stop) | `a_lost_device_is_recovered_under_its_admission` · refused: no adapter is requested and no sleep is taken | B9 · 0.4.6 (D-42) |
| `SettingsWrite` · A2b2a | `SettingsStore::write_now` (reached from `store`, `release_writes`, `release_trial`) | Running, Exiting | N: `DurableWrite` (`write_settings_atomic`: 98 ×1, 97 ×1) | none | the in-memory value stays and the document is marked unsaved (`DocumentWrites::unsaved`), so the next `store` writes it even when it re-picks the same value (today `wants_write(false)` would make that a no-op) | `a_settings_change_is_admitted_as_settings_write` · refused: `settings.json` byte-identical; the next `store` writes the value | B8 · 0.4.6 (D-47) |
| `KeybindingsWrite` · A2b2a | `KeybindingsStore::write_now` (`store`, `release_trial`) | Running, Exiting | N: `DurableWrite` (`write_keybindings_atomic`: 98, 97) | none | as `SettingsWrite` | `a_keybinding_change_is_admitted_as_keybindings_write` · as `SettingsWrite` | B8 · 0.4.6 (D-47) |
| `ProfilesWrite` · A2b2a | `ProfilesStore::write_now` (`store`, `release_trial`) | Running, Exiting | N: `DurableWrite` (`write_profiles_atomic`: 98, 97) | none | as `SettingsWrite`; B8's retry and snapshot order hold | `a_profile_change_is_admitted_as_profiles_write` · as `SettingsWrite` | B8 · 0.4.6 (D-47) |
| `DurableWrite` · A2b2a (new, R4) | nested at every owner caller of S1: the three stores' `write_now`; **new** — `PinsStore::write_now` (`store`, `toggle`, `release_trial`, reached from `toggle_switcher_pin`) and `OfferState`'s writes (`mark_seen`, `skip`, `release_trial`: `write_update_check_atomic`), `export_settings_to` and `import_schemes` (`atomic_write`, today under the settings station); `Marks::write`; `replace_profile`'s write; `attention_hooks::Config::land`; `PreviewBuffer::save`'s replacement | Running, Exiting | S1: 98 ×1, 97 ×1. S2, by the replacing arm: [unix] 150 ×1, 118 ×1, 119 ×1; [win] 169 ×1, 168 ×1 (0/1: only `restore_replaced`'s road), 167 ×1 (only `atomic_write_carrying`, the preview save's plain arm) | none | `Err` with nothing written: no temp sibling left, the target byte-identical; the caller's own refusal cell holds | `a_durable_write_is_admitted_nested_under_its_caller` · refused: the target's bytes and metadata unchanged, no `.tmp` sibling | its callers' tickets (B4, B8, B12, row 20's document half) · — |
| `PreviewSave` · A2b2b | `Runtime::save_preview_on` (Ctrl+S, the head's button, the float's save) and `Runtime::quit_save` (the quit arm, `answer_dirty_gate`), around `PreviewBuffer::save` / `PreviewPool::save_dirty` | Running, Exiting | own: 43 ×1 (0 on a clean buffer; once on a conflict; twice on success, before and after). N: `DurableWrite` (`atomic_replace_keeping_metadata`: 98 ×1 — twice when the volume cannot preserve —, then [win] 169, 168, or 167 + 97; [unix] 150, 118, 119, or 150 + 97) — **new**: 168, 169. `quit_save` then calls `finish_rename(Blur)`: N: `RenameDisk`, `PathResolve` — **new** | none | the buffer stays dirty and the save is not reported | `a_preview_save_is_admitted_on_the_press_and_on_quit` · refused: the file's bytes and mtime unchanged, the buffer still dirty | row 20's document half, not ticketed (owner ruling 2026-09-25, 2) · owner |
| `RenameDisk` · A2b2b | `Runtime::finish_rename`, **before `self.window.rename.take()`** (Commit, Cancel, blur, focus loss, `close_tab`, `close_window`, the address and web-page roads, quit) | Running, Exiting | own: 56 ×1, 58 ×1. N: `PathResolve` (57 when the new name opens as a page; 91, the web-address arm) — **new**; `PreviewStat` (46, a refused page's source landing) — **new** | none | the rename editor stays open with the old name (the editor is put back, as the FilesNew and web-address arms already do) | `a_rename_commit_is_admitted_as_rename_disk` · refused: the file keeps its name and `window.rename` is `Some` | row 20's document half · owner |
| `DiagnosticWrite` · A2b2a | S12/S13/S17's owner entrances ((l)6): `trace_sink::{stderr_line_owned, file_line_owned}`, `trace::Trace::create`'s eager open, `bt_platform::write_std_error`'s and `write_to_console`'s owner entrances; own: `hotkey::SummonTrace::{opened, write}` (from `QuakeSummoned`, the `summons_wake` closure in winit's message hook on Windows and the Carbon handler on macOS — both on the window thread —, `Quake::reconcile`, macOS `register`); `report_frame_shape_stop`/`announce_stop` ((l)5); nested under `LaunchHandOver` (its refusal line) and `UpdateMounts` (`Machine::say`) | Starting, Running, Exiting | S12: 83 ×1 (**every run's exit** with no sink — **new** road), 77 ×1, 78 ×2 (`BT_GLYPH_CENSUS`: every frame); S13: [unix] 127 ×1; S17: [win] 198's ordinary copy ×1; own: 104 ×1 (once per process), 105 ×1 | none | the line is dropped and counted, as a full sink drops one | (l)6's four witnesses · refused: nothing on the captured stderr, the drop counter up by one | B8 ("diagnostic writes join the trace queue") · 0.4.6 |
| `AgentHooksWrite` · A2b2b | `apply_claude_hooks`, `apply_codex_notify`, `apply_copilot_hooks` (the Settings row and the first-run card's Done, through `apply_settings_choice_announcing`) — each at its top, **before `next_decision` consumes a pending takeover consent** | Running | S10: 6 ×1, 7 ×1 (0/1: only when the file existed and today's backup is absent). N: `AgentConfigState` (S9: 8 ×2 — **new** here —, 9 ×1 per recorded owner, 20 ×1, twice per owner or root; and the row re-read after the apply), `MarksRecord` (69–73 via `attention_ownership::record`, even when unchanged), `DurableWrite` (97, 98 for a new file; [unix] 150, 118, 119 — **new**; [win] 169, 168 — not 167) | S10's `land` re-read 0/1; copilot's `hooks_are_switched_off` (Attention) | the Agents row keeps its verb and `agent_takeovers` keeps its pending consent | `an_agent_hook_install_is_admitted` · refused: the config file, the backup's absence, the marks and `agent_takeovers` unchanged | B12 · owner |
| `AgentConfigState` · A2b2b | `Runtime::create` (the three `row_state`s); `Runtime::refresh_agent_rows` (the agents page's open edge); `Runtime::raise_first_run_if_due` **before `take_ready_edge` sets `first_run_attempted`**; nested under `AgentHooksWrite` | Running | S9: 8 ×2 (0/1 each, only when the path is a link), 20 ×1 (twice per owner, when installed), 9 ×1 (per owner). `raise_first_run_if_due` also: N: `SettingsWrite` (`record_first_run_card`) — **new**; starts `copilot-version-probe` | `standing` ×1 per family, `state`'s read when installed, copilot's `readiness` (Attention) | the row draws its last state (at birth, the rows' initial `Unknown`); the first-run card is raised on a later turn | `the_agent_rows_state_is_admitted` · refused: `first_run_attempted` stays false | B10 (with Q6's content ticket: one job, (l)9) · owner |
| `TrialWritesReleased` · A2b2a | the `AppEvent::TrialWritesReleased` handler, **before `update_trial::take_released()` empties the gate** | Running | own: 2 ×1 (the `DataFolder` arm). N: `StoreReread` (`RefusedCopies`, up to six: 101, 99, 100) — **new**; `SettingsWrite`, `KeybindingsWrite`, `ProfilesWrite`, `DurableWrite` (pins, the update check) — **new**; `ShellScriptsInstall` (65–68) — **new**; `PsReadLineApply` (the upgrade) — **new**; starts `session-writer`, `bt-update-check`, `powershell-profile-migration`, `folio-explorer-probe` | via N | the released writers stay in the gate, untaken, for the next release event (today's "the data folder is created at the next write" lost every other released writer) | `releasing_trial_writes_is_admitted_before_the_gate_is_emptied` · refused: `GATE.released` unchanged, nothing written | B12 · owner |
| `SchemeWrite` · A2b2b | `add_scheme`, `edit_scheme_at`, `delete_scheme_file` (the Settings rows), `runtime::configuration::Runtime::import_schemes` (the import's file pick) | Running | own: 61 ×1 (all four; 0/1 in `import_schemes`), 63 ×1 (`add_scheme` only — (k) listed it for all four). N: `SchemeCatalogue` (`rescan` / `user_documents`: 62 ×1, 60 ×1 per file) — **new**; `DurableWrite` (97, 98 per imported file; `apply_scheme`'s settings write through `SettingsWrite`) — **new**; `WatchStart` (`add_scheme` arms the scheme folder's watch) — **new**; `PreviewStat` (46, `open_scheme_for_editing`) — **new**; `Recycle` (`delete_scheme_file`); `StoreReread` (101, the import's `read_export`) — **new** | via N | the scheme is not written and the toast says so | `adding_a_scheme_is_admitted` · refused: the scheme folder's listing and the catalogue's revision unchanged | B12 · owner |
| `SchemeCatalogue` · A2b2b | `adopt_stored_schemes` (from `create`, `apply_scheme`), `Runtime::reread_schemes` (`advance_scheme_watch`, `import_schemes`), `settings_content`/`settings::scheme_labels` (the first enumeration only), `export_settings_to`; nested under `SchemeWrite` and `Recycle` | Running | own: 62 ×1, 60 ×1 (once per scheme file), on every `rescan`; `catalogue()` only while unset (at `create`). `export_settings_to`'s write: N: `DurableWrite` | `read_scheme_file` once per file (Settings) | the catalogue keeps its last reading: `CATALOGUE` and `REVISION` untouched | `the_scheme_catalogue_is_read_under_admission` · refused: the revision unchanged | B10 (one job with Q6's content ticket) · owner |
| `StoreOpen` · A2b2a (birth) | `Runtime::create`'s six opens: `SessionStore::open`, `SettingsStore::open`, `update::load` (`OfferState::load`), `ProfilesStore::open`, `PinsStore::open`, `KeybindingsStore::open`; nested under `LaunchHandOver` (`say_at_the_front_door`'s settings open); `probe_input` (`BT_PROBE_INPUT`) | Starting, Running | S4: 41 ×1 (per store: five; 0 in a trial). S3: 101 ×1 per document (six), 99 ×1 and 100 ×1 (0/1: a refused file under `Keeping::Now`). N: `ClaimName` ([unix] 109 on every `is_writer_of`; [win] 174, 0 here: `fn main` asked first) | S3's bounded read ×6 (Settings) | **the store opens as a non-writer with its defaults** (`writer_of_record = false`) and the settings-fault card says it was not read — never a writer's defaults, which the next `store` or autosave would write over the real `settings.json`/`session.json` (the walk's finding; (k)'s "opens with its defaults" is withdrawn) | `window_birth_admits_its_store_reads` · refused: after a following `store`, the files are byte-identical | proposed "stays" (owner's list, 2) or B10's pre-loop worker · owner |
| `StoreReread` · A2b2a (new, (l)9) | `Runtime::reread_profiles`, `reread_pins` (from `advance_storage_watch`), `import_settings_from`'s `read_export`, `update::answer_mark` → `OfferState::mark_seen`, `update::skip`; nested under `TrialWritesReleased` (`RefusedCopies`) and `SchemeWrite` | Running | N: `StoreOpen` (S3: 101 ×1, 99 ×1 and 100 ×1, 0/1); `mark_seen`/`skip`: `DurableWrite` (98, 97 when the value changed) | S3's read ×1 (Settings), through `StoreOpen` | the store keeps its adopted value | `a_live_reread_is_admitted_and_its_refusal_keeps_the_adopted_value` · refused: the profile table's revision unchanged | B10 · owner |
| `ClaimName` · A2b2a | nested around `persist::{is_writer_of, is_writer_of_document, is_storage_writer, adopt_claim}`: `fn main`'s ordinary claim before `LaunchHandOver` (**new**, Starting), the store opens, `Runtime::create`, `open_the_data_directorys_endpoints`; under `TrialClaim` | Starting, Running | S16: [unix] 109 ×1 (every call, before the cache lookup; once more on a first ask, inside `try_claim_data_directory`); [win] 174 ×1 (0/1: the first ask, when another Folio holds the name) | none | "not the writer" for this call, recorded as a refusal is recorded today (`None` for the process's life, R-3) — never a cached successful claim, and never "nothing", which would let the process become the writer mid-run | `the_claim_name_is_resolved_under_admission` · refused: no kernel object, lock file or runtime directory made, the table's row `None` | B10 (residue: `fn main`'s one Starting call) · owner |
| `PathResolve` · A2b2b | around each resolution: row 35 `page_destination` (`navigate_preview_page` ← `choose_preview_row`; `switcher_pin_is_allowed` ← `toggle_switcher_pin`; `revived_page_of`'s URL arm); row 52 `revived_page_of` (`revive_web_pages` ← `reopen_recent`, `answer_restore`, `revive_all_web_pages` ← `Runtime::create`, `open_window`, a new window's show, the quake window's first show — **window birth too**); row 57 `open_preview_web_file_on` (`open_preview_web_file`, `open_preview_source_on` — every document door —, `rename_preview_file`); row 91 `WebSeat::go_to` (`finish_rename`'s web-address arm). **Not** `dismiss_web_sheet_on`, which reaches none | Running | own: 35 ×1, 52 ×1 (one of the two per preview pane revived), 57 ×1, 91 ×1. `toggle_switcher_pin`: N: `DurableWrite` (the pins write) — **new** | none | the gesture is refused as for an unresolvable path; a revival leaves the page unrevived | `resolving_a_page_path_is_admitted` · refused: no page is opened and the pins file is unchanged | B10 · owner |
| `PreviewStat` · A2b2b | `Runtime::advance_preview_watch` **at its top, before `due_with` drains the news mailbox and `take_due`**; `ask_the_unwatched_preview_files` (focus regained; `land_preview_source_on`, i.e. every document landing) | Running | own: 46 ×1 (once per newly armed file, per due file, per file in an unwatchable folder). Departures drop watches: 191, 192 [win], 139 [mac] (the pinned `Drop`s, (5)). N: `WatchStart` (arrivals' `subscribe`) — **new** | none | the file is treated as unchanged until the next turn, with the mailbox and the due set intact (as placed in (k), a refusal would have lost them) | `a_preview_stamp_is_admitted` · refused: no false change news and the due set unchanged | B10 · owner |
| `RecentFolders` · A2b2b | `Runtime::toggle_root_menu` around `RecentFolders::refresh` (from `chrome_mouse_input`'s `ChromeTarget::FilesRoot`) | Running | own: 59 ×1 (once per entry, whether the toggle opened or closed the menu) | none | the menu lists the folders unchecked. **The admission protects the observation only**: `close_popups_except(Root)` and `root_menu.toggle` have already run, as a gesture's presentation | `the_root_menu_stats_are_admitted` · refused: no stat, the list as last checked | B10 · owner |
| `FilesRowCase` · A2b2b (row 196: A2c2a) | `Runtime::open_files_row_new` **at its top**, before `state.open.insert(parent)`, `settle_row` and the refresh request (from `floats.rs`'s `NewFile`/`NewFolder`) | Running | [unix] S-: `portable_impl::same_file` 126 ×2 (first ancestor with a cased letter); [win] own: 196 ×1 (only when `CreateFileW` opened) | none | **the platform's existing failure answer**: Windows folds case (`true`, as a failed open or query answers today), Unix is case-sensitive (`false`) — (k)'s "case-sensitive" matched Unix only | `a_new_files_row_asks_case_under_admission` · refused: no row opened, `state.open` unchanged | B10 · owner |
| `Recycle` · A2b2b | `runtime::files::Runtime::delete_files_row` (the float row menu), above `files_trees`; nested under `delete_scheme_file` | Running | [mac] own: 136 ×1. [win] no vocabulary row (`SHFileOperationW`, synchronous, may prompt); other Unix refuses at once | none | the delete is refused, as for a missing file | `a_recycle_is_admitted` · refused: the file is still there | its own request/receipt contract ((l)9) or B12 · owner |
| `PeekFacts` · A2b2b | `Runtime::file_peek_card_layers` (reached through T) | Running | own: 21 ×1 (0/1: the `(Some(path), None)` arm behind `may_read_unasked_through_links`) | none | the card shows no facts | `a_peek_cards_one_stat_is_admitted` · refused: no stat, the card drawn without facts | ruled — DESIGN §7.29 ④, ⑪(d), §7.37 ((l)12) · — |
| `EndpointStart` · A2c2a | `Runtime::create` around `open_the_data_directorys_endpoints` (once per process, the writer only): `attention_wire::open` → `AttentionPipe::start`, `launch_wire::open` → `LaunchPipe::start` | Running | [win] own, per start: 163/179 ×1 (`recv_timeout(5 s)`), 164/180 ×2 (joins, alternative arms: one runs on `Ok(Err)`, the other after `SetEvent` on a timeout, none on success), 165/181 ×2, 166/182 ×1; 162 ×1 (`logon_sid`'s `OwnedHandle` guard, counted-only (5)) — **new**. [unix] own: 148/155 ×2 (only if the listener's spawn fails). N: `ClaimName` (`is_storage_writer`: [unix] 109, and the two socket names, `attention_socket_path`/`launch_socket_path` → `directory_tag` → `canonical_path` — **new**; [win] 174 static, 0 at run time) | none (the Unix runtime directory's `mkdir`/`chmod`, `bind`, `remove_file` are outside the vocabulary) | the endpoint is absent for this run, as when it fails to start; the claim gate is unchanged | `the_endpoints_start_under_admission` · refused: no listener spawned, no socket bound | B11 ((l)10) · owner — row 24 |
| `RunLog` · A2b2a | `diagnostics::enter_resident_run` (after `LaunchHandOver`) | Starting | own: 15 ×1, 13 ×1, 16 ×1, 17 ×1 (0/1: oversized), 14 ×1 ([mac] in practice: the Log channel with a previous log), [unix] 125 ×1 (0 on the Console channel) | none | the run goes on with stderr unredirected (the console) | `the_run_log_opens_under_admission` · refused: no log folder made, stderr still the console | proposed "stays" (owner's list, 2) · owner |
| `StorageRelocate` · A2b2a | `fn main`'s `persist::storage_dir()` (the window road's first call; the standalone first callers — `say_at_the_front_door`, `explorer_menu::serve`, `remove_shell_integration`'s `operate` — take S5's `&WorkerCtx` entrance) | Starting | S5: 42 ×1 (0/1: once per process, Windows only, never in a trial) | none | the old folder is used this run and is what the `OnceLock` holds | `the_one_time_relocate_is_admitted` · refused: the old folder is where it was | proposed "stays" · owner |
| `UpdateMounts` · A2b2a | `update_startup::pass` — **the whole pass**, not `mounts_under` alone | Starting | own: [mac] S14 113 ×1 (per retirement); S18: 108 ×1 (static; runs 0 times: `try_hold` has no deadline). N: `DiagnosticWrite` (`Machine::say`: [unix] 127) — **new** | the journal ×1; the running and the rescue executables, whole, ×0/2 (Install) | the start answers as a pass that cannot hold its installation's admission does today (U-12): nothing of the data directory is touched | `the_startup_update_pass_is_admitted` · refused: the journal's bytes and the entrance unchanged | proposed "stays" · owner |
| `TrialClaim` · A2c2a | `fn main` around `update_trial::take_the_claim` (trials only) | Starting | own: 89 ×1 (every 100 ms while refused, up to `CLAIM_WAIT` = 30 s; 0 if the first try wins). N: `ClaimName` ([win] 174 per held try; [unix] 109 per try and in `adopt_claim`) — **new** [unix]; [win] 173's counted-only `Drop` (0 on the product road) | none | the trial refuses to start (its line under `DiagnosticWrite`, `leave_process(1)`) — its existing refusal | `a_trial_takes_its_claim_under_admission` · refused: no claim taken, the process leaves with 1 | B11 (startup claim ownership) or "stays" · owner — row 28 |
| `MediaQuiet` · A2c2a | `fn main` around `bt_platform::video::shutdown_media_session` (after the loop returns and `admission::exiting()`) | Exiting | [win] own: 184 ×1 (`Readers::quiet_within`'s `wait_timeout`, repeated against one deadline: at most `MEDIA_QUIET_BUDGET` = 1.5 s; the `MFShutdown` after it is outside the vocabulary and unbounded); other platforms: none (`video_portable`'s is empty) | none | quit proceeds, as on the budget's expiry | `the_media_session_quiets_under_admission` · refused: `MFShutdown` still runs, with the line it prints over a reader | D-80's ticket or "stays" · 0.4.7 — row 27 |
| `ClipboardOpen` · A2c2a | the three entrances of `windows_impl::open_clipboard_with_retry`, each around its call: `clipboard_text` (from `Runtime::settings_field_key`, `clipboard_line` ← `keyboard_input`/`palette_key`, `paste_into_preview`); `set_clipboard_text` (from `write_terminal_clipboard_text` — called directly by the copy verbs and handed as a function value to `copy_selection`/`write_selection_text` and `update_card::copy_command` — and directly by `copy_from_graph`, `copy_from_git`, `copy_math_latex`, `copy_text_to_clipboard`); **new** — `WindowsClipboard::begin` (`read_payload` ← `clipboard_payload` ← `Runtime::paste_from_clipboard_into`, from `paste_from_clipboard` and `run_term_menu_row`). The function-value roads pass a closure that borrows the token | Running | [win] own: 197 ×1 (`retry_open_clipboard`'s `wait`, handed `std::thread::sleep` today: 0–4 times, 5 + 10 + 20 + 40 = 75 ms, around five `OpenClipboard` attempts); [mac]: none | none | the clipboard is reported busy, as after the fifth failed open | `a_busy_clipboard_retries_under_admission` · refused: no `OpenClipboard` call | the owner's ruling ("stays" proposed) · owner — row 26 |
| `SelfTest` · A2c2a | `FolioApp::about_to_wait` → `hang_watch::run_selftest_if_due` (debug builds), **before `SELFTEST_FIRED.swap(true)`** and `at(Station::SelfTest)` | Running | own: 92 ×1 (once per process; `BT_HANG_SELFTEST` seconds, no upper limit) | none | no hold, and the test is **not consumed** | `the_selftest_hold_is_admitted` · refused: `SELFTEST_FIRED` still false | proposed "stays" (debug builds only) · owner |
| `VideoShutdown` · A2c2a (new) | at each window-thread caller, **before it removes, replaces or moves a seat**: `Runtime::sweep_video_seats` (from `service_pictures` and `refresh_preview_for_layout`'s nineteen callers), `stop_video_on`, `hide_file_peek` (sixteen callers), `play_video_file_on` (`VideoSeats::open`'s `close`), `promote_file_peek` (`rehome` → `put`; before the carry flag and `hide_file_peek`), `carry_the_recordings_of_moved_panes` (**before its `take` loop**: `put`'s `close`; from `absorb_tab`, `move_pane_across_tabs`, `extract_pane_into_new_tab`). `VideoSeats::{close, open, put, rehome}` are not pinned ((k)5's note said they were); the pinned chain is `VideoSeat::shutdown`, `VideoSeats::shutdown_all`, the two `Drop`s and the engines' `shutdown`, and the `WindowRuntime` drops (`reap_leaving_windows`, `settle_tear_out`, `settle_quit`, `fail`, `exiting`) stay on it, outside the admission | Running | through `VideoSeat::shutdown` → the engine's `shutdown` ((5) bodies, pinned, D-80): [win] 185 ×1, 186 ×1; [mac] 120 ×1, 121 ×1; other Unix: none. Per engine: the 2 ms poll 0–~1000 times (to `SHUTDOWN_BUDGET` = 2 s), then the join 0/1 (after the thread's last `stopped` store: effectively at once, not proven). A sweep that closes N seats is N × 2 s. `open`'s and `promote`'s inner `close` run 0 times on today's roads; `carry`'s only when the destination holds a seat | none | the seat stays where it was — not removed, replaced or moved — and the gesture that wanted it gone does not happen this turn (a doomed seat is swept on a later turn) | `a_video_seat_closes_under_admission` · refused: the seat map unchanged and the engine still running | D-80's ticket · 0.4.7 — row 25 |
| `GitMarkerRead` · A2b2b (new) | `git_watch::GitWatch::subscribe` around `linked_gitdir` (from `Runtime::turn` → `advance_git_watch` → `sync`), nested inside `WatchStart`'s subscribe road | Running | none (content only) | `linked_gitdir`: `read_to_string` ×1 per repository armed (Settings), whole, owned `String` | `None`: only the working tree is watched, as for an ordinary clone | `a_linked_gitdir_is_read_under_admission` · refused: one watch, not two | Q6's content ticket · owner |
| `ProfileOfferRead` · A2b2b (new) | `shell_integration::offer_for`'s callers: `settle_pane_notices` (`offer_once_per_run`, per PowerShell pane when its probe lands) and `spend_powershell_intent` (before `MarksInstall`) | Running | none (content only) | `offer_for`: `read` ×1 (Settings), whole, owned | `integration_offer` stays `None`, so the next turn asks again — never `Owed`, which would spend the once-per-run ask | `the_profile_offer_read_is_admitted` · refused: `integration_offer` is `None` and the ask is not spent | Q6's content ticket · owner |
| `TerminalFontLoad` · A2b2b (new) | `apply_stored_terminal_font` (from `Runtime::create`, `adopt_terminal_font` ← `apply_terminal_font`, `adopt_application_change`, `FontsScanned`) around `GpuContext::set_terminal_font`, **before the font epoch is bumped** | Running | none (content only) | `set_terminal_font`: `opaque` ×2 (Fonts; fontdb reads whole files) | no file is loaded; the family resolves to `DEFAULT_PRIMARY_FONT_FAMILY`, the function's own "family unavailable" answer; the epoch is not bumped | `a_terminal_font_change_loads_its_files_under_admission` · refused: the epoch unchanged | Q6's content ticket · owner |
| `AgentHookRowsRead` · A2b2b (new) | `FolioApp::user_event`'s `AttentionSpoke` arm, **before `attention_wire::take()` drains the inbox** | Running | none (content only) | `attention_{hooks,copilot}::installed_rows`: `read_to_string` each (Attention — not Settings, as (k)8 had it), whole | the inbox is not drained; the messages wait for the next drain (reading empty rows after the drain, as placed in (k), would have lost them) | `the_hook_rows_are_read_before_the_inbox_is_drained` · refused: the inbox still holds its messages | Q6's content ticket · owner |
| `SvgFontsFirstUse` · A2b2b (new) | S19's owner entrance, the window thread's first `bt_math::svg_document_options` (through T: `marks::rasterize` ← `Runtime::dress_new_window`'s first chrome at the first window's birth) | Running | none (content only) | `load_system_fonts` through `opaque` (Fonts), once per process (~100 ms) | the `OnceLock` is **not** initialised: the chrome marks of that frame rasterise without fonts (`svg_options_without_external_images`), and the next ask tries again — initialising it without fonts would strip text from markdown SVGs for the process's life. Its `get_or_init` also blocks this thread while `bt-math-worker` is inside it (and the reverse); stated, not changed | `the_svg_fonts_first_use_is_admitted` · refused: the lock is still unset | the owner (owner's list, 6) · owner |

**Counts.** 43 identities, 43 lines. A2b2 creates 29: 22 of (k)5's 34, plus `StoreReread`, `DurableWrite` and the
five content reads. A2c2 creates 14: (k)5's other 12 (`MarksInstall`, `PsReadLineApply`, `PsReadLineProbe`,
`ShellScriptsInstall`, `LocaleProbe`, `WatchStart`, `DeviceRecovery`, `EndpointStart`, `TrialClaim`, `MediaQuiet`,
`ClipboardOpen`, `SelfTest`), plus `MarksRecord` and `VideoShutdown`. With today's 24 `# doors` lines, the
registry will hold 67 when A2b2 and A2c2 have landed.

**The walk's findings against (k)5**, besides the new edges marked in the table: `import_schemes`,
`edit_scheme_at` and `delete_scheme_file` never reach row 63; `dismiss_web_sheet_on` reaches none of
`PathResolve`'s rows; `advance_scheme_watch`/`advance_storage_watch` start no watch; a refused watch start is not
retried by the next sync (except `DirNews`'s); `VideoSeats::{close, open, put, rehome}` are not pinned; Windows
`directory_folds_case` answers "folds" on failure; `installed_rows` reads on the Attention lane. Seven of (k)5's
admission sites, as placed, came after a state change a refusal would have lost — `StoreOpen` (a writer's
defaults), `TrialWritesReleased` (the emptied gate), `PreviewStat` (the drained mailbox), `RenameDisk` (the taken
editor), `SelfTest` (the fired latch), `AgentHooksWrite` (the consumed consent), `AgentConfigState` (the
first-run latch) — and are moved above it in the table.

**The five unnamed window-thread waits are registry rows, in this commit.** (k)9 item 4 found them; the review
asked for them to be `# rows` with dated rulings before A2b/A2c are briefed. They are rows 24–28 of
`crates/bt-app/src/window_waits.tsv`, each `open`, each citing one dated DESIGN entry (2026-09-27, *five
window-thread waits the thread-door survey found are registry rows 24–28*) whose lines have the shape the review
allows — an open ruling with a repayment assignment:

| row | the wait | where | the ruling line (DESIGN, 2026-09-27) |
|---|---|---|---|
| 24 | the data directory's endpoints start: on Windows each of `AttentionPipe::start` and `LaunchPipe::start` waits `recv_timeout(5 s)` for its listener's first word and, on a refusal or a timeout, joins the listener; on Unix each binds its socket synchronously | `Runtime::create` → `open_the_data_directorys_endpoints` | interim: retained on the window thread, bounded by 5 s per endpoint for the first word plus a join of a listener already told to stop (unbounded in principle: the total is not proven); repaid by B11 (version: owner) |
| 25 | a video seat's engine shut down outside a `Drop`: `video::engine::Engine::shutdown` (Windows) or `macos_player::Engine::shutdown` (macOS) — a 2 ms poll of the engine's stopped flag up to `SHUTDOWN_BUDGET` (2 s), then the join | `VideoSeats::{close, open, put}` from `Runtime::sweep_video_seats`, `stop_video_on`, `hide_file_peek`, `play_video_file_on`, `promote_file_peek`, `carry_the_recordings_of_moved_panes` | interim: retained on the window thread, bounded by 2 s per engine plus a join of a thread that has said it stopped (not proven; a sweep of N seats is N × 2 s); repaid by D-80's ticket (version: 0.4.7, the ledger's; the owner may move it) |
| 26 | the Windows clipboard's open: `retry_open_clipboard` sleeps 5, 10, 20 and 40 ms between five `OpenClipboard` attempts | `windows_impl::{clipboard_text, set_clipboard_text}`, `WindowsClipboard::begin`, from the copy and paste gestures | interim: retained on the window thread, bounded by 75 ms of sleeps and five non-blocking opens; repaid by the owner's ruling (a stay is proposed; if declined, a clipboard-window B-ticket is opened) (version: owner) |
| 27 | the media session's quiet at exit: `Readers::quiet_within(MEDIA_QUIET_BUDGET)`, a `Condvar::wait_timeout` against one deadline, for first-frame readers still inside Media Foundation, before `MFShutdown` | `fn main` → `video::shutdown_media_session`, after the loop returns (Exiting) | interim: retained on the window thread, bounded by 1.5 s (proven by its deadline loop; the `MFShutdown` after it is outside the vocabulary); repaid by D-80's ticket, which owns the media teardown, unless the owner rules it stays like rows 15–17 (version: owner) |
| 28 | an update's trial takes the data directory's claim: `update_trial::take_the_claim_within` tries every 100 ms until the old build lets go | `fn main`, after `enter_window_thread`, before `LaunchHandOver` (Starting), trials only | interim: retained on the window thread before the loop, bounded by `CLAIM_WAIT` (30 s) plus one last try; repaid by B11's startup claim ownership, unless the owner rules it stays like row 18 (version: owner) |

The registry's `where` cells use the same names; `open` is the status of all five, never `ruled to stay`. M9
(`scripts/ci/check-window-waits.ps1`) accepts a new row that is not `pending` and cites a dated `DESIGN.md` ruling.

**Rows 2 and 8, corrected in the same commit.** Row 2's `where` gains the two roads (k)9 item 1 found:
`attention_ownership::record` from the agent hook installs, and `psreadline::install_recorded` from
`Runtime::apply_psreadline`, `Runtime::create`'s `psreadline::upgrade_recorded` and `App::release_trial_writes`.
Row 8's `call` names both platforms: `windows_impl::DirWatch::start_scoped` waits on `listening.recv()` and joins on
its refusal arm, exactly as the macOS one, and both `Drop`s set the stop event and join; its `where` names the
starting roads of `WatchStart`'s line. Neither row's status or disposition changes.

**Counts, stated once:** the registry's `# rows` go from 24 lines (23 numbered rows and 16b) to 29; `# doors`
stays at 24 in this commit; the interim-door table above has 43 identities in 43 lines.

### (l)4 · F4 — rows 221–223 are a preparatory ticket, A2p: the probe leaves the product library

**Amends (k)4 rows 221–223.** The feature route is withdrawn. The review is right on both counts, and the source
agrees: the inventory is `cfg`-blind and a feature condition is `DecidedElsewhere` for `bt_source`, so the
guard's `Src::product_item` still counts a feature-gated body; and `bt-corpus` depends on `bt-render` by path
(`crates/bt-corpus/Cargo.toml`), so C-4's `--workspace --all-targets` build would turn the feature on for the
product library it lints. A feature changes what compiles, not who owns the effect.

**Factual correction to (k)4.** `WindowRenderer::probe_frame` is private; the public road to it is
`bt_render::HeadlessRenderProbe::prepare_frame` (`#[doc(hidden)] pub struct HeadlessRenderProbe`, whose
`prepare_frame` forwards to `self.window.probe_frame(&mut self.gpu, frame)`). `WindowRenderer::read_back` is
public.

#### A2p — the headless render probe is a tool's, not the product library's

| | |
|---|---|
| **target version** | the owner's (0.4.6 with A2, or 0.4.7); before A2e |
| size | S–M |
| who | Opus, local lane (bt-render and bt-corpus builds; no bt-app build beyond its example and one macOS test) |
| depends on | A2a (the inventory and its gate); independent of A2s–A2d and parallel with them |

**True on BASE.** Three bare sites of the inventory stand in `bt-render`'s product library and are reached by no
product caller: `WindowRenderer::probe_frame` (`wgpu::Queue::present` ×1, `wgpu::Queue::submit` ×1; rows 221,
222) and `WindowRenderer::read_back` (`wgpu::Queue::submit` ×1; row 223). Their callers are
`HeadlessRenderProbe::{new, prepare_frame}` (used by `bt-corpus`'s `bt-replay` and `bt-zoom-perf`), bt-render's
own unit tests, `crates/bt-render/tests/glyph_output.rs`, `crates/bt-app/tests/macos_glyph_surface.rs` and
`crates/bt-app/examples/video-probe.rs`. `GpuContext::headless` and `WindowRenderer::offscreen` are the
constructors they use.

**Goal.** Move the effect ownership of the probe to an excluded target with a reviewed API, so that the product
library holds no probe effect:

- A new workspace crate, `bt-render-probe` (a library that is a **tool/test target** by (j)2's forms: it is
  depended on only by `bt-corpus`'s binaries, by `dev-dependencies` and by the example; it is outside the
  product closure C-4 computes from the `folio` binary), owns `HeadlessRenderProbe`, the offscreen frame's
  present/submit (`probe_frame`'s body) and `read_back`'s submit.
- `bt-render` exposes to it the narrow read-only surface those bodies need — the encoder a frame builds and
  the offscreen target — as one `#[doc(hidden)]` module `bt_render::probe_surface` whose functions **return
  work to submit** (a `wgpu::CommandBuffer`, the texture and buffer to copy through) and **do not submit or
  present**. The submit and present move to `bt-render-probe`. That is the reviewed API: its surface is listed
  in the brief's architecture impact, and it holds no vocabulary site (the guard's product scan sees it).
- `glyph_output.rs`, `macos_glyph_surface.rs`, bt-render's unit tests that read back, and `video-probe.rs`
  depend on `bt-render-probe` (as a `dev-dependency` or the example's dependency).

**Tests red on BASE.**

- `the_product_library_holds_no_probe_effect` (bt-app guard): the inventory has no `bt-render` row outside
  `compose_frame` and `handle_surface_failure`; red on BASE by rows 221–223.
- `the_probe_crate_is_outside_the_product_closure` (bt-app guard, through `bt_source`'s workspace): C-4's
  product closure computed from the `folio` binary does not contain `bt-render-probe`, and no product crate's
  `[dependencies]` names it.
- The inventory shrinks by exactly rows 221–223 (the gate's own shrink check, M-series), and nothing is added.

**Proven separately** (the review's ask): (1) the inventory shrink and the two guard tests; (2) green product
builds — `cargo check -p bt-app --all-targets` on Windows (DGX wincheck) and on the Mac mini; (3) green
development builds — `cargo build -p bt-corpus --bins`, `cargo test -p bt-render` (the read-back tests),
`cargo test -p bt-app --test macos_glyph_surface` on the Mac, `cargo build -p bt-app --example video-probe`. A
green product build does not prove the tools still build, and the reverse.

**Out of scope.** No feature flag; no behaviour change to the probe's measurements (the digests `bt-replay`
prints are compared before and after on one corpus file).

**Architecture impact.** (a) none. (b) the probe's GPU submissions leave the product library; a new crate
outside the product closure. (c) none (the rows go with no debt row). (c′) none. (d) no.

### (l)5 · F5 — the panic road is a named emergency exception with two lexical holders

**Amends (k)4 rows 5 and 198.** "(5) read as a destructor or unwind road" is withdrawn: the hook is not a
destructor, and "unwind" is not exact. `std::panic::set_hook`'s closure runs **before** unwinding begins, and it
runs just the same on a panic that aborts (a panic inside a `Drop` during unwinding, a `panic = "abort"`
profile, a panic in a `extern "C"` callback that cannot unwind). The exception is therefore named for what it
is: **the emergency panic report**.

**The two lexical holders** (source at `0a176ed3`):

| holder | effect ×count | why it is the holder |
|---|---|---|
| `bt-app crate::append_panic_report` | `std::io::Write::write_fmt` ×1 (`writeln!` to the panic log opened with `OpenOptions::append`) | the hook closure in `install_panic_log_hook_at` calls it; the raw write is here, not in the closure |
| `bt-platform [windows] crate::windows_impl::write_to_console` | `windows::Win32::Foundation::CloseHandle` ×1 (the `CONOUT$` handle it opened) | `announce_panic` calls it on the hook's fatal road (`install_panic_log_hook`'s `fatal` closure) |

**The allowed incoming paths, pinned** (the guard walks them the way A1e walks a `Drop` chain):

- `install_panic_log_hook_at`'s closure → `append_panic_report` (×1). The closure's other calls — `panic_report`
  (pure), `bt_math::render_panic_is_contained`, the previous hook, `fatal` — are its listed edges.
- `install_panic_log_hook`'s `fatal` closure → `announce_panic` → `bt_platform::write_to_console` (×1, only
  when `diagnostics::a_screen_is_watching`), then `message_box`, `hide_every_window_of_this_process`,
  `leave_process`.

**Ordinary callers are fenced to checked entrances.** Both holders have ordinary callers today:

- `append_panic_report` is also called by `report_frame_shape_stop`, from `FolioApp::fail` on the window thread
  (Running or Exiting), and `fail` hands it `announce_panic` as its `announce` closure. That road stops being a
  caller of either holder: `report_frame_shape_stop` appends its report through `DiagnosticWrite`'s owner
  entrance of S12 ((l)1) and announces through an ordinary `announce_stop` that takes the same token and
  reaches S17's owner entrance; it shares `PANIC_ANNOUNCED`, so the one-alert rule of §7.43 is unchanged. The
  emergency holder keeps exactly one caller, the hook closure; `announce_panic` keeps exactly one caller, the
  `fatal` closure.
- `write_to_console` is also called by `say_at_the_front_door` (the command line's refusal before any window,
  and `launch_wire::hand_over`'s `say` callback inside `LaunchHandOver`), `attention_wire::report`, the
  `--remove-shell-integration` and `--remove-explorer-menu` branches of `fn main`, and `uninstall::run`'s report.
  Those become S17's two checked entrances ((l)1): a `&WorkerCtx` entrance for every standalone road ((l)6 puts
  each of them inside its verb's single entry) and a `DiagnosticWrite` entrance for the hand-over's refusal
  line (nested inside `LaunchHandOver`) and `fail`'s stop. The emergency body and S17's private body are two
  lexical copies of the `CONOUT$` open/write/close; the duplication is the price of keeping the emergency road
  free of any capability, and it is small.

**Exact counts, as the guard pins them:** emergency holders 2; their effects `write_fmt` ×1 and `CloseHandle` ×1
(Windows arm only; the portable `write_to_console` holds no vocabulary effect); incoming edges 2 (the hook
closure, the `fatal` closure through `announce_panic`); callers of each holder 1.

**Why not the queue.** Posting the report to the trace sink would put the process's only record of its crash
behind a writer that may be the stalled thread, or may be killed by the same `leave_process` a moment later.
The report is the case the hook exists for, so it stays synchronous and raw where its pin says it is. This is an
explicit, small extension of (5) — a third category beside the wait-reaching `Drop` exceptions and the
counted-only closes — and not a claim that the `Drop` category already covered it.

**Owner:** A2c2b owns the exception rows (the two `# effects` lines of kind `emergency`, authority
`emergency (panic hook)`), the guard's walk of the two incoming paths, the fencing of the ordinary callers
(`report_frame_shape_stop`'s move to S12, the front door's and the verbs' move to S17's entrances, together with
(l)6), and the two inventory removals (rows 5 and 198). **Owner decision** (the owner's list, item 5): approve
the synchronous emergency road, with its reliability trade-off stated — a crash on a thread that is itself stuck
in a console write can hold that thread in the hook, and nothing else waits for it.

### (l)6 · F6 — startup, diagnostic and standalone effects each have a capability

**Amends (k)1's "Standalone mains", (k)2 item 6 and (k)5's `DiagnosticWrite` line.** (k) gave these roads an
identity only for the Running window thread and left "any thread without a sink" to `Authority`, which (l)1
withdraws. Each calling interface and each early branch, as the source has them on `0a176ed3`:

**The trace interfaces** (`crates/bt-app/src/trace_sink.rs`). Four ways in, and where each writes:

| interface | a run with a sink | a run without one | callers |
|---|---|---|---|
| `stderr_line(String)` | offers to the queue; the `bt-trace-sink` worker writes | `write_line` → `write_here` → `writeln!(stderr)` on the calling thread (row 83) | the window thread (`Runtime` traces, `settings`, the startup line, and the exit's stopped line, budget summary and footer), workers (`taskbar_lane`'s `BT_PERF_TRACE` line), and function-value roads below |
| `file_line(Arc<TraceFile>, String)` | offers to the queue | `write_here` → `TraceFile::append` (row 77) | `trace::Trace::write` and `trace::Dump::line`, the named traces' two writers (every `trace::Gate` and `Dump` static: `BT_IME_TRACE`, `BT_FOCUS_THUMB_DUMP`, `BT_GLYPH_CENSUS`'s `glyph_trace::frame`, …), on the thread that says the line; and `trace::Trace::create`, which opens the file itself (row 78) when `trace_sink::started()` is false |
| `offer_stderr_line(String)` | offers | **drops** (answers `false`) | `diagnostics::note` |
| a function value: `bt_render::set_trace_writer(trace_sink::stderr_line)` (installed in `fn main` after `trace_sink::start`), and `hang_watch`'s `reads.tick(…, crate::trace_sink::stderr_line)` | as `stderr_line` | as `stderr_line`, on whichever thread calls the callback: the window thread for bt-render's and bt-term's lines (through `bt_viewport::trace::line`), the hang watch for the ledger's | — |

A sink exists exactly when a `BT_*` variable naming `TRACE` (or `BT_FOCUS_THUMB_DUMP`) is set
(`a_trace_was_asked_for`) **and** `start` has run. So the sink-less arm runs in the product in four cases:

- **every ordinary run's exit** — `fn main`'s "stopped" line, `hang_watch`'s budget-summary lines and
  `diagnostics::run_footer` go through `stderr_line` on the window thread in `Exiting`, and with no trace variable
  set there is no sink: row 83's everyday road (found by this revision's walk; (k) listed row 83 but not this
  road);
- a line said before `trace_sink::start` (the `Starting` phase above it, and every standalone verb, which never
  starts a sink);
- a switch that writes a trace without naming `TRACE`: `BT_GLYPH_CENSUS` ((k)9 item 6), whose
  `glyph_trace::frame` runs in `Runtime::present_seats_and_commit` after the `PresentFrame` door and writes rows
  77 and 78 synchronously on every frame;
- and nothing else: every renderer, bt-term, taskbar and ledger line is gated by a `*_TRACE` switch, so with the
  callback installed it always meets a sink.

**The design.**

- **The queue road is capability-free.** Offering a `Line` to the bounded queue is not an effect of the
  vocabulary: it never blocks (`try_send`, `try_lock`) and it never writes. The two function-value roads become
  queue-only: `set_trace_writer` and `reads.tick` are handed `trace_sink::offer_line` (new; `stderr_line`'s
  offer half), which offers when there is a sink and **drops and counts** when there is none. That is the
  narrow capability-free road the review allows; it changes no product output (a callback line with no sink is
  unreachable in the product, as above), and in a test binary or `bt-replay` — which install no callback —
  `bt_viewport::trace::line` keeps printing with `eprintln!` as it does today.
- **The sink-less write takes a capability** (S12 of (l)1): `write_here` becomes the private body of two
  entrances, `stderr_line_on_worker(&WorkerCtx, String)` / `file_line_on_worker` and
  `stderr_line_owned(&WaitToken<'_, doors::DiagnosticWrite>, String)` / `file_line_owned`. With a sink both
  entrances only offer. Worker callers already hold their context once A2b1 threads it ((k)9 item 10); the
  window thread's callers mint `DiagnosticWrite` around the line (the station is the door's own; the cost with
  a sink is one offer).
- **`DiagnosticWrite` is Starting-capable**: its phases become `Starting, Running, Exiting`. It is the one
  owner identity for the window thread's console and diagnostic-file writes: `write_here`, `TraceFile::open`
  and `append` (S12), `write_std_error` (S13), `write_to_console` (S17), `hotkey`'s `SummonTrace::{opened,
  write}` (rows 104, 105, owner-only: R2), and `report_frame_shape_stop`/`announce_stop` ((l)5). A separate
  Starting identity was considered and not taken: the station and the refusal are the same, and one identity
  keeps the list of who writes diagnostics from the window thread in one place.
- **`fn main`'s startup trace** (`BT_STARTUP_TRACE: from here Folio talks to …`, after `enter_window_thread`
  and `trace_sink::start`, before the loop) goes through `stderr_line_owned` under `DiagnosticWrite` in
  `Starting`. With the variable set there is always a sink, so its dynamic count of raw writes is 0; its static
  multiset still holds row 83 through the entrance, and the admission is what makes that honest.

**The standalone verbs: one entry each, at the top of the verb, enclosing its reporting.** The current pin
(`window_waits_tests.rs`'s standalone callers) has **four** entries on `0a176ed3`, not three:
`attention_wire::payload_on_stdin`, `explorer_menu::removal_waited_on`, `explorer_menu::cleanup_waited_on`,
`update_apply_macos::run_here`. Each of the first three enters around one wait and leaves the rest of its
process roleless; `run_here` enters around `apply` but reports (`world.say`, a line to stderr) **after** the
entry has returned. Every branch of `fn main` above `enter_window_thread`, and what it does outside an entry
today:

| branch in `fn main` (in order) | early exits and their effects today | after (l) |
|---|---|---|
| `cli::uninstall_cleanup` → `uninstall::run` | `Err(reason)` → `write_std_error` (row 127 on Unix), Unset; `run` enters only inside `cleanup_waited_on`, and reports with `write_to_console` after | `enter_standalone_main("folio-uninstall-cleanup")` around the whole branch, the `Err` arm included |
| `cli::attention` → `attention_wire::run_verb` | `report` → `write_to_console` outside `payload_on_stdin`'s entry; `lede_in_tail`'s read after it | one entry around `run_verb`; `payload_on_stdin` takes the context |
| `cli::explorer_command` → `explorer_menu::serve` | settings read (`SettingsStore::open`: S3, S4), the COM server's pump; no entry | one entry around `serve` |
| `cli::remove_shell_integration` | `remove_shell_integration(Asker::Door)` (S6, S7), `write_to_console` ×2, `write_std_error`; **no entry at all** | one entry around the branch |
| `cli::remove_explorer_menu` | enters inside `removal_waited_on`; `write_to_console` after | one entry around the branch; `removal_waited_on` takes the context |
| `cli::update_door` → `Recover` | `update_recover::run_here`: no entry (its reads are (k)8's standalone `Install` row) | one entry inside `run_here`, around its whole body and its report |
| `cli::update_door` → `Apply` on macOS | `run_here`: entry around `apply` only; `world.say` after | the entry moves up to enclose `world.say` in both arms |
| `cli::update_door` → `Apply` on Windows, or `Err(usage)` | `write_std_error(update_door_refusal)`, Unset | one entry, `"folio-front-door"`, around the refusal line |
| `cli::parse` → `Err(fault)` | `report_at_the_front_door` → `say_at_the_front_door` → `SettingsStore::open` (S3, S4, S5 via `storage_dir`) and `write_to_console` (S17), Unset | the same `"folio-front-door"` entry around the refusal |
| `update_trial::take_the_claim` → `Err(line)` (after `enter_window_thread`) | `write_std_error`, owner `Starting` | `DiagnosticWrite` (Starting) around the line |
| `persist::is_writer_of(&storage)` (after `enter_window_thread`, before `LaunchHandOver`) | the ordinary claim: `claim_name` → `directory_tag` → `canonical_path` (row 109, Unix) and `try_claim_data_directory` (row 174's `CloseHandle` when the name is held, Windows), owner `Starting`, **outside any token** | `ClaimName` (Starting) around the call |

After (l) the pin lists **nine** entries, the verbs themselves: `uninstall_cleanup`, `attention`,
`explorer_command`, `remove_shell_integration`, `remove_explorer_menu`, `update_recover`, `update_apply`
(macOS), and the one front-door entry the two refusal branches share (each runs in its own process, so "once per
process" holds). `enter_standalone_main`'s refusal of a second entry stays; a verb that today entered in a helper
passes the context down instead.

**Owner:** the standalone entries move in A2b1 (the first doors that need them, as (k)7 said); the trace
interfaces and `DiagnosticWrite`'s phases in A2b2a; the panic fencing in A2c2b ((l)5).

**Witnesses** (red on BASE, green after; each with its refusal control):

- **startup** — `the_startup_trace_line_is_admitted_as_diagnostic_write_in_starting`: a test thread entered as
  the window thread, phase `Starting`, no sink; the real `stderr_line_owned` writes one line to a captured
  stderr and the admitted record names `DiagnosticWrite` in `Starting`. Control: the same call in phase
  `Exiting` with `DiagnosticWrite`'s phases narrowed by a plant is refused, writes nothing, and counts one
  refusal.
- **standalone error** — `a_refused_update_door_says_its_line_inside_the_front_door_entry`: the real
  `update_door_refusal` road writes its line while the thread's role is `Worker("folio-front-door")`. Control:
  a second `enter_standalone_main` in the same process is refused and the line is not written twice.
- **worker sink-less** — `a_worker_trace_line_without_a_sink_is_written_through_its_context`: a thread started
  through `spawn_at_priority`, no sink, `stderr_line_on_worker(ctx, …)` writes one line. Control: with a sink,
  the same call writes nothing on the calling thread (the stalled-writer helper `StalledWriter` holds the
  sink) and returns at once.
- **callback** — `a_callback_line_never_writes_on_the_calling_thread`: `offer_line` with no sink writes
  nothing and counts one drop; with `StalledWriter`'s sink it offers and returns at once.

### (l)7 · F7 — the animation re-cut is its own ticket, A2anim; the owner's whole reads return owned data

**Amends (k)8's "One source outlives its worker body" and (k)10(a).** (k) said A2b "re-cuts" the animation source
and that the tickets move no fact's owner. Both are withdrawn. The types settle it (source at `0a176ed3`):
`animation::AnimationSource: Read + Send` ("`Send`, because the cursor crosses to a worker");
`AnimationCursor` owns `reader: Option<gif::Decoder<Box<dyn AnimationSource>>>` and is asserted `Send` by
`is_send`; the cursor travels as `cursor: Box<animation::AnimationCursor>` in `MathWorkerRequest::AnimationFill`
and `DecorationWorkerCompletion::AnimationFill`, and between fills it is parked in the window's animation state
on the window thread. A source that borrows `&'w WorkerCtx` is `!Send` and not `'static`, so it cannot be the
`Box<dyn AnimationSource>` inside that cursor. Keeping it in a table on the worker is the remedy, and it is an
**ownership change**: the open file, the decoder and their retirement move from the window's cursor to the
worker. (k)10(a)'s "moves no fact's owner" is amended to "A2anim moves the animation decoder's owner; nothing
else in A2 does".

#### A2anim — an animation's decoder lives on its worker, and the window holds a key

| | |
|---|---|
| **target version** | the owner's; before A2b1's `file_reads` option (d) lands for the `Animation` lane (A2b1 depends on it) |
| size | M |
| who | Opus, local lane |
| depends on | A1 (the thread door lends `&WorkerCtx`), A5's lane contract; a one-page design note reviewed by Codex first (CONVENTIONS §十 rule 11: it changes who owns a fact) |

**True on BASE.** `FileAnimationSource::open` runs on `bt-math-worker` (through `first_frame` →
`file_source_in_lane`); the cursor that holds it crosses to the window thread in the completion, is parked in
the window's animation entry, and crosses back in each `AnimationFill`; its reads (`Read::read`, counted into
`file_reads::LEDGER`) always run on the worker. A failed send re-parks the cursor or drops it where it stands,
closing the file on whichever thread holds it — the window thread included.

**Goal.**

- The worker's main receive loop owns a table `generation → Decoder` for its whole life (it may borrow the
  loop's `&WorkerCtx`); the window holds only `AnimationKey { path, serial, generation }`.
- `AnimationFill` carries the key and `want`; the completion carries the key and the frames. No decoder, file
  or source crosses a thread.
- **Generation.** Each new playback of a path gets a new generation; a fill for a generation the table does not
  hold (retired, evicted, or from before a file change) answers `Stale` and the window drops it. Today's
  `serial` check (adversarial review 2026-09-11, B10) becomes this.
- **Release.** The window tells the worker to retire a key when the playback ends, its pane or window closes,
  or the ring evicts it; the worker drops the decoder (closing the file) on the worker. A retire for an
  unknown key is a no-op.
- **Failed sends.** A request that cannot be sent (worker gone) retires the playback on the window side with no
  file to close there; a completion that cannot be sent (window gone) makes the worker retire the key itself.
- **Worker failure.** If the worker's loop ends (panic, disconnect), the table goes with it; the window's keys
  answer `Stale` on the next ask and the playback stops as a failed one does today.
- **Bounded retention.** The table holds at most the number of playbacks the ring budget allows; a key past the
  bound retires the least recently filled, and the count of retirements by eviction is in the budget line.
- **Shutdown.** The worker's exit drops the table; nothing on the window thread waits for it.
- **Preserved:** a file change restarts the playback from its first frame (today's `AnimationStamp` check), and
  the ring budget's frame count and bytes are unchanged.

**Tests red on BASE.** `no_animation_decoder_is_dropped_on_the_window_thread` (a counting source's drop
records its thread); `a_fill_for_a_retired_generation_is_stale`; `closing_the_pane_retires_its_decoder_on_the_worker`;
`a_failed_completion_send_retires_the_key_on_the_worker`; `the_decoder_table_is_bounded_by_the_ring_budget`;
`a_changed_file_restarts_its_playback` (green on BASE, kept); `the_cursor_crossing_threads_holds_no_source`
(compile-level: `AnimationFill`'s payload holds no `Box<dyn AnimationSource>`).

**Architecture impact.** (a) the animation decoder's owner moves from the window's cursor to the math worker's
table; its one writer there is the worker loop. (b) no new door; the `Animation` lane's reads keep their
worker. (c) none. (c′) a new source of the window's playback ending: a `Stale` answer. (d) **yes** — the design
note first.

#### The owner's whole-read APIs

Under option (d) as (j)8 wrote it, an owner read completes inside the admission and returns owned data. The two
owner-side reads the review named, and their contracts:

- `bt_persist::migrate::read_bounded(path, cap) -> Result<Vec<u8>, BoundedRead>` stats (row 101), then opens
  through `file_reads::open` and drains with `Read::take(cap + 1).read_to_end`. It becomes S3's private body;
  its owner entrance takes `&WaitToken<'_, doors::StoreOpen>` (nested under `StoreReread` on the live rereads,
  (l)9) and returns the owned `Vec<u8>`. The lazy reader is made and drained inside that call and never leaves it.
- `shell_integration::read_profile_for_edit(profile) -> io::Result<Option<Vec<u8>>>` opens with its own
  `OpenOptions` (read + write, the share mode on Windows) and wraps the file in `file_reads::Reader::new` to
  count. It becomes S7's private body; its owner entrance takes `&WaitToken<'_, doors::MarksInstall>` and
  returns the owned bytes. The `Reader` is consumed inside.

No owner-borrowed reader escapes: `file_reads`' owner half has no function returning a `Reader`, a `File` or an
iterator ((l)1's generated table holds whole reads only). A new streaming owner contract would need its own
review; none is proposed.

**SVG first use stays admitted on its current thread** for the mechanical stage: `bt_math`'s
`svg_document_options` (a `OnceLock` that loads the system fonts on first use) gets the owner identity
`SvgFontsFirstUse` for the window thread's first ask ((l)3) and the worker's `&WorkerCtx` for `bt-math-worker`'s,
and nothing moves. Warming it on the worker at start is an actual move, with late readiness and a fallback to
decide; it is the owner's (the owner's list, item 6), not a parameter edit.

### (l)8 · F8 — one owner per identity and per shared row; A2s first; A2b and A2c re-cut and re-sized

**Amends (k)7 as a whole.** The review is right that (k)7's ownership contradicted itself (`WatchStart` was A2c's
among "identities only waits or handles reach" while rows 140/141 are file observations under it; A2b was told to
create every file-reached identity) and that A2b1's change to the atomics' and `migrate`'s parameters would force
plumbing through A2c's functions. Under (l)1 the contradiction dissolves: an owner-only site stays in its owner
body (R2), so an identity's owner converts its rows whatever their family; only shared bodies (R3) and nested
identities (R4) cross tickets, and each such crossing is named below.

**The rules.**

- **An identity has one owner**: the ticket that creates its `admission::doors` type, its `# doors` line, its mint
  sites, its registry row and its witness pair ((l)3's first column).
- **A row has one owner**: the ticket that removes it from the inventory ((l)13's last column).
- **A shared row has one creator and a serial order of appenders.** The `# doors` lines of the nested identities
  (`DurableWrite`, `ClaimName`, `DiagnosticWrite`, `StoreReread`, `AgentConfigState`, `SchemeCatalogue`,
  `PsReadLineProbe`, `ShellScriptsInstall`, `SettingsWrite`) gain mint sites from later tickets; `file_reads`' owner
  table ((l)1) gains lines from later tickets. Every such append happens in the owner-side order below and nowhere
  else, so no two tickets in flight mutate one line. `wait::sleep_within`'s `# effects` row is not mutated by any
  A2 ticket ((l)1).
- **Capability plumbing is owned with the raw site's removal.** The ticket that removes a row also threads the
  parameter its body needs. A parameter a body needs for a *later* ticket's row is not added early.

**`WatchStart` is A2c2a's, whole.** Its file observations (140, 141) stay in `macos_watch::DirWatch::start_scoped`,
an `interim-owner` body under its own token (R2); no A2b door is involved and no parameter crosses tickets. Its
nested `PreviewStat` (46) and `GitMarkerRead` (`linked_gitdir`) are A2b2b's identities: A2c2a mints nothing for them
— A2b2b's mints stand at `sync_with`'s stamp and at `subscribe`'s read and are reached from inside A2c2a's admission.

**The tickets, sized.** Rows · sites · inventory items (functions), from (l)13's allocation. The review's point
stands: the 24 (5)-rows of the old A2c are **13** `# effects` lines, the six of A2d **five**; the sizes below
count lines of work, not inventory rows alone.

| ticket | owns | rows · sites · items | size | lands after |
|---|---|---|---|---|
| **A2s** (new) | the registry's schema and the guard's shape: `# effects` kinds `shared-body` and `emergency`, the authority values of (l)1, the two-entrance check, the no-deferred-effect extension of `every_door_runs_its_effect_inside_its_own_call`, the spawn-closure attribution, the counted-only `Drop` list (3) beside `EXCEPTIONS` (13), the owner-read table's check (empty allowed); plants 1 and 2 of (l)1 as `compile_fail` doctests over two existing doors (`trace_sink::flush` handed a `FontFamilyLookup` token; handed a `&WorkerCtx`), plants 3 and 4 as recorded mutations | 0 | S–M | A2a |
| **A2p** ((l)4) | the headless probe's move | 3 · 3 · 2 | S–M | A2a; parallel |
| **A2anim** ((l)7) | the animation decoder's owner | 0 (a `file_reads` source, no inventory row) | M, design note first | A1, A5; parallel |
| **A2b1** | `file_reads::observe` and `file_writes::*` on `&WorkerCtx`; `file_reads`' worker forms (option (d)) for every lane but the animation source; the nine standalone entries of (l)6; `WorkerCtx` threaded into every worker body that reaches one of its rows | 35 · 38 · 29 | M | A2s |
| **A2b2a** | the stores and the start: `SettingsWrite`, `KeybindingsWrite`, `ProfilesWrite`, `DurableWrite`, `StoreOpen`, `StoreReread`, `ClaimName`, `RunLog`, `StorageRelocate`, `UpdateMounts`, `DiagnosticWrite`, `TrialWritesReleased`; shared bodies S1–S5, S12–S16, S18; the trace interfaces of (l)6; `file_reads`' owner table (created, with its lines for these identities) | 31 · 33 · 29 | M–L | A2b1 |
| **A2b2b** | the gestures and views: `PreviewSave`, `RenameDisk`, `AgentHooksWrite`, `AgentConfigState`, `SchemeWrite`, `SchemeCatalogue`, `PathResolve`, `PreviewStat`, `RecentFolders`, `FilesRowCase`, `Recycle`, `PeekFacts` and the five content reads; S9, S10, S19; appends its identities' lines to the owner table and its nested mints to `DurableWrite`, `StoreReread`, `SettingsWrite` | 21 · 23 · 20 | M | A2b2a |
| **A2c1** | `wait::*`, `quiet_command::*`, `handles::*` on `&WorkerCtx` for every worker and standalone wait, child process and handle; the hand-off body (row 199, (1)) | 47 · 49 · 42 | M | A2b1; parallel with A2b2a/b |
| **A2c2a** | the owner waits: `LocaleProbe`, `WatchStart`, `DeviceRecovery`, `EndpointStart`, `TrialClaim`, `MediaQuiet`, `ClipboardOpen`, `SelfTest`, `VideoShutdown`; the in-door helpers `flush_sink`, `compose_frame`, `handle_surface_failure` ((1)); rows 183 (in `LaunchHandOver`) and 196 (`FilesRowCase`'s Windows handle, under A2b2b's token); appends nested mints to `ClaimName` and `DiagnosticWrite` | 31 · 39 · 18 | M | A2c1, A2b2b |
| **A2c2b** | the profile family: `MarksInstall`, `MarksRecord`, `PsReadLineApply`, `PsReadLineProbe`, `ShellScriptsInstall`; S6, S7, S8, S11; the 13 `drop-exception` lines and 3 counted-only lines ((l)11); (l)5's emergency exception, S17 and the fencing of the ordinary panic-road callers; appends its identities' lines to the owner table and its nested mints to `DurableWrite` and `SettingsWrite` | 40 · 40 · 23 | M | A2c2a |
| **A2d** | bt-pty only: 12 `transport` lines, the pinned `PtySession` chain's five `drop-exception` lines, (c)6's debt row, row 215's split arm ((l)11) | 18 · 23 · 15 | S–M, unchanged | A2s; **parallel** with everything after it: exclusive bt-pty ownership, no bt-platform edge |
| **A2e** | the lint | 0 | M (L with the owner's list item 4) | all of the above |

Sum: 3 + 35 + 31 + 21 + 47 + 31 + 40 + 18 = **226 rows**, 3 + 38 + 33 + 23 + 49 + 39 + 40 + 23 = **248 sites**.

**The two-M splits of (k) do not hold** once animation, the callback plumbing, the standalone entries, the shared
bodies and the emergency exception are in: A2b is three tickets (M, M–L, M) and A2c three (M, M, M), with A2s in
front and A2p and A2anim beside. The owner may set capacity constraints on that schedule (the owner's list, item
3); the order is the one above.

**Intermediate-green signatures after A2b1 alone** (what compiles and what the guard holds when A2b1 has merged and
nothing after it):

- **New:** `bt_platform::file_reads::observe::{metadata, read_dir, canonicalize}(&WorkerCtx, …)` returning owned
  data; `bt_platform::file_writes::{create_dir_all, write, rename, create, sync_all, write_line}(&WorkerCtx, …)`;
  `file_reads::{read, read_to_string, open, opaque, pipe_output}` and `Reader::new` in their `&WorkerCtx` forms.
- **Kept, unchanged, for the callers A2b1 does not convert**: `file_reads`' capability-free forms, used only by
  the window thread's content reads (converted by A2b2a/A2b2b/A2c2b) and by `FileAnimationSource` (converted by
  A2anim). The guard holds their callers to a closed list that only shrinks; A2e requires it empty.
- **Unchanged:** every shared operation S1–S19 keeps today's signature and its rows stay in the inventory — the
  worker callers of `bt_persist::atomic_write`, `migrate::read_bounded`, `profile_marks::lock` and the rest keep
  calling them without a context until the owner-side ticket creates both entrances at once. So A2b1 forces no
  plumbing through A2c's functions.
- **Changed:** the four standalone entries' bodies take `&WorkerCtx` from the verb's single entry; the pin lists the
  nine verbs.
- **Registry:** new `worker-door-body` lines for the `file_reads::observe` and `file_writes` functions; no owner
  line; the inventory shrinks by A2b1's 35 rows.

**Conflicts to expect, and their resolution.** A2c1 and A2b2a/b run in parallel and both touch worker roots (A2c1
threads wait parameters, A2b2 converts owner roads); the textual overlaps are in `shell_integration`,
`profile_runtime` and `update_*`, and the coordinator's merge check (the standing rules' `cargo check` after a
same-day merge) catches a signature one side changed. `Runtime::create` is touched by A2b2a (the stores),
A2b2b (the agent rows, the scheme catalogue), A2c2a (the endpoints, the watches) and A2c2b (the PSReadLine
upgrade): they land serially in the owner-side order, which is the resolution.

### (l)9 · F9 — B10, re-briefed

**Amends (k)6 as a whole.** Corrections first: (k)6's list has **nine** doors, not ten; `StoreOpen` mixes a
birth read with live rereads and with writes; `macos_files::recycle` stats and then mutates; the observed facts
of `AgentConfigState` and `SchemeCatalogue` come from content reads (k)6 put out of scope; "residue none" for
`ClaimName` skipped its first computation; and a pre-loop store worker contradicts "no new thread".

#### B10 — the window thread's file observation moves to a lane, a cache or a request

| | |
|---|---|
| **target version** | the owner's (0.4.6 or 0.4.7); after A2b2b |
| size | **L** as one ticket; recommended as **B10a** (M: the observation lane's four views and `ClaimName`'s cache) and **B10b** (M: the gesture requests, `FilesRowCase` and `PathResolve`); `StoreOpen`'s birth half is S–M on its own if the owner does not rule it "stays" |
| who | Opus, local lane |
| depends on | A2b2a and A2b2b (the interim identities it repays); A2c2a's `FilesRowCase` Windows road (`directory_folds_case`'s handle, row 196); **the observation lane** — A5 supplies a *partial* lane contract (`bt-app::lane`, R-D's reference), not B5's completed observation lane: B10a either lands after B5 or creates the lane under A5's contract itself, and says which in its brief; Q6's content ticket for the two whole jobs below |

**True on BASE.** Nine interim doors observe files on the window thread, each an `open` registry line owed to
B10: `AgentConfigState` (8, 9, 20), `StoreOpen`/`StoreReread` (41, 99–101), `ClaimName` (109, 174),
`PathResolve` (35, 52, 57, 91), `PreviewStat` (46), `RecentFolders` (59), `SchemeCatalogue` (60, 62),
`FilesRowCase` (126, 196) and `Recycle` (136).

**Goal, per door.**

- **`ClaimName` — a cache with one owner, and its first computation named.** The claim table is keyed by
  `instance::claim_name(directory)`, and on Unix obtaining that key runs `canonical_path` (row 109) on every call,
  before the lookup. The cache is keyed by **the directory as the caller spelled it** (`PathBuf`, not canonical),
  with an alias step: the first ask for a spelling computes the canonical name once (the first miss) and records
  `spelling → name`; a second spelling that canonicalises to a known name joins its row. **First-miss policy:** the
  first computation for the data directory happens in `fn main` in `Starting`, before `LaunchHandOver` — the one
  place the claim must be taken — and stays admitted there as the door's **residue** (`ClaimName`, Starting,
  one call); every later ask in `Running` is a cache hit. Its registry line goes to `done` with that residue, not
  "none".
- **The observation lane, as a versioned latest-value request** (B5's shape) for answers a view draws:
  `PreviewStat`, `RecentFolders`, and the stat half of `AgentConfigState`. The view draws the last adopted answer;
  an older answer is never adopted.
- **Whole jobs, coordinated with Q6's content ticket, not split stat from content.** `AgentConfigState` and
  `SchemeCatalogue` read content to know what they observe (the agent configuration files; every scheme file). A
  lane request that stats on the worker and leaves the read on the owner would still block the owner. So each is
  **one job** on the lane — resolve, read and parse the agent files; list and read the scheme folder — whose answer
  is the parsed state, and the content read moves with it. That is the content ticket's work for these two
  (the owner's list, item 6); B10 takes them only if the owner schedules that ticket with or before it.
- **Gesture requests on the existing lane of the gesture** for answers a gesture needs before it acts:
  `PathResolve` on the hand-off lane's pattern (the path-verify worker already resolves the same paths) and
  `FilesRowCase` on the files worker. The gesture completes when its answer lands; a gesture cancelled first
  does nothing when it lands.
- **`Recycle` is a mutation, not an observation.** `macos_files::recycle` stats the path and then sends it to the
  Trash (Windows: `SHFileOperationW`, synchronous and able to show a prompt). It gets a **mutation request and a
  receipt** — the files worker performs the delete and answers `Recycled | Refused(reason) | Cancelled`, and the
  row and the scheme list change on the receipt — or, if the owner prefers, its own B-ticket. It is listed here
  only because (k) listed it; its contract is B12's shape, not the lane's.
- **`StoreOpen` and `StoreReread` are separate.** The **birth** reads (`Runtime::create`'s six opens) are proposed
  to stay (the owner's list, item 2: they must precede the first frame; their refusal policy is (l)3's non-writer
  open). If the owner declines, they move to a **pre-loop worker started at the top of `fn main`** and adopted by
  `Runtime::create` — which is a new thread, and B10's impact says so. The **live** rereads and imports
  (`reread_profiles`, `reread_pins`, `import_settings_from`'s read, `OfferState::{mark_seen, skip}`) become lane
  requests whose answer is adopted, keeping the adopted value when refused or failed.
- **Writes found on these roads are not B10's.** `migrate::keep_oversized`/`keep_rejected` (rows 99, 100: a
  rejected document kept beside the original) and `make_data_folder` (row 41) are writes; they go to the storage
  lane's writer and its transaction contract (B8 for the stores; B12's transaction section for the rejected copy),
  and B10 only calls them.

**Tests red on BASE.**

- `no_turn_stats_a_path_outside_an_admitted_door` (bt-source: the owner mint sites of the nine identities are
  exactly the residues B10 keeps).
- `a_claim_name_is_computed_once_per_spelling_and_joined_by_alias`.
- `an_older_scheme_catalogue_answer_is_never_adopted`.
- `a_path_resolved_after_its_gesture_was_cancelled_does_nothing`.
- `a_recycle_changes_the_row_only_on_its_receipt`.
- `a_refused_live_reread_keeps_the_adopted_profiles`.

**Docs.** Each door's registry line to `done` with its residue (`ClaimName`: one Starting call); the ledger's B10
row opened at dispatch; a DESIGN entry.

**Architecture impact.** (a) the views' observed-file facts get one writer each: the adopt; the claim cache gets
one owner, `persist`. (b) the observation lane (B5's, or created here under A5's contract), the files, preview and
hand-off workers; **a new pre-loop thread** only if `StoreOpen`'s birth half moves. (c) repays B10's row. (c′) the
rows, menus and catalogues can lag the disk by one answer; a path gesture completes one turn later; a recycle's
row changes on its receipt. (d) no (the claim table's owner is unchanged).

### (l)10 · F10 — B11 gets a brief; B7 covers Windows; the transactional writers are a new ticket, B12

#### B11 — the data directory's endpoints start without the window thread waiting

| | |
|---|---|
| **target version** | the owner's (0.4.6 or 0.4.7) |
| size | M |
| who | Opus, local lane; both platforms' witnesses (the Mac mini for Unix) |
| depends on | A1 and A5 (the lane contract); A2c2a's admitted baseline (`EndpointStart` and `TrialClaim` exist, row 24 and row 28 are `open`); coordinated with D-79 (below) |

**True on BASE** (source at `0a176ed3`). `Runtime::create` → `open_the_data_directorys_endpoints` →
`attention_wire::open` and `launch_wire::open` → `AttentionPipe::start` and `LaunchPipe::start`, on the window
thread before the first frame, each once per process for the writer of the data directory (a non-writer opens
none). On Windows each start spawns its listener, then **`first_word.recv_timeout(5 s)`**; on `Ok(Err)` it
joins the listener and closes the stop event; on a timeout or a disconnect it sets the stop event, **joins** the
listener, and closes the event. The receive has a budget; the join after it has none, so the start's total
bound is not proven to be 5 s — it is 5 s plus a join of a listener that has been told to stop, per endpoint,
two endpoints in a row. On Unix `start` binds the socket and prepares synchronously (names from
`instance::canonical_path`, row 109; two `libc::close` on its failure arms, rows 148/155), with no first-word
receive. Only one of the two lexical joins executes on any one start (they are alternative arms).

**Goal.** The endpoints are **armed** by a request from the window thread and **armed or refused** by an answer
on a lane; the window thread never waits for a listener's first word or its join.

- States: `Unarmed → Arming → Armed | Refused`, plus `Retiring` for an endpoint closed before it was armed.
- **Startup claim ownership.** The claim on the data directory (row 174's road; `persist::is_writer_of`, the
  claim table) stays taken on the window thread before the loop, as today, under `ClaimName`; only a writer
  arms. The trial's claim (`TrialClaim`, row 28) is B11's too: the trial's poll for the old build's claim moves
  to the lane, and the window opens only once it is adopted — or the owner rules it stays (the owner's list,
  item 2).
- **Wake and adoption.** The lane's answer wakes the loop (`EventLoopProxy`), and the window adopts it on the
  next turn; a launch that arrives while `Arming` is queued by the listener as today and delivered once
  `Armed`.
- **Failure and close-before-ready.** A listener that never speaks is `Refused` after its budget, on the lane;
  a window closed (or a quit) while `Arming` retires the attempt on the lane, and the quit does not wait for it.
- **Bounded retention.** At most one stuck start per endpoint is retained; its count is in the budget line.
- **Witnesses (both platforms):** `the_endpoints_arm_without_the_window_thread_waiting` (a listener that
  delays its first word by 3 s: the loop's first frame is not delayed); `a_listener_that_never_speaks_is_refused_on_the_lane`;
  `quitting_while_arming_does_not_wait_for_the_listener`; `a_launch_during_arming_is_delivered_once_armed`;
  and the refusal control: with `EndpointStart`'s admission refused, no listener is spawned and the claim is
  unchanged.
- **Retained semantics until B11 lands:** A2c2a admits the start as it is (`EndpointStart`, the 5 s receive and
  the join unchanged); no A2 ticket shortens a budget.

**D-79.** Asynchronous startup does not repay D-79 (the endpoints' `Drop`s join their listener): the endpoints
live in `OnceLock` statics and are never dropped in the product. B11 states that it leaves D-79 open; an explicit
retire door for an endpoint is D-79's own 0.4.7 ticket.

**Docs.** Registry rows 24 and 28 to `done` (with their residue) or narrowed; the ledger's B11 row opened at
dispatch; a DESIGN entry. **Architecture impact.** (a) the endpoints' readiness gets one writer, the lane's
answer. (b) the endpoints' start moves to a lane. (c) repays rows 24 and 28 (and B11's row). (c′) launches and
attention deliveries can arrive before `Armed` and are queued. (d) no.

#### B7, widened to Windows (established by source; the schedule is the owner's)

`windows_impl::DirWatch::start_scoped` waits on the window thread exactly as the macOS one does: an unbounded
`listening.recv()` for the watcher's first word (row 193), and a `join` (row 194) on its failure arm; its `Drop`
sets the event and joins (rows 191, 192, pinned) and `windows_impl::close` closes the handle (row 195). So B7's
problem is established on both platforms by the source; only when to repay it is the owner's. **B7's brief
changes:** size M (from S–M), with both platforms' witnesses; its R-F additions (armed, refused and
closed-before-armed states; a rescan on armed; callback lifetime; a bound on retained stuck watches, counted in
the budget line) hold per platform; registry row 8 names both arms (corrected in this commit).

#### B8 stays as it is; the transactional writers are B12

B8 (budget note §R-F: settings, keybindings and profiles on the storage lane, admission refusal, a failed last
write, retry and snapshot order, quit's writers) already includes the diagnostic writes; that is not a
widening. The other window-thread writers (k) proposed to add to B8 are not per-file latest-value snapshots, and
B8 is not broadened.

#### B12 — the window thread's transactional writers run on the storage lane

| | |
|---|---|
| **target version** | the owner's |
| size | M–L (the design note included) |
| who | Opus; a design note reviewed by Codex first |
| depends on | B4 (the marks resource and its owner: the storage lane), B8 (the storage lane's writer, its receipts and refusal), A2b2a, A2b2b and A2c2b (the interim identities it repays) |

**True on BASE.** On a turn, the window thread performs whole transactions: the agent hook installs
(`AgentHooksWrite`: resolve, back up, replace preserving, record ownership in the marks), the shell scripts
(`ShellScriptsInstall`: read, compare, write), a scheme's create and delete (`SchemeWrite`, `Recycle`, then a
rescan), and the trial release's writes (`TrialWritesReleased`: the data folder, the held-back store writes,
the scripts, the PSReadLine upgrade).

**The transaction design section** the note must write, per writer:

- **Backup** — which bytes are kept before the write (the hooks' `.folio-backup` sibling; the profile's dated
  copy), where, and when they are removed.
- **Precondition** — what is re-read on the lane before writing (the file's bytes or stamp as the gesture saw
  them), and what a changed precondition answers (refused, not overwritten).
- **Marks lock** — the order of the marks resource (B4) against the file write: the lock before the first byte,
  the record after the last, both on the storage lane; no window-thread wait for another writer's turn.
- **Receipt ordering** — the gesture's row changes state only on the lane's receipt; a receipt for a superseded
  request is dropped; a refused or failed transaction leaves the row's verb as today.
- **Quit** — what an in-flight transaction does at quit (finished within the storage lane's quit budget or
  abandoned with its backup intact).

**Out of scope.** The document rename and the preserving preview save (row 20's document half) keep their own
separately designed scope and the existing receipt ruling; `macos_files::recycle`'s Trash hand-off is (l)9's.

**Architecture impact.** (a) the hook files, scripts, scheme files and the trial's held writes get the storage
lane as their writer. (b) the storage lane. (c) repays the interim identities it names. (c′) the rows' states
change on receipts, one turn later. (d) yes for the marks resource, which B4 already moves; the note covers it.

### (l)11 · F11 — inventory rows are not `# effects` rows; (5) is extended precisely; row 215's `expect`

**Measured** (from (k)3's disposition column, grouped by `(crate, arm, item)`): A2c's **24** inventory rows of
disposition (5) are **13** effect functions — macOS `macos_player::Engine::shutdown` (rows 120, 121); macOS
`macos_watch::DirWatch::drop` (139); Unix `AttentionPipe::drop` (145–147); Unix `LaunchPipe::drop` (152–154);
Windows `AttentionPipe::drop` (158–160); Windows `Overlapped::drop` (161); Windows `OwnedHandle::drop` (162);
Windows `http::Request::drop` (170); Windows `DataDirectoryClaim::drop` (173); Windows `LaunchPipe::drop`
(175–177); Windows `video::engine::Engine::shutdown` (185, 186); Windows `windows_impl::DirWatch::drop` (191,
192); Windows `windows_impl::close` (195) — 24 sites. A2d's **six** (5) rows are **five** functions:
`PtyDump::finish` (204), `PtyDump::publish` (205), `PtySession::shutdown` (209), `join_within` (212, 213),
`reap_within` (214) — 8 sites. The registry's schema is one `# effects` line per function and arm, with its
entries and multiplicities combined, so A2c2b writes 13 `drop-exception` lines, not 24, and A2d five. The ticket
sizes in (l)8 count lines.

**The pinned chains keep their raw effects.** D-78–D-82's bodies stay as A1e pins them, edges and counts
unchanged; no helper is introduced inside a pinned chain and no effect moves into a generic helper. Where a
pinned body also has ordinary non-`Drop` callers, the admission is placed **at those callers**, and the guard
pins those incoming routes:

- `trace_sink::flush_sink`: its non-`Drop` caller is `trace_sink::flush(token: WaitToken<'_,
  doors::TraceFlush>)`; `flush_sink` takes `&WaitToken<'_, doors::TraceFlush>` from it (row 17's door), and
  `Shutdown::drop` mints `TraceFlush` before calling `flush` (A1d, (g)1). Rows 79–81 are (1) in the door.
- the video engines' `shutdown`: `VideoShutdown` at the window-thread callers ((l)3), outside the pinned
  `VideoSeat`/`VideoSeats` drops;
- `windows_impl::close`: reached inside `WatchStart`'s admitted start (its refusal arm) and in the pinned `Drop`;
- `PtyDump::publish`: reached inside `PtyBirth`'s admitted spawn and on transport threads and in the pinned
  `Drop`; A2d's `drop-exception` line for it stands, and the transport label says only that its other roads are
  the transport's.

**The three counted-only closes** (rows 161, 162, 173: `Overlapped::drop`, `OwnedHandle::drop`,
`DataDirectoryClaim::drop`, each `CloseHandle` ×1, no edge) are an **explicit extension of (5)**: a closed list of
three bodies, their exact counts, and no repayment ticket, because a handle closed by its owner's destructor is
the design. They are distinguished in the pinned table from the **thirteen** wait-reaching `Drop` exceptions
(`EXCEPTIONS: [Exception; 13]`), whose D-78–D-82 repayment obligations stand. "A1e's checks untouched" in (k)
means: the thirteen and their `PINNED` bodies are preserved exactly; the counted-only list is added beside them
with its own count (3).

**A2d's publisher.** `spawn_dump_publisher`'s `sync_data` ×2 and `sleep` ×1 (rows 216, 217) are written in the
spawner but execute in the closure it spawns. Its `# effects` line names the closure's executing context
(`transport`, executing in the spawned publisher closure) and the guard's (l)1 item 5 attributes the sites to
the closure; the label does not say they run synchronously in the spawning call.

**Row 215, per `cfg`.** `SystemShellEnvironment::is_file`'s Unix arm holds `std::fs::metadata` ×1 as a
statement under `#[cfg(unix)]`; its Windows arm is `path.is_file()` (outside the vocabulary, (k)2 item 2). An
unconditional `expect` on the method would be unfulfilled on Windows. **Placement:** A2d splits the arms — the
Unix statement moves into a private `#[cfg(unix)] fn unix_executable_file(path: &Path) -> bool` holding the
`metadata` ×1, registered as one `# effects` line with arm `[unix]`, kind `transport`, carrying the `expect`;
the trait method calls it. No conditional `cfg_attr` expectation is admitted, and (k)2 item 3's
"statement-level `cfg` for `# effects` rows" is withdrawn: the guard stays item-level.

### (l)12 · PeekFacts is already ruled

`PeekFacts` (row 21, `facts_of_a_file_the_user_chose`'s one `metadata`) is recorded, not reopened: DESIGN §7.29
rules it — ④ (the card stats its file once per frame, and the size is read off that one stat: no second call,
no worker) and ⑪(d) (the disk is asked on the pointer's move without a cache, behind `is_readable_unasked`) —
and §7.37 restricts it to the one local hover, excluding network paths. (The review cites "§7.29 ⑪"; the
per-frame stat is ④'s sentence and ⑪(d) is the no-cache one; both hold.) Its identity is an `interim-owner` body under R2; its registry line cites that
ruling; it is not in the owner's list.

### (l)13 · The amended allocation: every row whose disposition or door changed

**Amends (k)3 row by row** for the rows below; every other row of (k)3 keeps its disposition and door, and its
ticket becomes the split one: an untouched (2) row of A2b is **A2b1's** (a worker or standalone file site), an
untouched (2) row of A2c is **A2c1's** (a worker or standalone wait, child or handle), (k)'s (1) rows are A2c2a's
(79–81, 218–220) and A2c1's (199), its (5) rows are A2c2b's (24) and A2d's (6), and bt-pty's rows stay A2d's.
Row 215 keeps (1) `transport` under A2d; its effect function becomes the split `[unix]` helper of (l)11.

Dispositions under (l): **(1)** in an existing door's body, 19 rows · 23 sites (unchanged); **(1) `interim-owner`**
(R2, the site stays in its owner body under its identity's token), 54 · 62; **(2)** through a worker family door
(R1), 81 · 86; **(2′) `shared-body`** (R3, a private body with a `&WorkerCtx` and an owner entrance), 37 · 40;
**(5)** retained `Drop` roads and counted-only closes, 30 · 32; **(5′) `emergency`**, 2 · 2 (row 198's site also
has an ordinary copy under S17); **moved** to A2p, 3 · 3. Total **226 · 248**.

| # | item | entry ×count | disposition (l) | owner identity, or shared body · owner entrance | ticket (k) → (l) |
|---:|---|---|---|---|---|
| 1 | `crate::<TheDeviceAndItsWindows as LostDevice>::rebuild` | `pollster::block_on` ×1 | (1) `interim-owner` | DeviceRecovery | A2c → A2c2a |
| 2 | `crate::App::release_trial_writes` | `std::fs::create_dir_all` ×1 | (1) `interim-owner` | TrialWritesReleased | A2b → A2b2a |
| 3 | `crate::FolioApp::recovered_from_a_lost_device` | `std::thread::sleep` ×1 | (1) `interim-owner` | DeviceRecovery | A2c → A2c2a |
| 5 | `crate::append_panic_report` | `std::io::Write::write_fmt` ×1 | (5′) `emergency` | emergency panic report | — → A2c2b |
| 6 | `crate::attention_hooks::Config::land` | `std::fs::create_dir_all` ×1 | (2′) `shared-body` | S10 · AgentHooksWrite | A2b → A2b2b |
| 7 | `crate::attention_hooks::Config::land` | `std::fs::write` ×1 | (2′) `shared-body` | S10 · AgentHooksWrite | A2b → A2b2b |
| 8 | `crate::attention_hooks::editable_target` | `std::fs::canonicalize` ×2 | (2′) `shared-body` | S9 · AgentConfigState | A2b → A2b2b |
| 9 | `crate::attention_ownership::other_live` | `std::fs::metadata` ×1 | (2′) `shared-body` | S9 · AgentConfigState | A2b → A2b2b |
| 13 | `crate::diagnostics::last_written` | `std::fs::metadata` ×1 | (1) `interim-owner` | RunLog | A2b → A2b2a |
| 14 | `crate::diagnostics::newest_crash_report` | `std::fs::read_dir` ×1 | (1) `interim-owner` | RunLog | A2b → A2b2a |
| 15 | `crate::diagnostics::open_run_log` | `std::fs::create_dir_all` ×1 | (1) `interim-owner` | RunLog | A2b → A2b2a |
| 16 | `crate::diagnostics::rotate_if_oversized` | `std::fs::metadata` ×1 | (1) `interim-owner` | RunLog | A2b → A2b2a |
| 17 | `crate::diagnostics::rotate_if_oversized` | `std::fs::rename` ×1 | (1) `interim-owner` | RunLog | A2b → A2b2a |
| 20 | `crate::explorer_menu::same_path` | `std::fs::canonicalize` ×1 | (2′) `shared-body` | S9 · AgentConfigState | A2b → A2b2b |
| 21 | `crate::facts_of_a_file_the_user_chose` | `std::fs::metadata` ×1 | (1) `interim-owner` | PeekFacts | A2b → A2b2b |
| 35 | `crate::page_destination` | `std::fs::canonicalize` ×1 | (1) `interim-owner` | PathResolve | A2b → A2b2b |
| 41 | `crate::persist::make_data_folder` | `std::fs::create_dir_all` ×1 | (2′) `shared-body` | S4 · StoreOpen | A2b → A2b2a |
| 42 | `crate::persist::relocate` | `std::fs::rename` ×1 | (2′) `shared-body` | S5 · StorageRelocate | A2b → A2b2a |
| 43 | `crate::preview::file_mtime` | `std::fs::metadata` ×1 | (1) `interim-owner` | PreviewSave | A2b → A2b2b |
| 46 | `crate::preview_watch::Stamp::of` | `std::fs::metadata` ×1 | (1) `interim-owner` | PreviewStat | A2b → A2b2b |
| 47 | `crate::psreadline::install_checked` | `std::fs::create_dir_all` ×1 | (1) `interim-owner` | PsReadLineApply | A2c → A2c2b |
| 48 | `crate::psreadline::install_checked` | `std::fs::write` ×1 | (1) `interim-owner` | PsReadLineApply | A2c → A2c2b |
| 49 | `crate::psreadline::installed_disk::<System as Disk>::entries` | `std::fs::read_dir` ×1 | (2′) `shared-body` | S11 · PsReadLineProbe | A2c → A2c2b |
| 52 | `crate::revived_page_of` | `std::fs::canonicalize` ×1 | (1) `interim-owner` | PathResolve | A2b → A2b2b |
| 56 | `crate::runtime::files::Runtime::rename_files_row` | `std::fs::rename` ×1 | (1) `interim-owner` | RenameDisk | A2b → A2b2b |
| 57 | `crate::runtime::preview::Runtime::open_preview_web_file_on` | `std::fs::canonicalize` ×1 | (1) `interim-owner` | PathResolve | A2b → A2b2b |
| 58 | `crate::runtime::preview::Runtime::rename_preview_file` | `std::fs::rename` ×1 | (1) `interim-owner` | RenameDisk | A2b → A2b2b |
| 59 | `crate::runtime::profiles::Runtime::toggle_root_menu` | `std::fs::metadata` ×1 | (1) `interim-owner` | RecentFolders | A2b → A2b2b |
| 60 | `crate::schemes::read_scheme_file` | `std::fs::metadata` ×1 | (1) `interim-owner` | SchemeCatalogue | A2b → A2b2b |
| 61 | `crate::schemes::user_dir` | `std::fs::create_dir_all` ×1 | (1) `interim-owner` | SchemeWrite | A2b → A2b2b |
| 62 | `crate::schemes::user_sources` | `std::fs::read_dir` ×1 | (1) `interim-owner` | SchemeCatalogue | A2b → A2b2b |
| 63 | `crate::schemes::write_custom_copy` | `std::fs::write` ×1 | (1) `interim-owner` | SchemeWrite | A2b → A2b2b |
| 65 | `crate::shell_integration::install_script_at` | `std::fs::create_dir_all` ×1 | (2′) `shared-body` | S8 · ShellScriptsInstall | A2c → A2c2b |
| 66 | `crate::shell_integration::install_script_at` | `std::fs::write` ×1 | (2′) `shared-body` | S8 · ShellScriptsInstall | A2c → A2c2b |
| 67 | `crate::shell_integration::install_zdotdir` | `std::fs::create_dir_all` ×1 | (1) `interim-owner` | ShellScriptsInstall | A2c → A2c2b |
| 68 | `crate::shell_integration::install_zdotdir` | `std::fs::write` ×1 | (1) `interim-owner` | ShellScriptsInstall | A2c → A2c2b |
| 69 | `crate::shell_integration::profile_marks::Marks::write` | `std::fs::create_dir_all` ×1 | (2′) `shared-body` | S6 · MarksRecord | A2c → A2c2b |
| 70 | `crate::shell_integration::profile_marks::OurTurn::take` | `std::sync::Condvar::wait` ×1 | (2′) `shared-body` | S6 · MarksRecord | A2c → A2c2b |
| 71 | `crate::shell_integration::profile_marks::lock` | `std::fs::canonicalize` ×1 | (2′) `shared-body` | S6 · MarksRecord | A2c → A2c2b |
| 72 | `crate::shell_integration::profile_marks::lock` | `std::fs::create_dir_all` ×1 | (2′) `shared-body` | S6 · MarksRecord | A2c → A2c2b |
| 73 | `crate::shell_integration::profile_marks::lock` | `std::thread::sleep` ×1 | (2′) `shared-body` | S6 · MarksRecord | A2c → A2c2b |
| 74 | `crate::shell_integration::replace_profile` | `std::fs::File::sync_all` ×1 | (2′) `shared-body` | S7 · MarksInstall | A2c → A2c2b |
| 75 | `crate::shell_integration::replace_profile` | `std::fs::create_dir_all` ×1 | (2′) `shared-body` | S7 · MarksInstall | A2c → A2c2b |
| 77 | `crate::trace::TraceFile::append` | `std::io::Write::write_fmt` ×1 | (2′) `shared-body` | S12 · DiagnosticWrite | A2b → A2b2a |
| 78 | `crate::trace::TraceFile::open` | `std::io::Write::write_fmt` ×2 | (2′) `shared-body` | S12 · DiagnosticWrite | A2b → A2b2a |
| 83 | `crate::trace_sink::write_here` | `std::io::Write::write_fmt` ×1 | (2′) `shared-body` | S12 · DiagnosticWrite | A2b → A2b2a |
| 89 | `crate::update_trial::take_the_claim_within` | `std::thread::sleep` ×1 | (1) `interim-owner` | TrialClaim | A2c → A2c2a |
| 91 | `crate::webhost::WebSeat::go_to` | `std::fs::canonicalize` ×1 | (1) `interim-owner` | PathResolve | A2b → A2b2b |
| 92 | `crate::hang_watch::run_selftest_if_due` | `std::thread::sleep` ×1 | (1) `interim-owner` | SelfTest | A2c → A2c2a |
| 97 | `crate::atomic::commit_rename` | `std::fs::rename` ×1 | (2′) `shared-body` | S1 · DurableWrite | A2b → A2b2a |
| 98 | `crate::atomic::write_temp` | `std::fs::File::sync_all` ×1 | (2′) `shared-body` | S1 · DurableWrite | A2b → A2b2a |
| 99 | `crate::migrate::keep_oversized` | `std::fs::rename` ×1 | (2′) `shared-body` | S3 · StoreOpen | A2b → A2b2a |
| 100 | `crate::migrate::keep_rejected` | `std::fs::write` ×1 | (2′) `shared-body` | S3 · StoreOpen | A2b → A2b2a |
| 101 | `crate::migrate::read_bounded` | `std::fs::metadata` ×1 | (2′) `shared-body` | S3 · StoreOpen | A2b → A2b2a |
| 104 | `crate::hotkey::SummonTrace::opened` | `std::io::Write::write_fmt` ×1 | (1) `interim-owner` | DiagnosticWrite | A2b → A2b2a |
| 105 | `crate::hotkey::SummonTrace::write` | `std::io::Write::write_fmt` ×1 | (1) `interim-owner` | DiagnosticWrite | A2b → A2b2a |
| 108 | `crate::install_txn::hold_until` | `std::thread::sleep` ×1 | (2′) `shared-body` | S18 · UpdateMounts | A2c → A2b2a |
| 109 | `crate::instance::canonical_path` | `std::fs::canonicalize` ×1 | (2′) `shared-body` | S16 · ClaimName | A2b → A2b2a |
| 113 | `crate::macos_update::points_under` | `std::fs::canonicalize` ×1 | (2′) `shared-body` | S14 · UpdateMounts | A2b → A2b2a |
| 118 | `crate::file_replace::replace_file_preserving` | `std::fs::File::sync_all` ×1 | (2′) `shared-body` | S2 · DurableWrite | A2b → A2b2a |
| 119 | `crate::file_replace::replace_file_preserving` | `std::fs::rename` ×1 | (2′) `shared-body` | S2 · DurableWrite | A2b → A2b2a |
| 125 | `crate::portable_impl::redirect_std_streams_to_file` | `libc::close` ×1 | (1) `interim-owner` | RunLog | A2c → A2b2a |
| 126 | `crate::portable_impl::same_file` | `std::fs::metadata` ×2 | (1) `interim-owner` | FilesRowCase | A2b → A2b2b |
| 127 | `crate::portable_impl::write_std_error` | `libc::write` ×1 | (2′) `shared-body` | S13 · DiagnosticWrite | A2c → A2b2a |
| 136 | `crate::macos_files::recycle` | `std::fs::metadata` ×1 | (1) `interim-owner` | Recycle | A2b → A2b2b |
| 140 | `crate::macos_watch::DirWatch::start_scoped` | `std::fs::canonicalize` ×1 | (1) `interim-owner` | WatchStart | A2c → A2c2a |
| 141 | `crate::macos_watch::DirWatch::start_scoped` | `std::fs::metadata` ×1 | (1) `interim-owner` | WatchStart | A2c → A2c2a |
| 142 | `crate::macos_watch::DirWatch::start_scoped` | `std::sync::mpsc::Receiver::recv` ×1 | (1) `interim-owner` | WatchStart | A2c → A2c2a |
| 143 | `crate::macos_watch::DirWatch::start_scoped` | `std::thread::JoinHandle::join` ×2 | (1) `interim-owner` | WatchStart | A2c → A2c2a |
| 144 | `crate::quiet_command_text` | `std::process::Command::output` ×1 | (1) `interim-owner` | LocaleProbe | A2c → A2c2a |
| 148 | `crate::attention_pipe::AttentionPipe::start` | `libc::close` ×2 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 150 | `crate::file_replace::carry_metadata` | `std::fs::metadata` ×1 | (2′) `shared-body` | S2 · DurableWrite | A2b → A2b2a |
| 155 | `crate::launch_pipe::LaunchPipe::start` | `libc::close` ×2 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 157 | `crate::launch_pipe::vet_executable` | `std::fs::metadata` ×2 | (2′) `shared-body` | S15 · LaunchHandOver | A2b → A2b2a |
| 163 | `crate::attention_pipe::AttentionPipe::start` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 164 | `crate::attention_pipe::AttentionPipe::start` | `std::thread::JoinHandle::join` ×2 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 165 | `crate::attention_pipe::AttentionPipe::start` | `windows::Win32::Foundation::CloseHandle` ×2 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 166 | `crate::attention_pipe::AttentionPipe::start` | `windows::Win32::System::Threading::SetEvent` ×1 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 167 | `crate::file_replace::carry_metadata` | `std::fs::metadata` ×1 | (2′) `shared-body` | S2 · DurableWrite | A2b → A2b2a |
| 168 | `crate::file_replace::rename_path` | `std::fs::rename` ×1 | (2′) `shared-body` | S2 · DurableWrite | A2b → A2b2a |
| 169 | `crate::file_replace::replace_file_preserving_with` | `std::fs::metadata` ×1 | (2′) `shared-body` | S2 · DurableWrite | A2b → A2b2a |
| 174 | `crate::instance::try_claim_data_directory` | `windows::Win32::Foundation::CloseHandle` ×1 | (2′) `shared-body` | S16 · ClaimName | A2c → A2b2a |
| 179 | `crate::launch_pipe::LaunchPipe::start` | `std::sync::mpsc::Receiver::recv_timeout` ×1 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 180 | `crate::launch_pipe::LaunchPipe::start` | `std::thread::JoinHandle::join` ×2 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 181 | `crate::launch_pipe::LaunchPipe::start` | `windows::Win32::Foundation::CloseHandle` ×2 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 182 | `crate::launch_pipe::LaunchPipe::start` | `windows::Win32::System::Threading::SetEvent` ×1 | (1) `interim-owner` | EndpointStart | A2c → A2c2a |
| 183 | `crate::process_image_path` | `windows::Win32::Foundation::CloseHandle` ×1 | (1) `interim-owner` | LaunchHandOver | A2c → A2c2a |
| 184 | `crate::video::Readers::quiet_within` | `std::sync::Condvar::wait_timeout` ×1 | (1) `interim-owner` | MediaQuiet | A2c → A2c2a |
| 193 | `crate::windows_impl::DirWatch::start_scoped` | `std::sync::mpsc::Receiver::recv` ×1 | (1) `interim-owner` | WatchStart | A2c → A2c2a |
| 194 | `crate::windows_impl::DirWatch::start_scoped` | `std::thread::JoinHandle::join` ×1 | (1) `interim-owner` | WatchStart | A2c → A2c2a |
| 196 | `crate::windows_impl::directory_folds_case` | `windows::Win32::Foundation::CloseHandle` ×1 | (1) `interim-owner` | FilesRowCase | A2c → A2c2a |
| 197 | `crate::windows_impl::open_clipboard_with_retry` | `std::thread::sleep` ×1 | (1) `interim-owner` | ClipboardOpen | A2c → A2c2a |
| 198 | `crate::windows_impl::write_to_console` | `windows::Win32::Foundation::CloseHandle` ×1 | (5′) `emergency` + (2′) `shared-body` | emergency panic report; S17 · DiagnosticWrite | — → A2c2b |
| 221 | `crate::WindowRenderer::probe_frame` | `wgpu::Queue::present` ×1 | moved (A2p) | A2p | — → A2p |
| 222 | `crate::WindowRenderer::probe_frame` | `wgpu::Queue::submit` ×1 | moved (A2p) | A2p | — → A2p |
| 223 | `crate::WindowRenderer::read_back` | `wgpu::Queue::submit` ×1 | moved (A2p) | A2p | — → A2p |

### (l)14 · Where the source and the review part, and what (l) disputes

Every fact the review names was checked in the source on `0a176ed3`. It is right on each, with these refinements:

- **Row 215's Windows arm** (review, row 215): `path.is_file()` is outside the vocabulary, as (k)2 item 2 says; not a
  missed site. Agreed with the review's placement concern; (l)11 splits the arm.
- **`announce_panic` has an ordinary caller too**: `FolioApp::fail` hands it to `report_frame_shape_stop` as its
  `announce` closure. The review named `append_panic_report`'s and `write_to_console`'s ordinary callers but not
  this one; (l)5 fences all three.
- **"§7.29 ⑪"** (review, PeekFacts): the per-frame stat is §7.29 ④'s sentence; ⑪(d) is the no-cache one; both hold
  ((l)12).
- **The pinned video functions** (review F3): `VideoSeats::shutdown_all` is pinned (a body of `VideoSeats`' `Drop`
  chain); `VideoSeats::{close, open, put, rehome}` are not — (k)5's note said they were, and the review's "admission
  placed before a seat is removed or replaced" is therefore possible inside the callers without touching a pin.
- **`EndpointStart`'s two joins** (review F2): confirmed as alternative arms; in addition the Windows start drops
  `logon_sid`'s `OwnedHandle` guard (row 162, counted-only) on every start, which neither (k) nor the review listed.
- **`UpdateMounts`**: the review's "must enclose the journal/executable reads" holds; the walk also found row 108
  (`install_txn::hold_until`, static only: `try_hold` has no deadline) and row 127 (`Machine::say`) on the pass,
  and that the retirement removes the entrance before it detaches — so the admission stands at `pass`, whole.
- **Row 83 runs on every ordinary run's exit** (the stopped line, the budget summary, the footer, with no sink):
  the review said `write_here` "can be called without a sink on various executing threads"; the everyday road is
  the exit's, on the window thread in `Exiting` ((l)6).

Nothing the review states as a fact was found wrong; nothing of (k) is kept against it.

### (l)15 · This revision's own architecture impact, and the owner's list

**Impact** (this commit: docs and five registry rows).

- **(a)** None by this commit. The tickets it briefs: **A2anim moves the animation decoder's owner** (from the
  window's cursor to the math worker's table; design note first); B10 gives the views' observed facts one writer
  each and the claim cache one owner; B11 gives the endpoints' readiness one writer; B12 gives the transactional
  files the storage lane as writer. Nothing else in A2 moves a fact's owner. (k)10(a) is amended accordingly.
- **(b)** This commit: five window-thread waits become registry rows (24–28), each with an open ruling; no door
  yet. The tickets: the worker family doors on `&WorkerCtx`; 43 owner identities (29 by A2b2a/b, 14 by A2c2a/b);
  19 shared bodies with two entrances each; `file_reads`' owner table; the emergency panic exception; nine
  standalone entries; a new tool crate (A2p).
- **(c)** This commit opens no ledger row; the five `# rows` are open interim rulings. At dispatch: A2p (none),
  A2anim (none), B10 (amended), B11, B12, (c)6's transport debt row (A2d). D-78–D-82 stay open, their pins
  preserved.
- **(c′)** None by this commit. The tickets add two new sources: a `Stale` answer ends an animation's playback
  (A2anim), and a refused admission is a new, documented source of each identity's refusal state ((l)3's column).
- **(d)** Yes, as amendments to the note's own contract: (k)1's `Authority` is withdrawn and replaced by (l)1's
  four rules (C-5's operation-specific authorisation kept; nested operation identities added); (5) is extended by
  the three counted-only closes and by the emergency panic report ((l)5, (l)11); (k)7's ownership is replaced by
  (l)8's. Codex reads (l) before A2b–A2e are briefed.

**The owner's list** (replaces (k)10's "Decisions for the owner"). Each item: the alternatives, what each costs,
the recommendation.

1. **Versions and release placement of the new work.**
   - *A2s, A2p, A2anim* — (i) 0.4.6 with A2b–A2d; (ii) 0.4.7. A2s is small and blocks everything after it: 0.4.6.
     A2p is independent and small: either; it must precede A2e. A2anim is an ownership change with a design note
     (M): 0.4.6 only if A2 as a whole is 0.4.6, since A2b1's `file_reads` option (d) keeps one capability-free
     form alive for it until it lands. **Recommend:** all three in the release A2 lands in.
   - *B11* — (i) 0.4.6: removes up to 10 s plus two joins from a slow start; M; (ii) 0.4.7 with D-79's endpoint
     retirement: one ticket's worth of design for both. **Recommend 0.4.7, beside D-79**, the start staying
     admitted (row 24) until then.
   - *B12* — (i) with B8 and B4 (they share the storage lane and the marks resource); (ii) after them. **Recommend
     after B4 and B8**, the same release or the next: its transaction design needs both.
   - *D-80* stays 0.4.7 (the ledger's); rows 25 and 27 are owed to its ticket. Moving it is the owner's call; no
     change recommended.
   - *B7 on Windows* — established by source ((l)10); only its schedule is the owner's: with the macOS half
     (recommended: one ticket, both platforms, M) or after it.
2. **The stays that need dated rulings**, each with the bound it proposes (each is `open` today; a "stays" ruling
   turns its line into `ruled to stay`):
   - `RunLog` — before the loop; bounded by one directory create, two stats and a rename. Alternative: open the log
     on a worker and redirect late, losing the first lines' destination. **Recommend stay.**
   - `StorageRelocate` — once per process, Windows only, before the loop; one rename. Alternative: none cheaper.
     **Recommend stay.**
   - `UpdateMounts` — the startup update pass, before the loop; bounded by its reads (the journal; the two
     executables when preparing) and one mount listing per retirement. Alternative: move the retirement to the
     update job's worker, deferring it by one launch's worth. **Recommend stay.**
   - `TrialClaim` (row 28) — before the loop, trials only; bounded by 30 s. Alternative: B11's startup claim
     ownership. **Recommend stay** (a trial is rare and has no window yet), or fold into B11 if it lands.
   - `MediaQuiet` (row 27) — at exit; bounded by 1.5 s. Alternative: D-80's ticket. **Recommend stay**, as rows
     15–17.
   - `ClipboardOpen` (row 26) — a gesture; bounded by 75 ms. Alternative: a hidden clipboard-owner window on a
     worker (new B-ticket, M). **Recommend stay.**
   - debug `SelfTest` — debug builds only, `BT_HANG_SELFTEST` seconds by design. **Recommend stay.**
   - `StoreOpen`'s birth reads — before the first frame; bounded by six bounded reads. Alternative: B10's pre-loop
     worker (a new thread). **Recommend stay.**
   (`PeekFacts` is already ruled; not on this list.)
3. **Scheduling constraints on the splits** — the order is fixed by ownership ((l)8); the owner may cap how many
   of A2c1 / A2b2a–b / A2d / A2p / A2anim run at once on the two local lanes. **Recommend:** A2d and A2p beside the
   A2b chain; A2c1 beside A2b2a.
4. **C-1's path vocabulary** (`Path::{exists, is_file, is_dir, try_exists, symlink_metadata}`, `fs::symlink_metadata`,
   and the others the walk saw, (l)2) — (i) expand now: A2e grows to L and every added effect needs a probe, a
   disposition and a route (~205 text hits, not a product census); (ii) state them outside C-1 (they are, today, a
   stated limitation). **Recommend (ii) for A2, and a follow-up ticket after A2e** that takes the census and routes
   them through the doors A2 built.
5. **The emergency panic road** — (i) the synchronous exception of (l)5: the crash record is written even when the
   process dies next; a panicking thread stuck in a console write holds only itself; (ii) a queued report through
   the trace sink: no exception, but the record is lost exactly when it matters. **Recommend (i).**
6. **Window-thread content reads and SVG first use** — (i) a content-read ticket (the five identities of (l)3 plus
   the whole `AgentConfigState`/`SchemeCatalogue` jobs of B10): each read moves to its lane, and the view draws a
   late answer (a font change applies a turn later; the agent rows and scheme list lag one answer; a linked
   gitdir's second watch arms a turn later); (ii) leave them admitted on the window thread. For SVG: (i) warm
   `svg_document_options` on `bt-math-worker` at start, the chrome's first marks drawn without fonts until it lands;
   (ii) keep first use where it falls, admitted. **Recommend (i) for the content reads, in the release after A2**,
   and **(ii) for SVG** until a measurement says the ~100 ms first use is felt.
