//! **Which kind of thread this is, and whether the window thread may wait here now.**
//!
//! The owner of three facts no code owned before (`docs/plans/design/thread-door-2026-09-26.md`,
//! the ruling copy with its revisions (b)–(f); `docs/ARCHITECTURE.md` §5.1):
//!
//! - **a thread's role** — [`Role`], one thread-local, written only by the entries below;
//! - **the window thread's phase** — [`Phase`], a thread-local meaningful only where the role is
//!   [`Role::Window`], written only by the four phase writers;
//! - **the door registry** — [`doors`], one uninhabited type per line of
//!   `crates/bt-app/src/window_waits.tsv`, each carrying its §5.3 row, its `hang_watch` station
//!   and the phases it is admitted in.
//!
//! An owner-thread wait is admitted through [`admitted`], which checks the role and the phase in
//! every build, hands the work a [`WaitToken`] for exactly that door, and measures the call through
//! the [`Meter`] `hang_watch` installs once. A refusal does not run the work, is counted, and is
//! returned as [`Refused`] for the caller to handle as it handles the door's own error.
//!
//! **The thread door lives here too** ([`spawn_at_priority`], re-exported at the crate root): every
//! thread it starts is [`Role::Worker`] by its name and its body is lent a [`WorkerCtx`], the
//! capability a worker-only door takes (A1b). The one such door today is the hand-off's
//! (`ShellThread::enter`).
//!
//! **What is not here yet, said so that nobody reads more into it:** no owner-thread door takes a
//! token yet (A1d converts them), and the threads started outside the door — `bt-platform`'s and
//! `bt-app`'s bare spawns (A1c), `bt-pty`'s four and `bt-term`'s resample pool (by design) — are
//! still [`Role::Unset`].
//!
//! # What the compiler proves, and how the proofs are written
//!
//! The owner-thread token is generative, `!Send`, `!Sync`, not `Copy`, not constructible outside
//! this module and parameterised by its door. Each of those is proved by a `compile_fail` block
//! beside a control that differs from it by the one statement the property is about; each block
//! names, in a `MUTATION:` line, the one edit to this module that would make it compile. **On the
//! pinned stable toolchain rustdoc does not check a `compile_fail` block's error code** (it turns
//! that check on only for a nightly build), so a block proves that it fails and no more; the
//! control is what shows the failure is the statement's. The block below is the canary for that
//! claim: it fails with a type mismatch and is tagged with an unrelated code on purpose, so it
//! passes only where codes are not checked — the day it goes red, the codes on every other block
//! here have started to mean something.
//!
//! ```compile_fail,E0599
//! // The wrong-code canary: this fails with E0308, not E0599.
//! let byte: u8 = "not a byte";
//! ```
#![forbid(unsafe_code)]

use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

// ---------------------------------------------------------------------------
// The role
// ---------------------------------------------------------------------------

/// **What kind of thread the calling thread is.**
///
/// `Unset` is never a worker and never the window: it is every thread nothing has named yet — a
/// thread started outside the thread door, the main thread of a door process, and an OS thread
/// outside a callback scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Unset,
    /// The thread that runs `fn main` in a launch that got past the argument parse.
    Window,
    /// A thread the thread door ([`spawn_at_priority`]) started, or a standalone process's main thread
    /// ([`enter_standalone_main`]), by its name.
    Worker(&'static str),
    /// An OS-owned thread running one of our callbacks, by the callback's name, for the length of
    /// its [`CallbackScope`].
    Callback(&'static str),
}

thread_local! {
    static ROLE: Cell<Role> = const { Cell::new(Role::Unset) };
    static PHASE: Cell<Phase> = const { Cell::new(Phase::Starting) };
}

/// The calling thread's role. Anybody may ask.
#[must_use]
pub fn role() -> Role {
    ROLE.with(Cell::get)
}

// ---------------------------------------------------------------------------
// The phase
// ---------------------------------------------------------------------------

/// **Where the window thread is in its run.** Not `quit`'s phase, which is the quit transaction's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// From [`enter_window_thread`] until the loop's first callback.
    Starting,
    /// From the loop's first callback ([`loop_running`]) on.
    Running,
    /// On the way out ([`exiting`]); a quit whose session write is refused comes back to
    /// `Running` ([`quit_abandoned`]).
    Exiting,
}

/// The window thread's phase, and `None` on any other thread: the phase is the window thread's
/// fact and means nothing elsewhere.
#[must_use]
pub fn phase() -> Option<Phase> {
    (role() == Role::Window).then(|| PHASE.with(Cell::get))
}

/// A set of [`Phase`]s: the phases one door is admitted in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Phases(u8);

impl Phases {
    /// The set holding exactly `phases`.
    #[must_use]
    pub const fn of(phases: &[Phase]) -> Self {
        let mut bits = 0;
        let mut index = 0;
        while index < phases.len() {
            bits |= Self::bit(phases[index]);
            index += 1;
        }
        Self(bits)
    }

    /// Whether `phase` is in the set.
    #[must_use]
    pub const fn contains(self, phase: Phase) -> bool {
        self.0 & Self::bit(phase) != 0
    }

    const fn bit(phase: Phase) -> u8 {
        match phase {
            Phase::Starting => 1,
            Phase::Running => 2,
            Phase::Exiting => 4,
        }
    }
}

// ---------------------------------------------------------------------------
// Refusals and their counters
// ---------------------------------------------------------------------------

/// **An owner-thread wait, or a role or phase write, asked for where it may not happen.** The
/// work did not run; the refusal is counted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refused {
    /// The door's name (the [`doors`] type's), or the entry's (`enter_standalone_main`).
    pub door: &'static str,
    /// The role of the thread that asked.
    pub role: Role,
    /// The window thread's phase when it asked, and `None` on any other thread.
    pub phase: Option<Phase>,
}

/// Every refusal of this process: admissions, and role and phase writes.
static REFUSALS: AtomicU64 = AtomicU64::new(0);

/// **How many refusals this process has counted.** Persistent: nothing resets it, and it is
/// written in the run's footer.
#[must_use]
pub fn refusals() -> u64 {
    REFUSALS.load(Ordering::Relaxed)
}

/// How many times [`admitted`] refused door `D` in this process.
#[must_use]
pub fn refusals_of<D: Door>() -> u64 {
    D::refused().load(Ordering::Relaxed)
}

fn count(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
    REFUSALS.fetch_add(1, Ordering::Relaxed);
}

static ENTER_WINDOW_REFUSED: AtomicU64 = AtomicU64::new(0);
static LOOP_RUNNING_REFUSED: AtomicU64 = AtomicU64::new(0);
static EXITING_REFUSED: AtomicU64 = AtomicU64::new(0);
static QUIT_ABANDONED_REFUSED: AtomicU64 = AtomicU64::new(0);
static STANDALONE_REFUSED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// The writers
// ---------------------------------------------------------------------------

/// **The thread that owns the window says so**: role [`Role::Window`], phase [`Phase::Starting`].
///
/// Once per thread, not once per process: libtest runs every case on a thread of its own, and a
/// test that drives the window thread's doors enters here on that thread. The product calls it
/// once, in `fn main`, directly after the argument parse. On a thread that already has a role —
/// a second call on the window thread, a worker, a callback — it changes nothing, so a second
/// call cannot put a running window thread back to `Starting`, and it counts.
///
/// Answers whether the entry was taken.
pub fn enter_window_thread() -> bool {
    if role() != Role::Unset {
        count(&ENTER_WINDOW_REFUSED);
        return false;
    }
    ROLE.with(|role| role.set(Role::Window));
    PHASE.with(|phase| phase.set(Phase::Starting));
    true
}

/// **The loop took its first turn**: `Starting` → `Running`. Called where winit's first callback
/// arrives (`FolioApp::new_events`, `StartCause::Init`).
///
/// Refused and counted off the window thread, and from any phase but `Starting`. Answers whether
/// the transition was taken.
pub fn loop_running() -> bool {
    write_phase(&LOOP_RUNNING_REFUSED, |from| match from {
        Phase::Starting => Some(Phase::Running),
        Phase::Running | Phase::Exiting => None,
    })
}

/// **On the way out**: `Running` or `Starting` → `Exiting`, and nothing from `Exiting`.
///
/// `Starting` is a road: a loop that fails before its first callback returns from `run_app` with
/// the phase never having turned. Refused and counted off the window thread. Answers whether the
/// thread is now `Exiting`.
pub fn exiting() -> bool {
    write_phase(&EXITING_REFUSED, |from| match from {
        Phase::Starting | Phase::Running | Phase::Exiting => Some(Phase::Exiting),
    })
}

/// **A quit that did not happen**: `Exiting` → `Running`, and nothing from `Running`.
///
/// A quit whose session write was refused comes back from `Exiting`; a quit cancelled at its card,
/// or whose saves did not all land, never left `Running`, and both are ordinary. Refused and
/// counted off the window thread, and from `Starting`. Answers whether the thread is now
/// `Running`.
pub fn quit_abandoned() -> bool {
    write_phase(&QUIT_ABANDONED_REFUSED, |from| match from {
        Phase::Running | Phase::Exiting => Some(Phase::Running),
        Phase::Starting => None,
    })
}

fn write_phase(refused: &AtomicU64, to: impl FnOnce(Phase) -> Option<Phase>) -> bool {
    if role() != Role::Window {
        count(refused);
        return false;
    }
    let from = PHASE.with(Cell::get);
    let Some(next) = to(from) else {
        count(refused);
        return false;
    };
    PHASE.with(|phase| phase.set(next));
    true
}

/// **One of our callbacks, on a thread the operating system chose.**
///
/// On an `Unset` thread the scope *owns* the thread for its length: the role is
/// [`Role::Callback`] `(name)` until the scope drops, and `Unset` again after, on unwind too, so
/// the next block a pooled thread runs does not inherit the name. On a thread that already has a
/// role — the window thread, where AppKit and the message pump deliver most callbacks, a worker,
/// an enclosing callback — the scope changes nothing and restores nothing: that role is the true
/// answer there.
pub fn enter_callback(name: &'static str) -> CallbackScope {
    let restore = (role() == Role::Unset).then(|| {
        ROLE.with(|role| role.set(Role::Callback(name)));
        Role::Unset
    });
    CallbackScope {
        restore,
        _local: PhantomData,
    }
}

/// **The length of one callback's claim on its thread.** Made only by [`enter_callback`].
///
/// `!Send` and `!Sync`: a scope made on an `Unset` thread cannot be dropped on another thread,
/// where its restoration would take a worker's role away while the worker still holds its
/// [`WorkerCtx`].
///
/// ```compile_fail
/// // RED (A1a) — a callback's scope cannot be sent to another thread.
/// let scope = bt_platform::admission::enter_callback("probe");
/// std::thread::scope(|threads| {
///     threads.spawn(move || drop(scope));
/// });
/// ```
///
/// MUTATION: drop `_local` from `CallbackScope` and the block above compiles.
///
/// ```
/// // The control: the same scoped spawn, carrying a byte; the scope drops where it was made.
/// let scope = bt_platform::admission::enter_callback("probe");
/// let byte = 0_u8;
/// std::thread::scope(|threads| {
///     threads.spawn(move || drop(byte));
/// });
/// drop(scope);
/// ```
///
/// Its auto traits, each probed on its own, with no lifetime involved:
///
/// ```compile_fail
/// fn is_send<T: Send>() {}
/// is_send::<bt_platform::admission::CallbackScope>();
/// ```
///
/// ```compile_fail
/// fn is_sync<T: Sync>() {}
/// is_sync::<bt_platform::admission::CallbackScope>();
/// ```
///
/// MUTATION: drop `_local` and both blocks compile. The control instantiates the same probes at a
/// byte:
///
/// ```
/// fn is_send<T: Send>() {}
/// fn is_sync<T: Sync>() {}
/// is_send::<u8>();
/// is_sync::<u8>();
/// ```
#[must_use = "the callback's role lasts as long as its scope"]
pub struct CallbackScope {
    /// `Some(Unset)` when this scope set `Callback`; `None` when it found a role.
    restore: Option<Role>,
    _local: PhantomData<*const ()>,
}

impl Drop for CallbackScope {
    /// The one guard in this module. It restores a thread-local and runs no meter code.
    fn drop(&mut self) {
        if let Some(role) = self.restore {
            ROLE.with(|cell| cell.set(role));
        }
    }
}

// ---------------------------------------------------------------------------
// The worker capability
// ---------------------------------------------------------------------------

/// **Lent to the body of a thread that is a worker, and to nothing else.**
///
/// Private fields, `!Send`, `!Sync`, no `Clone`, `Default` or public constructor. Made in one
/// function, [`lend_worker`], on the thread it names, in the statement after that thread's role
/// becomes [`Role::Worker`]. Its two callers are the thread door ([`spawn_at_priority_with_stack`])
/// and [`enter_standalone_main`]; a worker-only door takes `&WorkerCtx`, so a caller with none does
/// not compile (the proofs are on [`spawn_at_priority`]).
///
/// ```compile_fail
/// // RED (A1a) — a `WorkerCtx` cannot be built outside `admission`.
/// let ctx = bt_platform::admission::WorkerCtx {
///     name: "forged",
///     _local: std::marker::PhantomData,
/// };
/// ```
///
/// MUTATION: make `WorkerCtx`'s fields `pub` and the block above compiles. The controls are the
/// two public roads, each of which lends one to a body:
///
/// ```no_run
/// let _ = bt_platform::admission::enter_standalone_main("doc-probe", |ctx| ctx.name());
/// ```
///
/// ```no_run
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |ctx| ctx.name(),
/// );
/// ```
///
/// Its auto traits, probed alone:
///
/// ```compile_fail
/// fn is_send<T: Send>() {}
/// is_send::<bt_platform::admission::WorkerCtx>();
/// ```
///
/// ```compile_fail
/// fn is_sync<T: Sync>() {}
/// is_sync::<bt_platform::admission::WorkerCtx>();
/// ```
///
/// MUTATION: drop `_local` from `WorkerCtx` and both compile. Control, the same probes at a byte:
///
/// ```
/// fn is_send<T: Send>() {}
/// fn is_sync<T: Sync>() {}
/// is_send::<u8>();
/// is_sync::<u8>();
/// ```
///
/// And the borrowed capability does not cross into another thread, with no `'static` bound in the
/// way (a scoped thread):
///
/// ```compile_fail
/// let _ = bt_platform::admission::enter_standalone_main("doc-probe", |ctx| {
///     std::thread::scope(|threads| {
///         threads.spawn(move || ctx.name());
///     });
/// });
/// ```
///
/// MUTATION: drop `_local` (a `Sync` `WorkerCtx` makes `&WorkerCtx` `Send`) and it compiles. The
/// control is the same scoped spawn carrying a byte:
///
/// ```no_run
/// let _ = bt_platform::admission::enter_standalone_main("doc-probe", |ctx| {
///     let byte = 0_u8;
///     std::thread::scope(|threads| {
///         threads.spawn(move || byte);
///     });
///     ctx.name()
/// });
/// ```
pub struct WorkerCtx {
    name: &'static str,
    _local: PhantomData<*const ()>,
}

impl WorkerCtx {
    /// The name the worker was started with.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }
}

/// **The worker writer**: the calling thread becomes `Worker(name)`, permanently, and the
/// capability that says so is built on its stack in the next statement, so role and capability
/// cannot disagree. Private; its two callers make sure the thread is theirs to name — the thread
/// door calls it first thing on a thread it has just started (after the band), and
/// [`enter_standalone_main`] only on an `Unset` thread, once per process.
fn lend_worker(name: &'static str) -> WorkerCtx {
    ROLE.with(|role| role.set(Role::Worker(name)));
    WorkerCtx {
        name,
        _local: PhantomData,
    }
}

// ---------------------------------------------------------------------------
// The thread door
// ---------------------------------------------------------------------------

/// **Start a named thread that is already in its band, and lend its body the capability that says
/// it is a worker** — the thread door (`docs/ARCHITECTURE.md` §6, `docs/RULES.md` rows 52 and 53;
/// design note `docs/plans/design/thread-door-2026-09-26.md` §2).
///
/// Inside the new thread, in this order: the band, as the first statement; the role
/// [`Role::Worker`] `(name)`; then a [`WorkerCtx`] built on the thread's own stack and lent to
/// `body` by reference. `body` cannot keep the capability past its return (it is lent, and the
/// result type is chosen outside the loan), cannot send it to another thread (`!Sync`), and nothing
/// but this door and [`enter_standalone_main`] can make one.
///
/// **The band is set from inside the new thread, not from the spawner**, and that is the whole
/// reason this helper exists rather than a `set_priority(&handle)` called after `spawn`: between a
/// `spawn` and a call on its `JoinHandle` the new thread is already running, and under the exact
/// saturation this is for, "already running" can mean "has already decoded the image" — a worker
/// that spends its first and busiest milliseconds at the frame's priority. Here the first statement
/// the thread executes is the one that gets out of the frame's way. Off Windows the band is
/// requested and not taken ([`crate::set_current_thread_priority`] answers `false`); the name, the
/// role and the capability are the same on every platform.
///
/// # Errors
///
/// Whatever [`std::thread::Builder::spawn`] answers when the operating system will not start a
/// thread; `body` has not run and no role was written.
///
/// # The capability, proved
///
/// A worker-only door takes `&WorkerCtx`, so code with none in scope does not compile (M3):
///
/// ```compile_fail
/// // RED (A1b, M3) — a function the door did not start has no capability to hand a worker door.
/// fn on_any_thread() {
///     drop(bt_platform::ShellThread::enter());
/// }
/// ```
///
/// MUTATION: make `ShellThread::enter` take no capability and the block above compiles. The
/// control is the same statement inside a body the door started, handing it the lent `ctx`:
///
/// ```no_run
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |ctx| {
///         drop(bt_platform::ShellThread::enter(ctx));
///     },
/// );
/// ```
///
/// The lent capability does not cross into another thread, with no `'static` bound in the way (a
/// scoped thread; M3r):
///
/// ```compile_fail
/// // RED (A1b, M3r) — a door-started body cannot hand its capability to a thread it starts.
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |ctx| {
///         std::thread::scope(|threads| {
///             threads.spawn(move || ctx.name());
///         });
///     },
/// );
/// ```
///
/// MUTATION: drop `_local` from `WorkerCtx` (a `Sync` `WorkerCtx` makes `&WorkerCtx` `Send`) and it
/// compiles. The control is the same scoped spawn carrying a byte, with the capability used where
/// it was lent:
///
/// ```no_run
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |ctx| {
///         let byte = 0_u8;
///         std::thread::scope(|threads| {
///             threads.spawn(move || byte);
///         });
///         ctx.name()
///     },
/// );
/// ```
///
/// An indirection does not carry a capability it was not given (worker-side M5): a boxed closure
/// and a function pointer, each typed with no capability, cannot reach the door even inside a body
/// the door started —
///
/// ```compile_fail
/// // RED (A1b, M5) — a boxed closure with no capability in its type.
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |_ctx| {
///         let enter: Box<dyn Fn()> = Box::new(|| drop(bt_platform::ShellThread::enter()));
///         enter();
///     },
/// );
/// ```
///
/// ```compile_fail
/// // RED (A1b, M5) — a function pointer with no capability in its type.
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |_ctx| {
///         let enter: fn() = || drop(bt_platform::ShellThread::enter());
///         enter();
///     },
/// );
/// ```
///
/// MUTATION: make `ShellThread::enter` take no capability and both compile. The controls type the
/// same indirections with the capability and pass the lent one on:
///
/// ```no_run
/// use bt_platform::admission::WorkerCtx;
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |ctx| {
///         let enter: Box<dyn Fn(&WorkerCtx)> =
///             Box::new(|ctx| drop(bt_platform::ShellThread::enter(ctx)));
///         enter(ctx);
///     },
/// );
/// ```
///
/// ```no_run
/// use bt_platform::admission::WorkerCtx;
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |ctx| {
///         let enter: fn(&WorkerCtx) = |ctx| drop(bt_platform::ShellThread::enter(ctx));
///         enter(ctx);
///     },
/// );
/// ```
pub fn spawn_at_priority<F, T>(
    name: &'static str,
    band: crate::ThreadPriority,
    body: F,
) -> std::io::Result<std::thread::JoinHandle<T>>
where
    F: FnOnce(&WorkerCtx) -> T + Send + 'static,
    T: Send + 'static,
{
    spawn_at_priority_with_stack(name, band, None, body)
}

/// **The same door, for a thread that has a reason to say how much stack it needs.**
///
/// Rust's default is two mebibytes, which is nobody's measurement of any particular work. A caller
/// that recurses over input it did not write — the math worker descends through a LaTeX parser and
/// then Typst's parser and layout — states its own, because a stack overflow is not a panic and
/// cannot be contained by the thread that suffers it.
///
/// # Errors
///
/// As [`spawn_at_priority`].
pub fn spawn_at_priority_with_stack<F, T>(
    name: &'static str,
    band: crate::ThreadPriority,
    stack_bytes: Option<usize>,
    body: F,
) -> std::io::Result<std::thread::JoinHandle<T>>
where
    F: FnOnce(&WorkerCtx) -> T + Send + 'static,
    T: Send + 'static,
{
    let mut builder = std::thread::Builder::new().name(name.to_owned());
    if let Some(bytes) = stack_bytes {
        builder = builder.stack_size(bytes);
    }
    builder.spawn(move || {
        // The band first (RULES 53): Windows hands a new thread `Normal` whatever its creator
        // stands in, and this is the statement that gets it out of the frame's way.
        crate::set_current_thread_priority(band);
        let ctx = lend_worker(name);
        body(&ctx)
    })
}

static STANDALONE_ENTERED: AtomicBool = AtomicBool::new(false);

/// **A standalone process's main thread is a worker** (revision (c)6): the `attention` verb's
/// payload reader and the two Explorer-menu removals run in processes that never have a window.
///
/// Once per process and only on an `Unset` thread: the thread becomes `Worker(name)` for good and
/// `body` is lent that process's one [`WorkerCtx`]. A second call, or a call on a thread that
/// already has a role, is refused and counted, and `body` does not run. Its product callers come
/// with A1c.
pub fn enter_standalone_main<R>(
    name: &'static str,
    body: impl FnOnce(&WorkerCtx) -> R,
) -> Result<R, Refused> {
    let role = role();
    if role != Role::Unset || STANDALONE_ENTERED.swap(true, Ordering::Relaxed) {
        count(&STANDALONE_REFUSED);
        return Err(Refused {
            door: "enter_standalone_main",
            role,
            phase: phase(),
        });
    }
    let ctx = lend_worker(name);
    Ok(body(&ctx))
}

// ---------------------------------------------------------------------------
// Doors
// ---------------------------------------------------------------------------

mod sealed {
    use std::sync::atomic::AtomicU64;

    /// Implemented only inside `admission`, which is what makes [`super::Door`] sealed.
    pub trait Sealed {
        /// This door's refusal counter.
        fn refused() -> &'static AtomicU64;
    }
}

/// **A §5.3 row's label, as `window_waits.tsv` spells it** (`"11"`, `"16b"`, `"§5.2"`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Row(&'static str);

impl Row {
    /// The label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        self.0
    }
}

/// **One door's identity, as the meter is told it.** Built only here, one per [`doors`] type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DoorKey {
    name: &'static str,
    row: Row,
    station: u8,
    phases: Phases,
}

impl DoorKey {
    /// The door type's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// The §5.3 row the door serves.
    #[must_use]
    pub const fn row(self) -> Row {
        self.row
    }

    /// The byte of `bt-app`'s `hang_watch::Station` the meter enters for this door.
    #[must_use]
    pub const fn station(self) -> u8 {
        self.station
    }

    /// The phases the door is admitted in.
    #[must_use]
    pub const fn phases(self) -> Phases {
        self.phases
    }
}

/// **An owner-thread door's identity is a type.** Sealed: the only implementations are the
/// [`doors`] types, one per registry line.
pub trait Door: sealed::Sealed + 'static {
    /// Everything below, in one value.
    const KEY: DoorKey;
    /// The §5.3 row.
    const ROW: Row = Self::KEY.row;
    /// `hang_watch`'s station byte.
    const STATION: u8 = Self::KEY.station;
    /// The phases it is admitted in.
    const PHASES: Phases = Self::KEY.phases;
}

/// One uninhabited door type, sealed, with its own refusal counter.
macro_rules! door {
    ($(#[$meta:meta])* $name:ident, $row:literal, $station:literal, [$($phase:ident),+]) => {
        $(#[$meta])*
        pub enum $name {}

        impl $crate::admission::sealed::Sealed for $name {
            fn refused() -> &'static ::std::sync::atomic::AtomicU64 {
                static REFUSED: ::std::sync::atomic::AtomicU64 =
                    ::std::sync::atomic::AtomicU64::new(0);
                &REFUSED
            }
        }

        impl $crate::admission::Door for $name {
            const KEY: $crate::admission::DoorKey = $crate::admission::DoorKey {
                name: stringify!($name),
                row: $crate::admission::Row($row),
                station: $station,
                phases: $crate::admission::Phases::of(&[$($crate::admission::Phase::$phase),+]),
            };
        }
    };
}

/// **The door registry, as types**: one per line of `crates/bt-app/src/window_waits.tsv`'s
/// `doors` section, held equal to it — name, row, station and phases — by `bt-app`'s
/// `window_waits_tests`. The station is `hang_watch::Station`'s byte, named in the comment.
///
/// No door takes its token yet: A1d converts them, row by row, as revision (e)2 of the design
/// note tables them.
pub mod doors {
    macro_rules! doors {
        ($($(#[$meta:meta])* $name:ident => $row:literal, $station:literal, [$($phase:ident),+];)+) => {
            $(door!($(#[$meta])* $name, $row, $station, [$($phase),+]);)+

            /// Every door type's key, in the registry's order.
            pub const ALL: &[super::DoorKey] = &[$(<$name as super::Door>::KEY),+];
        };
    }

    doors! {
        /// Row 5's residue: one family asked of the system collection (`FontLookup`).
        FontFamilyLookup => "5", 206, [Running];
        /// Row 9: one window's compose, configure, acquire, submit and present (`RenderCompose`).
        PresentFrame => "9", 50, [Running, Exiting];
        /// Row 9: the DirectComposition commit (`CompositorCommit`).
        CompositorCommit => "9", 65, [Running, Exiting];
        /// Row 9: a window's DirectComposition tree built, or the spare's parent (`CompositorBirth`).
        CompositorBirth => "9", 216, [Running];
        /// Row 9: the window's own ground placed and committed after a resize
        /// (`CompositorWindowSize`).
        CompositorWindowSize => "9", 217, [Running, Exiting];
        /// Row 9: a second window's surface created and configured (`SurfaceConfigure`).
        SurfaceBirth => "9", 194, [Running];
        /// Row 11: `CreatePseudoConsole` and the shell's process (`PtyBirth`).
        PtyBirth => "11", 210, [Running, Exiting];
        /// Row 12: one leaf's `ResizePseudoConsole` round trip (`PtyResize`).
        PtyResize => "12", 4, [Running, Exiting];
        /// Row 13's residue: `IsIconic` and the cloak (`PlaceHidden`).
        PlaceHidden => "13", 195, [Running, Exiting];
        /// Row 13's residue: whether any of the window is exposed (`PlaceExposure`).
        PlaceExposure => "13", 196, [Running, Exiting];
        /// Row 14's residue: `Window::set_title` (`WindowTitle`).
        TitleFlush => "14", 102, [Running, Exiting];
        /// Row 15: the bounded wait for the panes being taken apart (`PaneRetirementWait`).
        PaneRetirementWait => "15", 211, [Exiting];
        /// Row 16: the synchronous session save's bounded wait (`SessionWriteWait`).
        SessionWriteWait => "16", 212, [Exiting];
        /// Row 16b: the session writer's bounded poll and join (`SessionWriterRetire`).
        SessionWriterRetire => "16b", 213, [Exiting];
        /// Row 17: the trace writer's bounded flush (`TraceFlush`).
        TraceFlush => "17", 214, [Exiting];
        /// Row 18: the launch handed to a running Folio, before the loop exists (`Starting`).
        LaunchHandOver => "18", 0, [Starting];
        /// Row 21: `CreateCoreWebView2CompositionController` (`WebController`).
        WebController => "21", 200, [Running];
        /// Row 21: `CreateCoreWebView2EnvironmentWithOptions` (`WebEnvironment`).
        WebEnvironment => "21", 199, [Running];
        /// Row 21: a page moved to another window's tree, with its commits (`WebRehost`).
        WebRehost => "21", 218, [Running];
        /// Row 22's residue: the input method's caret area (`ImeCursorArea`).
        ImeCaretArea => "22", 54, [Running, Exiting];
        /// Row 23, pending: the first window's `pollster::block_on(GpuContext::open)` (`GpuOpen`).
        GpuOpen => "23", 215, [Running];
        /// §5.2: `Window::focus_window` (`WindowFocus`).
        FocusWindow => "§5.2", 186, [Running];
        /// §5.2: `Window::set_visible` (`WindowVisible`).
        SetVisible => "§5.2", 187, [Running, Exiting];
        /// §5.2: `Window::set_cursor` (`WindowCursor`).
        SetCursor => "§5.2", 188, [Running];
    }
}

// ---------------------------------------------------------------------------
// The token and the admission
// ---------------------------------------------------------------------------

/// **Leave to make one call of door `D`, on the window thread, now.**
///
/// Made only by [`admitted`], inside the call it measures, and handed to its work by value; a door
/// takes it by value (`fn door(token: WaitToken<'_, doors::X>, …)`), so one admission pays for one
/// call. The lifetime is fresh per admission — the work is higher-ranked over it and its result is
/// chosen outside it — so the token cannot be returned, stored or captured past the call.
///
/// Each property below is proved on its own, beside a control that differs by one statement. The
/// two door types are real ones, and the `door` functions are declared in each block, so no product
/// door runs.
///
/// **Its result cannot carry it out** (higher-ranked escape):
///
/// ```compile_fail
/// use bt_platform::admission::{admitted, doors};
/// let _escaped = admitted::<doors::FontFamilyLookup, _>(|token| token);
/// ```
///
/// ```
/// use bt_platform::admission::{admitted, doors};
/// let _ = admitted::<doors::FontFamilyLookup, _>(|token| drop(token));
/// ```
///
/// **Nor can a place that outlives the work** (safe outer storage):
///
/// ```compile_fail
/// use bt_platform::admission::{admitted, doors};
/// let mut slot = None;
/// let _ = admitted::<doors::FontFamilyLookup, _>(|token| {
///     slot = Some(token);
/// });
/// ```
///
/// ```
/// use bt_platform::admission::{admitted, doors};
/// let mut slot = None;
/// let _ = admitted::<doors::FontFamilyLookup, _>(|token| {
///     drop(token);
///     slot = Some(0_u8);
/// });
/// ```
///
/// **Nor a closure or a future the work hands back**:
///
/// ```compile_fail
/// use bt_platform::admission::{WaitToken, admitted, doors};
/// fn door(_: WaitToken<'_, doors::FontFamilyLookup>) {}
/// let _later = admitted::<doors::FontFamilyLookup, _>(|token| move || door(token));
/// ```
///
/// ```compile_fail
/// use bt_platform::admission::{WaitToken, admitted, doors};
/// fn door(_: WaitToken<'_, doors::FontFamilyLookup>) {}
/// let _later = admitted::<doors::FontFamilyLookup, _>(|token| async move { door(token) });
/// ```
///
/// ```
/// use bt_platform::admission::{WaitToken, admitted, doors};
/// fn door(_: WaitToken<'_, doors::FontFamilyLookup>) {}
/// let _ = admitted::<doors::FontFamilyLookup, _>(|token| door(token));
/// ```
///
/// MUTATION (all three of the above): give `admitted`'s work a named lifetime instead of the
/// binder — `work: impl FnOnce(WaitToken<'static, D>) -> R` — and each compiles.
///
/// **There is no token outside an admission** (M4a: a door called after the scope, with a token
/// made any public way):
///
/// ```compile_fail
/// use bt_platform::admission::{WaitToken, admitted, doors};
/// fn door(_: WaitToken<'_, doors::FontFamilyLookup>) {}
/// let _ = admitted::<doors::FontFamilyLookup, _>(|_token| ());
/// door(Default::default());
/// ```
///
/// MUTATION: derive `Default` on `WaitToken` and it compiles. The control calls the door inside
/// the work, which is the block above this one's.
///
/// **Nor a literal** (privacy):
///
/// ```compile_fail
/// use bt_platform::admission::{WaitToken, doors};
/// use std::marker::PhantomData;
/// fn door(_: WaitToken<'_, doors::FontFamilyLookup>) {}
/// door(WaitToken { _scope: PhantomData, _door: PhantomData, _local: PhantomData });
/// ```
///
/// MUTATION: make `WaitToken`'s fields `pub` and it compiles. The control is the public road, the
/// block that calls `door(token)` inside `admitted`.
///
/// **A token is one door's** (wrong identity):
///
/// ```compile_fail
/// use bt_platform::admission::{WaitToken, admitted, doors};
/// fn present(_: WaitToken<'_, doors::PresentFrame>) {}
/// let _ = admitted::<doors::FontFamilyLookup, _>(|token| present(token));
/// ```
///
/// ```
/// use bt_platform::admission::{WaitToken, admitted, doors};
/// fn present(_: WaitToken<'_, doors::PresentFrame>) {}
/// let _ = admitted::<doors::PresentFrame, _>(|token| present(token));
/// ```
///
/// MUTATION: take the door parameter off `WaitToken` (one token type for every door) and the
/// first compiles.
///
/// **And pays for one call** (double consumption):
///
/// ```compile_fail
/// use bt_platform::admission::{WaitToken, admitted, doors};
/// fn door(_: WaitToken<'_, doors::FontFamilyLookup>) {}
/// let _ = admitted::<doors::FontFamilyLookup, _>(|token| {
///     door(token);
///     door(token);
/// });
/// ```
///
/// MUTATION: derive `Clone` and `Copy` on `WaitToken` and it compiles. The control is the single
/// call above.
///
/// **It stays on its thread** (M4e), by value and by reference, in a scoped thread so that no
/// `'static` bound stands in the way:
///
/// ```compile_fail
/// use bt_platform::admission::{admitted, doors};
/// let _ = admitted::<doors::FontFamilyLookup, _>(|token| {
///     std::thread::scope(|threads| {
///         threads.spawn(move || drop(token));
///     });
/// });
/// ```
///
/// ```compile_fail
/// use bt_platform::admission::{admitted, doors};
/// let _ = admitted::<doors::FontFamilyLookup, _>(|token| {
///     let lent = &token;
///     std::thread::scope(|threads| {
///         threads.spawn(move || {
///             std::hint::black_box(lent);
///         });
///     });
/// });
/// ```
///
/// ```
/// use bt_platform::admission::{admitted, doors};
/// let _ = admitted::<doors::FontFamilyLookup, _>(|token| {
///     let byte = 0_u8;
///     std::thread::scope(|threads| {
///         threads.spawn(move || drop(byte));
///     });
///     drop(token);
/// });
/// ```
///
/// MUTATION: drop `_local` from `WaitToken` and both compile.
///
/// Its auto traits alone, at `'static` (the type exists there even though no value does):
///
/// ```compile_fail
/// fn is_send<T: Send>() {}
/// is_send::<bt_platform::admission::WaitToken<'static, bt_platform::admission::doors::FontFamilyLookup>>();
/// ```
///
/// ```compile_fail
/// fn is_sync<T: Sync>() {}
/// is_sync::<bt_platform::admission::WaitToken<'static, bt_platform::admission::doors::FontFamilyLookup>>();
/// ```
///
/// ```
/// fn is_send<T: Send>() {}
/// fn is_sync<T: Sync>() {}
/// is_send::<u8>();
/// is_sync::<u8>();
/// ```
///
/// MUTATION: drop `_local` and both probes compile.
pub struct WaitToken<'scope, D: Door> {
    /// Invariant in `'scope`, so the fresh lifetime cannot be widened or narrowed.
    _scope: PhantomData<fn(&'scope ()) -> &'scope ()>,
    _door: PhantomData<fn() -> D>,
    /// `!Send`, `!Sync`.
    _local: PhantomData<*const ()>,
}

impl<D: Door> WaitToken<'_, D> {
    fn fresh() -> Self {
        Self {
            _scope: PhantomData,
            _door: PhantomData,
            _local: PhantomData,
        }
    }
}

/// **Run `work` as one admitted call of door `D`**: on the window thread, in one of `D`'s phases,
/// measured by the installed [`Meter`].
///
/// Checked in every build: a thread whose role is not [`Role::Window`], or a phase outside
/// `D::PHASES`, gets [`Refused`] — `work` does not run, and the door's counter and the process
/// total each rise by one. Otherwise the meter's `enter` is told the door before `work` runs and
/// its `leave` after `work` returns, with the two instants this function read around it.
///
/// **No guard**: a `work` that panics unwinds straight out, `leave` is not called, and the door's
/// station stays the thread's current one — the next hang report names the call that did not come
/// back (revision (b)4, `hang_watch::during`'s own discipline).
pub fn admitted<D: Door, R>(
    work: impl for<'scope> FnOnce(WaitToken<'scope, D>) -> R,
) -> Result<R, Refused> {
    let role = role();
    let phase = PHASE.with(Cell::get);
    if role != Role::Window || !D::PHASES.contains(phase) {
        count(D::refused());
        return Err(Refused {
            door: D::KEY.name,
            role,
            phase: (role == Role::Window).then_some(phase),
        });
    }
    let Some(meter) = meter() else {
        return Ok(work(WaitToken::fresh()));
    };
    let cookie = (meter.enter)(D::KEY);
    let start = Instant::now();
    let output = work(WaitToken::fresh());
    let end = Instant::now();
    (meter.leave)(D::KEY, cookie, start, end);
    Ok(output)
}

// ---------------------------------------------------------------------------
// The meter
// ---------------------------------------------------------------------------

/// **What the meter's `enter` hands back and its `leave` gets again.** Opaque here: built and read
/// only by the meter's owner (`hang_watch` packs the station, node and scope it must restore).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cookie(u64);

impl Cookie {
    /// A cookie holding `raw`.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// What it holds.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// **How an admitted call is measured**: `enter` before the work, so a call that never returns is
/// already named; `leave` after it, with the cookie `enter` gave and the call's start and end.
#[derive(Clone, Copy, Debug)]
pub struct Meter {
    pub enter: fn(DoorKey) -> Cookie,
    pub leave: fn(DoorKey, Cookie, Instant, Instant),
}

/// [`install_meter`] was called a second time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlreadyInstalled;

static METER: OnceLock<Meter> = OnceLock::new();

/// **Install the process's one meter.** Before it is installed, [`admitted`] measures nothing.
pub fn install_meter(meter: Meter) -> Result<(), AlreadyInstalled> {
    METER.set(meter).map_err(|_| AlreadyInstalled)
}

fn meter() -> Option<Meter> {
    #[cfg(test)]
    if let Some(meter) = TEST_METER.with(Cell::get) {
        return Some(meter);
    }
    METER.get().copied()
}

#[cfg(test)]
thread_local! {
    /// A test's own meter, consulted before the process's: this crate's tests never install the
    /// process-wide one, so one test's meter cannot see another test's calls.
    static TEST_METER: Cell<Option<Meter>> = const { Cell::new(None) };
}

/// A test meter for the calling thread, until the scope drops.
#[cfg(test)]
pub(crate) fn test_meter_scope(meter: Meter) -> TestMeterScope {
    TestMeterScope {
        previous: TEST_METER.with(|cell| cell.replace(Some(meter))),
    }
}

#[cfg(test)]
pub(crate) struct TestMeterScope {
    previous: Option<Meter>,
}

#[cfg(test)]
impl Drop for TestMeterScope {
    fn drop(&mut self) {
        TEST_METER.with(|cell| cell.set(self.previous));
    }
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
