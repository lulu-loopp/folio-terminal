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
