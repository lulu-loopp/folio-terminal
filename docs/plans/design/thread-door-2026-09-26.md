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
  in 17 owners (the note's 20, at `78a3699a`), `extern` blocks 6, `#[link]` **5**
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
